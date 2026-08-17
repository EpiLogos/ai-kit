---
name: aikit-verification
description: Verify AIKit managed sources, SkillSets, resolution, projection and provider/application contracts without treating green tests as authority promotion.
---

# Verification and conformance

Semantic ref: `aikit:verification`. Native owner: `EpiLogos/ai-kit`.

## Procedure

1. Identify the changed public contract and run its narrow conformance test first, then the repository baseline appropriate to the change.
2. For capsules/Skills, validate manifest path/id, payload root/frontmatter, source/revision and registry scan behaviour.
3. For SkillSets, verify explicit membership, additive resolution and withholding of members that fail their own gates.
4. For projection, verify target-compatible effective material plus provenance/receipt/generation; never bless the projection as authoritative source.
5. For provider/runtime changes, include degraded/absent/unauthorised paths and the real provider activation boundary.
6. Record exact source head, tests/workflow run and any skipped external/live provider requirement. Green conformance is evidence for owner review, not self-promotion.
