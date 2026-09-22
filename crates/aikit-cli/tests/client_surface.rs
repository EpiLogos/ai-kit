//! `aikit client status` derives its surface from the live detection record,
//! never a hard-coded roster.
//!
//! The acceptance law: every descriptor the detector reports answers with the
//! state its intake outcomes support — a resolved capability descriptor plus
//! an AIKit overlay is installable, a present harness whose descriptor is
//! refused is a disclosed compatibility gap (or, with no AIKit adapter at
//! all, the honest generic row), a harness detection cannot see is absent
//! with the evidence named, and an intake that cannot be read at all is
//! disclosed as unavailable. A harness with resolved capability can never be
//! missing from the output, and `client status pi` can never again come back
//! empty.
//!
//! The static overlay surface (the per-client detail AIKit carries) is named
//! here so a change to it is a deliberate surface change. The harness rows
//! themselves are the detector's, not this list's.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

/// The pinned overlay surface: the catalog slugs AIKit carries per-client
/// detail for (adapters, admission censuses, dispatch decisions), by
/// CLI-facing name. Harnesses without an overlay here still render — as the
/// honest generic rows this file also pins.
const OVERLAY_NAMES: &[&str] = &[
    "claude",
    "codex",
    "zcode",
    "opencode",
    "gemini-cli",
    "pi",
    "gemini-antigravity",
    "grok-bot",
    "kimi",
    "openclaw",
    "ollama",
    "hermes",
    "hermes-acp",
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

/// The catalog slug an overlay's CLI-facing name joins by; the claude and
/// gemini precedents — the client name may differ, the join key must not.
fn catalog_slug_of(name: &str) -> &str {
    match name {
        "claude" => "claude-code",
        // The gemini client keeps the client name gemini-cli and joins by
        // the catalog slug gemini (the same contract TargetId::GEMINI
        // carries); fixtures stage descriptors under the join key.
        "gemini-cli" => "gemini",
        other => other,
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
/// `claude-code`, detects `claude-code`, `pi`, a slug AIKit carries no overlay
/// for, and reports `aider` not installed — the three-state spread plus the
/// generic-row path in one scenario.
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
           {"slug": "future-harness", "harness_ref": "harness/future-harness", "state": "detected",
            "probes": [{"kind": "config-dir", "result": "pass", "spec": "~/.future"}]},
           {"slug": "aider", "harness_ref": "harness/aider", "state": "not-installed"}"#,
    );
    home
}

#[test]
fn a_client_honours_its_own_config_home_override_over_every_default() {
    // claude is detected with a resolved descriptor whose seam names
    // `~/seams/claude-code.json`; the harness's own documented env override
    // must still win — install and read models otherwise describe and wire a
    // different installation than the one that will run. This is also what
    // makes the surface exercisable in an isolated receiving scope.
    let home = scenario_with_partial_intake();
    let scratch = home.path().join("scratch-claude");
    fs::create_dir_all(&scratch).unwrap();

    let mut command = Command::cargo_bin("aikit").unwrap();
    command
        .env("AIKIT_HOME", home.path().join("aikit-home"))
        .env("HOME", home.path().join("user-home"))
        .env("CLAUDE_CONFIG_DIR", &scratch)
        .env(
            "PATH",
            format!(
                "{}:/usr/bin:/bin",
                home.path().join("actuation-bin").display()
            ),
        )
        .env("FIXTURES", home.path().join("actuation-fixtures"))
        .arg("--json")
        .args(["client", "status", "claude"])
        .current_dir(home.path().join("project"));
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "status must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).unwrap();
    let rows = value["data"]["clients"].as_array().unwrap();
    let claude = by_name(rows, "claude");
    assert_eq!(
        claude["config_dir"],
        scratch.display().to_string(),
        "CLAUDE_CONFIG_DIR must win over the descriptor seam and every ~/ default, got {}",
        claude["config_dir"]
    );
}

#[test]
fn a_registered_adapter_with_a_descriptor_fixture_gets_its_row() {
    let home = scenario_with_partial_intake();
    let rows = rows_with_fixtures_env(&home, &["client", "status"]);
    // The record's four entries (two overlaid, one generic, one not-installed)
    // + the eight overlays the record does not name + the broker.
    assert_eq!(
        rows.len(),
        4 + (OVERLAY_NAMES.len() - 2) + 1,
        "every reported descriptor answers, overlaid or not"
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

    // A descriptor the record reports with no AIKit overlay renders as the
    // honest generic row: adapter-only, nothing planned, the missing contract
    // named — never invented detail.
    let generic = by_name(&rows, "future-harness");
    assert_eq!(generic["harness"], "future-harness");
    assert_eq!(generic["state"], "gap");
    assert_eq!(generic["dispatch"], "adapter-only");
    assert_eq!(generic["effect"], Value::Null);
    assert_eq!(
        generic["gap"]["missing_contract"],
        "aikit.harness-adapter/v1"
    );
    assert_eq!(generic["detection"], "detected");

    // Detection's absence evidence is honoured even though capability refuses.
    let aider = by_name(&rows, "aider");
    assert_eq!(aider["state"], "absent");
    assert_eq!(aider["detection"], "not-installed");

    // An overlay slug the record does not name is disclosed absence, not
    // silence — AIKit's own integrations stay on the surface.
    let codex = by_name(&rows, "codex");
    assert_eq!(codex["state"], "absent");
    assert!(codex["detection_reason"]
        .as_str()
        .unwrap()
        .contains("names no entry for slug codex"));

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
    // Every overlay harness gets a descriptor under its catalog slug (the one
    // the capability intake asks for): the surface must show all of them,
    // installable, with none missing.
    for name in OVERLAY_NAMES {
        write_descriptor_fixture(&fixtures, catalog_slug_of(name));
    }
    write_detection_fixture(&fixtures, "");

    let rows = rows_with_fixtures_env(&home, &["client", "status"]);

    for name in OVERLAY_NAMES {
        let row = by_name(&rows, name);
        assert_eq!(
            row["state"], "installable",
            "{name} resolved capability but is missing or not installable: {row}"
        );
        assert_eq!(row["capability"], "descriptor");
    }
    // The broker needs no descriptor: it is AIKit's own.
    assert_eq!(by_name(&rows, "broker")["state"], "self");
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
    // A generic record slug is addressable by its slug alone.
    let generic_rows = rows_with_fixtures_env(&home, &["client", "status", "future-harness"]);
    assert_eq!(generic_rows.len(), 1);
    assert_eq!(generic_rows[0]["client"], "future-harness");
    assert_eq!(generic_rows[0]["state"], "gap");
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
    // The overlays stay on the surface with their disclosure; no generic rows
    // can exist (the record is unreadable and nothing is invented in its
    // place); the broker closes the surface.
    assert_eq!(all_rows.len(), OVERLAY_NAMES.len() + 1);
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

/// Issue #394 K2: the embedded adapter registry carries opencode, so the
/// client surface must answer for it by name — a roster row with its honest
/// state, and an install refusal that names the harness as known-but-undispatchable,
/// never `client.unknown` against a roster that omits it.
#[test]
fn client_status_opencode_answers_in_an_isolated_scope_and_install_is_a_named_refusal() {
    let home = scenario_with_partial_intake();

    // This scenario's detection record names no opencode entry and its
    // descriptor is refused — the row still answers, with the absence named.
    let rows = rows_with_fixtures_env(&home, &["client", "status", "opencode"]);
    assert_eq!(
        rows.len(),
        1,
        "opencode must be a first-class row: {rows:?}"
    );
    assert_eq!(rows[0]["client"], "opencode");
    assert_eq!(rows[0]["state"], "absent");
    assert_eq!(rows[0]["detection"], "absent-from-record");
    assert_eq!(rows[0]["dispatch"], "adapter-only");
    assert_eq!(rows[0]["admission"]["source_revision"], "v1.18.29");

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
        .args(["client", "install", "opencode"])
        .current_dir(home.path().join("project"));
    let output = command.output().unwrap();
    assert!(!output.status.success(), "no dispatch seam, so no install");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("client.not_dispatchable"), "{stdout}");
    assert!(
        !stdout.contains("client.unknown"),
        "a carried harness must never be unknown: {stdout}"
    );
}

/// Issue #394 K4: the admission descriptor declares the edition its evidence
/// was gathered on; when the installed product reports a different edition,
/// the read model surfaces both and names the divergence — the designed
/// honesty law (facts surfaced, never normalised away) with no invented gate.
#[test]
fn the_admission_read_model_surfaces_a_stale_edition_instead_of_passing_silently() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("project/.aikit")).unwrap();
    fs::write(
        home.path().join("project/.aikit/profile.toml"),
        "schema = 1\n",
    )
    .unwrap();
    let fixtures = stage_actuation(home.path());
    write_detection_fixture(
        &fixtures,
        r#"{"slug": "opencode", "harness_ref": "harness/opencode", "state": "detected",
            "version": "1.18.30",
            "probes": [{"kind": "config-dir", "result": "pass", "spec": "~/.config/opencode"}]}"#,
    );

    let rows = rows_with_fixtures_env(&home, &["client", "status", "opencode"]);
    assert_eq!(
        rows.len(),
        1,
        "the filter must select exactly opencode: {rows:?}"
    );
    let row = &rows[0];
    // The declared edition facts ride the row, with what the installed
    // product reports beside them.
    assert_eq!(row["admission"]["source_revision"], "v1.18.29");
    assert_eq!(row["admission"]["native_version"], Value::Null);
    assert_eq!(row["detected_version"], "1.18.30");
    let notes = row["notes"].as_array().unwrap();
    assert!(
        notes.iter().any(|note| {
            note.as_str().is_some_and(|text| {
                text.contains("pinned to edition v1.18.29") && text.contains("reports 1.18.30")
            })
        }),
        "the stale-edition mismatch must be disclosed: {notes:?}"
    );
}

#[test]
fn install_refuses_for_harnesses_without_a_dispatch_seam() {
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
    // The unchanged law: install refuses without a descriptor.
    // (pi lost its place here when the extension carrier gave it a real
    // install seam; its supported path is covered by the carrier tests.)
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

    // A record-named slug with no AIKit overlay is not installable either —
    // it is unknown to the dispatch surface, and the error says where the
    // derived roster lives.
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
        .args(["client", "install", "future-harness"])
        .current_dir(home.path().join("project"));
    let output = command.output().unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("client.unknown"), "{stdout}");
    assert!(stdout.contains("actuation harness detect"), "{stdout}");
}
