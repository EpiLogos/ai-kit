# Part XVII — Neighbouring modules

## 56. Human-owned source tooling

A separate human-owned filesystem/control protocol or small CLI may manage durable personal source material, Work roots, bootstrap mechanisms, or ordinary project navigation.

That tool remains useful without AIKit.

AIKit consumes it through source/project-discovery providers rather than becoming its synchroniser, package manager, or canonical author.

---

## 57. Factory Core

Factory Core owns developmental semantics such as Project, Run, Run Map, Decision, Candidate, Claim, Evidence, Assessment/Recognition, Recursion, and canonical domain relations.

AIKit indexes and resolves actor resources around those semantics.

---

## 58. Workcell

Workcell owns material execution planning and binding.

AIKit resolves semantic demand and available offers.

---

## 59. QL/MEF module

The standalone QL/MEF module owns executable formal/refraction semantics and their provenance.

AIKit is a client/provider host, not the QL semantic authority.

---

## 60. Meta-skills

Intelligent configuration and authoring activities should live as Skills rather than deterministic AIKit core logic where judgement is required.

Examples include:

- recovering and proposing durable human/agent orientation;
- composing situated Agency language;
- crafting/refining Skills and affordance descriptions;
- reviewing traces and proposing durable realignment.

AIKit supplies the data, scopes, resources, Procedures, and retrieval surface. The Skill performs the intelligent composition.

---

# Part XVIII — V2 migration posture

## 61. V2 is a product rework, not a compatibility costume

The current AIKit proves valuable mechanisms:

- Rust crate separation;
- deterministic scope resolution;
- trust;
- immutable Generations;
- reversible Procedures;
- mux integration;
- Claude/Codex projection;
- Skill discovery;
- frecency foundations;
- JSON CLI;
- TUI event/reducer architecture;
- terminal restoration, host handling and snapshot/performance testing.

These mechanisms should be retained where they continue to serve the full product.

The following are explicitly open to substantial V2 change:

- TUI information architecture and visual design;
- TUI controller/state topology;
- internal resource schemas;
- store/index layout;
- command taxonomy;
- Profile schema;
- projection strategy;
- project binding model;
- source-provider architecture;
- naming carried over from narrower V1 assumptions.

The design target governs over accidental implementation shape.

### 61.1 TUI migration specifically

The existing TUI's palette/tree separation is migration evidence, not a V2 invariant. V2 should preserve the useful pure-event/reducer discipline, host safety, terminal restoration, staging/confirmation practices, accessibility foundations, snapshots and performance discipline while replacing semantic duplication between surface controllers.

The migration target is one authoritative `TuiState` and `UiAction` reducer over resource-oriented application services/read models. Quick and Workspace presentations, and list/tree/graph relation projections, must not maintain separate copies of canonical selection, staging or resolution state.

The existing capsule-shaped backend may be adapted incrementally while V2 resource/application contracts land, but it must not become a compatibility wall that forces Profiles, ContextSources, Agent/Agency, Knowledge Navigation or projection back into Capsule/Capability-only shapes.

Migration should explicitly remove or replace interaction patterns where old semantics are unsafe or misleading, including:

- manual Palette ↔ Tree state synchronisation;
- mouse paths that bypass the semantic action reducer;
- context-dependent Esc behaviour that can discard staged work;
- TUI-local resolver/provider semantics;
- hidden CLI shell-out as an application-service substitute;
- transient row/index identity where stable ResourceRefs exist.

The terminal host model should remain honest: real popup primitives may be used where present; inline/fullscreen behaviour must remain explicit and recoverable; no host capability should be fabricated for visual symmetry.

---

## 62. Migration principle

V2 should preserve **semantic safety invariants**, not necessarily every internal representation.

Preserve:

```text
determinism
explainability
source ownership
trust boundaries
atomic Generation switching
reversible external mutation
honest target effects
machine-readable interfaces
session-local scoping
stable resource identity across presentation changes
```

Refactor freely where needed to achieve:

```text
wider resource indexing
context cognition
source/retrieval separation
Agent/Agency integration
Action indexing
project binding
higher-quality TUI
human environment composition
unified knowledge navigation
relation list/tree/graph projections
memory/familiarity
QL/MEF interop
```

---
