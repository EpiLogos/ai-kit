---
name: aikit-profile-skillset-management
description: Manage Profile and SkillSet source state through previewable AIKit resolution rather than editing generated projections.
---

# Profile and SkillSet management

Semantic ref: `aikit:profile-skillset`. Native owner: `EpiLogos/ai-kit`.

Profile is a resolution input; SkillSet is an additive projection request. SkillSet membership never carries trust or authority. SkillSets compose by union and have no semantic precedence/reorder operation; any optional presentation order is not resolution authority.

## Operation

Use AIKit's shared Profile/SkillSet composition application operation. The operation is UI-neutral: CLI, TUI and agent surfaces may project it differently, but all consumers must preserve the same canonical capability identities, authored/effective distinction, resolver basis, staged intent, changed-ground preview and apply evidence.

## Intent

Change authored Profile activation or writable SkillSet membership while preserving ownership, provenance, provider gates and the difference between authored intent and effective resolution.

## Preconditions

- Inspect the current Project/Profile/scope and record the resolver `catalog_revision` + `resolution_hash` basis.
- Inspect authored Profile state separately from effective Profile state.
- Inspect the relevant SkillSet identity, provenance, writability, membership and withheld/effective member states.
- Treat observed SkillSets as read-only until an explicit adoption/ownership operation exists.
- Do not interpret SkillSet membership as activation, trust, fitness or authority.

## Procedure

1. **Inspect.** Read the shared Profile composition model: authored Profile, effective resolution, SkillSet relations, warnings and current basis.
2. **Stage.** Stage only typed source intent. Profile capability changes are `enable`/`disable`; SkillSet relation changes are `add`/`remove`. Staging must not write source state.
3. **Preview.** Re-run the canonical resolver for the staged authored Profile and inspect authored-before/after, effective-before/after, withheld/unavailable members, warnings and `changed_ground`. Do not infer effective state from the authored diff.
4. **Validate basis.** Before apply, compare the accepted preview's `basis_before` with the live catalog revision and resolution hash. If they differ, stop with `composition.preview_stale` and preview again.
5. **Apply.** Use AIKit-owned mutation authority for the affected source. Do not edit generated harness projections, renderer state or generated resolution artifacts as the authoring mechanism. Never convert membership into a trust decision.
6. **Re-resolve.** Read the new effective resolution and compare it with the previewed `basis_after` / changed ground.
7. **Receipt.** Report the resulting immutable Generation and Procedure/source mutation evidence where the owning operation provides them, together with warnings and changed ground.
8. **Drift/degradation.** If a provider or related Resource becomes unavailable while intent remains authored, report degradation/unavailability under the same canonical identity. Do not substitute a different identity silently.

## Expected evidence

A successful operation should make these fields inspectable to the agent and to human surfaces using the same application data:

- mutation scope and canonical refs;
- authored Profile before/after;
- effective Profile before/after;
- SkillSet identity/provenance and relation delta;
- `basis_before` and `basis_after`;
- `changed_ground` (effective capabilities, declarations, unavailable states and warnings that changed);
- target/projection effects when the application operation exposes them;
- immutable Generation id and Procedure/source receipt where applicable.

## Failure and degradation handling

- `composition.preview_stale`: state changed materially after preview; do not apply, re-inspect/re-preview.
- `composition.skillset_read_only`: the SkillSet is observed; do not mutate it in place.
- `composition.skillset_not_found`: do not create a new semantic owner implicitly; resolve the intended SkillSet first.
- trust/policy/platform/target/provider withholding: preserve the authored relation and report why it is not effective/projected.
- ContextSource association must not retrieve/load content merely because it is selected; selection, retrieval and context-pack materialisation remain separate operations.

## Verification

Use the shared core Profile-composition tests plus existing SkillSet resolution/projection tests and native-Skill verifier. Acceptance requires that staging is write-free, preview is resolver-backed, stale preview is rejected, and an untrusted/incompatible member can remain named in a set while being withheld from effective projection. The Skill must never encode terminal coordinates, TUI tabs or keybindings as canonical operation semantics.