---
name: aikit-product-understanding
description: Develop product understanding only as far as the task requires, preserving the authority and provenance of human positions, product intent, design, architecture, implementation, evidence and current work.
---

# Product Understanding

Semantic ref: `aikit:product-understanding`. Native owner: `EpiLogos/ai-kit`.

Use this Skill when the task genuinely depends on what a product is, why it exists, what experience it intends, how its design and architecture express that intent, what is actually implemented now, or how returned reality should revise prior understanding.

Do **not** load a whole philosophical or product corpus merely because this Skill is available. Availability is not selection, and selection is not permission to disclose unrelated context.

The governing law is:

> **Provenance determines authority for the question being asked.**

Neither `vision wins` nor `code wins` is an adequate rule. Current code is authoritative for what is implemented now; it does not retroactively determine why the product exists. Authored vision and positions are authoritative for intended meaning at their scope; they do not prove current capability.

## Stop rule — choose depth before retrieval

Classify the task first and stop at the smallest sufficient depth:

```text
straight retrieval / lookup
    -> retrieve the requested source or answer from the already-resolved local source

local mechanical code task
    -> inspect the local code, tests and directly governing contracts; do not ingest product philosophy by default

behaviour / current-capability / current-state question
    -> descend to live implementation, current tests/evidence and relevant current issue/PR state

product meaning / experience / design / architecture / copy / major refactor
    -> perform the provenance-aware traversal below, only as far as required to make the consequential relation intelligible
```

If a shallower source already answers the question at the right authority, stop. If a claim about present capability would otherwise rest only on vision/design, descend. If a design change would otherwise be made without understanding the authored reason for the distinction, ascend toward authored ground first.

## Canonical traversal for meaning-bearing work

Traverse the current product world in this order when the question requires it:

```text
AUTHORED HUMAN POSITION / EXPERIENCE
        ↓
PRODUCT VISION / CONSTITUTIONAL INTENT
        ↓
PLANNING + DESIGN DECISIONS
        ↓
EXPERIENTIAL / CONCEPTUAL DIAGRAMS
        ↓
ARCHITECTURAL CONTRACTS
        ↓
LIVE IMPLEMENTATION
        ↓
CURRENT ISSUES / PRS / TESTS / EXPERIMENTAL EVIDENCE
        ↓
RETURNED UNDERSTANDING
```

For the O:I field, begin from the current authored positions in `docs/positions/FOUNDING-POSITIONS.md` or its canonical successor. For another product, locate that product's current authored position/vision source instead of assuming the O:I path exists. When authorised Central Control product ground is available, treat `Control/user/products/<product>/` as human-authored source according to Central's open-tree/content protocol, not as generated memory.

The traversal is directional, not a command to read every layer. Skip layers that do not exist or do not bear on the question, but disclose meaningful absences rather than inventing them.

## Claim classes — keep them distinct

Classify consequential claims using at least these source/authority classes:

```text
AUTHORED HUMAN POSITION
    relatively raw or stabilised human-authored intent, experience, desired encounter, stipulation or rejection

PRODUCT / CONSTITUTIONAL INTENT
    recognised product vision, constitution or product-level meaning

DESIGN DECISION
    a chosen experiential, interaction, visual or program-design determination

ARCHITECTURAL CONTRACT
    a normative technical boundary, invariant or interface the implementation is meant to satisfy

IMPLEMENTATION FACT
    what current code/configuration actually contains or does

EXPERIMENTAL FINDING
    an observed result from a test, experiment, runtime encounter or other bounded evidence

CURRENT DEVELOPMENT STATE
    current branch/issue/PR/test/integration state that may still be moving

INFERENCE / INTERPRETATION
    a synthesis not directly asserted by a source; state the evidence from which it is inferred
```

One claim can have support from several classes. Preserve all of them; do not collapse them into a single `truth` label.

## Retrieval and provenance through current AIKit seams

Use existing Context/Knowledge Navigation operations where they are available rather than creating a product-understanding database:

```text
sources -> search -> read -> relations / route -> frame -> explain / history
```

Prefer source-owned refs and exact revisions. Preserve provider, source, revision, lens/authority and degradation evidence already carried by Knowledge Navigation. A context frame is selected retrieval material for this act; it is not a new authored source. Route/familiarity history makes retrieval easier; it does not increase authority.

For live repository state, inspect the actual current branch/code and current issues/PRs before making time-sensitive implementation claims. Historical tickets and old diagrams remain evidence of development history, not automatic descriptions of current reality.

## Returned understanding

At the end of a meaning-bearing traversal, report the result as a compact provenance-aware synthesis:

```text
claim / distinction
    class(es)
    source ref(s) + revision/current-state marker where material
    what the source establishes
    what it does not establish
    unresolved tension or inference, if any
```

Where implementation or experimental evidence puts pressure on authored product ground, make the relation explicit:

```text
returned implementation / finding
        ↓
pressure on an authored position or vision
        ↓
reviewable proposal with provenance and reasons
        ↓
explicit human acceptance, revision or rejection
```

Do not silently rewrite Central Control or another human-authored source. A correct observation is still supporting context until the owner authors or accepts the durable change.

## Relation to vision/design authoring

Product understanding is the returning conjugate of product vision/design authoring:

```text
human experience / purpose
        ↓
authored ground + intent
        ↓
vision / journeys / desired encounters / visual and conceptual design
        ↓
development + encounter + evidence
        ↓
returned understanding
        ↓
explicitly renewed authored ground where warranted
```

Consume the real current authoring practices and artifacts present in the product. Do not fabricate a legacy HTML-prototyping Skill, mandatory diagram format, or document ontology merely because such an artifact could be useful.

## Completion checks

Before claiming understanding, ask:

- Did I use the smallest sufficient context for this task?
- Did I distinguish authored human position, product intent, design, architecture, implementation, experimental evidence, current development state and inference where they matter?
- Are current-capability claims grounded in current implementation/evidence rather than vision alone?
- Are product-meaning claims grounded in authored/constitutional sources rather than inferred backwards from code?
- Did I preserve exact source/revision/provenance where current AIKit operations provide it?
- If returned reality challenges authored ground, did I produce a proposal rather than silently mutate the authored source?

This Skill grants no authority to read private Control source, mutate product canon, merge a PR, or change a Profile merely because it is available.