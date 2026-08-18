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
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))
from shuttle_ipc import to_rfc3339

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

    state_path = gowa_home() / "runtime.json"
    if state_path.exists():
        try:
            state = json.loads(state_path.read_text())
            url = state.get("url")
            password = state.get("password")
            pid = state.get("pid")
            if url and password and pid and _pid_alive(pid):
                if wait_http(url, (GOWA_USER, password), timeout=3):
                    return url, (GOWA_USER, password), None
        except Exception as e:
            log(f"stale GOWA state ignored: {e}")

    binary = find_gowa_binary()
    if binary is None:
        raise FileNotFoundError(
            "GOWA binary not found. Run ./connectors/gowa/fetch.sh or set SHUTTLE_GOWA_BIN."
        )

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
            "DB_URI": f"file:{gowa_home() / 'storages' / 'whatsapp.db'}?_foreign_keys=on",
        }
    )
    log(f"starting GOWA {binary} on {url}")
    proc = subprocess.Popen(
        [str(binary), "rest", f"--host=127.0.0.1", f"--port={port}", f"--os=Shuttle", f"--basic-auth={GOWA_USER}:{password}"],
        cwd=str(gowa_home()),
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=open(gowa_home() / "gowa.log", "ab"),
        start_new_session=True,
    )
    state_path.write_text(
        json.dumps({"url": url, "password": password, "pid": proc.pid, "port": port})
    )
    if not wait_http(url, (GOWA_USER, password), timeout=25):
        proc.kill()
        raise RuntimeError("GOWA did not become ready on loopback")
    return url, (GOWA_USER, password), proc


def _pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


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
        sock.settimeout(30)
        self.sock = sock
        self._pending = rest

    def recv_text(self) -> Optional[str]:
        assert self.sock is not None
        while True:
            opcode, payload = self._read_frame()
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
        if code == 400 and msg and "exist" in str(msg).lower():
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
        while not self.stop.is_set():
            ws = MiniWebSocket(host, port, path, self.client.auth)
            try:
                ws.connect()
                log("websocket connected")
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
                last_logged_in = logged_in
                if not logged_in and not self._connected:
                    self._refresh_qr_if_needed()
            except Exception as e:
                log(f"poll: {e}")
            self.stop.wait(4 if last_logged_in else 2)

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
        if upper in {"LOGIN_SUCCESS", "SUCCESS_LOGIN", "CONNECTED"} or "LOGIN" in upper and "SUCCESS" in upper:
            self._on_connected(results(payload) or payload)
            return
        event = str(payload.get("event") or code).lower()
        data = payload.get("payload") if isinstance(payload.get("payload"), dict) else payload
        if event in {"message", "message.received"} or "message" in data:
            self._emit_incoming(data if isinstance(data, dict) else payload)

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
        try:
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
                    title = chat.get("name") or str(jid).split("@")[0]
                    ctype = "group" if str(jid).endswith("@g.us") else "direct"
                    last_at = chat.get("last_message_time") or chat.get("updated_at")
                    send(
                        {
                            "type": "event",
                            "event": "conversation.updated",
                            "account_id": self.account_id,
                            "payload": {
                                "remote_id": jid,
                                "title": title,
                                "conversation_type": ctype,
                                "last_message_at": to_rfc3339(last_at) if last_at else None,
                                "archived": bool(chat.get("archived")),
                                "preview": chat.get("last_message") or chat.get("last_message_preview"),
                            },
                        }
                    )
                    self._sync_messages(str(jid), title)
                if len(chats) < page:
                    break
                offset += page
            self._history_done = True
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

    def _sync_messages(self, jid: str, title: str) -> None:
        assert self.client
        encoded = urllib.parse.quote(jid, safe="")
        offset = 0
        page = 80
        max_messages = 500
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
                text = row.get("content") or row.get("message") or row.get("text") or ""
                if isinstance(text, dict):
                    text = text.get("text") or text.get("conversation") or ""
                if not text:
                    media = row.get("media_type")
                    text = f"[{media}]" if media else ""
                event = "message.sent" if from_me else "message.received"
                send(
                    {
                        "type": "event",
                        "event": event,
                        "account_id": self.account_id,
                        "payload": {
                            "conversation_id": jid,
                            "remote_id": jid,
                            "history": True,
                            "message": {
                                "id": row.get("id"),
                                "sender_id": row.get("sender_jid"),
                                "sender_name": title if not from_me else "You",
                                "text": text,
                                "timestamp": to_rfc3339(row.get("timestamp") or row.get("created_at")),
                                "from_me": from_me,
                            },
                        },
                    }
                )
            if len(rows) < page:
                break
            offset += page

    def _emit_incoming(self, data: dict[str, Any]) -> None:
        inner = data.get("message") if isinstance(data.get("message"), dict) else data
        if not isinstance(inner, dict):
            inner = data
        chat_jid = (
            data.get("chat_id")
            or data.get("chat_jid")
            or inner.get("chat_jid")
            or data.get("from")
            or inner.get("from")
        )
        text = (
            inner.get("text")
            if isinstance(inner, dict)
            else None
        )
        if text is None:
            maybe = data.get("message")
            text = maybe if isinstance(maybe, str) else data.get("content") or data.get("body") or ""
        if isinstance(text, dict):
            text = text.get("text") or text.get("conversation") or ""
        from_me = bool(data.get("from_me") or inner.get("is_from_me") or inner.get("from_me"))
        sender = data.get("sender_jid") or inner.get("sender_jid") or data.get("pushname") or chat_jid
        msg_id = data.get("id") or inner.get("id") or inner.get("message_id")
        if not chat_jid:
            return
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
                        "sender_id": sender,
                        "sender_name": data.get("pushname") or sender,
                        "text": text or "",
                        "timestamp": to_rfc3339(data.get("timestamp")),
                        "from_me": from_me,
                    },
                },
            }
        )

    def send_text(self, remote_id: str, text: str) -> None:
        assert self.client
        phone = remote_id
        code, body = self.client.post("/send/message", json_body={"phone": phone, "message": text})
        if code == 200:
            mid = results(body).get("message_id")
            send({"type": "ok", "request_id": mid})
        else:
            send({"type": "error", "message": f"send failed ({code}): {body}"})

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


def main() -> None:
    session: Optional[WhatsAppSession] = None
    account_id = os.environ.get("SHUTTLE_ACCOUNT_ID")

    while True:
        req = read_line()
        if req is None:
            break
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
        elif rtype == "authenticate":
            account_id = req.get("account_id") or account_id
            if not account_id:
                send({"type": "error", "message": "missing account_id"})
                continue
            try:
                session = WhatsAppSession(account_id)
                session.connect()
            except FileNotFoundError as e:
                send({"type": "error", "message": str(e)})
            except Exception as e:
                log(f"authenticate failed: {e}")
                send({"type": "error", "message": str(e)})
        elif rtype == "connect":
            account_id = req.get("account_id") or account_id
            send({"type": "status", "account_id": account_id, "status": "connecting", "identity": None})
            if session is None and account_id:
                try:
                    session = WhatsAppSession(account_id)
                    session.connect()
                except Exception as e:
                    send({"type": "error", "message": str(e)})
        elif rtype == "sync_history":
            if session and session._connected:
                try:
                    session.sync_history()
                except Exception as e:
                    log(f"sync_history: {e}")
            send({"type": "ok", "request_id": None})
        elif rtype == "send_message":
            if not session:
                send({"type": "error", "message": "not connected"})
                continue
            session.send_text(req.get("conversation_id") or "", req.get("text") or "")
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
        elif rtype == "shutdown" or rtype == "disconnect":
            if session:
                session.shutdown()
            break


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        send({"type": "error", "message": str(e)})
        raise
