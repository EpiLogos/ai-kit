# AI Kit V2 Migration Closure Ledger

**Status:** closure verification in progress  
**Owner:** V2 integration line / issue #59  
**Rule:** every V1/transitional item has exactly one terminal classification: `DELETE`, `REPLACE`, `EXTERNAL-COMPATIBILITY`, or `STILL-CANONICAL`. `COMPLETE` means the resting branch no longer depends on the retired semantic owner; CI evidence is tracked separately below.

## Classification semantics

- **DELETE** — obsolete implementation/test/document; no resting product owner remains.
- **REPLACE** — useful behavior migrated to the named V2 owner before the old semantic owner was removed.
- **EXTERNAL-COMPATIBILITY** — retained only at an explicit outer/public compatibility boundary; it may translate to canonical V2 state but may not own resolution, identity, staging, mutation, or product navigation semantics.
- **STILL-CANONICAL** — legitimate V2 primitive/substrate, not a second semantic controller.

## Core

| Item | Classification | Resting owner / boundary | Evidence | Status |
|---|---|---|---|---|
| `capsule.rs` package/source model | STILL-CANONICAL | package/import/runtime payload substrate only; application identity is `ResourceRef`/`ResourceRecord` | canonical `resource/*`; `ApplicationService` only converts a proven package-backed `ResourceKind::Capability` to `CapsuleId` | COMPLETE |
| `resource/legacy.rs` Capsule→Resource shim | DELETE | source/package ingestion is no longer a canonical core-wide legacy API | `b38080e1a1a1e8fd007e6d6dcbfe0b7509411d02` physically deletes the file | COMPLETE |
| generic V2 Resource→Capsule fallback | DELETE | only proven package-backed Capability translation is permitted | `application_service.rs` checks Resource index kind and backend package ownership before translation | COMPLETE |
| `resource/search.rs` | STILL-CANONICAL | universal ResourceRef-native application search / contextual Actions / explainable familiarity ranking | `ResourceSearchIndex`, V2 application tests | COMPLETE |
| `search.rs` Capsule query/ranking model | EXTERNAL-COMPATIBILITY | package/CLI compatibility only; it is not consulted by canonical V2 navigation | `backend.rs::navigation_index` projects ResourceRecords directly from the resolved catalogue and explicitly does not consume `SearchDoc`; `22e409d67b3550d6c6d73d2fa5c9aae3c71e7274` proves legacy usage/SearchDoc evidence cannot manufacture V2 navigation identity | COMPLETE |
| `ResolvedView` | STILL-CANONICAL | package activation/resolution substrate behind `ContextResolution`; not application identity | `ContextResolution` and Resource projections preserve ResourceRef identity above it | COMPLETE |
| `ContextResolution`, Resource model, Generation, Procedure/lifecycle primitives | STILL-CANONICAL | canonical semantic/mutation substrate | V2 core/application tests | COMPLETE |
| `HarnessComposition` grammar and staged runtime mutation | STILL-CANONICAL | one Component/Contract/Provider/Contribution/Surface grammar and one resolver; runtime staging mutates authored Component selections only | `composition_mutation.rs` reuses `resolve_harness_composition` and `diff_harness_compositions`; `composition_mutation_v2.rs` proves staged mount/retract, explicit confirm, stable ResourceRef projections and no invented live state | COMPLETE |
| Knowledge / SemanticWiki / SourcePool / CodeReference / ProjectMap primitives | STILL-CANONICAL | federated provider-native Knowledge application | provider and V2 Knowledge acceptance | COMPLETE for migration classification; product acceptance remains in #60 |

## TUI / terminal surface

| Item | Classification | V2 owner / boundary | Evidence | Status |
|---|---|---|---|---|
| `palette_service.rs` | REPLACE | canonical `ApplicationService` | deleted at `70080072ae7bd6493e45c29bf3e15a6e45981013`; V2 service tests migrated | COMPLETE |
| old `surface.rs` controller | REPLACE | `ApplicationSurfaceController` over one `TuiState`/`TuiRuntime` | deleted at `4d3f6fc8b00b62b7d03331ad2846d9286b510186`; final-surface acceptance exists | COMPLETE |
| old Palette reducer `app.rs` | REPLACE | `application.rs` pure V2 reducer/state | `20ca843fd72a4b5046f8de271d4f1cc8708014ef` deletes old reducer | COMPLETE |
| old Palette driver | REPLACE | final surface event loop → V2 `UiAction` reducer | `914ea1c39bdf…` deletion tranche | COMPLETE |
| old Palette form controller | REPLACE | contextual Action/application-service invocation; no second form-state controller | `d8d8144f…` deletion tranche | COMPLETE |
| old Palette-local search | REPLACE | `ApplicationService::search` / `ResourceSearchIndex` | `abf9bb5aae63f21c6e08bfbe9e6e6a3788486595` deletes `aikit-tui/src/search.rs` | COMPLETE |
| `tree_driver.rs` mutable Tree controller | REPLACE | one `TuiState` relation state projected as List/Tree/Graph | controller deleted at `f003e188ee8c920cb0d7f0fcfd207a661e84ba0a`; controller tests deleted at `4ba2d1d7c3869f58c7d7917cd5915fa336842072` | COMPLETE |
| `tree.rs` mutable staging/reducer semantics | REPLACE | `TuiState`/`TuiRuntime`; relation projections in final surface | `eaf3e97800c297ccc70e8e4e2f2c006963389f23` trims mutation/reducer code and tests | COMPLETE |
| `tree.rs` read-only hierarchy used by `aikit tree` | EXTERNAL-COMPATIBILITY | published human CLI read view only; no resolver, staging, mutation or V2 navigation authority | `tree_build.rs` builds it for `aikit tree`; mutable controller is gone | COMPLETE |
| old TUI staging semantic owner (`StagedSet`/Tree staging) | REPLACE | canonical `application::StagedChanges` → preview → confirmation → apply for the shipped V2 terminal surface | final reducer/surface acceptance proves the route; old mutable Tree staging is gone | COMPLETE |
| `aikit diff` package preview adapter | EXTERNAL-COMPATIBILITY | read-only package toggle preview over the same immutable backend resolver; cannot apply and cannot accept generic V2 Resources | `4fd10ea22f7ddf1bdde4e23041cf23af59b55fb4` restores only `StagedSet`/`StagedDiff`/`stage` compatibility around `PaletteBackend::preview` and documents canonical V2 staging ownership | COMPLETE |
| test-common helpers driving deleted Palette reducer/driver | DELETE | V2 surface/application fixtures | `95b55c8a8562049c8e4432dc010aafc65362d7ea` removes `AppState`/driver helpers | COMPLETE |
| old Palette staging tests | DELETE | final-surface/reducer staging acceptance | `c6e00e3e258d0f961de174fa5b76cfe127cf21a4` deletes retired staging test | COMPLETE |
| `application.rs` + `ApplicationService` + `ApplicationSurfaceController` | STILL-CANONICAL | one semantic UI/application authority | `v2_prelocal_acceptance.rs`, `application_surface_prelocal_v2.rs`, navigation/mouse/performance tests | COMPLETE |
| `project_world_api.rs` old Palette-service binding | REPLACE | same canonical `ApplicationService` used by Search/Actions/composition | `ee8d9f1add91937293479996df20589a4ab69926` | COMPLETE |

## CLI / agent-facing boundaries

| Item | Classification | Resting owner / boundary | Evidence | Status |
|---|---|---|---|---|
| shipped `aikit ui` and implicit interactive entry | REPLACE | final `ApplicationSurfaceController` | `crates/aikit-cli/src/ui.rs` delegates to final V2 surface | COMPLETE |
| `aikit tree` / `tree_build.rs` | EXTERNAL-COMPATIBILITY | read-only public hierarchy; must not become a second application state or mutation path | mutable Tree controller deleted; command only builds/renders read model | COMPLETE |
| `PaletteBackend` type name | EXTERNAL-COMPATIBILITY | low-level package/runtime backend contract under `ApplicationService`; name is historical, semantics are not a Palette controller | final surface/service and tests use the same backend object; no Palette reducer/driver/service remains | COMPLETE — naming cleanup optional, not a second architecture |
| `palette_run_intent.rs` test name | EXTERNAL-COMPATIBILITY | protects real run-intent semantics (mode/cwd/env), not Palette state | run intent is consumed by production backend/runtime | COMPLETE — rename optional |
| `AikitApplication::stage` / `aikit diff` | EXTERNAL-COMPATIBILITY | public package preview verb translated through the read-only adapter above | `4fd10ea22f7ddf1bdde4e23041cf23af59b55fb4`; core/TUI compile green on later head `f638125…` | COMPLETE |
| Capsule-shaped CLI registry/package commands | EXTERNAL-COMPATIBILITY | explicit package/registry CLI vocabulary at the outer boundary; V2 application/TUI identity remains ResourceRef | package operations remain source/runtime operations rather than generic Resource identity | COMPLETE for #59 boundary classification |
| agent-facing canonical Resource/Action operations | STILL-CANONICAL | same Resource/application/composition services; target-native Surface identity never replaces canonical identity | runtime composition acceptance projects one Action to TUI and AgentTool Surfaces with one ResourceRef | COMPLETE for migration classification; integrated #60 route remains separate |

## Tests / docs / provider boundaries

| Item | Classification | Evidence | Status |
|---|---|---|---|
| tests whose only premise was Palette↔Tree semantic switching / Tree-owned staging / Palette-owned apply | DELETE | old controller/staging test deletions plus final-surface replacements | **OPEN — two stale tests remain in `crates/aikit-cli/tests/acceptance.rs` and are the only exact-head CLI/static compile blockers** |
| test-only imports keeping retired app/driver/form controllers compiled | DELETE | most removed at `95b55c8…`; exact-head CI identifies two remaining imports at acceptance.rs around lines 1744 and 1790 | OPEN — delete/migrate those two tests; do not restore retired modules |
| generated GitNexus wiki treated as authored SemanticWiki | DELETE | prohibited by Knowledge authority design/tests | COMPLETE |
| universal copied AI Kit knowledge graph | DELETE | federation architecture keeps provider-native authority | COMPLETE |
| separate CLI/TUI/agent Knowledge stores | DELETE | canonical Knowledge composition is shared; no TUI-local semantic store | COMPLETE for migration classification |
| physical/local provider truth | EXTERNAL-COMPATIBILITY | deliberately outside cloud migration closure; tracked by #60 local boundary | COMPLETE classification only |

## Exact-head verification state

At `f638125a64fbb158205e4aebe0c43559faf3b36f`:

- `aikit-core` — green;
- `aikit-store` — green;
- `aikit-tui` — green;
- `aikit-adapters` — green;
- real bkmr provider lane — green;
- real GitNexus provider lane — green;
- `aikit-cli`, V2 static/dead-code and umbrella verify — blocked by the same two stale deleted-controller tests in the monolithic CLI acceptance file.

The failure is not production-code linkage: compiler errors are unresolved imports of the intentionally deleted `aikit_tui::tree_driver` and `aikit_tui::surface` from those two tests.

## Current closure blockers

The migration inventory is otherwise terminally classified. The only #59 blocker is mechanical test cleanup plus the resulting exact-head rerun:

1. remove/migrate `the_interactive_tree_host_accepts_mouse_navigation_and_applies_staged_ids`;
2. remove/migrate `a_real_skillset_failure_stays_inside_the_resident_tree_surface`;
3. rerun exact-head CI and require CLI/static/integration green.

No retired controller should be reintroduced to satisfy those tests.

## Closure rule

#59 may close when:

- the two stale tests above no longer import deleted controllers;
- exact-head CI is green for the cloud-achievable repository gates;
- no `DELETE` or `REPLACE` row remains `OPEN`;
- every retained compatibility surface is explicitly read-only/package/runtime/external and cannot own V2 identity, resolution, staging, mutation, or application navigation semantics.
