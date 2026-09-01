# Part XXV — Working-environment providers

## Determination

`aikit.working-environment-provider/v1` is AIKit's bounded public participation seam for an external mux, IDE, desktop or other working-environment provider.

It exists because a SessionSpace may be encountered through several heterogeneous provider worlds at once while remaining one semantic SessionSpace. Provider-native workspaces, panes, windows, views and sessions are therefore observations/bindings around canonical AIKit refs; they are not alternate SessionSpace, AgentSession, Project or Surface identities.

The governing relation is:

```text
SessionSpaceRef
    one durable semantic workspace field
        ↓ explicit bindings / observations
WorkingEnvironmentProvider A
WorkingEnvironmentProvider B
Harness-native Surface/provider
Gateway / connector relation
        ↓
provider-native session/view/window/pane ids
```

A provider may disappear while the SessionSpace and unrelated provider relations remain present. The correct return is local degradation of that provider relation, not semantic replacement of the whole workspace.

## Public contract

External implementations consume the public `aikit-adapters` crate exports:

- `WorkingEnvironmentProvider`;
- `WorkingEnvironmentObservation`;
- `WorkingEnvironmentCapabilities`;
- `WorkingEnvironmentHealth`;
- `ProviderNativeBinding`;
- `NativeBindingKind`;
- `WORKING_ENVIRONMENT_PROVIDER_VERSION`.

The provider reports discovery/open/focus/select and supported Surface/attachment capabilities, then returns observations containing provider-native bindings and provenance.

A provider-native id carries no canonical meaning by itself:

```text
ProviderNativeBinding {
    kind
    native_id
    canonical_ref: None
}
```

Only an explicit caller/provider relation may bind it to a canonical ref:

```text
ProviderNativeBinding {
    kind: Surface
    native_id: "native-window-7"
    canonical_ref: Some(surface/...)
}
```

This preserves heterogeneous existing worlds instead of forcing them to migrate into an AIKit-shaped workspace ontology.

## Relation to SessionSpace and Gateway

`SessionSpaceRuntime` owns the semantic workspace relation and observed connection state. It can carry several `SessionSpaceConnection`s for one canonical AgentSession. Each connection retains its provider, protocol, native session evidence, authority and provenance separately.

`observe_provider_unavailable(provider, reason)` degrades only observations owned by that provider. It does not mint a replacement SessionSpace, rebind the canonical AgentSession, delete unrelated canonical Surfaces or imply authority changes.

Gateway connection projection follows the same law. A canonical Gateway/ActuationStream/AgentSession relation may be another connection into the same SessionSpace; it is not a second remote-session mechanism.

## Current reference providers

Herdr and Hyprland are current first-party reference implementations of this public seam. tmux/cmux retain their existing mux owner paths and are wrapped where appropriate rather than rewritten into Herdr/Hyprland semantics.

The deterministic #139 reference-world specimen additionally proves that an external-style provider can implement the public trait using only the crate-root public contract. That fixture is conformance evidence, not a privileged built-in provider.

## Acceptance boundary

Remote/source acceptance can prove:

- public trait compilation;
- canonical/native identity separation;
- one SessionSpace carrying several provider relations;
- isolated provider-loss degradation;
- deterministic provider observations and capability declarations.

It cannot prove physical availability, boot/relogin survival, desktop placement, live remote attachment or cross-machine Gateway continuity. Those require the actual #97 local/reference-machine acceptance environment and must remain explicitly unclaimed until exercised there.
