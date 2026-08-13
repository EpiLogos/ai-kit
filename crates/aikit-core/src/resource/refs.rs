use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{AikitError, Result};

macro_rules! opaque_ref {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(raw: &str) -> Result<Self> {
                if raw.is_empty() || raw != raw.trim() || raw.contains('\0') {
                    return Err(AikitError::new(
                        "resource.invalid_ref",
                        format!("`{raw}` is not a valid {} reference", $label),
                    ));
                }
                Ok(Self(raw.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

opaque_ref!(ResourceRef, "resource");
opaque_ref!(OwnerRef, "owner");
opaque_ref!(SourceRef, "source");
opaque_ref!(ProviderRef, "provider");
opaque_ref!(SourceRevision, "source revision");
