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
//!
//! The models layer may declare key delivery: per provider, the env var the
//! harness's native launch reads (or the own-login fact that it takes no env
//! key). A declared selector must satisfy the shared credential-variable
//! shape law — [`crate::credential::valid_credential_variable`] — and is
//! refused here otherwise; the launch path that consumes the declaration
//! materialises through the same credential seam the selected-model path
//! uses, never passing an empty or ambient value.

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

/// One declared env-var key delivery: the provider whose key this harness
/// reads from the process environment at launch, and the exact variable name
/// its native launch reads. The name is a delivery selector, never a value —
/// a declaration cannot carry material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ModelKeyDelivery {
    pub provider_ref: String,
    pub env_var: String,
}

/// The argv flags a provider-plural harness reads per invocation, observed on
/// its own command surface. A provider-plural dispatch alone says that
/// selection happens per invocation, not *how* — without the observed flags a
/// launcher would have to invent a selector, which posture truth forbids. The
/// names are the harness's own spellings, carried verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ModelArgvSelectors {
    pub provider: String,
    pub model: String,
}

/// One declared own-login fact: the provider this harness can also serve
/// through its own login store, with the census note. An own-login fact is
/// what makes a missing binding survivable — where it is absent, an unbound
/// declared key refuses the launch instead of silently starting a body that
/// cannot authenticate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ModelOwnLogin {
    pub provider_ref: String,
    pub note: String,
}

/// Declared key-delivery facts of the models layer: which env var each served
/// provider's launch reads, which providers the harness serves through its
/// own login store, and — where no per-provider entry applies — an honest
/// note saying so. A declaration is a delivery fact, never an availability
/// claim: whether a key is presently bound stays with the credential store.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ModelKeyDeliveryLayer {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_var: Vec<ModelKeyDelivery>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub own_login: Vec<ModelOwnLogin>,
    /// Free-text disclosure of the harness's overall key posture where no
    /// per-provider entry applies (own managed login, local serving, no key
    /// read at all). Prose, like the skills layer's shared-tree disclosure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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
    /// The observed argv flags a provider-plural harness reads per invocation.
    /// Only a provider-plural dispatch may declare them — a natively bound
    /// harness selects through its declared `selector`, and a `none` dispatch
    /// selects nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv_selectors: Option<ModelArgvSelectors>,
    /// Declared key delivery: the env var each served provider's launch
    /// reads, and the own-login facts. Absent means the census recorded no
    /// key-delivery fact for this harness at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_delivery: Option<ModelKeyDeliveryLayer>,
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
        if let Some(models) = &self.models {
            if let Some(key_delivery) = &models.key_delivery {
                validate_key_delivery(key_delivery)?;
            }
            validate_argv_selectors(&models.dispatch, models.argv_selectors.as_ref())?;
        }
        Ok(())
    }
}

/// Posture truth for the argv-selector declaration: only a provider-plural
/// dispatch may carry observed per-invocation flags, and the declared names
/// must look like the flags the harness actually reads (a leading dash and no
/// whitespace). Anything else would let a launcher pass a model through a
/// surface the harness never had.
fn validate_argv_selectors(
    dispatch: &ModelDispatchPosture,
    selectors: Option<&ModelArgvSelectors>,
) -> Result<(), HarnessProfileError> {
    let Some(selectors) = selectors else {
        return Ok(());
    };
    if !matches!(dispatch, ModelDispatchPosture::ProviderPlural) {
        return Err(HarnessProfileError::new(
            "harness_profile.argv_selectors_outside_provider_plural",
            "the models layer declares argv selectors, but its dispatch posture is not \
             provider-plural; a harness selects through its declared dispatch surface — \
             remove `argv-selectors` or change the dispatch posture",
        )
        .with("field", "argv-selectors"));
    }
    for (field, name) in [
        ("provider", &selectors.provider),
        ("model", &selectors.model),
    ] {
        if name.trim().is_empty()
            || name.len() < 2
            || !name.starts_with('-')
            || name.split_whitespace().count() != 1
        {
            return Err(HarnessProfileError::new(
                "harness_profile.invalid_argv_selector",
                format!(
                    "the declared argv selector for {field} is {name:?}, which is not a single \
                     command-line flag; record the exact flag the harness's own command surface \
                     reads (for example \"--model\")"
                ),
            )
            .with("field", format!("argv-selectors.{field}")));
        }
    }
    Ok(())
}

/// Posture truth for the key-delivery declarations. A declared env-var
/// selector must satisfy the shared credential-variable shape law
/// ([`crate::credential::valid_credential_variable`]) — a selector that could
/// not be lawfully injected is refused here, at validation, rather than at
/// launch. One provider may not declare two different variables, and an
/// own-login fact must actually say something about its provider.
fn validate_key_delivery(delivery: &ModelKeyDeliveryLayer) -> Result<(), HarnessProfileError> {
    let mut seen = BTreeMap::new();
    for entry in &delivery.env_var {
        if entry.provider_ref.trim().is_empty() {
            return Err(HarnessProfileError::new(
                "harness_profile.empty_key_delivery_provider",
                "a key delivery must name the provider whose key it delivers; \
                 set `provider-ref` to a `provider:<vendor>` ref",
            )
            .with("field", "provider-ref"));
        }
        if !crate::credential::valid_credential_variable(&entry.env_var) {
            return Err(HarnessProfileError::new(
                "harness_profile.invalid_key_env_var",
                format!(
                    "the declared key-delivery variable {:?} is not a lawful credential \
                     variable; a provider key variable must end _API_KEY, _TOKEN or _KEY, \
                     start with a letter or underscore, and never collide with AIKit's \
                     own surfaces (CENTRAL_/WORKCELL_/AIKIT_/LD_/DYLD_)",
                    entry.env_var
                ),
            )
            .with("provider-ref", entry.provider_ref.clone())
            .with("env-var", entry.env_var.clone()));
        }
        if let Some(previous) = seen.insert(entry.provider_ref.as_str(), entry.env_var.as_str()) {
            return Err(HarnessProfileError::new(
                "harness_profile.duplicate_key_delivery",
                format!(
                    "provider {} declares two delivery variables ({previous} and {}); \
                     one provider is read through one variable — remove the entry that \
                     is not what the harness actually reads",
                    entry.provider_ref, entry.env_var,
                ),
            )
            .with("provider-ref", entry.provider_ref.clone()));
        }
    }
    for fact in &delivery.own_login {
        if fact.provider_ref.trim().is_empty() {
            return Err(HarnessProfileError::new(
                "harness_profile.empty_key_delivery_provider",
                "an own-login fact must name the provider it serves; set `provider-ref` \
                 to a `provider:<vendor>` ref",
            )
            .with("field", "provider-ref"));
        }
        if fact.note.trim().is_empty() {
            return Err(HarnessProfileError::new(
                "harness_profile.empty_own_login_note",
                "an own-login fact must carry its census note; say which store the \
                 harness authenticates through",
            )
            .with("provider-ref", fact.provider_ref.clone())
            .with("field", "note"));
        }
    }
    if delivery.env_var.is_empty()
        && delivery.own_login.is_empty()
        && delivery
            .note
            .as_deref()
            .is_none_or(|note| note.trim().is_empty())
    {
        return Err(HarnessProfileError::new(
            "harness_profile.empty_key_delivery",
            "the key-delivery layer declares nothing; a layer that says nothing is \
             removed rather than kept empty — delete `key-delivery` or record the \
             harness's actual key posture",
        )
        .with("field", "key-delivery"));
    }
    Ok(())
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

    fn claude_shaped_profile() -> HarnessProfile {
        let mut profile = openclaw_profile();
        profile.models = Some(ModelsLayer {
            posture: LayerPosture::Observed,
            dispatch: ModelDispatchPosture::NativeProviderBinding {
                provider_ref: "provider:anthropic".to_string(),
                selector_kind: "config-key".to_string(),
                selector_name: "model".to_string(),
            },
            roster_note: None,
            compatibility_note: None,
            argv_selectors: None,
            key_delivery: Some(ModelKeyDeliveryLayer {
                env_var: vec![ModelKeyDelivery {
                    provider_ref: "provider:anthropic".to_string(),
                    env_var: "ANTHROPIC_API_KEY".to_string(),
                }],
                own_login: vec![ModelOwnLogin {
                    provider_ref: "provider:anthropic".to_string(),
                    note: "claude login stores its own credential".to_string(),
                }],
                note: None,
            }),
        });
        profile
    }

    #[test]
    fn a_lawful_key_delivery_declaration_validates_and_round_trips() {
        let profile = claude_shaped_profile();
        profile
            .validate()
            .expect("a well-shaped key delivery satisfies posture truth");
        let models = profile.models.as_ref().unwrap();
        let delivery = models.key_delivery.as_ref().unwrap();
        assert_eq!(
            delivery.env_var.first().unwrap().env_var,
            "ANTHROPIC_API_KEY"
        );
    }

    #[test]
    fn a_key_delivery_selector_that_fails_the_variable_law_is_refused_at_validation() {
        for unlawful in [
            "PATH",
            "KEY",
            "ANTHROPIC-API-KEY",
            "AIKIT_GATEWAY_TOKEN",
            "CENTRAL_NATIVE_TOKEN",
            "DYLD_LIBRARY_KEY",
        ] {
            let mut profile = claude_shaped_profile();
            profile
                .models
                .as_mut()
                .unwrap()
                .key_delivery
                .as_mut()
                .unwrap()
                .env_var[0]
                .env_var = unlawful.to_string();
            let error = profile.validate().unwrap_err();
            assert_eq!(
                error.code, "harness_profile.invalid_key_env_var",
                "{unlawful} must be refused"
            );
            assert!(
                error.to_string().contains(unlawful),
                "the refusal must name the offending variable: {error}"
            );
        }
    }

    #[test]
    fn one_provider_cannot_declare_two_delivery_variables() {
        let mut profile = claude_shaped_profile();
        let delivery = profile
            .models
            .as_mut()
            .unwrap()
            .key_delivery
            .as_mut()
            .unwrap();
        delivery.env_var.push(ModelKeyDelivery {
            provider_ref: "provider:anthropic".to_string(),
            env_var: "ANTHROPIC_AUTH_TOKEN".to_string(),
        });
        let error = profile.validate().unwrap_err();
        assert_eq!(error.code, "harness_profile.duplicate_key_delivery");
        assert!(error.to_string().contains("provider:anthropic"));
    }

    #[test]
    fn an_own_login_fact_without_a_note_is_refused() {
        let mut profile = claude_shaped_profile();
        profile
            .models
            .as_mut()
            .unwrap()
            .key_delivery
            .as_mut()
            .unwrap()
            .own_login[0]
            .note = "   ".to_string();
        let error = profile.validate().unwrap_err();
        assert_eq!(error.code, "harness_profile.empty_own_login_note");
    }

    #[test]
    fn an_empty_key_delivery_layer_is_refused_rather_than_kept() {
        let mut profile = claude_shaped_profile();
        profile.models.as_mut().unwrap().key_delivery = Some(ModelKeyDeliveryLayer::default());
        let error = profile.validate().unwrap_err();
        assert_eq!(error.code, "harness_profile.empty_key_delivery");
    }

    #[test]
    fn a_note_only_key_delivery_declares_an_honest_no_env_path_fact() {
        let mut profile = openclaw_profile();
        let models = profile.models.as_mut().unwrap();
        models.key_delivery = Some(ModelKeyDeliveryLayer {
            env_var: vec![],
            own_login: vec![],
            note: Some(
                "this harness authenticates through its own managed login; no env-var \
                 key path is declared"
                    .to_string(),
            ),
        });
        profile
            .validate()
            .expect("a note-only delivery is a fact, not an omission");
    }

    fn provider_plural_profile() -> HarnessProfile {
        let mut profile = openclaw_profile();
        profile.models = Some(ModelsLayer {
            posture: LayerPosture::Observed,
            dispatch: ModelDispatchPosture::ProviderPlural,
            roster_note: None,
            compatibility_note: None,
            argv_selectors: Some(ModelArgvSelectors {
                provider: "--provider".to_string(),
                model: "--model".to_string(),
            }),
            key_delivery: None,
        });
        profile
    }

    #[test]
    fn argv_selectors_round_trip_and_belong_to_provider_plural_dispatch() {
        let profile = provider_plural_profile();
        profile
            .validate()
            .expect("provider-plural dispatch may declare observed argv flags");
        let toml_text = toml::to_string_pretty(&profile).expect("serialises");
        assert!(
            toml_text.contains("argv-selectors"),
            "the declaration must round-trip under its kebab name: {toml_text}"
        );
        let reparsed: HarnessProfile = toml::from_str(&toml_text).expect("reparses");
        assert_eq!(reparsed, profile);
    }

    #[test]
    fn argv_selectors_outside_provider_plural_dispatch_are_refused() {
        let mut profile = claude_shaped_profile();
        profile.models.as_mut().unwrap().argv_selectors = Some(ModelArgvSelectors {
            provider: "--provider".to_string(),
            model: "--model".to_string(),
        });
        let error = profile.validate().unwrap_err();
        assert_eq!(
            error.code, "harness_profile.argv_selectors_outside_provider_plural",
            "a natively bound harness selects through its declared selector, not argv flags"
        );
    }

    #[test]
    fn an_argv_selector_that_is_not_a_single_flag_is_refused() {
        for unlawful in [
            ("provider", "provider"),
            ("model", "--model gpt"),
            ("model", "model"),
            ("provider", "-"),
        ] {
            let mut profile = provider_plural_profile();
            let selectors = profile
                .models
                .as_mut()
                .unwrap()
                .argv_selectors
                .as_mut()
                .unwrap();
            match unlawful.0 {
                "provider" => selectors.provider = unlawful.1.to_string(),
                _ => selectors.model = unlawful.1.to_string(),
            }
            let error = profile.validate().unwrap_err();
            assert_eq!(
                error.code, "harness_profile.invalid_argv_selector",
                "{unlawful:?} must be refused"
            );
            assert!(
                error.to_string().contains(unlawful.1),
                "the refusal must name the offending spelling: {error}"
            );
        }
    }
}
