#!/usr/bin/env sh
# Start the Workcell-owned Kev decision service for one target directory.
# Usage: serve-kev.sh <target-dir> [port]
# Idempotent per pidfile: a running service is adopted, not duplicated.
#
# Serving optimizations (kept beside the service, applied on every start):
# - KEV_PREFIX_CACHE=8: the knowledge-driven pattern repeats one state across
#   many questions and repeated prepares; eight retained states keep the
#   participants' working sets hot (upstream: ~5x on repeat state).
# Readiness belongs to Workcell's separate bounded readiness probe. The
# target start command must return within its ten-second carrier boundary.
# Startup performs no inference; benchmark the first request separately.
set -eu
TARGET="${1:?usage: serve-kev.sh <target-dir> [port]}"
PORT="${2:-8019}"
PIDFILE="$TARGET/kev-decision.pid"
PYTHON="$TARGET/kev/.venv/bin/python"
[ -x "$PYTHON" ] || { echo "installed Kev Python environment is missing" >&2; exit 1; }
if [ "$(uname -s)" = Darwin ]; then
  # launchd owns the persistent process independently of the short-lived
  # Workcell command. Use the already installed environment, without uv
  # resolution, dependency changes or downloads during ordinary startup.
  PLIST="$TARGET/kev-launchd.plist"
  python3 - "$TARGET" "$PORT" "$PLIST" <<'PY'
import hashlib, os, pathlib, plistlib, sys
target = pathlib.Path(sys.argv[1]).resolve(strict=True)
port = int(sys.argv[2])
if not 1 <= port <= 65535:
    raise ValueError("invalid local service port")
label = "org.epilogos.workcell.kev-" + hashlib.sha256(str(target).encode()).hexdigest()[:16]
value = {
    "Label": label,
    "ProgramArguments": [str(target / "kev/.venv/bin/python"), "-m", "kev.serve",
                         "--run", "jaredpalmer/kev-0.8b", "--host", "127.0.0.1", "--port", str(port)],
    "WorkingDirectory": str(target / "kev"),
    "EnvironmentVariables": {"HF_HUB_OFFLINE": "1", "TRANSFORMERS_OFFLINE": "1", "KEV_PREFIX_CACHE": "8"},
    "StandardOutPath": str(target / "kev-decision.log"),
    "StandardErrorPath": str(target / "kev-decision.log"),
    "RunAtLoad": True,
    "KeepAlive": False,
}
path = pathlib.Path(sys.argv[3]); temporary = path.with_suffix(".tmp")
temporary.write_bytes(plistlib.dumps(value)); os.replace(temporary, path)
PY
  LABEL=$(/usr/libexec/PlistBuddy -c 'Print :Label' "$PLIST")
  DOMAIN="gui/$(id -u)"
  if /bin/launchctl print "$DOMAIN/$LABEL" >/dev/null 2>&1; then
    echo "Kev target service already registered: $LABEL"
  else
    /bin/launchctl bootstrap "$DOMAIN" "$PLIST"
    echo "Kev target service registered: $LABEL; readiness remains a separate Workcell probe"
  fi
  exit 0
fi
if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
  echo "kev decision service already running (pid $(cat "$PIDFILE")) on port $PORT"
  exit 0
fi
cd "$TARGET/kev"
# Cached weights only: the serving process must start and answer with
# outbound network disabled. These two variables enforce that at load time.
HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 KEV_PREFIX_CACHE=8 \
  nohup "$PYTHON" -m kev.serve \
    --run jaredpalmer/kev-0.8b \
    --host 127.0.0.1 --port "$PORT" \
    > "$TARGET/kev-decision.log" 2>&1 < /dev/null &
SERVER_PID=$!
echo "$SERVER_PID" > "$PIDFILE"
echo "started pid $SERVER_PID; readiness remains a separate Workcell probe"
