---
name: aikit-operation
description: Operate AIKit context, Wayfinder, capability resolution and projections from semantic/application state rather than UI-private steps.
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
