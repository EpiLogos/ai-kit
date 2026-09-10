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
}
