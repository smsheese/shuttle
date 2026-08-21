#!/usr/bin/env python3
"""Instagram DM connector via instagrapi (isolated sidecar)."""

from __future__ import annotations

import os
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
    req_account_id,
    send,
    to_rfc3339,
)

CONNECTOR_ID = "instagram"
VERSION = "1.0.0"
CAPABILITIES = ["text", "media"]


class InstagramSession:
    def __init__(self, account_id: str, credentials: dict[str, Any]):
        self.account_id = account_id
        self.credentials = dict(credentials)
        self.dir = account_dir(CONNECTOR_ID, account_id)
        self.client = None
        self.stop = threading.Event()
        self._seen: set[str] = set()
        self._history_done = False
        self._syncing = False

    def start(self) -> None:
        try:
            from instagrapi import Client
            from instagrapi.exceptions import ChallengeRequired, TwoFactorRequired
        except ImportError as e:
            raise ImportError(
                "instagrapi is not installed. Run: pip install -r connectors/requirements.txt"
            ) from e

        username = self.credentials.get("username") or self.credentials.get("email")
        password = self.credentials.get("password")
        if not username or not password:
            emit_auth("password", message="Enter your Instagram username and password")
            emit_status(self.account_id, "awaiting_auth")
            return

        cl = Client()
        settings = self.dir / "session.json"
        if settings.exists():
            try:
                cl.load_settings(settings)
            except Exception as e:
                log(CONNECTOR_ID, f"session load: {e}")
        try:
            if self.credentials.get("verification_code") or self.credentials.get("code"):
                cl.login(
                    username,
                    password,
                    verification_code=str(
                        self.credentials.get("verification_code") or self.credentials.get("code")
                    ),
                )
            else:
                cl.login(username, password)
        except TwoFactorRequired:
            emit_auth("code", message="Enter the Instagram two-factor code")
            emit_status(self.account_id, "awaiting_auth")
            return
        except ChallengeRequired:
            emit_auth("code", message="Instagram sent a challenge code. Enter it to continue.")
            emit_status(self.account_id, "awaiting_auth")
            return
        cl.dump_settings(settings)
        try:
            settings.chmod(0o600)
        except OSError:
            pass
        self.client = cl
        identity = username
        try:
            me = cl.account_info()
            identity = getattr(me, "username", None) or username
        except Exception:
            pass
        emit_status(self.account_id, "connected", identity)
        emit_event(self.account_id, "account.connected", {"identity": identity})
        threading.Thread(target=self._poll, daemon=True).start()
        self._sync(initial=True)

    def _sync(self, initial: bool = False) -> None:
        assert self.client
        if initial:
            if self._syncing or self._history_done:
                return
            self._syncing = True
            emit_event(self.account_id, "history.sync.started", {})
        try:
            amount = 80 if initial else 8
            threads = self.client.direct_threads(amount=amount, thread_message_limit=20)
        except TypeError:
            try:
                threads = self.client.direct_threads(amount=80 if initial else 8)
            except Exception as e:
                log(CONNECTOR_ID, f"threads: {e}")
                if initial:
                    self._syncing = False
                    emit_event(self.account_id, "history.sync.completed", {})
                return
        except Exception as e:
            log(CONNECTOR_ID, f"threads: {e}")
            if initial:
                self._syncing = False
                emit_event(self.account_id, "history.sync.completed", {})
            return
        for thread in threads or []:
            tid = str(getattr(thread, "id", "") or "")
            title = getattr(thread, "thread_title", None) or tid
            if not tid:
                continue
            emit_event(
                self.account_id,
                "conversation.updated",
                {
                    "remote_id": tid,
                    "title": title,
                    "conversation_type": "group" if getattr(thread, "is_group", False) else "direct",
                },
            )
            items = list(getattr(thread, "messages", None) or [])
            if initial:
                try:
                    extra = self.client.direct_messages(int(tid), amount=100)
                    if extra:
                        items = list(extra)
                except Exception as e:
                    log(CONNECTOR_ID, f"messages {tid}: {e}")
            for item in reversed(items):
                self._emit_item(tid, item, history=initial)
        if initial:
            self._history_done = True
            self._syncing = False
            emit_event(self.account_id, "history.sync.completed", {})

    def _emit_item(self, tid: str, item: Any, history: bool) -> None:
        mid = str(getattr(item, "id", "") or "")
        if mid and mid in self._seen:
            return
        if mid:
            self._seen.add(mid)
        text = getattr(item, "text", None) or ""
        user_id = str(getattr(item, "user_id", "") or "")
        me = str(getattr(self.client, "user_id", "") or "")
        from_me = bool(user_id and me and user_id == me)
        emit_event(
            self.account_id,
            "message.sent" if from_me else "message.received",
            {
                "conversation_id": tid,
                "remote_id": tid,
                "history": history,
                "message": {
                    "id": mid,
                    "sender_id": user_id,
                    "text": text,
                    "timestamp": to_rfc3339(getattr(item, "timestamp", None)),
                    "from_me": from_me,
                },
            },
        )

    def _poll(self) -> None:
        while not self.stop.is_set():
            try:
                self._sync(initial=False)
            except Exception as e:
                log(CONNECTOR_ID, f"poll: {e}")
            self.stop.wait(12)

    def submit(self, credentials: dict[str, Any]) -> None:
        self.credentials.update(credentials)
        try:
            self.start()
        except Exception as e:
            emit_error(str(e))

    def send_text(self, remote_id: str, text: str) -> None:
        if not self.client:
            emit_error("not connected")
            return
        try:
            thread_id = int(remote_id)
        except ValueError:
            thread_id = remote_id
        self.client.direct_send(text, thread_ids=[thread_id])
        send({"type": "ok", "request_id": None})

    def shutdown(self) -> None:
        self.stop.set()


def main() -> None:
    sessions: dict[str, InstagramSession] = {}
    fallback_id = os.environ.get("SHUTTLE_ACCOUNT_ID")
    while True:
        req = read_line()
        if req is None:
            break
        rtype = req.get("type")
        account_id = req_account_id(req, fallback_id)
        session = sessions.get(account_id) if account_id else None
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
                emit_error("missing account_id")
                continue
            try:
                old = sessions.pop(account_id, None)
                if old:
                    old.shutdown()
                session = InstagramSession(account_id, creds(req))
                sessions[account_id] = session
                session.start()
            except Exception as e:
                log(CONNECTOR_ID, str(e))
                emit_error(str(e), account_id)
        elif rtype == "submit_auth":
            if session:
                session.submit(creds(req))
        elif rtype == "sync_history":
            if session and session.client:
                session._sync(initial=True)
            send({"type": "ok", "request_id": None})
        elif rtype == "send_message":
            if session:
                session.send_text(req.get("conversation_id") or "", req.get("text") or "")
            else:
                emit_error("not connected", account_id)
        elif rtype == "mark_read":
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


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        emit_error(str(e))
        raise
