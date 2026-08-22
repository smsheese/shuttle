#!/usr/bin/env python3
"""WhatsApp connector: Shuttle JSON protocol on stdin/stdout, GOWA REST/WebSocket on loopback."""

from __future__ import annotations

import base64
import json
import os
import secrets
import socket
import struct
import subprocess
import sys
import concurrent.futures
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from shuttle_ipc import (
    child_pdeathsig,
    file_lock_exclusive,
    file_unlock,
    find_processes_matching,
    pid_alive,
    spawn_parent_death_watchdog,
    terminate_pid,
    to_rfc3339,
)

CONNECTOR_ID = "whatsapp"
VERSION = "1.0.0"
CAPABILITIES = ["text", "media", "read_receipts", "groups"]
GOWA_USER = "shuttle"


def log(msg: str) -> None:
    sys.stderr.write(f"[whatsapp-connector] {msg}\n")
    sys.stderr.flush()


def send(msg: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


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


def files_dir() -> Path:
    override = os.environ.get("SHUTTLE_FILES_DIR")
    if override:
        root = Path(override)
    else:
        docs = os.environ.get("XDG_DOCUMENTS_DIR")
        base = Path(docs) if docs else Path.home() / "Documents"
        account = os.environ.get("SHUTTLE_ACCOUNT_ID", "default")
        root = base / "shuttle" / account
    (root / "media").mkdir(parents=True, exist_ok=True)
    (root / "avatars").mkdir(parents=True, exist_ok=True)
    return root


def phone_digits(value: str) -> str:
    return "".join(ch for ch in str(value) if ch.isdigit())


def jids_same(a: str, b: str) -> bool:
    da = phone_digits(a)
    db = phone_digits(b)
    return bool(da) and da == db


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
            except Exception:
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

    def download(self, url: str) -> bytes:
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
        with urllib.request.urlopen(req, timeout=15) as resp:
            return resp.read()


def load_or_start_gowa() -> tuple[str, tuple[str, str], Optional[subprocess.Popen]]:
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
        log(f"starting GOWA {binary} on {url}")
        proc = subprocess.Popen(
            [
                str(binary),
                "rest",
                "--host=127.0.0.1",
                f"--port={port}",
                "--os=Shuttle",
                f"--basic-auth={GOWA_USER}:{password}",
            ],
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


def results(payload: Any) -> dict[str, Any]:
    if isinstance(payload, dict):
        r = payload.get("results")
        if isinstance(r, dict):
            return r
        return payload
    return {}


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
    if any(ch in n for ch in ("∙", "•", "·")):
        return True
    letters = [c for c in n if c.isalpha()]
    digits = [c for c in n if c.isdigit()]
    if digits and not letters:
        return True
    return False


def load_saved_contacts(client: GowaClient) -> dict[str, str]:
    code, body = client.get("/user/my/contacts")
    if code != 200:
        log(f"GET /user/my/contacts returned {code}")
        return {}
    rows = results(body).get("data") or []
    if not isinstance(rows, list):
        return {}
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


def avatar_cache_file(account_id: str, jid: str) -> Path:
    digits = phone_digits(jid) or jid_user_part(jid)
    safe = "".join(ch if ch.isalnum() else "_" for ch in digits)[:80]
    folder = files_dir() / "avatars"
    folder.mkdir(parents=True, exist_ok=True)
    return folder / f"{safe}.jpg"


def media_cache_file(message_id: str, media_type: str, filename: Optional[str] = None) -> Path:
    safe = "".join(ch if ch.isalnum() else "_" for ch in str(message_id))[:80]
    ext = Path(str(filename)).suffix if filename else _ext_for_media(media_type)
    folder = files_dir() / "media"
    folder.mkdir(parents=True, exist_ok=True)
    return folder / f"{safe}{ext}"


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


def download_gowa_media(client: GowaClient, message_id: str, phone: str) -> Optional[dict[str, Any]]:
    code, body = client.get(
        f"/message/{urllib.parse.quote(str(message_id), safe='')}/download",
        query={"phone": phone},
        timeout=25.0,
    )
    if code != 200:
        log(f"media download {message_id} -> {code}")
        return None
    r = results(body)
    file_url = r.get("file_url")
    if not file_url:
        return None
    try:
        raw = client.download(str(file_url))
    except Exception as e:
        log(f"media file fetch {message_id}: {e}")
        return None
    if not raw:
        return None
    media_type = str(r.get("media_type") or "image").lower()
    max_bytes = 12_000_000 if media_type == "document" else 2_500_000
    if len(raw) > max_bytes:
        log(f"media download {message_id} too large ({len(raw)} bytes)")
        return None
    path = media_cache_file(message_id, media_type, r.get("filename"))
    if path.is_file() and path.stat().st_size > 0:
        return {
            "media_type": media_type,
            "media_path": str(path),
            "filename": r.get("filename"),
        }
    path.write_bytes(raw)
    return {
        "media_type": media_type,
        "media_path": str(path),
        "filename": r.get("filename"),
    }


def nested_text(value: Any) -> str:
    if isinstance(value, str) and value.strip():
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
            "name",
            "displayName",
            "title",
        ):
            found = nested_text(value.get(key))
            if found:
                return found
        for nested in value.values():
            found = nested_text(nested)
            if found:
                return found
    if isinstance(value, list):
        for nested in value:
            found = nested_text(nested)
            if found:
                return found
    return ""


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
        blob = json.dumps(row).lower()
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
        elif "pollcreationmessage" in blob or "templateMessage" in json.dumps(row):
            if "poll" in blob:
                media_type = "poll"
    if media_type:
        extra["media_type"] = str(media_type)
        if row.get("filename"):
            extra["filename"] = row.get("filename")
        if row.get("file_length") is not None:
            extra["file_length"] = row.get("file_length")
        if not text.strip():
            text = f"[{media_type}]"
    if text.strip() and looks_like_jid(text.strip()) and not extra.get("media_type"):
        text = ""
    return text, extra


def load_lid_map() -> dict[str, str]:
    path = gowa_home() / "storages" / "whatsapp.db"
    if not path.is_file():
        return {}
    try:
        import sqlite3

        con = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        out: dict[str, str] = {}
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
        con.close()
        return out
    except Exception as e:
        log(f"lid map: {e}")
        return {}


def canonical_chat_jid(jid: str, lid_map: Optional[dict[str, str]] = None) -> str:
    raw = str(jid or "").strip()
    if not raw:
        return raw
    mapped = (lid_map or {}).get(raw) or (lid_map or {}).get(jid_user_part(raw))
    return mapped or raw


def history_files() -> list[Path]:
    store = gowa_home() / "storages"
    if not store.is_dir():
        return []
    files = sorted(store.glob("history-*.json"), key=lambda p: p.stat().st_mtime, reverse=True)
    return files[:8]


def parse_history_conversations() -> list[dict[str, Any]]:
    merged: dict[str, dict[str, Any]] = {}
    for path in history_files():
        try:
            payload = json.loads(path.read_text(encoding="utf-8", errors="replace"))
        except Exception:
            continue
        rows = payload.get("conversations") if isinstance(payload, dict) else None
        if not isinstance(rows, list):
            continue
        for conv in rows:
            if not isinstance(conv, dict):
                continue
            cid = str(conv.get("ID") or conv.get("id") or "")
            if not cid or cid.endswith("@broadcast") or cid.startswith("status@"):
                continue
            prev = merged.get(cid)
            if prev is None:
                merged[cid] = conv
                continue
            prev_msgs = prev.get("messages") if isinstance(prev.get("messages"), list) else []
            new_msgs = conv.get("messages") if isinstance(conv.get("messages"), list) else []
            by_id: dict[str, Any] = {}
            for wrap in list(prev_msgs) + list(new_msgs):
                if not isinstance(wrap, dict):
                    continue
                msg = wrap.get("message") if isinstance(wrap.get("message"), dict) else wrap
                key = (msg.get("key") or {}) if isinstance(msg, dict) else {}
                mid = str(key.get("ID") or key.get("id") or wrap.get("msgOrderID") or len(by_id))
                by_id[mid] = wrap
            prev["messages"] = list(by_id.values())
            for field in ("unreadCount", "name", "pnJID", "conversationTimestamp"):
                if conv.get(field) not in (None, "", 0) and not prev.get(field):
                    prev[field] = conv.get(field)
            unread = conv.get("unreadCount")
            if isinstance(unread, int) and unread > int(prev.get("unreadCount") or 0):
                prev["unreadCount"] = unread
    return list(merged.values())


def history_message_fields(wrap: dict[str, Any]) -> Optional[dict[str, Any]]:
    msg = wrap.get("message") if isinstance(wrap.get("message"), dict) else wrap
    if not isinstance(msg, dict):
        return None
    key = msg.get("key") if isinstance(msg.get("key"), dict) else {}
    inner = msg.get("message") if isinstance(msg.get("message"), dict) else {}
    text = nested_text(inner) or nested_text(msg)
    stubs = msg.get("messageStubParameters") or []
    stub_name = ""
    if isinstance(stubs, list):
        for item in stubs:
            if isinstance(item, str) and any(ch.isalpha() for ch in item):
                stub_name = item
                break
    if not text.strip() and not stub_name:
        return None
    if text.strip() and looks_like_jid(text.strip()) and not stub_name:
        return None
    ts = msg.get("messageTimestamp") or wrap.get("messageTimestamp")
    return {
        "id": key.get("ID") or key.get("id") or wrap.get("msgOrderID"),
        "from_me": bool(key.get("fromMe") or key.get("from_me")),
        "text": (text.strip() or f"[{stub_name}]") if (text.strip() or stub_name) else "",
        "timestamp": ts,
        "stub_name": stub_name,
        "sender_jid": key.get("participant") or key.get("remoteJID") or key.get("remoteJid"),
    }


def load_self_profile(client: GowaClient) -> tuple[Optional[str], Optional[str]]:
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
    name: Optional[str] = None
    if jid:
        code, body = client.get("/user/info", query={"phone": jid})
        if code == 200:
            info = results(body)
            name = info.get("verified_name") or info.get("push_name") or info.get("name")
            if isinstance(name, str):
                name = name.strip() or None
    return jid, name


def emit_contacts_synced(account_id: str, contacts: dict[str, str]) -> None:
    items: list[dict[str, str]] = []
    seen: set[str] = set()
    for jid, name in contacts.items():
        if "@" not in str(jid) or jid in seen:
            continue
        seen.add(str(jid))
        items.append({"remote_id": str(jid), "display_name": str(name)})
    send(
        {
            "type": "event",
            "event": "contacts.synced",
            "account_id": account_id,
            "payload": {"contacts": items},
        }
    )


def resolve_chat_title(
    chat: dict[str, Any],
    contacts: dict[str, str],
    self_jid: Optional[str] = None,
    self_name: Optional[str] = None,
) -> str:
    jid = str(chat.get("jid") or chat.get("id") or "")
    if self_jid and jid and jids_same(jid, self_jid):
        saved = lookup_contact_name(contacts, jid)
        if saved:
            return f"{saved} (You)"
        if self_name:
            return f"{self_name} (You)"
        return "Message yourself"
    raw = chat.get("name") or chat.get("push_name") or chat.get("pushname")
    name = str(raw).strip() if raw else ""
    saved = lookup_contact_name(contacts, jid)
    if jid.endswith("@g.us"):
        if name and not is_placeholder_name(name, jid):
            return name
        return saved or jid_user_part(jid) or jid or "Group"
    if saved:
        return saved
    if name and not is_placeholder_name(name, jid):
        return name
    return jid_user_part(jid) or jid or "Chat"


def resolve_sender_name(
    row: dict[str, Any],
    data: dict[str, Any],
    chat_jid: str,
    chat_title: str,
    contacts: dict[str, str],
) -> str:
    if row.get("is_from_me") or row.get("from_me") or data.get("from_me"):
        return "You"
    sender = row.get("sender_jid") or data.get("sender_jid") or data.get("from") or row.get("from")
    saved = lookup_contact_name(contacts, str(sender) if sender else None) or lookup_contact_name(
        contacts, chat_jid
    )
    for key in (
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
        if not is_placeholder_name(text, str(sender or chat_jid)):
            return text
    if saved:
        return saved
    if chat_jid.endswith("@g.us"):
        return jid_user_part(str(sender)) if sender else "Unknown"
    if chat_title and not is_placeholder_name(chat_title, chat_jid):
        return chat_title
    if sender:
        return jid_user_part(str(sender))
    return chat_title or "Unknown"


class WhatsAppSession:
    def __init__(self, account_id: str):
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
        self._contacts: dict[str, str] = {}
        self._self_jid: Optional[str] = None
        self._self_name: Optional[str] = None
        self._lid_map: dict[str, str] = {}

    def connect(self) -> None:
        url, auth, proc = load_or_start_gowa()
        self.gowa_proc = proc
        self.client = GowaClient(url, auth, self.account_id)
        self.ensure_device()
        status = self.device_status()
        if status.get("is_logged_in") or status.get("state") in {"logged_in", "connected"}:
            self._on_connected(status)
            return
        self.start_qr_login(start_listeners=True)

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
        if code != 200:
            send({"type": "error", "message": f"GOWA login failed ({code}): {body}"})
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
                    self._handle_ws(payload)
            except Exception as e:
                log(f"websocket: {e}")
                time.sleep(2)
            finally:
                ws.close()

    def _poll_loop(self) -> None:
        last_logged_in = False
        while not self.stop.is_set():
            try:
                status = self.device_status()
                logged_in = bool(status.get("is_logged_in") or status.get("state") in {"logged_in", "connected"})
                if logged_in and not last_logged_in:
                    self._on_connected(status)
                elif last_logged_in and not logged_in:
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
            except Exception as e:
                log(f"poll: {e}")
            # Login detection only. Live messages come from the websocket, not this loop.
            self.stop.wait(2 if not last_logged_in else 60)

    def _refresh_qr_if_needed(self) -> None:
        if time.time() - self._last_qr_at < 25:
            return
        if self.stop.is_set() or self._connected:
            return
        try:
            self.start_qr_login(start_listeners=False)
        except Exception as e:
            log(f"qr refresh: {e}")

    def _handle_ws(self, payload: dict[str, Any]) -> None:
        code = str(payload.get("code") or payload.get("type") or payload.get("event") or "")
        upper = code.upper()
        if upper in {"LOGIN_SUCCESS", "SUCCESS_LOGIN", "CONNECTED"} or ("LOGIN" in upper and "SUCCESS" in upper):
            self._on_connected(results(payload) or payload)
            return
        event = str(payload.get("event") or code).lower()
        if event in {"message.ack", "chat_presence"}:
            return
        data = payload.get("payload") if isinstance(payload.get("payload"), dict) else payload
        if not isinstance(data, dict):
            return
        if event in {"message", "message.received"} or event.startswith("message.") or data.get("chat_id") or data.get("chat_jid"):
            self._emit_incoming(data)

    def _on_connected(self, status: dict[str, Any]) -> None:
        if self._connected:
            return
        self._connected = True
        identity = (
            status.get("device_id")
            or status.get("jid")
            or status.get("phone_number")
            or status.get("id")
        )
        if self.client:
            self._self_jid, self._self_name = load_self_profile(self.client)
        send(
            {
                "type": "status",
                "account_id": self.account_id,
                "status": "connected",
                "identity": identity,
            }
        )
        send(
            {
                "type": "event",
                "event": "account.connected",
                "account_id": self.account_id,
                "payload": {"identity": identity},
            }
        )
        try:
            self.sync_history()
        except Exception as e:
            log(f"history sync: {e}")
        self._start_listeners()

    def sync_history(self, force: bool = False) -> None:
        if self._syncing or (self._history_done and not force):
            return
        assert self.client
        self._syncing = True
        send(
            {
                "type": "event",
                "event": "history.sync.started",
                "account_id": self.account_id,
                "payload": {},
            }
        )
        chat_jids: list[str] = []
        try:
            self._contacts = load_saved_contacts(self.client)
            contacts = self._contacts
            self._lid_map = load_lid_map()
            self._self_jid, self._self_name = load_self_profile(self.client)
            emit_contacts_synced(self.account_id, contacts)
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
                for idx, chat in enumerate(chats):
                    if not isinstance(chat, dict):
                        continue
                    jid = chat.get("jid") or chat.get("id")
                    if not jid:
                        continue
                    jid_s = canonical_chat_jid(str(jid), self._lid_map)
                    chat_jids.append(jid_s)
                    title = resolve_chat_title(chat, contacts, self._self_jid, self._self_name)
                    ctype = "group" if jid_s.endswith("@g.us") else "direct"
                    last_at = chat.get("last_message_time") or chat.get("updated_at") or chat.get("lastMessageTime")
                    pinned = chat.get("pinned")
                    if pinned is None:
                        pinned = chat.get("is_pinned")
                    archived = chat.get("archived")
                    if archived is None:
                        archived = chat.get("is_archived")
                    raw_preview = chat.get("last_message") or chat.get("last_message_preview")
                    preview = str(raw_preview) if raw_preview else None
                    if preview and preview.strip().startswith("[") and preview.strip().endswith("]"):
                        kind = preview.strip()[1:-1].lower()
                        preview = PREVIEW_LABELS.get(kind, preview)
                    unread = chat.get("unread_count")
                    if unread is None:
                        unread = chat.get("unreadCount")
                    payload: dict[str, Any] = {
                        "remote_id": jid_s,
                        "title": title,
                        "conversation_type": ctype,
                        "last_message_at": to_rfc3339(last_at) if last_at else None,
                        "preview": preview,
                        "history": True,
                        "force_recency": True,
                        "list_rank": offset + idx,
                    }
                    if isinstance(unread, int):
                        payload["unread_count"] = max(unread, 0)
                    if isinstance(pinned, bool):
                        payload["pinned"] = pinned
                    if isinstance(archived, bool):
                        payload["archived"] = archived
                    send(
                        {
                            "type": "event",
                            "event": "conversation.updated",
                            "account_id": self.account_id,
                            "payload": payload,
                        }
                    )
                    self._sync_messages(jid_s, title, contacts)
                if len(chats) < page:
                    break
                offset += page
            self._ingest_history_chats(contacts, chat_jids)
            self._history_done = True
            self._emit_self_conversation(contacts)
        finally:
            self._syncing = False
            send(
                {
                    "type": "event",
                    "event": "history.sync.completed",
                    "account_id": self.account_id,
                    "payload": {},
                }
            )
            if chat_jids:
                threading.Thread(
                    target=self._prefetch_avatars,
                    args=(chat_jids,),
                    daemon=True,
                    name="wa-avatars",
                ).start()

    def _ingest_history_chats(self, contacts: dict[str, str], known: list[str]) -> None:
        known_set = {canonical_chat_jid(j, self._lid_map) for j in known}
        imported = 0
        for conv in parse_history_conversations():
            raw_id = str(conv.get("ID") or "")
            pn = conv.get("pnJID")
            jid = canonical_chat_jid(str(pn or raw_id), self._lid_map)
            if not jid or jid.endswith("@broadcast") or jid.endswith("@newsletter") or jid.startswith("status@"):
                continue
            wraps = conv.get("messages") if isinstance(conv.get("messages"), list) else []
            parsed: list[dict[str, Any]] = []
            stub_name = ""
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
            title = (
                lookup_contact_name(contacts, jid)
                or stub_name
                or conv.get("name")
                or ""
            )
            if not parsed and not str(title).strip() and not stub_name:
                continue
            title = str(title).strip() or stub_name or jid_user_part(jid)
            last = parsed[-1] if parsed else None
            last_at = last.get("timestamp") if last else conv.get("conversationTimestamp")
            preview = last.get("text") if last else stub_name
            unread = conv.get("unreadCount") or 0
            try:
                unread_n = int(unread)
            except (TypeError, ValueError):
                unread_n = 0
            payload: dict[str, Any] = {
                "remote_id": jid,
                "title": str(title),
                "conversation_type": "group" if jid.endswith("@g.us") else "direct",
                "last_message_at": to_rfc3339(last_at) if last_at else None,
                "preview": preview,
                "history": True,
                "unread_count": unread_n,
            }
            send(
                {
                    "type": "event",
                    "event": "conversation.updated",
                    "account_id": self.account_id,
                    "payload": payload,
                }
            )
            imported += 1
            if jid in known_set and unread_n <= 0 and parsed:
                continue
            limit = 20 if jid in known_set else 80
            for fields in parsed[-limit:]:
                extra: dict[str, Any] = {}
                text, extra = message_body_and_media({"text": fields["text"]})
                send(
                    {
                        "type": "event",
                        "event": "message.sent" if fields["from_me"] else "message.received",
                        "account_id": self.account_id,
                        "payload": {
                            "conversation_id": jid,
                            "remote_id": jid,
                            "history": True,
                            "message": {
                                "id": fields.get("id"),
                                "sender_id": fields.get("sender_jid") or jid,
                                "sender_name": "You" if fields["from_me"] else str(title),
                                "text": text,
                                "preview": preview_for_message(text, extra),
                                "timestamp": to_rfc3339(fields.get("timestamp")),
                                "from_me": fields["from_me"],
                                **extra,
                            },
                        },
                    }
                )
            if jid not in known_set:
                known.append(jid)
                known_set.add(jid)
        log(f"history ingest: {imported} chats from WhatsApp backup")

    def _emit_self_conversation(self, contacts: dict[str, str]) -> None:
        if not self._self_jid:
            return
        title = resolve_chat_title(
            {"jid": self._self_jid}, contacts, self._self_jid, self._self_name
        )
        jids = {str(self._self_jid)}
        digits = phone_digits(self._self_jid)
        if digits:
            jids.add(f"{digits}@s.whatsapp.net")
        for jid in jids:
            send(
                {
                    "type": "event",
                    "event": "conversation.updated",
                    "account_id": self.account_id,
                    "payload": {
                        "remote_id": jid,
                        "title": title,
                        "conversation_type": "direct",
                    },
                }
            )

    def _sync_messages(
        self, jid: str, title: str, contacts: dict[str, str], max_messages: int = 500
    ) -> None:
        assert self.client
        encoded = urllib.parse.quote(jid, safe="")
        offset = 0
        page = 80
        while offset < max_messages:
            code, body = self.client.get(
                f"/chat/{encoded}/messages",
                query={"limit": page, "offset": offset},
            )
            if code != 200:
                return
            r = results(body)
            rows = r.get("data") or r.get("messages") or []
            if not isinstance(rows, list) or not rows:
                return
            # API returns newest first; emit oldest first so last_message_at advances.
            for row in reversed(rows):
                if not isinstance(row, dict):
                    continue
                from_me = bool(row.get("is_from_me") or row.get("from_me"))
                text, extra = message_body_and_media(row)
                event = "message.sent" if from_me else "message.received"
                sender_name = resolve_sender_name(row, row, jid, title, contacts)
                message: dict[str, Any] = {
                    "id": row.get("id"),
                    "sender_id": row.get("sender_jid"),
                    "sender_name": sender_name,
                    "text": text,
                    "preview": preview_for_message(text, extra),
                    "timestamp": to_rfc3339(row.get("timestamp") or row.get("created_at")),
                    "from_me": from_me,
                }
                message.update(extra)
                send(
                    {
                        "type": "event",
                        "event": event,
                        "account_id": self.account_id,
                        "payload": {
                            "conversation_id": jid,
                            "remote_id": jid,
                            "history": True,
                            "message": message,
                        },
                    }
                )
            if len(rows) < page:
                break
            offset += page

    def refresh_chat(self, jid: str) -> None:
        if not self.client:
            return
        contacts = self._contacts or load_saved_contacts(self.client)
        self._contacts = contacts
        title = resolve_chat_title({"jid": jid}, contacts, self._self_jid, self._self_name)
        self._sync_messages(jid, title, contacts, max_messages=80)

    def catch_up_recent(self) -> None:
        """One-shot pull of recent chats after a websocket reconnect. Not a poll loop."""
        if not self.client or not self._connected:
            return
        contacts = self._contacts or load_saved_contacts(self.client)
        self._contacts = contacts
        code, body = self.client.get("/chats", query={"limit": 40, "offset": 0})
        if code != 200:
            log(f"catch-up GET /chats {code}: {body}")
            return
        chats = results(body).get("data") or results(body).get("chats") or []
        if not isinstance(chats, list):
            return
        for chat in chats:
            if not isinstance(chat, dict):
                continue
            jid = chat.get("jid") or chat.get("id")
            if not jid:
                continue
            jid_s = canonical_chat_jid(str(jid), self._lid_map)
            title = resolve_chat_title(chat, contacts, self._self_jid, self._self_name)
            self._sync_messages(jid_s, title, contacts, max_messages=40)
        send(
            {
                "type": "event",
                "event": "inbox.catchup",
                "account_id": self.account_id,
                "payload": {},
            }
        )

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
        send(
            {
                "type": "event",
                "event": "avatar.updated",
                "account_id": self.account_id,
                "payload": {"remote_id": jid, "avatar_data": data},
            }
        )

    def fetch_contact_profile(self, jid: str) -> None:
        jid_s = canonical_chat_jid(jid, self._lid_map)
        profile: dict[str, Any] = {
            "username": None,
            "phone": format_intl_phone(jid_s),
            "about": None,
            "business_name": None,
        }
        if self.client and not jid_s.endswith("@g.us"):
            code, body = self.client.get("/user/info", query={"phone": jid_s})
            if code == 200:
                info = results(body)
                profile["username"] = info.get("push_name") or info.get("verified_name") or info.get("name")
                profile["about"] = info.get("status") or info.get("about")
                if isinstance(profile["username"], str):
                    profile["username"] = profile["username"].strip() or None
                if isinstance(profile["about"], str):
                    profile["about"] = profile["about"].strip() or None
            code, body = self.client.get("/user/business-profile", query={"phone": jid_s})
            if code == 200:
                biz = results(body)
                profile["business_name"] = biz.get("business_name") or biz.get("name")
                if isinstance(profile["business_name"], str):
                    profile["business_name"] = profile["business_name"].strip() or None
                if not profile["about"]:
                    profile["about"] = biz.get("description") or biz.get("status")
        send(
            {
                "type": "contact_profile",
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
            send(
                {
                    "type": "event",
                    "event": "media.downloaded",
                    "account_id": self.account_id,
                    "payload": payload,
                }
            )
            return
        got = download_gowa_media(self.client, message_id, jid)
        if got:
            payload.update(got)
        else:
            payload["error"] = "download failed"
        send(
            {
                "type": "event",
                "event": "media.downloaded",
                "account_id": self.account_id,
                "payload": payload,
            }
        )

    def create_group(self, title: str, participants: list[str]) -> None:
        if not self.client:
            send({"type": "error", "message": "not connected"})
            return
        phones: list[str] = []
        for raw in participants:
            p = str(raw).strip()
            if not p:
                continue
            phones.append(p)
        if not title.strip() or not phones:
            send({"type": "error", "message": "group needs a title and at least one participant"})
            return
        code, body = self.client.post(
            "/group", json_body={"title": title.strip(), "participants": phones}
        )
        if code != 200:
            send({"type": "error", "message": f"create group failed ({code}): {body}"})
            return
        gid = results(body).get("group_id")
        if gid:
            send(
                {
                    "type": "event",
                    "event": "conversation.updated",
                    "account_id": self.account_id,
                    "payload": {
                        "remote_id": gid,
                        "title": title.strip(),
                        "conversation_type": "group",
                    },
                }
            )
        send({"type": "ok", "request_id": gid})

    def _emit_incoming(self, data: dict[str, Any]) -> None:
        inner = data.get("message") if isinstance(data.get("message"), dict) else data
        if not isinstance(inner, dict):
            inner = data
        text = nested_text(inner) or nested_text(data)
        from_me = bool(data.get("from_me") or inner.get("is_from_me") or inner.get("from_me"))
        chat_jid = str(
            data.get("chat_id")
            or data.get("chat_jid")
            or inner.get("chat_jid")
            or data.get("from")
            or inner.get("from")
            or ""
        )
        chat_jid = canonical_chat_jid(chat_jid, self._lid_map)
        msg_id = data.get("id") or inner.get("id") or inner.get("message_id")
        if not chat_jid:
            return
        contacts = self._contacts
        if self.client and not contacts:
            contacts = load_saved_contacts(self.client)
            self._contacts = contacts
        chat_title = lookup_contact_name(contacts, chat_jid) or (
            data.get("chat_name")
            or data.get("name")
            or jid_user_part(chat_jid)
        )
        if self._self_jid and jids_same(chat_jid, self._self_jid):
            saved = lookup_contact_name(contacts, chat_jid)
            if saved:
                chat_title = f"{saved} (You)"
            elif self._self_name:
                chat_title = f"{self._self_name} (You)"
            else:
                chat_title = "Message yourself"
        elif is_placeholder_name(str(chat_title), chat_jid):
            chat_title = lookup_contact_name(contacts, chat_jid) or jid_user_part(chat_jid)
        sender_name = resolve_sender_name(inner, data, chat_jid, str(chat_title), contacts)
        merged = dict(inner)
        merged.update(data)
        merged["text"] = text
        merged["content"] = text
        text, extra = message_body_and_media(merged)
        ts = to_rfc3339(
            data.get("timestamp")
            or inner.get("timestamp")
            or data.get("messageTimestamp")
            or inner.get("messageTimestamp")
        )
        preview = preview_for_message(text or "", extra)
        send(
            {
                "type": "event",
                "event": "conversation.updated",
                "account_id": self.account_id,
                "payload": {
                    "remote_id": chat_jid,
                    "title": str(chat_title),
                    "conversation_type": "group" if chat_jid.endswith("@g.us") else "direct",
                    "last_message_at": ts,
                    "preview": preview,
                },
            }
        )
        send(
            {
                "type": "event",
                "event": "message.sent" if from_me else "message.received",
                "account_id": self.account_id,
                "payload": {
                    "conversation_id": chat_jid,
                    "remote_id": chat_jid,
                    "history": False,
                    "message": {
                        "id": msg_id,
                        "sender_id": data.get("sender_jid") or inner.get("sender_jid") or data.get("from"),
                        "sender_name": sender_name,
                        "text": text or "",
                        "preview": preview,
                        "timestamp": ts,
                        "from_me": from_me,
                        **extra,
                    },
                },
            }
        )

    def send_text(self, remote_id: str, text: str) -> None:
        assert self.client
        phone = canonical_chat_jid(remote_id, self._lid_map)
        code, body = self.client.post("/send/message", json_body={"phone": phone, "message": text})
        if code == 200:
            mid = results(body).get("message_id")
            send({"type": "ok", "request_id": mid})
        else:
            send({"type": "error", "message": f"send failed ({code}): {body}"})

    def send_attachment(self, remote_id: str, payload: dict[str, Any]) -> None:
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

        def ok(body: Any) -> None:
            mid = results(body).get("message_id")
            send({"type": "ok", "request_id": mid})

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
                send({"type": "error", "message": "missing image data"})
                return
            is_gif = (mime or "").lower() in {"image/gif", "image/webp"} or (filename or "").lower().endswith((".gif", ".webp"))
            fields = {"phone": phone, "caption": caption, "compress": "false" if is_gif else "true"}
            print(f"[send_attachment] image: phone={phone} filename={filename} mime={mime} is_gif={is_gif} raw_len={len(raw)} compress={fields['compress']}", file=sys.stderr)
            code, body = self.client.post_multipart(
                "/send/image", fields, {"image": (filename or "image.gif", raw, mime or "image/gif")}
            )
            print(f"[send_attachment] image response: code={code} body={body!r}", file=sys.stderr)
        elif kind == "video":
            if not raw:
                send({"type": "error", "message": "missing video data"})
                return
            code, body = self.client.post_multipart(
                "/send/video",
                {"phone": phone, "caption": caption},
                {"video": (filename or "video.mp4", raw, mime or "video/mp4")},
            )
        elif kind in {"audio", "ptt"}:
            if not raw:
                send({"type": "error", "message": "missing audio data"})
                return
            fields = {"phone": phone, "ptt": "true" if kind == "ptt" else "false"}
            code, body = self.client.post_multipart(
                "/send/audio",
                fields,
                {"audio": (filename or "audio.ogg", raw, mime or "audio/ogg")},
            )
        elif kind == "sticker":
            if not raw:
                send({"type": "error", "message": "missing sticker data"})
                return
            code, body = self.client.post_multipart(
                "/send/sticker",
                {"phone": phone},
                {"sticker": (filename or "sticker.webp", raw, mime or "image/webp")},
            )
        else:
            if not raw:
                send({"type": "error", "message": "missing file data"})
                return
            code, body = self.client.post_multipart(
                "/send/file",
                {"phone": phone, "caption": caption},
                {"file": (filename or "file.bin", raw, mime)},
            )
        if code == 200:
            ok(body)
        else:
            send({"type": "error", "message": f"send {kind} failed ({code}): {body}"})

    def mark_read(self, remote_id: str, message_id: Optional[str] = None) -> None:
        assert self.client
        if not message_id:
            send({"type": "ok", "request_id": None})
            return
        code, body = self.client.post(
            f"/message/{urllib.parse.quote(message_id, safe='')}/read",
            json_body={"phone": remote_id},
        )
        if code == 200:
            send({"type": "ok", "request_id": None})
        else:
            log(f"mark read {code}: {body}")
            send({"type": "ok", "request_id": None})

    def shutdown(self) -> None:
        self.stop.set()
        # GOWA is shared across accounts in this process; stopped in main()'s finally.

def main() -> None:
    spawn_parent_death_watchdog()
    sessions: dict[str, WhatsAppSession] = {}
    fallback_id = os.environ.get("SHUTTLE_ACCOUNT_ID")

    def pick(req: dict[str, Any]) -> tuple[Optional[str], Optional[WhatsAppSession]]:
        aid = req.get("account_id") or fallback_id
        aid = str(aid) if aid else None
        return aid, sessions.get(aid) if aid else None

    try:
        while True:
            req = read_line()
            if req is None:
                break
            rtype = req.get("type")
            account_id, session = pick(req)
            if rtype == "handshake":
                send(
                    {
                        "type": "handshake_ok",
                        "connector_id": CONNECTOR_ID,
                        "version": VERSION,
                        "capabilities": CAPABILITIES,
                    }
                )
            elif rtype in {"authenticate", "connect"}:
                if not account_id:
                    send({"type": "error", "message": "missing account_id"})
                    continue
                send({"type": "status", "account_id": account_id, "status": "connecting", "identity": None})
                try:
                    old = sessions.pop(account_id, None)
                    if old:
                        old.shutdown()
                    session = WhatsAppSession(account_id)
                    sessions[account_id] = session
                    session.connect()
                except FileNotFoundError as e:
                    send({"type": "error", "message": str(e), "account_id": account_id})
                except Exception as e:
                    log(f"authenticate failed: {e}")
                    send({"type": "error", "message": str(e), "account_id": account_id})
            elif rtype == "sync_history":
                if session and session._connected:
                    try:
                        session.sync_history(force=True)
                    except Exception as e:
                        log(f"sync_history: {e}")
                send({"type": "ok", "request_id": None})
            elif rtype == "sync_chat":
                if session and session._connected:
                    try:
                        session.refresh_chat(req.get("conversation_id") or "")
                    except Exception as e:
                        log(f"sync_chat: {e}")
                send({"type": "ok", "request_id": None})
            elif rtype == "download_media":
                if session:
                    session.download_media(req.get("conversation_id") or "", req.get("message_id") or "")
                else:
                    send({"type": "error", "message": "not connected", "account_id": account_id})
            elif rtype == "fetch_avatar":
                if session:
                    session.emit_avatar(req.get("conversation_id") or "")
                send({"type": "ok", "request_id": None})
            elif rtype == "fetch_contact_profile":
                if session:
                    session.fetch_contact_profile(req.get("conversation_id") or "")
                else:
                    send({"type": "error", "message": "not connected", "account_id": account_id})
            elif rtype == "create_group":
                if not session:
                    send({"type": "error", "message": "not connected", "account_id": account_id})
                    continue
                parts = req.get("participants") or []
                if not isinstance(parts, list):
                    parts = []
                session.create_group(req.get("title") or "", [str(p) for p in parts])
            elif rtype == "send_message":
                if not session:
                    send({"type": "error", "message": "not connected", "account_id": account_id})
                    continue
                session.send_text(req.get("conversation_id") or "", req.get("text") or "")
            elif rtype == "send_attachment":
                if not session:
                    send({"type": "error", "message": "not connected", "account_id": account_id})
                    continue
                session.send_attachment(req.get("conversation_id") or "", req)
            elif rtype == "mark_read":
                if session:
                    session.mark_read(req.get("conversation_id") or "")
                else:
                    send({"type": "ok", "request_id": None})
            elif rtype == "get_status":
                if session:
                    st = session.device_status()
                    logged = bool(st.get("is_logged_in"))
                    send(
                        {
                            "type": "status",
                            "account_id": account_id,
                            "status": "connected" if logged else "awaiting_auth",
                            "identity": st.get("device_id") or st.get("jid"),
                        }
                    )
                else:
                    send({"type": "status", "account_id": account_id, "status": "disconnected", "identity": None})
            elif rtype == "submit_auth":
                send({"type": "ok", "request_id": None})
            elif rtype == "disconnect":
                if account_id:
                    old = sessions.pop(account_id, None)
                    if old:
                        old.shutdown()
            elif rtype == "shutdown":
                for old in sessions.values():
                    old.shutdown()
                sessions.clear()
                break
    finally:
        for old in list(sessions.values()):
            old.shutdown()
        sessions.clear()
        stop_gowa_singleton()


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        send({"type": "error", "message": str(e)})
        raise
