//! The inbox channel is broker-readable through the one application service:
//! whatever the system (or an agent) publishes, `Service::inbox_items` — the seam
//! `aikit inbox list --json` runs through — reads back.

use std::collections::BTreeMap;

use aikit_cli::app::Service;
use aikit_store::channel::{InboxChannel, InboxKind, NewItem};
use aikit_store::home::AikitHome;
use aikit_store::index::Index;
use tempfile::TempDir;

#[test]
fn the_service_reads_back_channel_items_the_system_published() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    // The system publishes a drift notice (as the curator/drift path would).
    let aikit_home = AikitHome::at(home.path());
    aikit_home.ensure_layout().unwrap();
    let index = Index::open(&aikit_home.database()).unwrap();
    InboxChannel::new(&index)
        .publish(NewItem::new(
            InboxKind::DriftNotice,
            "script/test/nt changed since it was applied",
            "re-apply to pick up the change",
        ))
        .unwrap();
    drop(index);

    // A fresh Service on the same home sees it — the broker's read path.
    let env: BTreeMap<String, String> = BTreeMap::new();
    let service = Service::open(AikitHome::at(home.path()), project.path(), |k| {
        env.get(k).cloned()
    })
    .unwrap();
    let items = service.inbox_items(false).unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, InboxKind::DriftNotice);
    assert!(items[0].title.contains("script/test/nt"));
}
