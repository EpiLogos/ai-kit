//! Derived readings kept under the home's cache, never as an authority.
//!
//! A reading derived from an immutable record (a generation's lock, a procedure's plan) is kept
//! beside the record's file signature — its length and modification time — so the next reading
//! takes it when the file is the one it came from and derives it again when it is not. A cache
//! that is missing, unreadable, of another version or unwritable costs the derivation it would
//! have saved and nothing else: nothing is ever read *from* a cache that the record would not
//! say, and a reading never fails because it could not be kept.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::AikitHome;

/// The file a derived reading came from: if either differs the reading is derived again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileSignature {
    pub len: u64,
    pub mtime_ns: u128,
}

impl FileSignature {
    /// `(0, 0)` for a file that cannot be read: it matches nothing a real file produced.
    pub(crate) fn of(path: &Path) -> Self {
        fs::metadata(path).ok().map_or(
            Self {
                len: 0,
                mtime_ns: 0,
            },
            |stat| Self {
                len: stat.len(),
                mtime_ns: stat
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |since| since.as_nanos()),
            },
        )
    }
}

/// Where a cache named `relative` lives under the home.
pub(crate) fn path_in(home: &AikitHome, relative: &str) -> PathBuf {
    home.cache().join(relative)
}

/// The cache at `path` when it can be read as a `T`; `None` otherwise.
pub(crate) fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Keep `value` at `path`, atomically. A cache that cannot be written is a cache not kept.
pub(crate) fn save<T: Serialize>(path: &Path, value: &T) {
    let Ok(bytes) = serde_json::to_vec(value) else {
        return;
    };
    let Some(directory) = path.parent() else {
        return;
    };
    let staged = directory.join(format!(".derived.{}.tmp", std::process::id()));
    let written = fs::create_dir_all(directory)
        .and_then(|()| fs::write(&staged, bytes))
        .and_then(|()| fs::rename(&staged, path));
    if written.is_err() {
        let _ = fs::remove_file(&staged);
    }
}
