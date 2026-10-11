//! The one shared identity-source compositor for the `central.agent-profile/v1`
//! `self_definition` pins.
//!
//! Central pins two exact sources — the Agent's self definition and its
//! relational Logos — as `central:source:{world_ref}:{relative path}`
//! references with sha256 byte digests. Both delivery routes that must reach
//! the real model context (the Direct parent-session prompt payload and the
//! admitted-Agency turn text) resolve and verify those pins *here* and only
//! here, so the two routes cannot drift apart in what they treat as the
//! accepted first-person orientation. Exact bytes are embedded verbatim: no
//! reflow, no truncation, no second normalisation.

use crate::central_agent_profile::{CentralAgentSelfDefinition, CentralAgentSelfSourcePin};
use aikit_core::AikitError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

pub const IDENTITY_COMPOSITOR_VERSION: &str = "aikit.identity-composition/v1";

/// How the composed orientation reaches the model on the Direct route: the
/// parent-session prompt payload. The Agency route composes the same section
/// into its turn text and records that mode on its receipt.
pub const DIRECT_DELIVERY_MODE: &str = "native-parent-session-prompt-payload";
pub const AGENCY_DELIVERY_MODE: &str = "native-agency-turn-text";

/// The reference grammar's fixed prefix: `central:source:` followed by the
/// world ref, a colon, and the plain relative path under the world root.
pub const CENTRAL_SOURCE_REF_PREFIX: &str = "central:source:";

/// A bounded exact-source read: the same 4 MiB ceiling the required-context
/// admission applies to admitted source material.
const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;

/// A named refusal to compose identity from the pinned sources. Each variant
/// is honest about what happened; nothing here silently substitutes a fallback
/// for an accepted source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentitySourceError {
    /// The pinned reference resolves to nothing on disk.
    Missing { reference: String },
    /// The reference resolves, but not to a plain readable regular file (a
    /// symlink, a directory, or a path escaping the world root).
    Withheld { reference: String },
    /// The file's actual sha256 differs from the accepted pin.
    DigestMismatch {
        reference: String,
        expected: String,
        actual: String,
    },
    /// The file exists but cannot be honestly delivered (unreadable, oversize,
    /// not UTF-8, or a malformed pin).
    Unreadable { reference: String, reason: String },
}

impl IdentitySourceError {
    /// The route-scoped error code, e.g. `direct_agent.self_source_missing`.
    /// Both delivery routes name the same conditions under their own prefix.
    pub fn into_aikit(self, route: IdentitySourceRoute) -> AikitError {
        let (code, message) = match (&self, route) {
            (IdentitySourceError::Missing { reference }, route) => (
                route.missing_code(),
                format!("The accepted self source {reference} is missing; reread and explicitly accept the Agent definition again"),
            ),
            (IdentitySourceError::Withheld { reference }, route) => (
                route.withheld_code(),
                format!("The accepted self source {reference} is not a plain readable file in its World; it is withheld"),
            ),
            (
                IdentitySourceError::DigestMismatch {
                    reference,
                    expected,
                    actual,
                },
                route,
            ) => (
                route.digest_mismatch_code(),
                format!("The accepted self source {reference} changed since acceptance (expected {expected}, found {actual}); reread and explicitly accept the Agent definition again"),
            ),
            (
                IdentitySourceError::Unreadable { reference, reason },
                route,
            ) => (
                route.unreadable_code(),
                format!("The accepted self source {reference} could not be read: {reason}"),
            ),
        };
        AikitError::new(code, message)
    }
}

/// The delivery route asking for composition; only the error-code prefix
/// differs between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySourceRoute {
    Direct,
    Agency,
}

impl IdentitySourceRoute {
    fn missing_code(self) -> &'static str {
        match self {
            Self::Direct => "direct_agent.self_source_missing",
            Self::Agency => "encounter.self_source_missing",
        }
    }
    fn withheld_code(self) -> &'static str {
        match self {
            Self::Direct => "direct_agent.self_source_withheld",
            Self::Agency => "encounter.self_source_withheld",
        }
    }
    fn digest_mismatch_code(self) -> &'static str {
        match self {
            Self::Direct => "direct_agent.self_source_digest_mismatch",
            Self::Agency => "encounter.self_source_digest_mismatch",
        }
    }
    fn unreadable_code(self) -> &'static str {
        match self {
            Self::Direct => "direct_agent.self_source_unreadable",
            Self::Agency => "encounter.self_source_unreadable",
        }
    }
}

impl std::fmt::Display for IdentitySourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { reference } => write!(f, "self source {reference} is missing"),
            Self::Withheld { reference } => {
                write!(
                    f,
                    "self source {reference} is withheld (not a plain regular file)"
                )
            }
            Self::DigestMismatch {
                reference,
                expected,
                actual,
            } => write!(
                f,
                "self source {reference} digest mismatch: expected {expected}, found {actual}"
            ),
            Self::Unreadable { reference, reason } => {
                write!(f, "self source {reference} is unreadable: {reason}")
            }
        }
    }
}

impl std::error::Error for IdentitySourceError {}

/// One resolved, digest-verified source: the exact accepted bytes and the
/// digest they were verified against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIdentitySource {
    pub reference: String,
    pub digest: String,
    pub text: String,
}

/// Both pinned sources of one `self_definition`, resolved and verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIdentitySources {
    pub self_source: ResolvedIdentitySource,
    pub relational_logos: ResolvedIdentitySource,
}

/// Resolve and sha256-verify a profile's `self_definition` pins against the
/// world root on disk. Refuses anything that is not the exact accepted source:
/// a missing file, a symlink or directory, a byte change, an oversize or
/// non-UTF-8 file. The world root is the Central root for root scope and the
/// native Project root for project scope, matching the
/// `central:source:{world_ref}:{relative path}` grammar.
pub fn resolve_self_sources(
    world_root: &Path,
    self_def: &CentralAgentSelfDefinition,
) -> Result<ResolvedIdentitySources, IdentitySourceError> {
    Ok(ResolvedIdentitySources {
        self_source: resolve_one(world_root, &self_def.source)?,
        relational_logos: resolve_one(world_root, &self_def.relational_logos)?,
    })
}

fn resolve_one(
    world_root: &Path,
    pin: &CentralAgentSelfSourcePin,
) -> Result<ResolvedIdentitySource, IdentitySourceError> {
    let reference = pin.reference.as_str().to_owned();
    let relative = parse_source_reference(&reference)?;
    let path = world_root.join(&relative);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(IdentitySourceError::Missing { reference })
        }
        Err(e) => {
            return Err(IdentitySourceError::Unreadable {
                reference,
                reason: e.to_string(),
            })
        }
    };
    // A final-component symlink is not the accepted file; neither is a
    // redirected ancestor, checked through the canonical-equality idiom the
    // Agency source basis uses.
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(IdentitySourceError::Withheld { reference });
    }
    let canonical = match path.canonicalize() {
        Ok(canonical) => canonical,
        Err(e) => {
            return Err(IdentitySourceError::Unreadable {
                reference,
                reason: e.to_string(),
            })
        }
    };
    if canonical != path {
        return Err(IdentitySourceError::Withheld { reference });
    }
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(e) => {
            return Err(IdentitySourceError::Unreadable {
                reference,
                reason: e.to_string(),
            })
        }
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(e) => {
            return Err(IdentitySourceError::Unreadable {
                reference,
                reason: e.to_string(),
            })
        }
    };
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(IdentitySourceError::Unreadable {
            reference,
            reason: format!("exceeds the {} byte source bound", MAX_SOURCE_BYTES),
        });
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    if let Err(e) = (&mut file)
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
    {
        return Err(IdentitySourceError::Unreadable {
            reference,
            reason: e.to_string(),
        });
    }
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(IdentitySourceError::Unreadable {
            reference,
            reason: format!("exceeds the {} byte source bound", MAX_SOURCE_BYTES),
        });
    }
    if bytes.contains(&0) {
        return Err(IdentitySourceError::Unreadable {
            reference,
            reason: "contains NUL bytes; a source is UTF-8 text".to_string(),
        });
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => {
            return Err(IdentitySourceError::Unreadable {
                reference,
                reason: format!("not valid UTF-8: {e}"),
            })
        }
    };
    let actual = format!("sha256:{}", hex_lowercase(&Sha256::digest(text.as_bytes())));
    if pin.content_digest != actual {
        return Err(IdentitySourceError::DigestMismatch {
            reference,
            expected: pin.content_digest.clone(),
            actual,
        });
    }
    Ok(ResolvedIdentitySource {
        reference,
        digest: actual,
        text,
    })
}

/// Parse `central:source:{world_ref}:{relative path}` into the relative path
/// under the world root. A malformed reference is a defensiveness refusal at
/// this layer; profile intake already validates the shape authoritatively.
fn parse_source_reference(reference: &str) -> Result<PathBuf, IdentitySourceError> {
    let malformed = |reason: String| IdentitySourceError::Unreadable {
        reference: reference.to_owned(),
        reason,
    };
    if reference
        .chars()
        .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(malformed(
            "the reference must not contain whitespace or control characters".to_string(),
        ));
    }
    let Some(rest) = reference.strip_prefix(CENTRAL_SOURCE_REF_PREFIX) else {
        return Err(malformed(format!(
            "the reference must start with {CENTRAL_SOURCE_REF_PREFIX:?}"
        )));
    };
    // Mirror Central's own grammar (ctrl parse_self_source_reference): the
    // world ref is everything before the LAST colon; a path colon is escaped
    // as %3A at mint time, so rsplit is unambiguous.
    let Some((world_ref, relative)) = rest.rsplit_once(':') else {
        return Err(malformed(
            "the reference must be central:source:{world_ref}:{relative path}".to_string(),
        ));
    };
    if world_ref.is_empty() || relative.is_empty() {
        return Err(malformed(
            "the reference must name a non-empty world ref and relative path".to_string(),
        ));
    }
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || !relative_path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(malformed(
            "the reference path must be a plain relative path under the world root".to_string(),
        ));
    }
    Ok(relative_path.to_path_buf())
}

fn hex_lowercase(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Compose the deterministic operative-orientation wrapper around the exact
/// accepted source bytes. The same agent, profile revision and resolved bytes
/// always produce byte-identical output, so a Direct parent prompt and an
/// Agency turn carry the same section for the same accepted source.
pub fn compose_operative_orientation(
    agent_ref: &str,
    profile_ref: &str,
    profile_revision: &str,
    resolved: &ResolvedIdentitySources,
) -> String {
    format!(
        "# Operative orientation of the selected Agent\n\n\
        The following is the first-person orientation of the agent selected for this session \
        ({agent_ref}; Central profile {profile_ref} revision {profile_revision}, human-accepted \
        exact source). Enact it in attention, judgement and expression. Its descriptions organise \
        the agent's acts; the native host continues to supply actual capabilities, permissions, \
        current World and available continuity. Interpret the present request through this whole \
        orientation. \"I\" in these sources is the selected Agent — not the model, a temporary \
        role, or the author of any retrieved document.\n\n\
        {self_text}\n\n---\n\n{logos_text}",
        self_text = resolved.self_source.text,
        logos_text = resolved.relational_logos.text,
    )
}

/// The evidence a composition actually happened: which accepted sources were
/// verified and how the result was delivered. Serialized into delivery
/// evidence; it never contains the source text itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityCompositionReceipt {
    pub compositor_version: String,
    pub agent_ref: String,
    pub profile_ref: String,
    pub profile_revision: String,
    pub self_source_ref: String,
    pub self_source_digest: String,
    pub relational_logos_ref: String,
    pub relational_logos_digest: String,
    pub delivery_mode: String,
}

impl IdentityCompositionReceipt {
    pub fn new(
        agent_ref: &str,
        profile_ref: &str,
        profile_revision: &str,
        resolved: &ResolvedIdentitySources,
        delivery_mode: &str,
    ) -> Self {
        Self {
            compositor_version: IDENTITY_COMPOSITOR_VERSION.to_string(),
            agent_ref: agent_ref.to_string(),
            profile_ref: profile_ref.to_string(),
            profile_revision: profile_revision.to_string(),
            self_source_ref: resolved.self_source.reference.clone(),
            self_source_digest: resolved.self_source.digest.clone(),
            relational_logos_ref: resolved.relational_logos.reference.clone(),
            relational_logos_digest: resolved.relational_logos.digest.clone(),
            delivery_mode: delivery_mode.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::ResourceRef;
    use serde_json::json;

    fn pin(reference: &str, digest: &str) -> CentralAgentSelfSourcePin {
        CentralAgentSelfSourcePin {
            reference: ResourceRef::parse(reference).unwrap(),
            content_digest: digest.to_string(),
        }
    }

    fn sha256_of(bytes: &[u8]) -> String {
        format!("sha256:{}", hex_lowercase(&Sha256::digest(bytes)))
    }

    fn self_definition(self_digest: &str, logos_digest: &str) -> CentralAgentSelfDefinition {
        CentralAgentSelfDefinition {
            source: pin(
                "central:source:control:root:Control/agents/self/definition.md",
                self_digest,
            ),
            relational_logos: pin(
                "central:source:control:root:Control/agents/self/logos.md",
                logos_digest,
            ),
        }
    }

    /// A canonical temp Central root with both pinned sources on disk.
    fn fixture(bytes: &[u8], logos: &[u8]) -> (tempfile::TempDir, CentralAgentSelfDefinition) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("Control/agents/self")).unwrap();
        std::fs::write(root.join("Control/agents/self/definition.md"), bytes).unwrap();
        std::fs::write(root.join("Control/agents/self/logos.md"), logos).unwrap();
        let definition = self_definition(&sha256_of(bytes), &sha256_of(logos));
        (temp, definition)
    }

    #[test]
    fn composition_embeds_the_exact_accepted_source_bytes() {
        let self_bytes =
            b"# My standing\n\nI keep the World's registers honest, in the owner's voice.\n";
        let logos_bytes = b"You act from within a relation.\n";
        let (temp, definition) = fixture(self_bytes, logos_bytes);
        let root = temp.path().canonicalize().unwrap();
        let resolved = resolve_self_sources(&root, &definition).unwrap();
        assert_eq!(
            resolved.self_source.text,
            String::from_utf8_lossy(self_bytes)
        );
        assert_eq!(
            resolved.relational_logos.text,
            String::from_utf8_lossy(logos_bytes)
        );
        assert_eq!(resolved.self_source.digest, sha256_of(self_bytes));

        let composed =
            compose_operative_orientation("agent:test", "profile/central/test", "r1", &resolved);
        // The exact deterministic wrapper, spelled out independently of the
        // implementation, with the source bytes embedded verbatim in order.
        let expected = format!(
            "{}\n\n{}\n\n{}\n\n---\n\n{}",
            "# Operative orientation of the selected Agent",
            "The following is the first-person orientation of the agent selected for this session (agent:test; Central profile profile/central/test revision r1, human-accepted exact source). Enact it in attention, judgement and expression. Its descriptions organise the agent's acts; the native host continues to supply actual capabilities, permissions, current World and available continuity. Interpret the present request through this whole orientation. \u{22}I\u{22} in these sources is the selected Agent \u{2014} not the model, a temporary role, or the author of any retrieved document.",
            String::from_utf8_lossy(self_bytes),
            String::from_utf8_lossy(logos_bytes),
        );
        assert_eq!(composed, expected);
        // Determinism: the same inputs compose byte-identical output.
        let again = compose_operative_orientation(
            "agent:test",
            "profile/central/test",
            "r1",
            &resolve_self_sources(&root, &definition).unwrap(),
        );
        assert_eq!(composed, again);
    }

    #[test]
    fn a_changed_source_is_refused_by_digest() {
        let (temp, definition) = fixture(b"accepted bytes\n", b"logos\n");
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(
            root.join("Control/agents/self/definition.md"),
            b"changed bytes\n",
        )
        .unwrap();
        let error = resolve_self_sources(&root, &definition).unwrap_err();
        match error {
            IdentitySourceError::DigestMismatch {
                ref reference,
                ref expected,
                ref actual,
            } => {
                assert!(reference.ends_with("definition.md"));
                assert_eq!(expected, &definition.source.content_digest);
                assert_eq!(actual, &sha256_of(b"changed bytes\n"));
                assert_ne!(expected, actual);
            }
            other => panic!("expected a digest mismatch, got {other:?}"),
        }
        // The route-scoped code is named, not generic.
        assert_eq!(
            error.clone().into_aikit(IdentitySourceRoute::Direct).code(),
            "direct_agent.self_source_digest_mismatch"
        );
        assert_eq!(
            error.into_aikit(IdentitySourceRoute::Agency).code(),
            "encounter.self_source_digest_mismatch"
        );
    }

    #[test]
    fn a_missing_source_is_refused_by_name() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let definition = self_definition(&sha256_of(b"x"), &sha256_of(b"y"));
        let error = resolve_self_sources(&root, &definition).unwrap_err();
        assert_eq!(
            error,
            IdentitySourceError::Missing {
                reference: definition.source.reference.as_str().to_owned(),
            }
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_symlinked_source_is_withheld_not_followed() {
        use std::os::unix::fs::symlink;
        let (temp, definition) = fixture(b"accepted bytes\n", b"logos\n");
        let root = temp.path().canonicalize().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let smuggled = outside.path().join("smuggled.md");
        std::fs::write(&smuggled, b"accepted bytes\n").unwrap();
        let target = root.join("Control/agents/self/definition.md");
        std::fs::remove_file(&target).unwrap();
        symlink(&smuggled, &target).unwrap();
        let error = resolve_self_sources(&root, &definition).unwrap_err();
        assert_eq!(
            error,
            IdentitySourceError::Withheld {
                reference: definition.source.reference.as_str().to_owned(),
            }
        );
    }

    #[test]
    fn a_reference_outside_the_world_root_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let mut definition = self_definition(&sha256_of(b"x"), &sha256_of(b"y"));
        definition.source.reference =
            ResourceRef::parse("central:source:control:root:../outside.md").unwrap();
        let error = resolve_self_sources(&root, &definition).unwrap_err();
        assert!(
            matches!(error, IdentitySourceError::Unreadable { .. }),
            "a traversing path is refused: {error:?}"
        );
    }

    /// The receipt serialises the verified pins and the delivery mode — the
    /// evidence shape both routes record.
    #[test]
    fn the_receipt_serialises_for_evidence() {
        let (temp, definition) = fixture(b"accepted bytes\n", b"logos\n");
        let root = temp.path().canonicalize().unwrap();
        let resolved = resolve_self_sources(&root, &definition).unwrap();
        let receipt = IdentityCompositionReceipt::new(
            "agent:test",
            "profile/central/test",
            "r1",
            &resolved,
            DIRECT_DELIVERY_MODE,
        );
        let value = serde_json::to_value(&receipt).unwrap();
        assert_eq!(
            value,
            json!({
                "compositor_version": IDENTITY_COMPOSITOR_VERSION,
                "agent_ref": "agent:test",
                "profile_ref": "profile/central/test",
                "profile_revision": "r1",
                "self_source_ref": definition.source.reference.as_str(),
                "self_source_digest": receipt.self_source_digest,
                "relational_logos_ref": definition.relational_logos.reference.as_str(),
                "relational_logos_digest": receipt.relational_logos_digest,
                "delivery_mode": DIRECT_DELIVERY_MODE,
            })
        );
    }
}
