---
name: aikit-profile-skillset-management
description: Manage Profile and SkillSet source state through previewable AIKit resolution rather than editing generated projections.
---

# Profile and SkillSet management

Semantic ref: `aikit:profile-skillset`. Native owner: `EpiLogos/ai-kit`.

Profile is a resolution input; SkillSet is an additive projection request. SkillSet membership never carries trust or authority.

## Procedure

1. Inspect the current Project/Profile/scope context and the effective resolution before proposing a change.
2. Locate the authoritative managed Skill source and the source definition of the relevant SkillSet. Do not edit a harness projection as the normal authoring mechanism.
3. Stage the smallest source change: add/remove the intended capsule/member or Profile selection while preserving explicit provenance.
4. Preview the resulting set/effective resolution with the canonical set/resolution read surface. Inspect withheld members and their trust/policy/platform/target reasons.
5. Apply through AIKit's normal Profile/Project/SkillSet source operation. Never convert membership into a trust decision.
6. Re-resolve and verify the effective view, source revision and projection receipt/generation where available.
7. If drift changes the managed source revision, surface it for review; do not silently replace the previously projected source.

## Verification

Use the existing SkillSet CLI tests and resolution/projection tests. Acceptance requires that an untrusted/incompatible member can remain named in a set while being withheld from effective projection.
