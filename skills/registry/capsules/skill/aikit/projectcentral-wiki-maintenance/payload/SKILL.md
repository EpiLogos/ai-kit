---
name: aikit-projectcentral-wiki-maintenance
description: Maintain a Central ProjectCentral Agent Wiki from bounded project/source evidence without mutating human-authored Project ground.
---

# ProjectCentral Agent Wiki Maintenance

Semantic ref: `aikit:projectcentral-wiki-maintenance`. Native operational owner: `EpiLogos/ai-kit`. Filesystem/source identity owner: `EpiLogos/Central`.

This procedure extends the existing `aikit:knowledge-navigation` faculty. It does not create another Wiki, graph, source tree, document aperture or prompt-time context dump.

## Standing relation

Keep these source standings explicit throughout the act:

```text
ProjectCentral/user/**                     human-authored Project source
ProjectCentral/agents/governance/**        human-authored Agent governance
ProjectCentral/agents/wiki/wiki.json       Agent-maintained semantic knowledge
ordinary native Project source             authored/observed according to provenance
observed evidence                          not authored merely because it was observed
inferred / derived reading                 not observed merely because it was inferred
```

A WikiNode or WikiEdge inside the Agent-maintained Wiki may cite human-authored source. That citation does not transfer human authorship to the Agent-maintained Wiki object.

## Procedure

1. Enter through the ProjectCentral binding supplied by AIKit. Establish Project identity, human-source availability, governance availability, canonical Agent Wiki identity, adopted Wiki sources, native Project source and optional Control root Wiki without reading all of them.
2. Use Knowledge Navigation progressively: `search`, `read`, `relations`, `route`, `frame`, `sources`, `explain`, `history`. Treat `exists`, `known-to-exist`, `askable`, `retrieved` and `focused` as distinct states.
3. Retrieve exact human/native source only when the current Claim, relation or developmental question requires it. Respect `.no-agent-retrieval` recursively; a hidden subtree is not eligible for Agent retrieval even though it exists on the human filesystem.
4. Inspect the current canonical Wiki revision and the exact source revisions cited by affected Wiki objects. Distinguish stale knowledge, conflicting knowledge and merely additional knowledge.
5. Form the smallest coherent set of WikiNode/WikiEdge/reading changes. Every maintained knowledge object must preserve exact source/provenance and the producer/run or generation reference when available. Do not flatten adopted Wikis into the canonical Wiki merely because they participate in traversal.
6. Validate the proposed whole through the existing SemanticWiki index before persistence. Persist only to `ProjectCentral/agents/wiki/wiki.json` (or `Control/agents/wiki/wiki.json` when explicitly maintaining the root Wiki). Derived indexes remain rebuildable state.
7. When project development returns new evidence, update Agent-maintained knowledge where warranted. If the returned reality challenges a human-authored Project position, create an explicit human-source revision proposal / Decision pressure with evidence. Do not rewrite `ProjectCentral/user/**` or human governance.
8. When a human wants a high-altitude project account, pass the stable ProjectCentral source refs and standings into the existing `product-understanding` and `structured-account-authoring` Skills. `projection-authoring` and `html-account` may render a derivative account when explicitly requested; they do not become canonical Project ground and are not run automatically during Wiki maintenance.

## Source return

The normal developmental return is:

```text
human-authored Project source space
    ↓
Agent-maintained SemanticWiki
    ↓
bounded traversal
    ↓
exact human/native source or evidence
    ↓
developmental work / returned evidence
    ↓
Agent Wiki maintenance with provenance
    ↓
proposal / Decision pressure when human source is challenged
```

Updating the Agent Wiki and proposing a human-source revision are separate operations.

## Verification

Acceptance requires the ProjectCentral conformance tests to prove: no README requirement; arbitrary nesting; recursive `.no-agent-retrieval`; canonical Project and Control root Wiki paths; adopted Wiki participation without canonical replacement; no eager human-tree or Wiki loading; bounded exact source retrieval; provenance-bearing Wiki maintenance; no silent human-source mutation; explicit revision proposals; and preservation of existing Knowledge Navigation list/tree/graph/search/read/relations/route/frame behaviour.
