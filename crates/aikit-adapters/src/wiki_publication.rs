//! Material publication shared by the native Wiki writers. Semantic ownership
//! stays with their callers; this adapter serializes exact-basis publication.
//! A requested parent alias must continue to resolve to the held physical
//! directory at admission, effect and readback checkpoints. Namespace changes
//! after the last reading remain subsequent activity, not an atomic guarantee.
use std::path::Path;

use aikit_core::{AikitError, Result};
use sha2::Digest;

pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// Observe an ordinary material source through the native physical boundary.
/// The bounded hash is preliminary read evidence, not permission to mutate or
/// an exclusion of subsequent external activity. Publication still checks CAS.
pub fn material_basis(path: &Path) -> Result<String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return native::material_basis(path);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Err(AikitError::new(
        "knowledge.wiki_publication_metadata_unsupported",
        "native ordinary material basis is unavailable on this platform",
    )
    .with("path", path.display().to_string()))
}

/// Read an ordinary source through the same held physical observation used
/// for its basis. The limit is 1..=16 MiB; oversized sources are refused,
/// never truncated. Source identity and disclosure admission stay with the
/// caller, which must check its native boundary before and after this read.
pub fn material_bytes(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return native::material_bytes(path, max_bytes);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = max_bytes;
        Err(AikitError::new(
            "knowledge.wiki_publication_metadata_unsupported",
            "native ordinary material observation is unavailable on this platform",
        )
        .with("path", path.display().to_string()))
    }
}

/// Read a proven normal member of the physical root retained by its owner.
/// The admitted device/inode is continuity evidence, never source identity or
/// admission. The caller checks its original member mapping and native policy.
pub fn material_bytes_affiliated(
    requested_root: &Path,
    expected_root_identity: (u64, u64),
    member: &Path,
    max_bytes: u64,
) -> Result<Vec<u8>> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return native::material_bytes_affiliated(
        requested_root,
        expected_root_identity,
        member,
        max_bytes,
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (expected_root_identity, member, max_bytes);
        Err(AikitError::new(
            "knowledge.wiki_publication_metadata_unsupported",
            "retained native physical material observation is unavailable on this platform",
        )
        .with("path", requested_root.display().to_string()))
    }
}

/// Publish through the root actually admitted by a retained native binding.
/// Target opens and replacement derive from its held directory, never from a
/// newly selected root alias. This preserves the ordinary publication API.
pub fn publish_wiki_affiliated(
    requested_root: &Path,
    expected_root_identity: (u64, u64),
    member: &Path,
    rendered: &str,
    expected_basis: &str,
) -> Result<bool> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return native::publish_affiliated(
        requested_root,
        expected_root_identity,
        member,
        rendered,
        expected_basis,
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (expected_root_identity, member, rendered, expected_basis);
        Err(AikitError::new(
            "knowledge.wiki_publication_metadata_unsupported",
            "retained native metadata-preserving publication is unavailable on this platform",
        )
        .with("path", requested_root.display().to_string()))
    }
}

/// Publish an existing canonical source. All participating writers use the
/// persistent `.<name>.publication.lock`; old staging/lock files are never
/// promoted, overwritten or removed. `false` means exact-byte no-op.
pub fn publish_wiki(path: &Path, rendered: &str, expected_basis: &str) -> Result<bool> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return native::publish(path, rendered, expected_basis);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (rendered, expected_basis);
        Err(AikitError::new(
            "knowledge.wiki_publication_metadata_unsupported",
            "native metadata-preserving Wiki publication is unavailable on this platform",
        )
        .with("path", path.display().to_string()))
    }
}

/// Publish material whose caller observed an absent source. This is the same
/// physical seam, with atomic no-clobber absence instead of a content basis;
/// it does not confer Wiki semantics on a SourcePool caller.
pub fn publish_absent_material(path: &Path, rendered: &str) -> Result<bool> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return native::publish_absent(path, rendered);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = rendered;
        Err(AikitError::new(
            "knowledge.wiki_publication_metadata_unsupported",
            "atomic native absent-source publication is unavailable on this platform",
        )
        .with("path", path.display().to_string()))
    }
}

/// Remove only the exact material basis captured by its semantic owner. All
/// participating publication writers share the lock; arbitrary external
/// same-UID filesystem mutation is not excluded by an advisory protocol.
pub fn remove_material(path: &Path, expected_basis: &str) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    return native::remove(path, expected_basis);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = expected_basis;
        Err(AikitError::new(
            "knowledge.wiki_publication_metadata_unsupported",
            "native exact-basis material removal is unavailable on this platform",
        )
        .with("path", path.display().to_string()))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::CString;
    use std::fs::{self, File, Metadata, OpenOptions};
    use std::io::{Read, Seek, SeekFrom, Write};
    #[cfg(target_os = "macos")]
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use fs4::FileExt;
    use rustix::fs::{
        fchown, fgetxattr, flistxattr, openat, renameat, renameat_with, unlinkat, AtFlags, Gid,
        Mode, OFlags, RenameFlags, Uid,
    };
    #[cfg(target_os = "linux")]
    use rustix::fs::{fsetxattr, XattrFlags};

    const METADATA_BUDGET: usize = 256 * 1024;
    const SOURCE_BUDGET: u64 = 16 * 1024 * 1024;

    #[derive(Debug, PartialEq, Eq)]
    struct RetainedMetadata {
        uid: u32,
        gid: u32,
        mode: u32,
        attributes: BTreeMap<Vec<u8>, Vec<u8>>,
        acl: Vec<String>,
    }

    fn failure(code: &'static str, path: &Path, detail: impl std::fmt::Display) -> AikitError {
        AikitError::new(code, format!("{}: {detail}", path.display()))
            .with("path", path.display().to_string())
    }

    fn io_failure(code: &'static str, path: &Path, error: impl Into<std::io::Error>) -> AikitError {
        let error = error.into();
        failure(code, path, &error)
            .with("cause_kind", format!("{:?}", error.kind()))
            .with(
                "cause_raw_os_error",
                serde_json::json!(error.raw_os_error()).to_string(),
            )
            .with_io_source(error)
    }

    fn cause_detail(error: &AikitError) -> String {
        serde_json::json!({"code":error.code(), "message":error.message(), "details":error.details()}).to_string()
    }

    fn metadata_failure(path: &Path, detail: impl std::fmt::Display) -> AikitError {
        failure(
            "knowledge.wiki_publication_metadata_unsupported",
            path,
            detail,
        )
    }

    fn identity(metadata: &Metadata) -> (u64, u64) {
        (metadata.dev(), metadata.ino())
    }

    fn directory_at_path(path: &Path, directory: &File) -> Result<()> {
        let opened = directory
            .metadata()
            .map_err(|e| io_failure("knowledge.wiki_publication_identity", path, e))?;
        let named = fs::symlink_metadata(path)
            .map_err(|e| io_failure("knowledge.wiki_publication_identity", path, e))?;
        if !opened.is_dir() || !named.is_dir() || identity(&opened) != identity(&named) {
            return Err(failure(
                "knowledge.wiki_publication_identity",
                path,
                "native parent directory moved or was replaced; no other directory is a publication destination",
            ).with("held_directory_identity", format!("{}:{}", opened.dev(), opened.ino())));
        }
        Ok(())
    }

    fn publication_parents(path: &Path) -> Result<(PathBuf, PathBuf)> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let requested = if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|error| io_failure("knowledge.wiki_write_failed", path, error))?
                .join(parent)
        };
        let physical = fs::canonicalize(&requested)
            .map_err(|error| io_failure("knowledge.wiki_write_failed", path, error))?;
        Ok((requested, physical))
    }

    fn directory_affiliation(
        parent: &Path,
        requested_parent: &Path,
        directory: &File,
    ) -> Result<()> {
        directory_at_path(parent, directory)?;
        let opened = directory.metadata().map_err(|error| {
            io_failure(
                "knowledge.wiki_publication_identity",
                requested_parent,
                error,
            )
        })?;
        // Follow only the originally admitted parent relation, so an unchanged
        // directory alias remains useful. Source, lock and stage opens retain
        // their existing NOFOLLOW and ordinary single-link admission.
        let named = fs::metadata(requested_parent).map_err(|error| {
            io_failure(
                "knowledge.wiki_publication_identity",
                requested_parent,
                error,
            )
        })?;
        if !opened.is_dir() || !named.is_dir() || identity(&opened) != identity(&named) {
            return Err(failure(
                "knowledge.wiki_publication_identity",
                requested_parent,
                "requested native parent no longer resolves to the held physical directory",
            )
            .with(
                "held_directory_identity",
                format!("{}:{}", opened.dev(), opened.ino()),
            ));
        }
        directory_at_path(parent, directory)
    }

    fn open_directory(path: &Path) -> Result<File> {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags((OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32)
            .open(path)
            .map_err(|e| io_failure("knowledge.wiki_publication_identity", path, e))?;
        directory_at_path(path, &directory)?;
        Ok(directory)
    }

    struct OwnerAffiliation {
        requested_root: PathBuf,
        physical_root: PathBuf,
        root: File,
        parents: Vec<(PathBuf, File)>,
    }

    impl OwnerAffiliation {
        fn check(&self) -> Result<()> {
            directory_affiliation(&self.physical_root, &self.requested_root, &self.root)
                .map_err(|error| error.with("observation_stage", "owner_root"))?;
            for (path, parent) in &self.parents {
                directory_at_path(path, parent)
                    .map_err(|error| error.with("observation_stage", "owner_parent"))?;
            }
            directory_affiliation(&self.physical_root, &self.requested_root, &self.root)
                .map_err(|error| error.with("observation_stage", "owner_root"))
        }
    }

    struct PhysicalContext {
        requested_parent: PathBuf,
        parent: PathBuf,
        source_path: PathBuf,
        directory: File,
        owner: Option<OwnerAffiliation>,
    }

    impl PhysicalContext {
        fn independent(path: &Path, filename_failure: &str) -> Result<Self> {
            let (requested_parent, parent) = publication_parents(path)?;
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| metadata_failure(path, filename_failure))?;
            let source_path = parent.join(name);
            let directory = open_directory(&parent)?;
            directory_affiliation(&parent, &requested_parent, &directory)?;
            Ok(Self {
                requested_parent,
                parent,
                source_path,
                directory,
                owner: None,
            })
        }

        fn affiliated(requested_root: &Path, expected: (u64, u64), member: &Path) -> Result<Self> {
            use std::path::Component;
            if member.as_os_str().is_empty()
                || !member
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
            {
                return Err(failure(
                    "knowledge.wiki_publication_identity",
                    member,
                    "retained native member must be a nonempty normal relative path",
                )
                .with("observation_stage", "owner_parent"));
            }
            let name = member
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    metadata_failure(member, "publication requires a UTF-8 native filename")
                        .with("observation_stage", "owner_parent")
                })?;
            let requested_root = if requested_root.is_absolute() {
                requested_root.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|error| {
                        io_failure("knowledge.wiki_write_failed", requested_root, error)
                            .with("observation_stage", "owner_root")
                    })?
                    .join(requested_root)
            };
            let physical_root = fs::canonicalize(&requested_root).map_err(|error| {
                io_failure(
                    "knowledge.wiki_publication_identity",
                    &requested_root,
                    error,
                )
                .with("observation_stage", "owner_root")
            })?;
            let root = open_directory(&physical_root)
                .map_err(|error| error.with("observation_stage", "owner_root"))?;
            let opened = root.metadata().map_err(|error| {
                io_failure(
                    "knowledge.wiki_publication_identity",
                    &requested_root,
                    error,
                )
                .with("observation_stage", "owner_root")
            })?;
            if identity(&opened) != expected {
                return Err(failure(
                    "knowledge.wiki_publication_identity",
                    &requested_root,
                    "requested root does not retain the native owner's admitted physical directory",
                )
                .with("observation_stage", "owner_root")
                .with(
                    "expected_directory_identity",
                    format!("{}:{}", expected.0, expected.1),
                )
                .with(
                    "held_directory_identity",
                    format!("{}:{}", opened.dev(), opened.ino()),
                ));
            }
            let mut owner = OwnerAffiliation {
                requested_root,
                physical_root,
                root,
                parents: Vec::new(),
            };
            #[cfg(test)]
            tests::after_affiliated_root_open(&owner.requested_root);
            owner.check()?;
            let mut parent = owner.physical_root.clone();
            let mut directory = owner.root.try_clone().map_err(|error| {
                io_failure("knowledge.wiki_publication_identity", &parent, error)
                    .with("observation_stage", "owner_parent")
            })?;
            for component in member.parent().unwrap_or(Path::new("")).components() {
                let Component::Normal(component) = component else {
                    unreachable!("member validated above")
                };
                owner.check()?;
                parent.push(component);
                directory = openat(
                    &directory,
                    component,
                    OFlags::RDONLY
                        | OFlags::DIRECTORY
                        | OFlags::NOFOLLOW
                        | OFlags::NONBLOCK
                        | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map(File::from)
                .map_err(|error| {
                    io_failure("knowledge.wiki_publication_identity", &parent, error)
                        .with("observation_stage", "owner_parent")
                })?;
                directory_at_path(&parent, &directory)
                    .map_err(|error| error.with("observation_stage", "owner_parent"))?;
                let held = directory.try_clone().map_err(|error| {
                    io_failure("knowledge.wiki_publication_identity", &parent, error)
                        .with("observation_stage", "owner_parent")
                })?;
                owner.parents.push((parent.clone(), held));
            }
            owner.check()?;
            let source_path = parent.join(name);
            Ok(Self {
                requested_parent: parent.clone(),
                parent,
                source_path,
                directory,
                owner: Some(owner),
            })
        }
    }

    fn physical_affiliation(
        parent: &Path,
        requested_parent: &Path,
        directory: &File,
        owner: Option<&OwnerAffiliation>,
    ) -> Result<()> {
        if let Some(owner) = owner {
            owner.check()?;
        }
        directory_affiliation(parent, requested_parent, directory).map_err(|error| {
            if owner.is_some() {
                error.with("observation_stage", "owner_parent")
            } else {
                error
            }
        })?;
        if let Some(owner) = owner {
            owner.check()?;
        }
        Ok(())
    }

    fn ordinary(path: &Path, file: &File) -> Result<Metadata> {
        let opened = file
            .metadata()
            .map_err(|e| io_failure("knowledge.wiki_write_failed", path, e))?;
        let named = fs::symlink_metadata(path)
            .map_err(|e| io_failure("knowledge.wiki_concurrent_write", path, e))?;
        if !named.is_file()
            || named.file_type().is_symlink()
            || named.nlink() != 1
            || identity(&named) != identity(&opened)
        {
            return Err(failure(
                "knowledge.wiki_publication_identity",
                path,
                "publication requires the same ordinary single-link file at its native name",
            ));
        }
        Ok(opened)
    }

    fn open_existing_at(directory: &File, name: &std::ffi::OsStr, path: &Path) -> Result<File> {
        let named = fs::symlink_metadata(path)
            .map_err(|e| io_failure("knowledge.wiki_concurrent_write", path, e))?;
        if !named.is_file() || named.file_type().is_symlink() || named.nlink() != 1 {
            return Err(failure(
                "knowledge.wiki_publication_identity",
                path,
                "publication requires an ordinary single-link source",
            ));
        }
        #[cfg(test)]
        tests::before_source_open(path);
        openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|e| io_failure("knowledge.wiki_concurrent_write", path, e))
    }

    #[cfg(test)]
    fn open_existing(path: &Path) -> Result<File> {
        let parent = path.parent().unwrap_or(Path::new("."));
        open_existing_at(&open_directory(parent)?, path.file_name().unwrap(), path)
    }

    fn create_stage(
        directory: &File,
        parent: &Path,
        name: &str,
    ) -> Result<tempfile::NamedTempFile> {
        // TempPath's normal Drop removes a pathname without checking its inode.
        // A refused or interrupted stage is retained instead: conditional
        // check-then-unlink is itself racy. Only publication removes the stage
        // name, by rename relative to the directory we actually hold.
        tempfile::Builder::new()
            .prefix(&format!(".{name}.publication-"))
            .suffix(".tmp")
            .disable_cleanup(true)
            .make_in(parent, |path| {
                openat(
                    directory,
                    path.file_name().unwrap(),
                    OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::RDWR
                        | OFlags::NOFOLLOW
                        | OFlags::NONBLOCK
                        | OFlags::CLOEXEC,
                    Mode::RUSR | Mode::WUSR,
                )
                .map(File::from)
                .map_err(std::io::Error::from)
            })
            .map_err(|e| io_failure("knowledge.wiki_write_failed", parent, e))
    }

    fn read_source(file: &mut File, path: &Path) -> Result<Vec<u8>> {
        read_source_bounded(file, path, SOURCE_BUDGET)
    }

    fn read_source_bounded(file: &mut File, path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
        file.seek(SeekFrom::Start(0))
            .map_err(|e| io_failure("knowledge.wiki_write_failed", path, e))?;
        let mut bytes = Vec::new();
        file.take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| io_failure("knowledge.wiki_write_failed", path, e))?;
        if bytes.len() as u64 > max_bytes {
            return Err(failure(
                "knowledge.wiki_publication_budget",
                path,
                if max_bytes == SOURCE_BUDGET {
                    "source exceeds 16 MiB".to_owned()
                } else {
                    format!(
                        "source exceeds the declared {max_bytes}-byte material observation limit"
                    )
                },
            ));
        }
        Ok(bytes)
    }

    fn attributes(file: &File, path: &Path) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
        let mut names = vec![0_u8; METADATA_BUDGET];
        let count = flistxattr(file, names.as_mut_slice())
            .map_err(|e| io_failure("knowledge.wiki_publication_metadata_unsupported", path, e))?;
        let mut attributes = BTreeMap::new();
        let mut total = count;
        for name in names[..count].split(|b| *b == 0).filter(|s| !s.is_empty()) {
            let key = CString::new(name).map_err(|e| metadata_failure(path, e))?;
            let mut value = vec![0_u8; METADATA_BUDGET];
            let length = fgetxattr(file, key.as_c_str(), value.as_mut_slice()).map_err(|e| {
                io_failure("knowledge.wiki_publication_metadata_unsupported", path, e)
            })?;
            total += length;
            if total > METADATA_BUDGET || attributes.len() >= 128 {
                return Err(metadata_failure(
                    path,
                    "retained metadata exceeds the bounded publication budget",
                ));
            }
            value.truncate(length);
            attributes.insert(name.to_vec(), value);
        }
        Ok(attributes)
    }

    #[cfg(target_os = "macos")]
    fn acl(path: &Path) -> Result<Vec<String>> {
        use crate::runner::{CommandRunner, SystemRunner};
        let name = path.to_str().ok_or_else(|| {
            failure(
                "knowledge.wiki_publication_metadata_unsupported",
                path,
                "ACL observation requires a UTF-8 native path",
            )
        })?;
        let argv = vec!["/bin/ls".into(), "-lde".into(), name.into()];
        let output = SystemRunner::new()
            .with_timeout(Duration::from_secs(1))
            .run(&argv)?
            .require(&argv, "knowledge.wiki_publication_metadata_unsupported")?;
        Ok(output
            .stdout
            .lines()
            .skip(1)
            .filter_map(|line| {
                let line = line.trim();
                let (ordinal, _) = line.split_once(':')?;
                ordinal.parse::<usize>().ok().map(|_| line.to_string())
            })
            .collect())
    }

    #[cfg(target_os = "linux")]
    fn acl(_path: &Path) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn retained(file: &File, path: &Path) -> Result<RetainedMetadata> {
        let m = ordinary(path, file)?;
        Ok(RetainedMetadata {
            uid: m.uid(),
            gid: m.gid(),
            mode: m.mode() & 0o7777,
            attributes: attributes(file, path)?,
            acl: acl(path)?,
        })
    }

    #[cfg(target_os = "macos")]
    fn copy_metadata(source: &File, stage: &File, path: &Path) -> Result<()> {
        // std's safe macOS existing-destination copy retains ACL/stat/xattrs.
        // Descriptor aliases bind both opens to our held inodes: a renamed or
        // substituted stage path cannot redirect this material copy elsewhere.
        let from = PathBuf::from(format!("/dev/fd/{}", source.as_raw_fd()));
        let to = PathBuf::from(format!("/dev/fd/{}", stage.as_raw_fd()));
        fs::copy(&from, &to)
            .map_err(|e| io_failure("knowledge.wiki_publication_metadata_unsupported", path, e))?;
        Ok(())
    }

    fn recover_peer_lock_at(directory: &File, name: &std::ffi::OsStr, path: &Path) -> Result<File> {
        let parent = path.parent().unwrap_or(Path::new("."));
        directory_at_path(parent, directory)?;
        let peer = fs::symlink_metadata(path)
            .map_err(|error| io_failure("knowledge.wiki_publication_identity", path, error))?;
        if !peer.is_file() || peer.file_type().is_symlink() || peer.nlink() != 1 {
            return Err(failure(
                "knowledge.wiki_publication_identity",
                path,
                "bootstrap recovery requires an existing ordinary single-link peer lock",
            ));
        }
        #[cfg(test)]
        tests::after_peer_lock_observation(path);
        let file = openat(
            directory,
            name,
            OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|error| io_failure("knowledge.wiki_publication_identity", path, error))?;
        if identity(&ordinary(path, &file)?) != identity(&peer) {
            return Err(failure(
                "knowledge.wiki_publication_identity",
                path,
                "peer lock changed between bootstrap observation and reopening",
            ));
        }
        directory_at_path(parent, directory)?;
        Ok(file)
    }

    /// One admitted native publication lease ends at its original owner.
    /// A duplicate description may survive, but it cannot extend this lease.
    #[derive(Debug)]
    struct PublicationLock {
        file: File,
        owner_pid: u32,
    }

    impl std::ops::Deref for PublicationLock {
        type Target = File;

        fn deref(&self) -> &File {
            &self.file
        }
    }

    impl Drop for PublicationLock {
        fn drop(&mut self) {
            // A fork shares the description; its destructor does not own
            // releasing the parent's still-live publication lease.
            if self.owner_pid != std::process::id() {
                return;
            }
            if let Err(cause) = FileExt::unlock(&self.file) {
                eprintln!("release native publication lock: {cause}");
            }
        }
    }

    fn lock_at(directory: &File, name: &std::ffi::OsStr, path: &Path) -> Result<PublicationLock> {
        #[cfg(test)]
        tests::before_initial_lock_open(path);
        let file = match openat(
            directory,
            name,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(file) => File::from(file),
            Err(error) if error == rustix::io::Errno::NOENT => {
                // The initial CREATE returned actual ENOENT. Recover only
                // the actual peer lock already at this name, never another
                // creation or a whole-publication retry. The actual original
                // open error remains the cause if this bounded recovery fails.
                let initial = io_failure("knowledge.wiki_publication_identity", path, error);
                recover_peer_lock_at(directory, name, path).map_err(|recovery| {
                    initial
                        .with("lock_bootstrap_recovery", "refused")
                        .with("lock_bootstrap_recovery_cause", cause_detail(&recovery))
                })?
            }
            Err(error) => {
                return Err(io_failure(
                    "knowledge.wiki_publication_identity",
                    path,
                    error,
                ))
            }
        };
        ordinary(path, &file)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match FileExt::try_lock(&file) {
                Ok(()) => {
                    return Ok(PublicationLock {
                        file,
                        owner_pid: std::process::id(),
                    })
                }
                Err(fs4::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(fs4::TryLockError::WouldBlock) => {
                    return Err(failure(
                        "knowledge.wiki_publication_locked",
                        path,
                        "publication lock remained occupied for 5 seconds",
                    ));
                }
                Err(fs4::TryLockError::Error(e)) => {
                    return Err(io_failure("knowledge.wiki_write_failed", path, e));
                }
            }
        }
    }

    #[cfg(test)]
    fn lock(path: &Path) -> Result<PublicationLock> {
        let parent = path.parent().unwrap_or(Path::new("."));
        lock_at(&open_directory(parent)?, path.file_name().unwrap(), path)
    }

    pub(super) fn material_basis(path: &Path) -> Result<String> {
        Ok(content_hash(&material_bytes(path, SOURCE_BUDGET)?))
    }

    fn material_limit(path: &Path, max_bytes: u64) -> Result<()> {
        if max_bytes == 0 || max_bytes > SOURCE_BUDGET {
            return Err(failure(
                "knowledge.wiki_publication_budget",
                path,
                "material observation limit must be 1..=16 MiB",
            ));
        }
        Ok(())
    }

    pub(super) fn material_bytes(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
        material_limit(path, max_bytes)?;
        material_bytes_in_context(
            PhysicalContext::independent(path, "material basis requires a UTF-8 native filename")?,
            max_bytes,
        )
    }

    pub(super) fn material_bytes_affiliated(
        requested_root: &Path,
        expected: (u64, u64),
        member: &Path,
        max_bytes: u64,
    ) -> Result<Vec<u8>> {
        material_limit(&requested_root.join(member), max_bytes)?;
        material_bytes_in_context(
            PhysicalContext::affiliated(requested_root, expected, member)?,
            max_bytes,
        )
    }

    fn material_bytes_in_context(context: PhysicalContext, max_bytes: u64) -> Result<Vec<u8>> {
        let PhysicalContext {
            requested_parent,
            parent,
            source_path,
            directory,
            owner,
        } = context;
        let observation: Result<Vec<u8>> = (|| {
            physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
            let mut source =
                open_existing_at(&directory, source_path.file_name().unwrap(), &source_path)?;
            let source_identity = identity(&ordinary(&source_path, &source)?);
            let bytes = read_source_bounded(&mut source, &source_path, max_bytes)?;
            #[cfg(test)]
            tests::after_material_read(&source_path);
            physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
            if identity(&ordinary(&source_path, &source)?) != source_identity
                || read_source_bounded(&mut source, &source_path, max_bytes)? != bytes
            {
                return Err(failure(
                    "knowledge.wiki_concurrent_write",
                    &source_path,
                    "material source changed during its preliminary basis observation",
                ));
            }
            #[cfg(test)]
            tests::after_material_final_read(&source_path);
            physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
            ordinary(&source_path, &source)?;
            physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
            Ok(bytes)
        })();
        observation.map_err(|error| {
            // A final-source NotFound does not establish source absence when
            // the SAME held owner context has itself lost its named relation.
            // Preserve the first actual cause; report the separate observed
            // affiliation failure without rereading a body or guessing paths.
            if owner.is_some() {
                if let Err(affiliation) =
                    physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())
                {
                    let stage = affiliation
                        .details()
                        .get("observation_stage")
                        .map(String::as_str)
                        .unwrap_or("owner_parent");
                    return error
                        .with("observation_stage", stage)
                        .with("owner_affiliation_cause", cause_detail(&affiliation));
                }
            }
            error
        })
    }

    fn publication_readback(
        directory: &File,
        parent: &Path,
        requested_parent: &Path,
        source_path: &Path,
        stage_identity: (u64, u64),
        rendered_hash: &str,
        metadata: &RetainedMetadata,
    ) -> Result<()> {
        publication_readback_in_context(
            ReadbackContext {
                directory,
                parent,
                requested_parent,
                owner: None,
            },
            source_path,
            stage_identity,
            rendered_hash,
            metadata,
        )
    }

    struct ReadbackContext<'a> {
        directory: &'a File,
        parent: &'a Path,
        requested_parent: &'a Path,
        owner: Option<&'a OwnerAffiliation>,
    }

    fn publication_readback_in_context(
        context: ReadbackContext<'_>,
        source_path: &Path,
        stage_identity: (u64, u64),
        rendered_hash: &str,
        metadata: &RetainedMetadata,
    ) -> Result<()> {
        let ReadbackContext {
            directory,
            parent,
            requested_parent,
            owner,
        } = context;
        // Once rename succeeded, any failure is an uncertain Return of an
        // actual effect. Sync the held directory, never a reopened pathname.
        let checked: Result<()> =
            (|| {
                directory
                    .sync_all()
                    .map_err(|e| io_failure("knowledge.wiki_write_failed", parent, e))?;
                physical_affiliation(parent, requested_parent, directory, owner)?;
                let mut published =
                    open_existing_at(directory, source_path.file_name().unwrap(), source_path)?;
                if identity(&ordinary(source_path, &published)?) != stage_identity
                    || content_hash(&read_source(&mut published, source_path)?) != rendered_hash
                    || retained(&published, source_path)? != *metadata
                {
                    return Err(failure("knowledge.wiki_publication_identity", source_path,
                    "published source inode, bytes or retained metadata changed before readback"));
                }
                physical_affiliation(parent, requested_parent, directory, owner)?;
                ordinary(source_path, &published)?;
                physical_affiliation(parent, requested_parent, directory, owner)?;
                Ok(())
            })();
        checked.map_err(|cause| {
            failure("knowledge.wiki_publication_uncertain", source_path,
                format!("publication committed but its durable source readback was not confirmed: {cause}"))
                .with_io_source_from(&cause)
                .with("cause_code", cause.code())
                .with("cause", cause_detail(&cause))
                .with("published", "true")
                .with("published_hash", rendered_hash)
                .with("automatic_retry", "false")
                .with("instruction", "do not retry the mutation automatically; inspect the original native operation and source")
        })
    }

    pub(super) fn publish(path: &Path, rendered: &str, expected_basis: &str) -> Result<bool> {
        publication_limit(path, rendered)?;
        publish_in_context(
            PhysicalContext::independent(path, "publication requires a UTF-8 native filename")?,
            rendered,
            expected_basis,
        )
    }

    pub(super) fn publish_affiliated(
        requested_root: &Path,
        expected: (u64, u64),
        member: &Path,
        rendered: &str,
        expected_basis: &str,
    ) -> Result<bool> {
        publication_limit(&requested_root.join(member), rendered)?;
        publish_in_context(
            PhysicalContext::affiliated(requested_root, expected, member)?,
            rendered,
            expected_basis,
        )
    }

    fn publication_limit(path: &Path, rendered: &str) -> Result<()> {
        if rendered.len() as u64 > SOURCE_BUDGET {
            return Err(failure(
                "knowledge.wiki_publication_budget",
                path,
                "result exceeds 16 MiB",
            ));
        }
        Ok(())
    }

    fn publish_in_context(
        context: PhysicalContext,
        rendered: &str,
        expected_basis: &str,
    ) -> Result<bool> {
        let PhysicalContext {
            requested_parent,
            parent,
            source_path,
            directory,
            owner,
        } = context;
        let name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                metadata_failure(&source_path, "publication requires a UTF-8 native filename")
            })?;
        let lock_path = parent.join(format!(".{name}.publication.lock"));
        physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
        let lock = lock_at(&directory, lock_path.file_name().unwrap(), &lock_path)?;
        physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
        let mut source =
            open_existing_at(&directory, source_path.file_name().unwrap(), &source_path)?;
        let source_identity = identity(&ordinary(&source_path, &source)?);
        let before = read_source(&mut source, &source_path)?;
        if content_hash(&before) != expected_basis {
            return Err(failure(
                "knowledge.wiki_concurrent_write",
                &source_path,
                "source changed; re-read and re-apply the mutation",
            ));
        }
        if before == rendered.as_bytes() {
            #[cfg(test)]
            tests::before_mutation(&source_path);
            physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
            if identity(&ordinary(&source_path, &source)?) != source_identity
                || read_source(&mut source, &source_path)? != before
            {
                return Err(failure(
                    "knowledge.wiki_concurrent_write",
                    &source_path,
                    "source basis changed before exact-byte no-op acknowledgement",
                ));
            }
            #[cfg(test)]
            tests::after_noop_read(&source_path);
            let acknowledgement: Result<()> = (|| {
                ordinary(&source_path, &source)?;
                ordinary(&lock_path, &lock)?;
                physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())
            })();
            acknowledgement.map_err(|error| {
                if owner.is_some() {
                    error.with("changed", "false").with("published", "false")
                } else {
                    error
                }
            })?;
            return Ok(false);
        }
        // Replacing through a writable directory must not bypass source ACLs.
        // Opening without truncation establishes actual write access while the
        // held original inode remains the exact source basis.
        let writable = openat(
            &directory,
            source_path.file_name().unwrap(),
            OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|e| io_failure("knowledge.wiki_publication_write_denied", &source_path, e))?;
        if identity(&ordinary(&source_path, &writable)?) != source_identity {
            return Err(failure(
                "knowledge.wiki_concurrent_write",
                &source_path,
                "write authority opened a different source inode",
            ));
        }
        let metadata = retained(&source, &source_path)?;
        physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
        let mut stage = create_stage(&directory, &parent, name)?;
        let stage_path: PathBuf = stage.path().into();
        let stage_identity = identity(&stage.as_file().metadata().map_err(|e| {
            io_failure("knowledge.wiki_write_failed", &stage_path, e)
                .with("stage_path", stage_path.display().to_string())
                .with(
                    "failed_stage_policy",
                    "retain; never unlink a substituted pathname",
                )
        })?);
        let publication: Result<()> = (|| {
            #[cfg(test)]
            tests::before_mutation(&source_path);
            physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
            ordinary(&stage_path, stage.as_file())?;
            #[cfg(target_os = "macos")]
            copy_metadata(&source, stage.as_file(), &stage_path)?;
            if identity(&ordinary(&stage_path, stage.as_file())?) != stage_identity {
                return Err(failure(
                    "knowledge.wiki_publication_identity",
                    &stage_path,
                    "metadata copy replaced the owned stage",
                ));
            }
            let copied = stage
                .as_file()
                .metadata()
                .map_err(|e| io_failure("knowledge.wiki_write_failed", &stage_path, e))?;
            if copied.uid() != metadata.uid || copied.gid() != metadata.gid {
                fchown(
                    stage.as_file(),
                    Some(Uid::from_raw(metadata.uid)),
                    Some(Gid::from_raw(metadata.gid)),
                )
                .map_err(|e| {
                    io_failure(
                        "knowledge.wiki_publication_metadata_unsupported",
                        &stage_path,
                        e,
                    )
                })?;
            }
            #[cfg(target_os = "linux")]
            for (key, value) in &metadata.attributes {
                let key = CString::new(key.as_slice()).map_err(|e| {
                    failure(
                        "knowledge.wiki_publication_metadata_unsupported",
                        &stage_path,
                        e,
                    )
                })?;
                fsetxattr(stage.as_file(), key.as_c_str(), value, XattrFlags::empty()).map_err(
                    |e| {
                        io_failure(
                            "knowledge.wiki_publication_metadata_unsupported",
                            &stage_path,
                            e,
                        )
                    },
                )?;
            }
            stage
                .as_file_mut()
                .seek(SeekFrom::Start(0))
                .and_then(|_| stage.as_file_mut().set_len(0))
                .and_then(|_| stage.write_all(rendered.as_bytes()))
                .map_err(|e| io_failure("knowledge.wiki_write_failed", &stage_path, e))?;
            stage
                .as_file()
                .set_permissions(fs::Permissions::from_mode(metadata.mode))
                .and_then(|_| stage.as_file().sync_all())
                .map_err(|e| io_failure("knowledge.wiki_write_failed", &stage_path, e))?;
            if identity(&ordinary(&source_path, &source)?) != source_identity
                || content_hash(&read_source(&mut source, &source_path)?) != expected_basis
                || retained(&source, &source_path)? != metadata
            {
                return Err(failure(
                    "knowledge.wiki_concurrent_write",
                    &source_path,
                    "source basis or retained metadata changed during publication",
                ));
            }
            ordinary(&lock_path, &lock)?;
            if identity(&ordinary(&stage_path, stage.as_file())?) != stage_identity
                || retained(stage.as_file(), &stage_path)? != metadata
            {
                return Err(failure(
                    "knowledge.wiki_publication_metadata_unsupported",
                    &stage_path,
                    "stage did not retain source identity, ownership, permissions, ACL and attributes",
                ));
            }
            physical_affiliation(&parent, &requested_parent, &directory, owner.as_ref())?;
            renameat(
                &directory,
                stage_path.file_name().unwrap(),
                &directory,
                source_path.file_name().unwrap(),
            )
            .map_err(|e| io_failure("knowledge.wiki_write_failed", &source_path, e))?;
            #[cfg(test)]
            tests::after_mutation(&source_path);
            publication_readback_in_context(
                ReadbackContext {
                    directory: &directory,
                    parent: &parent,
                    requested_parent: &requested_parent,
                    owner: owner.as_ref(),
                },
                &source_path,
                stage_identity,
                &content_hash(rendered.as_bytes()),
                &metadata,
            )
        })();
        publication.map_err(|e| {
            e.with("stage_path", stage_path.display().to_string())
                .with(
                    "stage_identity",
                    format!("{}:{}", stage_identity.0, stage_identity.1),
                )
                .with(
                    "failed_stage_policy",
                    "retain; never unlink a pathname after its owned inode may have moved",
                )
        })?;
        Ok(true)
    }

    fn require_absent(path: &Path) -> Result<()> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_failure("knowledge.wiki_concurrent_write", path, error)),
            Ok(_) => Err(failure(
                "knowledge.wiki_concurrent_write",
                path,
                "absent publication basis changed; retained existing destination",
            )),
        }
    }

    fn rename_absent(directory: &File, stage: &Path, source: &Path) -> Result<()> {
        renameat_with(
            directory,
            stage.file_name().unwrap(),
            directory,
            source.file_name().unwrap(),
            RenameFlags::NOREPLACE,
        )
        .map_err(|error| {
            let unsupported = error == rustix::io::Errno::NOSYS
                || error == rustix::io::Errno::OPNOTSUPP
                || error == rustix::io::Errno::INVAL;
            io_failure(
                if unsupported {
                    "knowledge.wiki_publication_metadata_unsupported"
                } else {
                    "knowledge.wiki_write_failed"
                },
                source,
                error,
            )
        })
    }

    pub(super) fn publish_absent(path: &Path, rendered: &str) -> Result<bool> {
        if rendered.len() as u64 > SOURCE_BUDGET {
            return Err(failure(
                "knowledge.wiki_publication_budget",
                path,
                "result exceeds 16 MiB",
            ));
        }
        let (requested_parent, parent) = publication_parents(path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                metadata_failure(path, "publication requires a UTF-8 native filename")
            })?;
        let source_path = parent.join(name);
        let lock_path = parent.join(format!(".{name}.publication.lock"));
        let directory = open_directory(&parent)?;
        directory_affiliation(&parent, &requested_parent, &directory)?;
        let lock = lock_at(&directory, lock_path.file_name().unwrap(), &lock_path)?;
        directory_affiliation(&parent, &requested_parent, &directory)?;
        require_absent(&source_path)?;
        let mut stage = create_stage(&directory, &parent, name)?;
        let stage_path: PathBuf = stage.path().into();
        let stage_identity =
            identity(&ordinary(&stage_path, stage.as_file()).map_err(|error| {
                error
                    .with("stage_path", stage_path.display().to_string())
                    .with(
                        "failed_stage_policy",
                        "retain; never unlink a substituted pathname",
                    )
            })?);
        // Admit actual create-new defaults before writing a candidate. No
        // later observer/copy can silently redefine the privacy expectation.
        let metadata = retained(stage.as_file(), &stage_path).map_err(|error| {
            error
                .with("stage_path", stage_path.display().to_string())
                .with(
                    "failed_stage_policy",
                    "retain; bootstrap metadata admission was not completed",
                )
        })?;
        let publication: Result<()> = (|| {
            #[cfg(test)]
            tests::before_mutation(&source_path);
            directory_affiliation(&parent, &requested_parent, &directory)?;
            if metadata.mode & 0o7077 != 0
                || !metadata.acl.is_empty()
                || metadata
                    .attributes
                    .contains_key(b"system.posix_acl_access".as_slice())
            {
                return Err(metadata_failure(&stage_path,
                    "bootstrap source requires private create-new mode with no additional inherited ACL grants"));
            }
            stage
                .write_all(rendered.as_bytes())
                .and_then(|_| stage.as_file().sync_all())
                .map_err(|error| io_failure("knowledge.wiki_write_failed", &stage_path, error))?;
            if identity(&ordinary(&stage_path, stage.as_file())?) != stage_identity
                || retained(stage.as_file(), &stage_path)? != metadata
            {
                return Err(metadata_failure(
                    &stage_path,
                    "bootstrap metadata changed after its privacy admission",
                ));
            }
            ordinary(&lock_path, &lock)?;
            directory_affiliation(&parent, &requested_parent, &directory)?;
            require_absent(&source_path)?;
            directory_affiliation(&parent, &requested_parent, &directory)?;
            // No link/unlink interval: a crash cannot leave a canonical
            // two-link source that every subsequent native writer rejects.
            rename_absent(&directory, &stage_path, &source_path)?;
            #[cfg(test)]
            tests::after_mutation(&source_path);
            publication_readback(
                &directory,
                &parent,
                &requested_parent,
                &source_path,
                stage_identity,
                &content_hash(rendered.as_bytes()),
                &metadata,
            )
        })();
        publication.map_err(|error| {
            error
                .with("stage_path", stage_path.display().to_string())
                .with(
                    "stage_identity",
                    format!("{}:{}", stage_identity.0, stage_identity.1),
                )
                .with(
                    "failed_stage_policy",
                    "retain; never unlink a pathname after its owned inode may have moved",
                )
        })?;
        Ok(true)
    }

    pub(super) fn remove(path: &Path, expected_basis: &str) -> Result<()> {
        let (requested_parent, parent) = publication_parents(path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| metadata_failure(path, "removal requires a UTF-8 native filename"))?;
        let source_path = parent.join(name);
        let lock_path = parent.join(format!(".{name}.publication.lock"));
        let directory = open_directory(&parent)?;
        directory_affiliation(&parent, &requested_parent, &directory)?;
        let lock = lock_at(&directory, lock_path.file_name().unwrap(), &lock_path)?;
        directory_affiliation(&parent, &requested_parent, &directory)?;
        let mut source =
            open_existing_at(&directory, source_path.file_name().unwrap(), &source_path)?;
        let source_identity = identity(&ordinary(&source_path, &source)?);
        if content_hash(&read_source(&mut source, &source_path)?) != expected_basis {
            return Err(failure(
                "knowledge.wiki_concurrent_write",
                &source_path,
                "stale material basis changed; retained the peer's source",
            ));
        }
        let writable = openat(
            &directory,
            source_path.file_name().unwrap(),
            OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|error| {
            io_failure(
                "knowledge.wiki_publication_write_denied",
                &source_path,
                error,
            )
        })?;
        if identity(&ordinary(&source_path, &writable)?) != source_identity {
            return Err(failure(
                "knowledge.wiki_concurrent_write",
                &source_path,
                "removal authority opened a different source inode",
            ));
        }
        #[cfg(test)]
        tests::before_mutation(&source_path);
        ordinary(&lock_path, &lock)?;
        directory_affiliation(&parent, &requested_parent, &directory)?;
        ordinary(&source_path, &source)?;
        if content_hash(&read_source(&mut source, &source_path)?) != expected_basis {
            return Err(failure(
                "knowledge.wiki_concurrent_write",
                &source_path,
                "material basis changed during removal admission",
            ));
        }
        directory_affiliation(&parent, &requested_parent, &directory)?;
        unlinkat(
            &directory,
            source_path.file_name().unwrap(),
            AtFlags::empty(),
        )
        .map_err(|error| io_failure("knowledge.wiki_write_failed", &source_path, error))?;
        #[cfg(test)]
        tests::after_mutation(&source_path);
        removal_readback(
            &directory,
            &parent,
            &requested_parent,
            &source_path,
            expected_basis,
        )
    }

    fn removal_readback(
        directory: &File,
        parent: &Path,
        requested_parent: &Path,
        source_path: &Path,
        expected_basis: &str,
    ) -> Result<()> {
        let readback: Result<()> = (|| {
            directory
                .sync_all()
                .map_err(|error| io_failure("knowledge.wiki_write_failed", parent, error))?;
            directory_affiliation(parent, requested_parent, directory)?;
            require_absent(source_path)?;
            directory_affiliation(parent, requested_parent, directory)
        })();
        readback.map_err(|cause| {
            failure(
                "knowledge.source_pool_removal_uncertain",
                source_path,
                "material was removed but its durable absence was not confirmed",
            )
            .with_io_source_from(&cause)
            .with("cause_code", cause.code())
            .with("cause", cause_detail(&cause))
            .with("removed", "true")
            .with("removed_basis", expected_basis)
            .with("automatic_retry", "false")
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::{Arc, Barrier};

        type PathObserver = Box<dyn FnOnce(&Path)>;

        std::thread_local! {
            static AFTER_AFFILIATED_ROOT_OPEN: std::cell::RefCell<Option<PathObserver>> =
                const { std::cell::RefCell::new(None) };
            static BEFORE_INITIAL_LOCK_OPEN: std::cell::RefCell<Option<PathObserver>> =
                const { std::cell::RefCell::new(None) };
            static AFTER_PEER_LOCK_OBSERVATION: std::cell::RefCell<Option<PathObserver>> =
                const { std::cell::RefCell::new(None) };
            static BEFORE_MUTATION: std::cell::RefCell<Option<PathObserver>> =
                const { std::cell::RefCell::new(None) };
            static AFTER_MUTATION: std::cell::RefCell<Option<PathObserver>> =
                const { std::cell::RefCell::new(None) };
            static BEFORE_SOURCE_OPEN: std::cell::RefCell<Option<PathObserver>> =
                const { std::cell::RefCell::new(None) };
            static AFTER_MATERIAL_READ: std::cell::RefCell<Option<PathObserver>> =
                const { std::cell::RefCell::new(None) };
            static AFTER_MATERIAL_FINAL_READ: std::cell::RefCell<Option<PathObserver>> =
                const { std::cell::RefCell::new(None) };
            static AFTER_NOOP_READ: std::cell::RefCell<Option<PathObserver>> =
                const { std::cell::RefCell::new(None) };
        }

        pub(super) fn after_affiliated_root_open(path: &Path) {
            let observer = AFTER_AFFILIATED_ROOT_OPEN.with(|slot| slot.borrow_mut().take());
            if let Some(observer) = observer {
                observer(path);
            }
        }

        pub(super) fn before_initial_lock_open(path: &Path) {
            let observer = BEFORE_INITIAL_LOCK_OPEN.with(|slot| slot.borrow_mut().take());
            if let Some(observer) = observer {
                observer(path);
            }
        }

        pub(super) fn after_peer_lock_observation(path: &Path) {
            let observer = AFTER_PEER_LOCK_OBSERVATION.with(|slot| slot.borrow_mut().take());
            if let Some(observer) = observer {
                observer(path);
            }
        }

        pub(super) fn after_noop_read(path: &Path) {
            let observer = AFTER_NOOP_READ.with(|slot| slot.borrow_mut().take());
            if let Some(observer) = observer {
                observer(path);
            }
        }

        pub(super) fn before_source_open(path: &Path) {
            let observer = BEFORE_SOURCE_OPEN.with(|slot| slot.borrow_mut().take());
            if let Some(observer) = observer {
                observer(path);
            }
        }

        pub(super) fn after_material_read(path: &Path) {
            let observer = AFTER_MATERIAL_READ.with(|slot| slot.borrow_mut().take());
            if let Some(observer) = observer {
                observer(path);
            }
        }

        pub(super) fn after_material_final_read(path: &Path) {
            let observer = AFTER_MATERIAL_FINAL_READ.with(|slot| slot.borrow_mut().take());
            if let Some(observer) = observer {
                observer(path);
            }
        }

        pub(super) fn before_mutation(path: &Path) {
            let observer = BEFORE_MUTATION.with(|slot| slot.borrow_mut().take());
            if let Some(observer) = observer {
                observer(path);
            }
        }

        pub(super) fn after_mutation(path: &Path) {
            let observer = AFTER_MUTATION.with(|slot| slot.borrow_mut().take());
            if let Some(observer) = observer {
                observer(path);
            }
        }

        fn material_fixture(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
            let scratch =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
            fs::create_dir_all(&scratch).unwrap();
            let root = tempfile::Builder::new()
                .prefix("material-basis-")
                .tempdir_in(&scratch)
                .unwrap();
            let path = root.path().join("material.json");
            fs::write(&path, bytes).unwrap();
            (root, path)
        }

        #[test]
        fn actual_material_bytes_readonly_alias_and_basis_share_exact_observation() {
            let (root, path) = material_fixture(b"ordinary readonly body");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
            let before = fs::metadata(&path).unwrap();
            let alias = root.path().join("admitted-parent-alias");
            std::os::unix::fs::symlink(root.path(), &alias).unwrap();
            for request in [&path, &alias.join("material.json")] {
                let bytes = super::super::material_bytes(request, 4 * 1024 * 1024).unwrap();
                assert_eq!(bytes, b"ordinary readonly body");
                assert_eq!(
                    super::super::material_basis(request).unwrap(),
                    content_hash(&bytes)
                );
            }
            let after = fs::metadata(&path).unwrap();
            assert_eq!(
                (
                    before.dev(),
                    before.ino(),
                    before.mode(),
                    before.mtime(),
                    before.mtime_nsec()
                ),
                (
                    after.dev(),
                    after.ino(),
                    after.mode(),
                    after.mtime(),
                    after.mtime_nsec()
                )
            );
            assert!(!root.path().join(".material.json.publication.lock").exists());
        }

        #[test]
        fn actual_material_bytes_missing_source_retains_original_typed_os_cause() {
            use std::error::Error;
            let (root, path) = material_fixture(b"old body");
            fs::remove_file(&path).unwrap();
            let error = super::super::material_bytes(&path, 4 * 1024 * 1024).unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_concurrent_write");
            let cause = error
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap();
            assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
            assert_eq!(
                serde_json::json!(cause.raw_os_error()).to_string(),
                error.details()["cause_raw_os_error"]
            );
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        }

        #[test]
        fn actual_material_bytes_refuses_declared_limit_without_truncating_or_changing_basis() {
            let (_root, path) = material_fixture(b"0123456789");
            for budget in [0, 9, SOURCE_BUDGET + 1] {
                let error = super::super::material_bytes(&path, budget).unwrap_err();
                assert_eq!(error.code(), "knowledge.wiki_publication_budget");
                assert!(!error.details().contains_key("published"));
            }
            assert_eq!(
                super::super::material_bytes(&path, 10).unwrap(),
                b"0123456789"
            );
            let expanded = File::options().write(true).open(&path).unwrap();
            expanded.set_len(4 * 1024 * 1024 + 1).unwrap();
            assert_eq!(
                super::super::material_bytes(&path, 4 * 1024 * 1024)
                    .unwrap_err()
                    .code(),
                "knowledge.wiki_publication_budget"
            );
            assert!(
                super::super::material_basis(&path).is_ok(),
                "existing16MiB basis is preserved"
            );
            assert_eq!(fs::metadata(&path).unwrap().len(), 4 * 1024 * 1024 + 1);
        }

        #[test]
        fn actual_material_bytes_detects_same_inode_rewrite_between_held_reads() {
            let (_root, path) = material_fixture(b"initial ordinary body");
            let initial_identity = identity(&fs::metadata(&path).unwrap());
            AFTER_MATERIAL_READ.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(|path| {
                    fs::write(path, b"changed ordinary body").unwrap();
                }));
            });
            let error = super::super::material_bytes(&path, 4096).unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_concurrent_write");
            assert_eq!(identity(&fs::metadata(&path).unwrap()), initial_identity);
            assert_eq!(fs::read(&path).unwrap(), b"changed ordinary body");
        }

        #[test]
        fn actual_material_bytes_refuses_source_name_substitution_after_final_held_read() {
            let (root, path) = material_fixture(b"same body new identity");
            let retained = root.path().join("retained-original");
            let retained_move = retained.clone();
            let initial_identity = identity(&fs::metadata(&path).unwrap());
            AFTER_MATERIAL_FINAL_READ.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |path| {
                    fs::rename(path, &retained_move).unwrap();
                    fs::write(path, b"same body new identity").unwrap();
                }));
            });
            let error = super::super::material_bytes(&path, 4096).unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_publication_identity");
            assert_eq!(
                identity(&fs::metadata(&retained).unwrap()),
                initial_identity
            );
            assert_ne!(identity(&fs::metadata(&path).unwrap()), initial_identity);
            assert_eq!(fs::read(&retained).unwrap(), b"same body new identity");
            assert_eq!(fs::read(&path).unwrap(), b"same body new identity");
        }

        #[test]
        fn actual_material_bytes_substituted_fifo_refuses_without_a_writer() {
            use std::os::unix::fs::FileTypeExt;
            let (root, path) = material_fixture(b"selected ordinary body");
            let retained = root.path().join("retained-original");
            let retained_move = retained.clone();
            let request = path.clone();
            let (sent, received) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                BEFORE_SOURCE_OPEN.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move |path| {
                        fs::rename(path, &retained_move).unwrap();
                        let output = std::process::Command::new("mkfifo")
                            .arg("-m")
                            .arg("600")
                            .arg(path)
                            .output()
                            .unwrap();
                        assert!(
                            output.status.success(),
                            "{}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }));
                });
                sent.send(super::super::material_bytes(&request, 4096))
                    .unwrap();
            });
            let error = received
                .recv_timeout(Duration::from_secs(3))
                .expect("actual FIFO observation must not wait for a writer")
                .unwrap_err();
            worker.join().unwrap();
            assert_eq!(error.code(), "knowledge.wiki_publication_identity");
            assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
            assert_eq!(fs::read(&retained).unwrap(), b"selected ordinary body");
            assert!(!root.path().join(".material.json.publication.lock").exists());
        }

        #[test]
        fn actual_noop_refuses_same_inode_rewrite_after_initial_basis_read() {
            let (_root, path) = material_fixture(b"original admitted bytes");
            let before = fs::metadata(&path).unwrap();
            BEFORE_MUTATION.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(|path| {
                    fs::write(path, b"actual external same-inode rewrite").unwrap();
                }));
            });
            let error = publish_wiki(
                &path,
                "original admitted bytes",
                &content_hash(b"original admitted bytes"),
            )
            .unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_concurrent_write");
            assert!(!error.details().contains_key("published"));
            assert!(!error.details().contains_key("removed"));
            assert_eq!(identity(&fs::metadata(&path).unwrap()), identity(&before));
            assert_eq!(
                fs::read(&path).unwrap(),
                b"actual external same-inode rewrite"
            );
        }

        #[test]
        fn actual_noop_refuses_named_source_replacement_after_final_held_read() {
            let (root, path) = material_fixture(b"original admitted bytes");
            let before = fs::metadata(&path).unwrap();
            let retained = root.path().join("retained-original");
            let retained_move = retained.clone();
            AFTER_NOOP_READ.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |path| {
                    fs::rename(path, &retained_move).unwrap();
                    // Identical bytes cannot substitute for native source identity.
                    fs::write(path, b"original admitted bytes").unwrap();
                }));
            });
            let error = publish_wiki(
                &path,
                "original admitted bytes",
                &content_hash(b"original admitted bytes"),
            )
            .unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_publication_identity");
            assert!(!error.details().contains_key("published"));
            assert!(!error.details().contains_key("removed"));
            assert_eq!(
                identity(&fs::metadata(&retained).unwrap()),
                identity(&before)
            );
            assert_ne!(identity(&fs::metadata(&path).unwrap()), identity(&before));
            assert_eq!(fs::read(&retained).unwrap(), b"original admitted bytes");
            assert_eq!(fs::read(&path).unwrap(), b"original admitted bytes");
        }

        #[test]
        fn actual_material_basis_reads_ordinary_readonly_source_and_unchanged_parent_alias() {
            let (root, path) = material_fixture(b"observed ordinary material");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
            let before = fs::metadata(&path).unwrap();
            let alias = root.path().join("parent-alias");
            std::os::unix::fs::symlink(root.path(), &alias).unwrap();
            let alias_request = alias.join("material.json");
            for request in [&path, &alias_request] {
                assert_eq!(
                    super::super::material_basis(request).unwrap(),
                    content_hash(b"observed ordinary material")
                );
            }
            let after = fs::metadata(&path).unwrap();
            assert_eq!(identity(&before), identity(&after));
            assert_eq!(
                (before.mode(), before.mtime(), before.mtime_nsec()),
                (after.mode(), after.mtime(), after.mtime_nsec())
            );
            assert!(!root.path().join(".material.json.publication.lock").exists());
        }

        #[test]
        fn actual_material_basis_missing_source_keeps_original_os_cause_and_has_no_effect() {
            let (root, path) = material_fixture(b"original material");
            fs::remove_file(&path).unwrap();
            let error = super::super::material_basis(&path).unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_concurrent_write");
            assert_eq!(error.details()["cause_kind"], "NotFound");
            assert_ne!(error.details()["cause_raw_os_error"], "null");
            assert!(!error.details().contains_key("published"));
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        }

        #[test]
        fn actual_material_basis_refuses_final_symlink_and_hardlink_without_reading_foreign_identity(
        ) {
            let (root, path) = material_fixture(b"retained selected source");
            let foreign = root.path().join("foreign-private-source");
            fs::write(&foreign, b"unselected private bytes").unwrap();
            let addressed = root.path().join("selected-alias.json");
            std::os::unix::fs::symlink(&foreign, &addressed).unwrap();
            assert_eq!(
                super::super::material_basis(&addressed).unwrap_err().code(),
                "knowledge.wiki_publication_identity"
            );
            fs::hard_link(&path, root.path().join("retained-hardlink.json")).unwrap();
            assert_eq!(
                super::super::material_basis(&path).unwrap_err().code(),
                "knowledge.wiki_publication_identity"
            );
            assert_eq!(fs::read(&foreign).unwrap(), b"unselected private bytes");
            assert_eq!(fs::read(&path).unwrap(), b"retained selected source");
            assert_eq!(
                fs::read(root.path().join("retained-hardlink.json")).unwrap(),
                b"retained selected source"
            );
            assert!(!root.path().join(".material.json.publication.lock").exists());
        }

        #[test]
        fn actual_material_basis_open_refuses_substituted_symlink_after_metadata_admission() {
            let (root, path) = material_fixture(b"selected original bytes");
            let retained = root.path().join("retained-original");
            let foreign = root.path().join("foreign-private");
            fs::write(&foreign, b"unselected private bytes").unwrap();
            let retained_move = retained.clone();
            let foreign_link = foreign.clone();
            BEFORE_SOURCE_OPEN.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |path| {
                    fs::rename(path, &retained_move).unwrap();
                    std::os::unix::fs::symlink(&foreign_link, path).unwrap();
                }));
            });
            let error = super::super::material_basis(&path).unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_concurrent_write");
            assert_ne!(error.details()["cause_raw_os_error"], "null");
            assert_eq!(fs::read(&retained).unwrap(), b"selected original bytes");
            assert_eq!(fs::read(&foreign).unwrap(), b"unselected private bytes");
            assert!(fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink());
            assert!(!root.path().join(".material.json.publication.lock").exists());
        }

        #[test]
        fn actual_material_basis_open_refuses_substituted_fifo_without_waiting_for_a_writer() {
            use std::os::unix::fs::FileTypeExt;
            let (root, path) = material_fixture(b"selected original bytes");
            let retained = root.path().join("retained-original");
            let request = path.clone();
            let retained_move = retained.clone();
            let (sent, received) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                BEFORE_SOURCE_OPEN.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move |path| {
                        fs::rename(path, &retained_move).unwrap();
                        let created = std::process::Command::new("mkfifo")
                            .arg("-m")
                            .arg("600")
                            .arg(path)
                            .output()
                            .unwrap();
                        assert!(
                            created.status.success(),
                            "{}",
                            String::from_utf8_lossy(&created.stderr)
                        );
                    }));
                });
                sent.send(super::super::material_basis(&request)).unwrap();
            });
            let error = received
                .recv_timeout(Duration::from_secs(3))
                .expect("actual FIFO open must not wait for a writer")
                .unwrap_err();
            worker.join().unwrap();
            assert_eq!(error.code(), "knowledge.wiki_publication_identity");
            assert_eq!(fs::read(&retained).unwrap(), b"selected original bytes");
            assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
            assert!(!root.path().join(".material.json.publication.lock").exists());
        }

        #[test]
        fn actual_material_basis_refuses_same_inode_rewrite_during_observation() {
            let (_root, path) = material_fixture(b"initial observed bytes");
            let before = fs::metadata(&path).unwrap();
            AFTER_MATERIAL_READ.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(|path| {
                    fs::write(path, b"later external in-place bytes").unwrap();
                }));
            });
            assert_eq!(
                super::super::material_basis(&path).unwrap_err().code(),
                "knowledge.wiki_concurrent_write"
            );
            assert_eq!(identity(&fs::metadata(&path).unwrap()), identity(&before));
            assert_eq!(fs::read(&path).unwrap(), b"later external in-place bytes");
        }

        #[test]
        fn actual_material_basis_refuses_requested_parent_alias_retarget_during_observation() {
            let (root, path) = material_fixture(b"initial selected source");
            let alias = root.path().join("parent-alias");
            let foreign = root.path().join("foreign-parent");
            fs::create_dir(&foreign).unwrap();
            fs::write(foreign.join("material.json"), b"unselected foreign source").unwrap();
            std::os::unix::fs::symlink(root.path(), &alias).unwrap();
            let requested = alias.join("material.json");
            let alias_move = alias.clone();
            let foreign_move = foreign.clone();
            AFTER_MATERIAL_READ.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |_| {
                    fs::remove_file(&alias_move).unwrap();
                    std::os::unix::fs::symlink(&foreign_move, &alias_move).unwrap();
                }));
            });
            let error = super::super::material_basis(&requested).unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_publication_identity");
            assert!(!error.details().contains_key("published"));
            assert_eq!(fs::read(&path).unwrap(), b"initial selected source");
            assert_eq!(
                fs::read(foreign.join("material.json")).unwrap(),
                b"unselected foreign source"
            );
        }

        #[test]
        fn actual_material_basis_refuses_source_above_existing_native_budget() {
            let (_root, path) = material_fixture(&vec![b'x'; SOURCE_BUDGET as usize + 1]);
            assert_eq!(
                super::super::material_basis(&path).unwrap_err().code(),
                "knowledge.wiki_publication_budget"
            );
            assert_eq!(fs::metadata(&path).unwrap().len(), SOURCE_BUDGET + 1);
        }

        #[derive(Clone, Copy, Debug)]
        enum MaterialRoute {
            Replace,
            Create,
            Remove,
        }
        impl MaterialRoute {
            fn invoke(self, path: &Path) -> Result<()> {
                match self {
                    Self::Replace => publish_wiki(
                        path,
                        "actual next material",
                        &content_hash(b"actual old material"),
                    )
                    .map(|changed| assert!(changed)),
                    Self::Create => publish_absent_material(path, "actual next material")
                        .map(|changed| assert!(changed)),
                    Self::Remove => remove_material(path, &content_hash(b"actual old material")),
                }
            }
        }

        struct AliasFixture {
            _world: tempfile::TempDir,
            physical: PathBuf,
            foreign: PathBuf,
            alias: PathBuf,
        }
        impl AliasFixture {
            fn new(route: MaterialRoute) -> Self {
                let scratch =
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
                fs::create_dir_all(&scratch).unwrap();
                let world = tempfile::Builder::new()
                    .prefix("native-parent-affiliation-")
                    .tempdir_in(&scratch)
                    .unwrap();
                let physical = world.path().join("physical");
                let foreign = world.path().join("foreign");
                let alias = world.path().join("requested-parent");
                fs::create_dir(&physical).unwrap();
                fs::create_dir(&foreign).unwrap();
                if !matches!(route, MaterialRoute::Create) {
                    fs::write(physical.join("wiki.json"), "actual old material").unwrap();
                    fs::set_permissions(
                        physical.join("wiki.json"),
                        fs::Permissions::from_mode(0o600),
                    )
                    .unwrap();
                }
                fs::write(foreign.join("wiki.json"), "foreign selected source").unwrap();
                std::os::unix::fs::symlink(&physical, &alias).unwrap();
                Self {
                    _world: world,
                    physical,
                    foreign,
                    alias,
                }
            }
            fn request(&self) -> PathBuf {
                self.alias.join("wiki.json")
            }
            fn retarget_observer(&self) -> Box<dyn FnOnce(&Path)> {
                let alias = self.alias.clone();
                let foreign = self.foreign.clone();
                Box::new(move |_| {
                    fs::remove_file(&alias).unwrap();
                    std::os::unix::fs::symlink(&foreign, &alias).unwrap();
                })
            }
            fn check_foreign(&self) {
                assert_eq!(
                    fs::read_to_string(self.foreign.join("wiki.json")).unwrap(),
                    "foreign selected source"
                );
            }
            fn check_effect(&self, route: MaterialRoute) {
                let source = self.physical.join("wiki.json");
                if matches!(route, MaterialRoute::Remove) {
                    assert!(!source.exists(), "the actual removal must not be undone");
                } else {
                    assert_eq!(fs::read_to_string(&source).unwrap(), "actual next material");
                    assert_eq!(fs::metadata(&source).unwrap().mode() & 0o7777, 0o600);
                    assert_eq!(fs::metadata(&source).unwrap().nlink(), 1);
                }
            }
        }

        fn assert_uncertain_effect(error: &AikitError, route: MaterialRoute, physical: &Path) {
            assert_eq!(
                error.details()["path"],
                fs::canonicalize(physical)
                    .unwrap()
                    .join("wiki.json")
                    .display()
                    .to_string()
            );
            assert_eq!(error.details()["automatic_retry"], "false");
            assert_eq!(
                error.details()["cause_code"],
                "knowledge.wiki_publication_identity"
            );
            if matches!(route, MaterialRoute::Remove) {
                assert_eq!(error.code(), "knowledge.source_pool_removal_uncertain");
                assert_eq!(error.details()["removed"], "true");
                assert_eq!(
                    error.details()["removed_basis"],
                    content_hash(b"actual old material")
                );
                assert!(!error.details().contains_key("published"));
            } else {
                assert_eq!(error.code(), "knowledge.wiki_publication_uncertain");
                assert_eq!(error.details()["published"], "true");
                assert_eq!(
                    error.details()["published_hash"],
                    content_hash(b"actual next material")
                );
                assert!(!error.details().contains_key("removed"));
            }
        }

        #[test]
        fn actual_requested_parent_alias_retarget_refuses_all_routes_before_effect() {
            for route in [
                MaterialRoute::Replace,
                MaterialRoute::Create,
                MaterialRoute::Remove,
            ] {
                let world = AliasFixture::new(route);
                BEFORE_MUTATION.with(|slot| {
                    *slot.borrow_mut() = Some(world.retarget_observer());
                });
                let error = route.invoke(&world.request()).unwrap_err();
                assert_eq!(
                    error.code(),
                    "knowledge.wiki_publication_identity",
                    "{route:?}: {error}"
                );
                assert!(!error.details().contains_key("published"));
                assert!(!error.details().contains_key("removed"));
                let original = world.physical.join("wiki.json");
                if matches!(route, MaterialRoute::Create) {
                    assert!(!original.exists());
                } else {
                    assert_eq!(fs::read_to_string(original).unwrap(), "actual old material");
                }
                world.check_foreign();
                assert_eq!(
                    fs::read_to_string(world.request()).unwrap(),
                    "foreign selected source"
                );
                let stages: Vec<_> = fs::read_dir(&world.physical)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| {
                        path.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".wiki.json.publication-")
                    })
                    .collect();
                assert_eq!(
                    stages.len(),
                    if matches!(route, MaterialRoute::Remove) {
                        0
                    } else {
                        1
                    }
                );
                for stage in stages {
                    assert_eq!(
                        fs::read(stage).unwrap(),
                        b"",
                        "refuse before candidate bytes"
                    );
                }
            }
        }

        #[test]
        fn actual_requested_parent_alias_retarget_after_effect_retains_all_native_outcomes() {
            for route in [
                MaterialRoute::Replace,
                MaterialRoute::Create,
                MaterialRoute::Remove,
            ] {
                let world = AliasFixture::new(route);
                AFTER_MUTATION.with(|slot| {
                    *slot.borrow_mut() = Some(world.retarget_observer());
                });
                let error = route.invoke(&world.request()).unwrap_err();
                assert_uncertain_effect(&error, route, &world.physical);
                let cause: serde_json::Value =
                    serde_json::from_str(&error.details()["cause"]).unwrap();
                assert_eq!(cause["code"], "knowledge.wiki_publication_identity");
                assert_eq!(cause["details"]["path"], world.alias.display().to_string());
                assert!(cause["details"].get("held_directory_identity").is_some());
                world.check_effect(route);
                world.check_foreign();
                assert_eq!(
                    fs::read_to_string(world.request()).unwrap(),
                    "foreign selected source"
                );
            }
        }

        #[test]
        fn actual_missing_requested_parent_after_effect_retains_original_os_cause_all_routes() {
            use std::error::Error;
            for route in [
                MaterialRoute::Replace,
                MaterialRoute::Create,
                MaterialRoute::Remove,
            ] {
                let world = AliasFixture::new(route);
                let alias = world.alias.clone();
                AFTER_MUTATION.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move |_| fs::remove_file(&alias).unwrap()));
                });
                let error = route.invoke(&world.request()).unwrap_err();
                assert_uncertain_effect(&error, route, &world.physical);
                let cause: serde_json::Value =
                    serde_json::from_str(&error.details()["cause"]).unwrap();
                assert_eq!(cause["details"]["path"], world.alias.display().to_string());
                assert_eq!(cause["details"]["cause_kind"], "NotFound");
                let original_os_error = fs::metadata(&world.alias).unwrap_err();
                let retained_cause = error
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap();
                assert_eq!(retained_cause.kind(), std::io::ErrorKind::NotFound);
                assert_eq!(
                    retained_cause.raw_os_error(),
                    original_os_error.raw_os_error()
                );
                assert_eq!(
                    cause["details"]["cause_raw_os_error"],
                    serde_json::json!(original_os_error.raw_os_error()).to_string()
                );
                assert!(original_os_error.raw_os_error().is_some());
                world.check_effect(route);
                world.check_foreign();
                assert!(!world.alias.exists());
            }
        }

        #[test]
        fn actual_unchanged_parent_alias_preserves_all_routes_and_exact_noop() {
            for route in [
                MaterialRoute::Replace,
                MaterialRoute::Create,
                MaterialRoute::Remove,
            ] {
                let world = AliasFixture::new(route);
                if matches!(route, MaterialRoute::Replace) {
                    let before = fs::metadata(world.physical.join("wiki.json")).unwrap();
                    assert!(!publish_wiki(
                        &world.request(),
                        "actual old material",
                        &content_hash(b"actual old material")
                    )
                    .unwrap());
                    let after = fs::metadata(world.physical.join("wiki.json")).unwrap();
                    assert_eq!(identity(&before), identity(&after));
                    assert_eq!(
                        (before.mtime(), before.mtime_nsec()),
                        (after.mtime(), after.mtime_nsec())
                    );
                }
                route.invoke(&world.request()).unwrap();
                world.check_effect(route);
                world.check_foreign();
                assert_eq!(fs::read_link(&world.alias).unwrap(), world.physical);
            }
        }

        #[test]
        fn actual_noop_refuses_requested_parent_retarget_without_claiming_publication() {
            let world = AliasFixture::new(MaterialRoute::Replace);
            let before = fs::metadata(world.physical.join("wiki.json")).unwrap();
            BEFORE_MUTATION.with(|slot| {
                *slot.borrow_mut() = Some(world.retarget_observer());
            });
            let error = publish_wiki(
                &world.request(),
                "actual old material",
                &content_hash(b"actual old material"),
            )
            .unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_publication_identity");
            assert!(!error.details().contains_key("published"));
            assert!(!error.details().contains_key("removed"));
            let after = fs::metadata(world.physical.join("wiki.json")).unwrap();
            assert_eq!(identity(&before), identity(&after));
            assert_eq!(
                (before.mtime(), before.mtime_nsec()),
                (after.mtime(), after.mtime_nsec())
            );
            assert_eq!(
                fs::read_to_string(world.physical.join("wiki.json")).unwrap(),
                "actual old material"
            );
            world.check_foreign();
        }

        #[test]
        fn absent_material_creation_has_one_actual_winner_and_private_single_link_source() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("corpus-000.json");
            let barrier = Arc::new(Barrier::new(2));
            let workers: Vec<_> = ["first actual candidate", "second actual candidate"]
                .into_iter()
                .map(|text| {
                    let path = path.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        (text, publish_absent_material(&path, text))
                    })
                })
                .collect();
            let results: Vec<_> = workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect();
            let winners: Vec<_> = results
                .iter()
                .filter(|(_, result)| result.is_ok())
                .collect();
            assert_eq!(winners.len(), 1);
            assert_eq!(fs::read_to_string(&path).unwrap(), winners[0].0);
            let metadata = fs::metadata(&path).unwrap();
            assert_eq!(metadata.nlink(), 1, "no link/unlink crash interval");
            assert_eq!(metadata.mode() & 0o7777, 0o600);
            assert_eq!(
                results.iter().filter(|(_, result)| result.is_err()).count(),
                1
            );
            let original = fs::read_to_string(&path).unwrap();
            assert_eq!(
                publish_absent_material(&path, "must not replace")
                    .unwrap_err()
                    .code(),
                "knowledge.wiki_concurrent_write"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }

        #[test]
        fn absent_material_refuses_a_real_symlink_without_changing_its_target() {
            let directory = tempfile::tempdir().unwrap();
            let target = directory.path().join("retained-private-source");
            fs::write(&target, "private original").unwrap();
            let path = directory.path().join("corpus-000.json");
            std::os::unix::fs::symlink(&target, &path).unwrap();
            assert_eq!(
                publish_absent_material(&path, "candidate")
                    .unwrap_err()
                    .code(),
                "knowledge.wiki_concurrent_write"
            );
            assert_eq!(fs::read_to_string(&target).unwrap(), "private original");
            assert!(fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink());
        }

        #[test]
        fn material_removal_requires_the_actual_old_basis_and_preserves_peer_updates() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("corpus-999.json");
            publish_absent_material(&path, "old selected material").unwrap();
            let basis = content_hash(b"old selected material");
            publish_wiki(&path, "actual later peer material", &basis).unwrap();
            assert_eq!(
                remove_material(&path, &basis).unwrap_err().code(),
                "knowledge.wiki_concurrent_write"
            );
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                "actual later peer material"
            );
            remove_material(&path, &content_hash(b"actual later peer material")).unwrap();
            assert!(!path.exists());
        }

        #[test]
        fn real_absent_publication_then_directory_replacement_retains_native_uncertainty() {
            let world = tempfile::tempdir().unwrap();
            let parent = world.path().join("native");
            let retained_parent = world.path().join("retained-native");
            fs::create_dir(&parent).unwrap();
            let path = parent.join("corpus-000.json");
            let held = open_directory(&parent).unwrap();
            publish_absent_material(&path, "actual newly published material").unwrap();
            let source = open_existing(&path).unwrap();
            let source_identity = identity(&source.metadata().unwrap());
            let metadata = retained(&source, &path).unwrap();
            fs::rename(&parent, &retained_parent).unwrap();
            fs::create_dir(&parent).unwrap();
            fs::write(&path, "foreign source").unwrap();
            let failure = publication_readback(
                &held,
                &parent,
                &parent,
                &path,
                source_identity,
                &content_hash(b"actual newly published material"),
                &metadata,
            )
            .unwrap_err();
            assert_eq!(failure.code(), "knowledge.wiki_publication_uncertain");
            assert_eq!(failure.details()["published"], "true");
            let cause: serde_json::Value =
                serde_json::from_str(&failure.details()["cause"]).unwrap();
            assert_eq!(cause["code"], "knowledge.wiki_publication_identity");
            assert!(cause["details"].get("held_directory_identity").is_some());
            assert_eq!(
                fs::read_to_string(retained_parent.join("corpus-000.json")).unwrap(),
                "actual newly published material"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), "foreign source");
        }

        #[test]
        fn actual_removal_then_directory_replacement_reports_removal_not_wiki_publication() {
            let world = tempfile::tempdir().unwrap();
            let parent = world.path().join("native");
            let retained_parent = world.path().join("retained-native");
            fs::create_dir(&parent).unwrap();
            let path = parent.join("corpus-999.json");
            publish_absent_material(&path, "actual retained material").unwrap();
            let basis = content_hash(b"actual retained material");
            let held = open_directory(&parent).unwrap();
            remove_material(&path, &basis).unwrap();
            fs::rename(&parent, &retained_parent).unwrap();
            fs::create_dir(&parent).unwrap();
            fs::write(&path, "foreign material").unwrap();
            let failure = removal_readback(&held, &parent, &parent, &path, &basis).unwrap_err();
            assert_eq!(failure.code(), "knowledge.source_pool_removal_uncertain");
            assert_eq!(failure.details()["removed"], "true");
            assert_eq!(failure.details()["removed_basis"], basis);
            assert!(!failure.details().contains_key("published"));
            let cause: serde_json::Value =
                serde_json::from_str(&failure.details()["cause"]).unwrap();
            assert_eq!(cause["code"], "knowledge.wiki_publication_identity");
            assert!(!retained_parent.join("corpus-999.json").exists());
            assert_eq!(fs::read_to_string(&path).unwrap(), "foreign material");
        }

        #[test]
        fn actual_postpublication_missing_source_retains_original_io_kind_and_errno() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("corpus-000.json");
            let held = open_directory(directory.path()).unwrap();
            publish_absent_material(&path, "actual publication").unwrap();
            let source = open_existing(&path).unwrap();
            let source_identity = identity(&source.metadata().unwrap());
            let metadata = retained(&source, &path).unwrap();
            fs::remove_file(&path).unwrap();
            let failure = publication_readback(
                &held,
                directory.path(),
                directory.path(),
                &path,
                source_identity,
                &content_hash(b"actual publication"),
                &metadata,
            )
            .unwrap_err();
            assert_eq!(failure.code(), "knowledge.wiki_publication_uncertain");
            assert_eq!(failure.details()["published"], "true");
            let cause: serde_json::Value =
                serde_json::from_str(&failure.details()["cause"]).unwrap();
            assert_eq!(cause["code"], "knowledge.wiki_concurrent_write");
            assert_eq!(cause["details"]["cause_kind"], "NotFound");
            let errno: Option<i32> =
                serde_json::from_str(cause["details"]["cause_raw_os_error"].as_str().unwrap())
                    .unwrap();
            assert!(
                errno.is_some(),
                "retain actual OS errno rather than reconstructing it from prose"
            );
        }

        #[test]
        #[ignore = "subprocess entry used only by the real lock/restart test"]
        fn publication_lock_holder_process() {
            let path =
                std::env::var_os("AIKIT_WIKI_PUBLICATION_TEST_LOCK").expect("native lock path");
            let _held = lock(Path::new(&path)).unwrap();
            println!("publication-lock-acquired");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }

        #[test]
        fn occupied_lock_is_bounded_and_process_interruption_releases_native_lock() {
            use std::io::{BufRead, BufReader};
            use std::process::{Child, Command, Stdio};
            struct Reap(Child);
            impl Drop for Reap {
                fn drop(&mut self) {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("wiki.json");
            let lock_path = directory.path().join(".wiki.json.publication.lock");
            fs::write(&path, "retained").unwrap();
            let mut child = Reap(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored",
                        "--exact",
                        "wiki_publication::native::tests::publication_lock_holder_process",
                        "--nocapture",
                    ])
                    .env("AIKIT_WIKI_PUBLICATION_TEST_LOCK", &lock_path)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let stdout = child.0.stdout.take().unwrap();
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if line.unwrap().contains("publication-lock-acquired") {
                        ready_tx.send(()).unwrap();
                        break;
                    }
                }
            });
            ready_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("real subprocess acquired native lock");
            let started = Instant::now();
            assert_eq!(
                publish_wiki(&path, "blocked", &content_hash(b"retained"))
                    .unwrap_err()
                    .code(),
                "knowledge.wiki_publication_locked"
            );
            assert!(started.elapsed() < Duration::from_secs(7));
            assert_eq!(fs::read_to_string(&path).unwrap(), "retained");
            let lock_identity = identity(&fs::metadata(&lock_path).unwrap());
            child.0.kill().unwrap();
            child.0.wait().unwrap();
            reader.join().unwrap();
            publish_wiki(&path, "recovered", &content_hash(b"retained")).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), "recovered");
            assert_eq!(identity(&fs::metadata(&lock_path).unwrap()), lock_identity);
        }

        fn affiliated_fixture() -> (
            tempfile::TempDir,
            PathBuf,
            PathBuf,
            PathBuf,
            PathBuf,
            (u64, u64),
        ) {
            let owned = tempfile::tempdir().unwrap();
            let old = owned.path().join("original-project");
            let foreign = owned.path().join("foreign-project");
            let member = PathBuf::from("native/wiki.json");
            for root in [&old, &foreign] {
                fs::create_dir_all(root.join("native")).unwrap();
                fs::write(root.join(&member), b"same original basis").unwrap();
                fs::set_permissions(root.join(&member), fs::Permissions::from_mode(0o600)).unwrap();
            }
            let old = fs::canonicalize(old).unwrap();
            let foreign = fs::canonicalize(foreign).unwrap();
            let alias = owned.path().join("declared-project");
            std::os::unix::fs::symlink(&old, &alias).unwrap();
            let expected = identity(&fs::metadata(&old).unwrap());
            (owned, old, foreign, alias, member, expected)
        }

        fn retarget_root(alias: &Path, target: &Path) {
            fs::remove_file(alias).unwrap();
            std::os::unix::fs::symlink(target, alias).unwrap();
        }

        #[test]
        fn affiliated_public_read_and_publication_preserve_alias_cas_privacy_and_noop() {
            let (_owned, old, foreign, alias, member, expected) = affiliated_fixture();
            let source = old.join(&member);
            let original = identity(&fs::metadata(&source).unwrap());
            let root_mode = fs::metadata(&old).unwrap().mode();
            let bytes =
                material_bytes_affiliated(&alias, expected, &member, SOURCE_BUDGET).unwrap();
            assert_eq!(bytes, b"same original basis");
            assert!(!source
                .parent()
                .unwrap()
                .join(".wiki.json.publication.lock")
                .exists());
            assert!(publish_wiki_affiliated(
                &alias,
                expected,
                &member,
                "original owner result",
                &content_hash(&bytes)
            )
            .unwrap());
            let published = fs::metadata(&source).unwrap();
            assert_ne!(identity(&published), original);
            assert_eq!(published.mode() & 0o7777, 0o600);
            assert_eq!(fs::metadata(&old).unwrap().mode(), root_mode);
            assert!(!publish_wiki_affiliated(
                &alias,
                expected,
                &member,
                "original owner result",
                &content_hash(b"original owner result")
            )
            .unwrap());
            assert_eq!(
                identity(&fs::metadata(&source).unwrap()),
                identity(&published)
            );
            assert_eq!(
                material_bytes_affiliated(&alias, expected, &member, SOURCE_BUDGET).unwrap(),
                b"original owner result"
            );
            assert_eq!(
                fs::read(foreign.join(member)).unwrap(),
                b"same original basis"
            );
        }

        #[test]
        fn affiliated_equal_basis_foreign_root_is_not_an_admitted_destination_or_read() {
            let (_owned, old, foreign, alias, member, expected) = affiliated_fixture();
            retarget_root(&alias, &foreign);
            let read =
                material_bytes_affiliated(&alias, expected, &member, SOURCE_BUDGET).unwrap_err();
            let write = publish_wiki_affiliated(
                &alias,
                expected,
                &member,
                "foreign transfer",
                &content_hash(b"same original basis"),
            )
            .unwrap_err();
            for error in [read, write] {
                assert_eq!(error.code(), "knowledge.wiki_publication_identity");
                assert_eq!(error.details()["observation_stage"], "owner_root");
                assert!(error.details().get("published").is_none());
            }
            for root in [&old, &foreign] {
                assert_eq!(
                    fs::read(root.join(&member)).unwrap(),
                    b"same original basis"
                );
                assert!(!root.join("native/.wiki.json.publication.lock").exists());
            }
        }

        #[test]
        fn affiliated_root_alias_retarget_after_capture_refuses_before_any_source_effect() {
            for write in [false, true] {
                let (_owned, old, foreign, alias, member, expected) = affiliated_fixture();
                let moved_foreign = foreign.clone();
                AFTER_AFFILIATED_ROOT_OPEN.with(|slot| {
                    *slot.borrow_mut() =
                        Some(Box::new(move |alias| retarget_root(alias, &moved_foreign)));
                });
                let error = if write {
                    publish_wiki_affiliated(
                        &alias,
                        expected,
                        &member,
                        "other result",
                        &content_hash(b"same original basis"),
                    )
                    .unwrap_err()
                } else {
                    material_bytes_affiliated(&alias, expected, &member, SOURCE_BUDGET).unwrap_err()
                };
                assert_eq!(error.details()["observation_stage"], "owner_root");
                for root in [&old, &foreign] {
                    assert_eq!(
                        fs::read(root.join(&member)).unwrap(),
                        b"same original basis"
                    );
                    assert!(!root.join("native/.wiki.json.publication.lock").exists());
                }
            }
        }

        #[test]
        fn affiliated_root_loss_before_effect_preserves_both_projects_and_refused_stage() {
            let (_owned, old, foreign, alias, member, expected) = affiliated_fixture();
            let moved_alias = alias.clone();
            let moved_foreign = foreign.clone();
            BEFORE_MUTATION.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |_| {
                    retarget_root(&moved_alias, &moved_foreign)
                }));
            });
            let error = publish_wiki_affiliated(
                &alias,
                expected,
                &member,
                "candidate",
                &content_hash(b"same original basis"),
            )
            .unwrap_err();
            assert_eq!(error.details()["observation_stage"], "owner_root");
            assert!(error.details().get("published").is_none());
            let stage = PathBuf::from(&error.details()["stage_path"]);
            assert_eq!(stage.parent().unwrap(), old.join("native"));
            assert!(stage.exists());
            assert_eq!(fs::read(old.join(&member)).unwrap(), b"same original basis");
            assert_eq!(
                fs::read(foreign.join(member)).unwrap(),
                b"same original basis"
            );
            assert_eq!(fs::read_dir(foreign.join("native")).unwrap().count(), 1);
        }

        #[test]
        fn affiliated_missing_root_after_commit_keeps_original_result_and_typed_actual_io() {
            use std::error::Error;
            let (_owned, old, foreign, alias, member, expected) = affiliated_fixture();
            let removed_alias = alias.clone();
            AFTER_MUTATION.with(|slot| {
                *slot.borrow_mut() =
                    Some(Box::new(move |_| fs::remove_file(&removed_alias).unwrap()));
            });
            let error = publish_wiki_affiliated(
                &alias,
                expected,
                &member,
                "committed original result",
                &content_hash(b"same original basis"),
            )
            .unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_publication_uncertain");
            assert_eq!(error.details()["published"], "true");
            assert_eq!(
                error.details()["published_hash"],
                content_hash(b"committed original result")
            );
            assert_eq!(error.details()["automatic_retry"], "false");
            assert_eq!(PathBuf::from(&error.details()["path"]), old.join(&member));
            let cause = error
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap();
            assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
            assert_eq!(cause.raw_os_error(), Some(2));
            let detail: serde_json::Value =
                serde_json::from_str(&error.details()["cause"]).unwrap();
            assert_eq!(detail["details"]["observation_stage"], "owner_root");
            assert_eq!(
                fs::read(old.join(&member)).unwrap(),
                b"committed original result"
            );
            assert_eq!(
                fs::read(foreign.join(member)).unwrap(),
                b"same original basis"
            );
        }

        #[test]
        fn affiliated_noop_late_alias_loss_never_invents_a_publication_effect() {
            let (_owned, old, foreign, alias, member, expected) = affiliated_fixture();
            let moved_alias = alias.clone();
            let moved_foreign = foreign.clone();
            let before = identity(&fs::metadata(old.join(&member)).unwrap());
            AFTER_NOOP_READ.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |_| {
                    retarget_root(&moved_alias, &moved_foreign)
                }));
            });
            let error = publish_wiki_affiliated(
                &alias,
                expected,
                &member,
                "same original basis",
                &content_hash(b"same original basis"),
            )
            .unwrap_err();
            assert_eq!(error.details()["changed"], "false");
            assert_eq!(error.details()["published"], "false");
            assert_eq!(error.details()["observation_stage"], "owner_root");
            assert_eq!(identity(&fs::metadata(old.join(&member)).unwrap()), before);
            assert_eq!(fs::read(old.join(&member)).unwrap(), b"same original basis");
            assert_eq!(
                fs::read(foreign.join(member)).unwrap(),
                b"same original basis"
            );
        }

        #[test]
        fn affiliated_read_final_root_retarget_withholds_copied_body_and_distinguishes_absence() {
            use std::error::Error;
            let (_owned, old, foreign, alias, member, expected) = affiliated_fixture();
            let moved_alias = alias.clone();
            let moved_foreign = foreign.clone();
            AFTER_MATERIAL_FINAL_READ.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |_| {
                    retarget_root(&moved_alias, &moved_foreign)
                }));
            });
            let error =
                material_bytes_affiliated(&alias, expected, &member, SOURCE_BUDGET).unwrap_err();
            assert_eq!(error.details()["observation_stage"], "owner_root");
            retarget_root(&alias, &old);
            fs::remove_file(&alias).unwrap();
            let root_missing =
                material_bytes_affiliated(&alias, expected, &member, SOURCE_BUDGET).unwrap_err();
            assert_eq!(root_missing.details()["observation_stage"], "owner_root");
            assert_eq!(
                root_missing
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::NotFound
            );
            std::os::unix::fs::symlink(&old, &alias).unwrap();
            fs::remove_file(old.join(&member)).unwrap();
            let source_missing =
                material_bytes_affiliated(&alias, expected, &member, SOURCE_BUDGET).unwrap_err();
            assert!(source_missing.details().get("observation_stage").is_none());
            assert_eq!(
                source_missing
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::NotFound
            );
            assert_eq!(
                fs::read(foreign.join(member)).unwrap(),
                b"same original basis"
            );
        }

        #[test]
        fn affiliated_canonical_member_preserves_legitimate_in_root_declared_alias_mapping() {
            let (_owned, old, foreign, alias, member, expected) = affiliated_fixture();
            std::os::unix::fs::symlink(old.join("native"), old.join("declared-member")).unwrap();
            let declared = Path::new("declared-member/wiki.json");
            let observed_member = fs::canonicalize(alias.join(declared))
                .unwrap()
                .strip_prefix(fs::canonicalize(&alias).unwrap())
                .unwrap()
                .to_path_buf();
            assert_eq!(observed_member, member);
            assert!(publish_wiki_affiliated(
                &alias,
                expected,
                &observed_member,
                "mapped result",
                &content_hash(b"same original basis")
            )
            .unwrap());
            assert_eq!(
                fs::canonicalize(alias.join(declared)).unwrap(),
                old.join(&member)
            );
            assert_eq!(
                material_bytes_affiliated(&alias, expected, &observed_member, SOURCE_BUDGET)
                    .unwrap(),
                b"mapped result"
            );
            assert_eq!(
                fs::read(foreign.join(member)).unwrap(),
                b"same original basis"
            );
        }

        #[test]
        fn affiliated_held_member_parent_substitution_refuses_without_cross_project_effect() {
            let (_owned, old, foreign, alias, member, expected) = affiliated_fixture();
            let native = old.join("native");
            let retained = old.join("retained-native");
            let moved_native = native.clone();
            let moved_retained = retained.clone();
            BEFORE_MUTATION.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |_| {
                    fs::rename(&moved_native, &moved_retained).unwrap();
                    fs::create_dir(&moved_native).unwrap();
                    fs::write(moved_native.join("wiki.json"), b"replacement parent basis").unwrap();
                }));
            });
            let error = publish_wiki_affiliated(
                &alias,
                expected,
                &member,
                "candidate",
                &content_hash(b"same original basis"),
            )
            .unwrap_err();
            assert_eq!(error.details()["observation_stage"], "owner_parent");
            assert!(error.details().get("published").is_none());
            assert_eq!(
                fs::read(retained.join("wiki.json")).unwrap(),
                b"same original basis"
            );
            assert_eq!(
                fs::read(native.join("wiki.json")).unwrap(),
                b"replacement parent basis"
            );
            assert_eq!(
                fs::read(foreign.join(member)).unwrap(),
                b"same original basis"
            );
        }

        #[test]
        fn affiliated_source_notfound_after_directory_substitution_retains_actual_cause_and_phase()
        {
            use std::error::Error;
            for root_loss in [false, true] {
                let (owned, old, _foreign, alias, member, expected) = affiliated_fixture();
                let (moved, retained) = if root_loss {
                    (old.clone(), owned.path().join("retained-project"))
                } else {
                    (old.join("native"), old.join("retained-native"))
                };
                let moved_copy = moved.clone();
                let retained_copy = retained.clone();
                BEFORE_SOURCE_OPEN.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move |_| {
                        fs::rename(&moved_copy, &retained_copy).unwrap();
                        fs::create_dir(&moved_copy).unwrap();
                        if root_loss {
                            fs::create_dir(moved_copy.join("native")).unwrap();
                        }
                    }));
                });
                let error = material_bytes_affiliated(&alias, expected, &member, SOURCE_BUDGET)
                    .unwrap_err();
                assert_eq!(error.code(), "knowledge.wiki_concurrent_write");
                assert_eq!(
                    error.details()["observation_stage"],
                    if root_loss {
                        "owner_root"
                    } else {
                        "owner_parent"
                    }
                );
                assert!(error.details().get("owner_affiliation_cause").is_some());
                let first = error
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap();
                assert_eq!(first.kind(), std::io::ErrorKind::NotFound);
                assert_eq!(first.raw_os_error(), Some(2));
                let retained_source = if root_loss {
                    retained.join(&member)
                } else {
                    retained.join("wiki.json")
                };
                assert_eq!(fs::read(retained_source).unwrap(), b"same original basis");
                assert!(!old.join(member).exists());
            }
        }

        #[test]
        fn affiliated_member_escape_and_unproven_directory_alias_refuse_without_effects() {
            let (_owned, old, foreign, alias, _member, expected) = affiliated_fixture();
            std::os::unix::fs::symlink(foreign.join("native"), old.join("escape")).unwrap();
            for member in [
                Path::new("../foreign-project/native/wiki.json"),
                Path::new("/wiki.json"),
                Path::new("escape/wiki.json"),
            ] {
                let error = publish_wiki_affiliated(
                    &alias,
                    expected,
                    member,
                    "transfer",
                    &content_hash(b"same original basis"),
                )
                .unwrap_err();
                assert_eq!(error.details()["observation_stage"], "owner_parent");
                assert!(error.details().get("published").is_none());
            }
            assert_eq!(
                fs::read(old.join("native/wiki.json")).unwrap(),
                b"same original basis"
            );
            assert_eq!(
                fs::read(foreign.join("native/wiki.json")).unwrap(),
                b"same original basis"
            );
            assert!(!foreign.join("native/.wiki.json.publication.lock").exists());
        }

        fn actual_peer_lock_fixture() -> (tempfile::TempDir, PathBuf, File) {
            let owned = tempfile::tempdir().unwrap();
            let parent = fs::canonicalize(owned.path()).unwrap();
            let path = parent.join(".wiki.json.publication.lock");
            let mut peer = OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(&path)
                .unwrap();
            peer.write_all(b"retained peer lock bytes").unwrap();
            let directory = open_directory(&parent).unwrap();
            (owned, path, directory)
        }

        #[test]
        fn actual_peer_lock_recovery_reopens_without_create_or_truncate_and_releases_its_fd() {
            let (_owned, path, directory) = actual_peer_lock_fixture();
            let before = fs::metadata(&path).unwrap();
            let recovered =
                recover_peer_lock_at(&directory, path.file_name().unwrap(), &path).unwrap();
            assert_eq!(identity(&recovered.metadata().unwrap()), identity(&before));
            assert_eq!(fs::read(&path).unwrap(), b"retained peer lock bytes");
            assert_eq!(fs::metadata(&path).unwrap().mode(), before.mode());
            FileExt::try_lock(&recovered).unwrap();
            let recovered = PublicationLock {
                file: recovered,
                owner_pid: std::process::id(),
            };
            // Real duplication shares the open file description, just as a
            // concurrent native child can retain it before exec.
            let duplicate = recovered.file.try_clone().unwrap();
            let other = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            assert!(matches!(
                FileExt::try_lock(&other),
                Err(fs4::TryLockError::WouldBlock)
            ));
            drop(recovered);
            FileExt::try_lock(&other).unwrap();
            drop(duplicate);
            let contender = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            assert!(matches!(
                FileExt::try_lock(&contender),
                Err(fs4::TryLockError::WouldBlock)
            ));
            FileExt::unlock(&other).unwrap();
            assert_eq!(identity(&fs::metadata(&path).unwrap()), identity(&before));
            assert_eq!(fs::read(&path).unwrap(), b"retained peer lock bytes");
        }

        #[test]
        fn actual_unlinked_held_parent_enoent_cannot_recover_into_replaced_named_parent() {
            use std::error::Error;
            let owned = tempfile::tempdir().unwrap();
            let parent = owned.path().join("owned-empty-parent");
            fs::create_dir(&parent).unwrap();
            let parent = fs::canonicalize(parent).unwrap();
            let path = parent.join(".wiki.json.publication.lock");
            BEFORE_INITIAL_LOCK_OPEN.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(|path| {
                    let parent = path.parent().unwrap();
                    fs::remove_dir(parent).unwrap();
                    fs::create_dir(parent).unwrap();
                    fs::write(path, b"foreign peer at replaced parent").unwrap();
                }));
            });
            let error = lock(&path).unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_publication_identity");
            let initial = error
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap();
            assert_eq!(initial.kind(), std::io::ErrorKind::NotFound);
            assert_eq!(initial.raw_os_error(), Some(2));
            assert_eq!(error.details()["lock_bootstrap_recovery"], "refused");
            let recovery: serde_json::Value =
                serde_json::from_str(&error.details()["lock_bootstrap_recovery_cause"]).unwrap();
            assert_eq!(recovery["code"], "knowledge.wiki_publication_identity");
            assert!(recovery["details"].get("held_directory_identity").is_some());
            assert_eq!(fs::read(&path).unwrap(), b"foreign peer at replaced parent");
            assert!(!parent.join("wiki.json").exists());
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        }

        #[test]
        fn actual_peer_lock_recovery_missing_source_does_not_create_a_replacement() {
            use std::error::Error;
            let (_owned, path, directory) = actual_peer_lock_fixture();
            fs::remove_file(&path).unwrap();
            let error =
                recover_peer_lock_at(&directory, path.file_name().unwrap(), &path).unwrap_err();
            assert_eq!(
                error
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                std::io::ErrorKind::NotFound
            );
            assert!(!path.exists());
            assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 0);
        }

        #[test]
        fn actual_peer_lock_recovery_refuses_aliases_and_fifo_without_mutating_peer_data() {
            for form in ["symlink", "hardlink", "fifo"] {
                let (_owned, path, directory) = actual_peer_lock_fixture();
                let retained = path.parent().unwrap().join("retained-peer");
                fs::rename(&path, &retained).unwrap();
                match form {
                    "symlink" => std::os::unix::fs::symlink(&retained, &path).unwrap(),
                    "hardlink" => fs::hard_link(&retained, &path).unwrap(),
                    "fifo" => {
                        use crate::runner::{CommandRunner, SystemRunner};
                        SystemRunner::new()
                            .with_timeout(Duration::from_secs(1))
                            .run(&[
                                "mkfifo".into(),
                                "-m".into(),
                                "600".into(),
                                path.display().to_string(),
                            ])
                            .unwrap()
                            .require(&[], "test.mkfifo_failed")
                            .unwrap();
                    }
                    _ => unreachable!(),
                }
                let error =
                    recover_peer_lock_at(&directory, path.file_name().unwrap(), &path).unwrap_err();
                assert_eq!(
                    error.code(),
                    "knowledge.wiki_publication_identity",
                    "{form}: {error}"
                );
                assert_eq!(fs::read(&retained).unwrap(), b"retained peer lock bytes");
                assert!(fs::symlink_metadata(&path).is_ok());
                assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 2);
            }
        }

        #[test]
        fn actual_peer_lock_recovery_refuses_name_substitution_after_peer_observation() {
            let (_owned, path, directory) = actual_peer_lock_fixture();
            let retained = path.parent().unwrap().join("retained-peer");
            let retained_move = retained.clone();
            let before = identity(&fs::metadata(&path).unwrap());
            AFTER_PEER_LOCK_OBSERVATION.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |path| {
                    fs::rename(path, &retained_move).unwrap();
                    fs::write(path, b"new foreign peer bytes").unwrap();
                }));
            });
            let error =
                recover_peer_lock_at(&directory, path.file_name().unwrap(), &path).unwrap_err();
            assert_eq!(error.code(), "knowledge.wiki_publication_identity");
            assert_eq!(identity(&fs::metadata(&retained).unwrap()), before);
            assert_eq!(fs::read(&retained).unwrap(), b"retained peer lock bytes");
            assert_eq!(fs::read(&path).unwrap(), b"new foreign peer bytes");
            assert_ne!(identity(&fs::metadata(&path).unwrap()), before);
        }

        #[test]
        fn concurrent_exact_basis_writers_have_one_acknowledged_result_then_compose() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("wiki.json");
            fs::write(&path, "original").unwrap();
            let barrier = Arc::new(Barrier::new(3));
            let handles: Vec<_> = ["first", "second"]
                .into_iter()
                .map(|value| {
                    let path = path.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        (
                            value,
                            publish_wiki(&path, value, &content_hash(b"original")),
                        )
                    })
                })
                .collect();
            barrier.wait();
            let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            let diagnostic = serde_json::json!({
                "results": results.iter().map(|(writer, result)| match result {
                    Ok(changed) => serde_json::json!({"writer":writer,"acknowledged":true,"changed":changed}),
                    Err(error) => serde_json::json!({"writer":writer,"acknowledged":false,
                        "code":error.code(),"message":error.message(),"details":error.details()}),
                }).collect::<Vec<_>>(),
                "observed_source": fs::read_to_string(&path).map_err(|error| error.to_string()),
                "source_identity": fs::symlink_metadata(&path).map(|metadata|
                    (metadata.dev(),metadata.ino(),metadata.nlink(),metadata.mode())).map_err(|error| error.to_string()),
                "lock_identity": fs::symlink_metadata(directory.path().join(".wiki.json.publication.lock"))
                    .map(|metadata|(metadata.dev(),metadata.ino(),metadata.nlink(),metadata.mode()))
                    .map_err(|error|error.to_string()),
                "retained_entries": fs::read_dir(directory.path()).unwrap().map(|entry| {
                    let entry = entry.unwrap();
                    let metadata = fs::symlink_metadata(entry.path()).unwrap();
                    serde_json::json!({"name":entry.file_name().to_string_lossy(),
                        "dev":metadata.dev(),"ino":metadata.ino(),"nlink":metadata.nlink(),
                        "mode":metadata.mode(),"len":metadata.len()})
                }).collect::<Vec<_>>(),
            });
            eprintln!("native concurrent publisher results: {diagnostic}");
            assert_eq!(
                results.iter().filter(|(_, r)| r.is_ok()).count(),
                1,
                "{diagnostic}"
            );
            assert_eq!(
                results
                    .iter()
                    .filter(|(_, r)| r
                        .as_ref()
                        .is_err_and(|e| e.code() == "knowledge.wiki_concurrent_write"))
                    .count(),
                1,
                "{diagnostic}"
            );
            let acknowledged = results.iter().find(|(_, r)| r.is_ok()).unwrap().0;
            assert_eq!(fs::read_to_string(&path).unwrap(), acknowledged);
            let composed = format!("{acknowledged}\nlater");
            assert!(
                publish_wiki(&path, &composed, &content_hash(acknowledged.as_bytes())).unwrap()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), composed);
        }

        #[test]
        fn unchanged_keeps_inode_mtime_and_legacy_staging_is_never_touched() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("wiki.json");
            fs::write(&path, " exact source \n").unwrap();
            let legacy = directory.path().join("wiki.json.aikit-tmp");
            let pid_stage = directory
                .path()
                .join(format!(".wiki.json.tmp-{}", std::process::id()));
            fs::write(&legacy, "retained abandoned source").unwrap();
            std::os::unix::fs::symlink(&legacy, &pid_stage).unwrap();
            let before = fs::metadata(&path).unwrap();
            assert!(!publish_wiki(
                &path,
                " exact source \n",
                &content_hash(b" exact source \n")
            )
            .unwrap());
            let after = fs::metadata(&path).unwrap();
            assert_eq!(identity(&before), identity(&after));
            assert_eq!(before.modified().unwrap(), after.modified().unwrap());
            publish_wiki(&path, "new", &content_hash(b" exact source \n")).unwrap();
            assert_eq!(
                fs::read_to_string(&legacy).unwrap(),
                "retained abandoned source"
            );
            assert!(fs::symlink_metadata(&pid_stage)
                .unwrap()
                .file_type()
                .is_symlink());
        }

        #[test]
        fn readonly_no_op_is_stable_and_changed_publication_obeys_actual_os_access() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("wiki.json");
            fs::write(&path, "retained").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
            let before = fs::metadata(&path).unwrap();
            assert!(!publish_wiki(&path, "retained", &content_hash(b"retained")).unwrap());
            let after = fs::metadata(&path).unwrap();
            assert_eq!(identity(&before), identity(&after));
            assert_eq!(before.modified().unwrap(), after.modified().unwrap());
            // A privileged CI user may actually have write access to mode0444.
            // Follow that observed access rather than guessing from mode bits.
            let can_write = OpenOptions::new().write(true).open(&path).is_ok();
            let result = publish_wiki(&path, "changed", &content_hash(b"retained"));
            if can_write {
                assert!(result.unwrap());
                assert_eq!(fs::read_to_string(&path).unwrap(), "changed");
            } else {
                assert_eq!(
                    result.unwrap_err().code(),
                    "knowledge.wiki_publication_write_denied"
                );
                assert_eq!(fs::read_to_string(&path).unwrap(), "retained");
            }
            assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o444);
        }

        #[test]
        fn parent_aliases_share_publication_and_final_aliases_refuse_before_effect() {
            let directory = tempfile::tempdir().unwrap();
            let real = directory.path().join("real");
            fs::create_dir(&real).unwrap();
            let alias = directory.path().join("alias");
            std::os::unix::fs::symlink(&real, &alias).unwrap();
            let path = real.join("wiki.json");
            fs::write(&path, "basis").unwrap();
            publish_wiki(&alias.join("wiki.json"), "first", &content_hash(b"basis")).unwrap();
            assert_eq!(
                publish_wiki(&path, "stale", &content_hash(b"basis"))
                    .unwrap_err()
                    .code(),
                "knowledge.wiki_concurrent_write"
            );
            let hardlink = real.join("hard.json");
            fs::hard_link(&path, &hardlink).unwrap();
            assert_eq!(
                publish_wiki(&path, "second", &content_hash(b"first"))
                    .unwrap_err()
                    .code(),
                "knowledge.wiki_publication_identity"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), "first");
            let symlink = real.join("link.json");
            std::os::unix::fs::symlink(&path, &symlink).unwrap();
            assert!(publish_wiki(&symlink, "second", &content_hash(b"first")).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), "first");
        }

        #[test]
        fn nonregular_source_and_lock_refuse_without_blocking_fifo_open() {
            use crate::runner::{CommandRunner, SystemRunner};
            let directory = tempfile::tempdir().unwrap();
            let fifo = directory.path().join("fifo.json");
            let lock_fifo = directory.path().join(".wiki.json.publication.lock");
            for path in [&fifo, &lock_fifo] {
                let argv = vec!["/usr/bin/mkfifo".into(), path.to_str().unwrap().into()];
                SystemRunner::new()
                    .with_timeout(Duration::from_secs(1))
                    .run(&argv)
                    .unwrap()
                    .require(&argv, "test.fifo_setup")
                    .unwrap();
            }
            let started = Instant::now();
            assert_eq!(
                publish_wiki(&fifo, "effect", &content_hash(b""))
                    .unwrap_err()
                    .code(),
                "knowledge.wiki_publication_identity"
            );
            let source = directory.path().join("wiki.json");
            fs::write(&source, "retained").unwrap();
            assert_eq!(
                publish_wiki(&source, "effect", &content_hash(b"retained"))
                    .unwrap_err()
                    .code(),
                "knowledge.wiki_publication_identity"
            );
            assert!(started.elapsed() < Duration::from_secs(1));
            assert_eq!(fs::read_to_string(&source).unwrap(), "retained");
        }

        #[test]
        fn source_and_stage_name_substitution_are_detected_by_owned_descriptor() {
            let directory = tempfile::tempdir().unwrap();
            for name in ["wiki.json", ".wiki.json.publication-owned.tmp"] {
                let path = directory.path().join(name);
                fs::write(&path, "retained").unwrap();
                let owned = open_existing(&path).unwrap();
                fs::rename(&path, path.with_extension("retained")).unwrap();
                fs::write(&path, "substituted").unwrap();
                assert_eq!(
                    ordinary(&path, &owned).unwrap_err().code(),
                    "knowledge.wiki_publication_identity"
                );
                assert_eq!(fs::read_to_string(&path).unwrap(), "substituted");
            }
        }

        #[test]
        fn refused_named_temp_stage_never_drops_a_substituted_foreign_file() {
            let directory = tempfile::tempdir().unwrap();
            let held = open_directory(directory.path()).unwrap();
            for symlink in [false, true] {
                let mut stage = create_stage(&held, directory.path(), "wiki.json").unwrap();
                stage.write_all(b"owned failed stage").unwrap();
                let stage_path = stage.path().to_path_buf();
                let retained_path = directory.path().join(if symlink {
                    "retained-link.tmp"
                } else {
                    "retained-file.tmp"
                });
                fs::rename(&stage_path, &retained_path).unwrap();
                let foreign = directory.path().join("foreign-source.json");
                fs::write(&foreign, "unselected retained source").unwrap();
                if symlink {
                    std::os::unix::fs::symlink(&foreign, &stage_path).unwrap();
                } else {
                    fs::write(&stage_path, "foreign replacement stage").unwrap();
                }
                assert_eq!(
                    ordinary(&stage_path, stage.as_file()).unwrap_err().code(),
                    "knowledge.wiki_publication_identity"
                );
                drop(stage); // Real NamedTempFile/TempPath Drop, not a plain File.
                assert_eq!(
                    fs::read_to_string(&retained_path).unwrap(),
                    "owned failed stage"
                );
                assert_eq!(
                    fs::read_to_string(&foreign).unwrap(),
                    "unselected retained source"
                );
                if symlink {
                    assert!(fs::symlink_metadata(&stage_path)
                        .unwrap()
                        .file_type()
                        .is_symlink());
                } else {
                    assert_eq!(
                        fs::read_to_string(&stage_path).unwrap(),
                        "foreign replacement stage"
                    );
                }
            }
        }

        #[test]
        fn replaced_parent_is_refused_and_stage_creation_stays_in_the_held_directory() {
            let world = tempfile::tempdir().unwrap();
            let parent = world.path().join("native");
            let retained = world.path().join("retained-native");
            fs::create_dir(&parent).unwrap();
            fs::write(parent.join("wiki.json"), "retained source").unwrap();
            let held = open_directory(&parent).unwrap();
            fs::rename(&parent, &retained).unwrap();
            fs::create_dir(&parent).unwrap();
            fs::write(parent.join("wiki.json"), "foreign source").unwrap();
            assert_eq!(
                directory_at_path(&parent, &held).unwrap_err().code(),
                "knowledge.wiki_publication_identity"
            );
            let stage = create_stage(&held, &parent, "wiki.json").unwrap();
            let name = stage.path().file_name().unwrap().to_owned();
            assert!(retained.join(&name).exists());
            assert!(!parent.join(&name).exists());
            drop(stage);
            assert!(retained.join(&name).exists());
            assert_eq!(
                fs::read_to_string(parent.join("wiki.json")).unwrap(),
                "foreign source"
            );
            assert_eq!(
                fs::read_to_string(retained.join("wiki.json")).unwrap(),
                "retained source"
            );
        }

        #[test]
        fn committed_readback_reports_uncertainty_without_following_a_replaced_parent() {
            let world = tempfile::tempdir().unwrap();
            let parent = world.path().join("native");
            let moved = world.path().join("retained-native");
            fs::create_dir(&parent).unwrap();
            let source_path = parent.join("wiki.json");
            fs::write(&source_path, "basis").unwrap();
            let held = open_directory(&parent).unwrap();
            assert!(publish_wiki(&source_path, "committed", &content_hash(b"basis")).unwrap());
            let published = open_existing(&source_path).unwrap();
            let published_identity = identity(&published.metadata().unwrap());
            let metadata = retained(&published, &source_path).unwrap();
            fs::rename(&parent, &moved).unwrap();
            fs::create_dir(&parent).unwrap();
            fs::write(parent.join("wiki.json"), "foreign source").unwrap();
            let failure = publication_readback(
                &held,
                &parent,
                &parent,
                &source_path,
                published_identity,
                &content_hash(b"committed"),
                &metadata,
            )
            .unwrap_err();
            assert_eq!(failure.code(), "knowledge.wiki_publication_uncertain");
            assert_eq!(failure.details()["published"], "true");
            assert_eq!(
                failure.details()["cause_code"],
                "knowledge.wiki_publication_identity"
            );
            let cause: serde_json::Value =
                serde_json::from_str(&failure.details()["cause"]).unwrap();
            assert_eq!(cause["code"], "knowledge.wiki_publication_identity");
            assert_eq!(
                cause["details"]["held_directory_identity"],
                format!(
                    "{}:{}",
                    held.metadata().unwrap().dev(),
                    held.metadata().unwrap().ino()
                )
            );
            assert_eq!(
                fs::read_to_string(moved.join("wiki.json")).unwrap(),
                "committed"
            );
            assert_eq!(
                fs::read_to_string(parent.join("wiki.json")).unwrap(),
                "foreign source"
            );
        }

        #[test]
        fn committed_readback_refuses_a_foreign_canonical_inode_and_retains_its_bytes() {
            let directory = tempfile::tempdir().unwrap();
            let source_path = directory.path().join("wiki.json");
            fs::write(&source_path, "basis").unwrap();
            let held = open_directory(directory.path()).unwrap();
            assert!(publish_wiki(&source_path, "committed", &content_hash(b"basis")).unwrap());
            let published = open_existing(&source_path).unwrap();
            let published_identity = identity(&published.metadata().unwrap());
            let metadata = retained(&published, &source_path).unwrap();
            fs::rename(&source_path, directory.path().join("retained.json")).unwrap();
            fs::write(&source_path, "foreign source").unwrap();
            let failure = publication_readback(
                &held,
                directory.path(),
                directory.path(),
                &source_path,
                published_identity,
                &content_hash(b"committed"),
                &metadata,
            )
            .unwrap_err();
            assert_eq!(failure.code(), "knowledge.wiki_publication_uncertain");
            assert_eq!(failure.details()["published"], "true");
            assert_eq!(fs::read_to_string(&source_path).unwrap(), "foreign source");
            assert_eq!(
                fs::read_to_string(directory.path().join("retained.json")).unwrap(),
                "committed"
            );
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn substituted_stage_name_cannot_redirect_descriptor_metadata_copy() {
            let directory = tempfile::tempdir().unwrap();
            let source_path = directory.path().join("wiki.json");
            let stage_path = directory.path().join("owned.tmp");
            let retained_path = directory.path().join("retained-stage.tmp");
            let unrelated = directory.path().join("unrelated.json");
            fs::write(&source_path, "source").unwrap();
            fs::write(&stage_path, "stage").unwrap();
            fs::write(&unrelated, "unselected source").unwrap();
            let source = open_existing(&source_path).unwrap();
            let stage = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&stage_path)
                .unwrap();
            fs::rename(&stage_path, &retained_path).unwrap();
            std::os::unix::fs::symlink(&unrelated, &stage_path).unwrap();
            copy_metadata(&source, &stage, &stage_path).unwrap();
            assert_eq!(fs::read_to_string(&unrelated).unwrap(), "unselected source");
            assert_eq!(fs::read_to_string(&retained_path).unwrap(), "source");
            assert_eq!(
                ordinary(&stage_path, &stage).unwrap_err().code(),
                "knowledge.wiki_publication_identity"
            );
        }

        #[test]
        fn publication_retains_real_file_mode_ownership_and_extended_attributes() {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("wiki.json");
            fs::write(&path, "basis").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
            let source = open_existing(&path).unwrap();
            #[cfg(target_os = "macos")]
            let attribute = "org.oi.wiki-publication-test";
            #[cfg(target_os = "linux")]
            let attribute = "user.oi-wiki-publication-test";
            rustix::fs::fsetxattr(
                &source,
                attribute,
                b"retained metadata",
                rustix::fs::XattrFlags::empty(),
            )
            .unwrap();
            #[cfg(target_os = "macos")]
            {
                use crate::runner::{CommandRunner, SystemRunner};
                let argv = vec![
                    "/bin/chmod".into(),
                    "+a".into(),
                    "everyone allow read,readattr,readextattr,readsecurity".into(),
                    path.to_str().unwrap().into(),
                ];
                SystemRunner::new()
                    .with_timeout(Duration::from_secs(1))
                    .run(&argv)
                    .unwrap()
                    .require(&argv, "test.acl_setup")
                    .unwrap();
                assert!(!acl(&path).unwrap().is_empty());
            }
            let before = retained(&source, &path).unwrap();
            publish_wiki(&path, "acknowledged", &content_hash(b"basis")).unwrap();
            let after = retained(&open_existing(&path).unwrap(), &path).unwrap();
            assert_eq!(before, after);
            assert_eq!(fs::read_to_string(&path).unwrap(), "acknowledged");
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn source_acl_write_denial_is_not_bypassed_by_parent_directory_access() {
            use crate::runner::{CommandRunner, SystemRunner};
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("wiki.json");
            fs::write(&path, "retained").unwrap();
            let argv = vec![
                "/bin/chmod".into(),
                "+a".into(),
                "everyone deny write,append".into(),
                path.to_str().unwrap().into(),
            ];
            SystemRunner::new()
                .with_timeout(Duration::from_secs(1))
                .run(&argv)
                .unwrap()
                .require(&argv, "test.acl_setup")
                .unwrap();
            assert!(!publish_wiki(&path, "retained", &content_hash(b"retained")).unwrap());
            assert_eq!(
                publish_wiki(&path, "bypass", &content_hash(b"retained"))
                    .unwrap_err()
                    .code(),
                "knowledge.wiki_publication_write_denied"
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), "retained");
        }
    }
}
