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
6. Explain withheld members and degraded surfaces rather than silently substituting authority or trust.
7. Preserve exact source/revision and effective-resolution evidence when handing state to another Agent or product.

## Authority boundary

A Skill teaches procedure. It does not grant a Capability or authorise an Action. Selecting an operator SkillSet does not establish Root Agency, metagency or repository authority.

## Verification

Use the repository acceptance/set/projection tests and inspect source provenance in the effective view. For SkillSet behaviour, `aikit set show` must expose withheld members and reasons rather than treating membership as activation.
