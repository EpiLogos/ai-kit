//! Errors carry a stable machine code because the CLI's JSON envelope publishes it.
//!
//! The code is part of AIKit's public interface: alternative front-ends, shell
//! integrations and tests match on it. Message text may be reworded freely; codes
//! may not.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Private bounded actual native execution evidence, never semantic authority.
pub struct NativeCapture {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl fmt::Debug for NativeCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeCapture")
            .field("status", &self.status)
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .field("output", &"[private actual native output withheld]")
            .finish()
    }
}

/// A domain error with stable public code/message/details and private evidence.
#[derive(Clone)]
pub struct AikitError {
    code: &'static str,
    message: String,
    details: BTreeMap<String, String>,
    io_source: Option<Arc<std::io::Error>>,
    secondary_io_sources: Vec<Arc<std::io::Error>>,
    native_capture: Option<Arc<NativeCapture>>,
    private_native_cause: Option<Arc<AikitError>>,
    native_result: Option<Arc<serde_json::Value>>,
}

impl AikitError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: BTreeMap::new(),
            io_source: None,
            secondary_io_sources: Vec::new(),
            native_capture: None,
            private_native_cause: None,
            native_result: None,
        }
    }

    /// Attach a structured detail. Details are surfaced verbatim in `--json` output.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }

    /// Retain the actual physical failure independently of the stable domain
    /// envelope. Cloning this error retains the same original IO cause.
    #[must_use]
    pub fn with_io_source(mut self, cause: std::io::Error) -> Self {
        self.io_source = Some(Arc::new(cause));
        self
    }

    /// Forward the actual retained IO cause into another native domain
    /// envelope. No cause is inferred from its code, details or message.
    #[must_use]
    pub fn with_io_source_from(mut self, cause: &Self) -> Self {
        self.io_source = cause.io_source.clone();
        self.secondary_io_sources = cause.secondary_io_sources.clone();
        self.native_capture = cause.native_capture.clone();
        self.private_native_cause = cause.private_native_cause.clone();
        self.native_result = cause.native_result.clone();
        self
    }

    /// Preserve an independently observed cleanup/readback IO failure without
    /// replacing the original primary source or exposing private output.
    #[must_use]
    pub fn with_secondary_io_source_from(mut self, cause: &Self) -> Self {
        if let Some(source) = &cause.io_source {
            self.secondary_io_sources.push(source.clone());
        }
        self.secondary_io_sources
            .extend(cause.secondary_io_sources.iter().cloned());
        self
    }

    pub fn secondary_io_sources(&self) -> impl Iterator<Item = &std::io::Error> {
        self.secondary_io_sources.iter().map(Arc::as_ref)
    }

    /// Existing native error evidence, intentionally absent from public JSON
    /// details and diagnostics. Callers must supply the actual bounded capture.
    #[must_use]
    pub fn with_native_capture(
        mut self,
        status: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    ) -> Self {
        self.native_capture = Some(Arc::new(NativeCapture {
            status,
            stdout,
            stderr,
        }));
        self
    }
    pub fn native_capture(&self) -> Option<&NativeCapture> {
        self.native_capture.as_deref()
    }
    #[must_use]
    pub fn with_private_native_cause(mut self, cause: &Self) -> Self {
        self.private_native_cause = Some(Arc::new(cause.clone()));
        self
    }
    pub fn private_native_cause(&self) -> Option<&Self> {
        self.private_native_cause.as_deref()
    }

    /// Actual already-parsed native result, distinct from raw capture/status.
    #[must_use]
    pub fn with_native_result(mut self, actual: serde_json::Value) -> Self {
        self.native_result = Some(Arc::new(actual));
        self
    }
    pub fn native_result(&self) -> Option<&serde_json::Value> {
        self.native_result.as_deref()
    }
    #[must_use]
    pub fn with_native_result_from(self, actual: Option<&serde_json::Value>) -> Self {
        match actual {
            Some(actual) => self.with_native_result(actual.clone()),
            None => self,
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn details(&self) -> &BTreeMap<String, String> {
        &self.details
    }
}

impl fmt::Debug for AikitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AikitError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("details", &self.details)
            .field(
                "io_source",
                &self
                    .io_source
                    .as_ref()
                    .map(|cause| (cause.kind(), cause.raw_os_error())),
            )
            .field(
                "secondary_io_sources",
                &self
                    .secondary_io_sources
                    .iter()
                    .map(|cause| (cause.kind(), cause.raw_os_error()))
                    .collect::<Vec<_>>(),
            )
            .field("native_capture", &self.native_capture)
            .field(
                "private_native_cause",
                &self
                    .private_native_cause
                    .as_ref()
                    .map(|_| "[private actual native error withheld]"),
            )
            .field(
                "native_result",
                &self
                    .native_result
                    .as_ref()
                    .map(|_| "[private actual parsed native result withheld]"),
            )
            .finish()
    }
}

// Domain equality is the existing public code/message/details contract. It
// does not claim that two physical operations or their OS causes are equal.
impl PartialEq for AikitError {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.message == other.message && self.details == other.details
    }
}

impl Eq for AikitError {}

impl fmt::Display for AikitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if !self.details.is_empty() {
            let rendered: Vec<String> = self
                .details
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            write!(f, " ({})", rendered.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for AikitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.io_source
            .as_deref()
            .map(|cause| cause as &dyn std::error::Error)
    }
}

pub type Result<T> = std::result::Result<T, AikitError>;

/// Convenience constructor used throughout the crate.
pub fn err<T>(code: &'static str, message: impl Into<String>) -> Result<T> {
    Err(AikitError::new(code, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::path::PathBuf;

    // Test material belongs to this compiled product's Run space. An explicit
    // root narrows that same physical scratch area; it never supplies authority.
    // Unix custody compares actual objects. Windows custody holds each native
    // name against delete/rename; it does not manufacture a file identity.
    #[derive(Clone, Debug, Eq, PartialEq)]
    enum DirectoryCustody {
        #[cfg(unix)]
        UnixObject { device: u64, inode: u64 },
        #[cfg(windows)]
        WindowsHeldName,
    }

    #[cfg(unix)]
    fn directory_custody(file: &std::fs::File) -> std::io::Result<DirectoryCustody> {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        if !metadata.is_dir() {
            return Err(std::io::Error::other("held test object is not a directory"));
        }
        Ok(DirectoryCustody::UnixObject {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    #[cfg(windows)]
    fn directory_custody(file: &std::fs::File) -> std::io::Result<DirectoryCustody> {
        use std::os::windows::fs::MetadataExt;
        let metadata = file.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(std::io::Error::other(
                "held test object must be a physical directory",
            ));
        }
        // open_directory owns the native sharing restriction for this handle.
        // This names that custody mechanism, not an object ID or equality proof.
        Ok(DirectoryCustody::WindowsHeldName)
    }

    #[cfg(not(any(unix, windows)))]
    fn directory_custody(_: &std::fs::File) -> std::io::Result<DirectoryCustody> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "native test directory custody is unavailable on this platform",
        ))
    }

    fn open_directory(path: &std::path::Path) -> std::io::Result<std::fs::File> {
        let named = std::fs::symlink_metadata(path)?;
        if !named.is_dir() || named.file_type().is_symlink() {
            return Err(std::io::Error::other(
                "test directory must be physical and non-symlink",
            ));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if named.file_attributes() & 0x400 != 0 {
                return Err(std::io::Error::other(
                    "named test directory is a reparse object",
                ));
            }
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK: a raced FIFO cannot block.
            options.custom_flags(0x10000 | 0x20000 | 0x800);
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // arm64 UAPI retains its distinct O_DIRECTORY/O_NOFOLLOW values.
            options.custom_flags(0x4000 | 0x8000 | 0x800);
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Darwin O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK.
            options.custom_flags(0x100000 | 0x100 | 0x4);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_SHARE_READ | FILE_SHARE_WRITE, deliberately no SHARE_DELETE.
            // Keep native delete/rename exclusion until this handle closes.
            options.share_mode(0x1 | 0x2);
            // FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT.
            options.custom_flags(0x02000000 | 0x00200000);
        }
        let held = options.open(path)?;
        directory_custody(&held)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let opened = held.metadata()?;
            if (named.dev(), named.ino()) != (opened.dev(), opened.ino()) {
                return Err(std::io::Error::other(
                    "test directory changed while opening",
                ));
            }
        }
        Ok(held)
    }

    fn require_affiliation(
        path: &std::path::Path,
        held: &std::fs::File,
        expected: &DirectoryCustody,
    ) -> std::io::Result<()> {
        let named = open_directory(path)?;
        #[cfg(unix)]
        if &directory_custody(held)? != expected || &directory_custody(&named)? != expected {
            return Err(std::io::Error::other("owned test directory object changed"));
        }
        #[cfg(windows)]
        {
            // The held name and every parent stay deletion-excluded. Recheck
            // physical/reparse state on both real handles; no fake ID comparison.
            match expected {
                DirectoryCustody::WindowsHeldName => {
                    directory_custody(held)?;
                    directory_custody(&named)?;
                }
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = expected;
            directory_custody(held)?;
            directory_custody(&named)?;
        }
        if std::fs::canonicalize(path)? != path {
            return Err(std::io::Error::other(
                "owned test directory canonical affiliation changed",
            ));
        }
        Ok(())
    }

    struct HeldDirectory {
        path: PathBuf,
        file: std::fs::File,
        custody: DirectoryCustody,
    }

    impl HeldDirectory {
        fn open(path: PathBuf) -> std::io::Result<Self> {
            let file = open_directory(&path)?;
            let custody = directory_custody(&file)?;
            require_affiliation(&path, &file, &custody)?;
            Ok(Self {
                path,
                file,
                custody,
            })
        }

        fn check(&self) -> std::io::Result<()> {
            require_affiliation(&self.path, &self.file, &self.custody)
        }
    }

    fn check_parents(parents: &[HeldDirectory]) -> std::io::Result<()> {
        parents.iter().try_for_each(HeldDirectory::check)
    }

    // .0 remains the actual fixture path used by all four unchanged IO tests.
    // Parents include the product, scratch and every explicit root ancestor.
    struct OwnedDirectory(PathBuf, Option<HeldDirectory>, Vec<HeldDirectory>);

    impl OwnedDirectory {
        fn new() -> Self {
            let product = std::fs::canonicalize(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
            )
            .expect("actual compiled Core product coordinate");
            let scratch = product.join("ProjectCentral/now/tmp");
            let requested_root = std::env::var_os("AIKIT_CORE_TEST_ROOT").map(|requested| {
                let requested = PathBuf::from(requested);
                assert!(
                    requested.is_absolute(),
                    "explicit Core test root must be absolute"
                );
                let physical = std::fs::canonicalize(&requested)
                    .expect("explicit Core test root must already exist");
                assert_eq!(
                    physical, requested,
                    "explicit Core test root must be physical"
                );
                assert!(
                    physical.starts_with(&scratch),
                    "explicit Core test root must remain in this product Run space"
                );
                physical
            });
            let mut parents = vec![HeldDirectory::open(product.clone())
                .expect("hold physical compiled Core product directory")];
            let mut current = product;
            for member in ["ProjectCentral", "now", "tmp"] {
                check_parents(&parents)
                    .expect("held product ancestors before Run-space preparation");
                current.push(member);
                if requested_root.is_none() {
                    match std::fs::create_dir(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => panic!(
                            "create product test Run space {}: {error}",
                            current.display()
                        ),
                    }
                }
                parents.push(
                    HeldDirectory::open(current.clone())
                        .expect("hold physical product test Run-space ancestor"),
                );
            }
            let root = match requested_root {
                Some(root) => {
                    let relative = root.strip_prefix(&scratch).unwrap().to_path_buf();
                    for member in relative.components() {
                        check_parents(&parents).expect("held explicit test-root ancestors");
                        current.push(member);
                        parents.push(
                            HeldDirectory::open(current.clone())
                                .expect("hold physical explicit test-root ancestor"),
                        );
                    }
                    root
                }
                None => current,
            };
            check_parents(&parents)
                .expect("actual Core test-root affiliation before fixture creation");
            let path = root.join(format!("aikit-error-{}", ulid::Ulid::generate()));
            std::fs::create_dir(&path).expect("create a fresh owned Core test directory");
            let held = HeldDirectory::open(path.clone()).expect("hold fresh Core test directory");
            check_parents(&parents)
                .expect("test-root affiliation after fixture creation; retain on uncertainty");
            held.check().expect("owned Core test-directory affiliation");
            Self(path, Some(held), parents)
        }
    }

    impl Drop for OwnedDirectory {
        fn drop(&mut self) {
            let cleanup = check_parents(&self.2)
                .and_then(|()| {
                    self.1
                        .as_ref()
                        .expect("owned Core fixture handle retained until cleanup")
                        .check()
                })
                .and_then(|()| {
                    // Windows must close only the fixture's deletion-excluding
                    // handle, while every parent/root remains held. This final
                    // close-to-remove interval is not an atomic conditional delete.
                    #[cfg(windows)]
                    drop(self.1.take());
                    // Unix has a finite check-to-unlink interval too. These tests
                    // leave an empty directory; never recursively remove a child.
                    std::fs::remove_dir(&self.0)
                });
            if let Err(error) = cleanup {
                eprintln!(
                    "Core test cleanup uncertain; retained {}: {error}",
                    self.0.display()
                );
                if !std::thread::panicking() {
                    panic!("owned Core test directory cleanup failed: {error}");
                }
            }
        }
    }

    #[test]
    fn actual_missing_file_cause_survives_clone_details_and_domain_display() {
        let owned = OwnedDirectory::new();
        let missing = owned.0.join("missing-source");
        let cause = std::fs::File::open(&missing).unwrap_err();
        let errno = cause.raw_os_error();
        assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
        let domain = AikitError::new("source.unavailable", "source read unavailable")
            .with("path", missing.display().to_string());
        let physical = domain.clone().with_io_source(cause);
        let cloned = physical.clone().with("path", missing.display().to_string());
        assert_eq!(physical, domain);
        assert_eq!(physical.to_string(), domain.to_string());
        assert_eq!(physical.code(), domain.code());
        assert_eq!(physical.message(), domain.message());
        assert_eq!(physical.details(), domain.details());
        let original = physical
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        let retained = cloned
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        assert_eq!(original.raw_os_error(), errno);
        assert_eq!(retained.kind(), std::io::ErrorKind::NotFound);
        assert!(
            std::ptr::eq(original, retained),
            "clone preserves the original cause object"
        );
    }

    #[test]
    fn actual_primary_and_secondary_io_objects_survive_wrap_and_clone() {
        let owned = OwnedDirectory::new();
        let first = AikitError::new("source.primary", "primary native failure")
            .with_io_source(std::fs::File::open(owned.0.join("missing-primary")).unwrap_err());
        let second = AikitError::new("source.secondary", "secondary native readback failure")
            .with_io_source(std::fs::File::open(owned.0.join("missing-secondary")).unwrap_err());
        let combined = AikitError::new("source.uncertain", "original effect uncertain")
            .with_io_source_from(&first)
            .with_secondary_io_source_from(&second);
        let retained = combined.clone();
        let primary = first
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        let secondary = second
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        assert!(std::ptr::eq(
            primary,
            retained
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
        ));
        assert!(std::ptr::eq(
            secondary,
            retained.secondary_io_sources().next().unwrap()
        ));
        assert_eq!(retained.secondary_io_sources().count(), 1);
    }

    #[test]
    fn native_wrap_and_clone_keep_identical_actual_filesystem_cause() {
        let owned = OwnedDirectory::new();
        let missing = owned.0.join("missing-source");
        let cause = std::fs::File::open(&missing).unwrap_err();
        let errno = cause.raw_os_error();
        let inner = AikitError::new("source.unavailable", "original native read failure")
            .with_io_source(cause);
        let envelope = AikitError::new(
            "source.effect_uncertain",
            "actual effect readback unavailable",
        )
        .with("published", "true");
        let outer = envelope.clone().with_io_source_from(&inner);
        let cloned = outer.clone().with("published", "true");
        assert_eq!(
            outer, envelope,
            "domain equality does not claim effect or IO equality"
        );
        assert_eq!(outer.to_string(), envelope.to_string());
        let original = inner
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        let forwarded = outer
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        let retained = cloned
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        assert_eq!(forwarded.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(forwarded.raw_os_error(), errno);
        assert!(std::ptr::eq(original, forwarded));
        assert!(std::ptr::eq(original, retained));
    }

    #[test]
    fn domain_equality_does_not_erase_distinct_actual_filesystem_causes() {
        let owned = OwnedDirectory::new();
        let missing = std::fs::read(owned.0.join("missing-source")).unwrap_err();
        let directory = std::fs::read(&owned.0).unwrap_err();
        assert_ne!(missing.kind(), directory.kind());
        let first = AikitError::new("source.unavailable", "source read unavailable")
            .with_io_source(missing);
        let second = AikitError::new("source.unavailable", "source read unavailable")
            .with_io_source(directory);
        assert_eq!(first, second, "existing domain equality remains compatible");
        let first_cause = first
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        let second_cause = second
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        assert_ne!(first_cause.kind(), second_cause.kind());
        assert!(!std::ptr::eq(first_cause, second_cause));
    }

    #[test]
    fn actual_directory_form_and_reparse_refusal_retains_targets() {
        let owned = OwnedDirectory::new();
        let ordinary = owned.0.join("ordinary-file");
        std::fs::write(&ordinary, b"retained ordinary bytes").unwrap();
        let wrong_form = open_directory(&ordinary).unwrap_err();
        eprintln!("actual ordinary-file refusal: {wrong_form}");
        assert_eq!(
            std::fs::read(&ordinary).unwrap(),
            b"retained ordinary bytes"
        );
        let target = owned.0.join("physical-target");
        std::fs::create_dir(&target).unwrap();
        let target_body = target.join("retained-source");
        std::fs::write(&target_body, b"retained target bytes").unwrap();
        let link = owned.0.join("native-directory-link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link)
            .expect("real native directory symlink prerequisite");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, &link)
            .expect("real Windows directory reparse creation prerequisite; no skip");
        let refusal = open_directory(&link).unwrap_err();
        eprintln!("actual directory-link/reparse refusal: {refusal}");
        assert_eq!(
            std::fs::read(&target_body).unwrap(),
            b"retained target bytes"
        );
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        #[cfg(unix)]
        std::fs::remove_file(&link).unwrap();
        #[cfg(windows)]
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_file(target_body).unwrap();
        std::fs::remove_dir(target).unwrap();
        std::fs::remove_file(ordinary).unwrap();
    }

    #[test]
    fn actual_unexpected_child_refuses_empty_cleanup_and_retains_bytes() {
        let owned = OwnedDirectory::new();
        let path = owned.0.clone();
        let child = path.join("unexpected-retained-material");
        std::fs::write(&child, b"actual child must survive refusal").unwrap();
        let refusal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(owned)));
        assert!(
            refusal.is_err(),
            "actual nonempty cleanup must refuse, never recursively delete"
        );
        assert!(path.is_dir());
        assert_eq!(
            std::fs::read(&child).unwrap(),
            b"actual child must survive refusal"
        );
        // This test owns the child it created. Only after proving retention does
        // it remove that exact file and the now-empty fixture, never recursively.
        std::fs::remove_file(child).unwrap();
        std::fs::remove_dir(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn actual_unix_fixture_replacement_refuses_cleanup_and_retains_both_objects() {
        let owned = OwnedDirectory::new();
        let path = owned.0.clone();
        let retained = path.with_file_name(format!(
            "{}-retained",
            path.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(path.join("original-material"), b"original owned material").unwrap();
        std::fs::rename(&path, &retained).unwrap();
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("foreign-material"),
            b"distinct intervening material",
        )
        .unwrap();
        let refusal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(owned)));
        assert!(
            refusal.is_err(),
            "same held-inode affiliation must reject a different named directory"
        );
        assert_eq!(
            std::fs::read(retained.join("original-material")).unwrap(),
            b"original owned material"
        );
        assert_eq!(
            std::fs::read(path.join("foreign-material")).unwrap(),
            b"distinct intervening material"
        );
        use std::os::unix::fs::MetadataExt;
        let original = std::fs::metadata(&retained).unwrap();
        let intervening = std::fs::metadata(&path).unwrap();
        assert_ne!(
            (original.dev(), original.ino()),
            (intervening.dev(), intervening.ino())
        );
        // Both branches are exclusively created by this adverse test. The
        // helper's refusal has already preserved them; empty-only test cleanup
        // does not assert an atomic check/remove guarantee against other writers.
        std::fs::remove_file(retained.join("original-material")).unwrap();
        std::fs::remove_dir(retained).unwrap();
        std::fs::remove_file(path.join("foreign-material")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn actual_unix_ancestor_replacement_refuses_cleanup_and_retains_both_branches() {
        let outer = OwnedDirectory::new();
        let ancestor = outer.0.join("owned-ancestor");
        let retained = outer.0.join("retained-ancestor");
        let child = ancestor.join("owned-child");
        std::fs::create_dir(&ancestor).unwrap();
        std::fs::create_dir(&child).unwrap();
        std::fs::write(child.join("original-material"), b"original ancestor branch").unwrap();
        let owned = OwnedDirectory(
            child.clone(),
            Some(HeldDirectory::open(child.clone()).unwrap()),
            vec![
                HeldDirectory::open(outer.0.clone()).unwrap(),
                HeldDirectory::open(ancestor.clone()).unwrap(),
            ],
        );
        std::fs::rename(&ancestor, &retained).unwrap();
        std::fs::create_dir(&ancestor).unwrap();
        std::fs::create_dir(&child).unwrap();
        std::fs::write(
            child.join("foreign-material"),
            b"intervening ancestor branch",
        )
        .unwrap();
        let refusal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(owned)));
        assert!(
            refusal.is_err(),
            "actual retained parent must reject named ancestor replacement"
        );
        assert_eq!(
            std::fs::read(retained.join("owned-child/original-material")).unwrap(),
            b"original ancestor branch"
        );
        assert_eq!(
            std::fs::read(child.join("foreign-material")).unwrap(),
            b"intervening ancestor branch"
        );
        std::fs::remove_file(retained.join("owned-child/original-material")).unwrap();
        std::fs::remove_dir(retained.join("owned-child")).unwrap();
        std::fs::remove_dir(retained).unwrap();
        std::fs::remove_file(child.join("foreign-material")).unwrap();
        std::fs::remove_dir(child).unwrap();
        std::fs::remove_dir(ancestor).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn actual_windows_held_fixture_refuses_delete_and_rename_until_legal_close() {
        let mut owned = OwnedDirectory::new();
        let original = owned.0.clone();
        let renamed = original.with_file_name(format!(
            "{}-renamed",
            original.file_name().unwrap().to_string_lossy()
        ));
        let rename_refusal = std::fs::rename(&original, &renamed).unwrap_err();
        let delete_refusal = std::fs::remove_dir(&original).unwrap_err();
        eprintln!("actual live Windows rename refusal: kind={:?} errno={:?}; delete refusal: kind={:?} errno={:?}",
            rename_refusal.kind(), rename_refusal.raw_os_error(), delete_refusal.kind(), delete_refusal.raw_os_error());
        assert!(rename_refusal.raw_os_error().is_some());
        assert!(delete_refusal.raw_os_error().is_some());
        assert!(original.is_dir());
        assert!(!renamed.exists());
        check_parents(&owned.2).unwrap();
        owned.1.as_ref().unwrap().check().unwrap();
        // Close only this fixture handle. Every real parent remains held. A
        // successful actual rename now distinguishes held-name exclusion from
        // an unrelated permission denial; it is not a128bit identity claim.
        drop(owned.1.take());
        std::fs::rename(&original, &renamed).unwrap();
        assert!(!original.exists());
        assert!(renamed.is_dir());
        std::fs::rename(&renamed, &original).unwrap();
        owned.1 = Some(HeldDirectory::open(original.clone()).unwrap());
        check_parents(&owned.2).unwrap();
        drop(owned);
        assert!(
            !original.exists(),
            "same empty-only owner cleanup must finish after its legal close"
        );
        assert!(!renamed.exists());
    }
}
