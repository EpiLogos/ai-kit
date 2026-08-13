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
- TUI event/reducer architecture.

These mechanisms should be retained where they continue to serve the full product.

The following are explicitly open to substantial V2 change:

- TUI information architecture and visual design;
- internal resource schemas;
- store/index layout;
- command taxonomy;
- Profile schema;
- projection strategy;
- project binding model;
- source-provider architecture;
- naming carried over from narrower V1 assumptions.

The design target governs over accidental implementation shape.

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
memory/familiarity
QL/MEF interop
```

---
