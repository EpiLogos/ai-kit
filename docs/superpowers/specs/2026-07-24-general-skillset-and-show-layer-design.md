# AIKit — composables, two default suites, and the internal/external seam

Status: draft for review · 2026-07-24 (rev 3)

Three things, one architecture:

1. **A completed composables taxonomy.** AIKit already makes *verbs* durable and
   reusable (`script`). The same discipline is owed to **meaning** (the shared
   language between agent and user) and to **modules** (whole instantiable units of
   code). Both already have a home in the existing model; neither is currently
   pointed at that job.
2. **Matt Pocock's engineering skills** (<https://github.com/mattpocock/skills>) as
   the general **internal-facing** suite — everyday coding discipline, out of the box.
3. **wigglystuff** (<https://github.com/koaning/wigglystuff>) as the general
   **external-facing** suite — display tools the agent invokes to show the user a
   concept, so they learn while they code.

**No new capsule kind is introduced.** Everything below lands on `Kind::Guidance`,
`Kind::Template`, `Kind::Script` and the existing procedures. Where a distinction is
needed it is a facet under `metadata.aikit`, per the spec's own namespacing rule.

---

## 0. The seam: internal-facing vs external-facing

Every skill's deliverable goes to one of two places.

- **Internal-facing** — the deliverable feeds the agent's own continued work: a
  sharpened spec, a passing test, a resolved merge conflict. The user may be heavily
  involved (a grilling interview) but the *product is the work*.
- **External-facing** — the deliverable is *for the user to look at*: a prototype, a
  chart, an interactive widget, a lesson. The product lands in the user's
  understanding, not the agent's task state.

Classify by **who consumes the deliverable**, not by whether the user is spoken to.
That resolves the fuzzy middle: `grilling` is internal (it sharpens the plan);
`handoff` is internal (a doc for the next agent); `teach` and `prototype` are
external.

Why this seam is worth managing rather than leaving to prose:

1. **It tells the agent when to reach for which** — do, vs. show.
2. **It is a real mechanical difference.** Internal-facing output flows back into the
   agent's context. External-facing output has to *reach the user's screen* — a
   notebook, a browser, an artifact on disk. That surfacing step is the only new
   mechanic in this document.
3. **It is how suites group.** Pocock is internal-facing; wigglystuff is
   external-facing; a project points at both.
4. **Shared language sits exactly on the hinge** — it is the one thing that faces
   both ways, which is why §2 gets its own section rather than being folded into
   either suite.

---

## 1. The composables taxonomy

`STANDARDS.md §0` states the gradient: *"the work an agent does — figuring out a
command, a sequence, a piece of guidance, a fix — should leave behind something
named, reusable, composable and dependable."* Today that promise is best kept for
commands. Three siblings, one gradient:

| Composable | Kind | Within one project | Across projects |
|---|---|---|---|
| **verbs** | `script` | project commands | your command library |
| **meaning** | `guidance` (+ facet) | per-module interface notes — navigability | glossary, vocabulary, "my approach to neo4j" |
| **modules** | `template` | instantiated here | a neo4j client, a redis compose — define once, instantiate anywhere |

The two new columns are the point, and the **cross-project column is where the value
compounds**: per-project is useful once, cross-project is useful every time after.
Both ride machinery that already exists — capture → `ProcedureKind::Promote` → an
`Owned` registry — and frecency, which is per-context-first but knows global usage,
so each new project starts richer than the last.

### 1.1 Two tiers, and the word that matters

There is a cheap tier and a durable tier, and the distinction between them is the
difference between a useful library and a junk drawer.

- **Cheap capture — bkmr.** `docs/integrations/bkmr.md` opens by describing bkmr as
  *"a single-binary knowledge base: bookmarks, snippets, shell scripts and markdown
  files in one SQLite database."* It is already project-scoped (`AIKIT_BKMR_DB`),
  already cross-searchable over a **declared** `also = [...]` set, already
  attributable by the `_<project>` tag convention (§5 #17). A fragment worked out mid
  session goes here at near-zero cost.
- **Durable — a capsule.** When a fragment earns it, `Promote` turns it into a
  `guidance` or `template` capsule in a registry: manifest, declared effects,
  provenance, trust gate, revision.

**The manifest is the curation.** A bank of half-useful fragments is what you get from
un-manifested capture; a library of composable specifics is what you get when each
member is a named capsule that declares what it is, what it touches, and where it came
from. The promote step is the filter, and it is deliberately a human's
(`STANDARDS.md §0` counterweight: *"nothing becomes trusted because it became
durable"*).

Terminology, deliberately: **"snippet" is the cheap tier only.** The durable tier is a
**composable module** — a whole, self-contained, Unix-philosophy unit. Calling a
`template/service/neo4j-client` a snippet undersells it and invites the junk drawer
back in.

---

## 2. Meaning as a composable — shared language

### 2.1 Why this is the interesting one

Matt's `domain-modeling` skill maintains a repo glossary (his `CONTEXT.md`) and ADRs so
that agent and user mean the same thing by each term. His README frames two of his four
agent failure modes as language failures: *the agent didn't do what I want*
(misalignment) and *the agent is too verbose* (no shared vocabulary to be terse in).

Shared language is therefore not documentation. It is the **alignment contract between
the agent's work and the user's understanding** — the hinge of §0's seam, and the
reason it belongs in this document alongside the display tools rather than in a docs
section.

### 2.2 The mechanism already exists

`aikit-core::guidance` composes prose fragments with `order`, `dedup_key`,
`precedence` and a token budget, and its module header states the invariant it owns:
composition is bounded and accounted for, and a fragment that does not fit is dropped
**whole** rather than truncated. Critically, `dedup_key` + `precedence` mean *a
project-scoped fragment replaces the global one it was written to override* — the
`bkmr` guidance capsule already uses `dedup_key = "tool:bkmr"` for exactly this.

That is a prompt-component library's merge algebra, already written, already tested.
Shared language needs no new composer.

### 2.3 Two scopes, one facet

The vocabulary gets used at two scales, and both are already the system's own ethos:

- **Across codebases — the vocabulary itself.** `codebase-design`'s glossary
  (`Module`, `Interface`, `Depth`, `Seam`, `Adapter`, `Leverage`, `Locality`) applies
  to any repo; the skill's own instruction is *"Use these terms exactly… Consistent
  language is the whole point."* Your neo4j-approach language is the same shape:
  extract once, use in any project.
- **Within a codebase — applying it to these modules.** `codebase-design` defines
  `Interface` as *"everything a caller must know to use the module correctly"* — type
  signature plus invariants, ordering constraints, error modes, performance — and
  pitches the payoff as making code **AI-navigable**. That is the API-header
  navigability win: a per-module description an agent can read instead of the
  internals.

AIKit already believes this about its own source: `STANDARDS.md §4` ("`lib.rs` is a
map"; "every module header states the invariant it owns and why that seam exists") and
`SPEC-II §10` ("the code is the map", plus `docs/MAP.md`) *are* deep-module discipline
applied to AIKit. So the within-codebase usage is not a new idea to import — it is an
existing standard, now capturable as a composable.

One facet, two scopes, no second mechanism: the composer's `dedup_key`/`precedence`
already lets the project layer extend or override the global vocabulary.

```toml
# capsules/guidance/language/deep-modules/manifest.toml  (cross-project)
kind = "guidance"
id   = "guidance/language/deep-modules"

[metadata.aikit]
facing = "both"          # the hinge: aligns the agent AND teaches the user
language = "vocabulary"  # vocabulary | module-interface | approach

[guidance]
entry     = "payload/glossary.md"
inject    = ["SessionStart"]
dedup_key = "language:deep-modules"
token_budget = 500
```

`language = "module-interface"` is the within-codebase case (project-scoped,
per-module); `"approach"` is the "how I do neo4j" case. The facet is for search,
presentation and dedup discipline — **not** activation: rule 6 stands, *"nothing
becomes active merely because it matches a tag."*

---

## 3. Modules as a composable — `Kind::Template`

The `Kind` enum already carries `Template`, described in `capsule.rs` as *"available to
materialize into a project or task."* That is define-once-instantiate-many, already in
the domain model and currently under-used.

So: a neo4j client is `template/service/neo4j-client`; a redis compose file is
`template/infra/redis-compose`. Each is a whole unit with a manifest, declared
effects, provenance and a trust gate — instantiable into any number of projects, and
`Kind::Template` already participates in the existing "materialize into a project"
path.

This is the distinction that keeps the library clean: **"my approach to neo4j"
(meaning) is `guidance`; "a neo4j client I can drop in" (code) is `template`.** Same
subject, different kinds. Conflating them is precisely how a composable library decays
into a fragment bank.

---

## 4. Suite one — Pocock's skills (internal-facing default)

### 4.1 Why this suite

Not because the skills are individually good (they are), but because their design ethic
already is AIKit's: every skill leaves a durable, named, reusable artifact behind —
glossaries, ADRs, specs, tracer-bullet tickets. That is `STANDARDS.md §0` almost
verbatim. It is also already dual-harness (every skill ships an `agents/openai.yaml`
beside its `SKILL.md`), so it lands on the claude and codex targets without a rewrite.

### 4.2 What comes in — wholesale

The 22 promoted skills (17 `engineering/`, 5 `productivity/`), through the ordinary
npx-skills-compatible import AIKit already models, grouped by the skill-sets addition.
Two exceptions, hardcoded to Matt's own setup and not generalisable: **drop**
`obsidian-vault` (hardcoded vault path) and `scaffold-exercises` (coupled to his
course's `ai-hero-cli`). The 19 non-promoted skills stay out, matching his own line;
re-check `in-progress/` on each upstream sync.

### 4.3 The composition graph is `[[requires]]`

Several of his skills are one-liners that delegate — `grill-me → grilling`,
`grill-with-docs → grilling + domain-modeling`, `implement → tdd + code-review`,
`triage → grilling + domain-modeling`. Today that lives in prose plus a hand-maintained
router (`ask-matt`). In AIKit it is `[[requires]]` edges, the pattern
`contrib/bkmr/` already uses. The payoff is `aikit set show`: rather than silently
shipping `grill-with-docs` when `domain-modeling` is unreviewed — a skill referencing a
verb that does not exist — the set reports the withholding and why (`SPEC-III §6`). The
router is replaced by the resolved graph plus frecency (`aikit z grill`).

### 4.4 Trust, per the spec

Delivery is a reviewed in-repo bundle, `contrib/mattpocock/`, mirroring
`contrib/bkmr/`, copied into a registry — **not** a live `npx skills add`, which is
silent-clobber with no gate (`SKILLS-ECOSYSTEM.md §5.4`). Bringing in 22 skills is not
one decision (`SPEC-III §1.3`): most are pure prose, and four ship executables and take
the `script` gate with `requires_run_confirmation` (`setup-pre-commit`,
`setup-ts-deep-modules`, `wizard`, `migrate-to-shoehorn`).

His `CONTEXT.md`/ADR conventions ride along with his skills. They are his convention,
not AIKit's; v1 neither adopts nor fights them. §2 is the more general home for the
same instinct, and the two can coexist.

### 4.5 Translate, don't transplant, the Claude plumbing

`git-guardrails-claude-code` installs a `PreToolUse` bash hook via `settings.json`.
AIKit has a first-class hook architecture (`ARCHITECTURE.md §8`) — ship the safety as a
native `hook/gate/dangerous-git` capsule so it joins the visible, ordered chain and the
bypass-token model instead of being an opaque shell guard. Likewise ship `handoff`, not
the Claude-specific `claude-handoff`.

---

## 5. Suite two — the external-facing family

### 5.1 The family, and the failure mode it addresses

One family, several members, one reason to exist: **an agent must not race ahead in
complexity without bringing the user along.** That is a real and common failure — the
code works, the user no longer understands their own system — and naming the family
around it makes engagement a design constraint rather than a vibe.

| Member | From | Shows |
|---|---|---|
| display tools | wigglystuff (~54 widgets) | a concept, interactively — learn while you code |
| `prototype` | Pocock | a throwaway that answers a design question |
| `teach` | Pocock | a lesson, across sessions |
| architecture report | Pocock's `improve-codebase-architecture` | a self-contained HTML report |

wigglystuff is the learn-while-coding member: Bret-Victor-style direct manipulation
where a drag on a concrete surface (a matrix cell, a coefficient in a formula, a curve
knot) updates the abstract consequence live. The suite comes in **wholesale** — the
full palette available, since which widget fits a moment is not knowable up front.

### 5.2 How AIKit runs them — natively, no sidecar

AIKit is a meta-layer for AI activity across languages. A capsule runs a Python
payload the same way `bkmr`'s capsules run shell: by declaring its interpreter. The
widgets are capsules with Python payloads and declared effects. No companion process —
a sidecar would only make sense if organising wigglystuff *were* the product, and
running arbitrary-language tooling is exactly what AIKit is for.

### 5.3 Invocation: "invoke when correct"

The contract is deliberately dumb. The agent has the *option* to show; it takes it when
apt; the user can ask for one; the agent can offer. Engagement stays user-mediated,
structurally encouraged by the affordance existing. No elaborate return protocol is
specified, and none is needed.

The one thing that does need declaring is **how the output reaches the user**, because
that genuinely varies:

| Context | Surface |
|---|---|
| marimo / Jupyter session | renders in the notebook (wigglystuff's native, reactive home) |
| terminal / chat | opens a browser tab, or writes and opens a self-contained HTML artifact (what `improve-codebase-architecture` already does) |
| headless / CI | writes the artifact and reports the path — honestly, rather than pretending it showed anything (`STANDARDS.md §1`, no silent degradation) |

---

## 6. The edges — a flow across composables, not a new language

No flow-language is needed. The edges exist; they need to be walkable and, optionally,
visible.

- `[[requires]]` — the dependency graph (§4.3).
- `conflicts` / export collisions — fail visibly (rule 5).
- **`related` — the one addition.** `PRIOR-ART-ACTIONS.md L5` already flags
  `related_skills` as first-class capsule metadata, *"surfaced in the tree and the
  palette ('often used with…'), richer than a flat tag"*, status **III**. Make it a
  real edge and the "what composables suit this toolchain" query has a graph to walk.
- profiles as lenses — typed params and forking (`SPEC-II §5`), so a project declares
  its toolchain (`test_runner = "cargo-nextest"`) and keeps its deviation legibly.
- intent + frecency — `aikit run --intent` returns tiered candidates and *never*
  executes (`SPEC-II §6`); `aikit z` ranks by match quality with usage as an ordered
  tiebreak (`SPEC-III §3.1`).

The UX you described falls straight out of these: new project → its profile declares
the toolchain → resolver plus `requires`/`related` plus frecency surface the apt
composables → `aikit z <words>` proposes them, never activates (rule 6). Cross-project
reuse is the personal registry plus a profile fork carrying your deviations.

The dependency graph is just the visible name for `requires + conflicts + related`.
Rendering it is one *optional* external-facing show, not core plumbing.

---

## 7. Representing it — facets only

```toml
[metadata.aikit]
facing   = "internal"        # internal | external | both   (default: internal)
surface  = "browser"         # external only: notebook | browser | artifact-path
language = "vocabulary"      # guidance only: vocabulary | module-interface | approach
```

Unknown frontmatter and `metadata` keys are preserved and re-emitted per
`SKILLS-ECOSYSTEM.md §4.1` and `PRIOR-ART-ACTIONS.md #30`, and `metadata.aikit.*`
namespacing is what the spec asks for. Facets drive guidance, surfacing, search and
presentation. They never drive activation.

One `guidance` capsule bridges the suites: it teaches the agent that external-facing
tools exist, when showing beats telling, and how they surface. Prose, not plumbing.

---

## 8. What earlier revisions got wrong (recorded, so the reversal is deliberate)

- **No new capsule kind.** `Guidance` and `Template` already cover meaning and modules.
- **No Python sidecar or companion process** (§5.2).
- **No vertical slice.** Both suites come wholesale; only the invocation pattern needs
  proving, and it is one sentence (§5.3).
- **No rendering of AIKit's own internals** as a headline feature, and **no hooks
  involved in showing.** Showing happens when an agent invokes a display skill.
  External-facing tools show the *user's* concepts and code.
- **No elaborate invocation contract** (§5.3).
- **"Snippet" demoted to the cheap tier.** The durable tier is a composable module
  (§1.1).

---

## 9. Open questions

1. **`language` facet values.** Are `vocabulary | module-interface | approach` the
   right three, or is `module-interface` really a distinct facet since it is
   per-module and project-scoped rather than a portable fragment?
2. **Promote ergonomics for language.** `SPEC-II` promotes a *candidate* into a
   capsule. What raises a language candidate — an agent noticing a term used
   consistently, an `InboxKind::AgentProposal`, or purely a human act?
3. **`related` edge semantics** (§6). Directed or symmetric? Weighted by co-usage from
   the store's existing usage records, or hand-declared only?
4. **Template instantiation — the real gap.** Measured: `TemplateSection` is exactly
   `{ root: String, destination: Option<String> }` (`capsule.rs:493`); the kind parses,
   catalogues, is classified runnable, and `store::inbox` can scaffold one — but
   **there is no instantiate implementation anywhere in the workspace**, and no
   parameters. So §3 is the one part of this document that needs real code rather than
   content. Two sub-questions: does a composable module need parameters (a project
   name, a port, a service name)? And if so, do they reuse `SPEC-II §5`'s typed profile
   params rather than inventing a second parameter system?

---

## 10. Delivery — three plans, not one

This document is one architecture but three separable bodies of work, and they have
very different shapes. Recording that so nobody tries to write a single plan for it:

| Piece | Nature | Depends on |
|---|---|---|
| **A. Pocock bundle** (§4) | almost entirely **content** — capsules, manifests, `[[requires]]` edges, one translated hook. Uses the import/registry/trust machinery as-is. | nothing new |
| **B. Composables** (§1–§3, §6–§7) | **core code**: the `facing`/`language` facets, `related` as a real edge, and `template` instantiation (§9.4, the genuine gap). | nothing new; touches `aikit-core` |
| **C. wigglystuff bundle** (§5) | **content plus one mechanic** — capsules with Python payloads, and the surfacing table in §5.3. | B's `facing`/`surface` facets |

A and B are independent and can proceed in parallel. C wants B's facets first. The
`facing` facet is the smallest useful increment in B and unblocks the most.

---

## Sources

- AIKit: `STANDARDS.md`, `docs/ARCHITECTURE.md`,
  `docs/SPEC-II-PROCEDURES-AND-INBOX.md`,
  `docs/SPEC-III-SKILLSETS-AND-FRECENCY.md`, `docs/SKILLS-ECOSYSTEM.md`,
  `docs/PRIOR-ART-ACTIONS.md`, `docs/integrations/bkmr.md`,
  `crates/aikit-core/src/{capsule.rs,guidance.rs}`, `contrib/bkmr/`.
- <https://github.com/mattpocock/skills> — 41 skills (22 promoted); `codebase-design`
  SKILL.md + DEEPENING.md read directly. Surveyed 2026-07-24.
- <https://github.com/koaning/wigglystuff> — ~54 anywidget primitives, surveyed
  2026-07-24.
