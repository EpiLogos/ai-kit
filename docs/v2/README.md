# AIKit V2 — Vision and Design Specification

**Status:** AUTHORITATIVE TARGET DESIGN — proposed for V2 programme  
**Date:** 2026-08-19  
**Repository:** `EpiLogos/ai-kit`  
**Scope:** product vision, ownership, context cognition, resource resolution, source/retrieval model, Knowledge Navigation, actor environment, composable runtime environments, projection, memory, human TUI/CLI, runtime integration, QL/MEF interoperability, migration, native praxis and Project reflection  
**Implementation posture:** the current AIKit is a valuable production-oriented alpha and a source of proven mechanisms. This specification defines the correct full product, while later numbered implementation/evidence contracts record where returned code and conformance have made parts of that target concrete or revised its earlier abstractions.

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
Methods / UsageOverlays / SkillSets
Actions / Action Sets
Context Sources
SemanticWiki / SourcePool / ProjectMap knowledge horizons
Project meaning ↔ local description ↔ exact code reflection
Models
Harnesses
Components / Contracts / runtime composition
Surfaces and target-native contributions
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

Knowledge Navigation preserves distinct provider authorities: SemanticWiki is maintained semantic/meaning structure, SourcePool is an evidential retrieval horizon, code-index providers own derived structural code intelligence, ProjectMap federates those lenses through stable refs, and KnowledgeRoute records actual traversal/familiarity without manufacturing semantic truth. The same addressable field may be rendered as list, tree or local graph without collapsing relation ownership.

AIKit does not become the owner of the meanings it carries. Project meaning remains Project-owned. Agent identity remains independent of AIKit. Application Actions remain application-owned. Human-authored material remains human-owned. Workcell remains the materialisation layer. QL/MEF remains an independent formal/semantic module. Harnesses remain the embodied agent-loop technologies.

Some Harnesses expose a richly composable body rather than a fixed runtime shell. V2 therefore also resolves target-native Components, requirements/contracts, providers, activation scopes, contributions and Surfaces into an inspectable `HarnessComposition` while preserving the same canonical Agent, Action, Capability, Project and Context identities. DeepSeek Harness/Cordis is the first rich reference target for this contract, not a required dependency or a replacement ontology.

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
 target + runtime-body projection
                ↓
       Agent / Agency / human
                ↓
            execution / returned evidence
```

---

# Specification package

This directory is one AIKit V2 specification and evidence package split into sectional files for navigation and review. The early files establish target design; later files record migration, implementation, evidence and returned refinements. Read the files relevant to the question, preserving that distinction rather than treating every document as the same kind of authority.

Current numbered route:

1. `01-PRODUCT-AND-OWNERSHIP.md`
2. `02-RESOLUTION-AND-CONTEXT-COGNITION.md`
3. `03-ACTOR-RUNTIME-AND-PROJECTION.md`
4. `04-INTERFACES-TUI-AND-SOFTWARE-DESIGN.md`
5. `05-QL-INTEGRATION-AND-RUNTIME.md`
6. `06-NEIGHBOURS-AND-MIGRATION.md`
7. `07-DEVELOPMENT-AND-ACCEPTANCE.md`
8. `08-VERIFICATION-RUNS-AND-CLOSURE.md`
9. `09-COMPOSABLE-RUNTIME-ENVIRONMENTS.md`
10. `10-PERSISTENT-AGENCY-AND-MATERIAL-HOSTING.md`
11. `11-MIGRATION-CLOSURE-LEDGER.md`
12. `12-PRELOCAL-ACCEPTANCE-EVIDENCE.md`
13. `13-COMPOSITION-CONNECTION-TUI-CONVERGENCE-EVIDENCE.md`
14. `14-SESSIONSPACE-FIRST-PARTY-CIRCUIT-EVIDENCE.md`
15. `15-MODEL-ROSTER-CAPABILITY-FIT.md`
16. `16-provider-connection-conformance.md`
17. `18-PROFILE-COMPOSITION-APPLICATION-PARITY.md`
18. `19-COMPOSITION-CONVERGENCE-REPAIR.md`
19. `20-PRAXIS-METHODS-AND-SKILL-COMPOSITION.md`
20. `21-PROJECT-REFLECTION-AND-LOCAL-ARTICULATION.md`

The numeric filename prefixes are historical programme sequence identifiers rather than a promise that every integer is occupied; do not renumber existing accepted files merely to remove a gap.

`../ARCHITECTURE.md` remains evidence for the current implementation and migration baseline rather than a constraint on the V2 target. For current implementation claims, inspect live code/tests and the later evidence contracts; for product meaning, follow the authored product ground and its provenance rather than inferring purpose backwards from code.
