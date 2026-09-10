use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{AikitError, Result};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectRef(String);

impl ProjectRef {
    pub fn parse(raw: &str) -> Result<Self> {
        if raw.is_empty() || raw != raw.trim() || raw.contains('\0') {
            return Err(AikitError::new(
                "project.invalid_ref",
                "invalid Project reference",
            ));
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectConstituentRef(String);

impl ProjectConstituentRef {
    pub fn parse(raw: &str) -> Result<Self> {
        if raw.is_empty() || raw != raw.trim() || raw.contains('\0') {
            return Err(AikitError::new(
                "project.invalid_constituent_ref",
                "invalid Project constituent reference",
            ));
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectConstituentRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
