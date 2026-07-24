//! The capture inbox.
//!
//! Release-blocking case 10 is "a captured secret never enters the ordinary
//! registry", and case 11 is "promotion can be completed without hand-writing a
//! manifest". Both are end-to-end properties of this module, so both are tested
//! end to end here: real files in a real inbox, a real registry directory
//! afterwards, and the registry loader asked whether it can read what promotion
//! wrote.

mod common;

use common::*;

use std::fs;

use aikit_core::catalog::Catalog;
use aikit_core::{Kind, Maturity, RegistrySource};
use aikit_store::index::Index;
use aikit_store::inbox::{Capture, CandidateState, Inbox, PromotionEdits, SimilarityBasis};
use aikit_store::registry::load_registry;
use aikit_store::AikitHome;

const SECRET_SCRIPT: &str = "#!/bin/sh\n\
    # publish the release\n\
    export GITHUB_TOKEN=ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8\n\
    cargo publish\n";

const CLEAN_SCRIPT: &str = "#!/bin/sh\n\
    # run the tests the way CI does\n\
    cargo nextest run --workspace --no-fail-fast\n";

fn setup(dir: &std::path::Path) -> (AikitHome, Index) {
    let home = AikitHome::at(dir.join("home"));
    home.ensure_layout().unwrap();
    let index = Index::open(&home.database()).unwrap();
    (home, index)
}

fn capture(title: &str, body: &str) -> Capture {
    Capture {
        title: title.to_string(),
        body: body.to_string(),
        suggested_kind: None,
        exports: vec![],
        project_root: None,
        session: None,
    }
}

// ---------------------------------------------------------------------------
// The clean path
// ---------------------------------------------------------------------------

#[test]
fn an_ordinary_capture_lands_in_the_ready_inbox_and_is_indexed() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let outcome = inbox.capture(capture("ci tests", CLEAN_SCRIPT)).unwrap();
    let candidate = outcome.candidate;

    assert_eq!(candidate.state, CandidateState::Ready);
    assert!(candidate.findings.is_empty());
    assert!(
        candidate.path.starts_with(home.inbox_ready()),
        "{} should be under inbox/ready",
        candidate.path.display()
    );
    assert!(candidate.path.join("body").is_file());
    assert_eq!(inbox.candidates().unwrap().len(), 1);
}

#[test]
fn a_shebang_is_classified_as_a_script_and_prose_as_guidance() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let script = inbox.capture(capture("ci", CLEAN_SCRIPT)).unwrap().candidate;
    assert_eq!(script.kind, Kind::Script);

    let prose = inbox
        .capture(capture(
            "review notes",
            "# Reviewing migrations\n\nAlways read the down migration first.\n",
        ))
        .unwrap()
        .candidate;
    assert_eq!(prose.kind, Kind::Guidance);

    let skill = inbox
        .capture(capture(
            "rust review",
            "---\nname: rust-review\ndescription: Reviews Rust\n---\n\n# Rust review\n",
        ))
        .unwrap()
        .candidate;
    assert_eq!(skill.kind, Kind::Skill);
}

#[test]
fn an_explicit_kind_hint_beats_the_classifier() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let mut c = capture("gate", "#!/bin/sh\nexit 0\n");
    c.suggested_kind = Some(Kind::Hook);
    assert_eq!(inbox.capture(c).unwrap().candidate.kind, Kind::Hook);
}

// ---------------------------------------------------------------------------
// A captured secret never reaches a registry
// ---------------------------------------------------------------------------

#[test]
fn a_capture_containing_a_secret_is_quarantined_redacted_and_unpromotable() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let registry = tmp.path().join("registry");
    let inbox = Inbox::new(&home, &index);

    // 1. It is quarantined, not merely flagged.
    let candidate = inbox
        .capture(capture("release", SECRET_SCRIPT))
        .unwrap()
        .candidate;
    assert_eq!(candidate.state, CandidateState::Quarantined);
    assert!(!candidate.findings.is_empty());
    assert!(candidate.path.starts_with(home.inbox_quarantine()));
    assert!(
        !candidate.path.starts_with(home.inbox_ready()),
        "a quarantined candidate must not sit in the ready queue"
    );

    // 2. What was written to disk does not contain the secret at all.
    let stored = fs::read_to_string(candidate.path.join("body")).unwrap();
    assert!(!stored.contains("ghp_A1b2C3d4E5f6"));
    assert!(
        stored.contains("cargo publish"),
        "redaction removes the secret, not the surrounding work"
    );

    // 3. Neither does the preview.
    let preview = inbox.preview(&candidate.id).unwrap();
    assert!(!preview.contains("ghp_A1b2C3d4E5f6"));
    assert!(preview.contains("redacted"));

    // 4. Promotion is refused.
    let error = inbox
        .promote(
            &candidate.id,
            &PromotionEdits::new(cid("script/release/publish"), "Publishes the release."),
            &registry,
        )
        .unwrap_err();
    assert_eq!(error.code(), "inbox.quarantined");

    // 5. And nothing was written into the registry.
    assert!(
        !registry.exists(),
        "the registry directory must not even have been created"
    );
    let load = load_registry(&registry, RegistrySource::personal()).unwrap();
    assert!(load.catalog.is_empty());
}

#[test]
fn releasing_a_quarantined_candidate_requires_the_secret_to_be_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let candidate = inbox
        .capture(capture("release", SECRET_SCRIPT))
        .unwrap()
        .candidate;

    // Re-scanning the stored (already redacted) body finds nothing, so the
    // candidate can be released for review by a human who has read it.
    let released = inbox.release_from_quarantine(&candidate.id).unwrap();
    assert_eq!(released.state, CandidateState::Ready);
    assert!(released.path.starts_with(home.inbox_ready()));
    assert!(!fs::read_to_string(released.path.join("body"))
        .unwrap()
        .contains("ghp_"));
}

#[test]
fn rejecting_a_candidate_moves_it_out_of_the_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let candidate = inbox.capture(capture("ci", CLEAN_SCRIPT)).unwrap().candidate;
    let rejected = inbox.reject(&candidate.id, "we already have one").unwrap();

    assert_eq!(rejected.state, CandidateState::Rejected);
    assert!(rejected.path.starts_with(home.inbox_rejected()));
    assert!(inbox
        .candidates()
        .unwrap()
        .iter()
        .all(|c| c.state != CandidateState::Ready));
}

// ---------------------------------------------------------------------------
// Deduplication and similarity
// ---------------------------------------------------------------------------

#[test]
fn capturing_the_identical_body_twice_reports_a_duplicate_rather_than_filing_it_again() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let first = inbox.capture(capture("ci", CLEAN_SCRIPT)).unwrap();
    let second = inbox.capture(capture("ci again", CLEAN_SCRIPT)).unwrap();

    assert!(first.duplicate_of.is_none());
    assert_eq!(second.duplicate_of.as_deref(), Some(first.candidate.id.as_str()));
    assert_eq!(inbox.candidates().unwrap().len(), 1);
}

#[test]
fn a_body_that_differs_only_in_whitespace_is_recognized_as_the_same_thing() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let first = inbox.capture(capture("ci", CLEAN_SCRIPT)).unwrap();
    let reformatted = CLEAN_SCRIPT.replace('\n', "  \n") + "\n\n\n";
    let second = inbox.capture(capture("ci", &reformatted)).unwrap();

    assert_eq!(
        second.duplicate_of.as_deref(),
        Some(first.candidate.id.as_str()),
        "trailing whitespace is not a new capability"
    );
}

#[test]
fn a_near_copy_of_a_catalogued_capsule_is_ranked_with_a_readable_explanation() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.capsule(
        "script/test/nt",
        "script",
        "entry = \"payload/run.sh\"",
        "",
        &[("payload/run.sh", CLEAN_SCRIPT)],
    );
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();

    let near = format!("{CLEAN_SCRIPT}echo done\n");
    let outcome = inbox
        .capture_against(capture("ci tests", &near), &load.catalog)
        .unwrap();

    let similar = &outcome.similar;
    assert!(!similar.is_empty(), "a near copy must be reported");
    assert_eq!(similar[0].other, "script/test/nt");
    assert!(
        similar[0].percentage >= 60,
        "expected a high score, got {}",
        similar[0].percentage
    );
    assert!(
        similar[0].summary.contains("line"),
        "the summary has to say what differs in words: {}",
        similar[0].summary
    );
}

#[test]
fn an_identical_body_already_in_the_catalog_is_reported_as_exactly_that() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.capsule(
        "script/test/nt",
        "script",
        "entry = \"payload/run.sh\"",
        "",
        &[("payload/run.sh", CLEAN_SCRIPT)],
    );
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();

    let outcome = inbox
        .capture_against(capture("ci tests", CLEAN_SCRIPT), &load.catalog)
        .unwrap();

    assert_eq!(outcome.similar[0].basis, SimilarityBasis::ExactContent);
    assert_eq!(outcome.similar[0].percentage, 100);
    assert!(outcome.similar[0].summary.to_lowercase().contains("identical"));
}

#[test]
fn a_capsule_that_shares_only_an_export_name_is_still_worth_mentioning() {
    // Different text, same command name: exactly the collision that would fail
    // resolution later with `resolution.export_collision`.
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.capsule(
        "script/test/nt",
        "script",
        "entry = \"payload/run.sh\"\nexports = [\"nt\"]",
        "",
        &[("payload/run.sh", "#!/usr/bin/env python3\nprint('totally different')\n")],
    );
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();

    let mut c = capture("nt", CLEAN_SCRIPT);
    c.exports = vec!["nt".to_string()];
    let outcome = inbox.capture_against(c, &load.catalog).unwrap();

    let by_name = outcome
        .similar
        .iter()
        .find(|s| s.basis == SimilarityBasis::ExportNames)
        .expect("a shared export name must be reported");
    assert!(by_name.summary.contains("nt"));
}

#[test]
fn an_unrelated_capsule_is_not_reported_as_similar() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.capsule(
        "guidance/mode/research",
        "guidance",
        "entry = \"payload/guidance.md\"",
        "",
        &[(
            "payload/guidance.md",
            "Prefer reading the tests before the implementation. Ask before refactoring.\n",
        )],
    );
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();

    let outcome = inbox
        .capture_against(capture("ci", CLEAN_SCRIPT), &load.catalog)
        .unwrap();
    assert!(
        outcome.similar.is_empty(),
        "false neighbours are as annoying as false positives: {:#?}",
        outcome.similar
    );
}

// ---------------------------------------------------------------------------
// Promotion writes a real capsule
// ---------------------------------------------------------------------------

#[test]
fn promotion_writes_a_capsule_the_registry_loader_can_read_without_anyone_writing_a_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let registry = tmp.path().join("registry");

    let candidate = inbox.capture(capture("ci tests", CLEAN_SCRIPT)).unwrap().candidate;

    let edits = PromotionEdits::new(
        cid("script/test/cargo-nextest"),
        "Runs the workspace test suite the way CI does.",
    )
    .with_name("nextest")
    .with_tags(["testing", "rust"])
    .with_exports(["nt"])
    .with_maturity(Maturity::Candidate);

    let promoted = inbox.promote(&candidate.id, &edits, &registry).unwrap();

    // The manifest exists and nobody typed it.
    assert!(promoted.manifest_path.is_file());
    assert!(promoted.payload_path.is_file());

    // The registry loader — the same code path a real sync uses — reads it back.
    let load = load_registry(&registry, RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);

    let capsule = load.catalog.get(&cid("script/test/cargo-nextest")).unwrap();
    assert_eq!(capsule.kind, Kind::Script);
    assert_eq!(capsule.name, "nextest");
    assert_eq!(capsule.description, "Runs the workspace test suite the way CI does.");
    assert_eq!(capsule.tags, vec!["testing".to_string(), "rust".to_string()]);
    assert_eq!(capsule.maturity, Maturity::Candidate);
    assert_eq!(capsule.exported_commands(), vec!["nt".to_string()]);
    assert!(capsule.revision.is_some(), "it gets a content revision like any other");

    // The payload is the captured body, and it can actually run.
    let body = fs::read_to_string(&promoted.payload_path).unwrap();
    assert!(body.contains("cargo nextest run"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(&promoted.payload_path).unwrap().permissions().mode() & 0o111,
            0
        );
    }
}

#[test]
fn a_promoted_candidate_is_marked_promoted_and_leaves_the_queue() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let registry = tmp.path().join("registry");

    let candidate = inbox.capture(capture("ci", CLEAN_SCRIPT)).unwrap().candidate;
    inbox
        .promote(
            &candidate.id,
            &PromotionEdits::new(cid("script/test/nt"), "Runs the tests."),
            &registry,
        )
        .unwrap();

    let after = inbox.candidate(&candidate.id).unwrap().unwrap();
    assert_eq!(after.state, CandidateState::Promoted);
    assert!(inbox
        .candidates()
        .unwrap()
        .iter()
        .all(|c| c.state != CandidateState::Ready));
}

#[test]
fn promotion_refuses_to_overwrite_an_existing_capsule() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let fixture = RegistryFixture::at(tmp.path().join("registry"));
    fixture.script("script/test/nt");
    let existing = fs::read_to_string(fixture.capsule_dir("script/test/nt").join("manifest.toml"))
        .unwrap();

    let candidate = inbox.capture(capture("ci", CLEAN_SCRIPT)).unwrap().candidate;
    let error = inbox
        .promote(
            &candidate.id,
            &PromotionEdits::new(cid("script/test/nt"), "Runs the tests."),
            fixture.root(),
        )
        .unwrap_err();

    assert_eq!(error.code(), "inbox.id_taken");
    assert_eq!(
        fs::read_to_string(fixture.capsule_dir("script/test/nt").join("manifest.toml")).unwrap(),
        existing,
        "the capsule that was already there must be untouched"
    );
}

#[test]
fn promotion_refuses_edits_that_would_produce_an_invalid_manifest() {
    // The generated manifest goes through the same validation a hand-written one
    // would, before anything is written — so a bad promotion cannot leave a
    // half-made capsule in the registry.
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let registry = tmp.path().join("registry");

    let candidate = inbox.capture(capture("ci", CLEAN_SCRIPT)).unwrap().candidate;
    let error = inbox
        .promote(
            &candidate.id,
            &PromotionEdits::new(cid("script/test/nt"), "   "),
            &registry,
        )
        .unwrap_err();

    assert_eq!(error.code(), "manifest.invalid");
    assert!(!registry.exists());
}

#[test]
fn promoting_a_guidance_capture_produces_a_guidance_capsule_with_the_right_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let registry = tmp.path().join("registry");

    let candidate = inbox
        .capture(capture(
            "migration notes",
            "# Migrations\n\nAlways read the down migration first.\n",
        ))
        .unwrap()
        .candidate;
    assert_eq!(candidate.kind, Kind::Guidance);

    inbox
        .promote(
            &candidate.id,
            &PromotionEdits::new(
                cid("guidance/db/migrations"),
                "How to read a migration before approving it.",
            ),
            &registry,
        )
        .unwrap();

    let load = load_registry(&registry, RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);
    let capsule = load.catalog.get(&cid("guidance/db/migrations")).unwrap();
    assert_eq!(capsule.kind, Kind::Guidance);
    let entry = &capsule.guidance().unwrap().entry;
    assert!(
        registry.join(capsule.id.registry_path()).join(entry).is_file(),
        "the manifest's entry has to name a file that exists"
    );
}

#[test]
fn a_promoted_capsule_starts_unreviewed_however_it_was_captured() {
    // Capture is not review. A capsule that arrived by being observed is exactly
    // as untrusted as one that arrived by being downloaded.
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let registry = tmp.path().join("registry");

    let candidate = inbox.capture(capture("ci", CLEAN_SCRIPT)).unwrap().candidate;
    inbox
        .promote(
            &candidate.id,
            &PromotionEdits::new(cid("script/test/nt"), "Runs the tests."),
            &registry,
        )
        .unwrap();

    let load = load_registry(&registry, RegistrySource::personal()).unwrap();
    let capsule = load.catalog.get(&cid("script/test/nt")).unwrap();
    let trust = aikit_store::trust::TrustStore::new(&index);
    assert_eq!(
        aikit_core::trust::TrustOracle::state_for(
            &trust,
            capsule.source.as_ref(),
            &capsule.id,
            capsule.revision.as_ref()
        ),
        aikit_core::TrustState::Unseen
    );
}
