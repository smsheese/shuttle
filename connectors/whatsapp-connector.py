#!/usr/bin/env python3
"""WhatsApp connector: Shuttle JSON protocol on stdin/stdout, GOWA REST/WebSocket on loopback."""

from __future__ import annotations

import base64
import json
import os
import queue
import re
import hashlib
import tempfile
import secrets
import socket
import struct
import subprocess
import sys
import concurrent.futures
import http.server
import socketserver
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from shuttle_ipc import (
    child_pdeathsig,
    file_lock_exclusive,
    file_unlock,
    find_processes_matching,
    pid_alive,
    parse_timestamp_strict,
    spawn_parent_death_watchdog,
    terminate_pid,
    to_rfc3339,
    to_rfc3339_strict,
)

CONNECTOR_ID = "whatsapp"
VERSION = "1.0.0"
CAPABILITIES = ["text", "media", "read_receipts", "groups"]
GOWA_USER = "shuttle"
# GOWA v9.9.0 route under /message/:message_id for each message op (checked on the pinned binary).
MESSAGE_OP_ROUTES = {
    "react": "reaction",
    "edit": "update",
    "delete_for_everyone": "revoke",
    "delete_for_me": "delete",
}
# GOWA change webhook -> (field naming the changed message, `message.changed` kind).
CHANGE_EVENTS = {
    "message.edited": ("original_message_id", "edited"),
    "message.revoked": ("revoked_message_id", "revoked"),
    "message.deleted": ("deleted_message_id", "deleted_for_me"),
}


def log(msg: str) -> None:
    sys.stderr.write(f"[whatsapp-connector] {msg}\n")
    sys.stderr.flush()


_send_lock = threading.Lock()


def send(msg: dict[str, Any]) -> None:
    """Write one protocol line to stdout. Every writer goes through this lock."""
    line = json.dumps(msg, default=str) + "\n"
    with _send_lock:
        sys.stdout.write(line)
        sys.stdout.flush()


def send_ok(account_id: Optional[str], **extra: Any) -> None:
    msg: dict[str, Any] = {"type": "ok", "request_id": None}
    msg.update(extra)
    if account_id:
        msg["account_id"] = account_id
    send(msg)


def send_error(account_id: Optional[str], message: str, request_id: Optional[str] = None) -> None:
    msg: dict[str, Any] = {"type": "error", "message": message}
    if account_id:
        msg["account_id"] = account_id
    if request_id:
        msg["request_id"] = request_id
    send(msg)


def read_line() -> Optional[dict[str, Any]]:
    line = sys.stdin.readline()
    if not line:
        return None
    line = line.strip()
    if not line:
        return {}
    return json.loads(line)


def data_dir() -> Path:
    override = os.environ.get("SHUTTLE_DATA_DIR")
    if override:
        p = Path(override)
    else:
        p = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share")) / "shuttle"
    p.mkdir(parents=True, exist_ok=True)
    return p


def safe_dir_name(account_id: str) -> str:
    """Same rule as Rust `safe_dir_name`: anything outside [A-Za-z0-9_-] becomes `_`."""
    return "".join(ch if (ch.isascii() and (ch.isalnum() or ch in "_-")) else "_" for ch in str(account_id))


_account_files: dict[str, Path] = {}


def media_cache_root() -> Path:
    """App-private cache for downloaded media: `<data_dir>/media_cache` (never the Documents folder).

    The Shuttle core passes the same root as `SHUTTLE_FILES_ROOT` / `files_dir`; the default only
    matters when the connector runs without the core (tests, manual runs).
    """
    return data_dir() / "media_cache"


def files_dir(account_id: Optional[str] = None) -> Path:
    """Per-account cache folder with `media/`, `avatars/` and `thumbs/` (never user-visible saves)."""
    aid = account_id or os.environ.get("SHUTTLE_ACCOUNT_ID") or "default"
    files_root = os.environ.get("SHUTTLE_FILES_ROOT")
    if aid in _account_files:
        root = _account_files[aid]
    elif files_root and account_id:
        root = Path(files_root) / safe_dir_name(aid)
    else:
        override = os.environ.get("SHUTTLE_FILES_DIR")
        root = Path(override) if override else media_cache_root() / safe_dir_name(aid)
    for sub in ("media", "avatars", "thumbs"):
        (root / sub).mkdir(parents=True, exist_ok=True)
    return root


def phone_digits(value: str) -> str:
    return "".join(ch for ch in str(value) if ch.isdigit())


def is_status_jid(jid: str) -> bool:
    j = str(jid or "").lower()
    return j.startswith("status@")


def jid_core_user(jid: Any) -> str:
    """User part of a JID without the `:device` / `.agent` suffix or a leading `+`."""
    local = str(jid or "").strip().split("@", 1)[0]
    local = local.split(":", 1)[0].split(".", 1)[0]
    return local.lstrip("+")


def jids_same(a: str, b: str) -> bool:
    ua = jid_core_user(a)
    ub = jid_core_user(b)
    return bool(ua) and ua == ub


def gowa_home() -> Path:
    p = data_dir() / "gowa"
    (p / "storages").mkdir(parents=True, exist_ok=True)
    return p


def find_gowa_binary() -> Optional[Path]:
    env = os.environ.get("SHUTTLE_GOWA_BIN")
    if env:
        path = Path(env)
        if path.is_file():
            return path
    here = Path(__file__).resolve().parent
    candidates = [
        here / "gowa" / "whatsapp",
        here / "bin" / "gowa" / "whatsapp",
        Path(os.environ.get("CARGO_MANIFEST_DIR", ".")) / ".." / ".." / "connectors" / "gowa" / "whatsapp",
        gowa_home() / "whatsapp",
    ]
    for c in candidates:
        try:
            if c.is_file() and os.access(c, os.X_OK):
                return c.resolve()
        except OSError:
            continue
    return None


def wait_http(url: str, auth: tuple[str, str], timeout: float = 20.0) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        for path in ("/health", "/app/status", "/devices", "/"):
            try:
                req = urllib.request.Request(url + path)
                _request(req, auth, timeout=2)
                return True
            except Exception as e:
                log(f"wait_http {path}: {type(e).__name__}")
                continue
        time.sleep(0.25)
    return False


def _request(
    req: urllib.request.Request,
    auth: tuple[str, str],
    timeout: float = 15.0,
    data: Optional[bytes] = None,
) -> tuple[int, Any]:
    token = base64.b64encode(f"{auth[0]}:{auth[1]}".encode()).decode()
    req.add_header("Authorization", f"Basic {token}")
    req.add_header("Accept", "application/json")
    try:
        with urllib.request.urlopen(req, data=data, timeout=timeout) as resp:
            body = resp.read()
            code = resp.getcode()
    except urllib.error.HTTPError as e:
        body = e.read()
        code = e.code
        # Any HTTP response means the server is up.
    except urllib.error.URLError:
        raise
    if not body:
        return code, {}
    try:
        return code, json.loads(body.decode("utf-8", errors="replace"))
    except json.JSONDecodeError:
        return code, {"raw": body.decode("utf-8", errors="replace")}


class GowaClient:
    def __init__(self, base_url: str, auth: tuple[str, str], device_id: str):
        self.base_url = base_url.rstrip("/")
        self.auth = auth
        self.device_id = device_id

    def call(
        self,
        method: str,
        path: str,
        query: Optional[dict[str, Any]] = None,
        json_body: Any = None,
        headers: Optional[dict[str, str]] = None,
        timeout: float = 20.0,
    ) -> tuple[int, Any]:
        url = self.base_url + path
        if query:
            url += "?" + urllib.parse.urlencode({k: v for k, v in query.items() if v is not None})
        data = None
        req = urllib.request.Request(url, method=method)
        req.add_header("X-Device-Id", self.device_id)
        if headers:
            for k, v in headers.items():
                req.add_header(k, v)
        if json_body is not None:
            data = json.dumps(json_body).encode()
            req.add_header("Content-Type", "application/json")
        return _request(req, self.auth, timeout=timeout, data=data)

    def get(self, path: str, **kwargs: Any) -> tuple[int, Any]:
        return self.call("GET", path, **kwargs)

    def post(self, path: str, **kwargs: Any) -> tuple[int, Any]:
        return self.call("POST", path, **kwargs)

    def patch(self, path: str, **kwargs: Any) -> tuple[int, Any]:
        return self.call("PATCH", path, **kwargs)

    def post_multipart(
        self,
        path: str,
        fields: dict[str, str],
        files: Optional[dict[str, tuple[str, bytes, str]]] = None,
        timeout: float = 60.0,
    ) -> tuple[int, Any]:
        boundary = "----Shuttle" + secrets.token_hex(12)
        chunks: list[bytes] = []
        for key, value in fields.items():
            chunks.append(
                (
                    f"--{boundary}\r\n"
                    f'Content-Disposition: form-data; name="{key}"\r\n\r\n'
                    f"{value}\r\n"
                ).encode()
            )
        for key, (filename, raw, mime) in (files or {}).items():
            safe = filename.replace('"', "")
            chunks.append(
                (
                    f"--{boundary}\r\n"
                    f'Content-Disposition: form-data; name="{key}"; filename="{safe}"\r\n'
                    f"Content-Type: {mime or 'application/octet-stream'}\r\n\r\n"
                ).encode()
            )
            chunks.append(raw)
            chunks.append(b"\r\n")
        chunks.append(f"--{boundary}--\r\n".encode())
        body = b"".join(chunks)
        url = self.base_url + path
        req = urllib.request.Request(url, method="POST", data=body)
        req.add_header("X-Device-Id", self.device_id)
        req.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")
        return _request(req, self.auth, timeout=timeout, data=body)

    def _download_request(self, url: str) -> urllib.request.Request:
        if url.startswith("/"):
            url = self.base_url + url
        else:
            parsed = urllib.parse.urlparse(url)
            base = urllib.parse.urlparse(self.base_url)
            if parsed.hostname in {"localhost", "127.0.0.1", "::1"}:
                url = urllib.parse.urlunparse(
                    parsed._replace(scheme=base.scheme or "http", netloc=base.netloc)
                )
        req = urllib.request.Request(url)
        token = base64.b64encode(f"{self.auth[0]}:{self.auth[1]}".encode()).decode()
        req.add_header("Authorization", f"Basic {token}")
        return req

    def download(self, url: str) -> bytes:
        req = self._download_request(url)
        with urllib.request.urlopen(req, timeout=15) as resp:
            return resp.read()

    def download_to_file(self, url: str, path: Path, *, max_bytes: int) -> None:
        """Bounded memory, atomic publication; failed transfers never become cached files."""
        temporary: Optional[str] = None
        try:
            with urllib.request.urlopen(self._download_request(url), timeout=60) as resp:
                expected = resp.headers.get("Content-Length")
                length = int(expected) if expected else None
                if length is not None and length > max_bytes:
                    raise ValueError("Media exceeds the download size limit")
                with tempfile.NamedTemporaryFile(dir=path.parent, suffix=".part", delete=False) as out:
                    temporary = out.name
                    total = 0
                    while chunk := resp.read(64 * 1024):
                        total += len(chunk)
                        if total > max_bytes:
                            raise ValueError("Media exceeds the download size limit")
                        out.write(chunk)
                if total == 0 or (length is not None and total != length):
                    raise OSError("Media transfer was incomplete")
            os.replace(temporary, path)
            temporary = None
        finally:
            if temporary:
                Path(temporary).unlink(missing_ok=True)


def load_or_start_gowa(
    webhook_url: Optional[str] = None,
) -> tuple[str, tuple[str, str], Optional[subprocess.Popen]]:
    existing = os.environ.get("SHUTTLE_GOWA_URL")
    password_env = os.environ.get("SHUTTLE_GOWA_PASSWORD")
    if existing:
        user = os.environ.get("SHUTTLE_GOWA_USER", GOWA_USER)
        password = password_env or "shuttle"
        return existing.rstrip("/"), (user, password), None

    home = gowa_home()
    home.mkdir(parents=True, exist_ok=True)
    state_path = home / "runtime.json"
    lock_path = home / "gowa.lock"

    # Serialize start so only one GOWA can be created; kill strays under the lock.
    lock_f = open(lock_path, "a+", encoding="utf-8")
    file_lock_exclusive(lock_f)
    try:
        reusable = _try_reuse_gowa(state_path)
        if reusable is not None:
            return reusable

        binary = find_gowa_binary()
        if binary is None:
            raise FileNotFoundError(
                "GOWA binary not found. Run ./connectors/gowa/fetch.sh or set SHUTTLE_GOWA_BIN."
            )

        # Hard singleton: never leave more than one Shuttle GOWA alive.
        for pid in _find_gowa_pids(binary):
            log(f"killing stray GOWA pid {pid}")
            terminate_pid(pid)

        sock = socket.socket()
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
        sock.close()
        password = secrets.token_urlsafe(18)
        url = f"http://127.0.0.1:{port}"
        env = os.environ.copy()
        env.update(
            {
                "APP_HOST": "127.0.0.1",
                "APP_PORT": str(port),
                "APP_OS": "Shuttle",
                "APP_DEBUG": "false",
                "APP_BASIC_AUTH": f"{GOWA_USER}:{password}",
                "WHATSAPP_AUTO_MARK_READ": "false",
                "WHATSAPP_AUTO_DOWNLOAD_MEDIA": "false",
                "WHATSAPP_PRESENCE_ON_CONNECT": "unavailable",
                "WHATSAPP_CHAT_STORAGE": "true",
                "DB_URI": f"file:{home / 'storages' / 'whatsapp.db'}?_foreign_keys=on",
            }
        )
        cmd = [
            str(binary),
            "rest",
            "--host=127.0.0.1",
            f"--port={port}",
            "--os=Shuttle",
            f"--basic-auth={GOWA_USER}:{password}",
        ]
        if webhook_url:
            env["WHATSAPP_WEBHOOK"] = webhook_url
            cmd.append(f"--webhook={webhook_url}")
        log(f"starting GOWA {binary} on {url}")
        proc = subprocess.Popen(
            cmd,
            cwd=str(home),
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=open(home / "gowa.log", "ab"),
            # Stay in the connector process tree so Shuttle/connector death cleans GOWA up.
            preexec_fn=child_pdeathsig if sys.platform == "linux" else None,
        )
        state_path.write_text(
            json.dumps({"url": url, "password": password, "pid": proc.pid, "port": port})
        )
        if not wait_http(url, (GOWA_USER, password), timeout=25):
            proc.kill()
            try:
                state_path.unlink(missing_ok=True)
            except TypeError:
                if state_path.exists():
                    state_path.unlink()
            raise RuntimeError("GOWA did not become ready on loopback")
        return url, (GOWA_USER, password), proc
    finally:
        file_unlock(lock_f)
        lock_f.close()


def _try_reuse_gowa(
    state_path: Path,
) -> Optional[tuple[str, tuple[str, str], Optional[subprocess.Popen]]]:
    if not state_path.exists():
        return None
    try:
        state = json.loads(state_path.read_text())
        url = state.get("url")
        password = state.get("password")
        pid = state.get("pid")
        if url and password and pid and pid_alive(int(pid)):
            if wait_http(url, (GOWA_USER, password), timeout=3):
                return url, (GOWA_USER, password), None
            log(f"GOWA pid {pid} alive but HTTP not ready; replacing")
            terminate_pid(int(pid))
        elif pid and pid_alive(int(pid)):
            terminate_pid(int(pid))
    except Exception as e:
        log(f"stale GOWA state ignored: {e}")
    try:
        state_path.unlink(missing_ok=True)
    except TypeError:
        if state_path.exists():
            state_path.unlink()
    return None


def _find_gowa_pids(binary: Path) -> list[int]:
    """Find Shuttle-managed GOWA rest processes (max should be 0 or 1)."""
    bin_name = binary.name
    pids = set(
        find_processes_matching("rest", "--host=127.0.0.1", "--os=Shuttle", binary_name=bin_name)
    )
    pids.update(
        find_processes_matching(
            "rest", "--host=127.0.0.1", "basic-auth=shuttle:", binary_name=bin_name
        )
    )
    return sorted(pids)


def stop_gowa_singleton() -> None:
    """Stop the shared GOWA process for this Shuttle data dir (if any)."""
    state_path = gowa_home() / "runtime.json"
    if not state_path.exists():
        return
    try:
        state = json.loads(state_path.read_text())
        pid = state.get("pid")
        if pid and pid_alive(int(pid)):
            log(f"stopping GOWA pid {pid}")
            terminate_pid(int(pid))
    except Exception as e:
        log(f"GOWA stop failed: {e}")
    try:
        state_path.unlink(missing_ok=True)
    except TypeError:
        if state_path.exists():
            state_path.unlink()


class MiniWebSocket:
    def __init__(self, host: str, port: int, path: str, auth: tuple[str, str]):
        self.host = host
        self.port = port
        self.path = path
        self.auth = auth
        self.sock: Optional[socket.socket] = None

    def connect(self) -> None:
        key = base64.b64encode(os.urandom(16)).decode()
        token = base64.b64encode(f"{self.auth[0]}:{self.auth[1]}".encode()).decode()
        req = (
            f"GET {self.path} HTTP/1.1\r\n"
            f"Host: {self.host}:{self.port}\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n"
            f"Authorization: Basic {token}\r\n"
            "\r\n"
        )
        sock = socket.create_connection((self.host, self.port), timeout=15)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_KEEPALIVE, 1)
        sock.sendall(req.encode())
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = sock.recv(4096)
            if not chunk:
                raise ConnectionError("websocket handshake closed")
            buf += chunk
        header, rest = buf.split(b"\r\n\r\n", 1)
        status_line = header.split(b"\r\n", 1)[0]
        if b"101" not in status_line:
            raise ConnectionError(f"websocket handshake failed: {status_line!r}")
        # Idle timeout only for keepalive pings — do not treat it as a disconnect.
        sock.settimeout(25)
        self.sock = sock
        self._pending = rest

    def recv_text(self) -> Optional[str]:
        assert self.sock is not None
        while True:
            try:
                opcode, payload = self._read_frame()
            except socket.timeout:
                try:
                    self._write_frame(0x9, b"ping")
                except OSError:
                    return None
                continue
            if opcode is None:
                return None
            if opcode == 0x8:
                return None
            if opcode == 0x9:
                self._write_frame(0xA, payload)
                continue
            if opcode == 0xA:
                continue
            if opcode in (0x1, 0x0):
                return payload.decode("utf-8", errors="replace")

    def close(self) -> None:
        if self.sock:
            try:
                self._write_frame(0x8, b"")
                self.sock.close()
            except OSError:
                pass
            self.sock = None

    def _read_exact(self, n: int) -> bytes:
        assert self.sock is not None
        buf = getattr(self, "_pending", b"")
        self._pending = b""
        while len(buf) < n:
            chunk = self.sock.recv(max(n - len(buf), 4096))
            if not chunk:
                raise ConnectionError("socket closed")
            buf += chunk
        out, self._pending = buf[:n], buf[n:]
        return out

    def _read_frame(self) -> tuple[Optional[int], bytes]:
        try:
            hdr = self._read_exact(2)
        except socket.timeout:
            raise
        except (OSError, ConnectionError):
            return None, b""
        opcode = hdr[0] & 0x0F
        length = hdr[1] & 0x7F
        masked = bool(hdr[1] & 0x80)
        if length == 126:
            length = struct.unpack("!H", self._read_exact(2))[0]
        elif length == 127:
            length = struct.unpack("!Q", self._read_exact(8))[0]
        mask = self._read_exact(4) if masked else b""
        payload = self._read_exact(length)
        if masked:
            payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        return opcode, payload

    def _write_frame(self, opcode: int, payload: bytes) -> None:
        assert self.sock is not None
        mask = os.urandom(4)
        header = bytearray()
        header.append(0x80 | opcode)
        n = len(payload)
        if n < 126:
            header.append(0x80 | n)
        elif n < 65536:
            header.append(0x80 | 126)
            header.extend(struct.pack("!H", n))
        else:
            header.append(0x80 | 127)
            header.extend(struct.pack("!Q", n))
        header.extend(mask)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self.sock.sendall(header + masked)


_sessions_by_account: dict[str, "WhatsAppSession"] = {}
_webhook_lock = threading.Lock()
_webhook_url: Optional[str] = None


class _GowaWebhookHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: Any) -> None:  # noqa: A002 - base class name
        return

    def _ok(self) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(b"ok")

    def do_GET(self) -> None:  # noqa: N802
        self._ok()

    def do_HEAD(self) -> None:  # noqa: N802
        self.send_response(200)
        self.end_headers()

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length > 0 else b""
        self._ok()
        try:
            payload = json.loads(raw.decode("utf-8") or "{}")
        except json.JSONDecodeError:
            return
        if isinstance(payload, dict):
            threading.Thread(
                target=_dispatch_gowa_webhook,
                args=(payload,),
                daemon=True,
                name="wa-webhook",
            ).start()


def ensure_webhook_server() -> Optional[str]:
    global _webhook_url
    with _webhook_lock:
        if _webhook_url:
            return _webhook_url
        sock = socket.socket()
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
        sock.close()
        try:
            class _WebhookServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
                daemon_threads = True
                allow_reuse_address = True

            server = _WebhookServer(("127.0.0.1", port), _GowaWebhookHandler)
        except OSError as e:
            log(f"webhook listen failed: {e}")
            return None
        threading.Thread(target=server.serve_forever, daemon=True, name="wa-webhook-http").start()
        _webhook_url = f"http://127.0.0.1:{port}/"
        log(f"GOWA webhook listening on {_webhook_url}")
        return _webhook_url


def route_webhook(
    payload: dict[str, Any], sessions: dict[str, "WhatsAppSession"]
) -> Optional["WhatsAppSession"]:
    """Pick the session a GOWA webhook belongs to.

    `session_id` is the device id Shuttle registered (= Shuttle account id); the device
    JID's user part is the fallback. Guessing the only session is allowed only when there
    is exactly one.
    """
    snapshot = dict(sessions)
    aid = str(payload.get("session_id") or "")
    if aid and aid in snapshot:
        return snapshot[aid]
    device = str(payload.get("device_id") or "")
    if device:
        if device in snapshot:
            return snapshot[device]
        for candidate in snapshot.values():
            if candidate._self_jid and jids_same(candidate._self_jid, device):
                return candidate
    if not aid and len(snapshot) == 1:
        # Exactly one account: no other session can own this event.
        return next(iter(snapshot.values()))
    return None


def _dispatch_gowa_webhook(payload: dict[str, Any]) -> None:
    session = route_webhook(payload, _sessions_by_account)
    if session is None:
        log("webhook dropped: no matching WhatsApp session")
        return
    log(f"webhook event={payload.get('event') or payload.get('code')}")
    try:
        session._handle_ws(payload)
    except Exception as e:
        log(f"webhook: {e}")


def _as_dict(value: Any) -> dict[str, Any]:
    """A decoded JSON value as a dict, or an empty one (payloads arrive untyped)."""
    return value if isinstance(value, dict) else {}


def _as_list(value: Any) -> list[Any]:
    """A decoded JSON value as a list, or an empty one."""
    return value if isinstance(value, list) else []


def results(payload: Any) -> dict[str, Any]:
    if isinstance(payload, dict):
        r = payload.get("results")
        if isinstance(r, dict):
            return r
        return payload
    return {}


def user_info_record(payload: Any) -> dict[str, Any]:
    """Unwrap GOWA /user/info results (`{data: [ {...} ]}` or a flat dict)."""
    r = results(payload)
    data = r.get("data")
    if isinstance(data, list) and data:
        first = data[0]
        return first if isinstance(first, dict) else {}
    if isinstance(data, dict):
        return data
    return r if isinstance(r, dict) else {}


def jid_user_part(jid: str) -> str:
    return str(jid).split("@", 1)[0]


def looks_like_jid(value: str) -> bool:
    s = str(value or "").strip()
    if not s or "@" not in s:
        return False
    local, domain = s.split("@", 1)
    if not local or not domain:
        return False
    if domain in {"lid", "s.whatsapp.net", "g.us", "broadcast", "newsletter"}:
        return True
    return bool(local.replace("+", "").isdigit() and "." in domain)


def format_intl_phone(jid: str) -> Optional[str]:
    digits = "".join(ch for ch in jid_user_part(jid) if ch.isdigit())
    if not digits:
        return None
    return f"+{digits}"


def is_placeholder_name(name: str, jid: str = "") -> bool:
    """True when GOWA's chat.name is a phone, JID, or privacy-masked number."""
    n = (name or "").strip()
    if not n:
        return True
    user = jid_user_part(jid) if jid else ""
    if n == user or n == jid:
        return True
    if jid.endswith(("@g.us", "@newsletter")) and n.casefold() in {
        "group", "newsletter", "channel",
        f"group {user}".casefold(), f"newsletter {user}".casefold(), f"channel {user}".casefold(),
    }:
        return True
    if looks_like_jid(n):
        return True
    if any(ch in n for ch in ("∙", "•", "·")):
        return True
    letters = [c for c in n if c.isalpha()]
    digits = [c for c in n if c.isdigit()]
    if digits and not letters:
        return True
    return False


def load_newsletter_names(client: GowaClient, jids: list[str]) -> dict[str, str] | None:
    """Real names of the channels this account follows, from GOWA's newsletter info.

    GOWA's chat list names a channel `Newsletter <id>`, so the name WhatsApp keeps in the
    channel's own metadata is the only real source. Returns None when the endpoint is not
    there (an older GOWA), so the caller can stop asking.
    """
    out: dict[str, str] = {}
    for jid in jids:
        try:
            code, body = client.get("/newsletter/info", query={"newsletter_id": jid})
        except Exception as e:
            log(f"GET /newsletter/info failed: {e}")
            return None
        if code == 404:
            return None
        if code != 200:
            continue
        meta = results(body)
        thread = meta.get("thread_metadata")
        name = thread.get("name") if isinstance(thread, dict) else None
        if isinstance(name, dict):
            name = name.get("text")
        if isinstance(name, str) and name.strip():
            out[jid] = name.strip()
    return out


def load_saved_contacts(client: GowaClient) -> Optional[dict[str, str]]:
    """Address book as {jid: name, user: name}. Returns None when the fetch failed."""
    try:
        code, body = client.get("/user/my/contacts")
    except Exception as e:
        log(f"GET /user/my/contacts failed: {e}")
        return None
    if code != 200:
        log(f"GET /user/my/contacts returned {code}")
        return None
    rows = results(body).get("data")
    if rows is None:
        rows = []
    if not isinstance(rows, list):
        return None
    out: dict[str, str] = {}
    for row in rows:
        if not isinstance(row, dict):
            continue
        jid = row.get("jid") or row.get("id")
        name = row.get("name") or row.get("push_name") or row.get("full_name")
        if not jid or not name:
            continue
        name = str(name).strip()
        if not name:
            continue
        jid_s = str(jid)
        out[jid_s] = name
        out[jid_user_part(jid_s)] = name
    log(f"loaded {len(out) // 2} saved contacts")
    return out


def sender_only(value: Any) -> str:
    """GOWA writes a group sender as `<sender jid> in <group jid>`; keep the sender part."""
    return str(value or "").split(" in ", 1)[0].strip()


def lookup_contact_name(contacts: dict[str, str], jid: Optional[str]) -> Optional[str]:
    if not jid:
        return None
    key = str(jid)
    return contacts.get(key) or contacts.get(jid_user_part(key))


def _guess_mime(filename: str, media_type: str) -> str:
    name = (filename or "").lower()
    mt = (media_type or "").lower()
    if name.endswith(".png"):
        return "image/png"
    if name.endswith(".webp") or mt == "sticker":
        return "image/webp"
    if name.endswith(".gif"):
        return "image/gif"
    if name.endswith(".pdf"):
        return "application/pdf"
    if name.endswith((".mp3", ".m4a", ".ogg")):
        return "audio/mpeg"
    if mt in {"video"}:
        return "video/mp4"
    if mt in {"audio", "ptt"}:
        return "audio/ogg"
    if mt == "document":
        return "application/octet-stream"
    return "image/jpeg"


PREVIEW_LABELS: dict[str, str] = {
    "image": "📷 Photo",
    "photo": "📷 Photo",
    "sticker": "Sticker",
    "video": "🎬 Video",
    "audio": "🎵 Audio",
    "ptt": "🎤 Voice message",
    "document": "📎 Document",
    "contact": "👤 Contact",
    "poll": "📊 Poll",
    "event": "📅 Event",
    "location": "📍 Location",
}


def preview_for_message(text: str, extra: dict[str, Any]) -> str:
    media = str(extra.get("media_type") or "").lower()
    body = (text or "").strip()
    if not media:
        return body
    label = PREVIEW_LABELS.get(media, media.title())
    placeholder = f"[{media}]"
    raw_type = str(extra.get("media_type") or "")
    if body and body.lower() not in {placeholder, f"[{raw_type.lower()}]", f"[{raw_type}]"}:
        return body
    filename = extra.get("filename")
    if media == "document" and filename:
        return f"📎 {filename}"
    return label


# ------------------------------------------------------------------- polls
#
# GOWA reports a poll as `poll` beside the message: `{type, poll_id, question,
# options: [{name, hash}], selectable_options_count, selected_option_hashes, `
# `selected_options, resolution_status}`. A creation is a message. A vote, a poll edit and an
# added option are facts about the poll message that already exists, so they never land in the
# thread as messages of their own.


def poll_definition(poll: dict[str, Any]) -> dict[str, Any]:
    """Question and options of a GOWA `poll` payload, in the shape the core stores.

    An option's `hash` is its stable id and what a vote selects; a payload without hashes falls
    back to the option text, so an id always exists.
    """
    options: list[dict[str, str]] = []
    for raw in _as_list(poll.get("options")):
        row = _as_dict(raw)
        name = str(row.get("name") or "").strip()
        if not name:
            continue
        options.append({"id": str(row.get("hash") or "").strip() or name, "text": name})
    return {
        "question": str(poll.get("question") or "").strip(),
        "multiple": int(poll.get("selectable_options_count") or 0) > 1,
        "options": options,
    }


def poll_selected_ids(poll: dict[str, Any], definition: dict[str, Any]) -> list[str]:
    """Option ids one vote picked, empty when the voter cleared their vote.

    GOWA sends the selected hashes, which are the ids. A vote that could not be resolved to a
    definition still carries hashes, so it can be folded once the poll's own definition is
    stored. Names are a fallback for a payload built without hashes.
    """
    ids = [str(h).strip() for h in _as_list(poll.get("selected_option_hashes")) if str(h).strip()]
    if ids:
        return list(dict.fromkeys(ids))
    by_text = {o["text"]: o["id"] for o in definition.get("options") or []}
    for name in _as_list(poll.get("selected_options")):
        oid = by_text.get(str(name).strip())
        if oid and oid not in ids:
            ids.append(oid)
    return ids


def avatar_cache_file(account_id: str, jid: str) -> Path:
    digits = phone_digits(jid) or jid_user_part(jid)
    safe = "".join(ch if ch.isalnum() else "_" for ch in digits)[:80]
    folder = files_dir(account_id) / "avatars"
    folder.mkdir(parents=True, exist_ok=True)
    return folder / f"{safe}.jpg"


def _safe_media_stem(message_id: str) -> str:
    return "".join(ch if ch.isalnum() else "_" for ch in str(message_id))[:80]


_EXT_RE = re.compile(r"^\.[A-Za-z0-9]{1,8}$")

_MIME_EXTENSIONS: dict[str, str] = {
    "image/jpeg": ".jpg",
    "image/jpg": ".jpg",
    "image/pjpeg": ".jpg",
    "image/png": ".png",
    "image/webp": ".webp",
    "image/gif": ".gif",
    "image/heic": ".heic",
    "image/heif": ".heif",
    "image/bmp": ".bmp",
    "image/tiff": ".tif",
    "image/svg+xml": ".svg",
    "video/mp4": ".mp4",
    "video/3gpp": ".3gp",
    "video/quicktime": ".mov",
    "video/webm": ".webm",
    "video/x-matroska": ".mkv",
    "video/x-msvideo": ".avi",
    "audio/ogg": ".ogg",
    "audio/opus": ".ogg",
    "audio/mpeg": ".mp3",
    "audio/mp3": ".mp3",
    "audio/mp4": ".m4a",
    "audio/aac": ".aac",
    "audio/x-m4a": ".m4a",
    "audio/wav": ".wav",
    "audio/x-wav": ".wav",
    "audio/amr": ".amr",
    "audio/webm": ".weba",
    "application/pdf": ".pdf",
    "application/zip": ".zip",
    "application/json": ".json",
    "application/msword": ".doc",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document": ".docx",
    "application/vnd.ms-excel": ".xls",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet": ".xlsx",
    "application/vnd.ms-powerpoint": ".ppt",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation": ".pptx",
    "text/plain": ".txt",
    "text/csv": ".csv",
    "text/html": ".html",
}


def _ext_for_mime(mime: Optional[str]) -> str:
    """File extension (with dot) for a mime type, or "" when it is unknown or generic."""
    base = str(mime or "").split(";", 1)[0].strip().lower()
    return _MIME_EXTENSIONS.get(base, "")


def _ext_from_magic(head: bytes) -> str:
    """File extension (with dot) for well-known signatures in the first bytes, else ""."""
    if head.startswith(b"\xff\xd8\xff"):
        return ".jpg"
    if head.startswith(b"\x89PNG\r\n\x1a\n"):
        return ".png"
    if head[:6] in (b"GIF87a", b"GIF89a"):
        return ".gif"
    if head[:4] == b"RIFF" and head[8:12] == b"WEBP":
        return ".webp"
    if head[:4] == b"RIFF" and head[8:12] == b"WAVE":
        return ".wav"
    if head.startswith(b"BM") and len(head) > 14:
        return ".bmp"
    if head[:4] == b"%PDF":
        return ".pdf"
    if head[:4] == b"OggS":
        return ".ogg"
    if head[:3] == b"ID3" or (len(head) > 1 and head[0] == 0xFF and head[1] & 0xE0 == 0xE0):
        return ".mp3"
    if head[:4] == b"\x1a\x45\xdf\xa3":
        return ".webm"
    if head[:6] == b"#!AMR\n":
        return ".amr"
    if head[4:8] == b"ftyp":
        brand = head[8:12]
        if brand == b"qt  ":
            return ".mov"
        if brand in (b"heic", b"heix", b"mif1", b"msf1"):
            return ".heic"
        if brand[:3] == b"3gp":
            return ".3gp"
        if brand in (b"M4A ", b"M4B "):
            return ".m4a"
        return ".mp4"
    return ""


def pick_media_extension(
    media_type: str,
    filename: Optional[str] = None,
    mime: Optional[str] = None,
    head: Optional[bytes] = None,
) -> tuple[str, bool]:
    """Extension for a cached download and whether it is a firm fact (not a type default).

    Order: the reported file name's suffix, the mime type, the file's own magic bytes (when
    already downloaded), then the media type's default.
    """
    suffix = Path(str(filename)).suffix if filename else ""
    if _EXT_RE.match(suffix):
        return suffix.lower(), True
    ext = _ext_for_mime(mime)
    if ext:
        return ext, True
    ext = _ext_from_magic(head) if head else ""
    if ext:
        return ext, True
    return _ext_for_media(media_type), False


def media_cache_file(
    account_id: str,
    message_id: str,
    media_type: str,
    filename: Optional[str] = None,
    mime: Optional[str] = None,
) -> Path:
    ext, _ = pick_media_extension(media_type, filename, mime)
    folder = files_dir(account_id) / "media"
    folder.mkdir(parents=True, exist_ok=True)
    return folder / f"{_safe_media_stem(message_id)}{ext}"


def find_cached_media(account_id: str, message_id: str) -> Optional[Path]:
    """An already downloaded file for this message whatever extension it ended up with.

    Matches `<id>.<ext>` and the extensionless `<id>` older builds wrote; in-flight `.part`
    files never count.
    """
    stem = _safe_media_stem(message_id)
    if not stem:
        return None
    folder = files_dir(account_id) / "media"
    try:
        for entry in folder.iterdir():
            name = entry.name
            if name.endswith(".part") or not (name == stem or Path(name).stem == stem):
                continue
            if entry.is_file() and entry.stat().st_size > 0:
                return entry
    except OSError:
        pass
    return None


def settle_media_extension(
    path: Path, media_type: str, filename: Optional[str] = None, mime: Optional[str] = None
) -> Path:
    """Rename a finished download so its extension matches what the bytes really are.

    Only runs when the name was a guess (no usable file name or mime type), so a document
    called `report.xyz` keeps its own name. Returns the path the file now lives at.
    """
    try:
        known, firm = pick_media_extension(media_type, filename, mime)
        if firm:
            # A real name or mime type: keep an existing extension, add a missing one.
            if path.suffix:
                return path
            ext = known
        else:
            with open(path, "rb") as f:
                ext = _ext_from_magic(f.read(16))
            if not ext or ext == path.suffix.lower():
                return path
        target = path.with_name(path.stem + ext) if path.suffix else path.with_name(path.name + ext)
        if target.exists():
            return path
        os.replace(path, target)
        return target
    except OSError:
        return path


THUMB_MAX_BYTES = 64 * 1024
# Message kinds whose WhatsApp payload embeds a small JPEG preview (`JPEGThumbnail`).
THUMB_MESSAGE_KEYS = (
    "imageMessage",
    "videoMessage",
    "ptvMessage",
    "stickerMessage",
    "documentMessage",
)
THUMB_MEDIA_TYPES = {"image", "photo", "video", "sticker", "document"}
# Bumped when previews move to another folder: a state written for the old one forgets
# which dumps it already read, so the previews are extracted again where the UI looks.
THUMBS_LAYOUT = 2


def thumbnail_dir(account_id: str) -> Path:
    """Folder of message previews for one account (a sibling of `media/`, never purged)."""
    return files_dir(account_id) / "thumbs"


def thumbnail_file(account_id: str, message_id: str) -> Path:
    """Where the embedded preview of one message lives (kept small, never purged by retention)."""
    safe = "".join(ch if ch.isalnum() else "_" for ch in str(message_id))[:80]
    return thumbnail_dir(account_id) / f"{safe}.jpg"


def save_thumbnail(account_id: str, message_id: str, raw: bytes) -> Optional[Path]:
    """Write a message preview atomically. Only small real JPEGs are stored."""
    if not message_id or not raw or len(raw) > THUMB_MAX_BYTES or raw[:2] != b"\xff\xd8":
        return None
    path = thumbnail_file(account_id, message_id)
    if path.is_file() and path.stat().st_size > 0:
        return path
    tmp = path.with_name(path.name + ".part")
    try:
        tmp.write_bytes(raw)
        os.replace(tmp, path)
    except OSError as e:
        log(f"thumbnail write failed: {type(e).__name__}")
        tmp.unlink(missing_ok=True)
        return None
    return path


def history_message_thumbnail(wrap: dict[str, Any]) -> Optional[tuple[str, bytes]]:
    """(message id, JPEG bytes) when a history-dump message embeds a preview, else None.

    GOWA's REST rows and webhooks carry no thumbnail; only the history-sync dumps keep the
    protobuf `JPEGThumbnail` (base64 in the dump's JSON).
    """
    msg = wrap.get("message") if isinstance(wrap.get("message"), dict) else wrap
    if not isinstance(msg, dict):
        return None
    key = _as_dict(msg.get("key"))
    mid = str(key.get("ID") or key.get("id") or "").strip()
    inner = _as_dict(msg.get("message"))
    if not mid or not inner:
        return None
    for name in THUMB_MESSAGE_KEYS:
        part = inner.get(name)
        if not isinstance(part, dict):
            continue
        raw = _embedded_thumbnail(part)
        if raw:
            return (mid, raw)
    return None


STATUS_JID = "status@broadcast"
_URL_RE = re.compile(r"https?://\S+")
# Message parts a status post can carry, in the order WhatsApp writes them.
STATUS_MEDIA_PARTS = (
    ("imageMessage", "image"),
    ("videoMessage", "video"),
    ("ptvMessage", "video"),
    ("audioMessage", "audio"),
    ("documentMessage", "document"),
    ("stickerMessage", "sticker"),
)


def _jpeg_bytes(value: Any) -> bytes | None:
    """Base64 of an embedded preview as JPEG bytes, or None when it is not one."""
    if not isinstance(value, str) or not value:
        return None
    try:
        raw = base64.b64decode(value, validate=False)
    except Exception:
        return None
    return raw if raw[:2] == b"\xff\xd8" else None


def _embedded_thumbnail(part: dict[str, Any]) -> bytes | None:
    """The small JPEG WhatsApp embeds next to a media part, when there is one."""
    return _jpeg_bytes(part.get("JPEGThumbnail") or part.get("jpegThumbnail"))


def status_media_type(inner: dict[str, Any]) -> str:
    for part_name, label in STATUS_MEDIA_PARTS:
        if part_name in inner:
            return label
    return "text"


def status_post_text(inner: dict[str, Any]) -> str:
    """The words of a status post: its text, its link card text, or a media caption.

    Deliberately narrow. `nested_text` would fall through to the base64 preview or the
    media key and make them the body.
    """
    plain = inner.get("conversation")
    if isinstance(plain, str) and plain.strip():
        return plain.strip()
    card = inner.get("extendedTextMessage")
    if isinstance(card, dict):
        text = card.get("text")
        if isinstance(text, str) and text.strip():
            return text.strip()
    for part_name, _label in STATUS_MEDIA_PARTS:
        part = inner.get(part_name)
        if isinstance(part, dict):
            caption = part.get("caption")
            if isinstance(caption, str) and caption.strip():
                return caption.strip()
    return ""


def _first_link_url(part: dict[str, Any]) -> str:
    for key in ("canonicalUrl", "matchedText"):
        value = part.get(key)
        if isinstance(value, str) and value.strip().lower().startswith(("http://", "https://")):
            return value.strip()
    text = part.get("text")
    if isinstance(text, str):
        found = _URL_RE.search(text)
        if found:
            return found.group(0)
    return ""


def status_link_preview(inner: dict[str, Any]) -> dict[str, Any] | None:
    """The link card WhatsApp attached to a text status (a reel, a video, an article).

    The card rides inside the message: `matchedText` (or `canonicalUrl`) is the URL, `title`
    the headline and `JPEGThumbnail` a small preview image. The bytes come back under
    `thumbnail` for the caller to store; the other two keys are the reported card.
    """
    part = inner.get("extendedTextMessage")
    if not isinstance(part, dict):
        return None
    url = _first_link_url(part)
    if not url:
        return None
    title = part.get("title")
    return {
        "url": url,
        "title": title.strip() if isinstance(title, str) and title.strip() else None,
        "thumbnail": _embedded_thumbnail(part),
    }


def status_link_card(account_id: str, message_id: str, link: Any) -> dict[str, Any] | None:
    """The reported link card: its URL, its title, and the preview image when one came with it."""
    if not isinstance(link, dict):
        return None
    url = link.get("url")
    if not isinstance(url, str) or not url.strip():
        return None
    card: dict[str, Any] = {"url": url.strip(), "title": link.get("title") or None}
    raw = link.get("thumbnail")
    stored = save_thumbnail(account_id, f"link-{message_id}", raw) if isinstance(raw, bytes) else None
    card["thumbnail_path"] = str(stored) if stored else None
    return card


def lid_pairs(value: Any) -> dict[str, str]:
    """A dump's `phoneNumberToLidMappings` as {LID user part: phone JID}."""
    out: dict[str, str] = {}
    if not isinstance(value, list):
        return out
    for row in value:
        if not isinstance(row, dict):
            continue
        lid = str(row.get("lidJID") or row.get("lid") or "").strip()
        phone = str(row.get("pnJID") or row.get("pn") or "").strip()
        if lid and phone:
            out[jid_core_user(lid)] = phone
    return out


def status_thumbnail_bytes(inner: dict[str, Any]) -> bytes | None:
    """The small JPEG WhatsApp embeds in a status post, whichever media it is."""
    for part_name, _label in STATUS_MEDIA_PARTS:
        part = inner.get(part_name)
        if isinstance(part, dict):
            raw = _embedded_thumbnail(part)
            if raw:
                return raw
    return None


def status_post_facts(entry: dict[str, Any]) -> dict[str, Any] | None:
    """One status post out of a `statusV3Messages` dump entry or a live webhook payload."""
    if not isinstance(entry, dict):
        return None
    key = _as_dict(entry.get("key"))
    inner = _as_dict(entry.get("message"))
    if not inner:
        return None
    ts = to_rfc3339_strict(entry.get("messageTimestamp") or entry.get("timestamp"))
    if not ts:
        return None
    inner = {
        k: v
        for k, v in inner.items()
        if k not in {"messageContextInfo", "deviceSentMessage", "senderKeyDistributionMessage"}
    }
    sender = str(
        entry.get("participant") or key.get("participant") or entry.get("from") or ""
    ).strip()
    thumb = status_thumbnail_bytes(inner)
    media_type = status_media_type(inner)
    return {
        "id": str(key.get("ID") or key.get("id") or entry.get("id") or "").strip(),
        "sender": sender,
        "timestamp": ts,
        "from_me": bool(key.get("fromMe") or entry.get("from_me")),
        "media_type": media_type,
        "text": status_post_text(inner)[:500],
        "thumbnail": thumb,
        "link_preview": status_link_preview(inner),
    }


def _ext_for_media(media_type: str) -> str:
    mt = (media_type or "").lower()
    if mt == "sticker":
        return ".webp"
    if mt == "video":
        return ".mp4"
    if mt in {"audio", "ptt"}:
        return ".ogg"
    if mt == "document":
        return ".bin"
    return ".jpg"


def bytes_to_data_url(raw: bytes) -> str:
    mime = "image/png" if raw.startswith(b"\x89PNG") else "image/jpeg"
    if raw[:4] == b"RIFF" and raw[8:12] == b"WEBP":
        mime = "image/webp"
    return f"data:{mime};base64,{base64.b64encode(raw).decode()}"


def load_cached_avatar(account_id: str, jid: str) -> Optional[str]:
    path = avatar_cache_file(account_id, jid)
    if not path.is_file():
        return None
    raw = path.read_bytes()
    if not raw:
        return None
    return bytes_to_data_url(raw)


def fetch_avatar_bytes(client: GowaClient, account_id: str, jid: str) -> Optional[bytes]:
    cached = avatar_cache_file(account_id, jid)
    if cached.is_file() and cached.stat().st_size > 0:
        return cached.read_bytes()
    code, body = client.get("/user/avatar", query={"phone": jid, "is_preview": "true"})
    if code != 200:
        return None
    url = results(body).get("url")
    if not url:
        return None
    try:
        req = urllib.request.Request(str(url), headers={"User-Agent": "Mozilla/5.0 Shuttle"})
        with urllib.request.urlopen(req, timeout=12) as resp:
            raw = resp.read()
        if raw:
            cached.write_bytes(raw)
        return raw or None
    except Exception as e:
        log(f"avatar download {jid}: {e}")
        return None


def download_gowa_media(
    client: GowaClient,
    account_id: str,
    message_id: str,
    phone: str,
    *,
    for_status: bool = False,
) -> Optional[dict[str, Any]]:
    code, body = client.get(
        f"/message/{urllib.parse.quote(str(message_id), safe='')}/download",
        query={"phone": phone},
        timeout=25.0,
    )
    if code != 200:
        # GOWA wraps WhatsApp CDN errors in HTTP 500. Preserve the reason without
        # exposing raw upstream messages (which can contain account identifiers).
        reason = str(body.get("message") or "").lower() if isinstance(body, dict) else ""
        if "status code 403" in reason or "status code 404" in reason or "status code 410" in reason:
            error = "WhatsApp could not retrieve this media. Try again with your phone online, or ask the sender to resend it."
        elif "not found" in reason:
            error = "This media is not in this linked device's message history."
        elif code == 401:
            error = "WhatsApp is still reconnecting. Try the download again shortly."
        else:
            error = "WhatsApp media download failed. Try again."
        log(f"media download failed: HTTP {code}; {error}")
        return {"error": error}
    r = results(body)
    file_url = r.get("file_url")
    if not file_url:
        return {"error": "WhatsApp did not provide a media download URL."}
    media_type = str(r.get("media_type") or "image").lower()
    filename = r.get("filename")
    mime = r.get("mime_type") or r.get("mimetype") or r.get("mime")
    path = media_cache_file(account_id, message_id, media_type, filename, mime)
    done = {
        "media_type": media_type,
        "media_path": str(path),
        "filename": filename,
    }
    try:
        # Reuse an earlier download whatever extension it got (older builds wrote none).
        cached = path if path.is_file() and path.stat().st_size > 0 else find_cached_media(account_id, message_id)
        if cached is not None:
            cached = settle_media_extension(cached, media_type, filename, mime)
            touch_cached(cached)
            done["media_path"] = str(cached)
            return done
        try:
            client.download_to_file(
                str(file_url), path, max_bytes=30_000_000 if for_status else 2_000_000_000
            )
        except ValueError:
            return {"error": "Media exceeds the download size limit (30 MB for statuses, 2 GB for messages)."}
        except Exception as e:
            log(f"media file transfer failed: {type(e).__name__}")
            return {"error": "Media transfer failed. Check your connection and free disk space, then retry."}
        done["media_path"] = str(settle_media_extension(path, media_type, filename, mime))
        return done
    finally:
        # GOWA wrote its own copy under statics/media; ours is the only one that should stay.
        discard_gowa_copy(str(file_url))


def touch_cached(path: Path) -> None:
    """Mark a cached file as just used so retention (last use, else mtime) keeps it."""
    try:
        os.utime(path, None)
    except OSError:
        pass


def discard_gowa_copy(file_url: str) -> None:
    """Delete the file GOWA produced for `file_url`, but only inside GOWA's own media folder."""
    try:
        url_path = urllib.parse.urlparse(file_url).path
        marker = "/statics/media/"
        if marker not in url_path:
            return
        name = Path(urllib.parse.unquote(url_path.split(marker, 1)[1])).name
        if not name:
            return
        (gowa_home() / "statics" / "media" / name).unlink(missing_ok=True)
    except Exception as e:
        log(f"gowa copy cleanup skipped: {type(e).__name__}")


_NESTED_TEXT_SKIP = {
    "chat_id",
    "chat_jid",
    "chat_lid",
    "from",
    "to",
    "sender_jid",
    "remotejid",
    "participant",
    "jid",
    "id",
    "message_id",
    "timestamp",
    "device_id",
    "session_id",
    "from_lid",
    "from_name",
    "verified_name",
    # A sender's verification level is not text. WhatsApp's default value for it is the
    # literal `unknown`, which used to become the body of a captionless media message.
    "verified_level",
    "lid",
    "key",
    "info",
    "url",
    "media_path",
    # Names and file facts are never a caption: without these a media message with no text
    # picked up the sender's push name (or the mime type / file name) as its body.
    "pushname",
    "push_name",
    "notify_name",
    "sender_display_name",
    "sender_name",
    "sender",
    "mime_type",
    "mimetype",
    "mime",
    "media_type",
    "file_name",
    "filename",
    "filesha256",
    "fileencsha256",
    "mediakey",
    "jpegthumbnail",
    # The text a reply quotes belongs to the older message, never to this one.
    "quoted_body",
    "quotedmessage",
    "quoted_message",
    "context_metadata",
    "contextinfo",
    "replied_to_id",
    "reply_to",
}

_MIME_TEXT_RE = re.compile(r"^[a-z]+/[a-z0-9.+-]+\s*(;.*)?$", re.IGNORECASE)
# GOWA's live webhook names the media by type instead of sending a `media_type` field.
_LIVE_MEDIA_KEYS = ("image", "video", "audio", "document", "sticker")
# The file hash WhatsApp puts in the body of a media message that has no caption.
_FILE_HASH_RE = re.compile(r"^(?:[0-9a-f]{16,}|[A-Za-z0-9+/]{33,}={0,2})$", re.IGNORECASE)
_NAME_KEYS = (
    "from_name",
    "sender_display_name",
    "pushname",
    "push_name",
    "notify_name",
    "verified_name",
    "sender_name",
)


def nested_text(value: Any) -> str:
    if isinstance(value, str) and value.strip():
        if looks_like_jid(value.strip()):
            return ""
        return value
    if isinstance(value, dict):
        for key in (
            "hydratedContentText",
            "conversation",
            "caption",
            "text",
            "body",
            "content",
            "matchedText",
        ):
            found = nested_text(value.get(key))
            if found:
                return found
        for key, nested in value.items():
            if str(key).lower() in _NESTED_TEXT_SKIP:
                continue
            found = nested_text(nested)
            if found:
                return found
    if isinstance(value, list):
        for nested in value:
            found = nested_text(nested)
            if found:
                return found
    return ""


def _is_media_noise(text: str, row: dict[str, Any], extra: dict[str, Any]) -> bool:
    """Body text of a media message that is not a caption: the sender's name, a mime type, a file
    hash or the file's own name. Real captions are kept."""
    t = text.strip()
    if _MIME_TEXT_RE.match(t) or _FILE_HASH_RE.match(t):
        return True
    names = {str(row.get(k)).strip() for k in _NAME_KEYS if row.get(k)}
    if t in names:
        return True
    filename = extra.get("filename")
    return bool(filename) and t == str(filename).strip()


def message_body_and_media(row: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    text = row.get("content") or row.get("message") or row.get("text") or row.get("body") or ""
    if isinstance(text, dict):
        text = nested_text(text) or text.get("text") or text.get("conversation") or ""
    text = str(text or "")
    if not text.strip():
        text = nested_text(row)
    extra: dict[str, Any] = {}
    media_type = row.get("media_type")
    if not media_type:
        media_type = next((kind for kind in _LIVE_MEDIA_KEYS if row.get(kind)), None)
    if not media_type:
        blob = json.dumps(row, default=str).lower()
        if "imagemessage" in blob or '"image"' in blob:
            media_type = "image"
        elif "videomessage" in blob:
            media_type = "video"
        elif "audiomessage" in blob or "ptt" in blob:
            media_type = "audio"
        elif "documentmessage" in blob:
            media_type = "document"
        elif "stickermessage" in blob:
            media_type = "sticker"
        elif "locationmessage" in blob:
            media_type = "location"
        elif "pollcreationmessage" in blob or "templateMessage" in json.dumps(row, default=str):
            if "poll" in blob:
                media_type = "poll"
    if media_type:
        extra["media_type"] = str(media_type)
        # Live webhooks describe the media in `media_info`; REST rows use flat fields.
        info = _as_dict(row.get("media_info"))
        filename = row.get("filename") or info.get("file_name")
        if filename:
            extra["filename"] = filename
        file_length = row.get("file_length")
        if file_length is None:
            file_length = info.get("file_length")
        if isinstance(file_length, (int, float)) and file_length > 0:
            extra["file_length"] = int(file_length)
        elif row.get("file_length") is not None:
            extra["file_length"] = row.get("file_length")
        if isinstance(info.get("mime_type"), str) and info["mime_type"]:
            extra["mime"] = info["mime_type"]
        for src, dst in (("width", "width"), ("height", "height"), ("seconds", "duration")):
            v = info.get(src)
            if isinstance(v, (int, float)) and v > 0:
                extra[dst] = v
        if text.strip() and _is_media_noise(text, row, extra):
            text = ""
        if not text.strip():
            text = f"[{media_type}]"
    if text.strip() and looks_like_jid(text.strip()) and not extra.get("media_type"):
        text = ""
    return text, extra


_MENTION_TOKEN_RE = re.compile(r"(?<![\w@])@(\d{5,16})(?!\d)")
_MENTION_KEYS = {"mentionedjid", "mentionedjids", "mentioned_jid", "mentioned_jids"}
_QUOTED_KEYS = {"quotedmessage", "quoted_message"}


def mention_tokens(text: Any) -> list[str]:
    """Digit runs written as `@<digits>` in a message text (a phone number or a LID), in order."""
    seen: list[str] = []
    for m in _MENTION_TOKEN_RE.finditer(str(text or "")):
        if m.group(1) not in seen:
            seen.append(m.group(1))
    return seen


def mentioned_jids_in(obj: Any, depth: int = 0) -> list[str]:
    """JIDs a message payload lists as mentioned (`contextInfo.mentionedJID`).

    The quoted message has a contextInfo of its own: its mentions belong to the older message
    and are skipped.
    """
    out: list[str] = []
    if depth > 6:
        return out
    if isinstance(obj, dict):
        for key, val in obj.items():
            k = str(key).lower()
            if k in _QUOTED_KEYS:
                continue
            if k in _MENTION_KEYS and isinstance(val, list):
                out.extend(str(j) for j in val if isinstance(j, str) and j.strip())
            elif isinstance(val, (dict, list)):
                out.extend(mentioned_jids_in(val, depth + 1))
    elif isinstance(obj, list):
        for val in obj:
            out.extend(mentioned_jids_in(val, depth + 1))
    return list(dict.fromkeys(out))


# ------------------------------------------------------------------ replies
#
# A reply is a fact about a message: "this message answers that older one". The connector only
# reports it (`reply_to` in the message metadata); the core resolves it against the stored
# original when it shows the quote.

_REPLY_SNIPPET_CHARS = 160
_QUOTED_MEDIA_KEYS = (
    ("imageMessage", "image"),
    ("videoMessage", "video"),
    ("audioMessage", "audio"),
    ("documentMessage", "document"),
    ("documentWithCaptionMessage", "document"),
    ("stickerMessage", "sticker"),
    ("locationMessage", "location"),
    ("liveLocationMessage", "location"),
    ("contactMessage", "contact"),
    ("pollCreationMessage", "poll"),
)


def _clip_snippet(text: Any) -> str:
    t = re.sub(r"\s+", " ", str(text or "")).strip()
    return t[:_REPLY_SNIPPET_CHARS]


def _json_dict(value: Any) -> dict[str, Any]:
    """A dict from a dict or a JSON object string (GOWA stores `context_metadata` as a string)."""
    if isinstance(value, dict):
        return value
    if isinstance(value, str) and value.strip().startswith("{"):
        try:
            parsed = json.loads(value)
        except (ValueError, TypeError):
            return {}
        return parsed if isinstance(parsed, dict) else {}
    return {}


def quoted_message_facts(quoted: Any) -> tuple[str, Optional[str]]:
    """(snippet, media_type) of a WhatsApp `quotedMessage` proto as a history dump writes it."""
    if not isinstance(quoted, dict):
        return "", None
    media_type: Optional[str] = None
    for key, kind in _QUOTED_MEDIA_KEYS:
        if key in quoted:
            media_type = kind
            break
    text = ""
    ext = quoted.get("extendedTextMessage")
    if isinstance(quoted.get("conversation"), str):
        text = quoted["conversation"]
    elif isinstance(ext, dict) and isinstance(ext.get("text"), str):
        text = ext["text"]
    elif media_type:
        for key, _kind in _QUOTED_MEDIA_KEYS:
            node = quoted.get(key)
            if isinstance(node, dict):
                if key == "documentWithCaptionMessage":
                    node = ((node.get("message") or {}).get("documentMessage")) or node
                caption = node.get("caption")
                if isinstance(caption, str):
                    text = caption
                break
    return _clip_snippet(text), media_type


def _find_context_info(obj: Any, depth: int = 0) -> Optional[dict[str, Any]]:
    """The `contextInfo` that belongs to a message itself (the one naming what it replies to).

    The quoted message carries a `contextInfo` of its own; the search never enters it.
    """
    if depth > 6 or not isinstance(obj, dict):
        return None
    direct = obj.get("contextInfo")
    if isinstance(direct, dict) and (direct.get("stanzaID") or direct.get("stanzaId")):
        return direct
    for key, val in obj.items():
        k = str(key).lower()
        if k in _QUOTED_KEYS or k in ("contextinfo", "messagecontextinfo", "key"):
            continue
        if isinstance(val, dict):
            found = _find_context_info(val, depth + 1)
            if found:
                return found
    return None


def reply_to_from_dump(inner: Any) -> Optional[dict[str, Any]]:
    """`reply_to` fact of a history-dump message body (`message.message`), or None.

    Keys: `remote_id` (the replied-to message id), `sender` (its author as written in the dump,
    when named), `snippet` and `media_type` of the quoted content.
    """
    ctx = _find_context_info(inner)
    if not ctx:
        return None
    rid = str(ctx.get("stanzaID") or ctx.get("stanzaId") or "").strip()
    if not rid:
        return None
    fact: dict[str, Any] = {"remote_id": rid}
    participant = ctx.get("participant")
    if isinstance(participant, str) and participant.strip():
        fact["sender"] = participant.strip()
    snippet, media_type = quoted_message_facts(ctx.get("quotedMessage"))
    if snippet:
        fact["snippet"] = snippet
    if media_type:
        fact["media_type"] = media_type
    return fact


def reply_to_from_row(*sources: Any) -> Optional[dict[str, Any]]:
    """`reply_to` fact of a live GOWA payload or a REST message row, or None.

    GOWA reports a reply as `replied_to_id` (and `quoted_body`, the older message's text) at
    the top of a webhook payload, and as `{"replied_to_id": ...}` inside the JSON string
    `context_metadata` of a REST row. Older GOWA builds nested `replied_id` / `quoted_message`
    under `message`. A ready-made `reply_to` object is passed through.
    """
    rid = ""
    snippet = ""
    sender = ""
    for src in sources:
        if not isinstance(src, dict):
            continue
        layers = [src, _json_dict(src.get("context_metadata"))]
        if isinstance(src.get("message"), dict):
            layers.append(src["message"])
        for layer in layers:
            for key in ("replied_to_id", "replied_id", "reply_to_id"):
                v = layer.get(key)
                if not rid and isinstance(v, (str, int)) and str(v).strip():
                    rid = str(v).strip()
            for key in ("quoted_body", "quoted_message"):
                v = layer.get(key)
                if not snippet and isinstance(v, str) and v.strip():
                    snippet = _clip_snippet(v)
        ready = src.get("reply_to")
        if isinstance(ready, dict):
            if not rid and str(ready.get("remote_id") or "").strip():
                rid = str(ready["remote_id"]).strip()
            if not snippet and isinstance(ready.get("snippet"), str):
                snippet = _clip_snippet(ready["snippet"])
            if not sender and isinstance(ready.get("sender"), str):
                sender = ready["sender"].strip()
    if not rid:
        return None
    fact: dict[str, Any] = {"remote_id": rid}
    if sender:
        fact["sender"] = sender
    if snippet:
        fact["snippet"] = snippet
    return fact


def whatsapp_db_path() -> Path:
    return gowa_home() / "storages" / "whatsapp.db"


def load_lid_map() -> dict[str, str]:
    path = whatsapp_db_path()
    if not path.is_file():
        return {}
    try:
        import sqlite3

        con = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        out: dict[str, str] = {}
        try:
            for lid, pn in con.execute("SELECT lid, pn FROM whatsmeow_lid_map"):
                if not lid or not pn:
                    continue
                lid_s = str(lid)
                pn_s = str(pn)
                if "@" not in lid_s:
                    lid_s = f"{lid_s}@lid"
                if "@" not in pn_s:
                    pn_s = f"{pn_s}@s.whatsapp.net"
                out[lid_s] = pn_s
                out[jid_user_part(lid_s)] = pn_s
        finally:
            con.close()
        return out
    except Exception as e:
        log(f"lid map: {e}")
        return {}


def load_chat_settings(
    lid_map: Optional[dict[str, str]] = None,
    own_jid: Optional[str] = None,
    db_path: Optional[Path] = None,
) -> dict[str, dict[str, bool]]:
    """Pinned/archived flags from whatsmeow (GOWA /chats omits these).

    The table is shared by every account in the GOWA database, so only rows whose
    `our_jid` matches this account's own JID (user part) are used. Without a known
    own JID nothing is returned.
    """
    if not own_jid:
        return {}
    path = db_path or whatsapp_db_path()
    if not path.is_file():
        return {}
    own_user = jid_core_user(own_jid)
    if not own_user:
        return {}
    try:
        import sqlite3

        con = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        out: dict[str, dict[str, bool]] = {}
        try:
            cols = {str(r[1]).lower() for r in con.execute("PRAGMA table_info(whatsmeow_chat_settings)")}
            if "our_jid" not in cols:
                log("chat settings: table has no our_jid column; ignoring")
                return {}
            for our_jid, chat_jid, pinned, archived in con.execute(
                "SELECT our_jid, chat_jid, pinned, archived FROM whatsmeow_chat_settings"
            ):
                if not chat_jid or jid_core_user(our_jid) != own_user:
                    continue
                jid = canonical_chat_jid(str(chat_jid), lid_map)
                flags = {
                    "pinned": bool(pinned),
                    "archived": bool(archived),
                }
                out[jid] = flags
                out[str(chat_jid)] = flags
                out[jid_user_part(jid)] = flags
        finally:
            con.close()
        if out:
            log(f"chat settings: {sum(1 for v in out.values() if v.get('pinned'))} pinned")
        return out
    except Exception as e:
        log(f"chat settings: {e}")
        return {}


def normalize_jid(jid: str) -> str:
    """Drop `:device` / `.agent` from a user JID and turn bare digits into a phone JID."""
    raw = str(jid or "").strip()
    if not raw:
        return raw
    if "@" not in raw:
        digits = phone_digits(raw)
        return f"{digits}@s.whatsapp.net" if digits else raw
    local, domain = raw.split("@", 1)
    if domain in {"s.whatsapp.net", "lid", "c.us"}:
        local = local.split(":", 1)[0]
        if domain != "lid":
            local = local.split(".", 1)[0]
        if domain == "c.us":
            domain = "s.whatsapp.net"
        return f"{local}@{domain}"
    return raw


def canonical_chat_jid(jid: str, lid_map: Optional[dict[str, str]] = None) -> str:
    raw = normalize_jid(jid)
    if not raw:
        return raw
    if raw.endswith("@lid"):
        mapped = (lid_map or {}).get(raw) or (lid_map or {}).get(jid_user_part(raw))
        return mapped or raw
    return raw


_HISTORY_NAME_RE = re.compile(r"^history-(\d+)-(.+)-(\d+)-([A-Za-z0-9_]+)\.json$")


def history_file_jid(name: str) -> Optional[str]:
    """Device JID embedded in a GOWA history dump file name, or None."""
    m = _HISTORY_NAME_RE.match(str(name))
    return m.group(2) if m else None


def history_files(own_jid: Optional[str], store: Optional[Path] = None) -> list[Path]:
    """History dumps belonging to this account only (matched on JID user part)."""
    if not own_jid:
        return []
    own_user = jid_core_user(own_jid)
    if not own_user:
        return []
    store = store or (gowa_home() / "storages")
    if not store.is_dir():
        return []
    out: list[Path] = []
    for p in store.glob("history-*.json"):
        owner = history_file_jid(p.name)
        if owner and jid_core_user(owner) == own_user:
            out.append(p)

    def mtime(p: Path) -> float:
        try:
            return p.stat().st_mtime
        except OSError:
            return 0.0

    return sorted(out, key=mtime, reverse=True)


def parse_history_conversations(paths: list[Path]) -> tuple[list[dict[str, Any]], list[Path]]:
    """Merge the conversations of the given dumps. Returns (conversations, readable paths)."""
    merged: dict[str, dict[str, Any]] = {}
    readable: list[Path] = []
    for path in paths:
        try:
            payload = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except Exception as e:
            log(f"history dump {path.name}: {type(e).__name__}")
            continue
        readable.append(path)
        rows = payload.get("conversations") if isinstance(payload, dict) else None
        if not isinstance(rows, list):
            continue
        for conv in rows:
            if not isinstance(conv, dict):
                continue
            cid = str(
                conv.get("pnJID")
                or conv.get("pnJid")
                or conv.get("ID")
                or conv.get("id")
                or ""
            )
            if not cid or cid.endswith("@broadcast") or cid.startswith("status@"):
                continue
            prev = merged.get(cid)
            if prev is None:
                merged[cid] = conv
                continue
            # Unchanged from before: the earlier dump's messages are only kept when it had some.
            prev_msgs: list[Any] = _as_list(conv.get("messages")) if isinstance(prev.get("messages"), list) else []
            new_msgs: list[Any] = _as_list(conv.get("messages"))
            by_id: dict[str, Any] = {}
            for wrap in [*prev_msgs, *new_msgs]:
                if not isinstance(wrap, dict):
                    continue
                msg = wrap.get("message") if isinstance(wrap.get("message"), dict) else wrap
                key = (msg.get("key") or {}) if isinstance(msg, dict) else {}
                mid = str(key.get("ID") or key.get("id") or wrap.get("msgOrderID") or len(by_id))
                by_id[mid] = wrap
            prev["messages"] = list(by_id.values())
            for field in ("name", "pnJID", "conversationTimestamp"):
                if conv.get(field) not in (None, "", 0) and not prev.get(field):
                    prev[field] = conv.get(field)
    return list(merged.values()), readable


def history_message_fields(wrap: dict[str, Any]) -> Optional[dict[str, Any]]:
    msg = wrap.get("message") if isinstance(wrap.get("message"), dict) else wrap
    if not isinstance(msg, dict):
        return None
    key = _as_dict(msg.get("key"))
    inner = _as_dict(msg.get("message"))
    inner_keys = inner.keys() if isinstance(inner, dict) else ()
    text = nested_text(inner) or nested_text(msg)
    stubs = msg.get("messageStubParameters") or []
    stub_name = ""
    if isinstance(stubs, list):
        for item in stubs:
            if isinstance(item, str) and any(ch.isalpha() for ch in item):
                stub_name = item
                break
    kind = next(
        (
            label
            for part, label in (
                ("imageMessage", "image"),
                ("videoMessage", "video"),
                ("audioMessage", "audio"),
                ("documentMessage", "document"),
                ("stickerMessage", "sticker"),
            )
            if part in inner_keys
        ),
        "",
    )
    if not text.strip() and not stub_name:
        if any(k in inner_keys for k in ("locationMessage", "liveLocationMessage")):
            text = "[location]"
        else:
            # Captionless media keeps a placeholder body (its own mime type or the sender's
            # name used to stand in for it).
            if not kind:
                return None
            text = f"[{kind}]"
    if text.strip() and looks_like_jid(text.strip()) and not stub_name:
        return None
    ts = msg.get("messageTimestamp") or wrap.get("messageTimestamp")
    return {
        "id": key.get("ID") or key.get("id") or wrap.get("msgOrderID"),
        "from_me": bool(key.get("fromMe") or key.get("from_me")),
        "text": (text.strip() or f"[{stub_name}]") if (text.strip() or stub_name) else "",
        # The media a dump carries, so its media message shows as media. `location` stays a
        # text placeholder: the core has no media type for it.
        "media_type": kind or None,
        "timestamp": ts,
        "stub_name": stub_name,
        "sender_jid": history_sender_jid(msg, wrap),
        "mentioned_jids": mentioned_jids_in(inner),
        "reply_to": reply_to_from_dump(inner),
    }


def history_sender_jid(msg: dict[str, Any], wrap: Optional[dict[str, Any]] = None) -> Optional[str]:
    """Who sent a dump message.

    A group message carries its author in the message-level `participant` (almost always a
    LID); `key.participant` is only set on some rows. The chat JID is a sender only in a
    1:1 chat; in a group it is the group itself and must never be reported as a person.
    """
    key = _as_dict(msg.get("key"))
    chat = str(key.get("remoteJID") or key.get("remoteJid") or "")
    who = (
        key.get("participant")
        or msg.get("participant")
        or ((wrap or {}).get("participant") if isinstance(wrap, dict) else None)
    )
    if who:
        return str(who)
    if chat and not chat.endswith(("@g.us", "@broadcast", "@newsletter")):
        return chat
    return None


def history_message_reactions(wrap: dict[str, Any]) -> tuple[Optional[str], list[dict[str, Any]]]:
    """Reactions WhatsApp attached to a dump message: (target message id, reaction facts).

    Each fact has `jid` (the reactor as written in the dump, or None when `from_me`),
    `from_me`, `emoji` and `timestamp`. In a group the reactor is `key.participant`; in a
    1:1 chat it is the chat JID.
    """
    msg = wrap.get("message") if isinstance(wrap.get("message"), dict) else wrap
    if not isinstance(msg, dict):
        return None, []
    key = _as_dict(msg.get("key"))
    target = str(key.get("ID") or key.get("id") or "").strip()
    rows = msg.get("reactions") or wrap.get("reactions")
    if not target or not isinstance(rows, list):
        return None, []
    out: list[dict[str, Any]] = []
    for r in rows:
        if not isinstance(r, dict):
            continue
        rkey = _as_dict(r.get("key"))
        emoji = r.get("text")
        if not isinstance(emoji, str) or not emoji.strip():
            continue
        from_me = bool(rkey.get("fromMe") or rkey.get("from_me"))
        jid: Optional[str] = None
        if not from_me:
            jid = rkey.get("participant") or rkey.get("remoteJID") or rkey.get("remoteJid")
            if not jid:
                continue
            jid = str(jid)
            if jid.endswith(("@g.us", "@broadcast")):
                continue
        out.append(
            {
                "jid": jid,
                "from_me": from_me,
                "emoji": emoji.strip(),
                "timestamp": r.get("senderTimestampMS")
                or r.get("senderTimestampMs")
                or r.get("timestamp"),
            }
        )
    return target, out


def load_self_profile(
    client: GowaClient,
    contacts: Optional[dict[str, str]] = None,
) -> tuple[Optional[str], Optional[str], set[str]]:
    """Return (jid, display_name, aliases).

    GOWA often stamps the connected *business* display name onto other chats'
    `name` / `verified_name` fields. Aliases let us reject those as titles.
    """
    code, body = client.get("/app/status")
    jid: Optional[str] = None
    if code == 200:
        r = results(body)
        jid = r.get("jid") or r.get("device_id")
        if isinstance(jid, str) and "@" not in jid and jid.isdigit():
            jid = f"{jid}@s.whatsapp.net"
    if not jid:
        code, body = client.get(f"/devices/{client.device_id}/status")
        if code == 200:
            r = results(body)
            jid = r.get("jid") or r.get("device_id")
            if isinstance(jid, str) and "@" not in jid and jid.isdigit():
                jid = f"{jid}@s.whatsapp.net"
    if isinstance(jid, str) and "@" in jid:
        jid = normalize_jid(jid)

    aliases: set[str] = set()
    name: Optional[str] = None

    code, body = client.get("/devices")
    if code == 200:
        rows: list[Any] = []
        if isinstance(body, dict):
            maybe = body.get("results")
            if isinstance(maybe, list):
                rows = maybe
            elif isinstance(maybe, dict):
                data = maybe.get("data")
                if isinstance(data, list):
                    rows = data
        for row in rows:
            if not isinstance(row, dict):
                continue
            if str(row.get("id") or "") != str(client.device_id):
                continue
            display = row.get("display_name") or row.get("name")
            if isinstance(display, str) and display.strip():
                aliases.add(display.strip())
                name = name or display.strip()
            if not jid:
                dev_jid = row.get("jid") or row.get("device")
                if isinstance(dev_jid, str) and "@" in dev_jid:
                    jid = normalize_jid(dev_jid)
            break

    if jid:
        code, body = client.get("/user/info", query={"phone": jid})
        if code == 200:
            info = user_info_record(body)
            for key in ("verified_name", "push_name", "business_name", "name"):
                val = info.get(key)
                if isinstance(val, str) and val.strip() and not is_placeholder_name(val.strip(), jid):
                    aliases.add(val.strip())
                    if key in ("verified_name", "business_name", "push_name") and not name:
                        name = val.strip()
        code, body = client.get("/user/business-profile", query={"phone": jid})
        if code == 200:
            biz = results(body)
            for key in ("business_name", "name", "verified_name"):
                val = biz.get(key)
                if isinstance(val, str) and val.strip():
                    aliases.add(val.strip())
                    name = name or val.strip()

        book = contacts if contacts is not None else {}
        saved = lookup_contact_name(book, jid)
        if saved and not is_placeholder_name(saved, jid):
            if saved not in aliases:
                name = name or saved
            elif not name:
                name = saved

    aliases = {a for a in aliases if a and not is_placeholder_name(a, jid or "")}
    if name and is_placeholder_name(name, jid or ""):
        name = next(iter(aliases), None)
    return jid, name, aliases


def load_saved_contact_jids(
    own_jid: Optional[str], db_path: Optional[Path] = None
) -> Optional[set[str]]:
    """JIDs (and bare user parts) this account has saved in its address book.

    GOWA's contact list also holds everyone whose push name was seen in a group. A saved entry
    has a `full_name` / `first_name`; a group-only one has just a `push_name`. The table is shared
    by every account in the GOWA database, so only rows whose `our_jid` matches this account's own
    JID (user part) count. None means unknown (no own JID, no database, or a read error).
    """
    if not own_jid:
        return None
    own_user = jid_core_user(own_jid)
    path = db_path or whatsapp_db_path()
    if not own_user or not path.is_file():
        return None
    try:
        import sqlite3

        con = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        out: set[str] = set()
        try:
            for our_jid, their_jid, full_name, first_name in con.execute(
                "SELECT our_jid, their_jid, full_name, first_name FROM whatsmeow_contacts"
            ):
                if not their_jid or jid_core_user(our_jid) != own_user:
                    continue
                if not (str(full_name or "").strip() or str(first_name or "").strip()):
                    continue
                out.add(str(their_jid))
                out.add(jid_user_part(str(their_jid)))
        finally:
            con.close()
        return out
    except Exception as e:
        log(f"saved contacts: {e}")
        return None


def normalize_phone_number(raw: Any) -> Optional[str]:
    """Digits of a typed phone number (no `+`, spaces, dashes or `00` prefix), or None when the
    result is not a plausible international number (E.164: 7 to 15 digits)."""
    digits = phone_digits(str(raw or ""))
    if digits.startswith("00"):
        digits = digits[2:]
    return digits if 7 <= len(digits) <= 15 else None


def emit_contacts_synced(
    account_id: str, contacts: dict[str, str], saved: Optional[set[str]] = None
) -> None:
    items: list[dict[str, Any]] = []
    seen: set[str] = set()
    for jid, name in contacts.items():
        if "@" not in str(jid) or jid in seen:
            continue
        seen.add(str(jid))
        item: dict[str, Any] = {"remote_id": str(jid), "display_name": str(name)}
        if saved is not None:
            item["saved"] = str(jid) in saved or jid_user_part(str(jid)) in saved
        items.append(item)
    send(
        {
            "type": "event",
            "event": "contacts.synced",
            "account_id": account_id,
            "payload": {"contacts": items},
        }
    )


NAME_KEYS = ("contact", "subject", "verified", "push")


def valid_chat_name(
    name: Any,
    jid: str,
    own_names: set[str],
    *,
    allow_own: bool = False,
) -> Optional[str]:
    """A real name for the chat, or None for placeholders, JIDs, numbers, masked numbers
    and (unless allowed) the connected account's own names."""
    if not isinstance(name, str):
        return None
    text = name.strip()
    if not text or is_placeholder_name(text, jid):
        return None
    if text in own_names and not allow_own:
        return None
    return text


def choose_title(
    jid: str,
    names: dict[str, str],
    *,
    is_self: bool = False,
    self_name: Optional[str] = None,
    is_group: bool = False,
) -> str:
    """Legacy single title; mirrors the core's title rules (contract R3)."""
    if is_self:
        contact = names.get("contact")
        if contact:
            return f"{contact} (You)"
        return "Message yourself"
    if is_group:
        return names.get("subject") or "Group"
    for key in ("contact", "verified", "push"):
        if names.get(key):
            return names[key]
    if jid.endswith("@s.whatsapp.net"):
        phone = format_intl_phone(jid)
        if phone:
            return phone
    return jid_user_part(jid) or jid or "Chat"


def resolve_sender_name(
    row: dict[str, Any],
    data: dict[str, Any],
    chat_jid: str,
    chat_title: str,
    contacts: dict[str, str],
    self_aliases: Optional[set[str]] = None,
) -> str:
    if (
        row.get("is_from_me")
        or row.get("from_me")
        or data.get("from_me")
        or data.get("is_from_me")
    ):
        return "You"
    aliases = {a.strip() for a in (self_aliases or set()) if isinstance(a, str) and a.strip()}
    sender = sender_only(
        row.get("sender_jid") or data.get("sender_jid") or data.get("from") or row.get("from")
    )
    saved = lookup_contact_name(contacts, sender or None) or (
        lookup_contact_name(contacts, chat_jid) if not chat_jid.endswith("@g.us") else None
    )
    if saved and saved in aliases:
        saved = None
    for key in (
        "from_name",
        "sender_display_name",
        "pushname",
        "push_name",
        "notify_name",
        "verified_name",
    ):
        val = row.get(key) or data.get(key)
        if not val:
            continue
        text = str(val).strip()
        if text in aliases:
            continue
        if not is_placeholder_name(text, str(sender or chat_jid)):
            return text
    if saved:
        return saved
    if chat_jid.endswith("@g.us"):
        return jid_user_part(str(sender)) if sender else "Unknown"
    if chat_title and not is_placeholder_name(chat_title, chat_jid) and chat_title not in aliases:
        return chat_title
    if sender:
        return jid_user_part(str(sender))
    return chat_title or "Unknown"


def _str_set(raw: dict[str, Any], key: str) -> set[str]:
    """A stored list of names as a set, empty when the file has nothing usable."""
    value = raw.get(key)
    if not isinstance(value, list):
        return set()
    return {str(x) for x in value if x}


class SyncState:
    """Small per-account sync memory: one-time backfill, per-chat cursors, imported dumps."""

    def __init__(self, path: Optional[Path]):
        self.path = path
        self._lock = threading.RLock()
        self.backfilled: set[str] = set()
        self.cursors: dict[str, str] = {}
        self.imported_files: set[str] = set()
        # Dumps already re-read for reactions and group sender repair (one pass per file).
        self.reaction_files: set[str] = set()
        # Dumps already read for the statuses they carry (`statusV3Messages`).
        self.status_files: set[str] = set()
        # Dumps whose embedded JPEG thumbnails were already written to the media cache, and
        # which folder those previews went to (a moved cache invalidates the set).
        self.thumb_files: set[str] = set()
        self.thumbs_layout = 0
        self.thumbs_root = ""
        self.thumbs_count = 0
        # Dumps already re-read for reply links, and chats whose REST rows were already
        # checked for them (one pass each; both only matter for messages stored before replies
        # were reported).
        self.reply_files: set[str] = set()
        self.reply_chats: set[str] = set()
        self._dirty = False
        self._load()

    def _load(self) -> None:
        if not self.path:
            return
        try:
            raw = json.loads(self.path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            return
        except Exception as e:
            log(f"sync state unreadable ({e}); starting fresh")
            return
        if not isinstance(raw, dict):
            return
        self.backfilled = _str_set(raw, "backfilled")
        self.imported_files = _str_set(raw, "imported_files")
        self.reaction_files = _str_set(raw, "reaction_files")
        self.status_files = _str_set(raw, "status_files")
        self.thumb_files = _str_set(raw, "thumb_files")
        self.reply_files = _str_set(raw, "reply_files")
        self.reply_chats = _str_set(raw, "reply_chats")
        c = raw.get("cursors")
        if isinstance(c, dict):
            for k, v in c.items():
                iso = to_rfc3339_strict(v)
                if iso:
                    self.cursors[str(k)] = iso
        layout = raw.get("thumbs_layout")
        if isinstance(layout, int):
            self.thumbs_layout = layout
        root = raw.get("thumbs_root")
        if isinstance(root, str):
            self.thumbs_root = root
        count = raw.get("thumbs_count")
        if isinstance(count, int) and count > 0:
            self.thumbs_count = count

    def save(self) -> None:
        if not self.path:
            return
        with self._lock:
            if not self._dirty:
                return
            data = {
                "version": 1,
                "backfilled": sorted(self.backfilled),
                "cursors": dict(self.cursors),
                "imported_files": sorted(self.imported_files),
                "reaction_files": sorted(self.reaction_files),
                "status_files": sorted(self.status_files),
                "thumb_files": sorted(self.thumb_files),
                "thumbs_layout": self.thumbs_layout,
                "thumbs_root": self.thumbs_root,
                "thumbs_count": self.thumbs_count,
                "reply_files": sorted(self.reply_files),
                "reply_chats": sorted(self.reply_chats),
            }
            self._dirty = False
        tmp = self.path.with_name(self.path.name + ".tmp")
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            tmp.write_text(json.dumps(data), encoding="utf-8")
            os.replace(tmp, self.path)
        except OSError as e:
            log(f"sync state save: {e}")
            with self._lock:
                self._dirty = True

    def cursor_for(self, keys: list[str]) -> Optional[str]:
        best: Optional[str] = None
        best_ts = 0.0
        with self._lock:
            for k in keys:
                v = self.cursors.get(k)
                dt = parse_timestamp_strict(v) if v else None
                if dt and (best is None or dt.timestamp() > best_ts):
                    best, best_ts = v, dt.timestamp()
        return best

    def advance_cursor(self, key: str, iso: str) -> None:
        dt = parse_timestamp_strict(iso)
        if not dt:
            return
        with self._lock:
            cur = parse_timestamp_strict(self.cursors.get(key)) if self.cursors.get(key) else None
            if cur is None or dt > cur:
                self.cursors[key] = dt.isoformat()
                self._dirty = True

    def mark_backfilled(self, keys: list[str]) -> None:
        with self._lock:
            before = len(self.backfilled)
            self.backfilled.update(keys)
            if len(self.backfilled) != before:
                self._dirty = True

    def is_backfilled(self, keys: list[str]) -> bool:
        with self._lock:
            return any(k in self.backfilled for k in keys)

    def mark_imported(self, names: list[str]) -> None:
        with self._lock:
            before = len(self.imported_files)
            self.imported_files.update(names)
            if len(self.imported_files) != before:
                self._dirty = True

    def mark_reactions_imported(self, names: list[str]) -> None:
        with self._lock:
            before = len(self.reaction_files)
            self.reaction_files.update(names)
            if len(self.reaction_files) != before:
                self._dirty = True

    def mark_status_files(self, names: list[str]) -> None:
        with self._lock:
            before = len(self.status_files)
            self.status_files.update(names)
            if len(self.status_files) != before:
                self._dirty = True

    def mark_thumbs_done(self, names: list[str], written: int = 0) -> None:
        with self._lock:
            before = len(self.thumb_files)
            self.thumb_files.update(names)
            if written > 0:
                self.thumbs_count += written
            if len(self.thumb_files) != before or written > 0:
                self._dirty = True

    def bind_thumbs(self, root: Path) -> bool:
        """Drop the done set when its previews are not under `root` any more.

        Returns True when it was dropped, so the caller re-extracts the previews from the
        dumps. An older build wrote them to the Documents folder and still marked the dumps
        done, which left every chat without a preview and no reason to retry.
        """
        with self._lock:
            current = str(root)
            stale = self.thumbs_layout != THUMBS_LAYOUT or self.thumbs_root != current
            if not stale and self.thumbs_count > 0:
                try:
                    present = sum(1 for _ in root.glob("*.jpg"))
                except OSError:
                    present = 0
                stale = present < self.thumbs_count
            self.thumbs_layout = THUMBS_LAYOUT
            self.thumbs_root = current
            if not stale or not self.thumb_files:
                return False
            self.thumb_files.clear()
            self.thumbs_count = 0
            self._dirty = True
            return True

    def mark_replies_imported(self, names: list[str]) -> None:
        with self._lock:
            before = len(self.reply_files)
            self.reply_files.update(names)
            if len(self.reply_files) != before:
                self._dirty = True

    def mark_replies_checked(self, keys: list[str]) -> None:
        with self._lock:
            before = len(self.reply_chats)
            self.reply_chats.update(keys)
            if len(self.reply_chats) != before:
                self._dirty = True

    def replies_checked(self, keys: list[str]) -> bool:
        with self._lock:
            return any(k in self.reply_chats for k in keys)


def sync_state_path(account_id: str) -> Path:
    return data_dir() / "connectors" / CONNECTOR_ID / safe_dir_name(account_id) / "sync-state.json"


def _epoch(iso: Optional[str]) -> float:
    dt = parse_timestamp_strict(iso) if iso else None
    return dt.timestamp() if dt else 0.0


def _newer(a: Optional[str], b: Optional[str]) -> bool:
    """True when `a` is a valid time strictly later than `b` (a missing `b` counts as oldest)."""
    ea = _epoch(a)
    return ea > 0 and ea > _epoch(b)


class WhatsAppSession:
    CATCHUP_PAGE = 50
    # Every Nth catch-up cycle re-checks listed chats even when GOWA's chat time did not
    # move, so a missed message that shares its second with the cursor is still recovered.
    SWEEP_EVERY = 38
    FEED_REFRESH_SECONDS = 30.0
    LID_RELOAD_SECONDS = 30.0
    METADATA_REFRESH_SECONDS = 60.0
    GROUP_REFRESH_SECONDS = 300.0

    def __init__(self, account_id: str, state: Optional[SyncState] = None):
        self.account_id = account_id
        self.client: Optional[GowaClient] = None
        self.stop = threading.Event()
        self.gowa_proc: Optional[subprocess.Popen] = None
        self.ws_thread: Optional[threading.Thread] = None
        self.poll_thread: Optional[threading.Thread] = None
        self._last_qr_at = 0.0
        self._connected = False
        self._listeners_started = False
        self._syncing = False
        self._history_done = False
        self._connect_lock = threading.Lock()
        self._sync_lock = threading.Lock()
        self._catchup_lock = threading.Lock()
        self._listener_lock = threading.Lock()
        self._emit_lock = threading.RLock()
        self._names_lock = threading.Lock()
        self._lid_lock = threading.Lock()
        self._backfill_lock = threading.Lock()
        self._contacts: dict[str, str] = {}
        self._contacts_loaded = False
        self._contacts_signature: Optional[str] = None
        self._saved_jids: Optional[set[str]] = None
        self._groups_checked_at = 0.0
        self._metadata_checked_at = 0.0
        self._metadata_lock = threading.Lock()
        self._chat_settings: dict[str, dict[str, bool]] = {}
        # group jid -> (announce only, this account is an admin), from GOWA's group info.
        self._group_send: dict[str, tuple[bool, bool]] = {}
        # Group JID -> members, as [{"user": mention digits, "ids": all digits of the person}].
        self._group_members: dict[str, list[dict[str, Any]]] = {}
        # Chat JID -> real subject, for groups (`/user/my/groups`) and channels alike.
        self._subjects: dict[str, str] = {}
        self._newsletter_ok = True
        self._self_jid: Optional[str] = None
        self._self_name: Optional[str] = None
        self._self_aliases: set[str] = set()
        self._lid_map: dict[str, str] = {}
        self._pn_to_lid: dict[str, str] = {}
        self._lid_loaded_at = 0.0
        self._names: dict[str, dict[str, str]] = {}
        self._chat_cache: dict[str, dict[str, Any]] = {}
        self._lid_chats: set[str] = set()
        self._listed: set[str] = set()
        self._feed_sig: Optional[str] = None
        self._feed_checked_at = 0.0
        self._catchup_cycles = 0
        # Per chat: (epoch of newest emitted message, ids emitted at that exact time).
        self._tail: dict[str, tuple[float, set[str]]] = {}
        self._tail_lock = threading.Lock()
        self._backfill_thread: Optional[threading.Thread] = None
        self._reply_thread: Optional[threading.Thread] = None
        self._state = state
        self._state_made = state is not None

    # ------------------------------------------------------------------ state

    @property
    def state(self) -> SyncState:
        if self._state is None:
            self._state = SyncState(sync_state_path(self.account_id))
        return self._state

    def _emit(self, event: str, payload: dict[str, Any]) -> None:
        send(
            {
                "type": "event",
                "event": event,
                "account_id": self.account_id,
                "payload": payload,
            }
        )

    # -------------------------------------------------------------- connection

    def connect(self) -> None:
        webhook = ensure_webhook_server()
        url, auth, proc = load_or_start_gowa(webhook)
        self.gowa_proc = proc
        self.client = GowaClient(url, auth, self.account_id)
        _sessions_by_account[self.account_id] = self
        self.ensure_device()
        if webhook:
            self._register_gowa_webhook(webhook)
        self._start_listeners()
        status = self._wait_until_logged_in(timeout=30.0)
        if self._status_logged_in(status):
            self._on_connected(status)
            return
        self.start_qr_login(start_listeners=False)

    def _wait_until_logged_in(self, timeout: float = 30.0) -> dict[str, Any]:
        deadline = time.time() + timeout
        last: dict[str, Any] = {}
        while time.time() < deadline and not self.stop.is_set():
            last = self.device_status()
            if self._status_logged_in(last):
                return last
            assert self.client
            code, body = self.client.get(f"/devices/{self.account_id}/login")
            if self._login_pending(code, body):
                time.sleep(1.0)
                continue
            time.sleep(0.45)
        return last

    def _status_logged_in(self, status: dict[str, Any]) -> bool:
        if isinstance(status.get("is_logged_in"), bool):
            return status["is_logged_in"]
        if status.get("is_connected") is True:
            return True
        return str(status.get("state") or "").lower() in {"logged_in", "connected"}

    def _login_already_active(self, code: int, body: Any) -> bool:
        r = results(body) if isinstance(body, dict) else {}
        if self._status_logged_in(r):
            return True
        code_s = str(body.get("code") if isinstance(body, dict) else "").upper()
        msg = str(body.get("message") if isinstance(body, dict) else body).lower()
        if code_s in {"ALREADY_LOGGED_IN", "SESSION_SAVED_ERROR"}:
            return True
        if code in (400, 409, 500) and (
            "already logged" in msg or "session" in msg and "wait" in msg
        ):
            return True
        return False

    def _login_pending(self, code: int, body: Any) -> bool:
        if self._login_already_active(code, body):
            return True
        code_s = str(body.get("code") if isinstance(body, dict) else "").upper()
        return code in (400, 409, 500) and code_s in {
            "ALREADY_LOGGED_IN",
            "SESSION_SAVED_ERROR",
        }

    def ensure_device(self) -> None:
        assert self.client
        code, body = self.client.post("/devices", json_body={"device_id": self.account_id})
        if code in (200, 201, 409):
            return
        msg = body.get("message") if isinstance(body, dict) else str(body)
        if msg and "exist" in str(msg).lower():
            return
        log(f"POST /devices returned {code}: {body}")

    def device_status(self) -> dict[str, Any]:
        assert self.client
        for path in (f"/devices/{self.account_id}/status", "/app/status"):
            code, body = self.client.get(path)
            if code == 200:
                r = results(body)
                if r:
                    return r
        return {}

    def start_qr_login(self, start_listeners: bool = False) -> None:
        if self._connected:
            return
        assert self.client
        code, body = self.client.get(f"/devices/{self.account_id}/login")
        if code != 200:
            code, body = self.client.get("/app/login")
        if self._login_already_active(code, body):
            self._on_connected(self.device_status() or results(body))
            return
        if code != 200:
            log(f"GOWA login not ready ({code}): {body}")
            return
        r = results(body)
        qr_link = r.get("qr_link")
        qr_data = None
        if qr_link:
            try:
                raw = self.client.download(str(qr_link))
                if raw.startswith(b"\x89PNG"):
                    mime = "image/png"
                elif raw.startswith(b"\xff\xd8"):
                    mime = "image/jpeg"
                else:
                    mime = "image/png"
                qr_data = f"data:{mime};base64,{base64.b64encode(raw).decode()}"
            except Exception as e:
                log(f"QR download failed: {e}")
                qr_data = None
        self._last_qr_at = time.time()
        send(
            {
                "type": "auth_required",
                "method": "qr",
                "qr_data": qr_data,
                "url": qr_link,
                "account_id": self.account_id,
            }
        )
        send(
            {
                "type": "status",
                "account_id": self.account_id,
                "status": "awaiting_auth",
                "identity": None,
            }
        )
        if start_listeners:
            self._start_listeners()

    def _start_listeners(self) -> None:
        with self._listener_lock:
            if self._listeners_started:
                return
            self._listeners_started = True
        self.ws_thread = threading.Thread(target=self._ws_loop, daemon=True)
        self.poll_thread = threading.Thread(target=self._poll_loop, daemon=True)
        self.ws_thread.start()
        self.poll_thread.start()

    def _ws_loop(self) -> None:
        assert self.client
        parsed = urllib.parse.urlparse(self.client.base_url)
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or 80
        token = base64.b64encode(f"{self.client.auth[0]}:{self.client.auth[1]}".encode()).decode()
        path = f"/ws?device_id={urllib.parse.quote(self.account_id)}&authorization={token}"
        seen_open = False
        while not self.stop.is_set():
            ws = MiniWebSocket(host, port, path, self.client.auth)
            try:
                ws.connect()
                log("websocket connected")
                if seen_open and self._connected:
                    try:
                        self.catch_up_recent()
                    except Exception as e:
                        log(f"catch-up: {e}")
                seen_open = True
                while not self.stop.is_set():
                    text = ws.recv_text()
                    if text is None:
                        break
                    try:
                        payload = json.loads(text)
                    except json.JSONDecodeError:
                        continue
                    try:
                        self._handle_ws(payload)
                    except Exception as e:
                        log(f"ws event: {e}")
            except Exception as e:
                log(f"websocket: {e}")
                time.sleep(2)
            finally:
                ws.close()

    def _poll_loop(self) -> None:
        last_logged_in = False
        last_catchup = 0.0
        while not self.stop.is_set():
            try:
                status = self.device_status()
                logged_in = self._status_logged_in(status)
                if logged_in and not last_logged_in:
                    self._on_connected(status)
                    last_catchup = time.time()
                elif last_logged_in and not logged_in:
                    with self._connect_lock:
                        self._connected = False
                    send(
                        {
                            "type": "status",
                            "account_id": self.account_id,
                            "status": "awaiting_auth",
                            "identity": None,
                        }
                    )
                last_logged_in = logged_in
                if not logged_in and not self._connected:
                    self._refresh_qr_if_needed()
                elif logged_in and self._connected and time.time() - last_catchup >= 8:
                    last_catchup = time.time()
                    self.catch_up_recent()
            except Exception as e:
                log(f"poll: {e}")
            # Websocket/webhook is primary; catch-up recovers missed events.
            self.stop.wait(2 if not last_logged_in else 8)

    def _refresh_qr_if_needed(self) -> None:
        if time.time() - self._last_qr_at < 25:
            return
        if self.stop.is_set() or self._connected:
            return
        try:
            self.start_qr_login(start_listeners=False)
        except Exception as e:
            log(f"qr refresh: {e}")

    def _on_connected(self, status: dict[str, Any]) -> None:
        with self._connect_lock:
            if self._connected:
                return
            self._connected = True
        if self.client:
            try:
                self._reload_lid_map(force=True)
                self._self_jid, self._self_name, self._self_aliases = load_self_profile(
                    self.client
                )
            except Exception as e:
                log(f"self profile: {e}")
        identity = (
            status.get("jid")
            or self._self_jid
            or status.get("phone_number")
            or status.get("id")
        )
        send(
            {
                "type": "status",
                "account_id": self.account_id,
                "status": "connected",
                "identity": identity,
            }
        )
        self._emit("account.connected", {"identity": identity})
        self._start_listeners()
        self.start_sync(force=True)

    # ------------------------------------------------------------ LID mapping

    def _set_lid_map(self, new: dict[str, str]) -> None:
        with self._lid_lock:
            self._lid_map.update(new)
            self._pn_to_lid = {
                pn: lid for lid, pn in self._lid_map.items() if "@" in lid and "@" in pn
            }

    def _reload_lid_map(self, force: bool = False) -> bool:
        """Re-read whatsmeow's LID map (rate limited) and promote chats that became known."""
        now = time.time()
        with self._lid_lock:
            if not force and now - self._lid_loaded_at < self.LID_RELOAD_SECONDS:
                return False
            self._lid_loaded_at = now
        new = load_lid_map()
        if not new:
            return False
        self._set_lid_map(new)
        self._promote_lid_chats()
        return True

    def _learn_lid(self, lid: str, phone: str) -> None:
        lid_n = normalize_jid(lid)
        pn_n = normalize_jid(phone)
        if not lid_n.endswith("@lid") or not pn_n.endswith("@s.whatsapp.net"):
            return
        if self._lid_map.get(lid_n) == pn_n:
            return
        self._set_lid_map({lid_n: pn_n, jid_user_part(lid_n): pn_n})
        self._promote_lid_chats()

    def _promote_lid_chats(self) -> None:
        """A chat emitted under its @lid JID now has a phone JID: tell the core to merge."""
        with self._emit_lock:
            for lid in list(self._lid_chats):
                mapped = canonical_chat_jid(lid, self._lid_map)
                if mapped == lid:
                    continue
                self._lid_chats.discard(lid)
                cached = self._chat_cache.get(lid)
                if not cached:
                    continue
                ref = self._build_ref(mapped, [lid])
                names = self._cached_names(ref)
                self._emit_conversation(
                    ref,
                    names,
                    last_message_at=cached.get("last_message_at"),
                    archived=cached.get("archived"),
                    pinned=cached.get("pinned"),
                    preview=(cached.get("payload") or {}).get("preview"),
                    history=False,
                )

    # ------------------------------------------------------------ chat identity

    def _build_ref(self, remote: str, alts: list[str], raw: Optional[str] = None) -> dict[str, Any]:
        alts = [a for a in dict.fromkeys(alts) if a and a != remote]
        fetch: list[str] = []
        for j in [raw, remote, *alts]:
            if j and j not in fetch:
                fetch.append(j)
        return {"remote_id": remote, "alts": alts, "all": [remote, *alts], "fetch": fetch}

    def _chat_ref(self, raw_jid: Any, lid_hint: Any = None) -> Optional[dict[str, Any]]:
        raw_in = str(raw_jid or "").strip()
        raw = normalize_jid(raw_in)
        if not raw or is_status_jid(raw):
            return None
        hint = normalize_jid(str(lid_hint)) if lid_hint else ""
        if hint and not hint.endswith("@lid"):
            hint = ""
        alts: list[str] = []
        if raw.endswith("@lid"):
            remote = canonical_chat_jid(raw, self._lid_map)
            if remote == raw:
                if self._reload_lid_map():
                    remote = canonical_chat_jid(raw, self._lid_map)
            if remote != raw:
                alts.append(raw)
        else:
            remote = raw
            if raw.endswith("@s.whatsapp.net"):
                if hint:
                    self._learn_lid(hint, raw)
                    alts.append(hint)
                known = self._pn_to_lid.get(raw)
                if known:
                    alts.append(known)
        return self._build_ref(remote, alts, raw=raw)

    @staticmethod
    def _ctype(remote: str) -> str:
        if remote.endswith("@g.us"):
            return "group"
        if remote.endswith("@newsletter"):
            return "channel"
        return "direct"

    def _group_send_facts(self, group: dict[str, Any]) -> Optional[tuple[bool, bool]]:
        """(announce only, this account is an admin) from a GOWA GroupInfo, or None when unknown.

        Modern groups list members by LID, so the owner is matched on the participant's phone
        JID, LID and PhoneNumber alike. A roster this account is missing from says nothing.
        """
        announce = group.get("IsAnnounce")
        if not isinstance(announce, bool):
            return None
        if not announce:
            return (False, False)
        participants = group.get("Participants")
        me = jid_core_user(self._self_jid)
        if not me or not isinstance(participants, list) or not participants:
            return None
        for p in participants:
            if not isinstance(p, dict) or not (p.get("IsAdmin") or p.get("IsSuperAdmin")):
                continue
            if any(jid_core_user(p.get(key)) == me for key in ("JID", "LID", "PhoneNumber")):
                return (True, True)
        return (True, False)

    @staticmethod
    def _group_member_list(group: dict[str, Any]) -> list[dict[str, Any]]:
        """The people of a GOWA GroupInfo. `user` is the digits an @mention is written with
        (the phone when known, else the LID); `ids` are every digit run that names the person."""
        out: list[dict[str, Any]] = []
        participants = group.get("Participants")
        if not isinstance(participants, list):
            return out
        for p in participants:
            if not isinstance(p, dict):
                continue
            phone = jid_core_user(p.get("PhoneNumber")) or (
                jid_core_user(p.get("JID")) if not str(p.get("JID") or "").endswith("@lid") else ""
            )
            ids = [i for i in dict.fromkeys([phone, jid_core_user(p.get("LID")), jid_core_user(p.get("JID"))]) if i]
            if ids:
                out.append({"user": phone or ids[0], "ids": ids})
        return out

    def _members_of(self, jid: str) -> list[dict[str, Any]]:
        cached = self._group_members.get(jid)
        if cached or not self.client:
            return cached or []
        code, body = self.client.get("/group/info", query={"group_id": jid})
        data = results(body).get("data") if code == 200 else None
        if isinstance(data, dict):
            members = self._group_member_list(data)
            if members:
                self._group_members[jid] = members
            return members
        return []

    def _is_self(self, ref: dict[str, Any]) -> bool:
        if not self._self_jid:
            return False
        return any(jids_same(j, self._self_jid) for j in ref["all"] if not j.endswith("@lid")) or (
            any(
                canonical_chat_jid(j, self._lid_map) != j
                and jids_same(canonical_chat_jid(j, self._lid_map), self._self_jid)
                for j in ref["all"]
                if j.endswith("@lid")
            )
        )

    def _own_names(self) -> set[str]:
        names = {a.strip() for a in self._self_aliases if isinstance(a, str) and a.strip()}
        if self._self_name and str(self._self_name).strip():
            names.add(str(self._self_name).strip())
        return names

    # --------------------------------------------------------------------- names

    def _contact_name_for(self, ref: dict[str, Any]) -> Optional[str]:
        own = self._own_names()
        for j in ref["all"]:
            name = lookup_contact_name(self._contacts, j)
            ok = valid_chat_name(name, j, own, allow_own=self._is_self(ref))
            if ok:
                return ok
        return None

    def _cached_names(self, ref: dict[str, Any]) -> dict[str, str]:
        own = self._own_names()
        is_group = self._ctype(ref["remote_id"]) != "direct"
        with self._names_lock:
            merged: dict[str, str] = {}
            for j in reversed(ref["all"]):
                for k, v in (self._names.get(j) or {}).items():
                    merged[k] = v
        if not is_group and not self._is_self(ref):
            merged = {k: v for k, v in merged.items() if v not in own}
        return merged

    def _compose_names(
        self,
        ref: dict[str, Any],
        *,
        row: Optional[dict[str, Any]] = None,
        push: Optional[str] = None,
        verified: Optional[str] = None,
        subject: Optional[str] = None,
        conv_name: Optional[str] = None,
    ) -> dict[str, str]:
        """Merge every name source for this chat into the per-chat cache and return it."""
        remote = ref["remote_id"]
        ctype = self._ctype(remote)
        own = self._own_names()
        is_self = self._is_self(ref)
        updates: dict[str, str] = {}
        contact = self._contact_name_for(ref) if ctype == "direct" else None
        if contact:
            updates["contact"] = contact
        if ctype != "direct":
            raw_subject = subject or self._subjects.get(remote)
            if row is not None:
                raw_subject = raw_subject or row.get("name") or row.get("chat_name") or row.get("subject")
            raw_subject = raw_subject or conv_name
            ok = valid_chat_name(raw_subject, remote, own, allow_own=True)
            if ok:
                updates["subject"] = ok
        elif not is_self:
            candidates: list[Any] = []
            if row is not None:
                candidates.append(row.get("name") or row.get("chat_name"))
            candidates.append(conv_name)
            for cand in candidates:
                if not own:
                    # GOWA can store the account's own name as a chat name. Until the own
                    # names are known that cannot be told apart from a real push name.
                    break
                ok = valid_chat_name(cand, remote, own)
                if ok and ok != (contact or updates.get("contact")):
                    updates["push"] = ok
                    break
            if row is not None:
                ver = valid_chat_name(
                    row.get("verified_name") or row.get("business_name"), remote, own
                )
                if ver:
                    updates["verified"] = ver
            ok_push = valid_chat_name(push, remote, own)
            if ok_push:
                updates["push"] = ok_push
            ok_ver = valid_chat_name(verified, remote, own)
            if ok_ver:
                updates["verified"] = ok_ver
        with self._names_lock:
            cur = self._names.setdefault(remote, {})
            for k, v in updates.items():
                if k in NAME_KEYS and v:
                    cur[k] = v
        names = self._cached_names(ref)
        if is_self:
            names = {k: v for k, v in names.items() if k == "contact"}
        return names

    # ------------------------------------------------------------ conversation

    def _emit_conversation(
        self,
        ref: dict[str, Any],
        names: dict[str, str],
        *,
        last_message_at: Optional[str] = None,
        preview: Optional[str] = None,
        archived: Optional[bool] = None,
        pinned: Optional[bool] = None,
        history: bool = False,
    ) -> bool:
        """Emit conversation.updated unless nothing changed since the last emit. Returns True if sent."""
        remote = ref["remote_id"]
        ctype = self._ctype(remote)
        is_self = self._is_self(ref)
        with self._emit_lock:
            prev_same = self._chat_cache.get(remote)
            prev = prev_same
            if prev is None:
                for a in ref["alts"]:
                    if a in self._chat_cache:
                        prev = self._chat_cache[a]
                        break
            cached_dt = parse_timestamp_strict(prev.get("last_message_at")) if prev else None
            new_dt = parse_timestamp_strict(last_message_at)
            best = new_dt
            if cached_dt and (best is None or cached_dt > best):
                best = cached_dt
            at_iso = best.isoformat() if best else None
            if archived is None and prev:
                archived = prev.get("archived")
            if pinned is None and prev:
                pinned = prev.get("pinned")
            send = self._group_send.get(remote) if ctype == "group" else None
            title = choose_title(
                remote,
                names,
                is_self=is_self,
                self_name=self._self_name,
                is_group=ctype != "direct",
            )
            sig = (
                remote,
                tuple(ref["alts"]),
                tuple(sorted(names.items())),
                title,
                at_iso,
                archived,
                pinned,
                is_self,
                ctype,
                send,
            )
            if prev_same is not None and prev_same.get("sig") == sig:
                return False
            payload: dict[str, Any] = {
                "remote_id": remote,
                "names": dict(names),
                "is_self": is_self,
                "title": title,
                "conversation_type": ctype,
                "history": history,
            }
            if ref["alts"]:
                payload["alt_remote_ids"] = list(ref["alts"])
            if at_iso:
                payload["last_message_at"] = at_iso
            send_preview = bool(preview) and (
                cached_dt is None or (new_dt is not None and new_dt >= cached_dt)
            )
            if send_preview:
                payload["preview"] = preview
            if isinstance(archived, bool):
                payload["archived"] = archived
            if isinstance(pinned, bool):
                payload["pinned"] = pinned
            if send is not None:
                payload["announce_only"], payload["self_is_admin"] = send
            self._chat_cache[remote] = {
                "sig": sig,
                "last_message_at": at_iso,
                "archived": archived if isinstance(archived, bool) else None,
                "pinned": pinned if isinstance(pinned, bool) else None,
                "payload": dict(payload),
            }
            if remote.endswith("@lid"):
                self._lid_chats.add(remote)
            self._emit("conversation.updated", payload)
            return True

    def _thumbnail_for(self, message: dict[str, Any]) -> Optional[str]:
        """Path of the stored preview for a media message, if one was extracted from a dump."""
        kind = str(message.get("media_type") or "").lower()
        if kind not in THUMB_MEDIA_TYPES or not message.get("id"):
            return None
        try:
            path = thumbnail_file(self.account_id, str(message["id"]))
            return str(path) if path.is_file() else None
        except OSError:
            return None

    def _store_dump_thumbnails(self, convs: list[dict[str, Any]]) -> list[dict[str, str]]:
        """Write every embedded preview of the given dump conversations. Returns fact rows."""
        facts: list[dict[str, str]] = []
        for conv in convs:
            wraps = conv.get("messages")
            if not isinstance(wraps, list):
                continue
            raw_id = str(conv.get("ID") or conv.get("id") or "")
            primary = str(conv.get("pnJID") or conv.get("pnJid") or raw_id)
            for wrap in wraps:
                if not isinstance(wrap, dict):
                    continue
                found = history_message_thumbnail(wrap)
                if not found:
                    continue
                mid, raw = found
                path = save_thumbnail(self.account_id, mid, raw)
                if path:
                    facts.append(
                        {"conversation_id": primary, "message_id": mid, "thumbnail_path": str(path)}
                    )
        return facts

    def _backfill_dump_thumbnails(self) -> None:
        """One pass over dumps imported before previews were kept; reports them as facts."""
        self.state.bind_thumbs(thumbnail_dir(self.account_id))
        pending = [
            p
            for p in history_files(self._self_jid)
            if p.name in self.state.imported_files and p.name not in self.state.thumb_files
        ]
        if not pending:
            return
        try:
            convs = self._read_dump_conversations(pending)
            readable = [p for p, _ in convs]
            facts = self._store_dump_thumbnails([c for _, rows in convs for c in rows])
        except Exception as e:
            log(f"dump thumbnails: {type(e).__name__}")
            return
        for i in range(0, len(facts), 200):
            self._emit("media.thumbnails", {"items": facts[i : i + 200]})
        self.state.mark_thumbs_done([p.name for p in readable], len(facts))
        self.state.save()
        log(f"dump thumbnails: {len(facts)} previews from {len(readable)} dump(s)")

    @staticmethod
    def _read_dump_conversations(paths: list[Path]) -> list[tuple[Path, list[dict[str, Any]]]]:
        """Conversations per readable dump, status broadcasts included (their posts have previews)."""
        out: list[tuple[Path, list[dict[str, Any]]]] = []
        for path in paths:
            try:
                payload = json.loads(path.read_text(encoding="utf-8", errors="replace"))
            except Exception:
                continue
            rows = payload.get("conversations") if isinstance(payload, dict) else None
            out.append((path, [c for c in rows if isinstance(c, dict)] if isinstance(rows, list) else []))
        return out

    def _emit_message(self, ref: dict[str, Any], msg: dict[str, Any], history: bool) -> None:
        remote = ref["remote_id"]
        message: dict[str, Any] = {
            "id": msg["id"],
            "sender_id": msg.get("sender_id"),
            "sender_name": msg.get("sender_name"),
            "text": msg.get("text") or "",
            "preview": msg.get("preview") or "",
            "timestamp": msg["timestamp"],
            "from_me": bool(msg.get("from_me")),
        }
        message.update(msg.get("extra") or {})
        thumb = self._thumbnail_for(message)
        if thumb:
            message["thumbnail_path"] = thumb
        payload: dict[str, Any] = {
            "conversation_id": remote,
            "remote_id": remote,
            "history": history,
            "message": message,
        }
        if ref["alts"]:
            payload["alt_remote_ids"] = list(ref["alts"])
        self._emit("message.sent" if msg.get("from_me") else "message.received", payload)
        self.state.advance_cursor(remote, msg["timestamp"])
        at = _epoch(msg["timestamp"])
        with self._tail_lock:
            tail = self._tail.get(remote)
            if tail is None or at > tail[0]:
                self._tail[remote] = (at, {str(msg["id"])})
            elif at == tail[0]:
                tail[1].add(str(msg["id"]))

    # -------------------------------------------------------------- REST rows

    def _row_to_message(
        self, row: dict[str, Any], ref: dict[str, Any], title: str
    ) -> Optional[dict[str, Any]]:
        if row.get("message_stub_type") or row.get("messageStubType"):
            inner = _as_dict(row.get("message"))
            if not (row.get("content") or row.get("media_type") or (inner or {}).get("conversation")):
                return None
        msg_id = str(row.get("id") or "").strip()
        if not msg_id:
            log(f"drop message without id in {ref['remote_id']}")
            return None
        from_me = bool(row.get("is_from_me") or row.get("from_me"))
        text, extra = message_body_and_media(row)
        if not str(text or "").strip() and not extra.get("media_type"):
            return None
        mentions = self._mention_facts(text, mentioned_jids_in(row))
        if mentions:
            extra["mentions"] = mentions
        reply = self._reply_fact(reply_to_from_row(row))
        if reply:
            extra["reply_to"] = reply
        ts = to_rfc3339_strict(row.get("timestamp") or row.get("created_at"))
        if not ts:
            log(f"drop message {msg_id} in {ref['remote_id']}: missing/invalid timestamp")
            return None
        sender_name = resolve_sender_name(
            row, row, ref["remote_id"], title, self._contacts, self._self_aliases
        )
        return {
            "id": msg_id,
            "sender_id": row.get("sender_jid"),
            "sender_name": sender_name,
            "text": text,
            "preview": preview_for_message(text, extra),
            "timestamp": ts,
            "from_me": from_me,
            "extra": extra,
        }

    def _fetch_rows(
        self,
        ref: dict[str, Any],
        *,
        max_messages: int,
        since: Optional[str] = None,
    ) -> list[dict[str, Any]]:
        """Newest-first message rows for a chat, optionally only those at/after `since`."""
        assert self.client
        answered = False
        last_code = 0
        for jid in ref["fetch"]:
            encoded = urllib.parse.quote(jid, safe="")
            rows_all: list[dict[str, Any]] = []
            offset = 0
            page = 80 if since is None else self.CATCHUP_PAGE
            failed = False
            while offset < max_messages:
                batch = min(page, max_messages - offset)
                query: dict[str, Any] = {"limit": batch, "offset": offset}
                if since:
                    query["start_time"] = since
                code, body = self.client.get(f"/chat/{encoded}/messages", query=query)
                if code != 200:
                    failed = True
                    last_code = code
                    break
                answered = True
                r = results(body)
                rows = r.get("data") or r.get("messages") or []
                if not isinstance(rows, list) or not rows:
                    break
                rows_all.extend(x for x in rows if isinstance(x, dict))
                if len(rows) < batch:
                    break
                offset += batch
            if rows_all:
                return rows_all
            if failed:
                continue
        if not answered:
            # Every attempt failed: callers must not treat this as "no new messages"
            # (they would move the cursor past messages that were never fetched).
            raise RuntimeError(f"GOWA message fetch failed ({last_code}) for {ref['remote_id']}")
        return []

    def _emit_rows(
        self,
        ref: dict[str, Any],
        rows: list[dict[str, Any]],
        *,
        history: bool,
        skip_ids: Optional[set[str]] = None,
    ) -> int:
        """Emit rows (given newest first) oldest first. Returns number of messages emitted."""
        names = self._compose_names(ref)
        title = choose_title(
            ref["remote_id"],
            names,
            is_self=self._is_self(ref),
            self_name=self._self_name,
            is_group=self._ctype(ref["remote_id"]) != "direct",
        )
        msgs: list[dict[str, Any]] = []
        for row in reversed(rows):
            m = self._row_to_message(row, ref, title)
            if m is None:
                continue
            if skip_ids and m["id"] in skip_ids:
                continue
            msgs.append(m)
        if not msgs:
            return 0
        newest = max(msgs, key=lambda m: _epoch(m["timestamp"]))
        self._emit_conversation(
            ref,
            names,
            last_message_at=newest["timestamp"],
            preview=newest["preview"],
            history=history,
        )
        for m in msgs:
            self._emit_message(ref, m, history)
        return len(msgs)

    def _sync_messages(
        self,
        ref: dict[str, Any],
        max_messages: int = 500,
        *,
        history: bool = True,
        since: Optional[str] = None,
    ) -> int:
        if not self.client:
            return 0
        rows = self._fetch_rows(ref, max_messages=max_messages, since=since)
        if not rows:
            return 0
        return self._emit_rows(ref, rows, history=history)

    # ------------------------------------------------------------ sync pipeline

    def _refresh_contacts(self) -> None:
        """Reload the address book; a failed fetch keeps the previous contacts."""
        if not self.client:
            return
        fresh = load_saved_contacts(self.client)
        if fresh is None:
            return
        if not fresh and self._contacts:
            # An empty book right after a non-empty one is far more likely a transient
            # GOWA state than every contact being deleted.
            return
        self._contacts = fresh
        self._contacts_loaded = True

    def _refresh_metadata(self, force: bool = False) -> bool:
        """Initial linking populates contacts/settings asynchronously. Keep retrying,
        including chats outside the catch-up page, without re-fetching messages.
        """
        if not self.client or not self._metadata_lock.acquire(blocking=False):
            return False
        try:
            now = time.monotonic()
            if not force and now - self._metadata_checked_at < self.METADATA_REFRESH_SECONDS:
                return False
            self._metadata_checked_at = now
            self._refresh_contacts()
            self._reload_lid_map(force=True)
            own_jid, own_name, aliases = load_self_profile(self.client, self._contacts)
            if own_jid and "@" in own_jid:
                self._self_jid = own_jid
            if own_name:
                self._self_name = own_name
            self._self_aliases.update(aliases)
            self._saved_jids = load_saved_contact_jids(self._self_jid)
            signature = hashlib.sha256(
                json.dumps(
                    [self._contacts, None if self._saved_jids is None else sorted(self._saved_jids)],
                    sort_keys=True,
                ).encode()
            ).hexdigest()
            if self._contacts and signature != self._contacts_signature:
                emit_contacts_synced(self.account_id, self._contacts, self._saved_jids)
                self._contacts_signature = signature
            if force or now - self._groups_checked_at >= self.GROUP_REFRESH_SECONDS:
                code, body = self.client.get("/user/my/groups")
                groups = results(body).get("data") if code == 200 else None
                if isinstance(groups, list) and groups:
                    self._groups_checked_at = now
                    for group in groups:
                        if not isinstance(group, dict):
                            continue
                        jid = group.get("JID") or group.get("jid")
                        if not jid:
                            continue
                        send = self._group_send_facts(group)
                        if send is not None:
                            self._group_send[str(jid)] = send
                        members = self._group_member_list(group)
                        if members:
                            self._group_members[str(jid)] = members
                        subject = group.get("Name") or group.get("name")
                        if isinstance(subject, str) and subject.strip():
                            self._subjects[str(jid)] = subject.strip()
                self._refresh_channel_names()
            self._chat_settings = load_chat_settings(self._lid_map, self._self_jid)
            with self._emit_lock:
                cached = list(self._chat_cache)
            # Settings can introduce an old pinned chat outside GOWA's newest page.
            remotes = dict.fromkeys(cached + [j for j in self._chat_settings if "@" in j])
            for jid in remotes:
                ref = self._chat_ref(jid)
                if ref is None:
                    continue
                archived, pinned = self._row_flags({}, ref, self._chat_settings)
                self._emit_conversation(
                    ref, self._compose_names(ref), archived=archived, pinned=pinned, history=True
                )
            return True
        finally:
            self._metadata_lock.release()

    def _begin_sync(self, force: bool) -> bool:
        with self._sync_lock:
            if self._syncing or (self._history_done and not force):
                return False
            self._syncing = True
            return True

    def start_sync(self, force: bool = False) -> bool:
        """Start the connect-time sync on a background thread (at most one at a time)."""
        if not self._begin_sync(force):
            return False
        threading.Thread(target=self._guarded_sync, daemon=True, name="wa-history").start()
        return True

    def _guarded_sync(self) -> None:
        try:
            self._do_sync()
        except Exception as e:
            log(f"history sync: {e}")

    def sync_history(self, force: bool = False) -> None:
        """Blocking variant of start_sync (used by tests)."""
        if not self._begin_sync(force):
            return
        self._do_sync()

    def _do_sync(self) -> None:
        assert self.client
        refs: list[dict[str, Any]] = []
        last_times: dict[str, Optional[str]] = {}
        try:
            self._emit("history.sync.started", {})
            self._refresh_metadata(force=True)
            chat_settings = self._chat_settings
            self._emit_status_feed(force=True)
            self._emit_account_avatar()
            offset = 0
            page = 100
            while True:
                code, body = self.client.get("/chats", query={"limit": page, "offset": offset})
                if code != 200:
                    log(f"GET /chats {code}: {body}")
                    return
                r = results(body)
                chats = r.get("data") or r.get("chats") or []
                if not isinstance(chats, list):
                    return
                for chat in chats:
                    if not isinstance(chat, dict):
                        continue
                    jid = chat.get("jid") or chat.get("id")
                    if not jid:
                        continue
                    ref = self._chat_ref(jid, chat.get("chat_lid") or chat.get("lid"))
                    if ref is None:
                        continue
                    refs.append(ref)
                    self._listed.add(ref["remote_id"])
                    names = self._compose_names(ref, row=chat)
                    last_iso = to_rfc3339_strict(
                        chat.get("last_message_time") or chat.get("lastMessageTime")
                    )
                    last_times[ref["remote_id"]] = last_iso
                    archived, pinned = self._row_flags(chat, ref, chat_settings)
                    self._emit_conversation(
                        ref,
                        names,
                        last_message_at=last_iso,
                        archived=archived,
                        pinned=pinned,
                        history=True,
                    )
                if len(chats) < page:
                    break
                offset += page
            self._history_done = True
            self._emit_self_conversation(chat_settings)
            self._emit("history.sync.completed", {})
            for idx, ref in enumerate(refs):
                if self.stop.is_set():
                    return
                last_iso = last_times.get(ref["remote_id"])
                cursor = self.state.cursor_for(ref["all"])
                try:
                    if cursor:
                        if _newer(last_iso, cursor):
                            self._sync_messages(ref, 200, history=True, since=cursor)
                    elif idx < 30 and not self.state.is_backfilled(ref["all"]):
                        self._sync_messages(ref, 120, history=True)
                except Exception as e:
                    log(f"sync messages {ref['remote_id']}: {e}")
            try:
                self._import_history_dumps()
            except Exception as e:
                log(f"history import: {e}")
            self._start_backfill(refs)
            self._start_reply_backfill(refs)
            self.state.save()
        finally:
            with self._sync_lock:
                self._syncing = False
            self._emit("inbox.catchup", {})
            if refs:
                threading.Thread(
                    target=self._prefetch_avatars,
                    args=([r["remote_id"] for r in refs],),
                    daemon=True,
                    name="wa-avatars",
                ).start()

    def _refresh_channel_names(self) -> None:
        """Fill in channel names GOWA's chat list never carries (`Newsletter <id>`).

        Contacts, group subjects and channel names are the three name sources; this is the
        only one that needs a request per chat, so it runs with the group refresh, not with
        every metadata pass.
        """
        if not self.client or not self._newsletter_ok:
            return
        with self._emit_lock:
            jids = sorted(j for j in self._chat_cache if j.endswith("@newsletter"))
        jids += sorted(
            j for j in self._listed if j.endswith("@newsletter") and j not in jids
        )
        if not jids:
            return
        found = load_newsletter_names(self.client, jids)
        if found is None:
            # An older GOWA without the endpoint: stop asking on every refresh.
            self._newsletter_ok = False
            log("channel names: /newsletter/info is not available")
            return
        with self._emit_lock:
            for jid, name in found.items():
                self._subjects[jid] = name

    def _row_flags(
        self,
        chat: dict[str, Any],
        ref: dict[str, Any],
        settings: dict[str, dict[str, bool]],
    ) -> tuple[Optional[bool], Optional[bool]]:
        pinned = chat.get("pinned")
        if pinned is None:
            pinned = chat.get("is_pinned")
        archived = chat.get("archived")
        if archived is None:
            archived = chat.get("is_archived")
        flags: dict[str, bool] = {}
        for j in ref["all"]:
            flags = settings.get(j) or settings.get(jid_user_part(j)) or {}
            if flags:
                break
        if "pinned" in flags:
            pinned = flags["pinned"]
        if "archived" in flags:
            archived = flags["archived"]
        return (
            archived if isinstance(archived, bool) else None,
            pinned if isinstance(pinned, bool) else None,
        )

    def _start_backfill(self, refs: list[dict[str, Any]]) -> None:
        """One-time message backfill per chat, remembered in the sync-state file."""
        todo = [r for r in refs if not self.state.is_backfilled(r["all"])]
        if not todo:
            return
        with self._backfill_lock:
            if self._backfill_thread and self._backfill_thread.is_alive():
                return
            self._backfill_thread = threading.Thread(
                target=self._backfill_run,
                args=(todo,),
                daemon=True,
                name="wa-msg-backfill",
            )
            self._backfill_thread.start()

    def _backfill_run(self, todo: list[dict[str, Any]]) -> None:
        total = len(todo)
        done = 0
        for ref in todo:
            if self.stop.is_set() or not self._connected:
                return
            try:
                self._sync_messages(ref, 500, history=True)
                self.state.mark_backfilled(ref["all"])
                # A fresh backfill reports reply links with the rows themselves.
                self.state.mark_replies_checked(ref["all"])
            except Exception as e:
                log(f"message backfill {ref['remote_id']}: {e}")
            done += 1
            if done % 4 == 0:
                self.state.save()
                self._emit("inbox.backfill", {"done": False, "progress": done, "total": total})
            time.sleep(0.12)
        self.state.save()
        self._emit("inbox.backfill", {"done": True, "total": total})

    STATUS_PAGE = 100
    STATUS_MAX = 500

    def _status_rows(self) -> list[dict[str, Any]]:
        """Every status GOWA still holds, read page by page (one page hid most of them)."""
        assert self.client
        encoded = urllib.parse.quote(STATUS_JID, safe="")
        rows: list[dict[str, Any]] = []
        offset = 0
        while offset < self.STATUS_MAX:
            batch = min(self.STATUS_PAGE, self.STATUS_MAX - offset)
            code, body = self.client.get(
                f"/chat/{encoded}/messages", query={"limit": batch, "offset": offset}
            )
            if code != 200:
                log(f"status feed {code}: {body}")
                break
            r = results(body)
            page = r.get("data") or r.get("messages") or []
            if not isinstance(page, list) or not page:
                break
            rows.extend(x for x in page if isinstance(x, dict))
            if len(page) < batch:
                break
            offset += batch
        return rows

    def _status_rest_posts(self, rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
        """Status posts from GOWA's own rows. Those rows carry no link card, only its text."""
        posts: list[dict[str, Any]] = []
        for row in rows:
            sender = canonical_chat_jid(str(row.get("sender_jid") or ""), self._lid_map)
            if not sender:
                continue
            ts = to_rfc3339_strict(row.get("timestamp") or row.get("created_at"))
            if not ts:
                continue
            text, extra = message_body_and_media(row)
            posts.append(
                {
                    "id": str(row.get("id") or "").strip(),
                    "sender": sender,
                    "sender_name": row.get("sender_display_name") or "",
                    "timestamp": ts,
                    "from_me": bool(row.get("is_from_me") or row.get("from_me")),
                    "media_type": str(extra.get("media_type") or row.get("media_type") or "text"),
                    "text": text or "",
                    "thumbnail": None,
                    "link_preview": None,
                }
            )
        return posts

    def _status_dump_posts(self) -> list[dict[str, Any]]:
        """Statuses only the history dumps carry (`statusV3Messages`), one pass per dump.

        A linked phone syncs the statuses it already had, while GOWA's own table only sees the
        ones that arrived during a session. The dumps also keep the link card a text status
        was posted with, which GOWA's rows drop.
        """
        pending = [
            p for p in history_files(self._self_jid) if p.name not in self.state.status_files
        ]
        if not pending:
            return []
        posts: list[dict[str, Any]] = []
        readable: list[str] = []
        for path in pending:
            try:
                payload = json.loads(path.read_text(encoding="utf-8", errors="replace"))
            except Exception as e:
                log(f"status dump {path.name}: {type(e).__name__}")
                continue
            readable.append(path.name)
            if not isinstance(payload, dict):
                continue
            entries = payload.get("statusV3Messages")
            if not isinstance(entries, list):
                continue
            by_lid = lid_pairs(payload.get("phoneNumberToLidMappings"))
            for entry in entries:
                facts = status_post_facts(entry)
                if not facts:
                    continue
                facts["sender"] = self._status_sender(str(facts["sender"]), by_lid)
                if not facts["sender"]:
                    continue
                facts["sender_name"] = ""
                posts.append(facts)
        if readable:
            self.state.mark_status_files(readable)
            self.state.save()
        return posts

    def _status_sender(self, sender: str, by_lid: dict[str, str]) -> str:
        """A status author as a phone JID when WhatsApp only names them by LID."""
        if not sender:
            return ""
        if sender.endswith("@lid"):
            phone = by_lid.get(jid_core_user(sender)) or ""
            if phone:
                self._learn_lid(sender, phone)
        return self._canonical_person(sender)

    def _emit_status_feed(self, force: bool = False) -> None:
        if not self.client:
            return
        self._feed_checked_at = time.time()
        try:
            found = self._status_rest_posts(self._status_rows())
        except Exception as e:
            log(f"status feed: {type(e).__name__}")
            return
        try:
            found.extend(self._status_dump_posts())
        except Exception as e:
            log(f"status dumps: {type(e).__name__}")
        items = self._group_status_posts(found)
        sig = json.dumps(items, sort_keys=True, default=str)
        if not force and sig == self._feed_sig:
            return
        self._feed_sig = sig
        self._emit("status.feed", {"items": items})

    def _group_status_posts(self, found: list[dict[str, Any]]) -> list[dict[str, Any]]:
        """One rail entry per author, its posts oldest first, newest activity first."""
        by_sender: dict[str, dict[str, Any]] = {}
        seen: set[tuple[str, str]] = set()
        for post in found:
            sender = str(post.get("sender") or "")
            msg_id = str(post.get("id") or "")
            if not sender:
                continue
            if msg_id:
                if (sender, msg_id) in seen:
                    continue
                seen.add((sender, msg_id))
            media_type = str(post.get("media_type") or "text")
            text = str(post.get("text") or "")
            preview = text[:160]
            thumb = None
            raw = post.get("thumbnail")
            if isinstance(raw, bytes):
                thumb = save_thumbnail(self.account_id, msg_id, raw)
            if thumb is None:
                thumb = self._thumbnail_for({"id": msg_id, "media_type": media_type})
            published: dict[str, Any] = {
                "id": msg_id,
                "media_type": media_type,
                "text": text if text and not text.startswith("[") else (preview or ""),
                "timestamp": post["timestamp"],
            }
            if thumb:
                published["thumbnail_path"] = str(thumb)
            card = status_link_card(self.account_id, msg_id, post.get("link_preview"))
            if card:
                published["link_preview"] = card
            entry = by_sender.get(sender)
            if entry is None:
                entry = {
                    "sender_id": sender,
                    "sender_name": post.get("sender_name") or self._person_name(sender),
                    "timestamp": post["timestamp"],
                    "media_type": media_type,
                    "preview": preview,
                    "from_me": bool(post.get("from_me")),
                    "count": 0,
                    "posts": [],
                }
                by_sender[sender] = entry
            if msg_id:
                entry["posts"].append(published)
            entry["count"] = int(entry.get("count") or 0) + 1
            if _newer(post["timestamp"], entry.get("timestamp")):
                entry["timestamp"] = post["timestamp"]
                entry["preview"] = preview
                entry["media_type"] = media_type
                if post.get("sender_name"):
                    entry["sender_name"] = post["sender_name"]
        items: list[dict[str, Any]] = []
        for entry in by_sender.values():
            posts = entry.get("posts")
            if isinstance(posts, list) and posts:
                # Oldest to newest within a sender (WhatsApp-style playback).
                posts.sort(key=lambda p: _epoch((p or {}).get("timestamp")))
                entry["count"] = len(posts)
            items.append(entry)
        # Newest activity first in the stories rail.
        items.sort(key=lambda e: _epoch(e.get("timestamp")), reverse=True)
        return items

    def _emit_account_avatar(self) -> None:
        """Push the connected account's profile photo for the sidebar rail."""
        if not self.client or not self._self_jid:
            return
        try:
            data = load_cached_avatar(self.account_id, self._self_jid)
            if not data:
                raw = fetch_avatar_bytes(self.client, self.account_id, self._self_jid)
                if raw:
                    data = load_cached_avatar(self.account_id, self._self_jid)
            if not data:
                return
            self._emit("account.avatar", {"avatar_data": data})
        except Exception as e:
            log(f"account avatar: {e}")

    # --------------------------------------------------------------- dump import

    def _parse_history_wraps(self, wraps: Any) -> tuple[list[dict[str, Any]], str]:
        parsed: list[dict[str, Any]] = []
        stub_name = ""
        if not isinstance(wraps, list):
            return parsed, stub_name
        for wrap in wraps:
            if not isinstance(wrap, dict):
                continue
            fields = history_message_fields(wrap)
            if not fields:
                continue
            if fields.get("stub_name") and not stub_name:
                stub_name = str(fields["stub_name"])
            body = str(fields.get("text") or "").strip()
            if body.startswith("[") and body.endswith("]") and fields.get("stub_name"):
                continue
            if not body:
                continue
            parsed.append(fields)
        return parsed, stub_name

    def _emit_history_messages(
        self, ref: dict[str, Any], title: str, parsed: list[dict[str, Any]]
    ) -> int:
        emitted = 0
        dropped = 0
        for fields in parsed[-500:]:
            msg_id = str(fields.get("id") or "").strip()
            ts = to_rfc3339_strict(fields.get("timestamp"))
            if not msg_id or not ts:
                dropped += 1
                continue
            text, extra = message_body_and_media(
                {"text": fields["text"], "media_type": fields.get("media_type")}
            )
            mentions = self._mention_facts(text, fields.get("mentioned_jids"))
            if mentions:
                extra["mentions"] = mentions
            reply = self._reply_fact(fields.get("reply_to"))
            if reply:
                extra["reply_to"] = reply
            sender = fields.get("sender_jid")
            if sender and not fields["from_me"]:
                sender = self._canonical_person(sender)
            sender_name = resolve_sender_name(
                {"sender_jid": sender, "from_me": fields["from_me"]},
                {},
                ref["remote_id"],
                title,
                self._contacts,
                self._self_aliases,
            )
            self._emit_message(
                ref,
                {
                    "id": msg_id,
                    # In a group an unknown author stays unknown; the group JID is not a person.
                    "sender_id": sender
                    or (None if ref["remote_id"].endswith("@g.us") else ref["remote_id"]),
                    "sender_name": sender_name,
                    "text": text,
                    "preview": preview_for_message(text, extra),
                    "timestamp": ts,
                    "from_me": fields["from_me"],
                    "extra": extra,
                },
                True,
            )
            emitted += 1
        if dropped:
            log(f"history dump {ref['remote_id']}: dropped {dropped} rows without id/timestamp")
        return emitted

    def _import_history_dumps(self) -> None:
        """Import this account's GOWA history dumps once each; never again afterwards."""
        pending = [p for p in history_files(self._self_jid) if p.name not in self.state.imported_files]
        if not pending:
            self._repair_history_dumps()
            self._repair_dump_replies()
            self._backfill_dump_thumbnails()
            return
        convs, readable = parse_history_conversations(pending)
        imported = 0
        for conv in convs:
            raw_id = str(conv.get("ID") or conv.get("id") or "")
            pn = conv.get("pnJID") or conv.get("pnJid")
            primary = str(pn or raw_id)
            lid_hint = raw_id if raw_id.endswith("@lid") else None
            if not primary or primary.endswith("@broadcast"):
                continue
            ref = self._chat_ref(primary, lid_hint)
            if ref is None:
                continue
            parsed, stub_name = self._parse_history_wraps(conv.get("messages"))
            names = self._compose_names(ref, conv_name=conv.get("name"))
            if not parsed and not names and not stub_name:
                continue
            title = choose_title(
                ref["remote_id"],
                names,
                is_self=self._is_self(ref),
                self_name=self._self_name,
                is_group=self._ctype(ref["remote_id"]) != "direct",
            )
            last = parsed[-1] if parsed else None
            last_at = to_rfc3339_strict(last.get("timestamp") if last else conv.get("conversationTimestamp"))
            preview = last.get("text") if last else stub_name
            self._emit_conversation(
                ref, names, last_message_at=last_at, preview=preview or None, history=True
            )
            imported += 1
            self._emit_history_messages(ref, title, parsed)
            self._emit_dump_reactions(ref, conv.get("messages"))
        # Only files that were readable count as imported; a half-written dump is retried.
        self.state.mark_imported([p.name for p in readable])
        self.state.mark_reactions_imported([p.name for p in readable])
        # This import reported reply links itself; no repair pass is needed for these files.
        self.state.mark_replies_imported([p.name for p in readable])
        self.state.save()
        log(f"history ingest: {imported} chats from {len(readable)} WhatsApp dump(s)")
        self._repair_history_dumps()
        self._repair_dump_replies()
        self._backfill_dump_thumbnails()

    def _emit_dump_reactions(self, ref: dict[str, Any], wraps: Any) -> int:
        """Report the reactions stored on a chat's dump messages. Returns how many were sent."""
        if not isinstance(wraps, list):
            return 0
        facts: list[dict[str, Any]] = []
        for wrap in wraps:
            if not isinstance(wrap, dict):
                continue
            target, rows = history_message_reactions(wrap)
            for r in rows:
                ts = to_rfc3339_strict(r.get("timestamp"))
                if not ts:
                    continue
                facts.append(
                    self._reaction_fact(target or "", r["jid"], r["from_me"], r["emoji"], ts)
                )
        for i in range(0, len(facts), 200):
            self._emit_reactions(ref, facts[i : i + 200], history=True)
        return len(facts)

    def _repair_history_dumps(self) -> None:
        """One extra pass over dumps imported by an older build.

        Those builds stored every group message under the group JID as its sender (so no
        author name could be shown) and ignored reactions. Re-reading each dump once re-reports
        group messages with their real author (the core corrects the stored sender of a known
        message) and reports the reactions. Never run again for a file.
        """
        pending = [
            p
            for p in history_files(self._self_jid)
            if p.name in self.state.imported_files and p.name not in self.state.reaction_files
        ]
        if not pending:
            return
        convs, readable = parse_history_conversations(pending)
        sent = 0
        for conv in convs:
            raw_id = str(conv.get("ID") or conv.get("id") or "")
            primary = str(conv.get("pnJID") or conv.get("pnJid") or raw_id)
            if not primary or primary.endswith(("@broadcast", "@newsletter")):
                continue
            lid_hint = raw_id if raw_id.endswith("@lid") else None
            ref = self._chat_ref(primary, lid_hint)
            if ref is None:
                continue
            if ref["remote_id"].endswith("@g.us"):
                parsed, _stub = self._parse_history_wraps(conv.get("messages"))
                names = self._compose_names(ref, conv_name=conv.get("name"))
                title = choose_title(
                    ref["remote_id"],
                    names,
                    is_self=False,
                    self_name=self._self_name,
                    is_group=True,
                )
                self._emit_history_messages(
                    ref, title, [f for f in parsed if f.get("sender_jid") and not f.get("from_me")]
                )
            sent += self._emit_dump_reactions(ref, conv.get("messages"))
        self.state.mark_reactions_imported([p.name for p in readable])
        self.state.save()
        log(f"history repair: {sent} reactions from {len(readable)} WhatsApp dump(s)")

    def _repair_dump_replies(self) -> None:
        """One extra pass over dumps imported before replies were reported.

        Re-reads each such dump once and re-reports only the messages that answer another
        message, with their `reply_to` fact. The core merges it into the stored message's
        metadata (a known message is never duplicated). Never run again for a file.
        """
        pending = [
            p
            for p in history_files(self._self_jid)
            if p.name in self.state.imported_files and p.name not in self.state.reply_files
        ]
        if not pending:
            return
        convs, readable = parse_history_conversations(pending)
        sent = 0
        for conv in convs:
            raw_id = str(conv.get("ID") or conv.get("id") or "")
            primary = str(conv.get("pnJID") or conv.get("pnJid") or raw_id)
            if not primary or primary.endswith(("@broadcast", "@newsletter")):
                continue
            parsed, _stub = self._parse_history_wraps(conv.get("messages"))
            replies = [f for f in parsed if f.get("reply_to")]
            if not replies:
                continue
            ref = self._chat_ref(primary, raw_id if raw_id.endswith("@lid") else None)
            if ref is None:
                continue
            names = self._compose_names(ref, conv_name=conv.get("name"))
            title = choose_title(
                ref["remote_id"],
                names,
                is_self=self._is_self(ref),
                self_name=self._self_name,
                is_group=self._ctype(ref["remote_id"]) != "direct",
            )
            sent += self._emit_history_messages(ref, title, replies)
        self.state.mark_replies_imported([p.name for p in readable])
        self.state.save()
        log(f"history repair: {sent} reply links from {len(readable)} WhatsApp dump(s)")

    def _start_reply_backfill(self, refs: list[dict[str, Any]]) -> None:
        """One-time pass over chats synced before replies were reported (see `_reply_pass`)."""
        todo = [
            r
            for r in refs
            if self.state.is_backfilled(r["all"]) and not self.state.replies_checked(r["all"])
        ]
        if not todo:
            return
        with self._backfill_lock:
            if self._reply_thread and self._reply_thread.is_alive():
                return
            self._reply_thread = threading.Thread(
                target=self._reply_pass, args=(todo,), daemon=True, name="wa-reply-backfill"
            )
            self._reply_thread.start()

    def _reply_pass(self, todo: list[dict[str, Any]]) -> None:
        """Re-report the REST rows that answer a message, so older replies get their link.

        Only the messages go out (never the conversation row): the core merges `reply_to` into
        the stored message and leaves chat title, time and preview alone.
        """
        for ref in todo:
            if self.stop.is_set() or not self._connected:
                return
            try:
                rows = [r for r in self._fetch_rows(ref, max_messages=500) if reply_to_from_row(r)]
                if rows:
                    names = self._compose_names(ref)
                    title = choose_title(
                        ref["remote_id"],
                        names,
                        is_self=self._is_self(ref),
                        self_name=self._self_name,
                        is_group=self._ctype(ref["remote_id"]) != "direct",
                    )
                    for row in reversed(rows):
                        m = self._row_to_message(row, ref, title)
                        if m is not None:
                            self._emit_message(ref, m, True)
                self.state.mark_replies_checked(ref["all"])
            except Exception as e:
                log(f"reply backfill {ref['remote_id']}: {e}")
            time.sleep(0.12)
        self.state.save()

    def _emit_self_conversation(self, settings: dict[str, dict[str, bool]]) -> None:
        if not self._self_jid:
            return
        ref = self._chat_ref(self._self_jid)
        if ref is None:
            return
        names = self._compose_names(ref)
        flags: dict[str, bool] = {}
        for j in ref["all"]:
            flags = settings.get(j) or settings.get(jid_user_part(j)) or {}
            if flags:
                break
        self._emit_conversation(
            ref,
            names,
            archived=flags.get("archived"),
            pinned=flags.get("pinned"),
            history=True,
        )

    # --------------------------------------------------------------- open chat

    def refresh_chat(self, jid: str) -> None:
        """Open-chat refresh: read the chat's recent messages from GOWA. Never parses dumps."""
        if not self.client:
            return
        ref = self._chat_ref(jid)
        if ref is None:
            return
        if not self._contacts_loaded:
            self._refresh_contacts()
        try:
            self._sync_messages(ref, 80, history=True)
        finally:
            self._emit("chat.synced", {"remote_id": jid})

    # ----------------------------------------------------------------- catch-up

    def catch_up_recent(self) -> None:
        """Cheap fallback for missed webhooks: one /chats page, fetch only newer messages."""
        if not self.client or not self._connected:
            return
        if not self._history_done:
            # The connect-time sync has not listed the chats yet; until it has, every chat
            # would look new here.
            return
        if not self._catchup_lock.acquire(blocking=False):
            return
        try:
            self._catch_up_locked()
        finally:
            self._catchup_lock.release()

    def _catch_up_locked(self) -> None:
        assert self.client
        refreshed = self._refresh_metadata()
        # Dumps can arrive after the initial /chats listing. Previously those files
        # were never revisited until a reconnect, leaving most of the inbox absent.
        if refreshed and not self._syncing:
            self._import_history_dumps()
        code, body = self.client.get("/chats", query={"limit": self.CATCHUP_PAGE, "offset": 0})
        if code != 200:
            log(f"catch-up GET /chats {code}: {body}")
            return
        chats = results(body).get("data") or results(body).get("chats") or []
        if not isinstance(chats, list):
            return
        emitted = False
        self._catchup_cycles += 1
        sweep = self._catchup_cycles % self.SWEEP_EVERY == 0
        for chat in chats:
            if not isinstance(chat, dict):
                continue
            jid = chat.get("jid") or chat.get("id")
            if not jid:
                continue
            ref = self._chat_ref(jid, chat.get("chat_lid") or chat.get("lid"))
            if ref is None:
                continue
            last_iso = to_rfc3339_strict(
                chat.get("last_message_time") or chat.get("lastMessageTime")
            )
            cursor = self.state.cursor_for(ref["all"])
            is_new = cursor is None and ref["remote_id"] not in self._listed
            changed = _newer(last_iso, cursor)
            archived, pinned = self._row_flags(chat, ref, self._chat_settings)
            names = self._compose_names(ref, row=chat)
            if cursor is None and not is_new:
                # Awaiting its one-time backfill; only keep the row facts fresh.
                if self._emit_conversation(
                    ref, names, last_message_at=last_iso, archived=archived, pinned=pinned
                ):
                    emitted = True
                continue
            if self._emit_conversation(
                ref, names, last_message_at=last_iso, archived=archived, pinned=pinned
            ):
                emitted = True
            if not (changed or is_new or (sweep and cursor)):
                continue
            self._listed.add(ref["remote_id"])
            try:
                rows = self._fetch_rows(
                    ref,
                    max_messages=self.CATCHUP_PAGE * 2 if cursor else 30,
                    since=cursor,
                )
            except Exception as e:
                log(f"catch-up {ref['remote_id']}: {e}")
                continue
            fresh: list[dict[str, Any]] = []
            cursor_at = _epoch(cursor)
            with self._tail_lock:
                tail = self._tail.get(ref["remote_id"])
                tail_at, tail_ids = (tail[0], set(tail[1])) if tail else (0.0, set())
            for row in rows:
                ts = to_rfc3339_strict(row.get("timestamp") or row.get("created_at"))
                if cursor and ts:
                    at = _epoch(ts)
                    if at < cursor_at:
                        continue
                    # Same second as the cursor: keep it unless this process already
                    # emitted that very message (the core ignores true duplicates).
                    if at == tail_at and str(row.get("id") or "") in tail_ids:
                        continue
                fresh.append(row)
            if fresh and self._emit_rows(ref, fresh, history=False):
                emitted = True
            if last_iso:
                self.state.advance_cursor(ref["remote_id"], last_iso)
        if time.time() - self._feed_checked_at >= self.FEED_REFRESH_SECONDS:
            sig_before = self._feed_sig
            try:
                self._emit_status_feed()
            except Exception as e:
                log(f"catch-up status feed: {e}")
            if self._feed_sig != sig_before:
                emitted = True
        if emitted:
            self.state.save()
            self._emit("inbox.catchup", {})

    # ------------------------------------------------------------------ avatars

    def _prefetch_avatars(self, jids: list[str]) -> None:
        if not self.client:
            return
        unique: list[str] = []
        seen: set[str] = set()
        for jid in jids:
            if jid in seen:
                continue
            if jid.endswith("@broadcast") or jid.endswith("@newsletter") or jid.startswith("status@"):
                continue
            seen.add(jid)
            unique.append(jid)

        def one(jid: str) -> None:
            try:
                self.emit_avatar(jid)
            except Exception as e:
                log(f"avatar {jid}: {e}")

        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            list(pool.map(one, unique))

    def emit_avatar(self, jid: str) -> None:
        if not self.client or not jid:
            return
        cached = load_cached_avatar(self.account_id, jid)
        if cached:
            data = cached
        else:
            raw = fetch_avatar_bytes(self.client, self.account_id, jid)
            data = bytes_to_data_url(raw) if raw else None
        self._emit("avatar.updated", {"remote_id": jid, "avatar_data": data})

    def fetch_contact_profile(self, jid: str) -> None:
        jid_s = canonical_chat_jid(jid, self._lid_map)
        profile: dict[str, Any] = {
            "username": None,
            "phone": format_intl_phone(jid_s),
            "about": None,
            "business_name": None,
        }
        verified_name = None
        push_name = None
        if jid_s.endswith("@g.us"):
            me = jid_core_user(self._self_jid)
            profile["members"] = [m for m in self._members_of(jid_s) if me not in m["ids"]]
        if self.client and not jid_s.endswith("@g.us"):
            code, body = self.client.get("/user/info", query={"phone": jid_s})
            if code == 200:
                info = user_info_record(body)
                verified_name = info.get("verified_name") or info.get("business_name")
                push_name = info.get("push_name")
                username = push_name or verified_name or info.get("name")
                if (
                    isinstance(username, str)
                    and username.strip()
                    and username.strip() not in self._self_aliases
                ):
                    profile["username"] = username.strip()
                elif saved := lookup_contact_name(self._contacts, jid_s):
                    profile["username"] = saved
                profile["about"] = info.get("status") or info.get("about")
                if isinstance(profile["about"], str):
                    profile["about"] = profile["about"].strip() or None
            code, body = self.client.get("/user/business-profile", query={"phone": jid_s})
            if code == 200:
                biz = user_info_record(body)
                verified_name = verified_name or biz.get("business_name") or biz.get("verified_name") or biz.get("name")
                profile["business_name"] = verified_name
                if isinstance(profile["business_name"], str):
                    profile["business_name"] = profile["business_name"].strip() or None
                if profile["business_name"] and profile["business_name"] in self._self_aliases:
                    profile["business_name"] = lookup_contact_name(self._contacts, jid_s)
                if not profile["about"]:
                    profile["about"] = biz.get("description") or biz.get("status")
            if not profile["username"]:
                profile["username"] = lookup_contact_name(self._contacts, jid_s)
            ref = self._chat_ref(jid_s)
            if ref and not self._is_self(ref):
                names = self._compose_names(ref, push=push_name, verified=verified_name)
                if names:
                    self._emit_conversation(ref, names, history=True)
        send(
            {
                "type": "contact_profile",
                "account_id": self.account_id,
                "conversation_id": jid_s,
                "profile": profile,
            }
        )

    def download_media(self, jid: str, message_id: str) -> None:
        payload: dict[str, Any] = {
            "conversation_id": jid,
            "message_id": message_id,
        }
        if not self.client or not message_id:
            payload["error"] = "not connected"
            self._emit("media.downloaded", payload)
            return
        got = download_gowa_media(self.client, self.account_id, message_id, jid)
        if got:
            payload.update(got)
        else:
            payload["error"] = "download failed"
        self._emit("media.downloaded", payload)

    def download_status_media(self, message_id: str) -> None:
        """Download a status@broadcast media item for the story viewer."""
        payload: dict[str, Any] = {"message_id": message_id}
        if not self.client or not message_id:
            payload["error"] = "not connected"
            self._emit("status.media", payload)
            return
        got = download_gowa_media(
            self.client, self.account_id, message_id, "status@broadcast", for_status=True
        )
        if got:
            payload.update(got)
        else:
            payload["error"] = "download failed"
        self._emit("status.media", payload)

    def check_number(self, phone: str, request_id: str) -> None:
        """Ask GOWA whether a typed number is on WhatsApp. Always answers with a
        `number.checked` event; `on_whatsapp` is None when the check could not be made."""
        digits = normalize_phone_number(phone)
        reply: dict[str, Any] = {
            "request_id": request_id,
            "phone": digits,
            "jid": f"{digits}@s.whatsapp.net" if digits else None,
            "on_whatsapp": None,
            "error": None,
        }
        if not digits:
            reply["error"] = "invalid number"
        elif not self.client:
            reply["error"] = "not connected"
        else:
            try:
                code, body = self.client.get("/user/check", query={"phone": digits})
                if code != 200:
                    reply["error"] = f"check failed ({code})"
                else:
                    r = results(body)
                    for key in ("is_on_whatsapp", "IsOnWhatsapp", "is_on_whatsApp"):
                        if isinstance(r.get(key), bool):
                            reply["on_whatsapp"] = r[key]
                            break
            except Exception as e:
                log(f"check_number: {e}")
                reply["error"] = "check failed"
        self._emit("number.checked", reply)

    def create_group(self, title: str, participants: list[str]) -> None:
        if not self.client:
            send_error(self.account_id, "not connected")
            return
        phones: list[str] = []
        for raw in participants:
            p = str(raw).strip()
            if not p:
                continue
            phones.append(p)
        if not title.strip() or not phones:
            send_error(self.account_id, "group needs a title and at least one participant")
            return
        code, body = self.client.post(
            "/group", json_body={"title": title.strip(), "participants": phones}
        )
        if code != 200:
            send_error(self.account_id, f"create group failed ({code}): {body}")
            return
        gid = results(body).get("group_id")
        if gid:
            self._emit(
                "conversation.updated",
                {
                    "remote_id": gid,
                    "names": {"subject": title.strip()},
                    "is_self": False,
                    "title": title.strip(),
                    "conversation_type": "group",
                    "history": False,
                },
            )
        send({"type": "ok", "account_id": self.account_id, "request_id": gid})

    # --------------------------------------------------------- live delivery

    def _handle_ws(self, payload: dict[str, Any]) -> None:
        code = str(payload.get("code") or payload.get("type") or payload.get("event") or "")
        upper = code.upper()
        if upper in {"LOGIN_SUCCESS", "SUCCESS_LOGIN", "CONNECTED"} or ("LOGIN" in upper and "SUCCESS" in upper):
            self._on_connected(results(payload) or payload)
            return
        event = str(payload.get("event") or code).lower()
        if event == "message.ack":
            self._handle_receipt(payload)
            return
        if event == "message.reaction":
            self._handle_reaction(payload)
            return
        if event in CHANGE_EVENTS:
            self._handle_change(event, payload)
            return
        if event == "chat_presence":
            return
        data = payload.get("payload") if isinstance(payload.get("payload"), dict) else payload
        if not isinstance(data, dict):
            return
        emit = event in {"message", "message.received"} or bool(
            data.get("chat_id")
            or data.get("chat_jid")
            or data.get("body")
        )
        if emit:
            self._emit_incoming(data)

    # --------------------------------------------------------- message changes

    def _emit_change(self, ref: dict[str, Any], message_id: str, change: dict[str, Any]) -> None:
        payload: dict[str, Any] = {
            "conversation_id": ref["remote_id"],
            "remote_id": ref["remote_id"],
            "message_id": message_id,
            "change": change,
        }
        if ref["alts"]:
            payload["alt_remote_ids"] = list(ref["alts"])
        self._emit("message.changed", payload)

    def _handle_change(self, event: str, payload: dict[str, Any]) -> None:
        """GOWA `message.edited` / `message.revoked` / `message.deleted` from the phone or a contact.

        Reported as the same `message.changed` fact a change made from Shuttle produces.
        """
        data = payload.get("payload") if isinstance(payload.get("payload"), dict) else payload
        if not isinstance(data, dict):
            return
        target_key, _ = CHANGE_EVENTS[event]
        target = str(data.get(target_key) or "").strip()
        chat = data.get("chat_id") or data.get("chat_jid") or data.get("revoked_chat")
        ts = to_rfc3339_strict(payload.get("timestamp") or data.get("timestamp")) or (
            datetime.now(timezone.utc).isoformat()
        )
        change: dict[str, Any] = {"kind": CHANGE_EVENTS[event][1]}
        if event == "message.edited":
            text = data.get("body") if isinstance(data.get("body"), str) else nested_text(data)
            if not text or not str(text).strip():
                log(f"drop edit of {target or '?'} without text")
                return
            change.update(text=str(text), at=ts)
        elif event == "message.revoked":
            change["at"] = ts
        if not target or not chat:
            log(f"drop {event} without target or chat")
            return
        ref = self._chat_ref(chat, data.get("chat_lid"))
        if ref is None:
            return
        self._emit_change(ref, target, change)

    # ------------------------------------------------------------- reactions

    def _canonical_person(self, jid: Any) -> str:
        """A person's JID with a known LID mapped to the phone JID (one lazy map reload)."""
        raw = normalize_jid(str(jid or ""))
        if not raw.endswith("@lid"):
            return raw
        mapped = canonical_chat_jid(raw, self._lid_map)
        if mapped == raw and self._reload_lid_map():
            mapped = canonical_chat_jid(raw, self._lid_map)
        return mapped

    def _person_name(self, jid: str, push: Optional[str] = None) -> str:
        """Display name for a chat member: push name, saved contact, then the number."""
        aliases = self._own_names()
        if push and push.strip() and push.strip() not in aliases and not is_placeholder_name(
            push.strip(), jid
        ):
            return push.strip()
        saved = lookup_contact_name(self._contacts, jid)
        if saved and saved not in aliases:
            return saved
        return jid_user_part(jid)

    def _reply_fact(self, fact: Optional[dict[str, Any]]) -> Optional[dict[str, Any]]:
        """A `reply_to` fact with the quoted author mapped to a phone JID when a LID is known.

        `from_me` marks a reply to one of this account's own messages (the dump names its
        author, not who "you" are).
        """
        if not fact:
            return None
        out = dict(fact)
        sender = out.get("sender")
        if isinstance(sender, str) and sender.strip():
            canon = self._canonical_person(sender)
            out["sender"] = canon
            if self._self_jid and jids_same(canon, self._self_jid):
                out["from_me"] = True
        return out

    def _mention_facts(self, text: Any, jids: Optional[list[str]] = None) -> list[dict[str, Any]]:
        """Who the `@<digits>` tokens of a message text mention, as facts.

        `token` is the digits as written in the text, `jid` the person (the phone JID when a
        LID is known, else the LID JID) and `self` marks this account. A token is only listed
        when its person is known; the UI matches the remaining digits itself.
        """
        tokens = mention_tokens(text)
        if not tokens:
            return []
        known: dict[str, str] = {}
        for raw in jids or []:
            norm = normalize_jid(str(raw))
            canon = self._canonical_person(norm)
            known.setdefault(jid_core_user(norm), canon)
            known.setdefault(jid_core_user(canon), canon)
        facts: list[dict[str, Any]] = []
        for tok in tokens:
            canon = known.get(tok)
            if not canon:
                lid = f"{tok}@lid"
                canon = self._canonical_person(lid)
                if canon == lid:
                    continue
            fact: dict[str, Any] = {"token": tok, "jid": canon}
            if self._self_jid and jids_same(canon, self._self_jid):
                fact["self"] = True
            facts.append(fact)
        return facts

    @staticmethod
    def _reaction_emoji(data: dict[str, Any]) -> Optional[str]:
        """The reaction text of a GOWA payload; "" means the reaction was removed."""
        raw = data.get("reaction")
        if isinstance(raw, dict):
            raw = raw.get("text") if "text" in raw else raw.get("emoji")
        if raw is None:
            raw = data.get("emoji") if "emoji" in data else data.get("reaction_text")
        if raw is None:
            return None
        return raw.strip() if isinstance(raw, str) else None

    def _handle_reaction(self, payload: dict[str, Any]) -> None:
        """GOWA `message.reaction`: someone reacted to (or un-reacted from) a message.

        Reports a fact only; the core stores it keyed by the target's remote id. A reaction is
        never a message, so it does not touch the chat's time, preview or unread count.
        """
        data = payload.get("payload") if isinstance(payload.get("payload"), dict) else payload
        if not isinstance(data, dict):
            return
        emoji = self._reaction_emoji(data)
        reaction_obj = _as_dict(data.get("reaction"))
        target = (
            data.get("reacted_message_id")
            or data.get("reaction_message_id")
            or data.get("target_message_id")
            or (reaction_obj.get("key") or {}).get("ID")
        )
        target = str(target).strip() if target else ""
        chat = data.get("chat_id") or data.get("chat_jid")
        if emoji is None or not target or not chat:
            log("drop reaction without emoji, target or chat")
            return
        ref = self._chat_ref(chat, data.get("chat_lid"))
        if ref is None:
            return
        from_me = bool(data.get("from_me") or data.get("is_from_me"))
        sender = sender_only(data.get("sender_jid") or data.get("from")) or None
        from_lid = sender_only(data.get("from_lid")) or None
        if from_lid and sender and "@" in sender:
            self._learn_lid(from_lid, sender)
        who = sender or from_lid
        if not from_me and not who:
            log(f"drop reaction on {target}: no reactor")
            return
        if (
            not from_me
            and self._self_jid
            and jids_same(self._canonical_person(who), self._self_jid)
        ):
            from_me = True
        if not self._contacts_loaded and self.client:
            self._refresh_contacts()
        ts = to_rfc3339_strict(payload.get("timestamp") or data.get("timestamp")) or (
            datetime.now(timezone.utc).isoformat()
        )
        fact = self._reaction_fact(
            target,
            None if from_me else str(who),
            from_me,
            emoji,
            ts,
            push=self._first_str([data], ("from_name", "pushname", "push_name", "notify_name")),
        )
        self._emit_reactions(ref, [fact], history=False)

    def _reaction_fact(
        self,
        target: str,
        jid: Optional[str],
        from_me: bool,
        emoji: str,
        ts: str,
        push: Optional[str] = None,
    ) -> dict[str, Any]:
        """One reaction as a fact. The reactor is `me` for this account, else a phone/LID JID."""
        if from_me or not jid:
            reactor_id, name = "me", "You"
        else:
            reactor_id = self._canonical_person(jid)
            name = self._person_name(reactor_id, push)
        return {
            "target_message_id": target,
            "reactor_id": reactor_id,
            "reactor_name": name,
            "emoji": emoji,
            "timestamp": ts,
            "from_me": bool(from_me),
        }

    def _emit_reactions(
        self, ref: dict[str, Any], facts: list[dict[str, Any]], *, history: bool
    ) -> None:
        if not facts:
            return
        payload: dict[str, Any] = {
            "conversation_id": ref["remote_id"],
            "remote_id": ref["remote_id"],
            "history": history,
            "reactions": facts,
        }
        if ref["alts"]:
            payload["alt_remote_ids"] = list(ref["alts"])
        self._emit("reaction", payload)

    def _emit_poll_change(
        self,
        ref: dict[str, Any],
        poll: dict[str, Any],
        definition: dict[str, Any],
        kind: str,
        data: dict[str, Any],
        inner: dict[str, Any],
        ts: str,
        from_me: bool,
    ) -> None:
        """A vote, an edit or an added option as a fact about the poll message (`poll.changed`).

        The definition travels with it: a vote for a poll whose creation arrived first still
        lets the core repair a renamed question or a missing option list.
        """
        poll_id = str(poll.get("poll_id") or "").strip()
        if not poll_id:
            log(f"drop poll {kind} without a poll id in {ref['remote_id']}")
            return
        payload: dict[str, Any] = {
            "conversation_id": ref["remote_id"],
            "remote_id": ref["remote_id"],
            "poll_id": poll_id,
            "kind": kind,
            "poll": definition,
        }
        if ref["alts"]:
            payload["alt_remote_ids"] = list(ref["alts"])
        if kind == "vote":
            who = sender_only(
                data.get("sender_jid") or data.get("from") or inner.get("from")
            )
            from_lid = sender_only(data.get("from_lid") or inner.get("from_lid"))
            if from_lid and who and "@" in who:
                self._learn_lid(str(from_lid), str(who))
            if not from_me and not who:
                log(f"drop vote on {poll_id}: no voter")
                return
            if not from_me and self._self_jid and jids_same(self._canonical_person(who), self._self_jid):
                from_me = True
            payload["vote"] = {
                "voter_id": "me" if from_me else self._canonical_person(str(who)),
                "voter_name": "You"
                if from_me
                else self._person_name(self._canonical_person(str(who)), self._first_str([data, inner], ("from_name", "pushname"))),
                "from_me": bool(from_me),
                "selected": poll_selected_ids(poll, definition),
                "timestamp": ts,
            }
        self._emit("poll.changed", payload)

    def _handle_receipt(self, payload: dict[str, Any]) -> None:
        """GOWA `message.ack`: a read receipt sent by this account itself means the chat was
        read on another device (event_receipt.go: `from` is the receipt sender)."""
        data = payload.get("payload") if isinstance(payload.get("payload"), dict) else None
        if not data or not self._self_jid:
            return
        rtype = str(data.get("receipt_type") or "").lower().replace("_", "-")
        if rtype not in {"read", "read-self"}:
            return
        sender = str(data.get("from") or "")
        from_lid = str(data.get("from_lid") or "")
        mine = bool(sender) and jids_same(sender, self._self_jid)
        if not mine and from_lid:
            mapped = canonical_chat_jid(from_lid, self._lid_map)
            mine = mapped != normalize_jid(from_lid) and jids_same(mapped, self._self_jid)
        if not mine:
            return
        read_through = to_rfc3339_strict(payload.get("timestamp") or data.get("timestamp"))
        chat = data.get("chat_id") or data.get("chat_jid")
        if not read_through or not chat:
            return
        ref = self._chat_ref(chat, data.get("chat_lid"))
        if ref is None:
            return
        out: dict[str, Any] = {"remote_id": ref["remote_id"], "read_through": read_through}
        if ref["alts"]:
            out["alt_remote_ids"] = list(ref["alts"])
        self._emit("chat.read", out)

    @staticmethod
    def _first_str(sources: list[dict[str, Any]], keys: tuple[str, ...]) -> Optional[str]:
        for src in sources:
            for k in keys:
                v = src.get(k)
                if isinstance(v, str) and v.strip():
                    return v.strip()
        return None

    def _emit_incoming(self, data: dict[str, Any]) -> None:
        inner = data.get("message") if isinstance(data.get("message"), dict) else data
        if not isinstance(inner, dict):
            inner = data
        info = _as_dict(data.get("info"))
        text = nested_text(inner) or nested_text(data)
        if isinstance(data.get("body"), str) and data["body"].strip():
            text = data["body"]
        from_me = bool(
            data.get("from_me")
            or data.get("is_from_me")
            or inner.get("is_from_me")
            or inner.get("from_me")
            or info.get("IsFromMe")
            or info.get("is_from_me")
        )
        data_key = _as_dict(data.get("key"))
        inner_key = _as_dict(inner.get("key"))
        chat_raw = str(
            data.get("chat_id")
            or data.get("chat_jid")
            or inner.get("chat_jid")
            or inner.get("chat_id")
            or info.get("Chat")
            or info.get("chat")
            or data_key.get("remoteJID")
            or data_key.get("remoteJid")
            or inner_key.get("remoteJID")
            or inner_key.get("remoteJid")
            or (data.get("to") if from_me else None)
            or (inner.get("to") if from_me else None)
            or (data.get("recipient") if from_me else None)
            or data.get("from")
            or inner.get("from")
            or ""
        )
        if not chat_raw:
            return
        msg_id = (
            data.get("id")
            or inner.get("id")
            or inner.get("message_id")
            or data_key.get("ID")
            or data_key.get("id")
            or inner_key.get("ID")
            or inner_key.get("id")
            or info.get("ID")
            or info.get("id")
        )
        msg_id = str(msg_id).strip() if msg_id else ""
        ts = to_rfc3339_strict(
            data.get("timestamp")
            or inner.get("timestamp")
            or data.get("messageTimestamp")
            or inner.get("messageTimestamp")
        )
        push = self._first_str(
            [data, inner], ("from_name", "pushname", "push_name", "notify_name", "pushName")
        )
        if is_status_jid(normalize_jid(chat_raw)):
            self._emit_status_upsert(data, inner, text, from_me, msg_id, ts, push)
            return
        if not msg_id:
            log(f"drop live message without id in {chat_raw}")
            return
        if not ts:
            log(f"drop live message {msg_id} in {chat_raw}: missing/invalid timestamp")
            return
        lid_hint = data.get("chat_lid") or inner.get("chat_lid")
        ref = self._chat_ref(chat_raw, lid_hint)
        if ref is None:
            return
        from_lid = sender_only(data.get("from_lid") or inner.get("from_lid"))
        sender_phone = sender_only(data.get("from") or inner.get("from"))
        if from_lid and sender_phone and "@" in sender_phone:
            # A 1:1 chat's sender is the chat itself: learn LID <-> phone from the pair.
            self._learn_lid(str(from_lid), str(sender_phone))
            if ref["remote_id"].endswith("@lid"):
                ref = self._chat_ref(chat_raw, lid_hint) or ref
        if not self._contacts_loaded and self.client:
            self._refresh_contacts()
        poll = _as_dict(data.get("poll")) or _as_dict(inner.get("poll"))
        poll_kind = str(poll.get("type") or "").strip() if poll else ""
        poll_def = poll_definition(poll) if poll else {}
        if poll and poll_kind and poll_kind != "creation":
            self._emit_poll_change(ref, poll, poll_def, poll_kind, data, inner, ts, from_me)
            return
        is_group = self._ctype(ref["remote_id"]) != "direct"
        verified = None if (is_group or from_me) else self._first_str(
            [data, inner], ("verified_name",)
        )
        names = self._compose_names(
            ref,
            push=None if (is_group or from_me) else push,
            verified=verified,
        )
        title = choose_title(
            ref["remote_id"],
            names,
            is_self=self._is_self(ref),
            self_name=self._self_name,
            is_group=is_group,
        )
        sender_name = resolve_sender_name(
            inner, data, ref["remote_id"], str(title), self._contacts, self._self_aliases
        )
        merged = dict(inner)
        merged.update(data)
        merged["text"] = text
        merged["content"] = text
        text, extra = message_body_and_media(merged)
        if poll_kind == "creation":
            extra["media_type"] = "poll"
            if poll_def.get("options"):
                extra["poll"] = poll_def
            text = str(poll_def.get("question") or text)
        mentions = self._mention_facts(text, mentioned_jids_in(data) or mentioned_jids_in(inner))
        if mentions:
            extra["mentions"] = mentions
        reply = self._reply_fact(reply_to_from_row(data, inner))
        if reply:
            extra["reply_to"] = reply
        if poll_kind == "creation":
            preview = f"Poll: {poll_def['question']}" if poll_def.get("question") else "Poll"
        else:
            preview = preview_for_message(text or "", extra)
        self._emit_conversation(
            ref, names, last_message_at=ts, preview=preview, history=False
        )
        self._emit_message(
            ref,
            {
                "id": msg_id,
                "sender_id": sender_only(
                    data.get("sender_jid")
                    or inner.get("sender_jid")
                    or data.get("from")
                    or info.get("Sender")
                    or info.get("sender")
                )
                or None,
                "sender_name": sender_name,
                "text": text or "",
                "preview": preview,
                "timestamp": ts,
                "from_me": from_me,
                "extra": extra,
            },
            False,
        )

    def _emit_status_upsert(
        self,
        data: dict[str, Any],
        inner: dict[str, Any],
        text: str,
        from_me: bool,
        msg_id: str,
        ts: Optional[str],
        push: Optional[str],
    ) -> None:
        if not ts:
            log("drop status post: missing/invalid timestamp")
            return
        sender = canonical_chat_jid(
            str(data.get("sender_jid") or inner.get("sender_jid") or data.get("from") or ""),
            self._lid_map,
        )
        media_type = inner.get("media_type") or data.get("media_type") or "text"
        live_post: dict[str, Any] = {
            "id": msg_id,
            "media_type": str(media_type or "text"),
            "text": (text or "")[:500],
            "timestamp": ts,
        }
        raw = status_thumbnail_bytes(inner)
        thumb = save_thumbnail(self.account_id, msg_id, raw) if raw else None
        if thumb is None:
            thumb = self._thumbnail_for({"id": msg_id, "media_type": str(media_type or "")})
        if thumb:
            live_post["thumbnail_path"] = str(thumb)
        card = status_link_card(self.account_id, msg_id, status_link_preview(inner))
        if card:
            live_post["link_preview"] = card
        self._emit(
            "status.feed",
            {
                "upsert": {
                    "sender_id": sender or "status@broadcast",
                    "sender_name": push
                    or inner.get("sender_display_name")
                    or jid_user_part(sender or "status@broadcast"),
                    "timestamp": ts,
                    "media_type": media_type,
                    "preview": (text or "")[:160],
                    "from_me": from_me,
                    "count": 1,
                    "posts": [live_post] if msg_id else [],
                }
            },
        )

    # ------------------------------------------------------------------ sending

    def _send_reply(
        self,
        code: int,
        body: Any,
        kind: str,
        remote_id: str,
        request_id: Optional[str],
    ) -> None:
        if code == 200:
            mid = results(body).get("message_id")
            if request_id:
                send(
                    {
                        "type": "ok",
                        "account_id": self.account_id,
                        "request_id": request_id,
                        "message_id": mid,
                        "conversation_id": remote_id,
                    }
                )
            else:
                send({"type": "ok", "account_id": self.account_id, "request_id": mid})
        else:
            send_error(self.account_id, f"send {kind} failed ({code}): {body}", request_id)

    @staticmethod
    def reply_target(req: dict[str, Any]) -> Optional[str]:
        """Id of the message a send answers (`reply_to_id` in the request), or None.

        The author (`reply_to_sender`, needed for a group message) is part of the request, but
        GOWA looks the author up itself from the stored message: its send API takes the id only.
        """
        rid = req.get("reply_to_id")
        return str(rid).strip() if isinstance(rid, (str, int)) and str(rid).strip() else None

    def send_text(
        self,
        remote_id: str,
        text: str,
        request_id: Optional[str] = None,
        reply_to_id: Optional[str] = None,
        forwarded: bool = False,
    ) -> None:
        assert self.client
        phone = canonical_chat_jid(remote_id, self._lid_map)
        payload: dict[str, Any] = {"phone": phone, "message": text}
        if reply_to_id:
            payload["reply_message_id"] = reply_to_id
        if forwarded:
            payload["is_forwarded"] = True
        # `@<digits>` tokens (the UI writes them for picked people) become real mentions.
        mentions = mention_tokens(text)
        if mentions and canonical_chat_jid(remote_id, self._lid_map).endswith("@g.us"):
            payload["mentions"] = mentions
        code, body = self.client.post("/send/message", json_body=payload)
        self._send_reply(code, body, "message", remote_id, request_id)

    def send_attachment(
        self, remote_id: str, payload: dict[str, Any], request_id: Optional[str] = None
    ) -> None:
        assert self.client
        phone = canonical_chat_jid(remote_id, self._lid_map)
        kind = str(payload.get("kind") or "document").lower()
        caption = str(payload.get("caption") or payload.get("text") or "")
        filename = str(payload.get("filename") or "file")
        mime = str(payload.get("mime") or "application/octet-stream")
        raw: Optional[bytes] = None
        b64 = payload.get("data_base64")
        if isinstance(b64, str) and b64:
            raw = base64.b64decode(b64)
        path = payload.get("path")
        if raw is None and isinstance(path, str) and path:
            raw = Path(path).read_bytes()

        def missing(what: str) -> None:
            send_error(self.account_id, f"missing {what} data", request_id)

        # GOWA v9.9.0 documents `reply_message_id` for text only; media endpoints ignore the
        # extra form field, so a media reply may arrive unlinked until GOWA supports it.
        reply = self.reply_target(payload)
        send_fields: dict[str, str] = {"reply_message_id": reply} if reply else {}
        if payload.get("forwarded"):
            send_fields["is_forwarded"] = "true"

        if kind == "location":
            lat = payload.get("latitude")
            lng = payload.get("longitude")
            code, body = self.client.post(
                "/send/location",
                json_body={"phone": phone, "latitude": lat, "longitude": lng},
            )
        elif kind == "poll":
            options = payload.get("options") or []
            if not isinstance(options, list):
                options = []
            code, body = self.client.post(
                "/send/poll",
                json_body={
                    "phone": phone,
                    "question": caption or payload.get("question") or "Poll",
                    "options": [str(o) for o in options if str(o).strip()],
                    "max_answer": int(payload.get("max_answer") or 1),
                },
            )
        elif kind in {"image", "gif"}:
            if not raw:
                return missing("image")
            is_gif = (mime or "").lower() in {"image/gif", "image/webp"} or (filename or "").lower().endswith((".gif", ".webp"))
            fields = {
                "phone": phone,
                "caption": caption,
                "compress": "false" if is_gif else "true",
                **send_fields,
            }
            code, body = self.client.post_multipart(
                "/send/image", fields, {"image": (filename or "image.gif", raw, mime or "image/gif")}
            )
        elif kind == "video":
            if not raw:
                return missing("video")
            code, body = self.client.post_multipart(
                "/send/video",
                {"phone": phone, "caption": caption, **send_fields},
                {"video": (filename or "video.mp4", raw, mime or "video/mp4")},
            )
        elif kind in {"audio", "ptt"}:
            if not raw:
                return missing("audio")
            fields = {"phone": phone, "ptt": "true" if kind == "ptt" else "false", **send_fields}
            code, body = self.client.post_multipart(
                "/send/audio",
                fields,
                {"audio": (filename or "audio.ogg", raw, mime or "audio/ogg")},
            )
        elif kind == "sticker":
            if not raw:
                return missing("sticker")
            code, body = self.client.post_multipart(
                "/send/sticker",
                {"phone": phone, **send_fields},
                {"sticker": (filename or "sticker.webp", raw, mime or "image/webp")},
            )
        else:
            if not raw:
                return missing("file")
            code, body = self.client.post_multipart(
                "/send/file",
                {"phone": phone, "caption": caption, **send_fields},
                {"file": (filename or "file.bin", raw, mime)},
            )
        self._send_reply(code, body, kind, remote_id, request_id)

    def mark_read(self, remote_id: str, message_id: Optional[str] = None) -> None:
        """Send a read receipt for `message_id` (the newest inbound message). No id: no-op."""
        assert self.client
        if message_id:
            phone = canonical_chat_jid(remote_id, self._lid_map)
            code, body = self.client.post(
                f"/message/{urllib.parse.quote(str(message_id), safe='')}/read",
                json_body={"phone": phone},
            )
            if code != 200:
                log(f"mark read {code}: {body}")
        send({"type": "ok", "account_id": self.account_id, "request_id": None})

    def apply_op(
        self, remote_id: str, message_id: str, op: dict[str, Any], request_id: Optional[str]
    ) -> None:
        """React to, edit or delete a message that is on WhatsApp, then report the result as
        the same fact an incoming change would produce."""
        assert self.client
        kind = str(op.get("kind") or "")
        route = MESSAGE_OP_ROUTES.get(kind)
        ref = self._chat_ref(remote_id)
        if route is None:
            send_error(self.account_id, f"unknown message op {kind!r}", request_id)
            return
        if ref is None or not message_id:
            send_error(self.account_id, f"{kind}: missing chat or message", request_id)
            return
        body: dict[str, Any] = {"phone": canonical_chat_jid(remote_id, self._lid_map)}
        now = datetime.now(timezone.utc).isoformat()
        emoji = str(op.get("emoji") or "").strip()
        text = str(op.get("text") or "")
        if kind == "react":
            body["emoji"] = emoji
        elif kind == "edit":
            if not text.strip():
                send_error(self.account_id, "edit: empty text", request_id)
                return
            body["message"] = text
        code, resp = self.client.post(
            f"/message/{urllib.parse.quote(message_id, safe='')}/{route}", json_body=body
        )
        if code != 200:
            reason = resp.get("message") if isinstance(resp, dict) else None
            send_error(self.account_id, f"{kind} failed ({code}): {reason or resp}", request_id)
            return
        if kind == "react":
            self._emit_reactions(
                ref, [self._reaction_fact(message_id, None, True, emoji, now)], history=False
            )
        elif kind == "edit":
            self._emit_change(ref, message_id, {"kind": "edited", "text": text, "at": now})
        elif kind == "delete_for_everyone":
            self._emit_change(ref, message_id, {"kind": "revoked", "at": now})
        else:
            self._emit_change(ref, message_id, {"kind": "deleted_for_me"})
        send({"type": "ok", "account_id": self.account_id, "request_id": request_id})

    def _register_gowa_webhook(self, webhook_url: str) -> None:
        assert self.client
        code, body = self.client.patch(
            f"/devices/{self.account_id}/webhook",
            json_body={"webhook_url": webhook_url},
        )
        if code not in (200, 201):
            log(f"device webhook {code}: {body}")

    def shutdown(self) -> None:
        self.stop.set()
        if _sessions_by_account.get(self.account_id) is self:
            _sessions_by_account.pop(self.account_id, None)
        try:
            self.state.save()
        except Exception:
            pass
        # GOWA is shared across accounts in this process; stopped in main()'s finally.


SEND_REQUESTS = {"send_message", "send_attachment", "message_op"}


class AccountWorker:
    """Runs one account's requests in order on its own thread."""

    def __init__(self, account_id: str, handler: Any):
        self.account_id = account_id
        self._handler = handler
        self._queue: "queue.Queue[Optional[dict[str, Any]]]" = queue.Queue()
        self._thread = threading.Thread(
            target=self._run, daemon=True, name=f"wa-worker-{safe_dir_name(account_id)[:24]}"
        )
        self._thread.start()

    def submit(self, req: dict[str, Any]) -> None:
        self._queue.put(req)

    def close(self) -> None:
        self._queue.put(None)

    def _run(self) -> None:
        while True:
            req = self._queue.get()
            if req is None:
                return
            try:
                self._handler(req, self.account_id)
            except Exception as e:  # never let one request kill the worker
                log(f"request {req.get('type')} failed: {e}")
                rid = req.get("request_id") if req.get("type") in SEND_REQUESTS else None
                send_error(self.account_id or None, str(e), rid)


class Dispatcher:
    """The stdin loop only reads and dispatches; each account has its own worker."""

    def __init__(self, fallback_id: Optional[str] = None, session_factory: Any = None):
        self.sessions: dict[str, Any] = {}
        self.workers: dict[str, AccountWorker] = {}
        # Sends run on their own thread per account, so a long history sync or avatar backlog
        # on the main worker never holds a message back.
        self.send_workers: dict[str, AccountWorker] = {}
        self.fallback_id = fallback_id
        self.session_factory = session_factory or WhatsAppSession
        self._lock = threading.Lock()

    def _account_id(self, req: dict[str, Any]) -> Optional[str]:
        aid = req.get("account_id") or self.fallback_id
        return str(aid) if aid else None

    def _worker(self, account_id: str, lane: str = "main") -> AccountWorker:
        with self._lock:
            pool = self.send_workers if lane == "send" else self.workers
            w = pool.get(account_id)
            if w is None:
                w = AccountWorker(account_id, self.handle)
                pool[account_id] = w
            return w

    def dispatch(self, req: dict[str, Any]) -> bool:
        """Handle one request line. Returns False when the loop should end."""
        rtype = req.get("type")
        if rtype == "handshake":
            send(
                {
                    "type": "handshake_ok",
                    "connector_id": CONNECTOR_ID,
                    "version": VERSION,
                    "capabilities": CAPABILITIES,
                }
            )
            return True
        if rtype == "shutdown":
            self.shutdown_all()
            return False
        if not rtype:
            return True
        account_id = self._account_id(req)
        if not account_id:
            if rtype in {"authenticate", "connect"}:
                send_error(None, "missing account_id")
            elif rtype in SEND_REQUESTS:
                send_error(None, "missing account_id", req.get("request_id"))
            return True
        self._worker(account_id, "send" if rtype in SEND_REQUESTS else "main").submit(req)
        return True

    def shutdown_all(self) -> None:
        with self._lock:
            sessions = list(self.sessions.values())
            workers = list(self.workers.values()) + list(self.send_workers.values())
            self.sessions.clear()
            self.workers.clear()
            self.send_workers.clear()
        for w in workers:
            w.close()
        for s in sessions:
            try:
                s.shutdown()
            except Exception as e:
                log(f"shutdown: {e}")

    # Runs on the account's worker thread.
    def handle(self, req: dict[str, Any], account_id: str) -> None:
        rtype = req.get("type")
        if req.get("files_dir"):
            _account_files[account_id] = Path(req["files_dir"])
        session = self.sessions.get(account_id)
        if rtype in {"authenticate", "connect"}:
            send({"type": "status", "account_id": account_id, "status": "connecting", "identity": None})
            try:
                old = self.sessions.pop(account_id, None)
                if old:
                    old.shutdown()
                session = self.session_factory(account_id)
                self.sessions[account_id] = session
                session.connect()
            except FileNotFoundError as e:
                send_error(account_id, str(e))
            except Exception as e:
                log(f"authenticate failed: {e}")
                send_error(account_id, str(e))
        elif rtype == "sync_history":
            if session and session._connected:
                try:
                    session.start_sync(force=True)
                except Exception as e:
                    log(f"sync_history: {e}")
            send_ok(account_id)
        elif rtype == "sync_chat":
            if session and session._connected:
                try:
                    session.refresh_chat(req.get("conversation_id") or "")
                except Exception as e:
                    log(f"sync_chat: {e}")
            send_ok(account_id)
        elif rtype == "download_media":
            if session:
                session.download_media(req.get("conversation_id") or "", req.get("message_id") or "")
            else:
                send_error(account_id, "not connected")
        elif rtype == "download_status_media":
            if session:
                session.download_status_media(req.get("message_id") or "")
            else:
                send_error(account_id, "not connected")
        elif rtype == "fetch_avatar":
            if session:
                session.emit_avatar(req.get("conversation_id") or "")
            send_ok(account_id)
        elif rtype == "fetch_contact_profile":
            if session:
                session.fetch_contact_profile(req.get("conversation_id") or "")
            else:
                send_error(account_id, "not connected")
        elif rtype == "check_number":
            request_id = str(req.get("request_id") or "")
            if session:
                session.check_number(str(req.get("phone") or ""), request_id)
            else:
                send(
                    {
                        "type": "event",
                        "event": "number.checked",
                        "account_id": account_id,
                        "payload": {"request_id": request_id, "on_whatsapp": None, "error": "not connected"},
                    }
                )
        elif rtype == "create_group":
            if not session:
                send_error(account_id, "not connected")
                return
            parts = req.get("participants") or []
            if not isinstance(parts, list):
                parts = []
            session.create_group(req.get("title") or "", [str(p) for p in parts])
        elif rtype == "send_message":
            rid = req.get("request_id")
            if not session or not session.client:
                send_error(account_id, "not connected", rid)
                return
            session.send_text(
                req.get("conversation_id") or "",
                req.get("text") or "",
                rid,
                session.reply_target(req),
                bool(req.get("forwarded")),
            )
        elif rtype == "send_attachment":
            rid = req.get("request_id")
            if not session or not session.client:
                send_error(account_id, "not connected", rid)
                return
            session.send_attachment(req.get("conversation_id") or "", req, rid)
        elif rtype == "message_op":
            rid = req.get("request_id")
            if not session or not session.client:
                send_error(account_id, "not connected", rid)
                return
            op = _as_dict(req.get("op"))
            session.apply_op(
                str(req.get("conversation_id") or ""), str(req.get("message_id") or ""), op, rid
            )
        elif rtype == "mark_read":
            if session and session.client:
                session.mark_read(req.get("conversation_id") or "", req.get("message_id") or None)
            else:
                send_ok(account_id)
        elif rtype == "get_status":
            if session:
                st = session.device_status()
                logged = bool(st.get("is_logged_in"))
                send(
                    {
                        "type": "status",
                        "account_id": account_id,
                        "status": "connected" if logged else "awaiting_auth",
                        # Only a logged-in device has an identity; GOWA's device_id is just
                        # the Shuttle account id.
                        "identity": (st.get("jid") or session._self_jid) if logged else None,
                    }
                )
            else:
                send({"type": "status", "account_id": account_id, "status": "disconnected", "identity": None})
        elif rtype == "submit_auth":
            send_ok(account_id)
        elif rtype == "disconnect":
            old = self.sessions.pop(account_id, None)
            if old:
                old.shutdown()


def main() -> None:
    spawn_parent_death_watchdog()
    dispatcher = Dispatcher(os.environ.get("SHUTTLE_ACCOUNT_ID"))
    try:
        while True:
            try:
                req = read_line()
            except json.JSONDecodeError as e:
                log(f"bad request line: {e}")
                continue
            if req is None:
                break
            if not dispatcher.dispatch(req):
                break
    finally:
        dispatcher.shutdown_all()
        stop_gowa_singleton()


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        send({"type": "error", "message": str(e)})
        raise
