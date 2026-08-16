# AI Kit V2 Migration Closure Ledger

**Status:** active closure ledger  
**Owner:** V2 integration line / issue #59  
**Rule:** every V1 or transitional item is assigned exactly one terminal classification: `DELETE`, `REPLACE`, `EXTERNAL-COMPATIBILITY`, or `STILL-CANONICAL`. A classification is not completion; `status` records whether the migration/deletion is actually finished.

## Classification semantics

- **DELETE** — the behavior is obsolete or its acceptance has moved to V2; the named implementation/test/document must leave the resting product.
- **REPLACE** — useful behavior remains, but the named implementation is not canonical. V2 must first own and prove the behavior, then this item is removed.
- **EXTERNAL-COMPATIBILITY** — retained only because a deliberate external consumer still speaks this shape. It may translate at an outer boundary but may not own internal semantics.
- **STILL-CANONICAL** — the thing remains a legitimate V2 primitive or substrate and is not a second semantic controller.

## Core

| Item | Classification | Behavior worth preserving | V2 owner / replacement | Evidence | Status |
|---|---|---|---|---|---|
| `crates/aikit-core/src/capsule.rs` | STILL-CANONICAL | Source/package envelope for existing skill/script/hook/guidance/alias/session/tool/template payloads, including manifest validation | `ResourceRecord` owns application identity/search/resolution; Capsule is packaging/import material only | `resource/legacy.rs` explicitly converts package evidence into `ResourceKind::Capability`; V2 Resource tests | OPEN — audit all callers and remove any Capsule-owned application semantics |
| `crates/aikit-core/src/resource/legacy.rs` | REPLACE | Lossless ingestion of existing Capsule packages into stable V2 Resource identity/provenance | Source/package adapter at catalog boundary; no core-wide legacy conversion path | current `TryFrom<&Capsule> for ResourceRecord` | OPEN — move/contain at compatibility boundary, then remove `resource::legacy` from canonical core API |
| `crates/aikit-core/src/search.rs` | REPLACE | Query grammar/ranking behaviors that are still intentional: fast prefixes, filters, bounded usage influence | `resource/search.rs` + Knowledge search + contextual Actions | `ResourceSearchIndex`; TUI V2 search acceptance | OPEN — migrate deliberate query grammar and remaining CLI consumers, then delete old Capsule/ResolvedView search model |
| `crates/aikit-core/src/resource/search.rs` | STILL-CANONICAL | ResourceRef-native shallow navigation, contextual Actions, explainable familiarity-aware ranking | `ResourceSearchIndex` | V2 Quick/Workspace tests | ACTIVE |
| Capsule → Resource conversion paths outside a source/import boundary | REPLACE | Existing registry/package compatibility without semantic duplication | canonical `ResourceIndex`/Resource providers | #24/#26/#59 acceptance | OPEN — enumerate callers and collapse to one ingestion seam |
| duplicate V1 resolution/read-model structures (`ResolvedView`, Capsule-shaped status/search where used as application truth) | REPLACE | Scope/trust/effective activation evidence | `ContextResolution`, Resource projections, `TuiApplicationService` | V2 ContextResolution + projection tests | OPEN — remaining callers must migrate before types can be deleted/isolated |
| `ContextResolution` / Resource model / Generation / Procedure / lifecycle primitives | STILL-CANONICAL | V2 semantic and mutation authority | same | V2 core tests | ACTIVE |
| `knowledge.rs`, `knowledge_navigation.rs`, `knowledge_wiki*`, `knowledge_source_pool.rs`, `knowledge_code.rs`, `project_map.rs` | STILL-CANONICAL | Federated Knowledge application and provider-native identity/provenance | same | commits `b68a1a2…`, `609ae9e…`, `119d3853…` plus provider acceptance | ACTIVE — production composition/surface wiring still open |

## TUI

| Item | Classification | Behavior worth preserving | V2 owner / replacement | Evidence | Status |
|---|---|---|---|---|---|
| `crates/aikit-tui/src/palette_service.rs` | REPLACE | Shallow search, explain/history/familiarity projection, contextual Actions, staged mutation adapter behavior | one V2 application composition implementing `TuiApplicationService`, with core Knowledge/Context/Compose services underneath | `application.rs`, reducer tests, `v2_prelocal_acceptance.rs` | OPEN — still instantiated as the transitional service; must not survive as Palette semantic owner |
| `crates/aikit-tui/src/tree.rs` | REPLACE | bounded relation/tree presentation and stable selection | one relation state projected as List/Tree/Graph by the V2 reducer/read model | #45 acceptance | OPEN — migrate useful traversal/presentation tests, then remove Tree semantic store/controller |
| `crates/aikit-tui/src/tree_driver.rs` | REPLACE | keyboard/mouse navigation behavior that remains valid | `TuiState` + `TuiRuntime` reducer actions | #40/#45 acceptance | OPEN |
| old Palette/Tree semantic-controller tests | DELETE | only interactions still valid under the final grammar | V2 reducer/application acceptance | `v2_prelocal_acceptance.rs`, TUI V2 tests | OPEN — delete after coverage mapping |
| transitional renderer paths that read controller-specific state | REPLACE | terminal rendering at narrow/medium/wide widths | projection-only V2 renderer over `TuiState` | #46 acceptance | OPEN |
| Palette-owned apply / Tree-owned staging / plain-Space staging while fuzzy query is active | DELETE | none; these are superseded interaction semantics | explicit contextual Actions + Ctrl-Space/Insert staging + preview/confirm reducer grammar | V2 reducer acceptance | OPEN — remove stale code/tests where still present |
| `crates/aikit-tui/src/application.rs` and canonical `TuiState`/reducer runtime | STILL-CANONICAL | one semantic UI authority | same | reducer/unit/integration tests | ACTIVE |
| `crates/aikit-tui/src/knowledge_service.rs` | STILL-CANONICAL | UI projection over the core Knowledge application, without provider/store semantics | same | #34–#38 acceptance | ACTIVE — production composition still open |

## CLI / agent-facing surfaces

| Item | Classification | Behavior worth preserving | V2 owner / replacement | Evidence | Status |
|---|---|---|---|---|---|
| `crates/aikit-cli/src/tree_build.rs` | REPLACE | human-readable bounded hierarchy where useful | common relation projection / ResourceRef-native navigation | #45/#59 | OPEN — inspect current call sites and migrate/delete |
| Palette-specific CLI routes/tests including `palette_run_intent.rs` | REPLACE | run/action intent and safe mutation behavior | common Actions/Compose/application service | #26/#28/#59 | OPEN |
| Capsule-shaped internal command APIs | REPLACE | deliberate external package/registry compatibility only | Resource/Application command vocabulary internally | #24/#26/#59 | OPEN — inventory all commands |
| deliberate CLI aliases for documented V1 external consumers | EXTERNAL-COMPATIBILITY | command compatibility only | thin translation to V2 application services | compatibility tests must name the consumer | OPEN — no alias is justified merely by historical existence |
| agent-native Action/Resource operations that invoke the same application services as TUI/CLI | STILL-CANONICAL | machine-facing equivalent operation | canonical application field | #26/#60 | OPEN — integrated acceptance still required |

## Tests / fixtures / docs

| Item | Classification | Behavior worth preserving | V2 owner / replacement | Evidence | Status |
|---|---|---|---|---|---|
| PTY tests whose premise is Palette ↔ Tree semantic switching | DELETE | terminal I/O only where it can be restated without controller switching | V2 terminal/reducer acceptance | #46/#60 | OPEN |
| tests asserting Tree-owned staging or Palette-owned apply | DELETE | staged/preview/confirm/apply safety | V2 reducer + application service tests | #43/#60 | OPEN |
| tests asserting plain Space staging during a non-empty fuzzy query | DELETE | explicit staging accessibility | Ctrl-Space/Insert/contextual Action acceptance | #40/#43 | OPEN |
| popup/chrome-name assertions whose only contract is legacy naming | DELETE | none unless they encode accessibility/layout behavior | semantic-state/render acceptance | #46 | OPEN |
| wording-only Codex assertion (`"aikit capabilities"`) | DELETE | brokered/no-material-projection semantics | activation-effect and materialisation assertions | commit `83f9d591…` | COMPLETE |
| old Capsule terminology in V2-facing docs/examples | REPLACE | package/source compatibility where actually relevant | Resource/Capability vocabulary, with Capsule named only as package envelope | this ledger + docs/v2 | OPEN |
| test-only imports/modules whose only effect is keeping dead V1 product code compiled | DELETE | migrate any unique behavior first | V2 acceptance | #59 | OPEN |

## Provider / knowledge migration classification

| Item | Classification | Reason | Status |
|---|---|---|---|
| native SemanticWiki application/provider | STILL-CANONICAL | authored semantic authority remains provider-native | ACTIVE |
| native SourcePool provider + real bkmr adapter | STILL-CANONICAL | provider-neutral source identity; bkmr IDs remain private bindings | ACTIVE |
| Git/source CodeReference + derived GitNexus adapter | STILL-CANONICAL | Git is canonical identity; graph is rebuildable derived intelligence | ACTIVE |
| generated GitNexus wiki treated as authored SemanticWiki | DELETE | violates authority boundary | PROHIBITED by design/tests |
| universal copied AI Kit knowledge graph | DELETE | violates federation architecture | PROHIBITED by `ProjectMap`/Knowledge design |
| separate CLI/TUI/agent Knowledge stores | DELETE | duplicates semantic authority | PROHIBITED; production shared composition still to prove |

## Closure procedure

For every `OPEN` row:

1. identify all live callers/tests/docs;
2. name the V2 semantic behavior that must survive;
3. add or point to V2 acceptance evidence;
4. migrate external callers to the canonical application field;
5. delete/contain the transitional implementation;
6. run strict static/dead-code and full pre-local acceptance;
7. mark the row `COMPLETE` with the exact commit.

#59 is not closed until no `REPLACE` or `DELETE` row remains `OPEN`, every compatibility row names a real external consumer, and repository scans prove no unclassified V1/transitional semantic path remains.
