# AI Kit V2 Pre-Local Acceptance Evidence

**Status:** substantive cloud acceptance complete on code candidate `d22012a319c3e7937fe0a8b29fca9336433b292c`; final evidence-only head still requires exact-head CI  
**Rule:** `COMPLETE` means the implementation and repository-owned evidence exist on the V2 integration line. `LOCAL` is reserved for truth that requires the actual O:I/Central/Workcell package or physical host/process/network/terminal environment. `DOWNSTREAM`/`PARALLEL` work does not expand #60.

## Current integration line

- PR #58 — `feat(v2): converge the operational V2 integration line`
- branch `agent/aikit-v2-integration`
- code candidate: `d22012a319c3e7937fe0a8b29fca9336433b292c`
- code-candidate GitHub Actions: CI #420 / run `31980096724` — **SUCCESS**
- V2 product law: `docs/v2/README.md` through `docs/v2/10-PERSISTENT-AGENCY-AND-MATERIAL-HOSTING.md`
- migration/legacy classification: `docs/v2/11-MIGRATION-CLOSURE-LEDGER.md`

## Cloud-achievable evidence

| Requirement | Canonical implementation / evidence | Status |
|---|---|---|
| one Resource identity | `ResourceRef` / `ResourceRecord` / `ResourceIndex`; selection and relation views preserve the same ref | COMPLETE |
| one ContextResolution/application architecture | `context_resolution.rs`, project-world application service, `ApplicationService`; renderers do not resolve | COMPLETE |
| one TUI state/reducer | `application.rs` `TuiState` + `TuiRuntime`; final `ApplicationSurfaceController` only | COMPLETE |
| canonical Search / Context / Compose / Knowledge / Explain / History field | `project_workspace_render.rs`; `application_surface_prelocal_v2.rs` | COMPLETE |
| explicit selection rather than query-owned cursor identity | reducer keeps search and `Select(ResourceRef)` distinct; final-surface tests explicitly select before actions/staging | COMPLETE |
| universal ResourceRef-native search | `resource/search.rs` + `ApplicationService::search` | COMPLETE |
| contextual Actions | canonical Action resources / descriptors; text action lane and explicit stageability | COMPLETE |
| one relation state → List / Tree / Graph | `RelationReadModel` + `RelationView`; mutable Tree controller deleted; final-surface identity test | COMPLETE |
| SemanticWiki native provider | OKF objects/index/provider/application | COMPLETE |
| native SourcePool | `knowledge_source_pool.rs`; Knowledge application tests | COMPLETE |
| Git/source CodeReference | canonical code-reference provider model | COMPLETE |
| real bkmr | adapter + dedicated real 7.6.7 CI lane | COMPLETE on exact code candidate CI #420 |
| real GitNexus | adapter + dedicated 1.6.9 analyze/query/context/impact/trace/detect-changes/check lane | COMPLETE on exact code candidate CI #420 |
| federated ProjectMap | stable endpoints and reversible bindings | COMPLETE |
| one KnowledgeApplication across human/CLI/agent projections | core `KnowledgeApplication`; `aikit-tui/src/knowledge_service.rs` is a projection only | COMPLETE |
| real Wiki → Source traversal | Knowledge application contract tests plus integrated pre-local route traverse `wiki:node:auth` to `source:spec` | COMPLETE |
| KnowledgeRoute / ContextPack | canonical provider-neutral route/read/context contracts | COMPLETE |
| route → familiarity evidence | `KnowledgeRoute::familiarity_observation`; `familiarity_v2.rs` preserves route/provider/lens/revision evidence; integrated route records and assesses the route | COMPLETE |
| durable familiarity/history replay | `aikit-store/tests/familiarity.rs` reopens the database and replays observations/resets without changing canonical identity; shipped UI decorator records/replays through the real store | COMPLETE |
| familiarity has no authority over trust/eligibility/preference | `familiarity_v2.rs` | COMPLETE |
| optional QL/MEF | `ql_provider_v2.rs`: disabled/no-provider parity; present, degraded, incompatible and required-provider cases | COMPLETE |
| Explain | `ApplicationService::explain`; composition Explain exposes provider/scope/lifetime/surface evidence; Project-world Explain shows authored/effective state and revisions; integrated route asserts native Wiki explanation/source provenance | COMPLETE |
| History | Application history combines run/familiarity evidence; composition history diff covers mounts/retracts/rebinds/contributions/surfaces/body fingerprint; generation lineage exposed in Project world | COMPLETE |
| Compose project horizon | Project-world Compose exposes Profiles, Skill Sets, capabilities/actions, information, actor/runtime and projection horizons from one read model; Profiles/Skill Sets are advertised as first-class ResourceRecords in the same field | COMPLETE |
| runtime composition grammar | canonical Component / Contract / Requirement / Provider contribution / Surface / HarnessComposition model | COMPLETE for #60 minimum |
| action and non-action Surface projections | `composition_v2.rs` proves one Action across Surfaces without identity multiplication and non-Action Knowledge Surface behavior | COMPLETE |
| provider binding/degradation | required/optional requirements, deterministic substitution and explicit degraded state in composition/Knowledge tests | COMPLETE |
| independent resolution / activation / lifetime scopes | `composition_v2.rs` | COMPLETE |
| thin/static Harness | empty composition is valid | COMPLETE |
| composition-capable Harness and body identity/history | composition/body fingerprint and `composition_views_v2.rs` diff/Explain acceptance | COMPLETE for #60 minimum |
| staged Component/Surface/projection mutation | `composition_mutation.rs` mutates authored Component selections through the one resolver; `composition_mutation_v2.rs` proves stage → preview → confirm → desired resolved body and retraction | COMPLETE |
| richer DSH/Cordis maximal conformance | child #65 by explicit #53 handoff; extends the accepted grammar rather than defining the #60 minimum | PARALLEL — NOT A #60 BLOCKER |
| actor bootstrap | `actor_bootstrap_v2.rs`: stable Project/Agent/Agency/Harness identity, model/host/session provenance, thin body, body/session replacement | COMPLETE |
| durable mutation route | `StagedChanges` → preview/explain → separate confirm → apply; final surface acceptance drives the same route | COMPLETE |
| immutable Generation | core/store generation lifecycle and apply receipts; integrated route creates an immutable Generation through real `Service`/store application | COMPLETE |
| reversible Procedure | `aikit-store/tests/procedure.rs`: real diff, apply, undo, drift refusal, rollback on failure, idempotent replay | COMPLETE |
| truthful target lifecycle | projection/lifecycle contracts preserve actual activation effect; integrated runtime acceptance ends at honest `CompositionState::Resolved`, not invented physical/live materialisation | COMPLETE for cloud-testable contract |
| trust / eligibility / source ownership | canonical trust/source/resource models and provider/application evidence | COMPLETE |
| keyboard/mouse semantic parity | `mouse_context_v2.rs` routes both to the same semantic Actions/sections/presentation | COMPLETE |
| narrow/medium/wide terminal behavior | final-surface render acceptance at 48/88/140 columns plus project-world narrow/wide tests | COMPLETE |
| performance budgets | `performance.rs`: cold first frame <150ms, warm p95 <60ms, 5k-resource search p95 <16ms in release acceptance | COMPLETE as repository-owned budget |
| V1 semantic-controller deletion/classification | #59 closure ledger: old Palette service/surface/reducer/driver/form/search and mutable Tree controller removed; retained package/CLI read compatibility explicitly classified | COMPLETE on code candidate; #59 issue closure is the next procedural step |
| strict clippy/dead-code and per-crate CI | CI #420 / run `31980096724`: static/dead-code, clippy `-D warnings`, release build, diff hygiene and every V2 crate green | COMPLETE |
| original integration suite | CI #420 / run `31980096724`: real integration suite / repository `scripts/verify` green | COMPLETE |
| **single integrated #60 route** | `crates/aikit-cli/tests/v2_prelocal_product_acceptance.rs` (code-candidate blob `b853f9947232bb47c7f0fdd910e26b67f298dc01`) binds production `Service`/store Context+Generation, ResourceRef search, SemanticWiki/Source Explain+relations, KnowledgeRoute+familiarity, HarnessComposition stage/preview/confirm/apply, History/retraction/reuse and one canonical Action identity across TUI + AgentTool Surfaces | **COMPLETE** on CI #420 |

## Integrated #60 route: exact scope

The integrated lane is intentionally a **binding proof**, not a duplicate test architecture. It crosses production contracts for:

```text
Project
→ resolved Context
→ ResourceRef-native Search
→ SemanticWiki Node / Source
→ Explain with source provenance
→ real Wiki→Source relation traversal
→ KnowledgeRoute
→ familiarity observation/assessment
→ real Service apply → immutable Generation
→ HarnessComposition inspection
→ stage Component selection / Surface projections
→ preview
→ explicit confirm
→ apply accepted desired resolved body
→ composition History/diff
→ retract/recover prior body shape
→ reuse the same KnowledgeRoute
→ equivalent TUI + AgentTool Action projection with one canonical ResourceRef
```

It does **not** counterfeit physical target materialisation. `apply_confirmed_harness_composition` accepts the desired resolved composition body only; a target/provider must separately prove stronger live/material truth.

Dedicated exact-head lanes remain the correct evidence for dimensions that do not need to be redundantly reenacted inside this one test: OKF/QL variants, real bkmr, real GitNexus, provider absence/degradation, Procedure rollback, terminal widths/input parity/performance, and the full Project Compose horizon including Profile/Skill Set/ContextSource facets. Those lanes run under the same repository CI and share the same canonical models/services.

## Exact code-candidate verification

At `d22012a319c3e7937fe0a8b29fca9336433b292c`, GitHub Actions CI #420 / run `31980096724` completed **SUCCESS** with:

- real integration suite / repository `scripts/verify`;
- V2 static/dead-code gate;
- clippy `-D warnings` across all targets;
- release build;
- diff hygiene;
- `aikit-cli`, `aikit-core`, `aikit-store`, `aikit-adapters`, `aikit-tui`;
- real bkmr 7.6.7 SourcePool conformance;
- real GitNexus 1.6.9 ProjectMap conformance.

The code candidate also corrects the brief `2d1e0cbac7ef5b8c5d67015d4fb635e81d0cb567` regression which had restored retired Tree/Surface acceptance tests. The resting acceptance file is the migration-safe blob `9c8892fd00474d628ddc50ce84274c2783bc3d4c`; the retired controller imports are absent.

## Deliberate downstream/local boundaries

| Boundary | Status | Why it is not a cloud claim |
|---|---|---|
| actual O:I/Central/Workcell package installation | LOCAL | requires the assembled external package and user's installation |
| actual cross-product O:I/Central/Workcell integration | LOCAL after AI Kit cloud closure | external repository/package truth |
| physical host/process/network/tmux/cmux/GUI behavior not faithfully represented in CI | LOCAL | requires real host/process/network/terminal topology |
| Workcell-hosted material services | LOCAL / external | Workcell owns provider materialisation; AI Kit must not fabricate it |
| rich DSH/Cordis maximal-provider convergence | PARALLEL #65 | explicit child programme; consumes/extends the native grammar after its #60 minimum exists |
| SessionSpace #61–#63 | DOWNSTREAM | must consume this converged product after #60; it may not become a parallel resolver/state/store/composition architecture |

## Closure state

All cloud-achievable #60 obligations are substantively complete on code candidate `d22012a…`, and the exact code-candidate CI is green. The remaining actions are procedural and evidence-only:

1. commit this final #59/#60 evidence update without changing product code;
2. require that exact evidence-only head to rerun repository CI green;
3. close #59 first, recording migration/deletion evidence;
4. then close #60, recording the accepted local/downstream boundary.

After #60 closure, #61/#62 may consume the accepted architecture. Their handoff constraint is exact: SessionSpace must build on the one `ResourceRef` / `ApplicationService` / `ApplicationSurfaceController` / `TuiState` / `KnowledgeApplication` / `HarnessComposition` architecture and must not introduce a second resolver, semantic store, TUI controller/state, or composition grammar.
