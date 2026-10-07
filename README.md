# AIKit

AIKit is a Rust command-line tool and terminal application, `aikit`. For each project, agent and task, it works out which skills, tools, models and knowledge sources apply. It then makes exactly that set available to the agent harness you already use, writing into the harness's own settings where it has a declared, reversible way to do so. AIKit holds O:I's **capability** facet: the models, skills, tools and sources available to an agent here and now.

Three words carry the design:

- **Capability**: any power an agent could use, such as a Skill, a script, a hook, an MCP tool, a model or an Action.
- **Context**: the stack of scopes for one act: user, host, project, session, task and target client.
- **Projection**: the harness-specific form of the capabilities resolved for a context, for example a skills directory for Claude Code or hook entries for Codex. Projections live in immutable, content-addressed generations, so `aikit rollback` can return to the previous one.

## What it does today

AIKit 0.1.0 is a pre-release ("production-oriented alpha").

**Resolve.** `aikit world status` (also `aikit status`) shows the effective view for the current directory: which capabilities are catalogued, which are active, and which are present but held back, for example because a skill revision has not been reviewed or its authored standing is retired. `aikit explain <ref>` says why something is in its current state. The scope order runs user, then host, then project (`.aikit/profile.toml`), then session, then task, then one-shot override, and resolution is deterministic for a given context.

**Compose and apply.** `aikit compose plan | profile | enable | disable | use | apply | rollback` declares what a scope should carry and materialises it as a new generation. `aikit compose --model … --realise` selects a model from the roster and asks Actuation to admit it.

**Project into harnesses.** What AIKit actually writes depends on the harness (`aikit client status` lists 25 rows):

| Harness | What AIKit does |
|---|---|
| Claude Code | Skills are served from AIKit's generation through `aikit client launch claude`. Hook-dispatcher entries are merged into `~/.claude/settings.json`, and enabled MCP tool capsules into `~/.claude.json`, keeping foreign records. Takes effect after a client restart. |
| Codex | Writes `.agents/skills` for isolated tasks. In a shared working tree it projects only project-stable skills, or brokers, and says which. |
| zcode | Hooks through its config seam; skills through the shared tree. |
| pi | `.pi/skills` and hooks; takes effect from the next session. |
| openclaw | MCP tools only. |
| others (Gemini CLI, Hermes, OpenCode, Cursor CLI, …) | Brokered: AIKit writes nothing into them. The agent reaches capabilities through `aikit capabilities list` / `read` and `aikit run`. |

Sources are not copied into prompts. They are disclosed and retrieved on demand.

**Adopt existing skills without taking them over.** `aikit init` and `aikit collate` show the skill roots already on the machine. `aikit adopt` moves one under AIKit's management through a reviewed, digest-bound **Procedure**, a recorded external change that `aikit procedure undo` reverses.

**Knowledge.** `aikit knowledge search | open | read | relations | graph | wiki` navigates project knowledge, Agent Wikis (`okf-wiki/v1`) and code (through GitNexus). `aikit wiki ingest` compiles a Markdown corpus into a wiki, as a dry run by default. `aikit search` finds capabilities, Skills and resources by name.

**Act.** `aikit act`, `aikit act describe <ref>` and `aikit act invoke <ref> --input @file` discover and invoke the Actions available to the current subject.

**Sessions and places.** `aikit session up | diff | reconcile | down` brings up a portable TOML session topology on tmux or cmux. `aikit mux install` integrates the multiplexer, and `aikit ui` opens the terminal application.

**Inhabit a position.** `aikit inhabit --position <ref>` claims a World Position through Actuation's occupancy ledger and launches a harness into it with `OI_POSITION_REF` and `OI_OCCUPANT_GENERATION` set. `aikit whoami` and `aikit refocus` read back where the current agent stands.

**Agency Gateway** (optional). `aikit gateway serve` is a persistent service, installable as a macOS LaunchAgent or a systemd user unit. It carries agent-to-agent and surface-to-agent messages. The core CLI needs no daemon.

**Worktree projection.** `aikit worktree project` reports each checkout's drift from `origin/main`. With `--apply`, it fast-forwards only clean checkouts that are behind.

**Limits, stated plainly:**

- `aikit harness-profile validate <FILE>` checks an externally authored harness profile and returns `admit` or `refuse`. There is no verb to register one; profiles are still compiled in.
- Gemini CLI, OpenCode, Hermes and openclaw skills do not have native projection yet. Gemini Antigravity, Hermes and Kimi have no Actuation capability descriptor.
- Much of the V2 design corpus (`docs/v2/`) is still marked target design. Its acceptance ledger records which parts are on `main`.
- `oi` reports that the suite's builds "have not passed physical acceptance".

## How it fits O:I

O:I gives each facet of an agent's world its own product. AIKit holds **capability**. It resolves what is available and relevant for an act. It does not own the identity of what it indexes. Every neighbour is reached by running that product's binary and reading versioned JSON; there are no crate dependencies.

| Neighbouring facet | Where they meet |
|---|---|
| Ground / Central | AIKit reads authored ground through `ctrl` Actions (`central.world.here`, `central.now.read`, `central.day.read`, `central.position.*`, `agent-profile.read`, …) and reads Central's skill collections as its personal skill source (`--control-ground`). Central owns the authored Agent profile, and AIKit never re-authors it. |
| Agency / Actuation | Actuation owns what each harness *is* (`actuation.harness-detection/v1`, `actuation.harness-capability/v1`); AIKit owns what it does about it. The Claude adapter refuses to install without Actuation's descriptor. `compose --realise` uses Actuation's agency admission, and `aikit inhabit` claims occupancy through Actuation. |
| Development / Software Factory | `aikit factory` and `aikit work factory` enter work through Factory's Commission boundary. Factory consumes AIKit's model-selection, task-dispatch, encounter-delivery and Routine evidence. Factory's skills are projected through AIKit. |
| Environment / Workcell | AIKit reads Workcell's harness-instance and run records. Workcell owns processes and endpoints, and AIKit owns the semantic Surface relation. `aikit worktree project` verdicts are carried by Workcell as correlated observations. |
| Reflection / QL | AIKit calls the installed `ql` for capability negotiation and validates QL-shaped wiki constellations against QL's shape contract. Base AIKit is correct without QL. |
| O:I | `oi aikit …` (alias `oi kit`) dispatches to `aikit`. `oi search`, `oi explain` and `oi ui` run through the installed AIKit, and `oi skills sync` hands the guardian SkillSet to AIKit. |

**In the Cradle.** The O:I desktop shows what powers, context, knowledge and body are available here and now, and the session space a body runs in. The Cradle's kernel reads AIKit's resolved context and encounter state as one of its owner readings.

## Install and quick start

Through O:I (ordinary route):

```sh
oi install ai-kit
aikit doctor
```

From source (developer route). This needs Rust 1.98, pinned in `rust-toolchain.toml`. tmux or cmux is needed only for those integrations.

```sh
git clone https://github.com/EpiLogos/ai-kit.git
cd ai-kit
cargo install --locked --path crates/aikit-cli
```

Release archives are published as pre-releases (`aikit-v0.1.0-prelocal.6`).

First commands:

```sh
aikit world status          # where am I: the effective context reading
aikit search <name>         # find a capability, Skill or resource by name
aikit explain <ref>         # why a capability has its current state
aikit client status         # what AIKit does for each harness on this machine
aikit praxis list           # the Skills / Methods this scope carries
aikit ui                    # the terminal application
```

`aikit help <command>` documents any one command. `aikit system commands --json` emits the complete generated command reference. Older root spellings such as `aikit status` and `aikit tree` remain exact aliases.

---

## Availability is not use

AIKit keeps several relations distinct because collapsing them produces misleading agency.

```text
exists
≠ eligible here
≠ available from a provider
≠ selected / preferred
≠ projected into this client
≠ loaded into context
≠ invoked
```

The same discipline applies to knowledge. A source can be known and askable without its payload entering standing context. Retrieval is preferable to indiscriminate injection because a broad information horizon is useful while a permanently bloated prompt is not.

The same discipline applies to runtime bodies. A Component can be known without being active; a Surface can be visible without conferring mutation authority; a generated configuration is not proof that a target process actually activated it.

These distinctions are what make the environment explainable rather than merely convenient.

## Why this exists

Without an operative composition layer, agentic environments tend to become accidental global state:

- skills are copied or symlinked by several installers;
- hooks exist on disk without a clear active relation;
- project-specific values leak into unrelated work;
- session and multiplexer integrations assume they own an environment;
- a capability is treated as available merely because some file exists;
- large knowledge stores are injected into prompts instead of remaining retrievable horizons;
- several clients each invent their own version of the same resource state.

The deeper problem is not filesystem tidiness. It is that an actor can no longer reliably answer **what world am I actually operating in, why is this resource available, what is absent, and what would change if I composed the environment differently?**

AIKit treats that as a context-resolution and disclosure problem.

## The product relation

AIKit separates a large available world from the smaller world that should become effective for a particular act.

```text
heterogeneous available world
    models · skills · Actions · sources · projects
    harnesses · components · sessions · hosts · Surfaces
                    │
                    │ resolve for this Project / actor / task / client
                    ▼
             operative horizon
                    │
        ┌───────────┼───────────┐
        ▼           ▼           ▼
   available     relevant     permitted
   powers        knowledge    operation
                    │
                    ▼
                disclosure
                    │
          human Surface / agent context
```

The point of resolution is not to flatten every resource into one format. A capability can remain supplied by an existing skill ecosystem. A ContextSource can remain owned by its provider. A model can remain local or remote. A rich harness can expose a composition of Components and Surfaces while a thin harness remains valid. AIKit gives those things common addressability and operative relations where a common relation is actually needed.

## Current implementation

> **Status:** production-oriented alpha.

Current `main` implements and exercises a Rust control plane including deterministic context-scoped capability resolution, persistence, the terminal application, reversible Procedures, tmux integration, cmux 0.63 integration, portable sessions and foreign skills discovery, together with Knowledge navigation, SessionSpace, Explain/History, the Agency Gateway and the inhabitation readings described above.

Current implemented areas include:

| Area | Current behaviour |
|---|---|
| Terminal application | `aikit ui`: the V2 application surface (Quick and Workspace modes; List, Tree and Graph presentations) over one staged graph. |
| tmux | Managed popup, live binding verification, collision refusal, idempotent install and exact undo. |
| cmux 0.63 | Native Command Palette entry, lossless configuration merge, durable session ownership, live handle rebinding and ownership-bounded teardown. |
| Sessions | Portable TOML topology, idempotent `up`, physically read-only `diff`, additive or exact reconciliation. |
| Skills | Read-only discovery across agent roots plus supported lock provenance and hash verification. |
| Clients | Client dispatch for Claude Code, Codex and zcode; managed skill and hook layers for pi; MCP tools for openclaw; a broker for every other detected harness. Codex keeps explicit shared-tree degradation and opt-in task isolation. |
| Safety | Trust gates, secret quarantine, immutable generations, compare-and-swap apply and reversible external mutations. |
| Harness profiles | Declarative `aikit.harness-profile/v1` documents (detection, projection and ownership posture per layer: skills, guidance, hooks, tools, models, sessions), a `tool-protocol` capsule kind for MCP servers as capability sources, and one layer merge engine behind parity-tested grammars. Profiles are currently embedded at build time. `aikit harness-profile validate <FILE>` checks an externally authored profile against the same schema and admission grammar (`admit` / `refuse`); there is no register verb yet, so an external author's route to shipping a profile is the adapter SDK contract and `skill/aikit/harness-adapter-authoring`. Design: `docs/plans/2026-09-16-harness-profile-design.md`. |

The broader **AIKit V2** programme is an active design and implementation migration. It extends the same product toward a wider typed Resource field, ContextSources and Knowledge Navigation, Project-world disclosure, composable runtime bodies, persistent multi-Surface agency, Explain/History and learned familiarity. Open V2 PRs are current development state, not evidence that every target capability has landed on `main`.

That distinction is deliberate: current code tells us what is real now; the V2 corpus tells us what the product is being developed toward.

## Existing tools remain authoritative

Discovery is not adoption.

AIKit can inspect existing skill roots and supported lockfiles without rewriting them or claiming ownership. When adoption or external mutation is requested, AIKit uses explicit, reviewable Procedures with enough information to undo what it changed.

For personal bootstrap, `aikit adopt ROOT --control-ground CENTRAL/Control/user/skills` previews adoption into the human-curated Central collection. Central also publishes `ProjectCentral/user/skills` for project skills and `Control/machines/ROLE/skills` for genuinely machine-specific skills. Confirm the reviewed digest after human acceptance. Adoption preserves originals until projection cutover, retains identical staged skills and their authored standing, and refuses conflicting payloads or external skill links. Register accepted personal ground with `aikit source add-directory personal-ground PATH --control-ground`, then sync, promote, select and apply through AIKit. Active standing permits selection; retired or unresolved standing withholds projection even when selected. Project selection uses existing ProjectCentral bindings and project enable scopes. Product and external skills retain their native registered sources.

After staging, source promotion and `apply`, finish the handoff with `aikit adopt ROOT --control-ground CENTRAL/Control/user/skills --projection CURRENT/projections/codex/.agents/skills`. Here `CURRENT` is the context's stable `current` generation pointer from the applied AIKit state. Preview and confirm the returned digest. AIKit verifies every original payload against Control and the generated tree, refuses unaccounted files or stale standing, moves the original tree into its Procedure undo archive, then publishes a directory link to the generation. Claude uses `CURRENT/projections/claude/.claude/skills`. Future `apply` calls update the projection; `procedure undo` restores the original tree. This cutover does not author Control or accept an unresolved external source.

For an existing mixed harness directory, use `aikit adopt ROOT --projection CURRENT/projections/codex/.agents/skills` without `--control-ground`. It reconciles generated skill entries, preserves unrelated harness files and unselected skills, checks existing payload bytes and modes, and retains replaced entries in the reversible Procedure archive. Broken links are replaced when the corresponding skill is generated from a registered source. Repeat reconciliation to expose newly selected skill names; existing links follow later generations automatically.

Registering and promoting a local directory accepts its local skill revisions without a separate `--trust` step. Git downloads retain per-revision trust choices. This does not change the active/retired standing authored in Control.


This is a non-displacement principle as much as a safety feature. Existing agentic arrangements are part of the user's real world. AIKit should make them intelligible and composable before asking them to become something else.

## Portable sessions

The current session contract remains provider-aware without making a multiplexer the semantic owner:

```toml
schema = 1
id = "dev"
name = "dev"
root = "."

[backend]
kind = "tmux"

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

`diff` remains inspection-only. Reconciliation is bounded by durable ownership markers so unowned panes and Surfaces are not silently treated as AIKit state.

## Worktree projection

A whole suite of repository checkouts — the worktrees a dev environment or a
machine holds — should be able to *project* the canonical branch, `origin/main`,
in one command, with drift surfaced and safely repaired. AIKit owns the
repository/worktree side of a project, so this is its command:

```sh
# Observe only (default): report each checkout's drift from origin/main.
aikit worktree project \
  --repo o-i=/path/O-I --repo central=/path/Central --repo ai-kit=/path/ai-kit

# Reconcile: fast-forward the clean, behind checkouts to the target.
aikit worktree project --apply --repo ql-mef=/path/Quaternal-Logic
```

The one law it never breaks: a projection **never discards uncommitted or
unmerged work** to force the target. Only a *clean* checkout that is strictly
behind the target (its HEAD an ancestor of `origin/main`) is fast-forwarded, and
only with `--apply`. A dirty, ahead, or diverged checkout is *surfaced* — named
as a delta a human must resolve — never reset, cleaned, or rebased. Detached
HEADs (the usual shape of a dev worktree) are fast-forwarded in place and stay
detached. `--no-fetch` compares against the last-fetched target; `--target`
overrides `origin/main`.

The checkout roots are supplied by the caller (explicit `--repo`, or a resolver
such as O-I's dev-world machine file); AIKit never guesses them. See
[Worktree projection](docs/WORKTREE-PROJECTION.md).

## Verification

```sh
aikit status --json
aikit doctor --json
aikit init --json
aikit mux detect --json
aikit tree --all --ascii
```

The repository verification suite exercises real tmux servers, subprocesses, SQLite state, PTY-driven TUI flows and fresh-machine binary acceptance where the environment permits it. Provider-specific limitations are represented as such rather than promoted into generic semantic claims.

## Documentation

- [Using and verifying AIKit](docs/USING-AIKIT.md)
- [Current architecture and state model](docs/ARCHITECTURE.md)
- [AIKit V2 product architecture](docs/v2/README.md)
- [Procedures, inbox, trust and reversal](docs/SPEC-II-PROCEDURES-AND-INBOX.md)
- [Skillsets, frecency and tree semantics](docs/SPEC-III-SKILLSETS-AND-FRECENCY.md)
- [Skills ecosystem compatibility](docs/SKILLS-ECOSYSTEM.md)
- [Engineering standards](STANDARDS.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Wiki operational projection](docs/WIKI-OPERATIONAL-PROJECTION.md) — the adaptive Wiki → operational projection → next-turn hook path. Stored feedback, harness output and observed model use remain separate receipts.
- [Harness-profile design](docs/plans/2026-09-16-harness-profile-design.md)

## Development

```sh
cargo test --locked --workspace --all-targets --no-fail-fast
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --workspace --release
git diff --check
```

The full CI gate is `scripts/verify`; `scripts/verify quick -p <pkg>` is the inner-loop variant.

## What changes for a human

A human can treat the agentic environment as something that can be searched, composed, inspected and understood instead of as a scattering of hidden tool directories and client-specific setup.

The intended human experience is one in which:

- Project and current focus remain legible;
- available powers and knowledge can be found quickly;
- changes can be staged and previewed before durable mutation;
- the reason a resource is active, absent or degraded can be explained;
- sessions and runtime Surfaces can be entered without confusing their provider-specific form with semantic identity;
- familiar destinations become easier to reach without learned frequency silently becoming trust or authored preference.

The TUI, CLI and future Surfaces are presentations of that underlying product relation. They are not separate semantic controllers.

## What changes for an agent

An agent can receive a small orientation into a much larger operative world.

Instead of serialising the whole environment into a standing system prompt, AIKit can disclose the Project, actor/session binding and compact horizons, then let the agent retrieve deeper state or source material when the act requires it.

This makes context cognition possible: the actor can distinguish what it presently knows from what it can ask, what it can do from what merely exists, and its enduring Agent/Agency identity from the replaceable model, harness, session or material body carrying the current act.

## Relation to the wider O:I field

**O:I** is the whole field of technological agency. AIKit is the operative composition/disclosure centre within that field; it does not become the owner of every resource it can index.

**Central** supplies durable human-authored ground. AIKit can resolve and disclose permitted Central material without turning its observations or learned state into authored Control.

**Actuation** owns Agent/Agency constitution, determination, authority, delegation, federation and Return. AIKit resolves the body and operative horizon through which a locus acts; `HarnessComposition` is not `AgenticComposition`.

**Software Factory** gives operative capability a developmental reason: Project intent, Runs, evidence, candidates and Recognition. Factory can ask AIKit what is available for a developmental act without making AIKit the owner of the Run.

**Workcell** materialises the actual processes, services, storage, network bindings and execution worlds beneath provider-neutral demand. AIKit resolves semantic availability; Workcell makes material requirements real.

**Quaternal Logic** can provide optional QL/MEF readings and navigation faculties. Base AIKit remains correct without a QL provider; derived formal readings must retain their provenance rather than replacing provider-owned meaning.

## License

Licensed at your option under either the Apache License, Version 2.0, or the MIT License.

---

## Background

O:I stands for Objective : Internality. It names the means through which a life knows and acts within a world: memory, language, tools, permissions and other people. Those means are internal because every act proceeds through them, and objective because each can be examined and changed. AIKit makes one of those means, what an agent can reach and use here and now, something that can be resolved, inspected and explained. The idea is developed in the essay [*Confronting the Limit: Determination, Subjectivity and Mind as Objective Internality*](https://oi.epi-logos.org/essay/).

**The operative composition and disclosure layer for heterogeneous agentic worlds.**

AIKit exists to make the technological world around an actor **available here and now without requiring that world to be rewritten into one agent runtime**.

A person may already have models, CLI agents, skills, tools, Actions, source systems, project conventions, tmux or cmux sessions, IDEs, remote execution, existing configuration and several different harnesses. An artificial actor needs a usable horizon over that world: what exists, what is relevant here, what is permitted, what body or session it is operating through, which knowledge can be asked for, which capabilities can be invoked, and which Surfaces make those relations encounterable.

AIKit is the layer that resolves and discloses that operative horizon.

It is therefore not a bag of agent features. Models, skills, capabilities, tools, sources, sessions, runtime bodies and Surfaces matter because **their relations can become one explainable situated environment for an actor while retaining their native identities and owners**.
