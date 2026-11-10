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
# them: it reads a private copy of each database, made in a folder of this
# search's own under TMPDIR and removed when the search ends.
#
# The copy is one state of the database. It takes the -wal, where committed
# bookmarks can still stand, and the -journal, which bkmr rolls back in the copy
# as it would have in the database. SQLite keeps both beside the file the
# database's path leads to, links followed, so they are looked for there. When
# any of the three moved while they were copied, they are copied again.

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
trap 'exit 131' QUIT
trap 'exit 141' PIPE
trap 'exit 143' TERM
# A run killed outright, as a runner's timeout kills one (SIGKILL), cannot
# remove its folder. So each run first removes this user's own folders that are
# more than ten minutes old: no search lasts that long.
find "${TMPDIR:-/tmp}/." ! -name . -prune -type d -name 'aikit-bkmr-search.*' \
  -user "$(id -u)" -mmin +10 -exec rm -rf {} + 2>/dev/null || :

# look_at FILE: FILE's inode, size and modification time, or "none" where no
# FILE stands. It fails where no stat this script knows answers.
if stat -c %i / >/dev/null 2>&1; then
  look_at() { stat -c '%i %s %y' "$1" 2>/dev/null || echo none; }
elif stat -f %i / >/dev/null 2>&1; then
  look_at() { stat -f '%i %z %Fm' "$1" 2>/dev/null || echo none; }
else
  look_at() { return 1; }
fi

# real_file PATH: the file PATH leads to, every link on the way followed, as
# SQLite opens it; left in $real.
real_file() {
  real=$1
  links=0
  while [ -L "$real" ]; do
    links=$((links + 1))
    [ "$links" -le 40 ] || return 1
    link=$(readlink "$real") || return 1
    case $link in
      /*) real=$link ;;
      *) real=$(dirname "$real")/$link ;;
    esac
  done
  folder=$(CDPATH= cd -P "$(dirname "$real")" && pwd) || return 1
  real=$folder/${real##*/}
}

# private_copy DB: copies DB, and its -wal and -journal where they stand, into
# this run's own folder, made at the first copy and removed when the run ends,
# and leaves the copy's path in $copy. The three are looked at before and after
# they are copied. When any of them moved, they are copied again, three tries
# in all.
private_copy() {
  if [ -z "$copies" ]; then
    copies=$(mktemp -d "${TMPDIR:-/tmp}/aikit-bkmr-search.XXXXXX") || {
      printf '%s: could not make a private folder to search a copy of the database in: %s\n' "$me" "$1" >&2
      return 74
    }
  fi
  real_file "$1" || {
    printf '%s: could not follow the links to the database: %s\n' "$me" "$1" >&2
    return 74
  }
  copy=$copies/search.db
  tries=0
  while :; do
    tries=$((tries + 1))
    rm -f "$copy" "$copy-wal" "$copy-journal" "$copy-shm"
    if ! before=$(look_at "$real" && look_at "$real-wal" && look_at "$real-journal"); then
      printf '%s: could not look at the database to tell whether it moved while it was copied: %s\n' "$me" "$1" >&2
      return 74
    fi
    copied=yes
    cp "$real" "$copy" || copied=no
    [ ! -f "$real-wal" ] || cp "$real-wal" "$copy-wal" || copied=no
    [ ! -f "$real-journal" ] || cp "$real-journal" "$copy-journal" || copied=no
    after=$(look_at "$real" && look_at "$real-wal" && look_at "$real-journal") || after=
    if [ "$before" = "$after" ]; then
      [ "$copied" = no ] || break
      printf '%s: could not copy the database to search it: %s\n' "$me" "$1" >&2
      return 74
    fi
    if [ "$tries" -ge 3 ]; then
      printf '%s: the database changed on every try while it was copied (%s tries): %s\n' "$me" "$tries" "$1" >&2
      return 75
    fi
  done
  chmod u+w "$copy"
  [ ! -f "$copy-wal" ] || chmod u+w "$copy-wal"
  [ ! -f "$copy-journal" ] || chmod u+w "$copy-journal"
}

run_one() {
  # $1 = database path
  private_copy "$1" || return "$?"
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
