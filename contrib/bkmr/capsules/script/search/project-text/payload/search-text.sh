#!/bin/sh
# search-text.sh - full-text search of the bkmr database bound to this context.
#
# Offline, no credentials, ~30 ms on the databases this was written against.
# Uses `bkmr search --json`, which changes no bookmark. `bkmr show --json` is NOT
# used: it bumps access_count and the update timestamp, so reading with it would
# rewrite the corpus a search is supposed to observe.
#
# bkmr 7.x opens every database it reads for writing, a search's included: it
# switches the database to WAL journal mode, runs its migrations, and makes the
# -wal and -shm sidecars that its exit does not always remove. A search must
# leave the bound database and its sidecars as they were, so bkmr is never given
# them: it reads a private copy of each database (its -wal too, where committed
# bookmarks can still stand), made in a folder of this search's own and removed
# when the search ends.

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

copies=
trap '[ -z "$copies" ] || rm -rf "$copies"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

run_one() {
  # $1 = database path
  if [ -z "$copies" ]; then
    copies=$(mktemp -d "${TMPDIR:-/tmp}/aikit-bkmr-search.XXXXXX") || {
      printf '%s: could not make a private folder to search a copy in\n' "$me" >&2
      return 74
    }
  fi
  copy=$copies/search.db
  rm -f "$copy" "$copy-wal" "$copy-shm"
  if ! cp "$1" "$copy" || { [ -f "$1-wal" ] && ! cp "$1-wal" "$copy-wal"; }; then
    printf '%s: could not copy the database to search it: %s\n' "$me" "$1" >&2
    return 74
  fi
  chmod u+w "$copy"
  [ ! -f "$copy-wal" ] || chmod u+w "$copy-wal"
  BKMR_DB_URL=$copy
  export BKMR_DB_URL
  set -- search --np --limit "$limit"
  [ "$raw" -eq 1 ] || set -- "$@" --json
  [ -z "$tags" ] || set -- "$@" --tags "$tags"
  if [ "$raw" -eq 1 ]; then
    # When its stdout is a pipe, bkmr sends the formatted listing to stderr and
    # only a comma-separated id list to stdout. A wrapper that captures stdout
    # alone therefore gets "31,37" and looks broken. Merge the streams.
    bkmr "$@" "$query" 2>&1
  else
    bkmr "$@" "$query"
  fi
}

if [ "$all" -eq 0 ]; then
  [ -f "$db" ] || die "the bound database does not exist: $db" 78
  if run_one "$db"; then
    exit 0
  else
    exit "$?"
  fi
fi

[ -n "$dbset" ] || die "--all needs a declared database set.
Add one to .aikit/project.toml:

  [integrations.bkmr]
  db = \"<name>\"
  also = [\"<other>\", \"<other>\"]

AIKit exports the resolved paths as AIKIT_BKMR_DB_SET (colon separated).
This is deliberately not a directory glob: a glob would silently pick up
*_backup_YYYYMMDD.db files that bkmr 7.x writes next to the database." 78

status=0
while IFS= read -r one; do
  [ -n "$one" ] || continue
  if [ ! -f "$one" ]; then
    printf '%s: declared database missing, skipped: %s\n' "$me" "$one" >&2
    [ "$status" -ne 0 ] || status=78
    continue
  fi
  printf '### %s\n' "$one"
  if run_one "$one"; then
    :
  else
    one_status=$?
    [ "$status" -ne 0 ] || status=$one_status
  fi
  printf '\n'
done <<EOF
$(printf '%s\n' "$dbset" | tr ':' '\n')
EOF
exit "$status"
