# Gateway status and actions — the contract desktop consumers call

Native owner: AIKit (`aikit`). This page names the exact readings and actions a
desktop surface (O-I lane C) calls, with their command lines and the JSON shape
each returns. Every command is machine-readable with `--json` and answers as an
envelope: `{"ok": true, "data": {...}, "warnings": []}` — `ok:false` carries
`error.code` and a three-part `error.details` (fact / consequence / action).

Ownership lanes: lane A owns material providers and capacity; lane C owns the
aggregate NOW view and desktop consumers; this gateway plane (lane B, this
commission) owns routing, delivery, connector adapters, access/lifecycle and
the conversational TUI. Consumers call these commands; they never open the
gateway state file.

## Status readings (all read-only)

```sh
aikit gateway status --json        # counters + connector health + build identity
aikit gateway ecology --json       # live agencies → sessions → streams → surfaces
aikit gateway discover --json      # connectors, capabilities, bindings (roster source)
aikit gateway protocol --json      # negotiated features + build (peer-readable)
aikit gateway who --json           # Positions with occupancy and undelivered Communiques
aikit gateway doctor --json        # every finding, each with the command that fixes it
aikit gateway upgrade plan --json  # running vs installed vs peers, action to take
aikit gateway recover --deliveries --json   # pending outbound operations (owner)
```

Fields consumers should surface:

- `status.build` — `{revision, pid, started_at_unix_ms, executable_sha256,
  workcell_ref, lifecycle, oi_revision}`: which build answers, since when, and
  which `oi` the machine runs. `oi_revision` names a mixed-oi fleet.
- `status.connector_health[]` — `{connector_ref, state, detail}`; `state` is
  `connected | degraded | reconnecting | unavailable | …`.
- `status.pending_delivery_count` / `pending_operations` — outbound operations
  prepared and not receipted, named. A pending send is *held*, never re-sent.
- `doctor.findings[]` — `{code, severity: ok|info|warn|fail, fact, details,
  next}`; the verdict is the worst severity.
- `upgrade plan.peers[]` — each declared peer's `{workcell_ref, reachable,
  revision, oi_revision, missing_features}`.

## Actions

```sh
aikit gateway setup --mode <mode> [--apply]        # local-ipc | loopback-service |
                                                   # private-tailnet | tailscale-serve | ssh-tunnel
aikit gateway agent --binding <REF> status|stop|new|sessions|restart|pause|resume
aikit gateway forward                              # one relay pass now
aikit gateway recover --resolve <OP> --state delivered|abandoned --evidence "…"
aikit gateway upgrade apply --wait [--install]     # managed upgrade; receipt returns to the
                                                   # conversation that asked (`/upgrade apply`)
```

Laws the actions keep (consumers may rely on all of them):

1. **Carrier scope**: the unix socket and an owner token grant owner scope
   (drain, restore, bind, resolve, stop); a peer token grants relay and reads
   only. A denied peer is told which token to use
   (`agency_gateway.carrier_scope_denied`).
2. **Drain**: refuse new turns, bounded grace, interrupt and RECORD. The
   `DrainReport` names interrupted turns, unreceipted operations and messages
   admitted unserved. Nothing is replayed, ever.
3. **Upgrade**: plan → apply → drain → restart → verify (a *different* process
   answers as the *expected* build) → receipt into the asking conversation. A
   receipt that cannot be queued is an error the driver retries.
4. **Delivery**: an unreceipted operation is re-attempted only when idempotent;
   a send with unknown outcome is held for `recover --resolve` evidence.
5. **Sender attribution**: a relayed send carries its self-declared sender
   authenticated by the machine-level peer token; conversation /ask attribution
   comes from occupancy (`verified`), agency identity (`claimed`), or is
   labelled `<unknown sender>`.
6. **Remote targeting**: an addressed remote that cannot be resolved stays a
   remote connection error (TUI Absent-with-reason, CLI
   `gateway.remote_undeclared`), never a quiet local conversation.

## Conversation surface (TUI)

Ctrl+G opens the Conversation aperture over this home's gateway carrier (unix
socket by default; `AIKIT_GATEWAY_AT=workcell:<ref>` addresses a declared
remote's WebSocket carrier). Roster: ecology bindings; Enter opens the
conversation with history from the journal replay; live events append without
user action; typing composes; Enter sends through the gateway's own ingest
path; Esc back. A dropped carrier degrades visibly and reconnects from the
last seen sequence without duplicates; nothing the operator sent is re-sent by
a reconnect.

## Test conversation (Omarchy, live)

- Telegram: connector `gateway-connector/telegram/main` (bot `@Ohisysbot`).
  Binding `gateway-binding/telegram-frank-private/generation-2`, private chat
  `6381957258`, ingress deny-default + owner allowlist. Exactly one ingress
  poller: `aikit-gateway.service` (connector pump). Conversation commands:
  `/status /stop /new /sessions /restart /pause /resume /model /harness
  /skills /upgrade [apply] /ask <position> <message>`.
- Local TUI: `aikit` (Ctrl+G).
- Mac: `aikit gateway --at workcell:mac <command>` over the private tailnet.
