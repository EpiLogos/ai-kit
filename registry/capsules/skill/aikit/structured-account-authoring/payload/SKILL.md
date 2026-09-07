---
name: structured-account-authoring
description: Compose a deep, navigable, provenance-aware reading of Central, a Project, Wiki space, research corpus, or another coherent authored world without replacing its native source or forcing one ontology.
argument-hint: Name the authored world, audience, purpose, source horizon, desired depth, and whether the reading is private, local, or intended for Projection.
---

# Structured Account Authoring

Create a coherent reading of an authored world from the material that already owns its meaning.

An **account** is an authored or generated reading. Generation alone gives it no new authority. The human may choose an editable account as product source, with provenance and recognition recorded at the appropriate scope; derived readings retain their source links.

Use this Skill for Central, Projects, Wiki spaces, research corpora, or other coherent authored worlds when the human asks for deep understanding, documentation, design account, review, or Projection preparation.

## 1. Ownership first

Before composing the reading, identify:

- the native source authority;
- the source revision or horizon;
- the audience and visibility;
- the purpose of this reading;
- which objects already have native identities;
- which material is authored position, design, implementation fact, evidence, current state, or inference.

Do not invent a new profile, Project, Claim, Evidence, Wiki, Agent, Run, or source ontology when one already exists.

For Central:

```text
Control/user
Control/agents
Control/machines
Work
```

remain ordinary authored source. The Skill can read selected material from them, but must not imply that an entire root is public or that Central has a fixed content schema.

For a Project, preserve the Project's own authority and ordinary filesystem/repository form. Rich documentation develops with the Project; it is not an adoption gate.

## 2. Compose through provenance

When product or system meaning matters, compose with `skill/aikit/product-understanding` if available.

Keep the six documentation standings distinguishable:

- AUTHORED HUMAN POSITION
- DESIGN COMMITMENT
- ARCHITECTURE CONTRACT
- IMPLEMENTATION FACT
- OBSERVED EVIDENCE
- AGENT INFERENCE

Product/constitutional intent describes content and scope; current development state describes time and context. Neither adds a standing. Use Central’s source relation and canonical standing protocol when mapping claims.

Provenance determines authority for the question being asked.

Vision can explain intended meaning. It does not prove current behaviour. Current code can establish what exists now. It does not retroactively author why the Project exists.

## 3. Develop the smallest whole reading

Do not assume one enormous document is required.

A useful reading may combine:

- prose;
- positions and distinctions;
- sources;
- diagrams;
- Claim/Evidence readings;
- timelines;
- comparisons;
- code or schema;
- images or mockups;
- Wiki excerpts;
- Project or Agent references;
- Run/history readings;
- Actions or review controls.

Choose only the modules that make the subject easier to understand, inspect, verify, or act upon.

Several accounts may legitimately exist over one source. A Project can have public, developer, research, design, and release readings. Central can have public-world, private personal, machine/environment, and agent-collaboration readings.

## 4. Natural structure

Use the subject's own vocabulary for visible sections.

QL may be used internally as a completeness check when that helps. It must not be imposed as visible terminology or a six-section template.

A personal reading might naturally use headings such as:

```text
Where I am
What matters to me
How I work
What I am making
Where this is going
```

A software Project might use:

```text
Why this exists
The product
Experience
Design
System
Current frontier
```

A scientific, artistic, historical, or ordinary filesystem Project can use a completely different form.

Do not manufacture visible symmetry merely to satisfy a hidden grammar.

### Human-selected seed-led accounts

For the suite documentation convention, compose the 0/1 six-question seed first, then its six expanded layers. Use the human's seed where supplied; otherwise return a source-grounded candidate with Agent attribution. Keep the concrete What and experiential Why visible before technical detail. Follow the native `html-account` seed-led mode for the renderer and Central's `docs-methodology` for capability links, seed revision reconciliation and source standing. Preserve ordinary files as valid source around this selected account. Route capability-matrix authoring to Central’s `capability-matrices` Skill and `docs/CAPABILITY-MATRIX-PROTOCOL.md` (`ql-capability-matrix/1`), with templates in `skills/capability-matrices/assets` (`matrix.csv` and `matrix.schema.json`). Bind account units to stable capability IDs; the manifest owns ordered axes and views, while uniform CSV records retain capabilities and their placements. Other views change the declared organisation, not the artifact protocol.

## 5. Source ledger

For every consequential module, retain enough provenance to answer:

- which source produced this reading;
- which revision or state was read;
- what standing the statement has;
- which native object is being presented;
- whether the module is source-supported or interpretive;
- what would change its standing.

Prefer native semantic refs when available through ProjectMap, SemanticWiki, Claims/Evidence, Explain/History, or another existing provider.

Do not copy source text into a new canonical document merely to make it easier to render.

## 6. Projection preparation

If the reading is intended for O:I Projection, compose it so it can become a `WorldPresentation` representation without changing source ownership.

```text
native source
    ↓ selective authored reading
WorldPresentation
    ↓ explicit ratification
Projection revision
```

The account itself is not the Projection. A draft is not publication.

Use `skill/aikit/projection-authoring` for the selection, audience, revision, withdrawal/supersession, and return-path work.

## 7. HTML rendering

If a portable standalone artifact is requested, compose this reading with `skill/aikit/html-account`.

HTML is one renderer of the structured reading. It must not replace Markdown, Control files, WikiNodes, code, diagrams, Claims, Evidence, or Project canon.

An Agent should consume structured modules and provenance directly where available rather than scraping generated HTML.

## 8. Central return path

A public or local presentation refinement can reveal a better formulation of the underlying authored world. That insight does not silently mutate Central.

Use:

```text
Projection or account difference
        ↓ explicit proposal
Central-owned review
        ↓ accepted mutation
Control source revision
```

A proposal should contain the target, proposed content, reason, supporting context, and final diff required by Central's content protocol.

## 9. Stop rule

Do not invoke deep account craft merely because the Skill is available.

Use the smallest sufficient depth:

- direct retrieval → read the requested source;
- local mechanical fix → code/tests/local contracts;
- current capability → implementation plus evidence/current state;
- whole-product meaning, documentation, design, major change, or Projection → deepen through provenance as required.

Stop when the reading is sufficient for the human's actual purpose.

## 10. Completion check

Before presenting or handing the reading to a renderer, verify:

- native source ownership is still clear;
- no visibility inference exposes unselected material;
- claims keep their provenance standing;
- visible headings are natural to the subject;
- no QL dependency is imposed on an ordinary Project;
- the reading can be understood by an Agent without HTML scraping;
- Project/Central identities have not been duplicated;
- publication, if any, will create or refine an explicit Projection revision.
