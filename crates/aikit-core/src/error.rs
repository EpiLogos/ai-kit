//! Errors carry a stable machine code because the CLI's JSON envelope publishes it.
//!
//! The code is part of AIKit's public interface: alternative front-ends, shell
//! integrations and tests match on it. Message text may be reworded freely; codes
//! may not.

use std::collections::BTreeMap;
use std::fmt;

/// A domain error with a stable machine-readable code and structured details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AikitError {
    code: &'static str,
    message: String,
    details: BTreeMap<String, String>,
}

impl AikitError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: BTreeMap::new(),
        }
    }

    /// Attach a structured detail. Details are surfaced verbatim in `--json` output.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
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

impl std::error::Error for AikitError {}

pub type Result<T> = std::result::Result<T, AikitError>;

/// Convenience constructor used throughout the crate.
pub fn err<T>(code: &'static str, message: impl Into<String>) -> Result<T> {
    Err(AikitError::new(code, message))
}
