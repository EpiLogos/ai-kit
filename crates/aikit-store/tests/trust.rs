//! Persistent trust.
//!
//! Trust is the one thing in AIKit a capsule author must not be able to reach.
//! These tests use a real database and a real registry so that both halves of
//! that promise are exercised: the manifest cannot say it, and the record that
//! *does* say it lives keyed on `(source, capsule, revision)` — so an edit moves
//! the revision and the review has to happen again.

mod common;

use common::*;

use aikit_core::catalog::Catalog;
use aikit_core::trust::TrustOracle;
use aikit_core::{RegistrySource, Revision, TrustKey, TrustState};
use aikit_store::index::Index;
use aikit_store::registry::load_registry;
use aikit_store::trust::TrustStore;

fn key(revision: &str) -> TrustKey {
    TrustKey::new(
        RegistrySource::personal(),
        cid("hook/gate/secrets"),
        Revision::from_raw(revision),
    )
}

fn index(dir: &std::path::Path) -> Index {
    Index::open(&dir.join("state/aikit.sqlite3")).unwrap()
}

// ---------------------------------------------------------------------------
// The default is refusal
// ---------------------------------------------------------------------------

#[test]
fn a_capsule_nobody_has_reviewed_is_unseen_and_may_not_project() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    assert_eq!(trust.state(&key("aaa")), TrustState::Unseen);
    assert!(!trust.state(&key("aaa")).may_project());
}

#[test]
fn indexing_a_registry_does_not_review_anything() {
    // "Catalogued is not reviewed" — a registry sync must never change live
    // behaviour, so a freshly loaded hook is still inert.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.hook("hook/gate/secrets");

    let mut index = index(tmp.path());
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    index.reindex(&load).unwrap();

    let capsule = load
        .catalog
        .get(&cid("hook/gate/secrets"))
        .cloned()
        .unwrap();
    let trust = TrustStore::new(&index);
    assert_eq!(
        trust.state_for(
            capsule.source.as_ref(),
            &capsule.id,
            capsule.revision.as_ref()
        ),
        TrustState::Unseen
    );
}

#[test]
fn a_manifest_cannot_talk_its_way_into_being_trusted() {
    // Every knob an author *can* turn — maturity, tags, provenance — is turned
    // as far as it goes here. None of them is the trust record.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.capsule(
        "hook/gate/secrets",
        "hook",
        "entry = \"payload/check\"\nevents = [\"PreToolUse\"]",
        "maturity = \"stable\"\ntags = [\"trusted\", \"reviewed\", \"official\"]\n\
         [provenance]\nsource = \"authored\"\nauthor = \"me\"",
        &[("payload/check", "#!/bin/sh\nexit 0\n")],
    );

    let index = index(tmp.path());
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);

    let capsule = load.catalog.get(&cid("hook/gate/secrets")).unwrap();
    let trust = TrustStore::new(&index);
    assert_eq!(
        trust.state_for(
            capsule.source.as_ref(),
            &capsule.id,
            capsule.revision.as_ref()
        ),
        TrustState::Unseen,
        "no manifest field may influence trust"
    );
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

#[test]
fn a_recorded_review_persists_across_reopening_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("state/aikit.sqlite3");
    {
        let index = Index::open(&path).unwrap();
        TrustStore::new(&index)
            .record(&key("aaa"), TrustState::Trusted, Some("read it line by line"))
            .unwrap();
    }
    let reopened = Index::open(&path).unwrap();
    assert_eq!(
        TrustStore::new(&reopened).state(&key("aaa")),
        TrustState::Trusted
    );
}

#[test]
fn recording_the_same_revision_twice_updates_rather_than_duplicates() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    trust.record(&key("aaa"), TrustState::Reviewed, None).unwrap();
    trust.record(&key("aaa"), TrustState::Trusted, None).unwrap();

    assert_eq!(trust.state(&key("aaa")), TrustState::Trusted);
    assert_eq!(
        trust
            .history(&RegistrySource::personal(), &cid("hook/gate/secrets"))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn reviewing_a_new_revision_supersedes_the_one_it_replaces() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    trust.record(&key("aaa"), TrustState::Trusted, None).unwrap();
    let superseded = trust.record(&key("bbb"), TrustState::Reviewed, None).unwrap();

    assert_eq!(trust.state(&key("bbb")), TrustState::Reviewed);
    assert_eq!(
        trust.state(&key("aaa")),
        TrustState::Superseded,
        "the old revision is retained for audit, not left claiming to be trusted"
    );
    assert_eq!(superseded, vec![key("aaa")]);
}

#[test]
fn superseding_does_not_reach_across_capsules_or_across_registries() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    let other_capsule = TrustKey::new(
        RegistrySource::personal(),
        cid("hook/gate/other"),
        Revision::from_raw("aaa"),
    );
    let other_source = TrustKey::new(
        RegistrySource::project_local(),
        cid("hook/gate/secrets"),
        Revision::from_raw("aaa"),
    );
    trust.record(&other_capsule, TrustState::Trusted, None).unwrap();
    trust.record(&other_source, TrustState::Trusted, None).unwrap();
    trust.record(&key("aaa"), TrustState::Trusted, None).unwrap();

    trust.record(&key("bbb"), TrustState::Trusted, None).unwrap();

    assert_eq!(trust.state(&other_capsule), TrustState::Trusted);
    assert_eq!(
        trust.state(&other_source),
        TrustState::Trusted,
        "a project-local review and a personal review are separate decisions"
    );
}

#[test]
fn a_block_is_not_swept_away_by_a_later_review_of_another_revision() {
    // A refusal is a standing decision, keyed on identity: it is not stored per
    // revision, so a later review of any revision cannot sweep it away. It is
    // read through the effective `state_for`, not the raw per-revision `state`.
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    trust.record(&key("aaa"), TrustState::Blocked, Some("exfiltrates")).unwrap();
    trust.record(&key("ccc"), TrustState::Quarantined, None).unwrap();
    trust.record(&key("bbb"), TrustState::Trusted, None).unwrap();

    let effective = |rev: &str| {
        trust.state_for(
            Some(&RegistrySource::personal()),
            &cid("hook/gate/secrets"),
            Some(&Revision::from_raw(rev)),
        )
    };
    // The block applies to every revision, including the one that was later
    // reviewed — the whole point of keying a refusal on identity.
    assert_eq!(effective("aaa"), TrustState::Blocked);
    assert_eq!(effective("bbb"), TrustState::Blocked);
    // Quarantine is per-revision (a specific captured revision is held), so it
    // is still visible at its own revision through the raw oracle.
    assert_eq!(trust.state(&key("ccc")), TrustState::Quarantined);
}

#[test]
fn blocking_a_revision_does_not_supersede_the_trusted_one() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    trust.record(&key("aaa"), TrustState::Trusted, None).unwrap();
    let superseded = trust.record(&key("bbb"), TrustState::Blocked, None).unwrap();

    assert!(superseded.is_empty());
    assert_eq!(trust.state(&key("aaa")), TrustState::Trusted);
}

#[test]
fn an_edited_capsule_loses_its_review_because_the_revision_moved() {
    // The end-to-end version of the property: review the capsule, edit a payload
    // byte, reload, and the reviewed state does not follow the id across.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.hook("hook/gate/secrets");
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    let before = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    let capsule = before.catalog.get(&cid("hook/gate/secrets")).cloned().unwrap();
    trust
        .record(
            &TrustKey::new(
                capsule.source.clone().unwrap(),
                capsule.id.clone(),
                capsule.revision.clone().unwrap(),
            ),
            TrustState::Trusted,
            None,
        )
        .unwrap();

    fixture.write_payload("hook/gate/secrets", "payload/check", "#!/bin/sh\ncurl evil\n");
    let after = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    let edited = after.catalog.get(&cid("hook/gate/secrets")).unwrap();

    assert_eq!(
        trust.state_for(edited.source.as_ref(), &edited.id, edited.revision.as_ref()),
        TrustState::Unseen
    );
}

// ---------------------------------------------------------------------------
// The snapshot the resolver uses
// ---------------------------------------------------------------------------

#[test]
fn a_snapshot_answers_the_same_way_the_live_store_does() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let store = TrustStore::new(&index);
    store.record(&key("aaa"), TrustState::Trusted, None).unwrap();
    store
        .record(
            &TrustKey::new(
                RegistrySource::personal(),
                cid("skill/rust/review"),
                Revision::from_raw("zzz"),
            ),
            TrustState::Reviewed,
            None,
        )
        .unwrap();

    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot.state(&key("aaa")), TrustState::Trusted);
    assert_eq!(snapshot.state(&key("unknown")), TrustState::Unseen);
    assert_eq!(snapshot.len(), 2);
}

#[test]
fn a_resolver_run_against_the_persistent_oracle_holds_an_unreviewed_hook_back() {
    // The integration that matters: the store's oracle plugged into the real
    // resolver refuses to activate a hook nobody has read.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.hook("hook/gate/secrets");
    let index = index(tmp.path());
    let store = TrustStore::new(&index);

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    let capsule = load.catalog.get(&cid("hook/gate/secrets")).cloned().unwrap();

    let request = aikit_core::ResolveRequest {
        context: aikit_core::ContextDescriptor::for_project(tmp.path()),
        layers: vec![aikit_core::ScopeLayer::new(
            aikit_core::ScopeKind::Project,
            aikit_core::LayerOrigin::new("test"),
            aikit_core::PoolPatch {
                enable: vec![cid("hook/gate/secrets")],
                ..Default::default()
            },
        )],
        policy: Default::default(),
    };

    let view = aikit_core::resolve(&load.catalog, &store.snapshot().unwrap(), &request).unwrap();
    assert!(!view.is_active(&cid("hook/gate/secrets")));
    assert_eq!(
        view.unavailable_reason(&cid("hook/gate/secrets")),
        Some(&aikit_core::UnavailableReason::TrustRequired)
    );

    // Review it, and the same request now activates it.
    store
        .record(
            &TrustKey::new(
                capsule.source.clone().unwrap(),
                capsule.id.clone(),
                capsule.revision.clone().unwrap(),
            ),
            TrustState::Reviewed,
            None,
        )
        .unwrap();
    let view = aikit_core::resolve(&load.catalog, &store.snapshot().unwrap(), &request).unwrap();
    assert!(view.is_active(&cid("hook/gate/secrets")));
}

// ---------------------------------------------------------------------------
// Standing refusal: identity-keyed, so an edit cannot clear it
//
// The in-memory oracle already gets this right (aikit-core::tests::trust_keying).
// These tests hold the *persistent* store to the same promise, against a real
// database, because a security property that only holds in memory is not one.
// ---------------------------------------------------------------------------

#[test]
fn a_persisted_block_survives_a_content_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    trust
        .block(&RegistrySource::personal(), &cid("hook/gate/secrets"))
        .unwrap();

    // The revision the block was recorded against is irrelevant: any revision,
    // including one that did not exist when the block was made, is still blocked.
    for revision in ["original", "edited", "a-brand-new-hash"] {
        assert_eq!(
            trust.state_for(
                Some(&RegistrySource::personal()),
                &cid("hook/gate/secrets"),
                Some(&Revision::from_raw(revision)),
            ),
            TrustState::Blocked,
            "a block that a version bump clears is not a block",
        );
    }
}

#[test]
fn a_persisted_block_survives_reopening_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let index = index(tmp.path());
        TrustStore::new(&index)
            .block(&RegistrySource::personal(), &cid("hook/gate/secrets"))
            .unwrap();
    }
    // A fresh process, a fresh handle to the same file.
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);
    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::personal()),
            &cid("hook/gate/secrets"),
            Some(&Revision::from_raw("edited")),
        ),
        TrustState::Blocked
    );
}

#[test]
fn a_persisted_block_outranks_a_later_per_revision_approval() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    trust
        .block(&RegistrySource::personal(), &cid("hook/gate/secrets"))
        .unwrap();
    // Something records an approval of a specific revision anyway.
    trust
        .record(&key("edited"), TrustState::Trusted, None)
        .unwrap();

    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::personal()),
            &cid("hook/gate/secrets"),
            Some(&Revision::from_raw("edited")),
        ),
        TrustState::Blocked,
        "a standing refusal must not be defeated by a per-revision yes",
    );
}

#[test]
fn a_standing_verdict_is_carried_by_the_resolution_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);
    trust
        .block(&RegistrySource::personal(), &cid("hook/gate/secrets"))
        .unwrap();

    // The snapshot is what resolution actually consults; the standing verdict
    // must survive the copy into it.
    let snapshot = trust.snapshot().unwrap();
    assert_eq!(
        snapshot.state_for(
            Some(&RegistrySource::personal()),
            &cid("hook/gate/secrets"),
            Some(&Revision::from_raw("anything")),
        ),
        TrustState::Blocked
    );
}

#[test]
fn unblocking_restores_per_revision_keying_without_granting_approval() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    trust
        .block(&RegistrySource::personal(), &cid("hook/gate/secrets"))
        .unwrap();
    trust
        .unblock(&RegistrySource::personal(), &cid("hook/gate/secrets"))
        .unwrap();

    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::personal()),
            &cid("hook/gate/secrets"),
            Some(&Revision::from_raw("original")),
        ),
        TrustState::Unseen,
        "lifting a block is not the same as approving the capsule",
    );
}

#[test]
fn a_dismissal_survives_an_edit_but_yields_to_a_review_of_that_revision() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let trust = TrustStore::new(&index);

    trust
        .dismiss(&RegistrySource::personal(), &cid("hook/gate/secrets"))
        .unwrap();
    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::personal()),
            &cid("hook/gate/secrets"),
            Some(&Revision::from_raw("edited")),
        ),
        TrustState::Dismissed
    );

    // Answering the question for a specific revision clears the "not now".
    trust
        .record(&key("edited"), TrustState::Reviewed, None)
        .unwrap();
    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::personal()),
            &cid("hook/gate/secrets"),
            Some(&Revision::from_raw("edited")),
        ),
        TrustState::Reviewed
    );
}
