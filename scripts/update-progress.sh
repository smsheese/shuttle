#!/usr/bin/env bash
# Usage: ./scripts/update-progress.sh <piece_id> <status> [round] [winner] [gap_message] [log_message]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROGRESS="$ROOT/progress.json"
PIECE_ID="${1:?piece id}"
STATUS="${2:?status}"
ROUND="${3:-}"
WINNER="${4:-}"
GAP="${5:-}"
LOG_MSG="${6:-}"

python3 - "$PROGRESS" "$PIECE_ID" "$STATUS" "$ROUND" "$WINNER" "$GAP" "$LOG_MSG" <<'PY'
import json, sys
from datetime import datetime, timezone

path, piece_id, status, rnd, winner, gap, log_msg = sys.argv[1:8]
with open(path) as f:
    data = json.load(f)

now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
for p in data["pieces"]:
    if p["id"] == piece_id:
        p["status"] = status
        if rnd:
            p["round"] = int(rnd)
        if winner:
            p["winner"] = winner
        if gap:
            p["biggest_gap"] = gap
        break

if log_msg:
    data["log"].append({"time": now, "message": log_msg})
data["updated_at"] = now

won = sum(1 for p in data["pieces"] if p["status"] == "won")
total = len(data["pieces"])
if won == total:
    data["overall_status"] = "complete"
elif any(p["status"] == "in_progress" for p in data["pieces"]):
    data["overall_status"] = "in_progress"

with open(path, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
PY
