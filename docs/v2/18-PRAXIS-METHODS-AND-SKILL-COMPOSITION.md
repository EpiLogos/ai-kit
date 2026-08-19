# AIKit V2 — Praxis, Methods, and Skill Composition

**Status:** V2 design clarification  
**Date:** 2026-08-19  
**Companion to:** `01-PRODUCT-AND-OWNERSHIP.md`, `02-RESOLUTION-AND-CONTEXT-COGNITION.md`, `SPEC-III-SKILLSETS-AND-FRECENCY.md`  
**Coordinates with:** O:I authored position on praxis, Central/ProjectCentral source contracts, Factory praxis primitives

---

## 0. Product consequence

AIKit already defines itself as the actor's faculty of context cognition: the system which makes the available world of powers and information discoverable, resolvable, explainable and projectable.

Skills are therefore not an incidental plugin format. They are one important form in which intelligent praxis becomes available to an actor.

The missing distinction is the relation between reusable praxis and situated use.

AIKit adopts the following operational grammar:

```text
Guidance
    standing orientation / relation

Skill
    reusable organised intelligent praxis

UsageOverlay
    scoped addition to how an unchanged Skill is used here

Method
    contextual composition of Skills + Actions + ContextSources + overlays
    around a purpose / Focus

SkillSet
    additive repertoire selected for projection

Profile / ContextResolution
    resolution of what becomes operative here
```

The existing SkillSet law remains intact:

```text
profile : resolution :: skill-set : projection
```

`Method` is not a replacement for either side. It answers a different question:

> **How should the available praxis and resources be related for this kind of act in this context?**

---

## 1. Skill remains atomic reusable praxis

AIKit's existing Skill definition remains valid:

> a reusable body of organised intelligent praxis.

A Skill should normally have a coherent reusable purpose and a bounded common path. It may expose compressed routing language while larger procedural detail remains progressively disclosed.

A Skill may call or route toward:

```text
Actions
ContextSources
scripts/tools
other capabilities
other Skills where useful
```

but its source remains independently owned and addressable.

A Project should not have to fork a good reusable Skill merely to say how that Skill should be used in this Project.

---

## 2. UsageOverlay is the atomic adaptation mechanism

AIKit already has the important feature of appending use-case-specific guidance to a Skill at runtime without modifying the Skill source.

That mechanism should be understood explicitly as `UsageOverlay` semantics.

A UsageOverlay may be scoped to a relation such as:

```text
personal
Project
Profile
session
Run/task
Focus
```

It may add, for example:

- Project vocabulary which the Skill should preserve;
- a Project ContextSource to consult;
- a local verification command or ActionRef;
- a required return shape;
- a constraint about which part of the Skill matters here;
- a relation to another Skill or Method.

The invariant is:

```text
UsageOverlay != Skill source mutation
```

Overlay identity/provenance should be inspectable enough that later evidence can answer what adaptation was actually active.

---

## 3. Method is situated praxis composition

A Method is an addressable relation rather than a copied mega-Skill.

A Method may compose:

```text
SkillRefs
UsageOverlay refs / inline bounded overlays
ActionRefs / CapabilityRefs
ContextSourceRefs
ordering / conditional relations where the method genuinely requires them
expected intermediate and return forms
verification / recognition expectations
```

A Method may:

- compose several atomic Skills;
- point one Skill toward another;
- specialise the use of a Skill for a Project;
- establish a workflow relation around several independently owned Skills;
- bind the same reusable Skill to different Project-specific ContextSources or verification practices;
- encode a Project practice without copying the underlying external or first-party Skills.

Method is therefore the main operational seam between:

```text
praxis out of context
        ↓
praxis in context
```

Method source may be personal, Project-local, product-owned, observed/adopted or externally sourced. AIKit owns its operational indexing/resolution/explanation, not universal authorship.

---

## 4. SkillSet stays deliberately simple

Spec III remains normative:

> A SkillSet is a folder/additive set of capabilities for projection. It has no trust of its own and composes by union.

Do not turn SkillSet into Method.

A SkillSet answers:

> **What repertoire do I hand to this harness?**

A Method answers:

> **How should relevant members of the available repertoire and wider resource field be related for this act?**

A Profile answers:

> **What should resolve as active/eligible here under these scopes?**

Keeping these three questions separate avoids rebuilding the resolver inside set composition or hiding procedural semantics inside projection membership.

---

## 5. Central / ProjectCentral source shape

When Central is present, the intended visible authored source relation is:

```text
Control/
  user/
  agents/
    governance/
    wiki/
  skills/
  methods/

Work/<project>/ProjectCentral/
  user/
  agents/
    governance/
    wiki/
  skills/
  methods/
```

Semantic placement matters:

- `governance/` is standing human-authored Agent relation;
- `wiki/` is Agent-maintained project knowledge;
- `skills/` is reusable praxis source;
- `methods/` is contextual praxis composition;
- Project Ground remains distinct from all of them.

`skills/` and `methods/` should be siblings of `agents/`, not children of it, because they are part of the shared human↔Agent operative seam rather than Agent-owned knowledge.

AIKit should discover these as ordinary source relations while preserving source ownership and provenance.

Existing native Skill/Method-like directories are not invalidated. On bootstrap they may be:

```text
retained and related in place
adopted/rebounded with provenance
centralised explicitly by user choice
left native and only indexed
```

Generated harness directories remain projection destinations, not conceptual source.

---

## 6. Personal → Project praxis composition

The expected composition is:

```text
personal Ground
personal governance
personal Skills / Methods
        ↓
      Project
        ↓
Project Ground / ontology / governance
Project Skills / Methods
        ↓
Profile + SkillSets + ContextResolution
        ↓
Run / session / Focus overlays
```

`Control/skills` and `Control/methods` form a personal cross-Project praxis library. Presence there does not mean every item is projected into every harness.

User Baseline Profile, named SkillSets and normal resolution decide the default personal repertoire.

`ProjectCentral/skills` and `ProjectCentral/methods` provide the Project-specific specialisation layer. A Project may default its local SkillSet/Methods by convention while still applying ordinary per-member trust/eligibility gates.

---

## 7. Bootstrap should be an Agent-assisted praxis-development process

A median O:I installation should be able to establish a coherent Project world from intent-level human authorship rather than requiring the human to hand-configure ontology, governance and skill wiring.

The bootstrap Method should be able to follow a path equivalent to:

```text
existing Project / fresh Project
+ personal Central Ground / praxis where present
        ↓
recover existing authored intent, docs, code, history, native Skills
        ↓
Wayfinder / product-understanding
        ↓
recover Project vocabulary and ontology
        ↓
establish / update ProjectCentral
        ↓
bind Agent Wiki / ProjectMap / ContextSources
        ↓
derive capability matrix / gaps
        ↓
identify standing governance
        ↓
identify reusable Skills
        ↓
compose Project Methods
        ↓
select Project SkillSets / Profile relation
        ↓
preview ContextResolution + harness projection
        ↓
human Recognition only for genuinely consequential authored choices
```

The human should be asked primarily for things only the human can settle:

```text
What is this?
Why does it matter?
What should it become?
What must be preserved?
Which genuine alternative do you choose?
```

Existing source, implementation evidence and reversible engineering judgement should be exhausted before creating routine human interrogation.

---

## 8. AIKit as faculty; projected Skills as target-native affordance

The architectural relation should be explicit:

> **AIKit is the actor's faculty. Projected Skills are target-native affordances through which parts of that faculty become cognitively available inside a particular harness.**

A generation/bootstrap should be able to project:

```text
thin AIKit context/operator seed
selected SkillSet roots
Project/personal Methods or Method affordances germane to the context
target-native Skill projections
compact governance/orientation
ContextSource horizon pointers
stable Project / Agent / Agency / Focus refs
```

The thin seed should make clear that the projected Skills do not exhaust the available world.

An Agent should be able to use direct AIKit operations to discover the wider latent field, including Search, Context, Explain, History, Knowledge Navigation, resource inspection and eligible composition.

Projected Skills are therefore the low-friction habitual surface; direct AIKit remains the wider addressable faculty.

First-party Skills should increasingly route through canonical AIKit Actions/application services rather than implementing parallel hidden semantics.

---

## 9. First-party median praxis foundation

AIKit should offer, but not force, a coherent first-party praxis system comparable in architectural completeness to strong external Skill ecosystems.

The median installation is an optional developed working order between a minimal AIKit integration and a maximal O:I/Factory world.

The first-party foundation should intentionally cover the developmental field, approximately:

```text
#0 GROUND
Wayfinder · source/provenance navigation · research · existing-world recovery

#1 INTENT
human-authored vision/docs · product understanding · project language/ontology
governance extraction · experience articulation

#2 DESIGN
architecture/program/domain methods · capability matrices
Skill / Method / SkillSet composition

#3 DEVELOPMENT
coding · code intelligence · implementation craft · desktop/UI craft
multi-Agent working methods where appropriate

#4 APPLICATION
verification · review · browser/user experience · code/prose quality disciplines

#5 RETURN
Claims / Evidence / Explain / History · Wiki return
praxis fitness · documentation/governance revision pressure
```

The ordinary user does not need QL labels to use the system. The architecture may still be QL-shaped and the capability matrix may preserve QL affinity/use-type relations.

Minimal AIKit remains valid without this foundation. Installation/enablement should make the median praxis package a clear opt-in/default choice, not a hidden mandatory dependency.

---

## 10. External Skill architectures as managed sources and research exemplars

Matt Pocock's Skills and pstack/cursor-team-kit represent valuable, differently opinionated attempts at full Skill architecture.

AIKit should treat them as:

```text
managed external sources
+ provenance-preserving usable Skills
+ research exemplars for first-party praxis design
```

not as raw content to rewrite into AIKit voice.

Useful adopted practices may include, subject to live source review:

- unslop / deslop distinctions;
- verification disciplines;
- technical writing;
- skill-authoring craft;
- routing, handoff, grilling, specification and review patterns.

Upstream voice/authorship should remain attributable. Where an AIKit-native Method composes external Skills with first-party Skills, the Method holds the composition; the external Skill source need not be forked.

Rebounds/forks must retain upstream provenance and local delta identity.

---

## 11. Meta-praxis: authoring Skills and composing praxis systems

`aikit:skill-authoring` should continue beyond packaging into instruction architecture and ontology-aligned praxis craft.

Its deep path should ask:

```text
What is the owner and triggering situation?
Is this governance, Skill, Method, reference, Project doc or no durable instruction?
What vocabulary already exists in the Project?
Which terms correspond to real refs/operations/resources?
Are we inventing synonyms that create semantic drift?
What is the smallest sufficient common path?
What should remain progressively disclosed?
How will positive and negative triggers be tested?
How will returned evidence revise or delete this instruction?
```

A companion first-party composition skill/method should teach the higher-order task:

- compose atomic Skills into coherent SkillSets without inventing workflow semantics in the set;
- decide when a UsageOverlay is enough;
- decide when repeated adaptation deserves a named Method;
- compose Methods from Skills, Actions and ContextSources;
- migrate/adapt an existing external Skill family into a Project's ontology, principles and values without erasing provenance;
- inspect whether a Method/SkillSet is over-broad, redundant or contradictory;
- build capability-matrix coverage and leave gaps explicit.

This is the native AIKit analogue of a complete Skill architecture rather than a collection of unrelated prompt files.

---

## 12. Explain / History / research evidence

AIKit should be able to explain, for a situated act:

```text
which Skill exists and where it came from
why it was eligible/trusted
which SkillSet selected/projected it
which Method related it to this Focus
which UsageOverlay changed its use here
which Project/Profile/scope contributed that relation
which harness projection exposed it
which Action/ContextSource/resource it routed toward
```

Factory may then consume those stable refs/receipts as part of Run evidence and evaluate praxis under model/harness/Project conditions.

Frequency/frecency is accessibility evidence, not skill quality or trust.

Repeated successful use is not automatic source promotion.

---

## 13. Explore / publication boundary

Skills, Methods and SkillSets are legitimate O:I local-world resources.

AIKit should expose stable source/provenance/ref relations sufficient for O:I Explore to project them without becoming their source owner.

Publication may later support:

```text
inspect
share
edit within source authority
fork/rebound with provenance
compose into local Methods/SkillSets
return improvements upstream explicitly
```

No shared library needs to become a second authoritative Skill store.

---

## 14. Acceptance shape

A representative median vertical should eventually prove:

```text
Central / ProjectCentral bootstrap
  ↓
visible skills/ + methods/ source roots
  ↓
human or Agent-assisted Project Skill / Method authoring
  ↓
ontology-aligned skill/method craft
  ↓
AIKit source discovery with stable refs/provenance
  ↓
Profile + Project SkillSet + Method resolution
  ↓
ContextResolution / Focus records the operative relation
  ↓
Claude + Codex target-native projection
  ↓
thin AIKit bootstrap discloses wider faculty
  ↓
Agent executes Method through Skills + ContextSources + Actions
  ↓
Artifacts + Claims + Evidence return
  ↓
Explain/History reconstruct source → resolution → projection → use
  ↓
Factory can relate returned outcome to praxis/model/harness conditions
```

This proof should include a contrasting minimal AIKit case showing that the median first-party foundation is optional rather than a hidden dependency.
