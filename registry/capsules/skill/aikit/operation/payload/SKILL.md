---
name: aikit-operation
description: Orient and operate AIKit from application state, never UI-private steps. Proactively run aikit status and aikit explain at the start of work in an AIKit-governed context, and route every question about why a capability is active, withheld, or degraded through aikit explain rather than guessing from config files; near-miss: knowledge-navigation answers recorded-knowledge questions, operation establishes AIKit's own state.
---

# AIKit operation and Wayfinder

Semantic ref: `aikit:operation`. Native owner: `EpiLogos/ai-kit`.

Use this Skill to orient an Agent or human in AIKit from day one. Existing Wayfinder/default foundation Skills remain separate authoritative members; do not copy their bodies here.

## Procedure

1. Establish the current Project, Profile, scope chain, actor/session and target client/harness from AIKit's context surfaces.
2. Inspect resolved capability/SkillSet state and source provenance before assuming a tool or Skill is usable. Availability, trust, policy/platform compatibility and target projection remain independent gates.
3. Use Wayfinder/ProjectMap and the resolved information horizon to orient to the work. Retrieve progressively; do not stuff the entire available horizon into a prompt.
4. Inspect proposed projection/materialisation before applying changes. A projected Skill is a derived copy; its native/managed source and revision remain authoritative.
5. Request operations through AIKit's canonical CLI/application/domain seams. TUI/desktop/harness projections may present the same operation differently but do not change its semantics.
6. Use **Explain** to inspect why a Resource is present, unavailable, staged, projected, degraded or learned-easy. Preserve the evidence class actually owned by the source: `Authored`, `Observed`, `Derived`, `Learned` or `Generated`. Do not upgrade a projected `LiveMounted` activation mode into an observation that a target is live.
7. Use **History** as a cross-domain read over existing authorities, not as a second event database. Distinguish recent observed runs, learned destination/KnowledgeRoute use, immutable Generations, Procedures, SessionSpace receipts and runtime-body fingerprints. A route may be replayable navigation without becoming relation truth.
8. Treat **Changed** as a comparison of immutable evidence where available. Compare committed Generation locks directly rather than rerunning today's resolver and calling the result historical truth.
9. Recover only through the authority that owns recovery. A SessionSpace historical state is restaged through the current SessionSpace basis; Procedure undo remains Procedure-owned; arbitrary old Generations are inspectable unless an explicit current-authority recovery operation exists.
10. Preserve canonical Resource identity across History. Provider-native pane/window/plugin identifiers are provenance, not replacement identity. One Action remains one Action across Surfaces; a Reading or knowledge object does not become an Action merely because a TUI or CLI exposes it.
11. Explain withheld members and degraded surfaces rather than silently substituting authority or trust.
12. Preserve exact source/revision, effective-resolution and historical evidence when handing state to another Agent or product.

## Authority boundary

A Skill teaches procedure. It does not grant a Capability or authorise an Action. Selecting an operator SkillSet does not establish Root Agency, metagency or repository authority. Explain/History are evidence faculties: they may project or compare owner-held evidence, but they do not acquire mutation, provider, resolver or persistence authority by doing so.

## Verification

Use the repository acceptance/set/projection tests and inspect source provenance in the effective view. For SkillSet behaviour, `aikit set show` must expose withheld members and reasons rather than treating membership as activation. For Explain/History, verify that CLI, TUI and agent operation consume the same application evidence model, that no TUI-local history state becomes authoritative, and that recovery paths preserve the owning domain's preview/basis/apply law.


## Prepared NOW context and Jev

When an undertaking selects Redis-backed NOW preparation, treat it as AIKit's operative-context delivery path, not as a separate memory product.

1. Inspect the selected store with `aikit now-context status --config-file <redis-config>`. A selected required store must be available; an optional selected store degrades explicitly and leaves the ordinary encounter path intact.
2. Prepare through `aikit now-context prepare --request-file <request>`. The request names the existing participant, NOW, Project, disclosure revision, Central source routes, optional Factory Run/workflow scope, Wiki queries and either `all` or general Jev selection. Do not reconstruct those identities from Redis keys.
3. Use `aikit now-context inspect --participant-ref <ref> --config-file <redis-config>` to read the prepared version, independent participant change cursor and last delivery receipt. A warm eligible view is reusable without another Jev call.
4. Material source, dependency, Factory Return or disclosure changes are appended as typed participant changes. Heartbeats and ordinary cache reads are not semantic changes and do not justify provider inference.
5. Configured encounter entry, addressed delivery, queued re-entry and continuation attach the participant-specific prepared envelope **before the provider turn**. The acting model does not need to remember a context-planning tool call. The durable receipt records the exact prepared version/digest and change cursor that crossed the boundary.
6. Revocation is an access decision, not key deletion. `aikit now-context revoke` makes a cached view unreadable at the native store boundary; delivery rechecks disclosure and Project/session identity.
7. General `aikit jev` remains available for caller-authored typed questions. Reusable document, matrix, Wiki and development questions should be expressed through the relevant Skill/Method, but no Skill turns Jev into an authority or requires an extra LLM approval step.

Canonical sources, Wiki knowledge, Factory evidence, Central continuations and Actuation authority remain with their owners. Redis is a hot participant-specific projection over those relations. Do not inspect raw Redis keys as a substitute for the native commands.
