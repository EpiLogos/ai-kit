# AIKit first-party operational Skills

This directory publishes AIKit-owned Skills through the **existing capsule and SkillSet model**. It is source material for a managed registry/install path, not a second registry implementation.

`registry/capsules/` follows the same manifest/payload contract as `examples/registry`. `skillsets/` publishes explicit member lists plus stable semantic refs for the small default sets requested by AIKit #73.

The existing Wayfinder/default foundation members remain authoritative where they already exist. These operational Skills compose with that foundation; they do not clone its bodies.

Invariants:

```text
Skill available != Capability granted
SkillSet member != trusted member
SkillSet selected != Root position / metagency
projected Skill copy != authoritative Skill source
successful use != automatic source promotion
```
