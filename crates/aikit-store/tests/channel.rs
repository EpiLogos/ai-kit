//! The Inbox as the system's channel (Spec II §2): unconditional redaction,
//! idempotent dedup, and the open → resolved lifecycle. Every assertion runs
//! against a real SQLite file.

use std::path::Path;

use aikit_store::channel::{InboxChannel, InboxKind, InboxState, NewItem};
use aikit_store::events::Timestamp;
use aikit_store::index::Index;

fn index(dir: &Path) -> Index {
    Index::open(&dir.join("state/aikit.sqlite3")).unwrap()
}

#[test]
fn a_published_item_is_redacted_before_storage_and_read_back_redacted() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());

    let secret = "ghp_0123456789012345678901234567890123";
    let item = InboxChannel::new(&index)
        .publish(NewItem::new(
            InboxKind::AgentProposal,
            "found a token",
            format!("the leaked value is {secret} in the log"),
        ))
        .unwrap();

    assert!(
        !item.body.contains(secret),
        "the secret must be redacted before storage: {}",
        item.body
    );
    assert!(item.body.contains("[redacted"), "a redaction marker is present");

    // Read back through a fresh channel: the stored bytes are the redacted ones,
    // so the secret never reached the database.
    let reread = InboxChannel::new(&index).get(&item.id).unwrap().unwrap();
    assert_eq!(reread.body, item.body);
    assert!(!reread.body.contains("ghp_"));
}

#[test]
fn publishing_with_a_dedup_key_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let channel = InboxChannel::new(&index);

    let first = channel
        .publish(NewItem::new(InboxKind::DriftNotice, "first title", "first body").deduped_by("k1"))
        .unwrap();
    let again = channel
        .publish(
            NewItem::new(InboxKind::DriftNotice, "second title", "second body").deduped_by("k1"),
        )
        .unwrap();

    assert_eq!(first.id, again.id, "the same dedup key returns the existing item");
    assert_eq!(again.title, first.title, "the existing item is unchanged");
    assert_eq!(channel.items().unwrap().len(), 1, "no near-duplicate was filed");
}

#[test]
fn a_resolved_item_drops_out_of_pending_but_stays_for_audit() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let channel = InboxChannel::new(&index);

    let item = channel
        .publish(NewItem::new(InboxKind::Breakage, "dead symlink", "…"))
        .unwrap();
    let now = Timestamp::now();
    assert_eq!(channel.pending(now).unwrap().len(), 1);

    channel.resolve(&item.id, "fixed by re-linking").unwrap();

    assert_eq!(
        channel.pending(now).unwrap().len(),
        0,
        "a resolved item no longer wants attention"
    );
    let reread = channel.get(&item.id).unwrap().unwrap();
    assert_eq!(
        reread.state,
        InboxState::Resolved {
            decision: "fixed by re-linking".to_string()
        },
        "but the decision is kept for audit"
    );
    assert_eq!(channel.items().unwrap().len(), 1);
}

#[test]
fn a_deferred_item_returns_to_pending_once_its_time_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());
    let channel = InboxChannel::new(&index);

    let item = channel
        .publish(NewItem::new(InboxKind::TrustReview, "review me", "…"))
        .unwrap();

    let now = Timestamp::now();
    let soon = Timestamp::from_nanos(now.as_nanos() + 1_000_000_000);
    channel.defer(&item.id, soon).unwrap();

    assert_eq!(channel.pending(now).unwrap().len(), 0, "still snoozed");
    let later = Timestamp::from_nanos(soon.as_nanos() + 1);
    assert_eq!(
        channel.pending(later).unwrap().len(),
        1,
        "a deferral that has elapsed is pending again"
    );
}

#[test]
fn every_evidence_variant_is_redacted_not_just_the_summary() {
    // Redaction is UNCONDITIONAL (Spec II §2, STANDARDS §6). The channel is
    // agent-writable and brokered out through `aikit inbox list --json`, so a
    // secret reaching ANY stored field would reach a preview. Summary was
    // redacted; File{path} and Hash{value} were not — this pins all three.
    use aikit_store::channel::Evidence;

    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());

    let token = "ghp_0123456789012345678901234567890123";
    let item = InboxChannel::new(&index)
        .publish(
            NewItem::new(InboxKind::AgentProposal, "proposal", "body").with_evidence(vec![
                Evidence::Summary {
                    text: format!("summary holding {token}"),
                },
                Evidence::File {
                    path: format!("/tmp/{token}/notes.md"),
                },
                Evidence::Hash {
                    label: format!("label {token}"),
                    value: token.to_string(),
                },
            ]),
        )
        .unwrap();

    let rendered = serde_json::to_string(&item.evidence).unwrap();
    assert!(
        !rendered.contains(token),
        "no evidence variant may carry a secret into storage: {rendered}"
    );

    // And it is redacted at rest, not merely on the way out.
    let reread = InboxChannel::new(&index).get(&item.id).unwrap().unwrap();
    let stored = serde_json::to_string(&reread.evidence).unwrap();
    assert!(!stored.contains(token), "the stored bytes still hold a secret: {stored}");
}
