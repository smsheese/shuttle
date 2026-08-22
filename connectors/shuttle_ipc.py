#!/usr/bin/env python3
"""Shared stdin/stdout JSON protocol helpers for Shuttle sidecars."""

from __future__ import annotations

import json
import os
import stat
import sys
import time
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


def file_lock_exclusive(f) -> None:
    """Cross-platform exclusive lock on an open lock file."""
    if sys.platform == "win32":
        import msvcrt

        f.seek(0)
        msvcrt.locking(f.fileno(), msvcrt.LK_LOCK, 1)
        return
    try:
        import fcntl

        fcntl.flock(f.fileno(), fcntl.LOCK_EX)
    except ImportError:
        pass


def file_unlock(f) -> None:
    if sys.platform == "win32":
        import msvcrt

        try:
            f.seek(0)
            msvcrt.locking(f.fileno(), msvcrt.LK_UNLCK, 1)
        except OSError:
            pass
        return
    try:
        import fcntl

        fcntl.flock(f.fileno(), fcntl.LOCK_UN)
    except ImportError:
        pass


def pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def terminate_pid(pid: int) -> None:
    import signal

    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        return
    for _ in range(30):
        if not pid_alive(pid):
            return
        time.sleep(0.1)
    try:
        os.kill(pid, signal.SIGKILL)
    except OSError:
        pass


def find_processes_matching(
    *needles: str,
    binary_name: Optional[str] = None,
) -> list[int]:
    """Return PIDs whose command line contains all needles (Linux, macOS, Windows)."""
    needles_l = [n.lower() for n in needles if n]
    if not needles_l:
        return []

    if sys.platform == "linux":
        proc_root = Path("/proc")
        if not proc_root.exists():
            return []
        pids: list[int] = []
        for entry in proc_root.iterdir():
            if not entry.name.isdigit():
                continue
            try:
                raw = (entry / "cmdline").read_bytes()
            except OSError:
                continue
            cmd = raw.replace(b"\x00", b" ").decode(errors="ignore").lower()
            if all(n in cmd for n in needles_l):
                if binary_name and binary_name.lower() not in cmd:
                    continue
                pids.append(int(entry.name))
        return pids

    if sys.platform == "darwin":
        import subprocess

        try:
            out = subprocess.check_output(["ps", "-ax", "-o", "pid=,command="], text=True)
        except (OSError, subprocess.CalledProcessError):
            return []
        pids: list[int] = []
        for line in out.splitlines():
            line = line.strip()
            if not line:
                continue
            parts = line.split(None, 1)
            if len(parts) != 2:
                continue
            pid_s, cmd = parts
            cmd_l = cmd.lower()
            if all(n in cmd_l for n in needles_l):
                if binary_name and binary_name.lower() not in cmd_l:
                    continue
                try:
                    pids.append(int(pid_s))
                except ValueError:
                    continue
        return pids

    if sys.platform == "win32":
        import subprocess

        try:
            out = subprocess.check_output(
                [
                    "powershell",
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    "Get-CimInstance Win32_Process | Select-Object ProcessId,CommandLine | ConvertTo-Json -Compress",
                ],
                text=True,
            )
            data = json.loads(out)
        except (OSError, subprocess.CalledProcessError, json.JSONDecodeError):
            return []
        rows = data if isinstance(data, list) else [data]
        pids: list[int] = []
        for row in rows:
            if not isinstance(row, dict):
                continue
            cmd = str(row.get("CommandLine") or "").lower()
            if not all(n in cmd for n in needles_l):
                continue
            if binary_name and binary_name.lower() not in cmd:
                continue
            try:
                pids.append(int(row["ProcessId"]))
            except (KeyError, TypeError, ValueError):
                continue
        return pids

    return []


def child_pdeathsig() -> None:
    """Linux preexec_fn: child gets SIGTERM when its parent process dies."""
    if sys.platform != "linux":
        return
    import signal

    try:
        import ctypes

        libc = ctypes.CDLL(None, use_errno=True)
        PR_SET_PDEATHSIG = 1
        if libc.prctl(PR_SET_PDEATHSIG, int(signal.SIGTERM)) != 0:
            return
        if os.getppid() == 1:
            os.kill(os.getpid(), signal.SIGTERM)
    except Exception:
        pass


def spawn_parent_death_watchdog() -> None:
    """Exit the sidecar when Shuttle (or our parent) dies — macOS/Windows/Linux fallback."""
    import threading

    parent = os.getppid()

    def watch() -> None:
        if sys.platform == "win32":
            import ctypes

            kernel32 = ctypes.windll.kernel32
            SYNCHRONIZE = 0x00100000
            handle = kernel32.OpenProcess(SYNCHRONIZE, False, parent)
            if not handle:
                os._exit(1)
            try:
                WAIT_OBJECT_0 = 0x00000000
                while kernel32.WaitForSingleObject(handle, 1000) != WAIT_OBJECT_0:
                    pass
            finally:
                kernel32.CloseHandle(handle)
            os._exit(1)

        while True:
            time.sleep(1.0)
            if os.getppid() != parent:
                os._exit(1)

    threading.Thread(target=watch, name="shuttle-parent-watchdog", daemon=True).start()
