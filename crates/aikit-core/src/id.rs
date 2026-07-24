//! Identifiers.
//!
//! Three identity families matter, and the specification is deliberate about
//! keeping them separate:
//!
//! * [`CapsuleId`] / [`ProfileId`] — content identity within a registry.
//! * [`ProjectId`] — a durable project marker, *not* an absolute path, because
//!   projects and worktrees move.
//! * [`SessionId`] / [`ContextId`] — runtime identity. One session space can own
//!   several contexts (one per project/worktree/task overlay).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::capsule::Kind;
use crate::error::{err, AikitError, Result};

/// Segment charset shared by capsule ids, profile ids and export names.
///
/// Deliberately narrow: these strings become directory names, symlink names and
/// shell command names, so `.`-only segments, whitespace and separators are out.
fn valid_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    if segment == "." || segment == ".." {
        return false;
    }
    if segment.starts_with('-') || segment.ends_with('-') {
        return false;
    }
    segment
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.')
}

/// `kind/group/name`, e.g. `script/test/pytest-gate`.
///
/// Ordering is lexicographic on the *rendered* string, not on the `Kind` enum's
/// declaration order. Every ordered collection of capsule ids in AIKit — the
/// effective view, the lock file, the palette list — is therefore in the order a
/// person reading `script/...` before `skill/...` would expect, and the
/// resolution hash is stable regardless of declaration order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapsuleId {
    kind: Kind,
    /// Everything after the kind, joined with `/`. Always at least one segment.
    path: String,
}

impl Ord for CapsuleId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.kind.as_str(), self.path.as_str()).cmp(&(other.kind.as_str(), other.path.as_str()))
    }
}

impl PartialOrd for CapsuleId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl CapsuleId {
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        let mut parts = raw.split('/');
        let Some(kind_str) = parts.next() else {
            return err("id.malformed", format!("`{raw}` is not a capsule id"));
        };
        let rest: Vec<&str> = parts.collect();
        if rest.is_empty() {
            return err(
                "id.malformed",
                format!("`{raw}` is missing a path after the kind"),
            );
        }
        let kind = Kind::from_str(kind_str).map_err(|_| {
            AikitError::new("id.malformed", format!("`{kind_str}` is not a capsule kind"))
                .with("id", raw)
        })?;
        for segment in &rest {
            if !valid_segment(segment) {
                return err(
                    "id.malformed",
                    format!("`{raw}` contains an invalid path segment `{segment}`"),
                );
            }
        }
        Ok(Self {
            kind,
            path: rest.join("/"),
        })
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The portion after the kind, e.g. `rust/code-review`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The final segment, e.g. `code-review`. Used as a default export name.
    pub fn leaf(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// A filesystem-safe slug, used for generation directories and projections.
    pub fn slug(&self) -> String {
        format!("{}-{}", self.kind.as_str(), self.path.replace('/', "-"))
    }

    /// Registry-relative directory: `capsules/skill/rust/code-review`.
    pub fn registry_path(&self) -> String {
        format!("capsules/{}/{}", self.kind.as_str(), self.path)
    }
}

impl fmt::Display for CapsuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.kind.as_str(), self.path)
    }
}

impl FromStr for CapsuleId {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

impl Serialize for CapsuleId {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CapsuleId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        CapsuleId::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// `profile/<group>/<name>`. A profile is a composition recipe, not a capsule.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileId {
    path: String,
}

impl ProfileId {
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        let Some(rest) = raw.strip_prefix("profile/") else {
            return err(
                "id.malformed",
                format!("`{raw}` is not a profile id (expected `profile/...`)"),
            );
        };
        let segments: Vec<&str> = rest.split('/').collect();
        if segments.is_empty() || segments.iter().any(|s| !valid_segment(s)) {
            return err(
                "id.malformed",
                format!("`{raw}` contains an invalid profile path"),
            );
        }
        Ok(Self {
            path: segments.join("/"),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn registry_path(&self) -> String {
        format!("profiles/{}", self.path)
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "profile/{}", self.path)
    }
}

impl FromStr for ProfileId {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

impl Serialize for ProfileId {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ProfileId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        ProfileId::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// A process-wide monotonic ULID source.
///
/// A plain `Ulid::generate()` only orders correctly across *different*
/// milliseconds: two ids minted inside one millisecond differ only in their
/// random bits and sort arbitrarily. AIKit leans on the sortability — session and
/// context ids are used as a creation order in listings and in the event log —
/// and two sessions or two contexts really are created back to back inside a
/// millisecond during `session up`. So the guarantee has to be real, not
/// probabilistic.
fn next_ulid() -> ulid::Ulid {
    use std::sync::Mutex;
    static GENERATOR: Mutex<ulid::Generator> = Mutex::new(ulid::Generator::new());

    // A poisoned lock means another thread panicked mid-generation. The
    // generator's only state is the previous id, so recovering it is safe and
    // strictly better than propagating a panic into id minting.
    let mut generator = GENERATOR.lock().unwrap_or_else(|e| e.into_inner());
    match generator.generate() {
        Ok(id) => id,
        // Overflow needs 2^80 ids inside one millisecond. Incrementing keeps the
        // ordering guarantee rather than trading it for fresh randomness.
        Err(overflow) => overflow.commit_overflow_increment(),
    }
}

macro_rules! prefixed_id {
    ($name:ident, $prefix:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Generate a fresh, lexicographically sortable identifier.
            pub fn generate() -> Self {
                Self(format!("{}{}", $prefix, next_ulid()))
            }

            pub fn parse(raw: &str) -> Result<Self> {
                let raw = raw.trim();
                if !raw.starts_with($prefix) || raw.len() <= $prefix.len() {
                    return err(
                        "id.malformed",
                        format!("`{raw}` is not a valid {} id", stringify!($name)),
                    );
                }
                if !raw[$prefix.len()..]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return err(
                        "id.malformed",
                        format!("`{raw}` contains characters that are not permitted in an id"),
                    );
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

        impl FromStr for $name {
            type Err = AikitError;
            fn from_str(s: &str) -> Result<Self> {
                Self::parse(s)
            }
        }
    };
}

prefixed_id!(
    SessionId,
    "ses_",
    "A session space: a tmux session, a cmux workspace (group), or a plain terminal context."
);
prefixed_id!(
    ContextId,
    "ctx_",
    "An effective scope tuple: session space + project/worktree + optional task overlay."
);
prefixed_id!(
    ProjectId,
    "prj_",
    "A durable project marker. Deliberately not derived from an absolute path."
);
prefixed_id!(EventId, "evt_", "A recorded observability event.");
prefixed_id!(
    CandidateId,
    "cnd_",
    "A capture candidate awaiting promotion."
);

/// A content-addressed revision of a capsule's payload plus manifest.
///
/// Trust is keyed on `(registry source, capsule id, revision)`. Changing the
/// payload therefore *always* produces a new revision and drops back to review.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(String);

impl Revision {
    pub fn from_hash(hash: blake3::Hash) -> Self {
        Self(hash.to_hex().to_string())
    }

    pub fn from_raw(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The short form used in the TUI and `explain` output.
    ///
    /// Content revisions are blake3 hex, but `from_raw` also accepts strings read
    /// back from the database, so this truncates on a char boundary rather than a
    /// byte index — a corrupt multi-byte revision must not panic the palette.
    pub fn short(&self) -> &str {
        match self.0.char_indices().nth(6) {
            Some((byte, _)) => &self.0[..byte],
            None => &self.0,
        }
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A content-addressed materialization of an effective view.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GenerationId(String);

impl GenerationId {
    pub fn from_hash(hash: blake3::Hash) -> Self {
        Self(format!("gen_{}", &hash.to_hex()[..16]))
    }

    pub fn parse(raw: &str) -> Result<Self> {
        if !raw.starts_with("gen_") || raw.len() <= 4 {
            return err("id.malformed", format!("`{raw}` is not a generation id"));
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn short(&self) -> &str {
        &self.0[..self.0.len().min(10)]
    }
}

impl fmt::Display for GenerationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which registry a capsule came from. Part of the trust key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegistrySource(String);

impl RegistrySource {
    pub const PERSONAL: &'static str = "personal";
    pub const PROJECT_LOCAL: &'static str = "project-local";

    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn personal() -> Self {
        Self(Self::PERSONAL.to_string())
    }

    pub fn project_local() -> Self {
        Self(Self::PROJECT_LOCAL.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Project-local registries never inherit the personal registry's trust.
    pub fn is_project_local(&self) -> bool {
        self.0 == Self::PROJECT_LOCAL
    }
}

impl fmt::Display for RegistrySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_unique_and_prefixed() {
        let a = SessionId::generate();
        let b = SessionId::generate();
        assert_ne!(a, b);
        assert!(a.as_str().starts_with("ses_"));
        assert!(SessionId::parse(a.as_str()).is_ok());
    }

    #[test]
    fn generated_session_ids_sort_by_creation_order() {
        let a = SessionId::generate();
        let b = SessionId::generate();
        assert!(a < b, "ULIDs must remain lexicographically sortable");
    }

    #[test]
    fn profile_ids_require_the_profile_prefix() {
        assert!(ProfileId::parse("profile/code/rust").is_ok());
        assert_eq!(ProfileId::parse("code/rust").unwrap_err().code(), "id.malformed");
    }

    #[test]
    fn capsule_slug_is_filesystem_safe() {
        let id = CapsuleId::parse("skill/rust/code-review").unwrap();
        assert_eq!(id.slug(), "skill-rust-code-review");
        assert_eq!(id.registry_path(), "capsules/skill/rust/code-review");
    }

    #[test]
    fn revisions_shorten_for_display_without_panicking_on_short_input() {
        assert_eq!(Revision::from_raw("ab").short(), "ab");
        assert_eq!(Revision::from_raw("7c2a9e1234").short(), "7c2a9e");
    }
}
