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
| **#40 E0** | One canonical `TuiState`/reducer/effect runtime; stable `ResourceRef`; staging survives refresh/navigation; one application-service path; keyboard/mouse semantic selection parity. | **Substantively implemented and issue closed.** Original closure candidate is PR #48; its code is present in the converged line. `TuiState` owns semantic selection/staging/presentation/relation-view/navigation/preview state and refresh reconciles by stable ref. | Provenance: PR #48. **Accepted current ancestry: merged PR #58 → current `main`.** | #23, ContextSources baseline. | PR #48 recorded exact candidate `a1af8a227ced82bd28b7e4c5cb02539c4f4ba919`, Actions run `31819969792` success. PR #58 exact accepted head `1036a8de0bb6bdc234e5334ba38730d60118aa4c`, run `31980459486` success. | Neutral application DTOs/service extension vocabulary still has some physical ownership under `aikit-tui`; that does not create a second reducer, but it remains a parity/dependency smell for #73. PR #74 begins removing domain composition from that boundary. | Physical terminal quality remains governed by #46, not #40. |
| **#41 E1** | Quick resource navigation; contextual Actions; ambient Context; keyboard/mouse semantic parity; Quick↔Workspace state preservation; no TUI ranking/resolver. | **Substantive semantics implemented; ticket remains open.** PR #51 added the distinct E1 grammar; PR #58 integrated live Quick/Workspace, Action search/invocation and shared state. | Provenance: PR #51 (draft stack). **Accepted: PR #58/current `main`.** | #40; familiarity #29; wider Project world #42. | PR #51 tests navigation/action/presentation parity and learned-evidence labelling. PR #58 pre-local suite covers the integrated surface. | No new Resource/Action resolver is required. Remaining shared-operation work is chiefly broader application parity: Profile/SkillSet/SessionSpace operations must be callable without the TUI. | Finish human-legible ambient Context/action affordances and live mouse/chrome review where still weak; physical judgement belongs to #46. |
| **#42 E2** | Workspace Project world; authored/effective Context; Capabilities/Actions/ContextSources; Agent/Agency/Host; Model/Harness; projection; staged composition. | **Large majority implemented.** `ProjectWorldReadModel`, `ContextResolution`, ContextSource horizon, intent/effective split, model/harness candidates, projection, runtime composition and current SessionSpace/model-roster continuations are real. PR #74 moves Project/Host/scope ContextResolution composition out of TUI ownership. | Provenance: PR #52. **Accepted foundation: PR #58; composition/SessionSpace: PR #68; model roster: PR #70; corrective parity: PR #74.** | #40/#41; #53; #61; model roster #64/#70; #73. | Core `project_world`, TUI Project/Compose/Projection tests, PR #58 pre-local acceptance; PR #68 SessionSpace evidence/run `31982798522`; PR #70 model-roster acceptance. PR #74 adds core application-context tests. | **Still material:** canonical authored/effective Profile + SkillSet relational read/mutation operations; ContextSource selector mutation (without retrieval collapse); common SessionSpace application operations; changed-ground/history after broader mutations; remove remaining application semantics physically hosted by TUI compatibility traits. | Richer human composition inspector/preset authoring and final layout quality are presentation work once the shared operations exist. |
| **#43 E3** | Provider-backed Knowledge Navigation: SemanticWiki, SourcePool, CodeIndex, ProjectMap, KnowledgeRoute, Frame/context pack, Explain; degraded provider truth; list/tree/graph views over same relations. | **Core/provider semantics are implemented to the accepted pre-local floor.** `KnowledgeApplication` is core-owned and the TUI `KnowledgeNavigationService` explicitly delegates to it; SemanticWiki/SourcePool/CodeIndex/ProjectMap/KnowledgeRoute/context packs exist. Parent #34 and I4 #38 remain open because the full programme is broader than the pre-local floor. | **Accepted: PR #58/current `main`.** | #34–#38; #29 familiarity; #40–#42. | PR #58 records real bkmr `7.6.7` and GitNexus `1.6.9` provider conformance plus repository verification; `knowledge_navigation_v2.rs` and provider suites remain in tree. | Full #38 closure still needs complete common agent/CLI operation coverage and provider-partial/degraded route/frame/history acceptance. Do not replace provider relations with a TUI graph. | Graph/tree/list interaction richness and human navigation judgement remain open; QL overlays remain optional. |
| **#44 E4** | Workflow composition; Explain/History; stage → preview → apply; same operation for CLI/TUI/agent; Profile/SkillSet and runtime-body composition. | **Partially implemented, not closable.** Legacy capability composition has scoped staging/preview/confirm/apply. Harness composition has canonical mutation types and PR #58/#68 runtime body state. Explain/History/familiarity exist. | **Accepted substrate: PR #58 + PR #68/current `main`; current parity work PR #74.** | #40–#43; #53; #61/#62; #73. | `application_v2`, `application_service_*`, composition mutation suites, familiarity history, SessionSpace suites. | **Primary semantic gap:** UI-neutral application-operation layer must cover inspect authored/effective Profile, stage SkillSet relation, preview resolution/changed ground, apply through canonical Procedure/Generation paths; equivalent SessionSpace list/show/create/open/bind/unbind/focus/reconcile/reconstruct/explain/history remains #62 work. TUI keybindings must never be canonical Skill semantics. | Composition affordances/presets may be refined after operations are real; no opaque TUI-only preset ontology. |
| **#45 E5** | Headless semantic acceptance: keyboard/mouse parity; Quick↔Workspace; list↔tree↔graph; staging persistence; provider loss; stable refs; resize; snapshots; no accidental writes. | **Much of the test floor exists, but ticket remains open.** Current suites already cover stable-ref refresh, staging persistence, Quick/Workspace and relation-view state, mouse context, Project-world surfaces, provider-aware knowledge, working-field SessionSpace projection, and pre-local product acceptance. | **Accepted test substrate: PR #58 + PR #68/current `main`; continue on PR #74 or later dedicated E5 tranche.** | #40–#44. | `crates/aikit-tui/tests/application_v2.rs`, `application_service_v2.rs`, `application_service_backend_v2.rs`, `mouse_context_v2.rs`, `project_world_surface_v2.rs`, `knowledge_navigation_v2.rs`, `working_field_session_space_v2.rs`, `v2_prelocal_acceptance.rs`. | Need an explicit final parity matrix after #44 shared-operation expansion: provider loss during staged work, refresh during non-capability relational staging, agent/CLI/TUI application-service parity, SessionSpace reconstruction, and no-write guarantees across those new operations. | Snapshot/headless coverage is necessary but not sufficient for #46. |
| **#46 E6** | Physical terminal acceptance, documentation and final TUI closure without semantic regressions. | **Open and intentionally not claimed.** No hosted CI result can satisfy the physical human-review contract. | Future physical-acceptance tranche only; **not PR #74 by default.** | #40–#45 genuinely complete; local/physical environment. | Hosted snapshots/pre-local evidence are supporting evidence only. | No semantic requirements may be waived merely to reach physical alpha. First O:I install may proceed if day-one Agent/CLI/application operation and #73 Skills are real, while #46 stays open. | **Primary remaining work:** real terminal/device review, interaction coherence, legibility, mouse/keyboard feel, resizing, local provider degradation, docs/screenshots where useful, and explicit human acceptance. |

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
5. **Broader changed-ground/history.** Capability toggles are mature; Profile/SkillSet/ContextSource/runtime/SessionSpace mutations need a common preview that discloses changed effective ground, provider/target effects, restart/live requirements and warnings.
6. **Stable relation state under broader mutation.** List/tree/graph already share TUI relation-view state, but final E5 must prove stable selection/staging when provider-backed relations disappear/reappear during richer composition.

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

This means the first O:I install need not wait for #46 visual closure, but it **does** depend on a minimally complete shared operator seam plus #73 repository-owned Skill publication.

## Preset law

Presets should remain authored starting compositions over real objects:

`Project + Profile + SkillSets + ContextSources + Agent/Agency + model/harness/host + SessionSpace/HarnessComposition + projection targets`.

A preset may select/default those relations. It must not become an opaque TUI-only ontology or a second resolver.

## Next safe V2 tranche

After PR #74 is green, the safest semantic continuation is **#44/#73 application parity**, not renderer restyling:

1. expose one UI-neutral Profile/SkillSet relation read model over authored and effective state;
2. implement staged add/remove/reorder relation intent using existing Procedures/store authority;
3. preview the resulting ContextResolution/changed ground without writing;
4. apply only the reviewed preview through the existing durable mutation path;
5. expose that operation to CLI/TUI/agent consumers from the same service;
6. add cross-consumer tests plus provider-loss/stale-preview tests;
7. then extend the same shape to the remaining #62 SessionSpace operations.

Only after that semantic tranche should E5 be ratified and E6 move into physical human acceptance.
