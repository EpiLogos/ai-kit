# Harness Profiles Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: implement task-by-task with failing
> tests first; do not mark a task done without its verification command
> passing. Follow STANDARDS.md §1–§8 exactly.

**Goal:** replace per-harness bespoke handling with the general harness
outline designed in `2026-09-16-harness-profile-design.md`: a declarative
`aikit.harness-profile/v1`, a `tool-protocol` capsule kind (MCP servers as
capability sources), one layer merge engine, and a single roster derived from
profiles.

**Architecture:** new `harness_profile` module in `aikit-core` (schema +
posture + layer declarations); new `Kind::ToolProtocol` capsule section; new
`layers` module in `aikit-adapters` owning ALL native-config merge grammars
(claude hook map, zcode hook wrapper, mcp-servers record map) behind one
engine with parity goldens; profiles as data in `aikit-cli` driving the
registry and the tools-layer projection through the existing
`plan_install`/`WorldEdit`/`Procedure` pipeline.

**Tech Stack:** Rust (workspace crates `aikit-core`, `aikit-adapters`,
`aikit-cli`), serde TOML/JSON, existing test infrastructure (vitest-style
goldens are not used here — Rust integration tests with tempfile).

**Baseline (2026-09-16, branch `techne/t1-facets`, tree as found):** core 456
lib tests green, adapters 208 lib tests green. The tree carries unrelated
in-flight work; tasks below touch ONLY their listed files.

---

### Task 1: `aikit.harness-profile/v1` schema (aikit-core)

**Files:**
- Create: `crates/aikit-core/src/harness_profile.rs`
- Modify: `crates/aikit-core/src/lib.rs` (module declaration + export only)

Types: `HarnessProfile` (`schema`, `slug`, `edition: HarnessEditionKind`,
`presence` probes echo, per-layer sections `skills`/`guidance`/`hooks`/
`tools`/`models`/`sessions`/`settings`), `LayerPosture` (`Managed`/`Brokered`/
`Observed`), `ToolSourceRecord` (`command` xor `url`, `args`, `env`, `cwd`),
observe declarations (`path` + `collection`), project declarations
(`file`, `key`, `format: MergeGrammar`, `merge` policy), `MergeGrammar`
(`ClaudeHookMap`, `ZcodeHookWrapper`, `McpServersRecord`),
`ActivationEffectName` (string enum mirroring `ActivationEffect` variants).
Validation: `Managed` layers require a project declaration; `Observed`/
`Brokered` require observe or an explicit reason; tool records require
command xor url; unknown schema version refused with a plain error naming
the expected version.

Tests: parse a full profile (openclaw shape from the design doc); posture
validation failures name the layer and the missing declaration; round-trip
TOML and JSON.

Verify: `cargo test -p aikit-core --lib harness_profile` then
`cargo test -p aikit-core --lib`.

### Task 2: `tool-protocol` capsule kind (aikit-core)

**Files:**
- Modify: `crates/aikit-core/src/capsule.rs` (`Kind`, sections, parse,
  `activation_meaning`, `requires_trust_to_activate`)
- Modify: any `match` on `Kind` the compiler flags (make arms exhaustive,
  no silent defaults)

`[tool-protocol]` section: one `ToolServerRecord` (command/url, args, env,
cwd) + optional `export_name` (defaults to capsule leaf) + `targets`
(explanatory). Trust: `requires_trust_to_activate` returns true (an MCP
server executes code). `activation_meaning` states what activation means for
a tool source. The record is whole-record config: default
`config_merge = "replace"` semantics documented in the section docs.

Tests: parse a `[tool-protocol]` capsule; command-only, url-only, both
(refused), trust requirement, exhaustive-match compile.

Verify: `cargo test -p aikit-core --lib capsule` then full core lib tests.

### Task 3: layer merge engine (aikit-adapters)

**Files:**
- Create: `crates/aikit-adapters/src/layers/mod.rs` (+ submodules)
- Modify: `crates/aikit-adapters/src/clients/hook_map.rs` (delegate or
  retire), `crates/clients/zcode.rs` merge function, `crates/clients/claude.rs`
  call site, `crates/aikit-adapters/src/lib.rs` (module export)

One engine, three grammars, each with parity goldens captured from the
CURRENT hand-written implementations before refactor:
- `claude_hook_map` — exact current `hook_map::merge_hook_map_entries`
  behavior (matcher policies, stale sweep by dispatch-command identity,
  empty-key pruning). Goldens = existing hook_map tests, kept green.
- `zcode_hook_wrapper` — exact current `merge_dispatcher_entries` behavior
  (`enabled` law, user-disabled refusal, matcher omission). Capture goldens
  first: write golden tests against the current function output, then move.
- `mcp_servers_record` — new: record-map merge keyed by server name;
  preserve foreign, replace managed (identified by ownership identity),
  sweep stale managed; never emit secret values (env values pass through
  only into the projected file, never into logs/errors).

Engine reports a `MergeReport {added, replaced, removed_kept_foreign,
swept}` for receipts/disclosure. No silent degradation: unreachable key
paths and unparseable existing documents are errors naming the file and
problem.

Verify: `cargo test -p aikit-adapters --lib layers` and adapters lib suite
green; parity goldens prove byte-equality with pre-refactor outputs.

### Task 4: profiles as data + tools-layer projection (aikit-adapters/aikit-cli)

**Files:**
- Create: `crates/aikit-adapters/src/profiles/` (profile instances for
  claude, codex, zcode, pi, gemini-cli, kimi, openclaw — postures and layer
  declarations as coded data with the catalog slugs as join keys)
- Modify: `crates/aikit-cli/src/client.rs` REGISTRY rows to derive reach and
  admission hooks from profiles where declared; `crates/aikit-cli/src/app/mod.rs`
  `client_effects` to source qwen/ollama from the same table (registry
  divergence retired by construction)
- Create: tools-layer projection: resolved `tool-protocol` capsules →
  `MergeReport`d record map → `ProjectionItem::Write` of the target config
  through the existing `plan_install` pipeline (install items remain
  Write-only)

Acceptance integration test: compose two `tool-protocol` capsules (one real:
bimba, launcher
`/Users/admin/Central/Work/epi/bimba-portable/bimba-mcp.sh`) plus a foreign
server entry pre-seeded in a temp openclaw-shaped config; project; assert
byte-level: both managed records present, foreign entry untouched; remove one
capsule, reproject, assert managed record swept and foreign STILL present.

Verify: `cargo test -p aikit-cli --lib harness_profiles` (or integration test
crate) and the new acceptance test.

### Task 5: workspace hygiene + docs

**Files:** README or docs entry naming the profile schema, the capsule kind
and the merge engine; `cargo fmt --all`; `cargo clippy --workspace` clean of
new warnings; full workspace test run recorded.

---

## Execution order and ownership

Wave 1 (parallel): Task 1, Task 2 — disjoint files.
Wave 2 (parallel): Task 3, Task 4 — Task 4 consumes Task 1+2 types only;
Task 3 owns the merge surface.
Wave 3: integration verification (fmt, clippy, full test suite, acceptance),
fix-forward.
No commits: the branch carries unrelated in-flight work; all changes land in
the working tree, verified green, with a file manifest in the session return.
