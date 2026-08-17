# AI Kit V2 Migration Closure Ledger

**Status:** cloud migration closure complete on code candidate `d22012a319c3e7937fe0a8b29fca9336433b292c`; final evidence-only head still requires exact-head CI  
**Owner:** V2 integration line / issue #59  
**Rule:** every V1/transitional item has exactly one terminal classification: `DELETE`, `REPLACE`, `EXTERNAL-COMPATIBILITY`, or `STILL-CANONICAL`. `COMPLETE` means the resting branch no longer depends on the retired semantic owner.

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
| Knowledge / SemanticWiki / SourcePool / CodeReference / ProjectMap primitives | STILL-CANONICAL | federated provider-native Knowledge application | provider and V2 Knowledge acceptance | COMPLETE for migration classification; product acceptance is recorded in #60 |

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
| `AikitApplication::stage` / `aikit diff` | EXTERNAL-COMPATIBILITY | public package preview verb translated through the read-only adapter above | `4fd10ea22f7ddf1bdde4e23041cf23af59b55fb4`; exact candidate CI #420 is green | COMPLETE |
| Capsule-shaped CLI registry/package commands | EXTERNAL-COMPATIBILITY | explicit package/registry CLI vocabulary at the outer boundary; V2 application/TUI identity remains ResourceRef | package operations remain source/runtime operations rather than generic Resource identity | COMPLETE for #59 boundary classification |
| agent-facing canonical Resource/Action operations | STILL-CANONICAL | same Resource/application/composition services; target-native Surface identity never replaces canonical identity | runtime composition acceptance projects one Action to TUI and AgentTool Surfaces with one ResourceRef | COMPLETE |

## Tests / docs / provider boundaries

| Item | Classification | Evidence | Status |
|---|---|---|---|
| tests whose only premise was Palette↔Tree semantic switching / Tree-owned staging / Palette-owned apply | DELETE | retired controller tests removed; replacement coverage is `application_surface_prelocal_v2.rs`, `mouse_context_v2.rs`, and the integrated pre-local route | COMPLETE |
| test-only imports keeping retired app/driver/form/Tree/Surface controllers compiled | DELETE | `95b55c8…` removed test-common imports; final `acceptance.rs` blob `9c8892fd00474d628ddc50ce84274c2783bc3d4c` contains no `tree_driver`/old `surface` imports | COMPLETE |
| generated GitNexus wiki treated as authored SemanticWiki | DELETE | prohibited by Knowledge authority design/tests | COMPLETE |
| universal copied AI Kit knowledge graph | DELETE | federation architecture keeps provider-native authority | COMPLETE |
| separate CLI/TUI/agent Knowledge stores | DELETE | canonical Knowledge composition is shared; no TUI-local semantic store | COMPLETE |
| physical/local provider truth | EXTERNAL-COMPATIBILITY | deliberately outside cloud migration closure; tracked by #60 local boundary | COMPLETE classification only |

## Exact code-candidate verification

At `d22012a319c3e7937fe0a8b29fca9336433b292c`, GitHub Actions CI #420 / run `31980096724` completed **SUCCESS**:

- `aikit-core` — green;
- `aikit-store` — green;
- `aikit-tui` — green;
- `aikit-adapters` — green;
- `aikit-cli` — green;
- real bkmr 7.6.7 SourcePool conformance — green;
- real GitNexus 1.6.9 ProjectMap conformance — green;
- V2 static/dead-code gate — green;
- clippy `-D warnings` — green;
- release build — green;
- diff hygiene — green;
- real integration suite / repository `scripts/verify` — green.

The branch briefly received `2d1e0cbac7ef5b8c5d67015d4fb635e81d0cb567`, which restored retired Tree/Surface acceptance tests. `d22012a…` explicitly corrected that regression by restoring only the migration-safe, already-green acceptance blobs. The resting candidate does **not** reintroduce the retired controllers.

## Closure state

There are no remaining cloud-achievable `DELETE` or `REPLACE` rows and no unclassified migration semantic owner. Deliberate retained compatibility is limited to explicit package/runtime/read-only boundaries (`aikit tree`, `aikit diff`, Capsule-shaped package/registry commands and the historical low-level `PaletteBackend` name); none owns canonical V2 identity, resolution, staging, mutation, or product navigation.

The only action after this ledger update is procedural evidence: require the evidence-only final head to rerun repository CI green, then record that exact head in issue #59 and close the issue. No production-code blocker remains.

## Closure rule

#59 may close when the evidence-only final head is green. The code candidate already satisfies the substantive conditions:

- retired-controller tests/imports are absent from the resting code;
- all cloud-achievable repository gates are green;
- no `DELETE` or `REPLACE` row remains `OPEN`;
- every retained compatibility surface is explicitly read-only/package/runtime/external and cannot own V2 identity, resolution, staging, mutation, or application navigation semantics.
