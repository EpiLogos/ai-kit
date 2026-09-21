//! Queued delivery transport state: the durable wait for a recipient whose
//! resident is not currently live. Provider events below are explicitly
//! fixtures; nothing here is a model response.
use aikit_core::ResourceRef;
use aikit_store::{encounter::EncounterStore, AikitHome};
use serde_json::json;
fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}
fn store(home: &AikitHome) -> EncounterStore {
    EncounterStore::open(home).unwrap()
}
#[test]
fn queued_survives_restart_holds_the_slot_and_dispatches_through_the_lifecycle() {
    let t = tempfile::tempdir().unwrap();
    let home = AikitHome::at(t.path().join("home"));
    let s = r("agent-session/a");
    let sender = r("agent:sender");
    // The queued request is exactly the admitted addressed turn plus the
    // queued marker; deterministic, so a sender replay recognises it.
    let request = json!({"submission":{"turn":{"text":"waiting"}},"queued":true});
    assert!(
        store(&home)
            .queue_delivery(&s, &r("delivery/a"), &sender, &request)
            .unwrap()
            .fresh
    );
    // A replay of the same identity and request is idempotent, never a second wait.
    assert!(
        !store(&home)
            .queue_delivery(&s, &r("delivery/a"), &sender, &request)
            .unwrap()
            .fresh
    );
    // A different request under the same delivery identity conflicts.
    assert_eq!(
        store(&home)
            .queue_delivery(
                &s,
                &r("delivery/a"),
                &sender,
                &json!({"submission":{"turn":{"text":"other"}},"queued":true})
            )
            .unwrap_err()
            .code(),
        "encounter.delivery_conflict"
    );
    // The queued row holds the session's single active delivery slot.
    assert_eq!(
        store(&home)
            .queue_delivery(&s, &r("delivery/b"), &sender, &json!({"queued":true}))
            .unwrap_err()
            .code(),
        "encounter.delivery_pending"
    );
    assert_eq!(
        store(&home)
            .reserve_delivery(&s, &r("delivery/b"), &sender, &json!({"fixture":true}))
            .unwrap_err()
            .code(),
        "encounter.delivery_pending"
    );
    // Restart survival: the wait is durable in SQLite, not process state.
    drop(store(&home));
    let reopened = store(&home);
    let queued = reopened.queued_deliveries(&s).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].delivery_ref, r("delivery/a"));
    assert_eq!(queued[0].phase, "queued");
    assert_eq!(queued[0].sender, sender);
    // Oldest-first read order.
    assert_eq!(
        reopened.queued_deliveries(&s).unwrap()[0].first_cursor,
        reopened
            .delivery(&s, &r("delivery/a"))
            .unwrap()
            .unwrap()
            .first_cursor
    );
    // Claim for dispatch at a ready turn boundary: queued → dispatching, with
    // the connection generation recorded so provider events attribute to it.
    let claimed = reopened
        .dispatch_queued_delivery(&s, &r("delivery/a"), "generation-1")
        .unwrap();
    assert_eq!(claimed.phase, "dispatching");
    assert_eq!(
        claimed.request["connection_generation"],
        json!("generation-1")
    );
    // The admitted turn rides through the claim untouched.
    assert_eq!(
        claimed.request["submission"]["turn"]["text"],
        json!("waiting")
    );
    assert_eq!(
        reopened
            .dispatch_queued_delivery(&s, &r("delivery/a"), "generation-2")
            .unwrap_err()
            .code(),
        "encounter.delivery_changed"
    );
    // From dispatching the row resolves through the ordinary lifecycle.
    assert_eq!(
        reopened
            .delivery_ack(&s, &r("delivery/a"), true, None)
            .unwrap()
            .phase,
        "submitted"
    );
    let event = json!({"kind":"provider","connection_generation":"generation-1","event":{"TurnEnded":{"stop":{"Completed":{"stop_reason":"fixture"}}}}});
    reopened.append(&s, &event).unwrap();
    assert_eq!(
        reopened
            .delivery(&s, &r("delivery/a"))
            .unwrap()
            .unwrap()
            .phase,
        "returned"
    );
    // Terminal resolution releases the slot for the next wait.
    assert!(
        store(&home)
            .queue_delivery(&s, &r("delivery/b"), &sender, &json!({"queued":true}))
            .unwrap()
            .fresh
    );
}
#[test]
fn drain_refusal_is_terminal_and_releases_the_slot() {
    let t = tempfile::tempdir().unwrap();
    let home = AikitHome::at(t.path().join("home"));
    let s = r("agent-session/a");
    let store = store(&home);
    store
        .queue_delivery(
            &s,
            &r("delivery/a"),
            &r("agent:sender"),
            &json!({"submission":{"turn":{"text":"waiting"}},"queued":true}),
        )
        .unwrap();
    // An admission failure at drain is journaled, terminal, and never delivered.
    let refused = store
        .refuse_queued_delivery(
            &s,
            &r("delivery/a"),
            "encounter.disclosure_denied",
            "sender fell outside the disclosed transport",
        )
        .unwrap();
    assert_eq!(refused.phase, "failed");
    assert!(refused.terminal_cursor.is_some());
    // Failed is outside reconcile's reach: no replay, no resurrection.
    assert!(store
        .reconcile_delivery(
            &s,
            &r("delivery/a"),
            &r("evidence/operator-review"),
            "failed"
        )
        .is_err());
    // Refusal released the single active slot.
    assert!(
        store
            .queue_delivery(
                &s,
                &r("delivery/b"),
                &r("agent:sender"),
                &json!({"queued":true})
            )
            .unwrap()
            .fresh
    );
    // Only a queued row can be refused.
    assert_eq!(
        store
            .refuse_queued_delivery(&s, &r("delivery/b"), "encounter.test", "still queued")
            .unwrap()
            .phase,
        "failed"
    );
    assert_eq!(
        store
            .refuse_queued_delivery(&s, &r("delivery/b"), "encounter.test", "already terminal")
            .unwrap_err()
            .code(),
        "encounter.delivery_changed"
    );
    // The refusal evidence is on the journal.
    let page = store.events(&s, 0, 50).unwrap();
    assert!(page
        .events
        .iter()
        .any(|e| e.event["kind"] == "queued-delivery-refused"
            && e.event["delivery_ref"] == json!("delivery/a")));
}
