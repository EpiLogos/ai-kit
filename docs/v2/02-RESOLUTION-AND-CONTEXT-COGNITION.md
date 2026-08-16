# Part IV — Resolution

## 12. ContextResolution

AIKit should produce an inspectable operational resolution beneath canonical Context.

Conceptually:

```text
ContextResolutionRequest {
    project?: ProjectRef
    profile_refs: ProfileRef[]
    agent?: AgentRef
    agency?: AgencyRef
    focus?: FocusRef | structured focus
    session?: SessionRef
    task?: TaskRef
    target?: TargetClient
    explicit_overrides?: ...
    required_capabilities?: CapabilityRequirement[]
    required_components_or_contracts?: ...
    current_execution_conditions?: ...
}
```

The result may resolve:

```text
ProjectBinding
Profiles/scopes
Agent/Agency relation
CapabilitySet
ActionSet
ContextSource horizon
Model/Harness candidates or binding
Component eligibility/selection
Contract/provider bindings
Surface/projection availability
HarnessComposition where the target supports internal composition
Host relation
execution-world requirements/offers
projection policy
retrieval/disclosure policy
```

The exact serialized schema is a V2 implementation contract. The semantic separation is the important part.

A rich composable target may therefore receive an inspectable body resolution such as:

```text
Harness
  + selected Components
  + satisfied Contract/provider relations
  + Component requirements
  + activation scopes/lifetimes
  + Surface contributions
  + canonical Action/Capability/Source projection bindings
  = HarnessComposition
```

`HarnessComposition` remains derived operational state. It does not replace canonical Context, Agent, Agency, Project, Action, Capability, or Harness meaning.

---

## 13. Scope precedence and activation scope

The current specificity model remains a strong foundation for **resolution precedence**:

```text
managed policy constraints
    user/global
    host
    project shared
    project-local private
    session
    task/pane
    one-shot invocation
```

V2 should extend what can be resolved through this algebra without creating a parallel precedence system for personal sources, Actions, Agents, Components, or QL relations.

A source or Component may live outside AIKit while entering resolution at the appropriate scope.

Later/more-specific ordinary declarations may develop earlier ones. Managed denials and hard eligibility boundaries remain non-overridable through ordinary lower-scope preference.

Composable runtimes add two related but distinct lifecycle questions:

```text
ResolutionScope
    which declaration/scope caused this Component or binding to be selected?

ActivationScope
    for which Agent/Agency/session/task/runtime region is the contribution live?

Lifetime owner
    what owns the active registration and causes it to retract/rebuild?
```

These must not be collapsed. A Component can be selected because of Project scope, activated only for one AgentSession, and have its actual registration lifetime owned by a target-native plugin/runtime context.

---

## 14. Eligibility versus preference

AIKit must keep distinct:

```text
trust
availability
policy
platform compatibility
dependencies/conflicts
component/contract compatibility
explicit authored preference
contextual relevance
fitness
frecency
```

Eligibility is determined by hard boundaries such as policy, trust, compatibility, dependency/Contract satisfaction, and target support.

Preference and learned signals operate only among eligible possibilities unless an explicit interface is asking for diagnostics rather than selection.

A human declaration that a resource or Component is preferred never makes it trusted, available, compatible, or activatable.

---

## 15. Authored preference versus effective binding

The durable semantic choice and the current implementation binding are different objects of knowledge.

```text
AUTHORED PREFERENCE INTENT
          ↓
semantic statement / selector
          ↓
AIKit resolution against current world
          ↓
EFFECTIVE OPERATIONAL BINDING
```

Example:

```text
Authored intent:
"Prefer structural code intelligence before broad text search."

Observed world:
GitNexus is installed/trusted.
Semantic provider is unavailable.
ripgrep is available.

Resolved binding:
prefer GitNexus for this demand.
```

The same law applies inside a composable harness. An authored preference for a trajectory inspector or filesystem faculty may resolve to one target-native Component/provider today and another tomorrow while the higher-level preference and canonical resource refs remain stable.

AIKit may cache the resolved binding, but it must retain provenance back to the authored intent and current resource observations.

---

# Part V — Information horizon and disclosure

## 16. Context Sources

A ContextSource is an addressable source of information that may participate in an actor's information horizon.

Examples include:

- Project Canon;
- repository/source tree;
- Project Map;
- GitNexus;
- semantic wiki;
- external documentation;
- papers/research collections;
- prior Runs;
- authored human/agent orientation sources;
- other Projects;
- optional graph/knowledge systems.

AIKit should not mirror all source payloads into one proprietary database as the source of truth.

A provider should be able to expose descriptors containing enough information to support discovery, eligibility, freshness, provenance, and retrieval.

### 16.1 Knowledge navigation over Context Sources

Semantic Wikis and SourcePools are first-class specialised Context Sources. They participate in the same resolution, Search, Context, Explain and History application state as other resources even though their provider semantics differ.

The generic knowledge-navigation field should be able to address concepts equivalent to:

```text
Space
    a persistent/addressable whole or subspace

Node
    an addressable semantic or knowledge item

Source
    an addressable evidential/retrieval item

Frame
    a contextual whole selected for a present act; may cross Spaces/providers

Route
    an operational traversal through addressable resources
```

These are navigation roles, not a command to collapse provider ontologies into one schema. `SemanticWiki`, `SourcePoolProvider`, `CodeIndexProvider`, ordinary ContextSources and Project-owned material retain their own authority and relation meanings.

`ProjectMap` is the Project-scoped federation/index that can bind these distinct lenses through stable refs. It is not a universal graph database. A Wiki edge, a code-graph edge, a source-provenance relation and a learned route remain distinguishable even when one navigation path crosses all four.

A selected knowledge item should be able to move from **leaf to local whole**: exact lookup/search may first resolve one Node/Source/ref, after which deliberate relation expansion can reveal its enclosing Space, neighbourhood, provenance, cross-lens bindings, Frames and known Routes. Expansion is explicit, bounded and explainable; discovering a larger neighbourhood does not imply loading its payload into current context.

`KnowledgeRoute` records actual traversal and may contribute learned accessibility/familiarity. It never manufactures authored semantic relations, code-graph truth, trust or preference. Route history remains evidence about use.

The same provider-neutral navigation/application services should serve human TUI, CLI and agent surfaces. A richer visual relation view must therefore be a read-model projection over canonical refs and provider-owned relations rather than a TUI-only semantic store.

The persistent `Space` / contextual `Frame` distinction also protects runtime composition from a level error: a target-native plugin `Context`, mounted Component, ActivationScope, or Surface is not automatically a WikiSpace, Project, or canonical Factory Context merely because it defines a local environment. Containment, federation, framing, composition, binding and projection remain distinct relations.

---

## 17. Five disclosure states

The core epistemic disclosure ladder is:

```text
EXISTS
    the resource/source is part of the wider world

KNOWN-TO-EXIST
    the actor has been given enough information to recognise it

ASKABLE
    the actor knows that a faculty/query can disclose more

RETRIEVED / PRESENTLY KNOWN
    material has entered active context or structured working memory

FOCUSED
    the actor has intentionally made it salient to the current act
```

These states are orthogonal to operational capability/composition states such as available/enabled/projected/loaded/invoked or mounted/retracted.

A Skill may be enabled while not currently attended to. A source may be askable while none of its payload is loaded. A Component may be mounted while the Capability it exposes is never invoked.

---

## 18. Operational capability and composition states

V2 should preserve and generalise the current distinction between available, enabled, and loaded.

A fuller operational chain is:

```text
CATALOGUED / EXISTS
        ↓
ELIGIBLE / AVAILABLE
        ↓
ENABLED / RESOLVED
        ↓
PROJECTED / BROKERED / SELECTED FOR COMPOSITION
        ↓
LOADED / ACTIVE / MOUNTED IN CLIENT
        ↓
INVOKED / USED
```

The UI and API must not imply one state merely because another is true.

For a target-native Component, AIKit should additionally distinguish actual activation semantics where relevant:

```text
live-mounted / retractable
next-session
Generation-bound
Procedure-mediated
unsupported
```

These describe effect timing/lifecycle, not semantic authority.

---

## 19. Structured absence

V2 should use the following six absence meanings as a provisional design scaffold:

```text
OPEN
    deliberately undetermined; genuine latitude remains.

LATENT
    determinate material/power exists but is not presently disclosed.

UNKNOWN
    the current system genuinely lacks the answer.

IRRELEVANT
    the resource exists but lies outside present Focus/horizon.

BOUND
    a real authority, trust, privacy, egress, or capability boundary prevents access.

MISSING
    expected material/power is absent; this is a defect or unmet dependency.
```

These should initially guide explanation, retrieval, diagnostics, composition and agent-facing language. They should become hard enums only where implementation evidence shows that typing produces clear value.

A missing required Contract/provider in a Component graph is `MISSING` or `BOUND` according to cause; an optional unselected Component may simply be `LATENT` or `IRRELEVANT`. The resolver should preserve the reason rather than flattening all unmounted state into "disabled".

The important capability is that an actor can distinguish **the meaning of its own non-knowing and non-availability**.

---

## 20. Retrieval over injection

The desired sequence is:

```text
INDEX
    what exists?

RESOLVE ELIGIBILITY
    what may this actor/project/target access?

ESTABLISH HORIZON
    what is addressable?

DISCOVER / ASK
    what does the actor know it can inquire about?

RETRIEVE
    what matters now?

LOAD / PROJECT / MOUNT
    what actually enters this client/context/body?

FOCUS
    what becomes salient to the present activity?
```

A broad horizon is a feature. Indiscriminate prompt inclusion or indiscriminate runtime mounting is not.

---
