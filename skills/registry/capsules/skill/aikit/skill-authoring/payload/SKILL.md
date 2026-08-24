---
name: aikit-skill-authoring
description: Author executable procedural knowledge as managed Skill or Method praxis with explicit source, triggers, operations, authority assumptions and verification.
---

# Skill and Method authoring

Semantic ref: `aikit:skill-authoring`. Native owner: `EpiLogos/ai-kit`.

A **Skill** is reusable organised intelligent praxis. A **Method** is a Focus-bearing situated composition of independently owned Skills, UsageOverlay receipts, Actions/Capabilities, ContextSources, Project/domain refs and verification/expected-return forms. A **SkillSet** is only additive repertoire. Profile/ContextResolution determine why and where resources become operative.

Do not make a Skill absorb a Project merely because the Project needs it, and do not make a SkillSet become a workflow engine merely because several Skills are commonly used together.

## Skill procedure

1. State the procedure's owner and the user/Agent situation that should trigger it. A Skill should route work through the owner's public application/domain contracts rather than UI coordinates or private runtime mutation.
2. Recover the target Project vocabulary and applicable source/contract chain before naming durable procedural distinctions. When stable SemanticWiki/ProjectMap/source/code refs already exist, point to them instead of restating architecture in prompt prose.
3. Create the ordinary Agent Skill body (`SKILL.md`) and package it with the existing AIKit capsule manifest (`kind = "skill"`, `[skill] root = "payload"`). Keep capsule id/path aligned.
4. Include purpose/triggers, inputs, semantic operations, required Capabilities/Actions where relevant, risk/permission boundaries, outputs and verification/conformance paths.
5. Make source/projection status explicit: repository/managed source + exact revision is authoritative; harness projections are derived.
6. Prefer stable semantic steps such as `inspect -> stage -> preview -> apply -> verify` over key presses or panel positions.
7. Add examples/fixtures where they prove behaviour. For an authoring Skill, prove that it can guide creation of a small valid Skill that passes the same structural rules.
8. Submit source/revision to the managed source review/trust lifecycle. Repeated success or fitness evidence may inform review but never promotes source automatically.

## Instruction architecture

Use this deeper craft when the task is creating, reviewing or simplifying instructions themselves rather than merely using an existing Skill.

### Source ownership first

Recover the actual source owner before moving language between governance, Guidance, Skill, Method, Project docs or a conditional reference. Human-authored Central governance such as `central.agent-governance-relations/v1` remains Central source; AIKit composes it operationally and does not reinterpret source scope as an instruction-precedence algorithm.

A Skill description is a **routing affordance**. It should name the concrete situation that selects the Skill and a nearby situation that should not. Put reusable procedure in the body rather than accumulating keywords and procedure in the routing description.

### Governance, procedure and disclosure

Classify durable language by what it does:

```text
stable fact / vocabulary / care / collaboration boundary
    -> compact governance / Guidance where its source owner warrants persistence

situational reusable procedure
    -> Skill

Focus-bearing relation among independently owned resources
    -> Method

rare provider/template/deep example
    -> conditional reference / context pointer

current Project fact
    -> Project source / Wiki / evidence owner

one-off workaround
    -> normally no durable instruction unless returned evidence warrants it
```

Choose disclosure mode by an inspectable local trade-off between **model/context load** and **human invocation load**. There is no universal token threshold. The common Skill path should state the branch condition and use **progressive disclosure** so rare references do not enter ordinary context merely because they exist.

For consequential boundaries, pair a prohibition with a **positive operational specification**: state the transition the Agent should perform instead of only naming what it must avoid.

### Human authority is inherited, not reimplemented

Instruction craft consumes `aikit:product-understanding` human-authority discipline. Resolve current request, authored source, evidence and reversible engineering before escalating. If materially different product futures remain, ask at the experienced/product/architectural consequence. Historical repetition or successful Agent output never self-promotes into human-authored governance.

### Phase separation

Use phase separation only when a later artifact predictably causes premature completion, hides an unresolved evidence boundary or crosses a real authority boundary. Name the phase closure condition and what later artifact remains unavailable. Direct work does not need ceremony merely because phases are possible.

### Vertical slices and tracer bullets

Recommend a vertical slice only when you can name the risk, the layers crossed and the evidence it returns earlier than a horizontal build. The phrase is a technique, not mandatory engineering dogma.

### Regression before sediment

Attach positive/nearby-negative trigger fixtures and behavioural regression where the language is load-bearing. Run a no-op/deletion test against accumulated instructions: if removing a rule changes neither the target behaviour nor a recurring evidence-backed failure, consolidate or delete it. The goal is the smallest sufficient architecture, not minimum character count.

### Historical behaviour is evidence, not authorship

Sessions, Runs, PRs and repeated failures may justify a governance or Skill proposal after the cause is classified. They do not confer authority to rewrite human source automatically. Route the returned pressure to the real owner.

### Communication is part of capability

Where successful operation requires the Agent to expose evidence, uncertainty, returned difference or a human authorial fork, that communication is part of the Skill's capability contract rather than ornamental verbosity.

The practical premise here is language-conditioned behaviour: instruction language changes distinctions, salience and action. This is not evidence of phenomenal consciousness.

For substantial governance audits, repeated failures, trigger ambiguity, progressive-disclosure redesign, phase separation or historical instruction review, open `references/instruction-architecture-review.md`. Do not load that deep review on the ordinary authoring path.

## UsageOverlay before source mutation

When an unchanged Skill is broadly correct but a user/Project/Focus needs a small situated orientation, use the existing scoped Skill Usage Overlay mechanism rather than forking or rewriting the Skill source.

Keep the distinction explicit:

```text
Skill source                         reusable owned praxis
UsageOverlay                         scoped adaptation of that unchanged Skill
reviewed-against / digest            exact adaptation evidence
Effective Skill projection           derived harness-facing material
```

A repeated useful overlay may create evidence for a Project Method or later reusable Skill refinement, but repeated use is not automatic promotion and the overlay does not become Skill source.

## When to author a Method

Author a Method when the useful durable thing is **the contextual relation among independently owned resources around a purpose/Focus**, rather than a new reusable Skill body.

A Method should be able to retain stable refs to:

```text
Focus / Project / domain
SkillRef(s)
UsageOverlay ref/digest(s)
ActionRef(s) / CapabilityRef(s)
ContextSourceRef(s)
verification refs
expected return forms
```

Do not copy the referenced bodies into the Method. Do not encode trust, activation authority or Profile precedence in Method membership. Do not convert the Method into a sequence DSL merely to make its composition look procedural; order belongs only where the actual practice requires and owns order.

Before writing a Project Method:

1. Recover the Project's actual language/ontology and the stable refs already expressing it.
2. Check whether an existing reusable Skill plus UsageOverlay is sufficient. If so, stop there.
3. Check whether the need is merely additive availability. If so, use SkillSet rather than Method.
4. Compose only the refs materially germane to the Focus and state the expected verification/return relation.
5. Resolve the selected Method **under** the existing Profile/ContextResolution. Method selection never activates an unavailable capability or bypasses trust/policy/Action authority.
6. Preserve source/revision and immutable overlay digests so Explain/History can reconstruct the praxis condition later.
7. After real use, return fitness evidence as evidence about the Method/overlay/Skill condition; do not silently mutate durable praxis.

## Project vocabulary and structural fidelity

If a Project owns a constitutive ontology, coordinate map, protocol/state machine, schema graph or equivalent structural source, praxis must stay answerable to it. A generic convenient workflow must not flatten target-owned distinctions.

Use the target's stable semantic/source/code refs where possible. If no such structure exists, do not invent one merely to satisfy the authoring procedure. Ordinary Projects remain valid without QL/MEF, Bimba coordinates, Method trees or special local file conventions.

## Specimen

`skills/fixtures/minimal-authored-skill/` is the representative small Skill produced by this procedure. `scripts/verify-native-skills.py` validates both its capsule path/id and its Skill frontmatter.

For Method implementation/conformance, use AIKit's native `Method` / `resolve_method` / `resolve_praxis` contracts and their tests rather than inventing a second Method store in this Skill.
