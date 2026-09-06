---
name: wiki-inhabitation
description: Inhabit a project wiki — generate agent cognition around a project from its identified ground with the landed `aikit wiki` write surface, organised by the QL foundational meanings.
---

# Wiki inhabitation

Semantic ref: `aikit:wiki-inhabitation`. Native owner: `EpiLogos/ai-kit`.

**METHOD:** Inhabit a project wiki — generate the agent's living cognition around a project from its identified ground. The wiki is agent-generated cognition relative to the live human project docs; it is not document reflection. Both sides stay live.

## Ground first

Identify the project's ground before any generation:

- the ProjectCentral fractal — the Central `projectcentral-ontology` skill teaches the shape;
- read the human source directly; ordinary filesystem reads, nothing imported;
- declared relations in `source-relations.json` override the tree fallback;
- the NOW field is opt-in; its absence is not a fault.

## Generate with the landed write surface

Every generation is parse → mutate in memory → validate the whole → render; the caller persists atomically. The verbs:

- `aikit wiki validate` — validate a whole wiki file
- `aikit wiki node create` / `node update` — add or change nodes
- `aikit wiki edge add` — relate two nodes
- `aikit wiki space create` / `space link` — grow the space graph
- `aikit wiki root doctor` / `prune` / `adopt` — tend the root wiki
- `aikit wiki stage` — retain an explicitly authored Markdown `ql:` alignment as a source-linked node; use explicit stable source/node refs and `--update` only for an intended revision

## Organise by the QL foundational meanings

The `ql-foundations` skill (Quaternal-Logic) carries the minimal shape and the L0/L1 meanings. Use it when composing: an atomic node is non-QL (literally 1) or QL (an 0/1); nothing is gated; shapes are derived-at-read and attributed-at-generation; there is no canon other than the QL-derived one.

## Boundary

Wiki generation leaves authored source in place. Source changes follow the user's authorised documentation task and the source owner's reconciliation rules; generation alone confers no authorship or adoption. Wiki files persist through the caller atomically; Central's read-only mapping and additive reprojection stay Central's to run. Use the landed `aikit knowledge` faculty for search, read, relations, route, frame, sources, explain, history and status. Check the installed command surface and provider state before use; source projection support varies by format.


## Structured accounts and ordinary source

Treat source readability and semantic structure separately. The current ProjectCentral authored-relation adapter reads eligible `.md` sources and preserves explicit wikilinks and optional OKF Properties in the existing SemanticWiki field. Plain Markdown needs neither QL frontmatter nor a sixfold. A source with only unresolved links remains a valid focus; retain pending/ambiguous targets rather than manufacturing nodes. An explicit source link is a `references` relation unless a stronger relation was actually declared. Source-explicit edges retain the source owner's standing, including Agent-derived standing.

For `wiki stage`, the current CLI requires Markdown `ql:` frontmatter. `position` must be 0–5 relative to a nonempty `unit`; optional `face` is direct or conjugate. Preserve other declared labels. Staging retains alignment and source linkage, leaving source prose in place. It does not infer alignment for an ordinary file. Use `wiki node create` for an authorised Agent determination grounded in such a file.

A seed-led HTML account has a 0/1 whole, six expanded layers, typed units and a resource map; its companion capability matrix declares its whole, ordered axes and stable capability/relation records under Central’s `docs/CAPABILITY-MATRIX-PROTOCOL.md` (`ql-capability-matrix/1`; templates in `skills/capability-matrices/assets` (`matrix.csv` and `matrix.schema.json`)). The current Markdown adapter/staging command does not extract that HTML structure or turn CSV rows into semantic subjects. Read those sources through their owner/provider, and create only the explicitly authorised, source-linked Agent knowledge needed for the current question. Preserve unit/row locators and exact source revisions. Do not create Markdown copies solely to obtain staging support.

The intended format parity is one source/revision/standing contract with richer metadata where present. An authored QL coordinate supplies structure, not authority. A plain note can remain a non-QL node or an ordinary source. A partial whole stays partial. Source adapters and membership/relations should feed the existing index; a separate document-format Wiki or duplicated backing store is unnecessary.

## Population and return

For a documentation population task, first inventory the selected source refs, exact revisions, unit/row locators, provenance, retrieval policy and existing Wiki identities. Read the purpose seed and affected capabilities alongside ordinary notes and eligible NOW/Run evidence. The source files remain the authored account; populate only the bounded knowledge required to disclose their relations and answer the task.

An ordinary item is `1`. A disclosed whole is `0/1`, with its actual members, grain and Return. Preserve declared QL positions and shapes; arbitrary matrix axes retain their own member identities. A product seed × field-contribution 6×6 does not acquire the native direct/conjugate shape simply because both have 36 addresses. A cell can carry no determination, one determination or several; capability identity survives a change of view.

For a selected capability, collect its seed/expanded account units, matrix records and exact supporting or conflicting notes/Run passages. Explicit contemplation may produce an attributable Wiki reading and a bounded source-change proposal. Review proposed HTML, CSV and Markdown changes together at their basis revisions. Reconcile according to existing user authorisation and source standing, then validate links, shape, trace and companion parity before publishing the corresponding Wiki revision. A stale basis requires renewed reconciliation. Do not treat a stage operation as a multi-file transaction.

Central owns the durable filesystem arrangement, source identity and lifecycle metadata for document retirement. AIKit owns assessment, consolidation proposals, reconciliation and the actual retirement/archive/restore operations using that contract. Preserve source-to-successor mappings and unresolved content; retirement changes lifecycle, not standing. Current native Wiki commands do not implement a general retirement transaction: scope or prepare the proposal until the operation exists, and report that boundary explicitly. Ordinary task-authorised source editing remains available.

The maintained population and real-file acceptance procedure is in AIKit's `docs/v2/21-PROJECT-REFLECTION-AND-LOCAL-ARTICULATION.md`, sections 15–18. Read it for suite population or retirement work. This source Skill does not imply that an installed generation or live harness has reloaded it.
