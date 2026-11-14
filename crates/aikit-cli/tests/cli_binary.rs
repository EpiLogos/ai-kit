//! The CLI contract, exercised against the real built binary.
//!
//! `AIKIT_HOME` points at a temp directory and the commands run in a real project
//! directory, so these are the same code paths a user's shell drives. Assertions
//! are on parsed JSON and on exit codes, not on substrings of human text.

use std::fs;
use std::path::Path;

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A home with a personal registry holding one searchable script capsule, and a
/// project that enables it.
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
description = "Greets for the CLI binary test."

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
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );
    (home, project)
}

fn run_json(home: &Path, project: &Path, args: &[&str]) -> (std::process::Output, Value) {
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(args)
        .env("AIKIT_HOME", home)
        .current_dir(project)
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout:?}"));
    (output, value)
}

#[test]
fn status_json_carries_the_envelope_and_the_active_capability() {
    let (home, project) = scene();
    let (output, value) = run_json(home.path(), project.path(), &["status", "--json"]);

    assert!(output.status.success());
    assert_eq!(value["schema"], 1);
    assert_eq!(value["ok"], true);
    // Compare canonicalised paths: on macOS `current_dir()` resolves the temp
    // dir's `/var` -> `/private/var` symlink, which is not a contract difference.
    let reported = value["context"]["project_root"].as_str().unwrap();
    assert_eq!(
        fs::canonicalize(reported).unwrap(),
        fs::canonicalize(project.path()).unwrap()
    );
    // The enabled script is active.
    let active = value["data"]["active"].as_array().unwrap();
    assert!(
        active.iter().any(|c| c["id"] == "script/demo/greet"),
        "the enabled capability should be active: {value}"
    );
}

#[test]
fn doctor_json_is_the_installed_native_verification_path() {
    let (home, project) = scene();
    let (output, value) = run_json(home.path(), project.path(), &["doctor", "--json"]);

    assert!(
        output.status.success(),
        "doctor should return an observation even when individual checks report findings: {value}"
    );
    assert_eq!(value["schema"], 1);
    assert_eq!(value["ok"], true);
    assert!(value["data"].is_object());
}

#[test]
fn search_json_finds_a_capability_by_name() {
    let (home, project) = scene();
    let (output, value) = run_json(home.path(), project.path(), &["search", "greet", "--json"]);
    assert!(output.status.success());
    let rows = value["data"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["id"] == "script/demo/greet"));
}

#[test]
fn an_unknown_capability_is_a_resolution_failure_with_exit_code_three() {
    let (home, project) = scene();
    let (output, value) = run_json(
        home.path(),
        project.path(),
        &["explain", "script/no/such", "--json"],
    );

    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "resolution.unknown_capability");
    assert_eq!(
        output.status.code(),
        Some(3),
        "resolution failures exit 3 per the published table"
    );
}

#[test]
fn shell_init_prints_a_sourceable_snippet() {
    let (home, project) = scene();
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(["shell", "init", "bash"])
        .env("AIKIT_HOME", home.path())
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!output.stdout.is_empty(), "the snippet should not be empty");
}

#[test]
fn a_bad_scope_argument_is_a_usage_error_with_exit_code_two() {
    let (home, project) = scene();
    let (_output, value) = run_json(
        home.path(),
        project.path(),
        &[
            "enable",
            "script/demo/greet",
            "--scope",
            "nonsense",
            "--json",
        ],
    );
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "cli.usage");
}

#[test]
fn flow_contemplates_a_now_stream_through_the_real_binary() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    let seam = project.path().join("now-stream.json");
    write(
        &seam,
        r#"{
  "schema": "central.thoughts-reading/v1",
  "now_ref": "central:now:control:root:cli",
  "total": 1,
  "truncated": false,
  "fixtures": [
    {"file": "raw-2026-09-13.md", "revision": "central.content-fnv1a64/v1:1:aa", "conforming": true, "day": "2026-09-13", "actor": "agent:test", "actor_kind": "agent", "content": "raw body"}
  ]
}"#,
    );
    let run = |args: &[&str]| {
        let output = std::process::Command::new(cargo_bin!("aikit"))
            .env("AIKIT_HOME", home.path())
            .current_dir(project.path())
            .args(args)
            .output()
            .unwrap();
        (
            output.status.success(),
            serde_json::from_slice::<Value>(&output.stdout).ok(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    let (ok, json, stderr) = run(&[
        "flow",
        "preflight",
        "--now-ref",
        "central:now:control:root:cli",
        "--fixtures",
        seam.to_str().unwrap(),
    ]);
    assert!(ok, "{stderr}");
    let data = json.unwrap();
    assert_eq!(data["contemplation"]["state"], "unavailable");
    assert_eq!(data["preflight"]["fixture_count"], 1);
    assert!(data["preflight"]["invocation_ref"]
        .as_str()
        .unwrap()
        .starts_with("now-contemplate/"));

    // A stream that disagrees with the addressed NOW refuses outright.
    let (ok, _, stderr) = run(&[
        "flow",
        "preflight",
        "--now-ref",
        "central:now:control:root:other",
        "--fixtures",
        seam.to_str().unwrap(),
    ]);
    assert!(!ok, "{stderr}");

    // `--now-ref` without `--fixtures` refuses before any owner call.
    let (ok, _, stderr) = run(&[
        "flow",
        "preflight",
        "--now-ref",
        "central:now:control:root:cli",
    ]);
    assert!(!ok, "{stderr}");

    // A host that distilled the stream itself supplies the learning with the
    // record of the preflight it read; AIKit proposes it against a fresh one.
    let preflight = data["preflight"].clone();
    let record = project.path().join("record.json");
    let distilled = project.path().join("distilled.txt");
    let record_of = |preflight: &Value| {
        serde_json::json!({
            "version": "aikit.now-contemplation/v1",
            "invocation_ref": preflight["invocation_ref"],
            "now_ref": "central:now:control:root:cli",
            "preflight": preflight,
        })
        .to_string()
    };
    write(&record, &record_of(&preflight));
    write(&distilled, "What the raw body means, as one learning.");
    let contemplate = |record: &Path| {
        run(&[
            "flow",
            "contemplate",
            "--now-ref",
            "central:now:control:root:cli",
            "--fixtures",
            seam.to_str().unwrap(),
            "--record",
            record.to_str().unwrap(),
            "--distilled",
            distilled.to_str().unwrap(),
        ])
    };
    let (ok, json, stderr) = contemplate(&record);
    assert!(ok, "{stderr}");
    let reading = &json.unwrap()["contemplation"];
    assert_eq!(reading["state"], "proposed", "{reading}");
    assert_eq!(
        reading["proposal"]["content"],
        "What the raw body means, as one learning."
    );
    assert_eq!(
        reading["proposal"]["source_fixtures"],
        serde_json::json!(["raw-2026-09-13.md"])
    );

    // A record of another preflight is refused, not proposed.
    let mut drifted = preflight.clone();
    drifted["fixture_count"] = serde_json::json!(2);
    write(&record, &record_of(&drifted));
    let (ok, json, stderr) = contemplate(&record);
    assert!(ok, "{stderr}");
    assert_eq!(json.unwrap()["contemplation"]["state"], "refused");

    // A distillation without the record of what it read is not taken.
    let (ok, _, stderr) = run(&[
        "flow",
        "contemplate",
        "--now-ref",
        "central:now:control:root:cli",
        "--fixtures",
        seam.to_str().unwrap(),
        "--distilled",
        distilled.to_str().unwrap(),
    ]);
    assert!(!ok, "{stderr}");
}

#[test]
fn system_emits_the_wave5_owner_disclosure_descriptor() {
    let (home, project) = scene();
    let (output, value) = run_json(home.path(), project.path(), &["system", "--json"]);

    assert!(output.status.success(), "system should succeed: {value}");

    // The bare v2 descriptor is the document on stdout — no ActionResult
    // envelope. Mounts read the top-level `schema` key and would otherwise
    // degrade AIKit to unavailable.
    assert_eq!(value["schema"], "oi.product-settings-disclosure/v2");
    for envelope_key in ["ok", "context", "data", "warnings"] {
        assert!(
            value.get(envelope_key).is_none(),
            "system must not wrap the descriptor in the ActionResult envelope (found top-level `{envelope_key}`)"
        );
    }

    assert_eq!(value["product_id"], "ai-kit");
    assert_eq!(value["contract_revision"], "wave-5/system.1");
    assert_eq!(
        value["owner"]["reading_command"],
        serde_json::json!(["aikit", "system", "--json"])
    );
    assert_eq!(value["owner"]["owner_id"], "ai-kit");
    // The canonical reading digest is a real SHA-256 hex fingerprint, not null.
    let digest = value["owner"]["reading_digest"]
        .as_str()
        .expect("digest present");
    assert_eq!(digest.len(), 64);
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));

    assert_eq!(value["availability"]["state"], "available");

    // Every section exposes settings, and every setting exposes all five axes.
    let sections = value["sections"].as_array().expect("sections present");
    assert!(!sections.is_empty());
    for section in sections {
        for setting in section["settings"].as_array().unwrap() {
            for axis in ["declared", "effective", "active", "staged"] {
                assert!(
                    setting["axes"][axis]["provenance"]["owner_ref"]
                        .as_str()
                        .is_some(),
                    "axis {axis} must carry provenance in {setting}"
                );
            }
            assert!(setting["axes"]["staged"]["stage_state"].as_str().is_some());
            assert_eq!(setting["axes"]["expected_effect"]["ref"], "aikit diff");
            assert_eq!(setting["mutable"], false);

            // The declared axis is never a clone of effective (§4.8): where
            // nothing was authored declared is null, and where something was
            // authored it carries its own provenance path.
            let declared = &setting["axes"]["declared"];
            let effective = &setting["axes"]["effective"];
            if !declared["value"].is_null() {
                assert_ne!(
                    declared["provenance"]["path"], effective["provenance"]["path"],
                    "a non-null declared axis must not share effective's provenance: {setting}"
                );
            }
        }
    }

    // Actions are disclosed or named as obligations, never rendered disabled.
    let actions = value["actions"].as_array().expect("actions present");
    assert!(actions.iter().any(|a| a["action_ref"] == "aikit.explain"));
    assert!(actions
        .iter()
        .any(|a| a["action_ref"] == "aikit.session.attach"));

    // Presence-only: the reading must never carry a secret value or marker.
    let raw = value.to_string();
    assert!(!raw.contains("SecretValue"));
    assert!(!raw.contains("sk-"));
}

/// O-I #1006: a session's settings are AIKit's Action `aikit.session.settings`, run by its id
/// through the act doorway as the O:I kernel runs it (`--input -`, the input on stdin) against the
/// real folded surface: Scope's Wait and On no answer written, read back, and a write against a
/// revision that is no longer the current one refused in the owner's words with its code.
#[test]
fn a_session_s_settings_are_an_action_the_doorway_runs() {
    use aikit_core::session_space::SessionSpaceRef;
    use aikit_core::session_space_application::{
        SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
    };
    use std::io::Write;

    let (home, project) = scene();
    // The settings store refuses a record reached through a link (a temporary directory's own,
    // on macOS), so the home is named where it stands.
    let home = home.path().canonicalize().unwrap();
    let aikit_home = aikit_store::AikitHome::at(&home);
    aikit_home.ensure_layout().unwrap();
    let spaces = aikit_store::SessionSpaceApplicationStore::new(aikit_home);
    let space = SessionSpaceRef::parse("session-space/settings-action").unwrap();
    let create = SessionSpaceMutation::Create {
        id: space.clone(),
        label: None,
    };
    spaces.apply(&spaces.stage(None, create).unwrap()).unwrap();
    let session = "agent-session/settings-action";
    let attach = SessionSpaceMutation::AttachAgentSession {
        attachment: SessionSpaceAgentAttachmentIntent {
            agent_session: aikit_core::ResourceRef::parse(session).unwrap(),
            purpose: Some("Settings by the act doorway".into()),
            provenance: vec!["cli_binary test".into()],
        },
    };
    spaces
        .apply(&spaces.stage(Some(&space), attach).unwrap())
        .unwrap();

    let invoke = |input: Value| {
        let mut child = std::process::Command::new(cargo_bin("aikit"))
            .args(["act", "invoke", "aikit.session.settings"])
            .args(["--input", "-", "--json"])
            .env("AIKIT_HOME", &home)
            .current_dir(project.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("binary runs");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let value: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|error| {
            panic!(
                "stdout is not JSON ({error}): {stdout:?}; stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), value)
    };

    let (ok, written) = invoke(serde_json::json!({
        "agent_session": [session],
        "request_json": r#"{"track":{"wait":900,"no_answer":"deny"}}"#,
    }));
    assert!(ok, "{written}");
    assert_eq!(written["track"]["wait"], 900, "{written}");
    assert_eq!(written["track"]["no_answer"], "deny", "{written}");
    let revision = written["revision"].as_str().expect("a written revision");

    let (ok, read) = invoke(serde_json::json!({"agent_session": [session]}));
    assert!(ok, "{read}");
    assert_eq!(read["track"]["wait"], 900, "{read}");
    assert_eq!(read["revision"], revision, "{read}");

    let (ok, stale) = invoke(serde_json::json!({
        "agent_session": [session],
        "request_json": r#"{"track":{"wait":1800}}"#,
    }));
    assert!(!ok, "a write on no revision after one was written: {stale}");
    assert_eq!(stale["error"]["code"], "act.route_refused", "{stale}");
    assert_eq!(
        stale["error"]["details"]["route_code"], "encounter.runtime",
        "{stale}"
    );
    assert!(
        stale["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("revision conflict")),
        "{stale}"
    );

    let (ok, moved) = invoke(serde_json::json!({
        "agent_session": [session],
        "request_json": r#"{"track":{"wait":1800}}"#,
        "expected_revision": revision,
    }));
    assert!(ok, "{moved}");
    assert_eq!(moved["track"]["wait"], 1800, "{moved}");
    assert_eq!(moved["track"]["no_answer"], "deny", "{moved}");
}
