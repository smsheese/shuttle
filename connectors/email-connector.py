#!/usr/bin/env python3
"""Email connector: IMAP (inbox + IDLE/poll) and SMTP send, TLS required."""

from __future__ import annotations

import email
import imaplib
import os
import smtplib
import ssl
import sys
import threading
from email.header import decode_header
from email.message import EmailMessage
from email.utils import formatdate, make_msgid, parseaddr, parsedate_to_datetime
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent))

from shuttle_ipc import (
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

CONNECTOR_ID = "email"
VERSION = "1.0.0"
CAPABILITIES = ["text"]


def _decode(value: Optional[str]) -> str:
    if not value:
        return ""
    parts = decode_header(value)
    out = []
    for text, enc in parts:
        if isinstance(text, bytes):
            out.append(text.decode(enc or "utf-8", errors="replace"))
        else:
            out.append(text)
    return "".join(out)


def infer_hosts(address: str) -> tuple[str, str]:
    domain = address.split("@")[-1].lower()
    presets = {
        "gmail.com": ("imap.gmail.com", "smtp.gmail.com"),
        "googlemail.com": ("imap.gmail.com", "smtp.gmail.com"),
        "outlook.com": ("outlook.office365.com", "smtp.office365.com"),
        "hotmail.com": ("outlook.office365.com", "smtp.office365.com"),
        "live.com": ("outlook.office365.com", "smtp.office365.com"),
        "yahoo.com": ("imap.mail.yahoo.com", "smtp.mail.yahoo.com"),
        "icloud.com": ("imap.mail.icloud.com", "smtp.mail.icloud.com"),
        "fastmail.com": ("imap.fastmail.com", "smtp.fastmail.com"),
    }
    if domain in presets:
        return presets[domain]
    return f"imap.{domain}", f"smtp.{domain}"


class EmailSession:
    def __init__(self, account_id: str, credentials: dict[str, Any]):
        self.account_id = account_id
        self.credentials = dict(credentials)
        self.stop = threading.Event()
        self.address = str(credentials.get("email") or credentials.get("username") or "").strip()
        self.password = str(credentials.get("password") or "")
        imap_host, smtp_host = infer_hosts(self.address) if "@" in self.address else ("", "")
        self.imap_host = str(credentials.get("imap_host") or imap_host)
        self.smtp_host = str(credentials.get("smtp_host") or smtp_host)
        self.imap_port = int(credentials.get("imap_port") or 993)
        self.smtp_port = int(credentials.get("smtp_port") or 587)
        self._history_done = False
        self._syncing = False

    def start(self) -> None:
        if not self.address or not self.password or not self.imap_host:
            emit_auth(
                "email",
                message="Enter email address, password (or app password), and IMAP/SMTP hosts if they are not guessed",
            )
            emit_status(self.account_id, "awaiting_auth")
            return
        imap = self._imap()
        imap.logout()
        emit_status(self.account_id, "connected", self.address)
        emit_event(self.account_id, "account.connected", {"identity": self.address})
        threading.Thread(target=self._poll, daemon=True).start()
        self._sync(initial=True)

    def _imap(self) -> imaplib.IMAP4_SSL:
        ctx = ssl.create_default_context()
        imap = imaplib.IMAP4_SSL(self.imap_host, self.imap_port, ssl_context=ctx)
        imap.login(self.address, self.password)
        return imap

    def _sync(self, initial: bool = False) -> None:
        if initial:
            if self._syncing or self._history_done:
                return
            self._syncing = True
            emit_event(self.account_id, "history.sync.started", {})
        try:
            imap = self._imap()
            inbox_limit = 200 if initial else 20
            self._sync_mailbox(imap, "INBOX", from_me=False, limit=inbox_limit, history=initial)
            if initial:
                for name in ("Sent", "Sent Items", "[Gmail]/Sent Mail", "INBOX.Sent"):
                    if self._sync_mailbox(imap, name, from_me=True, limit=100, history=True):
                        break
            imap.logout()
            if initial:
                self._history_done = True
        except Exception as e:
            log(CONNECTOR_ID, f"sync: {e}")
        finally:
            if initial:
                self._syncing = False
                emit_event(self.account_id, "history.sync.completed", {})

    def _sync_mailbox(
        self,
        imap: imaplib.IMAP4_SSL,
        mailbox: str,
        from_me: bool,
        limit: int,
        history: bool,
    ) -> bool:
        typ, _ = imap.select(mailbox, readonly=True)
        if typ != "OK":
            return False
        typ, data = imap.search(None, "ALL")
        if typ != "OK":
            return False
        ids = (data[0] or b"").split()[-limit:]
        for msg_id in ids:
            typ, fetched = imap.fetch(msg_id, "(RFC822)")
            if typ != "OK" or not fetched or not fetched[0]:
                continue
            raw = fetched[0][1]
            if not isinstance(raw, (bytes, bytearray)):
                continue
            self._emit_mail(email.message_from_bytes(raw), history=history, from_me=from_me)
        return True

    def _mail_ts(self, msg: email.message.Message) -> str:
        raw = msg.get("Date")
        if raw:
            try:
                dt = parsedate_to_datetime(raw)
                return to_rfc3339(dt)
            except (TypeError, ValueError, OverflowError):
                pass
        return now_iso()

    def _emit_mail(self, msg: email.message.Message, history: bool, from_me: bool = False) -> None:
        sender = parseaddr(_decode(msg.get("From")))[1] or "unknown"
        to_addr = parseaddr(_decode(msg.get("To")))[1] or sender
        remote = to_addr if from_me else sender
        subject = _decode(msg.get("Subject")) or "(no subject)"
        body = ""
        if msg.is_multipart():
            for part in msg.walk():
                if part.get_content_type() == "text/plain" and not part.get_filename():
                    payload = part.get_payload(decode=True) or b""
                    body = payload.decode(part.get_content_charset() or "utf-8", errors="replace")
                    break
        else:
            payload = msg.get_payload(decode=True) or b""
            if isinstance(payload, bytes):
                body = payload.decode(msg.get_content_charset() or "utf-8", errors="replace")
        preview = body.strip().splitlines()[0] if body.strip() else subject
        timestamp = self._mail_ts(msg)
        emit_event(
            self.account_id,
            "conversation.updated",
            {
                "remote_id": remote,
                "title": remote,
                "conversation_type": "direct",
                "preview": preview,
                "last_message_at": timestamp,
            },
        )
        emit_event(
            self.account_id,
            "message.sent" if from_me else "message.received",
            {
                "conversation_id": remote,
                "remote_id": remote,
                "history": history,
                "message": {
                    "id": msg.get("Message-ID") or timestamp,
                    "sender_id": sender,
                    "sender_name": sender,
                    "text": f"{subject}\n\n{body.strip()}".strip(),
                    "timestamp": timestamp,
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
            self.stop.wait(25)

    def submit(self, credentials: dict[str, Any]) -> None:
        self.credentials.update(credentials)
        self.address = str(self.credentials.get("email") or self.address)
        self.password = str(self.credentials.get("password") or self.password)
        if self.credentials.get("imap_host"):
            self.imap_host = str(self.credentials["imap_host"])
        if self.credentials.get("smtp_host"):
            self.smtp_host = str(self.credentials["smtp_host"])
        try:
            self.start()
        except Exception as e:
            emit_error(str(e))

    def send_text(self, remote_id: str, text: str) -> None:
        if not self.address or not self.password:
            emit_error("not connected")
            return
        msg = EmailMessage()
        msg["From"] = self.address
        msg["To"] = remote_id
        first, _, rest = text.partition("\n")
        msg["Subject"] = first[:120] if first else "Message from Shuttle"
        msg["Date"] = formatdate(localtime=True)
        msg["Message-ID"] = make_msgid()
        msg.set_content(rest.strip() or text)
        ctx = ssl.create_default_context()
        with smtplib.SMTP(self.smtp_host, self.smtp_port, timeout=20) as smtp:
            smtp.starttls(context=ctx)
            smtp.login(self.address, self.password)
            smtp.send_message(msg)
        send({"type": "ok", "request_id": None})

    def shutdown(self) -> None:
        self.stop.set()


def main() -> None:
    session: Optional[EmailSession] = None
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
                session = EmailSession(account_id, creds(req))
                session.start()
            except Exception as e:
                log(CONNECTOR_ID, str(e))
                emit_error(str(e))
        elif rtype == "submit_auth":
            if session:
                session.submit(creds(req))
        elif rtype == "sync_history":
            if session:
                session._sync(initial=True)
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
