//! W4/CASE 06 — completed tool use becomes real, inspectable project activity
//! evidence through the production reaction function and SQLite store.

use aikit_cli::activity_evidence::{describe, record};
use aikit_cli::app::Service;
use aikit_core::catalog::Catalog;
use aikit_core::hooks::{HookEvent, HookEventKind};
use aikit_core::{CapsuleId, ContextId, TrustKey, TrustState};
use aikit_store::{AikitHome, Index, TrustStore};
use serde_json::json;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;

fn write(path: &std::path::Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn service_with_selection(
    home: &std::path::Path,
    project: &std::path::Path,
    selected: bool,
) -> Service {
    let root = home.join("registries/personal/capsules/hook/continuity/activity-evidence");
    write(
        &root.join("manifest.toml"),
        r#"schema = 1
id = "hook/continuity/activity-evidence"
kind = "hook"
name = "Activity evidence"
description = "Records attributed project activity."
[hook]
entry = "payload/activity-evidence"
events = ["PostToolUse"]
"#,
    );
    let payload = root.join("payload/activity-evidence");
    write(&payload, "#!/bin/sh\nexit 0\n");
    let mut permissions = std::fs::metadata(&payload).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&payload, permissions).unwrap();
    write(
        &project.join(".aikit/profile.toml"),
        if selected {
            "schema = 1\nenable = [\"hook/continuity/activity-evidence\"]\n"
        } else {
            "schema = 1\n"
        },
    );
    let context = ContextId::generate();
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_owned(), context.to_string());
    let mut service =
        Service::open(AikitHome::at(home), project, |key| env.get(key).cloned()).unwrap();

    // The real personal-source trust gate must be crossed before a hook can
    // become operative. Tests record the same explicit review production uses.
    let id = CapsuleId::parse("hook/continuity/activity-evidence").unwrap();
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
fn post_tool_use_records_only_attributed_metadata_and_a_relative_touched_path() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    let index = Index::open(&tmp.path().join("state/aikit.sqlite3")).unwrap();
    let context = ContextId::generate();
    let event = HookEvent::new(
        "claude",
        HookEventKind::PostToolUse,
        json!({
            "tool_input": {
                "file_path": project.join("src/lib.rs"),
                "secret_content": "this must never be persisted"
            },
            "tool_response": "also never persisted"
        }),
    )
    .with_tool_name("Edit")
    .in_cwd(&project);

    let receipt = record(&index, &context, &project, &event).unwrap();
    assert_eq!(receipt.project_root, project);
    assert_eq!(receipt.context_id, context);
    assert_eq!(receipt.tool.as_deref(), Some("Edit"));
    assert_eq!(
        receipt.touched_path.as_deref(),
        Some(std::path::Path::new("src/lib.rs"))
    );

    let stored = index
        .project_last_activity(&receipt.project_root)
        .unwrap()
        .unwrap();
    assert_eq!(stored, receipt);
    let report = describe(&stored);
    assert_eq!(report["evidence_id"], receipt.evidence_id.to_string());
    assert_eq!(report["context_id"], context.to_string());
    assert_eq!(report["tool"], "Edit");
    assert_eq!(report["touched_path"], "src/lib.rs");
    let serialized = format!("{stored:?}{report}");
    assert!(!serialized.contains("secret_content"));
    assert!(!serialized.contains("tool_response"));
}

#[test]
fn project_activity_does_not_require_a_file_path() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let index = Index::open(&tmp.path().join("state/aikit.sqlite3")).unwrap();
    let context = ContextId::generate();
    let event = HookEvent::new(
        "codex",
        HookEventKind::PostToolUse,
        json!({"tool_input": {"query": "safe metadata boundary"}}),
    )
    .with_tool_name("Search");

    let receipt = record(&index, &context, &project, &event).unwrap();
    assert_eq!(receipt.touched_path, None);
    assert_eq!(receipt.tool.as_deref(), Some("Search"));
}

#[test]
fn service_dispatch_records_only_when_the_capability_is_composed() {
    for selected in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let project = tmp.path().join("project");
        let service = service_with_selection(&home, &project, selected);
        let event = HookEvent::new(
            "claude",
            HookEventKind::PostToolUse,
            json!({"tool_input": {"file_path": project.join("src/lib.rs")}}),
        )
        .with_tool_name("Edit")
        .in_cwd(&project);

        let decision = service.dispatch_hook(&event).unwrap();
        assert!(decision.allowed);
        assert!(decision.warnings.is_empty(), "{:?}", decision.warnings);
        let stored = service.index().project_last_activity(&project).unwrap();
        assert_eq!(
            stored.is_some(),
            selected,
            "selection alone determines whether PostToolUse leaves activity evidence"
        );
    }
}
