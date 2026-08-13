# Part XII — Human and agent interfaces

## 42. CLI contract

The current principle remains:

> every substantive operation should have a structured machine-readable form.

`--json` remains a public interface with stable error codes.

V2 should converge on coherent resource-oriented operations such as:

```text
aikit search
aikit context
aikit capabilities
aikit actions
aikit sources
aikit agents
aikit profiles
aikit project
aikit model
aikit harness
aikit history
aikit explain
aikit procedure
```

Exact naming remains subject to implementation UX.

Common verbs should trend toward:

```text
search
list
show/open
read
relations
history
explain
use/run
```

---

## 43. Agent-facing API

Agents should use the same semantic application service as humans.

The machine surface must support questions equivalent to:

```text
Who/where am I?
What world is resolved?
What exists?
What can I ask about?
What can I do?
Search resources.
Read a resource.
Search Context Sources.
Read source material.
Inspect an Action/Capability.
Invoke a permitted Action/Capability.
Explain an absence.
Inspect familiarity/history.
Inspect what changed.
```

Rich harnesses may receive multiple native Skills/Actions. Limited harnesses may receive one brokered AIKit faculty.

---

# Part XIII — TUI V2

## 44. TUI product role

The TUI is not merely a renderer for a list of Skills. It is the human portal into the resolved actor environment.

V2 may substantially rework the current TUI.

The quality target is a fast, calm, terminal-native **context navigator and composer** rather than a dense full-screen control dashboard.

The TUI should preserve immediate entry/exit and low-latency search while making the fuller resource world legible.

---

## 45. Core TUI surfaces

The UI should be built around a small number of coherent surfaces sharing one state model.

### 45.1 Search / Command surface

Universal fuzzy/search surface over eligible resource types:

```text
Capabilities
Actions
Skills
Agents/Agencies
Context Sources
Projects
Profiles
Models/Harnesses
Procedures
recent/familiar routes
```

Results should make resource type, scope, source, and availability immediately legible.

### 45.2 Context surface

A compact explanation of the present resolved world:

```text
Project / local region
Agent / Agency
Profile + scopes
Focus
model/harness/host
CapabilitySet / ActionSet
source horizon
current Generation
important boundaries/warnings
```

This is the human analogue of `ContextDisclosure`.

### 45.3 Compose surface

Allow reversible composition of:

- Profile membership;
- Capability/Skill Set changes;
- session/task overrides;
- actor/Agency selection where valid;
- source selectors;
- model/harness choices where valid.

Staging is visibly distinct from applying.

### 45.4 Explain surface

For any selected item or resolved state, answer:

```text
why is this here?
why is it unavailable?
where did it come from?
what scope selected it?
what would change if I toggled it?
what target effect will occur?
```

### 45.5 History / familiarity surface

Show recent and familiar routes without confusing use frequency with quality or trust.

---

## 46. TUI interaction model

A recommended geometry is:

```text
┌──────────────────────────────────────────────────────────┐
│ Project / Agency / Focus breadcrumb      host · target   │
├──────────────────────────────┬───────────────────────────┤
│ search / resource list       │ selected item / explain   │
│                              │ provenance / availability  │
│                              │ scope / target effect      │
├──────────────────────────────┴───────────────────────────┤
│ staged changes · contextual keys · warnings              │
└──────────────────────────────────────────────────────────┘
```

This is a design direction, not a pixel contract.

Keyboard and mouse paths must converge on the same reducer/application actions. The TUI must not duplicate resolver semantics or shell out to the CLI internally.

Search should remain instant enough to feel like an extension of thought.

---

## 47. TUI information hierarchy

The TUI should prioritise:

1. present location/context;
2. direct search/action;
3. selected resource meaning;
4. scope/provenance/explanation;
5. staging/impact;
6. deeper metadata on demand.

The user should not need to understand AIKit internals to answer ordinary questions.

QL/MEF terminology should not be required for ordinary AIKit use.

---

# Part XIV — Internal software architecture

## 48. Existing crate separation

The current five-crate separation remains a good baseline:

```text
aikit-core
    pure domain/resolver/search/projection contracts

aikit-store
    filesystem/SQLite/generations/trust/events/locks

aikit-adapters
    harness/mux/shell/provider adapters

aikit-tui
    UI controller/rendering only

aikit-cli
    application service + CLI + hook dispatcher
```

V2 should fit the expanded product into these boundaries before multiplying crates.

---

## 49. Likely V2 domain modules

Illustrative internal modules include:

```text
resource/
    typed resource descriptors/provider contracts

context/
    extended ContextResolution + disclosure relations

source/
    ContextSource descriptors and retrieval providers

actor/
    Agent/Agency imported resource views

action/
    Action index + ActionSet

execution/
    Model/Harness/Host/Workcell resource views

memory/
    UsageSignal/Frecency/Fitness

bootstrap/
    agentic seed + target bootstrap contracts

privacy/
    visibility and egress eligibility

project/
    ProjectBinding + Project resource declarations

ql/
    passive QL interop + optional provider adapter
```

These names are not constitutional nouns and need not each become a crate.

---

## 50. Core determinism

The resolution core must remain deterministic for identical canonical inputs, observed resource revisions, and explicit runtime conditions.

Learned signals may influence ranking/selection only through explicit stable inputs to the resolver.

A model call or semantic LLM inference must not occur inside the deterministic core resolver merely to decide ordinary capability eligibility.

Semantic composition may occur in a higher agent/Skill layer when appropriate.

---
