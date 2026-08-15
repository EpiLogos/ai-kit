# Part XXI — Composable runtime environments and agent-native surfaces

## 70. Determination

AIKit must be able to describe and resolve an embodied agent environment whose runtime body is itself compositionally constituted.

A Harness remains the runtime technology through which a model receives context, maintains a session, invokes powers, emits events, and encounters an environment. Some Harnesses are comparatively fixed. Others expose first-class internal composition: model adapters, loop drivers, tools, services, policies, persistence, context contributions, UI nodes, observers, sandboxes, subagent facilities, and other runtime faculties may all be mounted, replaced, scoped, or withdrawn.

AIKit therefore needs a composition grammar beneath the Harness boundary without making any one plugin framework the global ontology.

The governing relation is:

```text
Agent
    enduring identity
        ↓ situated as
Agency
        ↓ embodied through
Harness
        ↓ resolved/materialised as
HarnessComposition
        ↓ constituted by
Components + bindings + scopes + contributions
```

Changing the Harness, HarnessComposition, model, session, or provider does not by itself change Agent identity.

`HarnessComposition` is a derived operational resolution. It is not a second Agent, Project, Context, Run, or Harness identity.

---

## 71. Why this belongs in AIKit

AIKit already resolves which world of powers and information should be available to an actor and projects that result into heterogeneous harnesses. A composable harness makes the target of that projection richer: the target is not only a named harness plus files/configuration, but may be a live graph of runtime components and surface contributions.

AIKit should therefore be able to answer:

```text
Which runtime components exist?
Which are eligible here?
Which are selected for this Agency/session?
What does each require?
Which provider satisfies each requirement?
What does each contribute?
Where is each contribution visible/operative?
What owns its lifetime?
Can it activate or retract live?
What canonical Action/Capability/Source/Reading does it expose, if any?
What changed between Body₀ and Body₁?
Why did this actor receive this composition?
```

This extends context cognition into **embodiment cognition** without moving harness implementation semantics into AIKit.

---

## 72. Component

A `Component` is an addressable composable unit which may participate in a runtime environment.

A Component descriptor may declare:

```text
ComponentDescriptor {
    ref
    owner / source / revision
    implementation_target
    requirements[]
    provisions[]
    contributions[]
    supported_surfaces[]
    activation_modes[]
    configuration_schema?
    trust / policy metadata
    provenance
}
```

The serialized schema is an implementation contract. The semantic laws are:

- Component identity is not Capability identity.
- Component identity is not Action identity.
- A Component may provide several Capabilities, satisfy several Contracts, expose several Actions, contribute several Surface elements, or do none of those individually.
- The same semantic Component may have target-native implementations for different harnesses where an explicit adapter/projection relation preserves identity and provenance.
- A target-native package/plugin identifier must not silently become the canonical identity of an externally owned Action, Capability, ContextSource, Agent, or Project.

A Cordis plugin is one concrete example of a Component implementation. A Pi extension, harness hook package, native UI extension, or other dynamically composed module may realise the same higher-level relation without becoming Cordis-shaped.

---

## 73. Contracts, providers, consumers, requirements

Composable environments need a dependency grammar distinct from Action/Capability meaning.

### Contract

A `Contract` describes an interface or service seam against which providers and consumers can compose.

Examples may include a filesystem seam, session persistence seam, model adapter registry, surface-slot contract, runtime observer seam, or target-native service definition.

A Contract is not automatically an actor-facing Capability. It may be entirely infrastructural.

### Provider

A Provider is a Component or implementation binding that satisfies a Contract or operational requirement.

Provider identity remains distinct from the thing provided.

### Consumer

`Consumer` is primarily a role in a relation: a Component consumes a Contract/provider/faculty because its operation requires it. AIKit need not create a universal Consumer object merely to record that relation.

### ComponentRequirement / coeffect relation

A Component may declare what must be available in its surrounding runtime environment for it to activate or remain operative.

Conceptually:

```text
ComponentRequirement {
    contract_or_resource
    required | optional
    compatibility / constraint
    scope?
    reactive?: bool
}
```

This is the generic seam through which systems with **reactive coeffects** can be represented. A Cordis adapter may preserve the stronger native semantics that a component reacts as required context services appear, disappear, or change. Less dynamic targets may resolve the same requirement at generation/session start.

Requirement does not mean `Context` in the wider Factory sense. It describes a condition of runtime composition inside an operative environment.

---

## 74. Contributions, effects, and reversibility

A mounted Component may contribute changes to its runtime environment.

Examples include:

```text
service/provider registration
tool registration
Action projection
human command
prompt/context section
policy/interceptor/listener
ContextSource faculty
read-model provider
UI node / slot / renderer
trajectory or inspection view
observer / telemetry hook
model adapter
loop runtime provider
subagent facility
filesystem / shell / sandbox backend
```

AIKit should represent contribution identity, origin, target and lifecycle separately from the semantic identity of anything exposed through that contribution.

Conceptually:

```text
ComponentContribution {
    component_ref
    kind
    target_contract_or_surface
    exposed_ref?
    activation_scope
    lifetime_owner
    activation_mode
    retraction_mode
    provenance
}
```

When a runtime guarantees that activation effects are owned and completely unwound on removal, the contribution can advertise **revertible-effect semantics**. When the target cannot guarantee live reversal, AIKit must expose the weaker truth: restart, next-session activation, irreversible external mutation, or Procedure-mediated rollback.

AIKit must not claim dynamic unload/reversal merely because a source Component is conceptually removable.

---

## 75. Surface

A `Surface` is an addressable locus through which some part of an operative world becomes encounterable, visible, or invocable by a human, agent, or software client.

Examples include:

```text
CLI command surface
agent tool surface
conversation surface
trajectory / run-inspection surface
TUI region
web application region
human command registry
automation surface
HTTP/API surface
editor integration surface
```

Surface and Projection are related but distinct:

```text
Surface
    where/how encounter can occur

Projection
    how a particular canonical or resolved thing is represented there
```

The same Action can be projected onto several Surfaces while retaining one ActionRef and one authoritative domain handler.

The same Reading or read model can also be projected onto several Surfaces without becoming an Action. A status view, Context inspector, QL Circuit visualisation, trajectory renderer, or source reading is not made into an Action merely because it appears in a composable UI.

Surface contribution therefore composes naturally with the Agent-Native Action law:

```text
Project/Application Action
        ↓ canonical ActionRef
Component contribution
        ↓ target-native binding
CLI | tool | UI | API | other Surface
```

The transport remains a projection of the Action, never its semantic owner.

---

## 76. HarnessComposition: the resolved body

For composition-capable targets AIKit should be able to derive an inspectable `HarnessComposition` or equivalent read model.

Conceptually:

```text
HarnessComposition {
    harness_ref
    agency_ref?
    session_ref?
    model_binding?
    component_bindings[]
    contract_bindings[]
    capability_bindings[]
    action_projections[]
    context_source_faculties[]
    surfaces[]
    activation_scopes[]
    lifecycle_state
    target_revision
    generation / provenance
}
```

This is the actor's presently constituted runtime body, not an ontology of the whole actor.

A rich composition may include:

```text
model
loop runtime
prompt/context faculties
tool and Action surfaces
knowledge/retrieval faculties
filesystem / shell / terminals
sandbox and approval policy
subagent facilities
session/history services
observers
UI surfaces
material bindings
```

A thin harness may expose only a small subset. AIKit's architecture must span both without treating the thin case as the conceptual maximum.

---

## 77. Three distinct scope/lifetime questions

The existing AIKit scope chain answers resolution precedence:

```text
user/global → host → project → project-local → session → task → invocation
```

Composable runtimes introduce two additional questions which must not be collapsed into that chain.

### ResolutionScope

Why was this declaration/resource selected for the present resolution?

### ActivationScope

For which actor/session/task/runtime region is this contribution actually visible or operative?

### Lifetime ownership

What owns this active contribution, and what event causes it to retract or be rebuilt?

For example, a Component may be selected because of Project scope, mounted only for one AgentSession, and have contributions whose lifetime is owned by that session's target-native plugin context.

AIKit explanations and adapters must preserve all three answers.

---

## 78. Relation grammar

The wider architecture already contains several ways for things to be "together". Runtime composition must not flatten them into a generic parent edge.

At minimum preserve distinctions equivalent to:

```text
CONTAIN
    a durable whole owns/includes a member or subspace

FEDERATE
    independent Spaces/providers are navigable together

FRAME
    a contextual set counts as the relevant whole for this act

COMPOSE / MOUNT
    a Component participates in this runtime environment

REQUIRE
    a Component depends on a Contract/resource/condition

PROVIDE
    a provider satisfies a Contract/requirement

CONTRIBUTE
    an active Component adds a service/effect/surface element

SCOPE
    a contribution is visible/operative for a bounded region

PROJECT
    a canonical/resolved thing is represented on a target Surface

BIND
    a logical resource is presently realised by a concrete provider

MAP / REFRACT
    an independent object/whole is related through another semantic/meta field
```

These relations may all be rendered in common relation views while retaining distinct ownership and semantics.

---

## 79. Actor participation in body composition

A composable runtime makes it possible for an authorised actor to participate in shaping its own operative embodiment.

The desired model is not unrestricted self-modification. It is ordinary agent-native operation over inspectable composition state:

```text
Body₀
    ↓ inspect/discover
candidate Component
    ↓ stage / explain / policy / Procedure where required
activate or bind
    ↓
requirements satisfied
    ↓
contributions become operative
    ↓
Body₁
```

The reverse path should be equally inspectable where the target supports retraction.

Permitted composition changes may be exposed through application/AIKit Actions and Procedures. The agent should be able to ask what a change would add, what it requires, what authority permits it, what surfaces it changes, whether it takes effect live, and how it can be undone.

Agent identity does not change merely because its body composition changes.

This is a concrete extension of objective internality: the actor's effective interior world can be objectively constituted by runtime relations that are discoverable, recomposable, and provenance-bearing.

---

## 80. DeepSeek Harness / Cordis reference integration

DeepSeek Harness is the first rich reference target for this contract because its current architecture makes the relevant structure explicit:

- the running harness is a plugin tree;
- model adapters, tools, session logging, the agent loop, persistence, policies, runtime services, and the UI are composable plugins;
- Cordis services supply stable dependency seams;
- plugins declare required services;
- registrations are lifecycle-owned effects;
- typed events provide observation/interception boundaries;
- a capability seam distinguishes service definition, provider, and consumer;
- the session log provides an append-oriented inspectable trajectory;
- web UI nodes and renderers are themselves composable contributions;
- the loop is behind a replaceable agent/runtime seam.

The AIKit adapter should preserve those native semantics rather than reducing DeepSeek Harness to "another executable with a skills directory".

A reference mapping is:

```text
AIKit / shared meaning             DeepSeek Harness / Cordis
----------------------             -------------------------
Harness                            DeepSeek Harness
HarnessComposition                 resolved profile/plugin tree
Component                          Cordis plugin/package contribution
Contract                           Cordis service definition / seam
Provider                           service provider plugin
ComponentRequirement               injected service dependency / reactive coeffect
ComponentContribution              registration / effect
ActivationScope                    scoped registration context / agent scope
Surface                            chat / trajectory / command / tool / client surface
ProjectionBinding                  canonical ref → native registration/node/tool
Event / Trace input                durable SessionEvent + live extension events
LoopRuntime provider               replaceable agent-loop/runtime integration
```

Cordis `Context` is a runtime service context and must not be renamed to or confused with canonical Factory `Context = Operative World + Information Horizon + Focus`.

Cordis is reference prior art and a conformance target, not a required AIKit implementation dependency.

---

## 81. Generation, live activation, and Procedures

AIKit already has two useful mutation modes:

- immutable `Generation` for resolved/materialised target state;
- reversible `Procedure` for reviewable mutation of external state.

Composable runtimes add a third implementation characteristic: some target contributions can activate/retract live under target-native lifecycle management.

The architecture should therefore distinguish:

```text
GENERATED
    becomes effective through a new immutable AIKit Generation

LIVE-MOUNTED
    target confirms runtime activation/retraction without session replacement

NEXT-SESSION
    projection is present but target reload occurs only on new session/task

PROCEDURE-MEDIATED
    activation changes external/user-owned state and uses Procedure
```

A target adapter reports the actual mode. AIKit does not simulate hot reload by silently relabelling restart-required changes as live.

Composition state and target-native activation evidence should participate in explanation/provenance and, where materially relevant, the effective resolution hash.

---

## 82. QL runtime relation

The existing harness-neutral `LoopRuntime` architecture remains unchanged.

A composable Harness may simply provide an especially strong native mounting point for the already-defined runtime alternatives:

```text
Harness / Host
    model + tools + session + environment
              │
              ▼
        LoopRuntime
 classic | ql-direct | ql-deep
```

Runtime selection and Component composition are separately observable variables. A QL runtime may be supplied as a Component/provider on a capable target without turning Component semantics into QL semantics or changing the shared `LoopRuntime` contract.

QL-specific inspection UI can be implemented as independent Surface contributions over the same portable runtime events/state: Circuit position, `Rij`, return/difference, determination, closure/reopening, conjugation/depth and comparator views can become visible without altering the experimental recurrence condition.

---

## 83. Acceptance laws

V2 composition work is acceptable when fixtures/adapters prove at least:

1. one Component can expose several roles without collapsing Component, Capability and Action identity;
2. one canonical ActionRef can be projected through several Surfaces with one authoritative domain handler;
3. a non-Action Reading can contribute to a Surface without acquiring Action identity;
4. required versus optional Component requirements resolve visibly and provider substitution preserves Contract/resource identity;
5. ResolutionScope, ActivationScope and lifetime ownership remain distinguishable;
6. a Component selected at Project scope can be activated only for one AgentSession without scope confusion;
7. target-native live retraction is reported only when actually supported; otherwise next-session/Generation/Procedure effects are explicit;
8. removing/replacing a Component preserves unrelated canonical resource identities;
9. `HarnessComposition` can change while AgentRef and ProjectRef remain stable;
10. a thin/static harness and a richly composable harness both fit the same AIKit resolution model;
11. DeepSeek Harness/Cordis can round-trip a representative component/service/provider/consumer/effect/surface composition through an adapter without renaming Cordis Context into Factory Context;
12. target-native event/trajectory data retains provenance sufficient to explain which composition produced an Execution;
13. QL Classic/Direct/Deep runtime selection remains independent from harness-component selection;
14. no target plugin/package ID becomes semantic authority for an externally owned Action, Capability, Project, Agent, Source, or QL object.

The implementation should add only cross-system schemas that are needed for stable interoperability. Target-private plugin configuration remains target-private.
