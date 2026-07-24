# AIKit Spec III — Skill-sets, the tree, and frecency

Part I resolves a context into an effective view. Part II gives AIKit a
discipline for changing the world outside itself. Both are about *what is true*.

This part is about *how a person and a harness reach it* — and it turns out those
are the same problem, solved by two ideas that fit together: the **skill-set**
(the unit you point a harness at) and **frecency** (the reason you never have to
type a full id).

---

## 1. The skill-set

> A **skill-set** is a folder of capabilities. It has **no required manifest, no
> dependency semantics, and no trust of its own.**

The one-line placement:

```
profile : resolution  ::  skill-set : projection
```

A **profile** answers *what should be active here* — it is a patch, it can
disable, it carries config, it composes by precedence, it feeds the resolver.

A **skill-set** answers *what do I hand to this harness* — it is a set, it can
only add, it carries nothing, it composes by union, and it feeds projection.

They are different jobs and conflating them is how you end up with a plugin
system that is too heavy to make one of casually and too opaque to point at
precisely.

Named for the common case; a set may hold any capability kind, because in
practice a "rust review" set wants the skill, the script that runs the tests, and
the hook that gates the commit, and splitting those across three concepts to
satisfy a taxonomy would be a false economy.

### 1.1 A skill-set is a folder

> **A skill-set is a directory. Its members are what is in it. Nesting gives
> you sub-sets. There is no other rule.**

An earlier draft of this section invented a `members = [...]` manifest file.
That was over-thinking it. The shape already exists on this machine and works:

```
~/.hermes/skills/nara/
├── parā/
├── paśyantī/
├── madhyamā/
├── vaikharī/
└── quilt/
```

`nara/` is a set. `nara/paśyantī/` is a set. Point a harness at the first and it
gets everything; point it at the second and it gets the subtree. Nobody had to
learn a schema to build that, and nobody should have to.

**A manifest is optional and exists only when the folder cannot say something.**

```toml
# nara/set.toml — every field optional
description = "Nara voice registers"
include     = ["skill/rust/review"]   # members that live in another registry
order       = ["parā", "paśyantī"]    # presentation order, if it matters
```

No manifest, no problem. `mkdir` is a legitimate way to create a skill-set. If
writing one ever feels like an architectural act, this section has failed.

### 1.2 What AIKit adds: the folder becomes dynamic

`~/.hermes/skills/nara/` is a static tree of hand-maintained symlinks. So is
`Nara-Personal/.claude/skills`, which is a flattened projection of it, made by
hand, that will drift the first time either side changes.

That is the whole problem, and it is one word: **static**. AIKit keeps the same
familiar shape and makes it resolve:

| | today | with AIKit |
|---|---|---|
| membership | whatever was `ln -s`'d | resolved from the catalog, per context |
| per project | copy the tree | point at the set |
| per session | impossible | a session overlay |
| what dropped out and why | silence | reported |
| drift | discovered by breakage | a `doctor` finding |

Three provenances, one concept:

* **Observed** — a real directory that already exists (`~/.hermes/skills/nara/`).
  AIKit indexes it and can point harnesses at it. Read-only until adopted.
* **Composed** — a virtual directory AIKit builds from capsules across
  registries and materializes into a generation. The dynamic case.
* **Project** — `<repo>/.aikit/sets/<name>/`, committed, shared with the team.

They behave identically at the point of use. The distinction only shows up in
who may write to them.

### 1.3 Withholding the manifest is a safety property

The lightness is not only ergonomic. A set has **no trust of its own**, and it
must not be able to acquire any, because otherwise aggregation becomes a trust
laundering path: bundle a reviewed skill with an unreviewed hook, point a harness
at the bundle, and the bundle's reputation carries the hook in.

So the rule is absolute:

> **Projecting a set projects only those members that pass their own gates.**
> Trust, policy, platform and target checks are per-capsule and unchanged. The
> set reports what it dropped and why; it never vouches for a member.

A set with six members may project four. The projection notes say which two were
withheld and on what grounds, and the palette shows it. A set is a *request*, not
an authority.

### 1.4 Composition is union, and only union

Sets compose by union. There is no `exclude`, no precedence, no override.

If you want to subtract, you want a profile — that is what profiles are for, and
they already do it with full explainability. Giving sets subtraction would
recreate the resolver inside the projection layer with none of its guarantees.

```toml
# <repo>/.aikit/project.toml
[skillsets]
use = ["rust-review", "payments-domain", "@nara"]
```

`@` marks an observed set, so the origin of membership is visible at the point of
use rather than requiring a lookup.

### 1.5 Globs expand at authoring time, never at resolution time

A set may be authored with a glob:

```
aikit set create rust-review --match 'skill/rust/*' 'script/test/cargo-*'
```

The glob **expands immediately** to explicit ids, which are what the file
contains. The pattern is retained only as provenance.

This is not a detail — it is Part I rule 6 surviving contact with a convenience
feature. If sets matched dynamically, syncing a registry would silently change
what a harness sees, which is precisely the failure the rule exists to prevent.
Instead, a newly catalogued capsule matching a retained pattern raises an inbox
item:

```
SetCandidate — skill/rust/unsafe-audit matches the pattern that built
               `rust-review` (skill/rust/*). Add it?  [add / never / defer]
```

New matches are *proposed*, never joined.

---

## 2. Pointing a harness

This is the job the concept exists for, and each harness takes a different
shape of pointer. A set materializes as exactly one projection root per
`(context, target, set)`:

```
<generation>/projections/<target>/sets/<set-name>/
```

| Harness | Mechanism | Effect |
|---|---|---|
| Claude Code | `--add-dir <set root>` containing `.claude/skills/` | `LiveReloadExpected` |
| Codex | `.agents/skills` in an **isolated** tree; otherwise fall back honestly (Part I §3) | `LiveReloadExpected` \| `Brokered` |
| Hermes | `config.yaml → skills.external_dirs` — a **native** external-directory hook, currently `[]` on this machine | `RestartClient` |
| Broker | index grouped by set, so an agent sees the sets and can read into them | `Brokered` |
| Shell | `bin/` from the union of all active sets | `Immediate` |

Hermes is worth calling out: `external_dirs` is exactly the "point me at a
directory" seam skill-sets need, and it is unused. Of the three harnesses it has
the cleanest native integration point, and it is the one that needs the least
work.

The context exposes what is active so a harness or a script can ask without
parsing anything:

```
AIKIT_SKILLSETS=rust-review,payments-domain,@nara
AIKIT_SKILLSET_ROOT=$AIKIT_VIEW/projections/claude/sets
```

And the sets in force are recorded in `resolution.lock.toml`, so "which sets was
this session running" is answerable after the fact.

### 2.1 The problem this actually solves

Today every Claude session on this machine sees all 45 entries in
`~/.claude/skills`, whether the project is a Rust service or an essay. There is
no way to say *this project uses these six*. Skill-sets plus per-context
projection is that sentence, and it is the single largest reduction in
irrelevant context available here.

---

## 3. Frecency: you should never type an id

zoxide's insight is that you do not need aliases if the tool learns which
partial match you meant. The substring you happen to type **is** the alias, and
it costs nothing to create because you did not create it.

That transfers to AIKit almost unchanged, because **capsule ids are paths**:

```
zoxide:  z docs        → ~/Documents/…            (frecency over paths)
aikit:   aikit z nextest → script/test/cargo-nextest  (frecency over ids)
         aikit z rust    → skillset rust-review
```

### 3.1 Score and tiebreak are separate — a correction

An earlier draft of this section had `score()` combine match quality *and* usage
statistics into one number. **That is wrong, and fzf and nucleo are both right
not to do it.**

A single blended number is unstable — the same query returns a different order
tomorrow because a counter moved — and it is unexplainable, because "why did this
rank first" has no answer you can show a user. It also quietly violates Part I's
explainability requirement.

So:

> **`score` is match quality alone. Usage lives in an ordered tiebreak.**

```rust
/// Match quality only: deterministic, reproducible, explainable.
pub fn score(query: &Query, doc: &SearchDoc, text: TextScore) -> Score;

/// Applied in order, only between candidates of equal `Score`.
pub enum Tiebreak {
    ExactExportName,     // you typed the command's actual name
    CurrentProject,      // a match here beats a match elsewhere
    ActiveInContext,     // already active beats merely catalogued
    Frecency,            // successful uses, recency-decayed
    CapsuleId,           // total order, so results never jitter
}
```

The final `CapsuleId` tiebreak matters more than it looks: without a total order
at the bottom, equally-scored equally-used candidates swap places between
keystrokes, which reads as a broken UI.

Three further rules:

1. **Match on the tail first.** `nextest` should beat a capsule whose *group* is
   `nextest`, the same way `z docs` prefers a directory named `docs` over one
   merely containing it. Segment-aware, not flat substring — and this belongs in
   `score`, because it is match quality.
2. **Scope beats globality.** A match in the current project outranks a
   more-frecent match from elsewhere. This is a tiebreak ordered *above*
   frecency, not a term added to it.
3. **Success, not invocation.** The frecency counter increments on *successful*
   completion. A script you run and abort five times a day should not become
   your top match.

Follow fzf in expressing scoring constants as *relationships* rather than magic
numbers (`bonus_consecutive = -(gap_start + gap_extension)`), so the tuning stays
legible when someone changes one of them.

### 3.2 The single verb

The requirement is as few input surfaces as possible, so:

```
aikit z <words…>
```

* **Unambiguous** → act. The action is the capability's natural one: run a
  script, open a skill, activate a set, attach a session.
* **Ambiguous** → the palette opens **pre-filtered** to the candidates. This is
  zoxide's `zi`, and it means ambiguity is never an error message — it is the
  interactive case, one keystroke from resolved.
* **Nothing** → intent search (Part II §6), which returns candidates and never
  executes.

`z` stays explicit rather than making bare `aikit <word>` a catch-all: a typo'd
subcommand must not silently run something.

### 3.3 The same verb for agents

```
aikit z --json --dry-run "run the tests like CI"
```

Returns ranked candidates with their tier and score and executes nothing. An
agent gets the same affordance a human does, which is the property that keeps
the system explicable to any LLM instance: **there is one way to find things,
and it works identically headless.**

### 3.4 What frecency may never do

It ranks. It does not activate. A frecent capsule that is not selected by any
scope is still inactive, and `z` on it proposes running it, not enabling it.
Rule 6 again: nothing becomes active because of a score.

---

## 4. The tree: a virtual filesystem you already know how to use

Sets are folders. So the way to manage them is a **file browser** — and because
everything else in AIKit is also addressable by path, the same browser organises
scripts, hooks, contexts and the inbox.

This does not contradict "a palette, not a dashboard". They are two modes with
two jobs:

| | Palette | Tree |
|---|---|---|
| for | **invoking and toggling** | **organising** |
| lifetime | opens, acts, disappears | you enter it deliberately, you leave when done |
| default | yes | no — `aikit ui --tree`, or `Ctrl-T` from the palette |
| shape | one line of input, a list, a preview | a navigable hierarchy |

Neither is a permanent control centre. The tree is where you *arrange* things so
that the palette can stay one line long.

### 4.1 The roots

```
▾ sets/                       the folders from §1
  ▾ nara/                       23 members · projected to claude, hermes
    ▸ paśyantī/                  6 members
    ▸ quilt/                     4 members
  ▸ rust-review/                 3 members · project
▾ kinds/                      everything, by what it is
  ▸ skill/     412
  ▸ script/     86
  ▸ hook/       12
  ▸ guidance/   31
▾ hooks/                      by event, in dispatch order — the chain, visible
  ▾ PreToolUse/
      1. gate/project-boundary        closed · serial
      2. gate/secret-exfiltration     closed · serial
      3. verify/cargo-check           warn   · parallel
▾ contexts/                   this session, its tasks, other sessions
    ▸ ses_01J… payments             3 tasks · gen_b71f2f
▾ registries/                 where things came from
  ▸ personal          owned      312
  ▸ @claude-global    foreign     43   ⚠ 19 symlinks, 3 dead
  ▸ @hermes           foreign     95
▸ inbox/                      4 open
```

Two properties make this work rather than becoming another tree widget:

1. **It is a view, not an ownership hierarchy.** One capsule appears under
   `kinds/`, under every set containing it, and under `registries/`. Tags-as-
   folders, not a filesystem you can corrupt by moving something.
2. **`hooks/` shows the resolved chain in execution order.** That single screen
   answers the question nobody can currently answer on this machine — *what
   actually runs, in what order, when Claude edits a file* — and it is the
   direct fix for eleven hook scripts sitting on disk wired to nothing.

### 4.2 Operations are filesystem verbs

Because the model is folders, the verbs are ones people already have:

| Key | Action | Why this key |
|---|---|---|
| `a` | new set (`mkdir`) | |
| `y` / `p` | yank / put — add a capsule to a set | copy, not move: sets are views |
| `d` | remove from this set | never deletes the capsule |
| `D` | delete the set itself | confirmed |
| `r` | rename | |
| `Space` | stage an activation toggle | same as the palette |
| `Enter` | expand, or act on a leaf | |
| `Ctrl-Enter` | apply staged changes | same as the palette |
| `/` | filter the tree | |
| `?` | contextual help | |

Movement is vim (`j k h l`, `gg`, `G`, `Ctrl-d`, `Ctrl-u`, `zz`) **and** arrows
(`↑ ↓ ← →`, `Home`, `End`, `PgUp`, `PgDn`), always both. `h`/`←` collapses,
`l`/`→` expands, which is the one mapping every tree in every editor agrees on.

### 4.3 Accessibility is a hard requirement, not a nice-to-have

Three rules, testable:

1. **Everything doable with the mouse is doable with the keyboard, and the
   reverse.** Click, click-drag onto a set, double-click to expand, scroll — all
   supported; none required. A snapshot test asserts the same end state is
   reachable both ways.
2. **The selected row is describable in one line**, rendered in the status bar
   and available to `--json`. `sets/nara/paśyantī — 6 members, 4 projected, 2
   withheld (unreviewed)`. This is what a screen reader gets, and it is also
   what an agent gets, which is not a coincidence.
3. **No Nerd Font, no colour, no Unicode is ever load-bearing.** ASCII fallback
   renders the same information (`+`/`-` for expand state, `[x]`/`[ ]` for
   staged). Snapshot-tested in both modes. Colour is redundant emphasis, never
   the only carrier of meaning.

### 4.4 Making it easy to adopt

The tree is also the setup surface, because the honest answer to "how do I start
using this" is *look at what you already have*:

```
aikit ui --tree
  → registries/ shows every foreign root already discovered
  → each one's problems are already counted (dead symlinks, missing frontmatter,
    duplicate names across trees)
  → `a` on sets/ makes your first set from things you can see
  → `Space` stages it, `Ctrl-Enter` applies, harnesses are pointed
```

No config file to write before the first useful thing happens. `aikit init`
should discover, index and show — not interrogate.

---

## 5. How the pieces sit together

```
      profiles          →  what is ACTIVE        (resolver, deterministic, explainable)
      skill-sets        →  what is POINTED AT    (projection, union, per-harness)
      frecency          →  what you MEANT        (ranking, never activation)
      procedures        →  what CHANGES          (planned, isolated, reversible)
      inbox             →  what needs YOU        (the system's channel)
```

Five concepts, each with one job, each refusing the others' jobs. A user needs
to hold two of them to be productive — sets and `z` — and the other three stay
out of the way until something needs deciding.

---

## 6. Command surface

```
aikit set list                             # sets, membership counts, where they project
aikit set create <name> [ids…] [--match glob…] [--observe path]
aikit set add <name> <id…>
aikit set remove <name> <id…>
aikit set show <name>                      # members, and what would be withheld and why
aikit set use <name…> --scope session      # point this context at these sets
aikit set fork <name> --scope project
aikit z <words…>                           # frecency jump / act
aikit z --json --dry-run <words…>          # candidates, for agents and scripts
```

`aikit set show` is the one that earns its place: it lists members **and the
members that would not project here**, with the reason — unreviewed, denied,
wrong platform, no supported target. A set is a request, and this is the reply.

---

## 7. Implementation notes

* `aikit-core::skillset` — `SkillSet`, `SetMembership { Explicit, Observed }`,
  union composition, and `project(set, view) -> (projected, withheld)` where
  `withheld` carries an `UnavailableReason` per dropped member. Pure, no I/O.
* Sets are **not** capsules. They live in `skillsets/`, not `capsules/`, and they
  have no `Kind`, no revision, no trust key. Resisting the urge to unify here is
  the point of the whole section.
* `aikit-core::search` gains an explicit `Frecency` signal with segment-aware
  tail matching and a per-context-first ordering, documented half-life stated in
  the module header.
* `aikit-store` already records usage events; add the success-only counter and
  the per-context index.
* The set roots are ordinary `ProjectionItem`s, so generations, atomicity,
  rollback and garbage collection apply unchanged. Nothing new is needed in the
  generation model — which is the sign the concept sits at the right layer.
