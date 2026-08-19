#!/usr/bin/env python3
"""Signal connector: Shuttle JSON protocol on stdin/stdout, signal-cli JSON-RPC locally."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import threading
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

from shuttle_ipc import (
    account_dir,
    creds,
    emit_auth,
    emit_error,
    emit_event,
    emit_status,
    log,
    now_iso,
    read_line,
    send,
    to_rfc3339,
)

CONNECTOR_ID = "signal"
VERSION = "1.0.0"
CAPABILITIES = ["text", "media", "read_receipts", "groups", "calls:audio"]


def find_signal_cli() -> Optional[Path]:
    env = os.environ.get("SHUTTLE_SIGNAL_CLI")
    if env and Path(env).is_file():
        return Path(env)
    here = Path(__file__).resolve().parent
    for c in (here / "signal" / "signal-cli", here / "bin" / "signal" / "signal-cli"):
        if c.is_file():
            return c
    found = shutil.which("signal-cli")
    return Path(found) if found else None


class SignalSession:
    def __init__(self, account_id: str, credentials: dict[str, Any]):
        self.account_id = account_id
        self.credentials = dict(credentials)
        self.dir = account_dir(CONNECTOR_ID, account_id)
        self.proc: Optional[subprocess.Popen] = None
        self._id = 0
        self._lock = threading.Lock()
        self.phone = str(credentials.get("phone") or "").strip()
        self._history_done = False

    def start(self) -> None:
        binary = find_signal_cli()
        if binary is None:
            raise FileNotFoundError(
                "signal-cli not found. Run ./connectors/signal/fetch.sh or set SHUTTLE_SIGNAL_CLI."
            )
        if not self.phone:
            emit_auth("phone", message="Enter your Signal phone number including country code")
            emit_status(self.account_id, "awaiting_auth")
            return
        config = self.dir / "config"
        config.mkdir(parents=True, exist_ok=True)
        env = os.environ.copy()
        env["SIGNAL_CLI_CONFIG"] = str(config)
        # Already registered if identity keys exist.
        registered = any(config.rglob("accounts.json")) or any(
            p.name.endswith(".d") for p in config.iterdir()
        ) if config.exists() else False
        if not registered and not self.credentials.get("code"):
            self._register(binary, env)
            return
        if self.credentials.get("code") and not registered:
            self._verify(binary, env, str(self.credentials["code"]))
        self._start_rpc(binary, env)

    def _register(self, binary: Path, env: dict[str, str]) -> None:
        cmd = [str(binary), "-a", self.phone, "--config", str(self.dir / "config"), "register"]
        if self.credentials.get("captcha"):
            cmd.extend(["--captcha", str(self.credentials["captcha"])])
        result = subprocess.run(cmd, env=env, capture_output=True, text=True)
        log(CONNECTOR_ID, result.stderr[-500:] if result.stderr else result.stdout[-200:])
        if result.returncode != 0 and "captcha" in (result.stderr or "").lower():
            emit_auth(
                "captcha",
                message="Signal requires a captcha token. Get one from https://signalcaptchas.org/registration/generate.html",
            )
            emit_status(self.account_id, "awaiting_auth")
            return
        if result.returncode != 0:
            emit_error(result.stderr.strip() or "signal-cli register failed")
            return
        emit_auth("code", message="Enter the SMS verification code from Signal")
        emit_status(self.account_id, "awaiting_auth")

    def _verify(self, binary: Path, env: dict[str, str], code: str) -> None:
        result = subprocess.run(
            [str(binary), "-a", self.phone, "--config", str(self.dir / "config"), "verify", code],
            env=env,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            emit_error(result.stderr.strip() or "signal-cli verify failed")
            raise RuntimeError("verify failed")

    def _start_rpc(self, binary: Path, env: dict[str, str]) -> None:
        self.proc = subprocess.Popen(
            [str(binary), "-a", self.phone, "--config", str(self.dir / "config"), "jsonRpc"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            text=True,
            bufsize=1,
        )
        threading.Thread(target=self._read, daemon=True).start()
        emit_status(self.account_id, "connected", self.phone)
        emit_event(self.account_id, "account.connected", {"identity": self.phone})
        self._rpc("sendSyncRequest", {})
        self._rpc("listContacts", {})
        self._rpc("listGroups", {})
        self.sync_history()

    def sync_history(self) -> None:
        if self._history_done:
            return
        emit_event(self.account_id, "history.sync.started", {})
        self._rpc("listContacts", {})
        self._rpc("listGroups", {})
        try:
            self._rpc("listMessages", {"limit": 200})
        except Exception:
            pass
        self._history_done = True
        emit_event(self.account_id, "history.sync.completed", {})

    def _read(self) -> None:
        assert self.proc and self.proc.stdout
        for line in self.proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                payload = json.loads(line)
            except json.JSONDecodeError:
                continue
            self._handle_rpc(payload)

    def _handle_rpc(self, payload: dict[str, Any]) -> None:
        result = payload.get("result")
        if isinstance(result, dict):
            self._ingest_directory(result)
            messages = result.get("messages") or result.get("data")
            if isinstance(messages, list):
                for item in messages:
                    if isinstance(item, dict):
                        self._emit_incoming(item, history=True)
        elif isinstance(result, list):
            for item in result:
                if isinstance(item, dict):
                    if item.get("number") or item.get("uuid") or item.get("groupId") or item.get("id"):
                        self._ingest_directory({"contacts": [item], "groups": [item]})
                    if item.get("envelope") or item.get("dataMessage") or item.get("message"):
                        self._emit_incoming(item, history=True)
        method = payload.get("method")
        params = payload.get("params") or {}
        if not isinstance(params, dict):
            params = {}
        if method in {"receive", "receiveData"} or "envelope" in params or "dataMessage" in str(params):
            self._emit_incoming(params if isinstance(params, dict) else payload)
        elif method in {"listContacts", "listGroups"} or "contacts" in params or "groups" in params:
            self._ingest_directory(params)

    def _ingest_directory(self, data: dict[str, Any]) -> None:
        contacts = data.get("contacts") or []
        if isinstance(contacts, list):
            for c in contacts:
                if not isinstance(c, dict):
                    continue
                number = c.get("number") or c.get("uuid")
                name = c.get("name") or number
                if number:
                    emit_event(
                        self.account_id,
                        "conversation.updated",
                        {
                            "remote_id": number,
                            "title": name,
                            "conversation_type": "direct",
                        },
                    )
        groups = data.get("groups") or []
        if isinstance(groups, list):
            for g in groups:
                if not isinstance(g, dict):
                    continue
                gid = g.get("id") or g.get("groupId")
                name = g.get("name") or gid
                if gid:
                    emit_event(
                        self.account_id,
                        "conversation.updated",
                        {
                            "remote_id": str(gid),
                            "title": name,
                            "conversation_type": "group",
                        },
                    )

    def _emit_incoming(self, params: dict[str, Any], history: bool = False) -> None:
        envelope = params.get("envelope") or params
        if not isinstance(envelope, dict):
            return
        source = envelope.get("sourceNumber") or envelope.get("source") or envelope.get("sourceUuid")
        data = envelope.get("dataMessage") or {}
        sent = (envelope.get("syncMessage") or {}).get("sentMessage") or {}
        from_me = False
        if isinstance(sent, dict) and (sent.get("message") or sent.get("destination")):
            data = sent
            from_me = True
        if not isinstance(data, dict):
            data = {}
        text = data.get("message") or params.get("message") or ""
        group = (data.get("groupInfo") or {}).get("groupId")
        remote = group or data.get("destination") or source
        if not remote:
            return
        ts = envelope.get("timestamp") or data.get("timestamp")
        emit_event(
            self.account_id,
            "message.sent" if from_me else "message.received",
            {
                "conversation_id": remote,
                "remote_id": remote,
                "history": history,
                "message": {
                    "id": str(ts) if ts is not None else None,
                    "sender_id": source,
                    "sender_name": envelope.get("sourceName") or source,
                    "text": text,
                    "timestamp": to_rfc3339(ts),
                    "from_me": from_me,
                },
            },
        )

    def _rpc(self, method: str, params: dict[str, Any]) -> None:
        if not self.proc or not self.proc.stdin:
            return
        with self._lock:
            self._id += 1
            req_id = self._id
        self.proc.stdin.write(
            json.dumps({"jsonrpc": "2.0", "method": method, "params": params, "id": req_id}) + "\n"
        )
        self.proc.stdin.flush()

    def submit(self, credentials: dict[str, Any]) -> None:
        self.credentials.update(credentials)
        if credentials.get("phone"):
            self.phone = str(credentials["phone"]).strip()
        try:
            self.start()
        except Exception as e:
            emit_error(str(e))

    def send_text(self, remote_id: str, text: str) -> None:
        params: dict[str, Any] = {"message": text}
        if remote_id.startswith("+") or remote_id.isdigit():
            params["recipient"] = [remote_id]
        else:
            params["groupId"] = remote_id
        self._rpc("send", params)
        send({"type": "ok", "request_id": None})

    def fetch_contact_profile(self, remote_id: str) -> None:
        phone = remote_id if remote_id.startswith("+") else None
        if not phone and remote_id.isdigit():
            phone = f"+{remote_id}"
        profile = {
            "username": remote_id,
            "phone": phone,
            "about": None,
            "business_name": None,
        }
        send({"type": "contact_profile", "conversation_id": remote_id, "profile": profile})

    def start_call(self, remote_id: str, mode: str) -> None:
        call_id = remote_id
        offer = "video" if mode == "video" else "audio"
        try:
            self._rpc("startCall", {"recipient": remote_id, "offerType": offer})
        except Exception as e:
            emit_event(
                self.account_id,
                "call.error",
                {"call_id": call_id, "message": str(e), "conversation_id": remote_id},
            )
            return
        emit_event(
            self.account_id,
            "call.ringing",
            {
                "call_id": call_id,
                "conversation_id": remote_id,
                "direction": "outbound",
                "mode": mode,
                "status": "ringing",
            },
        )

    def accept_call(self, call_id: str) -> None:
        try:
            self._rpc("acceptCall", {"callId": call_id})
        except Exception as e:
            emit_event(self.account_id, "call.error", {"call_id": call_id, "message": str(e)})

    def reject_call(self, call_id: str) -> None:
        try:
            self._rpc("rejectCall", {"callId": call_id})
        except Exception:
            pass
        emit_event(self.account_id, "call.ended", {"call_id": call_id, "status": "rejected"})

    def hangup_call(self, call_id: str) -> None:
        try:
            self._rpc("hangupCall", {"callId": call_id})
        except Exception:
            pass
        emit_event(self.account_id, "call.ended", {"call_id": call_id, "status": "ended"})

    def shutdown(self) -> None:
        if self.proc:
            self.proc.terminate()


def main() -> None:
    session: Optional[SignalSession] = None
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
        elif rtype in {"authenticate", "connect"}:
            account_id = req.get("account_id") or account_id
            try:
                session = SignalSession(account_id, creds(req))
                session.start()
            except Exception as e:
                log(CONNECTOR_ID, str(e))
                emit_error(str(e))
        elif rtype == "submit_auth":
            if session:
                session.submit(creds(req))
        elif rtype == "sync_history":
            if session:
                session.sync_history()
            send({"type": "ok", "request_id": None})
        elif rtype == "send_message":
            if session:
                session.send_text(req.get("conversation_id") or "", req.get("text") or "")
            else:
                emit_error("not connected")
        elif rtype == "mark_read":
            send({"type": "ok", "request_id": None})
        elif rtype == "fetch_contact_profile":
            if session:
                session.fetch_contact_profile(req.get("conversation_id") or "")
            else:
                emit_error("not connected")
        elif rtype == "start_call":
            if session:
                session.start_call(req.get("conversation_id") or "", req.get("mode") or "audio")
            else:
                emit_error("not connected")
        elif rtype == "accept_call":
            if session:
                session.accept_call(req.get("call_id") or "")
        elif rtype == "reject_call":
            if session:
                session.reject_call(req.get("call_id") or "")
        elif rtype == "hangup_call":
            if session:
                session.hangup_call(req.get("call_id") or "")
        elif rtype in {"shutdown", "disconnect"}:
            if session:
                session.shutdown()
            break


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        emit_error(str(e))
        raise
