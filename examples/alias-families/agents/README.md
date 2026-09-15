# `agents` — the first alias-family flagship (Stage 0, pre-integration)

One command that keeps a persistent room of coding agents — pi, hermes,
codex — on the owner's Omarchy machine, herdr by default with tmux as the
explicit fallback. It is the working reference for ADR 0005: a user-owned
command family composed from suite primitives, awaiting the `aikit alias`
surface that will generalise it.

## Files

| File | Destination on the machine |
|---|---|
| `agents` | `~/.local/bin/agents` (executable) |
| `room.conf` | `~/.config/agents/room.conf` (owner-editable slot table) |

Runtime state lives in `~/.local/state/agents-room/` (the place grant and a
herdr-server log). `agents persist` optionally installs a systemd **user**
unit (`~/.config/systemd/user/herdr-server.service`) so the herdr server
starts at login and restarts on failure; `agents persist --off` removes it.

## UX

```text
agents [up] [--provider auto|herdr|tmux] [--no-attach] [--json]
agents attach   [--provider herdr|tmux]
agents status   [--json]
agents down     [--provider herdr|tmux]
agents doctor   [--json]
agents persist  [--off]
```

Room creation goes through `workcell place request` (generation-proofed
grant, typed `already-exists` refusal — adoption is explicit). On the tmux
path, if workcell cannot observe what it created (pre-#85 builds on tmux 3.7),
the command says so, removes the refused debris and claims natively,
disclosed.

## Persistence semantics, honestly proven (2026-09-15, omarchy)

- **ssh disconnect / reconnect** — agents keep running; the provider server
  holds them. Verified from fresh connections.
- **provider server restart / reboot** — herdr restores workspace and pane
  topology with identical pane ids; agent *processes* are gone. `agents up`
  re-seeds them into the restored panes (idempotent: running twice never
  duplicates; it adopts what is alive and seeds only what is missing).
- **pane killed** — status reports the slot absent; `up` recreates the pane
  and starts the agent.
- **teardown** — `down` releases through the workcell grant (pid +
  process-start-marker proof) and falls back to direct close, disclosed, when
  the proof is stale.

## Security posture (UI07)

`agents doctor` confirms auth material is present (`~/.pi/agent/auth.json`,
`~/.hermes/auth.json`, `~/.codex/auth.json`) without reading it. No command
reads, prints, stores or transmits key material.

## Design mapping (how this becomes a family manifest entry)

`room.conf`'s slot table is the seed of `aikit.alias-family/v1`:

```text
AGENTS_ROOM / AGENTS_PROVIDER   → family identity and default provider
AGENTS_SLOTS rows (name kind cwd) → family entries: label, harness kind, cwd
up/attach/status/down           → room verbs composed from place grants
doctor                          → presence checks over credential refs
```

Stage 1 replaces the hand-installed script with an `aikit alias install`
emission of the same shape; the manifest cites the harness registry, route
refs and place providers instead of hardcoding them.

## Test evidence

Live test log: O-I repository,
`campaign-evidence/2026-09-15-launcher-portal/` (discovery report, per-step
logs: first claim, idempotent re-up, pane-kill re-seed, server-restart
re-seed, systemd persist handover, tmux fallback claim/down, release-by-proof).
