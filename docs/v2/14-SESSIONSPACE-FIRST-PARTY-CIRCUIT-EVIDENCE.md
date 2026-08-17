# SessionSpace first-party circuit evidence

**Scope:** #61–#63 cross-product consumption and native lifecycle  
**Implementation line:** PR #68  
**Accepted V2 substrate:** PR #58 / `1036a8de0bb6bdc234e5334ba38730d60118aa4c`  
**Verified implementation evidence point:** `236356519433d181501e22fdebb6e458c05c2786` — CI #445 / run `31982798522` **SUCCESS**

This record extends `13-COMPOSITION-CONNECTION-TUI-CONVERGENCE-EVIDENCE.md` with the later first-party lifecycle/observation circuit. It does not redefine the SessionSpace model documented there.

## 1. Ownership cut

The governing relation is:

```text
AIKit
  owns SessionSpace identity, runtime/read model,
  contribution registration and provider observation semantics

Factory
  may carry an opaque AIKit SessionSpaceRef on Factory-owned Execution state

O:I
  may install/register product-owned contributions,
  host presentation and observe product-owned read models
```

Therefore:

```text
package identity                 != SessionSpace contribution identity
SessionSpace contribution        != SessionSpace identity
SessionSpace observation         != SessionSpace ownership
SessionSpace visibility          != provider activation
provider activation              != Capability grant
Capability availability          != Capability grant
Capability grant                 != Action authorisation
Factory execution.sessionSpaceRef != Factory ownership of SessionSpace
```

No consumer may synthesize `Active`, restore/mutate a SessionSpace from an observation, or infer trust/Capability/Action authority from presence or connection.

## 2. AIKit-owned native contribution lifecycle

The implementation exposes:

- `aikit.session-space-contribution/v1`;
- `aikit.session-space-contribution-registry/v1`;
- stable `SessionSpaceContributionRef` distinct from package identity and `SessionSpaceRef`;
- target-owned register → native readback → remove operations;
- optional verification against the actual `SessionSpaceReadModel`.

Removal deletes the registration relation only. It does **not** close, delete or mint the independently owned SessionSpace runtime/identity.

Repository conformance covers successful registration/readback/removal, duplicate rejection, SessionSpace identity mismatch rejection and preservation of independently owned SessionSpace state.

This is the native target lifecycle used by O:I's package envelope. O:I coordinates the lifecycle but calls AIKit's own operation; it does not provide a universal plugin/runtime ABI.

## 3. First-party read-only observation transport

`aikit.session-space-observation-file/v1` lives in `aikit-adapters`, not `aikit-core`.

`SessionSpaceFileObservationProvider` can:

```text
existing SessionSpaceRuntime
  → publish current SessionSpaceReadModel
  → another first-party process reads that exact model
```

It cannot:

- restore a SessionSpace runtime;
- mutate SessionSpace state;
- author Project/Context/AgentSession/Surface membership;
- manufacture provider activation;
- grant Capability or Action authority.

This placement preserves the explicit I/O-free core boundary. The file is a local first-party observation transport, not the durable persistence/restore mechanism still required by #62.

## 4. Cross-product executable circuit

At the verified first-party integration points the path is:

```text
Factory canonical state
  → FactoryBuildViewProvider
  → Factory-owned local provider
  → FactoryBuildView { execution.sessionSpaceRef? }

AIKit SessionSpaceRuntime
  → SessionSpaceReadModel
  → AIKit-owned observation adapter

O:I
  → native AIKit contribution register/readback/remove
  → observe Factory-owned FactoryBuildView
  → observe AIKit-owned SessionSpaceReadModel
  → correlate only by stable opaque SessionSpaceRef
  → render/mediate without becoming either product's semantic store
```

Provider disappearance is reflected from AIKit's own observed-state semantics. O:I does not edit a read model to counterfeit degradation.

Factory actions remain Factory-owned. O:I may mediate an explicitly authorised `{ actionRef, subjectRef }` request only after exact owner, required Capability identity/grant and Action authority checks. A visible Action does not grant itself.

## 5. Verified producer/consumer snapshots

### AIKit

PR #68 implementation evidence point:

- `236356519433d181501e22fdebb6e458c05c2786`;
- CI #445 / run `31982798522`: **SUCCESS**;
- includes executable `aikit.session-space/v1`, native contribution lifecycle, read-only observation adapter and real pinned DeepSeek/Cordis SessionSpace activation.

### Factory

`EpiLogos/agent-system-design` PR #146:

- head `c39bd63580cb7196f38c9a26b49e3977aac95e6a`;
- Factory Rust `31982029376`: SUCCESS;
- Factory Build UI `31982029324`: SUCCESS;
- QL Native `31982029348`: SUCCESS;
- QL Pi `31982029305`: SUCCESS;
- QL PydanticAI `31982029372`: SUCCESS.

Factory preserves `execution.sessionSpaceRef` only as an opaque AIKit-owned reference.

### O:I

`EpiLogos/O-I` PR #34:

- head `760aadc01cc3b603826d624e68c54eb1ac7cc547`;
- OI Verify `31982863432`: SUCCESS;
- O:I desktop `31982863426`: SUCCESS;
- current O:I code pins `aikit-core` to the AIKit implementation evidence point above.

O:I remains lifecycle/hosting/presentation/consumer infrastructure, not the owner of SessionSpace or Factory state.

## 6. Relation to #61–#63

The existence and correctness of the SessionSpace runtime/read-model floor are no longer open questions.

### #61 / #62 remain open for M1 completion

Still required:

- richer multi-Project `ProjectBinding` + `ContextResolution` provenance;
- provider/host/material/focus relations;
- durable persistence/restore/history/reconstruction;
- common SessionSpace operations through the existing shared `ApplicationService` for CLI/TUI/agent consumers.

The observation file in this record is intentionally **not** a substitute for that persistence/reconstruction work.

### #63 remains open for M2 provider conformance

Still required:

- actual cmux provider migration/proof;
- actual tmux open/attach/detach/reopen/recovery proof;
- one real rich IDE/editor conformance specimen;
- cross-provider semantic continuity;
- bounded external-style SessionSpace provider conformance.

The O:I first-party observation circuit is not a substitute for those provider-specific proofs.

## 7. Constitutional outcome

The first-party circuit demonstrates the intended product relation without collapsing ownership:

```text
product primitives and semantic mutation stay with their owners
        ↓
AIKit frames live human working relations as SessionSpace
        ↓
O:I installs/hosts/observes those native contracts
        ↓
Factory and other products correlate by stable refs only
```

This is the harmonised floor future #62/#63 work must extend. It must not introduce a second resolver, SessionSpace store, Surface family, connection stack, package runtime or authority model.
