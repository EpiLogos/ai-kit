//! Palette execution preserves the exact reviewed intent instead of rebuilding
//! it from manifest defaults after the terminal has been restored.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use aikit_cli::app::Service;
use aikit_cli::run;
use aikit_core::capsule::{ExecMode, WorkingDir};
use aikit_core::{CapsuleId, ContextId};
use aikit_store::home::AikitHome;
use aikit_tui::RunIntent;
use tempfile::TempDir;

const CONTEXT_ID: &str = "ctx_01HZYPALRUN0000000000000";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn fixture() -> (TempDir, TempDir, Service, PathBufFixture) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let capsule = home
        .path()
        .join("registries/personal/capsules/script/demo/exact");
    let marker = project.path().join("must-not-run");
    write(
        &capsule.join("manifest.toml"),
        r#"schema = 1
id = "script/demo/exact"
kind = "script"
name = "exact"
description = "Records whether an intent was silently degraded."

[script]
entry = "payload/run.sh"
interpreter = ["/bin/sh"]
mode = "foreground"
cwd = "project"

[script.env]
SOURCE = "manifest"
"#,
    );
    write(
        &capsule.join("payload/run.sh"),
        &format!("#!/bin/sh\nprintf ran > '{}'\n", marker.display()),
    );
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/exact\"]\n",
    );
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let service = Service::open(AikitHome::at(home.path()), project.path(), move |key| {
        env.get(key).cloned()
    })
    .unwrap();
    (home, project, service, PathBufFixture { capsule, marker })
}

struct PathBufFixture {
    capsule: std::path::PathBuf,
    marker: std::path::PathBuf,
}

#[test]
fn new_pane_mode_cwd_and_environment_survive_palette_teardown() {
    let (_home, _project, service, paths) = fixture();
    let intent = RunIntent {
        capsule: CapsuleId::parse("script/demo/exact").unwrap(),
        context: ContextId::parse(CONTEXT_ID).unwrap(),
        specs: Vec::new(),
        values: BTreeMap::new(),
        mode: ExecMode::NewPane,
        cwd: WorkingDir::Capsule,
        env: BTreeMap::from([("SOURCE".to_string(), "palette".to_string())]),
        requires_confirmation: false,
    };

    let command = service.plan_run_intent(&intent).unwrap();
    assert_eq!(command.mode, ExecMode::NewPane);
    assert_eq!(command.cwd, paths.capsule);
    assert_eq!(command.env, intent.env);

    let error = run::execute(&command).unwrap_err();
    assert_eq!(error.code(), "run.needs_mux");
    assert!(
        !paths.marker.exists(),
        "new-pane must never degrade into a foreground child"
    );
}
