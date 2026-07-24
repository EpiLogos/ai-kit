#!/bin/sh
# search-semantic.sh - semantic / hybrid search using bkmr 7.x local embeddings.
#
# Effects on this path: no network, no credentials. bkmr 7.x embeds with
# fastembed (ONNX, NomicEmbedTextV15, 768 dims by default) entirely on-device.
# The ONNX model itself is downloaded once, at first embed, by `bkmr backfill`
# — not by this script, which never writes.
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

# Refuse quietly-empty results: a database with no vectors answers every
# semantic query with silence, which reads exactly like "nothing matched".
if command -v sqlite3 >/dev/null 2>&1; then
  vec=$(sqlite3 -readonly "$db" \
    "SELECT COUNT(*) FROM sqlite_master WHERE name='vec_bookmarks';" 2>/dev/null || echo 0)
  [ "$vec" != "0" ] || die "$db has no vec_bookmarks table: nothing in it is embedded.
Semantic search here would return an empty result that looks like a miss.
Run \`bkmr backfill --force\` against this database first (see UPGRADE.md),
or use the text search capsule, which needs no embeddings." 78
fi

BKMR_DB_URL=$db
export BKMR_DB_URL

if [ "$mode" = hybrid ]; then
  set -- hsearch --np --json --mode hybrid --limit "$limit"
  [ -z "$tags" ] || set -- "$@" --tags "$tags"
  exec bkmr "$@" "$query"
fi

printf '%s: sem-search has no --json upstream (checked through 7.6.7); output is TSV: id<TAB>title<TAB>url<TAB>score\n' \
  "$me" >&2
exec bkmr sem-search --np --limit "$limit" "$query"
