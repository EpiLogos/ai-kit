# Production Integration Completion Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make AIKit discover and integrate with installed tmux, cmux, and npx-skills systems without guessing, overwriting foreign ownership, or claiming support that was not verified.

**Architecture:** Introduce explicit inventory/provenance models at the integration boundaries. Multiplexer selection will consume detected installation, server, active-stack, version, and capability evidence; cmux installation will structurally merge its supported `cmux.json` representation and remain exactly reversible. Skill discovery will read global and project npx lockfiles as provenance beside the observed filesystem, never execute npx, and never adopt authority implicitly.

**Tech Stack:** Rust, serde/serde_json, existing AIKit Procedure runner, real-process adapter tests, real CLI binary acceptance tests.

---

### Task 1: Inventory and ambiguity-safe mux selection

**Files:**
- Modify: `crates/aikit-cli/src/mux_install.rs`
- Modify: `crates/aikit-cli/src/main.rs`
- Test: `crates/aikit-cli/tests/mux_install.rs`
- Test: `crates/aikit-cli/tests/every_command.rs`

**Steps:**
1. Add failing tests proving unnamed installation refuses to guess when tmux and cmux are both installed.
2. Add failing tests proving an active layer may be selected only when it is uniquely active.
3. Run the focused tests and verify the expected failures.
4. Implement a system inventory containing path/version/installed/running/inside evidence for every mux.
5. Make install selection consume that inventory and emit an actionable ambiguity error.
6. Run focused and read-only-command tests to green.

### Task 2: Correct, reversible cmux configuration

**Files:**
- Modify: `crates/aikit-cli/src/mux_install.rs`
- Create: `crates/aikit-cli/tests/cmux_install.rs`
- Test: `crates/aikit-cli/tests/mux_install.rs`

**Steps:**
1. Add failing real-filesystem tests for `~/.config/cmux/cmux.json`, preservation of unrelated JSON, idempotence, collision refusal, deliberate replacement, and exact Procedure undo.
2. Add failing compatibility tests for cmux 0.63 command-palette integration and shortcut-capable action integration.
3. Run the focused tests and verify their failures.
4. Implement strict JSON parsing and structural merge without marker comments.
5. Select command/action representation from installed-version/capability evidence; never write unsupported keys.
6. Verify written config by parsing the exact owned entry; use live reload when the installed CLI supports it and otherwise rely on cmux's verified file watcher with an explicit warning.
7. Run focused tests to green.

### Task 3: Route session operations through the active mux stack

**Files:**
- Modify: `crates/aikit-cli/src/app/mod.rs`
- Modify: `crates/aikit-cli/src/main.rs`
- Modify: `crates/aikit-adapters/src/mux/stack.rs`
- Test: `crates/aikit-cli/tests/acceptance.rs`
- Test: `crates/aikit-adapters/tests/stack.rs`

**Steps:**
1. Add failing tests proving cmux-only and nested cmux→tmux session diff/reconcile reach the topology owner.
2. Add failing tests proving context binding and descriptor reporting use detected stack evidence.
3. Run focused tests and verify the failures.
4. Centralize system-stack construction and route topology/session operations through it.
5. Preserve non-destructive defaults and require explicit exact reconciliation for removal.
6. Run focused tests to green.

### Task 4: Reconcile existing cmux workspaces

**Files:**
- Modify: `crates/aikit-adapters/src/mux/cmux.rs`
- Test: `crates/aikit-adapters/tests/cmux_contract.rs`
- Test: `crates/aikit-cli/tests/acceptance.rs`

**Steps:**
1. Add failing tests proving a title-rebound workspace is inspected for existing panes/surfaces.
2. Add failing tests proving missing surfaces are added, healthy surfaces are preserved, and exact mode removes only AIKit-owned extras.
3. Run focused tests and verify the failures.
4. Implement readback/reconciliation using cmux tree/list commands and stable AIKit ownership metadata.
5. Ensure existing pane commands are not rerun and foreign surfaces remain untouched by default.
6. Run focused tests to green.

### Task 5: Ingest npx skills provenance without taking authority

**Files:**
- Modify: `crates/aikit-cli/src/foreign.rs`
- Modify: `crates/aikit-cli/src/main.rs`
- Modify: `crates/aikit-cli/src/tree_build.rs`
- Create: `crates/aikit-cli/tests/npx_skills.rs`
- Modify: `crates/aikit-cli/tests/init_command.rs`

**Steps:**
1. Add failing tests for global v3 `.skill-lock.json`, XDG location, project v1 `skills-lock.json`, malformed/unsupported lockfiles, and lock entries whose projected skill is absent.
2. Add failing real-binary tests proving `init` reports source, URL, path, hash, timestamps, scope, and lock path.
3. Run focused tests and verify the failures.
4. Implement tolerant read-only parsers preserving unknown fields and refusing to reinterpret unsupported versions.
5. Correlate lock entries with observed skill directories/symlinks and surface drift/missing state without executing npx.
6. Include project `.agents/skills` automatically when a project lock exists.
7. Run focused tests to green.

### Task 6: Consolidated real acceptance and documentation

**Files:**
- Modify: `crates/aikit-cli/tests/acceptance.rs`
- Modify: `crates/aikit-cli/tests/every_command.rs`
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/SKILLS-ECOSYSTEM.md`

**Steps:**
1. Add an actual-system test that executes installed mux binaries when available and asserts honest absent/not-running degradation.
2. Exercise cmux installation against a real temporary home and parse the resulting config with the production parser.
3. Exercise npx provenance using fixtures derived from the installed v1.5.10 formats, not invented schemas.
4. Run formatting, workspace tests, clippy, and release build.
5. Run a live machine smoke against installed tmux and cmux without overwriting user configuration.
6. Request adversarial review, fix all critical/important findings test-first, rerun the full verification gate.
7. Commit, push the feature branch, and open a GitHub pull request.
