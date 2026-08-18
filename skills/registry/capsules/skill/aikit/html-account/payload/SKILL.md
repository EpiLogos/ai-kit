---
name: html-account
description: Create clear, research-grounded, self-contained HTML accounts for explanation, analysis, design intent, review, reference, documentation, Projection presentation, and interactive dissemination. Use the canonical portable account template, adaptive depth, provenance discipline, natural visible headings, and task-appropriate diagrams, data views, mockups, links, and interactions.
argument-hint: Describe the subject, audience, sources, purpose, desired depth, intended Projection if any, and required surfaces or interactions.
---

# HTML Account

Create HTML that lets a reader understand, inspect, navigate, and use a body of information.

The output is an **account**: one coherent representational reading. It is not new canon and it is not a universal `Account` ontology.

When this Skill receives a structured account or O:I WorldPresentation reading, preserve its source refs, Projection identity, revision, provenance, and native object refs. HTML is one renderer of that reading. Agents should use structured data directly when available rather than scrape generated HTML.

The default deliverable is a standalone HTML file based on `full-account-template.html`.

## 1. Governing principles

1. **Meaning determines form.** Choose prose, diagrams, tables, charts, mockups, controls, images, or links because they reveal the relation at hand.
2. **Wholeness is an authoring test, not a visible schema.** QL can supply a semantic coordinate and recursive composition grammar without forcing six visible headings.
3. **Depth follows the task.** A full account can contain sustained technical prose, dense evidence, large diagrams, and working prototypes. Do not compress important reasoning into slogan cards.
4. **Research precedes assertion.** Ground consequential claims in supplied material or high-quality external sources.
5. **Writing is part of the interface.** Apply ASD-STE100 discipline and direct editorial judgement before styling prose.
6. **The shell serves the content.** Navigation and context rails must never reduce the main document to leftover space.
7. **Interaction is direct.** Sliders, toggles, selectors, filters, and editable fields update immediately when immediate feedback is meaningful.
8. **Standalone is the publication default.** Inline required CSS, JavaScript, SVG, and local images in the final HTML.
9. **Source authority survives rendering.** HTML must not replace Central Control, Project canon, Wiki material, code, Claims, Evidence, Runs, or another native source.
10. **Projection identity survives rendering.** When HTML renders an O:I Projection, include the exact Projection and presentation refs/revisions in machine-readable provenance.

## 2. QL-informed composition

A complete account may be tested internally through six practical semantic basins:

| QL | Practical basin | Primary question |
|---|---|---|
| `#0` | Ground / Source | Why is this object or question here? |
| `#1` | Definition / Material | What is it? |
| `#2` | Operation / Dynamis | How does it work or change? |
| `#3` | Pattern / Identity | What structure or relation makes it this kind of thing? |
| `#4` | Context / Horizon | Where, when, and under which conditions does it meet a field? |
| `#5` | Synthesis / Integration | What does the whole amount to and what returns forward? |

Use the smallest account that is whole for the human's purpose. A task can use one surface, selected surfaces, or all six.

Do not invent content to equalize the six surfaces.

### Natural visible headings

Derive visible headings from the subject.

A personal Central reading might use:

```text
Where I am
What matters to me
How I work
The people and agents around me
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

A scientific or ordinary non-QL Project can use a completely different natural form.

Keep QL coordinates in machine metadata or navigation only when they are useful. Do not expose them merely to prove that QL informed composition.

## 3. Nested completeness

For a substantial surface, the author may recursively test:

- local ground — what must already be true or understood;
- local definition — what becomes determinate;
- local operation — what acts, changes, relates, or transforms;
- local pattern — what structure or identity becomes visible;
- local context — under what conditions the reading holds;
- local synthesis — what result or recognition leaves the surface.

These are authoring functions. Two functions may share a visible block when the relation remains intelligible.

## 4. Research and epistemic grounding

Before authoring factual, technical, medical, legal, scientific, historical, product, or current-state material:

1. inspect supplied sources;
2. identify the claims the account must make;
3. gather primary or high-trust sources where external grounding is required;
4. separate source-supported statements from interpretation or inference;
5. preserve unresolved disagreement;
6. keep a source ledger that can be projected into the HTML.

For consequential statements, be able to answer:

- What is the claim?
- Why is it present?
- Which source or evidence supports it?
- Is it observation, definition, inference, interpretation, recommendation, or decision?
- What would change its standing?

When `skill/aikit/product-understanding` is available, retain its provenance classes rather than flattening intent, design, architecture, implementation and current evidence into one voice.

## 5. Writing standard

Use ASD-STE100 Simplified Technical English as the principal clarity discipline where appropriate.

Default prose rules:

- give information gradually;
- use key words and phrases to make logical structure visible;
- keep descriptive sentences near 25 words or fewer when practical;
- give each sentence one clear subject or idea where practical;
- group related information in paragraphs;
- keep one topic per paragraph;
- prefer active voice;
- use complete sentences;
- avoid contractions in formal explanatory text;
- keep terminology stable;
- define specialist terms before relying on them.

For procedures:

- keep instructions near 20 words or fewer;
- put one instruction in each sentence unless actions occur together;
- use the imperative;
- put necessary conditions before the command;
- do not hide instructions in notes.

Do not claim formal ASD-STE100 compliance unless the current official standard and dictionary were actually checked for the relevant text.

## 6. Depth

Long-form prose is first-class.

Use these planning ranges, not quotas:

- compact surface: 300–700 words;
- standard surface: 700–1,500 words;
- deep surface: 1,500–3,500+ words;
- full account: 4,000–20,000+ words when the evidence and subject warrant it.

Do not replace a necessary argument with cards or one diagram.

## 7. Representation routing

Choose the representation that makes the required relation easiest to inspect.

| Relation | Preferred forms |
|---|---|
| sustained reasoning | prose, citations |
| sequence / workflow / state transition | flowchart, timeline, state diagram |
| hierarchy / decomposition | tree, mindmap, nested outline |
| network / dependency | graph, node-edge map |
| architecture / topology | bounded SVG diagram, layer map, system schematic |
| quantities across categories | bar chart |
| ordered change | line chart |
| exact comparison | table, comparison matrix |
| provenance | source cards, evidence table, timeline |
| code / schema / formula | code block, syntax block, equation panel |
| visual evidence | image, annotated figure, gallery |
| UI or component intent | actual HTML mockup or prototype |
| parameter-sensitive behaviour | slider, toggle, selector, direct-manipulation simulator |
| review / decision | alternatives, criteria, evidence, notes, recognition controls |

A diagram must answer a visual question. Prefer inline SVG for generated standalone artifacts. Every SVG needs a real `viewBox`, readable labels, a bounded figure container, and an explanatory caption.

If Mermaid is useful during drafting, render it to inline SVG before final standalone export. Do not require a Mermaid runtime in the final file.

## 8. Interaction design

Interaction must change understanding, reveal information, or perform a useful operation.

Preferred behaviours:

- update sliders on `input`;
- update toggles immediately;
- filter tables and graphs immediately;
- switch views without an Apply button;
- update dependent diagrams and readouts together;
- save notes while the user types when persistence is useful.

Use Apply, Run, Submit, Commit, or Publish only when there is a real transactional boundary.

Persistence is best-effort. The account must still navigate if `localStorage` is unavailable.

## 9. Canonical shell

The maximal template has three regions.

### Left rail — addressability

The left rail provides generated contents, nested routes, active-route highlighting, and granular hash navigation.

### Center — the account

The center is the primary artifact. Sustained prose stays within a readable measure. Large diagrams, tables, graphs, and mockups can use wide or full-width regions.

Use CSS container queries when layout depends on the actual available main width.

### Right rail — situated context

The right rail is optional and section-relative. It can contain notes, current context, source links, review controls, provenance, confidence, or status fields.

Do not keep an empty right rail open.

## 10. Component grammar

Available presentation primitives include:

- prose and lede;
- definition / distinction;
- note / warning / caveat;
- Claim/Evidence reading;
- source card;
- table or comparison matrix;
- timeline;
- flowchart, architecture diagram, relation graph or mindmap;
- chart;
- equation;
- code or schema;
- image / figure / gallery;
- UI mockup;
- slider, toggle, selector or filter;
- editable field;
- Wiki excerpt;
- Project / Agent reference;
- Run/history reading;
- Action or review affordance.

These are rendering capabilities. They do not redefine native source objects.

## 11. Provenance envelope

Every generated standalone account must carry machine-readable provenance.

Recommended fields:

```json
{
  "uid": "account-...",
  "kind": "html-account",
  "title": "...",
  "project": "...",
  "session": "...",
  "status": "draft|review|recognised",
  "ql": [0, 1, 2, 3, 4, 5],
  "created": "ISO-8601 timestamp",
  "updated": "ISO-8601 timestamp",
  "source_basis": [],
  "relations": [],
  "generator": "...",
  "projection_ref": "optional exact O:I Projection ref",
  "projection_revision": "optional exact O:I Projection revision",
  "presentation_ref": "optional exact WorldPresentation ref",
  "presentation_revision": "optional exact WorldPresentation revision"
}
```

If a Projection is being rendered, copy its identity rather than inventing an HTML-specific public identity.

## 12. Standalone export contract

A default generated HTML file must work by itself:

- inline required CSS;
- inline required JavaScript;
- use inline SVG for generated diagrams;
- embed required local raster images as data URIs;
- do not depend on sibling CSS, JavaScript, fonts, or images;
- do not depend on a local server;
- allow ordinary external source links.

If the human asks for a deployable multi-file package, a shared-assets build can also be produced. Keep a standalone build when portability remains part of the request.

## 13. Accessibility

- use semantic landmarks;
- use real buttons for controls;
- preserve keyboard operation;
- show visible focus states;
- use `aria-expanded` for disclosure controls;
- give images useful alt text;
- give diagrams captions;
- do not use color as the only carrier of meaning;
- respect `prefers-reduced-motion`;
- keep text selectable and copyable.

## 14. Navigation

The maximal template supports hash routes such as:

```text
#q0
#q2/execution-path
#q4/evidence
```

Browser back/forward must work. Nested routes must switch to the correct surface and section. Active section tracking must remain correct at the bottom of a surface.

Natural visible titles remain independent of these coordinates.

## 15. Authoring workflow

### Step 1 — Resolve purpose

Identify audience, intended use, required depth, source basis, current-state versus historical material, visibility, and whether a Projection is involved.

### Step 2 — Research before layout

Gather the material needed to make the account true and useful.

### Step 3 — Compose the semantic map

Use the subject's natural sections. Use QL internally only when it improves completeness.

### Step 4 — Choose representation per relation

Ask what the reader needs to see, understand, compare, manipulate, or verify.

### Step 5 — Draft prose at full required depth

Write the explanation before reducing it into labels and components. Apply the clarity pass after technical content is correct.

### Step 6 — Compose the standalone HTML

Start from `full-account-template.html`. Remove unused surfaces or components rather than adding artificial material.

### Step 7 — Preserve structured identity

When rendering an O:I WorldPresentation, preserve its Projection/source/provenance refs and do not make HTML the only machine-readable form.

### Step 8 — Validate

Check content, provenance, responsive layout, navigation, interaction, accessibility, and standalone integrity.

## 16. Validation checklist

### Content

- [ ] purpose is explicit;
- [ ] visible headings are natural to the subject;
- [ ] the account has enough depth;
- [ ] cards do not replace necessary explanation;
- [ ] every visual answers a real question;
- [ ] consequential factual claims have source support;
- [ ] inference and interpretation remain distinguishable;
- [ ] native source ownership is still clear;
- [ ] projected material is explicitly selected.

### Layout

Test at minimum: 1600px, 1180px, 900px, 720px, and 390px.

At each size verify no page-level horizontal overflow, correct rail reflow, bounded diagrams, readable prose, and clear spacing.

### Interaction

- [ ] no JavaScript errors;
- [ ] theme switch works;
- [ ] rail toggles work;
- [ ] browser back/forward routing works;
- [ ] bottom-of-surface tracking works;
- [ ] controls update directly;
- [ ] blocked `localStorage` does not break navigation;
- [ ] keyboard focus is visible.

### Standalone integrity

- [ ] no required local stylesheet dependency;
- [ ] no required local script dependency;
- [ ] no required local font dependency;
- [ ] no required local image dependency;
- [ ] every inline SVG has a `viewBox`;
- [ ] all internal fragment routes resolve.

## 17. Central and Project use

For Central, a rich HTML personal/world account is an optional reading over authored Control and Work material. It must never imply that all Central content is public.

For a Project, HTML is an optional rendering of Project understanding, design, architecture, Wiki, source, Runs, Claims/Evidence, and current reality.

The same document craft can serve both. Their meanings remain different.

## 18. Template

Use `full-account-template.html` as the maximal base. It contains the complete shell, generated navigation, section-relative context, light/dark themes, robust hash routing, best-effort note persistence, responsive rails, long-form typography, wide breakouts, reusable component examples, print behaviour, and machine-readable provenance.

Keep the shell stable. Change content, headings, diagrams, controls, proportions, and rendered surfaces according to the actual subject.
