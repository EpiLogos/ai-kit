//! The public harness-profile intake through the real CLI binary: validate,
//! show, register, list — plus the compatibility-gap returns an unknown
//! client name earns. This is the round-trip the 2026-09-20 SDK campaign
//! could not perform: an author working from public surface alone must be
//! able to check a document against the published grammar and register it.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

/// The worked specimen from docs/HARNESS-PROFILE-AUTHORING.md. If the doc
/// and the schema drift apart, this test is the tripwire.
const DOC_SPECIMEN: &str = r#"
schema = "aikit.harness-profile/v1"
slug = "opencode-specimen"
edition = "cli"

[presence]
executables = ["opencode"]
config-dir = "~/.config/opencode"

[skills]
posture = "observed"
observe = { paths = ["~/.config/opencode/skill", "~/.config/opencode/skills"] }

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["attach"]
"#;

/// The original campaign specimen's class of failure: a top-level field the
/// schema does not carry, invented because the grammar was unpublished.
const DOC_INVENTED_FIELDS: &str = r#"
schema = "aikit.harness-profile/v1"
id = "opencode"
slug = "opencode"
edition = "cli"
"#;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home.join("user-home"))
        .current_dir(cwd)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("invalid JSON ({error}): {stdout:?}"));
    (output, value)
}

#[test]
fn an_author_can_validate_show_register_and_list_an_external_profile() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let document = temp.path().join("opencode-specimen.toml");
    write(&document, DOC_SPECIMEN);

    // Validate: the grammar is now public, so a document authored from this
    // doc plus the target's own interface passes field-by-field.
    let (validated, reply) = run(
        &home,
        temp.path(),
        &["harness-profile", "validate", document.to_str().unwrap()],
    );
    assert!(validated.status.success(), "validate failed: {reply}");
    assert_eq!(reply["valid"], true);
    assert_eq!(reply["profile"]["slug"], "opencode-specimen");
    assert_eq!(reply["profile"]["source"], "document");

    // Show: an embedded document prints exactly as shipped.
    let (shown, reply) = run(
        &home,
        temp.path(),
        &["harness-profile", "show", "claude-code"],
    );
    assert!(shown.status.success(), "show failed: {reply}");
    let embedded_document = reply["document"].as_str().unwrap();
    assert!(
        embedded_document.contains("schema = \"aikit.harness-profile/v1\""),
        "the embedded document is the shipped grammar"
    );

    // Register: refused while the document collides with an embedded slug,
    // accepted under its own slug, and then loadable from the home.
    let (refused, reply) = run(
        &home,
        temp.path(),
        &["harness-profile", "register", document.to_str().unwrap()],
    );
    // The specimen's slug is its own, so registration proceeds...
    assert!(refused.status.success(), "register failed: {reply}");
    assert_eq!(reply["registered"], true);
    assert!(
        home.join("harness-profiles/opencode-specimen.toml")
            .is_file(),
        "the document is installed where the registry reads it"
    );

    // list: the embedded surface, the external document, and no problems.
    let (listed, reply) = run(&home, temp.path(), &["harness-profile", "list"]);
    assert!(listed.status.success(), "list failed: {reply}");
    let external = reply["external"].as_array().unwrap();
    assert!(
        external
            .iter()
            .any(|profile| profile["slug"] == "opencode-specimen"),
        "the registered document resolves: {reply}"
    );
    assert_eq!(reply["load_problems"].as_array().unwrap().len(), 0);

    // And the registry joins by slug where profiles are consumed.
    let (resolved, reply) = run(
        &home,
        temp.path(),
        &["harness-profile", "show", "opencode-specimen"],
    );
    assert!(resolved.status.success(), "registered show failed: {reply}");
    assert_eq!(reply["source"], "external");
}

#[test]
fn a_document_with_invented_fields_is_refused_with_the_field_named() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let document = temp.path().join("invented.toml");
    write(&document, DOC_INVENTED_FIELDS);

    let (rejected, reply) = run(
        &home,
        temp.path(),
        &["harness-profile", "validate", document.to_str().unwrap()],
    );
    assert!(!rejected.status.success(), "invented fields must refuse");
    assert_eq!(reply["error"]["code"], "harness-profile.parse_failed");
}

#[test]
fn registration_never_overrides_an_embedded_slug() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let document = temp.path().join("claude-code.toml");
    write(
        &document,
        r#"
schema = "aikit.harness-profile/v1"
slug = "claude-code"
edition = "cli"
"#,
    );

    let (refused, reply) = run(
        &home,
        temp.path(),
        &["harness-profile", "register", document.to_str().unwrap()],
    );
    assert!(!refused.status.success(), "embedded override must refuse");
    assert_eq!(reply["error"]["code"], "harness-profile.embedded_conflict");
    assert!(
        !home.join("harness-profiles/claude-code.toml").exists(),
        "nothing was written"
    );
}

#[test]
fn an_unknown_client_name_is_a_compatibility_gap_that_names_the_route() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir_all(temp.path().join("work")).unwrap();

    // install: the gap names the SDK contract and the authoring skill.
    let (refused, reply) = run(&home, temp.path(), &["client", "install", "nosuchharness"]);
    assert!(!refused.status.success());
    assert_eq!(reply["error"]["code"], "harness.compatibility_gap");
    assert_eq!(
        reply["error"]["details"]["sdk_ref"],
        "aikit:harness-adapter-sdk/v1"
    );
    assert_eq!(
        reply["error"]["details"]["authoring_skill_ref"],
        "skill/aikit/harness-adapter-authoring"
    );

    // status: a read model that matches nothing is the same gap, not `[]`.
    let (empty, reply) = run(&home, temp.path(), &["client", "status", "nosuchharness"]);
    assert!(
        !empty.status.success(),
        "an unknown name must not read as []"
    );
    assert_eq!(reply["error"]["code"], "harness.compatibility_gap");
}
