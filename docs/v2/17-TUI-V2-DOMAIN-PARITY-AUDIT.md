# TUI V2 domain/application parity audit

**Audit date:** 2026-08-17  
**Programme:** #28, children #40–#46  
**Live base audited:** `main` at `b7843b437a8d4edd364b22aec9d7625bc8124915`  
**Current corrective branch:** `agent/tui-v2-application-domain-parity` / PR #74

This is a live-state audit, not a reconstruction from historical stack SHAs. Historical child PRs are recorded as provenance; the accepted integrated V2 implementation line is PR #58, with SessionSpace/composition continuation in PR #68, model-roster continuation in PR #70, and later provider-authority work already merged into current `main`.

The distinction used below is deliberate:

- **domain/application gap** — shared semantic operation/read model/state still missing, still owned by a presentation crate, or not yet available to CLI/TUI/agent consumers through the same operation;
- **presentation-only gap** — renderer/chrome/interaction disclosure over an already-real shared semantic state;
- **physical-review gap** — cannot be closed by hosted snapshots/headless tests alone.

## Exact #40–#46 matrix

| Ticket | Requirement | Live implementation state | Accepted branch / PR | Dependencies | Test / evidence | Remaining domain/application gap | Remaining presentation / physical gap |
|---|---|---|---|---|---|---|---|
| **#40 E0** | One canonical `TuiState`/reducer/effect runtime; stable `ResourceRef`; staging survives refresh/navigation; one application-service path; keyboard/mouse semantic selection parity. | **Substantively implemented and issue closed.** Original closure candidate is PR #48; its code is present in the converged line. `TuiState` owns semantic selection/staging/presentation/relation-view/navigation/preview state and refresh reconciles by stable ref. | Provenance: PR #48. **Accepted current ancestry: merged PR #58 → current `main`.** | #23, ContextSources baseline. | PR #48 recorded exact candidate `a1af8a227ced82bd28b7e4c5cb02539c4f4ba919`, Actions run `31819969792` success. PR #58 exact accepted head `1036a8de0bb6bdc234e5334ba38730d60118aa4c`, run `31980459486` success. | Neutral application DTOs/service extension vocabulary still has some physical ownership under `aikit-tui`; that does not create a second reducer, but it remains a parity/dependency smell for #73. PR #74 begins removing domain composition from that boundary. | Final terminal quality is governed by #45 and end-to-end acceptance by #46, not #40. |
| **#41 E1** | Quick resource navigation; contextual Actions; ambient Context; keyboard/mouse semantic parity; Quick↔Workspace state preservation; no TUI ranking/resolver. | **Substantive semantics implemented; ticket remains open.** PR #51 added the distinct E1 grammar; PR #58 integrated live Quick/Workspace, Action search/invocation and shared state. | Provenance: PR #51 (draft stack). **Accepted: PR #58/current `main`.** | #40; familiarity #29; wider Project world #42. | PR #51 tests navigation/action/presentation parity and learned-evidence labelling. PR #58 pre-local suite covers the integrated surface. | No new Resource/Action resolver is required. Remaining shared-operation work is chiefly broader application parity: Profile/SkillSet/SessionSpace operations must be callable without the TUI. | Finish human-legible ambient Context/action affordances and live mouse/chrome review where still weak; #45 owns the deliberate visual/host/accessibility system and #46 the final integrated route. |
| **#42 E2** | Project Context + Compose + Projection over one resolved Project world: Profile/SkillSet/Capability relations, ContextSources, Agent/Agency/Model/Harness/Host, optional HarnessComposition/Components/Surfaces, staged preview/apply and changed ground. | **Large majority implemented.** `ProjectWorldReadModel`, `ContextResolution`, ContextSource horizon, intent/effective split, model/harness candidates, projection, runtime composition and current SessionSpace/model-roster continuations are real. PR #74 moves Project/Host/scope ContextResolution composition out of TUI ownership. | Provenance: PR #52. **Accepted foundation: PR #58; composition/SessionSpace: PR #68; model roster: PR #70; corrective parity: PR #74.** | #40/#41; #25/#26; #53; #61; model roster #64/#70; #73. | Core `project_world`, TUI Project/Compose/Projection tests, PR #58 pre-local acceptance; PR #68 SessionSpace evidence/run `31982798522`; PR #70 model-roster acceptance. PR #74 adds core application-context tests. | **Still material:** canonical authored/effective Profile + SkillSet relational read/mutation operations; ContextSource selector mutation (without retrieval collapse); common SessionSpace application operations; changed-ground/history after broader mutations; remove remaining application semantics physically hosted by TUI compatibility traits. These are the principal #73-enabling operation gaps. | Richer human composition inspector/preset authoring and final layout quality are presentation work once the shared operations exist. |
| **#43 E3** | Provider-backed universal relation navigation: SemanticWiki, SourcePool, CodeIndex/ProjectMap/KnowledgeRoute through list/tree/graph projections of one stable relation/selection state, with bounded leaf→local-whole expansion and provider degradation. | **Core/provider semantics are implemented to the accepted pre-local floor.** `KnowledgeApplication` is core-owned and the TUI `KnowledgeNavigationService` explicitly delegates to it; SemanticWiki/SourcePool/CodeIndex/ProjectMap/KnowledgeRoute/context packs exist. Parent #34 and I4 #38 remain open because the full programme is broader than the pre-local floor. | **Accepted: PR #58/current `main`.** | #34–#38; #29 familiarity; #40–#42. | PR #58 records real bkmr `7.6.7` and GitNexus `1.6.9` provider conformance plus repository verification; `knowledge_navigation_v2.rs` and provider suites remain in tree. | Full #38/#43 closure still needs complete common agent/CLI operation coverage plus explicit partial/degraded relation acceptance, bounded large-neighbourhood behaviour, and final provider-provenance checks across route/frame/history. Do not replace provider relations with a TUI graph. | Graph/tree/list interaction richness and human navigation judgement remain open; QL overlays remain optional. |
| **#44 E4** | Explain, History and familiarity over the same application state: provenance/ownership/availability, staged-delta/target-effect explanation, recent/familiar/changed/prior-world history, prior HarnessComposition/body comparison, KnowledgeRoute history, and learned-evidence separation. | **Substantial foundations implemented, ticket remains open.** Core/resource Explain exists; familiarity has durable learned evidence/replay; Project/Knowledge/runtime read models carry provenance; PR #58 converged History/familiarity and Knowledge Navigation; PR #68 adds runtime-body evidence; PR #70 adds reconstructable model-ranking explanations. | Familiarity provenance: PR #56. **Accepted integrated substrate: PR #58 + PR #68 + PR #70/current `main`.** | #40/#41; #29; #38 where routes exist; #53 where runtime-body evidence exists; richer staged mutation evidence from #42. | Familiarity store/replay tests, `application_service_*` Explain/History tests, KnowledgeRoute/provider suites, HarnessComposition diff/explain tests, model-roster explanation tests and pre-local acceptance. | Remaining semantic work is mostly **evidence supply and common read operations**, not a TUI-local history engine: explain the broader #42 Profile/SkillSet/ContextSource mutations and changed ground; complete previous Generation/world/body comparison/recovery where valid; preserve live activation vs Generation/Procedure history; consume complete #38 route/provider degradation evidence. | Complete the human Explain/History surfaces over those shared read models, including changed/familiar/prior-world views and clear authored/observed/derived/learned/generated distinctions. |
| **#45 E5** | Terminal visual system, responsive host behaviour and accessibility: calm hierarchy, semantic theme roles, non-colour state cues, narrow/wide disclosure, ASCII/Unicode fallbacks, host honesty and terminal restoration. | **Open by design; not the primary target of this session.** The accepted V2 line already preserves host/terminal safety and has substantial snapshots/responsive rendering, but the ticket asks for a deliberate human-reviewed visual language rather than merely passing semantic tests. | Existing substrate: PR #58/current `main`; a dedicated E5 presentation/host tranche should be based on the post-domain-parity line rather than used to hide unfinished #42/#44 semantics. | #40/#41; stable read models from #42–#44. | Existing Quick/Workspace snapshots, resize/state-preservation tests, terminal guard/restoration tests and pre-local acceptance are supporting evidence. | No new resolver, relation ontology, state store or business logic belongs here. Any state needed by presentation must come from the shared application/read-model layer. | **Primary remaining work:** narrow/medium/wide reviewed snapshots; semantic theme/non-colour cues; ASCII/Unicode and reduced-capability fallback; readable staged/current/unavailable/warning/learned states; real host/terminal behaviour review and performance confirmation. |
| **#46 E6** | Final V2 integration/parity/Closure: one end-to-end Project→Context→Search→Explain→Compose→preview/apply→resulting Context/HarnessComposition→knowledge relation→History route, plus no-degradation matrix across #40–#45/#38/#29/#53. | **Open and intentionally not claimed.** Much of the state/parity test substrate already exists, but #46 explicitly cannot substitute for unfinished child semantics and hosted snapshots alone cannot prove the full human/local route. | Future final convergence/acceptance tranche over genuinely complete #40–#45 and required #38/#29/#53 dependencies; **not PR #74.** | #40–#45; #38; required #29 familiarity; #53 composition; local/physical evidence where applicable. | Existing `application_v2`, application-service, mouse, Project-world, Knowledge Navigation, SessionSpace, snapshot and pre-local suites are partial evidence. PR #74 adds a combined stable-ref/staged-state provider-refresh regression. | Before closure: finish #42 shared operator parity, required #38 route/frame/provider-degradation semantics, the #44 evidence those changes require, and any #62 operations needed by the representative working-world route. Produce the explicit requirement → implementation → evidence → exact commit matrix; waive nothing for install convenience. | Review the complete narrow/medium/wide and real-terminal route after #45. Physical/local gaps stay explicit. **First O:I install may proceed earlier if day-one Agent/CLI/application operation and #73 native Skills are real, but #46 remains open until its full contract is met.** |

## Live ownership findings

### Correctly below the renderer now

The current tree already contains real shared/domain implementations for:

- canonical `ResourceRef` / `ResourceSearchIndex`;
- one `TuiState` reducer with semantic state preservation;
- `ContextResolution` and `ContextSource` contracts;
- Project-world authored/effective disclosure;
- `Profile`, `SkillSet` and existing project SkillSet routing primitives;
- `KnowledgeApplication`, SemanticWiki, SourcePool, CodeIndex, ProjectMap, KnowledgeRoute and context packs;
- Harness/Component/Surface composition plus staged HarnessComposition mutation;
- SessionSpace runtime/read model and provider observations;
- contextual model roster/ranking;
- familiarity/History evidence;
- provider degradation/absence as explicit state rather than invented availability.

### Remaining application/domain parity debt

1. **Profile/SkillSet relational operation seam.** The domain primitives and store Procedures exist, but the V2 operator contract is not yet one obvious shared operation family that lets CLI/TUI/Agent perform `inspect → stage relation → preview resolution → apply` identically.
2. **SessionSpace application service.** #61 explicitly leaves list/show/create/open/bind/unbind/focus/reconcile/reconstruct/explain/history to #62 through the canonical shared ApplicationService.
3. **Project-world compatibility ownership.** Before PR #74, `aikit_tui::project_world_service` physically composed ProjectBinding/Host/scope/resource ContextResolution. PR #74 moves that composition into I/O-free `aikit-core::application_context_resolution`; TUI becomes an adapter.
4. **Neutral operation DTO ownership.** Some staging/preview/read-model types are still defined under the TUI crate even where their semantics are application-neutral. They should migrate only when doing so reduces dependency inversion without destabilising the accepted reducer.
5. **Broader changed-ground/history.** Capability toggles are mature; Profile/SkillSet/ContextSource/runtime/SessionSpace mutations need a common preview that discloses changed effective ground, provider/target effects, restart/live requirements and warnings. #44 should consume those receipts for Explain/History rather than re-derive them.
6. **Stable relation state under broader mutation.** List/tree/graph already share TUI relation-view state, but final #46 acceptance must prove stable selection/staging when provider-backed relations disappear/reappear during richer composition.

## #73 application-first relation

The native Skills programme must consume domain/application operations, not terminal gestures. The intended semantic sequence is:

```text
inspect Profile / Project world
  → inspect authored + effective SkillSet relations
  → stage relation mutation
  → preview ContextResolution / changed ground / target effects
  → apply through Procedure / Generation authority
  → inspect Explain / History receipt
```

The same rule applies to SessionSpace and Knowledge Navigation. A native Skill may teach operation names, preconditions, evidence and authority boundaries. It must not encode a TUI keybinding as the canonical meaning of the operation.

This means the first O:I install need not wait for full #28/#45/#46 human-facing closure, but it **does** depend on a minimally complete shared operator seam plus #73 repository-owned Skill publication. #46 remains the final closure gate and is not weakened by that installation sequencing.

## Preset law

Presets should remain authored starting compositions over real objects:

`Project + Profile + SkillSets + ContextSources + Agent/Agency + model/harness/host + SessionSpace/HarnessComposition + projection targets`.

A preset may select/default those relations. It must not become an opaque TUI-only ontology or a second resolver.

## Next safe V2 tranche

After PR #74 is green, the safest semantic continuation is **#42/#73 application parity**, not renderer restyling:

1. expose one UI-neutral Profile/SkillSet relation read model over authored and effective state;
2. implement staged add/remove/reorder relation intent using existing Procedures/store authority;
3. preview the resulting ContextResolution/changed ground without writing;
4. apply only the reviewed preview through the existing durable mutation path;
5. expose that operation to CLI/TUI/agent consumers from the same service;
6. add cross-consumer tests plus provider-loss/stale-preview tests;
7. then extend the same shared-operation shape to the remaining #62 SessionSpace operations;
8. let #44 consume those mutation/application receipts for Explain/History rather than rebuilding semantics.

After those semantic operations are real, #43 can finish any remaining provider-degraded relation acceptance, #45 can concentrate on the deliberate visual/host/accessibility system, and #46 can run the complete end-to-end parity/Closure matrix. The first O:I install does not need to wait for that final visual/human closure, but no #46 requirement is waived.
