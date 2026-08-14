# Part XII — Human and agent interfaces

## 42. CLI contract

The current principle remains:

> every substantive operation should have a structured machine-readable form.

`--json` remains a public interface with stable error codes.

The CLI is AIKit's operational language. It should expose the same semantic application services used by the TUI and agent surfaces rather than becoming a hidden implementation dependency of either.

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
Inspect relations around a resource.
Inspect an Action/Capability.
Invoke a permitted Action/Capability.
Explain an absence.
Inspect familiarity/history.
Inspect what changed.
```

Rich harnesses may receive multiple native Skills/Actions. Limited harnesses may receive one brokered AIKit faculty.

The agent path and human path share semantics but not necessarily ergonomics:

```text
agent
  search → inspect/explain → act → observe → history/changed ground

human
  choose Project → shape working world → inspect → stage → apply → navigate
```

---

# Part XIII — TUI V2

## 44. TUI product role

The TUI is not merely a renderer for a list of Skills and not a visual wrapper around CLI commands. It is AIKit's **human environment-composition and navigation instrument** over the resolved actor world.

A useful governing distinction is:

> **The CLI is AIKit's operational language. The TUI is AIKit's human environment-composition instrument.**

V2 may substantially rework the current TUI. Compatibility with an old screen arrangement is not a design goal where that arrangement obscures the V2 product.

The quality target is a fast, calm, terminal-native **context navigator and composer** rather than a dense dashboard or a sparse engineering palette.

The design doctrine is **simple, not basic**:

```text
basic
    few boxes, few controls, low visual ambition

simple
    few concepts visible at once
    clear hierarchy and location
    consistent navigation grammar
    progressive disclosure
    power nearby when requested
    nothing important unexplained
```

The TUI should preserve immediate entry/exit, low-latency search, terminal portability and accessibility while giving full depth to Project composition, knowledge navigation, projection and explanation.

### 44.1 Primary human object: the resolved Project world

For ordinary human use, the main object is not the implementation inventory. It is the world AIKit has resolved for the present Project/actor/Focus.

The TUI should make legible, at minimum:

```text
Project / local region
Profile + scopes
Agent / Agency
Focus
Capability horizon
Information horizon
Model / Harness / Host
Projection targets
current Generation / changed ground
important boundaries and warnings
```

The human should be able to answer: **what working world have I made, why is it this way, and what will change if I edit it?**

---

## 45. Core TUI capabilities and destinations

Search, Context, Compose, Explain and History are normative **semantic capabilities**. They need not become five equal tabs.

A coherent full-workspace shell may expose destinations equivalent to:

```text
Projects | Compose | Explore | Projection | History
```

while:

- **Context** remains ambient and persistent throughout the workspace;
- **Search** is universally summonable from anywhere;
- **Explain** is an inspector attached to the selected resource/state;
- the exact labels and geometry remain implementation UX, not constitutional nouns.

### 45.1 Quick surface and workspace surface

V2 should support two presentations of one TUI architecture:

```text
QUICK
    search + actions + recent/familiar routes
    small, immediate, transient where appropriate

WORKSPACE
    Projects + Compose + Explore + Projection + History
    fuller relational context and staging
```

They must share the same `TuiState`, selection, query, canonical `ResourceRef`, history/familiarity state, staged mutations and application service. Expanding Quick into Workspace preserves the actor's current selection and intent; it must not reconstruct a second controller and manually synchronise state.

### 45.2 Search / command capability

Universal low-latency discovery should span eligible addressable resources such as:

```text
Projects
Profiles
Skill Sets / Skills
Capabilities
Actions
Agents / Agencies
Context Sources
SemanticWiki Spaces / Nodes / Frames
SourcePool Sources
ProjectMap / code references
Models / Harnesses / Hosts
Procedures
recent / familiar destinations and Routes
```

Search should feel fzf-like: broad, fuzzy, immediate and composable with stable selection.

A selected resource should expose its contextual actions through the same application service. The core human grammar is:

```text
SEARCH
    find the thing

ACTION
    inspect, navigate, compose with, or operate on the thing

HISTORY
    make meaningful destinations and routes easier to recover
```

The UI should prefer a small stable interaction grammar plus searchable actions over dozens of memorised key chords. Conceptually useful bindings include `/` for find, Enter for open/inspect, an action summon key, Space for staging only where staging is meaningful, Esc for back/dismiss, `?` for controls/explanation and an explicit exit command. Exact bindings are an implementation contract, but ambiguous destructive overloading is not acceptable.

Zero-query Search may surface present-context destinations, recent/familiar routes, pinned/explicit preferences, changed resources and Project-relevant entries. Learned familiarity must never be presented as trust, authority or semantic recommendation merely because it is frequent.

### 45.3 Context capability

Context is the persistent human analogue of `ContextDisclosure`. It should expose the present resolved world without requiring navigation to a diagnostic screen.

The user should be able to inspect declared versus effective state, relevant scope/provenance, structured absence, disclosure state and important target effects from the current location.

### 45.4 Compose capability

Compose is likely the richest human surface. It should support reversible shaping of the Project world across distinct horizons:

```text
Capability horizon
    Profiles, Skill Sets, Skills, Capabilities, Actions, overrides

Information horizon
    ContextSources, SemanticWiki, SourcePools, selectors, retrieval policy

Actor/runtime world
    Agent / Agency, Model, Harness, Host where valid

Projection targets
    what is written/projected/brokered, where, and when it becomes effective
```

These horizons must remain distinct even when one composition workflow edits more than one.

Staged state is visibly separate from applied state. The TUI must explain the effect of a staged change before durable application, including restart/reprojection requirements and known external Procedures. Composition should preserve authored intent separately from the presently resolved binding.

A new-Project flow may naturally guide the human through Project → Profile → capability horizon → information horizon → actor/runtime → projection → preview/apply, but this should emerge from the composition model rather than requiring an arbitrary wizard ontology.

Skill Sets and Profiles should be craftable relational objects rather than opaque arrays. Declared inheritance/membership and effective resolution should remain distinguishable while being presented in readable human language.

### 45.5 Explain capability

For any selected item or resolved state, Explain should answer as applicable:

```text
why is this here?
why is it unavailable?
where did it come from?
what authority/source owns it?
what scope selected it?
what relation/path brought me here?
what did familiarity influence?
what would change if I staged this mutation?
what target effect will occur?
what is authored / observed / derived / learned / generated?
```

Explain is not a separate semantic engine. It is a read model over resolution, provider provenance, relation/path data, staging and history.

### 45.6 History / familiarity capability

History is larger than a list of recent commands. It should make inspectable where the actor has been, which destinations/routes have become familiar, what a Project world looked like previously, and what changed between meaningful Generations or compositions.

Useful views may include:

```text
Recent
Familiar
Changed
Previous worlds / Generations
recoverable prior compositions where valid
```

History remains evidence and navigation support, not authority. Frequency is not quality, trust or preference. Reset/forget of learned ease must not remove canonical resource relations or authored state.

---

## 46. Knowledge navigation and relation views

SemanticWiki, SourcePool, ProjectMap and other relation-bearing resources enter the same TUI state as the wider resource field. The TUI must not create a separate Wiki application with a private navigation model.

Search, graph and composition are three ways of encountering the same addressable world:

```text
SEARCH
    "I know roughly what I want."

RELATIONS / GRAPH
    "I know where I am; show me what is related."

COMPOSE
    "I know what world I want; let me shape the relations."

HISTORY
    "Help me recover the routes and worlds that mattered before."
```

### 46.1 Universal relation read model

AIKit should support a provider-neutral **read-model** abstraction equivalent to:

```text
RelationNode {
    ref
    kind
    label
    state?
    provenance_summary?
}

RelationEdge {
    from
    to
    relation
    direction
    provenance?
}

RelationView {
    focus
    nodes
    edges
    filters
    depth
}
```

These shapes are illustrative implementation contracts, not new constitutional resource types.

The critical law is:

> **Universal relation rendering must not become universal relation ownership.**

Provider/domain owners retain the meaning of their relations. A WikiEdge, GitNexus CALLS edge, source-provenance relation, Project membership and learned KnowledgeRoute do not become the same semantic thing because one renderer can display each of them.

### 46.2 List, tree and graph are projections, not application modes

The same relation/selection state should be renderable as:

```text
Relation read model
    ├─ LIST
    ├─ TREE
    └─ GRAPH
```

Switching presentation preserves canonical selection, query, expansion state where meaningful, staged changes and navigation context. Tree rendering may express genuine hierarchy; graph rendering may express neighbourhoods and cross-links. Neither may become a separate controller with a second copy of application state.

### 46.3 Leaf to local whole

Graph mode should be focus-centred rather than a miniature global graph browser.

A selected Node/Source/resource can expand from leaf to a manageable local whole containing its enclosing Space, immediate typed relations, provenance, cross-lens bindings, relevant Frames and optional known Routes. Enter/recentre should make the selected relation node the new focus. Depth and filters should be explicit and bounded.

This same pattern should support, without semantic collapse:

- Project → Profile / Skill Sets / ContextSources / projection targets;
- Skill Set → folders / Skills / Capabilities / dependencies;
- SemanticWiki Space → subspaces / Nodes / Frames / source provenance;
- SourcePool → collections / Sources / provider bindings;
- ProjectMap → Wiki / Source / code / design / Run lenses;
- projection world → Agent / Agency / Model / Harness / Host / generated target.

### 46.4 QL/MEF in knowledge views

QL may inform underlying Wiki topology and page/relation composition without requiring ordinary users to know QL terminology.

MEF and developed QL readings are explicit optional deeper views. They must retain provider identity, target identity and provenance, and must not silently mutate canonical relation state. The absence of a QL/MEF provider must not degrade ordinary relation navigation.

---

## 47. TUI state and interaction architecture

The TUI should have one authoritative UI state and reducer/effect path over shared application services.

Conceptually:

```text
Application services / read models
    ├─ Search
    ├─ Context disclosure
    ├─ Composition preview/apply
    ├─ Explain
    ├─ History/familiarity
    └─ Relations / Knowledge Navigation
             │
             ▼
          one TuiState
             │
      event → UiAction → reducer
             │
     ┌───────┼────────┐
     ▼       ▼        ▼
 keyboard   mouse   effects
             │
             ▼
          renderer
```

Required laws:

- no Palette-controller / Tree-controller semantic duplication or manual state synchronisation;
- stable canonical `ResourceRef` selection across view switches, refresh and Quick → Workspace expansion;
- re-resolution reconciles UI state by stable refs rather than reconstructing the experience around transient indexes;
- keyboard and mouse paths emit the same semantic `UiAction`s;
- renderer code does not duplicate resolver/provider semantics;
- the TUI does not shell out to the CLI for internal operations;
- durable mutations are staged, explained and explicitly applied;
- no navigation/back action may accidentally apply or discard staged work.

Esc/back behaviour should be predictable: dismiss the top transient surface or move back one navigation level. Query clear, staged-discard and application exit should be explicit rather than hidden behind context-dependent repeated Esc semantics.

### 47.1 Application-service/read-model boundary

The existing capsule-shaped TUI backend is a migration substrate, not the V2 semantic boundary. V2 TUI application services should accept/return resource-oriented refs, ContextResolution/ContextDisclosure, composition previews, target effects, explanations, history and relation read models.

The same services must remain callable from CLI/agent surfaces. The TUI owns interaction state and rendering, not a second semantic store.

### 47.2 Visual hierarchy and semantic theme

The default UI should favour whitespace, alignment, restrained dividers, consistent density and readable labels over cryptic glyph-heavy rows.

A row should prefer human-readable meaning equivalent to:

```text
Deploy API     Action     project · available
```

over compressed state sigils whose semantics require memorisation.

A semantic theme layer should distinguish roles equivalent to:

```text
surface
text
muted
accent
selected
positive
warning
danger
staged
relation
```

The default mapping may use portable terminal ANSI colours; richer 256/true-colour themes may be supported. Colour is never the sole carrier of state. ASCII/Unicode fallbacks and terminal-safe contrast remain requirements.

### 47.3 Host and responsive behaviour

The current honest host model remains valuable: inline rendering preserves scrollback where appropriate; fullscreen is explicit or justified by terminal/content size; tmux popup use depends on a real host primitive rather than emulation; other hosts must not pretend to support capabilities they do not have.

Responsive layouts should degrade by information priority and progressive disclosure, not by silently removing semantic state. Narrow terminals may move inspector/detail into a drill-in surface; wider terminals may show navigation and inspector concurrently.

### 47.4 Information hierarchy

The TUI should prioritise:

1. present location/context;
2. direct search/action;
3. selected resource meaning;
4. relation/provenance/explanation;
5. composition staging/target impact;
6. deeper metadata and optional QL/MEF views on demand.

The user should not need to understand AIKit internals or QL terminology to answer ordinary questions.

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

`aikit-tui` may contain pure UI reducers/read-model presentation logic, but domain resolution and provider relation semantics remain beneath the application-service boundary.

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

knowledge/
    ProjectMap / KnowledgeRoute / relation read models

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
