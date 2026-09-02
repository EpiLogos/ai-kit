//! The operational index.
//!
//! Every test opens a **real** SQLite file inside a temporary directory. The
//! central promise being tested is the one `ARCHITECTURE.md` §5 makes: the
//! database is *derived* and must be rebuildable from the canonical files —
//! except for the records that are genuinely operational and exist nowhere else.
//! A `reindex` that took the usage history with it would turn a routine
//! maintenance command into data loss.

mod common;

use common::*;

use aikit_core::catalog::Catalog;
use aikit_core::{Kind, Maturity, RegistrySource, Revision};
use aikit_store::events::{Event, EventAction, Outcome};
use aikit_store::index::{CapsuleFilter, Index};
use aikit_store::registry::load_registry;

fn open(dir: &std::path::Path) -> Index {
    Index::open(&dir.join("state/aikit.sqlite3")).unwrap()
}

fn seeded(dir: &std::path::Path) -> (Index, RegistryFixture) {
    let fixture = RegistryFixture::at(dir.join("registry"));
    fixture.script("script/test/nt");
    fixture.capsule(
        "skill/rust/review",
        "skill",
        "root = \"payload\"",
        "tags = [\"rust\", \"review\"]\nmaturity = \"stable\"",
        &[("payload/SKILL.md", "# review\n")],
    );
    fixture.profile("profile/code/rust", "enable = [\"script/test/nt\"]");

    let mut index = open(dir);
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    index.reindex(&load).unwrap();
    (index, fixture)
}

// ---------------------------------------------------------------------------
// The file itself
// ---------------------------------------------------------------------------

#[test]
fn opening_creates_the_database_in_write_ahead_logging_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("state/aikit.sqlite3");
    let index = Index::open(&path).unwrap();

    assert!(path.is_file(), "the database file should have been created");
    assert_eq!(index.journal_mode().unwrap().to_lowercase(), "wal");
}

#[test]
fn the_schema_version_is_recorded_and_reopening_does_not_migrate_again() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("state/aikit.sqlite3");

    let first = Index::open(&path).unwrap();
    let version = first.schema_version().unwrap();
    assert!(version >= 1);
    drop(first);

    let second = Index::open(&path).unwrap();
    assert_eq!(second.schema_version().unwrap(), version);
    assert_eq!(
        second.applied_migrations().unwrap().len() as u32,
        version,
        "one row per applied migration"
    );
}

#[test]
fn every_documented_table_exists_after_opening() {
    let tmp = tempfile::tempdir().unwrap();
    let index = open(tmp.path());
    let tables = index.tables().unwrap();
    for expected in [
        "schema_version",
        "capsules",
        "profiles",
        "usage_events",
        "contexts",
        "context_bindings",
        "generations",
        "candidates",
        "trust",
        "bypasses",
        "inbox_items",
    ] {
        assert!(
            tables.iter().any(|t| t == expected),
            "table `{expected}` is missing; have {tables:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Indexing the canonical files
// ---------------------------------------------------------------------------

#[test]
fn reindexing_writes_a_row_per_capsule_with_the_facts_the_palette_needs() {
    let tmp = tempfile::tempdir().unwrap();
    let (index, _fixture) = seeded(tmp.path());

    let rows = index.capsules().unwrap();
    assert_eq!(rows.len(), 2);

    let review = rows
        .iter()
        .find(|r| r.id == cid("skill/rust/review"))
        .unwrap();
    assert_eq!(review.kind, Kind::Skill);
    assert_eq!(review.maturity, Maturity::Stable);
    assert_eq!(review.tags, vec!["rust".to_string(), "review".to_string()]);
    assert_eq!(review.source, RegistrySource::personal());
    assert!(!review.revision.as_str().is_empty());
    assert!(review.description.contains("skill"));

    let nt = rows.iter().find(|r| r.id == cid("script/test/nt")).unwrap();
    assert_eq!(nt.exports, vec!["nt".to_string()]);
}

#[test]
fn reindexing_writes_the_profiles_too() {
    let tmp = tempfile::tempdir().unwrap();
    let (index, _fixture) = seeded(tmp.path());
    let profiles = index.profiles().unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].id, pid("profile/code/rust"));
}

#[test]
fn a_capsule_deleted_from_disk_disappears_from_the_index_on_the_next_reindex() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut index, fixture) = seeded(tmp.path());

    std::fs::remove_dir_all(fixture.capsule_dir("script/test/nt")).unwrap();
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    index.reindex(&load).unwrap();

    let ids: Vec<String> = index
        .capsules()
        .unwrap()
        .iter()
        .map(|r| r.id.to_string())
        .collect();
    assert_eq!(ids, vec!["skill/rust/review"]);
}

#[test]
fn an_edited_payload_shows_up_as_a_new_revision_in_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut index, fixture) = seeded(tmp.path());
    let before = index
        .capsule(&cid("script/test/nt"))
        .unwrap()
        .unwrap()
        .revision;

    fixture.write_payload(
        "script/test/nt",
        "payload/run.sh",
        "#!/bin/sh\necho changed\n",
    );
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    index.reindex(&load).unwrap();

    let after = index
        .capsule(&cid("script/test/nt"))
        .unwrap()
        .unwrap()
        .revision;
    assert_ne!(before, after);
}

#[test]
fn reindexing_twice_in_a_row_is_stable() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut index, fixture) = seeded(tmp.path());
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    index.reindex(&load).unwrap();
    index.reindex(&load).unwrap();
    assert_eq!(index.capsules().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// Operational records survive a rebuild
// ---------------------------------------------------------------------------

#[test]
fn a_reindex_preserves_usage_events_because_they_exist_nowhere_else() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut index, fixture) = seeded(tmp.path());

    let event = Event::new(EventAction::Run)
        .for_capsule(cid("script/test/nt"), Revision::from_raw("rev-1"))
        .with_outcome(Outcome::Success);
    index.record_event(&event).unwrap();
    assert_eq!(index.event_count().unwrap(), 1);

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    index.reindex(&load).unwrap();

    assert_eq!(
        index.event_count().unwrap(),
        1,
        "reindexing the derived catalog must not delete the usage history"
    );
    let stats = index.usage(&cid("script/test/nt")).unwrap();
    assert_eq!(stats.successful_runs, 1);
}

#[test]
fn a_reindex_preserves_context_bindings_and_issued_bypasses() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut index, fixture) = seeded(tmp.path());

    let context = aikit_core::ContextId::generate();
    let session = aikit_core::SessionId::generate();
    index
        .put_binding(&aikit_core::ContextBinding {
            context_id: context.clone(),
            session_id: session.clone(),
            mux: aikit_core::MuxKind::Tmux,
            mux_session: Some("payments".into()),
            mux_surface: Some("%3".into()),
            project_root: Some("/work/payments".into()),
            isolation: aikit_core::Isolation::Shared,
        })
        .unwrap();
    index
        .issue_bypass(
            &context,
            &aikit_core::BypassToken::new(aikit_core::BypassScope::NextEvent)
                .with_reason("the gate is wrong about generated code"),
        )
        .unwrap();

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    index.reindex(&load).unwrap();

    assert_eq!(index.bindings().unwrap().len(), 1);
    assert_eq!(index.open_bypasses(&context).unwrap().len(), 1);
}

#[test]
fn usage_counts_separate_successes_from_failures() {
    let tmp = tempfile::tempdir().unwrap();
    let (index, _fixture) = seeded(tmp.path());
    let id = cid("script/test/nt");

    for outcome in [
        Outcome::Success,
        Outcome::Success,
        Outcome::Failure {
            code: "run.exit_status".into(),
        },
    ] {
        index
            .record_event(
                &Event::new(EventAction::Run)
                    .for_capsule(id.clone(), Revision::from_raw("rev-1"))
                    .with_outcome(outcome),
            )
            .unwrap();
    }

    let stats = index.usage(&id).unwrap();
    assert_eq!(stats.successful_runs, 2);
    assert_eq!(stats.failed_runs, 1);
    assert!(
        stats.last_success_age.is_some(),
        "ranking decays on the age of the last success, so it has to be known"
    );
}

#[test]
fn usage_for_a_capsule_that_has_never_run_is_zeroed_rather_than_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let (index, _fixture) = seeded(tmp.path());
    let stats = index.usage(&cid("skill/rust/review")).unwrap();
    assert_eq!(stats.successful_runs, 0);
    assert_eq!(stats.last_success_age, None);
}

// ---------------------------------------------------------------------------
// Facets and filtering
// ---------------------------------------------------------------------------

#[test]
fn facets_count_the_catalog_by_the_dimensions_the_palette_offers() {
    let tmp = tempfile::tempdir().unwrap();
    let (index, _fixture) = seeded(tmp.path());
    let facets = index.facets().unwrap();

    assert_eq!(facets.kinds.get(&Kind::Script), Some(&1));
    assert_eq!(facets.kinds.get(&Kind::Skill), Some(&1));
    assert_eq!(facets.tags.get("rust"), Some(&1));
    assert_eq!(facets.sources.get(RegistrySource::PERSONAL), Some(&2));
    assert_eq!(facets.maturities.get(&Maturity::Stable), Some(&1));
    assert_eq!(facets.maturities.get(&Maturity::Draft), Some(&1));
}

#[test]
fn filtering_by_kind_and_tag_narrows_the_way_the_query_language_says_it_does() {
    let tmp = tempfile::tempdir().unwrap();
    let (index, _fixture) = seeded(tmp.path());

    let skills = index
        .find(&CapsuleFilter::default().with_kind(Kind::Skill))
        .unwrap();
    assert_eq!(skills.len(), 1);

    let tagged = index
        .find(&CapsuleFilter::default().with_tag("rust"))
        .unwrap();
    assert_eq!(tagged.len(), 1);

    let both = index
        .find(
            &CapsuleFilter::default()
                .with_kind(Kind::Script)
                .with_tag("rust"),
        )
        .unwrap();
    assert!(
        both.is_empty(),
        "different filter keys narrow; they do not widen"
    );
}

#[test]
fn text_filtering_matches_the_fields_a_person_would_expect_to_search() {
    let tmp = tempfile::tempdir().unwrap();
    let (index, _fixture) = seeded(tmp.path());

    assert_eq!(
        index
            .find(&CapsuleFilter::default().with_text("review"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        index
            .find(&CapsuleFilter::default().with_text("nt"))
            .unwrap()
            .len(),
        1
    );
    assert!(index
        .find(&CapsuleFilter::default().with_text("nothing-like-this"))
        .unwrap()
        .is_empty());
}

// ---------------------------------------------------------------------------
// The index as a catalog
// ---------------------------------------------------------------------------

#[test]
fn a_loaded_snapshot_is_a_catalog_the_resolver_can_use_directly() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.script("script/test/nt");
    fixture.profile("profile/code/rust", "enable = [\"script/test/nt\"]");

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    let catalog: &dyn Catalog = &load.catalog;

    assert!(catalog.get(&cid("script/test/nt")).is_some());
    assert!(catalog.profile(&pid("profile/code/rust")).is_some());
    assert!(!catalog.catalog_revision().is_empty());
}
