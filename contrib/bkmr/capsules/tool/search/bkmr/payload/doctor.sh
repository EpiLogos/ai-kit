#!/bin/sh
# doctor.sh - report what bkmr this context would actually use, and whether the
# bound database is usable. Read-only: it never writes to a database and never
# runs a bkmr subcommand that mutates one.
#
# `bkmr show` is deliberately not used anywhere here: it bumps access_count and
# last_update_ts / accessed_at, so it is a write dressed as a read.

set -eu

me=${0##*/}
warn() { printf '%s: %s\n' "$me" "$1" >&2; }
die() { printf '%s: %s\n' "$me" "$1" >&2; exit "${2:-1}"; }

command -v bkmr >/dev/null 2>&1 || die \
  "bkmr is not on PATH. Install it with the script/install/bkmr capsule."

version=$(bkmr --version 2>/dev/null | awk 'NR==1 {print $NF}')
[ -n "$version" ] || die "\`bkmr --version\` produced no version string."
major=${version%%.*}

db=${AIKIT_BKMR_DB:-${BKMR_DB_URL:-}}

printf 'bkmr binary   : %s\n' "$(command -v bkmr)"
printf 'bkmr version  : %s\n' "$version"

if [ -z "$db" ]; then
  printf 'bound database: (none)\n'
  warn "no database is bound to this context."
  warn "AIKit binds one by exporting AIKIT_BKMR_DB / BKMR_DB_URL from"
  warn "[integrations.bkmr] in .aikit/project.toml. Without it bkmr falls back"
  warn "to ~/.config/bkmr/config.toml, which is global and shared."
  exit 2
fi

printf 'bound database: %s\n' "$db"
[ -f "$db" ] || die "the bound database does not exist: $db" 2

if command -v sqlite3 >/dev/null 2>&1; then
  rows=$(sqlite3 -readonly "$db" 'SELECT COUNT(*) FROM bookmarks;' 2>/dev/null || echo '?')
  printf 'rows          : %s\n' "$rows"
  if [ "$major" -ge 7 ] 2>/dev/null; then
    vec=$(sqlite3 -readonly "$db" \
      "SELECT COUNT(*) FROM sqlite_master WHERE name='vec_bookmarks';" 2>/dev/null || echo 0)
    if [ "$vec" = "0" ]; then
      printf 'embeddings    : none (vec_bookmarks table absent)\n'
      warn "this database has not been migrated/backfilled for bkmr 7.x."
      warn "semantic search will return nothing here. See contrib/bkmr/UPGRADE.md."
    else
      printf 'embeddings    : vec_bookmarks present\n'
    fi
    legacy=$(sqlite3 -readonly "$db" \
      'SELECT COUNT(*) FROM bookmarks WHERE embedding IS NOT NULL;' 2>/dev/null || echo 0)
    [ "$legacy" = "0" ] || warn \
      "$legacy legacy 6.x embedding blobs are still present; 7.x ignores them."
  else
    emb=$(sqlite3 -readonly "$db" \
      'SELECT COUNT(*) FROM bookmarks WHERE embedding IS NOT NULL;' 2>/dev/null || echo '?')
    printf 'embeddings    : %s of %s rows (6.x inline BLOB)\n' "$emb" "$rows"
    [ "$emb" != "0" ] || warn \
      "no embeddings in this database: semantic search cannot work here."
  fi
else
  warn "sqlite3 not found; skipping row and embedding counts."
fi

if [ "$major" -lt 7 ] 2>/dev/null; then
  warn "bkmr $version needs a network round trip and GEMINI_API_KEY for semantic"
  warn "search. bkmr 7.x embeds locally. See contrib/bkmr/UPGRADE.md."
  [ -n "${GEMINI_API_KEY:-}" ] || warn "GEMINI_API_KEY is not set."
fi
