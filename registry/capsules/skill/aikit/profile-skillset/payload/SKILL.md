---
name: aikit-profile-skillset-management
description: Manage Profile and SkillSet source state through previewable AIKit resolution rather than editing generated projections.
---

# Profile and SkillSet management

Semantic ref: `aikit:profile-skillset`. Native owner: `EpiLogos/ai-kit`.

Profile is a resolution input; SkillSet is an additive projection request. Two SkillSet relations must remain distinct:

- **Project → SkillSet selection**: the Project selects named SkillSets, possibly alongside inherited/default selections;
- **SkillSet → Capability membership**: the SkillSet names explicit/pattern-derived members which are then projected against effective resolution.

Neither relation carries trust or authority. SkillSets compose by union and have no semantic precedence/reorder operation; any optional presentation order is not resolution authority.

## Operation

Use AIKit's shared Profile/SkillSet composition application operation. The operation is UI-neutral: CLI, TUI and agent surfaces may project it differently, but all consumers must preserve the same canonical capability identities, authored/effective distinction, resolver basis, staged intent, changed-ground preview and apply evidence.

## Intent

Change authored Profile activation or a supported writable SkillSet relation while preserving ownership, provenance, provider gates and the difference between authored intent and effective resolution.

## Preconditions

- Inspect the current Project/Profile/scope and record the resolver `catalog_revision` + `resolution_hash` basis.
- Inspect authored Profile state separately from effective Profile state.
- Inspect Project → SkillSet selections as authored / inherited / effective / unavailable where applicable.
- Inspect SkillSet → Capability membership separately: identity, provenance, writability, membership and withheld/effective member states.
- Treat observed SkillSets as read-only until an explicit adoption/ownership operation exists.
- Do not interpret either SkillSet relation as activation, trust, fitness or authority.

## Procedure

1. **Inspect.** Read the shared composition model: Project world, authored Profile, effective resolution, Project → SkillSet selections, SkillSet → Capability membership/projection, warnings and current basis.
2. **Stage.** Stage only typed source intent. Profile capability changes are `enable`/`disable`; Project → SkillSet selection is `add`/`remove` only when the shared application owner exposes that writable operation; SkillSet membership is `add`/`remove`. Staging must not write source state.
3. **Preview.** Re-run the canonical resolver for staged authored Profile state and inspect authored-before/after, effective-before/after, withheld/unavailable members, warnings and `changed_ground`. For store-owned SkillSet membership, inspect the exact reversible Procedure diff. Do not infer effective state from an authored diff.
4. **Validate basis.** Before apply, compare the accepted resolver preview's `basis_before` with the live catalog revision and resolution hash. If they differ, stop with `composition.preview_stale` and preview again. Store-owned membership apply must run the exact Procedure that was reviewed, whose world preconditions protect the review/apply interval.
5. **Apply.** Use AIKit-owned mutation authority for the affected source. Do not edit generated harness projections, renderer state or generated resolution artifacts as the authoring mechanism. Never convert selection/membership into a trust decision.
6. **Re-resolve.** Read the new effective resolution and compare it with the previewed `basis_after` / changed ground.
7. **Receipt.** Report the resulting immutable Generation and Procedure/source mutation evidence where the owning operation provides them, together with warnings and changed ground.
8. **Drift/degradation.** If a provider or related Resource becomes unavailable while intent remains authored, report degradation/unavailability under the same canonical identity. Do not substitute a different identity silently.

## Current authority boundary

If the shared application layer can inspect or stage a relation but does **not** yet expose a canonical preview/apply owner for it, stop at that boundary and report it. In particular, do not bypass the shared operation by directly rewriting a Project specification merely because a lower-level compatibility CLI has a writer.

## Expected evidence

A successful operation should make these fields inspectable to the agent and to human surfaces using the same application data:

- mutation scope and canonical refs;
- authored Profile before/after;
- effective Profile before/after;
- Project → SkillSet selection state/provenance where relevant;
- SkillSet → Capability identity/provenance, membership and projection delta where relevant;
- `basis_before` and `basis_after`;
- `changed_ground` (effective capabilities, declarations, unavailable states and warnings that changed);
- target/projection effects when the application operation exposes them;
- immutable Generation id and/or exact Procedure receipt where applicable.

## Failure and degradation handling

- `composition.preview_stale`: state changed materially after preview; do not apply, re-inspect/re-preview.
- `composition.skillset_read_only`: the SkillSet is observed; do not mutate it in place.
- `composition.skillset_not_found`: do not create a new semantic owner implicitly; resolve the intended SkillSet first.
- trust/policy/platform/target/provider withholding: preserve the authored relation and report why it is not effective/projected.
- ContextSource association must not retrieve/load content merely because it is selected; selection, retrieval and context-pack materialisation remain separate operations.

## Verification

Use the shared core Profile-composition tests plus existing SkillSet resolution/projection tests, the store Procedure-backed membership tests and native-Skill verifier. Acceptance requires that staging is write-free, preview is resolver-backed, stale preview is rejected, the exact reviewed Procedure is the apply authority for writable SkillSet membership, and an untrusted/incompatible member can remain named in a set while being withheld from effective projection. The Skill must never encode terminal coordinates, TUI tabs or keybindings as canonical operation semantics.