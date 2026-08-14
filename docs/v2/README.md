# AIKit V2 — Vision and Design Specification

**Status:** AUTHORITATIVE TARGET DESIGN — proposed for V2 programme  
**Date:** 2026-08-14  
**Repository:** `EpiLogos/ai-kit`  
**Scope:** product vision, ownership, context cognition, resource resolution, source/retrieval model, Knowledge Navigation, actor environment, projection, memory, human TUI/CLI, runtime integration, QL/MEF interoperability, migration from the current implementation  
**Implementation posture:** the current AIKit is a valuable production-oriented alpha and a source of proven mechanisms. This specification defines the correct full product even where V2 requires substantial internal, TUI, schema, or UX rework.

---

## 0. Executive determination

AIKit is the **context-cognitive control plane for human and artificial actors**.

It indexes independently owned sources, powers, actor resources, execution resources and addressable project knowledge; deterministically resolves the operational world appropriate to a Project, Profile, Agency, scope, and present Focus; makes broader information and capability horizons discoverable without indiscriminately loading them; remembers destinations and routes that become familiar through use; explains why resources are available, absent, selected, related or withheld; and projects the resulting world into heterogeneous agent harnesses and human surfaces.

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

V2 generalises **what the resolver can resolve and what an actor can navigate**.

The product moves from a context-scoped capability router for terminal work toward a context-cognitive operating substrate capable of resolving and disclosing:

```text
Project relation
Agent / Agency
Profiles and scopes
Capabilities
Skills
Actions / Action Sets
Context Sources
SemanticWiki / SourcePool / ProjectMap knowledge horizons
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

For humans, the CLI is AIKit's operational language while the TUI becomes the human environment-composition and context-navigation instrument over that same application state. Search, Context, Compose, Explain, History and relation navigation are semantic capabilities, not separate semantic stores.

Knowledge Navigation preserves distinct provider authorities: SemanticWiki is authored semantic/meaning structure, SourcePool is an evidential retrieval horizon, code-index providers own structural code intelligence, ProjectMap federates those lenses through stable refs, and KnowledgeRoute records actual traversal/familiarity without manufacturing semantic truth. The same addressable field may be rendered as list, tree or local graph without collapsing relation ownership.

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
      navigation / composition
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
5. `05-QL-INTEGRATION-AND-RUNTIME.md`
6. `06-NEIGHBOURS-AND-MIGRATION.md`
7. `07-DEVELOPMENT-AND-ACCEPTANCE.md`
8. `08-VERIFICATION-RUNS-AND-CLOSURE.md`

The package defines the target product. `../ARCHITECTURE.md` remains evidence for the current implementation and migration baseline rather than a constraint on the V2 target.
