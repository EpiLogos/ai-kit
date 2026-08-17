# Part XXII — Persistent agency, access surfaces and material hosting

## 84. Determination

AIKit's `Surface` abstraction already describes where and how an operative world can be encountered: CLI, agent tool, conversation, trajectory, TUI, web region, API, automation, editor integration and future target-native forms.

Persistent agent systems add one important relation that must remain explicit:

> **one effective Agent/Harness/session arrangement may remain continuously operative while being contacted through several independent Surfaces.**

The continuity of the agency, the semantics of its encounter Surfaces, and the material services that keep those Surfaces reachable are different concerns.

AIKit owns the operational relation between the Agent/Harness/session and its resolved Surfaces. Workcell owns material process/service/storage/network bindings beneath those Surfaces. Factory owns the authored/developmental reason the arrangement exists and the evidence by which its execution is judged.

No product should collapse these layers merely because a particular harness packages them together.

## 85. Surface is not endpoint

A Surface answers:

```text
where/how can this actor or capability be encountered or operated?
```

A material service binding answers:

```text
what process/service/socket/endpoint/network relation currently makes that encounter physically reachable?
```

They are related but not identical.

Conceptually:

```text
Agent
  ↓ situated as
Agency
  ↓ embodied through
Harness
  ↓ active as
HarnessComposition / AgentSession
  ├─ Surface: CLI
  ├─ Surface: TUI
  ├─ Surface: messaging conversation
  ├─ Surface: HTTP/API
  └─ Surface: webhook / automation
          ↓ material support relation
      Workcell service bindings
          ↓
  process · endpoint · storage · network · lifecycle
```

The same Surface may be rebound to a different physical endpoint without changing its semantic purpose. Several Surfaces may reach the same effective Agent/Harness/session. A physical endpoint may support more than one Surface where the target's own protocol says so.

AIKit must therefore preserve separately:

- Surface identity/kind and target-native meaning;
- ProjectionBinding / ComponentContribution that exposes the Surface;
- effective HarnessComposition/session relation;
- material service/binding reference where supplied;
- provider/host/revision provenance for that material binding;
- availability/degradation of the Surface independently from Agent identity.

A URL, port, socket, gateway process ID or Workcell BindingRef must not become Agent, Harness, Action, Capability or Surface identity.

## 86. Persistent agent hosts and gateways

Current agent systems frequently use a long-running gateway, daemon, server or messaging host to decouple the continuing agent runtime from the channels through which it is contacted.

This is useful prior art, but `Gateway` is not a universal AIKit primitive or protocol.

Different harnesses may place different responsibilities behind that word. One target may combine session hosting, messaging adapters and RPC in one process; another may expose distinct ACP, JSON-RPC, HTTP/SSE, WebSocket, stdio or target-native control surfaces.

AIKit should model what is operationally common without translating target-owned protocols into a fake shared gateway API:

- the enduring Agent remains distinct from the gateway process;
- Harness and HarnessComposition remain distinct from the material host;
- AgentSession remains replaceable execution/session binding rather than gateway identity;
- each user/agent/application access method is represented as a Surface or target-native Surface contribution;
- target-native gateway/service Components and Contracts remain target-owned runtime composition;
- material persistence, service health and endpoint reachability may be supplied by Workcell observations/bindings;
- losing/restarting one communication adapter can degrade a Surface without silently minting a new Agent.

Hermes and OpenClaw are reference interoperability targets for this relation, not architecture authorities. Adapters must be grounded in their current upstream source/contracts when implemented.

## 87. Workcell relation

The Workcell boundary is:

```text
AIKit resolved Surface / HarnessComposition need
              ↓ provider-neutral material demand or binding need
Workcell
  process · service · storage · network · endpoint · lifecycle
              ↓
MaterialisedExecutionWorld / BindingGraph
```

Workcell may expose a logical service binding such as an authenticated interactive stream, event ingress, local terminal or private control endpoint. AIKit may then bind a target-native Surface to that material relation.

Workcell does not interpret prompts, conversations, tools, messaging-app semantics or Agent identity merely because it hosts their service.

The Workcell Control Service is also separate: it is the optional persistent control surface for operating a remote/server Workcell itself. It is not the agent's gateway and must not be presented as one unless a target explicitly chooses to use it as an application-level transport through a separate adapter.

The coordinated Workcell implementation programme is `EpiLogos/Workcell#19-#25`, especially #21 Control Service, #22 persistent agent-hosting conformance and #25 Hermes/OpenClaw gateway-management interoperability.

## 88. Factory relation

Factory may author or resolve requirements such as:

```text
this agency should remain continuously available
this act needs an authenticated interactive access path
this system should expose a human conversation Surface and an automation/event Surface
this execution requires continuity/recovery evidence
```

Factory should not prescribe a Workcell provider, concrete endpoint, gateway process or AIKit target-native Surface implementation unless that detail is genuinely part of authored Project/implementation intent.

Execution provenance may retain opaque references to the effective HarnessComposition, Surface bindings and material world sufficient to explain which body/environment carried the act. Ownership remains with the respective systems.

## 89. Surface availability and identity

Surface availability is operational state, not identity truth.

The following transitions must be representable without Agent identity drift:

```text
Telegram adapter unavailable
    → messaging Surface degraded/absent
    → CLI Surface remains available

HTTP endpoint rebound
    → same API Surface purpose
    → new material binding/provenance

Harness gateway process restarted
    → session may persist or be replaced according to target truth
    → Agent identity unchanged unless an external semantic event says otherwise

agent moved from local host to remote Workcell
    → material binding changes
    → authored Project/Run/Agent identity unchanged
```

AIKit must report the real target truth about session replacement, runtime-body change and Surface availability rather than infer continuity merely from a stable semantic ref.

## 90. Acceptance laws

Persistent-agency/hosting integration is acceptable when fixtures prove at least:

1. one Agent/Harness/session arrangement can expose several Surfaces without multiplying Agent identity;
2. CLI/TUI/messaging/API/webhook Surfaces remain semantically distinct even when one target gateway process serves several of them;
3. a Surface can become unavailable while Agent and unrelated Surfaces remain intact;
4. rebinding a Surface to a different Workcell endpoint changes material provenance without changing the Surface's canonical relation where target semantics preserve it;
5. restarting a communication adapter/gateway process does not automatically create a new Agent;
6. replacing an AgentSession remains separately observable from restarting a material service;
7. Workcell endpoint/process/container IDs cannot replace Agent, Harness, Surface, Action, Capability, Project or Run refs;
8. a Workcell Control Service is not misclassified as an agent gateway;
9. Hermes/OpenClaw adapters preserve their actual current protocol/service meanings rather than being translated into a universal gateway ontology;
10. a thin harness with only local CLI/stdio access remains valid; persistent multi-Surface hosting is additive, not mandatory.

These laws extend `09-COMPOSABLE-RUNTIME-ENVIRONMENTS.md`; they do not replace its Component/Contract/Surface/HarnessComposition model.
