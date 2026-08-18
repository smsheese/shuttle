#!/usr/bin/env python3
"""Matrix connector using the public client-server API over HTTPS."""

from __future__ import annotations

import json
import os
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

from shuttle_ipc import creds, emit_auth, emit_error, emit_event, emit_status, log, now_iso, read_line, send, to_rfc3339

CONNECTOR_ID = "matrix"
VERSION = "1.0.0"
CAPABILITIES = ["text", "groups", "channels"]


class MatrixSession:
    def __init__(self, account_id: str, credentials: dict[str, Any]):
        self.account_id = account_id
        self.credentials = dict(credentials)
        self.homeserver = str(credentials.get("homeserver") or "https://matrix.org").rstrip("/")
        self.user = str(credentials.get("username") or credentials.get("user_id") or "")
        self.password = str(credentials.get("password") or "")
        self.access_token = str(credentials.get("access_token") or "")
        self.user_id = str(credentials.get("user_id") or "")
        self.device_id = str(credentials.get("device_id") or "")
        self.stop = threading.Event()
        self.since: Optional[str] = None
        self.seen_events: set[str] = set()

    def start(self) -> None:
        if not self.access_token:
            if not self.user or not self.password:
                emit_auth("password", message="Enter Matrix homeserver, username, and password")
                emit_status(self.account_id, "awaiting_auth")
                return
            self._login()
        emit_status(self.account_id, "connected", self.user_id or self.user)
        emit_event(self.account_id, "account.connected", {"identity": self.user_id or self.user})
        threading.Thread(target=self._sync_loop, daemon=True).start()
        self.sync_history()

    def _request(
        self,
        method: str,
        path: str,
        body: Optional[dict[str, Any]] = None,
        query: Optional[dict[str, Any]] = None,
    ) -> dict[str, Any]:
        url = f"{self.homeserver}{path}"
        if query:
            url += "?" + urllib.parse.urlencode({k: v for k, v in query.items() if v is not None})
        data = None
        headers = {"Content-Type": "application/json"}
        if self.access_token:
            headers["Authorization"] = f"Bearer {self.access_token}"
        if body is not None:
            data = json.dumps(body).encode()
        req = urllib.request.Request(url, data=data, headers=headers, method=method)
        with urllib.request.urlopen(req, timeout=35) as resp:
            raw = resp.read().decode() or "{}"
            return json.loads(raw)

    def _login(self) -> None:
        localpart = self.user
        if localpart.startswith("@") and ":" in localpart:
            identifier = {"type": "m.id.user", "user": localpart.split(":", 1)[0][1:]}
        else:
            identifier = {"type": "m.id.user", "user": localpart.lstrip("@")}
        payload = {
            "type": "m.login.password",
            "identifier": identifier,
            "password": self.password,
            "initial_device_display_name": "Shuttle",
        }
        try:
            data = self._request("POST", "/_matrix/client/v3/login", payload)
        except urllib.error.HTTPError as e:
            body = e.read().decode(errors="replace")
            raise RuntimeError(f"Matrix login failed: {body or e.reason}") from e
        self.access_token = str(data.get("access_token") or "")
        self.user_id = str(data.get("user_id") or self.user)
        self.device_id = str(data.get("device_id") or "")
        if not self.access_token:
            raise RuntimeError("Matrix login returned no access token")

    def submit(self, credentials: dict[str, Any]) -> None:
        self.credentials.update(credentials)
        self.homeserver = str(self.credentials.get("homeserver") or self.homeserver).rstrip("/")
        self.user = str(self.credentials.get("username") or self.credentials.get("user_id") or self.user)
        self.password = str(self.credentials.get("password") or self.password)
        if self.credentials.get("access_token"):
            self.access_token = str(self.credentials["access_token"])
        if self.credentials.get("user_id"):
            self.user_id = str(self.credentials["user_id"])
        try:
            self.start()
        except Exception as e:
            emit_error(str(e))

    def sync_history(self) -> None:
        emit_event(self.account_id, "history.sync.started", {})
        try:
            data = self._request("GET", "/_matrix/client/v3/sync", query={"timeout": 0, "full_state": "true"})
            self._consume_sync(data, history=True)
            self.since = data.get("next_batch")
        except Exception as e:
            log(CONNECTOR_ID, f"sync_history: {e}")
        finally:
            emit_event(self.account_id, "history.sync.completed", {})

    def _sync_loop(self) -> None:
        while not self.stop.is_set():
            try:
                data = self._request(
                    "GET",
                    "/_matrix/client/v3/sync",
                    query={"timeout": 30000, "since": self.since},
                )
                self._consume_sync(data, history=False)
                self.since = data.get("next_batch")
            except Exception as e:
                log(CONNECTOR_ID, f"sync loop: {e}")
                time.sleep(5)

    def _consume_sync(self, data: dict[str, Any], history: bool) -> None:
        joined = ((data.get("rooms") or {}).get("join") or {})
        for room_id, room in joined.items():
            state_events = ((room.get("state") or {}).get("events") or [])
            title = self._room_title(room_id, state_events)
            timeline = ((room.get("timeline") or {}).get("events") or [])
            if timeline:
                preview = self._event_text(timeline[-1]) or title
                last_ts = to_rfc3339((timeline[-1].get("origin_server_ts") or 0))
            else:
                preview = title
                last_ts = now_iso()
            emit_event(
                self.account_id,
                "conversation.updated",
                {
                    "remote_id": room_id,
                    "title": title,
                    "conversation_type": "group" if not self._is_direct(room_id, state_events) else "direct",
                    "preview": preview,
                    "last_message_at": last_ts,
                },
            )
            for event in timeline:
                self._emit_matrix_event(room_id, title, event, history=history)

    def _emit_matrix_event(self, room_id: str, title: str, event: dict[str, Any], history: bool) -> None:
        if event.get("type") != "m.room.message":
            return
        event_id = str(event.get("event_id") or "")
        if event_id and event_id in self.seen_events:
            return
        if event_id:
            self.seen_events.add(event_id)
        body = self._event_text(event)
        if not body:
            return
        sender = str(event.get("sender") or title)
        ts = to_rfc3339(event.get("origin_server_ts"))
        from_me = bool(self.user_id and sender == self.user_id)
        emit_event(
            self.account_id,
            "message.sent" if from_me else "message.received",
            {
                "conversation_id": room_id,
                "remote_id": room_id,
                "history": history,
                "message": {
                    "id": event_id or ts,
                    "sender_id": sender,
                    "sender_name": sender,
                    "text": body,
                    "timestamp": ts,
                    "from_me": from_me,
                },
            },
        )

    def _event_text(self, event: dict[str, Any]) -> str:
        content = event.get("content") or {}
        if content.get("msgtype") == "m.text":
            return str(content.get("body") or "")
        if content.get("msgtype") == "m.notice":
            return str(content.get("body") or "")
        if content.get("msgtype") == "m.image":
            return str(content.get("body") or "[Image]")
        if content.get("msgtype") == "m.file":
            return str(content.get("body") or "[File]")
        return str(content.get("body") or "")

    def _room_title(self, room_id: str, state_events: list[dict[str, Any]]) -> str:
        for event in state_events:
            if event.get("type") == "m.room.name":
                name = ((event.get("content") or {}).get("name") or "").strip()
                if name:
                    return name
        members = []
        for event in state_events:
            if event.get("type") == "m.room.member":
                content = event.get("content") or {}
                if content.get("membership") != "join":
                    continue
                user_id = event.get("state_key")
                if user_id and user_id != self.user_id:
                    members.append(str(user_id))
        if members:
            return ", ".join(members[:3])
        return room_id

    def _is_direct(self, _room_id: str, state_events: list[dict[str, Any]]) -> bool:
        joined = 0
        for event in state_events:
            if event.get("type") == "m.room.member" and (event.get("content") or {}).get("membership") == "join":
                joined += 1
        return joined <= 2

    def send_text(self, room_id: str, text: str) -> None:
        txn = urllib.parse.quote(str(int(time.time() * 1000)))
        self._request(
            "PUT",
            f"/_matrix/client/v3/rooms/{urllib.parse.quote(room_id, safe='')}/send/m.room.message/{txn}",
            {"msgtype": "m.text", "body": text},
        )
        send({"type": "ok", "request_id": None})

    def mark_read(self, room_id: str) -> None:
        send({"type": "ok", "request_id": None})

    def shutdown(self) -> None:
        self.stop.set()


def main() -> None:
    session: Optional[MatrixSession] = None
    account_id = os.environ.get("SHUTTLE_ACCOUNT_ID")
    while True:
        req = read_line()
        if req is None:
            break
        if not req:
            continue
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
        elif rtype in ("authenticate", "connect"):
            account_id = req.get("account_id") or account_id
            if not account_id:
                emit_error("missing account_id")
                continue
            try:
                session = MatrixSession(str(account_id), creds(req))
                session.start()
            except Exception as e:
                emit_error(str(e))
        elif rtype == "submit_auth":
            if session:
                session.submit(creds(req))
        elif rtype == "sync_history":
            if session:
                session.sync_history()
        elif rtype == "send_message":
            if session:
                try:
                    session.send_text(str(req.get("conversation_id") or ""), str(req.get("text") or ""))
                except Exception as e:
                    emit_error(str(e))
        elif rtype == "mark_read":
            if session:
                session.mark_read(str(req.get("conversation_id") or ""))
        elif rtype in ("disconnect", "shutdown"):
            if session:
                session.shutdown()
            break
        elif rtype == "get_status":
            emit_status(str(account_id or ""), "connected" if session and session.access_token else "disconnected")


if __name__ == "__main__":
    main()
