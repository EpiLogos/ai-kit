# V2 composition / connection / terminal-field convergence evidence

**Scope:** #61–#67  
**Implementation PR:** #68  
**Current architectural cut:** AIKit now owns a provider-neutral `aikit.session-space/v1` runtime/read-model. Target adapters may strengthen eligible `HarnessComposition` bindings to live only when a real provider operation proves the stronger state.

This ledger distinguishes three things which must not be blurred:

1. **upstream target facts** verified from pinned primary source;
2. **AIKit semantic/runtime facts** proved by repository code/tests;
3. **provider observations** required before an eligible Component becomes `Active`.

## 1. SessionSpace semantic/runtime floor — #61 / #62

`aikit-core::session_space` supplies one small UI-neutral runtime boundary rather than a new global application model.

Stable/durable identity:

- `SessionSpaceRef` is a canonical `session-space/...` `ResourceRef` and is independent of Project, AgentSession, mux, TUI and desktop identities;
- `SessionSpaceDefinition` carries durable identity, Project membership and provenance;
- runtime provider state remains ephemeral and is projected through `SessionSpaceReadModel`.

Binding/lifecycle:

```text
SessionSpaceDefinition
  -> SessionSpaceRuntime::open
  -> explicit AgentSession binding
  -> SessionSpaceLease (one binding epoch)
  -> admit canonical HarnessComposition
  -> eligible Components + contributed Surfaces
  -> target/provider activation observation
  -> active/degraded/unavailable runtime state
  -> recomposition / connection changes
  -> unbind / close
```

The lease epoch makes replacement explicit: rebinding an AgentSession invalidates stale mutation handles. Closing a space clears live AgentSession, connection, composition and Surface bindings; no process-global SessionSpace registry is introduced.

The read model preserves:

- SessionSpace identity/lifecycle/revision;
- Project membership;
- canonical AgentSession Ref separately from target/native session id;
- Component and Surface identity;
- provider attribution and provenance;
- connection attribution;
- explicit capability/action authority disclosure.

### Authority law

Presence, visibility, connection establishment and live activation are independent from authority:

```text
Component visible              != Capability granted
connection available/connected != Capability granted
Capability granted             != Action authorised
SessionSpace membership        != encounter/contact trust
```

`SessionSpaceAuthorityState` therefore carries `capability_available`, `capability_granted` and `action_authorised` independently. SessionSpace never infers any of them from composition or connection state.

### Executable adversarial coverage

`crates/aikit-core/tests/session_space_v2.rs` proves at least:

- two independent spaces can contain the same Component Ref without state bleed;
- an AgentSession mutates a space only through the explicit current binding lease;
- stale leases fail after rebinding and after close;
- one Component contributes multiple Surfaces and Surface identity survives recomposition;
- removed nested composition updates the read model rather than remaining silently active;
- provider disappearance degrades active Component/Surface state;
- connection presence never invents capability or Action authority;
- `NextSession` descriptors cannot be counterfeited as live ACTIVE.

## 2. Maximal composition specimen — DeepSeek Harness / Cordis — #65

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
| live runtime truth | `SessionSpaceActivationDriver` observation | `session_space.rs`; `deepseek_live.rs` |

### Live Cordis activation — current source, not stale demo assumptions

The exact current upstream Web bundle already mounts Cordis on both host and client sides. At revision `47f943859bef60e4160492346772ded9b24f765a`, `packages/bundle/web-app/cordis.patch.yml` inserts `cordis-host-runner` and `cordis-client-runner` as part of the ordinary Web composition, whose target-owned webserver defaults to `127.0.0.1:3080`.

The repository also still contains `examples/web-cordis/cordis.yml`. That file describes itself as a patch overlay over the Web profile and tries to insert `cordis-host-runner` again. A clean exact-revision CI run proved that the old demo command:

```text
node --import tsx apps/cli/src/bin.ts web --patch examples/web-cordis/cordis.yml
```

now fails before readiness with `duplicate loader entry id: cordis-host-runner`. AIKit therefore does **not** preserve that stale example seam. The live adapter follows the actual current Web composition:

```text
node --import tsx apps/cli/src/bin.ts web
```

and waits for the target-owned endpoint at `127.0.0.1:3080`.

`deepseek_live_cordis_composition()` still resolves the same canonical #65 `CompositionCatalog` through `resolve_harness_composition`; it does not introduce a second resolver. The target adapter strengthens only Component bindings whose process-level presence is actually evidenced by the current Web/Cordis body:

```text
component/deepseek/profile-root
component/deepseek/client-ui-slots
component/deepseek/client-ui-conversation
component/deepseek/client-ui-commands
component/deepseek/client-ui-permission
```

`component/deepseek/agent-loop` deliberately remains `NextSession`: the current Web source moves the agent plane behind per-session agent presets, so a live host/Web process is not evidence that one AgentSession's loop is active. The older/thin tool bindings likewise remain `NextSession` until a target operation separately proves them live.

`CordisProcessActivationDriver` starts the exact target-owned process, observes child-process health and the published endpoint, and only then returns `SessionSpaceActivationObservation::Active`. Process exit/startup failure cannot become ACTIVE.

The repository has two proof levels:

1. `session_space_live_v2` exercises a real child-process lifecycle deterministically and proves ACTIVE/teardown are provider-observation-driven rather than enum-only;
2. the **Real DeepSeek Cordis SessionSpace activation** CI job checks out the exact upstream revision, installs its pinned workspace, first proves bare `dsh web` itself reaches its target-owned endpoint, then runs that same process through the SessionSpace adapter and confirms ACTIVE plus final-provider teardown.

The current process seam intentionally does **not** claim arbitrary in-process Cordis Fiber deletion from Rust. Partial Component retraction while sibling Components share the same Cordis process fails explicitly with `cordis.process.partial_retraction_unsupported` rather than counterfeiting successful live disposal. Whole-provider teardown is real; a finer Cordis control protocol remains a genuine future extension if needed.

## 3. Interactive Agent connection — ACP plus classic shape — #66 / #63

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

`connection_into_session_space()` is the single bridge from the existing #66 normalised adapter state to `SessionSpaceConnection`. It rejects a `NativeSessionBinding` that lacks an explicit canonical AgentSession Ref, preserves native session id/protocol/provider provenance, maps lifecycle/degradation, and accepts authority only as explicit external input. No ACP operation is promoted to a canonical Action by the bridge.

`session_space_live_v2` covers the negative unbound-native-session case and the positive ACP case with a visible-but-withheld capability/Action state.

## 4. TUI / O:I working-field parity — #67 / O:I #34

`aikit.tui-working-field/v1` remains a terminal read-model/presentation seam over product-owned semantics. It is not a second store/controller and selection returns to the existing `TuiState` reducer.

`working_field_from_session_space(&SessionSpaceReadModel)` now projects the same UI-neutral live state into the TUI:

- enclosing SessionSpace;
- bound AgentSession;
- active/eligible/degraded/unavailable Components;
- contributed Surfaces, including explicit alternate rich Surfaces rather than pixel-counterfeit terminal copies;
- Agent connection state;
- separate capability/action authority meaning;
- provider/source provenance.

`working_field_session_space_v2` proves ACTIVE and provider-loss degradation are rendered from the shared model. The earlier `session-space/current` / ACP contract fixtures were retired from the parity fixture; only genuinely unlanded dependencies such as Factory #144/#145 remain `ContractFixture`.

### O:I PR #34 consumer boundary

O:I PR #34 remains a consumer rather than an owner. Its desktop contribution model accepts AIKit-owned Component/Surface/target eligibility and lineage; the SessionSpace read model supplies the same UI-neutral runtime facts without requiring desktop to activate Cordis or invent SessionSpace semantics.

No AIKit Skills move into O:I/Central and no O:I activation controller is introduced by this slice.

## 5. Status and remaining closure fronts

### #61 / #62

The executable semantic/runtime floor is implemented for this vertical slice: stable identity, explicit AgentSession binding, Component/Surface admission, provider-attributed live activation, connection state, isolation, provenance, authority separation, degradation, recomposition and close semantics. The parent programme remains open for its broader acceptance surface: richer multi-Project/Context binding and resolution provenance, provider/host/material bindings, focus/history/reconstruction services, persistence/restore, and the provider migrations owned by M2.

### #63

The shared #66 adapter now participates in SessionSpace without another ACP stack. Full cmux/tmux open/reopen migration, multiple live AgentSession/provider fixtures and IDE conformance remain separate acceptance fronts.

### #65

The previous universal “activate on next session” limitation is removed only where the current DSH Web/Cordis composition has an executable live process path. Per-session agent-loop and thin/tool bindings remain `NextSession` until their own target activation seams prove otherwise. Fine-grained in-process Fiber retraction is not invented.

### #66

ACP/classic connection semantics and the explicit AgentSession→SessionSpace bridge are executable. Broader provider-specific live ACP process acceptance may continue independently; the semantic bridge is no longer blocked on a missing SessionSpace.

### #67

The TUI now consumes live SessionSpace state through the shared read model rather than a TUI-local SessionSpace fixture. Full ticket closure still includes the remaining product dependencies named by the ticket, notably Factory Build contribution and broader suite Action dispatch/host convergence.
