//! Canonical-vs-projected skill content drift is visible in the native read
//! models.
//!
//! The defect this pins: `aikit diff` reported only set membership
//! (`would_add: []`, `would_drop: []`) while nine projected skill copies kept
//! serving pre-edit bytes after their canonical sources moved on — stale
//! practice with no signal. These tests hold the fix: `aikit diff` names the
//! drifted skills with both paths, `aikit status` counts them and warns, a
//! clean projection stays silent, and a deleted canonical source is reported
//! as drift (named for what it is), never as noise.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn skill(root: &Path, name: &str, marker: &str) {
    write(
        &root.join("SKILL.md"),
        &format!(
            "---\nname: {name}\ndescription: Projection drift visibility fixture.\ndisable-model-invocation: true\n---\n\n{marker}\n"
        ),
    );
    write(
        &root.join("references/rules.md"),
        &format!("rules:{marker}\n"),
    );
}

fn aikit(home: &Path, project: &Path, args: &[&str]) -> Value {
    let output = Command::cargo_bin("aikit")
        .unwrap()
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "aikit {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn text(home: &Path, project: &Path, args: &[&str]) -> String {
    let output = Command::cargo_bin("aikit")
        .unwrap()
        .args(args)
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A home with one directory source (`canon`) carrying one skill (`drifted`),
/// synced, promoted, enabled and applied — so a real generation with a real
/// projection exists, exactly as a drifted machine has.
fn applied() -> (tempfile::TempDir, tempfile::TempDir, PathBuf, PathBuf) {
    let home = tempfile::TempDir::new().unwrap();
    let project = tempfile::TempDir::new().unwrap();
    let source = home.path().join("canonical-skills");
    let skill_dir = source.join("drifted");
    skill(&skill_dir, "drifted", "version-one");
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    aikit(
        home.path(),
        project.path(),
        &["source", "add-directory", "canon", source.to_str().unwrap()],
    );
    aikit(home.path(), project.path(), &["source", "sync", "canon"]);
    aikit(
        home.path(),
        project.path(),
        &["source", "promote", "canon", "--trust"],
    );
    aikit(
        home.path(),
        project.path(),
        &["enable", "skill/canon/drifted", "--scope", "global"],
    );
    aikit(home.path(), project.path(), &["apply"]);
    (home, project, source, skill_dir)
}

#[test]
fn identical_projection_reports_no_drift() {
    let (home, project, _source, _skill_dir) = applied();
    let diff = aikit(home.path(), project.path(), &["diff"]);
    assert_eq!(diff["data"]["content_drift"], Value::Array(vec![]));
    assert_eq!(diff["data"]["content_drift_count"], 0);

    let status = aikit(home.path(), project.path(), &["status"]);
    assert_eq!(status["data"]["content_drift_count"], 0);
    let warning = status["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w.as_str().unwrap_or_default().contains("canonical sources"))
        .cloned();
    assert!(
        warning.is_none(),
        "a clean projection must not warn: {warning:?}"
    );
    let plain = text(home.path(), project.path(), &["status"]);
    assert!(
        !plain.contains("Skill content drift:"),
        "clean projection must not carry a drift line: {plain}"
    );
}

#[test]
fn canonical_edited_after_projection_names_the_drifted_skill_with_both_paths() {
    let (home, project, source, skill_dir) = applied();
    // The canonical ground moves on after the projection materialised — the
    // exact shape of the proven defect.
    skill(&skill_dir, "drifted", "version-two");
    write(&skill_dir.join("references/added.md"), "added later\n");
    fs::remove_file(skill_dir.join("references/rules.md")).unwrap();

    let diff = aikit(home.path(), project.path(), &["diff"]);
    assert_eq!(diff["data"]["content_drift_count"], 1);
    let entry = &diff["data"]["content_drift"][0];
    assert_eq!(entry["skill"], "skill/canon/drifted");
    assert_eq!(entry["kind"], "content_drift");
    // The source spec stores the canonical root, so compare canonically too.
    let canonical_source = fs::canonicalize(&source).unwrap().join("drifted");
    assert_eq!(entry["source"], canonical_source.to_str().unwrap());
    let projected = entry["projected"].as_str().unwrap();
    assert!(projected.contains("current/projections/"), "{projected}");
    assert!(projected.ends_with("skills/drifted"), "{projected}");
    assert_eq!(entry["direction"], "canonical-newer");
    let files: Vec<&str> = entry["differing_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        vec!["SKILL.md", "references/added.md", "references/rules.md"]
    );
    let summary = entry["summary"].as_str().unwrap();
    assert!(
        summary.starts_with("content_drift: skill/canon/drifted ("),
        "{summary}"
    );
    assert!(summary.contains(" vs "), "{summary}");
    assert!(summary.contains(", canonical-newer)"), "{summary}");

    // The entry names the owning context and the exact native repair, so
    // detection alone is enough to act on — the defect that motivated this
    // read model was drift nobody could see, let alone repair.
    let context_id = entry["context_id"]
        .as_str()
        .expect("entry names its context");
    assert!(context_id.starts_with("ctx_"), "{context_id}");
    let repair = entry["repair"].as_str().expect("entry carries its repair");
    assert!(
        repair.contains(&format!("AIKIT_CONTEXT_ID={context_id} aikit apply")),
        "{repair}"
    );
    assert!(repair.contains("-C "), "{repair}");
    assert!(
        summary.contains(&format!("repair: AIKIT_CONTEXT_ID={context_id}")),
        "{summary}"
    );

    // Set membership cannot see any of this; the stage must stay quiet so the
    // two readings stay distinguishable.
    assert_eq!(diff["data"]["would_add"], Value::Array(vec![]));
    assert_eq!(diff["data"]["would_drop"], Value::Array(vec![]));

    let status = aikit(home.path(), project.path(), &["status"]);
    assert_eq!(status["data"]["content_drift_count"], 1);
    let warnings: Vec<&str> = status["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    assert!(
        warnings.iter().any(
            |w| w.contains("1 projected skill differs from its canonical sources")
                && w.contains("`aikit diff`")
        ),
        "{warnings:?}"
    );
    let plain = text(home.path(), project.path(), &["status"]);
    assert!(
        plain.contains("Skill content drift: 1 projected copy differ"),
        "{plain}"
    );
}

#[test]
fn missing_canonical_source_is_reported_as_drift_not_silence() {
    let (home, project, source, skill_dir) = applied();
    fs::remove_dir_all(&skill_dir).unwrap();

    let diff = aikit(home.path(), project.path(), &["diff"]);
    assert_eq!(diff["data"]["content_drift_count"], 1);
    let entry = &diff["data"]["content_drift"][0];
    assert_eq!(entry["skill"], "skill/canon/drifted");
    assert_eq!(entry["kind"], "canonical_missing");
    let canonical_source = fs::canonicalize(&source).unwrap().join("drifted");
    assert_eq!(entry["source"], canonical_source.to_str().unwrap());
    assert_eq!(entry["direction"], "undetermined");
    assert_eq!(entry["differing_files"], Value::Array(vec![]));
    let summary = entry["summary"].as_str().unwrap();
    assert!(
        summary.starts_with("canonical_missing: skill/canon/drifted ("),
        "{summary}"
    );

    let status = aikit(home.path(), project.path(), &["status"]);
    assert_eq!(status["data"]["content_drift_count"], 1);
}

#[test]
fn projections_without_a_reachable_directory_source_stay_silent() {
    // A payload the trace cannot map to a live directory source (here: a
    // hand-made projection pointing outside any source spec) must not invent
    // drift — today's behaviour for the unreachable keeps standing.
    let (home, project, _source, _skill_dir) = applied();
    let payload = home.path().join("unmanaged/payload");
    write(&payload.join("SKILL.md"), "unmanaged\n");
    let projection = home
        .path()
        .join("state/contexts/ctx_UNMANAGED0000000000000000000/current/projections/codex/.agents/skills/unmanaged");
    fs::create_dir_all(projection.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&payload, &projection).unwrap();

    let diff = aikit(home.path(), project.path(), &["diff"]);
    assert_eq!(diff["data"]["content_drift"], Value::Array(vec![]));
    assert_eq!(diff["data"]["content_drift_count"], 0);
}
