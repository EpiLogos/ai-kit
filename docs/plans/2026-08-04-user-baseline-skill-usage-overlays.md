# User Baseline Profiles and Skill Usage Overlays Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete AIKit's persistent user/global scope and add scoped, additive per-skill guidance that is compiled into honest harness-facing Effective Skills.

**Architecture:** The global scope becomes a real `PoolPatch` stored below `AIKIT_HOME/scopes/global/profile.toml` and loaded before every project layer. `PoolPatch` also gains structured `skill-overlays` entries; resolution folds them in scope order, records provenance, supports an explicit inherited-overlay reset, and hashes the effective guidance. Agent Skill adapters materialize an Effective Skill only when overlays exist, preserving every upstream companion file while generating an augmented `SKILL.md`; unaugmented skills retain the zero-copy link path.

**Tech Stack:** Rust 2021, clap, serde/toml/toml_edit, serde_yaml for generated frontmatter, immutable projection plans, real CLI/filesystem integration tests.

---

### Task 1: Complete the persistent User Baseline Profile

**Files:**
- Modify: `crates/aikit-store/src/home.rs`
- Modify: `crates/aikit-cli/src/app/mod.rs`
- Modify: `crates/aikit-cli/src/main.rs`
- Modify: `crates/aikit-cli/src/ui.rs`
- Test: `crates/aikit-cli/tests/global_profile.rs`
- Test: `crates/aikit-store/tests/home_layout.rs`

**Step 1: Write the failing real CLI tests**

Create a temporary AIKit home, a real capsule, and two real project directories. Run the built `aikit` binary to prove:

```rust
run(home, outside, &["enable", ID, "--scope", "global"]);
assert!(run_json(home, project, &["status"])["data"]["active"]
    .as_array().unwrap().iter().any(|item| item["id"] == ID));
run(home, project, &["disable", ID, "--scope", "project"]);
assert!(!active(home, project, ID));
assert!(active(home, sibling, ID));
```

Also prove `--scope user` aliases `global`, a second process rediscovers the declaration, `aikit use <profile> --scope global` works, and malformed global TOML is reported without replacement.

**Step 2: Run the tests and verify RED**

Run: `cargo test -p aikit-cli --test global_profile -- --nocapture`

Expected: FAIL with `scope.unwritable` and/or absence of the global layer.

**Step 3: Implement the complete layer**

Add a typed accessor:

```rust
pub fn global_profile(&self) -> PathBuf {
    self.root.join("scopes/global/profile.toml")
}
```

Load this document first in `assemble_layers` as `ScopeKind::Global`. Extend `scope_document` to return a `ProfileDocument` for Global and create its parent atomically through the existing writer. Parse scope arguments using `ScopeKind::from_str` so `user` remains a supported alias. Correct TUI copy to name the active global document rather than the named-profile registry.

**Step 4: Run focused and regression tests**

Run: `cargo test -p aikit-cli --test global_profile -- --nocapture`

Expected: PASS.

Run: `cargo test -p aikit-core context scope && cargo test -p aikit-tui scope`

Expected: PASS.

**Step 5: Commit**

```sh
git add crates/aikit-store/src/home.rs crates/aikit-cli/src/app/mod.rs crates/aikit-cli/src/main.rs crates/aikit-cli/src/ui.rs crates/aikit-cli/tests/global_profile.rs crates/aikit-store/tests/home_layout.rs
git commit -m "feat: complete persistent user baseline profiles"
```

### Task 2: Model ordered Skill Usage Overlays in resolution

**Files:**
- Modify: `crates/aikit-core/src/profile.rs`
- Modify: `crates/aikit-core/src/resolve/mod.rs`
- Modify: `crates/aikit-core/src/resolve/explain.rs`
- Modify: `crates/aikit-core/src/resolve/hash.rs`
- Test: `crates/aikit-core/tests/resolution.rs`
- Test: `crates/aikit-core/tests/resolution_hash.rs`

**Step 1: Write failing core behavior tests**

Specify the wished-for patch form:

```toml
[skill-overlays."skill/mattpocock/engineering/wayfinder"]
description = "Prefer for work that crosses agent sessions."
guidance = "A map may carry execution when its Notes say so."
reviewed_against = "a2a56a..."
```

Tests must prove ordered accumulation from Global → Project → Session, profile-expanded overlays, `inherit = false` resetting lower overlays, rejection on non-skill targets, stable provenance, stale `reviewed_against` warnings, and a changed overlay changing the resolution hash without changing the capsule revision or trust.

**Step 2: Run and verify RED**

Run: `cargo test -p aikit-core --test resolution skill_usage_overlay -- --nocapture`

Expected: FAIL because `PoolPatch` and `ResolvedView` do not model overlays.

**Step 3: Add the model and fold algebra**

Introduce serializable types equivalent to:

```rust
pub struct SkillUsageOverlayPatch {
    pub inherit: bool,
    pub description: Option<String>,
    pub guidance: Option<String>,
    pub reviewed_against: Option<Revision>,
}

pub struct AppliedSkillUsageOverlay {
    pub description: Option<String>,
    pub guidance: Option<String>,
    pub scope: ScopeKind,
    pub origin: LayerOrigin,
    pub via_profile: Option<ProfileId>,
}
```

Store patches under `PoolPatch.skill_overlays`. Fold them in the same sorted scope/profile order as selection and config. `inherit = false` clears only earlier overlays for that skill before applying the current additive record. Reject empty records, NUL text, overlays aimed at non-skills, and a `reviewed_against` value that is not a valid revision. Include effective overlays in resolution serialization, explanation, and hashing.

**Step 4: Run tests and refactor green**

Run: `cargo test -p aikit-core --test resolution -- --nocapture`

Expected: PASS.

Run: `cargo test -p aikit-core --test resolution_hash -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```sh
git add crates/aikit-core/src/profile.rs crates/aikit-core/src/resolve crates/aikit-core/tests/resolution.rs crates/aikit-core/tests/resolution_hash.rs
git commit -m "feat: resolve scoped skill usage overlays"
```

### Task 3: Add format-preserving overlay authoring and CLI UX

**Files:**
- Modify: `crates/aikit-store/src/edit.rs`
- Modify: `crates/aikit-cli/src/cli.rs`
- Modify: `crates/aikit-cli/src/main.rs`
- Modify: `crates/aikit-cli/src/app/mod.rs`
- Test: `crates/aikit-store/tests/edit.rs`
- Test: `crates/aikit-cli/tests/skill_overlay.rs`

**Step 1: Write failing real authoring tests**

Exercise the public commands against real files:

```sh
aikit skill overlay set <id> --scope global \
  --description "Prefer for cross-session work." \
  --guidance-file guidance.md --reviewed-against <revision>
aikit skill overlay show <id>
aikit skill overlay clear <id> --scope global
```

Prove stdin/file text is read exactly, mutually exclusive inputs are rejected, no-content writes are rejected, `--no-inherit` produces the reset marker, comments and unrelated TOML remain byte-stable, clear removes only that scope's overlay, and a failed parse leaves the file byte-identical.

**Step 2: Run and verify RED**

Run: `cargo test -p aikit-cli --test skill_overlay -- --nocapture`

Expected: FAIL because `aikit skill overlay` does not exist.

**Step 3: Implement surgical editing and commands**

Add format-preserving `set_skill_overlay` and `clear_skill_overlay` operations to `ProfileDocument` and `OverlayDocument`. Add `skill overlay {set,show,clear}` clap commands. Resolve scope through the same scope writer as activation, validate the capsule is a Skill, write once, refresh, apply a generation, and return structured JSON containing the effective ordered overlays and activation effects.

**Step 4: Run focused tests**

Run: `cargo test -p aikit-store --test edit && cargo test -p aikit-cli --test skill_overlay`

Expected: PASS.

**Step 5: Commit**

```sh
git add crates/aikit-store/src/edit.rs crates/aikit-cli/src/cli.rs crates/aikit-cli/src/main.rs crates/aikit-cli/src/app/mod.rs crates/aikit-store/tests/edit.rs crates/aikit-cli/tests/skill_overlay.rs
git commit -m "feat: author scoped skill usage overlays"
```

### Task 4: Compile Effective Skills for every harness surface

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/aikit-adapters/Cargo.toml`
- Modify: `crates/aikit-adapters/src/clients/agent_skills.rs`
- Modify: `crates/aikit-adapters/src/clients/claude.rs`
- Modify: `crates/aikit-adapters/src/clients/codex.rs`
- Modify: `crates/aikit-adapters/src/clients/broker.rs`
- Modify: `crates/aikit-cli/src/main.rs`
- Test: `crates/aikit-adapters/tests/agent_skills.rs`
- Test: `crates/aikit-adapters/tests/claude.rs`
- Test: `crates/aikit-adapters/tests/codex.rs`
- Test: `crates/aikit-adapters/tests/broker.rs`

**Step 1: Write failing adapter tests using real skill trees**

Create real temporary skills containing YAML frontmatter, body text, `references/`, `scripts/`, executable permissions, and `disable-model-invocation: true`. Assert that an Effective Skill:

- appends routing text to `description` without altering any other frontmatter key;
- retains `disable-model-invocation: true` byte-semantically;
- appends a clearly delimited `AIKit Skill Usage Overlay` section naming scope and provenance;
- tells the agent the augmentation is user-authoritative, contextual, additive, and more-specific guidance governs conflicts;
- preserves every companion file and executable bit;
- uses the original whole-directory link when no overlay exists;
- produces a different projection digest when guidance changes;
- reaches Claude, Codex, broker index descriptions, and `capabilities read` consistently.

**Step 2: Run and verify RED**

Run: `cargo test -p aikit-adapters --test agent_skills effective_skill -- --nocapture`

Expected: FAIL because projections only link/copy immutable payloads.

**Step 3: Implement effective materialization**

Parse and serialize only the generated frontmatter with `serde_yaml`; never rewrite the source. When overlays exist, emit each companion file with the requested link/copy mode and emit `SKILL.md` as `ProjectionItem::Write`. Append one framed section containing ordered overlay blocks. Use the same renderer for native adapters and broker reads; use effective descriptions in the broker index. Leave upstream invocation-policy fields untouched and refuse overlay fields that attempt to name them.

**Step 4: Run all adapter tests**

Run: `cargo test -p aikit-adapters --tests -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```sh
git add Cargo.toml Cargo.lock crates/aikit-adapters crates/aikit-cli/src/main.rs
git commit -m "feat: project effective skills with additive guidance"
```

### Task 5: Prove the production vertical slice and document operation

**Files:**
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/AGENT-HARNESS-INTEGRATION.md`
- Test: `crates/aikit-cli/tests/skill_overlay.rs`
- Test: `crates/aikit-cli/tests/acceptance.rs`

**Step 1: Add a black-box acceptance test**

Use a real temp AIKit home, real Git-backed project directory, real managed skill payload, real global profile, and real project overlay. Run separate CLI processes to enable globally, apply user and project guidance, publish a generation, and inspect both materialized Codex and Claude `SKILL.md` files byte-for-byte. Prove project reset behavior, unrelated-project isolation, source immutability, explanation provenance, stale-review warning, and truthful next-task/restart effects. Do not mock Git, filesystems, processes, resolution, or adapters.

**Step 2: Run and verify RED before any acceptance-specific implementation fix**

Run: `cargo test -p aikit-cli --test acceptance user_baseline_and_skill_usage_overlays -- --nocapture`

Expected: FAIL on the first missing integration behavior; fix only after observing it.

**Step 3: Document the finished contract**

Document user/profile precedence, overlay syntax and framing, reset semantics, update review warnings, source immutability, wrapper boundary for invocation changes, and harness reload boundaries. Avoid claiming live reload.

**Step 4: Run final verification**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Run: `cargo test --workspace --quiet`

Run: `git diff --check`

Expected: every command exits zero with no warnings or failures.

**Step 5: Commit**

```sh
git add docs crates/aikit-cli/tests
git commit -m "docs: define user baselines and effective skills"
```

### Task 6: Independent review and integration

**Files:**
- Review: every branch change against `73ea71a`

**Step 1: Request independent production review**

Review for global-scope leakage, profile precedence, format-preserving writes, source mutation, symlink escapes, YAML/frontmatter preservation, trust laundering, policy overrides, stale-review handling, broker parity, and real test quality.

**Step 2: Address every actionable finding through TDD**

For each defect, add a failing regression test, observe RED, implement, and observe GREEN.

**Step 3: Repeat final verification**

Run the four final verification commands from Task 5 and require a clean branch.

**Step 4: Integrate safely**

Fast-forward local `main` only after review and verification. Do not push without explicit authorization.
