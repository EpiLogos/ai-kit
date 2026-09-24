//! Protect configured bookmark stores from the migrations performed by bkmr
//! even on `search` and `show`. Only a private online SQLite snapshot reaches
//! that executable; opening the original never requests write access.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aikit_core::{AikitError, Result};
use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags,
};

const MAX_BYTES: u64 = 256 * 1024 * 1024;
const BACKUP_BUDGET: Duration = Duration::from_secs(5);

pub(super) struct Snapshot {
    _directory: tempfile::TempDir,
    pub database: PathBuf,
    pub config: PathBuf,
}

fn failure(error: impl std::fmt::Display) -> AikitError {
    AikitError::new("knowledge.bkmr_snapshot_failed", error.to_string())
}

impl Snapshot {
    pub fn read_only(source: &Path) -> Result<Self> {
        let started = Instant::now();
        let original = Connection::open_with_flags(
            source,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(failure)?;
        original
            .busy_timeout(Duration::from_millis(50))
            .map_err(failure)?;
        // Pin one consistent version, including committed WAL pages. Immutable
        // URI mode and a filesystem copy can both miss the live WAL contents.
        original
            .execute_batch("PRAGMA query_only=ON; BEGIN;")
            .map_err(failure)?;
        let pages: u64 = original
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .map_err(failure)?;
        let page_size: u64 = original
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .map_err(failure)?;
        if pages.saturating_mul(page_size) > MAX_BYTES {
            return Err(failure(
                "bookmark store exceeds the 256 MiB read snapshot limit",
            ));
        }
        let directory = tempfile::Builder::new()
            .prefix("aikit-bkmr-read-")
            .tempdir()
            .map_err(failure)?;
        let database = directory.path().join("bookmarks.db");
        let config = directory.path().join("config.toml");
        let mut destination = Connection::open(&database).map_err(failure)?;
        {
            let backup = Backup::new(&original, &mut destination).map_err(failure)?;
            loop {
                if started.elapsed() >= BACKUP_BUDGET {
                    return Err(failure(
                        "bookmark read snapshot exceeded its five second budget",
                    ));
                }
                let step = backup.step(128).map_err(failure)?;
                if (backup.progress().pagecount.max(0) as u64).saturating_mul(page_size) > MAX_BYTES
                {
                    return Err(failure(
                        "bookmark store exceeds the 256 MiB read snapshot limit",
                    ));
                }
                match step {
                    StepResult::Done => break,
                    StepResult::Busy | StepResult::Locked => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    _ => {}
                }
            }
        }
        drop(destination);
        // An explicit config prevents consulting the person's bkmr config.
        // --db takes precedence over configuration and BKMR_DB_URL upstream.
        std::fs::write(&config, "# AIKit read snapshot; no personal settings\n")
            .map_err(failure)?;
        Ok(Self {
            _directory: directory,
            database,
            config,
        })
    }
}
