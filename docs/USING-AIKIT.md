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

## Start and observe Factory work

Start a developmental difference with Factory's exact versioned Commission
request. AIKit forwards the file to the native owner operation, returns the
exact Factory receipt, and immediately reads the resulting owner state back:

```sh
aikit --json factory start-work \
  --state /path/to/developmental-state.json \
  --request-file /path/to/factory-commission-request.json \
  --factory-bin /path/to/factory
```

Factory—not AIKit—mints the Project, Journey and initial Run identities and
persists them atomically. The Central composition remains explicitly
`membership-non-authoritative`, and the bounded root act remains
`commissioned-not-executed`; starting Factory work does not claim that an Agent
or execution has run.

AIKit can add a Factory-owned developmental field to Search, the Navigator and
the Work view without creating an AIKit copy of Factory state. Bind both the
native Factory provider state and its exact canonical Project ref:

```sh
AIKIT_FACTORY_STATE=/path/to/developmental-state.json \
AIKIT_FACTORY_PROJECT_REF=project:01ARZ3NDEKTSV4RRFFQ69G5FAE \
AIKIT_FACTORY_BIN=/path/to/factory \
aikit ui
```

`AIKIT_FACTORY_BIN` defaults to `factory` on `PATH`; the state and Project ref
never default. AIKit asks that executable for its public versioned Project,
Journey, Run, Routine-continuation, WorkflowUnit and telemetry readings. The
start-work operation additionally preserves the exact Commission reading on its
related Project, Journey and Run. AIKit indexes only stable owner refs and
existing Factory Actions. The resource annotation records
the exact accepted Factory contract revision and schema digest AIKit consumes;
it does not claim to identify an arbitrary configured executable. The mandatory
cross-product conformance lane separately checks that accepted revision with its
real binary and owner-generated state. A missing half-binding is an error. With
no binding, direct Sessions and externally discovered Harnesses remain
zero-Factory.

The Work destination presents these same application readings under visibly
separate `DIRECT`, `FACTORY`, and `ATTENTION` headings. Direct/current-context
Sessions never gain Factory ancestry merely because Factory readings are also
visible. Factory rows retain the exact owner refs and revisions; HumanRequest
and Recognition rows appear only when the owner readings supply them.

To expose the native **Start Factory Work** contextual Action in the Work
destination, provide a reviewed Commission request as well as its target state:

```sh
AIKIT_FACTORY_STATE=/path/to/developmental-state.json \
AIKIT_FACTORY_REQUEST_FILE=/path/to/factory-commission-request.json \
AIKIT_FACTORY_BIN=/path/to/factory \
aikit ui
```

Select `Work` in the Navigator, press `:`, then choose `Start Factory Work`.
The application invokes the same native Factory boundary as
`aikit factory start-work`, re-reads the owner state, and displays the exact
receipt. A new state path is a truthful zero-Factory view until that Action is
accepted. Its root Act remains `commissioned-not-executed`.

## Agency Gateway

The Agency Gateway is the persistent contact plane through which the same
Agency and attributable ActuationStream stay continuable across Surfaces —
harness UI, terminal, and connector platforms such as Telegram. It runs as an
ordinary service and speaks one request/response envelope over two carriers:
an owner-only Unix-domain socket for same-host queries, and an authenticated
WebSocket carrier for network control and events.

The gateway has one well-known endpoint, so it needs no flags in the ordinary
posture: `~/.aikit/state/gateway.sock`, with semantic state persisted to
`~/.aikit/state/gateway.json`. Run it, then query it:

```sh
aikit gateway serve
aikit gateway protocol
aikit gateway discover
aikit gateway status
aikit gateway ecology
aikit gateway snapshot
```

`serve` with no flags binds the default socket and restores state from the
default file; a `shutdown` command stops it cleanly and persists state.
Naming `--ws HOST:PORT` (with `--ws-token` or `AIKIT_GATEWAY_TOKEN`) is the
network posture; `--ws` alone is deliberately network-only and binds no local
socket. Explicit `--unix PATH` / `--state-file PATH` override the defaults for
remote-Workcell placements. A query with no running gateway fails with
`cli.gateway_unreachable` and says how to start one — it never pretends an
absent gateway answered.

`aikit doctor` accounts for the default endpoint in every state: answering
(a note with the negotiated version), present-but-degraded (a warning naming
the restart), or not running (a note naming the start command). Bootstrap and
the O:I desktop can read exactly this probe as their gateway presence check.

`ecology` answers the gateway ecology read model: which Agencies, Agent
sessions, Streams and Surfaces this gateway currently constitutes, with fork
lineage and context revisions, and the invocation vocabulary
(`communique`, `session-contribution`, `delegation`, `session-fork`,
`co-actuation`). Presence never implies authority — invocation is a separate
AIKit capability grant. The `aikit-gateway` binary remains the minimal
stdio/serve body for Workcell materialisation; the CLI group is the same
protocol with the product's JSON envelope.

The gateway is also the contact plane between World Positions: `aikit gateway
who | send | inbox | conversation | delegate | forward | remote` read the
population, send Communiques that are delivered at the recipient occupant's
next turn, and relay them to other Workcells' gateways. The service's tick also
fires native-body Routines such as the environmental DAY rollover. See
[GATEWAY-CONTACT-AND-DAY.md](GATEWAY-CONTACT-AND-DAY.md).

## World inhabitation: whoami, refocus, inhabit

O:I's World-inhabitation contract (`O-I/docs/contracts/WORLD-INHABITATION-V1.md`)
gives a body an address: a **Position** defined by Central, held by an
**occupant generation** that Actuation records, carrying work that Factory holds
in custody. AIKit joins those owners; it stores none of their facts.

```sh
aikit inhabit --position @aikit-guardian --reason "steward AIKit" -- claude
aikit whoami                  # compact block; --json compact; --full everything
aikit refocus                 # trace current work back to ProjectCentral ground
aikit inhabit --release --position @aikit-guardian
```

`aikit inhabit` resolves the Position (`central.position.read`, or an `@handle`
through `central.position.list`), defaults `--agent` to the Position's single
eligible Agent and `--agency` to the Agent's single admitted AIKit agency,
claims the tenure with `actuation occupancy claim` (`--expect-vacant` by
default; `--handover` expects the current generation; `--fresh` replaces it),
then execs the harness with `OI_POSITION_REF` and `OI_OCCUPANT_GENERATION`
added to its environment. Nothing is released when the harness exits; leaving
is `--release`, which ends the generation the body holds.

**An orchestrator comes with its team.** Suppose the Agent is the
`orchestrator_agent_ref` of a Central agent set (`central.agent-set.list`,
root register), as `agent/anima` orchestrates `anima` and `agent/aletheia`
orchestrates `aletheia`. Then every other member of that set becomes a Claude
Code subagent of the launched session:

- **Where each subagent comes from.** Each member's Central profile
  (`agent-profile.list`) names its expression file among its
  `governance_refs` (`Control/agents/expressions/<team>/members/<member>.md`).
  The file's frontmatter gives the subagent's `description`, `tools` and
  `skills`, and its body gives the instructions. Skills are named the way the
  Claude projection names them, by the id's last segment
  (`skill/ql/vak-evaluate` → `vak-evaluate`).
- **Where the files go.** They are written once per inhabitation, as a Claude
  Code plugin under `$AIKIT_HOME/state/inhabitations/<generation>/claude/<set>-team/`
  (`agents/<member>.md` and `.claude-plugin/plugin.json`), with a
  `receipt.json` that lists every file and its digest.
- **How Claude Code sees them.** The `claude` argv is launched with
  `--plugin-dir <that directory>`, which Claude Code loads for that session
  only. Nothing is written into the repository or `~/.claude`. `--attach`
  hands the same directory to the continued session, and `--release` removes
  it with the tenure.
- **All or nothing.** A member with no profile, no expression, or an
  unreadable expression refuses the whole launch before anything is claimed.
  `--no-team` launches the orchestrator alone.
- **Other harnesses.** Any harness other than Claude Code is launched without
  the team, and `aikit inhabit` says so on stderr.
- **No argv.** Without a harness argv, the JSON reply carries
  `team_projection` with the plugin directory to pass.

`aikit whoami` (`aikit.inhabitation-reading/v1`) resolves the Position from
`--position`, then the stamped `OI_POSITION_REF` (verified with
`actuation occupancy verify`), then the one open tenure naming the current
AgentSession (`--agent-session` / `AIKIT_SESSION_ID`), else reports it absent.
Every facet — World, Project World, Position, occupancy, Agent, Agency,
AgentSession, SessionSpace, body, Workcell, root and child NOW, current work,
peers, prepared context, authority, working Surface, Return — is
`present | absent | ambiguous | unavailable | not-attempted` with the source
that answered. A missing or failing owner verb is `unavailable` with the exact
command and its error; each owner call is bounded (the shared probe budget).
`--publish` / `--rebuild` write the Redis World projection and `--hot` reads it
first (see `docs/JEV-REDIS-NOW.md`).

Hooks use the same reading. At SessionStart, a body whose occupancy resolves
gets a lean inhabitation block (under 2,000 characters) **instead of** the
historical Central NOW/Flow dump, with pointers to read it on demand; a body
with no resolvable occupancy gets exactly the previous floor. Refocus is
delivered at fresh occupancy, after compaction (SessionStart `compact`
directly; PreCompact/PostCompact/Stop only mark it pending), when the current
work changes, and after `AIKIT_REFOCUS_PROMPTS` prompts (default 40) — never
every turn. It is recorded as delivered only after the hook output carrying it
was written. `AIKIT_INHABITATION_HOOKS=off` disables both.

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

## Source verification and installed verification

AIKit distinguishes two verification paths because they answer different
questions.

**Source verification** checks the development checkout: the full test suite,
clippy, the release build, and `git diff --check`. The repository-owned entry
point is `bash scripts/verify`, which runs the same operation as CI. The
underlying command is `cargo test --workspace --all-targets --locked`.

**Installed verification** checks a built binary on a real or isolated home
directory. The command is `aikit doctor --json`. It runs the checks defined in
`crates/aikit-cli/src/doctor.rs` — registry load health, declared-but-unavailable
capabilities, unreviewed trust gates, home directory layout, credential provider
visibility, open bypass tokens — and returns the result in the stable CLI
envelope (§12 of `docs/ARCHITECTURE.md`). A successful run means process exit 0,
`schema: 1`, `ok: true`, and an object-valued `data` carrying `findings`,
`count`, and `fixable`. Doctor returns a successful observation envelope even
when individual checks report findings; success means the diagnostic itself
completed, not that every check found nothing.

AIKit publishes the installed verification command in `.oi/product.json` under
`verify.installed_command`, so an O:I-managed installation can verify the binary
it installed without maintaining a separate copy of the command vocabulary.
`verify.source_command` in the same descriptor carries the source verification
command. The acceptance test `doctor_json_is_the_installed_native_verification_path`
in `crates/aikit-cli/tests/cli_binary.rs` exercises the contract: it runs
`aikit doctor --json` against an isolated home and asserts process success,
`schema: 1`, `ok: true`, and an object `data`.
