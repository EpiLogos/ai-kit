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
aikit gateway remote add --workcell workcell:omarchy --ws 192.168.1.20:7800 --token-location file:/Users/me/.aikit/omarchy-gateway.token
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
- Occupied: `pending`, delivered at that occupant's next turn.
- Vacant: `held`, with a three-part notice. It is delivered to whichever
  occupant claims the Position next.
- Occupied on another Workcell: relayed through that Workcell's gateway over
  the authenticated WebSocket carrier, using a declared endpoint (`aikit
  gateway remote add`, token by location only). No endpoint declared: refused
  before recording, naming the exact `remote add` command. Endpoint down: the
  Communique is recorded here, queued, and the sender is not blocked. The
  relay pass (`aikit gateway forward`, and every gateway service tick) sends it
  once the remote answers. Once relayed, delivery is recorded by the receiving
  gateway, and the sender's copy shows `forward.state: forwarded`.

This home's Workcell is `AIKIT_WORKCELL_REF` when set, otherwise the Workcell
`central.world.here` declares current.

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
  delegation into custody; the population reading; a missing owner. Across two
  AIKit homes and two gateways: relay, and a remote that is down and receives
  later.
- `crates/aikit-cli/tests/routine_native_day.rs`: a disposable Central root with
  the real `ctrl`. An occurrence falls due, one `gateway tick` runs, the Day
  opens and the NOW fields roll, using native Actions only. With Central #217
  the Workcell root and the active child carry, and the quiescent child is
  released but retained. The run also refuses when no credential is bound.
