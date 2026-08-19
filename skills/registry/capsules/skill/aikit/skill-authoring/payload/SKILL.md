---
name: aikit-skill-authoring
description: Use when creating, reviewing or simplifying a Skill or persistent guidance source; design its trigger, scope, disclosure, procedure and conformance without turning instruction history into permanent prompt sediment.
---

# Skill and instruction authoring

Semantic ref: `aikit:skill-authoring`. Native owner: `EpiLogos/ai-kit`.

Use this Skill when the task is to create, substantially revise, review or simplify operational guidance or a reusable Agent Skill. Do not invoke it merely because a task happens to use an existing Skill.

The practical premise is modest: instruction language is part of the operative environment for a language-model Agent. Names stabilise distinctions; verbs and examples bias possible acts; triggers and stop conditions shape what enters attention and when work should return. This is an engineering claim about language-conditioned behaviour, not a claim about phenomenal consciousness.

## Source ownership first

Keep durable human-authored governance source separate from AIKit's operational resolution:

```text
Central
    Control/agents/governance/**
    ProjectCentral/agents/governance/**
        owns authored source identity, provenance and durable scope

AIKit
    GuidanceFragment / Skill / Profile / ContextResolution
        owns availability, selection, disclosure, composition and harness projection
```

When Central provides `central.agent-governance-relations/v1` source relations, preserve the Central ref/path/provenance/treatment. Do not copy that source into a second AIKit canon merely to resolve it operationally.

Central's root-vs-Project source hierarchy is **not** an instruction-precedence algorithm. Explain the actual AIKit resolution/selection that produced an operative fragment when precedence or conflict matters.

## Authoring pipeline

Use this common path:

```text
identify owner + triggering situation
        ↓
separate stable governance from situational procedure
        ↓
choose invocation / disclosure mode
        ↓
define vocabulary, inputs, semantic operations and boundaries
        ↓
write the smallest sufficient common path
        ↓
attach references / examples only where they earn context cost
        ↓
add positive + negative trigger fixtures
        ↓
add behavioural conformance / failure regression
        ↓
review for no-op text, sediment and duplicated source
        ↓
publish through existing source / trust lifecycle
```

For a small ordinary Skill, stop after the common path is clear and tested. Open `references/instruction-architecture-review.md` only when the task involves a substantial guidance corpus, repeated failures, trigger ambiguity, progressive disclosure, phase separation or historical instruction audit.

## 1. Owner and trigger

State who owns the procedure and the situation that should make it enter consideration.

Treat the capsule/Skill description primarily as a **routing affordance**. It should be concise enough to compete well in selection while discriminating the relevant situation from nearby but unrelated work.

Prefer a short natural-language relation over raw keyword soup:

```text
weak
    "skills markdown prompt agent authoring"

better
    "Use when creating or revising an AIKit Skill so its trigger, procedure and conformance remain explicit."
```

Conformance should include both:

- a positive case that should select/consider the Skill;
- a nearby negative case that should not.

Availability does not mean automatic invocation.

## 2. Separate stable guidance from procedure

Use persistent guidance for relatively stable orientation such as:

```text
shared vocabulary
cares / priorities
architectural truths
collaboration stance
cross-project preferences
important environment/tool quirks
```

Use Skills for situational procedures/capabilities opened when a relevant act warrants them.

This is not a universal Markdown taxonomy. It is a context-economy rule.

When reviewing governance, process-heavy material should normally become one of:

```text
compact governance statement + Skill reference
Project-local governance
Skill procedure
context pointer/reference
ordinary Project documentation
no durable instruction at all
```

Do not move source merely for neatness. When Central governance is the authored owner, propose the smallest Central source change and leave adoption to the human.

## 3. Choose invocation and disclosure deliberately

Reason about two costs separately:

```text
model/context load
    always-visible or model-invoked material competes for operative attention

human invocation load
    user-only material keeps context lean but asks the human to remember when to call it
```

Choose deliberately among:

- always-present compact guidance;
- model-invoked Skill;
- user-invoked Skill;
- context pointer/reference opened only when a condition requires it.

There is no universal token threshold. Record the reason for the choice when it is consequential.

## 4. Define operative language

Use compact leading terms when they genuinely compress a recurring relation, for example `Project ground`, `Recognition`, `source return`, `vertical slice` or `tracer bullet`.

A term must retain the relation that gives it operational meaning. Do not replace an explanation with a catchy phrase and assume shared understanding.

Where terminology is load-bearing, add a fixture/example proving the intended interpretation.

Prefer positive operational specification:

```text
prohibition only
    do not dump the whole corpus

positive operating relation
    retrieve the smallest source set that answers the current question;
    deepen only when authority or evidence requires it
```

Keep a prohibition when it protects a real boundary, but say what to do instead wherever practical.

## 5. Write the smallest sufficient common path

A Skill body should orient the common act without loading every rare branch.

Use stable semantic operations rather than UI coordinates where the owning domain exposes them, for example:

```text
inspect -> stage -> preview -> apply -> verify
```

Move infrequent templates, long examples, provider-specific recipes and deep audit checklists behind explicit references:

```text
common procedure
    ↓ if condition X arises
open reference Y
```

A reference is progressive disclosure, not a second source of truth. Keep its ownership/revision clear.

## 6. Examples and contrasts

Use compact good/bad or alternative examples when prose leaves an important distinction underdetermined.

Examples should demonstrate the consequential behaviour rather than accumulate specimen noise. Add a regression that the example changes the targeted case without contaminating unrelated tasks.

## 7. Human authority is inherited, not reimplemented

Apply `aikit:product-understanding`'s human-authority discipline rather than creating a second decision framework:

```text
current request / recognised authored ground
    ↓
exact source / evidence
    ↓
current implementation / observation
    ↓
reversible engineering judgement
    ↓
bounded prototype where useful
    ↓
only then genuinely unresolved human authorship / Recognition
```

A planning Skill must not ask the human for a fact the Agent can retrieve or invent a human decision merely to continue.

When human authorship is genuinely required, ask at the consequential experienced/product/architectural level and keep incidental implementation mechanics as supporting detail.

## 8. Phase separation when future artifacts distort earlier work

Some procedures become shallow when later outputs are visible too early and turn into premature targets.

Use explicit phases when this is an observed risk:

```text
investigate / grill
        ↓ closure condition
plan
        ↓
execute
```

Phase separation is optional. State why it protects inquiry quality or a real authority boundary. Do not impose it on work whose risk is better served by a direct vertical slice.

## 9. Vertical slices and tracer bullets

Where development guidance owns build order, prefer a testable vertical slice/tracer bullet when crossing the whole relation gives earlier evidence for the relevant risk.

Do not turn the phrase into dogma. Some tasks are correctly horizontal. The authoring requirement is to name what earlier evidence the chosen slice produces.

## 10. Regression before sediment

For any accumulated instruction, ask:

```text
what recurring behaviour/evidence does this sentence change?
what failure does it prevent or capability does it enable?
if removed, does conformance materially regress?
```

If the answer is unclear, delete, consolidate, narrow or move it before adding more instruction.

One historical failure should not automatically become a permanent global rule. First classify whether the cause was:

- missing source/context;
- ambiguous vocabulary;
- poor procedure;
- tool/environment quirk;
- model/harness-specific behaviour;
- stale instruction;
- genuine product/architecture ambiguity.

Then change the smallest owning source/capability.

Historical behaviour is evidence, not authorship. An Agent may propose a change to human governance; it must not silently self-modify that source.

## 11. Communication is part of capability

Define what useful Return looks like where the Skill performs substantial work:

- concise problem -> solution framing;
- evidence in a human-inspectable form;
- visual/video/HTML artifact when it communicates the result better than terminal transcript;
- clear separation of authored claim from generated proof/readout;
- exact source/revision/CI evidence where completion depends on them.

Good communication is not synonymous with length.

## 12. Package and trust lifecycle

Create the ordinary Agent Skill body (`SKILL.md`) and package it with the existing AIKit capsule manifest (`kind = "skill"`, `[skill] root = "payload"`). Keep capsule id/path aligned.

Make source/projection status explicit: repository/managed source + exact revision is authoritative; harness projections are derived.

Submit source/revision through the existing managed-source review/trust lifecycle. Repeated success or fitness evidence may inform review but never promotes source automatically.

## Verification

Before completion, verify at least:

- owner and trigger are explicit;
- positive and negative trigger cases are present where routing matters;
- stable governance and procedure are not duplicated into each other;
- invocation/disclosure mode has a reason;
- the common path does not load rare references unnecessarily;
- positive operating behaviour accompanies important prohibitions;
- a no-op/deletion review was performed;
- historical failure evidence changes only the smallest owning source;
- human-authority cases reuse `product-understanding` rather than inventing a parallel decision ontology;
- source/projection/trust boundaries remain intact.

## Specimens

`skills/fixtures/minimal-authored-skill/` remains the representative small Skill produced by this procedure.

`skills/fixtures/instruction-craft/cases.toml` proves the instruction-architecture behaviours, including trigger routing, progressive disclosure, deletion/sediment control, phase separation, human-authority reuse and regression-driven governance proposals.
