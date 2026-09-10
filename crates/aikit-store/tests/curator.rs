//! The curator (L4) and the drift detector (L2): the system observing its own
//! tree and proposing to the inbox, never acting on its own. Real SQLite, real
//! registry loads, real content-hash revisions.

mod common;
use common::*;

use std::path::Path;

use aikit_core::id::RegistrySource;
use aikit_core::lifecycle::LifecycleThresholds;

use aikit_store::channel::{InboxChannel, InboxKind};
use aikit_store::curator::{curate, detect_drift, report_drift};
use aikit_store::events::{Event, EventAction, Outcome, Timestamp};
use aikit_store::index::Index;
use aikit_store::registry::load_registry;

const DAY_NS: i64 = 24 * 60 * 60 * 1_000_000_000;

fn index(dir: &Path) -> Index {
    Index::open(&dir.join("state/aikit.sqlite3")).unwrap()
}

fn ran(index: &Index, id: &str, at: Timestamp) {
    let event = Event::new(EventAction::Run)
        .for_capsule(cid(id), aikit_core::Revision::from_raw("rev-usage"))
        .with_outcome(Outcome::Success)
        .at(at);
    index.record_event(&event).unwrap();
}

#[test]
fn the_curator_proposes_review_of_stale_capabilities_and_archives_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.script("script/test/old");
    fixture.script("script/test/fresh");

    let mut index = index(tmp.path());
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    index.reindex(&load).unwrap();

    let now = Timestamp::now();
    ran(
        &index,
        "script/test/old",
        Timestamp::from_nanos(now.as_nanos() - 200 * DAY_NS),
    );
    ran(
        &index,
        "script/test/fresh",
        Timestamp::from_nanos(now.as_nanos() - DAY_NS),
    );

    let report = curate(&index, &LifecycleThresholds::default(), now).unwrap();

    assert_eq!(
        report.stale.len(),
        1,
        "only the long-idle capability is stale"
    );
    assert_eq!(report.stale[0].id, cid("script/test/old"));

    let item = report.published.expect("a stale tree files a report");
    assert_eq!(item.kind, InboxKind::ProcedureReport);
    assert!(item.body.contains("script/test/old"));
    assert!(
        !item.body.contains("script/test/fresh"),
        "fresh is not flagged"
    );

    // The curator only proposes: nothing was archived, the capsule is still
    // catalogued exactly as before.
    assert!(
        index.capsule(&cid("script/test/old")).unwrap().is_some(),
        "the stale capability must remain catalogued for audit"
    );

    // Re-running over the unchanged tree returns the same item, not another.
    let again = curate(&index, &LifecycleThresholds::default(), now).unwrap();
    assert_eq!(again.published.unwrap().id, item.id);
    assert_eq!(
        InboxChannel::new(&index).items().unwrap().len(),
        1,
        "the curator does not nag"
    );
}

#[test]
fn a_healthy_tree_files_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.script("script/test/fresh");
    let mut index = index(tmp.path());
    index
        .reindex(&load_registry(fixture.root(), RegistrySource::personal()).unwrap())
        .unwrap();

    let now = Timestamp::now();
    ran(
        &index,
        "script/test/fresh",
        Timestamp::from_nanos(now.as_nanos() - DAY_NS),
    );

    let report = curate(&index, &LifecycleThresholds::default(), now).unwrap();
    assert!(report.stale.is_empty());
    assert!(report.published.is_none(), "nothing stale, nothing filed");
    assert_eq!(InboxChannel::new(&index).items().unwrap().len(), 0);
}

#[test]
fn drift_is_detected_when_a_projected_payload_changes_and_filed_as_a_notice() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.script("script/test/nt");

    // Resolve against the payload as it is now → the view records revision A.
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let index = index(tmp.path());

    // The projected payload is edited out of band → a new content revision B.
    fixture.write_payload(
        "script/test/nt",
        "payload/run.sh",
        "#!/bin/sh\necho CHANGED\n",
    );
    let reloaded = load_registry(fixture.root(), RegistrySource::personal()).unwrap();

    let drifts = detect_drift(&resolved.view, &reloaded.catalog);
    assert_eq!(drifts.len(), 1, "the edited capsule drifted");
    assert_eq!(drifts[0].id, cid("script/test/nt"));
    assert_ne!(
        Some(&drifts[0].applied),
        drifts[0].current.as_ref(),
        "the applied and current revisions differ"
    );

    let items = report_drift(&index, &drifts, None).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, InboxKind::DriftNotice);
    assert!(items[0].title.contains("script/test/nt"));

    // Idempotent: re-checking the same drift returns the same notice.
    let again = report_drift(&index, &drifts, None).unwrap();
    assert_eq!(again[0].id, items[0].id);
    assert_eq!(InboxChannel::new(&index).items().unwrap().len(), 1);
}

#[test]
fn an_unchanged_tree_produces_no_drift() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.script("script/test/nt");
    let resolved = resolve_fixture(&fixture, &["script/test/nt"]);
    let reloaded = load_registry(fixture.root(), RegistrySource::personal()).unwrap();

    let drifts = detect_drift(&resolved.view, &reloaded.catalog);
    assert!(drifts.is_empty(), "identical content is not drift");
}
