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
use aikit_store::channel::{Evidence, InboxChannel, InboxKind, InboxState};
use aikit_store::inbox::{
    classify_with_confidence, CandidateState, Capture, CaptureScope, Confidence, Inbox,
    PromotionEdits, SimilarityBasis,
};
use aikit_store::index::Index;
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
    Capture::new(title, body)
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

    let script = inbox
        .capture(capture("ci", CLEAN_SCRIPT))
        .unwrap()
        .candidate;
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

    let candidate = inbox
        .capture(capture("ci", CLEAN_SCRIPT))
        .unwrap()
        .candidate;
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
    assert_eq!(
        second.duplicate_of.as_deref(),
        Some(first.candidate.id.as_str())
    );
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
    assert!(outcome.similar[0]
        .summary
        .to_lowercase()
        .contains("identical"));
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
        &[(
            "payload/run.sh",
            "#!/usr/bin/env python3\nprint('totally different')\n",
        )],
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

    let candidate = inbox
        .capture(capture("ci tests", CLEAN_SCRIPT))
        .unwrap()
        .candidate;

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
    assert_eq!(
        capsule.description,
        "Runs the workspace test suite the way CI does."
    );
    assert_eq!(
        capsule.tags,
        vec!["testing".to_string(), "rust".to_string()]
    );
    assert_eq!(capsule.maturity, Maturity::Candidate);
    assert_eq!(capsule.exported_commands(), vec!["nt".to_string()]);
    assert!(
        capsule.revision.is_some(),
        "it gets a content revision like any other"
    );

    // The payload is the captured body, and it can actually run.
    let body = fs::read_to_string(&promoted.payload_path).unwrap();
    assert!(body.contains("cargo nextest run"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(&promoted.payload_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
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

    let candidate = inbox
        .capture(capture("ci", CLEAN_SCRIPT))
        .unwrap()
        .candidate;
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
    let existing =
        fs::read_to_string(fixture.capsule_dir("script/test/nt").join("manifest.toml")).unwrap();

    let candidate = inbox
        .capture(capture("ci", CLEAN_SCRIPT))
        .unwrap()
        .candidate;
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

    let candidate = inbox
        .capture(capture("ci", CLEAN_SCRIPT))
        .unwrap()
        .candidate;
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
        registry
            .join(capsule.id.registry_path())
            .join(entry)
            .is_file(),
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

    let candidate = inbox
        .capture(capture("ci", CLEAN_SCRIPT))
        .unwrap()
        .candidate;
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

// ---------------------------------------------------------------------------
// Capture carries intent (saved operations, A1)
// ---------------------------------------------------------------------------

#[test]
fn the_classifier_says_how_sure_it_is_and_never_files_prose_as_a_script() {
    let cases: &[(&str, Kind, Confidence)] = &[
        ("#!/bin/sh\necho hi\n", Kind::Script, Confidence::High),
        (
            "---\nname: review\ndescription: Reviews\n---\n# Review\n",
            Kind::Skill,
            Confidence::High,
        ),
        (
            "Run this on PreToolUse to gate pushes.",
            Kind::Hook,
            Confidence::Medium,
        ),
        (
            "# Reviewing migrations\n\nRead the down first.\n",
            Kind::Guidance,
            Confidence::Medium,
        ),
        (
            "cargo nextest run --workspace",
            Kind::Script,
            Confidence::Medium,
        ),
        (
            "git log --oneline | head -5\nmake -j4 check",
            Kind::Script,
            Confidence::Medium,
        ),
        (
            "Remember to ask the owner before changing the schema.",
            Kind::Guidance,
            Confidence::Low,
        ),
        // A pre-commit hook mentioned in a sentence has a hyphen, not a flag.
        (
            "pre-commit hooks are useful; check them before pushing",
            Kind::Guidance,
            Confidence::Low,
        ),
        // A bare two-word command has no syntax to read: it asks, it does not guess.
        ("git status", Kind::Guidance, Confidence::Low),
    ];
    for (body, kind, confidence) in cases {
        let got = classify_with_confidence(body);
        assert_eq!(
            (got.kind, got.confidence),
            (*kind, *confidence),
            "{body:?} was read as {got:?}"
        );
        assert!(!got.reason.is_empty(), "every reading comes with a reason");
    }
}

#[test]
fn a_declared_intent_is_kept_on_the_candidate_and_survives_a_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let filed = inbox
        .capture(
            Capture::new("deploy", "cargo build --release && ./ship.sh")
                .of_kind(Kind::Script)
                .exporting(["deploy", "ship"])
                .in_scope(CaptureScope::Home)
                .from_source("terminal:pane-3")
                .by("claude-code:integrator"),
        )
        .unwrap()
        .candidate;
    assert_eq!(filed.kind, Kind::Script);
    assert_eq!(filed.confidence, Confidence::Declared);

    // A different process reads the same inbox: nothing lives only in memory.
    let reread = Inbox::new(&home, &Index::open(&home.database()).unwrap())
        .candidate(&filed.id)
        .unwrap()
        .unwrap();
    assert_eq!(reread.kind, Kind::Script);
    assert_eq!(reread.confidence, Confidence::Declared);
    assert_eq!(
        reread.exports,
        vec!["deploy".to_string(), "ship".to_string()]
    );
    assert_eq!(reread.scope, CaptureScope::Home);
    assert_eq!(reread.source_ref.as_deref(), Some("terminal:pane-3"));
    assert_eq!(reread.by.as_deref(), Some("claude-code:integrator"));
}

#[test]
fn a_command_captured_as_a_script_with_an_export_promotes_to_script_personal_deploy() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let registry = home.registry("personal");

    let filed = inbox
        .capture(
            Capture::new("deploy staging", "cargo build --release && ./ship.sh")
                .of_kind(Kind::Script)
                .exporting(["deploy"])
                .by("human"),
        )
        .unwrap()
        .candidate;

    let id = Inbox::proposed_id(&filed).unwrap();
    assert_eq!(id.to_string(), "script/personal/deploy");

    // The promoter names no exports: the capture's own are used.
    inbox
        .promote(
            &filed.id,
            &PromotionEdits::new(id.clone(), "Deploy to staging."),
            &registry,
        )
        .unwrap();

    let load = load_registry(&registry, RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);
    let capsule = load.catalog.get(&id).unwrap();
    assert_eq!(capsule.exported_commands(), vec!["deploy".to_string()]);
    assert_eq!(capsule.provenance.author.as_deref(), Some("human"));
    assert_eq!(
        capsule.script().unwrap().interpreter,
        vec!["sh".to_string()],
        "a captured command line has no shebang, so the manifest says how to run it"
    );
    let payload = fs::read_to_string(
        capsule
            .root
            .as_ref()
            .unwrap()
            .join(&capsule.script().unwrap().entry),
    )
    .unwrap();
    assert_eq!(
        payload, "cargo build --release && ./ship.sh",
        "the captured text is kept as it was"
    );
}

#[test]
fn a_script_that_has_its_own_shebang_is_not_given_an_interpreter() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let registry = home.registry("personal");
    let filed = inbox
        .capture(capture("ci", CLEAN_SCRIPT))
        .unwrap()
        .candidate;

    inbox
        .promote(
            &filed.id,
            &PromotionEdits::new(cid("script/personal/ci"), "CI tests."),
            &registry,
        )
        .unwrap();

    let load = load_registry(&registry, RegistrySource::personal()).unwrap();
    let capsule = load.catalog.get(&cid("script/personal/ci")).unwrap();
    assert!(capsule.script().unwrap().interpreter.is_empty());
}

#[test]
fn an_alias_keeps_the_captured_command_line_in_its_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let registry = home.registry("personal");

    let filed = inbox
        .capture(
            Capture::new("short git status", "git status -sb\n")
                .of_kind(Kind::Alias)
                .exporting(["gs"]),
        )
        .unwrap()
        .candidate;
    let id = Inbox::proposed_id(&filed).unwrap();
    assert_eq!(id.to_string(), "alias/personal/gs");

    let promoted = inbox
        .promote(
            &filed.id,
            &PromotionEdits::new(id.clone(), "git status, short."),
            &registry,
        )
        .unwrap();
    assert!(
        !promoted.root.join("payload").exists(),
        "an alias has no payload file; its command line is the manifest's body"
    );

    let load = load_registry(&registry, RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);
    let alias = load.catalog.get(&id).unwrap().alias().unwrap().clone();
    assert_eq!(alias.name, "gs");
    assert_eq!(
        alias.body, "git status -sb",
        "no empty body, no trailing newline"
    );
}

#[test]
fn exports_name_commands_so_only_a_script_or_an_alias_may_carry_them() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let prose = inbox
        .capture(
            Capture::new("note", "Ask the owner before changing the schema.").exporting(["note"]),
        )
        .unwrap_err();
    assert_eq!(prose.code(), "inbox.exports_need_a_command");

    let skill = inbox
        .capture(
            Capture::new("s", "body")
                .of_kind(Kind::Skill)
                .exporting(["s"]),
        )
        .unwrap_err();
    assert_eq!(skill.code(), "inbox.exports_need_a_command");

    for bad in ["a b", "a/b", "", "..", "-x"] {
        let error = inbox
            .capture(
                Capture::new("x", "echo hi && true")
                    .of_kind(Kind::Script)
                    .exporting([bad]),
            )
            .unwrap_err();
        assert_eq!(error.code(), "inbox.invalid_export", "{bad:?}");
    }
    assert!(
        inbox.candidates().unwrap().is_empty(),
        "a refused capture leaves nothing behind"
    );
}

#[test]
fn a_scope_the_capture_cannot_satisfy_is_refused_before_anything_is_stored() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let project = inbox
        .capture(Capture::new("p", "echo hi && true").in_scope(CaptureScope::Project))
        .unwrap_err();
    assert_eq!(project.code(), "inbox.scope_needs_project");
    let session = inbox
        .capture(Capture::new("s", "echo hi && true").in_scope(CaptureScope::Session))
        .unwrap_err();
    assert_eq!(session.code(), "inbox.scope_needs_session");
    assert!(inbox.candidates().unwrap().is_empty());
}

#[test]
fn a_project_scoped_capture_is_promoted_into_the_projects_own_registry() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let project = tmp.path().join("project");
    fs::create_dir_all(&project).unwrap();

    let mut c = Capture::new("lint", "cargo clippy --workspace -- -D warnings")
        .of_kind(Kind::Script)
        .exporting(["lint"])
        .in_scope(CaptureScope::Project);
    c.project_root = Some(project.clone());
    let filed = inbox.capture(c).unwrap().candidate;

    let id = Inbox::proposed_id(&filed).unwrap();
    assert_eq!(id.to_string(), "script/project/lint");
    let registry = inbox.registry_root(&filed).unwrap();
    assert_eq!(registry, project.join(".aikit"));

    inbox
        .promote(
            &filed.id,
            &PromotionEdits::new(id.clone(), "Lint."),
            &registry,
        )
        .unwrap();
    let load = load_registry(&registry, RegistrySource::project_local()).unwrap();
    assert!(load.catalog.get(&id).is_some());

    // And a home-scoped one still goes to the personal registry.
    let home_scoped = inbox
        .capture(Capture::new("other", "echo other && true").of_kind(Kind::Script))
        .unwrap()
        .candidate;
    assert_eq!(
        inbox.registry_root(&home_scoped).unwrap(),
        home.registry("personal")
    );
}

#[test]
fn naming_the_same_text_again_with_a_declared_kind_upgrades_a_guess() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let first = inbox
        .capture(Capture::new("status", "git status"))
        .unwrap()
        .candidate;
    assert_eq!(
        first.kind,
        Kind::Guidance,
        "a bare command is a question, not a script"
    );
    assert_eq!(first.confidence, Confidence::Low);

    let again = inbox
        .capture(
            Capture::new("status", "git status")
                .of_kind(Kind::Alias)
                .exporting(["gs"]),
        )
        .unwrap();
    assert_eq!(
        again.duplicate_of.as_deref(),
        Some(first.id.as_str()),
        "no second candidate"
    );
    assert_eq!(again.candidate.kind, Kind::Alias);
    assert_eq!(again.candidate.confidence, Confidence::Declared);
    assert_eq!(again.candidate.exports, vec!["gs".to_string()]);

    let reread = inbox.candidate(&first.id).unwrap().unwrap();
    assert_eq!(reread.kind, Kind::Alias);
    assert_eq!(inbox.candidates().unwrap().len(), 1);
}

#[test]
fn a_candidate_filed_before_intent_was_recorded_still_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let filed = inbox
        .capture(capture("ci", CLEAN_SCRIPT))
        .unwrap()
        .candidate;
    fs::remove_file(filed.path.join("meta.json")).unwrap();

    let reread = inbox.candidate(&filed.id).unwrap().unwrap();

    assert_eq!(reread.kind, Kind::Script);
    assert_eq!(reread.scope, CaptureScope::Home);
    assert!(reread.exports.is_empty());
    assert_eq!(reread.confidence, Confidence::Medium);
}

// ---------------------------------------------------------------------------
// One inbox (saved operations, A2)
// ---------------------------------------------------------------------------

fn deploy_capture() -> Capture {
    Capture::new("deploy staging", "cargo build --release && ./ship.sh")
        .of_kind(Kind::Script)
        .exporting(["deploy"])
        .by("claude-code:integrator")
}

fn pending(index: &Index) -> Vec<aikit_store::channel::InboxItem> {
    InboxChannel::new(index)
        .pending(aikit_store::Timestamp::now())
        .unwrap()
}

#[test]
fn a_capture_is_announced_on_the_channel_with_the_candidate_as_evidence_and_the_promotion_as_its_plan(
) {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let outcome = inbox.capture(deploy_capture()).unwrap();
    let announcement = outcome
        .announcement
        .clone()
        .expect("a capture is announced");

    let items = pending(&index);
    assert_eq!(items.len(), 1, "one inbox: the capture is on the channel");
    let item = &items[0];
    assert_eq!(item.id, announcement.item);
    assert_eq!(item.kind, InboxKind::CaptureCandidate);
    assert!(item.title.contains("deploy staging"), "{}", item.title);
    assert!(
        item.body.contains("script/personal/deploy"),
        "{}",
        item.body
    );
    assert!(
        item.body.contains("claude-code:integrator"),
        "{}",
        item.body
    );
    assert!(
        item.evidence.iter().any(|e| matches!(e,
            Evidence::Hash { label, value } if label == "candidate" && *value == outcome.candidate.id)),
        "the evidence is the candidate: {:?}",
        item.evidence
    );

    // The planned Procedure is the promotion: persisted, not applied.
    let procedure_id = item
        .proposal
        .clone()
        .expect("a ready capture proposes its promotion");
    let procedure = aikit_store::procedure::ProcedureRunner::new(&home)
        .load(&procedure_id)
        .unwrap();
    assert!(matches!(
        procedure.kind,
        aikit_core::procedure::ProcedureKind::Promote { ref candidate } if *candidate == outcome.candidate.id
    ));
    assert!(
        !home
            .registry("personal")
            .join("capsules/script/personal/deploy")
            .exists(),
        "planning writes nothing"
    );
}

#[test]
fn announcing_is_idempotent_so_the_same_text_never_nags() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);

    let first = inbox.capture(deploy_capture()).unwrap();
    let again = inbox.capture(deploy_capture()).unwrap();

    assert_eq!(
        again.duplicate_of.as_deref(),
        Some(first.candidate.id.as_str())
    );
    assert_eq!(pending(&index).len(), 1);
    assert_eq!(
        again.announcement.unwrap().item,
        first.announcement.unwrap().item,
        "the second capture points at the message that is already there"
    );
}

#[test]
fn accepting_runs_the_planned_procedure_and_resolves_the_example_and_its_message() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let filed = inbox.capture(deploy_capture()).unwrap();

    let accepted = inbox.accept(&filed.candidate.id).unwrap();

    assert_eq!(accepted.id.to_string(), "script/personal/deploy");
    assert!(accepted.applied > 0);
    assert_eq!(
        accepted.item,
        filed.announcement.as_ref().map(|a| a.item.clone())
    );
    assert_eq!(
        Some(&accepted.procedure),
        filed.announcement.as_ref().unwrap().proposal.as_ref(),
        "it ran the plan the message proposed, not another"
    );

    let registry = home.registry("personal");
    let load = load_registry(&registry, RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);
    let capsule = load.catalog.get(&accepted.id).unwrap();
    assert_eq!(capsule.exported_commands(), vec!["deploy".to_string()]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let entry = capsule
            .root
            .as_ref()
            .unwrap()
            .join(&capsule.script().unwrap().entry);
        assert_ne!(fs::metadata(entry).unwrap().permissions().mode() & 0o111, 0);
    }

    // Both stores agree it is decided.
    assert_eq!(
        inbox.candidate(&filed.candidate.id).unwrap().unwrap().state,
        CandidateState::Promoted
    );
    assert!(pending(&index).is_empty());
    let all = InboxChannel::new(&index).items().unwrap();
    assert!(
        matches!(&all[0].state, InboxState::Resolved { decision } if decision.contains("script/personal/deploy"))
    );

    // And it is reversible: the Procedure that wrote it undoes it.
    aikit_store::procedure::ProcedureRunner::new(&home)
        .undo(&accepted.procedure)
        .unwrap();
    assert!(!registry.join("capsules/script/personal/deploy").exists());

    let again = inbox.accept(&filed.candidate.id).unwrap_err();
    assert_eq!(again.code(), "inbox.already_promoted");
}

#[test]
fn rejecting_resolves_the_example_and_its_message_and_a_fresh_capture_is_a_fresh_candidate() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let filed = inbox.capture(deploy_capture()).unwrap();

    inbox
        .reject(&filed.candidate.id, "not worth keeping")
        .unwrap();

    assert!(pending(&index).is_empty());
    let all = InboxChannel::new(&index).items().unwrap();
    assert!(
        matches!(&all[0].state, InboxState::Resolved { decision } if decision == "rejected: not worth keeping")
    );

    let again = inbox.capture(deploy_capture()).unwrap();
    assert!(
        again.duplicate_of.is_none(),
        "a rejected capture is not a duplicate"
    );
    assert_eq!(pending(&index).len(), 1, "and it is announced afresh");
}

#[test]
fn promoting_directly_also_resolves_the_message() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let filed = inbox.capture(deploy_capture()).unwrap();

    inbox
        .promote(
            &filed.candidate.id,
            &PromotionEdits::new(cid("script/personal/ship"), "Ship it."),
            &home.registry("personal"),
        )
        .unwrap();

    assert!(
        pending(&index).is_empty(),
        "either road leaves the inbox in one state"
    );
}

#[test]
fn a_held_back_capture_is_announced_without_a_plan_and_released_into_a_proposal() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let held = inbox.capture(capture("publish", SECRET_SCRIPT)).unwrap();
    let id = held.candidate.id.clone();

    let items = pending(&index);
    assert_eq!(items.len(), 1);
    assert!(
        items[0].title.starts_with("Held back"),
        "{}",
        items[0].title
    );
    assert!(
        items[0].proposal.is_none(),
        "a held-back capture proposes nothing"
    );
    assert_eq!(inbox.accept(&id).unwrap_err().code(), "inbox.quarantined");

    // Remove the secret from the stored body, as a person editing it would.
    let body = held.candidate.path.join("body");
    fs::write(&body, "#!/bin/sh\ncargo publish\n").unwrap();
    inbox.release_from_quarantine(&id).unwrap();

    let now = pending(&index);
    assert_eq!(
        now.len(),
        1,
        "the stale message was closed, a fresh one opened"
    );
    assert!(now[0].title.starts_with("Save"), "{}", now[0].title);
    assert!(now[0].proposal.is_some());
}

#[test]
fn a_capture_that_cannot_be_saved_as_it_stands_says_why_and_stays_open() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    // Something already lives at the id this capture would take.
    let first = inbox.capture(deploy_capture()).unwrap();
    inbox.accept(&first.candidate.id).unwrap();

    let second = inbox
        .capture(
            Capture::new("deploy production", "./ship.sh --prod && true")
                .of_kind(Kind::Script)
                .exporting(["deploy"]),
        )
        .unwrap();

    let items = pending(&index);
    assert_eq!(items.len(), 1);
    assert!(items[0].proposal.is_none());
    assert!(
        items[0].body.contains("Not ready to save as it stands"),
        "{}",
        items[0].body
    );
    assert!(
        items[0].body.contains("already exists"),
        "{}",
        items[0].body
    );
    assert_eq!(
        inbox.accept(&second.candidate.id).unwrap_err().code(),
        "inbox.id_taken"
    );
    assert_eq!(
        pending(&index).len(),
        1,
        "a refused accept leaves the message open"
    );
}

#[test]
fn a_plan_made_before_the_capture_changed_is_replaced_not_run() {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let first = inbox
        .capture(Capture::new("status", "git status -sb && true").of_kind(Kind::Script))
        .unwrap();
    let stale = first.announcement.unwrap().proposal.unwrap();

    // The capturer comes back and says what it is: an alias called gs.
    inbox
        .capture(
            Capture::new("status", "git status -sb && true")
                .of_kind(Kind::Alias)
                .exporting(["gs"]),
        )
        .unwrap();
    let accepted = inbox.accept(&first.candidate.id).unwrap();

    assert_eq!(
        accepted.id.to_string(),
        "alias/personal/gs",
        "the declared kind won"
    );
    assert_ne!(
        accepted.procedure, stale,
        "the stale plan was not the one that ran"
    );
    let items = InboxChannel::new(&index).items().unwrap();
    assert_eq!(
        items.len(),
        1,
        "still one message, now pointing at the plan that ran"
    );
    assert_eq!(items[0].proposal.as_ref(), Some(&accepted.procedure));
}

#[test]
fn accepting_a_project_scoped_capture_stages_the_write_instead_of_branching_the_projects_repository(
) {
    let tmp = tempfile::tempdir().unwrap();
    let (home, index) = setup(tmp.path());
    let inbox = Inbox::new(&home, &index);
    let project = tmp.path().join("project");
    fs::create_dir_all(project.join(".git")).unwrap();

    let mut c = Capture::new("lint", "cargo clippy --workspace -- -D warnings")
        .of_kind(Kind::Script)
        .exporting(["lint"])
        .in_scope(CaptureScope::Project);
    c.project_root = Some(project.clone());
    let filed = inbox.capture(c).unwrap();

    let accepted = inbox.accept(&filed.candidate.id).unwrap();

    assert!(project
        .join(".aikit/capsules/script/project/lint/manifest.toml")
        .is_file());
    let procedure = aikit_store::procedure::ProcedureRunner::new(&home)
        .load(&accepted.procedure)
        .unwrap();
    assert!(
        matches!(
            procedure.isolation,
            aikit_core::procedure::MutationIsolation::Staged { .. }
        ),
        "{:?}",
        procedure.isolation
    );
}
