---
name: aikit-skill-authoring
description: Author executable procedural knowledge as a managed Skill capsule with explicit source, triggers, operations, authority assumptions and verification.
---

# Great-Skill authoring

Semantic ref: `aikit:skill-authoring`. Native owner: `EpiLogos/ai-kit`.

## Procedure

1. State the procedure's owner and the user/Agent situation that should trigger it. A Skill should route work through the owner's public application/domain contracts rather than UI coordinates or private runtime mutation.
2. Create the ordinary Agent Skill body (`SKILL.md`) and package it with the existing AIKit capsule manifest (`kind = "skill"`, `[skill] root = "payload"`). Keep capsule id/path aligned.
3. Include purpose/triggers, inputs, semantic operations, required Capabilities/Actions where relevant, risk/permission boundaries, outputs and verification/conformance paths.
4. Make source/projection status explicit: repository/managed source + exact revision is authoritative; harness projections are derived.
5. Prefer stable semantic steps such as `inspect -> stage -> preview -> apply -> verify` over key presses or panel positions.
6. Add examples/fixtures where they prove behaviour. For an authoring Skill, prove that it can guide creation of a small valid Skill that passes the same structural rules.
7. Submit source/revision to the managed source review/trust lifecycle. Repeated success or fitness evidence may inform review but never promotes source automatically.

## Specimen

`skills/fixtures/minimal-authored-skill/` is the representative small Skill produced by this procedure. `scripts/verify-native-skills.py` validates both its capsule path/id and its Skill frontmatter.
