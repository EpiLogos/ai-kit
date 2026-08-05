# AIKit

**A context-scoped control plane for agentic terminal work.**

[![CI](https://github.com/EpiLogos/ai-kit/actions/workflows/ci.yml/badge.svg)](https://github.com/EpiLogos/ai-kit/actions/workflows/ci.yml)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-93450a.svg)](https://www.rust-lang.org/)
[![License: MIT or Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

AIKit resolves the capabilities that belong in the project, session, task, and
client you are using now. It gives shells, Claude, Codex, hooks, tmux, and cmux
different projections of one explainable state instead of maintaining a global
mutable pile of skills and scripts.

> **Status:** production-oriented alpha. The core, TUI, persistence, reversible
> procedures, tmux integration, cmux 0.63 integration, and foreign skills
> discovery are implemented and exercised against real binaries. Installation
> is currently from source; configuration changes remain explicit and
> reversible.

## Why AIKit

Agent tooling usually accumulates in unrelated global directories:

- skills copied or symlinked by several installers;
- hooks present on disk but wired to nothing;
- project-specific environment values leaking into other projects;
- tmux and cmux integrations that assume they own an existing session;
- agent tasks that claim isolation while sharing the same working tree.

AIKit treats those as one lifecycle problem. It resolves a capability graph per
context, records why every capability is active or unavailable, materializes
immutable generations, and changes external configuration only through
reviewable Procedures with exact undo information.

## What is implemented

| Area | Behaviour |
|---|---|
| Unified TUI | Fast palette and hierarchical tree in one resident process; keyboard and mouse paths share the same staged graph. |
| tmux | Managed `Alt-A` 82% × 70% popup, live binding verification, collision refusal, idempotent install, exact undo. |
| cmux 0.63 | Native Command Palette entry, lossless JSON/JSONC merge, durable session ownership, live handle rebinding, ownership-bounded teardown. |
| Sessions | Portable TOML topology, idempotent `up`, physically read-only `diff`, additive or exact reconciliation. |
| Skills | Read-only discovery across agent roots plus npx `skills` global-v3/project-v1 provenance and native hash verification. |
| Clients | Context projections for Claude and Codex, including honest shared-tree fallbacks and opt-in task isolation. |
| Safety | Trust gates, secret quarantine, immutable generations, compare-and-swap apply, reversible external mutations. |

## Install

Requirements:

- Rust 1.88 or newer;
- tmux and/or [cmux](https://github.com/manaflow-ai/cmux) only if you want those
  integrations;
- Node.js only when verifying hashes from an npx `skills` lock.

```sh
git clone https://github.com/EpiLogos/ai-kit.git
cd ai-kit
cargo install --locked --path crates/aikit-cli
```

AIKit is daemonless. Every command can start on a fresh machine.

## Five-minute start

Inspect the machine without adopting or rewriting anything:

```sh
aikit init --json
aikit mux detect --json
aikit tree --all
aikit doctor --json
```

Open the unified interface directly:

```sh
aikit ui
```

Install one managed multiplexer entry:

```sh
aikit mux install tmux
# or
aikit mux install cmux
```

- **tmux:** `Alt-A` opens the real popup. AIKit refuses to steal an existing
  binding unless `--replace-key` is explicit.
- **cmux 0.63:** open `AIKit` from the native Command Palette. cmux does not
  expose a supported arbitrary-command global hotkey in this release, so AIKit
  does not invent one.

Every install reports the Procedure ID and undo command. AIKit never weakens
cmux's automation security setting; session commands that control cmux should
run inside a cmux terminal unless you intentionally change that setting.

## The interface

The popup starts in the capability palette. Its footer always shows the keys
that are valid in the current mode.

| Key | Action |
|---|---|
| Type | Filter capabilities |
| `Space` | Stage a change (`Ctrl-Space` while typing) |
| `Ctrl-Enter` | Apply the staged graph |
| `Enter` | Run or inspect the selected capability |
| `Alt-Enter` | Run in a new pane when the active mux supports it |
| `Ctrl-T` | Switch between palette and tree without restarting the TUI |
| `?` | Show the complete contextual key list |
| `Esc` | Dismiss the current layer, then close |

Durable project-level writes require a second confirmation. Staging alone never
changes disk state.

## Portable sessions

```toml
schema = 1
id = "dev"
name = "dev"
root = "."

[backend]
kind = "tmux" # or "cmux"

[[views]]
id = "main"

[[views.panes]]
id = "editor"
command = ["sh", "-l"]

[[views.panes]]
id = "tests"
split_from = "editor"
direction = "right"
command = ["cargo", "test"]
```

```sh
aikit session up session.toml
aikit session diff session.toml
aikit session reconcile session.toml
```

`diff` has a separate inspection path and cannot create, focus, retag, or close
topology. Reconciliation preserves unowned panes and surfaces. Even
`--destructive` is bounded by AIKit's durable session ownership markers.

## Existing tools remain authoritative

Discovery is not adoption. `aikit init` reads the skill roots and supported
lockfiles already present on the machine without running npx, changing the lock,
or claiming ownership.

When a supported npx lock declares a hash, AIKit computes the same Git-tree
SHA-1 or folder SHA-256—including Node's exact `localeCompare` ordering—and
reports whether the installed content drifted. The original lock bytes are left
untouched.

Adoption, when requested, is a Procedure: preview first, apply with explicit
confirmation, and retain an exact undo path.

## How to know it is working

```sh
aikit status --json
aikit doctor --json
aikit init --json
aikit mux detect --json
aikit tree --all --ascii
```

For tmux, installation should report `verified: true` and `Alt-A` should open the
popup from an existing pane. For cmux 0.63, installation should report
`command-palette:AIKit`, and that entry should open the same resident AIKit
surface.

## Documentation

- [Using and verifying AIKit](docs/USING-AIKIT.md)
- [Architecture and state model](docs/ARCHITECTURE.md)
- [Procedures, inbox, trust, and reversal](docs/SPEC-II-PROCEDURES-AND-INBOX.md)
- [Skillsets, frecency, and tree semantics](docs/SPEC-III-SKILLSETS-AND-FRECENCY.md)
- [Skills ecosystem compatibility](docs/SKILLS-ECOSYSTEM.md)
- [Engineering standards](STANDARDS.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)

## Development

```sh
cargo test --locked --workspace --all-targets --no-fail-fast
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --workspace --release
git diff --check
```

The suite includes real tmux servers, real subprocesses, real SQLite state,
PTY-driven TUI flows, and fresh-machine binary acceptance tests. Protocol
fixtures cover cmux paths that cannot safely run outside cmux's automation
boundary; real-machine detection verifies the installed binary and access
state.

## License

Licensed at your option under either:

- [Apache License, Version 2.0](LICENSE-APACHE), or
- [MIT License](LICENSE-MIT).
