# V2 composition / connection / terminal-field convergence evidence

**Scope:** #65, #66, #67  
**Implementation PR:** #68  
**Ordering law:** this document records the pre-#60-safe convergence state. It does not authorize #61–#63 SessionSpace implementation before #60 acceptance.

This ledger distinguishes three things which must not be blurred:

1. **upstream target facts** verified from pinned primary source;
2. **AIKit conformance/implementation facts** proved by repository code/tests;
3. **live-integration gates** owned by work which has not landed yet.

## 1. Maximal composition specimen — DeepSeek Harness / Cordis

Pinned primary source:

- repository: `deepseek-ai/deepseek-harness`;
- revision: `47f943859bef60e4160492346772ded9b24f765a`;
- repository licence at that revision: MIT;
- vendored Cordis licence at that revision: MIT;
- Cordis source is inspected through the vendored implementation at this exact DSH revision rather than assumed from another release;
- DSH currently includes its own ACP package/demo and pins `@agentclientprotocol/sdk` independently of AIKit.

Observed target pressure used by the specimen:

- nested loader/plugin/component groups;
- dependency injection / service requirements;
- scoped service/context availability;
- lifecycle-owned effect disposal;
- composable client UI slots/features;
- command and permission UI contributions;
- target-native session / trajectory material;
- replaceable agent-loop implementation.

AIKit interpretation is deliberately narrower than target vocabulary. Cordis `Context`, service/effect/plugin identities do not become canonical AIKit concepts.

### Native conformance mapping

| Target pressure | AIKit native expression | Evidence |
|---|---|---|
| component/plugin body | `ComponentDescriptor` + `ComponentSelection` | `aikit-core::composition`; `deepseek_maximal_v2` |
| nested plugin tree | `ComponentContainment` / `HarnessCompositionTopology` | `composition_topology.rs`; mounted/single-parent/cycle tests |
| service requirement | `ComponentRequirement` → `ContractProvider` | `composition_v2`; DSH fixture |
| provider substitution | same Contract, alternate Provider | `composition_v2::provider_substitution_is_deterministic_and_preserves_contract_identity` plus DSH shell substitution |
| reactive/coeffect pressure | reactive `ComponentRequirement` + effective binding | DSH UI-slot requirements; core required/optional tests |
| scope | separate `ResolutionScope`, `ActivationScope`, `LifetimeOwner` | `composition_v2`; DSH maximal fixture |
| owned effects | `ComponentContribution` + owner/lifetime + `RetractionMode` | DSH contribution fixture |
| tool/context/command/policy/UI | typed `ContributionKind` values | `deepseek_maximal_v2` |
| rich Web UI | `SurfaceKind::Web` with target-native provenance | `surface/deepseek/web-conversation` |
| trajectory/session reading | non-Action trajectory contribution | core + DSH base fixture |
| replaceable loop | `ContributionKind::LoopRuntime` | DSH maximal fixture |
| desired body mutation | `StagedHarnessComposition` → preview → confirm → apply | integration-line `composition_mutation`; DSH staged-mutation test |

### Activation truth

Cordis can own/retract effects in its own live Context/Fiber lifecycle, but AIKit does not currently possess a live Cordis control channel. Therefore the DSH conformance body remains `CompositionActivationMode::NextSession` even after AIKit stages/confirms a desired-body mutation.

This separation is intentional:

```text
AIKit desired/effective composition truth
    !=
proof that a foreign target has already mounted/unmounted it live
```

The native model can describe live retraction where a target proves it without falsely granting that capability to a target or adapter which has not proved it.

### Negative/thin-target floor

The maximal specimen does not raise the minimum harness shape. Existing core conformance retains:

- valid empty/static HarnessComposition;
- dependency-free Components;
- optional requirement absence as explicit degradation;
- required absence as structured failure;
- `NextSession != LiveMounted`;
- non-Action readings which remain non-Actions;
- multi-Surface projection of one canonical ActionRef without identity multiplication.

## 2. Interactive Agent connection — ACP plus classic shape

Pinned primary protocol source:

- repository: `agentclientprotocol/agent-client-protocol`;
- inspected stable schema/source revision: `62942933c42edade3ab8c85e055a5d1d753157fb`;
- stable wire protocol version: `1`;
- licence at that revision: Apache-2.0.

AIKit implements `aikit.connection-adapter/v1` as the reusable semantic seam. ACP is one protocol binding; `ClassicProcessConnectionAdapter` proves that ACP concepts are not promoted to universal AgentSession ontology.

Preserved distinctions:

```text
connection                    != AgentSession
ACP/native session id         != AgentSession Ref unless explicitly bound
ACP client / editor           != SessionSpace
agent process                 != Agent identity
ACP permission request        != Factory HumanRequest automatically
ACP session resume/close      != transport reconnect/disconnect
ACP                            != A2A != MCP
```

The ACP implementation negotiates rather than presumes capability-gated session lifecycle support, preserves JSON-RPC request ids for bidirectional permission calls, orders streamed signals, coordinates permission cancellation with prompt cancellation, owns `session/close` request/response lifecycle, carries provenance, and reports reconnect as unsupported/degraded unless a real reconnect binding is known.

No generic `attach` operation is invented where stable ACP v1 does not define one.

## 3. TUI / O:I working-field parity matrix

`aikit.tui-working-field/v1` is a terminal read-model/presentation seam over product-owned semantics. It is not a second store/controller and selection returns to the existing `TuiState` reducer.

Status meanings:

- **LIVE CONTRACT** — current producer code/contract was inspected and exact producer refs/revision are carried;
- **AIKIT LIVE** — native AIKit application/composition material is already part of the current V2 surface;
- **FIXTURE GATE** — deterministic contract fixture exists, but the producer/application dependency has not landed and no false live claim is made.

| Requirement / subject | Native owner | Shared Ref / contract | Desktop proof | TUI proof | Agent/CLI proof | Status / remaining gate |
|---|---|---|---|---|---|---|
| Project composition resources | AIKit | existing Profile / `skill-set/<name>` Resources | O:I consumes AIKit composition/readings through #68 | final V2 CLI surface advertises current Project Profiles and Skill Sets; working-field selection reuses `TuiState` | same `ResourceSearchIndex` / application service | **AIKIT LIVE** at integration `8eac20d7820086f63165f70c259c69dd7570513f` |
| Central Personal | Central | `personal.show`; `control.propose-change`; `control.review-proposal`; `control.apply-proposal`; `personal.notify` | O:I PR #34 consumes Central PR #53 | `working_field_v2` carries exact refs, owner, provenance and permission meaning | Central remains Action handler/source owner | **LIVE CONTRACT** Central PR #53 @ `3f0551090ae39bcef260a27b1a9db0da4729d8a3`; live AIKit dispatcher integration still separate |
| situated/root Agency | Actuation | `actuation.agency/v1`; executable root fixture `agency:root-position` | O:I PR #34 consumes Actuation PR #6 | `working_field_v2` carries exact Agency ref with no TUI-specific Agent subtype | Actuation `agencyReadModel` remains producer | **LIVE CONTRACT** Actuation PR #6 @ `b977939ec25c32b3dc8f5ed251b70e4c26933086` |
| Factory Build | Software Factory | native #144/#145 Build read model/Actions (not yet published) | O:I PR #34 truthfully reports pending adapter | `factory.surface/build` fixture, no counterfeit Actions | none claimed | **FIXTURE GATE** — Factory #144/#145 |
| current working world | AIKit / #61–#62 providers | SessionSpace contract | O:I PR #34 explicitly does not implement SessionSpace | `session-space/current` fixture only | none claimed | **FIXTURE GATE** — #60 acceptance, then #61/#62 |
| interactive AgentSession connection | AIKit | `aikit.connection-adapter/v1` | desktop may consume later through shared application path | ACP/classic adapter is implemented; working-field AgentSession remains fixture | adapters are repository-owned and executable | **IMPLEMENTED ADAPTER / FIXTURE APPLICATION GATE** — bind after #60/#62; #63 consumes #66 rather than reimplementing ACP |
| rich DSH Web contribution | DeepSeek Harness / Cordis | `surface/deepseek/web-conversation` | alternate rich Surface can be disclosed | explicit alternate-Surface reason; no fake terminal clone | composition/read-model remains inspectable | **LIVE CONFORMANCE SPECIMEN**, not a required suite dependency |

### Parity law exercised

The fixture/test contract checks the parts which can be deterministic before the post-#60 integrations:

```text
same canonical Ref
same native owner
same product-owned Action refs where they exist
same source/provenance
same permission meaning
explicit availability/degradation
explicit alternate Surface when terminal rendering is not equivalent
```

It deliberately does **not** claim same Action handler invocation for Central/Factory until the native dispatcher/application binding exists. That remains a live integration gate, not something a fixture can honestly prove.

## 4. Remaining closure gates

### #65

The maximal composition language and reusable nested-topology distinction are implemented and tested. A true live Cordis mount/retract proof would require a target control adapter; current conformance remains honest `NextSession`. This is not required to make Cordis a dependency.

### #66

The reusable protocol seam and ACP/classic implementations are executable. Product-level AgentSession/SessionSpace application binding is intentionally left to the ordered post-#60 path.

### #67

Full ticket closure remains blocked on the dependencies named by the ticket itself:

- #60 acceptance;
- #61/#62 SessionSpace implementation;
- #63 application of the shared #66 adapter;
- Factory #144/#145 Build read model/Actions;
- shared external Action dispatch proving same handler/lineage from desktop and TUI.

Until those land, the TUI may disclose and preserve current contracts but must not invent their state or authority.
