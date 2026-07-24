# AIKit Spec II — Procedures, the Inbox, and operating on the world

Part I (`ARCHITECTURE.md`) specifies how AIKit resolves a **context** into an
**effective view** and materializes it as an immutable **generation**. That is
complete and correct for everything AIKit owns.

It leaves out the harder half: **how AIKit changes the world it does not own.**

Importing 644 skills across thirteen trees, reconciling two live major versions
of the same skill pack, rewriting a client's settings file, deleting a wrapper
script, upgrading a dependency and re-embedding its database — none of these are
generations. They are mutations of a world that existed before AIKit and will
outlive it. Part I gives them no discipline, and consequently `doctor --fix`,
`promote`, `client install`, `mux install` and `collate` would each invent their
own safety story. That is the gap this part closes.

---

## 1. The Procedure

> **A Procedure is a named, planned, reviewable, reversible mutation of the world
> outside AIKit's own state directory.**

Everything that writes outside `~/.aikit/state/` is a Procedure. No exceptions.

A Procedure has exactly the same discipline a generation has, applied outward:

| Generation | Procedure |
|---|---|
| resolve → plan | survey → plan |
| content-addressed hash | content-addressed plan digest |
| materialize into a temp dir | stage into an isolation strategy |
| validate before promoting | validate before committing |
| atomic `current` swap | atomic commit (git, or recorded inverse) |
| `previous` retained | undo record retained |
| failed build leaves `current` intact | failed procedure leaves the world intact |

### 1.1 Shape

```rust
/// A planned mutation of the world outside AIKit's own state.
pub struct Procedure {
    pub id: ProcedureId,              // prc_...
    pub kind: ProcedureKind,
    pub plan: Plan,                   // computed BEFORE anything is written
    pub isolation: MutationIsolation,
    pub digest: PlanDigest,           // content hash; re-running a satisfied plan is a no-op
}

pub enum ProcedureKind {
    Import { source: ForeignRoot },
    Collate { sources: Vec<RegistrySource> },
    Adopt { capsules: Vec<CapsuleId> },
    Promote { candidate: CandidateId },
    Supersede { winner: CapsuleId, losers: Vec<CapsuleId> },
    ClientInstall { client: TargetId },
    MuxInstall { mux: MuxKind },
    DoctorFix { checks: Vec<CheckId> },
    IntegrationSetup { integration: String },   // e.g. bkmr
    DependencyInstall { tool: CapsuleId },
    Custom { capsule: CapsuleId },              // a `procedure` capsule
}

/// One reversible edit. The inverse is computed at plan time, not at failure time.
pub enum WorldEdit {
    WriteFile   { path: PathBuf, contents: Vec<u8>, inverse: Inverse },
    DeleteFile  { path: PathBuf, inverse: Inverse },
    CreateLink  { path: PathBuf, target: PathBuf, inverse: Inverse },
    MarkedBlock { path: PathBuf, marker: String, contents: String },  // idempotent by construction
    RunCommand  { argv: Vec<String>, cwd: PathBuf, undo: Option<Vec<String>> },
}

pub enum Inverse {
    Restore { blob: BlobId },   // backed up into state/procedures/<id>/undo/
    Remove,
    Recreate { target: PathBuf },
    None,                        // only legal when the edit is provably idempotent
}
```

### 1.2 Mutation isolation — distinct from task isolation

Two different axes share the word "isolation" and must not be confused:

* **Task isolation** (`context::Isolation`) — where an *agent task* works.
  Defaults to `Shared`. This is the user's binding correction to Part I: a
  worktree is opt-in.
* **Mutation isolation** (`MutationIsolation`) — how a *Procedure* stages its
  writes. Defaults to the **most isolated option the target supports**.

They point in opposite directions on purpose. An agent doing a review does not
need its own checkout. A procedure restructuring your skill trees does.

```rust
pub enum MutationIsolation {
    /// Target is a git repository: stage on a branch. The default when available.
    GitBranch { repo: PathBuf, branch: String },
    /// Target is a git repository and the change is large or long-running.
    GitWorktree { repo: PathBuf, branch: String, path: PathBuf },
    /// Target is not under version control: build a shadow tree, diff, then swap.
    Staged { shadow: PathBuf },
    /// Small, provably-reversible, explicitly confirmed.
    Direct,
}
```

Selection rule, in order:

1. If every path the plan touches is inside one git repository → `GitBranch`.
   (`GitWorktree` when the plan is expected to outlive one invocation, or when
   the user asks.)
2. Else → `Staged`: build the whole result in a shadow tree, present a real
   diff, and only then swap file by file with recorded inverses.
3. `Direct` requires an explicit confirmation and is refused entirely for any
   plan containing an edit whose `Inverse` is `None` and which is not a
   `MarkedBlock`.

**Consequence worth stating plainly:** AIKit restructuring `~/.claude/skills`
happens on a branch of a repository, or in a shadow tree with a reviewable diff
and a working undo. It never edits thirteen trees in place and hopes.

### 1.3 Marked blocks

Any edit to a file AIKit does not own is a **marked block**:

```
# >>> aikit >>> managed block, do not edit by hand
...
# <<< aikit <<<
```

Idempotent by construction: applying twice replaces, never appends. Human prose
outside the markers is never touched. This already governs the tmux config; it
now governs `AGENTS.md`, `CLAUDE.md`, client settings and shell rc files too.

### 1.4 Command surface

```
aikit procedure plan <kind> [args]     # survey + plan, write nothing
aikit procedure diff <prc>             # the full reviewable diff
aikit procedure run <prc>              # stage, validate, commit
aikit procedure undo <prc>             # apply the recorded inverses
aikit procedure list [--open]
```

`doctor --fix`, `collate`, `import`, `promote` and `client install` are all thin
front-ends over this. There is one safety story, not six.

---

## 2. The Inbox is the system's channel, not a promotion queue

Part I treats the inbox as a staging area for capture candidates. That is too
small. The inbox is **the system's own communication platform** — the place
where AIKit, and agents operating through it, address the user.

This is what brings AIKit to parity as an agent in the ordinary sense:
*activity that operates according to its own law to a circumstantially
non-trivial degree*. A system that can observe, plan and act but has no channel
of its own is a tool. One that can also **say what it found and what it proposes,
durably and addressably**, is an agent.

```rust
pub struct InboxItem {
    pub id: InboxId,                 // inb_...
    pub kind: InboxKind,
    pub project: Option<ProjectId>,
    pub created: Timestamp,
    pub state: InboxState,
    pub title: String,
    pub body: String,                // markdown, redacted
    pub evidence: Vec<Evidence>,     // file refs, diffs, hashes — never raw transcripts
    pub proposal: Option<ProcedureId>,   // a planned Procedure the user can just run
}

pub enum InboxKind {
    CaptureCandidate,   // "you ran this three times; want it as a script?"
    VersionConflict,    // "superpowers is v4.2.0 in Codex and v6.1.1 in Claude"
    TrustReview,        // "this revision changed; it needs a look before it activates"
    DriftNotice,        // ".aikit, .claude and AGENTS.md disagree"
    ProcedureReport,    // "collate ran; here is what changed"
    AgentProposal,      // an agent publishing a suggestion to the user
    Breakage,           // "this symlink is dead; this bridge calls a missing subcommand"
}

pub enum InboxState { Open, Deferred { until: Timestamp }, Resolved { decision: String } }
```

Three properties make it a channel rather than a list:

1. **An item may carry a planned Procedure.** Resolution is often one keystroke:
   the system did the work of figuring out what to do and staged it for review.
2. **It is readable by agents through the broker.** `aikit inbox list --json
   --project current` is in the brokered capability index, so a session can open
   with *"there are three open items here"* as real context. This is the "inbox
   becomes core context for agent update self-publishing" requirement.
3. **Agents can write to it.** An agent that notices something out of scope
   publishes an `AgentProposal` rather than either acting unilaterally or losing
   the observation when the session ends.

Redaction is unconditional: the secret scanner runs on every item before it is
stored, and quarantined content is never rendered in a preview or written to git.

---

## 3. Foreign roots and adoption

A registry AIKit indexes is not necessarily a registry AIKit owns.

```rust
pub enum RegistryOwnership {
    /// AIKit's own registries. Writable.
    Owned,
    /// Indexed, read-only. ~/.claude/skills, ~/.hermes/skills, a plugin cache.
    Foreign,
    /// Was foreign; the user ran an Adopt procedure. AIKit now owns it and the
    /// original location becomes a projection.
    Adopted { adopted_at: Timestamp, procedure: ProcedureId },
}
```

Import is read-only and always safe. **Adoption is a Procedure** — it moves
authority, and afterwards the original path is regenerated from the capsule
rather than edited by hand. That inversion is the point: today
`~/.claude/skills` is nineteen hand-made symlinks into `~/.agents/skills`; after
adoption it is a projection, and a broken link becomes a `doctor` finding rather
than a silent failure.

A foreign root's capsules are `TrustState::Unseen` on import. Being on your disk
is not being reviewed.

---

## 4. Fidelity: lifting and lowering across schemas

The third seam — *lean toward the higher, accommodate the lower* — is a lifting
problem, and it needs one rule in each direction.

**Lifting (import).** Every capsule field records its provenance:

```rust
pub enum FieldOrigin { Declared, Inferred { from: String }, Defaulted, Absent }
```

Hermes declares `version`, `platforms`, `tags`, `related_skills`. Claude declares
`name` and `description`. Lifting a Claude skill therefore produces a capsule
with most fields `Absent` — **visibly absent, not silently defaulted**. The
palette shows it, `doctor` counts it, and promotion is where a human fills it in.
The envelope is set by the richest source; the poorest source is accommodated by
honest emptiness.

**Lowering (projection).** Projecting a rich capsule onto a poor target drops
fields. That loss is recorded, never hidden:

```rust
pub struct Fidelity {
    pub target: TargetId,
    pub dropped: Vec<(&'static str, String)>,   // field, why
    pub degraded: Vec<(&'static str, String)>,
}
```

`ProjectionPlan` carries a `Fidelity`. `aikit explain` prints it. A user asking
"why does this behave differently in Codex" gets an answer instead of a guess.

---

## 5. Profiles as project lenses

Part I's profiles are global recipes referenced by scope. The requirement is
richer: *profiles become project-specific lenses; the base gives a
user-customised platform for cross-project forking.*

Two additions, both preserving determinism:

**Parameters.** A profile may declare typed parameters and a project may bind
them. Bindings are explicit values in a committed file — never inferred, never
queried.

```toml
# profile/code/rust
[params]
test_runner = { type = "enum", choices = ["cargo-test", "cargo-nextest"], default = "cargo-nextest" }
strictness  = { type = "enum", choices = ["fast", "full"], default = "fast" }
```

```toml
# <repo>/.aikit/profile.toml
[[use]]
profile = "profile/code/rust"
params  = { strictness = "full" }
```

**Forking.** `aikit profile fork profile/code/rust --scope project` writes a
project-local profile that `extends` the base and holds only the delta. The base
keeps evolving; the fork keeps its deviation, and `aikit profile diff` shows
exactly what this project changed and why. That is the lens: one platform, many
angles on it, each one legible.

The resolver is untouched — a bound parameter resolves to explicit capsule ids
before layering, so rules 1–7 hold unchanged.

---

## 6. Intent invocation

Direct invocation needs to work from intent, not just from a remembered name —
at CLI level, headless, for agents as much as for humans.

```
aikit run --intent "run the tests the way CI does"
```

Returns **candidates, never an execution**. Three tiers, always labelled:

| Tier | Source | Latency | Determinism |
|---|---|---|---|
| exact | export name / capsule id | instant | total |
| field | nucleo over name, id, tags, description | < 16 ms | total |
| semantic | the configured retrieval capability | ~ms local, ~1 s remote | none |

Rules that keep this honest:

* Semantic results are a **separate, labelled section**, never blended into one
  ranked list — otherwise you cannot tell why something surfaced.
* Semantic search **never runs per keystroke**. It is a deliberate action.
* Intent search may never activate anything (rule 6 stands); it only proposes
  what to *run*, which is a different act from what is *active*.
* `--json` returns the candidates with their tier, so an agent can decide rather
  than guess.

---

## 7. Supersession: automate the provable, queue the ambiguous

Version differentials get resolved automatically **only when the answer is
provable**. Otherwise they go to the inbox as a `VersionConflict`.

Automatic supersession requires one of:

1. **Containment.** The winner's normalized content is a strict superset of the
   loser's — every section present and unmodified, plus additions. Provable by
   diff, not by judgement.
2. **Lineage plus ordering.** Both derive from the same upstream (same
   `provenance.upstream`, or a shared git history), and one has a strictly
   greater semantic version.
3. **Identity.** Byte-identical after normalization — a dedup, not a decision.

Anything else — divergent edits, different flag contracts, unrelated lineage —
is ambiguity, and ambiguity is a human's. The four NotebookLM variants with three
incompatible flag contracts are ambiguity. `test-driven-development` in six
copies with four contents is mostly ambiguity. The nineteen byte-identical
symlink aliases are not.

Every automatic supersession is still a Procedure: staged on a branch, reported
to the inbox, undoable. Git history discipline is what makes it safe to be
decisive.

---

## 8. Self-hosting

**AIKit's own capabilities are capsules in AIKit.** Not a special case, not
hardcoded — the same envelope, trust, projection and explanation as everything
else. Concretely, the shipped registry includes:

* `skill/aikit/using-aikit` — the verbs and how they compose.
* `skill/aikit/authoring-capsules` — how to write one, with the manifest rules.
* `skill/aikit/procedures` — how to plan, review and undo a world mutation.
* `guidance/aikit/composition` — injected at `SessionStart` under a small token
  budget: what AIKit is, the verb grammar, and how to discover the rest. This is
  the thing that makes the system legible to *any* LLM instance regardless of
  context, and it must stay small enough that it always fits.

The test is exact: **AIKit must be able to install, explain and modify itself
using only its own public surface.** If a maintenance action needs a private
back door, the surface is wrong.

---

## 9. Dependencies without becoming a package manager

Part I says "no package manager", and that stands — AIKit must not own arbitrary
system packages. But it also already said the resolution: *installation is an
explicit script capsule, not implicit behaviour during resolution.* That seam is
the whole answer, and it should be used, not avoided:

* `tool/<group>/<name>` declares the check and the minimum version. Resolution
  only ever *checks*.
* `script/install/<name>` performs the install. It is reviewable, versioned,
  effect-declaring, and runs only when a human or a Procedure invokes it.
* `doctor` detects "missing" or "below minimum" and **offers** the install
  procedure, diff-first.
* Installation is never a side effect of `apply`, `enable`, or a search.

So AIKit installs its dependencies, deliberately, through the same machinery
that governs everything else. That is not a package manager; it is a capability
that happens to run `cargo install`.

---

## 10. Repo patterns: the code is the map

The codebase is an agent-facing API before it is anything else. Four rules:

1. **`lib.rs` is a map, not a manifest.** Module list, then a curated re-export
   surface, then prose saying what the crate is responsible for and what it
   refuses to do.
2. **Every module header states the invariant it owns and why**, in prose, and
   names its neighbours. Not what the code does — why this seam exists. The
   existing `resolve/mod.rs`, `context.rs` and `trust.rs` headers are the
   standard.
3. **One public surface per crate.** Cross-crate consumers use the crate root's
   re-exports; internals stay `pub(crate)` wherever they can. A reader should be
   able to learn what a crate offers from one screen.
4. **Errors are the documentation of failure.** Stable machine codes, structured
   details naming the file and line responsible. `resolution.required_capability_disabled`
   with `capability`, `required_by`, `scope`, `origin` teaches more about the
   system than a paragraph would.

Plus `docs/MAP.md`: "if you are looking for X, it is in Y" — the shortest path
from a question to a file.

---

## 11. What this makes the system

Part I made a resolver. Part II closes it:

* it can **describe** its world (catalog, view, explanation),
* it can **change** its world under law (Procedures: planned, isolated,
  reviewable, reversible),
* it can **speak** about its world (the Inbox),
* and it can **do all three to itself** (self-hosting).

That closure is what makes "the whole computer as a field of intelligence-qua-code"
a working proposition rather than a slogan: the machine becomes legible because
AIKit gives it one description, and safely modifiable because AIKit gives it one
mutation discipline. Anything AIKit can read but cannot explain and cannot safely
change is a leak in the model, and should be treated as a defect.

**The first proof is this machine.** Thirteen skill trees, 138 duplicate names,
two live versions of one skill pack, eleven orphaned hooks, five dead symlinks, a
903-line wrapper in triplicate, and three tools disagreeing about which project
is active. If AIKit cannot survey that, plan a resolution, stage it reviewably,
carry the ambiguity to a human and record what it did — it is not finished.
