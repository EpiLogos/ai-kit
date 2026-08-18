# AIKit first-party operational Skills

This directory publishes AIKit-owned Skills through the **existing capsule and SkillSet model**. It is source material for a managed registry/install path, not a second registry implementation.

`registry/capsules/` follows the same manifest/payload contract as `examples/registry`. `skillsets/` publishes explicit member lists plus stable semantic refs for the small default sets requested by AIKit #73.

The registry may also contain a deliberately small first-party `guidance` capsule when project-facing orientation belongs in AIKit's existing bounded guidance composer rather than in Skill procedure. The distinction is intentional:

```text
Guidance
    collaboration temperament / orientation injected into agent context

Skill
    reusable procedure an authorised agent invokes when the task requires it
```

The existing Wayfinder/default foundation members remain authoritative where they already exist. These operational Skills compose with that foundation; they do not clone its bodies.

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
