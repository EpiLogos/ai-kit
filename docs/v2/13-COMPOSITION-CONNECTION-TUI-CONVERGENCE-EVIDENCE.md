# V2 composition / connection / terminal-field convergence evidence

**Scope:** #61–#67  
**Implementation PR:** #68  
**Accepted substrate:** PR #58 / `1036a8de0bb6bdc234e5334ba38730d60118aa4c`  
**Architectural cut:** SessionSpace is a provider-neutral runtime/read-model frame over the accepted V2 product architecture. Canonical `HarnessComposition` remains resolver-owned desired state. Provider/runtime observations are represented separately and are always attributable to the exact desired-body fingerprint they observed.

This ledger distinguishes four things which must not be blurred:

1. **authored/canonical desired state** — Project membership, `ContextResolution`, `HarnessComposition`, canonical `SurfaceDescriptor` and Resource identity;
2. **upstream target facts** — verified from pinned provider source and supplied as resolver inputs where they affect desired composition;
3. **provider observations** — required before an eligible Component becomes `Active`, and attributable to an exact composition fingerprint;
4. **consumer projections** — TUI/O:I/agent views over the same UI-neutral state, never independent semantic owners.

## 1. SessionSpace semantic/runtime floor — #61 / #62

`aikit-core::session_space` supplies an owned UI-neutral runtime/read-model boundary rather than a new global application model, resolver or precedence scope.

Stable/durable identity:

- `SessionSpaceRef` is a canonical `session-space/...` `ResourceRef` independent of Project, Context, AgentSession, mux, TUI and desktop identities;
- `SessionSpaceDefinition` carries SessionSpace identity, explicitly authored Project membership and provenance;
- admitting a `HarnessComposition` which carries Project provenance **does not** author Project membership into the SessionSpace;
- richer Project/Context bindings with `ContextResolution` provenance remain an explicit #62 application-layer continuation, not an implicit side effect of runtime admission;
- runtime provider state remains separate and is projected through `SessionSpaceReadModel`.

Binding/lifecycle:

```text
SessionSpaceDefinition
  -> SessionSpaceRuntime::open
  -> explicit AgentSession binding
  -> SessionSpaceLease (one binding epoch)
  -> admit canonical resolver-owned HarnessComposition
  -> eligible Component readings + Surface readings over canonical SurfaceDescriptor
  -> provider activation observation against exact composition fingerprint
  -> active/degraded/unavailable observed runtime state
  -> provider deactivation returns to eligible observed state
  -> canonical recomposition changes desired membership
  -> connection changes / unbind / close
```

The lease epoch makes replacement explicit: rebinding an AgentSession invalidates stale mutation handles. Closing a space clears live AgentSession, connection, composition and Surface readings; no process-global SessionSpace registry is introduced.

### Desired state is not observed state

The accepted #59/#60 composition law remains authoritative:

```text
canonical HarnessComposition = desired/resolved body
provider observation          = runtime evidence about that exact body
```

SessionSpace therefore does **not** rewrite an admitted `HarnessComposition.state` to encode target observation. `Active`, `Degraded` and `Unavailable` are SessionSpace runtime readings. A provider-confirmed deactivation changes observed live truth only; it does not retract the Component or its Surfaces from the canonical desired composition. Only a newly admitted canonical recomposition may remove membership.

`SessionSpaceComponent.observed_composition_fingerprint` records the exact body against which provider evidence was obtained. If a different body fingerprint is admitted, prior live evidence is invalidated and the still-present Component returns to `Eligible` until the provider observes the new body.

### Surface identity law

There is no parallel SessionSpace Surface ontology. `SessionSpaceSurfaceReading` is explicitly a runtime reading over the canonical `SurfaceDescriptor`, adding only AgentSession attribution, observed state and provenance. The canonical Surface `ResourceRef` remains unchanged across Compose, SessionSpace, TUI and other consumers.

Recomposition tracks the exact canonical Surface set. If a Surface disappears from a new body, its SessionSpace reading disappears even when the owning Component remains. Provider deactivation, by contrast, preserves desired Surface membership and changes only observed state.

The read model preserves:

- SessionSpace identity/lifecycle/revision;
- explicitly authored Project membership;
- canonical AgentSession Ref separately from target/native session id;
- canonical Component and Surface refs;
- exact observed composition fingerprint for provider evidence;
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

The SessionSpace suites prove, among other cases:

- two independent spaces can contain the same Component Ref without state bleed;
- an AgentSession mutates a space only through the explicit current binding lease;
- stale leases fail after rebinding and after close;
- Project provenance on an admitted composition cannot silently author SessionSpace Project membership;
- one Component can contribute several canonical Surfaces;
- a changed body fingerprint invalidates stale provider `Active` evidence;
- exact Surface membership follows canonical recomposition even when an owner Component remains;
- provider deactivation preserves desired Component/Surface membership and returns observation to `Eligible`;
- provider disappearance degrades active Component/Surface readings;
- connection presence never invents capability or Action authority;
- `NextSession` descriptors cannot be counterfeited as live `Active`.

## 2. Maximal composition specimen — DeepSeek Harness / Cordis — #65

Pinned primary source:

- repository: `deepseek-ai/deepseek-harness`;
- revision: `47f943859bef60e4160492346772ded9b24f765a`;
- repository licence at that revision: MIT;
- vendored Cordis licence at that revision: MIT;
- Cordis source is inspected through the vendored implementation at this exact DSH revision rather than assumed from another release;
- DSH currently includes its own ACP package/demo and pins `@agentclientprotocol/sdk` independently of AIKit.

Observed target pressure used by the specimen includes nested loader/component groups, dependency injection/service requirements, scoped service/context availability, lifecycle-owned effects, composable client UI slots/features, command and permission UI contributions, target-native session/trajectory material and a replaceable agent-loop implementation.

AIKit interpretation remains narrower than target vocabulary. Cordis `Context`, service/effect/plugin identities do not become canonical AIKit concepts.

### Native conformance mapping

| Target pressure | AIKit native expression | Evidence |
|---|---|---|
| component/plugin body | `ComponentDescriptor` + `ComponentSelection` | `aikit-core::composition`; `deepseek_maximal_v2` |
| nested plugin tree | `ComponentContainment` / `HarnessCompositionTopology` | `composition_topology.rs` |
| service requirement | `ComponentRequirement` → `ContractProvider` | core composition + DSH fixture |
| provider substitution | same Contract, alternate Provider | deterministic substitution tests |
| scope | separate `ResolutionScope`, `ActivationScope`, `LifetimeOwner` | core composition tests |
| owned effects | `ComponentContribution` + owner/lifetime + `RetractionMode` | DSH contribution fixture |
| tool/context/command/policy/UI | typed `ContributionKind` values | `deepseek_maximal_v2` |
| rich Web UI | canonical `SurfaceKind::Web` descriptor | `surface/deepseek/web-conversation` |
| replaceable loop | `ContributionKind::LoopRuntime` | DSH maximal fixture |
| desired body mutation | `StagedHarnessComposition` → preview → confirm → apply | accepted V2 composition mutation |
| live runtime truth | `SessionSpaceActivationDriver` observation against body fingerprint | `session_space.rs`; `deepseek_live.rs` |

### Live Cordis composition is resolved once

The exact current upstream Web bundle already mounts Cordis host and client runners. At the pinned revision, `packages/bundle/web-app/cordis.patch.yml` includes `cordis-host-runner` and `cordis-client-runner` in the ordinary Web composition. The older `examples/web-cordis/cordis.yml` overlay attempts to insert the host runner again and fails with duplicate loader identity, so AIKit follows the current actual upstream body:

```text
node --import tsx apps/cli/src/bin.ts web
```

with the target-owned endpoint at `127.0.0.1:3080`.

The target adapter owns the source-derived fact that a small set of process-level Components can be `LiveMounted`. Those facts are applied to the target-specific `CompositionCatalog` / selection request **before** `resolve_harness_composition` is called. The canonical resolver then produces the final body and fingerprint once. No resolver-owned `ComponentBinding`, contribution or activation field is mutated after fingerprint calculation.

The process-evidenced set is:

```text
component/deepseek/profile-root
component/deepseek/client-ui-slots
component/deepseek/client-ui-conversation
component/deepseek/client-ui-commands
component/deepseek/client-ui-permission
```

`component/deepseek/agent-loop` deliberately remains `NextSession`: a live host/Web process is not evidence that one AgentSession's loop is active. Thin/tool bindings likewise remain `NextSession` until their own target operation proves them live.

`CordisProcessActivationDriver` starts the exact target-owned process, observes child-process health and endpoint readiness, and only then returns `SessionSpaceActivationObservation::Active`. The reading records the exact canonical composition fingerprint. Process exit/startup failure cannot become `Active`.

Provider-confirmed final teardown returns `Deactivated`, which SessionSpace maps to an `Eligible` reading for a still-desired Component. It does not counterfeit canonical retraction. The process seam still intentionally refuses arbitrary in-process Fiber deletion while sibling Components share the Cordis process; `cordis.process.partial_retraction_unsupported` remains the truthful boundary.

The repository has two proof levels:

1. `session_space_live_v2` exercises deterministic real child-process activation/deactivation and desired-vs-observed semantics;
2. the **Real DeepSeek Cordis SessionSpace activation** CI lane checks out the exact upstream revision, installs it, proves the target Web/Cordis endpoint, then runs the provider through SessionSpace `Active` → real process teardown while preserving desired Surface membership.

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

`connection_into_session_space()` is the bridge from the existing #66 normalised adapter state to `SessionSpaceConnection`. It rejects a native session binding without an explicit canonical AgentSession Ref, preserves native session id/protocol/provider provenance, maps lifecycle/degradation, and accepts authority only as explicit external input. No ACP operation is promoted to a canonical Action by the bridge.

Adversarial coverage proves ACP degradation can coexist with an independently healthy classic connection without poisoning the enclosing SessionSpace.

## 4. TUI / O:I working-field parity — #67 / O:I consumer boundary

`aikit.tui-working-field/v1` remains a terminal read-model/presentation seam over product-owned semantics. It is not a second store/controller, and semantic selection returns to the existing `TuiState` reducer.

`working_field_from_session_space(&SessionSpaceReadModel)` projects the same UI-neutral state into the TUI:

- enclosing SessionSpace;
- bound AgentSession;
- active/eligible/degraded/unavailable Component readings;
- canonical Surfaces via `SessionSpaceSurfaceReading`;
- Agent connection state;
- separate capability/action authority meaning;
- provider/source provenance.

Rich Web UI remains an alternate peer Surface rather than a counterfeit terminal copy. The TUI tests prove provider-loss degradation is observed through the shared read model.

O:I remains a consumer boundary. AIKit owns SessionSpace, Component/Surface runtime readings and provider activation semantics; a desktop host may render/compose those contributions but does not become the SessionSpace resolver or Cordis activation controller.

## 5. Harmonisation against accepted #59 / #60 substrate

The SessionSpace line has been explicitly reviewed against PR #58 exact accepted head `1036a8de0bb6bdc234e5334ba38730d60118aa4c` and converged as a merge descendant of that commit.

The harmonisation corrections are contractual, not a redesign:

- canonical `HarnessComposition` stays desired/resolved and resolver-owned;
- live target facts participate in canonical resolution before fingerprinting;
- runtime observations are body-attributed and cannot survive an unobserved changed fingerprint;
- provider deactivation is not desired-body recomposition;
- Project membership is explicit authored SessionSpace state, not inferred from composition provenance;
- `SessionSpaceSurfaceReading` is a reading over canonical `SurfaceDescriptor`, not a new Surface family;
- exact Surface membership follows canonical recomposition;
- the existing connection seam and existing TUI reducer remain canonical.

These guarantees are locked by `session_space_substrate_harmonization_v2.rs`, the adapter lifecycle tests, TUI fixtures and the real pinned Cordis CI lane.

## 6. Genuine remaining fronts

### #61 / #62

The executable runtime/read-model floor is now correctly shaped, but the parent programme remains open for substantive work:

- durable persistence/restore and reconstruction;
- richer multi-Project `ProjectBinding` and `ContextResolution` references with provenance;
- provider/host/material bindings and reconstructability disclosure;
- focus/history/reconstruction surfaces;
- integration of SessionSpace operations through the already-canonical shared `ApplicationService` for CLI/TUI/agent consumers.

These must extend the accepted architecture, not create a SessionSpace-local resolver, semantic store or second TUI/application state.

### #63

The shared #66 adapter participates in SessionSpace without another ACP stack. Full cmux/tmux/IDE provider migration, reopen/recovery and broader live provider acceptance remain open.

### #65 / #66 / #67

Maximal DSH/Cordis, connection and terminal projection work may continue on their own ticket criteria. The SessionSpace floor must not use those extensions to redefine canonical Resource, Surface, composition or application-service ownership.
