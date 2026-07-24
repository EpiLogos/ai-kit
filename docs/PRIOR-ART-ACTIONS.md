# Actions from the prior-art survey

Concrete changes `docs/PRIOR-ART.md` produced. Each is a decision, not a
suggestion; each names the system it came from so the reasoning is traceable.

Status: `DONE` applied · `NOW` apply during Part I integration · `II/III` folded
into the later specs.

---

## Security — the trust model was wrong in one place

| # | Change | From | Status |
|---|---|---|---|
| 1 | **Refusal keyed on identity, approval on content.** `TrustState::Blocked` must survive a revision bump; `Trusted`/`Reviewed` must not. A block cleared by an edit is not a block. | direnv: allow list hashes `path + contents`, deny list hashes path alone | **DONE** — `trust::TrustOracle::standing_verdict`, 9 tests in `tests/trust_keying.rs` |
| 2 | **A third state: `Dismissed`.** Without it every prompt is "yes" or "forever", so users learn to say yes. Dismissal stops the asking without refusing. | mise: `IGNORED_CONFIGS` separate from `TRUSTED_CONFIGS` | **DONE** |
| 3 | **The ledger carries human-readable identity.** A trust store you cannot enumerate in human terms cannot be audited or pruned. | direnv: grant file named by hash, contains the path — the only reason `status`/`prune` exist | **DONE** — `TrustEntry`, `MemoryTrust::ledger()` |
| 4 | **Trust inherits across git worktrees.** Canonicalise a worktree to its main checkout before keying, or every `--worktree` task re-prompts for everything. | mise: `git::main_checkout_equivalent()` | **NOW** — `aikit-store` trust backend |
| 5 | **Untrusted loads inert, not invisible.** An unreviewed capsule appears in the palette as an actionable row ("needs review") rather than vanishing. The inert mode is **global-only** — a project config must not be able to disable it for itself. | mise `safe` mode | **NOW** — already half-true via `UnavailableReason::TrustRequired`; needs the palette row and the global-only lock |
| 6 | **A profile may enable, never trust.** Extend `manifest.trust_not_self_declarable` to profiles: a committed `.aikit/profile.toml` cannot grant trust to the project-local capsules it references. | Claude Code: `enableAllProjectMcpServers` is ignored in an untrusted folder | **NOW** |
| 7 | **Never prompt on the hook path.** The dispatcher must resolve without any interactive trust question; an unreviewed hook is simply absent from the chain and recorded as such. | direnv: `trust_check()` never prompts when `cmd == "hook-env"` | **NOW** |

---

## Generations

| # | Change | From | Status |
|---|---|---|---|
| 8 | **`generation_format: 1` in `metadata.json` from the first commit.** Its presence is the test for "this directory is a generation". Impossible to retrofit. | Guix `%manifest-format-version` + `generation-profile`; home-manager `gen-version` | **NOW** |
| 9 | **Cosmetic metadata lives outside the resolution hash.** A `[properties]` table in the lock that is excluded from equality, or every label edit invalidates every generation. | Guix: `manifest-entry-properties` explicitly excluded from `manifest-entry=?` | **NOW** |
| 10 | **Generation 0 is synthesised on demand.** An empty generation materialised when needed makes "roll back past the first", "turn everything off here", and "`AIKIT_VIEW` is always valid" collapse into the ordinary apply path. | Guix `link-to-empty-profile` | **NOW** |
| 11 | **Clean-then-link, so an interrupted activation is always a subset of both endpoints** (`FA → FA ∩ FB → FB`). Ownership decided by a glob on the symlink *target*; anything not matching is never deleted. | home-manager, with the proof written in its source | **NOW** |
| 12 | **Three-way comparison**: desired / last-known-written / actual. Distinguishes "the user edited our projection" from "a previous generation wrote something different" — two-way plus a heuristic cannot. Store `(context, target, path, sha256, generation)` at materialisation. | chezmoi persistent state (BoltDB); home-manager only manages two-way | **NOW** |
| 13 | **Rollback changes what is live without minting a generation**; it appends to a separate history log. State and audit are different things. | flox | **NOW** |

---

## Dry run must not be a flag

| # | Change | From | Status |
|---|---|---|---|
| 14 | **`Materializer` trait with `RealFs` / `DryRun` / `Diff` implementations.** `apply`, `apply --dry-run`, `diff` and the palette's preview become one function parameterised by the implementation. A boolean flag threaded through call sites drifts the moment one path forgets it — and AIKit *promises* the palette shows the real per-client consequence of a toggle. | chezmoi's `System` interface; contrast home-manager's `DRY_RUN` + `run` shell convention | **NOW** |

This one also gives Part II's Procedures their `plan`/`diff`/`run` triad for free:
same code, three materializers.

---

## Hook ordering

| # | Change | From | Status |
|---|---|---|---|
| 15 | **Intra-phase `after` / `before` edges, toposorted at generation-build time.** `(phase, order, capsule id)` cannot express "this verifier after that verifier" without abusing the order field, and gives third-party capsules nothing to attach to. A cycle fails the *build* with `hooks.dependency_cycle` and can never be written into a generation. | home-manager `lib.hm.dag`, sorted at build time, cycle is an `abort` | **NOW** |
| 16 | **A `writeBoundary` sentinel.** A named no-op node; every step with an observable side effect declares itself after it. Steps before may verify and abort but may not mutate. This makes plan-then-mutate a *structural* property capsules attach to, not a convention they can violate. Apply the same boundary to materialisation. | home-manager `writeBoundary` | **II** |
| 17 | **Report collisions together with a remediation menu**, not fail-on-first. `resolve_diagnostic` already collects; the CLI must render all of them. | home-manager accumulates collisions into an array | **NOW** |
| 18 | **Warn on shadowing proactively** in `warnings[]` — do not wait for `explain`. Silent shadowing is a defect. | flox: "if one manifest overrides another, a warning is displayed" | **NOW** |

---

## Search

| # | Change | From | Status |
|---|---|---|---|
| 19 | **Score is match quality alone; usage lives in an ordered tiebreak**, ending in `CapsuleId` for a total order. A blended number is unstable between keystrokes and unexplainable. | fzf/nucleo separate score from ordered tiebreaks | **DONE** in Spec III §3.1; **NOW** in `core::search` |
| 20 | **Express scoring constants as relationships**, not magic numbers (`bonus_consecutive = -(gap_start + gap_extension)`). | fzf | **NOW** |
| 21 | **Palette scope selector is atuin's `FilterMode`**: always rendered, cycled by one key, defaulting to **the narrowest scope that actually exists**. Users already know the keystroke. | atuin `FilterMode` + `default_filter_mode(git_root)`; matches our `default_mutation_scope()` | **NOW** |
| 22 | **Secret patterns as a self-testing `(name, regex, test_value)` table** where each test value must match its own regex — the table cannot rot. Because patterns are *named*, a rejection is explainable instead of a silent drop. | atuin `SECRET_PATTERNS` | **NOW** — `store::scan` |

---

## Environment and startup budget

| # | Change | From | Status |
|---|---|---|---|
| 23 | **Reversible env diff**, recorded in a `defer` so it is captured **even when the load is disallowed or fails** — the env-level analogue of "a failed projection leaves the previous generation active". | direnv `DIRENV_DIFF`; mise `__MISE_DIFF.reverse()` | **NOW** |
| 24 | **Two-tier early exit**: a fast path that decides "nothing to do" before any config load. This is how the &lt;20 ms hook-dispatcher budget is actually met. | mise `should_exit_early_fast()` | **NOW** |

---

## Interop facts for the adapters

| # | Fact | Consequence |
|---|---|---|
| 25 | Codex scans `.agents/skills` in **every directory from cwd to repo root**, and **does not merge same-named skills**. | Nested project scope is native there. Our Codex adapter must not assume a single directory, and must expect shadowing rather than merging. |
| 26 | Claude Code is project-root-or-personal, resolved by precedence. | Different discovery shape from Codex; the adapters cannot share one assumption. |
| 27 | Claude Code MCP config precedence is **whole-record replacement, not field merge** — as is mise's `[tasks]` and flox's. | Our `profile::merge_config` does a recursive deep merge. That is defensible and tested, but **the algebra must be documented per `[config.*]` section and chosen deliberately.** Getting this wrong is the single most common "why isn't my config taking effect" failure across every system surveyed. |
| 28 | MCP registry namespace proofs scale breadth to proof strength: DNS verification grants `com.domain/*` and all subdomains; HTTP verification grants the exact domain only. | Worth copying if AIKit ever accepts third-party registries. |

---

## Capability lifecycle — learned from Hermes, which is ahead of us here

Hermes is the most developed capability tree on the machine, and it has a whole
dimension AIKit currently lacks: a **lifecycle**. Skills are not just present or
absent — they age, they carry usage, they can go stale, and there is a curator
that reasons about the set over time. AIKit has maturity (`draft`→`stable`) but
no notion of a capability's *life*: last used, gone quiet, superseded, retired.

We should build the lifecycle and keep the one guardrail Hermes does not: the
system may *observe and propose*, but the writes stay a human's.

| # | Change | Learned from | Status |
|---|---|---|---|
| L1 | **A capability has a lifecycle state derived from usage**: `active` · `quiet` (unused for N days) · `stale` (candidate for review) · `retired` (archived, still catalogued for audit). Derived, never a manifest field. | Hermes' `stale_after_days` / `archive_after_days` staging | **III/store** |
| L2 | **`.bundled_manifest`-style tamper detection**: a per-capsule content hash the store checks on load, so an out-of-band edit to a projected payload is *noticed*, not silently served. We already hash for revisions — surface a mismatch as a `DriftNotice`. | Hermes `.bundled_manifest` (name:md5); also `bkmr-essay`'s `manifest.tsv` SHA gate | **store** |
| L3 | **Per-capsule usage record** (`last_used`, `use_count`, `last_success`) feeding both frecency and lifecycle. | Hermes `.usage.json` | **store** |
| L4 | **A curator that runs and writes an inbox report, never the tree.** "18 capabilities have been quiet 90+ days; 3 are superseded; review?" — a `ProcedureReport`, one keystroke from acting, zero automatic archives. | Hermes' curator, minus the automatic write | **II** |
| L5 | **`related_skills` as first-class capsule metadata**, surfaced in the tree and the palette ("often used with…"). Richer than a flat tag. | Hermes frontmatter `related_skills[]` | **III** |

The lesson is the shape, not the schedule: Hermes proves a capability tree wants
a lifecycle. AIKit adds the resolution, the trust gate, and the rule that
curation proposes rather than deletes.

## Ecosystem compatibility — what we must read and emit without argument

From `docs/SKILLS-ECOSYSTEM.md`. AIKit has to slot into a machine that already
has tools on it, not demand a conversion first.

| # | Requirement | Why | Status |
|---|---|---|---|
| 29 | **Index both flat and two-level container layouts.** An indexer that walks one level deep misses 26 Hermes categories entirely. | Hermes nests `<category>/<name>/`; the Skills CLI handles the same case across ~60 container dirs | **NOW** — `store::registry` |
| 30 | **Preserve unknown frontmatter keys** on import and re-emit. Hermes' `platforms:`/`related_skills:` and our `metadata.aikit` coexist by spec. | Dropping unknown keys silently degrades every skill we touch | **NOW** |
| 31 | **Emit `.skill-lock.json` v3 exactly** when we write one. The vendor tool's `readSkillLock()` returns an **empty lock** when `version < CURRENT_VERSION` — it discards rather than migrates. | Emitting v4-ish JSON would erase the user's provenance the next time they run the vendor tool | **NOW** |
| 32 | **Never clobber an existing symlink.** `~/.claude/skills` currently holds 14 relative links into `~/.agents/skills` interleaved with 25 hand-made directories. Projections coexist or refuse; they do not overwrite. | This is someone's working setup | **NOW** — `clients::claude` |
| 33 | **Read** `installed_plugins.json` v2, `known_marketplaces.json`, Codex `plugin.json` / `marketplace.json` / `[[skills.config]]`, and `skills-lock.json` field semantics — as provenance sources. | Import, not conversion | **NOW** |
| 34 | **Align the digest format with the Cloudflare discovery RFC** (`sha256:` prefixed, verified on write, origin allowlists, scripts rejected by default) rather than inventing a third hash convention. | It and `skills-lock` are the only two serious integrity models in the ecosystem | **II** |
| 35 | **Generalise Codex's `[hooks.state] trusted_hash = "sha256:…"`.** The vendor already ships trust-on-first-use content approval — for hooks, not skills. Read it, honour it, extend the same idea to every kind. | Our trust gate is not novel; it is the mechanism next door, applied consistently | **NOW** |

## Ecosystem anti-patterns — divergences on purpose

Each is shipping somewhere today. Recorded so a contributor can see we diverged
knowingly, not in ignorance.

| Anti-pattern | Who | Our position |
|---|---|---|
| Global mutable canonical store | Skills CLI (`~/.agents/skills`) | Per-context generations |
| Hash recorded, never verified | Skills CLI `skillFolderHash` — exists only so `update` can diff | Verify on write |
| No gate between download and live | every agent skill tool surveyed | `TrustState` before projection |
| Silent clobber on name collision | Skills CLI — flat lock keyed by bare name; a second `pdf` destroys the first **and reassigns its provenance** | `resolution.export_collision`, fail visibly |
| Provenance discarded on schema bump | Skills CLI `readSkillLock()` | Migrate, never discard |
| Install count as a trust signal | registries generally | Usage suggests; it never promotes |
| Aggregation launders trust | plugins/bundles generally | A set projects only members that pass their own gates |
| Curation that acts without asking | a scheduled curator that archives on an idle timer | keep the *lifecycle* (below); make the archive step a proposal, not an automatic write |
| Silent fallback that changes mechanism | Skills CLI (GitHub API → sparse clone) | Report the mechanism actually used |

## What the survey confirms we are alone in doing

**Nobody has generations per context.** Nix, Guix and flox have excellent
generation machinery bolted to a *single mutable pointer*. mise, asdf and direnv
have excellent per-context resolution with no generation, no snapshot, no
rollback, no materialised artefact. Nobody has combined them.

The consequence is that everywhere else, "give this session a different
capability set" is either impossible or achieved by *leaving the managed model*
into an unmanaged, unexplainable, un-rollbackable subshell (`nix-shell`,
`guix shell`, `flox activate`). Acceptance cases §15.1 and §15.2 — two sessions,
same project, different capability sets, neither mutating the other — are not
achievable in any surveyed system today.

`state/contexts/<ctx>/current` has no precedent, and everything else AIKit does
is downstream of that one structural choice.

Runner-up gaps, all real:

* **No agent capability system has any trust mechanism at all.** The published
  guidance amounts to "treat it like installing software", and then nothing
  ships — a skill arriving via `git pull` is live next session.
* **Nobody separates available / enabled / loaded.**
* **Nobody explains the success case.** NixOS explains *conflicts* beautifully
  (`definitionsWithLocations`) and success poorly. Guix's `manifest-entry-parent`
  is the best success-case provenance found anywhere and is not surfaced as a
  command.
* **Nobody models concurrent divergent demand as normal.** It is a lock, an
  impossibility, or undefined.
