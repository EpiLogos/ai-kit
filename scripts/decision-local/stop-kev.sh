#!/usr/bin/env sh
# Stop only the target-owned Kev service registered by serve-kev.sh.
set -eu
TARGET="${1:?usage: stop-kev.sh <target-dir>}"
if [ "$(uname -s)" = Darwin ]; then
  PLIST="$TARGET/kev-launchd.plist"
  [ -f "$PLIST" ] || exit 0
  LABEL=$(/usr/libexec/PlistBuddy -c 'Print :Label' "$PLIST")
  DOMAIN="gui/$(id -u)"
  if /bin/launchctl print "$DOMAIN/$LABEL" >/dev/null 2>&1; then
    /bin/launchctl bootout "$DOMAIN/$LABEL"
  fi
  exit 0
fi
PIDFILE="$TARGET/kev-decision.pid"
[ -f "$PIDFILE" ] || exit 0
PID=$(cat "$PIDFILE")
case "$PID" in ''|*[!0-9]*) echo 'invalid Kev process identity' >&2; exit 1;; esac
if kill -0 "$PID" 2>/dev/null; then
  COMMAND=$(ps -p "$PID" -o args=)
  case "$COMMAND" in "$TARGET/kev/.venv/bin/python -m kev.serve "*) kill "$PID";;
    *) echo 'Kev process identity changed; refusing unrelated process' >&2; exit 1;; esac
fi
rm -f "$PIDFILE"
