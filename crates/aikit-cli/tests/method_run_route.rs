//! A3 executable praxis support: the Method invocation route (`aikit method
//! run`) over two real registered practices, driven through the real binary.
//! Preflight refusals land before any effect; one execution goes through the
//! same native runner `aikit run` uses; the receipt carries the digests a
//! later `method prove` verification consumes and never claims
//! postconditions — a successful shell exit is not proof.

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Support entries are executed directly, so the bit is part of the fixture.
fn write_executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    write(path, contents);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A home with two Method capsules bound to the two vertical practices
/// (composition/projection and verification/close-out), one plain Skill,
/// and a project. Each Method's support reads its one JSON argument and
/// echoes it, so the test can pin the exact payload the support received.
fn fixture() -> (TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    // The native seam speaks argv: the JSON input arrives as one argument.
    let support = "#!/bin/sh\nprintf '%s' \"$1\" > \"$0.received.json\"\nprintf '{\"received\":true}\\n'\n";
    let compose = home
        .path()
        .join("registries/personal/capsules/script/practice/compose-project");
    write(
        &compose.join("manifest.toml"),
        r#"schema = 1
id = "script/practice/compose-project"
kind = "script"
name = "compose-project"
description = "METHOD: compose and project the selected repertoire into the working world, then read the projection back."

[script]
entry = "payload/run.sh"
"#,
    );
    write_executable(&compose.join("payload/run.sh"), support);
    let closeout = home
        .path()
        .join("registries/personal/capsules/script/practice/verify-closeout");
    write(
        &closeout.join("manifest.toml"),
        r#"schema = 1
id = "script/practice/verify-closeout"
kind = "script"
name = "verify-closeout"
description = "METHOD: verify the landed change against its own postconditions and close the task out with native evidence."

[script]
entry = "payload/run.sh"
"#,
    );
    write_executable(&closeout.join("payload/run.sh"), support);
    let plain = home
        .path()
        .join("registries/personal/capsules/script/demo/greet");
    write(
        &plain.join("manifest.toml"),
        r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "A plain skill: no METHOD here."

[script]
entry = "payload/run.sh"
"#,
    );
    write(&plain.join("payload/run.sh"), "#!/bin/sh\necho hi\n");
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    (home, project)
}

fn run(home: &Path, project: &Path, args: &[&str]) -> (Value, String, bool) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .unwrap_or_else(|e| panic!("aikit {args:?} should run: {e}"));
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    // The Method's own support may write to stdout ahead of the envelope;
    // the envelope is the last JSON line.
    let text = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = text
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
        .unwrap_or(Value::Null);
    (parsed, stderr, output.status.success())
}

#[test]
fn method_runs_the_two_practices_and_the_receipt_carries_the_evidence() {
    let (home, project) = fixture();
    let input_file = home.path().join("composition-input.json");
    fs::write(&input_file, r#"{"repertoire":["skill/rust/code-review"]}"#).unwrap();

    // Vertical 1 — composition/projection: exact input, one execution, the
    // receipt names the Method and the digests, and the support received the
    // exact submitted JSON.
    let (envelope, _stderr, ok) = run(
        home.path(),
        project.path(),
        &[
            "method",
            "run",
            "script/practice/compose-project",
            "--input",
            &format!("@{}", input_file.display()),
            "--confirm",
        ],
    );
    assert!(ok, "method run should succeed: {envelope}");
    assert_eq!(envelope["data"]["schema"], "aikit.method-execution/v1");
    assert_eq!(envelope["data"]["method"], "script/practice/compose-project");
    assert_eq!(envelope["data"]["status"], "ok");
    assert_eq!(envelope["data"]["exit_status"], 0);
    assert!(
        envelope["data"]["result_digest"].as_str().is_some_and(|d| !d.is_empty()),
        "the receipt carries the result digest: {envelope}"
    );
    assert!(
        envelope["data"]["postconditions"]
            .as_str()
            .is_some_and(|text| text.contains("not claimed")),
        "the receipt never claims postconditions: {envelope}"
    );
    let received = fs::read_to_string(
        home.path()
            .join("registries/personal/capsules/script/practice/compose-project/payload/run.sh.received.json"),
    )
    .unwrap();
    assert_eq!(
        received.trim(),
        r#"{"repertoire":["skill/rust/code-review"]}"#,
        "the exact submitted input reached the Method's support"
    );

    // Vertical 2 — verification/close-out: the run's receipt is refused as
    // proof without a verified revision; a successful shell exit is not
    // proof, and the gate says so.
    let (envelope, _stderr, ok) = run(
        home.path(),
        project.path(),
        &[
            "method",
            "run",
            "script/practice/verify-closeout",
            "--input",
            "{}",
            "--confirm",
        ],
    );
    assert!(ok, "the close-out support runs: {envelope}");
    let (envelope, _stderr, ok) = run(
        home.path(),
        project.path(),
        &[
            "method",
            "prove",
            "--method",
            "script/practice/verify-closeout",
            "--proof-json",
            r#"{"proof_ref":"evidence/proof","context_resolution_ref":"evidence/context","activity_refs":["evidence/a"],"return_refs":["evidence/r"],"evidence_refs":["evidence/e"],"verification_refs":["evidence/v"],"invocation_succeeded":true,"verification_passed":false}"#,
        ],
    );
    assert!(
        !ok,
        "a successful shell exit is not proof: verification refused",
    );
    assert_eq!(
        envelope["error"]["code"], "routine.verification_required",
        "the proof gate demands explicit verification: {envelope}"
    );
}

#[test]
fn preflight_refusals_land_before_any_effect() {
    let (home, project) = fixture();

    // A plain Skill is not a Method; the route names the direct one.
    let (envelope, _stderr, ok) = run(
        home.path(),
        project.path(),
        &["method", "run", "script/demo/greet", "--confirm"],
    );
    assert!(!ok);
    assert_eq!(envelope["error"]["code"], "method.not_a_method", "{envelope}");

    // An unknown ref is refused.
    let (envelope, _stderr, ok) = run(
        home.path(),
        project.path(),
        &["method", "run", "script/no/such", "--confirm"],
    );
    assert!(!ok);
    assert_eq!(envelope["error"]["code"], "method.unknown", "{envelope}");

    // Input must be one JSON value; nothing ran (no receipt file appeared).
    let (envelope, _stderr, ok) = run(
        home.path(),
        project.path(),
        &[
            "method",
            "run",
            "script/practice/compose-project",
            "--input",
            "not-json",
            "--confirm",
        ],
    );
    assert!(!ok);
    assert_eq!(envelope["error"]["code"], "method.input_invalid", "{envelope}");
    assert!(
        !home
            .path()
            .join("registries/personal/capsules/script/practice/compose-project/payload/run.sh.received.json")
            .exists(),
        "the support never ran on an invalid input"
    );
}
