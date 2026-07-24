#!/bin/sh
# project-init.sh - create a bkmr database for a project and print the
# declaration that binds it.
#
# What this deliberately does NOT do: it does not write
# ~/.config/bkmr/projects/.current, and it does not switch anything. Creating a
# database and choosing which database a shell sees are two different acts;
# fusing them is what produced three entry points that disagreed about the
# active project. Binding is a declaration in .aikit/project.toml, resolved per
# context, and this script only tells you what to write.

set -eu

me=${0##*/}
die() { printf '%s: %s\n' "$me" "$1" >&2; exit "${2:-1}"; }

usage() {
  cat >&2 <<EOF
usage: $me [--dir DIR] NAME     create DIR/NAME.db and print the declaration
       $me [--dir DIR] --list   inventory the databases in DIR (read-only)
       $me --info PATH          report on one database (read-only)

  --dir DIR   where project databases live
              (default: \$AIKIT_BKMR_DB_DIR, else ~/.config/bkmr/projects)
EOF
  exit 64
}

dir=${AIKIT_BKMR_DB_DIR:-$HOME/.config/bkmr/projects}
action=create
name=
target=

while [ $# -gt 0 ]; do
  case $1 in
    --dir)     [ $# -ge 2 ] || usage; dir=$2; shift 2 ;;
    --list)    action=list; shift ;;
    --info)    [ $# -ge 2 ] || usage; action=info; target=$2; shift 2 ;;
    -h|--help) usage ;;
    -*)        die "unknown option: $1" 64 ;;
    *)         [ -z "$name" ] || die "only one project name is accepted" 64; name=$1; shift ;;
  esac
done

# A read-only report. Counts real embeddings (embedding IS NOT NULL on 6.x, the
# vec_bookmarks table on 7.x) rather than the `embeddable` flag, which says only
# that a row is *eligible* to be embedded.
report() {
  path=$1
  [ -f "$path" ] || { printf '  %s: missing\n' "$path"; return 0; }
  rows='?'; emb='?'
  if command -v sqlite3 >/dev/null 2>&1; then
    rows=$(sqlite3 -readonly "$path" 'SELECT COUNT(*) FROM bookmarks;' 2>/dev/null || echo '?')
    if [ "$(sqlite3 -readonly "$path" \
        "SELECT COUNT(*) FROM sqlite_master WHERE name='vec_bookmarks';" 2>/dev/null || echo 0)" != "0" ]; then
      emb='vec_bookmarks (7.x)'
    else
      emb=$(sqlite3 -readonly "$path" \
        'SELECT COUNT(*) FROM bookmarks WHERE embedding IS NOT NULL;' 2>/dev/null || echo '?')
      emb="$emb inline (6.x)"
    fi
  fi
  printf '  %-28s %6s rows  embedded: %s\n' "$(basename "$path")" "$rows" "$emb"
}

case $action in
  info)
    report "$target"
    exit 0 ;;
  list)
    [ -d "$dir" ] || die "no such directory: $dir" 78
    printf 'databases under %s\n' "$dir"
    found=0
    for f in "$dir"/*.db "$dir"/*.sqlite; do
      [ -f "$f" ] || continue
      case $(basename "$f") in
        *_backup_*) continue ;;   # bkmr 7.x auto-migration artefacts
      esac
      found=1
      report "$f"
    done
    [ "$found" -eq 1 ] || printf '  (none)\n'
    printf '\nNo database is "active". Bind one per context in .aikit/project.toml.\n'
    exit 0 ;;
esac

[ -n "$name" ] || usage
case $name in
  */*|.|..) die "a project name may not contain a path separator" 64 ;;
esac

command -v bkmr >/dev/null 2>&1 || die \
  "bkmr is not on PATH; enable tool/search/bkmr or run script/install/bkmr."

path="$dir/$name.db"
[ ! -e "$path" ] || die "already exists: $path
Refusing to touch it. Remove it yourself if that is what you meant." 78

mkdir -p "$dir"
bkmr create-db "$path" || die "bkmr create-db failed for $path"

cat <<EOF

Created $path

Bind it to a project by adding this to <repo>/.aikit/project.toml:

    [integrations.bkmr]
    db = "$name"

AIKit resolves that per context and exports AIKIT_BKMR_DB and BKMR_DB_URL into
the context's environment. Two sessions on two projects therefore get two
databases; nothing global changes, and nothing else's context is disturbed.

Convention worth keeping from the old wrapper: tag every row you add here with
_$name, so that rows remain attributable if databases are ever merged.
EOF
