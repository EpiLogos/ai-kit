---
name: aikit-skill-authoring
description: Author executable procedural knowledge as managed Skill or Method praxis with explicit source, triggers, operations, authority assumptions and verification.
---

# Skill and Method authoring

Semantic ref: `aikit:skill-authoring`. Native owner: `EpiLogos/ai-kit`.

A **Skill** is reusable organised intelligent praxis. A **Method** is a Focus-bearing situated composition of independently owned Skills, UsageOverlay receipts, Actions/Capabilities, ContextSources, Project/domain refs and verification/expected-return forms. A **SkillSet** is only additive repertoire. Profile/ContextResolution determine why and where resources become operative.

Do not make a Skill absorb a Project merely because the Project needs it, and do not make a SkillSet become a workflow engine merely because several Skills are commonly used together.

## Skill procedure

1. State the procedure's owner and the user/Agent situation that should trigger it. A Skill should route work through the owner's public application/domain contracts rather than UI coordinates or private runtime mutation.
2. Recover the target Project vocabulary and applicable source/contract chain before naming durable procedural distinctions. When stable SemanticWiki/ProjectMap/source/code refs already exist, point to them instead of restating architecture in prompt prose.
3. Create the ordinary Agent Skill body (`SKILL.md`) and package it with the existing AIKit capsule manifest (`kind = "skill"`, `[skill] root = "payload"`). Keep capsule id/path aligned.
4. Include purpose/triggers, inputs, semantic operations, required Capabilities/Actions where relevant, risk/permission boundaries, outputs and verification/conformance paths.
5. Make source/projection status explicit: repository/managed source + exact revision is authoritative; harness projections are derived.
6. Prefer stable semantic steps such as `inspect -> stage -> preview -> apply -> verify` over key presses or panel positions.
7. Add examples/fixtures where they prove behaviour. For an authoring Skill, prove that it can guide creation of a small valid Skill that passes the same structural rules.
8. Submit source/revision to the managed source review/trust lifecycle. Repeated success or fitness evidence may inform review but never promotes source automatically.

## UsageOverlay before source mutation

When an unchanged Skill is broadly correct but a user/Project/Focus needs a small situated orientation, use the existing scoped Skill Usage Overlay mechanism rather than forking or rewriting the Skill source.

Keep the distinction explicit:

```text
Skill source                         reusable owned praxis
UsageOverlay                         scoped adaptation of that unchanged Skill
reviewed-against / digest            exact adaptation evidence
Effective Skill projection           derived harness-facing material
```

A repeated useful overlay may create evidence for a Project Method or later reusable Skill refinement, but repeated use is not automatic promotion and the overlay does not become Skill source.

## When to author a Method

Author a Method when the useful durable thing is **the contextual relation among independently owned resources around a purpose/Focus**, rather than a new reusable Skill body.

A Method should be able to retain stable refs to:

```text
Focus / Project / domain
SkillRef(s)
UsageOverlay ref/digest(s)
ActionRef(s) / CapabilityRef(s)
ContextSourceRef(s)
verification refs
expected return forms
```

Do not copy the referenced bodies into the Method. Do not encode trust, activation authority or Profile precedence in Method membership. Do not convert the Method into a sequence DSL merely to make its composition look procedural; order belongs only where the actual practice requires and owns order.

Before writing a Project Method:

1. Recover the Project's actual language/ontology and the stable refs already expressing it.
2. Check whether an existing reusable Skill plus UsageOverlay is sufficient. If so, stop there.
3. Check whether the need is merely additive availability. If so, use SkillSet rather than Method.
4. Compose only the refs materially germane to the Focus and state the expected verification/return relation.
5. Resolve the selected Method **under** the existing Profile/ContextResolution. Method selection never activates an unavailable capability or bypasses trust/policy/Action authority.
6. Preserve source/revision and immutable overlay digests so Explain/History can reconstruct the praxis condition later.
7. After real use, return fitness evidence as evidence about the Method/overlay/Skill condition; do not silently mutate durable praxis.

## Project vocabulary and structural fidelity

If a Project owns a constitutive ontology, coordinate map, protocol/state machine, schema graph or equivalent structural source, praxis must stay answerable to it. A generic convenient workflow must not flatten target-owned distinctions.

Use the target's stable semantic/source/code refs where possible. If no such structure exists, do not invent one merely to satisfy the authoring procedure. Ordinary Projects remain valid without QL/MEF, Bimba coordinates, Method trees or special local file conventions.

## Specimen

`skills/fixtures/minimal-authored-skill/` is the representative small Skill produced by this procedure. `scripts/verify-native-skills.py` validates both its capsule path/id and its Skill frontmatter.

For Method implementation/conformance, use AIKit's native `Method` / `resolve_method` / `resolve_praxis` contracts and their tests rather than inventing a second Method store in this Skill.
