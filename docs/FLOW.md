# Flow in AIKit

**Status:** native AIKit owner implementation for O:I Flow · AIKit #122

Flow is one developing linguistic/conceptual thread carried by an ordinary source owned elsewhere. AIKit makes that source operative in situated agency: it binds one exact authorised Flow revision as standing context for an act, relates that act to the canonical AgentSession and current Method/praxis, and exposes deliberate `Contemplate(FlowRef)` through the existing Living Knowledge execution aperture.

AIKit does not own Flow files, Flow revision history, AgentSession transcripts, Claims, human Ground, Run identity, or Wiki authority.

## Provider-neutral source capability

`FlowProvider` is the native application seam. A provider exposes stable `FlowRef`, current `SourceRef` and revision, lifecycle/scope metadata, read/disclosure capability, write capability and its own exact expected-revision mutation semantics.

Central #93 is the first strong owner implementation. The AIKit contract has no ProjectCentral-path requirement: a non-Central provider can expose `notes/<timestamp>.md` or another retained source container through the same seam.

```text
Flow semantic role != source path
Flow available != payload disclosed
Flow selected != payload retrieved
Flow writable != Agent authorised to write
```

## Standing context and AgentSession continuity

`bind_flow_for_act` asks the owner for the current Flow state, verifies Project/lifecycle relation, and reads exactly that revision when disclosure is authorised. The returned `FlowStandingContext` records:

- FlowRef, SourceRef and exact disclosed revision;
- provider and Project;
- ContextResolution version/hash;
- canonical AgentSessionRef and current Agent/Agency refs when present;
- exact disclosed body digest, or an explicit undisclosed state.

The operation fetches that distinguished Flow only. It does not expand disclosure to unrelated Project sources and never invokes an Agent/model.

FlowRef remains independent of AgentSessionRef. A later AgentSession can bind the same current FlowRef from owner state without transcript replay.

## Revision-safe return

`FlowMutationIntent` records the exact Flow revision used by the act plus Agent/Agency/AgentSession, ContextResolution, Method and invocation refs. `apply_flow_mutation` validates that basis and delegates once to `FlowProvider::write`.

AIKit never implements last-write-wins or mechanical stale-output append. If the owner has advanced beyond the expected revision, the owner returns `FlowWriteResult::Conflict` with current state for situated re-reading/revision.

## Living Knowledge

A Flow source can be an ordinary exact `KnowledgeDependency`. Therefore Flow changes participate in the existing ChangeHorizon / impact / freshness field without a second knowledge graph. Explicit dependencies from Wiki objects/readings to a Flow revision can become `BasisChanged` or `IntegrationPending`; the deterministic impact path retains `automatic_agent_or_model_invocation=false`.

## First-party praxis

The first-party Flow praxis uses the completed AIKit grammar rather than a separate prompt system:

```text
Guidance
  → Flow working Skill
  → Knowledge Navigation + Living Knowledge
  → Method: Contemplate Flow
  → current ContextResolution / PraxisResolution
```

`first_party_flow_guidance` contributes positive standing guidance through the existing bounded Guidance compositor. `first_party_flow_resource_records` exposes the reusable Flow-working Skill/faculties and explicit contemplation Action through the V2 ResourceIndex. `first_party_flow_method` composes those references as a normal Method; Project/personal scopes may select or overlay the same resources without forking Flow semantics.

## `Contemplate(FlowRef)`

`flow_contemplate_preflight` requires:

- a currently disclosed active Flow;
- the same Project/ContextResolution and current PraxisResolution;
- an explicit selected Method;
- the canonical AgentSession when runtime supplies one;
- the current Flow SourceRef at the exact disclosed revision in the supplied ChangeHorizon.

It then reuses the accepted bounded Living Knowledge preflight and creates a deterministic invocation ref from the exact Flow/context/praxis/authority basis. No model invocation occurs during preflight.

`explicit_flow_contemplate` crosses the existing `explicit_contemplate` aperture exactly once through a typed adapter. The generated return can contain ordinary Living Knowledge changes plus unapplied Flow-owner mutation intents.

## Authority-preserving return

The preflight can carry explicit refs tagged as Flow, WikiReading, Claim, Ground, Run or AgentSession. These tags expose relation and provenance; they do not transfer authority into AIKit.

A deliberate contemplation can therefore return, independently:

```text
Flow mutation intent
    → Flow source owner

Agent Wiki / integrative WikiReading
    → existing Agent-Wiki / Living Knowledge path

ClaimRef / EvidenceRef
    → external owner ref remains external

human Ground implication
    → HumanSourceRevisionProposal / Recognition pressure

open question / tension
    → attributed returned knowledge
```

The current Flow may be refined while exact prior states remain the source owner's revision/ChangeHorizon/DAY concern.

## Zero-invocation law

No Agent/model call is caused by Flow inspection, opening, binding to another AgentSession, source change, DAY rollover, or Living Knowledge freshness becoming pending. Only explicit contemplation crosses the Agent/model aperture.
