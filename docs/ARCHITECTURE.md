# AIKit architecture

AIKit is a **context-scoped capability router for agentic terminal work**. It is
not a skill registry, a dotfiles manager, a session launcher or a command
palette; those are all views or consumers of the one thing it actually is.

The experience it exists to deliver:

1. Enter a project or attach to a working session.
2. AIKit knows the capability environment appropriate to that space.
3. Tap a key and get a small, fast palette.
4. Search and execute a script, launch a task, inspect a skill, or change what is active.
5. Any change applies to the current session unless deliberately promoted.
6. Claude, Codex, shells, hooks, tmux and cmux see projections of the same resolved state.
7. Closing the palette returns you immediately to the work.

The product is therefore **the resolver and the contextual lifecycle**.

```
 registries + project-local capsules
                  │
                  ▼
             indexed catalog
                  │
                  ▼
 user → host → project → session → task overlays
                  │
                  ▼
       deterministic capability resolver
                  │
                  ▼
 resolved graph + explanation + lock + hash
                  │
       ┌──────────┼──────────────┬───────────────┐
       ▼          ▼              ▼               ▼
 shell/bin     agent skills    hook chains    session topology
 projection    + guidance       + policies     tmux / cmux
```

---

## 1. Vocabulary (exact meanings — the UI must not conflate these)

| Term | Meaning |
|---|---|
| **Capsule** | The packaging unit: a directory with `manifest.toml` and a payload. |
| **Capability** | A capsule that has entered the catalog and is eligible for selection. User-facing term. |
| **Profile** | A reusable declarative patch naming capabilities to enable/disable. *Not* a capsule. |
| **User Baseline Profile** | The persistent, machine-local `global` scope applied to every context before project/session/task layers. `user` is its CLI alias. |
| **Skill Usage Overlay** | Scoped, additive user orientation for one immutable upstream skill: optional routing description plus body guidance with provenance. |
| **Effective Skill** | The upstream `SKILL.md` plus the ordered Skill Usage Overlays that survive scope inheritance. |
| **Pool patch** | The `profiles`/`enable`/`disable`/`[config.*]` declarations attached to one scope. |
| **Effective view** | The resolved graph after layering, dependency expansion, compatibility, policy, conflict and trust checks. |
| **Projection** | A target-specific representation of an effective view. |
| **Generation** | An immutable, content-addressed materialization of an effective view. |
| **Procedure** | An immutable, reviewable, forward-checked and reversible mutation outside a generation. Planned Procedures remain addressable by id for separate diff/run/undo invocations. |
| **Session space** | An AIKit concept bound to a tmux session, a cmux workspace/group, or a plain terminal. |

*Available*, *enabled* and *loaded* are three different things and are rendered
differently everywhere.

---

## 2. The central decision

There is **no global mutable live set** shared by already-running agents. There
is a persistent User Baseline Profile, but it is declarative input to each
context's resolution and may be overridden by more specific scopes. The primary
state is an *effective capability view resolved per context*, where a context is

```
user + host + project scope chain + session space + task + target client
```

Two tmux sessions on the same project can carry different skills. Two cmux
workspaces can carry different hooks. None of them mutate a shared symlink farm
underneath the others.

---

## 3. Deviation from the source specification: worktrees are opt-in

The source specification made a git worktree the implied default for agent tasks
(`aikit task spawn <name> --agent claude` → create worktree). **That is not the
default here.**

`Isolation` (`aikit-core::context::Isolation`) has three values:

* `Shared` — **the default.** The task uses the session's working tree as-is.
* `Directory` — a dedicated directory that is not a git worktree.
* `Worktree` — a git worktree with its own branch. Opt-in via `--worktree`.

Rationale: isolation buys a clean per-task client skill surface and costs a
checkout, a branch, disk, and a teardown decision (dirty tree / unpushed
commits / open PR). That trade belongs to the user, per task. Most tasks — a
focused review, a question, a quick edit — do not want a second checkout.

What AIKit owes the user in the shared case is **honesty rather than pretence**:

* `Isolation::is_isolated()` is the single question adapters ask.
* The Codex adapter must not silently write a per-task `.agents/skills` into a
  shared tree where a sibling task would see it. It falls back, in order:
  1. project-stable native skills only,
  2. brokered session capabilities,
  3. explicitly accepted shared projection (requires confirmation),
  and reports which via `ActivationEffect`.
* A synthetic `HOME` is never invented for a client: that would silently affect
  credentials, git config and ssh config.
* The palette shows the real consequence of a toggle per client. "Active in
  AIKit" must never imply "already loaded by every client".

`isolation` participates in the resolution hash, because it changes which
projections are possible.

---

## 4. Scope precedence

```
managed policy constraints          (not a normal layer; immutable)
  1. user / global profile
  2. host-local profile
  3. project shared profile         (repo root → cwd, `depth` increases)
  4. project-local private profile
  5. session overlay
  6. task / pane overlay
  7. one-shot invocation override
```

Files:

* `~/.aikit/scopes/global/profile.toml` — persistent User Baseline Profile.
* `<repo>/.aikit/profile.toml` — committed.
* `<repo>/.aikit/profile.local.toml` — ignored.
* `~/.aikit/state/sessions/<session-id>/overlay.toml` — session overlay, carries
  `base_generation` for compare-and-swap.

### The seven rules (implemented in `aikit-core::resolve`)

1. Later layers may undo earlier ordinary enable/disable operations.
2. Managed denials cannot be overridden.
3. Dependencies are expanded **after** explicit selection.
4. An explicitly disabled required dependency **fails resolution**; it is never
   silently re-enabled. Error code `resolution.required_capability_disabled`,
   with `capability`, `required_by`, `scope` and `origin` details.
5. Conflicts (and export-name collisions) fail visibly by default.
6. Nothing becomes active merely because it matches a tag. Tags are for search.
7. Every final decision is explainable (`aikit explain`).

### Skill Usage Overlays

`[skill-overlays."<skill-id>"]` is a scoped orientation layer, not a fork of a
skill. Each record may append a routing `description`, body `guidance`, and an
exact `reviewed_against` content revision. Lower-scope overlays accumulate in
precedence order. `inherit = false` discards lower-scope augmentations for that
skill before adding the current scope's text.

The generated Effective Skill labels this section as user-authoritative
orienting augmentation. More-specific contextual direction governs where it
conflicts with more-general orientation, but the overlay cannot change capsule
identity, source revision, trust, permissions, or the upstream skill's invocation
policy. A stale `reviewed_against` pin warns without silently discarding the
user's guidance. An overlay on a non-skill capability is a resolution error.

Overlays participate in the effective-view hash, while the immutable source
revision and trust tuple remain unchanged. Codex and Claude receive the same
rendered Effective Skill, and broker reads return those same instructions.

### Declared vs effective

A layer may declare a capability enabled while it is nevertheless unavailable.
That is **not** an error; it is a different rendering. `UnavailableReason` covers
`NotInCatalog`, `DeniedByPolicy`, `PlatformUnsupported`, `NoSupportedTarget`,
`TrustRequired`, `Quarantined`, `Blocked`, `DependencyUnavailable`.

### Config merge algebra (per `[config.*]` section)

When two scopes both carry `[config."<capsule>"]`, the higher scope has to
combine with the lower one somehow, and there are exactly two right answers
depending on what the section *is*. AIKit makes the choice explicit rather than
picking one silently — this is the single most common "why isn't my config
taking effect" failure across every surveyed tool (`PRIOR-ART.md`; Claude MCP,
mise `[tasks]` and flox all replace whole records where a naive tool deep-merges).

The mode is declared by the capsule the section configures (`config_merge` in the
manifest, `aikit_core::profile::ConfigMerge`), because whether config is a bag of
independent keys or one replaceable record is a fact about the thing configured,
not about who writes the section. `merge_config` (deep) and `combine_config`
(mode-aware) are the only two functions in the algebra, and the resolver applies
the mode in `apply_patch` as it folds layers in precedence order.

| `[config.*]` section shape | Mode | `config_merge` | Rationale |
|---|---|---|---|
| Key/value options (a hook's `timeout`/`mode`, a script's `profile`, the bkmr `db`/`dir`/`also` block) | **deep merge** (default) | omitted, or `"deep"` | A higher scope may change one field without restating the table. |
| A whole replaceable record — an MCP server entry, a command spec, a task definition | **whole-record replacement** | `"replace"` | The higher scope's record *is* the record; lower-scope keys it omits must not bleed through, matching Claude MCP / mise `[tasks]` / flox. |

Deep merge is the default because most capsule config is key/value; a section
that means "replace me as a unit" opts in with `config_merge = "replace"`. Both
modes are folded into the resolution hash through the resulting effective config,
so a section that changes mode changes the generation deliberately, not by
accident.

---

## 5. Storage

```
~/.aikit/
  config.toml
  scopes/global/profile.toml
  registries/<name>/capsules/<kind>/<group>/<name>/{manifest.toml,payload/}
  sources/<name>/{source.toml,state.toml,snapshots/<digest>/}
  projects/<name>.toml
  skillsets/<group>/<name>/members
  profiles/<group>/<name>.toml
  inbox/{ready,quarantine,rejected}/
  state/
    aikit.sqlite3          operational index + events (WAL)
    contexts/<ctx>/{context.toml,current->,previous->,generations/}
    sessions/<ses>/overlay.toml
    locks/
    trust/
  cache/
  logs/events.jsonl
```

`AIKIT_HOME` overrides the root.

**Canonical**: capsule files, profile TOML, project declarations, session specs,
registry git history. **Derived**: SQLite index, search facets, context bindings,
generated projections, usage stats, generation directories.

The SQLite database must be rebuildable from canonical files, except for
genuinely operational records (usage events, live session bindings).

No daemon. Every command works as a fresh short-lived process; coordination is
SQLite transactions plus per-context file locks.

---

## 6. Generations

```
~/.aikit/state/contexts/<ctx>/
  current   -> generations/<hash>
  previous  -> generations/<older-hash>
  generations/<hash>/
    resolution.lock.toml
    bin/ hooks/ guidance/
    projections/{claude,codex,shell}/
    metadata.json
```

Apply is: lock → re-read overlay + catalog revision → resolve → build a temp
generation → materialize → validate → rename to content hash → **atomically
replace `current`** → update the database → notify → retain `previous`.

A failed build never replaces the existing view. Rollback is another atomic
pointer replacement.

`AIKIT_VIEW=$HOME/.aikit/state/contexts/<ctx>/current` is stable across
generation swaps.

Managed skill sources and project routing are specified in ADR 0002. Every
generation carries both Codex and Claude Code native projections. A bound,
isolated project may expose the Codex projection through an AIKit-owned
`.agents/skills` link; publication never overwrites a user-owned skill tree.
Filesystem publication is hot, while in-process harness catalogue reload remains
harness-dependent.

---

## 7. Trust

Trust is keyed on `(registry source, capsule id, content revision)` and lives in
AIKit's database. **A manifest may not declare its own trust** — attempting to
is `manifest.trust_not_self_declarable`.

States: `unseen`, `quarantined`, `reviewed`, `trusted`, `blocked`, `superseded`.

* Unreviewed hooks / skills / guidance cannot activate.
* Unreviewed scripts may activate but carry `requires_run_confirmation`.
* Quarantined capsules never project.
* Catalogued ≠ reviewed. A registry sync never changes live behaviour.

---

## 8. Hook architecture

One permanent dispatcher entry per client event:

```
PreToolUse → aikit hook dispatch claude PreToolUse
```

The dispatcher normalizes the client event, then runs the immutable chain from
`current/hooks/`:

phases `gate → transform → verify → inject → observe → capture`,
ordered by (phase, numeric order, capsule id), short-circuiting on denial.

Defaults: gates and transforms serial, verifiers parallel only when independent,
observers non-blocking. **A capsule must opt in to parallel execution.**

Failure policy per hook: `closed` (default) / `open` / `warn`. A *system failure*
and a *policy denial* are distinct in logs and messages.

Bypass is a short-lived scoped token (`aikit bypass issue --scope next-event
--reason ...`), not a global environment switch, and is recorded and made
visually obvious.

---

## 9. Client projections

`ActivationEffect`: `Immediate | LiveReloadExpected | RestartClient |
NextSessionOnly | Brokered | Unsupported`.

* **Claude Code** — a context-specific `--add-dir` directory containing
  `.claude/skills/`. Never mutates `~/.claude/skills` or the project's
  `.claude/skills`.
* **Codex** — `.agents/skills` in the task's own tree *when the task is
  isolated*. When `Isolation::Shared`, fall back (see §3).
* **Broker** — a single generic skill exposing `aikit capabilities list|read`
  and `aikit run`, for clients that cannot take an arbitrary session directory.

Default is hybrid: durable project skills stay native, session-only deltas use a
context-specific native projection where possible, and the remainder is brokered.

---

## 10. Multiplexers

`MuxAdapter` + `MuxCapabilities` let tmux and cmux implement the same *semantic*
operations with their own geometry. Neither is a compatibility afterthought.

* **tmux** — real `display-popup` overlay; session/window/pane mapping;
  `set-environment` for child inheritance and `@aikit_*` user options for status
  rendering and recovery; idempotent, non-destructive `session up`.
* **cmux** — inline Ratatui modal in the focused terminal (no documented
  arbitrary-popup primitive is assumed), plus native workspace-group, status
  pill, progress, log and notification integration.

Hybrid stacks (cmux presenting a remote tmux) are modelled as a mux stack:
topology changes target the **innermost** active mux; status may fan out to the
outer one. Host identity is shown prominently and registries are never silently
mixed across a remote boundary.

Portable session topology is canonical; tmuxp / tmuxinator / cmux JSON are
export targets, never the source of truth.

---

## 11. Crates

| Crate | Contents | Depends on |
|---|---|---|
| `aikit-core` | domain, resolver, session IR, hook IR, guidance composer, search, projection contracts. **No I/O.** | — |
| `aikit-store` | registries, TOML edit, SQLite, generations, trust, events, locks, inbox | core |
| `aikit-adapters` | mux (tmux/cmux/plain), clients (claude/codex/broker), shells | core |
| `aikit-tui` | Ratatui palette. **No resolver semantics.** | core, store |
| `aikit-cli` | clap, JSON envelope, multicall shims, hook dispatcher, app service | all |

CLI and TUI share **one** application service. The TUI never shells out to
`aikit --json` internally.

Core resolution is synchronous and deterministic. TUI orchestration is
`event → Action → reducer → AppState → render`, with effects returning Actions.

---

## 12. CLI contract

Every substantive command supports `--json`. Envelope:

```json
{ "schema": 1, "ok": true,
  "context": { "session_id": "...", "context_id": "...", "project_root": "..." },
  "data": {}, "warnings": [] }
```

Errors:

```json
{ "schema": 1, "ok": false,
  "error": { "code": "resolution.required_capability_disabled",
             "message": "...", "details": { } } }
```

The JSON shape is a real public interface. Error **codes** are stable; messages
are not.

---

## 13. Performance budgets (experience targets, not correctness assumptions)

| | |
|---|---|
| cold palette first paint | < 150 ms |
| warm palette first paint | < 60 ms |
| search keystroke | < 16 ms |
| typical context resolution | < 50 ms |
| no-op apply | < 50 ms |
| hook dispatcher startup | < 20 ms before capsule work |

Supported by: SQLite index instead of payload scans, lazy previews, in-process
fuzzy matching, resolver cache keyed by context + catalog revision, immutable
`current`, no daemon handshake, no git on ordinary search.

The popup's cold and warm first-frame budgets and its 5,000-document search
budget are executable release gates in `crates/aikit-tui/tests/performance.rs`.
They measure the production controller, matcher, and Ratatui draw path; catalog
discovery and fixture construction are deliberately reported separately.

---

## 14. Explicitly not built

No global mutable active set. No global generated skill directory as the central
mechanism. No full-screen dashboard as the primary UI. No package manager. No
embedded terminal emulator. No daemon dependency. No automatic trust from
registry presence. No automatic promotion from usage count. No silent tag-based
activation. No tmux-specific canonical session format. No pretence that cmux and
tmux have identical UI primitives.

---

## 15. Release-blocking acceptance cases

1. Two tmux sessions for the same project carry different skill sets.
2. Two cmux workspaces for the same project carry different session overlays.
3. A project profile change does not mutate another project's context.
4. A session toggle cannot affect a non-child context.
5. The same portable session capsule launches in tmux and cmux.
6. A failed projection leaves the previous generation active.
7. A Claude session receives a live session-specific skill projection.
8. An isolated Codex task receives an isolated project/session skill projection,
   and a **shared** Codex task receives an honest fallback with a stated reason.
9. A hook bypass is visible and recorded.
10. A captured secret never enters the ordinary registry.
11. Promotion can be completed without hand-writing a manifest.
12. The entire CLI works without a running daemon.
13. Adoption is diff-first, moves authority into an owned registry, and its
    recorded Procedure restores the foreign tree.
14. Typed profile bindings resolve to explicit capsule ids, while a project fork
    stores only its delta and continues to inherit the evolving base.
15. The interactive tree accepts keyboard and mouse navigation through the same
    reducer and hands the exact staged set to the shared apply path.
16. A saved Procedure can be diffed and run by exact id/digest; source drift,
    post-apply drift and unrelated adoption journals are refused without
    overwriting newer work.
17. Every writable skill-set mutation, including rename and recoverable delete,
    has a durable Procedure id and a working undo path.
