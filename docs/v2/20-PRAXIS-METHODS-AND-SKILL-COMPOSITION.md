# AIKit V2 — Praxis, Methods, and Skill Composition

**Status:** current implementation contract  
**Date:** 2026-08-19  
**Implementation owner:** `crates/aikit-core/src/method.rs`, `praxis.rs`, existing Skill/SkillSet/Profile/ContextResolution/Skill Usage Overlay machinery  
**Coordinates with:** O:I authored praxis position; Central ProjectCentral source contracts; Factory praxis/evidence consumption

## 0. Why these distinctions exist

AIKit is the actor's context-cognition and operative-body faculty: it discovers, resolves, explains and projects powers and information into an actual working context. Skills are therefore not merely prompt files. They are one way reusable intelligent praxis becomes available to an actor.

The missing relation was between reusable praxis, situated adaptation, contextual composition, repertoire and operational resolution. These are now kept separate because collapsing them makes it impossible to answer basic provenance questions such as: what reusable practice existed, what was changed only for this Project, what relation among practices was selected for this Focus, what was merely projected into the harness, and what actually became operative under policy/resolution?

The implemented grammar is:

```text
Guidance
    standing orientation

Skill
    reusable organised intelligent praxis

UsageOverlay
    scoped adaptation of an unchanged Skill

Method
    Focus-bearing contextual composition of independently owned
    Skills + UsageOverlay receipts + Actions/Capabilities + ContextSources
    + Project/domain refs + verification/expected-return forms

SkillSet
    additive repertoire / projection unit

Profile / ContextResolution
    why and where resources become operative
```

The existing law remains:

```text
profile : resolution :: skill-set : projection
```

`Method` answers a different question: **how are already-addressable praxis/resources related for this kind of act around this Focus?**

## 1. Skill remains reusable praxis

A Skill remains an independently owned reusable body of organised intelligent praxis. It may route toward Actions, ContextSources, tools and other capabilities, but its source identity does not become Project-local merely because a Project uses it.

AIKit currently represents native Skill capability in the existing capability/capsule field. This implementation does **not** add a second `Skill` ResourceKind merely for conceptual symmetry.

A Project should not fork a sound reusable Skill just to state a small local orientation. That is the reason UsageOverlay exists.

## 2. UsageOverlay is the existing scoped adaptation seam

The existing `SkillUsageOverlayPatch` mechanism is the runtime adaptation mechanism. No second overlay store was introduced.

The source Skill remains unchanged. Scope resolution produces the effective adaptation. `UsageOverlayRef` in Method is an immutable evidence receipt containing:

```text
Skill ResourceRef
scope
exact lowercase 64-character content digest
optional source ref
```

The digest exists so later evidence can identify **which adaptation was actually in force** without copying the overlay text into every Method or Run receipt.

The invariant is:

```text
UsageOverlay != Skill source mutation
Effective Skill projection != Skill source
```

Repeated useful overlay use may create evidence for a Project Method or a later reusable Skill refinement. Repetition is not automatic promotion.

## 3. Method is a source-addressable situated relation

`ResourceKind::Method` is now implemented because a Method has an independent source identity and lifecycle worth addressing. The Method body remains source-owned; its referenced resources remain independently owned.

Current `Method` carries:

```text
MethodRef
SourceRef + optional SourceRevision
name / description
Focus refs
Project/domain refs
Skill refs
optional exact UsageOverlay receipts
Action refs
Capability refs
ContextSource refs
verification refs
expected return forms
```

It deliberately does **not** copy the referenced Skill, Action or ContextSource bodies.

`resolve_method` checks those refs against the V2 ResourceIndex and returns an observational `MethodResolution`. It does not enable, disable, trust, sequence, authorise or mutate members.

`resolve_praxis` selects explicitly requested Methods only **after** a `ContextResolution` already exists. This preserves the operational law:

```text
Method selected != capability activated
Method selected != Action authority
Method member != trusted resource
Method selected != Profile precedence
```

The current implementation does not introduce a sequence DSL. If a future practice genuinely owns order or conditions, that should be represented at the smallest owner that actually requires them rather than turning Method into a universal workflow engine.

## 4. SkillSet remains additive repertoire

SkillSet still answers:

> What repertoire is made available/projected as a set?

Its composition law remains additive union subject to each member's own gates. It has no trust of its own and contains no Method sequencing or Profile precedence.

Method instead answers:

> How are relevant available resources related for this Focus?

Profile/ContextResolution instead answers:

> What resolves as operative here under these scopes, providers, policies and targets?

Those three questions must remain distinct.

## 5. Project praxis source and ownership

Central #82 owns the ProjectCentral/personal source apertures for human/Project Skills and Methods where Central is present. AIKit consumes those sources and indexes/resolves their refs; it does not acquire their authorship.

Existing native Skill/Method-like material in heterogeneous Projects may remain in place when compatible. Adoption can preserve exact source identity and relation without copying material into ProjectCentral.

Generated Claude/Codex/harness projection directories are destinations/derived material, never source merely because they contain files named like Skills or instructions.

## 6. First-party praxis is a coherent family, not one Skill per noun

The current first-party working family is intentionally developed through existing Skills:

```text
product-understanding
    recover authored meaning, Project vocabulary, broad→local source chain,
    current implementation and returned difference

knowledge-navigation
    navigate SemanticWiki / source / ProjectMap / exact code / evidence

skill-authoring
    decide Skill vs UsageOverlay vs Method vs SkillSet and author in Project vocabulary

verification
    verify source, overlay, Method, SkillSet, reflection and implementation claims

profile-skillset
    retain Profile/resolution vs SkillSet/projection distinction
```

The implementation does not add `project-reflection` or `document-governance` Skills merely because those are useful nouns. Their behaviour is part of the existing practices through which an Agent encounters the Project.

## 7. Native filesystem/document-governance praxis

The median Project practice now carries these relation laws:

```text
read applicable broad -> local source before changing a region
closest applicable contract supplies local specificity
one durable fact/contract has one authoritative home
link rather than duplicate
routing/catalog material stays small
generated indexes are rebuildable and not hand-authored truth
stable reference and current working material stay distinct
structural changes return pressure to the nearest owning description/contract
load the smallest context relevant to the act
```

These are not a global repository format. AIKit does not require a universal `AGENTS.md`, `CONTEXT.md`, numbered stage tree or proprietary source layout.

## 8. Bootstrap is Project recovery, not configuration interrogation

`project_recovery` now composes an explicit recovery receipt around already-owned structures:

```text
Authored Ground where present
native Project source
SemanticWiki where present
local structural description where present
Project reflection where present
capability / Method praxis
ContextResolution
harness projection
```

Richer layers are optional. An ordinary Project with no Central, no coordinates and no Method tree remains valid.

A Project bootstrap therefore follows the median relation:

```text
existing/fresh Project
+ personal Central Ground/praxis where available
        ↓
recover authored intent / native docs / code / history / native Skills
        ↓
product-understanding / Wayfinder
        ↓
recover Project language / ontology
        ↓
recover scoped local descriptions/contracts
        ↓
establish/update ProjectCentral where chosen/available
        ↓
SemanticWiki / ProjectMap / ContextSources
        ↓
establish meaning ↔ description ↔ code reflection
        ↓
identify capability/praxis gaps
        ↓
standing governance
        ↓
reusable Skills / UsageOverlays
        ↓
Project Methods
        ↓
SkillSets / Profile
        ↓
ContextResolution + target-native projection
        ↓
act / evidence / return through the actual owning systems
```

Human attention belongs at genuine authored/recognitional boundaries. File classification and ordinary source relations should be recovered from evidence where possible rather than delegated to the human as clerical work.

`ProjectRecoveryReadModel.act_authority_inferred` is always false: recovering an articulated Project world is not an Actuation authority grant.

## 9. Explain / History requirement

For consequential use, later evidence must be able to reconstruct at least:

```text
Method source/revision
Skill refs materially selected
UsageOverlay exact digests
ContextSource / Action / Capability refs
Focus / Project relation
ContextResolution version/condition
projection targets / effective capability condition
semantic/source/code anchors materially traversed
verification / returned evidence produced by the owning operation/Run
```

AIKit's existing `ExplainEvidence` and `HistoryEvidence` remain the read-model carriers. The architecture should append owner-produced use/Return evidence rather than inventing an AIKit Run ontology.

Frequency/familiarity remains accessibility evidence, not Skill/Method fitness, trust or source promotion.

## 10. Boundaries

This architecture does not move ownership:

```text
Central/native Project
    durable authored source, local contracts, governance/source relation

AIKit
    source discovery/classification, cognition, praxis/reflection resolution,
    target-native projection, Explain/History read models

Factory
    developmental Project/Run/Candidate/evidence and returned praxis-fitness observation

Actuation
    situated Agency, WorldBinding, Determination, authority and Return
```

A filesystem path/local contract is not `WorldBinding` identity. Resolved guidance is not Agency authority. ProjectMap is not Actuation topology.

## 11. Implemented primitive decisions

Accepted in the current implementation:

- `Method` has independent `ResourceKind::Method` identity.
- UsageOverlay reuses the existing scoped Skill adaptation mechanism.
- Method selection is downstream of ContextResolution.
- stable `SourceRef`/`ResourceRef` are used rather than copied bodies.
- Project vocabulary/source/code refs should be referenced when available rather than restated in prompt prose.

Rejected:

- sequencing/trust/Profile precedence in SkillSet;
- a second UsageOverlay store;
- a Method mega-Skill which copies source;
- Method as Action authority;
- mandatory QL/MEF vocabulary for ordinary praxis;
- generated harness files as source.

## 12. Acceptance

A representative rich vertical must prove Project Ground → SemanticWiki → local description → exact code → reflection → Method/ContextResolution → Claude/Codex projection → use/evidence/return with stable differentiated refs.

A contrasting minimal Project must continue to prove ordinary Knowledge Navigation and native Skill operation without Central, Bimba coordinates, special local headers or Method source.

The target is not a more elaborate configuration model. It is a Project in which developed ways of acting can be named, adapted, composed, resolved, used and revised without losing where each distinction came from or which owner is entitled to change it.
