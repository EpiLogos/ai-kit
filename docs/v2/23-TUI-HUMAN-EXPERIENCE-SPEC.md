# TUI Human Experience — World Composition, Direct Sessions and Factory Execution

**Status:** authored UX/design determination for implementation  
**Programme:** #28 / #211  
**Ground at authoring:** AIKit `main` `9e586aae7c4f92c2c561b1392601a7536331e3cb`  
**Read with:** `04-INTERFACES-TUI-AND-SOFTWARE-DESIGN.md`, `17-TUI-V2-DOMAIN-PARITY-AUDIT.md`, current Project/Praxis/SessionSpace/working-environment/gateway/Git contracts, and O:I founding positions.

This document determines the experienced terminal product over the V2 architecture already built. It does not replace native ownership, introduce a second resolver, or turn the TUI into a new semantic store.

Reinspect current `main` before implementation. Parallel Praxis/Method work may revise exact types while this human design remains authoritative at the experience level.

---

## 0. Product act

The TUI exists so a person can:

> **observe and compose Agent Worlds, then work inside them.**

The desired movement is ordinary and direct:

```text
I need to do X with an Agent
        ↓
express / select the Agent I want
        ↓
compose its governance, praxis, information and Worlds
        ↓
resolve the body / Harness / working environment
        ↓
preview the operative World
        ↓
enter a simple AgentSession
        or
commission durable developmental work through Factory
        ↓
observe what is actually happening
        ↓
return to Knowledge / History / Recognition
```

The human should not have to think in product namespaces merely to do this. Product boundaries remain visible when they explain ownership, authority, failure or provenance.

The experienced object is a **World made operative for an actor**: authored ground, Project relations, praxis, information, authority, runtime body, material environment and continuity composed for a particular act.

---

# 1. Two ways of working

The TUI must make two execution modalities immediately legible without inventing a shared false identity.

## 1.1 Direct Session

Direct Session is the ordinary case: the person wants to get into a Project with an Agent and work.

```text
Project
+ Agent / AgentProfile
+ optional Profile / praxis refinement
+ optional SessionSpace / current working scope
        ↓
AIKit resolves the operative World
        ↓
Harness / model / working-environment / Surface
        ↓
AgentSession
        ↓
material Harness process/service exists
        ↓
Workcell observes/registers it
```

No Factory Run or Journey is created merely because an AgentSession exists.

The chosen encounter may be:

```text
terminal inline/fullscreen
terminal tab/window
new tmux pane/window
new cmux workspace/pane
Herdr workspace/pane
provider-native Harness GUI
Gateway-backed conversation Surface
O:I desktop Surface
```

The host/provider may create or focus its own native objects. AIKit retains the semantic bindings and canonical refs it actually owns; Workcell retains the material observation.

## 1.2 Factory Work

Factory Work is used when the person asks the system to carry an intended developmental difference through a durable execution protocol.

```text
Commission / intended difference
        ↓
Journey / Run / Run Map / frontier
        ↓
Execution Intelligence
        ↓
ExecutionDisposition
  task + Context
  Agent / Agency
  praxis / Capabilities / Actions
  Harness / model / body
  SessionSpace / material demand where relevant
  execution shape + rationale
        ↓
AIKit + Actuation + native Harness + Workcell enact the arrangement
        ↓
Executions / Activity / Claims / Evidence / Candidates
        ↓
Return / Recognition / recursion
```

From the person's perspective, this may be as simple as:

> Run this work in Herdr with these Agents, using these bounds.

Factory provides the **developmental execution relation**. It does not become a generic Harness/runtime/process owner.

## 1.3 The coexistence law

The material machine may hold all of these simultaneously:

```text
Factory-managed execution A
Factory-managed execution B
ordinary direct Codex session
ordinary direct Claude session
manual Pi process
gateway-hosted persistent AgentSession
external/native Harness process
```

Workcell provides the truthful material census beneath them. Factory provenance is attached only where a Factory semantic execution actually exists.

This is an important acceptance condition because it proves the products compose around an existing technological World rather than requiring every process to enter through one workflow.

---

# 2. The shell

The full Workspace uses six human destinations:

```text
Worlds   Compose   Work   Knowledge   History   System
```

Three other faculties are ambient rather than equal destinations:

```text
Navigator / Search   summon from anywhere
Context              always locatable
Explain / Inspector  attached to selected subject/state
```

A selected stable subject is the centre of interaction. Its native/application Actions are available contextually.

## 2.1 Wide resting shell

```text
 AIKit · World: O:I · Project: O-I · Agent: maker · SessionSpace: dev
 Worlds   Compose   Work   Knowledge   History   System        Ctrl+K Search
──────────────────────────────────────────────────────────────────────────────

 World · O:I development

 Project       O-I
 Focus         TUI / human experience
 Profile       developer
 Agent         maker
 Praxis        4 sets · 31 operative · 2 withheld
 Information   8 visible sources · 2 changed
 Harness       codex
 Workspace     Herdr · workspace 3
 Material      Workcell local · live
 Git           main · clean · upstream current
 Work          1 direct Session · 1 Factory Run

 Attention     Factory Recognition waiting
 Boundary      one gateway Surface degraded

                                                     ┊ INSPECTOR
                                                     ┊ selected subject
                                                     ┊ owner / source
                                                     ┊ effective state
                                                     ┊ relations / why
                                                     ┊ current material binding
                                                     ┊
                                                     ┊ : Actions

──────────────────────────────────────────────────────────────────────────────
 World · 2 staged · 1 warning                  : Actions   Ctrl+K   ? Help
```

The example communicates layout and information hierarchy, not literal data availability requirements. Missing authoritative fields are omitted or explicitly unresolved rather than guessed.

## 2.2 Medium

At medium width the primary surface gets the body. Inspector becomes an explicit drill-in/sheet. No semantic capability disappears.

## 2.3 Narrow

At narrow width the TUI becomes one-column progressive disclosure:

```text
location/context
search/current field
selected list/body
Enter → details
: → Actions
```

Spatial Graph view degrades to a grouped relational reading while preserving focus/filter/depth state.

---

# 3. Universal Navigator

Quick becomes the **Universal Navigator** over the same `TuiState` and application service as Workspace.

## 3.1 Invocation

Preferred semantic bindings:

```text
Ctrl+K       Universal Navigator
/            filter/search current field
:            Actions for current selection
Enter        open / inspect / recenter
Esc          return/dismiss one level
Ctrl+T       List → Tree → Graph where available
Ctrl+S       preview / advance reviewed apply
?            contextual help
Ctrl+Q       explicit exit
```

The implementation may provide terminal-safe aliases, but the UI must not accumulate a large chord vocabulary.

## 3.2 Search scope

Universal Search can return:

```text
Worlds / Projects
Agents / Agencies / AgentSets
AgentProfiles / Profiles
SkillSets / Skills / Methods
Capabilities / Actions
ContextSources
Wiki Spaces / Nodes / Frames
Sources / ProjectMap / code refs
KnowledgeRoutes
Models / Harnesses / Hosts
SessionSpaces / AgentSessions
working-environment bindings
Factory Journeys / Runs / Candidates / Executions
Workcell material resources
Gateways / communication Surfaces
TUI navigation destinations
recent / familiar routes
```

Only resources/readings actually exposed by application/provider contracts appear.

## 3.3 Navigation destinations are addressable

A screen/panel destination should be discoverable through the same Search experience:

```text
Ctrl+K → "skills"

DESTINATIONS
  Compose / Praxis / Skills
  Compose / Praxis / SkillSets
  System / AIKit / Skill sources

RESOURCES
  verification           Method
  skill-authoring        Skill
  developer              SkillSet

RECENT ROUTES
  O:I → developer → verification
```

This does not require a second `Page` ontology. The host publishes navigation Surfaces/routes/contributions whose destination semantics remain presentation-owned while the resources beneath them keep native identity.

## 3.4 Local search

`/` inside a destination searches that field first. A clearly exposed `Everywhere` route promotes the same query to Universal Search.

The query language remains the shared Resolve/Search language. UI filters compile to shared request semantics rather than becoming another resolver.

---

# 4. Worlds

`Worlds` is the primary resting destination because the human question is not “what settings exist?” but:

> What World am I actually inhabiting, and which World do I want next?

## 4.1 World summary

A World summary may disclose:

```text
Project / root World relation
Focus / current Now relation where supplied
Profile + effective scopes
Agent / Agency
praxis summary
Information horizon
Model / Harness
working-environment provider
SessionSpace
material Workcell relation
Git state
Projection / generated target state
current direct Sessions
current Factory Journeys/Runs
warnings / unresolved boundaries
```

## 4.2 World actions

Primary actions should be few and legible:

```text
Continue
Start simple session
Start Factory work
Compose Agent / World
Explore graph
Open another Project / World
Repair setup/problem
```

## 4.3 World selection

Selecting another Project/World updates the ambient Context and available composition/work routes through canonical application resolution. It does not silently mutate a running Session or staged composition.

---

# 5. Compose — building an Agent around intention

The Compose experience is intention-first.

A person should be able to start from:

```text
I need a research Agent that understands this Project,
uses my verification practice,
can read these sources,
and works in a constrained local environment.
```

and progressively determine the actual World.

## 5.1 Human composition spine

```text
Intention
Identity / expression
Governance
Praxis
Information
Worlds & bounds
Runtime
Workspace / continuity
Preview
Enter work
```

Each step is a projection over real owner contracts.

### Intention

Natural language description of the Agent/work role or an existing authored expression.

### Identity / expression

Name/select stable Agent/AgentProfile source. New expressions land through the Central-owned human-authorship route.

### Governance

Inspect/select human-authored principles, constraints, authority and World access intention. Generated recommendations remain proposals until accepted where authorship changes.

### Praxis

Search/browse:

```text
Profile
SkillSets
nested SkillSets
Skills
Methods as the current accepted Skill classification
Capabilities / Actions where relevant
```

Display containment/participation and effective resolution separately from authored membership.

### Information

Compose eligible information without confusing selection with retrieval:

```text
ContextSources
Wiki / SourcePool
source/disclosure scope
selectors
retrieval policy
```

### Worlds & bounds

Choose Project/World relations, scopes and authored placement/access intention.

### Runtime

Choose or leave resolvable:

```text
Harness
model/provider preference or requirement
required modalities/contracts
body/components where the target exposes them
```

A missing authored preference is not an error.

### Workspace / continuity

Choose:

```text
working-environment preference
SessionSpace / persistent frame
Surface preference
host/material requirements where actually authored
```

### Preview

Preview answers:

```text
What Agent/World did this actually resolve to?
What is authored?
What is effective?
What is withheld/unavailable?
What provider/body will carry it?
What information is eligible vs retrieved?
What material/working environment is selected?
What will be generated or activated?
What needs restart/reprojection?
```

### Enter work

Two explicit actions:

```text
Start simple session
Start Factory work
```

No hidden promotion from one mode to the other.

---

# 6. The O:I Agent inside Compose

The top-level O:I/Central-root Agent is a first-class assistant to composition.

It may operate in three human-selected modes:

```text
DO WITH ME
  propose a composition and stage the Actions for review

TEACH ME
  explain the field and let the person make each choice

DO FOR ME
  resolve/stage routine choices within granted authority,
  stop at authored/authority/Recognition boundaries
```

These are interaction modes, not new agent ontologies.

The O:I Agent can:

- interpret the intention;
- search Skills/SkillSets/ContextSources/Agents/Worlds;
- compare candidate Harness/model/environment arrangements;
- explain provider/adapter/credential conditions;
- propose SessionSpace/working-environment structure;
- call owner-native read/diagnostic operations;
- stage owner-native Actions;
- return unresolved authorship/authority decisions to the person.

It cannot silently turn its generated interpretation into human-authored Central ground.

---

# 7. Work

`Work` makes active agency legible and startable.

It has two entry actions and one shared observation field.

## 7.1 Start simple session

Fast form:

```text
Project              [O-I]
Agent / Profile       [maker]
Praxis                [effective project defaults]
SessionSpace          [development / none]
Open in               [resolve / terminal / tmux / cmux / Herdr / native]

                     [ Preview ] [ Start ]
```

The form should aggressively use current context/defaults so common work may be two selections and Enter.

The preview shows the resolved body and any boundary that would prevent launch.

## 7.2 Start Factory work

Factory form begins from developmental meaning:

```text
Desired difference
Project / World
Agent / AgentSet or resolve suitable Agency
closure / evidence condition
bounds / authority
working-environment/material constraints if intentional
```

Factory resolves a real Journey/Run/ExecutionDisposition through its native/application contract.

The human then sees:

```text
why this work exists
frontier
who/what is carrying it
where it is running
what evidence/return is expected
what Attention/Recognition is pending
```

## 7.3 Active Work dashboard

```text
WORK

Direct Sessions
› maker · O-I · codex · active · tmux:4
  researcher · Essay · claude · waiting · native terminal

Factory
› TUI implementation · Run graph-renderer
    2 Agencies active · Herdr workspace 3
    frontier: spatial graph layout
    verification pending

Attention
  permission · codex/tool
  Recognition · Factory Candidate 12
```

The dashboard is a projection over stable native refs. A process row may be linked to Workcell evidence without becoming AgentSession identity.

## 7.4 Factory depth

Selecting Factory work offers three depths:

### Semantic

```text
Journey / Run
Run Map / frontier
Claims / Evidence
Candidate(s)
Recognition / Return
```

### Live

```text
Agency / participating Agents
ExecutionDisposition
Executions
AgentSessions
SessionSpace
working-environment Surfaces
Harness/body
Workcell/material relations
```

### Trajectory

```text
ordered spans/events
model/harness events
capability/tool invocations
process events
permissions
errors/retries
artifact/evidence emissions
native trace refs
```

Trajectory is drill-down. The Run's developmental meaning remains primary.

---

# 8. Knowledge

Knowledge is the deep navigational surface over Wiki, SourcePool, ProjectMap, code, routes, praxis and other relation-bearing resources.

The relation state supports:

```text
List   Tree   Graph
```

without changing selected canonical identity.

## 8.1 List

Use List for scanning typed related subjects with readable relation/provenance summaries.

## 8.2 Tree

Tree is used only where the source relation is genuinely hierarchical or contains a meaningful nested decomposition.

Examples:

```text
nested SkillSets
Wiki Spaces/subspaces
World/Project containment
source collections
genuine component/provider containment
```

The old compatibility tree renderer may contribute pure layout/glyph techniques; its controller/state model must not return.

## 8.3 Spatial Graph

The production Graph is a deterministic local topology renderer.

### Layout

Default orientation:

```text
                    contextual / enclosing
                            ▲
                            │

incoming relations  ←   [ FOCUS ]   →  outgoing relations

                            │
                            ▼
                    contained / nested
```

Relation families may be grouped into lanes/clusters while retaining actual typed labels/origins.

### Interaction

```text
arrow / hjkl     move selected visible node where sensible
Enter            recenter selected node
Esc / Back       previous graph focus
+ / -            bounded depth
/                filter current graph
:                Actions for selected subject
Ctrl+T           List / Tree / Graph
Ctrl+K           Universal Navigator
```

Mouse hit-testing, where enabled, emits the same semantic selection/recenter actions.

### State

Graph presentation state may include:

```text
focus Ref
selected visible Ref
viewport / pan
bounded depth
relation/lens filters
collapsed relation groups
```

Canonical resource selection remains shared `TuiState`; Graph does not get a second semantic store.

### Stability

Layout recomputes only when the relation graph, filters, depth or viewport meaningfully changes. Cursor movement does not cause a physics simulation.

### Inspector

A selected edge/node can explain:

```text
relation type / direction
provider / owner
source authority
revision / provenance
learned route vs canonical edge
optional QL/MEF overlay origin
```

### Narrow fallback

When there is insufficient geometry:

```text
FOCUS

Incoming
  grounded-by → X
  source-of   → Y

Outgoing
  contains    → Z
  related-to  → A

Context
  Project O:I
```

The current Graph mode remains active semantically; only its projection changes.

---

# 9. Open elsewhere

Many terminal operations should support opening the same semantic subject in another real Surface.

Representative Actions:

```text
Open here
Open in new tmux pane/window
Open in cmux workspace/pane
Open in Herdr workspace/pane
Open native Harness Surface
Open O:I desktop Surface
```

Availability depends on actual WorkingEnvironmentProvider/Surface contracts.

The TUI must not pretend cmux has a primitive it lacks merely because tmux has one. Provider-native workspace/pane IDs remain bindings/provenance.

Opening Graph in a second pane preserves the focus Ref and request/filter payload where the application/host can carry it.

---

# 10. Git and Project responsibility

Git is part of the Project ecology, not a parallel TUI application.

For a Git-backed Project, ambient World/Work disclosure should show a bounded responsibility strip such as:

```text
Git  main @ 7bc… · clean · upstream current
```

or:

```text
Git  feature/x @ 31d… · dirty 3 · ahead 2 · no conflicts
```

Drill-in provides:

```text
repository / worktree relation
HEAD / branch / detached
staged / unstaged / untracked
conflicts
upstream/ahead/behind
bounded log/diff
linked worktrees
Factory Run/Candidate base relation where supplied
```

Factory worktree use remains exact-base developmental evidence; worktree/branch never replaces Run/Candidate/Journey identity.

Direct native Git CLI stays first-class. Structured Actions exist where UI/Factory/authority/recovery benefits from them, followed by reconciliation of actual state.

---

# 11. System

System shows the machinery that makes the World operable and gives clear routes to fix it.

## 11.1 Primary groups

```text
Products
Providers / adapters / SDK
Credentials
Models
Harnesses
Working environments
SessionSpaces / Surfaces
Gateways / communication
Workcell
Diagnostics / Repair
Setup / Verify
```

The six-product capability catalogue is discovery/provenance input, not a capability dashboard.

## 11.2 Product overview

```text
Central             healthy     ground bound
Actuation           healthy     Agency surface ready
AIKit               warning     one credential missing
Software Factory    healthy
Workcell             healthy     harness instances observed
Quaternal Logic      available   optional provider
```

Status words must be derived from real owner readings/verification rather than invented aggregation.

## 11.3 Credentials

Credential UX promotes the already-defined secure-store/provider semantics.

Example:

```text
OpenAI
  required by      codex / provider route
  credential       missing
  native store     macOS Keychain available
  env import       available only by explicit choice

  [Configure]
```

Secrets are never Search/history material and previews remain redacted.

## 11.4 Providers / adapters / SDK

The user should be able to inspect:

```text
what provider/adapter handles this technology?
which native version was detected?
what conformance does the adapter claim/prove?
what capabilities/Surfaces are actually exposed?
why is this adapter unavailable/degraded?
which native or SDK path would add support?
```

This is particularly important for heterogeneous Harnesses/working environments.

## 11.5 Working environments

Show tmux/cmux/Herdr and other providers with truthful native capabilities:

```text
tmux     available · workspace/pane control
cmux     available · actual supported primitives
Herdr    available · workspaces/panes + native Agent automation
```

Do not collapse Herdr to a pure mux or pretend pure muxes own AgentSession protocol.

## 11.6 Workcell

System can show:

```text
material Worlds
Harness instances
process/service evidence
live/stale/declared status
OpenSandbox/VM placement where used
network/service bindings
gateway services
```

A Workcell instance may be correlated to Direct Session, Factory Execution or neither.

---

# 12. Gateway / communication / chat

A gateway is a material/provider arrangement that may expose communication Surfaces. It is not a universal Agent protocol or identity.

## 12.1 Surface relation

```text
Agent / Agency
     ↓
Harness / HarnessComposition / AgentSession
     ├─ terminal Surface
     ├─ TUI conversation Surface
     ├─ GUI conversation Surface
     ├─ messaging Surface
     ├─ API/webhook Surface
     └─ provider-native gateway Surface
                 ↓
          Workcell service binding
```

## 12.2 TUI gateway page

Expose:

```text
provider / adapter
service/material status
credential/auth state
bound AgentSession / Surface refs
native endpoint provenance where disclosable
reconstructability
health/degradation
Actions: start / stop / reconcile / rebind / open Surface / explain
```

## 12.3 Chat passthrough

Where the provider exposes a real conversation Surface, the TUI may host that Surface as a terminal conversation view.

It must preserve:

```text
same AgentSession identity
same canonical Actions/authority
same activity/result relations
same provider/native transcript refs
```

It must not create a new TUI chat-history truth.

The O:I/Central-root Agent may use the same conversation Surface architecture.

---

# 13. Diagnostics and self-repair

When “the session won't start”, the product should identify the failed relation rather than merely print a subprocess error.

## 13.1 Resolution/failure chain

```text
subject/source exists?
       ↓
eligible in current World?
       ↓
selected/resolved?
       ↓
provider/adapter available?
       ↓
credential/authority valid?
       ↓
Harness/body compatible?
       ↓
working environment available?
       ↓
gateway/communication Surface available if required?
       ↓
Workcell material instance/service available?
       ↓
activation/start successful?
```

The first actual failing relation becomes the principal explanation. Neighbouring degraded facts remain visible as supporting context.

## 13.2 Repair

Safe owner-native repair Actions may include:

```text
refresh discovery
reconcile product registration
re-probe provider
rebind credential
re-resolve profile/body
reconstruct SessionSpace
reconcile working environment
scan/reconcile Workcell instances
refresh/rebind gateway service
retry target activation
```

O:I Agent may diagnose and stage/execute these within authority.

No “repair” button may become a generic shell/filesystem/process/network escape hatch.

---

# 14. Forms

The existing `ArgSpec` contract is the base form language.

Supported field classes remain:

```text
string
path
integer
float
boolean
enum
multiselect
duration
secret
key/value
```

Add one V2-needed semantic field shape:

```text
ResourceRef / Resolve picker
```

with:

```text
kind/lens constraints
single or multiple selection
universal Search/Navigator UI
canonical Ref return
provider/eligibility explanation
```

This allows forms to choose Agents, Harnesses, SkillSets, ContextSources, SessionSpaces, Worlds, Factory subjects, etc. without static target lists or another resolver.

Wide forms show Effect/Explain in the Inspector. Narrow forms drill into effect before confirmation.

---

# 15. Setup

Setup is a guided reading/action flow over actual current state.

It must support both fresh and existing Worlds.

## 15.1 Setup route

```text
Discover existing World
        ↓
Ground / Projects
        ↓
Products installed / registered
        ↓
Credentials / providers
        ↓
Gateway / communication
        ↓
Working environment
        ↓
Workcell material observation
        ↓
AgentProfile / praxis
        ↓
Direct-session smoke
        ↓
Factory-work smoke
        ↓
Verify
```

## 15.2 Step states

```text
complete
needs input
optional
unavailable
conflicted
```

A step's state is reconstructed from the native/current owner facts. There is no separate durable wizard progress store merely to say “step 4 complete”.

## 15.3 Existing-world UX

If tmux, Herdr, Claude, Codex, Pi, local models, gateways or existing Projects are already present, Setup first discovers and explains them.

The person may adopt/register/bind what already exists where supported rather than reinstalling it merely to satisfy O:I.

---

# 16. History

History is the return surface for navigation, worlds and work.

Views may include:

```text
Recent
Familiar
Changed
World / Generation history
SessionSpace history
Factory Journey / Run development
Knowledge routes / frames
Git history/difference
Workcell material observations where relevant
```

These remain heterogeneous readings linked by refs, not flattened into one universal event database.

Frequency/familiarity remains learned evidence, not trust, preference or semantic truth.

Recovery Actions are offered only where the native/application owner has an actual recover/reconcile operation.

---

# 17. Explain / Inspector

Explain is not a top-level destination in the final IA.

Everything selected can have an Inspector depth answering as applicable:

```text
what is this?
who owns it?
why is it here?
where did it come from?
what is authored vs observed vs derived vs learned vs generated?
why is it unavailable?
what selected it?
what contains/participates in it?
what body/material Surface carries it?
what would this staged change do?
what can I do to it?
```

Provider-specific Explain Actions remain available under `:`.

---

# 18. Visual language

The current terminal visual doctrine is preserved and completed.

## 18.1 Rules

- one outer frame where useful;
- no nested-box proliferation;
- whitespace, alignment and rhythm carry hierarchy;
- human-readable labels over cryptic sigils;
- ANSI/user-terminal colour basis;
- selected row uses reverse video;
- states carry words/glyphs in addition to colour;
- ASCII and Unicode are complete peer fallbacks;
- no Nerd Font dependency;
- restrained motion only for real activity/progress;
- no fake loading/health.

## 18.2 Where dashboards belong

Dashboards are appropriate for:

```text
Worlds
Work
System
```

because these summarize whole current conditions.

They are not the default for:

```text
Compose
Skills / Praxis
Knowledge
forms
History detail
```

which need browsers, relations, editors and timelines.

---

# 19. Desktop propagation

The terminal and Cradle are two hosts over one semantic field, not two product implementations that must be kept manually in sync.

## 19.1 Shared meaning

They should share:

```text
canonical subjects/refs
Search/Resolve semantics
Agent/World composition
Direct Session vs Factory Work distinction
AgentSession / SessionSpace identity
Factory Journey/Run/Candidate/Execution relations
Knowledge relation models
Gateway/communication Surface identity
credential/provider/adapter/material readings
canonical Actions / authority / receipts
```

## 19.2 Different host geometry

```text
TUI
  quick/fullscreen/popup
  terminal panes / working-environment providers
  Inspector drill-in

Desktop
  Canvas / tabs / windows
  Auxiliary / Inspector / Lower / System
  detachable graphical Surfaces
```

A TUI Graph and desktop Wiki graph can have different rendering engines while consuming equivalent typed relations and focus refs.

A TUI Factory trajectory and desktop SSSF-derived Factory Build can have different visual density while addressing the same Run/Execution/Candidate semantics.

---

# 20. Cross-product development consequences

## Central

The TUI depends on stable authored Agent expression/Profile/governance operations. The Compose assistant must preserve human-authorship/Recognition boundaries.

## Actuation

The TUI needs semantic Activity/Attention/authority/Return readings around Direct and Factory enactment but does not acquire Actuation ownership.

## AIKit

AIKit owns this TUI host, Universal Navigator, effective World resolution, Praxis/Information/runtime composition, SessionSpace, working-environment bindings, Knowledge presentation, forms/pickers, gateway cross-layer binding and Git Project ecology.

## Factory

Factory gains a clearer product placement: it is the developmental execution protocol used when a desired difference is commissioned. It consumes the composed Agent/Profile/Praxis by stable refs. Direct AgentSessions remain outside Factory unless explicitly promoted/commissioned.

The terminal Factory view should consume the same native Build/Journey/Run/ExecutionDisposition/Candidate read models/Actions as the graphical Build surface.

## Workcell

Workcell is the material truth floor for all Harness/process/service instances, not only Factory-managed ones. This makes mixed local operation a primary physical acceptance case.

## O:I

O:I `oi` remains the suite/operator front door and mostly transparent product command federation. The TUI should not create a new semantic command layer simply because it hosts six-product System views.

## QL

Optional QL/MEF readings may overlay Knowledge/World relations while ordinary UX remains complete without them.

---

# 21. Acceptance journeys

The final TUI must be accepted by journeys, not screen presence.

## Journey A — fast ordinary work

```text
open TUI
→ select Project
→ select existing AgentProfile
→ preview effective World
→ start simple Session in current terminal
→ use Agent
→ inspect Context / Git / Knowledge
→ close Surface
→ re-open SessionSpace/session where continuity permits
```

No Factory Journey/Run is created.

## Journey B — compose a new Agent

```text
Compose
→ “I need X”
→ create/select Agent expression
→ governance
→ SkillSets/Skills/Methods
→ ContextSources / Worlds
→ Harness/body/workspace
→ preview
→ ask O:I Agent why two things are withheld
→ accept/fix composition
→ start simple Session
```

## Journey C — Factory multi-Agent work

```text
Start Factory work
→ Commission intended difference
→ Project + AgentSet
→ resolve ExecutionDisposition
→ Herdr/tmux/cmux working arrangement
→ several AgentSessions become live
→ observe Semantic / Live / Trajectory views
→ receive Evidence/Candidate
→ human Recognition
```

## Journey D — mixed local World

```text
Factory Run active in Herdr
+ direct Codex in tmux
+ external/manual Claude process
+ gateway persistent session
        ↓
Workcell instance scan
        ↓
all material instances visible
        ↓
Factory correlation only on the Factory execution
        ↓
Direct/external sessions remain first-class and non-Factory
```

## Journey E — failure and repair

```text
start Agent
→ resolution fails
→ Explain identifies missing credential or degraded adapter
→ System opens exact provider/credential row
→ O:I Agent explains and proposes repair
→ owner-native Action fixes/reconciles
→ re-resolve
→ Session starts
```

## Journey F — Graph

```text
Search Wiki node
→ open Knowledge
→ Graph
→ local typed neighbourhood
→ inspect Wiki↔Source↔Code origins
→ recenter
→ filter
→ open same graph focus in new tmux/cmux Surface
→ canonical focus Ref preserved
```

## Journey G — Git responsibility

```text
open Git-backed Project
→ see branch/worktree/dirty state
→ direct Session uses current worktree
→ Factory work creates/binds isolated exact-base Candidate worktree where supported
→ both state relations remain distinct
→ external Git change
→ reconcile
→ UI shows actual resulting state
```

## Journey H — Gateway/chat parity

```text
open persistent Agent from gateway
→ TUI conversation Surface
→ same AgentSession opens through another supported Surface
→ gateway service restarts/rebinds
→ Agent/Session continuity changes only according to provider evidence
→ Workcell reflects actual service/material transition
```

---

# 22. Physical/local proving matrix

After deterministic implementation and exact-main conformance, run on the owner workstation:

1. direct native/external Harness Session;
2. direct mux Session;
3. SessionSpace close/reopen/reconstruction;
4. Factory multi-Agent execution in Herdr/mux;
5. Factory + unrelated direct Harness + external Harness simultaneously;
6. Workcell instance census/correlation across all of them;
7. gateway conversation plus another Surface over one AgentSession where supported;
8. missing credential setup to successful launch;
9. adapter/provider failure to safe repair/reconciliation;
10. Git current-worktree plus Factory isolated-worktree case;
11. Graph current pane plus new pane/window case;
12. real wide/medium/narrow terminal review;
13. terminal restore/panic/resize safety;
14. desktop semantic parity spot-check against the same owner refs/actions.

Hosted CI proves deterministic contracts. It does not substitute for real provider/terminal/human evidence.

---

# 23. Closure

This specification is realised when the TUI feels like one small, fast and deep O:I working environment:

- World is the primary human object;
- Universal Navigator reaches both things and places without a second search ontology;
- Compose begins from Agent intention and ends in a previewed operative World;
- O:I Agent can compose with/for/alongside the human without taking authorship;
- Direct Session is genuinely simple and carries no unwanted Factory ceremony;
- Factory Work turns a commissioned difference into durable developmental execution through current native owners;
- both modes coexist truthfully on one material machine;
- Workcell sees the actual Harness/process/service field beneath both;
- Knowledge Graph is genuinely spatial, bounded and navigable;
- Git is legible as part of Project responsibility;
- credentials/providers/adapters/gateways/working environments/material state are diagnosable in System;
- safe self-repair uses canonical owner Actions rather than a generic shell escape;
- History/Explain make provenance, change and recovery intelligible;
- terminal and desktop share semantic refs/read models/Actions without forced layout parity;
- the result is simple because the relations are well composed, not because their depth has been removed.
