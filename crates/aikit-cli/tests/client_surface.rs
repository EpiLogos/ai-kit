//! `aikit client status` derives its surface from live intake, never a
//! hard-coded roster.
//!
//! The acceptance law: every registered adapter answers with the state its
//! intake outcomes support — a resolved capability descriptor is installable,
//! a present harness whose descriptor is refused is a disclosed compatibility
//! gap, a harness detection cannot see is absent with the evidence named, and
//! an intake that cannot be read at all is disclosed as unavailable. A
//! harness with resolved capability can never be missing from the output, and
//! `client status pi` can never again come back empty.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

/// The pinned client surface. A change here is a deliberate surface change.
const REGISTERED: &[&str] = &[
    "claude",
    "codex",
    "zcode",
    "broker",
    "aider",
    "gemini-antigravity",
    "cursor-cli",
    "deepseek-harness",
    "gemini-cli",
    "goose",
    "grok-bot",
    "kimi",
    "opencode",
    "openclaw",
    "pi",
    "qwen-code",
    "ollama",
];

/// A stand-in `actuation` binary: serves fixture descriptors for the slugs a
/// scenario staged, the fixture detection record, and refuses every other
/// capability lookup exactly as Actuation does.
fn stage_actuation(home: &Path) -> PathBuf {
    let bin_dir = home.join("actuation-bin");
    let fixtures = home.join("actuation-fixtures");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(fixtures.join("descriptors")).unwrap();
    let script = bin_dir.join("actuation");
    fs::write(
        &script,
        "#!/bin/sh\nmode=\"$1 $2\"\nslug=\"$3\"\nif [ \"$mode\" = \"harness detect\" ]; then\n\x20 cat \"$FIXTURES/detection.json\"\n\x20 exit 0\nfi\nif [ \"$mode\" = \"harness capability\" ]; then\n\x20 if [ -f \"$FIXTURES/descriptors/$slug.json\" ]; then\n\x20   cat \"$FIXTURES/descriptors/$slug.json\"\n\x20   exit 0\n\x20 fi\n\x20 echo \"actuation: no capability descriptor declared for harness $slug; declared: fixture\" >&2\n\x20 exit 2\nfi\necho \"unexpected argv: $*\" >&2\nexit 3\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fixtures
}

fn write_detection_fixture(fixtures: &Path, harnesses: &str) {
    fs::write(
        fixtures.join("detection.json"),
        format!(
            r#"{{
              "schema": "actuation.harness-detection/v1",
              "detection_ref": "detection:fixture",
              "observed_at": "2026-09-15T00:00:00Z",
              "catalog_revision": 7,
              "detector": {{"implementation": "fixture"}},
              "harnesses": [{harnesses}],
              "absent": [],
              "availability": "complete"
            }}"#
        ),
    )
    .unwrap();
}

fn write_descriptor_fixture(fixtures: &Path, slug: &str) {
    fs::write(
        fixtures.join("descriptors").join(format!("{slug}.json")),
        format!(
            r#"{{
              "schema": "actuation.harness-capability/v1",
              "document": "capability-read-model",
              "capability": {{
                "schema": "actuation.harness-capability/v1",
                "document": "capability",
                "harness_slug": "{slug}",
                "native_events": [],
                "injection_channel": {{"kind": "hooks", "mechanism": "settings"}},
                "blocking_semantics": {{"kind": "exit-code"}},
                "wake_capability": {{"kind": "none"}},
                "install_seam": {{
                  "config_path": "~/seams/{slug}.json",
                  "format": "json",
                  "entry_shape": "object",
                  "ownership_marker": "aikit",
                  "preserves_foreign_entries": true
                }},
                "uninstall_seam": {{
                  "config_path": "~/seams/{slug}.json",
                  "format": "json",
                  "entry_shape": "object",
                  "ownership_marker": "aikit",
                  "preserves_foreign_entries": true
                }},
                "provenance": {{"authored_by": "fixture", "catalog_revision": 7}}
              }}
            }}"#
        ),
    )
    .unwrap();
}

/// The catalog slug a registered CLI name resolves to; `None` for the broker.
fn catalog_slug_of(name: &str) -> Option<&str> {
    match name {
        "claude" => Some("claude-code"),
        // The gemini client keeps the client name gemini-cli and joins by
        // the catalog slug gemini (the same contract TargetId::GEMINI
        // carries); fixtures stage descriptors under the join key.
        "gemini-cli" => Some("gemini"),
        "broker" => None,
        other => Some(other),
    }
}

fn run(home: &Path, path_override: Option<&Path>, args: &[&str]) -> std::process::Output {
    let mut command = Command::cargo_bin("aikit").unwrap();
    command
        .env("AIKIT_HOME", home.join("aikit-home"))
        .env("HOME", home.join("user-home"))
        .arg("--json")
        .args(args)
        .current_dir(home.join("project"));
    match path_override {
        Some(path) => {
            command.env("PATH", path);
        }
        None => {
            // Actuation deliberately absent: this scenario's live truth.
            command.env("PATH", "/usr/bin:/bin");
        }
    }
    command.output().unwrap()
}

fn rows(home: &Path, path_override: Option<&Path>, only: Option<&str>) -> Vec<Value> {
    let mut args = vec!["client", "status"];
    if let Some(only) = only {
        args.push(only);
    }
    let output = run(home, path_override, &args);
    assert!(
        output.status.success(),
        "client status must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    envelope["data"]["clients"].as_array().cloned().unwrap()
}

fn by_name(rows: &[Value], name: &str) -> Value {
    rows.iter()
        .find(|row| row["client"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no row for {name} in {rows:?}"))
}

/// A fixture world plus a stub actuation that resolves descriptors only for
/// `claude-code`, detects `claude-code` and `pi`, and reports `aider`
/// not installed — the three-state spread in one scenario.
fn scenario_with_partial_intake() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("project/.aikit")).unwrap();
    fs::write(
        home.path().join("project/.aikit/profile.toml"),
        "schema = 1\n",
    )
    .unwrap();
    let fixtures = stage_actuation(home.path());
    write_descriptor_fixture(&fixtures, "claude-code");
    write_detection_fixture(
        &fixtures,
        r#"{"slug": "claude-code", "harness_ref": "harness/claude-code", "state": "detected",
            "probes": [{"kind": "config-dir", "result": "pass", "spec": "~/.claude"}]},
           {"slug": "pi", "harness_ref": "harness/pi", "state": "detected",
            "probes": [{"kind": "config-dir", "result": "pass", "spec": "~/.pi/agent"}]},
           {"slug": "aider", "harness_ref": "harness/aider", "state": "not-installed"}"#,
    );
    home
}

#[test]
fn a_registered_adapter_with_a_descriptor_fixture_gets_its_row() {
    let home = scenario_with_partial_intake();
    let rows = rows_with_fixtures_env(&home, &["client", "status"]);
    assert_eq!(
        rows.len(),
        REGISTERED.len(),
        "every registered adapter answers"
    );

    let claude = by_name(&rows, "claude");
    assert_eq!(claude["state"], "installable");
    assert_eq!(claude["capability"], "descriptor");
    assert_eq!(claude["detection"], "detected");
    assert_eq!(claude["dispatch"], "client");

    // Acceptance (a): the fixture adapter (pi) answers — never `[]` again.
    let pi = by_name(&rows, "pi");
    assert_eq!(pi["state"], "gap");
    assert_eq!(pi["capability"], "unavailable");
    assert!(
        pi["capability_reason"]
            .as_str()
            .unwrap()
            .contains("no capability descriptor declared for harness pi"),
        "pi's refusal must be disclosed: {}",
        pi["capability_reason"]
    );
    assert_eq!(pi["gap"]["target"], "pi");
    assert_eq!(pi["gap"]["missing_contract"], "aikit.harness-adapter/v1");
    assert_eq!(pi["detection"], "detected");
    assert_eq!(pi["dispatch"], "adapter-only");
    // The detection probe is the read-model home while the seam is refused.
    let user_home = home.path().join("user-home");
    assert_eq!(
        pi["config_dir"],
        user_home.join(".pi/agent").display().to_string()
    );

    // Detection's absence evidence is honoured even though capability refuses.
    let aider = by_name(&rows, "aider");
    assert_eq!(aider["state"], "absent");
    assert_eq!(aider["detection"], "not-installed");

    // A slug the record does not name is disclosed absence, not silence.
    let opencode = by_name(&rows, "opencode");
    assert_eq!(opencode["state"], "absent");
    assert!(opencode["detection_reason"]
        .as_str()
        .unwrap()
        .contains("names no entry for slug opencode"));

    // The broker is AIKit's own and stands outside the three-state law.
    let broker = by_name(&rows, "broker");
    assert_eq!(broker["state"], "self");
    assert_eq!(broker["capability"], "self");
    assert_eq!(broker["detection"], "self");
}

/// The partial-intake scenario's rows, with the stub's FIXTURES environment
/// pointed at the fixtures and the given command arguments.
fn rows_with_fixtures_env(home: &tempfile::TempDir, args: &[&str]) -> Vec<Value> {
    let mut command = Command::cargo_bin("aikit").unwrap();
    command
        .env("AIKIT_HOME", home.path().join("aikit-home"))
        .env("HOME", home.path().join("user-home"))
        .env(
            "PATH",
            format!(
                "{}:/usr/bin:/bin",
                home.path().join("actuation-bin").display()
            ),
        )
        .env("FIXTURES", home.path().join("actuation-fixtures"))
        .arg("--json")
        .args(args)
        .current_dir(home.path().join("project"));
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "client status must succeed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    envelope["data"]["clients"].as_array().cloned().unwrap()
}

#[test]
fn no_harness_with_resolved_capability_is_missing_from_status() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("project/.aikit")).unwrap();
    fs::write(
        home.path().join("project/.aikit/profile.toml"),
        "schema = 1\n",
    )
    .unwrap();
    let fixtures = stage_actuation(home.path());
    // Every registered harness gets a descriptor under its catalog slug (the
    // one the capability intake asks for): the surface must show all of them,
    // installable, with none missing.
    for name in REGISTERED {
        let slug = catalog_slug_of(name);
        if let Some(slug) = slug {
            write_descriptor_fixture(&fixtures, slug);
        }
    }
    write_detection_fixture(&fixtures, "");

    let rows = rows_with_fixtures_env(&home, &["client", "status"]);

    for name in REGISTERED {
        let row = by_name(&rows, name);
        if *name == "broker" {
            assert_eq!(row["state"], "self");
            continue;
        }
        assert_eq!(
            row["state"], "installable",
            "{name} resolved capability but is missing or not installable: {row}"
        );
        assert_eq!(row["capability"], "descriptor");
    }
}

#[test]
fn client_status_pi_answers_single_row() {
    let home = scenario_with_partial_intake();
    let rows = rows_with_fixtures_env(&home, &["client", "status", "pi"]);
    assert_eq!(rows.len(), 1, "the filter must select exactly pi: {rows:?}");
    assert_eq!(rows[0]["client"], "pi");
    assert_eq!(rows[0]["state"], "gap");
    // The alias `claude-code` selects the claude row the same way.
    let claude_rows = rows_with_fixtures_env(&home, &["client", "status", "claude-code"]);
    assert_eq!(claude_rows.len(), 1);
    assert_eq!(claude_rows[0]["client"], "claude");
    assert_eq!(claude_rows[0]["state"], "installable");
}

#[test]
fn with_actuation_absent_every_row_discloses_instead_of_vanishing() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("project/.aikit")).unwrap();
    fs::write(
        home.path().join("project/.aikit/profile.toml"),
        "schema = 1\n",
    )
    .unwrap();

    let all_rows = rows(home.path(), None, None);
    assert_eq!(all_rows.len(), REGISTERED.len());
    for row in &all_rows {
        if row["client"] == "broker" {
            // The broker is AIKit's own client: no intake, no unavailability.
            assert_eq!(row["state"], "self");
            continue;
        }
        assert_eq!(
            row["state"], "unavailable",
            "an unreadable intake is disclosed, never silence: {row}"
        );
        assert!(row["capability_reason"].as_str().is_some());
        assert!(row["detection_reason"].as_str().is_some());
    }
    // The original defect, inverted: pi answers even with nothing to read.
    let pi_rows = rows(home.path(), None, Some("pi"));
    assert_eq!(pi_rows.len(), 1);
    assert_eq!(pi_rows[0]["client"], "pi");
    assert_eq!(pi_rows[0]["state"], "unavailable");
}

#[test]
fn install_refuses_for_registered_harnesses_without_a_dispatch_seam() {
    let home = scenario_with_partial_intake();
    let mut command = Command::cargo_bin("aikit").unwrap();
    command
        .env("AIKIT_HOME", home.path().join("aikit-home"))
        .env("HOME", home.path().join("user-home"))
        .env(
            "PATH",
            format!(
                "{}:/usr/bin:/bin",
                home.path().join("actuation-bin").display()
            ),
        )
        .env("FIXTURES", home.path().join("actuation-fixtures"))
        .arg("--json")
        .args(["client", "install", "pi"])
        .current_dir(home.path().join("project"));
    let output = command.output().unwrap();
    assert!(!output.status.success(), "pi has no install seam");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("client.not_dispatchable"), "{stdout}");

    // The unchanged law: install refuses without a descriptor.
    let mut command = Command::cargo_bin("aikit").unwrap();
    command
        .env("AIKIT_HOME", home.path().join("aikit-home"))
        .env("HOME", home.path().join("user-home"))
        .env(
            "PATH",
            format!(
                "{}:/usr/bin:/bin",
                home.path().join("actuation-bin").display()
            ),
        )
        .env("FIXTURES", home.path().join("actuation-fixtures"))
        .arg("--json")
        .args(["client", "install", "codex"])
        .current_dir(home.path().join("project"));
    let output = command.output().unwrap();
    assert!(
        !output.status.success(),
        "codex's descriptor is refused here"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("client.capability_unavailable"), "{stdout}");
}
