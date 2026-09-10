//! Trust keying is asymmetric, and the asymmetry is the security property.
//!
//! Approval is keyed on **content**: editing a capsule must drop it back to
//! review, because the thing you approved is not the thing that would now run.
//!
//! Refusal is keyed on **identity**: editing a capsule must *not* clear a block,
//! because otherwise "no" lasts exactly until the next commit. direnv gets this
//! right by hashing path+contents for its allow list while keying its deny list
//! on the path alone; AIKit needs the same shape.

use aikit_core::id::{CapsuleId, RegistrySource, Revision};
use aikit_core::trust::{MemoryTrust, TrustKey, TrustOracle, TrustState};

fn capsule() -> CapsuleId {
    CapsuleId::parse("hook/gate/suspicious").unwrap()
}

fn key(rev: &str) -> TrustKey {
    TrustKey::new(RegistrySource::personal(), capsule(), Revision::from_raw(rev))
}

#[test]
fn approval_does_not_survive_an_edit() {
    let mut trust = MemoryTrust::default();
    trust.set(
        RegistrySource::personal(),
        capsule(),
        Revision::from_raw("original"),
        TrustState::Trusted,
    );

    assert_eq!(trust.state(&key("original")), TrustState::Trusted);
    assert_eq!(
        trust.state(&key("edited")),
        TrustState::Unseen,
        "an edited capsule is a different capsule and must be reviewed again"
    );
}

#[test]
fn refusal_survives_an_edit() {
    let mut trust = MemoryTrust::default();
    trust.block(RegistrySource::personal(), capsule());

    let blocked_at_original = trust.state_for(
        Some(&RegistrySource::personal()),
        &capsule(),
        Some(&Revision::from_raw("original")),
    );
    let blocked_after_edit = trust.state_for(
        Some(&RegistrySource::personal()),
        &capsule(),
        Some(&Revision::from_raw("edited")),
    );

    assert_eq!(blocked_at_original, TrustState::Blocked);
    assert_eq!(
        blocked_after_edit,
        TrustState::Blocked,
        "a block that a version bump clears is not a block"
    );
}

#[test]
fn a_standing_refusal_outranks_a_later_approval_of_the_same_capsule() {
    let mut trust = MemoryTrust::default();
    trust.block(RegistrySource::personal(), capsule());
    // Something tries to approve a specific revision anyway.
    trust.set(
        RegistrySource::personal(),
        capsule(),
        Revision::from_raw("edited"),
        TrustState::Trusted,
    );

    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::personal()),
            &capsule(),
            Some(&Revision::from_raw("edited"))
        ),
        TrustState::Blocked,
        "a refusal is a standing decision; a per-revision approval must not defeat it"
    );
}

#[test]
fn a_refusal_is_scoped_to_the_registry_that_earned_it() {
    let mut trust = MemoryTrust::default();
    trust.block(RegistrySource::personal(), capsule());

    // A project-local capsule that merely shares an id is a different artefact
    // and must not inherit the personal registry's verdict in either direction.
    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::project_local()),
            &capsule(),
            Some(&Revision::from_raw("original"))
        ),
        TrustState::Unseen
    );
}

#[test]
fn unblocking_is_explicit_and_restores_ordinary_per_revision_keying() {
    let mut trust = MemoryTrust::default();
    trust.block(RegistrySource::personal(), capsule());
    trust.unblock(&RegistrySource::personal(), &capsule());

    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::personal()),
            &capsule(),
            Some(&Revision::from_raw("original"))
        ),
        TrustState::Unseen,
        "unblocking must not silently grant approval"
    );
}

#[test]
fn dismissal_is_distinct_from_refusal_and_from_never_having_been_seen() {
    // mise keeps `ignored` separate from `trusted` so that declining a prompt
    // stops the nagging without becoming a permanent refusal. AIKit needs the
    // same third state, or every prompt is a choice between "yes" and "forever".
    assert_ne!(TrustState::Dismissed, TrustState::Blocked);
    assert_ne!(TrustState::Dismissed, TrustState::Unseen);

    assert!(!TrustState::Dismissed.may_project());
    assert!(
        !TrustState::Dismissed.is_withheld(),
        "dismissal is not a hard stop; the user may still change their mind"
    );
    assert!(
        TrustState::Dismissed.suppresses_prompting(),
        "the point of dismissal is that AIKit stops asking"
    );
    assert!(!TrustState::Unseen.suppresses_prompting());
    assert!(TrustState::Blocked.is_withheld());
}

#[test]
fn a_standing_dismissal_also_survives_an_edit_but_yields_to_an_approval() {
    let mut trust = MemoryTrust::default();
    trust.dismiss(RegistrySource::personal(), capsule());

    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::personal()),
            &capsule(),
            Some(&Revision::from_raw("edited"))
        ),
        TrustState::Dismissed
    );

    // Unlike a block, dismissal is a "not now" — reviewing a revision clears it
    // for that revision, because the user has just answered the question.
    trust.set(
        RegistrySource::personal(),
        capsule(),
        Revision::from_raw("edited"),
        TrustState::Reviewed,
    );
    assert_eq!(
        trust.state_for(
            Some(&RegistrySource::personal()),
            &capsule(),
            Some(&Revision::from_raw("edited"))
        ),
        TrustState::Reviewed
    );
}

#[test]
fn the_ledger_records_human_readable_identity_alongside_the_key() {
    // direnv names its grant files by hash but stores the path inside, which is
    // the only reason `direnv status` and `direnv prune` can exist. A trust
    // ledger you cannot enumerate in human terms cannot be audited or pruned.
    let mut trust = MemoryTrust::default();
    trust.set(
        RegistrySource::personal(),
        capsule(),
        Revision::from_raw("original"),
        TrustState::Trusted,
    );
    trust.block(
        RegistrySource::personal(),
        CapsuleId::parse("script/danger/rm-rf").unwrap(),
    );

    let ledger = trust.ledger();
    assert_eq!(ledger.len(), 2);
    assert!(ledger
        .iter()
        .any(|e| e.capsule.to_string() == "hook/gate/suspicious"
            && e.state == TrustState::Trusted
            && e.revision.is_some()));
    assert!(ledger
        .iter()
        .any(|e| e.capsule.to_string() == "script/danger/rm-rf"
            && e.state == TrustState::Blocked
            && e.revision.is_none()));
}
