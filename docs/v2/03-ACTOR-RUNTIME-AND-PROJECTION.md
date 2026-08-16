# Part VI — Agent and harness relation

## 21. Agent, Agency, AgentSession, Execution

AIKit must preserve the wider Factory distinctions:

```text
Agent
    enduring identity

Agency
    situated/local articulation of that Agent

AgentSession
    harness-maintained continuity

Execution
    one concrete act
```

AIKit can index Agent definitions and Agency profiles, resolve actor-facing resources, and produce a situated disclosure. It does not turn model, Harness, HarnessComposition, target-native Component, or session identity into Agent identity.

---

## 22. Situated Agency disclosure

V2 should be able to produce a derived, inspectable actor-facing read model such as:

```text
ContextDisclosure {
    agent
    agency
    project
    run_or_focus
    profiles
    scope_chain
    model
    harness
    harness_composition?
    active_components?
    active_surfaces?
    host/world
    capabilities
    actions
    source_horizon
    retrieved_context
    familiarity/history
    trust/authority notes
    generation/provenance
}
```

This is not a new constitutional primitive. It is a projection assembled from existing identities and resolution products.

It should answer naturally:

```text
Who am I here?
Where am I?
What matters?
What can I do?
What can I know?
What body am I operating through?
Which faculties/components are active and why?
What is familiar?
What remains open or unresolved?
```

For a composition-capable target, disclosure should distinguish the enduring/situated actor from the presently constituted body through which that actor operates.

---

## 23. Agentic seed

AIKit should support a small target-native bootstrap into harnesses.

The seed is not a user profile, Project dump, task procedure, or full component manifest. It establishes that the wider objective-internal world exists and tells the agent how to encounter it.

Conceptually:

```text
You operate within an AIKit-resolved world.

AIKit provides faculties for discovering your situated Agency,
Project/Focus, available powers and Actions, information horizon,
relevant prior operation, current runtime embodiment, and boundaries.

Capability and Component descriptions may include questions they can answer
or changes they can make available. Use those faculties according to present need.

Distinguish what exists, what you know exists, what you can ask
about, what you have retrieved, what is active in your present body,
and what you have intentionally brought into Focus.

Re-orient when the material or runtime situation changes.
```

The exact language is an empirical design surface. The architectural requirement is its smallness and stability.

---

## 24. Harness bootstrap, composition, and projection

Each harness adapter declares what it can receive and what it can compose:

- standing instruction mechanisms;
- native Skills;
- dynamic or next-session reload;
- hooks/session-start events;
- tool protocols;
- session state;
- subagent facilities;
- target-specific filesystem/project surfaces;
- target-native Component/plugin mechanisms where present;
- service/Contract dependency seams where present;
- activation/retraction lifecycle semantics;
- UI/inspection/trajectory contribution surfaces where present;
- runtime/loop replacement seams where present.

AIKit uses the best supported seam while preserving the same semantic world.

Possible projection/activation modes include:

```text
native projection
brokered capability
managed bootstrap
session-scoped add-dir / equivalent
generated target file
hook-based disclosure
component/plugin mounting
surface contribution
live target-native activation
next-session composition
```

No adapter may claim immediate activation where the harness only reloads on the next session. No adapter may claim reversible live effects where the target cannot actually retract them.

A composition-capable adapter should be able to project a resolved `HarnessComposition` while preserving canonical refs through target-native registrations.

---

## 25. Authority modes for agent-facing files and runtime contributions

Agent-facing files and target-native runtime registrations must not silently change authority class.

Useful file modes include:

```text
AUTHORED SOURCE
    deliberately maintained by human/Project

GENERATED PROJECTION
    derived from AIKit resolution

MANAGED BOOTSTRAP
    thin AIKit-owned pointer into AIKit context cognition

MIXED / IMPORTING SURFACE
    authored file that explicitly imports/references managed content
```

Runtime contributions should likewise retain whether they are:

```text
TARGET-NATIVE
    owned by the harness/component implementation

PROJECTED
    generated/bound from an externally owned canonical resource

DERIVED
    read-model/inspection state over canonical/runtime evidence
```

Generated material retains source refs, generation hash, target, scope, and overwrite policy. Projected runtime contributions retain the canonical ref they expose where applicable.

Generated projection or target-native registration never writes backwards into canonical authored source merely because it is active.

---

# Part VII — Models, harnesses, hosts, Workcells

## 26. Models

A Model is a selectable generative intelligence substrate, not an Agent.

AIKit may index:

- provider/model identity;
- context/token limits;
- modalities;
- tool/calling support;
- cost/performance metadata where useful;
- harness compatibility;
- observed contextual fitness;
- declared and empirically learned dispositions.

Model disposition is empirical and contextual. It must not harden into personality folklore or become Agent identity.

---

## 27. Harnesses and HarnessComposition

A Harness is the embodied runtime technology through which a model receives context, maintains a session, invokes tools/capabilities, streams events, and interacts with the environment.

AIKit resolves harness compatibility and projects into harness-native surfaces.

A Harness need not be internally atomic. Rich targets may expose a composable body in which model adapters, loop drivers, tools, services, policies, persistence, context faculties, subagent facilities, observers and UI surfaces are themselves replaceable Components.

For those targets AIKit should be able to derive an inspectable relation equivalent to:

```text
HarnessComposition
    harness
    model binding
    Component bindings
    Contract/provider bindings
    Capability bindings
    Action projections
    ContextSource faculties
    active Surfaces
    ActivationScopes / lifetime state
    target revision
    Generation / provenance
```

This is the **resolved body**, not a second Harness identity.

The same Agent/Agency may therefore operate through:

```text
Harness A + Composition A₀
Harness A + Composition A₁
Harness B + Composition B₀
```

without identity drift.

Harnesses own their native mechanics. AIKit owns resolution, explanation, projection/binding and lifecycle truth about what it asked the target to activate. Target-native plugin/service semantics remain target-owned.

Harnesses own their loop mechanics unless a harness explicitly exposes a swappable runtime seam. Experimental QL-native loops remain separate providers over such a seam.

The full composition contract is in `09-COMPOSABLE-RUNTIME-ENVIRONMENTS.md`.

---

## 28. Hosts

AIKit Host discovery owns observed machine facts such as:

- OS/architecture;
- installed binaries;
- reachable services;
- model/harness availability;
- target-native component/plugin support where discoverable;
- resource capacity;
- current Workcell/provider availability;
- network reachability where relevant.

Authored descriptions of a machine's role may live in independently owned source material and bind explicitly to an observed Host.

Observed state does not silently rewrite authored machine meaning.

---

## 29. Workcell

Workcell remains the modular materialisation layer.

The boundary is:

```text
AIKit:
"What should this actor be able to do here, and what runtime body should expose it?"

Workcell:
"How can this deployment make the required material execution world true?"
```

AIKit may index Workcell offers and current availability.

It does not own provider planning, workspace creation, runtime/service binding, or the material Binding Graph.

A resolved actor world can therefore produce an `ExecutionDemand` while Workcell produces the material execution world that satisfies it. A harness Component may consume or expose a Workcell-backed faculty without becoming the owner of that material world.

---

# Part VIII — Memory and learned ease

## 30. AIKit memory is memory of operating relation

AIKit should remember **how actors traverse and compose the environment**, not become the canonical autobiographical memory store for every source domain.

Three important classes are:

### Operational history

```text
what resource/component was used
when
where
by which actor/context
through which HarnessComposition where relevant
```

### Fitness evidence

```text
how well did this resource/composition satisfy this type of demand
under these conditions?
```

### Navigational familiarity

```text
which routes through Projects, resources, capabilities, sources,
models, Components, Surfaces, and profiles have become well travelled?
```

---

## 31. Zoxide-style learned accessibility

The key UX principle is learned ease:

```text
search broadly
      ↓
use resource
      ↓
retain route/context
      ↓
future search becomes lower friction
```

This can improve:

- search ordering;
- autocomplete;
- TUI ranking;
- contextual retrieval proximity;
- component/surface discovery where the user explicitly seeks composition;
- explicit "what has worked here before?" queries;
- deterministic tie-breaking among otherwise eligible candidates where policy permits.

It does not need to become unsolicited capability or component suggestion.

Signals remain separate:

```text
frecency
fitness
contextual relevance
explicit preference
trust
availability
```

---

## 32. Growth and changed ground

AIKit should retain enough history to let an actor inspect change over time:

```text
state S0 / body B0
  ↓ activity or composition change
state S1 / body B1
  ↓ activity
state S2 / body B1
```

Useful questions include:

```text
What was true when this activity began?
What body/components were active?
What changed?
What became familiar?
What repeatedly failed?
What source or Profile changed?
What component/provider binding changed?
What is easier to reach now?
What remains unresolved?
```

This history is evidence for future resolution, not permission to rewrite authored intent silently.

---

# Part IX — Project relation

## 33. ProjectBinding

AIKit should understand a Project without equating Project identity with a repository or path.

`ProjectBinding` is the operational relation between a local/remote constituent and a durable ProjectRef.

A Project may contain multiple repositories, external sources, design canon, Run history, or other constituents.

A preferred filesystem root may help discover candidate Projects. It is not the constitutional definition of Project.

---

## 34. Project resource declarations

Projects may expose AIKit-specific operating declarations for:

- Profiles;
- capabilities;
- Actions;
- Context Sources;
- agents/agencies;
- model/harness requirements;
- Component/Contract requirements or preferences where appropriate;
- desired Surface/projection availability;
- execution/world requirements;
- source bindings.

These declarations are resolver configuration, not Project Canon.

Project-specific authored meaning remains Project-owned source material.

---

## 35. Project Map

AIKit should be able to route into a Project Map joining source/Git, code intelligence, Project Canon, semantic sources, Action Catalog, Runs/Evolution, and optional wider knowledge horizons.

The Project Map remains an index over Project reality, not a replacement for the underlying sources.

AIKit uses it as a high-value ContextSource/discovery provider.

Runtime Component/Surface relations may be navigable alongside ProjectMap resources in a common relation read model, but composition, containment, federation, framing, binding and semantic edges retain distinct meanings.

---

# Part X — Projection, Generation, Procedures, hooks

## 36. Generation

A Generation is an immutable content-addressed materialisation of an effective resolution.

V2 should preserve the current strengths:

```text
resolve
build temp generation
materialise
validate
content-hash
atomic current swap
retain previous
```

A failed generation must never replace the previous active view.

Where target-native Components are part of effective state, the intended composition and projection bindings participate in Generation provenance/hash when they materially affect the actor world. Live target activation may occur after Generation materialisation, but its observed activation state remains separately inspectable.

---

## 37. Projection provenance

Every materialised actor-facing view should be able to explain:

```text
which source refs contributed
which ProjectBinding / ProjectRef applied
which Profiles and ResolutionScopes applied
which Agent/Agency applied
which CapabilitySet and ActionSet applied
which ContextSources were eligible/retrieved
which model/harness target was used
which Components were selected
which Contract/provider bindings satisfied them
which Surfaces/projections were contributed
which ActivationScopes/lifetimes apply
which target-native activation mode is in force
which privacy/egress decisions applied
which optional QL-derived readings contributed
which adapter/version generated the target
which generation hash is active
```

The explanation question grows from:

> why is this Skill active?

into:

> **why did this actor receive this world and this body?**

---

## 38. Procedures

Procedure remains the correct boundary for reviewable, forward-checked, reversible mutation outside immutable Generations.

Use Procedure for operations such as:

- adopting or editing foreign configuration;
- installing target integrations or Components;
- modifying user-owned external state;
- invoking reproducibility/bootstrap mechanisms under agent control;
- durable promotion of generated/learned material where appropriate;
- composition changes whose target effects exceed the target-native reversible activation boundary.

Procedure does not become a generic workflow language.

A permitted live mount/unmount inside a target-native reversible component lifecycle need not be inflated into a Procedure if it does not mutate external/user-owned durable state. The adapter must still record activation provenance and actual lifecycle semantics.

---

## 39. Hooks and runtime events

The existing hook phases remain useful:

```text
gate → transform → verify → inject → observe → capture
```

V2 may use hooks for:

- session-start seed establishment;
- material context-change notification;
- new Generation disclosure;
- policy enforcement;
- activation-change signalling;
- trace capture.

Hooks do not become a hidden reasoning engine. They establish conditions, enforce policy, inject required orientation, or observe events.

Composable harnesses may additionally expose native event/interception systems. AIKit should preserve their target semantics through adapters rather than forcing every event into the hook phase vocabulary. Durable execution/session events, live interception events, Component lifecycle events and Factory Event/Trace evidence remain distinguishable.

---

# Part XI — Privacy and disclosure

## 40. Visibility is an explicit relation

Filesystem readability must not imply model visibility or active Surface exposure.

AIKit should distinguish, conceptually and eventually in policy:

```text
filesystem-readable
AIKit-indexable
AIKit-discoverable
agent-retrievable
Project-eligible
provider-egress-permitted
projected to target
mounted/active in target
visible on a human/agent Surface
loaded into current client/model context
```

These may not collapse to one boolean.

A local agent-visible source is not automatically permitted to leave the machine for every external model provider. A mounted UI Component is not automatically model-visible. A model-facing tool Surface is not automatically human-visible.

---

## 41. Source privacy classes

The design should leave room for source classes such as:

```text
shareable
private/local
agent-visible-by-context
machine-local
never-agent-visible
encrypted/reference-only
```

Exact syntax is an implementation task.

`never-agent-visible` payloads must remain outside the agent retrieval plane even if a runtime Component can technically access the underlying filesystem.

Secrets should be referenced through dedicated secret mechanisms rather than stored as ordinary source prose/configuration.

---
