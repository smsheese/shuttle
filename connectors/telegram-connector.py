#!/usr/bin/env python3
"""Telegram connector: Shuttle JSON protocol on stdin/stdout, TDLib (tdjson) locally."""

from __future__ import annotations

import ctypes
import json
import os
import sys
import threading
import time
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

CONNECTOR_ID = "telegram"
VERSION = "1.0.0"
CAPABILITIES = ["text", "media", "read_receipts", "groups", "channels", "calls:audio"]


def find_tdjson() -> Optional[Path]:
    env = os.environ.get("SHUTTLE_TDLIB")
    candidates: list[Path] = []
    if env:
        candidates.append(Path(env))
    here = Path(__file__).resolve().parent
    for name in ("libtdjson.so", "libtdjson.dylib", "tdjson.dll", "tdjson.so"):
        candidates.append(here / "tdlib" / name)
        candidates.append(here / "bin" / "tdlib" / name)
    for c in candidates:
        if c.is_file():
            return c
    return None


class TdJson:
    def __init__(self, lib_path: Path):
        lib = ctypes.CDLL(str(lib_path))
        lib.td_create_client_id.restype = ctypes.c_int
        lib.td_send.restype = None
        lib.td_send.argtypes = [ctypes.c_int, ctypes.c_char_p]
        lib.td_receive.restype = ctypes.c_char_p
        lib.td_receive.argtypes = [ctypes.c_double]
        lib.td_execute.restype = ctypes.c_char_p
        lib.td_execute.argtypes = [ctypes.c_char_p]
        self.lib = lib
        self.client_id = lib.td_create_client_id()

    def send(self, obj: dict[str, Any]) -> None:
        self.lib.td_send(self.client_id, json.dumps(obj).encode())

    def receive(self, timeout: float = 1.0) -> Optional[dict[str, Any]]:
        raw = self.lib.td_receive(timeout)
        if not raw:
            return None
        return json.loads(raw.decode())

    def execute(self, obj: dict[str, Any]) -> Optional[dict[str, Any]]:
        raw = self.lib.td_execute(json.dumps(obj).encode())
        if not raw:
            return None
        return json.loads(raw.decode())


class TelegramSession:
    def __init__(self, account_id: str, credentials: dict[str, Any]):
        self.account_id = account_id
        self.credentials = dict(credentials)
        self.stop = threading.Event()
        self.ready = False
        self.td: Optional[TdJson] = None
        self.dir = account_dir(CONNECTOR_ID, account_id)
        self._lock = threading.Lock()
        self._history_count: dict[int, int] = {}
        self._history_requested: set[int] = set()

    def start(self) -> None:
        lib = find_tdjson()
        if lib is None:
            raise FileNotFoundError(
                "TDLib (tdjson) not found. Run ./connectors/tdlib/fetch.sh or set SHUTTLE_TDLIB."
            )
        self.td = TdJson(lib)
        threading.Thread(target=self._loop, daemon=True).start()

    def _loop(self) -> None:
        assert self.td
        while not self.stop.is_set():
            event = self.td.receive(1.0)
            if not event:
                continue
            try:
                self._handle(event)
            except Exception as e:
                log(CONNECTOR_ID, f"event error: {e}")

    def _handle(self, event: dict[str, Any]) -> None:
        extra = event.get("@type") or ""
        if extra == "updateAuthorizationState":
            self._on_auth(event.get("authorization_state") or {})
        elif extra == "updateNewMessage":
            self._on_message(event.get("message") or {})
        elif extra == "updateChatLastMessage":
            chat_id = event.get("chat_id")
            if chat_id:
                self._request_chat(int(chat_id))
        elif extra == "updateNewChat":
            chat = event.get("chat") or {}
            if isinstance(chat, dict) and chat.get("id") is not None:
                self._emit_chat(chat)
        elif extra == "chats":
            for cid in event.get("chat_ids") or []:
                try:
                    self._request_chat(int(cid))
                except (TypeError, ValueError):
                    continue
        elif extra == "chat":
            self._emit_chat(event)
        elif extra == "messages":
            self._on_history_page(event)
        elif extra == "error":
            emit_error(event.get("message") or json.dumps(event))

    def _on_auth(self, state: dict[str, Any]) -> None:
        kind = state.get("@type") or ""
        if kind == "authorizationStateWaitTdlibParameters":
            api_id = self.credentials.get("api_id") or os.environ.get("SHUTTLE_TELEGRAM_API_ID")
            api_hash = self.credentials.get("api_hash") or os.environ.get("SHUTTLE_TELEGRAM_API_HASH")
            if not api_id or not api_hash:
                emit_auth(
                    "credentials",
                    message="Telegram needs api_id and api_hash from https://my.telegram.org",
                )
                emit_status(self.account_id, "awaiting_auth")
                return
            assert self.td
            self.td.send(
                {
                    "@type": "setTdlibParameters",
                    "database_directory": str(self.dir / "td.bin"),
                    "files_directory": str(self.dir / "files"),
                    "use_file_database": True,
                    "use_chat_info_database": True,
                    "use_message_database": True,
                    "use_secret_chats": False,
                    "api_id": int(api_id),
                    "api_hash": str(api_hash),
                    "system_language_code": "en",
                    "device_model": "Shuttle",
                    "system_version": "desktop",
                    "application_version": VERSION,
                }
            )
        elif kind == "authorizationStateWaitPhoneNumber":
            phone = self.credentials.get("phone")
            if phone:
                assert self.td
                self.td.send(
                    {
                        "@type": "setAuthenticationPhoneNumber",
                        "phone_number": str(phone),
                    }
                )
            else:
                emit_auth("phone", message="Enter the phone number for this Telegram account")
                emit_status(self.account_id, "awaiting_auth")
        elif kind == "authorizationStateWaitCode":
            code = self.credentials.pop("code", None) or self.credentials.pop("verification_code", None)
            if code:
                assert self.td
                self.td.send({"@type": "checkAuthenticationCode", "code": str(code)})
            else:
                emit_auth("code", message="Enter the code Telegram sent you")
                emit_status(self.account_id, "awaiting_auth")
        elif kind == "authorizationStateWaitPassword":
            password = self.credentials.get("two_factor_password") or self.credentials.get("password")
            if password:
                assert self.td
                self.td.send({"@type": "checkAuthenticationPassword", "password": str(password)})
            else:
                emit_auth("password", message="Enter your Telegram two-step password")
                emit_status(self.account_id, "awaiting_auth")
        elif kind == "authorizationStateReady":
            self.ready = True
            emit_status(self.account_id, "connected")
            emit_event(self.account_id, "account.connected", {})
            assert self.td
            self.td.send({"@type": "getMe"})
            self.td.send(
                {
                    "@type": "loadChats",
                    "chat_list": {"@type": "chatListMain"},
                    "limit": 200,
                }
            )
            self.td.send(
                {
                    "@type": "getChats",
                    "chat_list": {"@type": "chatListMain"},
                    "limit": 200,
                }
            )
            emit_event(self.account_id, "history.sync.started", {})
            threading.Thread(target=self._finish_history_later, daemon=True).start()
        elif kind == "authorizationStateClosed":
            emit_status(self.account_id, "disconnected")

        if extra := state.get("@type"):
            log(CONNECTOR_ID, extra)

    def _finish_history_later(self) -> None:
        time.sleep(15)
        emit_event(self.account_id, "history.sync.completed", {})

    def submit(self, credentials: dict[str, Any]) -> None:
        self.credentials.update(credentials)
        if not self.td:
            return
        if credentials.get("phone"):
            self.td.send(
                {
                    "@type": "setAuthenticationPhoneNumber",
                    "phone_number": str(credentials["phone"]),
                }
            )
        if credentials.get("code") or credentials.get("verification_code"):
            self.td.send(
                {
                    "@type": "checkAuthenticationCode",
                    "code": str(credentials.get("code") or credentials.get("verification_code")),
                }
            )
        if credentials.get("two_factor_password") or (
            credentials.get("password") and not credentials.get("email")
        ):
            self.td.send(
                {
                    "@type": "checkAuthenticationPassword",
                    "password": str(
                        credentials.get("two_factor_password") or credentials.get("password")
                    ),
                }
            )
        if credentials.get("api_id") and credentials.get("api_hash"):
            # Parameters are sent from WaitTdlibParameters; nudge a dummy getAuthorizationState.
            self.td.send({"@type": "getAuthorizationState"})

    def _request_chat(self, chat_id: int) -> None:
        assert self.td
        self.td.send({"@type": "getChat", "chat_id": chat_id})

    def _emit_chat(self, chat: dict[str, Any]) -> None:
        chat_id = chat.get("id")
        if chat_id is None:
            return
        title = chat.get("title") or str(chat_id)
        ctype = "direct"
        kind = (chat.get("type") or {}).get("@type")
        if kind == "chatTypeSupergroup" and (chat.get("type") or {}).get("is_channel"):
            ctype = "channel"
        elif kind in {"chatTypeBasicGroup", "chatTypeSupergroup"}:
            ctype = "group"
        last = ((chat.get("last_message") or {}).get("content") or {}).get("text") or {}
        preview = last.get("text") if isinstance(last, dict) else None
        emit_event(
            self.account_id,
            "conversation.updated",
            {
                "remote_id": str(chat_id),
                "title": title,
                "conversation_type": ctype,
                "preview": preview,
            },
        )
        if self.td:
            cid = int(chat_id)
            if cid not in self._history_requested:
                self._history_requested.add(cid)
                self._request_history(cid, 0)

    def _request_history(self, chat_id: int, from_message_id: int) -> None:
        assert self.td
        self.td.send(
            {
                "@type": "getChatHistory",
                "chat_id": chat_id,
                "from_message_id": from_message_id,
                "offset": 0,
                "limit": 100,
                "only_local": False,
            }
        )

    def _on_history_page(self, event: dict[str, Any]) -> None:
        msgs = [m for m in (event.get("messages") or []) if isinstance(m, dict)]
        if not msgs:
            return
        chat_id = msgs[0].get("chat_id")
        for msg in reversed(msgs):
            self._on_message(msg, history=True)
        if chat_id is None:
            return
        cid = int(chat_id)
        self._history_count[cid] = self._history_count.get(cid, 0) + len(msgs)
        if len(msgs) >= 50 and self._history_count[cid] < 400:
            oldest = min(int(m.get("id") or 0) for m in msgs)
            if oldest:
                self._request_history(cid, oldest)

    def _on_message(self, msg: dict[str, Any], history: bool = False) -> None:
        chat_id = msg.get("chat_id")
        if chat_id is None:
            return
        content = msg.get("content") or {}
        text = ""
        if content.get("@type") == "messageText":
            text = ((content.get("text") or {}).get("text")) or ""
        elif content.get("@type"):
            text = f"[{content.get('@type')}]"
        outgoing = bool(msg.get("is_outgoing"))
        timestamp = to_rfc3339(msg.get("date"))
        emit_event(
            self.account_id,
            "message.sent" if outgoing else "message.received",
            {
                "conversation_id": str(chat_id),
                "remote_id": str(chat_id),
                "history": history,
                "message": {
                    "id": str(msg.get("id")),
                    "sender_id": str(((msg.get("sender_id") or {}).get("user_id")) or chat_id),
                    "text": text,
                    "timestamp": timestamp,
                    "from_me": outgoing,
                },
            },
        )

    def send_text(self, remote_id: str, text: str) -> None:
        if not self.td:
            emit_error("not connected")
            return
        try:
            chat_id = int(remote_id)
        except ValueError:
            emit_error(f"invalid telegram chat id: {remote_id}")
            return
        self.td.send(
            {
                "@type": "sendMessage",
                "chat_id": chat_id,
                "input_message_content": {
                    "@type": "inputMessageText",
                    "text": {"@type": "formattedText", "text": text},
                },
            }
        )
        send({"type": "ok", "request_id": None})

    def fetch_contact_profile(self, remote_id: str) -> None:
        profile = {
            "username": remote_id,
            "phone": None,
            "about": None,
            "business_name": None,
        }
        if self.td:
            try:
                chat_id = int(remote_id)
                self.td.send({"@type": "getChat", "chat_id": chat_id})
            except ValueError:
                pass
        send({"type": "contact_profile", "conversation_id": remote_id, "profile": profile})

    def start_call(self, remote_id: str, mode: str) -> None:
        if not self.td:
            emit_error("not connected")
            return
        try:
            chat_id = int(remote_id)
        except ValueError:
            emit_error(f"invalid telegram chat id: {remote_id}")
            return
        call_id = str(chat_id)
        self.td.send(
            {
                "@type": "createCall",
                "user_id": chat_id,
                "protocol": {
                    "@type": "callProtocol",
                    "udp_p2p": True,
                    "udp_reflector": True,
                    "min_layer": 65,
                    "max_layer": 92,
                    "library_versions": ["2.7.7"],
                },
                "is_video": mode == "video",
            }
        )
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
        if not self.td:
            return
        try:
            self.td.send({"@type": "acceptCall", "call_id": int(call_id)})
        except Exception as e:
            emit_event(self.account_id, "call.error", {"call_id": call_id, "message": str(e)})

    def reject_call(self, call_id: str) -> None:
        if not self.td:
            return
        try:
            self.td.send({"@type": "discardCall", "call_id": int(call_id), "is_disconnected": True, "duration": 0, "is_video": False, "connection_id": 0})
        except Exception:
            pass
        emit_event(self.account_id, "call.ended", {"call_id": call_id, "status": "rejected"})

    def hangup_call(self, call_id: str) -> None:
        if not self.td:
            return
        try:
            self.td.send({"@type": "discardCall", "call_id": int(call_id), "is_disconnected": True, "duration": 0, "is_video": False, "connection_id": 0})
        except Exception:
            pass
        emit_event(self.account_id, "call.ended", {"call_id": call_id, "status": "ended"})

    def shutdown(self) -> None:
        self.stop.set()
        if self.td:
            self.td.send({"@type": "close"})


def main() -> None:
    sessions: dict[str, TelegramSession] = {}
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
                session = TelegramSession(account_id, creds(req))
                sessions[account_id] = session
                session.start()
            except Exception as e:
                log(CONNECTOR_ID, str(e))
                emit_error(str(e), account_id)
        elif rtype == "submit_auth":
            if session:
                session.submit(creds(req))
            else:
                emit_error("not connected", account_id)
        elif rtype == "sync_history":
            if session and session.ready and session.td:
                session.td.send(
                    {
                        "@type": "loadChats",
                        "chat_list": {"@type": "chatListMain"},
                        "limit": 200,
                    }
                )
                session.td.send(
                    {
                        "@type": "getChats",
                        "chat_list": {"@type": "chatListMain"},
                        "limit": 200,
                    }
                )
            send({"type": "ok", "request_id": None})
        elif rtype == "send_message":
            if not session:
                emit_error("not connected", account_id)
                continue
            session.send_text(req.get("conversation_id") or "", req.get("text") or "")
        elif rtype == "mark_read":
            send({"type": "ok", "request_id": None})
        elif rtype == "fetch_contact_profile":
            if session:
                session.fetch_contact_profile(req.get("conversation_id") or "")
            else:
                emit_error("not connected", account_id)
        elif rtype == "start_call":
            if session:
                session.start_call(req.get("conversation_id") or "", req.get("mode") or "audio")
            else:
                emit_error("not connected", account_id)
        elif rtype == "accept_call":
            if session:
                session.accept_call(req.get("call_id") or "")
        elif rtype == "reject_call":
            if session:
                session.reject_call(req.get("call_id") or "")
        elif rtype == "hangup_call":
            if session:
                session.hangup_call(req.get("call_id") or "")
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
