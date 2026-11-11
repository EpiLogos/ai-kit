#!/bin/sh
# search-semantic.sh - semantic / hybrid search using bkmr 7.x local embeddings.
#
# Effects on this path: no network, no credentials. bkmr 7.x embeds with
# fastembed (ONNX, NomicEmbedTextV15, 768 dims by default) entirely on-device.
# The ONNX model itself is downloaded once, at first embed, by `bkmr backfill`
# — not by this script, which never writes the bound database.
#
# bkmr 7.x opens every database it reads for writing, a search's included: it
# switches the database to WAL journal mode, runs its migrations, and makes the
# -wal and -shm sidecars that its exit does not always remove. A search must
# leave the bound database and its sidecars as they were, so bkmr is never given
# them: it reads a private copy, made in a folder of this search's own under
# TMPDIR and removed when the search ends. The copy is one state of the
# database, with its -wal and -journal (the private copy below says how). The
# vector check reads that copy too.
#
# Two modes, because upstream gives them different output contracts:
#   hybrid   -> `bkmr hsearch --json`   : has JSON, includes rrf_score
#   semantic -> `bkmr sem-search --np`  : has NO --json even in 7.6.7;
#                                         piped output is TSV id/title/url/score

set -eu

me=${0##*/}
die() { printf '%s: %s\n' "$me" "$1" >&2; exit "${2:-1}"; }

usage() {
  cat >&2 <<EOF
usage: $me [options] QUERY

  -l, --limit N        maximum results (default 10)
  -t, --tags LIST      require ALL of these comma-separated tags
  -m, --mode MODE      hybrid (default, JSON) or semantic (TSV, no JSON upstream)

Reads the database from \$AIKIT_BKMR_DB, else \$BKMR_DB_URL.
Requires bkmr >= 7.0.0 (local embedding; no network, no key).
EOF
  exit 64
}

limit=10
tags=
mode=hybrid
query=

while [ $# -gt 0 ]; do
  case $1 in
    -l|--limit) [ $# -ge 2 ] || usage; limit=$2; shift 2 ;;
    -t|--tags)  [ $# -ge 2 ] || usage; tags=$2; shift 2 ;;
    -m|--mode)  [ $# -ge 2 ] || usage; mode=$2; shift 2 ;;
    -h|--help)  usage ;;
    --)         shift; break ;;
    -*)         die "unknown option: $1" 64 ;;
    *)          [ -z "$query" ] || die "only one query is accepted" 64; query=$1; shift ;;
  esac
done
[ -n "$query" ] || usage
case $mode in hybrid|semantic) ;; *) die "--mode must be hybrid or semantic" 64 ;; esac

command -v bkmr >/dev/null 2>&1 || die \
  "bkmr is not on PATH; enable tool/search/bkmr or run script/install/bkmr."

version=$(bkmr --version 2>/dev/null | awk 'NR==1 {print $NF}')
major=${version%%.*}
case $major in
  ''|*[!0-9]*) die "could not read a version from \`bkmr --version\` ($version)" ;;
esac
[ "$major" -ge 7 ] || die "bkmr $version has no local embedding provider.
This capsule declares network = false and credentials = [], which holds only
from 7.0.0 onward (local fastembed). Upgrade first: aikit run script/install/bkmr
— see contrib/bkmr/UPGRADE.md." 78

db=${AIKIT_BKMR_DB:-${BKMR_DB_URL:-}}
[ -n "$db" ] || die "no bkmr database is bound to this context.
Declare one in .aikit/project.toml under [integrations.bkmr]." 78
[ -f "$db" ] || die "the bound database does not exist: $db" 78

# --- the private copy: the same in search-text.sh, search-semantic.sh and
# doctor.sh. A capsule ships only its own payload, so the three cannot share one
# file; a test holds them to the same text (bkmr_real.rs). ---
copies=
trap '[ -z "$copies" ] || rm -rf "$copies"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 131' QUIT
trap 'exit 141' PIPE
trap 'exit 143' TERM
# A run killed outright, as a runner's timeout kills one (SIGKILL), cannot
# remove its folder. So each run first removes this user's own folders that are
# more than an hour old. The runner stops a run after 30 s; only a run held up
# for an hour, by a reader that stalled or a process that was stopped, could
# lose its folder to another run.
find "${TMPDIR:-/tmp}/." ! -name . -prune -type d -name 'aikit-bkmr-search.*' \
  -user "$(id -u)" -mmin +60 -exec rm -rf {} + 2>/dev/null || :

# look_at FILE: FILE's inode, size and modification time, and a checksum of its
# first 32 bytes, or "none" where no FILE stands. It fails where no stat this
# script knows answers. The checksum sees a rewrite that keeps the size within
# one tick of a coarse clock, where the time alone cannot: a -wal's header,
# which a checkpoint or a restart rewrites, and the database's, which holds its
# change counter and its size.
head_of() { dd if="$1" bs=32 count=1 2>/dev/null | cksum; }
if stat -c %i / >/dev/null 2>&1; then
  look_at() { stat -c '%i %s %y' "$1" 2>/dev/null && head_of "$1" || echo none; }
elif stat -f %i / >/dev/null 2>&1; then
  look_at() { stat -f '%i %z %Fm' "$1" 2>/dev/null && head_of "$1" || echo none; }
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
    rm -f "$copy" "$copy-wal" "$copy-journal" "$copy-shm" "$copies"/search_backup_*.db
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

# migrated DB: after bkmr searched the copy of DB, says so, once, when bkmr
# migrated that copy, as it does a database kept in an older schema. bkmr's own
# words name the backup it made, which is of the copy and goes with it. (The
# doctor runs no bkmr and does not call it.)
migrated() {
  for backup in "$copies"/search_backup_*.db; do
    [ -f "$backup" ] || continue
    printf '%s: bkmr migrated the private copy this search made of %s, which is kept in an older schema. The backup bkmr names was made of that copy and is removed with it. Each search migrates its copy again until the database itself is migrated, once: bkmr --db %s search --np --limit 1 ""\n' "$me" "$1" "$1" >&2
    return 0
  done
}
# --- end of the private copy ---

private_copy "$db" || exit "$?"

# Refuse quietly-empty results: a database with no vectors answers every
# semantic query with silence, which reads exactly like "nothing matched".
# The copy is read, not the bound database: bkmr 7.x leaves its databases in WAL
# mode, which `sqlite3 -readonly` cannot open once their -shm is gone (it would
# refuse every cleanly closed database here), and a reader beside a live writer
# rewrites the -shm.
if command -v sqlite3 >/dev/null 2>&1; then
  vec=$(sqlite3 "$copy" \
    "SELECT COUNT(*) FROM sqlite_master WHERE name='vec_bookmarks';" 2>/dev/null || echo 0)
  [ "$vec" != "0" ] || die "$db has no vec_bookmarks table: nothing in it is embedded.
Semantic search here would return an empty result that looks like a miss.
Run \`bkmr backfill --force\` against this database first (see UPGRADE.md),
or use the text search capsule, which needs no embeddings." 78
fi

BKMR_DB_URL=$copy
export BKMR_DB_URL

# bkmr runs as a child, not by exec, so the EXIT trap removes the copy after it.
if [ "$mode" = hybrid ]; then
  set -- hsearch --np --json --mode hybrid --limit "$limit"
  [ -z "$tags" ] || set -- "$@" --tags "$tags"
  if bkmr "$@" "$query"; then found=0; else found=$?; fi
  migrated "$db"
  exit "$found"
fi

printf '%s: sem-search has no --json upstream (checked through 7.6.7); output is TSV: id<TAB>title<TAB>url<TAB>score\n' \
  "$me" >&2
if bkmr sem-search --np --limit "$limit" "$query"; then found=0; else found=$?; fi
migrated "$db"
exit "$found"
