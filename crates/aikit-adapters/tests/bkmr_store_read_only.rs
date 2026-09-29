//! These regressions execute the installed bkmr, with stores created by its
//! native CLI. No personal database, network fetch, or embedding is used.
use aikit_adapters::{
    bkmr::{BkmrStore, BkmrStoreSearchProvider},
    runner::{CommandRunner, SystemRunner},
};
use aikit_core::knowledge_source_pool::{SourcePoolProvider, SourceSearchMode};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

fn binary() -> Option<String> {
    let binary = std::env::var("AIKIT_BKMR_BIN").unwrap_or_else(|_| "bkmr".into());
    let found = SystemRunner::new()
        .run(&[binary.clone(), "--version".into()])
        .is_ok_and(|output| output.status == 0);
    assert!(
        found || std::env::var_os("AIKIT_REQUIRE_BKMR_REAL").is_none(),
        "real bkmr required but unavailable"
    );
    found.then_some(binary)
}

fn native(binary: &str, config: &Path, database: &Path, args: &[&str]) {
    let mut argv = vec![
        binary.into(),
        "--config".into(),
        config.display().to_string(),
        "--db".into(),
        database.display().to_string(),
    ];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let result = SystemRunner::new()
        .run_with_timeout(&argv, Duration::from_secs(30))
        .expect("native bkmr completes");
    assert_eq!(result.status, 0, "{}: {}", argv.join(" "), result.stderr);
}

fn create_store(binary: &str, directory: &Path) -> PathBuf {
    let config = directory.join("config.toml");
    std::fs::write(&config, "# isolated native regression\n").unwrap();
    let database = directory.join("original.db");
    native(
        binary,
        &config,
        &database,
        &["create-db", database.to_str().unwrap()],
    );
    native(
        binary,
        &config,
        &database,
        &[
            "add",
            "Galaxies contain snapshotquasars",
            "astronomy",
            "--title",
            "Snapshot astronomy",
            "--type",
            "text",
            "--no-web",
            "--no-embed",
        ],
    );
    database
}

fn bytes(directory: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                path,
            )
        })
        .filter(|(name, _)| !name.ends_with("-shm"))
        .map(|(name, path)| (name, std::fs::read(path).unwrap()))
        .collect()
}

fn assert_reads(binary: &str, database: &Path, term: &str) {
    let provider = BkmrStoreSearchProvider::connect(
        SystemRunner::new(),
        binary,
        vec![BkmrStore {
            name: "native-proof".into(),
            path: database.into(),
        }],
    );
    let hits = provider
        .search(term, SourceSearchMode::Fulltext, &[], 10)
        .unwrap();
    assert_eq!(hits.len(), 1, "{}", provider.status().detail);
    assert!(hits[0]
        .source
        .as_str()
        .starts_with("source:bkmr:native-proof:"));
    let reading = provider
        .read(&hits[0].source)
        .unwrap()
        .expect("native show reads the same row");
    assert!(reading.body.contains(term), "{}", reading.body);
}

#[test]
fn native_search_and_show_never_migrate_the_original_legacy_store() {
    let Some(binary) = binary() else { return };
    let directory = tempfile::tempdir().unwrap();
    let database = create_store(&binary, directory.path());
    // Revert the actual upstream 2026-04-03 migration on this disposable
    // native database. The installed CLI must migrate its snapshot to read it.
    {
        let db = rusqlite::Connection::open(&database).unwrap();
        db.execute_batch(
            "PRAGMA journal_mode=DELETE; ALTER TABLE bookmarks DROP COLUMN accessed_at;
            DELETE FROM __diesel_schema_migrations WHERE version >= '20260403100000';
            CREATE TRIGGER UpdateLastTime AFTER UPDATE ON bookmarks FOR EACH ROW
              WHEN NEW.last_update_ts <= OLD.last_update_ts BEGIN
              UPDATE bookmarks SET last_update_ts=CURRENT_TIMESTAMP WHERE id=OLD.id; END;",
        )
        .unwrap();
    }
    let before = bytes(directory.path());
    assert_reads(&binary, &database, "snapshotquasars");
    assert_eq!(
        before,
        bytes(directory.path()),
        "original bytes, schema and directory entries must remain unchanged"
    );
    let db = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('bookmarks') WHERE name='accessed_at')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!exists, "only the private snapshots may have been migrated");
}

#[test]
fn native_search_sees_committed_live_wal_without_mutating_the_original() {
    let Some(binary) = binary() else { return };
    let directory = tempfile::tempdir().unwrap();
    let database = create_store(&binary, directory.path());
    let writer = rusqlite::Connection::open(&database).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
        UPDATE bookmarks SET url='livewalquasars are visible through the online snapshot';",
        )
        .unwrap();
    assert!(
        std::fs::metadata(database.with_extension("db-wal"))
            .unwrap()
            .len()
            > 0
    );
    let before = bytes(directory.path());
    assert_reads(&binary, &database, "livewalquasars");
    assert_eq!(
        before,
        bytes(directory.path()),
        "live source main database and WAL bytes must remain unchanged"
    );
    drop(writer);
}
