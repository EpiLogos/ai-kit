#!/bin/sh
# project-snapshot.sh - consistent, timestamped backup of the bound bkmr
# database.
#
# The predecessor did `gzip -c live.db > snapshot.db.gz`. That reads a live
# SQLite file page by page while another process may be writing it, and bkmr
# 7.x runs in WAL mode, so committed data can be sitting in the -wal file that
# gzip never sees. The result can be a corrupt or stale image that only fails
# when you try to restore it. `VACUUM INTO` takes the read lock SQLite requires
# and produces a valid database.

set -eu

me=${0##*/}
die() { printf '%s: %s\n' "$me" "$1" >&2; exit "${2:-1}"; }

usage() {
  cat >&2 <<EOF
usage: $me [options]

      --dir DIR    where snapshots go
                   (default: \$AIKIT_BKMR_SNAPSHOT_DIR, else <db dir>/.snapshots)
      --keep N     keep the newest N snapshots of this database, prune the rest
                   (default 10; 0 disables pruning)
      --list       list existing snapshots for the bound database and exit
      --gzip       compress the snapshot after it is written

Reads the database from \$AIKIT_BKMR_DB, else \$BKMR_DB_URL.
EOF
  exit 64
}

keep=10
snapdir=
list=0
gz=0

while [ $# -gt 0 ]; do
  case $1 in
    --dir)     [ $# -ge 2 ] || usage; snapdir=$2; shift 2 ;;
    --keep)    [ $# -ge 2 ] || usage; keep=$2; shift 2 ;;
    --list)    list=1; shift ;;
    --gzip)    gz=1; shift ;;
    -h|--help) usage ;;
    *)         die "unexpected argument: $1" 64 ;;
  esac
done

case $keep in ''|*[!0-9]*) die "--keep takes a non-negative integer" 64 ;; esac

db=${AIKIT_BKMR_DB:-${BKMR_DB_URL:-}}
[ -n "$db" ] || die "no bkmr database is bound to this context.
Declare one in .aikit/project.toml under [integrations.bkmr]." 78
[ -f "$db" ] || die "the bound database does not exist: $db" 78

command -v sqlite3 >/dev/null 2>&1 || die \
  "sqlite3 is required: this script uses VACUUM INTO for a consistent copy."

base=$(basename "$db"); base=${base%.*}
[ -n "$snapdir" ] || snapdir=${AIKIT_BKMR_SNAPSHOT_DIR:-$(dirname "$db")/.snapshots}

if [ "$list" -eq 1 ]; then
  [ -d "$snapdir" ] || { printf 'no snapshots yet (%s does not exist)\n' "$snapdir"; exit 0; }
  found=0
  for f in "$snapdir/$base"-*; do
    [ -f "$f" ] || continue
    found=1
    printf '%s\t%s\n' "$(du -h "$f" | cut -f1)" "$f"
  done
  [ "$found" -eq 1 ] || printf 'no snapshots for %s under %s\n' "$base" "$snapdir"
  exit 0
fi

mkdir -p "$snapdir"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
out="$snapdir/$base-$stamp.db"
[ ! -e "$out" ] || die "snapshot already exists: $out" 78

# VACUUM INTO refuses to overwrite, so a partial file cannot be mistaken for a
# good one. Quote the path for SQL by doubling any single quotes.
sqlout=$(printf '%s' "$out" | sed "s/'/''/g")
sqlite3 -readonly "$db" "VACUUM INTO '$sqlout';" \
  || die "VACUUM INTO failed; no snapshot was written"

# Prove the copy opens and has the table we care about before claiming success.
sqlite3 -readonly "$out" 'SELECT COUNT(*) FROM bookmarks;' >/dev/null 2>&1 \
  || die "the snapshot at $out does not read back as a bkmr database"

if [ "$gz" -eq 1 ]; then
  gzip -- "$out" || die "gzip failed; the uncompressed snapshot is at $out"
  out="$out.gz"
fi

printf 'snapshot: %s\n' "$out"

if [ "$keep" -gt 0 ]; then
  # Names are UTC-timestamped, so lexical order is chronological order.
  n=0
  for f in $(ls -1 "$snapdir/$base"-* 2>/dev/null | sort -r); do
    n=$((n + 1))
    [ "$n" -gt "$keep" ] || continue
    rm -f -- "$f" && printf 'pruned:   %s\n' "$f"
  done
fi
