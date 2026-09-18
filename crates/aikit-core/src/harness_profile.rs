//! The `aikit.harness-profile/v1` outline: one declarative document per
//! harness, joining the Actuation catalog record (what the harness *is*) to
//! AIKit handling (what we do about it). Sections are layers; every layer
//! records an ownership posture and, where AIKit writes, the native seam it
//! projects into.
//!
//! The invariant this module owns is *posture truth*. A layer is projected
//! only where the profile records a seam the harness truthfully re-reads
//! (`managed`, with a project declaration); a layer AIKit merely watches
//! (`observed`) or composes through disclosure only (`brokered`) refuses a
//! project declaration rather than silently acquiring write authority. The
//! schema key is exact: a document written against another contract version
//! is refused naming the expected and found versions, never reinterpreted.
//! The catalog join key (`slug`) must be present — a profile that cannot say
//! which harness it profiles joins nothing.
//!
//! Detection stays with Actuation: `presence` here echoes its probe grammar
//! and is not a second detector. A [`MergeGrammar`] names a grammar
//! implemented once by the adapters' layer merge engine; this module carries
//! the declaration, never the merge code. An [`ActivationEffectName`] mirrors
//! the variants of [`crate::projection::ActivationEffect`] so activation truth
//! can be verified against the existing `verify_activation_truth` law.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::harness_admission::HarnessEditionKind;

/// The exact schema key a harness-profile document must declare.
pub const HARNESS_PROFILE_SCHEMA: &str = "aikit.harness-profile/v1";

/// The ownership decision recorded per layer: who writes this layer and how
/// AIKit may take part. `managed` projects and retracts; `brokered` composes
/// through disclosure only; `observed` feeds detection and UI, writing
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayerPosture {
    Managed,
    Brokered,
    Observed,
}

impl fmt::Display for LayerPosture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let word = match self {
            LayerPosture::Managed => "managed",
            LayerPosture::Brokered => "brokered",
            LayerPosture::Observed => "observed",
        };
        f.write_str(word)
    }
}

/// The record grammar a layer's project declaration is rendered in. Each
/// grammar is implemented once by the adapters' layer merge engine; adding a
/// grammar there is the only way to name it here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergeGrammar {
    ClaudeHookMap,
    ZcodeHookWrapper,
    McpServersRecord,
    /// pi's `extensions` string array (settings `extensions`): the seam the
    /// extension carrier is registered through. One managed entry — the
    /// content-addressed carrier path — swept and re-added by ownership
    /// marker on re-projection; foreign extension paths preserved.
    PiExtensionsRecord,
}

/// How projected entries combine with entries the harness's other writers
/// made. `preserve-foreign-sweep-owned` is the only policy for now: entries
/// AIKit does not own are never touched, entries AIKit owns are replaced, and
/// owned entries whose capability source disappeared are swept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MergePolicy {
    PreserveForeignSweepOwned,
}

/// The activation truth of a managed layer, mirroring the variants of
/// [`crate::projection::ActivationEffect`] as declarative data so a profile
/// can be checked against an adapter's `verify_activation_truth` evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationEffectName {
    Immediate,
    LiveReloadExpected,
    RestartClient,
    NextSessionOnly,
    Brokered,
    Unsupported,
}

/// Where the harness shows up on a machine: an optional echo of the Actuation
/// probe grammar. Presence detection itself stays in Actuation; this records
/// the probes a profile is known to answer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HarnessPresence {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub executables: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_markers: Vec<String>,
}

/// Skill trees the harness reads natively, plus the render rules a managed
/// skills projection would use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SkillsLayer {
    pub posture: LayerPosture,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe: Option<SkillsObserveDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_root: Option<String>,
    /// Free-text disclosure of the harness's shared-tree posture. Deliberately
    /// prose, not an enum: the shared-tree policies harnesses actually have
    /// are one or two sentences each, not a closed set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_tree: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SkillsObserveDeclaration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// Guidance files (AGENTS.md, rules files) the harness reads. Detection only;
/// the projection of generated guidance stays with the existing generation
/// path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct GuidanceLayer {
    pub posture: LayerPosture,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observe: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HooksLayer {
    pub posture: LayerPosture,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observe: Vec<HookObserveDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<HookProjectDeclaration>,
    /// When the harness sees a projected hooks change, declared like the
    /// tools layer's. pi reads its extensions at session start, so the
    /// carrier declares `next-session-only` (a running TUI can /reload).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<ActivationEffectName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HookObserveDeclaration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transports: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HookProjectDeclaration {
    pub file: String,
    pub format: MergeGrammar,
    /// The identity AIKit's hook entries carry in the target file, so the
    /// merge engine can tell managed from foreign and sweep stale owned
    /// entries without touching anyone else's.
    pub ownership_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ToolsLayer {
    pub posture: LayerPosture,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observe: Vec<ToolObserveDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ToolProjectDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<ActivationEffectName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ToolObserveDeclaration {
    pub path: String,
    /// Dotted collection path of the MCP server records inside the config
    /// document, e.g. `mcpServers` or `mcp.servers`.
    pub collection: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ToolProjectDeclaration {
    pub file: String,
    pub key: String,
    pub format: MergeGrammar,
    pub merge: MergePolicy,
}

/// How models reach this harness. This records which selector surface the
/// harness natively exposes; it never declares provider availability, which
/// stays with the catalog and the roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelDispatchPosture {
    /// The harness binds one provider natively; `selector_kind` /
    /// `selector_name` name the surface (argv flag, config key, ACP model
    /// selector) through which a model is chosen.
    #[serde(rename_all = "kebab-case")]
    NativeProviderBinding {
        provider_ref: String,
        selector_kind: String,
        selector_name: String,
    },
    /// The harness is provider-plural: several providers coexist and
    /// encounter-level selection decides, per the model dispatch policy.
    ProviderPlural,
    /// The catalog declares no native provider binding for this harness; the
    /// reason is carried because "none" without one is not a position.
    #[serde(rename_all = "kebab-case")]
    None { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ModelsLayer {
    pub posture: LayerPosture,
    pub dispatch: ModelDispatchPosture,
    /// Disclosed roster fact: what this harness demands of the model roster
    /// (`ModelRosterDemand.profile`). A note for the roster read model, never
    /// an availability claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roster_note: Option<String>,
    /// Disclosed roster fact: how candidates are gated for this harness
    /// (`harness_compatible` / `harness_capabilities`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility_note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionProtocol {
    Acp,
    Rpc,
    Process,
}

/// The harness's session faculties as plain flags, mirroring the shape of
/// aikit-adapters' `ConnectionCapabilities`. These are profile declarations
/// used for read models and refusal boundaries, not a negotiated capability
/// report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SessionCapabilityFlags {
    #[serde(default)]
    pub ordered_streaming: bool,
    #[serde(default)]
    pub cancellation: bool,
    #[serde(default)]
    pub permission_requests: bool,
    #[serde(default)]
    pub reconnect: bool,
    #[serde(default)]
    pub additional_directories: bool,
    #[serde(default)]
    pub mcp_servers: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SessionsLayer {
    pub posture: LayerPosture,
    pub protocol: SessionProtocol,
    /// The session-open modes the harness supports (e.g. `create`, `load`,
    /// `resume`, `attach`). Free strings so harness-specific mode names pass
    /// through undistorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_modes: Vec<String>,
    #[serde(default)]
    pub capabilities: SessionCapabilityFlags,
}

/// Install/config seams of the harness itself. Carried by the layers above;
/// this section records them where no layer-specific declaration fits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SettingsLayer {
    pub posture: LayerPosture,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observe: Vec<String>,
}

/// One `aikit.harness-profile/v1` document. Every layer section is optional;
/// a profile says only what is true of its harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessProfile {
    pub schema: String,
    /// Catalog join key: the Actuation catalog slug of the harness profiled.
    pub slug: String,
    pub edition: HarnessEditionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<HarnessPresence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<SkillsLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidance: Option<GuidanceLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HooksLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<ModelsLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<SessionsLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<SettingsLayer>,
}

/// A harness-profile validation failure. The message names the layer, the
/// field and the change that would fix it (STANDARDS §1); `code` is stable
/// machine surface in the `harness_profile.*` namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessProfileError {
    code: &'static str,
    message: String,
    details: BTreeMap<String, String>,
}

impl HarnessProfileError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: BTreeMap::new(),
        }
    }

    #[must_use]
    fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
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

impl fmt::Display for HarnessProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if !self.details.is_empty() {
            let rendered: Vec<String> = self
                .details
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect();
            write!(f, " ({})", rendered.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for HarnessProfileError {}

impl HarnessProfile {
    /// Check the document against posture truth: exact schema key, non-empty
    /// catalog join key, and — for every layer that declares one — a project
    /// declaration exactly when the posture is `managed`.
    pub fn validate(&self) -> Result<(), HarnessProfileError> {
        if self.schema != HARNESS_PROFILE_SCHEMA {
            return Err(HarnessProfileError::new(
                "harness_profile.schema_mismatch",
                format!(
                    "harness profile schema must be exactly {HARNESS_PROFILE_SCHEMA}, found {:?}; \
                     set the document's `schema` field to {HARNESS_PROFILE_SCHEMA}",
                    self.schema
                ),
            )
            .with("expected", HARNESS_PROFILE_SCHEMA)
            .with("found", self.schema.clone()));
        }
        if self.slug.trim().is_empty() {
            return Err(HarnessProfileError::new(
                "harness_profile.empty_slug",
                "harness profile `slug` must be a non-empty catalog join key; \
                 set `slug` to the harness's catalog slug (for example \"openclaw\")",
            )
            .with("field", "slug"));
        }
        if let Some(tools) = &self.tools {
            validate_layer_project("tools", tools.posture, tools.project.as_ref())?;
        }
        if let Some(hooks) = &self.hooks {
            validate_layer_project("hooks", hooks.posture, hooks.project.as_ref())?;
        }
        Ok(())
    }
}

fn validate_layer_project<T>(
    layer: &str,
    posture: LayerPosture,
    project: Option<&T>,
) -> Result<(), HarnessProfileError> {
    match posture {
        LayerPosture::Managed => {
            if project.is_none() {
                return Err(HarnessProfileError::new(
                    "harness_profile.managed_without_project",
                    format!(
                        "the {layer} layer posture is managed, but it declares no project; \
                         a managed layer must name the seam it projects into — add a `project` \
                         declaration (file, key, format, merge) or change the layer's posture"
                    ),
                )
                .with("layer", layer)
                .with("field", "project")
                .with("posture", posture.to_string()));
            }
        }
        LayerPosture::Brokered | LayerPosture::Observed => {
            if project.is_some() {
                return Err(HarnessProfileError::new(
                    "harness_profile.unmanaged_project",
                    format!(
                        "the {layer} layer posture is {posture}, but it declares a project; \
                         only a managed layer writes native config — set the posture to managed \
                         or remove the `project` declaration"
                    ),
                )
                .with("layer", layer)
                .with("field", "project")
                .with("posture", posture.to_string()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENCLAW_PROFILE_TOML: &str = r#"
schema = "aikit.harness-profile/v1"
slug = "openclaw"
edition = "cli"

[presence]
config-dir = "~/.openclaw"

[skills]
posture = "brokered"
observe.paths = ["~/.openclaw/workspace/AGENTS.md"]

[tools]
posture = "managed"
observe = [{ path = "~/.openclaw/mcp.json", collection = "mcpServers" }]
project = { file = "~/.openclaw/mcp.json", key = "mcpServers", format = "mcp-servers-record", merge = "preserve-foreign-sweep-owned" }
activation = "restart-client"

[models]
posture = "observed"
dispatch = { none = { reason = "catalog-declared: no native provider binding" } }
roster-note = "roster demand arrives from the catalog, never from this profile"
compatibility-note = "candidates gated by the harness_compatible roster facts"

[sessions]
posture = "observed"
protocol = "process"
open-modes = ["create", "load"]

[sessions.capabilities]
ordered-streaming = false
cancellation = false
permission-requests = false
reconnect = false
additional-directories = false
mcp-servers = false
"#;

    fn openclaw_profile() -> HarnessProfile {
        toml::from_str(OPENCLAW_PROFILE_TOML).expect("openclaw-shaped profile parses")
    }

    #[test]
    fn a_full_openclaw_shaped_profile_parses_from_toml() {
        let profile = openclaw_profile();
        assert_eq!(profile.schema, HARNESS_PROFILE_SCHEMA);
        assert_eq!(profile.slug, "openclaw");
        assert_eq!(profile.edition, HarnessEditionKind::Cli);
        assert_eq!(
            profile.presence.as_ref().unwrap().config_dir.as_deref(),
            Some("~/.openclaw")
        );

        let skills = profile.skills.as_ref().unwrap();
        assert_eq!(skills.posture, LayerPosture::Brokered);
        assert_eq!(
            skills.observe.as_ref().unwrap().paths,
            vec!["~/.openclaw/workspace/AGENTS.md".to_string()]
        );

        let tools = profile.tools.as_ref().unwrap();
        assert_eq!(tools.posture, LayerPosture::Managed);
        assert_eq!(
            tools.observe,
            vec![ToolObserveDeclaration {
                path: "~/.openclaw/mcp.json".to_string(),
                collection: "mcpServers".to_string(),
            }]
        );
        let project = tools.project.as_ref().unwrap();
        assert_eq!(project.file, "~/.openclaw/mcp.json");
        assert_eq!(project.key, "mcpServers");
        assert_eq!(project.format, MergeGrammar::McpServersRecord);
        assert_eq!(project.merge, MergePolicy::PreserveForeignSweepOwned);
        assert_eq!(tools.activation, Some(ActivationEffectName::RestartClient));

        let models = profile.models.as_ref().unwrap();
        assert_eq!(models.posture, LayerPosture::Observed);
        assert_eq!(
            models.dispatch,
            ModelDispatchPosture::None {
                reason: "catalog-declared: no native provider binding".to_string(),
            }
        );
        assert_eq!(
            models.roster_note.as_deref(),
            Some("roster demand arrives from the catalog, never from this profile")
        );
        assert_eq!(
            models.compatibility_note.as_deref(),
            Some("candidates gated by the harness_compatible roster facts")
        );

        let sessions = profile.sessions.as_ref().unwrap();
        assert_eq!(sessions.protocol, SessionProtocol::Process);
        assert_eq!(
            sessions.open_modes,
            vec!["create".to_string(), "load".to_string()]
        );
        assert!(!sessions.capabilities.mcp_servers);

        profile
            .validate()
            .expect("the openclaw-shaped profile satisfies its own posture declarations");
    }

    #[test]
    fn managed_tools_without_a_project_declaration_refuse_and_name_the_layer() {
        let mut profile = openclaw_profile();
        profile.tools.as_mut().unwrap().project = None;
        let error = profile.validate().unwrap_err();
        assert_eq!(error.code, "harness_profile.managed_without_project");
        assert!(
            error.to_string().contains("tools"),
            "error must name the layer: {error}"
        );
        assert!(
            error.to_string().contains("project"),
            "error must name the field: {error}"
        );
    }

    #[test]
    fn managed_hooks_without_a_project_declaration_refuse_and_name_the_layer() {
        let mut profile = openclaw_profile();
        profile.hooks = Some(HooksLayer {
            posture: LayerPosture::Managed,
            observe: vec![HookObserveDeclaration {
                events: vec!["SessionStart".to_string()],
                transports: None,
            }],
            project: None,
            activation: None,
        });
        let error = profile.validate().unwrap_err();
        assert_eq!(error.code, "harness_profile.managed_without_project");
        assert!(
            error.to_string().contains("hooks"),
            "error must name the layer: {error}"
        );
    }

    #[test]
    fn an_observed_layer_refuses_a_project_declaration_and_names_the_layer() {
        let mut profile = openclaw_profile();
        profile.tools.as_mut().unwrap().posture = LayerPosture::Observed;
        let error = profile.validate().unwrap_err();
        assert_eq!(error.code, "harness_profile.unmanaged_project");
        assert!(
            error.to_string().contains("tools"),
            "error must name the layer: {error}"
        );
        assert!(
            error.to_string().contains("observed"),
            "error must name the posture: {error}"
        );
    }

    #[test]
    fn a_brokered_layer_refuses_a_project_declaration_and_names_the_layer() {
        let mut profile = openclaw_profile();
        profile.hooks = Some(HooksLayer {
            posture: LayerPosture::Brokered,
            observe: vec![],
            project: Some(HookProjectDeclaration {
                file: "~/.zcode/settings.json".to_string(),
                format: MergeGrammar::ZcodeHookWrapper,
                ownership_identity: "aikit-hook-dispatcher".to_string(),
            }),
            activation: None,
        });
        let error = profile.validate().unwrap_err();
        assert_eq!(error.code, "harness_profile.unmanaged_project");
        assert!(
            error.to_string().contains("hooks"),
            "error must name the layer: {error}"
        );
        assert!(
            error.to_string().contains("brokered"),
            "error must name the posture: {error}"
        );
    }

    #[test]
    fn a_profile_round_trips_through_toml_and_json() {
        let profile = openclaw_profile();

        let toml_text = toml::to_string_pretty(&profile).expect("profile serialises to TOML");
        let from_toml: HarnessProfile =
            toml::from_str(&toml_text).expect("serialised TOML reparses");
        assert_eq!(from_toml, profile);

        let json_text = serde_json::to_string_pretty(&profile).expect("profile serialises to JSON");
        let from_json: HarnessProfile =
            serde_json::from_str(&json_text).expect("serialised JSON reparses");
        assert_eq!(from_json, profile);
    }

    #[test]
    fn a_wrong_schema_version_is_refused_naming_expected_and_found() {
        let mut profile = openclaw_profile();
        profile.schema = "aikit.harness-profile/v2".to_string();
        let error = profile.validate().unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains(HARNESS_PROFILE_SCHEMA),
            "error must name the expected version: {text}"
        );
        assert!(
            text.contains("aikit.harness-profile/v2"),
            "error must name the found version: {text}"
        );
    }

    #[test]
    fn an_empty_slug_is_refused() {
        let mut profile = openclaw_profile();
        profile.slug = "   ".to_string();
        let error = profile.validate().unwrap_err();
        assert_eq!(error.code, "harness_profile.empty_slug");
    }
}
