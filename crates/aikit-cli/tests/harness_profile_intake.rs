//! `aikit harness-profile validate` — the external author's intake check.
//!
//! The 09-20 SDK campaign's admission failure closed for opencode-the-harness
//! and stayed open for outside authors: no verb accepted an authored
//! `aikit.harness-profile/v1` TOML (`z harness-profile validate <file>`
//! answered the fuzzy jump's `decision: "nothing"`). These tests pin the
//! intake verb through the real binary: a valid specimen admits, a schema
//! violation and an admission-grammar violation refuse with the named
//! diagnostics and a non-zero exit — validation only, nothing applied.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

fn run(home: &Path, cwd: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .current_dir(cwd)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("invalid JSON ({error}): {stdout:?}"));
    (output, value)
}

/// A valid `aikit.harness-profile/v1` document — the same shape the embedded
/// opencode profile ships (brokered skills, observed layers, no model dispatch).
const VALID_SPECIMEN: &str = r#"
schema = "aikit.harness-profile/v1"
slug = "opencode"
edition = "cli"

[presence]
executables = ["opencode"]
config-dir = "~/.config/opencode"

[skills]
posture = "brokered"
shared-tree = "skills/ is a documented config subdirectory (global and project scope); this census revision brokers projection (opencode census)."
observe = { paths = ["~/.config/opencode/skills"] }

[guidance]
posture = "observed"
observe = ["~/.config/opencode/AGENTS.md"]

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["resume", "attach"]
"#;

fn write_specimen(home: &Path, name: &str, contents: &str) -> std::path::PathBuf {
    let path = home.join(name);
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn a_valid_external_specimen_is_admitted() {
    let temp = tempfile::tempdir().unwrap();
    let specimen = write_specimen(temp.path(), "valid.harness-profile.toml", VALID_SPECIMEN);

    let (output, reply) = run(
        temp.path().join("home").as_path(),
        temp.path(),
        &["harness-profile", "validate", specimen.to_str().unwrap()],
    );
    assert!(output.status.success(), "a valid document admits: {reply}");
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["decision"], "admit");
    assert_eq!(reply["data"]["profile"]["slug"], "opencode");
    assert_eq!(reply["data"]["diagnostics"].as_array().unwrap().len(), 0);
}

#[test]
fn an_unknown_field_is_a_named_schema_refusal() {
    let temp = tempfile::tempdir().unwrap();
    let specimen = write_specimen(
        temp.path(),
        "schema.harness-profile.toml",
        &format!("{VALID_SPECIMEN}\nfrobnicate = true\n",),
    );

    let (output, reply) = run(
        temp.path().join("home").as_path(),
        temp.path(),
        &["harness-profile", "validate", specimen.to_str().unwrap()],
    );
    assert!(
        !output.status.success(),
        "a schema violation refuses: {reply}"
    );
    assert_eq!(
        reply["ok"], true,
        "the verb succeeded; the document refused"
    );
    assert_eq!(reply["data"]["decision"], "refuse");
    let diagnostics = reply["data"]["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "harness_profile.schema_violation");
    let message = diagnostics[0]["message"].as_str().unwrap();
    assert!(
        message.contains("frobnicate"),
        "the diagnostic names the offending field: {message}"
    );
}

#[test]
fn a_grammar_violation_refuses_with_the_layer_named() {
    let temp = tempfile::tempdir().unwrap();
    // Posture truth: a managed layer must declare the seam it projects into.
    let specimen = write_specimen(
        temp.path(),
        "grammar.harness-profile.toml",
        &format!("{VALID_SPECIMEN}\n[hooks]\nposture = \"managed\"\n",),
    );

    let (output, reply) = run(
        temp.path().join("home").as_path(),
        temp.path(),
        &["harness-profile", "validate", specimen.to_str().unwrap()],
    );
    assert!(
        !output.status.success(),
        "an admission-grammar violation refuses: {reply}"
    );
    assert_eq!(reply["data"]["decision"], "refuse");
    let diagnostics = reply["data"]["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0]["code"],
        "harness_profile.managed_without_project"
    );
    let message = diagnostics[0]["message"].as_str().unwrap();
    assert!(
        message.contains("hooks"),
        "the diagnostic names the layer: {message}"
    );
}
