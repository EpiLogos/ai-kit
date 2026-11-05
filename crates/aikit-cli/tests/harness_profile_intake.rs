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

/// `aikit harness auth <harness> --check --json` (O-I #758, Sign in's
/// read-back): the harness's declared native logged-in check runs through the
/// real binary and only its declared answer comes back. A fake `codex` on
/// PATH answers the way codex-cli 0.155.1 does (on stderr); describe mode
/// still names the check without running it.
#[cfg(unix)]
#[test]
fn harness_auth_check_runs_the_declared_native_check_and_describe_does_not() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let asked = temp.path().join("asked");
    let codex = bin.join("codex");
    fs::write(
        &codex,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nif [ \"$*\" = \"login status\" ]; then echo 'Logged in using ChatGPT' >&2; exit 0; fi\nexit 9\n",
            asked.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let aikit = |args: &[&str]| {
        let output = Command::new(cargo_bin("aikit"))
            .args(args)
            .arg("--json")
            .env("AIKIT_HOME", temp.path().join("aikit"))
            .env("HOME", temp.path())
            .env("PATH", &path)
            .current_dir(temp.path())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let value: Value = serde_json::from_str(stdout.trim())
            .unwrap_or_else(|error| panic!("invalid JSON ({error}): {stdout:?}"));
        (output, value)
    };

    let (output, described) = aikit(&["harness", "auth", "codex"]);
    assert!(output.status.success(), "{described}");
    assert_eq!(described["data"]["own_login"][0]["status"][1], "login");
    assert!(!asked.exists(), "describe runs nothing");

    let (output, checked) = aikit(&["harness", "auth", "codex", "--check"]);
    assert!(output.status.success(), "{checked}");
    assert_eq!(checked["data"]["slug"], "codex");
    let check = &checked["data"]["checks"][0];
    assert_eq!(check["provider_ref"], "provider:openai");
    assert_eq!(check["signed_in"], true);
    assert_eq!(fs::read_to_string(&asked).unwrap(), "login status\n");

    let (output, none) = aikit(&["harness", "auth", "opencode", "--check"]);
    assert!(output.status.success(), "{none}");
    assert_eq!(none["data"]["checks"], serde_json::json!([]));
}

/// DLW build ruling 2026-11-04 (O-I #758 Sign in): Claude Code signs in like
/// Codex — `aikit harness auth claude` runs its one-shot `claude auth login`,
/// and `--check` reads back `claude auth status --json` by its `loggedIn`
/// alone. A fake `claude` stands on PATH: the real one is never run, and no
/// keychain is reached.
#[cfg(unix)]
#[test]
fn claude_signs_in_by_its_own_login_and_reads_back_its_logged_in_check() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let asked = temp.path().join("asked");
    let claude = bin.join("claude");
    fs::write(
        &claude,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nif [ \"$*\" = \"auth status --json\" ]; then echo '{{\"loggedIn\": true, \"authMethod\": \"claude.ai\", \"email\": \"owner@example.com\"}}'; exit 0; fi\nif [ \"$*\" = \"auth login\" ]; then exit 0; fi\nexit 9\n",
            asked.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let aikit = |args: &[&str]| {
        Command::new(cargo_bin("aikit"))
            .args(args)
            .env("AIKIT_HOME", temp.path().join("aikit"))
            .env("HOME", temp.path())
            .env("PATH", &path)
            .current_dir(temp.path())
            .output()
            .unwrap()
    };

    let login = aikit(&["harness", "auth", "claude"]);
    assert!(
        login.status.success(),
        "{}",
        String::from_utf8_lossy(&login.stderr)
    );
    assert_eq!(fs::read_to_string(&asked).unwrap(), "auth login\n");

    let checked = aikit(&["harness", "auth", "claude", "--check", "--json"]);
    assert!(checked.status.success());
    let stdout = String::from_utf8_lossy(&checked.stdout);
    let value: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("invalid JSON ({error}): {stdout:?}"));
    assert_eq!(value["data"]["slug"], "claude-code");
    let check = &value["data"]["checks"][0];
    assert_eq!(check["provider_ref"], "provider:anthropic");
    assert_eq!(check["signed_in"], true);
    assert!(
        !stdout.contains("example.com"),
        "nothing else the harness printed is carried: {stdout}"
    );
    assert_eq!(
        fs::read_to_string(&asked).unwrap(),
        "auth login\nauth status --json\n"
    );
}
