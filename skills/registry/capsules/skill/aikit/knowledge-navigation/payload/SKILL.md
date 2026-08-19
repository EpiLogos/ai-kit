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

## Project reflection: meaning ↔ description ↔ code

When the Project declares explicit cross-representation bindings, use the existing ProjectMap federation as the traversal seam. Do not build a second Wiki or flatten provider-native graphs into ProjectMap.

A selected semantic concept should be able to disclose a bounded answer to:

```text
WHAT IS THIS?
    SemanticWiki / Project vocabulary

WHY DOES IT EXIST?
    authored Ground / design source

WHERE / HOW IS IT REALISED?
    local structural description + exact CodeReference

WHAT IS ITS STRUCTURE NOW?
    CodeIndex / GitNexus context, impact and trace

WHAT PROVES IT?
    tests / verification / evidence

WHAT CHANGED?
    Run / Decision / history / returned difference
```

The reverse route begins from an exact `CodeReference` and recovers any explicit known concept, local description/ownership source, authored/design ground, verification/evidence and history. Preserve the same stable refs and relation provenance for CLI, TUI, desktop and Agent consumers; presentation may reduce them to pithy relations such as `this is`, `part of`, `implements`, `described by`, `verified by`.

Authority remains differentiated along the route:

```text
SemanticWiki                         maintained semantic knowledge
human Ground / Canon                 authored meaning at its scope
native local description/contract    source about a local implementation region
CodeReference                        exact code identity/address
CodeIndex / GitNexus                 derived structural observation
verification / observed evidence     bounded evidence
Run / Decision / history             developmental record
```

GitNexus or another CodeIndex provider never becomes semantic authority merely because it can reveal a richer code graph. A local structural description never becomes implementation truth merely because it names the implementation. ProjectMap never becomes a universal graph store merely because it can bridge those representations.

If a binding declares an exact reflection law, verify the relation rather than comparing labels. Missing/wrong/duplicate/stale mappings and lost constitutive relations are explicit discrepancy evidence. A name that still matches after its structural relation has been flattened is not parity.

## Native local source discovery

Project orientation may discover native material such as headers, `AGENTS.md`, `CLAUDE.md`, `CONTEXT.md`, module/package READMEs, ADRs, interface notes and manifests, but **filename and path are only candidate signals**. Resolve actual role from owner-issued/adopted relation, provenance and content where available.

At minimum keep these roles distinguishable:

```text
human-authored Project Ground
Agent governance
Agent-maintained Wiki
local structural description
ordinary source/docs
derived/generated documentation or code-index observation
temporal/current working material
praxis source
unresolved
```

A compatible native source may remain in place. Do not copy it into ProjectCentral simply to make it addressable. Generated harness projections and derived indexes remain rebuildable/derived even when their filenames resemble authored files.

For code-changing work, discover the smallest applicable broad→local source chain before entering the implementation region. The closest applicable contract supplies local specificity, but it does not retroactively become Project-wide meaning. When work makes a description stale, surface that as an attributable discrepancy and route pressure to the description owner rather than rewriting it silently.

## ProjectCentral local Wiki worlds

When a Project exposes Central's `central.project/v1` binding, consume it as a native Knowledge Navigation source relation rather than inventing an aperture document or another graph.

Keep ownership, provenance and truth standing separate:

```text
ProjectCentral/user/**                     human-owned authorship aperture
                                            (location alone does NOT prove authorship)
recognised human-authored/adopted source    Central-issued source ref + provenance/standing
ProjectCentral/agents/governance/**         human-authored Agent governance
ProjectCentral/agents/wiki/wiki.json        Agent-maintained semantic knowledge
ordinary native Project source             authored/observed/inferred only as provenance establishes
observed evidence                          != authored
inferred / derived reading                 != observed
```

When Central's accepted relation source is present, consume `ProjectCentral/relations/source-relations.json` with schema `central.project.ground-relations/v1` rather than inferring authority from filenames or paths. Preserve its exact distinctions:

- provenance: `human-authored`, `human-edited-draft`, `human-adopted`, `generated-suggestion`, `generated-derived`, `agent-maintained`, `observed`, `inference`, `unresolved`;
- truth standing: `authored-human-position`, `design-commitment`, `architecture-contract`, `implementation-fact`, `observed-evidence`, `current-development-state`, `agent-inference`, `unspecified`;
- treatment and roles as relation metadata rather than a directory taxonomy.

An unclassified file under `ProjectCentral/user/**` remains readable/askable when policy permits, but its provenance and truth standing stay unresolved until Central records direct authorship/adoption. This prevents generated material from acquiring human authority merely because it landed in the human-owned aperture. A recognised native human source may remain exactly where it is; consume its Central-issued source ref and `retain-native-in-place` relation rather than moving/copying it into ProjectCentral.

Project entry should establish Project identity, human-source availability, governance availability, canonical Agent Wiki identity, adopted Wiki sources, accepted source relations, native Project source, local structural-description candidates, praxis source apertures and optional Control root Wiki **without retrieving the whole tree or Wiki**. Preserve the ContextSource progression `exists -> known-to-exist -> askable -> retrieved -> focused`; a broad information horizon is not prompt inclusion.

Respect `.no-agent-retrieval` recursively before source disclosure. A human may freely organise `ProjectCentral/user/**`; arbitrary or non-text material may be known to exist even when the current provider cannot interpret its contents.

For Agent Wiki maintenance:

1. Inspect the canonical Wiki revision and the exact source revisions cited by affected Wiki objects.
2. Distinguish stale, conflicting and merely additional knowledge. Semantic conflict is an Agent judgement over Claims/evidence; deterministic code should validate revision, provenance and topology rather than pretending semantic disagreement is mechanically decidable.
3. Form the smallest coherent WikiNode/WikiEdge/reading changes and preserve exact source/provenance plus producer/run or generation refs when available.
4. Validate the proposed whole through the existing SemanticWiki index before persistence. Adopted Wikis remain participating sources and do not replace the canonical Wiki.
5. Persist Project knowledge only to `ProjectCentral/agents/wiki/wiki.json` (and maintain `Control/agents/wiki/wiki.json` only when the root Wiki is explicitly the target). Derived indexes remain rebuildable state.
6. If returned implementation/runtime evidence challenges recognised human-authored Project source, represent the pressure as a human-source revision proposal / Decision pressure with evidence. **Do not silently rewrite `ProjectCentral/user/**`, retained native human source, or human governance.** Updating Agent Wiki knowledge and proposing a human-source revision are different operations.
7. If returned implementation evidence instead makes a native local description stale, preserve exact semantic/source/code refs and route a discrepancy/update proposal to that source's actual owner. Do not promote the Agent Wiki into the description owner merely because the Wiki detected the mismatch.

The normal source-return relation is therefore:

```text
recognised human-authored Project source
    -> Agent-maintained SemanticWiki
    -> bounded traversal
    -> exact human/native source or evidence
    -> developmental return
    -> Agent Wiki update with provenance
    -> explicit proposal when human source is challenged
```

For optional account/document integration, hand `product-understanding` and `structured-account-authoring` the structured ProjectCentral account context: exact Central source refs, paths, provenance, truth standing, roles/treatment, canonical Agent Wiki identity and relation-source identity. Do not make those Skills rediscover path conventions in prompts. `projection-authoring` and `html-account` may render derivative readings when explicitly requested; capability availability does not auto-generate a document, make HTML canonical, or create a second Project ontology.

## Verification

Use AIKit knowledge-navigation/source-pool/wiki/familiarity/relation tests plus the real bkmr and GitNexus conformance lanes. Acceptance requires stable canonical refs; inspectable provider/lens/revision/source provenance; bounded relations; route/familiarity separation; exact addressed results protected from learned ranking; and no-QL-provider parity. Project reflection additionally requires bidirectional semantic↔code traversal through explicit bindings, local-description/source authority remaining distinct from implementation truth, and stale/flattened reflection becoming explicit discrepancy evidence rather than silent retargeting. ProjectCentral acceptance additionally requires no README dependency, arbitrary nesting, recursive `.no-agent-retrieval`, unresolved aperture material not acquiring human authority, exact accepted source/provenance/truth-standing consumption, retained native human source participation without movement, canonical Project/root Wiki paths, adopted-source participation without canonical replacement, lazy Project entry, explicit exact retrieval, provenance-bearing Agent Wiki maintenance, no silent human-source mutation and explicit revision proposals. This Skill does not encode TUI keybindings and does not grant provider, retrieval, trust or mutation authority.
