//! Manifest parsing is the boundary between hand-written TOML and the domain model.
//! These tests exercise real manifest documents of the shape the specification uses.

use aikit_core::capsule::{Capsule, Kind, Maturity, SkillActivation, SkillFormat};
use aikit_core::effects::EffectClass;
use aikit_core::id::CapsuleId;
use aikit_core::AikitError;

const SCRIPT_MANIFEST: &str = r#"
schema = 1
id = "script/test/pytest-gate"
kind = "script"
name = "Pytest gate"
description = "Run the appropriate project test suite using the active Python environment."
maturity = "candidate"
tags = ["python", "test", "verification"]
platforms = ["linux", "macos"]
targets = ["shell", "claude-code", "codex"]

[[requires]]
id = "script/env/venv-detect"
version = ">=0.2"
reason = "Select the project Python environment."

[[conflicts]]
id = "script/test/pytest-direct"
reason = "Both export the pytest-gate command."

[effects]
filesystem = ["read:project"]
network = false
credentials = []
subprocess = true

[script]
entry = "payload/run.py"
interpreter = ["python3"]
cwd = "project"
mode = "foreground"
interactive = false
timeout = "10m"
exports = ["pytest-gate"]

[[args]]
name = "path"
label = "Test path"
type = "path"
position = 1
default_from = "project_root"
must_exist = true

[[args]]
name = "changed"
label = "Changed files only"
type = "bool"
flag = "--changed"
default = false

[provenance]
source = "harvested"
created_at = "2026-07-23T10:15:00Z"
session_id = "ses_01J0000000000000000000000"
source_event = "repeated-successful-command"
"#;

#[test]
fn parses_a_full_script_manifest() {
    let capsule = Capsule::from_toml_str(SCRIPT_MANIFEST).expect("manifest should parse");

    assert_eq!(
        capsule.id,
        CapsuleId::parse("script/test/pytest-gate").unwrap()
    );
    assert_eq!(capsule.kind, Kind::Script);
    assert_eq!(capsule.maturity, Maturity::Candidate);
    assert_eq!(capsule.tags, vec!["python", "test", "verification"]);

    let script = capsule.script().expect("script section present");
    assert_eq!(script.entry, "payload/run.py");
    assert_eq!(script.exports, vec!["pytest-gate"]);
    assert_eq!(script.timeout.unwrap().as_secs(), 600);

    assert_eq!(capsule.requires.len(), 1);
    assert_eq!(
        capsule.requires[0].id,
        CapsuleId::parse("script/env/venv-detect").unwrap()
    );
    assert_eq!(capsule.conflicts.len(), 1);

    assert_eq!(capsule.args.len(), 2);
    assert_eq!(capsule.args[0].name, "path");
    assert_eq!(capsule.args[0].position, Some(1));
    assert_eq!(capsule.args[1].flag.as_deref(), Some("--changed"));
}

#[test]
fn script_manifest_derives_effect_classes() {
    let capsule = Capsule::from_toml_str(SCRIPT_MANIFEST).unwrap();
    let classes = capsule.effects.classes();
    assert!(classes.contains(&EffectClass::ReadProject));
    assert!(classes.contains(&EffectClass::Subprocess));
    assert!(!classes.contains(&EffectClass::Network));
    assert!(!classes.contains(&EffectClass::WriteOutsideProject));
}

#[test]
fn kind_must_agree_with_the_id_prefix() {
    let bad = r#"
schema = 1
id = "script/test/thing"
kind = "skill"
name = "Thing"
description = "Mismatched kind."
"#;
    let err = Capsule::from_toml_str(bad).unwrap_err();
    assert_eq!(err.code(), "manifest.kind_mismatch");
}

#[test]
fn a_capsule_may_not_declare_its_own_trust() {
    let sneaky = r#"
schema = 1
id = "hook/gate/sneaky"
kind = "hook"
name = "Sneaky"
description = "Declares itself trusted."
trust = "trusted"

[hook]
entry = "payload/check"
events = ["PreToolUse"]
"#;
    let err = Capsule::from_toml_str(sneaky).unwrap_err();
    assert_eq!(err.code(), "manifest.trust_not_self_declarable");
}

#[test]
fn a_script_capsule_requires_a_script_section() {
    let missing = r#"
schema = 1
id = "script/test/thing"
kind = "script"
name = "Thing"
description = "No script table."
"#;
    let err = Capsule::from_toml_str(missing).unwrap_err();
    assert_eq!(err.code(), "manifest.missing_kind_section");
}

#[test]
fn an_unknown_schema_version_is_rejected_with_a_clear_code() {
    let future = r#"
schema = 99
id = "guidance/mode/research"
kind = "guidance"
name = "Research"
description = "From the future."

[guidance]
entry = "payload/guidance.md"
"#;
    let err = Capsule::from_toml_str(future).unwrap_err();
    assert_eq!(err.code(), "manifest.unsupported_schema");
}

#[test]
fn parses_a_hook_manifest_with_phase_ordering_and_failure_policy() {
    let src = r#"
schema = 1
id = "hook/gate/project-boundary"
kind = "hook"
name = "Project boundary gate"
description = "Prevent file writes outside the current project or task worktree."
maturity = "stable"

[hook]
entry = "payload/check"
events = ["PreToolUse"]
matcher = "Edit|Write"
phase = "gate"
order = 100
timeout = "2s"
failure = "closed"
serial = true

[hook.bypass]
allowed = true
reason_required = true
"#;
    let capsule = Capsule::from_toml_str(src).unwrap();
    let hook = capsule.hook().unwrap();
    assert_eq!(hook.events, vec!["PreToolUse"]);
    assert_eq!(hook.matcher.as_deref(), Some("Edit|Write"));
    assert_eq!(hook.order, 100);
    assert!(hook.serial);
    assert!(hook.bypass.allowed);
    assert!(hook.bypass.reason_required);
    assert_eq!(hook.timeout.unwrap().as_millis(), 2000);
}

#[test]
fn hooks_default_to_failing_closed_and_running_serially() {
    let src = r#"
schema = 1
id = "hook/gate/minimal"
kind = "hook"
name = "Minimal"
description = "Only the required fields."

[hook]
entry = "payload/check"
events = ["PreToolUse"]
"#;
    let capsule = Capsule::from_toml_str(src).unwrap();
    let hook = capsule.hook().unwrap();
    assert_eq!(hook.failure, aikit_core::capsule::FailurePolicy::Closed);
    assert!(hook.serial, "a hook must opt in to parallel execution");
    assert_eq!(hook.phase, aikit_core::capsule::HookPhase::Gate);
}

#[test]
fn parses_a_skill_manifest() {
    let src = r#"
schema = 1
id = "skill/rust/review"
kind = "skill"
name = "Rust review"
description = "Review Rust changes for correctness, unsafe boundaries, and API coherence."
maturity = "stable"
tags = ["rust", "review"]

[skill]
format = "agent-skill"
root = "payload"
export_name = "rust-review"
activation = "model-or-user"

[effects]
filesystem = ["read:project"]
network = false
subprocess = false
"#;
    let capsule = Capsule::from_toml_str(src).unwrap();
    let skill = capsule.skill().unwrap();
    assert_eq!(skill.format, SkillFormat::AgentSkill);
    assert_eq!(skill.export_name, "rust-review");
    assert_eq!(skill.activation, SkillActivation::ModelOrUser);
}

#[test]
fn guidance_carries_an_injection_budget() {
    let src = r#"
schema = 1
id = "guidance/mode/research"
kind = "guidance"
name = "Research mode"
description = "Prefer evidence gathering, explicit uncertainty, and source comparison."
maturity = "stable"

[guidance]
entry = "payload/guidance.md"
inject = ["SessionStart", "UserPromptSubmit"]
order = 200
token_budget = 900
"#;
    let capsule = Capsule::from_toml_str(src).unwrap();
    let guidance = capsule.guidance().unwrap();
    assert_eq!(guidance.order, 200);
    assert_eq!(guidance.token_budget, Some(900));
    assert_eq!(guidance.inject, vec!["SessionStart", "UserPromptSubmit"]);
}

#[test]
fn tool_capsules_describe_an_external_check_rather_than_an_installer() {
    let src = r#"
schema = 1
id = "tool/search/ripgrep"
kind = "tool"
name = "ripgrep"
description = "Fast recursive search."

[tool]
commands = ["rg"]
check = ["rg", "--version"]
minimum_version = "14"
"#;
    let capsule = Capsule::from_toml_str(src).unwrap();
    let tool = capsule.tool().unwrap();
    assert_eq!(tool.commands, vec!["rg"]);
    assert_eq!(tool.check, vec!["rg", "--version"]);
    assert_eq!(tool.minimum_version.as_deref(), Some("14"));
}

#[test]
fn capsule_id_round_trips_and_rejects_malformed_input() {
    let id = CapsuleId::parse("skill/rust/code-review").unwrap();
    assert_eq!(id.kind(), Kind::Skill);
    assert_eq!(id.to_string(), "skill/rust/code-review");
    assert_eq!(id.path(), "rust/code-review");
    assert_eq!(id.leaf(), "code-review");

    assert!(CapsuleId::parse("skill").is_err());
    assert!(CapsuleId::parse("nope/rust/thing").is_err());
    assert!(CapsuleId::parse("skill//thing").is_err());
    assert!(CapsuleId::parse("skill/rust/Code Review").is_err());
    assert!(CapsuleId::parse("skill/../etc/passwd").is_err());
}

#[test]
fn capsule_ids_order_deterministically_for_stable_hashing() {
    let mut ids = [
        CapsuleId::parse("script/test/b").unwrap(),
        CapsuleId::parse("skill/rust/a").unwrap(),
        CapsuleId::parse("script/test/a").unwrap(),
    ];
    ids.sort();
    let rendered: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
    assert_eq!(
        rendered,
        vec!["script/test/a", "script/test/b", "skill/rust/a"]
    );
}

#[test]
fn errors_expose_stable_machine_codes() {
    let err: AikitError = CapsuleId::parse("bogus").unwrap_err();
    assert_eq!(err.code(), "id.malformed");
    assert!(!err.to_string().is_empty());
}
