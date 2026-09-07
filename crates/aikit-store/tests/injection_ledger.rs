//! The injection ledger (W1 dedup law): rendered-content hashes already
//! injected for a context are remembered, so unchanged ordinary payloads are
//! not re-injected. Every assertion runs against a real SQLite file.

use std::path::Path;

use aikit_core::ContextId;
use aikit_store::index::Index;

fn index(dir: &Path) -> Index {
    Index::open(&dir.join("state/aikit.sqlite3")).unwrap()
}

fn context(id: &str) -> ContextId {
    ContextId::parse(id).unwrap()
}

#[test]
fn the_ledger_migrates_records_and_answers_the_dedup_check() {
    let tmp = tempfile::tempdir().unwrap();
    let index = index(tmp.path());

    // The migration applied: the ledger exists and starts empty.
    assert!(index.applied_migrations().unwrap().contains(&"0002-injection-ledger".to_string()));
    let ctx = context("ctx_test");
    assert!(!index.injection_seen(&ctx, "abc123").unwrap());

    index.record_injection(&ctx, "abc123").unwrap();
    assert!(index.injection_seen(&ctx, "abc123").unwrap(), "recorded content is seen");
    assert!(!index.injection_seen(&ctx, "different").unwrap(), "other content is not");
    assert!(!index.injection_seen(&context("ctx_other"), "abc123").unwrap(), "other contexts are not");

    // Re-recording is idempotent (a refresh, not a duplicate).
    index.record_injection(&ctx, "abc123").unwrap();
    assert!(index.injection_seen(&ctx, "abc123").unwrap());
}
