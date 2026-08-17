# Contributing to AIKit

AIKit sits between user configuration, agent clients, shells, multiplexers, and
live work. Correctness and reversibility take priority over convenience.

## Before changing code

1. Read [STANDARDS.md](STANDARDS.md) and the relevant section of
   [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
2. Preserve the distinction between read-only discovery, immutable generation
   materialization, and external mutation through a Procedure.
3. Add a failing test that exercises the real behaviour before implementing a
   feature or bug fix.

Mocks that merely restate an implementation are not sufficient. Prefer real
filesystems, SQLite databases, subprocesses, PTYs, and private tmux servers.
Recorded cmux protocol fixtures are appropriate only where cmux's own automation
boundary prevents safe external control.

## Verification

Run the same repository-owned operation as CI:

```sh
bash scripts/verify
```

It runs the workspace test suite, clippy with warnings denied, the release build,
and `git diff --check`. Keep this operation and the CI invocation aligned rather
than duplicating verification semantics in workflow YAML.

The repository contains older formatting that is being cleaned incrementally.
Format files you materially change, but do not mix a repository-wide mechanical
reformat into a behavioural pull request.

Tests that create private tmux sockets need a real `tmux` binary. On macOS they
may also need to run outside an unusually restrictive filesystem sandbox.

## Pull requests

A pull request should state:

- the user-visible outcome;
- the authority or state boundary it changes;
- how failure and reversal behave;
- which real tests prove the result;
- any platform capability that remains unavailable.

Do not hide incomplete behaviour behind successful no-ops, catch-all dispatch,
placeholder output, or an optimistic capability probe.

## Commit scope

Keep commits reviewable and do not overwrite unrelated changes in a dirty
worktree. Generated build output and local AIKit state do not belong in git.

By contributing, you agree that your contribution is licensed under the
project's MIT OR Apache-2.0 terms.
