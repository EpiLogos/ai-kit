# Engineering-ground seed for the actor bootstrap

**Date:** 2026-09-04
**Status:** design, approved for implementation planning
**Branch:** ai-kit `main` (the `feat/actor-bootstrap-session-space` line merged);
ctrl work lands on a branch off Central `main`

## Purpose

The central/actuation/aikit bootstrap should be aware of the person's actual
engineering norms of the moment — git conventions, agent operations, coding
approach, liked global skills — so that agency creation, capability discovery,
capability-set construction, and method construction happen naturally, wrapped
in a grounded, editable meta-layer that the O:I system itself maintains.

The derived seed feeds ai-kit's *natural* resolution. No new ai-kit seed types.
The system whole already owns this class of object.

## Decisions (ratified in design dialogue)

1. **Authored ground → derived seed.** Preferences live as human-authored
   statements; ai-kit receives a derived projection. Agent-inferred proposals
   become durable only through an explicit acceptance act (Recognition /
   adoption), per the O:I authored-ground boundary.
2. **One path, two modes.** The base template is the QL six-slot statement form
   (`Control/agents/expressions/central-intent/templates/statement-form.md`).
   Hand-authoring is always available; the agent-led option is a conversation
   that fills the same templates — no second ontology, no interview schema.
   Because ai-kit has already loaded capabilities and skills across harnesses
   at setup time, it offers a curated selection for the base skillset ("the
   essentials") rather than asking the person to name skills cold.
3. **Home: `Control/agents/governance/engineering/`.** Agent-facing
   how-I-work ground, kept by the central-intent expression.
4. **Seam: existing source machinery.** Statements and their distillate are
   world sources riding the Source Change Horizon, ground relations, world
   source actions, ProjectCentral provenance taxonomy, WorldKnowledgeBinding,
   and ContextResolution. No parallel store.

## The anchored design

### 1. Engineering statements (authored layer)

New folder `Control/agents/governance/engineering/`. One statement per file:
first-person, spoken to the agent, six movements unlabeled (why the relation
exists / what it is / how it is done / who-whereby / where-when / why-for),
natural prose, minimal but catching all angles.

Baseline set (small — the hexad catches the angles, not file count):

- `agent-operations.md` — how agents operate on my behalf: git norms
  (branching, commits, PR flow, recovery), session behaviour
- `coding-approach.md` — how code is approached, shaped, and returned
- `verification.md` — what "done" means before return
- `base-skillset.md` — liked global skills: prose plus refs

Guardianship is existing law: `propose-not-write.md` governs agent edits,
central-intent keeps the form and prunes, Recognition closes any agent-proposed
transition into durable ground. `Unslop` may cut formulaic material without
collapsing the hexad.

### 2. Statements are Control-tree sources (nothing to build)

The Control tree already participates in the Source Change Horizon via
`control_source_bindings` (ctrl `source_horizon.rs`). With a ground relation,
each statement carries provenance `human-authored`, role
`agent-governance-source`, a stable `SourceRef`, and a deterministic content
revision. Machine-checked from ground relations, not asserted.

### 3. Distillate: a derived source

ctrl renders the foundational prompt document (CLAUDE.md / agents-shaped)
from the statements as a **derived source**:

- provenance `generated-derived`
- provenance refs naming the statement `SourceRef`s + exact revisions
- re-derivable: re-running the render after a statement change produces the
  new revision; CAS write with actor attribution
- the write gate protects human-authored ground, not derived material, so
  the derivation itself may be machine-run; human **adoption** is what
  promotes it

Edits happen at the statement layer (or via proposal → Recognition), never by
silently editing the distillate.

### 4. User resolution — two existing doors

Resolution is an explicit user act, never ambient:

- **Project world:** adopt via the template-stamp / source-write path
  (additive draft, never overwrite). On adoption the distillate becomes
  `human-adopted` project ground with a ground relation, enters the project
  source horizon, and resolves as a ContextSource in ContextResolution.
- **Profile:** AgentProfile `knowledge_source_refs` / `governance_refs` name
  the source; ai-kit binds it via `WorldKnowledgeBinding` — standing
  `PersonalGeneral` or `Inherited` with exact source World + revision.

Once resolved, ai-kit's ordinary machinery carries it: capability sets
construct from resolved refs; methods construct through `method_refs` via
praxis. The seed is operative because authored ground arrived through ai-kit's
own resolution.

### 5. Setup conversation (agent-led mode)

A conversation during bootstrap fills the same statement templates. ai-kit's
already-loaded capability/skill catalogue informs the base-skillset selection.
Agent output enters as `HumanSourceRevisionProposal` (target, reason,
supporting context, final diff). The person accepts, revises, or refuses;
adoption closes. No interview schema — the hexad is the shape, prose is the
form.

### 6. Bootstrap disclosure

The managed `aikit-context` skill gains one provenance line naming the
resolved engineering-ground source when present — smallest-sufficient: name
it, don't copy it. Eligibility ≠ retrieval already holds via
WorldKnowledgeDisclosure semantics. The `context_sources` summary in
`ActorBootstrap` carries it without schema change.

## Net new build

1. Statement content + template under
   `Control/agents/governance/engineering/` (with ground relations).
2. One ctrl render action: statements → foundational prompt derived source,
   with derived provenance and statement provenance refs.
3. Adoption wiring: template-stamp / source-write delivery of the distillate
   into a project world, and AgentProfile reference support where a gap
   exists.
4. One provenance line in the managed bootstrap skill render.

Explicitly **not** built: new ai-kit types, a preference/persona store, a
second profile database, ambient disclosure, agent writes to authored ground.

## Testing

- ctrl: render action is deterministic; provenance refs pin exact statement
  revisions; re-render after statement change bumps revision; write gate still
  refuses non-human writes to the *statements*.
- aikit: adopted source resolves as a ContextSource; WorldKnowledgeBinding
  validates standing + revision; bootstrap carries it in `context_sources`
  and the managed skill names it; no payload is copied into standing context.
- Cross: full loop — author statement, derive, adopt into a fixture project,
  resolve context, verify the bootstrap names the source and capability/skill
  refs are operative.

## Key existing references

- `ctrl/src/source_horizon.rs` — Source Change Horizon, `control_source_bindings`
- `ctrl/src/world_source.rs` — open/revise world source, write authority gate
- `ctrl/src/template_stamp.rs` — additive default stamping
- `ctrl/src/agent_profile.rs` — `central.agent-profile/v1` refs/intents
- `Control/agents/expressions/central-intent/templates/statement-form.md` — hexad
- `ai-kit/crates/aikit-core/src/projectcentral.rs` — provenance taxonomy, proposals
- `ai-kit/crates/aikit-core/src/world_inhabitation.rs` — WorldKnowledgeBinding
- `ai-kit/crates/aikit-core/src/actor_bootstrap.rs` — bootstrap projection
- `ai-kit/crates/aikit-adapters/src/clients/bootstrap.rs` — managed skill render
- `ai-kit/docs/v2/03-ACTOR-RUNTIME-AND-PROJECTION.md` §23 — agentic seed boundary
