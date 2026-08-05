# Using and Verifying AIKit

AIKit is usable as a local binary. It does not require a daemon, and discovery is
read-only until a command explicitly installs or applies something.

## Install the current checkout

```sh
cargo install --path crates/aikit-cli
aikit status
aikit doctor
```

`aikit init --json` inventories foreign skill roots and npx `skills` provenance.
It reads global lock version 3 and project lock version 1, including the
`XDG_STATE_HOME` global location. It never runs npx, rewrites a lock, or adopts
those skills into AIKit ownership. It computes the lock's native content hash
read-only and reports `hash_matches`, so a locally changed installed skill is
distinguished from the version recorded by npx.

Useful checks:

```sh
aikit init --json
aikit tree --all
aikit mux detect --json
```

The mux report separates binaries installed on `PATH`, running servers/apps,
whether this process is inside each mux, the effective topology stack, and a mux
declared by the current context. If both tmux and cmux are installed, commands
that would otherwise have to guess require an explicit mux name.

## tmux

Install the managed integration:

```sh
aikit mux install tmux
```

This writes a marked, reversible block in `~/.tmux.conf`, refuses to steal an
existing `Alt-A` binding, reloads a running tmux server, and verifies the
effective root key table. `Alt-A` then opens one 82% × 70% AIKit popup. The
command output includes the Procedure ID and its exact undo command.

To choose another global key or deliberately replace a collision:

```sh
aikit mux install tmux --key M-k
aikit mux install tmux --replace-key
```

## cmux

Install the managed integration explicitly:

```sh
aikit mux install cmux
```

For cmux 0.63, AIKit adds an `AIKit` command to the native Command Palette in
`~/.config/cmux/cmux.json`. This cmux release does not expose a supported
arbitrary-command global hotkey, so AIKit reports the limitation instead of
inventing a binding. The JSON/JSONC merge preserves unrelated bytes and comments,
refuses a foreign command named `AIKit`, and is exactly reversible.

cmux defaults its control socket to “cmux processes only.” That is a security
boundary, not an installation failure. Run `aikit session ...` inside a cmux
terminal, or intentionally choose an appropriate automation mode in cmux
Settings. AIKit never weakens that setting.

AIKit reads cmux topology before changing it. Workspace titles and surface
markers include AIKit's durable session ID, so two projects can both use ordinary
names such as `dev/main/shell` without claiming each other's topology. AIKit
preflights every matching workspace before its first mutation, tags only surfaces
it creates, adds missing tagged panes, preserves untagged user surfaces, and
under `--destructive` closes only objects carrying that session's ownership
marker. A same-named untagged workspace is refused as ambiguous.

## Portable sessions

Bring up a session capsule or TOML spec:

```sh
aikit session up path/to/session.toml
aikit session diff path/to/session.toml
aikit session reconcile path/to/session.toml
```

`session up` is idempotent. `session diff` uses a separate inspection path that
cannot create, retag, focus, or close topology, including when a running session
has drifted. Reconciliation preserves extra panes by default:

```sh
aikit session reconcile path/to/session.toml --destructive
```

The destructive form is still ownership-bounded: AIKit will not close untagged
cmux surfaces or unrelated workspaces.

`aikit session attach NAME --json` and `aikit session down NAME --json` scope
the lookup to the current project and any explicitly selected mux. cmux handles
are resolved again from the durable session marker every time, because cmux can
change window and workspace refs after an app restart. The JSON response keeps
the legacy `command` field and also includes `commands`; an ungrouped cmux
session can need several ownership-bounded `close-workspace` commands.

The operational store at `~/.aikit` is never treated as a project marker. A
project is scoped by its own `.aikit/` directory (or its exported
`AIKIT_PROJECT_ID`), even when `AIKIT_HOME` points somewhere custom.

## What “working” means

At minimum, all of these should succeed:

```sh
aikit status --json
aikit doctor --json
aikit init --json
aikit mux detect --json
aikit tree --all --ascii
```

For tmux, `aikit mux install tmux` must report `verified: true`, and `Alt-A` must
open the popup from an existing pane. For cmux 0.63, `aikit mux install cmux`
must report the `command-palette:AIKit` binding; opening that entry should run
the same unified AIKit surface in the current cmux terminal.
