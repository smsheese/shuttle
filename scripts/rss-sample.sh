#!/usr/bin/env bash
# rss-sample.sh — sample Shuttle + descendant process RSS (Linux /proc VmRSS)
set -euo pipefail

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  cat <<EOF
Usage: rss-sample.sh [interval_seconds] [duration_seconds] [output_file]

Samples Resident Set Size (RSS) for Shuttle and all descendant processes.

Method:
  - Linux VmRSS from /proc/<pid>/status (kB), summed across the process set
  - Process set: PIDs whose name or cmdline matches "shuttle" (excluding this
    script, rg, and grep), plus all descendants via pgrep -P recursion

Defaults: interval=120s, duration=3600s (60 min @ 2 min = 30 samples,
  same cadence as the Ferdium reference run documented in docs/roadmap.md).

If Shuttle is not running, exits with an error.
EOF
  exit 0
fi

INTERVAL="${1:-120}"
DURATION="${2:-3600}"
OUTPUT="${3:-}"

SCRIPT_PID=$$
SELF_NAME="$(basename "$0")"

usage() {
  cat <<EOF
Usage: $SELF_NAME [interval_seconds] [duration_seconds] [output_file]

Samples Resident Set Size (RSS) for Shuttle and all descendant processes.
See --help for method details.
EOF
}

if ! [[ "$INTERVAL" =~ ^[0-9]+$ && "$DURATION" =~ ^[0-9]+$ ]]; then
  echo "error: interval and duration must be positive integers (seconds)" >&2
  usage >&2
  exit 1
fi

if (( INTERVAL < 1 || DURATION < 1 )); then
  echo "error: interval and duration must be at least 1 second" >&2
  exit 1
fi

should_skip_pid() {
  local pid="$1"
  [[ "$pid" == "$SCRIPT_PID" ]] && return 0
  [[ ! -d "/proc/$pid" ]] && return 0
  local cmdline comm
  cmdline=""
  if [[ -r "/proc/$pid/cmdline" ]]; then
    cmdline="$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || true)"
  fi
  comm="$(cat "/proc/$pid/comm" 2>/dev/null || true)"
  case "$cmdline" in
    *"$SELF_NAME"*) return 0 ;;
    *"/rg "*) return 0 ;;
    *" rg "*) return 0 ;;
    rg\ *) return 0 ;;
    *"/grep "*) return 0 ;;
    *" grep "*) return 0 ;;
    grep\ *) return 0 ;;
  esac
  case "$comm" in
    rg|grep) return 0 ;;
  esac
  return 1
}

matches_shuttle() {
  local pid="$1"
  should_skip_pid "$pid" && return 1
  local comm exe base
  comm="$(cat "/proc/$pid/comm" 2>/dev/null || true)"
  [[ "$comm" == *shuttle* ]] && return 0
  exe="$(readlink -f "/proc/$pid/exe" 2>/dev/null || true)"
  base="$(basename "$exe" 2>/dev/null || true)"
  [[ "$base" == *shuttle* ]]
}

find_shuttle_roots() {
  local pid
  for pid in $(ps -eo pid= 2>/dev/null | tr -d ' '); do
    [[ -z "$pid" ]] && continue
    if matches_shuttle "$pid"; then
      echo "$pid"
    fi
  done | sort -u
}

collect_tree_pids() {
  local roots=("$@")
  local queue=()
  local seen=" "
  local pid child

  for pid in "${roots[@]}"; do
    queue+=("$pid")
  done

  while ((${#queue[@]} > 0)); do
    pid="${queue[0]}"
    queue=("${queue[@]:1}")
    case "$seen" in
      *" $pid "*) continue ;;
    esac
    seen="${seen}${pid} "
    echo "$pid"
    while IFS= read -r child; do
      [[ -n "$child" ]] && queue+=("$child")
    done < <(pgrep -P "$pid" 2>/dev/null || true)
  done
}

rss_kb_for_pid() {
  local pid="$1"
  awk '/^VmRSS:/ { print $2; exit }' "/proc/$pid/status" 2>/dev/null || echo 0
}

sample_rss() {
  local -a pids
  local pid total_kb count
  mapfile -t pids < <(collect_tree_pids "${SHUTTLE_ROOTS[@]}")
  total_kb=0
  count=0
  for pid in "${pids[@]}"; do
    local kb
    kb="$(rss_kb_for_pid "$pid")"
    total_kb=$((total_kb + kb))
    count=$((count + 1))
  done
  echo "$count $total_kb"
}

log_line() {
  if [[ -n "$OUTPUT" ]]; then
    echo "$1" >>"$OUTPUT"
  fi
  echo "$1"
}

if [[ -n "$OUTPUT" ]]; then
  : >"$OUTPUT"
fi

mapfile -t SHUTTLE_ROOTS < <(find_shuttle_roots)
if ((${#SHUTTLE_ROOTS[@]} == 0)); then
  echo "error: no shuttle process found (is Shuttle running?)" >&2
  exit 1
fi

HEADER="# Shuttle RSS sample — Linux VmRSS (/proc/<pid>/status), Shuttle + descendants"
log_line "$HEADER"
log_line "# interval=${INTERVAL}s duration=${DURATION}s started=$(date -Iseconds)"
log_line $'timestamp\tprocesses\trss_mib'

declare -a SAMPLES=()
START_EPOCH=$(date +%s)
END_EPOCH=$((START_EPOCH + DURATION))
SAMPLE_N=0

while (( $(date +%s) < END_EPOCH )); do
  read -r PROC_COUNT RSS_KB < <(sample_rss)
  RSS_MIB=$(awk -v kb="$RSS_KB" 'BEGIN { printf "%.2f", kb / 1024 }')
  TS="$(date -Iseconds)"
  log_line "$(printf '%s\t%s\t%s' "$TS" "$PROC_COUNT" "$RSS_MIB")"
  SAMPLES+=("$RSS_MIB")
  SAMPLE_N=$((SAMPLE_N + 1))

  NOW=$(date +%s)
  REMAINING=$((END_EPOCH - NOW))
  if (( REMAINING <= 0 )); then
    break
  fi
  SLEEP_FOR=$INTERVAL
  if (( SLEEP_FOR > REMAINING )); then
    SLEEP_FOR=$REMAINING
  fi
  sleep "$SLEEP_FOR"
done

if (( SAMPLE_N == 0 )); then
  echo "error: no samples collected" >&2
  exit 1
fi

STATS=$(
  printf '%s\n' "${SAMPLES[@]}" | awk '
    {
      vals[NR] = $1
      sum += $1
      if (NR == 1 || $1 < min) min = $1
      if (NR == 1 || $1 > max) max = $1
    }
    END {
      n = NR
      for (i = 1; i <= n; i++) {
        for (j = i + 1; j <= n; j++) {
          if (vals[i] > vals[j]) {
            t = vals[i]; vals[i] = vals[j]; vals[j] = t
          }
        }
      }
      if (n % 2 == 1) {
        median = vals[(n + 1) / 2]
      } else {
        median = (vals[n / 2] + vals[n / 2 + 1]) / 2
      }
      printf "samples=%d min_mib=%.2f avg_mib=%.2f median_mib=%.2f max_mib=%.2f\n", n, min, sum / n, median, max
    }
  '
)

log_line "# ${STATS}"
