---
name: aikit-knowledge-navigation
description: Navigate project/context knowledge through AIKit's source pool and knowledge operations while preserving source ownership and revisions.
---

# Context and Knowledge Navigation

Semantic ref: `aikit:knowledge-navigation`. Native owner: `EpiLogos/ai-kit`.

Use the canonical application operation family rather than terminal keybindings or provider-specific commands:

```text
search
read
relations
route / traverse
frame / context pack
sources
explain
history
status
forget
```

Provider status/degradation is an accompanying disclosure surface, not a replacement for any operation above.

The production CLI exposes the same faculty as `aikit knowledge search|read|relations|route|frame|sources|explain|history|status|forget`. Search returns typed addresses that can be passed back as JSON; `wiki=REF`, `source=REF` and `project=REF` are convenience forms for stable address classes. CLI and TUI consume the same application service; neither surface is a provider-specific semantic owner.

## Procedure

1. Read the current Context as operative world + information horizon + focus; distinguish what is addressable from what is already loaded.
2. Inspect declared Context/knowledge sources, ProjectMap/code navigation and SemanticWiki/index surfaces available to the current Project/Profile.
3. Search and read progressively around the current Claim/question. Prefer source-owned refs and exact revisions; do not turn a retrieved projection into the canonical source.
4. Use `relations` for bounded provider-native neighbourhoods and `route` for the ordered path actually traversed. A repeated route may become familiar but never becomes a Wiki/code/source relation, trust signal or authored preference.
5. Use `frame`/context-pack projection to collect selected refs, ordered route, readings, source evidence, revision/freshness, explanations and explicit absences for the current act. A frame is derived retrieval material, not a new canonical ContextSource.
6. Use `sources` and `explain` to preserve provider/lens/authority/provenance distinctions. Keep authored, observed, derived and learned evidence distinguishable. When familiarity affects search, retain provider-native relevance separately from application-level navigation score and expose destination/route familiarity evidence rather than silently rewriting provider rank.
7. Use `history` to recover durable AIKit-owned route/frame receipts in the same Project/actor/Focus context without manufacturing provider graph history. `forget` resets learned familiarity influence only; it does not erase canonical Resource identity, provider truth or the operation audit trail.
8. When a provider is absent/degraded, return that state honestly and use other eligible sources only if the effective context authorises them. Baseline address/fuzzy navigation must remain useful without rich providers or QL/MEF.

## ProjectCentral local Wiki worlds

When a Project exposes Central's `central.project/v1` binding, consume it as a native Knowledge Navigation source relation rather than inventing an aperture document or another graph.

Keep the standing relation explicit:

```text
ProjectCentral/user/**                     human-authored Project source
ProjectCentral/agents/governance/**        human-authored Agent governance
ProjectCentral/agents/wiki/wiki.json       Agent-maintained semantic knowledge
ordinary native Project source             authored/observed according to provenance
observed evidence                          != authored
inferred / derived reading                 != observed
```

Project entry should establish Project identity, human-source availability, governance availability, canonical Agent Wiki identity, adopted Wiki sources, native Project source and optional Control root Wiki **without retrieving the whole tree or Wiki**. Preserve the ContextSource progression `exists -> known-to-exist -> askable -> retrieved -> focused`; a broad information horizon is not prompt inclusion.

Respect `.no-agent-retrieval` recursively before source disclosure. A human may freely organise `ProjectCentral/user/**`; arbitrary or non-text material may be known to exist even when the current provider cannot interpret its contents.

For Agent Wiki maintenance:

1. Inspect the canonical Wiki revision and the exact source revisions cited by affected Wiki objects.
2. Distinguish stale, conflicting and merely additional knowledge.
3. Form the smallest coherent WikiNode/WikiEdge/reading changes and preserve exact source/provenance plus producer/run or generation refs when available.
4. Validate the proposed whole through the existing SemanticWiki index before persistence. Adopted Wikis remain participating sources and do not replace the canonical Wiki.
5. Persist Project knowledge only to `ProjectCentral/agents/wiki/wiki.json` (and maintain `Control/agents/wiki/wiki.json` only when the root Wiki is explicitly the target). Derived indexes remain rebuildable state.
6. If returned implementation/runtime evidence challenges human-authored Project source, represent the pressure as a human-source revision proposal / Decision pressure with evidence. **Do not silently rewrite `ProjectCentral/user/**` or human governance.** Updating Agent Wiki knowledge and proposing a human-source revision are different operations.

The normal source-return relation is therefore:

```text
human-authored Project source space
    -> Agent-maintained SemanticWiki
    -> bounded traversal
    -> exact human/native source or evidence
    -> developmental return
    -> Agent Wiki update with provenance
    -> explicit proposal when human source is challenged
```

For the optional account/document integration established by the current AIKit project-authoring line, pass stable ProjectCentral source refs and their provenance standings directly into `product-understanding` and `structured-account-authoring`. `projection-authoring` and `html-account` may render derivative readings when explicitly requested; availability of those Skills does not auto-generate a document, make HTML canonical, or create a second Project ontology.

## Verification

Use AIKit knowledge-navigation/source-pool/wiki/familiarity/relation tests plus the real bkmr and GitNexus conformance lanes. Acceptance requires stable canonical refs; inspectable provider/lens/revision/source provenance; bounded relations; route/familiarity separation; exact addressed results protected from learned ranking; and no-QL-provider parity. ProjectCentral acceptance additionally requires no README dependency, arbitrary nesting, recursive `.no-agent-retrieval`, canonical Project/root Wiki paths, adopted-source participation without canonical replacement, lazy Project entry, explicit exact retrieval, provenance-bearing Agent Wiki maintenance, no silent human-source mutation and explicit revision proposals. This Skill does not encode TUI keybindings and does not grant provider, retrieval, trust or mutation authority.
