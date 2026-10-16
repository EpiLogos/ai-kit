//! The SemanticWiki projection cache: one deletable SQLite materialisation per
//! horizon, `<cache>/knowledge/wiki/<blake3 of the horizon root>.sqlite3`.
//! Every projection is rebuilt from the World on demand, so a horizon nobody
//! reads any more is only disk.
//!
//! A projection's last use is the later of its two stamps. A rebuild that
//! changes it moves its modification time; a read keeps its access time
//! current itself, at most once a day, because the platform does not (APFS
//! advances the access time only while it trails the modification time, so a
//! projection read every day for a month can show a month-old access time).
//!
//! Pruning happens only where a projection is written new: then every other
//! horizon unused for [`RETENTION`] is removed, its SQLite sidecars with it.
//! Reading an existing projection never removes anything.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aikit_core::knowledge_wiki::WikiObject;
use aikit_core::knowledge_wiki_provider::WikiRegisterRevision;
use aikit_core::Result;
use aikit_store::SqliteWikiProvider;

const DAY: u64 = 86_400;

/// Fourteen days: AIKit's default retention for the coordination state of a
/// NOW context (`aikit-store` `now_context::default_coordination_retention`),
/// the code's own measure of how long rebuildable working state outlives its
/// last use. A projection is cheaper to lose than that state — the next read
/// rebuilds it from the World — so it is kept no longer.
pub(super) const RETENTION: Duration = Duration::from_secs(14 * DAY);

/// A read refreshes the access stamp only when it is older than this, so
/// reads cost one metadata write per horizon per day at most.
const READ_STAMP_INTERVAL: Duration = Duration::from_secs(DAY);

/// SQLite's companions of one projection file.
const SIDECARS: [&str; 3] = ["-journal", "-wal", "-shm"];

/// The projection file of one horizon.
pub(super) fn projection_path(cache: &Path, root: &Path) -> PathBuf {
    let horizon = blake3::hash(root.to_string_lossy().as_bytes()).to_hex();
    cache
        .join("knowledge/wiki")
        .join(format!("{horizon}.sqlite3"))
}

/// Materialise one horizon's projection. A projection written new prunes the
/// horizons unused for [`RETENTION`]; a read of an existing one only keeps
/// its own access stamp current.
pub(super) fn materialise(
    cache: &Path,
    root: &Path,
    objects: Vec<WikiObject>,
    registers: Vec<WikiRegisterRevision>,
    now: SystemTime,
) -> Result<SqliteWikiProvider> {
    let path = projection_path(cache, root);
    let new = matches!(
        fs::symlink_metadata(&path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    );
    let provider = SqliteWikiProvider::rebuild(&path, objects, registers)?;
    if new {
        if let Some(directory) = path.parent() {
            prune(directory, &path, now, RETENTION);
        }
    } else {
        stamp_read(&path, now);
    }
    Ok(provider)
}

/// The later of a projection's access and modification times.
fn last_use(metadata: &fs::Metadata) -> Option<SystemTime> {
    let modified = metadata.modified().ok();
    let accessed = metadata.accessed().ok();
    modified.max(accessed)
}

fn unused_for(metadata: &fs::Metadata, now: SystemTime, span: Duration) -> bool {
    last_use(metadata)
        .and_then(|used| now.duration_since(used).ok())
        .is_some_and(|age| age >= span)
}

/// Keep a read projection's access stamp within a day of its last read.
fn stamp_read(path: &Path, now: SystemTime) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if !unused_for(&metadata, now, READ_STAMP_INTERVAL) {
        return;
    }
    if let Ok(file) = fs::File::open(path) {
        let _ = file.set_times(fs::FileTimes::new().set_accessed(now));
    }
}

/// Only names the projection writer emits: 64 lowercase hex digits.
fn is_projection_name(name: &str) -> bool {
    name.strip_suffix(".sqlite3").is_some_and(|stem| {
        stem.len() == 64
            && stem
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// Remove every projection in `directory` other than `kept` that has not
/// been used for `retention`, with its SQLite sidecars. Foreign files and
/// anything that is not a plain file are left alone. Returns how many
/// projections were removed.
pub(super) fn prune(directory: &Path, kept: &Path, now: SystemTime, retention: Duration) -> usize {
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };
    let kept = kept.file_name();
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if Some(name.as_os_str()) == kept {
            continue;
        }
        let Some(name) = name.to_str().filter(|name| is_projection_name(name)) else {
            continue;
        };
        let path = directory.join(name);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || !unused_for(&metadata, now, retention) {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            removed += 1;
            for sidecar in SIDECARS {
                let _ = fs::remove_file(directory.join(format!("{name}{sidecar}")));
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    const HORIZON: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn cache() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("knowledge-projection-")
            .tempdir()
            .unwrap()
    }

    fn aged(path: &Path, now: SystemTime, days: u64) {
        let then = now - Duration::from_secs(days * DAY);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_accessed(then).set_modified(then))
            .unwrap();
    }

    fn write_aged(path: &Path, now: SystemTime, days: u64) {
        fs::write(path, b"projection").unwrap();
        aged(path, now, days);
    }

    fn horizon(n: u8) -> String {
        format!("{}{:02x}.sqlite3", &HORIZON[..62], n)
    }

    fn objects() -> Vec<WikiObject> {
        aikit_core::knowledge_wiki::parse_wiki_objects(
            &serde_json::json!({"objects": [{
                "profile": "okf-wiki/v1", "object": "node", "ref": "wiki:node:projection-cache",
                "type": "Concept", "title": "Projection cache", "space_refs": [],
                "source_refs": [], "provenance": [],
            }]})
            .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn a_prune_removes_only_projections_unused_past_retention_with_their_sidecars() {
        let dir = cache();
        let now = SystemTime::now();
        let stale = dir.path().join(horizon(1));
        let recent = dir.path().join(horizon(2));
        let kept = dir.path().join(horizon(3));
        let foreign = dir.path().join("notes.sqlite3");
        let short = dir.path().join("0123.sqlite3");
        write_aged(&stale, now, 15);
        write_aged(&recent, now, 13);
        write_aged(&kept, now, 30);
        write_aged(&foreign, now, 90);
        write_aged(&short, now, 90);
        let journal = dir.path().join(format!("{}-journal", horizon(1)));
        let wal = dir.path().join(format!("{}-wal", horizon(1)));
        fs::write(&journal, b"j").unwrap();
        fs::write(&wal, b"w").unwrap();
        // Read yesterday, written a month ago: in use, so kept.
        let read = dir.path().join(horizon(4));
        write_aged(&read, now, 30);
        fs::File::open(&read)
            .unwrap()
            .set_times(fs::FileTimes::new().set_accessed(now - Duration::from_secs(DAY)))
            .unwrap();

        assert_eq!(prune(dir.path(), &kept, now, RETENTION), 1);
        assert!(!stale.exists() && !journal.exists() && !wal.exists());
        for survivor in [&recent, &kept, &foreign, &short, &read] {
            assert!(survivor.exists(), "{} survives", survivor.display());
        }
    }

    #[test]
    fn only_a_new_projection_prunes_and_a_read_keeps_its_own_stamp_current() {
        let home = cache();
        let cache = home.path().join("cache");
        let now = SystemTime::now();
        let first = Path::new("/horizon/first");
        let second = Path::new("/horizon/second");

        // Written new: nothing stale yet.
        materialise(&cache, first, objects(), Vec::new(), now).unwrap();
        let first_path = projection_path(&cache, first);
        let directory = first_path.parent().unwrap().to_path_buf();
        let stale = directory.join(horizon(9));
        write_aged(&stale, now, 20);

        // Reading an existing projection never prunes, even past retention…
        aged(&first_path, now, 3);
        materialise(&cache, first, objects(), Vec::new(), now).unwrap();
        assert!(stale.exists(), "a read never removes a projection");
        // …and the read brought its own access stamp up to date.
        let accessed = fs::metadata(&first_path).unwrap().accessed().unwrap();
        assert!(now.duration_since(accessed).unwrap_or_default() < Duration::from_secs(1));

        // A new horizon's write prunes the stale one and keeps the read one.
        materialise(&cache, second, objects(), Vec::new(), now).unwrap();
        assert!(!stale.exists(), "a new projection prunes the stale horizon");
        assert!(first_path.exists() && projection_path(&cache, second).exists());
    }
}
