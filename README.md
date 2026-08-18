# AIKit

**The operative composition and disclosure layer for heterogeneous agentic worlds.**

AIKit exists to make the technological world around an actor **available here and now without requiring that world to be rewritten into one agent runtime**.

A person may already have models, CLI agents, skills, tools, Actions, source systems, project conventions, tmux or cmux sessions, IDEs, remote execution, existing configuration and several different harnesses. An artificial actor needs a usable horizon over that world: what exists, what is relevant here, what is permitted, what body or session it is operating through, which knowledge can be asked for, which capabilities can be invoked, and which Surfaces make those relations encounterable.

AIKit is the layer that resolves and discloses that operative horizon.

It is therefore not a bag of agent features. Models, skills, capabilities, tools, sources, sessions, runtime bodies and Surfaces matter because **their relations can become one explainable situated environment for an actor while retaining their native identities and owners**.

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

## Existing tools remain authoritative

Discovery is not adoption.

AIKit can inspect existing skill roots and supported lockfiles without rewriting them or claiming ownership. When adoption or external mutation is requested, AIKit uses explicit, reviewable Procedures with enough information to undo what it changed.

This is a non-displacement principle as much as a safety feature. Existing agentic arrangements are part of the user's real world. AIKit should make them intelligible and composable before asking them to become something else.

## Relation to the wider {O:I} field

**O:I** is the whole field of technological agency. AIKit is the operative composition/disclosure centre within that field; it does not become the owner of every resource it can index.

**Central** supplies durable human-authored ground. AIKit can resolve and disclose permitted Central material without turning its observations or learned state into authored Control.

**Actuation** owns Agent/Agency constitution, determination, authority, delegation, federation and Return. AIKit resolves the body and operative horizon through which a locus acts; `HarnessComposition` is not `AgenticComposition`.

**Software Factory** gives operative capability a developmental reason: Project intent, Runs, evidence, candidates and Recognition. Factory can ask AIKit what is available for a developmental act without making AIKit the owner of the Run.

**Workcell** materialises the actual processes, services, storage, network bindings and execution worlds beneath provider-neutral demand. AIKit resolves semantic availability; Workcell makes material requirements real.

**Quaternal Logic** can provide optional QL/MEF readings and navigation faculties. Base AIKit remains correct without a QL provider; derived formal readings must retain their provenance rather than replacing provider-owned meaning.

## Current implementation

> **Status:** production-oriented alpha.

Current `main` implements and exercises a Rust control plane including deterministic context-scoped capability resolution, persistence, a unified TUI, reversible Procedures, tmux integration, cmux 0.63 integration, portable sessions and foreign skills discovery.

Current implemented areas include:

| Area | Current behaviour |
|---|---|
| Unified TUI | Fast palette and hierarchical tree in one resident process; keyboard and mouse paths share the staged graph. |
| tmux | Managed popup, live binding verification, collision refusal, idempotent install and exact undo. |
| cmux 0.63 | Native Command Palette entry, lossless configuration merge, durable session ownership, live handle rebinding and ownership-bounded teardown. |
| Sessions | Portable TOML topology, idempotent `up`, physically read-only `diff`, additive or exact reconciliation. |
| Skills | Read-only discovery across agent roots plus supported lock provenance and hash verification. |
| Clients | Context projections for Claude and Codex, including explicit shared-tree degradation and opt-in task isolation. |
| Safety | Trust gates, secret quarantine, immutable generations, compare-and-swap apply and reversible external mutations. |

The broader **AIKit V2** programme is an active design and implementation migration. It extends the same product toward a wider typed Resource field, ContextSources and Knowledge Navigation, Project-world disclosure, composable runtime bodies, persistent multi-Surface agency, Explain/History and learned familiarity. Open V2 PRs are current development state, not evidence that every target capability has landed on `main`.

That distinction is deliberate: current code tells us what is real now; the V2 corpus tells us what the product is being developed toward.

## Install

Requirements:

- Rust 1.88 or newer;
- tmux and/or cmux only for those integrations;
- Node.js only where supported skill-lock verification requires it.

```sh
git clone https://github.com/EpiLogos/ai-kit.git
cd ai-kit
cargo install --locked --path crates/aikit-cli
```

AIKit is daemonless in the current implementation. Every command can start on a fresh machine.

A small first inspection is:

```sh
aikit init --json
aikit mux detect --json
aikit tree --all
aikit doctor --json
```

Open the current interface with:

```sh
aikit ui
```

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

## Development

```sh
cargo test --locked --workspace --all-targets --no-fail-fast
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --workspace --release
git diff --check
```

## License

Licensed at your option under either the Apache License, Version 2.0, or the MIT License.
