# V2 Profile / SkillSet Composition Application Parity

Status: implementation receipt / ownership map for #42, coordinating #28, #38, #44, #46, #53, #62 and #73.

This document records the ownership discovered from the live repository while implementing the first shared Profile/SkillSet composition tranche. It is not a TUI design document. It exists to make semantic ownership and continuation boundaries explicit before presentation work continues.

## 1. Governing operation

The shared operation is:

```text
inspect Project / Profile
  -> inspect authored Profile + effective resolution
  -> inspect Project -> SkillSet selections
  -> inspect SkillSet -> Capability membership/projection
  -> stage typed intent without writing
  -> resolve a canonical preview
  -> disclose changed ground / provider withholding / target effects
  -> reject stale accepted previews
  -> apply through the existing Generation or Procedure owner
  -> re-resolve and emit durable evidence
```

CLI syntax, TUI gestures and native Skill procedure text are projections of this operation. None is the operation itself.

## 2. Relation laws found live

### Profile

`Profile` / `PoolPatch` is authored resolution intent. `ResolvedView` is derived effective state. A staged Profile activation therefore changes an authored copy first and is previewed only by running the canonical resolver again.

Effective resolution is never treated as write authority.

### Project -> SkillSet selection

Legacy/current Project specifications name SkillSets through `skill_sets` and can inherit default SkillSets. Resolution composes these selections by stable union.

The relation can therefore be:

- authored on the Project;
- inherited/default;
- effective in the resolved Project selection;
- selected but temporarily unavailable to the current SkillSet provider/store.

There is no semantic precedence or reorder operation. Stable presentation order is not authority.

The new core `composition_workspace` model represents these states without moving ProjectSpec persistence into the TUI.

### SkillSet -> Capability membership

SkillSet is a projection request, not activation/trust authority. Membership is explicit or pattern-derived and projection is checked against `ResolvedView`.

SkillSets compose by union. There is no `exclude`, override or precedence relation and therefore no semantic reorder mutation.

Observed SkillSets are read-only. Project/composed sets are writable through their owning store path.

### ContextSource

The live `ContextSourceIndex` owns descriptor disclosure and provider-mediated retrieval. Its mutable flags describe operational/disclosure state (`retrieved`, `loaded`, `focused`, `invoked` etc.). They are not a canonical authored selector relation.

Therefore this tranche does **not** reinterpret `set_disclosure`, `set_loaded` or `set_focused` as Profile/Project ContextSource selection.

The blocker for selector mutation is precise: an authored selector identity/provenance + owning Project/Profile source + canonical write authority has not yet been established. Until that contract exists, read/preview may disclose the horizon, but selection must remain distinct from retrieval, loading and context-pack materialisation.

## 3. Ownership map

| Domain object / relation | Canonical owner | Read | Stage | Preview | Apply authority | Receipt / history | CLI | TUI | agent / native Skill |
|---|---|---|---|---|---|---|---|---|---|
| Project identity/binding | `aikit-core::project`, application discovery | `ProjectWorldReadModel` / `ProjectBinding` | no new owner in this tranche | Project-world disclosure | existing Project owner | existing Project/provenance records | yes | yes | bootstrap/context Skill surfaces |
| authored Profile | `aikit-core::profile`; scope documents in store | `PoolPatch`, Profile files | `StagedProfileComposition` | authored copy + resolver | existing scope writer + Generation authority | Generation + warnings/effects | existing enable/disable/apply | shared composition preview/apply | `aikit-profile-skillset-management` |
| effective Profile | resolver output | `ResolvedView`, `ProfileCompositionReadModel` | never directly staged | canonical resolver only | **never a write authority** | catalog revision + resolution hash | yes | yes | yes |
| Project -> SkillSet selection | Project specification owner | `ProjectSkillSetRelationReadModel` | `StagedProjectSkillSetSelections` | union over authored/inherited/effective input | existing Project specification writer; Procedure migration still open | Project source revision currently, richer receipt open | Project commands exist | read/stage model available; durable apply still open | Skill documents the distinction |
| SkillSet -> Capability membership | `aikit-core::skillset` + `aikit-store::skillsets` | membership + `project(set, ResolvedView)` | `StagedSkillSetRelations` | resolver-backed profile preview + Procedure diff | `aikit-store::composition_application` -> existing SkillSet Procedure runner | structured Procedure receipt + undo | existing `aikit set add/remove`; shared store seam reusable | application adapter can consume same store seam; full gesture wiring open | native Skill consumes the same relation semantics |
| capability changed ground | resolver | `ChangedGround` | n/a | `changed_ground(before, after)` | n/a | structured before/after evidence | compatibility diff projects it | production composition preview/apply projects it | native Skill names required fields |
| HarnessComposition | `aikit-core::composition` | resolver/read model | `StagedHarnessComposition` | canonical composition resolver | target/provider owner after confirmed desired body | fingerprint/diff; target history separate | application path | existing Compose surfaces | actor bootstrap/runtime operations |
| ContextSource horizon | `aikit-core::context_source` / provider | descriptor-only horizon/explain | no authored selector mutation yet | read-only horizon only | retrieval remains provider-owned, selector writer undefined | provider/source provenance | knowledge/context commands | Project-world/Compose reads | agent can discover/retrieve via provider operation |
| KnowledgeRoute | `KnowledgeApplication` / provider owners | shared knowledge application | no mutation ownership moved here | provider-backed | provider/domain owner | Explain/provenance | existing application consumers | TUI consumer | structured application consumer |
| SessionSpace | `aikit-core::session_space` + existing runtime authority | `SessionSpaceReadModel` | #62 continuation | #62 continuation | must use same shared ApplicationService architecture | must remain distinct from Generation/Procedure history | existing session commands | existing runtime/read floor | future native operator path |

## 4. Core contracts added in this tranche

`aikit-core::composition_mutation` now owns:

- `CompositionBasis { catalog_revision, resolution_hash }`;
- typed Profile activation staging;
- typed SkillSet membership add/remove staging;
- authored/effective Profile read models;
- SkillSet membership/projection read models with effective/withheld state;
- `ChangedGround` for active capabilities, authored declarations, unavailable states and warnings;
- resolver-backed structured composition preview;
- `composition.preview_stale` basis rejection.

`aikit-core::composition_workspace` adds:

- Project -> SkillSet authored/inherited/effective/available relation disclosure;
- typed write-free Project SkillSet selection staging;
- a `ProjectCompositionWorkspaceReadModel` that composes the existing `ProjectWorldReadModel` with the Profile composition model rather than copying Project/Context/runtime/projection truth.

## 5. Durable SkillSet apply authority

`aikit-store::composition_application` is a store-owned adapter over the existing SkillSet Procedure planners/runner.

It provides:

```text
SkillSetRelationMutation
  -> Procedure-backed diff preview (no write)
  -> membership basis recheck
  -> immutable Procedure identity recheck
  -> ProcedureRunner apply
  -> structured receipt + undo evidence
```

If membership changes after preview, apply returns `composition.preview_stale` before applying the accepted mutation.

The store does not resolve effective state. Resolution remains core/application work; the store only makes accepted authored intent durable.

## 6. TUI integration status

The existing single `TuiState` remains the only TUI semantic state machine. No ProfileController, SkillSetController, ComposeStore or PresetState was added.

The production TUI application service now:

- obtains a preview from the same backend resolver used by the CLI Service;
- computes effective changed ground through the core contract;
- binds preview identity to before **and** projected-after catalog/revision hashes;
- re-runs the canonical preview immediately before apply;
- rejects materially stale accepted previews as `composition.preview_stale`;
- returns the resulting Generation id after canonical apply.

The compatibility `aikit diff` staging module still exists because published CLI compatibility types remain package-oriented, but its effective before/after capability delta is now sourced from the same core `changed_ground` operation. It is no longer a TUI-only source of effective truth.

## 7. Native Skill parity

The #73 first-party `aikit-profile-skillset-management` Skill consumes the shared operation vocabulary directly:

- inspect authored and effective state;
- record resolver basis;
- stage typed source intent;
- preview through the canonical resolver;
- inspect changed ground and degradation;
- reject stale previews;
- apply only through AIKit-owned mutation authority;
- report Generation / Procedure evidence where applicable.

It explicitly does not encode terminal coordinates, TUI tabs or keybindings as canonical semantics.

## 8. Explain / History boundary (#44)

This tranche supplies evidence for #44 but does not make #44 a writer.

Available evidence now includes:

- authored before/after;
- effective before/after;
- resolver basis before/after;
- membership/projection withholding reason;
- changed ground;
- target activation effects on the existing application path;
- Generation id for Profile activation;
- Procedure id, applied edit count, resulting members and undo command for SkillSet membership.

#44 still owns the richer durable Explain/History presentation and meaningful historical composition-state navigation. Live target activation history must remain separate from Generation/Procedure history.

## 9. Knowledge Navigation boundary (#38)

No KnowledgeRoute or provider graph ontology was changed. `KnowledgeApplication` remains the UI-neutral knowledge operation surface; Project-world disclosure continues to consume descriptor-only ContextSource horizons and provider provenance.

No route step was turned into a provider graph edge and no familiarity signal was converted into trust or authored preference.

## 10. SessionSpace continuation (#62)

This tranche deliberately stops before inventing SessionSpace mutation semantics.

The safe continuation is to project foundational #62 operations through the **same application-service architecture** now used for Profile composition:

```text
list/show
  -> create/open
  -> bind/unbind Project + Context
  -> focus
  -> reconcile/reconstruct
  -> explain/history
```

Do not create a SessionSpace-local application service or second resolver. Runtime activation history remains separate from Profile Generation and Procedure history.

## 11. Installation readiness boundary

This tranche improves the day-one non-visual operator path materially:

- CLI already has real Generation-backed Profile activation and Procedure-backed SkillSet writes;
- shared core staging/read/preview/stale-protection contracts now exist beneath the TUI;
- the native #73 Skill can describe and drive the same operation vocabulary without renderer knowledge;
- SkillSet membership has a reusable Procedure-backed preview/apply receipt seam.

It does **not** by itself close #28, #42, #44, #45 or #46. In particular, Project -> SkillSet selection still needs migration from the current ProjectSpec direct writer into the same preview/apply receipt discipline before #42 can claim complete relational composition, and ContextSource authored selector authority remains undefined.

## 12. Exact remaining semantic frontier

### #42

- wire `ProjectCompositionWorkspaceReadModel` into the live Project/Compose workspace;
- migrate Project -> SkillSet selection apply from direct ProjectSpec persistence into canonical staged preview/apply authority;
- complete one combined structured receipt presented identically to all three consumers;
- define/consume ContextSource selector mutation only after its authoring owner exists.

### #38

- no redesign required here; retain KnowledgeApplication/provider provenance parity.

### #44

- consume structured composition receipts in durable Explain/History;
- preserve historical composition state without a TUI-local history database.

### #53

- continue consuming existing HarnessComposition resolver/activation contracts; no identity collapse introduced by this tranche.

### #62

- extend the same application-operation architecture after #42's foundational composition path is complete.

### #73

- first-party Skill is now an explicit consumer; future improvement is executable structured dispatch directly against the shared operation rather than relying on a human/agent choosing a CLI projection.

### #46

- end-to-end requirement -> implementation -> evidence matrix, physical host/accessibility and final live consumer parity remain open.
