#!/usr/bin/env sh
# Start the Workcell-owned Kev decision service for one target directory.
# Usage: serve-kev.sh <target-dir> [port]
# Idempotent per pidfile: a running service is adopted, not duplicated.
#
# Serving optimizations (kept beside the service, applied on every start):
# - KEV_PREFIX_CACHE=8: the knowledge-driven pattern repeats one state across
#   many questions and repeated prepares; eight retained states keep the
#   participants' working sets hot (upstream: ~5x on repeat state).
# - warm-at-start: one packed Noul/Choice/Score request after the model card
#   answers, so Metal kernel compilation happens at start, not on the first
#   real decision. The pidfile is written only once the service is warm, so
#   "started" means ready-at-speed, not merely listening.
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
HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 KEV_PREFIX_CACHE=8 \
  nohup uv run --extra serve python -m kev.serve \
    --run jaredpalmer/kev-0.8b \
    --host 127.0.0.1 --port "$PORT" \
    > "$TARGET/kev-decision.log" 2>&1 &
SERVER_PID=$!
echo "$SERVER_PID" > "$PIDFILE"
# Wait for the model card, then compile the kernels once with a packed
# request covering all three question kinds.
i=0
until curl -s --max-time 3 "http://127.0.0.1:$PORT/v1/models" | grep -q '"models"' 2>/dev/null; do
  i=$((i+1))
  # Under heavy machine load the 1.75 GB load + LoRA merge can take many
  # minutes; give it 20 before reporting, and say the server may still come up.
  [ $i -gt 600 ] && { echo "model card not answered within budget; the server process may still be loading - check $TARGET/kev-decision.log and retry" >&2; exit 1; }
  sleep 2
done
curl -s --max-time 120 "http://127.0.0.1:$PORT/v1/systemone" \
  -H 'content-type: application/json' \
  -d '{"model":"kev-latest","state":{"warmup":"Startup warmup: kernels compile on the first forward pass; this request is discarded."},"questions":{"warm_noul":{"type":"noul","instructions":"Warmup probe: answer true."},"warm_choice":{"type":"choice","instructions":"Warmup routing probe.","criteria":{"a":"first","b":"second"}},"warm_score":{"type":"score","instructions":"Warmup ordinal probe.","criteria":["low","high"]}}}' \
  > "$TARGET/kev-warmup.json" 2>/dev/null || true
echo "started pid $SERVER_PID (warm), log $TARGET/kev-decision.log"
