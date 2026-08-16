# AI Kit V2 Pre-Local Acceptance Evidence

**Status:** live evidence matrix for issue #60  
**Rule:** only observed implementation/test/CI evidence is marked complete. `OPEN` means cloud-implementable work remains. `LOCAL` is reserved for a gate that genuinely requires the user's O:I/Central/Workcell/full-package installation or physical host/process/network/terminal environment.

## Current integration line

- PR: #58 — `feat(v2): converge the operational V2 integration line`
- Branch: `agent/aikit-v2-integration`
- PR remains draft until this matrix has no cloud-implementable `OPEN` rows.
- V2 product law: `docs/v2/README.md` through `docs/v2/10-PERSISTENT-AGENCY-AND-MATERIAL-HOSTING.md` on this integration line.

## Evidence matrix

| Requirement | Implementation | Test / evidence | Exact commit | Status | Genuine local/external blocker |
|---|---|---|---|---|---|
| canonical Resource identity | `resource/*`, `ResourceRef`, `ResourceRecord`, `ResourceIndex` | V2 core/resource tests | inherited on #58 | COMPLETE | — |
| ContextResolution application architecture | `context_resolution.rs`, projections, generation/lifecycle | core + integration tests | inherited on #58 | COMPLETE for current core; integrated #60 path still OPEN | — |
| one TUI semantic state/reducer | `aikit-tui/src/application.rs`, `TuiState`, `TuiRuntime` | reducer + `v2_prelocal_acceptance.rs` | inherited on #58; stale test repaired `83f9d591…` | OPEN | Palette/Tree transitional implementations still need closure under #59 |
| ResourceRef-native Quick/Workspace search | `resource/search.rs`, V2 application projection | TUI/CLI V2 tests | inherited on #58 | COMPLETE for shallow navigation | — |
| contextual Actions | canonical Action resources + `ContextualActionDescriptor` | TUI action-search/staging acceptance | inherited on #58 | COMPLETE for current capability actions; multi-Surface runtime projection OPEN | — |
| SemanticWiki native operation | OKF core/index/provider/application | core tests | `8f9d84f…` | COMPLETE | — |
| SourcePool native provider | `knowledge_source_pool.rs` | core tests | prior #58 ancestry | COMPLETE | — |
| real bkmr adapter | `aikit-adapters/src/bkmr.rs` | dedicated exact 7.6.7 CI lane | `992f232…`, repairs `ef02d4d5…` | COMPLETE — observed green on #58 CI after `83f9d591…` | — |
| Git/source canonical CodeReference | `knowledge_code.rs` | core/adapter tests | prior #58 ancestry | COMPLETE | — |
| real GitNexus | `aikit-adapters/src/gitnexus.rs` | exact GitNexus 1.6.9 CI: analyze/query/context/impact/trace/detect-changes/check | `78ec14b…`, contract repair `83f9d591…` | COMPLETE — observed green after `83f9d591…` | — |
| federated ProjectMap | `project_map.rs` stable endpoints/reversible bindings | core federation tests | `609ae9e…` | COMPLETE as core primitive | — |
| KnowledgeApplication federation | `knowledge_navigation.rs` over Wiki/Source/code/ProjectMap | core tests including bound cross-lens route and rejection of arbitrary jumps | `119d3853bc2ed33c58eb841e3a14e0e3723745e3` | IMPLEMENTED; CI verification pending | — |
| Knowledge provider absence/degradation | provider status/absences + no silent identity fallback | core/adapter tests | prior #58 + `119d3853…` | IMPLEMENTED; full integrated acceptance OPEN | — |
| KnowledgeRoute / ContextPack | core route/read/context projections | core tests | prior #58; strengthened `119d3853…` | IMPLEMENTED; persistence/reuse + shipped-surface acceptance OPEN | — |
| familiarity evidence | `familiarity.rs`; route observation shape | familiarity tests + route test | prior #58 | OPEN | route history/reuse and real surface ingestion must be proven end-to-end |
| optional QL/MEF | provider-optional design and carried QL provider work | existing QL tests | inherited on #58 | OPEN | #60 requires paired no-provider/present-provider integrated acceptance |
| Explain | application/TUI explanation projections | current unit/TUI tests | inherited on #58 | OPEN | must cover provider absence, authored/effective, scopes, lifetimes, contributions, surfaces, familiarity, staged effects and target lifecycle in integrated flow |
| History: Recent/Familiar/Changed/Generations/world/body/routes | current history/familiarity/generation pieces | partial tests | inherited on #58 | OPEN | one canonical history projection + restore/reuse acceptance incomplete |
| Compose Profiles/Skill Sets/Skills/Capabilities/Actions/ContextSources | current resource/profile/context/staging services | partial tests | inherited on #58 | OPEN | full final Compose field and shared mutation path incomplete |
| Compose Agent/Agency/Model/Harness/Host/HarnessComposition/Components/Contracts/Surfaces/targets | composition + bootstrap modules | partial core/adapter tests | prior #58 ancestry | OPEN | cloud frontier in #53/#27 remains to close |
| durable mutation `stage → preview/explain → confirm → apply` | reducer/application mutation path | `v2_prelocal_acceptance.rs` | inherited + `83f9d591…` | IMPLEMENTED for capability composition; OPEN for full component/surface/projection path | — |
| immutable Generation | generation/store lifecycle | core/store tests | inherited on #58 | COMPLETE at subsystem level; integrated #60 path OPEN | — |
| reversible Procedure | `procedure.rs` | core tests | inherited on #58 | OPEN | integrated rollback/reuse proof required |
| target lifecycle truth | projection/lifecycle + Harness adapters | adapter/core tests | inherited on #58 | OPEN | #53 multi-effect acceptance still required; DeepSeek must remain honest `NextSession` until control exists |
| thin/static Harness | existing harness adapters | adapter tests | inherited on #58 | IMPLEMENTED; #60 integrated proof OPEN | — |
| composition-capable Harness | `composition.rs`, `composition_view.rs`, DeepSeek fixture | core/adapter tests | prior #58 ancestry | OPEN | body history/diff, full surfaces, material seam acceptance incomplete |
| actor bootstrap | `actor_bootstrap.rs` | core tests / #27 | prior #58 ancestry | OPEN | re-audit live #27 cloud acceptance and host restoration |
| verification/evolution | verification/run/evolution modules | partial tests / #31 | prior #58 ancestry | OPEN | re-audit all cloud-completable child acceptance |
| production Knowledge composition used by TUI/CLI/agent | core app + TUI `knowledge_service.rs` projection | no complete shipped-surface acceptance yet | — | OPEN | none; cloud-implementable |
| one relation state → List/Tree/Graph projections | V2 relation model + transitional Tree | partial tests | inherited | OPEN | replace/delete Tree-specific semantic paths; add stable-selection/recenter/provenance acceptance |
| keyboard/mouse semantic parity | reducer/input modules | partial TUI tests | inherited | OPEN | hosted semantic parity is cloud-testable; only final physical-terminal feel belongs local |
| terminal narrow/medium/wide | V2 renderer | partial render tests | inherited | OPEN | cloud-testable dimensions before local visual acceptance |
| performance budgets | search/render paths | existing benchmark/latency tests where present | inherited | OPEN | run/complete #60 budget evidence |
| trust / eligibility / source ownership | trust + source policy models | core/adapter tests | inherited | IMPLEMENTED; integrated path OPEN | — |
| provider degradation | SourcePool/Code/Knowledge status | core/adapter tests | prior + `119d3853…` | IMPLEMENTED; integrated path OPEN | — |
| V1 semantic-controller deletion | #59 ledger | current repository still has replacement rows | this document + closure ledger | OPEN | none; mandatory cloud work |
| strict clippy/dead-code | CI `V2 static contract — clippy + dead-code guards` | initial failures repaired `83f9d591…`, `ef02d4d5…` | latest | PENDING latest CI | — |
| aikit-core CI | granular job | green after `83f9d591…`; new Knowledge commit pending | latest | PENDING latest CI | — |
| aikit-store CI | granular job | observed green after `83f9d591…` | latest | COMPLETE for observed head; rerun pending latest | — |
| aikit-adapters CI | granular job | observed green after `83f9d591…` | latest | COMPLETE for observed head; rerun pending latest | — |
| aikit-tui CI | granular job | stale acceptance compile repaired `ef02d4d5…` | latest | PENDING latest CI | — |
| aikit-cli CI | granular job | observed green after `83f9d591…` | latest | COMPLETE for observed head; rerun pending latest | — |
| original macOS integration suite | existing workflow | prior failure reduced to same stale TUI compile error, repaired `ef02d4d5…` | latest | PENDING latest CI | — |
| full integrated #60 acceptance scenario | not yet a single acceptance lane | required sequence from Project through restore/reuse and agent equivalent | — | OPEN | none; mandatory cloud work |
| final local O:I/Central/Workcell package installation | outside this repo's hosted execution truth | physical package acceptance | — | LOCAL | requires actual user's full O:I package/machine |
| actual Central/O:I/Workcell cross-product integration | external repositories/package | full-system acceptance | — | LOCAL after AI Kit cloud closure | requires converged external products/package |
| physical host/process/network/tmux/cmux/GUI behavior not faithfully emulatable in CI | material environment | local acceptance | — | LOCAL | requires user's host(s), terminals, process/network topology |
| Workcell-hosted material services | Workcell-owned provider materialisation | local/full-package acceptance | — | LOCAL | intentionally not implemented inside AI Kit |

## Required single #60 acceptance route

This row stays `OPEN` until one repository-owned test/lane proves the complete chain with no alternate V1 semantic path:

```text
Project
→ Context
→ universal Search
→ SemanticWiki Node / Source / code ref
→ Explain
→ real relation traversal
→ KnowledgeRoute
→ familiarity
→ Compose Profile / Skill Set / ContextSource
→ actor/runtime composition
→ inspect HarnessComposition
→ stage Component/Surface/projection change
→ preview
→ confirm
→ apply
→ new immutable Generation / honest target state
→ Explain + History
→ restore/reuse previous Project world / body / route
→ equivalent agent-facing operation
```

The same protocol must include paired no-QL and optional-QL runs; native SourcePool; real bkmr 7.6.7; current tested GitNexus; thin and composition-capable Harnesses; terminal width/input semantic tests; cloud-testable host restoration; performance; Procedure rollback; trust/eligibility/ownership; provider absence/degradation; and #59 dead-code proof.

## Current cloud closure statement

**Cloud closure is not yet complete.** The remaining `OPEN` rows are implementation work, not local blockers. In particular: production Knowledge instantiation across real surfaces, final TUI/Compose/Relations/Explain/History, runtime composition cloud frontier, actor/verification re-audit, legacy deletion, and the single integrated #60 acceptance lane still have to be completed before this matrix can reduce to `LOCAL` gates only.
