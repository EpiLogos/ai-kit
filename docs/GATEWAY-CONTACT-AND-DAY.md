# Gateway contact and the environmental DAY

Two AIKit parts of O:I World-rooted inhabitation (EpiLogos/O-I #65, #220;
EpiLogos/Factory #195), built against the pinned contract
`O-I/docs/contracts/WORLD-INHABITATION-V1.md` §4:

- **Contact**: an occupant of a World Position addresses another Position.
  The message (a Communique, `aikit.communique/v1`) is attributed from the
  sender's own occupancy, appended to the gateway's journal, and delivered at
  the recipient occupant's next turn. It never waits for a reply and never
  becomes obligation-bearing work unless someone delegates it.
- **DAY**: at the civil day boundary the gateway dispatcher opens Central's Day
  and rolls every Project NOW field forward by calling Central's Actions
  directly. No model runs.

AIKit owns neither Positions, occupancy nor custody. It asks Central
(`central.position.list|read`, `central.world.here`), Actuation (`actuation
occupancy list|read|verify`) and Factory (`factory development
current-work|custody assign`) through their real CLIs. An owner that cannot
answer is reported as `unavailable` with the exact command that failed.

## Contact

```sh
aikit gateway who [--project-world project:O-I] --json
aikit gateway send --to @cradle-steward --body "Cradle build is red on main." --json
aikit gateway send --to central:position:project:O-I:factory-guardian --reply-to aikit:communique:… --body-file reply.md
aikit gateway inbox [--position P] [--ack] --json
aikit gateway conversation --with @factory-guardian --json
aikit gateway delegate --communique aikit:communique:… --work work:cradle-fix [--run R] [--journey J] [--workflow-unit U] --reason "…"
aikit gateway forward --json
aikit gateway remote add --workcell workcell:omarchy --ws 100.92.62.101:7800 --token-location file:/Users/me/.aikit/omarchy-gateway.token
aikit gateway serve --ws HOST:PORT --ws-token-location file:/ABS/PATH --unix
aikit gateway install-service [--ws HOST:PORT --ws-token-location file:/ABS/PATH] [--workcell-ref W] [--gateway-ref G]
aikit gateway remote list | remove --workcell W
```

`who` returns `aikit.population-reading/v1`: every Position of the Project
World (its own and inherited), each with its occupancy (from Actuation's
uncapped listing), its current work (Factory), and how many Communiques wait
for it. Anything an owner could not answer is listed under `absences` with the
command that failed. An occupied Position that Central does not define is
still listed, marked `definition: absent`.

### Who the sender is

The sender is the Position this body was launched into: `OI_POSITION_REF` and
`OI_OCCUPANT_GENERATION` (or `--from-position`), verified current with
`actuation occupancy verify`. The record keeps both refs and a plain-words
`attribution_basis`:

| attribution | when |
|---|---|
| `verified` | Actuation confirms the generation is the Position's current occupant |
| `claimed` | a Position was named but its occupancy could not be verified (no generation, or Actuation unavailable) |
| `unknown` | no Position at all; the Communique is still delivered, labelled `<unknown sender>` |

A generation Actuation refuses (superseded, never held the Position) is
refused before anything is recorded (`gateway.sender_not_current`): a body that
no longer holds an address may not speak for it. Nothing inside a body changes
attribution. At delivery every body line is quoted with `| `, so text in a
body can never pose as the header around it.

### Where it goes

- The recipient Position must exist in Central. An unknown ref or handle is
  refused and nothing is recorded; the refusal names `aikit gateway who --json`.

Each Workcell's Actuation keeps its own occupancy ledger, so "who occupies
this Position" has one answer per Workcell. The gateway keeps no copy of any
of them. It routes in this order:

1. **This Workcell's ledger first** (`actuation occupancy read`).
   - A current tenure on this Workcell: `pending`, delivered at that
     occupant's next turn.
   - A current tenure that this ledger places on another Workcell: relayed
     to that Workcell's declared gateway (below). No endpoint declared:
     refused before recording, naming the exact `remote add` command.
2. **No current tenure here: ask every declared remote.** Each remote gateway
   answers `occupancy-read` (over its authenticated WebSocket, or its Unix
   socket) from its own Workcell's Actuation, at the moment of asking, with
   its `gateway_ref` and `workcell_ref`. Remotes are asked in parallel, each
   bounded to 2 seconds.
   - Exactly one reports a current tenure: `pending`, relayed there. The
     Communique records the route it took in `routing`: the remote's
     `workcell_ref`, `gateway_ref`, the `generation_ref` it reported, and a
     plain-words `basis`.
   - More than one reports a current tenure: refused before recording
     (`gateway.occupancy_ambiguous`), naming each Workcell and generation.
     The gateway does not choose between two occupants of one address.
   - None does, or none could be asked: `held`, with a three-part notice
     naming which Workcells answered vacant and which could not be asked
     (unreachable, refused, or their Actuation unavailable).
3. **Actuation here could not answer**: `pending` here, not relayed, with a
   three-part notice.

A relay whose remote is down leaves the Communique recorded here and queued;
the sender is not blocked. The relay pass (`aikit gateway forward`, and every
gateway service tick) re-resolves every undelivered Communique the same way,
asking each declared remote for its whole listing (`occupancy-list`) once per
pass. So a `held` Communique reaches a recipient who occupies later on another
machine, a `pending` one follows an occupant whose address moved to another
Workcell, and one queued for a remote that was down is retried. What a
predecessor already received is never sent again. Once relayed, delivery is
recorded by the receiving gateway, the sender's copy shows `forward.state:
forwarded`, and the sender's attribution travels unchanged.

This home's Workcell is `AIKIT_WORKCELL_REF` when set, otherwise the Workcell
`central.world.here` declares current. The gateway service answers peers'
occupancy questions with the same Workcell identity.

`who` overlays the remotes too. A Position vacant in this ledger but occupied
on a reachable remote shows that occupancy, with `occupancy.workcell_ref` and
`occupancy.observed_via: "gateway:<gateway_ref>"`. Rows read from this ledger
carry `observed_via: "local"`. Two Workcells claiming one Position show
`state: unavailable` with both `claims` and an absence. `data.remotes` lists
every declared remote as `{workcell_ref, gateway_ref, status: reachable |
unreachable, detail}`.

### Delivery at the turn boundary

On `UserPromptSubmit` the hook dispatcher verifies the body's occupancy, reads
the Communiques waiting for its Position and adds them to the turn's context.
They are marked delivered to that exact generation only after the harness
document carrying them has been written. A `--json` inspection of the hook, a
denied prompt, or a harness without a context channel delivers nothing and
marks nothing. A superseded body gets none; they wait for the current
occupant. When occupants change, whatever the predecessor never received goes
to the successor, and what the predecessor did receive stays recorded under
the predecessor's generation.

### Delegation

`delegate` is the only way a Communique becomes obligation-bearing. It asks
Factory to assign custody of the named work to the Communique's recipient
Position (`factory development custody assign … --origin-communique <ref>`).
When Factory answers with a custody ref, the Communique is recorded as
`escalated` with that ref. If Factory refuses, the Communique is left exactly
as it was.

### Where it is stored

Communiques are part of the gateway's semantic snapshot (`communiques` in
`state/gateway.json`), beside connectors, bindings and stream journals. A
conversation is read from that journal. When no service is running, a contact
verb runs the same kernel command directly against the state file. The state
file is guarded by an advisory lock: a running service holds it for as long as
it runs, and an offline writer holds it for a single command. The two can
never interleave, and a sender is never blocked because the gateway service
is stopped.

### Serving the gateway for other Workcells

`aikit gateway serve` with no flags serves this home's Unix socket
(`$AIKIT_HOME/state/gateway.sock`). Another Workcell reaches the gateway over
its authenticated WebSocket carrier:

```sh
aikit gateway serve --ws 100.92.62.101:7800 --ws-token-location file:/home/frank/.aikit/gateway.token --unix
```

- `--ws-token-location` names where the bearer token lives, either an
  owner-only `file:` path or a keychain/pass/op/varlock ref. The token is read
  once, at start. A file that its group or anyone else can read is refused
  before any carrier is bound. `--ws-token TOKEN` and `AIKIT_GATEWAY_TOKEN`
  still work. A location cannot be given together with a raw token.
- `--unix` with no path serves this home's socket beside the WebSocket, so
  local `send`, `inbox`, turn-boundary delivery and the relay pass keep
  reaching the running service. `--ws` alone serves the network carrier only.
  In that case local contact verbs cannot reach the service, and the service
  says so on stderr.

`aikit gateway install-service` keeps that posture alive. It always serves the
Unix socket, and it also serves the WebSocket when you pass `--ws` and
`--ws-token-location`. The Workcell and gateway identity are set in the
service's environment:

```sh
aikit gateway install-service --ws 100.92.62.101:7800 \
  --ws-token-location file:/home/frank/.aikit/gateway.token \
  --workcell-ref workcell:omarchy --gateway-ref agency-gateway/omarchy
```

| platform | service definition | started with |
|---|---|---|
| macOS | `~/Library/LaunchAgents/ai.aikit.gateway.plist` (KeepAlive) | `launchctl bootstrap gui/<uid>` |
| Linux | `~/.config/systemd/user/aikit-gateway.service` (`Restart=always`, `WantedBy=default.target`) | `systemctl --user daemon-reload` then `enable --now` |

The service definition carries the token's location, never the token itself.
It also carries the owner relation the dispatcher and the relay pass need:
`HOME`, `AIKIT_HOME`, the Central root, the resolved `ctrl`, `factory` and
`actuation` executables, and a `PATH` of only their directories plus the
system paths. It carries no credentials. A socket left by a gateway that has
exited is cleared under the gateway state lock. A gateway that is still
answering is refused.
Install refuses a WebSocket without a token location, and a token file that
is not owner-only, before it writes anything. If the service manager refuses
the start, the definition is removed again. `aikit gateway uninstall-service`
is the exact inverse. On Linux the user manager runs while you are logged in.
To keep the gateway up without a login, the machine owner enables lingering
(`loginctl enable-linger`).

### Two Workcells, both directions

These steps link the Mac (`workcell:mac`, tailnet 100.109.102.82) with Omarchy
(`workcell:omarchy`, tailnet host `frank`, 100.92.62.101). Each machine
serves its own gateway and declares the other one.

1. **A token per gateway.** Each gateway checks the token presented to it, so
   each machine keeps its own and gives a copy to the other:

   ```sh
   # on each machine
   umask 077 && openssl rand -hex 32 > ~/.aikit/gateway.token
   ```

   Copy the Mac's token to Omarchy as `~/.aikit/mac-gateway.token`, and
   Omarchy's to the Mac as `~/.aikit/omarchy-gateway.token`. Use `chmod 600`
   on both copies.

2. **Serve.** On the Mac:

   ```sh
   aikit gateway install-service --ws 100.109.102.82:7800 \
     --ws-token-location file:$HOME/.aikit/gateway.token \
     --workcell-ref workcell:mac --gateway-ref agency-gateway/mac
   ```

   On Omarchy:

   ```sh
   aikit gateway install-service --ws 100.92.62.101:7800 \
     --ws-token-location file:$HOME/.aikit/gateway.token \
     --workcell-ref workcell:omarchy --gateway-ref agency-gateway/omarchy
   ```

3. **Declare each other.** On the Mac:

   ```sh
   aikit gateway remote add --workcell workcell:omarchy --ws 100.92.62.101:7800 \
     --token-location file:$HOME/.aikit/omarchy-gateway.token
   ```

   On Omarchy:

   ```sh
   aikit gateway remote add --workcell workcell:mac --ws 100.109.102.82:7800 \
     --token-location file:$HOME/.aikit/mac-gateway.token
   ```

4. **Check.** `aikit gateway who --json` on either machine lists the other
   under `data.remotes` with `status: reachable`. A Position occupied on the
   other machine shows `observed_via: "gateway:agency-gateway/<other>"`.

Shell commands run outside the service, such as `aikit gateway send` and the
hook dispatcher, need the same identity. Set `AIKIT_WORKCELL_REF` in the
shell profile on each machine, unless `central.world.here` already names the
current Workcell there.

## The environmental DAY Routine

The Method `skill/aikit/central-day-rollover` declares a **native body**
(`[metadata.native-method]` in its manifest): exactly which Central Actions it
calls, and which one needs `CENTRAL_NATIVE_TOKEN`. When a Routine runs it, the
dispatcher selects the native runner from the Method itself. There is no flag,
and the runner never falls back to a model. The runner:

1. reads `central.time.policy`,
2. calls `central.day.ensure` against that exact policy revision. The token
   comes from the Routine's bound location and goes only into this one child
   process's environment,
3. reads `central.world` for the Projects that carry a NOW field,
4. calls `projectcentral.now.rollover {project, day: D-1, next_day: D}` for
   each of those Projects, where D is the civil date Central opened.

It completes no task, recognises no Return, closes no clearing, and never
calls the human-only `central.day.lifecycle`. The Routine's authority can
grant only the Actions the Method declares (`routine.action_not_in_method`).
An older `aikit` resolves the same capsule with the generic capability-run
Action, so it refuses the Routine outright; it cannot hand it to a model.

```sh
aikit routine credential routine/central-day-rollover --env CENTRAL_NATIVE_TOKEN --location file:/ABS/PATH
aikit routine show routine/central-day-rollover     # method_body: native:central-day-rollover, credential_bindings
aikit gateway tick --json                          # dispatched[].method_body, outcome.detail.receipt
```

Credential bindings hold a location only (`file:` owner-only, or a
keychain/pass/op/varlock ref), in `state/routine-credentials.json`. Each run
writes `aikit.native-routine-run/v1` to `state/routine-native-runs/<hash>.json`:
every Action called, with its input and outcome, the Day opened, each Project's
close, and the list of things the run did not do.

For Central to accept an unattended ensure, two things must hold: the live
civil-time policy must say `automatic_day_rollover: true`, and
`Control/user/native-action-authority.json` must grant a non-human principal
`central.day.ensure`. Both are owner settings, and Central checks them.

## Proof

- `crates/aikit-adapters/src/gateway_communique.rs`: journal laws (atomic
  acknowledgement, replay versus rewrite, relay, escalation, restore).
- `crates/aikit-cli/tests/gateway_contact.rs`: the real binary and gateway.
  Covers same-Workcell send, inbox and ack; turn-boundary delivery; vacant →
  held → next claim; occupant replacement; unknown and forged attribution;
  delegation into custody; the population reading; a missing owner. Across
  AIKit homes that each keep their own occupancy ledger (the fixture's
  `FIXTURE_OCCUPANCY`) and their own gateway: a Position occupied only on B is
  reached from A through B's occupancy answer, and B's reply reaches A the same
  way; B down leaves the Communique held at A until a relay pass finds B's
  occupant; an occupant whose address moves from A to B receives what it never
  had and nothing twice; two Workcells claiming one Position are refused as
  ambiguous; `who` shows occupancy observed through a remote gateway; a tenure
  A's own ledger places on B is relayed, retried after B was down, or refused
  when B is undeclared.
- `crates/aikit-adapters/src/gateway_service.rs`: the occupancy query is
  answered by the service's owner hook on every ask (nothing cached), writes
  no gateway state, and the kernel alone refuses it.
- `crates/aikit-cli/tests/routine_native_day.rs`: a disposable Central root with
  the real `ctrl`. An occurrence falls due, one `gateway tick` runs, the Day
  opens and the NOW fields roll, using native Actions only. With Central #217
  the Workcell root and the active child carry, and the quiescent child is
  released but retained. The run also refuses when no credential is bound.
