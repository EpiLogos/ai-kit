//! Provider-side A2A Agent Card projection.
//!
//! [`crate::a2a`] validates a card someone else published. This module builds
//! the card an Agent publishes — as a *projection* of the native
//! AgentWorldParticipation reading, never as a second identity:
//!
//! ```text
//!                   AGENT 0/1
//!                      │
//!          AgentWorldParticipation (O:I composes; owners hold state)
//!             ┌────────┴────────┐
//!   human Agent Card      A2A Agent Card  ← this module
//! ```
//!
//! Target contract: A2A v1.0.1 (`a2aproject/A2A` `specification/a2a.proto`).
//! Required card fields: `name`, `description`, `supportedInterfaces[]`
//! (`url`, `protocolBinding`, `protocolVersion`), `version`, `capabilities`,
//! `defaultInputModes`, `defaultOutputModes`, `skills[]` (`id`, `name`,
//! `description`, `tags`). `provider` when present needs `url` and
//! `organization`. Discovery is `/.well-known/agent-card.json`.
//!
//! Only the participation's **public** capabilities become A2A `skills`.
//! Internal repertoire is not public disclosure: a carried SkillSet member that
//! the participation does not disclose publicly never reaches the card. The
//! authenticated extended card is declared only when the Agent actually
//! serves one.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::a2a::{public_url, A2A_PROTOCOL_BINDING, A2A_PROTOCOL_VERSION};
use crate::{AikitError, Result};

/// The released A2A specification this projection targets.
pub const A2A_CARD_SPEC_VERSION: &str = "1.0.1";
/// The discovery path for a served card.
pub const A2A_WELL_KNOWN_AGENT_CARD_PATH: &str = "/.well-known/agent-card.json";
/// The participation reading a card is projected from.
pub const AGENT_WORLD_PARTICIPATION_SCHEMA: &str = "oi.agent-world-participation/v1";
/// Extension carried on the card so a peer can see which World relation and
/// which native Agent the card projects, without the card claiming identity.
pub const WORLD_PARTICIPATION_EXTENSION_URI: &str =
    "https://epi-logos.dev/a2a/extensions/world-participation/v1";

/// One capability the participation discloses publicly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicCapability {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub input_modes: Vec<String>,
    #[serde(default)]
    pub output_modes: Vec<String>,
}

/// Everything the card needs, already reduced to public facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct A2aCardProjectionInput {
    pub agent_ref: String,
    pub world_ref: String,
    pub participation_ref: Option<String>,
    pub name: String,
    pub description: String,
    pub version: String,
    pub interface_url: String,
    #[serde(default = "default_binding")]
    pub protocol_binding: String,
    pub provider_organization: Option<String>,
    pub provider_url: Option<String>,
    pub documentation_url: Option<String>,
    pub icon_url: Option<String>,
    pub public_capabilities: Vec<PublicCapability>,
    #[serde(default)]
    pub streaming: bool,
    /// True only when an authenticated extended card is actually served.
    #[serde(default)]
    pub extended_card_served: bool,
    #[serde(default)]
    pub security_schemes: Option<Value>,
    #[serde(default)]
    pub security_requirements: Vec<Value>,
}

fn default_binding() -> String {
    A2A_PROTOCOL_BINDING.to_string()
}

const DEFAULT_MODES: [&str; 2] = ["text/plain", "application/json"];

/// Reduce an `oi.agent-world-participation/v1` reading to the public facts a
/// card may carry. Anything the reading does not disclose publicly stays out.
pub fn projection_input_from_participation(
    participation: &Value,
    interface_url: &str,
) -> Result<A2aCardProjectionInput> {
    let object = participation.as_object().ok_or_else(|| {
        AikitError::new(
            "a2a_card.participation_invalid",
            "participation reading must be a JSON object",
        )
    })?;
    let schema = object
        .get("schema")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if schema != AGENT_WORLD_PARTICIPATION_SCHEMA {
        return Err(AikitError::new(
            "a2a_card.participation_schema",
            format!("expected {AGENT_WORLD_PARTICIPATION_SCHEMA}, found `{schema}`"),
        ));
    }
    let text = |path: &[&str]| -> Option<String> {
        let mut value = participation;
        for key in path {
            value = value.get(*key)?;
        }
        value
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
    };
    let agent_ref = text(&["agent_ref"]).ok_or_else(|| missing("agent_ref"))?;
    let world_ref = text(&["world_ref"]).ok_or_else(|| missing("world_ref"))?;
    let name = text(&["expression", "name"])
        .or_else(|| text(&["profile", "name"]))
        .unwrap_or_else(|| agent_ref.clone());
    let description = text(&["expression", "purpose"]).ok_or_else(|| {
        AikitError::new(
            "a2a_card.description_missing",
            "the participation discloses no purpose; an A2A card requires a description",
        )
    })?;
    let version = text(&["profile", "revision"]).unwrap_or_else(|| "r0".into());
    let public = participation
        .get("public_capabilities")
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    let public_capabilities: Vec<PublicCapability> =
        serde_json::from_value(public).map_err(|error| {
            AikitError::new(
                "a2a_card.public_capabilities_invalid",
                format!("public_capabilities are not readable: {error}"),
            )
        })?;
    let extended_card_served = participation
        .pointer("/disclosure/extended_card_served")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(A2aCardProjectionInput {
        agent_ref,
        world_ref,
        participation_ref: text(&["participation_ref"]),
        name,
        description,
        version,
        interface_url: interface_url.to_string(),
        protocol_binding: A2A_PROTOCOL_BINDING.to_string(),
        provider_organization: text(&["disclosure", "provider", "organization"]),
        provider_url: text(&["disclosure", "provider", "url"]),
        documentation_url: text(&["disclosure", "documentation_url"]),
        icon_url: text(&["disclosure", "icon_url"]),
        public_capabilities,
        streaming: false,
        extended_card_served,
        security_schemes: participation
            .pointer("/disclosure/security_schemes")
            .cloned(),
        security_requirements: participation
            .pointer("/disclosure/security_requirements")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    })
}

fn missing(field: &str) -> AikitError {
    AikitError::new(
        "a2a_card.participation_field_missing",
        format!("participation reading lacks `{field}`"),
    )
    .with("field", field)
}

/// Build the A2A v1.0.1 Agent Card and validate it before returning.
pub fn project_a2a_agent_card(input: &A2aCardProjectionInput) -> Result<Value> {
    let url = public_url(&input.interface_url, "A2A AgentInterface.url")?;
    let modes: Vec<&str> = DEFAULT_MODES.to_vec();
    let skills: Vec<Value> = input
        .public_capabilities
        .iter()
        .map(|capability| {
            let mut skill = Map::new();
            skill.insert("id".into(), json!(capability.id));
            skill.insert("name".into(), json!(capability.name));
            skill.insert("description".into(), json!(capability.description));
            skill.insert("tags".into(), json!(capability.tags));
            if !capability.examples.is_empty() {
                skill.insert("examples".into(), json!(capability.examples));
            }
            if !capability.input_modes.is_empty() {
                skill.insert("inputModes".into(), json!(capability.input_modes));
            }
            if !capability.output_modes.is_empty() {
                skill.insert("outputModes".into(), json!(capability.output_modes));
            }
            Value::Object(skill)
        })
        .collect();

    let mut params = Map::new();
    params.insert("agentRef".into(), json!(input.agent_ref));
    params.insert("worldRef".into(), json!(input.world_ref));
    if let Some(reference) = &input.participation_ref {
        params.insert("participationRef".into(), json!(reference));
    }
    let mut capabilities = Map::new();
    capabilities.insert("streaming".into(), json!(input.streaming));
    capabilities.insert("pushNotifications".into(), json!(false));
    capabilities.insert(
        "extendedAgentCard".into(),
        json!(input.extended_card_served),
    );
    capabilities.insert(
        "extensions".into(),
        json!([{
            "uri": WORLD_PARTICIPATION_EXTENSION_URI,
            "description": "The native World participation this card projects. The card is not the Agent's identity.",
            "required": false,
            "params": Value::Object(params),
        }]),
    );

    let mut card = Map::new();
    card.insert("name".into(), json!(input.name));
    card.insert("description".into(), json!(input.description));
    card.insert(
        "supportedInterfaces".into(),
        json!([{
            "url": url,
            "protocolBinding": input.protocol_binding,
            "protocolVersion": A2A_PROTOCOL_VERSION,
        }]),
    );
    if let (Some(organization), Some(provider_url)) =
        (&input.provider_organization, &input.provider_url)
    {
        card.insert(
            "provider".into(),
            json!({"organization": organization, "url": provider_url}),
        );
    }
    card.insert("version".into(), json!(input.version));
    if let Some(documentation) = &input.documentation_url {
        card.insert("documentationUrl".into(), json!(documentation));
    }
    if let Some(icon) = &input.icon_url {
        card.insert("iconUrl".into(), json!(icon));
    }
    card.insert("capabilities".into(), Value::Object(capabilities));
    if let Some(schemes) = &input.security_schemes {
        card.insert("securitySchemes".into(), schemes.clone());
    }
    if !input.security_requirements.is_empty() {
        card.insert(
            "securityRequirements".into(),
            Value::Array(input.security_requirements.clone()),
        );
    }
    card.insert("defaultInputModes".into(), json!(modes));
    card.insert("defaultOutputModes".into(), json!(modes));
    card.insert("skills".into(), Value::Array(skills));

    let card = Value::Object(card);
    validate_a2a_agent_card(&card)?;
    Ok(card)
}

/// Check a card against the A2A v1.0.1 required-field rules. Used on every
/// card this module produces, and usable on any card.
pub fn validate_a2a_agent_card(card: &Value) -> Result<()> {
    let object = card
        .as_object()
        .ok_or_else(|| invalid("A2A Agent Card must be an object", "card"))?;
    for field in ["name", "description", "version"] {
        non_empty(object.get(field), field)?;
    }
    let interfaces = object
        .get("supportedInterfaces")
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
        .ok_or_else(|| {
            invalid(
                "supportedInterfaces must be a non-empty array",
                "supportedInterfaces",
            )
        })?;
    for interface in interfaces {
        for field in ["url", "protocolBinding", "protocolVersion"] {
            non_empty(
                interface.get(field),
                &format!("supportedInterfaces[].{field}"),
            )?;
        }
        let binding = interface["protocolBinding"].as_str().unwrap_or_default();
        if !["JSONRPC", "GRPC", "HTTP+JSON"].contains(&binding) {
            return Err(invalid(
                "protocolBinding must be JSONRPC, GRPC or HTTP+JSON",
                "supportedInterfaces[].protocolBinding",
            ));
        }
    }
    if let Some(provider) = object.get("provider") {
        non_empty(provider.get("organization"), "provider.organization")?;
        non_empty(provider.get("url"), "provider.url")?;
    }
    if !object.get("capabilities").is_some_and(Value::is_object) {
        return Err(invalid("capabilities must be an object", "capabilities"));
    }
    for field in ["defaultInputModes", "defaultOutputModes"] {
        let modes = object
            .get(field)
            .and_then(Value::as_array)
            .filter(|list| !list.is_empty())
            .ok_or_else(|| invalid("input/output modes must be a non-empty array", field))?;
        for mode in modes {
            non_empty(Some(mode), field)?;
        }
    }
    let skills = object
        .get("skills")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("skills must be an array", "skills"))?;
    let mut ids = std::collections::BTreeSet::new();
    for skill in skills {
        for field in ["id", "name", "description"] {
            non_empty(skill.get(field), &format!("skills[].{field}"))?;
        }
        if !skill.get("tags").is_some_and(Value::is_array) {
            return Err(invalid("skills[].tags must be an array", "skills[].tags"));
        }
        if !ids.insert(skill["id"].as_str().unwrap_or_default().to_string()) {
            return Err(invalid("skills[].id must be unique", "skills[].id"));
        }
    }
    // Retired v0.3 fields must not appear on a v1 card.
    for retired in [
        "url",
        "protocolVersion",
        "preferredTransport",
        "additionalInterfaces",
        "supportsAuthenticatedExtendedCard",
        "security",
    ] {
        if object.contains_key(retired) {
            return Err(invalid(
                "a v1 Agent Card must not carry retired v0.3 fields",
                retired,
            ));
        }
    }
    Ok(())
}

fn non_empty(value: Option<&Value>, field: &str) -> Result<()> {
    match value.and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() => Ok(()),
        _ => Err(invalid("must be a non-empty string", field)),
    }
}

fn invalid(message: &str, field: &str) -> AikitError {
    AikitError::new(
        "a2a_card.invalid",
        format!("A2A Agent Card {field}: {message}"),
    )
    .with("field", field)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a::{assert_a2a_card, create_a2a_binding, A2aBindingInput, A2aProvenanceEntry};

    fn participation() -> Value {
        json!({
            "schema": AGENT_WORLD_PARTICIPATION_SCHEMA,
            "participation_ref": "oi:participation:agent/factory-builder@project:Factory",
            "agent_ref": "agent/factory-builder",
            "world_ref": "project:Factory",
            "profile": {"ref": "profile/factory-builder", "revision": "r3", "name": "Factory builder"},
            "expression": {"name": "Factory builder", "purpose": "Develop Factory capabilities from authored intent"},
            "repertoire": {
                "skill_sets": ["central:core-development", "central:documentation"],
                "praxis": [
                    {"id": "skill/personal/wayfinder", "form": "methodology"},
                    {"id": "skill/central/ui-development", "form": "method"},
                    {"id": "skill/personal/grilling", "form": "method"}
                ]
            },
            "public_capabilities": [
                {"id": "ui-development", "name": "UI development",
                 "description": "Carry a UI change from Design through Mockup to verified implementation.",
                 "tags": ["documentation", "ui"]}
            ],
            "disclosure": {"public_basis": "profile-declared", "extended_card_served": false,
                           "provider": {"organization": "Epi-Logos", "url": "https://epi-logos.dev"}}
        })
    }

    #[test]
    fn card_projects_only_public_capabilities() {
        let input = projection_input_from_participation(
            &participation(),
            "https://agents.example/factory-builder/a2a",
        )
        .unwrap();
        let card = project_a2a_agent_card(&input).unwrap();
        let skills = card["skills"].as_array().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0]["id"], "ui-development");
        // Internal repertoire richer than public disclosure stays internal.
        let text = card.to_string();
        assert!(!text.contains("skill/personal/grilling"));
        assert!(!text.contains("central:core-development"));
        assert_eq!(card["capabilities"]["extendedAgentCard"], false);
        assert_eq!(card["version"], "r3");
        assert_eq!(card["provider"]["organization"], "Epi-Logos");
        assert_eq!(
            card["capabilities"]["extensions"][0]["params"]["worldRef"],
            "project:Factory"
        );
    }

    #[test]
    fn projected_card_is_accepted_by_the_consumer_side_check() {
        let input = projection_input_from_participation(
            &participation(),
            "https://agents.example/factory-builder/a2a",
        )
        .unwrap();
        let card = project_a2a_agent_card(&input).unwrap();
        let binding = create_a2a_binding(A2aBindingInput {
            binding_ref: "oi:a2a-binding:factory-builder".into(),
            binding_revision: 1,
            field_ref: "oi:field:factory".into(),
            participant_ref: "oi:participant:factory-builder".into(),
            agent_ref: "agent/factory-builder".into(),
            publisher_participant_ref: "oi:participant:owner".into(),
            publication_decision_ref: "oi:decision:publish-factory-builder".into(),
            source_revision: "r3".into(),
            projection_ref: None,
            published_at: "2026-09-24T12:00:00Z".into(),
            state: Default::default(),
            protocol_version: A2A_PROTOCOL_VERSION.into(),
            protocol_binding: A2A_PROTOCOL_BINDING.into(),
            endpoint_url: Some("https://agents.example/factory-builder/a2a".into()),
            agent_card_url: Some(format!(
                "https://agents.example{A2A_WELL_KNOWN_AGENT_CARD_PATH}"
            )),
            provenance: vec![A2aProvenanceEntry {
                kind: "participation".into(),
                reference: "oi:participation:agent/factory-builder@project:Factory".into(),
                source_system: "oi".into(),
                extra: Map::new(),
            }],
        })
        .unwrap();
        let selected = assert_a2a_card(&card, &binding).unwrap();
        assert_eq!(selected["protocolVersion"], "1.0");
    }

    #[test]
    fn an_agent_with_no_public_disclosure_publishes_no_skills() {
        let mut reading = participation();
        reading["public_capabilities"] = json!([]);
        let input =
            projection_input_from_participation(&reading, "https://agents.example/a2a").unwrap();
        let card = project_a2a_agent_card(&input).unwrap();
        assert_eq!(card["skills"], json!([]));
    }

    #[test]
    fn near_misses_are_refused() {
        let mut reading = participation();
        reading["schema"] = json!("oi.human-agent-card/v1");
        assert!(projection_input_from_participation(&reading, "https://a.example").is_err());

        let mut reading = participation();
        reading["expression"]["purpose"] = json!("");
        assert!(projection_input_from_participation(&reading, "https://a.example").is_err());

        let input =
            projection_input_from_participation(&participation(), "ftp://a.example").unwrap();
        assert!(project_a2a_agent_card(&input).is_err());

        let mut card = project_a2a_agent_card(
            &projection_input_from_participation(&participation(), "https://a.example").unwrap(),
        )
        .unwrap();
        card["url"] = json!("https://a.example");
        assert!(validate_a2a_agent_card(&card).is_err());
    }
}
