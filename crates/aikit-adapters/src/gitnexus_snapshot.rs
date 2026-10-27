//! GitNexus's read-only graph open can repair/quarantine its input. Give it only
//! a quiescent private copy; never forward the owner's graph or registry.
//!
//! The copy has no size or time bound of its own. Both used to: a 512 MiB size
//! gate and a five-second copy budget. Measured 27 Oct 2026, a cold `query`
//! against a clone of O-I's 1.5 GiB graph answers in 0.6 to 2.7 s, so the gate
//! was refusing a graph that reads fast, and ProjectMap code search for O-I was
//! dark behind it. Time is bounded once, by the deadline the search runs under.
use aikit_core::{AikitError, Result};
use std::fs;
use std::path::{Path, PathBuf};

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
        let storage = root.join(".gitnexus");
        active_sidecars(&storage)?;
        let source = storage.join("lbug");
        let before = fs::metadata(&source).map_err(failure)?;
        if !before.is_file() {
            return Err(failure("existing graph must be a regular file"));
        }
        let metadata_path = if storage.join("gitnexus.json").exists() {
            storage.join("gitnexus.json")
        } else {
            storage.join("meta.json")
        };
        let metadata_bytes = fs::read(&metadata_path).map_err(failure)?;
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
        // The platform copy clones the graph where the filesystem can (APFS
        // clonefile, a reflink elsewhere), so a query no longer rewrites
        // hundreds of MiB; the quiescence check below decides whether the copy
        // stands.
        let copied = fs::copy(&source, snapshot.join("lbug")).map_err(failure)?;
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
        // The storage self-identifies its repository and storage directory;
        // current GitNexus validation refuses a registry entry whose storage
        // metadata names another location. The isolated copy speaks for the
        // same repository at this snapshot's paths.
        let mut relocated = metadata.clone();
        if let Some(object) = relocated.as_object_mut() {
            object.insert("repoPath".into(), serde_json::json!(root));
            object.insert("storagePath".into(), serde_json::json!(snapshot));
        }
        let relocated_bytes = serde_json::to_vec(&relocated).map_err(failure)?;
        fs::write(snapshot.join("gitnexus.json"), &relocated_bytes).map_err(failure)?;
        let registry = serde_json::json!([{
            "name": root.to_string_lossy(), "path": root, "storagePath": snapshot,
            "lastCommit": relocated.get("lastCommit"), "indexedAt": relocated.get("indexedAt"),
            "stats": relocated.get("stats"), "branch": relocated.get("branch")
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
