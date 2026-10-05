# AIKit architecture

> **Reading boundary, 30 September 2026:** this is the earlier resolver/migration
> baseline. The [V2 field](v2/README.md) states target design and numbered returned
> implementation contracts; [current native architecture navigation](ARCHITECTURE-NAVIGATION.md)
> identifies source/context, praxis, encounter, Flow and upgrade owners. The old
> “not a session launcher” framing below is historical product scope, not a claim
> that AIKit lacks its later canonical encounter/session operations.

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
| **Session space** | An AIKit concept bound to the place technology that owns it — a tmux session, a cmux workspace/group, a plain terminal, or any other declared place technology (an open, validated name; tmux/cmux/plain are the built-ins this build drives). |

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

A plan names its place technology with an open, validated name (`PlaceTechnology`,
wire field `mux`; `tmux`/`cmux`/`plain` serialize exactly as the old closed enum
did). tmux and cmux are the built-ins this build drives; other declared names —
`herdr` today, anything else tomorrow — stage and persist as first-class plans,
and a place-technology registry in `aikit-adapters` decides what is detectable
and drivable. An unregistered name is a declared-unsupported outcome naming the
technology and what would support it, never a parse failure or a silent
fallback.

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
are not. `aikit doctor --json` is the published installed verification entry
(`.oi/product.json` `verify.installed_command`), so its success envelope is part
of this contract; the acceptance test
`doctor_json_is_the_installed_native_verification_path` guards it.

Multi-leg Wiki mutations retain the original failure code, message and details
and add whole-invocation effect evidence in `error.details`. `command_effect`
is `none`, `present` or `unknown`; `none` requires an observed pre-effect phase
with no earlier effect. A failed second leg or `published="false"` alone does
not establish absence. `completed_effects` and `failed_effect` are JSON strings
describing acknowledged effects and the failed owner phase separately. The
original failure remains available in the JSON-string `original_error`;
existing cause, source, revision and publication markers are retained.
Incomplete effects carry `outcome="partial"` or `"unknown"` and
`automatic_retry="false"`. A caller must preserve the whole native envelope;
it must not roll back acknowledged sources, resend automatically or mint a
replacement operation identity. An unchanged Wiki is not a new publication.

Wiki and SourcePool material have distinct effect scopes. Ingest may acknowledge
Wiki publication and then fail preparing, publishing or pruning source-material
shards. Each acknowledgement names only the effect actually observed; it does
not promise a multi-file transaction. An OS material-write acknowledgement is
not a claim of unverified durability or complete reconstruction. For a command
that may mutate, an unmarked failure is conservatively uncertain. Explicit
publication, partial/unknown outcome, completed effects or malformed/conflicting
effect evidence take precedence over a claimed whole-command `none`.

SourcePool refresh renders every next shard before effects, captures the old
material bases, then delegates replacement and atomic no-clobber first creation
to the shared physical publication adapter. Only after required replacements
are acknowledged can exact old stale bases be removed through that adapter.
Participating writers share its advisory lock; arbitrary external filesystem
mutation is not excluded. Removal uncertainty carries `removed="true"` with
the exact old basis and original cause; it is not Wiki publication. An
incomplete corpus IO read refuses apply before Wiki/material refresh, retaining
old bytes instead of treating unavailable material as deletion. Dry run still
discloses skipped files. This does not establish fresh retrieval of retained
unavailable material or resolve later audience withdrawal policy.

### Source origin and current disclosure

The existing SourceBinding metadata key `aikit.source-origin/v1` carries typed
semantic lineage: an explicitly selected authored corpus, or an actual native
World, SourceRef, original revision and body-free owner binding. Available and
agent_retrieval_allowed are observations, not audience grants. New physical
root/member/executable routes remain transient; this release persists no local
route and invents no owner or audience revision. Full material persistence and
an outward disclosure projection are separate operations. Source explanation
removes legacy Central routing/body payloads, local-route metadata and
origin-required physical locators while retaining semantic refs and revisions; canonical serde is not
silently redacted. Missing current origin routing withholds generic copies,
without deleting or automatically adopting retained bytes.

The pure already-authorised SourcePool and `ingest_corpus` APIs remain useful
without Central. The additive origin-aware compiler checks exact selected-input
coverage and actual compiler content basis, preserving stable corpus identity.
That basis is the existing plain 16-hex FNV-1a64 token; native prefixed revisions
and physical SHA-256 publication bases remain distinct. IO, current owner
admission and compatible output scope belong to the caller. Team denotes
Project-horizon eligibility, separately from external-provider egress. Current
selected local native retrieval alone does not establish the destination
World/Project publication relation; without that actual compatible relation
the corpus command refuses before Wiki/material publication. An enclosing native floor governs ancestor withholding, but a
manifest or test scratch path alone is not a native Source identity. Explicit
standalone selection retains its declared-root contract; an unavailable owner
is disclosed separately and never proves native nonparticipation.

An origin-required source read must obtain the current owner result, and its
identity, revision, origin and body must match retained material. None, error
or a changed basis cannot substitute copied bytes or put a new body under an
old revision. Source explanations, search title/snippet hits and route revisions use the
same current-material validator. `SourcePoolReading` carries transient owner
privacy, and additive `read_for` admits this operation's `RetrievalTarget`
through the existing `ContextSourcePrivacy::payload_boundary`. The default
KnowledgeApplication target is LocalAgent; callers explicitly select the actual
Human, LocalAgent or ExternalProvider delivery relation. Both held material and
unheld live hits are admitted before payload projection. An index hit without a
current selected reading cannot prove admission. Independent already-authorised
local memory material remains compatible, with external egress denied by default.
Copied Team/Public labels never substitute an owner grant. Denied search metadata is
omitted with only a generic unavailable/withheld reason and stable error code;
selected reads/explanations retain the exact native failure. Wiki-owned
citation-only relations remain meaningful without materialising the source.
Explicit named Wiki query reads have a separate document contract; this clause
does not claim universal origin admission for that route. The ordinary pure
in-memory fallback stays compatible. Native
file-map inspect/locate/resolve are ReadOnly owner operations; the separately
contracted projectcentral.source.read reconciles derived horizon state and is
LocallyMutating. Their actual envelopes and effects remain distinct.

---

### Captured script lifetime and Return

The existing script planner carries the actual manifest `timeout` into the
same `SystemRunner` used by native adapters. Capture uses strict UTF-8,
16 MiB per stream and a shared capacity of 65,536 actual LF bytes across both
streams. Script capture selects body-free failure diagnostics: partial bodies,
arguments, program/cwd/path values are not copied into public messages/details.
Actual code, lifecycle, IO kind/errno and captured byte counts remain. Executed
selected failures move their original bounded raw vectors and actually observed
status into the existing private Core NativeCapture; this does not certify EOF
completion or fabricate a completed receipt. Original and independently observed
cleanup IO remain typed, and clone/wrap retains the same private observation.
Public JSON, Display and Debug receive no captured body or private path values. Generic adapters retain their existing
diagnostic contract, including their unresolved audience obligations. The runner admits each chunk before retaining it; overflow returns a
specific capacity failure with actual execution and cleanup evidence, rather
than a truncated successful result. A script without a declared deadline
retains the existing live-leader policy. The separate two-second retirement
allowance, complete-EOF requirement and original IO causes remain native
runner responsibilities.

A completed `RunReport` retains the actual runner `Output` in an `Arc` alongside
its existing line view. Script capture maps an actual Unix signal to
`128 + signal`; generic adapter capture keeps its current default. CLI and
multicall consumers deliver the exact completed stdout and stderr separately,
without reconstructing streams from the line view. Delivery IO failure keeps
the completed streams in the caller's borrowed `Arc`, while normal error
diagnostics receive only known status, byte counts, delivery uncertainty and
the original IO cause. No completed body is rerouted to stderr or error JSON,
and no automatic retry occurs; an already delivered prefix is possible. Output delivery to
an external sink has no new finite IO deadline in this change.

The line view remains stdout `.lines()` followed by stderr `.lines()`: at most
65,538 rows and 32 MiB of logical text, preserving CRLF, empty rows and
unterminated tails. Requested line headers on a 64-bit host add at most
1,572,912 bytes; raw plus projection plus headers is at most 68,681,776 logical
bytes. Allocation and RSS are not certified by that bound. The existing Rust
infallible allocation policy remains; this change claims no OOM recovery. Cloning retains the
same raw `Arc`, while each cloned line view remains bounded; arbitrary retained
clone counts and generic failure-diagnostic copies are separate resource obligations.
Method and scoped v1 digests keep their existing line basis. Method hashing
streams that same basis instead of allocating another joined string.

This is an explicit v0.x Rust source API migration in the public CLI library,
not a claim of source compatibility or an invented release number:
`ScriptCommand` adds `timeout: Option<Duration>` and `RunReport` adds
`captured: Option<Arc<runner::Output>>`. External Git/path Rust consumers must
update struct literals and run their own compile gates. Bare manually planned
commands use an explicit budget or `None`; manual reports use `captured: None`.
Only actual native capture populates `Some`. Noncapture modes, semantic capsule
identity, trust, applied revisions and scoped authority remain unchanged. The
old `Command.output()` capture route is removed at cutover; no dual launcher is
retained. Wire receipts and historic Redis/Method/Return records are not rewritten.

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

## Central-backed file maps and skill sources

Inside a Central World, durable file/source/link meaning and bkmr database lifecycle
belong to Central. AIKit's native consumer resolves current owner sources, fetches
payloads on demand, and uses owner-grounded skill snapshots for contextual and
harness projection. It does not rebuild the owner's map. See
[integrations/bkmr.md](integrations/bkmr.md) for the implemented Actions,
`source bind-central` path, degraded behaviour and joined acceptance.

### Native command capture

The existing SystemRunner owns one bounded process-output lifecycle on Linux
and macOS. It reads held nonblocking pipes without output reader threads or
EOF joins. The default captured-byte capacity is 16 MiB per stream; an explicit
positive finite override admits a larger transport. A configured timeout still
bounds the live leader. Without a timeout a live leader may continue, while
output capacity and the post-leader cancellation, direct-child reap and drain
remain bounded. Exit observation keeps the owned leader unreaped until the
runner decides whether inherited pipes require a group signal. Completed EOF
reaps without signalling descendants whose streams are already closed. Actual
child-ownership loss or an earlier reap forbids group and direct-child signals;
no numeric PID/PGID lookup reconstructs that authority. The caller exclusively
owns its Child while this native lifecycle runs. Nonzero exits remain ordinary
status data. A limit is an error, never truncated success. Unsupported capture
platforms refuse before spawn.

Knowledge native owner invocations share one application-owned runner with a
60-second deadline per command and the explicit 128 MiB transport allowance.
This mechanical limit does not change the generic runner's no-timeout default,
source admission or source size. Timeout retains actual execution/effect facts
and original cause; it is unavailable, never copied-source fallback or absence.

Native body transport uses an explicit 128 MiB command allowance where a valid
16 MiB source can expand through JSON escaping and its envelope. The mechanical
source limit and native identity/admission do not change. An oversized aggregate
Wiki response is unavailable rather than silently truncated. A failure after
execution retains possible effects, actual known exit status, bounded captured
prefix and original IO cause. Signal delivery is distinct from reaping and from
retirement of arbitrary descendants; a finite cleanup failure cannot be reported
as normal command success or as rollback.

### Current Knowledge source admission

An explicitly configured Central owner keeps its original invocation locator,
including an alias, even when current observation is unavailable. Missing or
unreadable known-owner state cannot select an implicit standalone horizon. A
truly absent undeclared home hint remains optional. Current observation carries
the original IO cause; labels alone do not determine disclosure scope. This
relation does not establish retained semantic Service World/Project identity
or preserve an original selected Project route erased by upstream discovery.

Each Knowledge operation materialises its current native inputs and uses the
owning providers. A result-cache hit, a process-held material horizon, file
metadata or TTL does not grant permission to disclose a source body, snippet or
dependent relation. Native source identity, scope, revision and refusals remain
the owning contract. Useful indexes and read models remain valid when the
owning query checks their current basis and admission. This does not promise
exclusion of arbitrary external mutations after the last owner checkpoint.

The application result-cache wrappers, process basis memo and Ground/Full
shortcut are retired; the Knowledge runtime belongs to one operation. The
public store transport API remains compatible: storing bytes is not source
admission. Prepared NOW, delivery, revocation and learning retain their existing
native contracts. The application no longer reads the old disposable
`knowledge` result-key family or uses `AIKIT_KNOWLEDGE_RESULT_CACHE` to select a
shortcut. TTL bounds retained bytes, not authority. No source, Wiki, history,
configuration or user bytes are deleted. Older installed consumers require an
explicit cutover before composed acceptance.

Generic discovery checks the existing native path predicate before and after
candidate body IO, and prunes a marked directory before listing descendants.
A selected descendant narrows discovery, not its known native disclosure
boundary. Actual World or Project boundaries come from existing context and
bindings; genuinely standalone material retains its declared root without a
new universal above-root convention. The physical predicate is not a source
identity, an origin receipt or an atomic exclusion of concurrent writers.

Known live-source projections require their originating owner's current
admission and authoritative basis. Independently accepted retained material
follows its actual owner. Missing legacy origin remains preserved and
disclosed, never guessed or automatically promoted. Removing the result cache
does not by itself repair an absent compiler-origin relation. Open, route,
frame, accessibility, familiarity, history, span and coverage continue to
record actual use through their existing native paths. Retrieval and skill
selection do not grant mutation or disclosure authority.

Read-only physical material observation reuses the existing publication adapter:
held requested/canonical parent affiliation, an ordinary single-link source,
descriptor-relative no-follow/nonblocking opens, bounded rereads and final
named/held identity checks. It creates no lock and confers no source authority.
The material-basis operation retains its 16 MiB limit; generic JSON discovery
retains its 4 MiB limit. Eager ProjectCentral source/Wiki reads have an explicit
16 MiB capacity. A larger source receives an honest budget/unavailable result,
never truncation or a claim that the source is missing. Its bytes remain in
place; use a supported bounded native-owner read when available, otherwise the
next repair belongs to that owner's chunked-read contract. Direct IO failures
retain their actual original cause independently of the stable domain envelope.
