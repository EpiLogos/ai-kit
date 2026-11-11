#!/bin/sh
# doctor.sh - report what bkmr this context would actually use, and whether the
# bound database is usable. Read-only: it never writes to the bound database and
# never runs a bkmr subcommand that mutates one.
#
# Its counts are read from a private copy of the database, not the database: bkmr
# 7.x leaves its databases in WAL mode, which `sqlite3 -readonly` cannot open once
# their -shm is gone (the counts would read "?" and every database would look
# unmigrated), and a reader beside a live writer rewrites the -shm. The copy, one
# state of the database with its -wal and -journal, is made in a folder of this
# run's own under TMPDIR and removed when it ends (the private copy below says
# how).
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
# first 4096 bytes, or "none" where no FILE stands. It fails where no stat this
# script knows answers. The checksum sees a rewrite that keeps the size within
# one tick of a coarse clock, where the time alone cannot. Of the database it
# covers the header and the rest of the first page, where the schema's root
# stands. Of a -wal it covers the header, which a restart rewrites (the first
# write after a complete checkpoint; a checkpoint alone does not), and the first
# frame's header. A same-size rewrite past the first 4096 bytes inside one tick
# of a coarse clock is not seen.
head_of() { dd if="$1" bs=4096 count=1 2>/dev/null | cksum; }
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
# they are copied. When any of them moved, they are copied again, five tries
# in all, with a short and growing pause before each new try (20, 60, 180 and
# 540 ms), so that a writer that commits often leaves a gap. Where this sleep
# takes no fraction of a second the pause is none and the try follows at once.
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
    if [ "$tries" -ge 5 ]; then
      printf '%s: the database changed on every try while it was copied (%s tries): %s\n' "$me" "$tries" "$1" >&2
      return 75
    fi
    case $tries in
      1) sleep 0.02 2>/dev/null || : ;;
      2) sleep 0.06 2>/dev/null || : ;;
      3) sleep 0.18 2>/dev/null || : ;;
      *) sleep 0.54 2>/dev/null || : ;;
    esac
  done
  chmod u+w "$copy"
  [ ! -f "$copy-wal" ] || chmod u+w "$copy-wal"
  [ ! -f "$copy-journal" ] || chmod u+w "$copy-journal"
}

# migrated DB: after bkmr searched the copy of DB, says so, once, when bkmr
# migrated that copy, as it does a database kept in an older schema. bkmr's own
# words name the backup it made, which is of the copy and goes with it. Once,
# however many backups stand in the copy's folder. The command it gives names
# the database in single quotes, so that it can be pasted whatever the path
# holds. (The doctor runs no bkmr and does not call it.)
migrated() {
  for backup in "$copies"/search_backup_*.db; do
    [ -f "$backup" ] || continue
    quoted=$(printf '%s' "$1" | sed "s/'/'\\\\''/g")
    printf '%s: bkmr migrated the private copy this search made of %s, which is kept in an older schema. The backup bkmr names was made of that copy and is removed with it. Each search migrates its copy again until the database itself is migrated, once: bkmr --db '"'%s'"' search --np --limit 1 ""\n' "$me" "$1" "$quoted" >&2
    return 0
  done
}
# --- end of the private copy ---

if ! command -v sqlite3 >/dev/null 2>&1; then
  warn "sqlite3 not found; skipping row and embedding counts."
elif ! private_copy "$db"; then
  # The copy could not be made (the line above this one says why, naming the
  # database). The doctor reports and goes on, as it did before it counted from
  # a copy, when a database it could not read gave "?": its status stays 0, and
  # the embedding counts, which would be guesses, are left out.
  printf 'rows          : ? (not counted: the database could not be copied for the count)\n'
  warn "row and embedding counts are skipped; the reason is above."
else
  rows=$(sqlite3 "$copy" 'SELECT COUNT(*) FROM bookmarks;' 2>/dev/null || echo '?')
  printf 'rows          : %s\n' "$rows"
  if [ "$major" -ge 7 ] 2>/dev/null; then
    vec=$(sqlite3 "$copy" \
      "SELECT COUNT(*) FROM sqlite_master WHERE name='vec_bookmarks';" 2>/dev/null || echo 0)
    if [ "$vec" = "0" ]; then
      printf 'embeddings    : none (vec_bookmarks table absent)\n'
      warn "this database has not been migrated/backfilled for bkmr 7.x."
      warn "semantic search will return nothing here. See contrib/bkmr/UPGRADE.md."
    else
      printf 'embeddings    : vec_bookmarks present\n'
    fi
    legacy=$(sqlite3 "$copy" \
      'SELECT COUNT(*) FROM bookmarks WHERE embedding IS NOT NULL;' 2>/dev/null || echo 0)
    [ "$legacy" = "0" ] || warn \
      "$legacy legacy 6.x embedding blobs are still present; 7.x ignores them."
  else
    emb=$(sqlite3 "$copy" \
      'SELECT COUNT(*) FROM bookmarks WHERE embedding IS NOT NULL;' 2>/dev/null || echo '?')
    printf 'embeddings    : %s of %s rows (6.x inline BLOB)\n' "$emb" "$rows"
    [ "$emb" != "0" ] || warn \
      "no embeddings in this database: semantic search cannot work here."
  fi
fi

if [ "$major" -lt 7 ] 2>/dev/null; then
  warn "bkmr $version needs a network round trip and GEMINI_API_KEY for semantic"
  warn "search. bkmr 7.x embeds locally. See contrib/bkmr/UPGRADE.md."
  [ -n "${GEMINI_API_KEY:-}" ] || warn "GEMINI_API_KEY is not set."
fi
