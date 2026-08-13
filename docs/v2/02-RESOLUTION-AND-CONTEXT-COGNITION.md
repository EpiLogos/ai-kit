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
Host relation
execution-world requirements/offers
projection policy
retrieval/disclosure policy
```

The exact serialized schema is a V2 implementation contract. The semantic separation is the important part.

---

## 13. Scope precedence

The current specificity model remains a strong foundation:

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

V2 should extend what can be resolved through this algebra without creating a parallel precedence system for personal sources, Actions, Agents, or QL relations.

A source may live outside AIKit while entering resolution at the appropriate scope.

Later/more-specific ordinary declarations may develop earlier ones. Managed denials and hard eligibility boundaries remain non-overridable through ordinary lower-scope preference.

---

## 14. Eligibility versus preference

AIKit must keep distinct:

```text
trust
availability
policy
platform compatibility
dependencies/conflicts
explicit authored preference
contextual relevance
fitness
frecency
```

Eligibility is determined by hard boundaries such as policy, trust, compatibility, and dependency satisfaction.

Preference and learned signals operate only among eligible possibilities unless an explicit interface is asking for diagnostics rather than selection.

A human declaration that a resource is preferred never makes it trusted or available.

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

These states are orthogonal to operational capability states such as available/enabled/projected/loaded/invoked.

A Skill may be enabled while not currently attended to. A source may be askable while none of its payload is loaded.

---

## 18. Operational capability states

V2 should preserve and generalise the current distinction between available, enabled, and loaded.

A fuller operational chain is:

```text
CATALOGUED / EXISTS
        ↓
ELIGIBLE / AVAILABLE
        ↓
ENABLED / RESOLVED
        ↓
PROJECTED / BROKERED
        ↓
LOADED / ACTIVE IN CLIENT
        ↓
INVOKED
```

The UI and API must not imply one state merely because another is true.

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

These should initially guide explanation, retrieval, diagnostics, and agent-facing language. They should become hard enums only where implementation evidence shows that typing produces clear value.

The important capability is that an actor can distinguish **the meaning of its own non-knowing**.

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

LOAD / PROJECT
    what actually enters this client/context?

FOCUS
    what becomes salient to the present activity?
```

A broad horizon is a feature. Indiscriminate prompt inclusion is not.

---
