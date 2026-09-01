# ADR 0004 — Scope-neutral composition body beneath HarnessComposition

Status: implemented on PR #144; native-main acceptance pending repository CI.

## Context

AIKit already resolves heterogeneous runtime bodies through the canonical relation:

```text
Component
  → Contract requirements/providers
  → Contributions
  → Surfaces
  → projection / absence / scope / lifetime evidence
```

That resolver was first embodied inside `HarnessComposition` because the initial consumer was harness/session composition. The resolved body itself, however, is also needed by host, environment and application composition where a Harness is not the semantic parent.

Provider-local placement must remain distinct from semantic identity. A Hyprland workspace, Herdr session-space binding, Omarchy/Quickshell surface, remote body or other target-native carrier may realise a contribution without becoming the Component, Contract, Surface, AgentSession or World it carries.

## Decision

`aikit-core` exposes a reusable scope-neutral resolved body:

- `CompositionBodyRequest`;
- `CompositionBody`;
- `resolve_composition_body`;
- `COMPOSITION_BODY_VERSION = "aikit.composition-body/v1"`.

`CompositionBody` owns only the generic resolution result:

- component bindings and their resolution/activation/lifetime choices;
- contract-provider bindings;
- contributions;
- surfaces;
- canonical projection bindings;
- truthful optional absences;
- model/revision/generation evidence where supplied;
- a deterministic fingerprint of that scope-neutral body.

It does **not** require or acquire Harness, Project, Agent, Agency, AgentSession, SessionSpace, World, host, renderer or provider-native identity.

`HarnessComposition` remains the Harness-scoped semantic wrapper. `resolve_harness_composition` delegates generic body resolution to `resolve_composition_body`, then restores the Harness relation and its established wrapper fingerprint contract. This preserves existing Harness history/explain consumers while making the reusable body available at the correct altitude.

## Consequences

The same Component/Contract/Contribution/Surface grammar can now be consumed by harness, host and environment composition without inventing parallel resolvers or treating one consumer scope as universal ontology.

A change of target-native carrier can therefore be represented as provider/surface binding evidence around the same canonical body. Such a change does not by itself mint a new semantic session, world, component or contribution.

SessionSpace, AgentSession and Gateway ecology remain owned by their existing AIKit/Actuation relations. This ADR does not generalise those identities into `CompositionBody`; it gives those relations a shared body resolver to consume.

## Verification boundary

PR #144 contains focused tests proving that scope-neutral body resolution does not require Harness or actor identity and that Harness wrappers continue to resolve through the same body. Full repository CI remains the acceptance gate before this ADR and implementation become native-main fact.
