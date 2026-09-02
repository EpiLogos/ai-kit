//! `aikit apply --label` attaches a cosmetic label to the generation, and it is
//! read back by `status` (via `Service::current_generation_properties`). The
//! label is excluded from the generation's identity, so labelling an unchanged
//! view updates the label in place rather than minting a second generation.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use aikit_cli::app::{AikitApplication, ApplyRequest, Service};
use aikit_core::scope::ScopeKind;
use aikit_store::home::AikitHome;
use tempfile::TempDir;

const CONTEXT_ID: &str = "ctx_01HZYLABEL000000000000000";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn seed(home: &Path) {
    let base = home.join("registries/personal/capsules/script/demo/greet");
    write(
        &base.join("manifest.toml"),
        r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "A capsule for the label test."

[script]
entry = "payload/run.sh"
exports = ["greet"]
"#,
    );
    let run = base.join("payload/run.sh");
    write(&run, "#!/bin/sh\necho hi\n");
    let mut perms = fs::metadata(&run).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&run, perms).unwrap();
}

fn service(home_path: &Path, project: &Path) -> Service {
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    Service::open(AikitHome::at(home_path), project, move |k| {
        env.get(k).cloned()
    })
    .unwrap()
}

#[test]
fn apply_label_attaches_a_label_that_status_reads_back() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    seed(home.path());
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    let mut svc = service(home.path(), project.path());
    svc.apply(ApplyRequest {
        scope: ScopeKind::Project,
        toggles: vec![],
        label: Some("known-good".to_string()),
    })
    .expect("apply with a label");

    let props = svc.current_generation_properties();
    assert_eq!(
        props.get("label").map(String::as_str),
        Some("known-good"),
        "status reads the label off the current generation"
    );
}

#[test]
fn relabelling_an_unchanged_view_does_not_mint_a_second_generation() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    seed(home.path());
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    let mut svc = service(home.path(), project.path());
    let first = svc
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .unwrap();

    let mut svc = service(home.path(), project.path());
    let again = svc
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: Some("known-good".to_string()),
        })
        .unwrap();

    assert_eq!(
        first.id, again.id,
        "an identical view with only a new label is the same generation"
    );

    let generations = home
        .path()
        .join("state/contexts")
        .join(CONTEXT_ID)
        .join("generations");
    let count = fs::read_dir(&generations)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("gen_"))
        .count();
    assert_eq!(
        count, 1,
        "labelling must not create a near-duplicate generation"
    );

    assert_eq!(
        svc.current_generation_properties()
            .get("label")
            .map(String::as_str),
        Some("known-good"),
    );
}
