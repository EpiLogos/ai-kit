//! Argument forms, rendered straight from `[[args]]`.
//!
//! The form owns no notion of what a script accepts. Every type, every default,
//! every range and pattern comes from the manifest through `aikit_core::arg`, and
//! the argv the form produces is built by `aikit_core::arg::build_argv`. What this
//! module adds is the two things core cannot do: the *filesystem* checks a `path`
//! argument declares (`must_exist`, `path_kind`), and the presentation.
//!
//! The security property with a test of its own: a secret is masked in the field,
//! masked in the preview, and dropped from anything that could be replayed. There
//! is no code path in which the real value is formatted into a display string.

mod common;

use common::*;

use std::collections::BTreeMap;

use aikit_core::arg::{ArgType, ArgValue, DefaultSource};
use aikit_core::capsule::ExecMode;
use aikit_tui::backend::REDACTED;
use aikit_tui::form::{ArgForm, FormContext, RunPreview};

const SECRET: &str = "hunter2-do-not-print-me";

fn args_capsule(id: &str, args: &str) -> aikit_core::capsule::Capsule {
    manifest("script", id, args, "entry = \"payload/run.sh\"")
}

fn context() -> FormContext {
    FormContext::from_descriptor(&descriptor())
}

// ---------------------------------------------------------------------------
// Every declared type is rendered
// ---------------------------------------------------------------------------

#[test]
fn every_argument_type_in_the_manifest_becomes_a_field() {
    let capsule = args_capsule(
        "script/test/every-type",
        r#"
[[args]]
name = "text"
type = "string"

[[args]]
name = "where"
type = "path"

[[args]]
name = "jobs"
type = "integer"

[[args]]
name = "ratio"
type = "float"

[[args]]
name = "changed"
type = "bool"

[[args]]
name = "profile"
type = "enum"
choices = ["ci", "local"]

[[args]]
name = "suites"
type = "multiselect"
choices = ["unit", "integration"]

[[args]]
name = "timeout"
type = "duration"

[[args]]
name = "token"
type = "secret"

[[args]]
name = "env"
type = "key-value"
"#,
    );
    let form = ArgForm::new(&capsule, &context());
    let types: Vec<ArgType> = form.fields().iter().map(|f| f.spec.ty).collect();
    assert_eq!(
        types,
        vec![
            ArgType::String,
            ArgType::Path,
            ArgType::Integer,
            ArgType::Float,
            ArgType::Bool,
            ArgType::Enum,
            ArgType::Multiselect,
            ArgType::Duration,
            ArgType::Secret,
            ArgType::KeyValue,
        ]
    );
    for field in form.fields() {
        assert!(
            !field.type_hint().is_empty(),
            "{} has no rendering hint",
            field.spec.name
        );
    }
}

#[test]
fn required_and_optional_come_from_core_and_are_not_re_decided() {
    let capsule = args_capsule(
        "script/test/req",
        r#"
[[args]]
name = "path"
type = "path"
position = 1

[[args]]
name = "verbose"
type = "bool"
flag = "--verbose"
"#,
    );
    let form = ArgForm::new(&capsule, &context());
    for field in form.fields() {
        assert_eq!(field.required(), field.spec.is_required());
    }
    assert!(form.fields()[0].required(), "a bare positional is required");
    assert!(!form.fields()[1].required());
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

#[test]
fn a_literal_default_seeds_the_field() {
    let capsule = args_capsule(
        "script/test/defaults",
        r#"
[[args]]
name = "jobs"
type = "integer"
flag = "--jobs"
default = 4
"#,
    );
    let form = ArgForm::new(&capsule, &context());
    assert_eq!(form.fields()[0].input(), "4");
}

#[test]
fn a_context_derived_default_is_filled_from_the_context_rather_than_typed() {
    let capsule = args_capsule(
        "script/test/derived",
        r#"
[[args]]
name = "root"
type = "path"
position = 1
default_from = "project_root"

[[args]]
name = "session"
type = "string"
flag = "--session"
default_from = "session_id"
"#,
    );
    let descriptor = descriptor();
    let form = ArgForm::new(&capsule, &FormContext::from_descriptor(&descriptor));
    assert_eq!(form.fields()[0].input(), "/work/payments");
    assert_eq!(
        form.fields()[1].input(),
        descriptor.session_id.as_ref().unwrap().as_str()
    );
}

#[test]
fn a_context_derived_default_the_context_cannot_supply_leaves_the_field_empty() {
    let capsule = args_capsule(
        "script/test/derived",
        r#"
[[args]]
name = "branch"
type = "string"
flag = "--branch"
default_from = "git_branch"
"#,
    );
    // The palette does not run git; the application service supplies the branch.
    let form = ArgForm::new(&capsule, &context());
    assert_eq!(form.fields()[0].input(), "");

    let with_branch = context().with_default(DefaultSource::GitBranch, "feature/payments");
    let form = ArgForm::new(&capsule, &with_branch);
    assert_eq!(form.fields()[0].input(), "feature/payments");
}

// ---------------------------------------------------------------------------
// Validation, including the filesystem checks core cannot do
// ---------------------------------------------------------------------------

#[test]
fn a_range_violation_is_reported_on_the_field_with_cores_message() {
    let capsule = args_capsule(
        "script/test/range",
        r#"
[[args]]
name = "jobs"
type = "integer"
flag = "--jobs"
min = 1
max = 16
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    form.set_input(0, "32");
    assert!(!form.validate());
    let message = form.fields()[0].error().expect("a field error");
    assert!(message.contains("16"), "the message must name the bound: {message}");

    form.set_input(0, "8");
    assert!(form.validate());
    assert!(form.fields()[0].error().is_none());
}

#[test]
fn a_pattern_violation_is_reported_before_anything_runs() {
    let capsule = args_capsule(
        "script/test/pattern",
        r#"
[[args]]
name = "ticket"
type = "string"
flag = "--ticket"
pattern = "^[A-Z]+-[0-9]+$"
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    form.set_input(0, "nope");
    assert!(!form.validate());
    form.set_input(0, "PAY-91");
    assert!(form.validate());
}

#[test]
fn an_enum_only_accepts_a_declared_choice() {
    let capsule = args_capsule(
        "script/test/enum",
        r#"
[[args]]
name = "profile"
type = "enum"
flag = "--profile"
choices = ["ci", "local"]
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    form.set_input(0, "prod");
    assert!(!form.validate());
    form.set_input(0, "ci");
    assert!(form.validate());
}

#[test]
fn a_path_that_must_exist_is_checked_against_the_real_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report.txt");
    std::fs::write(&file, "hello").unwrap();

    let capsule = args_capsule(
        "script/test/paths",
        r#"
[[args]]
name = "input"
type = "path"
position = 1
must_exist = true
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());

    form.set_input(0, dir.path().join("absent.txt").to_string_lossy().as_ref());
    assert!(!form.validate());
    assert_eq!(form.fields()[0].error_code(), Some("arg.path_missing"));

    form.set_input(0, file.to_string_lossy().as_ref());
    assert!(form.validate(), "{:?}", form.fields()[0].error());
}

#[test]
fn a_directory_argument_refuses_a_file_and_a_file_argument_refuses_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report.txt");
    std::fs::write(&file, "hello").unwrap();

    let wants_dir = args_capsule(
        "script/test/dir",
        r#"
[[args]]
name = "out"
type = "path"
position = 1
must_exist = true
path_kind = "directory"
"#,
    );
    let mut form = ArgForm::new(&wants_dir, &context());
    form.set_input(0, file.to_string_lossy().as_ref());
    assert!(!form.validate());
    assert_eq!(form.fields()[0].error_code(), Some("arg.path_wrong_kind"));
    form.set_input(0, dir.path().to_string_lossy().as_ref());
    assert!(form.validate());

    let wants_file = args_capsule(
        "script/test/file",
        r#"
[[args]]
name = "input"
type = "path"
position = 1
must_exist = true
path_kind = "file"
"#,
    );
    let mut form = ArgForm::new(&wants_file, &context());
    form.set_input(0, dir.path().to_string_lossy().as_ref());
    assert!(!form.validate());
    form.set_input(0, file.to_string_lossy().as_ref());
    assert!(form.validate());
}

#[test]
fn a_missing_required_argument_stops_the_form_before_it_produces_an_intent() {
    let capsule = args_capsule(
        "script/test/required",
        r#"
[[args]]
name = "input"
type = "path"
position = 1
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    assert!(!form.validate());
    let error = form
        .intent(&capsule, &descriptor())
        .expect_err("an incomplete form has no intent");
    assert_eq!(error.code(), "arg.missing_required");
}

// ---------------------------------------------------------------------------
// Interaction
// ---------------------------------------------------------------------------

#[test]
fn space_flips_a_boolean_field() {
    let capsule = args_capsule(
        "script/test/bools",
        r#"
[[args]]
name = "changed"
type = "bool"
flag = "--changed"
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    assert_eq!(form.fields()[0].input(), "false");
    form.activate(0);
    assert_eq!(form.fields()[0].input(), "true");
    form.activate(0);
    assert_eq!(form.fields()[0].input(), "false");
}

#[test]
fn space_cycles_an_enum_through_its_declared_choices_and_wraps() {
    let capsule = args_capsule(
        "script/test/enum",
        r#"
[[args]]
name = "profile"
type = "enum"
flag = "--profile"
choices = ["ci", "local", "staging"]
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    assert_eq!(form.fields()[0].input(), "ci");
    form.activate(0);
    assert_eq!(form.fields()[0].input(), "local");
    form.activate(0);
    assert_eq!(form.fields()[0].input(), "staging");
    form.activate(0);
    assert_eq!(form.fields()[0].input(), "ci");
}

#[test]
fn space_walks_a_multiselect_turning_each_choice_on_as_it_passes() {
    let capsule = args_capsule(
        "script/test/multi",
        r#"
[[args]]
name = "suites"
type = "multiselect"
flag = "--suite"
repeatable = true
choices = ["unit", "integration", "e2e"]
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    assert_eq!(form.fields()[0].input(), "");
    form.activate(0);
    assert_eq!(form.fields()[0].input(), "unit");
    form.activate(0);
    assert_eq!(form.fields()[0].input(), "unit,integration");
    form.activate(0);
    assert_eq!(form.fields()[0].input(), "unit,integration,e2e");
    // Round the loop again and the first choice comes off.
    form.activate(0);
    assert_eq!(form.fields()[0].input(), "integration,e2e");
}

#[test]
fn typing_and_backspace_edit_the_focused_field_only() {
    let capsule = args_capsule(
        "script/test/two",
        r#"
[[args]]
name = "first"
type = "string"
flag = "--first"

[[args]]
name = "second"
type = "string"
flag = "--second"
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    for c in "abc".chars() {
        form.input_char(c);
    }
    form.backspace();
    assert_eq!(form.fields()[0].input(), "ab");
    assert_eq!(form.fields()[1].input(), "");

    form.focus_next();
    form.input_char('z');
    assert_eq!(form.fields()[0].input(), "ab");
    assert_eq!(form.fields()[1].input(), "z");

    form.focus_previous();
    assert_eq!(form.focused(), 0);
}

// ---------------------------------------------------------------------------
// argv comes from core
// ---------------------------------------------------------------------------

#[test]
fn the_produced_argv_is_exactly_what_core_builds_from_the_manifest() {
    let capsule = args_capsule(
        "script/test/argv",
        r#"
[[args]]
name = "second"
type = "string"
position = 2

[[args]]
name = "opt"
type = "string"
flag = "--opt"

[[args]]
name = "first"
type = "string"
position = 1
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    form.set_input(0, "b");
    form.set_input(1, "c");
    form.set_input(2, "a");
    assert!(form.validate());

    let intent = form.intent(&capsule, &descriptor()).unwrap();
    let values = form.values().unwrap();
    assert_eq!(
        intent.argv().unwrap(),
        aikit_core::arg::build_argv(&capsule.args, &values).unwrap()
    );
    assert_eq!(intent.argv().unwrap(), vec!["a", "b", "--opt", "c"]);
}

#[test]
fn a_repeatable_flag_emits_once_per_selected_choice() {
    let capsule = args_capsule(
        "script/test/multi",
        r#"
[[args]]
name = "suites"
type = "multiselect"
flag = "--suite"
repeatable = true
choices = ["unit", "integration"]
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    form.set_input(0, "unit,integration");
    assert!(form.validate());
    let intent = form.intent(&capsule, &descriptor()).unwrap();
    assert_eq!(
        intent.argv().unwrap(),
        vec!["--suite", "unit", "--suite", "integration"]
    );
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

#[test]
fn a_secret_is_masked_in_its_own_field() {
    let capsule = args_capsule(
        "script/test/secret",
        r#"
[[args]]
name = "token"
type = "secret"
flag = "--token"
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    form.set_input(0, SECRET);
    let shown = form.fields()[0].display();
    assert!(!shown.contains(SECRET), "the field printed the secret: {shown}");
    assert!(!shown.contains("hunter2"));
    assert_eq!(shown.chars().count(), SECRET.chars().count());
}

#[test]
fn a_secret_never_appears_anywhere_in_the_run_preview() {
    let capsule = args_capsule(
        "script/test/secret",
        r#"
[[args]]
name = "token"
type = "secret"
flag = "--token"

[[args]]
name = "target"
type = "string"
position = 1
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    form.set_input(0, SECRET);
    form.set_input(1, "production");
    assert!(form.validate());

    let intent = form.intent(&capsule, &descriptor()).unwrap();
    let preview = RunPreview::of(&capsule, &intent, &context());
    let text = preview.text();

    assert!(!text.contains(SECRET), "the preview printed the secret:\n{text}");
    assert!(!text.contains("hunter2"));
    assert!(text.contains(REDACTED), "the preview must show that a secret is there:\n{text}");
    assert!(text.contains("production"), "non-secret arguments are still shown:\n{text}");
}

#[test]
fn the_preview_shows_everything_the_specification_lists() {
    let capsule = manifest(
        "script",
        "script/test/full",
        r#"
[effects]
network = true
subprocess = true

[[args]]
name = "target"
type = "string"
position = 1
"#,
        r#"entry = "payload/run.sh"
mode = "capture"
cwd = "project"
exports = ["fullrun"]

[script.env]
RUST_LOG = "debug""#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    form.set_input(0, "production");
    assert!(form.validate());
    let intent = form.intent(&capsule, &descriptor()).unwrap();
    let text = RunPreview::of(&capsule, &intent, &context()).text();

    for expected in [
        "Command",
        "fullrun",
        "Arguments",
        "production",
        "Working directory",
        "/work/payments",
        "Context",
        "payments",
        "Environment",
        "RUST_LOG=debug",
        "Effects",
        "network",
        "Mode",
        "capture",
    ] {
        assert!(text.contains(expected), "the preview is missing `{expected}`:\n{text}");
    }
}

#[test]
fn the_preview_says_when_a_mode_will_hand_over_the_terminal() {
    let capsule = manifest(
        "script",
        "script/test/fg",
        "",
        "entry = \"payload/run.sh\"\nmode = \"foreground\"",
    );
    let form = ArgForm::new(&capsule, &context());
    let intent = form.intent(&capsule, &descriptor()).unwrap();
    assert_eq!(intent.mode, ExecMode::Foreground);
    let text = RunPreview::of(&capsule, &intent, &context()).text();
    assert!(
        text.contains("the palette closes first"),
        "a mode that takes the terminal must say so:\n{text}"
    );
}

#[test]
fn an_intent_kept_for_repetition_carries_no_secret_material() {
    let capsule = args_capsule(
        "script/test/secret",
        r#"
[[args]]
name = "token"
type = "secret"
flag = "--token"
required = true

[[args]]
name = "target"
type = "string"
position = 1
"#,
    );
    let mut form = ArgForm::new(&capsule, &context());
    form.set_input(0, SECRET);
    form.set_input(1, "production");
    assert!(form.validate());

    let intent = form.intent(&capsule, &descriptor()).unwrap();
    assert!(intent.has_secrets());

    let repeatable = intent.without_secrets();
    assert!(!repeatable.has_secrets());
    assert_eq!(
        repeatable.values.get("token"),
        None,
        "a secret must not survive into the recent list"
    );
    assert_eq!(
        repeatable.values.get("target"),
        Some(&ArgValue::String("production".into()))
    );
    assert_eq!(
        repeatable.argv().unwrap_err().code(),
        "arg.missing_required",
        "repeating a run that needed a secret must ask for it again"
    );
}

#[test]
fn an_unreviewed_script_produces_an_intent_that_demands_confirmation() {
    let capsule = args_capsule("script/test/plain", "");
    let form = ArgForm::new(&capsule, &context());
    let intent = form
        .intent_with_confirmation(&capsule, &descriptor(), true)
        .unwrap();
    assert!(intent.requires_confirmation);
    assert!(!form.intent(&capsule, &descriptor()).unwrap().requires_confirmation);
}

#[test]
fn env_overrides_from_the_manifest_reach_the_intent_unchanged() {
    let capsule = manifest(
        "script",
        "script/test/env",
        "",
        "entry = \"payload/run.sh\"\n\n[script.env]\nRUST_LOG = \"debug\"\nCI = \"1\"",
    );
    let form = ArgForm::new(&capsule, &context());
    let intent = form.intent(&capsule, &descriptor()).unwrap();
    let expected: BTreeMap<String, String> = [
        ("CI".to_string(), "1".to_string()),
        ("RUST_LOG".to_string(), "debug".to_string()),
    ]
    .into_iter()
    .collect();
    assert_eq!(intent.env, expected);
}
