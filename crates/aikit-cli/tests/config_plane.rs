//! The owner side of the O:I configuration plane, exercised against the real
//! binary in a sandboxed `AIKIT_HOME`.
//!
//! These are the Gate-B specimen tests: the four verbs round-trip against
//! AIKit's own declared state, replay returns `no_op`, refusals are structured
//! `oi.config-error/v1` documents with non-zero exits, secret material is
//! refused, and the contribution validates against the frozen C0 JSON Schema.

use std::fs;
use std::path::Path;

use assert_cmd::cargo::cargo_bin;
use serde_json::{json, Value};
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A home with one registry capsule and one registry profile, plus a bound
/// project that can address them.
fn scene() -> (TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    write(
        &home
            .path()
            .join("registries/personal/capsules/script/demo/greet/manifest.toml"),
        r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "Greets for the config-plane tests."

[script]
entry = "payload/run.sh"
interpreter = ["/bin/sh"]
exports = ["greet"]
"#,
    );
    write(
        &home
            .path()
            .join("registries/personal/capsules/script/demo/greet/payload/run.sh"),
        "#!/bin/sh\necho hi\n",
    );
    write(
        &home
            .path()
            .join("registries/personal/profiles/team/coding.toml"),
        r#"schema = 1
id = "profile/team/coding"
description = "The coding profile."
enable = []
"#,
    );

    let output = std::process::Command::new(cargo_bin("aikit"))
        .args([
            "project",
            "bind",
            "demo",
            "--directory",
            project.path().to_str().unwrap(),
            "--no-default-skill-sets",
            "--json",
        ])
        .env("AIKIT_HOME", home.path())
        .current_dir(project.path())
        .output()
        .expect("binary runs");
    assert!(
        output.status.success(),
        "project bind failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    (home, project)
}

/// Run a config command. Every config surface answers with a bare JSON
/// document — success or `oi.config-error/v1` — and its own exit status.
fn run(home: &Path, project: &Path, args: &[&str]) -> (i32, Value) {
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(args)
        .env("AIKIT_HOME", home)
        .current_dir(project)
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout:?}"));
    (output.status.code().unwrap_or(-1), value)
}

// ---------------------------------------------------------------------------
// The contribution document
// ---------------------------------------------------------------------------

#[test]
fn contribution_is_a_bare_conforming_document() {
    let (home, project) = scene();
    let (code, value) = run(
        home.path(),
        project.path(),
        &["config-contribution", "--json"],
    );

    assert_eq!(code, 0);
    // Bare: no envelope anywhere. The read/operability plane separation is
    // structural, not a convention.
    assert!(
        value.get("ok").is_none(),
        "config-contribution must be bare"
    );
    assert_eq!(value["schema"], "oi.configuration-contribution/v1");
    assert_eq!(
        value["contract_revision"],
        "configuration-plane/contribution.1"
    );
    assert_eq!(value["owner"]["owner_ref"], "ai-kit");
    assert_eq!(value["owner"]["owner_kind"], "product");
    assert_eq!(
        value["owner"]["contribution_command"],
        json!(["aikit", "config-contribution", "--json"])
    );
    assert_eq!(value["availability"]["state"], "available");
    // A contribution never carries native axes: no declared/effective/active.
    let text = value.to_string();
    assert!(
        !text.contains("\"effective\""),
        "no disclosure axes in a contribution"
    );
    assert!(
        !text.contains("\"declared\""),
        "no disclosure axes in a contribution"
    );

    let digest = value["owner"]["reading_digest"].as_str().unwrap();
    assert_eq!(digest.len(), 64, "reading_digest is sha256 hex");

    let sections: Vec<&str> = value["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    // The owner's own sections, then the derived per-harness trust sections
    // (embedded-profile order).
    assert_eq!(
        sections,
        vec![
            "resolution",
            "skills",
            "models",
            "claude-code",
            "codex",
            "zcode"
        ]
    );
    for section in value["sections"].as_array().unwrap() {
        for setting in section["settings"].as_array().unwrap() {
            assert_eq!(setting["section_ref"], section["id"]);
            let (owner, part, _) = split_ref(setting["setting_ref"].as_str().unwrap());
            assert_eq!(owner, "ai-kit");
            assert_eq!(part, section["id"].as_str().unwrap());
        }
    }
}

/// The harness trust/permissions sections are the general pattern made
/// concrete: each embedded profile's `settings.trust-settings` declarations
/// surface as disclosure-only plane settings `ai-kit:<slug>:<key>`, so an
/// authored `oi.profile/v1` can carry the machine's trust posture. This is
/// the test the next harness satisfies by declaring its own settings.
#[test]
fn harness_trust_settings_surface_as_disclosure_only_plane_settings() {
    let (home, project) = scene();
    let (_, value) = run(
        home.path(),
        project.path(),
        &["config-contribution", "--json"],
    );

    let setting = |reference: &str| -> Value {
        value["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["settings"].as_array().unwrap().iter().cloned())
            .find(|s| s["setting_ref"] == reference)
            .unwrap_or_else(|| panic!("{reference} is disclosed"))
    };

    for reference in [
        "ai-kit:codex:projects.trust_level",
        "ai-kit:codex:home.trust_level",
        "ai-kit:claude-code:hooks.fs-guardrail",
        "ai-kit:zcode:hooks.fs-guardrail",
    ] {
        let disclosed = setting(reference);
        assert_eq!(disclosed["writable"], json!(false), "{reference}");
        assert_eq!(disclosed["profileable"], json!(true), "{reference}");
        assert_eq!(
            disclosed["operations"]["apply"],
            json!(false),
            "{reference}"
        );
        assert_eq!(disclosed["operations"]["plan"], json!(false), "{reference}");
    }

    let codex_project = setting("ai-kit:codex:projects.trust_level");
    assert_eq!(codex_project["value_schema"]["type"], "enum");
    assert_eq!(
        codex_project["value_schema"]["options"],
        json!([{ "value": "trusted" }])
    );
    assert_eq!(
        codex_project["allowed_scopes"],
        json!([{ "scope_kind": "project", "scope_ref": null }])
    );

    let codex_home = setting("ai-kit:codex:home.trust_level");
    assert_eq!(codex_home["value_schema"]["type"], "boolean");
    assert_eq!(
        codex_home["allowed_scopes"],
        json!([{ "scope_kind": "machine", "scope_ref": null }])
    );

    for reference in [
        "ai-kit:claude-code:hooks.fs-guardrail",
        "ai-kit:zcode:hooks.fs-guardrail",
    ] {
        let guardrail = setting(reference);
        assert_eq!(guardrail["value_schema"]["type"], "scalar");
        assert_eq!(
            guardrail["allowed_scopes"],
            json!([{ "scope_kind": "machine", "scope_ref": null }])
        );
        assert!(
            guardrail["native_ref"].as_str().unwrap().contains("config"),
            "the declaration names the harness-native config location"
        );
    }
}

fn split_ref(reference: &str) -> (&str, &str, &str) {
    let parts: Vec<&str> = reference.split(':').collect();
    assert_eq!(
        parts.len(),
        3,
        "setting_ref parses into exactly three parts"
    );
    (parts[0], parts[1], parts[2])
}

/// The contribution validates against the frozen C0 JSON Schema, vendored
/// verbatim from the O:I contract checkout.
#[test]
fn contribution_validates_against_the_frozen_schema() {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/oi.configuration-contribution-v1.schema.json");
    let schema_json: Value =
        serde_json::from_str(&fs::read_to_string(&schema_path).unwrap()).unwrap();
    let schema = jsonschema::validator_for(&schema_json).unwrap();

    let (home, project) = scene();
    let (_, value) = run(
        home.path(),
        project.path(),
        &["config-contribution", "--json"],
    );
    let errors: Vec<String> = schema.iter_errors(&value).map(|e| format!("{e}")).collect();
    assert!(errors.is_empty(), "schema violations: {errors:?}");
}

/// The effects the plane discloses are AIKit's own, and the resolution chain
/// honestly says a running session keeps its composition.
#[test]
fn session_restart_effects_are_disclosed_on_the_resolution_chain() {
    let (home, project) = scene();
    let (_, value) = run(
        home.path(),
        project.path(),
        &["config-contribution", "--json"],
    );

    let effect = |reference: &str| {
        value["sections"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["settings"].as_array().unwrap())
            .find(|s| s["setting_ref"] == reference)
            .unwrap()["effect"]
            .clone()
    };
    for reference in [
        "ai-kit:resolution:resolution.profiles",
        "ai-kit:skills:skills.capabilities",
    ] {
        let effect = effect(reference);
        assert_eq!(effect["kind"], "session-restart-required", "{reference}");
        assert!(
            effect["summary"].as_str().unwrap().contains("keep"),
            "the summary says what running sessions do: {effect}"
        );
    }
    // Model choice resolves per launch; there is no restart axis to claim.
    assert_eq!(effect("ai-kit:models:models.candidates")["kind"], "none");
}

// ---------------------------------------------------------------------------
// The four verbs
// ---------------------------------------------------------------------------

#[test]
fn profile_selection_round_trips_through_plan_apply_replay_and_reset() {
    let (home, project) = scene();
    let setting = "ai-kit:resolution:resolution.profiles";
    let scope = "project:demo";

    let (code, validation) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "validate",
            "--json",
            "--setting",
            setting,
            "--scope",
            scope,
            "--value",
            "\"profile/team/coding\"",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(validation["schema"], "oi.config-validation/v1");
    assert_eq!(validation["valid"], true);
    assert_eq!(validation["scope"]["scope_kind"], "project");
    assert_eq!(validation["scope"]["scope_ref"], "demo");

    let (code, plan) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "plan",
            "--json",
            "--setting",
            setting,
            "--scope",
            scope,
            "--value",
            "\"profile/team/coding\"",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(plan["schema"], "oi.config-plan/v1");
    assert!(plan["plan_id"].as_str().unwrap().starts_with("plan-"));
    assert_eq!(plan["plan_digest"].as_str().unwrap().len(), 64);
    assert_eq!(plan["expected_effect"]["kind"], "session-restart-required");

    let plan_path = project.path().join("plan.json");
    fs::write(&plan_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();

    // The declared state before: no profile declaration at all.
    let before = fs::read_to_string(project.path().join(".aikit/profile.toml")).unwrap_or_default();
    assert!(
        !before.contains("coding"),
        "nothing declared before apply: {before}"
    );

    let (code, receipt) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "apply",
            "--json",
            "--plan-file",
            plan_path.to_str().unwrap(),
            "--changeset",
            "cs-test-profile-1",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(receipt["schema"], "oi.config-receipt/v1");
    assert_eq!(receipt["outcome"], "applied");
    assert_eq!(receipt["operation"], "apply");
    assert_eq!(receipt["owner_ref"], "ai-kit");
    assert_eq!(receipt["changeset_id"], "cs-test-profile-1");
    assert_eq!(receipt["plan_digest"], plan["plan_digest"]);
    assert!(receipt["native_ref"]
        .as_str()
        .unwrap()
        .starts_with("aikit:config:receipts/"));
    let original_receipt = receipt["receipt_id"].as_str().unwrap().to_string();

    // plan → apply → state actually changed: the scope's own declaration file
    // now carries the native profile ref, and nothing else was copied.
    let after = fs::read_to_string(project.path().join(".aikit/profile.toml")).unwrap();
    assert!(
        after.contains("profiles = [\"profile/team/coding\"]"),
        "{after}"
    );

    // Replay under the same idempotency key: no_op naming the original, and
    // the owner must not re-execute (the receipt ids must differ).
    let (code, replay) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "apply",
            "--json",
            "--plan-file",
            plan_path.to_str().unwrap(),
            "--changeset",
            "cs-test-profile-1",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(replay["outcome"], "no_op");
    assert_eq!(replay["original_receipt_id"], json!(original_receipt));
    assert_ne!(replay["receipt_id"], json!(original_receipt));

    // Reset returns the owner baseline.
    let (code, reset_receipt) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "reset",
            "--json",
            "--setting",
            setting,
            "--scope",
            scope,
            "--changeset",
            "cs-test-reset-1",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(reset_receipt["operation"], "reset");
    assert_eq!(reset_receipt["outcome"], "applied");
    let cleared = fs::read_to_string(project.path().join(".aikit/profile.toml")).unwrap();
    assert!(
        !cleared.contains("coding"),
        "the profile declaration is gone: {cleared}"
    );

    // A reset replay is also a no_op.
    let (code, reset_replay) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "reset",
            "--json",
            "--setting",
            setting,
            "--scope",
            scope,
            "--changeset",
            "cs-test-reset-1",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(reset_replay["outcome"], "no_op");
}

#[test]
fn capability_toggles_round_trip_and_reach_the_active_generation() {
    let (home, project) = scene();
    let setting = "ai-kit:skills:skills.capabilities";

    let (code, plan) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "plan",
            "--json",
            "--setting",
            setting,
            "--scope",
            "project:demo",
            "--value",
            r#"{"script/demo/greet": true}"#,
        ],
    );
    assert_eq!(code, 0, "plan: {plan}");

    let plan_path = project.path().join("plan.json");
    fs::write(&plan_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    let (code, receipt) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "apply",
            "--json",
            "--plan-file",
            plan_path.to_str().unwrap(),
            "--changeset",
            "cs-test-toggle-1",
        ],
    );
    assert_eq!(code, 0, "apply: {receipt}");
    assert_eq!(receipt["outcome"], "applied");

    let after = fs::read_to_string(project.path().join(".aikit/profile.toml")).unwrap();
    assert!(
        after.contains("enable = [\"script/demo/greet\"]"),
        "{after}"
    );

    // The native resolution follows the declared change: the toggle pipeline
    // re-materialised the scope's generation, so the disclosure's effective
    // view now carries the capability.
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(["status", "--json"])
        .env("AIKIT_HOME", home.path())
        .current_dir(project.path())
        .output()
        .unwrap();
    let status: Value = serde_json::from_slice(&output.stdout).expect("status emits JSON");
    let active = status["data"]["active"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == "script/demo/greet");
    assert!(active, "the enabled capability is active after apply");

    // Reset clears the scope's toggle declarations.
    let (code, _) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "reset",
            "--json",
            "--setting",
            setting,
            "--scope",
            "project:demo",
        ],
    );
    assert_eq!(code, 0);
    let cleared = fs::read_to_string(project.path().join(".aikit/profile.toml")).unwrap();
    assert!(!cleared.contains("greet"), "{cleared}");
}

#[test]
fn default_skill_sets_round_trip_at_machine_scope() {
    let (home, project) = scene();
    let setting = "ai-kit:resolution:skill-sets.default";

    let (code, plan) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "plan",
            "--json",
            "--setting",
            setting,
            "--scope",
            "machine",
            "--value",
            r#"["central-skills", "extra"]"#,
        ],
    );
    assert_eq!(code, 0, "plan: {plan}");
    assert_eq!(
        plan["scope"],
        json!({"scope_kind": "machine", "scope_ref": null})
    );

    let plan_path = project.path().join("plan.json");
    fs::write(&plan_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    let (code, receipt) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "apply",
            "--json",
            "--plan-file",
            plan_path.to_str().unwrap(),
            "--changeset",
            "cs-test-sets-1",
        ],
    );
    assert_eq!(code, 0, "apply: {receipt}");

    let config = fs::read_to_string(home.path().join("config.toml")).expect("home config written");
    assert!(config.contains("default_skill_sets"), "{config}");

    let (code, _) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "reset",
            "--json",
            "--setting",
            setting,
            "--scope",
            "machine",
        ],
    );
    assert_eq!(code, 0);
}

// ---------------------------------------------------------------------------
// Structured refusals
// ---------------------------------------------------------------------------

#[test]
fn secret_material_is_refused_with_a_structured_error() {
    let (home, project) = scene();
    let (code, error) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "validate",
            "--json",
            "--setting",
            "ai-kit:models:models.credentials",
            "--scope",
            "world",
            "--value",
            "\"sk-ant-api03-super-secret-material\"",
        ],
    );

    assert_ne!(code, 0, "a material value must fail");
    assert_eq!(error["schema"], "oi.config-error/v1");
    assert_eq!(error["error_code"], "invalid_value");
    assert_eq!(error["setting_ref"], "ai-kit:models:models.credentials");
    // The message names the law and never echoes the material.
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("secret_reference"), "{message}");
    assert!(
        !message.contains("sk-ant"),
        "no material in the error: {message}"
    );
    let entire = error.to_string();
    assert!(
        !entire.contains("sk-ant"),
        "no material anywhere in the document"
    );
}

#[test]
fn disclosure_only_settings_refuse_every_write() {
    let (home, project) = scene();

    let (code, error) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "plan",
            "--json",
            "--setting",
            "ai-kit:models:models.candidates",
            "--scope",
            "world",
            "--value",
            "\"m/greet\"",
        ],
    );
    assert_ne!(code, 0, "plan on a disclosure-only setting must fail");
    assert_eq!(error["error_code"], "unsupported_setting", "{error}");
    assert_eq!(error["setting_ref"], "ai-kit:models:models.candidates");

    let (code, error) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "reset",
            "--json",
            "--setting",
            "ai-kit:models:models.candidates",
            "--scope",
            "world",
        ],
    );
    assert_ne!(code, 0, "reset on a disclosure-only setting must fail");
    assert_eq!(error["error_code"], "unsupported_setting", "{error}");

    // The credential setting validates a well-formed reference (validate is
    // disclosed) but still refuses to write.
    let (code, validation) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "validate",
            "--json",
            "--setting",
            "ai-kit:models:models.credentials",
            "--scope",
            "world",
            "--value",
            r#"{"secret_reference":{"ref":"credential:anthropic","present":true}}"#,
        ],
    );
    assert_eq!(code, 0, "a secret_reference is a valid shape: {validation}");
    assert_eq!(validation["valid"], true);

    let (code, error) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "plan",
            "--json",
            "--setting",
            "ai-kit:models:models.credentials",
            "--scope",
            "world",
            "--value",
            r#"{"secret_reference":{"ref":"credential:anthropic","present":true}}"#,
        ],
    );
    assert_ne!(code, 0);
    assert_eq!(error["error_code"], "unsupported_setting");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("aikit credential setup"),
        "the error points at the owner-native mechanism: {error}"
    );
}

#[test]
fn refusals_are_structured_and_exit_non_zero() {
    let (home, project) = scene();

    let cases: Vec<(Vec<&str>, &str)> = vec![
        (
            vec![
                "config",
                "plan",
                "--json",
                "--setting",
                "ai-kit:resolution:model.default",
                "--scope",
                "project:demo",
                "--value",
                "\"x\"",
            ],
            "unsupported_setting",
        ),
        (
            vec![
                "config",
                "validate",
                "--json",
                "--setting",
                "ai-kit:skills:skills.capabilities",
                "--scope",
                "galaxy:demo",
                "--value",
                "{}",
            ],
            "unknown_scope_kind",
        ),
        (
            vec![
                "config",
                "plan",
                "--json",
                "--setting",
                "ai-kit:resolution:skill-sets.default",
                "--scope",
                "project:demo",
                "--value",
                "[]",
            ],
            "unsupported_scope",
        ),
        (
            vec![
                "config",
                "plan",
                "--json",
                "--setting",
                "ai-kit:resolution:resolution.profiles",
                "--scope",
                "project:elsewhere",
                "--value",
                "\"profile/team/coding\"",
            ],
            "unsupported_scope",
        ),
        (
            vec![
                "config",
                "plan",
                "--json",
                "--setting",
                "ai-kit:resolution:resolution.profiles",
                "--scope",
                "project:demo",
                "--value",
                "\"not-a-profile-id\"",
            ],
            "validation_failed",
        ),
        (
            vec![
                "config",
                "plan",
                "--json",
                "--setting",
                "ai-kit:skills:skills.capabilities",
                "--scope",
                "project:demo",
                "--value",
                r#"{"script/no/such": true}"#,
            ],
            "validation_failed",
        ),
    ];

    for (args, expected_code) in cases {
        let (code, error) = run(home.path(), project.path(), &args);
        assert_ne!(code, 0, "{args:?} must fail");
        assert_eq!(error["schema"], "oi.config-error/v1", "{args:?}");
        assert_eq!(error["error_code"], expected_code, "{args:?}: {error}");
    }

    // An owner that answers "no" to a value still answers in-document under
    // validate: valid:false with the owner's own violation reasons.
    let (code, validation) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "validate",
            "--json",
            "--setting",
            "ai-kit:resolution:resolution.profiles",
            "--scope",
            "project:demo",
            "--value",
            "\"not-a-profile-id\"",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(validation["valid"], false);
    assert_eq!(validation["violations"][0]["code"], "invalid_profile_ref");
}

#[test]
fn a_tampered_or_foreign_plan_is_refused_without_executing() {
    let (home, project) = scene();

    let (_, mut plan) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "plan",
            "--json",
            "--setting",
            "ai-kit:resolution:resolution.profiles",
            "--scope",
            "project:demo",
            "--value",
            "\"profile/team/coding\"",
        ],
    );
    // Someone swaps the value after minting: the digest no longer pins it.
    plan["value"] = json!("profile/team/other");
    let plan_path = project.path().join("tampered.json");
    fs::write(&plan_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    let (code, error) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "apply",
            "--json",
            "--plan-file",
            plan_path.to_str().unwrap(),
            "--changeset",
            "cs-test-tamper-1",
        ],
    );
    assert_ne!(code, 0);
    assert_eq!(error["error_code"], "validation_failed", "{error}");

    // A plan naming an unknown schema is refused outright.
    plan["schema"] = json!("oi.config-plan/v9");
    let foreign_path = project.path().join("foreign.json");
    fs::write(&foreign_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    let (code, error) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "apply",
            "--json",
            "--plan-file",
            foreign_path.to_str().unwrap(),
            "--changeset",
            "cs-test-tamper-2",
        ],
    );
    assert_ne!(code, 0);
    assert_eq!(error["error_code"], "unsupported_schema", "{error}");

    // Nothing was executed: the project still declares no profile.
    let state = fs::read_to_string(project.path().join(".aikit/profile.toml")).unwrap_or_default();
    assert!(!state.contains("coding"), "nothing applied: {state}");
}

#[test]
fn agent_session_scope_writes_only_the_ambient_session_overlay() {
    let (home, project) = scene();

    // No ambient session: the session scope is not writable from here.
    let (code, error) = run(
        home.path(),
        project.path(),
        &[
            "config",
            "plan",
            "--json",
            "--setting",
            "ai-kit:resolution:resolution.profiles",
            "--scope",
            "agent-session:ses_other",
            "--value",
            "\"profile/team/coding\"",
        ],
    );
    assert_ne!(code, 0);
    assert_eq!(error["error_code"], "unsupported_scope", "{error}");

    // With the ambient session, the overlay is written and reset works.
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args([
            "config",
            "plan",
            "--json",
            "--setting",
            "ai-kit:resolution:resolution.profiles",
            "--scope",
            "agent-session:ses_01TESTSESSION",
            "--value",
            "\"profile/team/coding\"",
        ])
        .env("AIKIT_HOME", home.path())
        .env("AIKIT_SESSION_ID", "ses_01TESTSESSION")
        .current_dir(project.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let plan: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(output.status.success(), "ambient plan: {plan}");
    assert_eq!(plan["scope"]["scope_kind"], "agent-session");

    let plan_path = project.path().join("plan.json");
    fs::write(&plan_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    let (code, _) = run_with_session(
        home.path(),
        project.path(),
        &[
            "config",
            "apply",
            "--json",
            "--plan-file",
            plan_path.to_str().unwrap(),
            "--changeset",
            "cs-test-session-1",
        ],
        "ses_01TESTSESSION",
    );
    assert_eq!(code, 0);
}

fn run_with_session(home: &Path, project: &Path, args: &[&str], session: &str) -> (i32, Value) {
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(args)
        .env("AIKIT_HOME", home)
        .env("AIKIT_SESSION_ID", session)
        .current_dir(project)
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout:?}"));
    (output.status.code().unwrap_or(-1), value)
}
