---
name: aikit-personal-history-methodology
description: METHOD: Bring a person's earlier writing through Central adoption into source-grounded knowledge and delivered personal context — collection intake, whole-entry episode reading, cross-period comparison, constellation construction, and correction. Select this method before any journal/notebook intake or personal-history question; near-miss: knowledge-navigation answers recorded-knowledge questions, this method generates and maintains personal knowledge from adopted history without flattening the person into a profile.
---

# Personal-History Contemplation Methodology

Semantic ref: `aikit:personal-history-methodology`. Native owners: `EpiLogos/ai-kit`
(knowledge), `EpiLogos/Central` (intake), `EpiLogos/O-I` (encounter).
Commissioned by Central #242.

A person's earlier writing becomes useful later knowledge only through actual
contemplation: reading whole entries, keeping their meaning qualified, citing
exact sources, and correcting errors without overwriting the person. Every
stage below has a native operation; none is optional and none may be
shortcut by summary.

## The five Methods

### 1. Collection intake (Central owns it)

Run `central.personal.collection.inspect` → `plan` → `apply` with
`acceptance:"human-accepted"` — an agent's plan alone is not authorship. The
collection record (`central.personal-collection/v1`) carries the person and
author bindings, per-entry dispositions, and event dates with bases. Never
manufacture present-day activity from historical entries; event time,
writing time, import time and interpretation time stay separate.

### 2. Episode reading (whole entries, never summaries-of-summaries)

Read coherent entries and episodes whole: `aikit knowledge read` returns the
complete body and records observed coverage itself. For long entries, read
by exact span (`knowledge read --span START:END`) — the span answer names
the uncovered ranges; continue until the relevant whole was actually
considered. Then state your semantic reading explicitly:

```sh
aikit knowledge coverage declare SOURCE --revision REV --extents 0:N \
  --actor AGENT_REF --note "what was read, and what the entry actually means"
```

A checksum, a successful copy, or a first-page parse proves nothing about
consideration. Declared and observed coverage stay separate kinds.

### 3. Cross-period comparison (qualify before you relate)

Compare across entries only at their actual meaning. A statement is evidence
that the person expressed it in a context — never automatically a permanent
trait or an externally verified event. Keep dream, quotation, recollection,
negation, earlier interpretation and later correction distinct; carry
tensions and counterexamples alongside patterns. A later correction
supersedes only its own stated scope.

### 4. Constellation construction (source-backed, or not at all)

Stage knowledge with `aikit wiki-construct apply --file <owner-wiki>` using
the native change grammar (create/member_add/relation_put). Every
participation cites `source_ref + source_revision (+ text_span selector)`;
relations carry direction and standing (`proposed/asserted/contested/
uncertain`); the inquiry and each participation's note say what the material
actually means and why the relation fits. A valid geometry or attractive
graph proves nothing; semantic fidelity is reviewed independently. After
persistence, prove the readback: `knowledge search` finds the staged nodes
and `knowledge open` reopens each cited source exactly.

### 5. Correction (invalidate forward, never rewrite the person)

A correction attaches to the affected interpretation and its source basis:
declare the corrected reading at the same source revision, stage the
corrected constellation change, and let downstream preparation recompose.
The original journal is never edited; the person's own words stay quoted.

## Boundaries

- Private material stays inside its selected processing scope; public tests
  use openly authored controlled corpora only.
- Historical intake does not reset a journey, rewrite natal/current inputs,
  or manufacture a cast. Journey continuity runs through its own native
  operations.
- The person is the subject, never a generated profile: the personal anchor
  (`central.personal.anchor.inspect`) names the person/world binding; the
  same display name on another person is a different subject.
