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

## Organise by the QL foundational meanings

The `ql-foundations` skill (Quaternal-Logic) carries the minimal shape and the L0/L1 meanings. Use it when composing: an atomic node is non-QL (literally 1) or QL (an 0/1); nothing is gated; shapes are derived-at-read and attributed-at-generation; there is no canon other than the QL-derived one.

## Boundary

Never write into human source. Never impose a human document. Durable authored material is human-accepted through Central's control-maintenance. Wiki files persist through the caller atomically; Central's read-only mapping and additive reprojection stay Central's to run. This Method is provisional: the frame/reading commands, ingestion walk and shape/constellation UI are still open work — point at them when they land rather than describing them here.

