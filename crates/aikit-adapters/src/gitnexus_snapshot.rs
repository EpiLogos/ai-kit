//! GitNexus's read-only graph open can repair/quarantine its input. Give it only
//! a bounded, quiescent private copy; never forward the owner's graph or registry.
use aikit_core::{AikitError, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) struct Snapshot {
    _directory: tempfile::TempDir,
    pub home: PathBuf,
}
fn failure(error: impl std::fmt::Display) -> AikitError {
    AikitError::new("knowledge.gitnexus_read_snapshot", error.to_string())
}
fn active_sidecars(storage: &Path) -> Result<()> {
    for name in [
        "lbug.wal",
        "lbug.shadow",
        "lbug.lock",
        "kuzu",
        "kuzu.wal",
        "kuzu.lock",
    ] {
        if storage.join(name).exists() {
            return Err(failure(format!("{name} present; read refuses recovery or legacy migration; complete owner indexing explicitly")));
        }
    }
    Ok(())
}
impl Snapshot {
    pub fn read_only(root: &Path) -> Result<Self> {
        let began = Instant::now();
        let storage = root.join(".gitnexus");
        active_sidecars(&storage)?;
        let source = storage.join("lbug");
        let before = fs::metadata(&source).map_err(failure)?;
        if !before.is_file() || before.len() > 512 * 1024 * 1024 {
            return Err(failure(
                "existing graph must be a regular file no larger than 512 MiB for an isolated read",
            ));
        }
        let metadata_path = if storage.join("gitnexus.json").exists() {
            storage.join("gitnexus.json")
        } else {
            storage.join("meta.json")
        };
        let mut metadata_bytes = Vec::new();
        fs::File::open(&metadata_path)
            .map_err(failure)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut metadata_bytes)
            .map_err(failure)?;
        if metadata_bytes.len() > 1024 * 1024 {
            return Err(failure("index metadata exceeds 1 MiB"));
        }
        let metadata: serde_json::Value =
            serde_json::from_slice(&metadata_bytes).map_err(failure)?;
        let directory = tempfile::Builder::new()
            .prefix("aikit-gitnexus-read-")
            .tempdir()
            .map_err(failure)?;
        let snapshot = directory.path().join("index");
        let home = directory.path().join("home");
        fs::create_dir(&snapshot).map_err(failure)?;
        fs::create_dir(&home).map_err(failure)?;
        let mut reader = fs::File::open(&source).map_err(failure)?;
        let mut writer = fs::File::create(snapshot.join("lbug")).map_err(failure)?;
        let mut buffer = vec![0; 1024 * 1024];
        let mut copied = 0u64;
        loop {
            if began.elapsed() > Duration::from_secs(5) {
                return Err(failure("graph snapshot exceeded five seconds"));
            }
            let count = reader.read(&mut buffer).map_err(failure)?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            if copied > 512 * 1024 * 1024 {
                return Err(failure("graph grew beyond 512 MiB"));
            }
            writer.write_all(&buffer[..count]).map_err(failure)?;
        }
        drop(writer);
        active_sidecars(&storage)?;
        let after = fs::metadata(&source).map_err(failure)?;
        if before.len() != copied
            || before.len() != after.len()
            || before.modified().map_err(failure)? != after.modified().map_err(failure)?
            || fs::read(&metadata_path).map_err(failure)? != metadata_bytes
        {
            return Err(failure(
                "owner index changed during snapshot; retry after indexing completes",
            ));
        }
        fs::write(snapshot.join("gitnexus.json"), &metadata_bytes).map_err(failure)?;
        let registry = serde_json::json!([{
            "name": root.to_string_lossy(), "path": root, "storagePath": snapshot,
            "lastCommit": metadata.get("lastCommit"), "indexedAt": metadata.get("indexedAt"),
            "stats": metadata.get("stats"), "branch": metadata.get("branch")
        }]);
        fs::write(
            home.join("registry.json"),
            serde_json::to_vec(&registry).map_err(failure)?,
        )
        .map_err(failure)?;
        Ok(Self {
            _directory: directory,
            home,
        })
    }
}
