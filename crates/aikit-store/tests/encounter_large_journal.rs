//! Real SQLite WAL connections concurrently retain and page large native event bodies.
use aikit_core::ResourceRef;
use aikit_store::{encounter::EncounterStore, AikitHome};
use serde_json::json;
use std::{
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};

#[test]
fn concurrent_large_events_are_complete_and_survive_reopen() {
    let root = tempfile::tempdir().unwrap();
    let home = AikitHome::at(root.path());
    let writer = EncounterStore::open(&home).unwrap();
    let reader = EncounterStore::open(&home).unwrap();
    let session = ResourceRef::parse("agent-session/large-concurrent-journal").unwrap();
    // Exceeds both the 8 KiB stream-buffer boundary and event-page byte budget.
    let body = "native context ∆\n\"source\" ".repeat(16_384);
    assert!(body.len() > 256 * 1024);
    let barrier = Arc::new(Barrier::new(2));
    let worker = {
        let barrier = barrier.clone();
        let session = session.clone();
        let body = body.clone();
        thread::spawn(move || {
            barrier.wait();
            for sequence in 0..24 {
                writer
                    .append(
                        &session,
                        &json!({"kind":"context-evidence","sequence":sequence,"body":body}),
                    )
                    .unwrap();
                thread::yield_now();
            }
        })
    };
    barrier.wait();
    let deadline = Instant::now() + Duration::from_secs(30);
    let (mut cursor, mut count) = (0, 0);
    while count < 24 {
        assert!(
            Instant::now() < deadline,
            "concurrent journal paging timed out"
        );
        let page = reader.events(&session, cursor, 256).unwrap();
        for event in &page.events {
            assert_eq!(event.event["sequence"], count);
            assert_eq!(event.event["body"], body);
            assert!(event.cursor > cursor);
            count += 1;
        }
        cursor = page.next_cursor;
        thread::yield_now();
    }
    worker.join().unwrap();
    drop(reader);
    let reopened = EncounterStore::open(&home).unwrap();
    cursor = 0;
    for sequence in 0..24 {
        let page = reopened.events(&session, cursor, 1).unwrap();
        assert_eq!(page.events.len(), 1);
        assert_eq!(
            page.events[0].event,
            json!({"kind":"context-evidence","sequence":sequence,"body":body})
        );
        cursor = page.next_cursor;
        assert_eq!(page.more, sequence < 23);
    }
    assert!(reopened
        .events(&session, cursor, 1)
        .unwrap()
        .events
        .is_empty());
}
