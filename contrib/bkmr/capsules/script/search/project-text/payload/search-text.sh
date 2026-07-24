#!/bin/sh
# search-text.sh - full-text search of the bkmr database bound to this context.
#
# Offline, no credentials, ~30 ms on the databases this was written against.
# Uses `bkmr search --json`, which is read-only. `bkmr show --json` is NOT used:
# it bumps access_count and the update timestamp, so reading with it would
# rewrite the corpus a search is supposed to observe.

set -eu

me=${0##*/}
die() { printf '%s: %s\n' "$me" "$1" >&2; exit "${2:-1}"; }

usage() {
  cat >&2 <<EOF
usage: $me [options] QUERY

  -l, --limit N     maximum results (default 10)
  -t, --tags LIST   require ALL of these comma-separated tags
      --all         search every database in the declared set as well
      --set LIST    colon-separated database paths for --all
                    (default: \$AIKIT_BKMR_DB_SET)
      --raw         human-readable output instead of JSON

Reads the database from \$AIKIT_BKMR_DB, else \$BKMR_DB_URL.
EOF
  exit 64
}

limit=10
tags=
all=0
raw=0
dbset=${AIKIT_BKMR_DB_SET:-}
query=

while [ $# -gt 0 ]; do
  case $1 in
    -l|--limit) [ $# -ge 2 ] || usage; limit=$2; shift 2 ;;
    -t|--tags)  [ $# -ge 2 ] || usage; tags=$2; shift 2 ;;
    --set)      [ $# -ge 2 ] || usage; dbset=$2; shift 2 ;;
    --all)      all=1; shift ;;
    --raw)      raw=1; shift ;;
    -h|--help)  usage ;;
    --)         shift; break ;;
    -*)         die "unknown option: $1" 64 ;;
    *)          [ -z "$query" ] || die "only one query is accepted" 64; query=$1; shift ;;
  esac
done
[ -n "$query" ] || [ $# -gt 0 ] || usage
[ -n "$query" ] || { query=$1; shift; }

command -v bkmr >/dev/null 2>&1 || die \
  "bkmr is not on PATH; enable tool/search/bkmr or run script/install/bkmr."

db=${AIKIT_BKMR_DB:-${BKMR_DB_URL:-}}
[ -n "$db" ] || die "no bkmr database is bound to this context.
Declare one in .aikit/project.toml:

  [integrations.bkmr]
  db = \"<name>\"

AIKit exports it as AIKIT_BKMR_DB/BKMR_DB_URL when the context is applied." 78
[ -f "$db" ] || die "the bound database does not exist: $db" 78

run_one() {
  # $1 = database path
  BKMR_DB_URL=$1
  export BKMR_DB_URL
  set -- search --np --limit "$limit"
  [ "$raw" -eq 1 ] || set -- "$@" --json
  [ -z "$tags" ] || set -- "$@" --tags "$tags"
  if [ "$raw" -eq 1 ]; then
    # When its stdout is a pipe, bkmr sends the formatted listing to stderr and
    # only a comma-separated id list to stdout. A wrapper that captures stdout
    # alone therefore gets "31,37" and looks broken. Merge the streams.
    bkmr "$@" "$query" 2>&1 || true
  else
    bkmr "$@" "$query" 2>/dev/null || true
  fi
}

if [ "$all" -eq 0 ]; then
  run_one "$db"
  exit 0
fi

[ -n "$dbset" ] || die "--all needs a declared database set.
Add one to .aikit/project.toml:

  [integrations.bkmr]
  db = \"<name>\"
  also = [\"<other>\", \"<other>\"]

AIKit exports the resolved paths as AIKIT_BKMR_DB_SET (colon separated).
This is deliberately not a directory glob: a glob would silently pick up
*_backup_YYYYMMDD.db files that bkmr 7.x writes next to the database." 78

printf '%s\n' "$dbset" | tr ':' '\n' | while IFS= read -r one; do
  [ -n "$one" ] || continue
  if [ ! -f "$one" ]; then
    printf '%s: declared database missing, skipped: %s\n' "$me" "$one" >&2
    continue
  fi
  printf '### %s\n' "$one"
  run_one "$one"
  printf '\n'
done
