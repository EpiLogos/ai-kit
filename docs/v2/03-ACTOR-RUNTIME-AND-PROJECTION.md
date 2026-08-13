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

AIKit can index Agent definitions and Agency profiles, resolve actor-facing resources, and produce a situated disclosure. It does not turn model or harness identity into Agent identity.

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
What is familiar?
What remains open or unresolved?
```

---

## 23. Agentic seed

AIKit should support a small target-native bootstrap into harnesses.

The seed is not a user profile, Project dump, or task procedure. It establishes that the wider objective-internal world exists and tells the agent how to encounter it.

Conceptually:

```text
You operate within an AIKit-resolved world.

AIKit provides faculties for discovering your situated Agency,
Project/Focus, available powers and Actions, information horizon,
relevant prior operation, and current boundaries.

Capability descriptions may include questions they can answer.
Use those faculties according to present need.

Distinguish what exists, what you know exists, what you can ask
about, what you have retrieved, and what you have intentionally
brought into Focus.

Re-orient when the material situation changes.
```

The exact language is an empirical design surface. The architectural requirement is its smallness and stability.

---

## 24. Harness bootstrap and projection

Each harness adapter declares what it can receive:

- standing instruction mechanisms;
- native Skills;
- dynamic or next-session reload;
- hooks/session-start events;
- tool protocols;
- session state;
- subagent facilities;
- target-specific filesystem/project surfaces.

AIKit uses the best supported seam while preserving the same semantic world.

Possible projection modes include:

```text
native projection
brokered capability
managed bootstrap
session-scoped add-dir / equivalent
generated target file
hook-based disclosure
```

No adapter may claim immediate activation where the harness only reloads on the next session.

---

## 25. Authority modes for agent-facing files

Agent-facing files must not silently change authority class.

Useful modes include:

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

Generated material retains source refs, generation hash, target, scope, and overwrite policy.

Generated projection never writes backwards into canonical authored source.

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

## 27. Harnesses

A Harness is the embodied runtime surface through which a model receives context, maintains a session, invokes tools/capabilities, streams events, and interacts with the environment.

AIKit resolves harness compatibility and projects into harness-native surfaces.

Harnesses own their loop mechanics unless a harness explicitly exposes a swappable runtime seam. Experimental QL-native loops remain separate providers over such a seam.

---

## 28. Hosts

AIKit Host discovery owns observed machine facts such as:

- OS/architecture;
- installed binaries;
- reachable services;
- model/harness availability;
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
"What should this actor be able to do here?"

Workcell:
"How can this deployment make that materially true?"
```

AIKit may index Workcell offers and current availability.

It does not own provider planning, workspace creation, runtime/service binding, or the material Binding Graph.

A resolved actor world can therefore produce an `ExecutionDemand` while Workcell produces the material execution world that satisfies it.

---

# Part VIII — Memory and learned ease

## 30. AIKit memory is memory of operating relation

AIKit should remember **how actors traverse the environment**, not become the canonical autobiographical memory store for every source domain.

Three important classes are:

### Operational history

```text
what resource was used
when
where
by which actor/context
```

### Fitness evidence

```text
how well did this resource/composition satisfy this type of demand
under these conditions?
```

### Navigational familiarity

```text
which routes through Projects, resources, capabilities, sources,
models, and profiles have become well travelled?
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
- explicit "what has worked here before?" queries;
- deterministic tie-breaking among otherwise eligible candidates where policy permits.

It does not need to become unsolicited capability suggestion.

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
state S0
  ↓ activity
state S1
  ↓ activity
state S2
```

Useful questions include:

```text
What was true when this activity began?
What changed?
What became familiar?
What repeatedly failed?
What source or Profile changed?
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
- execution/world requirements;
- source bindings.

These declarations are resolver configuration, not Project Canon.

Project-specific authored meaning remains Project-owned source material.

---

## 35. Project Map

AIKit should be able to route into a Project Map joining source/Git, code intelligence, Project Canon, semantic sources, Action Catalog, Runs/Evolution, and optional wider knowledge horizons.

The Project Map remains an index over Project reality, not a replacement for the underlying sources.

AIKit uses it as a high-value ContextSource/discovery provider.

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

---

## 37. Projection provenance

Every materialised actor-facing view should be able to explain:

```text
which source refs contributed
which ProjectBinding / ProjectRef applied
which Profiles and scopes applied
which Agent/Agency applied
which CapabilitySet and ActionSet applied
which ContextSources were eligible/retrieved
which privacy/egress decisions applied
which model/harness target was used
which optional QL-derived readings contributed
which adapter/version generated the target
which generation hash is active
```

The explanation question grows from:

> why is this Skill active?

into:

> **why did this actor receive this world?**

---

## 38. Procedures

Procedure remains the correct boundary for reviewable, forward-checked, reversible mutation outside immutable Generations.

Use Procedure for operations such as:

- adopting or editing foreign configuration;
- installing target integrations;
- modifying user-owned external state;
- invoking reproducibility/bootstrap mechanisms under agent control;
- durable promotion of generated/learned material where appropriate.

Procedure does not become a generic workflow language.

---

## 39. Hooks

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

---

# Part XI — Privacy and disclosure

## 40. Visibility is an explicit relation

Filesystem readability must not imply model visibility.

AIKit should distinguish, conceptually and eventually in policy:

```text
filesystem-readable
AIKit-indexable
AIKit-discoverable
agent-retrievable
Project-eligible
provider-egress-permitted
loaded into current client
```

These may not collapse to one boolean.

A local agent-visible source is not automatically permitted to leave the machine for every external model provider.

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

`never-agent-visible` payloads must remain outside the agent retrieval plane.

Secrets should be referenced through dedicated secret mechanisms rather than stored as ordinary source prose.

---
