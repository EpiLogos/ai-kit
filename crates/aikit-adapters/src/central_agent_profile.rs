//! AIKit intake of the Central-owned `central.agent-profile/v1` object.
//!
//! Central owns the *authored* Agent profile: refs/intents only. AIKit consumes
//! it as refs and never re-authors identity, and never derives effective
//! Profile/ContextResolution state from it — that is AIKit's own resolution job.
//! Actuation still owns Agent/Agency semantics and authority; the authored
//! profile merely records how an already-existing Agent is intended to inhabit a
//! personal or Project World.

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::actuation_instantiation::CentralAuthoredProjection;

pub const CENTRAL_AGENT_PROFILE_SCHEMA: &str = "central.agent-profile/v1";
/// The provenance block Central stamps when a profile is authored from an
/// expressed intent (`ctrl/src/agent_profile.rs:34`).
pub const CENTRAL_AGENT_PROFILE_PROVENANCE_SCHEMA: &str = "central.agent-profile-provenance/v1";
/// The reference grammar a self-definition pin must use:
/// `central:source:{world_ref}:{relative path under the world root}`.
pub const SELF_SOURCE_REF_PREFIX: &str = "central:source:";

/// Authorship of an Agent profile. Central represents exactly one state: an
/// Action can only ever author a generated proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CentralAgentProfileAuthorship {
    GeneratedProposal,
}

/// Recognition standing of an authored AgentProfile record. The recognition
/// act is the human owner's and no Central Action performs it, so
/// `Unrecognised` is the only representable state — a record claiming
/// otherwise fails to parse rather than silently becoming an adopted default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CentralAgentProfileRecognition {
    Unrecognised,
}

/// Generated-proposal provenance retained verbatim through intake. The intent
/// expression is what the author expressed, never rewritten and never
/// summarised, and its standing travels with it: text without standing would
/// let an unrecognised proposal be read as an adopted default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CentralAgentProfileIntentProvenance {
    pub schema: String,
    pub intent_expression: String,
    pub origin_action: String,
    pub authorship: CentralAgentProfileAuthorship,
    pub recognition: CentralAgentProfileRecognition,
}

/// Authored residence scope: a source relation, not a runtime identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CentralAgentProfileScope {
    Personal,
    Project,
}

/// One exact-source pin of a self definition: the `central:source:` reference
/// and the sha256 digest of the exact UTF-8 file bytes it accepted. Central
/// owns the acceptance; AIKit only ever verifies the pinned bytes against it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CentralAgentSelfSourcePin {
    pub reference: ResourceRef,
    pub content_digest: String,
}

/// The optional `self_definition` block: the Agent's first-person orientation
/// sources, pinned exactly. Absent means the legacy behaviour — no composed
/// orientation rides either delivery route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CentralAgentSelfDefinition {
    pub source: CentralAgentSelfSourcePin,
    pub relational_logos: CentralAgentSelfSourcePin,
}

/// The full Central-authored AgentProfile, consumed as refs + intents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CentralAgentProfileProjection {
    pub schema: String,
    #[serde(rename = "ref")]
    pub profile_ref: ResourceRef,
    pub revision: String,
    pub agent_ref: ResourceRef,
    pub scope: CentralAgentProfileScope,
    pub world_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_profile_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default)]
    pub governance_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub skill_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub skill_set_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub method_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub routine_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub ratified_world_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub knowledge_source_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub computer_access_intent_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub placement_intent_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub provenance_refs: Vec<ResourceRef>,
    /// Present exactly when Central authored this profile from an expressed
    /// intent. Directly owner-authored profiles carry no block. Retaining it
    /// is what lets a later renderer reach the intent — rendering at the end
    /// cannot recover a field lost here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_provenance: Option<CentralAgentProfileIntentProvenance>,
    /// Central file ref to the reusable expressive character (an
    /// `oi.expression/v1` material document of reuse kind `character`) the
    /// Agent appears through. Carried as a ref for downstream disclosure; AIKit
    /// neither dereferences nor interprets the material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expressive_character_ref: Option<ResourceRef>,
    /// Optional exact-source pins of the Agent's first-person orientation.
    /// Tolerated absent (legacy profiles), and the projection never rejects
    /// unknown fields, so Central may extend the document forward-compatibly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_definition: Option<CentralAgentSelfDefinition>,
}

impl CentralAgentProfileProjection {
    /// Deserialize a raw `central.agent-profile/v1` value and validate it.
    pub fn parse(value: &Value) -> Result<Self> {
        let projection: Self = serde_json::from_value(value.clone()).map_err(|error| {
            AikitError::new(
                "central_agent_profile.parse",
                format!("could not read {CENTRAL_AGENT_PROFILE_SCHEMA}: {error}"),
            )
        })?;
        projection.validate()?;
        Ok(projection)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != CENTRAL_AGENT_PROFILE_SCHEMA {
            return Err(AikitError::new(
                "central_agent_profile.invalid_schema",
                format!(
                    "expected `{CENTRAL_AGENT_PROFILE_SCHEMA}`, got `{}`",
                    self.schema
                ),
            ));
        }
        if self.profile_ref.as_str().is_empty() {
            return Err(AikitError::new(
                "central_agent_profile.invalid_ref",
                "profile ref must not be empty",
            ));
        }
        if self.agent_ref.as_str().is_empty() {
            return Err(AikitError::new(
                "central_agent_profile.invalid_ref",
                "agent_ref must not be empty",
            ));
        }
        if self.revision.trim().is_empty() {
            return Err(AikitError::new(
                "central_agent_profile.invalid_ref",
                "revision must not be empty",
            ));
        }
        if self.agent_ref == self.profile_ref {
            return Err(AikitError::new(
                "central_agent_profile.identity_collapse",
                "agent_ref and profile ref must remain distinct",
            ));
        }
        if let Some(character) = &self.expressive_character_ref {
            if ResourceRef::parse(character.as_str()).is_err() {
                return Err(AikitError::new(
                    "central_agent_profile.invalid_ref",
                    "expressive_character_ref must be a non-empty, trimmed ref",
                ));
            }
        }
        if let Some(provenance) = &self.intent_provenance {
            if provenance.schema != CENTRAL_AGENT_PROFILE_PROVENANCE_SCHEMA {
                return Err(AikitError::new(
                    "central_agent_profile.invalid_provenance",
                    format!(
                        "expected `{CENTRAL_AGENT_PROFILE_PROVENANCE_SCHEMA}`, got `{}`",
                        provenance.schema
                    ),
                ));
            }
            // Verbatim retention: the intent is carried exactly as Central
            // stored it. Central trims at `agent-profile.propose` time (a raw
            // `.md` trailing newline is refused), so the recorded expression is
            // already trimmed — anything else is not Central's own record, and
            // carrying it would smuggle a second normalisation into intake.
            if provenance.intent_expression.trim().is_empty()
                || provenance.intent_expression != provenance.intent_expression.trim()
            {
                return Err(AikitError::new(
                    "central_agent_profile.invalid_intent_expression",
                    "the recorded intent expression must be non-empty and trimmed, as Central stores it",
                ));
            }
        }
        if let Some(self_definition) = &self.self_definition {
            validate_self_pin(&self_definition.source, "self_definition.source")?;
            validate_self_pin(
                &self_definition.relational_logos,
                "self_definition.relational_logos",
            )?;
        }
        Ok(())
    }

    /// The Central-authored slice of the composition: enduring Agent identity
    /// plus the authored profile/praxis refs. Host/machine identity is a separate
    /// Central relation and is supplied by the caller.
    pub fn authored_projection(&self) -> CentralAuthoredProjection {
        let mut profile_refs = vec![self.profile_ref.clone()];
        profile_refs.extend(self.skill_refs.iter().cloned());
        profile_refs.extend(self.skill_set_refs.iter().cloned());
        profile_refs.extend(self.method_refs.iter().cloned());
        profile_refs.extend(self.governance_refs.iter().cloned());
        CentralAuthoredProjection {
            agent_ref: Some(self.agent_ref.clone()),
            host_ref: None,
            profile_refs,
            profile_source: Some(self.clone()),
        }
    }
}

/// A self-definition pin must reference an exact source under a World root and
/// pin it by sha256: `central:source:{world_ref}:{relative path}`, a plain
/// relative path without whitespace or control characters, and
/// `sha256:` + 64 lowercase hexadecimal digits.
fn validate_self_pin(pin: &CentralAgentSelfSourcePin, field: &str) -> Result<()> {
    let reference = pin.reference.as_str();
    if reference.is_empty()
        || !reference.starts_with(SELF_SOURCE_REF_PREFIX)
        || reference
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
        // Central's grammar (ctrl parse_self_source_reference): the world ref
        // is everything before the last colon (path colons are escaped as
        // %3A at mint time) and both halves must be non-empty.
        || reference[SELF_SOURCE_REF_PREFIX.len()..]
            .rsplit_once(':')
            .is_none_or(|(world_ref, relative)| world_ref.is_empty() || relative.is_empty())
    {
        return Err(AikitError::new(
            "central_agent_profile.invalid_ref",
            format!("{field}.reference must be a {SELF_SOURCE_REF_PREFIX}{{world_ref}}:{{relative path}} ref"),
        ));
    }
    if ResourceRef::parse(reference).is_err() {
        return Err(AikitError::new(
            "central_agent_profile.invalid_ref",
            format!("{field}.reference must be a non-empty, trimmed ref"),
        ));
    }
    let digest = pin.content_digest.as_bytes();
    let valid_digest = digest.len() == "sha256:".len() + 64
        && digest.starts_with(b"sha256:")
        && digest["sha256:".len()..]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b));
    if !valid_digest {
        return Err(AikitError::new(
            "central_agent_profile.invalid_digest",
            format!("{field}.content_digest must be `sha256:` followed by 64 lowercase hexadecimal digits"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_value() -> Value {
        serde_json::json!({
            "schema": "central.agent-profile/v1",
            "ref": "profile/central/build",
            "revision": "r1",
            "agent_ref": "agent/mahamaya",
            "scope": "project",
            "world_ref": "world/central",
            "source_profile_ref": "profile/central/personal",
            "role": "build engineer",
            "purpose": "compose the Central ground for downstream resolution",
            "governance_refs": ["governance/central"],
            "skill_refs": ["skill/control-maintenance"],
            "skill_set_refs": ["skill-set/build"],
            "method_refs": ["method/control-maintenance"],
            "routine_refs": ["routine/central-doctor"],
            "ratified_world_refs": ["world/central"],
            "knowledge_source_refs": ["source/central"],
            "computer_access_intent_refs": ["computer-access-intent/central"],
            "placement_intent_refs": [],
            "provenance_refs": ["provenance/central-profile"]
        })
    }

    #[test]
    fn parses_central_profile_as_refs_and_intents() {
        let projection = CentralAgentProfileProjection::parse(&profile_value()).unwrap();
        assert_eq!(projection.profile_ref.as_str(), "profile/central/build");
        assert_eq!(projection.agent_ref.as_str(), "agent/mahamaya");
        assert_eq!(projection.scope, CentralAgentProfileScope::Project);
        assert_eq!(projection.role.as_deref(), Some("build engineer"));
        assert_eq!(projection.skill_set_refs.len(), 1);
    }

    #[test]
    fn authored_projection_maps_agent_and_praxis_refs_without_host() {
        let projection = CentralAgentProfileProjection::parse(&profile_value()).unwrap();
        let authored = projection.authored_projection();
        assert_eq!(
            authored.agent_ref,
            Some(ResourceRef::parse("agent/mahamaya").unwrap())
        );
        assert!(authored.host_ref.is_none());
        assert!(authored
            .profile_refs
            .iter()
            .any(|r| r.as_str() == "profile/central/build"));
        assert!(authored
            .profile_refs
            .iter()
            .any(|r| r.as_str() == "skill-set/build"));
        assert!(authored
            .profile_refs
            .iter()
            .any(|r| r.as_str() == "method/control-maintenance"));
    }

    #[test]
    fn rejects_wrong_schema_or_identity_collapse() {
        let mut wrong = profile_value();
        wrong["schema"] = serde_json::json!("central.agent-profile/v2");
        assert_eq!(
            CentralAgentProfileProjection::parse(&wrong)
                .unwrap_err()
                .code(),
            "central_agent_profile.invalid_schema"
        );

        let mut collapsed = profile_value();
        collapsed["agent_ref"] = serde_json::json!("profile/central/build");
        assert_eq!(
            CentralAgentProfileProjection::parse(&collapsed)
                .unwrap_err()
                .code(),
            "central_agent_profile.identity_collapse"
        );
    }

    #[test]
    fn carries_expressive_character_ref_and_omits_it_when_absent() {
        let plain = CentralAgentProfileProjection::parse(&profile_value()).unwrap();
        assert!(plain.expressive_character_ref.is_none());
        assert!(serde_json::to_value(&plain)
            .unwrap()
            .get("expressive_character_ref")
            .is_none());

        let character = "central:Control/agents/expressive-material/character/nous.expression.json";
        let mut value = profile_value();
        value["expressive_character_ref"] = serde_json::json!(character);
        let projection = CentralAgentProfileProjection::parse(&value).unwrap();
        assert_eq!(
            projection
                .expressive_character_ref
                .as_ref()
                .map(ResourceRef::as_str),
            Some(character)
        );
        // The authored slice keeps the full source, so disclosure reaches it.
        let authored = projection.authored_projection();
        assert_eq!(
            authored
                .profile_source
                .as_ref()
                .and_then(|source| source.expressive_character_ref.as_ref())
                .map(ResourceRef::as_str),
            Some(character)
        );
        assert_eq!(
            serde_json::to_value(&projection).unwrap()["expressive_character_ref"],
            character
        );

        let mut untrimmed = profile_value();
        untrimmed["expressive_character_ref"] = serde_json::json!(" central:x");
        assert_eq!(
            CentralAgentProfileProjection::parse(&untrimmed)
                .unwrap_err()
                .code(),
            "central_agent_profile.invalid_ref"
        );
    }

    /// The generated-proposal block — verbatim intent plus its standing —
    /// survives intake, and the standing is not forgeable.
    #[test]
    fn retains_intent_provenance_verbatim_and_refuses_recognition_overclaims() {
        // As Central stores it: `agent-profile.propose` trims the expression,
        // so the recorded value keeps its internal newlines and no outer
        // whitespace.
        let intent = "# Oh, I!\n\nA Central needs a basic default agent.";
        let mut value = profile_value();
        value["intent_provenance"] = serde_json::json!({
            "schema": "central.agent-profile-provenance/v1",
            "intent_expression": intent,
            "origin_action": "agent-profile.propose",
            "authorship": "generated-proposal",
            "recognition": "unrecognised",
        });
        let projection = CentralAgentProfileProjection::parse(&value).unwrap();
        let provenance = projection.intent_provenance.expect("retained");
        assert_eq!(provenance.intent_expression, intent);
        assert_eq!(
            provenance.recognition,
            CentralAgentProfileRecognition::Unrecognised
        );

        // A record claiming recognition the human has not performed is
        // unparseable rather than silently adopted.
        let mut overclaim = value.clone();
        overclaim["intent_provenance"]["recognition"] = serde_json::json!("recognised");
        assert_eq!(
            CentralAgentProfileProjection::parse(&overclaim)
                .unwrap_err()
                .code(),
            "central_agent_profile.parse"
        );

        // Retention is byte-for-byte: a whitespace-mangled expression is not
        // the record Central authored and is refused, never quietly trimmed.
        let mut mangled = value;
        mangled["intent_provenance"]["intent_expression"] = serde_json::json!("trailing space ");
        assert_eq!(
            CentralAgentProfileProjection::parse(&mangled)
                .unwrap_err()
                .code(),
            "central_agent_profile.invalid_intent_expression"
        );
    }

    /// The optional `self_definition` block round-trips exactly, is absent
    /// (never defaulted into existence) for legacy profiles, and refuses pins
    /// that are not an exact `central:source:` + sha256 shape.
    #[test]
    fn carries_self_definition_pins_and_refuses_malformed_ones() {
        let digest = format!("sha256:{}", "ab".repeat(32));
        let mut value = profile_value();
        value["self_definition"] = serde_json::json!({
            "source": {
                "reference": "central:source:control:root:Control/agents/self/definition.md",
                "content_digest": digest,
            },
            "relational_logos": {
                "reference": "central:source:central:root:Control/agents/governance/being-there/relational-logos.md",
                "content_digest": digest,
            },
        });
        let projection = CentralAgentProfileProjection::parse(&value).unwrap();
        let self_definition = projection.self_definition.clone().expect("carried");
        assert_eq!(
            self_definition.source.reference.as_str(),
            "central:source:control:root:Control/agents/self/definition.md"
        );
        assert_eq!(self_definition.source.content_digest, digest);
        // Round trip: the parsed projection serialises the pins verbatim.
        let round_tripped: CentralAgentProfileProjection =
            serde_json::from_value(serde_json::to_value(&projection).unwrap()).unwrap();
        assert_eq!(round_tripped, projection);

        // Legacy behaviour: absent stays absent, on parse and on write.
        let plain = CentralAgentProfileProjection::parse(&profile_value()).unwrap();
        assert!(plain.self_definition.is_none());
        assert!(serde_json::to_value(&plain)
            .unwrap()
            .get("self_definition")
            .is_none());
        // Forward compatibility: unknown fields in the document are not a
        // parse refusal.
        let mut extended = value.clone();
        extended["a_future_field"] = serde_json::json!({"anything": true});
        assert!(CentralAgentProfileProjection::parse(&extended).is_ok());

        // A pin that is not the exact accepted-source shape is refused.
        for (field, bad) in [
            (
                "reference",
                serde_json::json!("Control/agents/self/definition.md"),
            ),
            ("reference", serde_json::json!("central:source:no-path")),
            (
                "reference",
                serde_json::json!("central:source:control:root: has space.md"),
            ),
            ("content_digest", serde_json::json!("blake3:deadbeef")),
            ("content_digest", serde_json::json!("sha256:ABCDEF")),
            (
                "content_digest",
                serde_json::json!(format!("sha256:{}", "g".repeat(64))),
            ),
        ] {
            let mut bad_pin = value.clone();
            bad_pin["self_definition"]["source"][field] = bad;
            let error = CentralAgentProfileProjection::parse(&bad_pin).unwrap_err();
            assert!(
                matches!(
                    error.code(),
                    "central_agent_profile.invalid_ref" | "central_agent_profile.invalid_digest"
                ),
                "{field} case refused with a named code, got {error:?}"
            );
        }
    }
}
