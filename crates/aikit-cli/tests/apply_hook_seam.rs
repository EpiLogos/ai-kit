//! `aikit apply` keeps the project's own managed hook seam current: codex's
//! per-project `.codex/hooks.json`, the seam the Actuation descriptor declares
//! and codex's own `[hooks.state]` entries prove the harness reads.
//!
//! The acceptance law: applying a project installs the project-scoped managed
//! hook seam through the same procedure pipeline as `aikit client install` —
//! foreign entries preserved, AIKit's stale entries swept — a second apply is
//! satisfied rather than a second install, the dispatcher entries the seam
//! carries actually dispatch, the doctor's `dispatch.chain` projection leg
//! reads the seam as installed, and a harness whose descriptor cannot be read
//! is disclosed as a refusal without failing the apply.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

const FOREIGN_HOOKS: &str = r#"{
  "hooks": {
    "session_start": [
      { "hooks": [ { "type": "command", "command": "my-own-guard" } ] }
    ]
  }
}"#;

/// A stand-in `actuation` binary serving the fixture descriptor for `codex`
/// only, exactly as Actuation refuses undeclared slugs.
fn stage_actuation(home: &Path) -> PathBuf {
    let bin_dir = home.join("actuation-bin");
    let fixtures = home.join("actuation-fixtures");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(fixtures.join("descriptors")).unwrap();
    let script = bin_dir.join("actuation");
    fs::write(
        &script,
        "#!/bin/sh\ncase \"$1 $2\" in\n  \"harness capability\") if [ -f \"$FIXTURES/descriptors/$3.json\" ]; then cat \"$FIXTURES/descriptors/$3.json\"; exit 0; fi\n    echo \"actuation: no capability descriptor declared for harness $3; declared: fixture\" >&2\n    exit 2 ;;\n  *) echo \"unexpected argv: $*\" >&2; exit 3 ;;\nesac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fixtures
}

/// The codex capability fixture mirrors the live descriptor's seam shape: a
/// project-relative `.codex/hooks.json` carrying the three hooks-json-file
/// events; the notify boundary rides config.toml and is never projected here.
fn write_codex_descriptor(fixtures: &Path) {
    fs::write(
        fixtures.join("descriptors").join("codex.json"),
        r#"{
  "schema": "actuation.harness-capability/v1",
  "document": "capability-read-model",
  "capability": {
    "schema": "actuation.harness-capability/v1",
    "document": "capability",
    "harness_slug": "codex",
    "native_events": [
      { "event": "session-start", "native_name": "session_start", "transport": "hooks-json-file", "can_block": false, "context_channel": "none" },
      { "event": "pre-tool-use", "native_name": "pre_tool_use", "transport": "hooks-json-file", "can_block": false, "context_channel": "none" },
      { "event": "post-tool-use", "native_name": "post_tool_use", "transport": "hooks-json-file", "can_block": false, "context_channel": "none" },
      { "event": "notification", "native_name": "notify: agent-turn-complete", "transport": "toml-notify", "can_block": false, "context_channel": "none" }
    ],
    "injection_channel": { "kind": "none", "mechanism": "fixture" },
    "blocking_semantics": { "kind": "none" },
    "wake_capability": { "kind": "none" },
    "install_seam": {
      "config_path": ".codex/hooks.json",
      "format": "json",
      "entry_shape": "per-event hook command entries",
      "ownership_marker": "hook command resolves to the AIKit dispatch executable",
      "preserves_foreign_entries": true
    },
    "uninstall_seam": {
      "config_path": ".codex/hooks.json",
      "format": "json",
      "entry_shape": "entries whose command matches the ownership marker are removed",
      "ownership_marker": "hook command resolves to the AIKit dispatch executable",
      "preserves_foreign_entries": true
    },
    "provenance": { "authored_by": "fixture", "catalog_revision": 7 }
  }
}"#,
    )
    .unwrap();
}

/// A project with `.aikit`, a foreign hook entry already in the seam, and —
/// unless `with_actuation` is false — the stub binary on PATH.
fn scenario(with_actuation: bool) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join("project/.aikit")).unwrap();
    fs::write(
        home.path().join("project/.aikit/profile.toml"),
        "schema = 1\n",
    )
    .unwrap();
    fs::create_dir_all(home.path().join("project/.codex")).unwrap();
    fs::write(home.path().join("project/.codex/hooks.json"), FOREIGN_HOOKS).unwrap();
    let fixtures = stage_actuation(home.path());
    write_codex_descriptor(&fixtures);
    if !with_actuation {
        // Remove the stub: the scenario's live truth is an unreadable intake.
        fs::remove_dir_all(home.path().join("actuation-bin")).unwrap();
    }
    home
}

fn aikit(home: &Path) -> Command {
    let mut command = Command::cargo_bin("aikit").unwrap();
    command
        .env("AIKIT_HOME", home.join("aikit-home"))
        .env("HOME", home.join("user-home"))
        .env("FIXTURES", home.join("actuation-fixtures"))
        .arg("--json")
        .current_dir(home.join("project"));
    if home.join("actuation-bin").exists() {
        command.env(
            "PATH",
            format!("{}:/usr/bin:/bin", home.join("actuation-bin").display()),
        );
    } else {
        command.env("PATH", "/usr/bin:/bin");
    }
    command
}

fn envelope(home: &Path, args: &[&str]) -> Value {
    let output = aikit(home).args(args).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "`aikit {args:?}` must succeed: {stdout} {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(&stdout).unwrap()
}

fn seam(home: &Path) -> String {
    fs::read_to_string(home.join("project/.codex/hooks.json")).unwrap()
}

#[test]
fn apply_installs_the_project_hook_seam_and_the_doctor_leg_reads_it_installed() {
    let home = scenario(true);

    // The apply: the seam merges AIKit's dispatch entries around the foreign
    // entry, through a reversible procedure, and says so in its reply.
    let applied = envelope(home.path(), &["apply"]);
    let seams = applied["data"]["hook_seams"].as_array().unwrap();
    assert_eq!(
        seams.len(),
        1,
        "codex is the project-scoped seam: {seams:?}"
    );
    assert_eq!(seams[0]["client"], "codex");
    assert_eq!(seams[0]["state"], "installed");
    assert!(
        seams[0]["undo"]
            .as_str()
            .is_some_and(|undo| undo.starts_with("aikit procedure undo")),
        "the install is reversible: {seams:?}"
    );
    assert!(
        applied["warnings"].as_array().unwrap().is_empty(),
        "nothing is refused here: {:?}",
        applied["warnings"]
    );

    let merged = seam(home.path());
    assert!(
        merged.contains("aikit hook dispatch codex SessionStart"),
        "the dispatched event lands: {merged}"
    );
    assert!(
        merged.contains("aikit hook dispatch codex PreToolUse")
            && merged.contains("aikit hook dispatch codex PostToolUse"),
        "every hooks-json-file event lands: {merged}"
    );
    assert!(
        merged.contains("my-own-guard"),
        "the foreign entry is preserved: {merged}"
    );
    assert!(
        !merged.contains("agent-turn-complete") && !merged.contains("Notification"),
        "the toml-notify boundary is disclosed by the descriptor, never projected into the seam: {merged}"
    );

    // The dispatcher the seam names actually dispatches.
    let dispatch = envelope(home.path(), &["hook", "dispatch", "codex", "SessionStart"]);
    assert_eq!(
        dispatch["data"]["allowed"], true,
        "the SessionStart dispatch answers: {}",
        dispatch["data"]
    );

    // The doctor's dispatch.chain projection leg reads the same seam.
    let doctor = envelope(home.path(), &["doctor"]);
    let codex_chain = doctor["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| {
            finding["check"] == "dispatch.chain"
                && finding["summary"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("dispatch chain for codex"))
        })
        .unwrap_or_else(|| panic!("doctor reports codex's dispatch chain: {doctor}"));
    assert!(
        codex_chain["summary"]
            .as_str()
            .unwrap()
            .contains("projection installed"),
        "the codex projection leg goes green: {}",
        codex_chain["summary"]
    );

    // The merge is byte-idempotent: re-applying with nothing changed rewrites
    // nothing (the merge output is identical, and the procedure digest — which
    // binds the seam's current bytes — converges once the seam matches it).
    envelope(home.path(), &["apply"]);
    assert_eq!(
        seam(home.path()),
        merged,
        "re-applying with nothing changed leaves the seam byte-identical"
    );
    let converged = envelope(home.path(), &["apply"]);
    let seams = converged["data"]["hook_seams"].as_array().unwrap();
    assert_eq!(
        seams[0]["state"], "satisfied",
        "once the seam matches the merge output, apply is satisfied, never a \
         fresh install: {seams:?}"
    );
}

#[test]
fn apply_discloses_a_refused_hook_seam_instead_of_failing() {
    let home = scenario(false);

    let refused = envelope(home.path(), &["apply"]);
    let seams = refused["data"]["hook_seams"].as_array().unwrap();
    assert_eq!(seams[0]["client"], "codex");
    assert_eq!(seams[0]["state"], "refused");
    assert!(
        seams[0]["reason"].as_str().unwrap().contains("capability"),
        "the refusal names the unreadable descriptor: {seams:?}"
    );
    assert!(
        refused["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap()
                .contains("codex hook seam not installed")),
        "the refusal surfaces as a warning: {:?}",
        refused["warnings"]
    );
    // Nothing was written: the foreign seam stands byte-identical.
    assert_eq!(
        seam(home.path()),
        FOREIGN_HOOKS,
        "a refused install writes nothing"
    );
    let untouched = seam(home.path());
    assert!(
        untouched.contains("my-own-guard") && !untouched.contains("aikit hook dispatch"),
        "a refused install writes nothing: {untouched}"
    );
}
