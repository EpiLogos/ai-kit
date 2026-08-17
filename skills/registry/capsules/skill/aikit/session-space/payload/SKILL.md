---
name: aikit-session-space-operation
description: Operate SessionSpace semantic state through AIKit's shared preview/apply authority while preserving Project, provider, AgentSession and Surface ownership.
---

# SessionSpace operation

Semantic ref: `aikit:session-space`. Native owner: `EpiLogos/ai-kit`.

A SessionSpace is the persistent semantic human working field through which Projects, AgentSessions, Surfaces and provider/material references are framed. It is **not** an aggregate Project, an aggregate Context, a provider workspace identity, an AgentSession, or a Workcell.

## Operation family

Use the shared SessionSpace application operation for:

- list, show, create, open and discover;
- bind/unbind Project + exact ContextResolution evidence;
- attach/detach AgentSession attachment intent;
- attach/detach Surface attachment intent;
- bind/unbind provider, Host, Workcell and material references without taking native ownership;
- focus;
- reconcile and reconstruct;
- explain and history;
- stage a historical restoration through current authority when eligible.

CLI, TUI and agent surfaces may render these differently, but they must use the same canonical refs, typed intent, preview, basis, apply authority, receipt and read model.

## Identity and authority invariants

Preserve these distinctions at all times:

```text
SessionSpace identity != Project identity
Project A            != Project B
Project binding      != ContextResolution
provider identity    != SessionSpace identity
Host                 != Project
Workcell/material    != Surface
material process     != AgentSession
transport reconnect  != AgentSession continuity
provider observation != authored durable state
```

SessionSpace may record a stable reference to provider/Host/Workcell/material state, but the native provider remains authoritative for that object. Do not persist window/pane/layout geometry as canonical SessionSpace semantics.

## Project + Context binding

Each Project binding must retain its exact `ProjectRef` and an independent, attributable ContextResolution evidence ref. The evidence basis includes the canonical resolver hash, catalog revision, ordered scope provenance, ContextSource refs, Host reference and exact ProjectBinding that produced the resolution.

Never merge several Projects into an aggregate Project or flatten their ContextResolution provenance into a space-level resolver. If SessionSpace intent affects Project resolution, make that input explicit at the owning Project/Context operation and preserve it in the resulting evidence.

## Durable mutation law

1. **Inspect.** Read current canonical SessionSpace semantic state separately from live provider/runtime observation.
2. **Stage typed intent.** Staging is write-free. Do not directly edit the canonical JSON file or generated provider artifacts.
3. **Preview.** Inspect the exact proposed semantic state and `changed` relation set.
4. **Validate basis.** The accepted preview carries revision + state hash. If canonical state changed before apply, stop with `session_space.preview_stale` and preview again.
5. **Apply.** Apply through `SessionSpaceApplicationStore`, which re-reads under the shared cross-process lock and re-stages the accepted typed intent. Preview/apply parity is required.
6. **Receipt.** Preserve the structured application receipt with before/after state, basis, changed relations and resulting basis.
7. **Re-read.** Read canonical state again. Do not infer live provider success from a durable receipt.
8. **Explain / History.** Consume receipts, ContextResolution evidence and provider observations. Never rerun resolver semantics inside Explain or History.

## Reconstruction and reconciliation

Reconstruction must distinguish:

- **restored canonical semantic state** — AIKit-owned SessionSpace identity and authored bindings restored from canonical persistence;
- **re-observed provider state** — the same stable native reference is visible again;
- **re-established relation** — the native owner supplied sufficient evidence that the relation continues;
- **unavailable relation** — authored reference remains but native state is absent;
- **degraded relation** — some transport/provider state returned but semantic continuity is not proven;
- **irrecoverable provider-native detail** — geometry or transient provider detail which AIKit never owned canonically.

A transport reconnect, matching provider-native session id, or reopened terminal does **not** prove AgentSession continuity. Only continuity evidence from the AgentSession owner can upgrade that relation to re-established.

## Explain

Explain is evidence disclosure, not resolution. For SessionSpace state disclose as available:

- what the space and relation are;
- exact canonical refs and native owner;
- why the relation is present or unavailable;
- authored vs effective/observed state;
- ContextResolution basis/provenance;
- provider/Host/Workcell/material reading;
- focus;
- receipt/operation that changed it;
- persisted vs live state;
- degradation/reconstruction status.

Where Project/Profile/SkillSet, HarnessComposition, Generation, Procedure, Knowledge or familiarity evidence is requested, consume their existing evidence owners. Do not copy their resolver logic into SessionSpace.

## History

History is the immutable SessionSpace application-receipt sequence stored with canonical SessionSpace state. It supports previous-state comparison and can select a prior receipt as a restoration target, but restoration is always staged and applied through the current SessionSpace authority. History itself never writes state.

Keep distinct:

```text
provider/runtime lifecycle history
!= SessionSpace application receipt history
!= Generation history
!= Procedure history
!= learned familiarity
```

Familiarity may explain learned ease or recency but is not trust, authored preference, fitness, or provider truth.

## Provider boundary

Consume current cmux/tmux/IDE and ACP/classic provider observations through their native contracts. Do not encode mux keybindings, IDE layout rules, ACP framing, classic transport details, provider reconnect rules or Workcell materialisation semantics in this Skill.

## Verification

Representative acceptance should prove the same semantic operation through CLI, TUI/application adapter and this native Skill over one canonical read/preview/apply seam. Required semantic tests include independent two-Project ContextResolution provenance, bind/unbind, AgentSession and Surface attachment intent, focus, native-reference disclosure, write-free staging, stale-preview rejection, restart/reopen, absent/returned provider behaviour, no inferred AgentSession continuity, Explain receipt parity, History comparison and receipt-backed restoration through current authority.