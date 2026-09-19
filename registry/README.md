# AIKit first-party operational Skills

This directory is the AIKit first-party registry. It publishes AIKit-owned Skills through the **existing capsule and SkillSet model** — the same shape the loader consumes from installed homes — so a committed invalid capsule fails CI (`builtin_registry.rs`) instead of surfacing as a runtime problem on a user's machine. It is source material for a managed registry/install path, not a second registry implementation.

`capsules/` follows the same manifest/payload contract as `examples/registry`; a capsule's directory must agree with its declared id (`<kind>/<namespace>/<name>`), enforced by the loader's anti-masquerade check. `skillsets/` publishes explicit member lists plus stable semantic refs for the small default sets requested by AIKit #73. `fixtures/` holds authoring example skills.

The registry may also contain a deliberately small first-party `guidance` capsule when project-facing orientation belongs in AIKit's existing bounded guidance composer rather than in Skill procedure. The distinction is intentional:

```text
Guidance
    collaboration temperament / orientation injected into agent context

Skill
    reusable procedure an authorised agent invokes when the task requires it
```

The existing Wayfinder/default foundation members remain authoritative where they already exist. These operational Skills compose with that foundation; they do not clone its bodies.

## Vendored default skillsets

The repo's own `.aikit/profile.toml` and the recommended default foundation
(ADR 0002, `mattpocock/wayfinder-foundation`) declare seven skills that must
resolve wherever this product ships — on a fresh clone and on a fresh install.
This registry therefore carries them as **vendored** capsules, byte-identical
to the reviewed skill-source snapshots, so a fresh `AIKIT_HOME` materialised
from this tree resolves every declared id (`first_party.rs` materialises the
registry into `<home>/registries/ai-kit` when absent). They keep their
source-qualified ids — the namespace is the provenance, per ADR 0002 — and,
like every Skill capsule, they stay inactive until the operator records trust
(`aikit trust record`); vendoring ships the bytes, it does not pre-review them.

```text
skill/mattpocock/engineering/wayfinder
skill/mattpocock/engineering/setup-matt-pocock-skills
skill/mattpocock/engineering/domain-modeling
skill/mattpocock/engineering/prototype
skill/mattpocock/engineering/research
skill/mattpocock/productivity/grilling
    upstream  https://github.com/mattpocock/skills.git
    revision  2ab958093e83e0ec752e6c1c5932da465bf23e0c
    source    skill source `mattpocock`, snapshot da1ea3e2d10c023334058bdecc0f4c5bc208c9a434a37f3a064d1a397600bb7d
    license   MIT (per the upstream repository)

skill/writing-guidance-tools/writing-guidance-tools
    author    owner-authored; canonical at the Antykathera-Essay-Work
              writing-guidance-tools directory (no upstream repo)
    source    skill source `writing-guidance-tools`,
              snapshot 9c8bedfece4a60996c0033bc19f8d42cba9af012d363cf826d5be191aac8db27
```

Upstream updates do not flow automatically: refreshing a vendored capsule is a
deliberate act — re-sync the skill source, re-copy the snapshot, and record the
new revision here. `scripts/verify-native-skills.py` pins the vendored ids
beside the AIKit-authored ones, so removing one while the profile still
declares it fails CI.

## Project understanding and account craft

The project-author SkillSet includes a small compositional authoring family:

```text
product-understanding
    establish meaning and current reality through provenance
        ↓
structured-account-authoring
    compose a coherent source-aware reading without creating new canon
        ↓
projection-authoring
    select/review/ratify the reading for O:I Projection when required
        ↓
html-account
    render the same reading as a standalone portable HTML artifact when required
```

The arrows describe useful composition, not an automatic pipeline. A task can invoke one Skill without invoking the others.

A straightforward code fix should normally stop at the implementation and evidence it needs. Opening Central `Control/user` must not automatically create an HTML account. Deep account craft becomes appropriate when the human is clarifying a whole, product understanding matters, documentation or design is requested, a Projection is being prepared, or returned reality requires renewed understanding.

The authoring Skills do not assume that Central, a Factory Project, a Wiki space, and an ordinary filesystem project share one ontology. They preserve native source authority and use the smallest sufficient depth for the task.

Invariants:

```text
Skill available != Capability granted
SkillSet member != trusted member
SkillSet selected != Root position / metagency
projected Skill copy != authoritative Skill source
successful use != automatic source promotion
account reading != canonical source
HTML rendering != canonical source
Projection refinement != silent source mutation
```
