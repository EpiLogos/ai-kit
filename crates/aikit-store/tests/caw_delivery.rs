//! Transport state tests; provider events below are explicitly fixtures.
use aikit_core::ResourceRef;
use aikit_store::{encounter::EncounterStore, AikitHome};
use serde_json::json;
use std::sync::{Arc, Barrier};
fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}
#[test]
fn restart_duplicate_conflict_and_uncertain_are_not_a_second_effect() {
    let t = tempfile::tempdir().unwrap();
    let home = AikitHome::at(t.path().join("home"));
    let s = r("agent-session/a");
    let d = r("delivery/a");
    let sender = r("agent:sender");
    let request = json!({"submission":"work","connection_generation":"g1"});
    let store = EncounterStore::open(&home).unwrap();
    store.set_draft(&s, 0, "Untouched human text").unwrap();
    assert!(
        store
            .reserve_delivery(&s, &d, &sender, &request)
            .unwrap()
            .fresh
    );
    drop(store);
    let store = EncounterStore::open(&home).unwrap();
    assert!(
        !store
            .reserve_delivery(&s, &d, &sender, &request)
            .unwrap()
            .fresh
    );
    assert_eq!(
        store
            .reserve_delivery(&s, &d, &sender, &json!({"different":true}))
            .unwrap_err()
            .code(),
        "encounter.delivery_conflict"
    );
    assert_eq!(
        store
            .delivery_ack(&s, &d, false, Some("lost ACK"))
            .unwrap()
            .phase,
        "uncertain"
    );
    assert_eq!(
        store
            .reserve_delivery(&s, &r("delivery/b"), &sender, &request)
            .unwrap_err()
            .code(),
        "encounter.delivery_pending"
    );
    assert_eq!(store.draft(&s).unwrap().text, "Untouched human text");
    assert!(store
        .reconcile_delivery(&s, &d, &r("evidence/operator-review"), "submitted")
        .is_err());
    assert_eq!(
        store
            .reconcile_delivery(&s, &d, &r("evidence/operator-review"), "uncertain")
            .unwrap()
            .phase,
        "reconciled-no-replay"
    );
    assert!(
        !store
            .reserve_delivery(&s, &d, &sender, &request)
            .unwrap()
            .fresh
    );
}
#[test]
fn reply_before_ack_is_terminal_and_old_generation_cannot_answer_new_delivery() {
    let t = tempfile::tempdir().unwrap();
    let store = EncounterStore::open(&AikitHome::at(t.path())).unwrap();
    let s = r("agent-session/a");
    let d = r("delivery/a");
    store
        .reserve_delivery(
            &s,
            &d,
            &r("agent:sender"),
            &json!({"connection_generation":"new"}),
        )
        .unwrap();
    let event = |generation: &str| json!({"kind":"provider","connection_generation":generation,"event":{"TurnEnded":{"stop":{"Completed":{"stop_reason":"fixture"}}}}});
    store.append(&s, &event("old")).unwrap();
    assert_eq!(
        store.delivery(&s, &d).unwrap().unwrap().phase,
        "dispatching"
    );
    store.append(&s, &event("new")).unwrap();
    assert_eq!(
        store.delivery_ack(&s, &d, true, None).unwrap().phase,
        "returned"
    );
    let page = store.events(&s, 0, 10).unwrap();
    assert!(page
        .events
        .iter()
        .find(|e| e.event["connection_generation"] == "old")
        .unwrap()
        .event
        .get("delivery_ref")
        .is_none());
}
#[test]
fn concurrent_process_connections_reserve_one_durable_intent() {
    let t = tempfile::tempdir().unwrap();
    let home = AikitHome::at(t.path());
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let home = home.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                let store = EncounterStore::open(&home).unwrap();
                b.wait();
                store
                    .reserve_delivery(
                        &r("agent-session/a"),
                        &r("delivery/a"),
                        &r("agent:sender"),
                        &json!({"fixture":true}),
                    )
                    .unwrap()
                    .fresh
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .filter(|h| h.thread().id() != std::thread::current().id())
            .map(|h| usize::from(h.join().unwrap()))
            .sum::<usize>(),
        1
    );
    let store = EncounterStore::open(&home).unwrap();
    assert_eq!(
        store
            .events(&r("agent-session/a"), 0, 30)
            .unwrap()
            .events
            .len(),
        1
    );
}
