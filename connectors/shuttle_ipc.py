#!/usr/bin/env python3
"""Shared stdin/stdout JSON protocol helpers for Shuttle sidecars."""

from __future__ import annotations

import json
import os
import stat
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional


def log(prefix: str, msg: str) -> None:
    sys.stderr.write(f"[{prefix}] {msg}\n")
    sys.stderr.flush()


def send(msg: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(msg, default=str) + "\n")
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
    secure_mkdir(p)
    return p


def account_dir(connector_id: str, account_id: str) -> Path:
    p = data_dir() / "connectors" / connector_id / account_id
    secure_mkdir(p)
    return p


def secure_mkdir(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)
    try:
        path.chmod(stat.S_IRWXU)
    except OSError:
        pass


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def to_rfc3339(value: Any) -> str:
    """Normalize API timestamps (unix seconds/ms, datetime, ISO strings) to RFC 3339."""
    if value is None or value == "":
        return now_iso()
    if isinstance(value, datetime):
        if value.tzinfo is None:
            value = value.replace(tzinfo=timezone.utc)
        return value.astimezone(timezone.utc).isoformat()
    if isinstance(value, (int, float)):
        n = float(value)
        if abs(n) > 1e12:
            n /= 1000.0
        try:
            return datetime.fromtimestamp(n, tz=timezone.utc).isoformat()
        except (OverflowError, OSError, ValueError):
            return now_iso()
    s = str(value).strip()
    if not s:
        return now_iso()
    try:
        return to_rfc3339(int(s))
    except ValueError:
        pass
    try:
        return to_rfc3339(float(s))
    except ValueError:
        return s


def emit_event(account_id: str, event: str, payload: dict[str, Any]) -> None:
    send(
        {
            "type": "event",
            "event": event,
            "account_id": account_id,
            "payload": payload,
        }
    )


def emit_status(account_id: str, status: str, identity: Optional[str] = None) -> None:
    send(
        {
            "type": "status",
            "account_id": account_id,
            "status": status,
            "identity": identity,
        }
    )


def emit_auth(
    method: str,
    qr_data: Optional[str] = None,
    url: Optional[str] = None,
    message: Optional[str] = None,
    account_id: Optional[str] = None,
) -> None:
    payload: dict[str, Any] = {
        "type": "auth_required",
        "method": method,
        "qr_data": qr_data,
        "url": url,
        "message": message,
    }
    if account_id:
        payload["account_id"] = account_id
    send(payload)


def emit_error(message: str, account_id: Optional[str] = None) -> None:
    payload: dict[str, Any] = {"type": "error", "message": message}
    if account_id:
        payload["account_id"] = account_id
    send(payload)


def req_account_id(req: dict[str, Any], fallback: Optional[str] = None) -> Optional[str]:
    aid = req.get("account_id") or fallback
    return str(aid) if aid else None


def emit_telemetry(
    event: str,
    connector_type: str,
    *,
    duration_ms: Optional[int] = None,
    items_processed: Optional[int] = None,
    errors: Optional[int] = None,
) -> None:
    """Emit a privacy-safe connector telemetry line validated by the Shuttle host."""
    payload: dict[str, Any] = {
        "type": "telemetry",
        "event": event,
        "connector_type": connector_type,
    }
    if duration_ms is not None:
        payload["duration_ms"] = int(duration_ms)
    if items_processed is not None:
        payload["items_processed"] = int(items_processed)
    if errors is not None:
        payload["errors"] = int(errors)
    send(payload)


def creds(req: dict[str, Any]) -> dict[str, Any]:
    raw = req.get("credentials") or {}
    return raw if isinstance(raw, dict) else {}
