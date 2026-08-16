# AI Kit V2 Pre-Local Acceptance Evidence

**Status:** final cloud-closure verification for issue #60  
**Rule:** `COMPLETE` means the implementation and repository-owned evidence already exist on the V2 integration line. `OPEN` means a cloud-achievable acceptance obligation still remains. `LOCAL` is reserved for truth that requires the actual O:I/Central/Workcell package or physical host/process/network/terminal environment.

## Current integration line

- PR #58 — `feat(v2): converge the operational V2 integration line`
- branch `agent/aikit-v2-integration`
- V2 product law: `docs/v2/README.md` through `docs/v2/10-PERSISTENT-AGENCY-AND-MATERIAL-HOSTING.md`
- migration/legacy classification: `docs/v2/11-MIGRATION-CLOSURE-LEDGER.md`
- PR remains draft until every cloud-achievable gate below is complete and exact-head CI is green.

## Cloud-achievable evidence

| Requirement | Canonical implementation / evidence | Status |
|---|---|---|
| one Resource identity | `ResourceRef` / `ResourceRecord` / `ResourceIndex`; selection and relation views preserve the same ref | COMPLETE |
| one ContextResolution/application architecture | `context_resolution.rs`, project-world application service, `ApplicationService`; renderers do not resolve | COMPLETE |
| one TUI state/reducer | `application.rs` `TuiState` + `TuiRuntime`; final `ApplicationSurfaceController` only | COMPLETE |
| canonical Search / Context / Compose / Knowledge / Explain / History field | `project_workspace_render.rs`; `application_surface_prelocal_v2.rs` | COMPLETE |
| explicit selection rather than query-owned cursor identity | reducer keeps search and `Select(ResourceRef)` distinct; final-surface tests explicitly select before actions/staging | COMPLETE at `f87d4bf…`, `d4c6c21…`, `43a9115…`, `ef52b71…` |
| universal ResourceRef-native search | `resource/search.rs` + `ApplicationService::search` | COMPLETE |
| contextual Actions | canonical Action resources / descriptors; text action lane and explicit stageability | COMPLETE |
| one relation state → List / Tree / Graph | `RelationReadModel` + `RelationView`; mutable Tree controller deleted; final-surface identity test | COMPLETE |
| SemanticWiki native provider | OKF objects/index/provider/application | COMPLETE |
| native SourcePool | `knowledge_source_pool.rs`; Knowledge application tests | COMPLETE |
| Git/source CodeReference | canonical code-reference provider model | COMPLETE |
| real bkmr | adapter + dedicated real 7.6.7 CI lane | COMPLETE on previously observed PR-head CI; exact final head still required |
| real GitNexus | adapter + dedicated 1.6.9 analyze/query/context/impact/trace/detect-changes/check lane | COMPLETE on previously observed PR-head CI; exact final head still required |
| federated ProjectMap | stable endpoints and reversible bindings | COMPLETE |
| one KnowledgeApplication across human/CLI/agent projections | core `KnowledgeApplication`; `aikit-tui/src/knowledge_service.rs` is a projection only | COMPLETE as architecture |
| real Wiki → Source traversal | `knowledge_service.rs` contract test searches/reads Wiki resource and traverses to `source:spec` | COMPLETE |
| KnowledgeRoute / ContextPack | canonical provider-neutral route/read/context contracts | COMPLETE |
| route → familiarity evidence | `KnowledgeRoute::familiarity_observation`; `familiarity_v2.rs` preserves route/provider/lens/revision evidence | COMPLETE |
| durable familiarity/history replay | `aikit-store/tests/familiarity.rs` reopens the database and replays observations/resets without changing canonical identity | COMPLETE |
| familiarity has no authority over trust/eligibility/preference | `familiarity_v2.rs` | COMPLETE |
| optional QL/MEF | `ql_provider_v2.rs`: disabled/no-provider parity; present, degraded, incompatible and required-provider cases | COMPLETE |
| Explain | `ApplicationService::explain`; composition Explain exposes provider/scope/lifetime/surface evidence; Project-world Explain shows authored/effective state and revisions | COMPLETE at subsystem/application level |
| History | Application history combines run/familiarity evidence; composition history diff covers mounts/retracts/rebinds/contributions/surfaces/body fingerprint; generation lineage exposed in Project world | COMPLETE at subsystem/application level |
| Compose project horizon | Project-world Compose exposes capabilities/actions, information, actor/runtime and projection horizons from one read model | COMPLETE |
| runtime composition grammar | canonical Component / Contract / Requirement / Provider contribution / Surface / HarnessComposition model | COMPLETE for #60 minimum |
| action and non-action Surface projections | `composition_v2.rs` proves one Action across Surfaces without identity multiplication and non-Action Knowledge Surface behavior | COMPLETE |
| provider binding/degradation | required/optional requirements, deterministic substitution and explicit degraded state in composition/Knowledge tests | COMPLETE |
| independent resolution / activation / lifetime scopes | `composition_v2.rs` | COMPLETE |
| thin/static Harness | empty composition is valid | COMPLETE |
| composition-capable Harness and body identity/history | composition/body fingerprint and `composition_views_v2.rs` diff/Explain acceptance | COMPLETE for #60 minimum |
| richer DSH/Cordis maximal conformance | child #65 by explicit #53 handoff; it extends the grammar rather than defining the #60 minimum | NOT A #60 BLOCKER |
| actor bootstrap | `actor_bootstrap_v2.rs`: stable Project/Agent/Agency/Harness identity, model/host/session provenance, thin body, body/session replacement | COMPLETE |
| durable mutation route | `StagedChanges` → preview/explain → separate confirm → apply; final surface acceptance drives the same route | COMPLETE |
| immutable Generation | core/store generation lifecycle and apply receipts | COMPLETE at subsystem level |
| reversible Procedure | `aikit-store/tests/procedure.rs`: real diff, apply, undo, drift refusal, rollback on failure, idempotent replay | COMPLETE |
| truthful target lifecycle | projection/lifecycle contracts preserve actual activation effect; no invented immediate success | COMPLETE for cloud-testable contract |
| trust / eligibility / source ownership | canonical trust/source/resource models and provider/application evidence | COMPLETE |
| keyboard/mouse semantic parity | `mouse_context_v2.rs` routes both to the same semantic Actions/sections/presentation | COMPLETE |
| narrow/medium/wide terminal behavior | final-surface render acceptance at 48/88/140 columns plus project-world narrow/wide tests | COMPLETE |
| performance budgets | `performance.rs`: cold first frame <150ms, warm p95 <60ms, 5k-resource search p95 <16ms in release acceptance | COMPLETE as repository-owned budget |
| V1 semantic-controller deletion/classification | #59 closure ledger: old Palette service/surface/reducer/driver/form/search and mutable Tree controller removed; retained package/CLI read compatibility explicitly classified | PENDING #59 exact-head closure |
| strict clippy/dead-code and per-crate CI | repository CI | PENDING exact final head |
| original integration suite | repository CI | PENDING exact final head |
| **single integrated #60 route** | one repository-owned acceptance lane must bind the already-complete subsystems below into the mandated Project→Knowledge→Compose/runtime→apply→History/reuse→agent-equivalent chain | **OPEN** |

## Required single #60 acceptance route

Subsystem coverage is no longer the blocker. The remaining cloud obligation is a **coherent integration acceptance**, not another architecture. One repository-owned lane must prove that the same canonical identities/services can be carried through:

```text
Project
→ Context
→ universal Search
→ SemanticWiki Node / Source / code ref
→ Explain
→ real relation traversal
→ KnowledgeRoute
→ durable familiarity
→ Compose Profile / Skill Set / ContextSource
→ actor/runtime composition
→ inspect HarnessComposition
→ stage Component/Surface/projection change
→ preview / explain
→ confirm
→ apply
→ immutable Generation / honest target state
→ Explain + History
→ restore/reuse prior Project world / body / route
→ equivalent agent-facing projection/operation
```

The lane may reuse the production fixtures and APIs already tested above; it must not create a test-only resolver, TUI-local store, alternate staging path or fake material success. Paired optional-QL/no-QL behavior, real bkmr/GitNexus, terminal/performance and rollback evidence may remain dedicated lanes if the integrated acceptance references the same contracts and exact-head CI executes all of them.

## Deliberate downstream/local boundaries

| Boundary | Status | Why it is not a cloud claim |
|---|---|---|
| actual O:I/Central/Workcell package installation | LOCAL | requires the assembled external package and user's installation |
| actual cross-product O:I/Central/Workcell integration | LOCAL after AI Kit cloud closure | external repository/package truth |
| physical host/process/network/tmux/cmux/GUI behavior not faithfully represented in CI | LOCAL | requires real host/process/network/terminal topology |
| Workcell-hosted material services | LOCAL / external | Workcell owns provider materialisation; AI Kit must not fabricate it |
| rich DSH/Cordis maximal-provider convergence | PARALLEL #65 | explicit child programme; consumes/extents the native grammar after its #60 minimum exists |
| SessionSpace #61–#63 | DOWNSTREAM | must consume this converged product after #60; it is not permitted to become a parallel composition architecture |

## Current closure statement

Cloud closure is **not yet complete**. The stale matrix previously made many already-implemented subsystems look open; that has now been corrected. The remaining legitimate blockers are:

1. close #59 on an exact green head, including the two stale deleted-controller CLI acceptance tests and the final core-search compatibility audit;
2. add the single integrated #60 acceptance lane above;
3. obtain green exact-head repository CI (static/dead-code, crates, real provider lanes and integration suite).

Once those three items are complete, this ledger should contain only `LOCAL`, downstream, or explicitly parallel boundaries and #60 may close without waiting for SessionSpace or #65.
