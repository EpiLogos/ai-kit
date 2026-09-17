//! W5/CASE 11 — project recency changes only the composed root SessionStart horizon.

use aikit_cli::app::Service;
use aikit_cli::projects;
use aikit_core::catalog::Catalog;
use aikit_core::hooks::{HookEvent, HookEventKind};
use aikit_core::{CapsuleId, ContextId, TrustKey, TrustState};
use aikit_store::{AikitHome, ProjectActivityEvidence, Timestamp, TrustStore};
use serde_json::json;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;

fn write(path: &std::path::Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn root_service(home_path: &std::path::Path, cwd: &std::path::Path, selected: bool) -> Service {
    let root = home_path.join("registries/personal/capsules/hook/continuity/project-recency");
    write(
        &root.join("manifest.toml"),
        r#"schema = 1
id = "hook/continuity/project-recency"
kind = "hook"
name = "Project recency"
description = "Bounds the automatic root project horizon."
[hook]
entry = "payload/project-recency"
events = ["SessionStart"]
"#,
    );
    let payload = root.join("payload/project-recency");
    write(&payload, "#!/bin/sh\nexit 0\n");
    let mut permissions = std::fs::metadata(&payload).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&payload, permissions).unwrap();

    let home = AikitHome::at(home_path);
    home.ensure_layout().unwrap();
    write(
        &home.global_profile(),
        if selected {
            "schema = 1\nenable = [\"hook/continuity/project-recency\"]\n"
        } else {
            "schema = 1\n"
        },
    );
    let context = ContextId::generate();
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_owned(), context.to_string());
    let mut service = Service::open(home, cwd, |key| env.get(key).cloned()).unwrap();
    assert!(service.descriptor().project_root.is_none());

    let id = CapsuleId::parse("hook/continuity/project-recency").unwrap();
    let capsule = service.snapshot().get(&id).unwrap();
    let key = TrustKey::new(
        capsule.source.clone().unwrap(),
        id,
        capsule.revision.clone().unwrap(),
    );
    TrustStore::new(service.index())
        .record(&key, TrustState::Trusted, Some("test review"))
        .unwrap();
    service.refresh().unwrap();
    service
}

#[test]
fn root_session_horizon_is_opt_in_and_reacts_to_new_activity_without_reregistration() {
    for selected in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let home_path = tmp.path().join("home");
        let root_cwd = tmp.path().join("world");
        let project = tmp.path().join("project-a");
        std::fs::create_dir_all(&root_cwd).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let home = AikitHome::at(&home_path);
        home.ensure_layout().unwrap();
        let project = projects::bind(
            &home,
            "project-a",
            std::slice::from_ref(&project),
            &[],
            &[],
            true,
        )
        .unwrap()
        .directories[0]
            .clone();

        let service = root_service(&home_path, &root_cwd, selected);
        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));
        let before = service.dispatch_hook(&start).unwrap();
        assert_eq!(
            before
                .injected
                .iter()
                .any(|block| block.contains("project-recency")),
            selected,
            "selection alone determines whether the root recency horizon runs"
        );
        if selected {
            assert!(before
                .injected
                .iter()
                .any(|block| block.contains("unknown=1")));
        }

        service
            .index()
            .record_project_activity(&ProjectActivityEvidence::new(
                &project,
                Timestamp::now(),
                ContextId::generate(),
                Some("Edit".into()),
                Some("src/lib.rs".into()),
            ))
            .unwrap();
        let after = service.dispatch_hook(&start).unwrap();
        if selected {
            let horizon = after
                .injected
                .iter()
                .find(|block| block.contains("project-recency"))
                .unwrap();
            assert!(horizon.contains("project-a"));
            assert!(horizon.contains(&project.display().to_string()));
            assert!(horizon.contains("unknown=0"));
            assert_eq!(
                projects::load_all(service.home()).unwrap().len(),
                1,
                "reactivation used the receipt, not a registration rewrite"
            );
        }
    }
}
