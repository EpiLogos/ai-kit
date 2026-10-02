//! Errors carry a stable machine code because the CLI's JSON envelope publishes it.
//!
//! The code is part of AIKit's public interface: alternative front-ends, shell
//! integrations and tests match on it. Message text may be reworded freely; codes
//! may not.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// A domain error with a stable machine-readable code and structured details.
#[derive(Debug, Clone)]
pub struct AikitError {
    code: &'static str,
    message: String,
    details: BTreeMap<String, String>,
    io_source: Option<Arc<std::io::Error>>,
}

impl AikitError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: BTreeMap::new(),
            io_source: None,
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
        self
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
        self.io_source.as_deref().map(|cause| cause as &dyn std::error::Error)
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

    struct OwnedDirectory(PathBuf);

    impl OwnedDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("aikit-error-{}", ulid::Ulid::generate()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for OwnedDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
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
        let original = physical.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        let retained = cloned.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(original.raw_os_error(), errno);
        assert_eq!(retained.kind(), std::io::ErrorKind::NotFound);
        assert!(std::ptr::eq(original, retained), "clone preserves the original cause object");
    }

    #[test]
    fn native_wrap_and_clone_keep_identical_actual_filesystem_cause() {
        let owned = OwnedDirectory::new();
        let missing = owned.0.join("missing-source");
        let cause = std::fs::File::open(&missing).unwrap_err();
        let errno = cause.raw_os_error();
        let inner = AikitError::new("source.unavailable", "original native read failure")
            .with_io_source(cause);
        let envelope = AikitError::new("source.effect_uncertain", "actual effect readback unavailable")
            .with("published", "true");
        let outer = envelope.clone().with_io_source_from(&inner);
        let cloned = outer.clone().with("published", "true");
        assert_eq!(outer, envelope, "domain equality does not claim effect or IO equality");
        assert_eq!(outer.to_string(), envelope.to_string());
        let original = inner.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        let forwarded = outer.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        let retained = cloned.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
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
        let first = AikitError::new("source.unavailable", "source read unavailable").with_io_source(missing);
        let second = AikitError::new("source.unavailable", "source read unavailable").with_io_source(directory);
        assert_eq!(first, second, "existing domain equality remains compatible");
        let first_cause = first.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        let second_cause = second.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_ne!(first_cause.kind(), second_cause.kind());
        assert!(!std::ptr::eq(first_cause, second_cause));
    }
}
