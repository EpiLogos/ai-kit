---
name: projection-authoring
description: Select, review, ratify, refine, withdraw, or prepare an authored world reading for O:I Projection without transferring canonical ownership from Central, a Project, Wiki space, or another native source.
argument-hint: Name the source world, intended audience, selected material, current Projection ref/revision if any, and whether this is draft, review, publication, refinement, withdrawal, or return-to-source work.
---

# Projection Authoring

Prepare a deliberate representation of a native authored world for O:I Projection.

A Projection makes selected aspects present elsewhere. It does not become the canonical owner of the thing projected.

## 1. Resolve the native owner

Before authoring, identify:

- the native subject ref and kind;
- the canonical source system, ref, and revision;
- the current Projection ref/revision when refining;
- the intended audience and visibility;
- the human or Agent editor provenance;
- the representation being projected.

Do not create a parallel profile or Project record because a public representation is needed.

For a person, the meaningful profile is the projected face of their Central world.

For a Project, the Project remains owned by its native filesystem, repository, Project Canon, or other established authority.

## 2. Selection is explicit

Treat every public or shared inclusion as selected material.

Never infer:

```text
one Control/user file selected -> Control/user is public
one Project selected -> all Project files are public
one Wiki node selected -> whole Wiki is public
local availability -> projection eligibility
```

Record source provenance for selected modules. Omit unselected private material entirely from the public representation.

## 3. Compose through WorldPresentation

When rich presentation is required, use the O:I `oi.world-presentation/v1` representation rather than inventing a page/profile schema.

A WorldPresentation can reference native Component, ComponentContribution, Surface, subject, nested Projection, Project, Wiki, Agent, Run, Claim, Evidence, or other semantic refs while preserving their native ownership.

Use `skill/aikit/structured-account-authoring` when a coherent deep reading must be composed first.

## 4. Draft, review, publication

Keep these states distinct:

```text
local draft != Projection
preview != publication
localStorage != source authority
```

A published or shared representation must become an explicit Projection revision through the O:I provider/runtime that owns publication.

Do not describe a preview as published merely because it renders correctly.

## 5. Refinement

A human edit to the public/world presentation creates a new Projection representation revision.

Preserve the canonical source system and source revision unless the native source itself changed.

```text
source R1
    ↓
Projection P1
    ↓ representation refinement
Projection P2
    ├─ still grounded in source R1
    ├─ editor provenance retained
    └─ supersedes P1
```

Do not silently switch the subject ref while refining one projected world.

## 6. Withdrawal and supersession

When a selected presentation should no longer be current, use Projection withdrawal or supersession semantics rather than deleting or rewriting canonical source.

Retain enough history to explain which Projection revision was visible and what replaced it.

## 7. Return to source

A public refinement can reveal a better formulation of native authored ground. That insight can return, but the native owner decides whether it becomes source.

For Central:

```text
Projection difference
    ↓ explicit proposal
Central review
    ↓ accepted mutation
Control revision
```

Do not silently rewrite Central Control.

For a Project, use the Project's existing authoring/review mechanism. If Software Factory Project/Run/Claim/Evidence semantics are active, return evidence through those native seams rather than inventing an O:I mutation owner.

## 8. Local and public readings

Treat local and public as readings of the same semantic world, not separate identities.

Local O:I may resolve more source, private working state, machine state, unpublished design, current Runs, or proposals than public Explore.

Public Explore presents only the ratified Projection revision.

## 9. Human and Agent parity

The same Projection ref/revision must identify the representation rendered for a human and read by an Agent.

Do not require an Agent to scrape standalone HTML. Prefer the structured WorldPresentation/Projection reading when available.

HTML, desktop, Explore, and agent surfaces are renderings or readings of the same Projection relation.

## 10. Completion check

Before publication or handoff, verify:

- subject identity still names the native world;
- source system/ref/revision are exact;
- audience/visibility are explicit;
- every projected module was intentionally selected;
- editor provenance is retained;
- unselected material is absent;
- the representation does not introduce remote executable authority;
- public refinement does not mutate native source;
- return-to-source work is an explicit proposal;
- the same Projection identity is available to human and Agent surfaces.
