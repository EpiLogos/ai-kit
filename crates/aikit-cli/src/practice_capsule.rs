//! A practice capsule as data: reading it as AIKit proves it, exporting it as
//! a portable archive, and verifying an archive before another World adopts it.
//!
//! A practice (a Skill, Method or Methodology) is a capsule directory whose
//! content revision AIKit computes over the manifest and every payload file,
//! path and permission bits included (`aikit_store::registry`). Everything here
//! speaks that one revision: a reading is only returned when the files on disk
//! recompute to the revision the catalogue (or a retained snapshot) names, an
//! export carries the same files and revision, and an archive is only accepted
//! when its files recompute to the revision it declares. Publication of an
//! archive is never trust — adopting it is `system source add-capsule`, then
//! the ordinary sync → promote path.

use std::path::{Component, Path};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use aikit_core::catalog::Catalog;
use aikit_core::id::CapsuleId;
use aikit_core::method::praxis_form;
use aikit_core::{AikitError, Capsule, Kind, Result, Revision};
use aikit_store::home::AikitHome;
use aikit_store::registry::{revision_of_entries, RevisionEntry, Snapshot, MANIFEST_FILE};

/// The portable archive a practice is exported as.
pub const PRACTICE_CAPSULE_SCHEMA: &str = "aikit.practice-capsule/v1";
/// `aikit praxis read` — a practice's files as AIKit proves them.
pub const PRACTICE_READING_SCHEMA: &str = "aikit.practice-reading/v1";
/// The capsule revision algorithm every revision here is computed with.
pub const REVISION_BASIS: &str = "aikit-capsule-revision-v2";

/// Bounds on an archive, matching the owner-bundle bounds sources already use.
const MAX_FILES: usize = 4096;
const MAX_BYTES: usize = 32 * 1024 * 1024;

/// One file of a capsule directory, `manifest.toml` included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapsuleFile {
    /// `/`-separated, relative to the capsule directory.
    pub path: String,
    /// Permission bits (`mode & 0o7777`), as the revision reads them.
    pub mode: u32,
    pub contents: Vec<u8>,
}

/// Where the proven files came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedFrom {
    /// The registry source the capsule was catalogued from.
    pub source_id: String,
    /// The managed source snapshot holding it, when the source is managed.
    #[serde(default)]
    pub snapshot: Option<String>,
}

/// A practice capsule whose files recompute to its revision.
#[derive(Debug, Clone)]
pub struct ProvenCapsule {
    pub id: CapsuleId,
    pub name: String,
    pub form: &'static str,
    pub revision: Revision,
    pub from: ExportedFrom,
    pub files: Vec<CapsuleFile>,
}

impl ProvenCapsule {
    fn file_rows(&self) -> Vec<Value> {
        self.files.iter().map(file_row).collect()
    }

    /// The `aikit.practice-reading/v1` document.
    pub fn reading(&self) -> Value {
        json!({
            "schema": PRACTICE_READING_SCHEMA,
            "id": self.id.to_string(),
            "name": self.name,
            "form": self.form,
            "revision": self.revision.to_string(),
            "revision_basis": REVISION_BASIS,
            "source_id": self.from.source_id,
            "snapshot": self.from.snapshot,
            "files": self.file_rows(),
        })
    }

    /// The `aikit.practice-capsule/v1` archive document.
    pub fn archive(&self) -> Value {
        json!({
            "schema": PRACTICE_CAPSULE_SCHEMA,
            "id": self.id.to_string(),
            "name": self.name,
            "form": self.form,
            "revision": self.revision.to_string(),
            "revision_basis": REVISION_BASIS,
            "exported_from": self.from,
            "files": self.file_rows(),
        })
    }
}

fn file_row(file: &CapsuleFile) -> Value {
    let mut row = json!({
        "path": file.path,
        "mode": file.mode,
        "bytes": file.contents.len(),
        "sha256": sha256(&file.contents),
    });
    match std::str::from_utf8(&file.contents) {
        Ok(text) => row["text"] = json!(text),
        Err(_) => {
            row["base64"] = json!(base64::engine::general_purpose::STANDARD.encode(&file.contents))
        }
    }
    row
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

/// The capsule revision over files held in memory.
pub fn revision_of(files: &[CapsuleFile]) -> Result<Revision> {
    let manifest = files
        .iter()
        .find(|file| file.path == MANIFEST_FILE)
        .ok_or_else(|| {
            AikitError::new(
                "capsule.manifest_missing",
                "a practice capsule must carry its manifest.toml",
            )
        })?;
    let entries: Vec<RevisionEntry> = files
        .iter()
        .filter(|file| file.path != MANIFEST_FILE)
        .map(|file| RevisionEntry {
            path: file.path.clone(),
            mode: file.mode,
            contents: file.contents.clone(),
        })
        .collect();
    Ok(revision_of_entries(
        &manifest.contents,
        manifest.mode,
        &entries,
    ))
}

/// Every regular file of a capsule directory, sorted by path. A symlink is
/// refused: the revision does not follow links, so a proof must not either.
pub fn read_capsule_dir(dir: &Path) -> Result<Vec<CapsuleFile>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(dir).follow_links(false) {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "capsule.read_failed",
                format!("could not walk {}: {error}", dir.display()),
            )
        })?;
        if entry.file_type().is_symlink() {
            return Err(AikitError::new(
                "capsule.symlink_not_supported",
                format!("capsule contains symlink {}", entry.path().display()),
            ));
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        let contents = std::fs::read(path).map_err(|error| {
            AikitError::new(
                "capsule.read_failed",
                format!("{}: {error}", path.display()),
            )
        })?;
        files.push(CapsuleFile {
            path: relative,
            mode: mode_of(path)?,
            contents,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

#[cfg(unix)]
fn mode_of(path: &Path) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o7777)
        .map_err(|error| {
            AikitError::new(
                "capsule.read_failed",
                format!("{}: {error}", path.display()),
            )
        })
}

#[cfg(not(unix))]
fn mode_of(path: &Path) -> Result<u32> {
    std::fs::metadata(path)
        .map(|metadata| u32::from(metadata.permissions().readonly()))
        .map_err(|error| {
            AikitError::new(
                "capsule.read_failed",
                format!("{}: {error}", path.display()),
            )
        })
}

/// Read a catalogued practice at its active revision, or at a named revision
/// still retained by its managed source, refusing anything AIKit cannot prove.
pub fn prove(
    home: &AikitHome,
    catalog: &Snapshot,
    id: &str,
    revision: Option<&str>,
) -> Result<ProvenCapsule> {
    let capsule_id = CapsuleId::parse(id)?;
    let capsule = catalog.get(&capsule_id).ok_or_else(|| {
        AikitError::new(
            "praxis.unknown",
            format!("`{id}` is not a catalogued practice in this scope"),
        )
        .with("id", id)
    })?;
    if capsule.kind != Kind::Skill {
        return Err(AikitError::new(
            "praxis.not_a_practice",
            format!(
                "`{id}` is a {} capsule; only Skills, Methods and Methodologies are practices",
                capsule.kind.as_str()
            ),
        ));
    }
    let source_id = capsule
        .source
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    let active = capsule.revision.as_ref().map(ToString::to_string);
    let wanted = revision.map(str::to_string).or_else(|| active.clone());
    let Some(wanted) = wanted else {
        return Err(AikitError::new(
            "praxis.revision_unproven",
            format!("`{id}` carries no content revision to prove"),
        ));
    };

    if active.as_deref() == Some(wanted.as_str()) {
        let root = capsule.root.as_ref().ok_or_else(|| {
            AikitError::new(
                "praxis.revision_unproven",
                format!("`{id}` has no capsule directory to read"),
            )
        })?;
        let snapshot = crate::skill_sources::active_snapshot_of(home, &source_id);
        return proven(capsule, root, &wanted, source_id, snapshot);
    }

    // A named revision other than the active one: only a snapshot the managed
    // source still retains can answer it, and only when its files recompute.
    for (digest, dir) in
        crate::skill_sources::retained_capsule_dirs(home, &source_id, &capsule_id.to_string())
    {
        let files = read_capsule_dir(&dir)?;
        if revision_of(&files)?.as_str() == wanted {
            return proven(capsule, &dir, &wanted, source_id, Some(digest));
        }
    }
    Err(AikitError::new(
        "praxis.revision_not_retained",
        format!(
            "`{id}` is active at {}; revision {wanted} is not one AIKit retains for it",
            active.as_deref().unwrap_or("no revision")
        ),
    )
    .with("id", id)
    .with("revision", wanted))
}

fn proven(
    capsule: &Capsule,
    dir: &Path,
    wanted: &str,
    source_id: String,
    snapshot: Option<String>,
) -> Result<ProvenCapsule> {
    let files = read_capsule_dir(dir)?;
    let revision = revision_of(&files)?;
    if revision.as_str() != wanted {
        return Err(AikitError::new(
            "praxis.revision_unproven",
            format!(
                "`{}` on disk recomputes to {}, not the revision {wanted} AIKit names; \
                 refusing to read a body it cannot prove",
                capsule.id, revision
            ),
        )
        .with("path", dir.display().to_string()));
    }
    // The manifest actually on disk names the practice; for a retained
    // snapshot the description (and so the form) may differ from the active.
    let manifest = files
        .iter()
        .find(|file| file.path == MANIFEST_FILE)
        .expect("revision_of refused a capsule without a manifest");
    let parsed = Capsule::from_toml_str(&String::from_utf8_lossy(&manifest.contents))?;
    Ok(ProvenCapsule {
        id: capsule.id.clone(),
        name: parsed.name,
        form: praxis_form(&parsed.description).as_str(),
        revision,
        from: ExportedFrom {
            source_id,
            snapshot,
        },
        files,
    })
}

/// Write a proven capsule as a `aikit.practice-capsule/v1` archive.
pub fn export(capsule: &ProvenCapsule, out: &Path) -> Result<Value> {
    let archive = capsule.archive();
    let text = serde_json::to_string_pretty(&archive).map_err(|error| {
        AikitError::new(
            "capsule.export_failed",
            format!("could not encode: {error}"),
        )
    })?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "capsule.export_failed",
                format!("{}: {error}", parent.display()),
            )
        })?;
    }
    std::fs::write(out, format!("{text}\n")).map_err(|error| {
        AikitError::new(
            "capsule.export_failed",
            format!("{}: {error}", out.display()),
        )
        .with("path", out.display().to_string())
    })?;
    Ok(json!({
        "schema": PRACTICE_CAPSULE_SCHEMA,
        "id": capsule.id.to_string(),
        "form": capsule.form,
        "revision": capsule.revision.to_string(),
        "exported_from": capsule.from,
        "files": capsule.files.len(),
        "archive": out.display().to_string(),
        "archive_sha256": sha256(format!("{text}\n").as_bytes()),
    }))
}

/// An archive whose every file matches its declared digest and length and
/// whose files recompute to the revision it declares.
#[derive(Debug, Clone)]
pub struct VerifiedArchive {
    pub id: CapsuleId,
    pub name: String,
    pub form: String,
    pub revision: String,
    pub exported_from: Option<ExportedFrom>,
    pub files: Vec<CapsuleFile>,
}

fn tampered(message: impl Into<String>) -> AikitError {
    AikitError::new("capsule.archive_invalid", message)
}

/// Verify an `aikit.practice-capsule/v1` archive, refusing anything whose
/// bytes do not prove the identity and revision it declares.
pub fn verify_archive(value: &Value) -> Result<VerifiedArchive> {
    if value["schema"] != PRACTICE_CAPSULE_SCHEMA {
        return Err(tampered(format!(
            "not a {PRACTICE_CAPSULE_SCHEMA} archive (schema {})",
            value["schema"]
        )));
    }
    if value["revision_basis"] != REVISION_BASIS {
        return Err(tampered(format!(
            "the archive is not proved against {REVISION_BASIS}"
        )));
    }
    let declared_id = value["id"]
        .as_str()
        .ok_or_else(|| tampered("the archive names no practice id"))?;
    let id = CapsuleId::parse(declared_id)?;
    let declared_revision = value["revision"]
        .as_str()
        .ok_or_else(|| tampered("the archive declares no revision"))?;
    let rows = value["files"]
        .as_array()
        .ok_or_else(|| tampered("the archive carries no file list"))?;
    if rows.len() > MAX_FILES {
        return Err(tampered("the archive exceeds the file bound"));
    }
    let mut files: Vec<CapsuleFile> = Vec::with_capacity(rows.len());
    let mut total = 0usize;
    for row in rows {
        let path = row["path"]
            .as_str()
            .ok_or_else(|| tampered("an archive file has no path"))?;
        let relative = Path::new(path);
        if path.is_empty()
            || path.contains('\\')
            || !relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(tampered(format!("unsafe archive path `{path}`")));
        }
        // `Path::components` normalises `a/./b`, `a//b` and a trailing `/`,
        // so a path is accepted only in its one canonical spelling; otherwise
        // two spellings of one file would pass the duplicate check below.
        let canonical = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if canonical != path {
            return Err(tampered(format!(
                "archive path `{path}` is not in canonical form (`{canonical}`)"
            )));
        }
        // Duplicates are compared case-insensitively: on a case-insensitive
        // filesystem `a/B` and `a/b` are one file, and the later would
        // silently overwrite the earlier.
        let folded = path.to_lowercase();
        if let Some(existing) = files.iter().find(|file| file.path.to_lowercase() == folded) {
            return Err(tampered(format!(
                "duplicate archive path `{path}` (collides with `{}`)",
                existing.path
            )));
        }
        let mode = row["mode"]
            .as_u64()
            .ok_or_else(|| tampered(format!("`{path}` has no valid mode")))?;
        // Only permission bits cross from a foreign archive: setuid, setgid
        // and sticky are refused, never applied.
        if mode & !0o777 != 0 {
            return Err(AikitError::new(
                "capsule.archive_mode_unsafe",
                format!(
                    "`{path}` declares mode {mode:#o}, which carries bits beyond rwx (setuid, setgid, sticky or out of range); refusing to apply it from an archive"
                ),
            )
            .with("path", path.to_string()));
        }
        let mode = mode as u32;
        let contents = match (row["text"].as_str(), row["base64"].as_str()) {
            (Some(text), None) => text.as_bytes().to_vec(),
            (None, Some(data)) => base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|error| tampered(format!("`{path}` is not valid base64: {error}")))?,
            _ => {
                return Err(tampered(format!(
                    "`{path}` must carry exactly one of text or base64"
                )))
            }
        };
        total += contents.len();
        if total > MAX_BYTES {
            return Err(tampered("the archive exceeds the payload bound"));
        }
        if row["bytes"].as_u64() != Some(contents.len() as u64)
            || row["sha256"].as_str() != Some(sha256(&contents).as_str())
        {
            return Err(tampered(format!(
                "`{path}` does not match its declared length and sha256"
            )));
        }
        files.push(CapsuleFile {
            path: path.to_string(),
            mode,
            contents,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let revision = revision_of(&files)?;
    if revision.as_str() != declared_revision {
        return Err(AikitError::new(
            "capsule.revision_mismatch",
            format!(
                "the archive's files recompute to {revision}, not the declared {declared_revision}; \
                 refusing an archive that does not prove its revision"
            ),
        ));
    }
    let manifest = files
        .iter()
        .find(|file| file.path == MANIFEST_FILE)
        .expect("revision_of refused an archive without a manifest");
    let parsed = Capsule::from_toml_str(&String::from_utf8_lossy(&manifest.contents))?;
    if parsed.id != id {
        return Err(tampered(format!(
            "the archive declares `{id}` but its manifest names `{}`",
            parsed.id
        )));
    }
    if parsed.kind != Kind::Skill {
        return Err(tampered(format!("`{id}` is not a practice capsule")));
    }
    let exported_from = serde_json::from_value(value["exported_from"].clone()).ok();
    Ok(VerifiedArchive {
        id,
        name: parsed.name,
        form: praxis_form(&parsed.description).as_str().to_string(),
        revision: revision.to_string(),
        exported_from,
        files,
    })
}

/// Read and verify an archive file.
pub fn read_archive(path: &Path) -> Result<(Vec<u8>, VerifiedArchive)> {
    let bytes = std::fs::read(path).map_err(|error| {
        AikitError::new(
            "capsule.archive_unreadable",
            format!("{}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| tampered(format!("{} is not JSON: {error}", path.display())))?;
    let verified = verify_archive(&value)?;
    Ok((bytes, verified))
}

/// Materialise verified files into `dir` with their modes.
pub fn write_files(dir: &Path, files: &[CapsuleFile]) -> Result<()> {
    for file in files {
        let to = dir.join(&file.path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AikitError::new(
                    "source.snapshot_failed",
                    format!("{}: {error}", parent.display()),
                )
            })?;
        }
        std::fs::write(&to, &file.contents).map_err(|error| {
            AikitError::new(
                "source.snapshot_failed",
                format!("{}: {error}", to.display()),
            )
        })?;
        set_mode(&to, file.mode)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|error| {
        AikitError::new(
            "source.snapshot_failed",
            format!("{}: {error}", path.display()),
        )
    })
}

#[cfg(not(unix))]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    let mut permissions = std::fs::metadata(path)
        .map_err(|error| AikitError::new("source.snapshot_failed", error.to_string()))?
        .permissions();
    permissions.set_readonly(mode != 0);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| AikitError::new("source.snapshot_failed", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str) -> String {
        format!(
            "schema = 1\nid = \"{id}\"\nkind = \"skill\"\nname = \"darshana\"\ndescription = \"METHOD: see\"\n\n[skill]\nroot = \"payload\"\n"
        )
    }

    fn capsule_dir(root: &Path) {
        std::fs::create_dir_all(root.join("payload/scripts")).unwrap();
        std::fs::write(root.join(MANIFEST_FILE), manifest("skill/ql/darshana")).unwrap();
        std::fs::write(
            root.join("payload/SKILL.md"),
            "---\nname: darshana\ndescription: METHOD: see\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(root.join("payload/scripts/run.py"), "print('x')\n").unwrap();
        std::fs::write(root.join("payload/blob.bin"), [0xff_u8, 0xfe, 0x00]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                root.join("payload/scripts/run.py"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
    }

    #[test]
    fn in_memory_revision_equals_the_directory_revision() {
        let temp = tempfile::tempdir().unwrap();
        capsule_dir(temp.path());
        let files = read_capsule_dir(temp.path()).unwrap();
        let manifest = std::fs::read(temp.path().join(MANIFEST_FILE)).unwrap();
        assert_eq!(
            revision_of(&files).unwrap(),
            aikit_store::registry::compute_revision(temp.path(), &manifest).unwrap()
        );
    }

    fn archive_of(root: &Path) -> Value {
        let files = read_capsule_dir(root).unwrap();
        ProvenCapsule {
            id: CapsuleId::parse("skill/ql/darshana").unwrap(),
            name: "darshana".into(),
            form: "method",
            revision: revision_of(&files).unwrap(),
            from: ExportedFrom {
                source_id: "ql".into(),
                snapshot: Some("abc".into()),
            },
            files,
        }
        .archive()
    }

    #[test]
    fn an_archive_round_trips_text_binary_and_modes() {
        let temp = tempfile::tempdir().unwrap();
        capsule_dir(temp.path());
        let archive = archive_of(temp.path());
        let blob = archive["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["path"] == "payload/blob.bin")
            .unwrap();
        assert!(blob["base64"].is_string() && blob["text"].is_null());
        let verified = verify_archive(&archive).unwrap();
        assert_eq!(verified.revision, archive["revision"].as_str().unwrap());
        assert_eq!(verified.form, "method");
        let out = tempfile::tempdir().unwrap();
        write_files(out.path(), &verified.files).unwrap();
        assert_eq!(read_capsule_dir(out.path()).unwrap(), verified.files);
    }

    #[test]
    fn a_tampered_archive_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        capsule_dir(temp.path());
        let archive = archive_of(temp.path());

        // A changed body whose digest was left alone.
        let mut body = archive.clone();
        for row in body["files"].as_array_mut().unwrap() {
            if row["path"] == "payload/SKILL.md" {
                row["text"] = json!("---\nname: darshana\ndescription: METHOD: see\n---\nevil\n");
            }
        }
        assert_eq!(
            verify_archive(&body).unwrap_err().code(),
            "capsule.archive_invalid"
        );

        // A changed mode with every digest intact still moves the revision.
        let mut mode = archive.clone();
        for row in mode["files"].as_array_mut().unwrap() {
            if row["path"] == "payload/scripts/run.py" {
                row["mode"] = json!(0o644);
            }
        }
        assert_eq!(
            verify_archive(&mode).unwrap_err().code(),
            "capsule.revision_mismatch"
        );

        // An escaping path.
        let mut escape = archive.clone();
        escape["files"].as_array_mut().unwrap()[0]["path"] = json!("../x");
        assert_eq!(
            verify_archive(&escape).unwrap_err().code(),
            "capsule.archive_invalid"
        );
    }

    fn with_path_rewritten(archive: &Value, from: &str, to: &str) -> Value {
        let mut changed = archive.clone();
        for row in changed["files"].as_array_mut().unwrap() {
            if row["path"] == from {
                row["path"] = json!(to);
            }
        }
        changed
    }

    fn with_extra_copy(archive: &Value, of: &str, spelled: &str) -> Value {
        let mut changed = archive.clone();
        let rows = changed["files"].as_array_mut().unwrap();
        let mut copy = rows.iter().find(|row| row["path"] == of).unwrap().clone();
        copy["path"] = json!(spelled);
        rows.push(copy);
        changed
    }

    #[test]
    fn non_canonical_and_case_only_duplicate_paths_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        capsule_dir(temp.path());
        let archive = archive_of(temp.path());
        verify_archive(&archive).unwrap();

        for spelled in [
            "payload/./SKILL.md",
            "payload//SKILL.md",
            "payload/SKILL.md/",
            "manifest.toml/",
        ] {
            let original = if spelled.starts_with("manifest") {
                MANIFEST_FILE
            } else {
                "payload/SKILL.md"
            };
            // A second spelling of an existing file beside it.
            let duplicate = with_extra_copy(&archive, original, spelled);
            let error = verify_archive(&duplicate).unwrap_err();
            assert_eq!(error.code(), "capsule.archive_invalid", "{spelled}");
            assert!(error.message().contains("canonical"), "{spelled}: {error}");
            // The file itself under the non-canonical spelling.
            let alone = with_path_rewritten(&archive, original, spelled);
            let error = verify_archive(&alone).unwrap_err();
            assert!(error.message().contains("canonical"), "{spelled}: {error}");
        }

        let case_only = with_extra_copy(&archive, "payload/SKILL.md", "payload/skill.md");
        let error = verify_archive(&case_only).unwrap_err();
        assert_eq!(error.code(), "capsule.archive_invalid");
        assert!(error.message().contains("duplicate"), "{error}");
    }

    #[test]
    fn special_mode_bits_from_an_archive_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        capsule_dir(temp.path());
        for mode in [0o4755_u32, 0o2755, 0o1755, 0o10000] {
            // A self-consistent archive: the revision is recomputed over the
            // special mode, so only the mode rule can refuse it.
            let mut files = read_capsule_dir(temp.path()).unwrap();
            for file in &mut files {
                if file.path == "payload/scripts/run.py" {
                    file.mode = mode;
                }
            }
            let changed = ProvenCapsule {
                id: CapsuleId::parse("skill/ql/darshana").unwrap(),
                name: "darshana".into(),
                form: "method",
                revision: revision_of(&files).unwrap(),
                from: ExportedFrom {
                    source_id: "ql".into(),
                    snapshot: None,
                },
                files,
            }
            .archive();
            assert_eq!(
                verify_archive(&changed).unwrap_err().code(),
                "capsule.archive_mode_unsafe",
                "{mode:#o}"
            );
        }
    }
}
