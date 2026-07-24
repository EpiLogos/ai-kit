#!/bin/sh
# install.sh - install or upgrade bkmr to 7.x, explicitly.
#
# This is how AIKit installs a dependency without becoming a package manager:
# never implicitly during resolution, always a capsule you run on purpose. It
# prints a plan and does nothing until you pass --apply.
#
# It refuses the upgrade when that upgrade would destroy embeddings, unless you
# say --allow-reembed. bkmr 7.0.0's migration runs
#
#     UPDATE bookmarks SET embedding = NULL WHERE embedding IS NOT NULL;
#
# automatically, on the first command that touches each database, with no
# prompt. Every 6.x vector is discarded and must be regenerated with the new
# local model. That is not a thing to discover afterwards.
#
# This script never writes to a database, never runs backfill, and never
# migrates anything. It installs a binary and tells you what happens next.

set -eu

me=${0##*/}
warn() { printf '%s: %s\n' "$me" "$1" >&2; }
die() { printf '%s: %s\n' "$me" "$1" >&2; exit "${2:-1}"; }

usage() {
  cat >&2 <<EOF
usage: $me [options]

      --apply           actually run cargo install (default: print the plan only)
      --version V       install this version (default: latest 7.x from crates.io)
      --allow-reembed   proceed even though upgrading invalidates existing
                        6.x embeddings, which must be regenerated afterwards
      --scan DIR        also scan DIR for bkmr databases (repeatable)

Read contrib/bkmr/UPGRADE.md before using --apply.
EOF
  exit 64
}

apply=0
want=
allow=0
scan_extra=

while [ $# -gt 0 ]; do
  case $1 in
    --apply)         apply=1; shift ;;
    --allow-reembed) allow=1; shift ;;
    --version)       [ $# -ge 2 ] || usage; want=$2; shift 2 ;;
    --scan)          [ $# -ge 2 ] || usage; scan_extra="$scan_extra $2"; shift 2 ;;
    -h|--help)       usage ;;
    *)               die "unexpected argument: $1" 64 ;;
  esac
done

case ${want:-7} in
  7|7.*) ;;
  *) die "this capsule installs bkmr 7.x. --version $want is not 7.x; the 6.x
     semantic path needs a Gemini key and a network round trip, and the
     capsules here declare effects for the 7.x local path." 64 ;;
esac

command -v cargo >/dev/null 2>&1 || die \
  "cargo is not on PATH. bkmr 7.x is also published to Homebrew and PyPI;
this capsule deliberately supports only the cargo path so that the build
inputs are the ones this repository documents."

# ---- architecture ---------------------------------------------------------
#
# The general rule, not a bkmr special case: never trust `rustc -vV`'s host
# triple to tell you the hardware. Under Rosetta on Apple Silicon an x86_64
# rustc reports `x86_64-apple-darwin` on arm64 hardware, and `cargo install`
# then builds an emulated x86_64 binary that pulls the x86_64 ONNX Runtime for a
# CPU-bound embedding workload. The hardware is the source of truth; ask the
# kernel, then pin the build to the native target so a Rosetta toolchain still
# produces a native binary, and verify the artefact afterwards.
hw=$(uname -m)
os=$(uname -s)
case "$os/$hw" in
  Darwin/arm64)       native=aarch64-apple-darwin ;;
  Darwin/x86_64)      native=x86_64-apple-darwin ;;
  Linux/aarch64|Linux/arm64) native=aarch64-unknown-linux-gnu ;;
  Linux/x86_64)       native=x86_64-unknown-linux-gnu ;;
  *)                  native= ;;
esac

rust_host=$(rustc -vV 2>/dev/null | awk '/^host:/ {print $2}')
target_flag=
if [ -n "$native" ]; then
  printf 'hardware       : %s %s → native target %s\n' "$os" "$hw" "$native"
  if [ "$rust_host" != "$native" ]; then
    warn "the active Rust toolchain host is $rust_host but this machine is $native."
    warn "a plain \`cargo install\` would build an emulated $rust_host binary."
    if command -v rustup >/dev/null 2>&1; then
      if rustup target list --installed 2>/dev/null | grep -qx "$native"; then
        target_flag="--target $native"
        warn "pinning the build to --target $native so the artefact is native."
      else
        warn "the native std for $native is not installed. Either fix the whole"
        warn "toolchain (best, one time):"
        warn "    rustup set default-host $native && rustup default stable"
        warn "or add just the target so this build can be pinned to it:"
        warn "    rustup target add $native"
        die  "refusing to build an emulated binary. Fix the toolchain and re-run." 70
      fi
    else
      warn "rustup is not present, so the build cannot be re-targeted here."
      warn "install a native toolchain before proceeding, or the result will be $rust_host."
    fi
  fi
else
  warn "unrecognised platform $os/$hw: cannot verify the build architecture."
fi

# ---- current state -------------------------------------------------------
current=none
if command -v bkmr >/dev/null 2>&1; then
  current=$(bkmr --version 2>/dev/null | awk 'NR==1 {print $NF}')
fi
printf 'installed bkmr : %s\n' "$current"
printf 'target         : %s\n' "${want:-latest 7.x}"

# ---- which databases would lose their vectors ----------------------------
scan_dirs="$HOME/.config/bkmr $HOME/.config/bkmr/projects $scan_extra"
at_risk=0
risk_list=

if command -v sqlite3 >/dev/null 2>&1; then
  for d in $scan_dirs; do
    [ -d "$d" ] || continue
    for f in "$d"/*.db "$d"/*.sqlite; do
      [ -f "$f" ] || continue
      case $(basename "$f") in *_backup_*) continue ;; esac
      n=$(sqlite3 -readonly "$f" \
        'SELECT COUNT(*) FROM bookmarks WHERE embedding IS NOT NULL;' 2>/dev/null || echo 0)
      [ "$n" -gt 0 ] 2>/dev/null || continue
      at_risk=$((at_risk + n))
      risk_list="$risk_list
  $n  $f"
    done
  done
else
  warn "sqlite3 not found: cannot count embeddings that this upgrade would discard."
fi

if [ "$at_risk" -gt 0 ]; then
  printf '\n%s legacy 6.x embeddings would be discarded by the 7.x migration:%s\n' \
    "$at_risk" "$risk_list"
  cat <<'EOT'

They are regenerated by `bkmr backfill --force`, per database, after the
upgrade. bkmr 7.x does write <name>_backup_YYYYMMDD.db beside each database
before migrating it, but that name carries only a date, so a second migration
on the same day overwrites the first backup. Take your own, timestamped:
script/bkmr/project-snapshot.
EOT
else
  printf '\nno legacy 6.x embeddings found in the scanned directories.\n'
fi

# ---- plan ----------------------------------------------------------------
if [ -n "$want" ]; then
  set -- install bkmr --version "$want" --locked
else
  set -- install bkmr --version '^7' --locked
fi
# shellcheck disable=SC2086 # target_flag is intentionally word-split (flag + value)
[ -n "$target_flag" ] && set -- "$@" $target_flag

printf '\nplan:\n  cargo %s\n' "$*"
printf '  (cargo builds fastembed with the ort-download-binaries feature, so the\n'
printf '   build itself fetches an ONNX Runtime binary; the ~130 MB embedding\n'
printf '   model is fetched separately, on the first backfill, into the bkmr\n'
printf '   model cache.)\n'

if [ "$at_risk" -gt 0 ] && [ "$allow" -eq 0 ]; then
  die "
refusing to proceed: this upgrade invalidates $at_risk embeddings.
Re-run with --allow-reembed once you have read contrib/bkmr/UPGRADE.md and
taken snapshots. Nothing has been changed." 78
fi

if [ "$apply" -eq 0 ]; then
  printf '\nplan only; nothing was installed. Re-run with --apply.\n'
  exit 0
fi

cargo "$@" || die "cargo install failed; the existing bkmr is untouched."

# ---- verify --------------------------------------------------------------
hash -r 2>/dev/null || true
new=$(bkmr --version 2>/dev/null | awk 'NR==1 {print $NF}')
[ -n "$new" ] || die "bkmr is not runnable after install."
major=${new%%.*}
case $major in ''|*[!0-9]*) die "unreadable version after install: $new" ;; esac
[ "$major" -ge 7 ] || die "expected 7.x after install, got $new.
An older bkmr earlier on PATH is the usual cause; \`command -v bkmr\` says
$(command -v bkmr)."

# Verify the artefact is the architecture we intended, not an emulated build.
# `file` reads the Mach-O/ELF header; this is the check that would have caught
# the Rosetta problem silently in the first place.
bkmr_path=$(command -v bkmr)
if [ -n "$native" ] && command -v file >/dev/null 2>&1; then
  desc=$(file -b "$bkmr_path" 2>/dev/null || echo "")
  case "$hw:$desc" in
    arm64:*arm64*|arm64:*aarch64*) : ;;
    x86_64:*x86_64*) : ;;
    arm64:*x86_64*)
      die "installed bkmr is an x86_64 binary on arm64 hardware — it will run
under Rosetta and pulled the x86_64 ONNX Runtime for a CPU-bound workload.
Fix the toolchain (rustup set default-host $native && rustup default stable)
and re-run. Location: $bkmr_path" 70 ;;
    *) warn "could not confirm architecture from: $desc" ;;
  esac
  printf 'architecture   : ok (%s)\n' "$hw"
fi

printf '\ninstalled bkmr %s at %s\n' "$new" "$bkmr_path"
cat <<'EOF'

Next, per database, and not by this script:

  1. snapshot it            aikit run script/bkmr/project-snapshot
  2. let 7.x migrate it     bkmr --db <path> search --np --limit 1 ""
  3. regenerate vectors     bkmr --db <path> backfill --force
  4. check                  bkmr --db <path> info

Step 2 is what triggers the irreversible `SET embedding = NULL`. Step 3 is
the only thing that makes semantic search work again.
EOF
