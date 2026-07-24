#!/bin/sh
# Run the workspace test suite.
#
# Prefer cargo-nextest for its parallelism and cleaner output, but fall back to
# `cargo test` so this capsule is useful on a machine that has not installed
# nextest yet. Either way, arguments are passed straight through, so
# `aikit run nt -- -p aikit-core` narrows to one crate.
set -eu

if command -v cargo-nextest >/dev/null 2>&1; then
    exec cargo nextest run "$@"
fi

echo "cargo-nextest is not installed; falling back to 'cargo test'." >&2
exec cargo test "$@"
