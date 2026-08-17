---
name: aikit-provider-authoring
description: Extend AIKit with provider/adaptor implementations through accepted public provider and registry seams rather than private resolution mutation.
---

# Provider SDK authoring

Semantic ref: `aikit:provider-authoring`. Native owner: `EpiLogos/ai-kit`.

## Procedure

1. Identify the existing AIKit provider/adapter contract that owns the integration: capability source, context/knowledge provider, harness/component provider, QL provider or another accepted public seam.
2. Implement against exported contracts and keep provider identity/version/capability advertisement inspectable. Do not mutate effective Context/Projection state directly.
3. Preserve source trust and review as AIKit concerns independent of provider fitness. A provider reporting a capability does not grant it to a Profile/actor.
4. Add a representative external-style fixture/test using the public contract. Include absent/degraded/incompatible/unauthorised cases where the seam supports them.
5. Run the owning crate's provider conformance plus the relevant resolver/application test. Record exact provider/source revision and configuration evidence.
6. Submit through the managed source/provider review lifecycle; never make one successful session automatic provider promotion.

## Representative specimens

Use current public-provider tests in `crates/aikit-core/tests/` (including QL provider and knowledge/provider conformance where relevant) as the pattern: advertise through public traits/contracts, consume through AIKit resolution, and preserve provider provenance.
