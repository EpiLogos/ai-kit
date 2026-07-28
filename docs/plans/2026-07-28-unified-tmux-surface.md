# Unified tmux Surface Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make AIKit's 82% × 70% tmux popup one continuous palette/tree surface, with shared staged state and a collision-safe, live-verified tmux binding.

**Architecture:** Add a surface coordinator to `aikit-tui` above the existing pure palette and tree reducers. Refactor both terminal loops into resumable controllers, route one event stream and one terminal lifecycle through the active controller, and keep the CLI application service alive for the entire popup. Extend the existing Procedure-backed tmux installer with explicit key selection, collision inspection, reload, and live verification.

**Tech Stack:** Rust, Ratatui, Crossterm, tmux 3.6+, existing AIKit application service and Procedure runner.

---

### Task 1: Resumable tree interaction controller

**Files:**
- Modify: `crates/aikit-tui/src/tree_driver.rs`
- Test: `crates/aikit-tui/tests/tree_driver.rs`

**Step 1: Write the failing tests**

Add tests proving a `TreeController`:

```rust
#[test]
fn tree_controller_returns_to_the_palette_without_losing_navigation_state() {
    let mut controller = TreeController::new(tree(), request());
    controller.handle(key(KeyCode::Down)).unwrap();
    let selected = controller.state().selected;
    assert_eq!(controller.handle(ctrl('t')).unwrap(), TreeStep::Palette);
    assert_eq!(controller.state().selected, selected);
}
```

Also prove `Esc` dismisses tree-local prompts before returning to the palette,
and that stage/apply/effect outcomes still use the existing reducer.

**Step 2: Verify RED**

Run:

```sh
cargo test -p aikit-tui --test tree_driver tree_controller -- --nocapture
```

Expected: compile failure because `TreeController` and `TreeStep` do not exist.

**Step 3: Implement the minimal controller**

Move `filtering`, confirmation, edit prompt, help, pending key chords, click,
drag, and centring state out of `event_loop` locals into `TreeController`.
Expose:

```rust
pub enum TreeStep {
    Continue,
    Palette,
    Apply(Vec<CapsuleId>),
    Effect(TreeEffect),
    Activate(CapsuleId),
}

impl TreeController {
    pub fn draw(&self, frame: &mut Frame);
    pub fn handle(&mut self, event: PaletteEvent) -> Result<TreeStep>;
    pub fn state(&self) -> &TreeState;
    pub fn state_mut(&mut self) -> &mut TreeState;
}
```

Keep the old `event_loop` as a compatibility wrapper over the controller.

**Step 4: Verify GREEN**

Run:

```sh
cargo test -p aikit-tui --test tree_driver
```

Expected: all tree-driver tests pass.

**Step 5: Commit**

```sh
git add crates/aikit-tui/src/tree_driver.rs crates/aikit-tui/tests/tree_driver.rs
git commit -m "refactor: make tree interaction resumable"
```

### Task 2: Resumable palette controller and shared staged state

**Files:**
- Modify: `crates/aikit-tui/src/driver.rs`
- Modify: `crates/aikit-tui/src/app.rs`
- Modify: `crates/aikit-tui/src/staging.rs`
- Test: `crates/aikit-tui/tests/reducer.rs`
- Test: `crates/aikit-tui/tests/staging.rs`

**Step 1: Write the failing tests**

Prove a `PaletteController` can yield to the tree without consuming its
`AppState`, resume with the same query/cursor, and import tree staging:

```rust
assert_eq!(controller.handle(ctrl('t'))?, PaletteStep::Tree);
assert_eq!(controller.state().query, "deploy");
controller.replace_staged(tree_staged)?;
assert_eq!(controller.state().staged.toggles(), expected);
```

Prove scope and staged consequences are recomputed through the existing
`Effect::Stage`, not copied as display-only marks.

**Step 2: Verify RED**

Run:

```sh
cargo test -p aikit-tui --test reducer palette_controller -- --nocapture
```

Expected: compile failure because the controller API does not exist.

**Step 3: Implement the minimal controller**

Move the existing `AppState`/`Runtime` setup and per-event stepping into
`PaletteController`. Introduce `PaletteStep::{Continue, Tree, Outcome}` and a
typed staging replacement method. Preserve `event_loop` as a wrapper.

**Step 4: Verify GREEN**

Run:

```sh
cargo test -p aikit-tui --test reducer
cargo test -p aikit-tui --test staging
```

Expected: all reducer and staging tests pass.

**Step 5: Commit**

```sh
git add crates/aikit-tui/src/driver.rs crates/aikit-tui/src/app.rs crates/aikit-tui/src/staging.rs crates/aikit-tui/tests/reducer.rs crates/aikit-tui/tests/staging.rs
git commit -m "refactor: make palette interaction resumable"
```

### Task 3: One unified surface loop

**Files:**
- Create: `crates/aikit-tui/src/surface.rs`
- Modify: `crates/aikit-tui/src/lib.rs`
- Modify: `crates/aikit-tui/src/tree.rs`
- Modify: `crates/aikit-tui/src/render.rs`
- Test: `crates/aikit-tui/tests/surface.rs`
- Test: `crates/aikit-tui/tests/render.rs`

**Step 1: Write the failing functional tests**

Drive one `TestBackend` terminal and one scripted event source through:

```text
palette query -> Ctrl-T -> tree navigation -> Space -> Ctrl-T
-> staged review -> Ctrl-T -> same tree selection -> Esc -> palette -> Esc
```

Assert:

- the terminal loop did not return at either mode switch;
- palette query/cursor and tree selection survived;
- staging is identical in both modes;
- the rendered header and footer name the current mode and `Ctrl-T`;
- tree `Esc` returns to palette, while palette `Esc` closes.

**Step 2: Verify RED**

Run:

```sh
cargo test -p aikit-tui --test surface -- --nocapture
```

Expected: compile failure because the surface module does not exist.

**Step 3: Implement the surface coordinator**

Create `SurfaceController`, `SurfaceMode`, `SurfaceRequest`, and
`SurfaceOutcome`. Own a palette controller and tree controller under one
terminal setup/teardown. Synchronise typed toggles and scope at every mode
transition. Keep all I/O in the driver layer.

**Step 4: Verify GREEN and snapshots**

Run:

```sh
cargo test -p aikit-tui --test surface
cargo test -p aikit-tui --test render
```

Expected: all surface tests and snapshots pass without pending snapshots.

**Step 5: Commit**

```sh
git add crates/aikit-tui/src/surface.rs crates/aikit-tui/src/lib.rs crates/aikit-tui/src/tree.rs crates/aikit-tui/src/render.rs crates/aikit-tui/tests/surface.rs crates/aikit-tui/tests/render.rs crates/aikit-tui/tests/snapshots
git commit -m "feat: add unified palette and tree surface"
```

### Task 4: Keep one CLI service alive across the popup

**Files:**
- Modify: `crates/aikit-cli/src/ui.rs`
- Modify: `crates/aikit-cli/src/main.rs`
- Modify: `crates/aikit-cli/src/tree_build.rs`
- Test: `crates/aikit-cli/tests/acceptance.rs`

**Step 1: Write the failing real-binary PTY test**

Launch `aikit ui` against a temporary real AIKit home and catalog. Send real key
events that enter the tree, stage a capability, return to the palette, review,
and apply. Record the child PID before and after `Ctrl-T` and assert it is the
same process.

**Step 2: Verify RED**

Run:

```sh
cargo test -p aikit-cli --test acceptance unified_popup -- --nocapture
```

Expected: failure because current `Ctrl-T` tears down and re-enters another TUI.

**Step 3: Implement the CLI surface backend**

Replace the `open_palette`/`open_tree` recursion with `ui::run_surface`.
Handle set-management Procedures through callbacks/effects that refresh the
existing service and tree without closing the terminal. Preserve terminal
restoration before all run intents.

**Step 4: Verify GREEN**

Run:

```sh
cargo test -p aikit-cli --test acceptance unified_popup -- --nocapture
cargo test -p aikit-cli --test palette_run_intent
```

Expected: all targeted tests pass.

**Step 5: Commit**

```sh
git add crates/aikit-cli/src/ui.rs crates/aikit-cli/src/main.rs crates/aikit-cli/src/tree_build.rs crates/aikit-cli/tests/acceptance.rs
git commit -m "feat: host the unified surface on one service"
```

### Task 5: Collision-safe tmux installation and live verification

**Files:**
- Modify: `crates/aikit-cli/src/cli.rs`
- Modify: `crates/aikit-cli/src/mux_install.rs`
- Modify: `crates/aikit-cli/src/main.rs`
- Test: `crates/aikit-cli/tests/cli_parse.rs`
- Create: `crates/aikit-cli/tests/mux_install.rs`

**Step 1: Write the failing tests**

Cover:

- default key is `M-a`;
- `--key` changes the managed binding;
- an effective pre-existing binding is refused with `mux.key_conflict`;
- AIKit's own existing managed binding is idempotent;
- applying reloads a running isolated tmux server;
- live `list-keys` verification must show the exact `display-popup` command;
- no running server still succeeds with a disk-verification warning.

Use a real private tmux socket for reload/verification tests, not a scripted
runner.

**Step 2: Verify RED**

Run:

```sh
cargo test -p aikit-cli --test mux_install -- --nocapture
```

Expected: parse/behavior failures because key selection and live verification
do not exist.

**Step 3: Implement minimal production behavior**

Add `--key M-a` and explicit conflict replacement only if the CLI contract
provides a reviewed flag. Inspect effective root-table bindings before planning.
After the Procedure applies, source the config on a running server and query the
live binding. Return stable structured verification fields and warnings.

**Step 4: Verify GREEN**

Run:

```sh
cargo test -p aikit-cli --test cli_parse
cargo test -p aikit-cli --test mux_install -- --nocapture
cargo test -p aikit-adapters --test tmux_install
```

Expected: all installer and compatibility tests pass.

**Step 5: Commit**

```sh
git add crates/aikit-cli/src/cli.rs crates/aikit-cli/src/mux_install.rs crates/aikit-cli/src/main.rs crates/aikit-cli/tests/cli_parse.rs crates/aikit-cli/tests/mux_install.rs
git commit -m "feat: verify the live tmux popup binding"
```

### Task 6: Performance gates

**Files:**
- Create: `crates/aikit-tui/tests/performance.rs`
- Modify: `crates/aikit-tui/src/surface.rs`
- Modify: `docs/ARCHITECTURE.md`

**Step 1: Write the timing tests**

Measure release-build cold first frame, repeated warm first frames, and 5,000
document search steps using monotonic time. Separate setup/discovery from render
and report distributions, not one lucky observation.

**Step 2: Verify RED**

Run:

```sh
cargo test --release -p aikit-tui --test performance -- --nocapture
```

Expected: failure until the surface exposes the measured first-frame seam.

**Step 3: Implement and optimise only proven failures**

Expose a measurement seam without changing behavior. Reuse matcher and render
allocations where the measured tests require it. Keep architecture targets as:
cold `<150 ms`, warm `<60 ms`, search step `<16 ms`.

**Step 4: Verify GREEN**

Run:

```sh
cargo test --release -p aikit-tui --test performance -- --nocapture
```

Expected: all budgets pass and print sample counts and worst observed values.

**Step 5: Commit**

```sh
git add crates/aikit-tui/tests/performance.rs crates/aikit-tui/src/surface.rs docs/ARCHITECTURE.md
git commit -m "test: enforce popup responsiveness budgets"
```

### Task 7: Consolidated verification and real tmux smoke

**Files:**
- Modify as required by failures only.

**Step 1: Format and lint**

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

**Step 2: Run every test**

```sh
cargo test --workspace
cargo test --release -p aikit-tui --test performance -- --nocapture
```

**Step 3: Run a real isolated tmux smoke**

Build the real binary, create a private tmux server and temporary home, install
the binding there, invoke it through `send-keys M-a`, verify the popup pane
exists at the expected geometry, exercise `Ctrl-T`, then close it and verify the
source pane remains.

**Step 4: Inspect the final diff**

```sh
git diff --check
git status --short
git log --oneline main..HEAD
```

**Step 5: Request adversarial review and finish the branch**

Use `superpowers:requesting-code-review`, address all valid findings, rerun the
full gates, then use `superpowers:finishing-a-development-branch`.
