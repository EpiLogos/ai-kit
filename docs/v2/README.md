# AIKit V2 — Vision and Design Specification

**Status:** AUTHORITATIVE TARGET DESIGN — proposed for V2 programme  
**Date:** 2026-08-13  
**Repository:** `EpiLogos/ai-kit`  
**Scope:** product vision, ownership, context cognition, resource resolution, source/retrieval model, actor environment, projection, memory, TUI/CLI, runtime integration, QL/MEF interoperability, migration from the current implementation  
**Implementation posture:** the current AIKit is a valuable production-oriented alpha and a source of proven mechanisms. This specification defines the correct full product even where V2 requires substantial internal, TUI, schema, or UX rework.

---

## 0. Executive determination

AIKit is the **context-cognitive control plane for human and artificial actors**.

It indexes independently owned sources, powers, actor resources, and execution resources; deterministically resolves the operational world appropriate to a Project, Profile, Agency, scope, and present Focus; makes a broader information and capability horizon discoverable without indiscriminately loading it; remembers routes that become familiar through use; explains why resources are available, absent, selected, or withheld; and projects the resulting world into heterogeneous agent harnesses and human surfaces.

The existing product correctly discovered the nucleus:

```text
sources / registries
        ↓
indexed catalog
        ↓
scoped overlays
        ↓
deterministic resolution
        ↓
explainable effective state
        ↓
target projection
```

V2 generalises **what the resolver can resolve**.

The product moves from a context-scoped capability router for terminal work toward a context-cognitive operating substrate capable of resolving and disclosing:

```text
Project relation
Agent / Agency
Profiles and scopes
Capabilities
Skills
Actions / Action Sets
Context Sources
Models
Harnesses
Hosts
available execution worlds
trust / availability / policy
preference bindings
memory / familiarity / fitness observations
present Focus
retrieved versus latent information
target-specific projections
```

AIKit does not become the owner of the meanings it carries. Project meaning remains Project-owned. Agent identity remains independent of AIKit. Application Actions remain application-owned. Human-authored material remains human-owned. Workcell remains the materialisation layer. QL/MEF remains an independent formal/semantic module. Harnesses remain the embodied agent-loop implementations.

The concise dependency direction is:

```text
independently authored / observed world
                ↓
            AIKit index
                ↓
      deterministic resolution
                ↓
         context cognition
                ↓
     selective retrieval/disclosure
                ↓
        target projection
                ↓
       Agent / Agency / human
                ↓
            execution
```

---

# Specification package

This directory is one authoritative AIKit V2 target-design specification split into sectional files for navigation and review. Read in order:

1. `01-PRODUCT-AND-OWNERSHIP.md`
2. `02-RESOLUTION-AND-CONTEXT-COGNITION.md`
3. `03-ACTOR-RUNTIME-AND-PROJECTION.md`
4. `04-INTERFACES-TUI-AND-SOFTWARE-DESIGN.md`
5. `05-QL-INTEGRATION-MIGRATION-AND-ACCEPTANCE.md`

The package defines the target product. `../ARCHITECTURE.md` remains evidence for the current implementation and migration baseline rather than a constraint on the V2 target.
