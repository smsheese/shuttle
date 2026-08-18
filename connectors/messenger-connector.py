#!/usr/bin/env python3
"""Facebook Messenger connector via fbchat (isolated sidecar)."""

from __future__ import annotations

import os
import pickle
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

CONNECTOR_ID = "messenger"
VERSION = "1.0.0"
CAPABILITIES = ["text", "media", "groups"]


def _load_fbchat():
    try:
        import fbchat
        from fbchat import Client, ThreadType, Message

        return fbchat, Client, ThreadType, Message
    except ImportError as e:
        raise ImportError(
            "fbchat is not installed. Run ./scripts/fetch-python-runtime.sh or install connectors/requirements.txt"
        ) from e


class MessengerSession:
    def __init__(self, account_id: str, credentials: dict[str, Any]):
        self.account_id = account_id
        self.credentials = dict(credentials)
        self.dir = account_dir(CONNECTOR_ID, account_id)
        self.client = None
        self._fbchat = None
        self._history_done = False
        self._syncing = False

    def start(self) -> None:
        fbchat, Client, ThreadType, Message = _load_fbchat()
        self._fbchat = (Client, ThreadType, Message)
        email = self.credentials.get("email") or self.credentials.get("username")
        password = self.credentials.get("password")
        session_file = self.dir / "session.pkl"
        cookies = None
        if session_file.exists():
            try:
                cookies = pickle.loads(session_file.read_bytes())
            except Exception:
                cookies = None
        if not email or (not password and not cookies):
            emit_auth("password", message="Enter your Facebook email and password")
            emit_status(self.account_id, "awaiting_auth")
            return

        class ShuttleClient(Client):
            outer = self

            def onMessage(self, mid=None, author_id=None, message_object=None, thread_id=None, thread_type=None, **kwargs):
                text = getattr(message_object, "text", None) or ""
                from_me = str(author_id) == str(self.uid)
                emit_event(
                    self.outer.account_id,
                    "message.sent" if from_me else "message.received",
                    {
                        "conversation_id": str(thread_id),
                        "remote_id": str(thread_id),
                        "history": False,
                        "message": {
                            "id": str(mid),
                            "sender_id": str(author_id),
                            "text": text,
                            "timestamp": to_rfc3339(getattr(message_object, "timestamp", None)),
                            "from_me": from_me,
                        },
                    },
                )

        try:
            client = ShuttleClient(email, password or "", session_cookies=cookies)
        except Exception as e:
            msg = str(e).lower()
            if "checkpoint" in msg or "2fa" in msg or "approval" in msg:
                emit_auth("code", message="Facebook needs a login approval / 2FA code")
                emit_status(self.account_id, "awaiting_auth")
                return
            raise
        self.client = client
        try:
            session_file.write_bytes(pickle.dumps(client.getSession()))
            session_file.chmod(0o600)
        except Exception as e:
            log(CONNECTOR_ID, f"session save: {e}")
        uid = getattr(client, "uid", None)
        emit_status(self.account_id, "connected", str(uid) if uid else email)
        emit_event(self.account_id, "account.connected", {"identity": email})
        threading.Thread(target=self._listen, daemon=True).start()
        self._sync_threads()

    def _listen(self) -> None:
        try:
            self.client.listen()
        except Exception as e:
            log(CONNECTOR_ID, f"listen: {e}")

    def _sync_threads(self) -> None:
        if self._syncing or self._history_done or not self.client:
            return
        self._syncing = True
        emit_event(self.account_id, "history.sync.started", {})
        try:
            threads = self.client.fetchThreadList(limit=75)
        except Exception as e:
            log(CONNECTOR_ID, f"threads: {e}")
            emit_event(self.account_id, "history.sync.completed", {})
            self._syncing = False
            return
        _Client, ThreadType, _Message = self._fbchat
        for thread in threads or []:
            tid = getattr(thread, "uid", None)
            name = getattr(thread, "name", None) or str(tid)
            ttype = "group" if getattr(thread, "type", None) == ThreadType.GROUP else "direct"
            if not tid:
                continue
            emit_event(
                self.account_id,
                "conversation.updated",
                {
                    "remote_id": str(tid),
                    "title": name,
                    "conversation_type": ttype,
                },
            )
            self._sync_messages(str(tid))
        self._history_done = True
        self._syncing = False
        emit_event(self.account_id, "history.sync.completed", {})

    def _sync_messages(self, tid: str) -> None:
        fetched = 0
        before = None
        while fetched < 200:
            try:
                if before is None:
                    msgs = self.client.fetchThreadMessages(thread_id=tid, limit=50)
                else:
                    msgs = self.client.fetchThreadMessages(thread_id=tid, limit=50, before=before)
            except TypeError:
                try:
                    msgs = self.client.fetchThreadMessages(tid, 50)
                except Exception as e:
                    log(CONNECTOR_ID, f"messages {tid}: {e}")
                    return
            except Exception as e:
                log(CONNECTOR_ID, f"messages {tid}: {e}")
                return
            if not msgs:
                return
            for msg in reversed(list(msgs)):
                self._emit_stored_message(tid, msg)
            fetched += len(msgs)
            oldest = msgs[-1]
            before = getattr(oldest, "timestamp", None)
            if not before or len(msgs) < 20:
                return

    def _emit_stored_message(self, tid: str, msg: Any) -> None:
        mid = getattr(msg, "uid", None) or getattr(msg, "id", None)
        text = getattr(msg, "text", None) or ""
        author = getattr(msg, "author", None) or getattr(msg, "author_id", None)
        uid = str(getattr(self.client, "uid", "") or "")
        from_me = bool(uid and str(author) == uid)
        emit_event(
            self.account_id,
            "message.sent" if from_me else "message.received",
            {
                "conversation_id": str(tid),
                "remote_id": str(tid),
                "history": True,
                "message": {
                    "id": str(mid) if mid is not None else None,
                    "sender_id": str(author) if author is not None else None,
                    "text": text,
                    "timestamp": to_rfc3339(getattr(msg, "timestamp", None)),
                    "from_me": from_me,
                },
            },
        )

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
        _Client, ThreadType, Message = self._fbchat
        self.client.send(Message(text=text), thread_id=remote_id, thread_type=ThreadType.USER)
        send({"type": "ok", "request_id": None})

    def shutdown(self) -> None:
        try:
            if self.client:
                self.client.stopListening()
        except Exception:
            pass


def main() -> None:
    session: Optional[MessengerSession] = None
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
                session = MessengerSession(account_id, creds(req))
                session.start()
            except Exception as e:
                log(CONNECTOR_ID, str(e))
                emit_error(str(e))
        elif rtype == "submit_auth":
            if session:
                session.submit(creds(req))
        elif rtype == "sync_history":
            if session:
                session._sync_threads()
            send({"type": "ok", "request_id": None})
        elif rtype == "send_message":
            if session:
                session.send_text(req.get("conversation_id") or "", req.get("text") or "")
            else:
                emit_error("not connected")
        elif rtype == "mark_read":
            send({"type": "ok", "request_id": None})
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
