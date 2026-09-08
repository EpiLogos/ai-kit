//! W4 project activity evidence against a real SQLite database.

mod common;

use std::path::{Path, PathBuf};

use aikit_core::{ContextId, RegistrySource};
use aikit_store::index::{Index, ProjectActivityEvidence};
use aikit_store::registry::load_registry;
use aikit_store::Timestamp;

use common::RegistryFixture;

fn open(dir: &Path) -> Index {
    Index::open(&dir.join("state/aikit.sqlite3")).unwrap()
}

fn evidence(
    root: &Path,
    nanos: i64,
    context_id: &ContextId,
    tool: &str,
    path: &str,
) -> ProjectActivityEvidence {
    ProjectActivityEvidence::new(
        root,
        Timestamp::from_nanos(nanos),
        context_id.clone(),
        Some(tool.to_string()),
        Some(PathBuf::from(path)),
    )
}

#[test]
fn latest_activity_is_attributed_isolated_and_survives_reopen_and_reindex() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("state/aikit.sqlite3");
    let project_a = tmp.path().join("project-a");
    let project_b = tmp.path().join("project-b");
    let context_a = ContextId::generate();
    let context_b = ContextId::generate();

    let mut index = Index::open(&db_path).unwrap();
    assert!(index
        .applied_migrations()
        .unwrap()
        .contains(&"0007-project-activity-evidence".to_string()));
    assert_eq!(index.project_last_activity(&project_a).unwrap(), None);

    let older = evidence(&project_a, 10, &context_a, "Read", "src/old.rs");
    let newer = evidence(&project_a, 20, &context_a, "Edit", "src/new.rs");
    let other = evidence(&project_b, 30, &context_b, "Write", "README.md");
    index.record_project_activity(&older).unwrap();
    index.record_project_activity(&newer).unwrap();
    index.record_project_activity(&other).unwrap();

    assert_eq!(
        index.project_last_activity(&project_a).unwrap(),
        Some(newer.clone())
    );
    assert_eq!(
        index.project_last_activity(&project_b).unwrap(),
        Some(other)
    );

    // A routine catalogue rebuild may replace only derived tables. This
    // operational evidence must remain byte-for-byte addressable afterwards.
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.script("script/test/activity");
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    index.reindex(&load).unwrap();
    assert_eq!(
        index.project_last_activity(&project_a).unwrap(),
        Some(newer.clone())
    );
    drop(index);

    let reopened = Index::open(&db_path).unwrap();
    assert_eq!(
        reopened.project_last_activity(&project_a).unwrap(),
        Some(newer)
    );
}

#[test]
fn equal_timestamps_remain_distinct_append_only_receipts() {
    let tmp = tempfile::tempdir().unwrap();
    let index = open(tmp.path());
    let project = tmp.path().join("project");
    let context = ContextId::generate();
    let first = evidence(&project, 42, &context, "Read", "one.rs");
    let second = evidence(&project, 42, &context, "Read", "two.rs");

    index.record_project_activity(&first).unwrap();
    index.record_project_activity(&second).unwrap();
    let latest = index.project_last_activity(&project).unwrap().unwrap();
    assert!(latest == first || latest == second);
    assert_ne!(first.evidence_id, second.evidence_id);
}
