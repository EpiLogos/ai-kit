//! Portable session topology.
//!
//! The canonical format is AIKit's own; tmuxp, tmuxinator and cmux JSON are export
//! targets. That only works if the compiled plan is genuinely mechanical — an
//! adapter must be able to walk it top to bottom, issuing one command per step,
//! without ever looking ahead to find out what a pane is going to split from.
//! These tests hold the compiler to that.

use aikit_core::context::Isolation;
use aikit_core::platform::MuxKind;
use aikit_core::session::{compile, Attach, Direction, Lifecycle, Placement, Restart, SessionSpec};

const FULL: &str = r#"
schema = 1
id = "payments-dev"
name = "Payments — development"
root = "~/work/payments"
backend = "auto"
attach = true
lifecycle = "persist"

[capabilities]
profiles = ["profile/code/rust"]
enable = ["skill/project/payments-domain"]

[[views]]
id = "code"
name = "Code"

[[views.panes]]
id = "editor"
command = ["nvim", "."]
focus = true

[[views.panes]]
id = "shell"
split_from = "editor"
direction = "right"
ratio = 0.35
restart = "if-exited"
command = ["zsh"]
cwd = "crates"

[views.panes.capabilities]
enable = ["script/test/cargo-nextest"]

[[views]]
id = "agents"

[[views.panes]]
id = "claude"
command = ["claude"]

[[views.panes]]
id = "logs"
split_from = "claude"
direction = "down"
ratio = 0.25
restart = "always"
command = ["tail", "-f", "log/dev.log"]

[task]
agent = "claude"
base = "main"
placement = "new-pane"
"#;

fn spec(src: &str) -> SessionSpec {
    SessionSpec::from_toml_str(src).unwrap_or_else(|e| panic!("spec should parse: {e}"))
}

fn error_code(src: &str) -> &'static str {
    let parsed = match SessionSpec::from_toml_str(src) {
        Ok(spec) => spec,
        Err(e) => return e.code(),
    };
    compile(&parsed)
        .err()
        .unwrap_or_else(|| panic!("expected compilation to fail"))
        .code()
}

/// A minimal spec with the given `[[views]]` body spliced in.
fn with_views(views: &str) -> String {
    format!(
        r#"
schema = 1
id = "test"
name = "Test"
{views}
"#
    )
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

#[test]
fn a_session_spec_parses_the_documented_toml() {
    let spec = spec(FULL);
    assert_eq!(spec.id, "payments-dev");
    assert_eq!(spec.name, "Payments — development");
    assert_eq!(
        spec.root.as_deref(),
        Some(std::path::Path::new("~/work/payments"))
    );
    assert_eq!(spec.backend.mux, None, "`auto` means the adapter decides");
    assert_eq!(spec.attach, Attach::Always);
    assert_eq!(spec.lifecycle, Lifecycle::Persist);
    assert_eq!(spec.capabilities.profiles.len(), 1);
    assert_eq!(spec.capabilities.enable.len(), 1);
    assert_eq!(spec.views.len(), 2);

    let code = &spec.views[0];
    assert_eq!(code.id, "code");
    assert_eq!(code.name.as_deref(), Some("Code"));
    assert_eq!(code.panes.len(), 2);

    let editor = &code.panes[0];
    assert_eq!(editor.command, vec!["nvim", "."]);
    assert!(editor.focus);
    assert_eq!(editor.split_from, None);
    assert_eq!(editor.restart, Restart::Never, "restart defaults to never");

    let shell = &code.panes[1];
    assert_eq!(shell.split_from.as_deref(), Some("editor"));
    assert_eq!(shell.direction, Some(Direction::Right));
    assert_eq!(shell.ratio, Some(0.35));
    assert_eq!(shell.restart, Restart::IfExited);
    assert_eq!(shell.cwd.as_deref(), Some(std::path::Path::new("crates")));
    assert_eq!(shell.capabilities.enable.len(), 1);
}

#[test]
fn a_task_declaration_parses_alongside_the_topology() {
    let task = spec(FULL).task.expect("the [task] table must parse");
    assert_eq!(task.agent, "claude");
    assert_eq!(task.base.as_deref(), Some("main"));
    assert_eq!(task.placement, Placement::NewPane);
}

#[test]
fn a_named_backend_pins_the_multiplexer() {
    let spec = spec(&with_views(
        "backend = \"tmux\"\n[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n",
    ));
    assert_eq!(spec.backend.mux, Some(MuxKind::Tmux));
}

#[test]
fn a_backend_table_carries_per_multiplexer_extensions() {
    let spec = spec(&with_views(
        r#"
[[views]]
id = "a"
[[views.panes]]
id = "p"

[backend]
kind = "cmux"

[backend.tmux]
status_style = "bg=default"

[backend.cmux]
workspace_group = "payments"
"#,
    ));

    assert_eq!(spec.backend.mux, Some(MuxKind::Cmux));
    assert_eq!(
        spec.backend
            .extensions
            .get("tmux")
            .and_then(|t| t.get("status_style"))
            .and_then(|v| v.as_str()),
        Some("bg=default")
    );
    assert_eq!(
        spec.backend
            .extensions
            .get("cmux")
            .and_then(|t| t.get("workspace_group"))
            .and_then(|v| v.as_str()),
        Some("payments")
    );
}

#[test]
fn an_unknown_backend_extension_is_warned_about_rather_than_rejected() {
    // A spec written for a multiplexer this build has never heard of must still
    // launch on the ones it has. Failing here would make every new adapter a
    // breaking change for everyone else's session files.
    let spec = spec(&with_views(
        r#"
[[views]]
id = "a"
[[views.panes]]
id = "p"

[backend.zellij]
layout = "compact"
"#,
    ));
    let plan = compile(&spec).unwrap();

    assert!(
        plan.warnings.iter().any(|w| w.contains("zellij")),
        "the ignored table must be named: {:?}",
        plan.warnings
    );
    assert!(!plan.backend_extensions.contains_key("zellij"));
    assert_eq!(plan.views.len(), 1, "the session still compiles");
}

#[test]
fn attach_accepts_a_bool_or_a_policy_name() {
    let make = |attach: &str| {
        spec(&with_views(&format!(
            "attach = {attach}\n[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n"
        )))
        .attach
    };
    assert_eq!(make("true"), Attach::Always);
    assert_eq!(make("false"), Attach::Never);
    assert_eq!(make("\"if-created\""), Attach::IfCreated);
}

#[test]
fn an_unsupported_schema_is_rejected_rather_than_guessed_at() {
    let src = "schema = 99\nid = \"a\"\nname = \"A\"\n";
    assert_eq!(
        SessionSpec::from_toml_str(src).unwrap_err().code(),
        "session.unsupported_schema"
    );
}

#[test]
fn a_malformed_document_reports_a_parse_error_with_a_stable_code() {
    assert_eq!(
        SessionSpec::from_toml_str("this is not toml =")
            .unwrap_err()
            .code(),
        "session.parse_error"
    );
}

// ---------------------------------------------------------------------------
// Compilation refuses incoherent topologies
// ---------------------------------------------------------------------------

#[test]
fn duplicate_view_ids_are_rejected() {
    assert_eq!(
        error_code(&with_views(
            "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n\
             [[views]]\nid = \"a\"\n[[views.panes]]\nid = \"q\"\n"
        )),
        "session.duplicate_view"
    );
}

#[test]
fn duplicate_pane_ids_within_one_view_are_rejected() {
    assert_eq!(
        error_code(&with_views(
            "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n\
             [[views.panes]]\nid = \"p\"\nsplit_from = \"p\"\n"
        )),
        "session.duplicate_pane"
    );
}

#[test]
fn the_same_pane_id_in_two_different_views_is_perfectly_fine() {
    let plan = compile(&spec(&with_views(
        "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"main\"\n\
         [[views]]\nid = \"b\"\n[[views.panes]]\nid = \"main\"\n",
    )))
    .unwrap();
    assert_eq!(plan.views.len(), 2);
    assert_eq!(plan.views[0].steps[0].pane, "main");
    assert_eq!(plan.views[1].steps[0].pane, "main");
}

#[test]
fn a_split_from_naming_no_such_pane_is_rejected() {
    let error = SessionSpec::from_toml_str(&with_views(
        "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n\
         [[views.panes]]\nid = \"q\"\nsplit_from = \"nowhere\"\n",
    ))
    .and_then(|s| compile(&s))
    .unwrap_err();

    assert_eq!(error.code(), "session.unknown_split_parent");
    assert_eq!(error.details().get("pane").map(String::as_str), Some("q"));
    assert_eq!(
        error.details().get("split_from").map(String::as_str),
        Some("nowhere")
    );
}

#[test]
fn a_split_cycle_is_rejected() {
    assert_eq!(
        error_code(&with_views(
            "[[views]]\nid = \"a\"\n\
             [[views.panes]]\nid = \"p\"\nsplit_from = \"q\"\n\
             [[views.panes]]\nid = \"q\"\nsplit_from = \"p\"\n"
        )),
        "session.split_cycle"
    );
}

#[test]
fn a_pane_that_splits_from_itself_is_a_cycle_too() {
    assert_eq!(
        error_code(&with_views(
            "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n\
             [[views.panes]]\nid = \"q\"\nsplit_from = \"q\"\n"
        )),
        "session.split_cycle"
    );
}

#[test]
fn a_ratio_outside_the_open_unit_interval_is_rejected() {
    for ratio in ["0.0", "1.0", "1.5", "-0.2"] {
        assert_eq!(
            error_code(&with_views(&format!(
                "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n\
                 [[views.panes]]\nid = \"q\"\nsplit_from = \"p\"\nratio = {ratio}\n"
            ))),
            "session.invalid_ratio",
            "ratio {ratio} must be rejected"
        );
    }
}

#[test]
fn a_ratio_inside_the_open_unit_interval_is_accepted() {
    for ratio in ["0.01", "0.5", "0.99"] {
        let plan = compile(&spec(&with_views(&format!(
            "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n\
             [[views.panes]]\nid = \"q\"\nsplit_from = \"p\"\nratio = {ratio}\n"
        ))))
        .unwrap();
        assert_eq!(
            plan.views[0].steps[1].split.as_ref().unwrap().ratio,
            Some(ratio.parse().unwrap())
        );
    }
}

#[test]
fn a_view_with_no_panes_is_rejected() {
    assert_eq!(
        error_code(&with_views("[[views]]\nid = \"empty\"\n")),
        "session.empty_view"
    );
}

#[test]
fn more_than_one_focused_pane_in_a_view_is_rejected() {
    assert_eq!(
        error_code(&with_views(
            "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\nfocus = true\n\
             [[views.panes]]\nid = \"q\"\nsplit_from = \"p\"\nfocus = true\n"
        )),
        "session.multiple_focus"
    );
}

#[test]
fn one_focused_pane_per_view_across_several_views_is_fine() {
    let plan = compile(&spec(&with_views(
        "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\nfocus = true\n\
         [[views]]\nid = \"b\"\n[[views.panes]]\nid = \"q\"\nfocus = true\n",
    )))
    .unwrap();
    assert_eq!(plan.views[0].focus.as_deref(), Some("p"));
    assert_eq!(plan.views[1].focus.as_deref(), Some("q"));
}

#[test]
fn a_view_where_every_pane_splits_from_another_has_no_root() {
    // Distinct from a cycle: the panes form a chain, but nothing starts it.
    assert_eq!(
        error_code(&with_views(
            "[[views]]\nid = \"a\"\n\
             [[views.panes]]\nid = \"p\"\nsplit_from = \"q\"\n\
             [[views.panes]]\nid = \"q\"\nsplit_from = \"r\"\n\
             [[views.panes]]\nid = \"r\"\nsplit_from = \"p\"\n"
        )),
        "session.split_cycle"
    );
}

#[test]
fn a_view_with_two_unsplit_panes_is_rejected() {
    // A multiplexer window starts with exactly one pane; a second root has no
    // creation command that could produce it.
    assert_eq!(
        error_code(&with_views(
            "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n[[views.panes]]\nid = \"q\"\n"
        )),
        "session.multiple_root_panes"
    );
}

#[test]
fn a_spec_with_no_views_at_all_is_rejected() {
    assert_eq!(
        error_code("schema = 1\nid = \"a\"\nname = \"A\"\n"),
        "session.no_views"
    );
}

// ---------------------------------------------------------------------------
// The plan is a creation script
// ---------------------------------------------------------------------------

#[test]
fn the_plan_can_be_executed_top_to_bottom_with_no_lookahead() {
    let plan = compile(&spec(FULL)).unwrap();
    for view in &plan.views {
        let mut created: Vec<&str> = Vec::new();
        for step in &view.steps {
            if let Some(split) = &step.split {
                assert!(
                    created.contains(&split.from.as_str()),
                    "{} splits from {}, which has not been created yet",
                    step.pane,
                    split.from
                );
            }
            created.push(&step.pane);
        }
    }
}

#[test]
fn the_first_step_of_every_view_is_its_root_and_has_no_split() {
    let plan = compile(&spec(FULL)).unwrap();
    for view in &plan.views {
        assert!(
            view.steps[0].split.is_none(),
            "the first pane of {} must be the view's root",
            view.id
        );
        assert!(view.steps[1..].iter().all(|s| s.split.is_some()));
    }
}

#[test]
fn panes_declared_before_their_parent_are_reordered_so_the_parent_comes_first() {
    let plan = compile(&spec(&with_views(
        "[[views]]\nid = \"a\"\n\
         [[views.panes]]\nid = \"child\"\nsplit_from = \"root\"\ndirection = \"down\"\n\
         [[views.panes]]\nid = \"root\"\n",
    )))
    .unwrap();

    let order: Vec<&str> = plan.views[0]
        .steps
        .iter()
        .map(|s| s.pane.as_str())
        .collect();
    assert_eq!(order, vec!["root", "child"]);
}

#[test]
fn independent_panes_keep_their_declared_order() {
    let plan = compile(&spec(&with_views(
        "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"root\"\n\
         [[views.panes]]\nid = \"z\"\nsplit_from = \"root\"\n\
         [[views.panes]]\nid = \"a\"\nsplit_from = \"root\"\n",
    )))
    .unwrap();

    let order: Vec<&str> = plan.views[0]
        .steps
        .iter()
        .map(|s| s.pane.as_str())
        .collect();
    assert_eq!(order, vec!["root", "z", "a"]);
}

#[test]
fn a_step_carries_everything_an_adapter_needs_to_create_the_pane() {
    let plan = compile(&spec(FULL)).unwrap();
    let shell = &plan.views[0].steps[1];

    assert_eq!(shell.view, "code");
    assert_eq!(shell.pane, "shell");
    assert_eq!(shell.command, vec!["zsh"]);
    assert_eq!(shell.cwd.as_deref(), Some(std::path::Path::new("crates")));
    assert_eq!(shell.restart, Restart::IfExited);
    assert!(!shell.focus);

    let split = shell.split.as_ref().unwrap();
    assert_eq!(split.from, "editor");
    assert_eq!(split.direction, Direction::Right);
    assert_eq!(split.ratio, Some(0.35));

    assert_eq!(
        shell.capabilities.enable[0].to_string(),
        "script/test/cargo-nextest",
        "a pane's own capability patch must survive into the plan"
    );
}

#[test]
fn a_split_with_no_direction_defaults_to_a_horizontal_neighbour() {
    let plan = compile(&spec(&with_views(
        "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n\
         [[views.panes]]\nid = \"q\"\nsplit_from = \"p\"\n",
    )))
    .unwrap();
    assert_eq!(
        plan.views[0].steps[1].split.as_ref().unwrap().direction,
        Direction::Right
    );
}

#[test]
fn the_plan_carries_the_sessions_own_capability_patch() {
    let plan = compile(&spec(FULL)).unwrap();
    assert_eq!(plan.capabilities.profiles.len(), 1);
    assert_eq!(plan.capabilities.enable.len(), 1);
    assert_eq!(plan.id, "payments-dev");
    assert_eq!(plan.attach, Attach::Always);
    assert_eq!(plan.lifecycle, Lifecycle::Persist);
}

#[test]
fn compiling_the_same_spec_twice_produces_the_same_plan() {
    let spec = spec(FULL);
    assert_eq!(compile(&spec).unwrap(), compile(&spec).unwrap());
}

// ---------------------------------------------------------------------------
// Tasks — isolation is opt-in
// ---------------------------------------------------------------------------

fn task_spec(body: &str) -> aikit_core::session::TaskSpec {
    let src = with_views(&format!(
        "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n\n[task]\n{body}"
    ));
    spec(&src).task.expect("the [task] table must parse")
}

#[test]
fn a_task_table_with_no_isolation_key_shares_the_working_tree() {
    let task = task_spec("agent = \"claude\"\n");
    assert_eq!(task.isolation, Isolation::Shared);
    assert!(!task.isolation.is_isolated());
}

#[test]
fn the_legacy_worktree_flag_still_maps_to_worktree_isolation() {
    // Older manifests were written when a worktree was the implied default. They
    // must keep meaning what they said, even though the default has changed.
    let task = task_spec("agent = \"claude\"\nworktree = true\n");
    assert_eq!(task.isolation, Isolation::Worktree);
}

#[test]
fn the_legacy_worktree_flag_set_to_false_means_shared() {
    let task = task_spec("agent = \"claude\"\nworktree = false\n");
    assert_eq!(task.isolation, Isolation::Shared);
}

#[test]
fn an_explicit_isolation_key_is_honoured() {
    assert_eq!(
        task_spec("agent = \"codex\"\nisolation = \"directory\"\n").isolation,
        Isolation::Directory
    );
    assert_eq!(
        task_spec("agent = \"codex\"\nisolation = \"worktree\"\n").isolation,
        Isolation::Worktree
    );
}

#[test]
fn an_isolation_key_that_contradicts_the_legacy_flag_is_rejected() {
    let src = with_views(
        "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n\n\
         [task]\nagent = \"claude\"\nisolation = \"shared\"\nworktree = true\n",
    );
    assert_eq!(
        SessionSpec::from_toml_str(&src).unwrap_err().code(),
        "task.isolation_conflict"
    );
}

#[test]
fn a_task_carries_its_own_capability_patch() {
    let task = task_spec(
        "agent = \"claude\"\nplacement = \"new-view\"\n\n\
         [task.capabilities]\nenable = [\"guidance/mode/research\"]\n",
    );
    assert_eq!(task.placement, Placement::NewView);
    assert_eq!(
        task.capabilities.enable[0].to_string(),
        "guidance/mode/research"
    );
}

#[test]
fn a_spec_with_no_task_table_declares_no_task() {
    assert!(spec(&with_views(
        "[[views]]\nid = \"a\"\n[[views.panes]]\nid = \"p\"\n"
    ))
    .task
    .is_none());
}
