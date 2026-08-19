---
name: aikit-verification
description: Verify AIKit managed sources, Methods, SkillSets, resolution, Project reflection, projection and provider/application contracts without treating green tests as authority promotion.
---

# Verification and conformance

Semantic ref: `aikit:verification`. Native owner: `EpiLogos/ai-kit`.

## Procedure

1. Identify the changed public contract and run its narrow conformance test first, then the repository baseline appropriate to the change.
2. For capsules/Skills, validate manifest path/id, payload root/frontmatter, source/revision and registry scan behaviour.
3. For UsageOverlays, verify the unchanged Skill source, exact scoped adaptation/review digest and resulting Effective Skill projection; a projected file never becomes source.
4. For Methods, verify source identity/revision, stable independently owned member refs, exact overlay digests where used, expected verification/return forms and resolution under the existing ContextResolution. Method selection must not activate, trust or authorise a referenced resource.
5. For SkillSets, verify explicit membership, additive resolution and withholding of members that fail their own gates. SkillSet union must remain independent from Method composition and Profile precedence.
6. For Project reflection, verify explicit ProjectMap bindings and both semantic→code and code→semantic traversal at bounded depth. Keep authored Ground, SemanticWiki, local structural description, CodeReference, CodeIndex observation, verification/evidence and history differently authoritative.
7. If the target declares a strong reflection law, test missing, wrong, duplicate and stale mappings plus constitutive structural relations. Matching names are not parity; structural flattening must fail even when labels survive.
8. For local structural descriptions, include at least one stale-description/reflection case. The result is discrepancy evidence routed to the source owner, not permission for the verifier to rewrite the description/Wiki/Ground.
9. For projection, verify target-compatible effective material plus provenance/receipt/generation; never bless the projection as authoritative source.
10. For provider/runtime changes, include degraded/absent/unauthorised paths and the real provider activation boundary.
11. Record exact source head, provider/version evidence, tests/workflow run and any skipped external/live owner requirement. Green conformance is evidence for owner review, not self-promotion.

## Project reflection acceptance

A meaningful Project reflection receipt should be able to demonstrate, where those representations exist:

```text
human-authored Ground / Canon
        ↓
SemanticWiki / Project language
        ↕ explicit ProjectMap binding
native local description / scoped contract
        ↕
exact CodeReference
        ↕
CodeIndex structural intelligence
        ↓
verification / evidence / history
        ↺ returned difference
```

From a semantic anchor, prove the route to exact implementation and evidence. From the exact CodeReference, prove the reverse route to known meaning/description/ownership. Provider-native graph richness is not required to be copied into ProjectMap.

For an ordinary contrasting Project with no Central directories, Bimba/QL coordinates, special local-header convention or Method source tree, verify that completed Knowledge Navigation and ordinary native Skill operation still work. Rich reflection/praxis is developmental capacity, not a new minimum-validity condition.

## Strong target-owned conformance

When a target owns a stable coordinate identity and reflection law, consume that law as source-owned conformance input rather than hard-coding the target ontology into AIKit. An Epi/QL specimen may therefore prove the generic mechanism while AIKit remains ignorant of what an `Mx/Mx′`, Bimba category or other target coordinate means.

If the exact owner source required for a stronger claim is unavailable, stop at the strongest honestly source-backed fixture and record the missing owner-level boundary. Do not fabricate a semantic coordinate in order to make the test look complete.
