#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/connectors/bin"
mkdir -p "$BIN"

for name in whatsapp telegram signal messenger instagram email matrix; do
  cat > "$BIN/${name}-connector" <<EOF
#!/usr/bin/env bash
export PYTHONPATH="$ROOT/connectors\${PYTHONPATH:+:\$PYTHONPATH}"
exec python3 "$ROOT/connectors/${name}-connector.py"
EOF
  chmod +x "$BIN/${name}-connector"
done

echo "Built connector sidecars in $BIN"
echo "Optional native deps:"
echo "  ./connectors/gowa/fetch.sh"
echo "  ./connectors/tdlib/fetch.sh"
echo "  ./connectors/signal/fetch.sh"
echo "Python extras (Messenger / Instagram): pip install -r connectors/requirements.txt"
