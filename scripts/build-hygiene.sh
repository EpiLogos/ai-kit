#!/usr/bin/env bash
#
# build-hygiene.sh — report, clean, and prune AIKit's Rust build state.
#
# The habits this script encodes:
#   1. One shared cargo target dir for the main checkout and every worktree,
#      so N checkouts do not mean N full build trees.
#   2. Worktrees are removed the moment their branch merges, so a merged branch
#      never leaves a checkout (or its target dir) behind.
#
# Nothing is deleted without being listed first. Non-interactive runs require
# --yes. Runs that would race a live cargo/rustc are refused unless --force.
#
# Usage:
#   build-hygiene.sh [command] [flags]
#
# Commands:
#   report   (default) Show target dirs, merged-but-open worktrees, stale
#            registrations, and the shared target dir.
#   clean    Remove target directories. Selectors:
#            --worktrees  worktree target dirs only
#            --repo       main checkout target dir only
#            --shared     the shared/user-level target dir(s) only
#            --all        (default) worktrees + repo + shared
#   prune    Drop stale registrations, then remove merged worktrees and their
#            branches. Dirty merged worktrees are kept unless --force.
#   gc       report, then prune, then clean --all.
#
# Flags:
#   --yes        answer the confirmation prompt in advance
#   --force      allow removal of dirty worktrees and running-build races
#   --help       show this text
#
set -euo pipefail

FLAG_YES=0
FLAG_FORCE=0
SELECTOR="all"

usage() {
  sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
  printf 'build-hygiene: %s\n' "$*" >&2
  exit 1
}

human_size() {
  local kb=$1
  awk -v kb="$kb" 'BEGIN {
    if (kb >= 1048576) printf "%.1fG\n", kb / 1048576
    else if (kb >= 1024) printf "%.1fM\n", kb / 1024
    else printf "%dK\n", kb
  }'
}

dir_size() {
  local dir=$1
  if [ -d "$dir" ]; then
    du -sk "$dir" 2>/dev/null | awk '{print $1}'
  else
    echo 0
  fi
}

# The main checkout of the repository this invocation belongs to, regardless
# of whether we are inside it or inside one of its worktrees.
main_checkout() {
  local common
  common=$(git rev-parse --git-common-dir)
  case "$common" in
    /*) ;;
    *) common="$PWD/$common" ;;
  esac
  cd "$common/.." && pwd
}

# Tab-separated "path<TAB>branch" lines for every worktree, main checkout
# included. Detached worktrees carry an empty branch.
list_worktrees() {
  git worktree list --porcelain | awk '
    /^worktree / { wt = $2; branch = "" }
    /^branch / { branch = substr($2, 12) }
    /^$/ { if (wt != "") print wt "\t" branch; wt = ""; branch = "" }
    END { if (wt != "") print wt "\t" branch }
  '
}

default_branch() {
  local b
  for b in main master; do
    if git rev-parse --verify -q "refs/heads/$b" >/dev/null 2>&1; then
      printf '%s\n' "$b"
      return
    fi
  done
  printf ''
}

branch_is_merged() {
  local branch=$1 default=$2
  [ -n "$branch" ] && [ -n "$default" ] &&
    [ "$branch" != "$default" ] &&
    git merge-base --is-ancestor "refs/heads/$branch" "refs/heads/$default" 2>/dev/null
}

worktree_is_dirty() {
  [ -n "$(git -C "$1" status --porcelain 2>/dev/null)" ]
}

prunable_count() {
  git worktree list 2>/dev/null | grep -c 'prunable' || true
}

build_is_running() {
  pgrep -x cargo >/dev/null 2>&1 || pgrep -x rustc >/dev/null 2>&1
}

confirm() {
  local what=$1
  if [ "$FLAG_YES" = 1 ]; then
    return 0
  fi
  if [ ! -t 0 ]; then
    fail "refusing to run non-interactively without --yes"
  fi
  printf '%s (y/N) ' "$what" >&2
  local ans
  read -r ans || return 1
  case "$ans" in
    y | Y) return 0 ;;
    *) return 1 ;;
  esac
}

target_dirs() {
  local main main_target wt wt_target shared
  main=$(main_checkout)
  main_target="$main/target"

  if [ "$SELECTOR" = "worktrees" ] || [ "$SELECTOR" = "all" ]; then
    while IFS=$'\t' read -r wt _branch; do
      [ -n "$wt" ] || continue
      [ "$wt" = "$main" ] && continue
      wt_target="$wt/target"
      if [ -d "$wt_target" ]; then
        printf '%s\n' "$wt_target"
      fi
    done < <(list_worktrees)
  fi

  if [ "$SELECTOR" = "repo" ] || [ "$SELECTOR" = "all" ]; then
    if [ -d "$main_target" ]; then
      printf '%s\n' "$main_target"
    fi
  fi

  if [ "$SELECTOR" = "shared" ] || [ "$SELECTOR" = "all" ]; then
    if [ -n "${CARGO_TARGET_DIR:-}" ] && [ -d "$CARGO_TARGET_DIR" ]; then
      printf '%s\n' "$CARGO_TARGET_DIR"
    fi
    local home_target="$HOME/.cargo/target"
    if [ -d "$home_target" ]; then
      printf '%s\n' "$home_target"
    fi
  fi
}

cmd_report() {
  local main default
  main=$(main_checkout)
  default=$(default_branch)

  printf 'Build hygiene report\n'
  printf '  main checkout : %s\n' "$main"
  printf '  default branch: %s\n' "${default:-<none>}"

  local total=0 size
  while IFS=$'\t' read -r wt branch; do
    [ -n "$wt" ] || continue
    size=$(dir_size "$wt/target")
    total=$((total + size))
    local label="worktree"
    [ "$wt" = "$main" ] && label="main"
    local marks=()
    if [ "$wt" != "$main" ]; then
      marks+=("$branch")
      if branch_is_merged "$branch" "$default"; then
        marks+=("merged")
      else
        marks+=("unmerged")
      fi
      worktree_is_dirty "$wt" && marks+=("dirty")
    fi
    printf '  %-9s %10s  %s\n' "$label" "$(human_size "$size")" "$wt"
    if [ -n "${marks[*]:-}" ]; then
      printf '             %10s  %s\n' "" "${marks[*]}"
    fi
  done < <(list_worktrees)

  local shared_total=0
  if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    size=$(dir_size "$CARGO_TARGET_DIR")
    shared_total=$((shared_total + size))
    printf '  shared (env) : %10s  %s\n' "$(human_size "$size")" "$CARGO_TARGET_DIR"
  fi
  if [ -d "$HOME/.cargo/target" ]; then
    size=$(dir_size "$HOME/.cargo/target")
    shared_total=$((shared_total + size))
    printf '  shared (user) : %9s  %s\n' "$(human_size "$size")" "$HOME/.cargo/target"
  fi

  local stale
  stale=$(prunable_count)
  if [ "$stale" -gt 0 ]; then
    printf '  stale registrations: %d (worktree dir gone; run `prune`)\n' "$stale"
  fi
  printf '  total target bytes: %s\n' "$(human_size $((total + shared_total)))"
}

cmd_clean() {
  local -a dirs=()
  local d
  while IFS= read -r d; do
    [ -n "$d" ] && dirs+=("$d")
  done < <(target_dirs)

  if [ "${#dirs[@]}" -eq 0 ]; then
    printf 'Nothing to clean.\n'
    return
  fi

  if build_is_running; then
    if [ "$FLAG_FORCE" = 1 ]; then
      printf 'warning: cargo/rustc is running; cleaning anyway (--force)\n' >&2
    else
      fail "cargo/rustc is running; finish the build or pass --force"
    fi
  fi

  printf 'Removing target directories:\n'
  local d size total=0
  for d in "${dirs[@]}"; do
    size=$(dir_size "$d")
    total=$((total + size))
    printf '  %10s  %s\n' "$(human_size "$size")" "$d"
  done
  printf '  total: %s\n' "$(human_size "$total")"

  confirm "Remove these build caches?" || {
    printf 'Aborted.\n'
    return 1
  }

  local removed=0
  for d in "${dirs[@]}"; do
    rm -rf -- "$d"
    removed=$((removed + 1))
    printf 'removed %s\n' "$d"
  done
  printf 'Cleaned %d target director%s.\n' "$removed" "$([ "$removed" = 1 ] && echo y || echo ies)"
}

cmd_prune() {
  local default
  default=$(default_branch)
  git worktree prune

  local removed=0
  while IFS=$'\t' read -r wt branch; do
    [ -n "$wt" ] || continue
    [ "$wt" != "$(main_checkout)" ] || continue
    if ! branch_is_merged "$branch" "$default"; then
      printf 'keep %s (%s): unmerged\n' "$wt" "${branch:-detached}"
      continue
    fi
    if worktree_is_dirty "$wt" && [ "$FLAG_FORCE" != 1 ]; then
      printf 'keep %s (%s): merged but dirty; pass --force to discard\n' "$wt" "$branch"
      continue
    fi
    if worktree_is_dirty "$wt"; then
      git worktree remove --force "$wt"
    else
      git worktree remove "$wt"
    fi
    git branch -d "$branch"
    removed=$((removed + 1))
    printf 'removed %s (%s)\n' "$wt" "$branch"
  done < <(list_worktrees)

  if [ "$removed" -eq 0 ]; then
    printf 'No merged worktrees to remove.\n'
  fi
}

cmd_gc() {
  cmd_report
  printf '\n'
  cmd_prune
  printf '\n'
  cmd_clean
}

main() {
  local command="report"
  local args=("$@")
  local i
  for i in "${!args[@]}"; do
    case "${args[$i]}" in
      report | clean | prune | gc) command="${args[$i]}" ;;
      --yes) FLAG_YES=1 ;;
      --force) FLAG_FORCE=1 ;;
      --worktrees) SELECTOR="worktrees" ;;
      --repo) SELECTOR="repo" ;;
      --shared) SELECTOR="shared" ;;
      --all) SELECTOR="all" ;;
      --help | -h)
        usage
        exit 0
        ;;
      *)
        fail "unknown argument: ${args[$i]} (try --help)"
        ;;
    esac
  done

  case "$command" in
    report) cmd_report ;;
    clean) cmd_clean ;;
    prune) cmd_prune ;;
    gc) cmd_gc ;;
  esac
}

main "$@"
