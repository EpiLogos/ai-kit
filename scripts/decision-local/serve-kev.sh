#!/usr/bin/env sh
# Start the Workcell-owned Kev decision service for one target directory.
# Usage: serve-kev.sh <target-dir> [port]
# Idempotent per pidfile: a running service is adopted, not duplicated.
set -eu
TARGET="${1:?usage: serve-kev.sh <target-dir> [port]}"
PORT="${2:-8019}"
PIDFILE="$TARGET/kev-decision.pid"
if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
  echo "kev decision service already running (pid $(cat "$PIDFILE")) on port $PORT"
  exit 0
fi
cd "$TARGET/kev"
# Cached weights only: the serving process must start and answer with
# outbound network disabled. These two variables enforce that at load time.
HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 \
  nohup uv run --extra serve python -m kev.serve \
    --run jaredpalmer/kev-0.8b \
    --host 127.0.0.1 --port "$PORT" \
    > "$TARGET/kev-decision.log" 2>&1 &
echo $! > "$PIDFILE"
echo "started pid $(cat "$PIDFILE"), log $TARGET/kev-decision.log"
