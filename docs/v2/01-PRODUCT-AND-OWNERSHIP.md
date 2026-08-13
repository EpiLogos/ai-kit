# Part I — Product identity

## 1. Product thesis

AIKit exists to answer, for humans, agents, and software clients:

> **What world of powers and information is actually available to this actor here and now, how can it be discovered, and why is it this world rather than another?**

For a human, AIKit should feel like the place where an agentic computing environment becomes locally intelligible and composable.

For an agent, AIKit should function as a faculty of **context cognition**: a way to discover where it is, what it can do, what it can ask about, what it already knows, what lies latent beyond the present Focus, and what has become familiar through prior operation.

For software clients, AIKit should expose a deterministic machine-readable resolution service over typed resources and sources.

These are not three products. They are three surfaces over one semantic service.

---

## 2. Context cognition

AIKit does not equate context with prompt contents.

The wider Factory architecture defines canonical Context as:

```text
Operative World
+
Information Horizon
+
Focus
```

AIKit's responsibility is the operational resolution beneath and around that Context.

Context cognition means an actor can maintain distinct relations to its world:

```text
what exists
what I know exists
what I know I can ask about
what I have actually retrieved / presently know
what I have chosen to attend to
```

An actor therefore does not need the whole information horizon, Skill registry, Action catalog, personal source field, Project map, or runtime inventory inside every model context.

The product should make the wider world **addressable before it is loaded**.

---

## 3. Objective internality

AIKit provides a principal technological substrate for an actor's **objective internality**: the structured and revisable context-world internal to the actor's operation while remaining inspectable through states, references, traces, relations, and consequences.

This objective internality may include:

```text
Agent identity
situated Agency
human relation
Project relation
Run / Focus relation
Profiles
Scopes
Capability field
Action field
Context Sources
retrieved material
model/harness embodiment
Host/world state
permissions and trust
familiarity/history
current unresolved boundaries
```

AIKit does not make claims about phenomenal subjectivity. It builds the inspectable operating relations from which an artificial actor can meaningfully orient and act.

The design objective is not to stuff a synthetic mind into a harness. It is to make the actor's world, powers, boundaries, provenance, and history **objectively available to cognition**.

---

## 4. Product boundaries

The high-level stack is:

```text
HUMAN / PROJECT SOURCES
        authored meaning and durable intention
                     │
                     ▼
                  AIKIT
       index · resolve · retrieve · explain
       compose · remember · project
                     │
        ┌────────────┼─────────────┐
        ▼            ▼             ▼
     HARNESS       FACTORY      WORKCELL
  embodied loop   developmental   material
                   semantics      execution
        │            │             │
        └────────────┼─────────────┘
                     ▼
                  EXECUTION
```

A separate QL/MEF module may refract or enrich any of these semantic objects through explicit provider contracts, but ordinary AIKit correctness does not require a live QL service.

---

# Part II — Ownership

## 5. What AIKit owns

AIKit owns the semantics and implementation of:

- typed resource indexing and discovery;
- Profiles and operational Profile composition;
- deterministic scope precedence;
- ContextResolution beneath canonical Context;
- Capability selection and dependency resolution;
- Action indexing and ActionSet exposure without Action ownership;
- ContextSource eligibility and retrieval access;
- trust and operational policy;
- current availability and compatibility;
- effective operational preference binding;
- Generation and target Projection;
- agent/harness bootstrap projection;
- search, ranking, explanation, and structured retrieval;
- operational memory: use history, frecency, contextual fitness observations;
- Procedures for reviewable/reversible external mutation;
- session-space relation and adapter integration;
- CLI/TUI and machine-readable API surfaces.

The central invariant is:

> **AIKit may resolve a relation without becoming the canonical owner of either side of that relation.**

---

## 6. What AIKit does not own

AIKit does not own:

- autobiographical or personally authored truth;
- Project intent, Project Canon, or Project identity;
- Run / Run Map developmental truth;
- Agent enduring identity;
- Agency's semantic meaning where defined by Project/Factory actor systems;
- application/domain Action meaning or implementation;
- Workcell provider planning and material execution;
- harness loop semantics;
- model inference;
- QL canon or MEF canon;
- QL-native experimental loop semantics;
- package-management or dotfile-management systems;
- secrets as ordinary prose/configuration.

AIKit may index, reference, resolve, retrieve, project, or invoke these things through stable provider contracts.

---

## 7. Source ownership principle

Human-owned source precedes operational resolution.

AIKit may index, interpret, resolve, learn from, and project authored user/Project material, but these operations do not transfer canonical authorship into AIKit state.

The product must distinguish at least:

```text
AUTHORED
    deliberately asserted or adopted by a human/Project

OBSERVED
    discovered from present machine or external state

DERIVED
    indexed, inferred, ranked, or synthesised by AIKit

LEARNED
    accumulated from use/evidence over time

GENERATED
    target-specific materialisation of a resolution
```

A derived or learned observation may be proposed back to an authored source. It does not silently become authored truth.

---

# Part III — Resource model

## 8. Resource field

V2 should index a wider field of typed resources while preserving source ownership.

At minimum the architecture should be capable of representing references to:

```text
Capability
Action
Agent
Agency / AgencyProfile
ContextSource
Model
Harness
Host
Workcell offer / execution-world offer
ProjectBinding
Profile
Generation
Procedure
```

Not every one of these must be AIKit-native canonical objects. Many are imported descriptors over externally owned identities.

The resource index answers:

```text
what exists?
where did it come from?
who owns it?
what revision is it?
what scopes can refer to it?
what can supply it?
what policies apply?
how can it be retrieved or invoked?
```

---

## 9. Refs and identity

V2 should prefer stable logical references over durable dependence on absolute filesystem paths.

A Project can move without changing Project identity. A ContextSource can be relocated without changing its semantic source identity. A Capability can have different local implementations while preserving its higher-level power identity where that relation is explicitly declared.

AIKit records must preserve:

- source identity;
- owner/authority;
- source revision/content hash;
- local/remote locator where applicable;
- provider identity;
- operational bindings separately from logical identity.

---

## 10. Capabilities, Skills, Tools, and Actions

The unified power language is:

### Capability

Any actor-available power that AIKit can index or resolve.

Possible implementations include:

- Skill;
- CLI operation;
- script;
- MCP tool/service;
- hook;
- HTTP/API integration;
- model-native feature;
- application Action;
- Action Set;
- agent resource.

### Skill

A reusable body of organised intelligent praxis. A Skill description can expose a compressed semantic affordance while its larger procedural body remains latent until opened.

### Tool

A directly invocable computational operation exposed through a model/harness or provider surface.

### Action

A canonical meaningful application/Project operation owned by the application or Project.

An Action can enter the Capability field without losing its Action identity.

```text
Action
    ⊂ actor-available Capability field

but

Capability ≠ Action
```

AIKit indexes Actions and can expose ActionSets. It does not duplicate application semantics into an AIKit-only command ontology.

---

## 11. Semantic affordance compression

AIKit should treat names, descriptions, signatures, and question-shaped routing text as first-class agentic surfaces.

A compact phrase can expose a large latent body of machinery:

```text
"Who am I here?"
    → Agent + Agency + Profile + ContextResolution

"What can I do?"
    → Capability/Action discovery

"Investigate this codebase structurally"
    → Skill + code-intelligence resources
```

The actor should be able to reason about a capability before the capability's complete instructions are loaded.

This is **progressive disclosure of praxis**, not merely context-window optimisation.

---
