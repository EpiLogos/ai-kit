//! Generic model modality, interaction and transport facts for resolved model
//! surfaces.
//!
//! This module owns the vocabulary through which a resolved body says what it
//! can actually hear, say and do: input/output modalities, transform
//! capabilities (speech-to-text, text-to-speech, speech-to-speech, audio
//! understanding), interaction forms (streaming, full-duplex realtime,
//! barge-in, turn detection, structured tool requests) and the transport the
//! surface is reached over, with its connection/reconnect semantics.
//!
//! Two invariants are the reason this seam exists:
//!
//! 1. **No lowest-common-denominator flattening.** A capability a provider
//!    does not offer is not silently dropped from the vocabulary — it is
//!    absent from the surface's declared sets, and every query answers with
//!    the explicit state it is in: supported, degraded (with the provider's
//!    own reason), unsupported (with why the absence is authoritative), or
//!    unknown (no modality contract was declared at all). Proven, degraded,
//!    proven-absent and unproven stay four different facts.
//! 2. **No provider or consumer ontology.** Provider-specific vocabulary
//!    (voice names, audio format strings, session parameter spellings) stays
//!    in the adapter that declared the contract and travels only as
//!    provider-native provenance strings. Nothing here names a consumer
//!    (voice clients, dialogue agents); those are downstream readers of
//!    these facts, never their owners.
//!
//! Credential facts are ref/presence only ([`CredentialCondition`]); secret
//! values have no representation in this module's types, so none can enter a
//! resolution read model, log or history entry through this seam.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::explain_history::{EvidenceProvenance, ExplainEvidence, ExplainFact};
use crate::model_runtime::ModelRuntimeReadModel;
use crate::resource::{CredentialCondition, ProviderRef, ResourceRef};
use crate::{AikitError, Result};

pub const MODEL_MODALITY_VERSION: &str = "aikit.model-modality/v1";

/// One input or output modality of a model surface. Absence from a declared
/// set is the explicit statement that the surface does not carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelModality {
    Text,
    /// Raw audio in or out (recordings, file transcription, audio output).
    Audio,
    /// Interactive speech in or out (microphone/voice channels).
    Speech,
}

impl ModelModality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Audio => "audio",
            Self::Speech => "speech",
        }
    }

    /// The acoustic modalities. Speech is the interactive carrier of audio.
    pub fn is_acoustic(self) -> bool {
        matches!(self, Self::Audio | Self::Speech)
    }
}

/// A named conversion the surface can perform. Declared per surface with its
/// own support state; a transform whose prerequisites (the modalities it
/// converts between) the surface does not carry is a validation error, not a
/// quiet downgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransformCapability {
    SpeechToText,
    TextToSpeech,
    SpeechToSpeech,
    AudioUnderstanding,
    /// Joint text+audio understanding/generation in one surface.
    MultimodalTextAudio,
}

impl TransformCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SpeechToText => "speech-to-text",
            Self::TextToSpeech => "text-to-speech",
            Self::SpeechToSpeech => "speech-to-speech",
            Self::AudioUnderstanding => "audio-understanding",
            Self::MultimodalTextAudio => "multimodal-text-audio",
        }
    }

    fn requires_acoustic_input(self) -> bool {
        matches!(
            self,
            Self::SpeechToText | Self::SpeechToSpeech | Self::AudioUnderstanding
        )
    }

    fn requires_acoustic_output(self) -> bool {
        matches!(self, Self::TextToSpeech | Self::SpeechToSpeech)
    }
}

/// One interaction form the surface offers. A structured tool request is an
/// interaction capability only: it names a channel on which the model may
/// emit a proposal, and grants no authority of its own — adjudication stays
/// with the caller through its own native-authority path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InteractionCapability {
    RequestResponse,
    StreamingInput,
    StreamingOutput,
    /// Simultaneous input and output on one live connection.
    FullDuplexRealtime,
    /// Discrete protocol events (partial results, state changes) rather than
    /// one terminal payload.
    StructuredEvents,
    /// The model may emit structured tool/action proposals on the surface.
    ToolRequests,
    Timestamps,
    /// Interim hypotheses while an utterance is still open.
    PartialTranscripts,
    /// A committed final transcript per utterance.
    FinalTranscripts,
    /// Voice-activity/turn detection is performed by the surface.
    VadTurnDetection,
    /// An in-flight output can be interrupted mid-stream.
    BargeIn,
}

impl InteractionCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RequestResponse => "request-response",
            Self::StreamingInput => "streaming-input",
            Self::StreamingOutput => "streaming-output",
            Self::FullDuplexRealtime => "full-duplex-realtime",
            Self::StructuredEvents => "structured-events",
            Self::ToolRequests => "tool-requests",
            Self::Timestamps => "timestamps",
            Self::PartialTranscripts => "partial-transcripts",
            Self::FinalTranscripts => "final-transcripts",
            Self::VadTurnDetection => "vad-turn-detection",
            Self::BargeIn => "barge-in",
        }
    }
}

/// The transport a surface is reached over. This is a surface fact, not the
/// surface's identity: the same body may expose several surfaces with
/// different transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransportKind {
    InProcess,
    Cli,
    Http,
    WebSocket,
    WebRtc,
    Sip,
    /// A provider-owned transport AIKit does not model generically. The
    /// provider-native spelling travels in provenance, not here.
    ProviderNative,
}

impl TransportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InProcess => "in-process",
            Self::Cli => "cli",
            Self::Http => "http",
            Self::WebSocket => "websocket",
            Self::WebRtc => "webrtc",
            Self::Sip => "sip",
            Self::ProviderNative => "provider-native",
        }
    }
}

/// What happens to a live session across a reconnection. "Cannot restore" is
/// a claim about the provider's session continuity only; caller-owned
/// Encounter/Agency/application state is always outside this fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReconnectSupport {
    /// No live connection exists to restore.
    NotApplicable,
    /// The provider can resume the same live session after a reconnect.
    Resumable,
    /// Reconnecting is possible, but live session state is lost and the
    /// caller must rebuild it from its own state.
    ReconnectWithoutSession,
    /// The surface offers no reconnect path at all.
    Unsupported,
}

impl ReconnectSupport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "not-applicable",
            Self::Resumable => "resumable",
            Self::ReconnectWithoutSession => "reconnect-without-session",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Connection semantics of the transport: either each request is independent,
/// or the surface holds a live connection whose reconnect behaviour is
/// [`ReconnectSupport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ConnectionSemantics {
    Stateless,
    Connected { reconnect: ReconnectSupport },
}

/// Who holds the credential a surface consumes. Presence and scope only —
/// the secret itself stays in its credential provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialScope {
    /// The calling process holds the credential for the whole body.
    #[default]
    Bearer,
    /// The surface consumes short-lived tokens scoped to exactly this
    /// surface/materialisation, minted by the credential holder.
    EphemeralSurfaceToken,
}

/// Current availability of a surface, with degradation kept distinct from
/// both full availability and absence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum SurfaceAvailability {
    Available,
    /// Usable with a stated reduction; the reason is the provider's or the
    /// body's own words, not a flattened label.
    Degraded { reason: String },
    Unavailable { reason: String },
}

impl SurfaceAvailability {
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Available | Self::Degraded { .. })
    }
}

/// Declared support for one transform capability on one surface. Only
/// positive states are declared; absence of the capability from the map is
/// the explicit unsupported fact, and queries widen it with a reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum DeclaredSupport {
    Supported,
    Degraded { reason: String },
}

/// Material/region/host constraints under which the declared facts hold.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MaterialConstraints {
    /// Provider/material region pin, when the surface declares one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Host the surface is bound to (`host/workcell` refs, "local", …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Further declared constraints as opaque name/value facts (e.g.
    /// `accelerator: gpu-required`). Never secret material.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<String, String>,
}

/// The full modality/interaction/transport contract of one model surface.
///
/// This is the generic type a provider adapter fills in and a consumer (an
/// agency constitution, a desktop client) reads. Everything on it is a fact
/// about the surface, not about any semantic Agent or consumer application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelModalityContract {
    pub schema: String,
    pub input_modalities: BTreeSet<ModelModality>,
    pub output_modalities: BTreeSet<ModelModality>,
    /// Declared positive transform supports; an absent key is explicitly
    /// unsupported (see [`Self::transform_support`]).
    #[serde(default)]
    pub transforms: BTreeMap<TransformCapability, DeclaredSupport>,
    /// Declared interaction capabilities; an absent capability is explicitly
    /// unsupported.
    #[serde(default)]
    pub interaction: BTreeSet<InteractionCapability>,
    /// Interaction capabilities that are offered in a reduced form, with the
    /// reason. Every key must also be in `interaction`.
    #[serde(default)]
    pub degraded_interaction: BTreeMap<InteractionCapability, String>,
    pub transport: TransportKind,
    pub connection: ConnectionSemantics,
    /// Who holds the credential this surface consumes (scope only, never a
    /// secret).
    #[serde(default)]
    pub credential_scope: CredentialScope,
    pub availability: SurfaceAvailability,
    /// Material provenance: which provider, which provider-native surface,
    /// which provider revision these facts were declared against. Provider
    /// spellings stay here; they never become the contract's identity.
    pub provider: ProviderRef,
    pub provider_native_surface: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_revision: Option<String>,
    /// Credential ref/presence condition of the route this surface sits on.
    pub credential: CredentialCondition,
    #[serde(default)]
    pub constraints: MaterialConstraints,
    /// Where these facts came from: adapter fixture revisions, provider
    /// documentation pins, detection refs.
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl ModelModalityContract {
    pub fn new(provider: ProviderRef, provider_native_surface: impl Into<String>) -> Self {
        Self {
            schema: MODEL_MODALITY_VERSION.to_string(),
            input_modalities: BTreeSet::new(),
            output_modalities: BTreeSet::new(),
            transforms: BTreeMap::new(),
            interaction: BTreeSet::new(),
            degraded_interaction: BTreeMap::new(),
            transport: TransportKind::ProviderNative,
            connection: ConnectionSemantics::Stateless,
            credential_scope: CredentialScope::Bearer,
            availability: SurfaceAvailability::Available,
            provider,
            provider_native_surface: provider_native_surface.into(),
            provider_revision: None,
            credential: CredentialCondition::NotRequired,
            constraints: MaterialConstraints::default(),
            provenance: Vec::new(),
        }
    }

    /// Validate internal consistency. The point is that a contract must not
    /// quietly claim a conversion whose modalities it does not carry: a
    /// speech-to-text transform with no acoustic input is a contradiction,
    /// and contradictions in capability declarations are how read models
    /// start lying.
    pub fn validate(&self) -> Result<()> {
        if self.schema != MODEL_MODALITY_VERSION {
            return Err(AikitError::new(
                "model_modality.schema_mismatch",
                format!("modality contract schema must be {MODEL_MODALITY_VERSION}"),
            ));
        }
        if self.provider_native_surface.trim().is_empty() {
            return Err(AikitError::new(
                "model_modality.empty_provider_surface",
                "provider_native_surface must name the provider-native surface these facts describe",
            ));
        }
        for capability in self.degraded_interaction.keys() {
            if !self.interaction.contains(capability) {
                return Err(AikitError::new(
                    "model_modality.degraded_without_capability",
                    format!(
                        "degradation recorded for `{}`, which the surface does not declare",
                        capability.as_str()
                    ),
                ));
            }
        }
        for (capability, support) in &self.transforms {
            let acoustic_input_ok = !capability.requires_acoustic_input()
                || self.input_modalities.iter().any(|m| m.is_acoustic());
            if !acoustic_input_ok {
                return Err(AikitError::new(
                    "model_modality.transform_without_input_modality",
                    format!(
                        "`{}` is declared but the surface carries no acoustic input modality",
                        capability.as_str()
                    ),
                )
                .with("transform", capability.as_str()));
            }
            let acoustic_output_ok = !capability.requires_acoustic_output()
                || self.output_modalities.iter().any(|m| m.is_acoustic());
            if !acoustic_output_ok {
                return Err(AikitError::new(
                    "model_modality.transform_without_output_modality",
                    format!(
                        "`{}` is declared but the surface carries no acoustic output modality",
                        capability.as_str()
                    ),
                )
                .with("transform", capability.as_str()));
            }
            if *support == DeclaredSupport::Supported
                && *capability == TransformCapability::MultimodalTextAudio
                && (!self.input_modalities.contains(&ModelModality::Text)
                    || !self.output_modalities.contains(&ModelModality::Text))
            {
                return Err(AikitError::new(
                    "model_modality.transform_without_text_modality",
                    "multimodal-text-audio is declared but the surface does not carry text in both directions",
                ));
            }
        }
        Ok(())
    }

    /// Explicit state of one input modality on this surface.
    pub fn input_support(&self, modality: ModelModality) -> ModalitySupport {
        self.direction_support(modality, &self.input_modalities, "input")
    }

    /// Explicit state of one output modality on this surface.
    pub fn output_support(&self, modality: ModelModality) -> ModalitySupport {
        self.direction_support(modality, &self.output_modalities, "output")
    }

    fn direction_support(
        &self,
        modality: ModelModality,
        declared: &BTreeSet<ModelModality>,
        direction: &str,
    ) -> ModalitySupport {
        let name = format!("{direction} modality `{}`", modality.as_str());
        if declared.contains(&modality) {
            match &self.availability {
                SurfaceAvailability::Available => ModalitySupport::Supported,
                SurfaceAvailability::Degraded { reason } => ModalitySupport::Degraded {
                    reason: reason.clone(),
                },
                SurfaceAvailability::Unavailable { reason } => ModalitySupport::Unsupported {
                    reason: format!("{name} declared but the surface is unavailable: {reason}"),
                },
            }
        } else {
            ModalitySupport::Unsupported {
                reason: format!(
                    "{name} is not declared by surface {}",
                    self.provider_native_surface
                ),
            }
        }
    }

    /// Explicit state of one transform capability on this surface.
    pub fn transform_support(&self, capability: TransformCapability) -> ModalitySupport {
        match self.transforms.get(&capability) {
            Some(DeclaredSupport::Supported) => match &self.availability {
                SurfaceAvailability::Available => ModalitySupport::Supported,
                SurfaceAvailability::Degraded { reason } => ModalitySupport::Degraded {
                    reason: reason.clone(),
                },
                SurfaceAvailability::Unavailable { reason } => ModalitySupport::Unsupported {
                    reason: format!(
                        "transform `{}` declared but the surface is unavailable: {reason}",
                        capability.as_str()
                    ),
                },
            },
            Some(DeclaredSupport::Degraded { reason }) => ModalitySupport::Degraded {
                reason: reason.clone(),
            },
            None => ModalitySupport::Unsupported {
                reason: format!(
                    "transform `{}` is not declared by surface {}",
                    capability.as_str(),
                    self.provider_native_surface
                ),
            },
        }
    }

    /// Explicit state of one interaction capability on this surface.
    pub fn interaction_support(&self, capability: InteractionCapability) -> ModalitySupport {
        if !self.interaction.contains(&capability) {
            return ModalitySupport::Unsupported {
                reason: format!(
                    "interaction `{}` is not declared by surface {}",
                    capability.as_str(),
                    self.provider_native_surface
                ),
            };
        }
        if let Some(reason) = self.degraded_interaction.get(&capability) {
            return ModalitySupport::Degraded {
                reason: reason.clone(),
            };
        }
        match &self.availability {
            SurfaceAvailability::Available => ModalitySupport::Supported,
            SurfaceAvailability::Degraded { reason } => ModalitySupport::Degraded {
                reason: reason.clone(),
            },
            SurfaceAvailability::Unavailable { reason } => ModalitySupport::Unsupported {
                reason: format!(
                    "interaction `{}` declared but the surface is unavailable: {reason}",
                    capability.as_str()
                ),
            },
        }
    }

    /// Whether the surface carries interactive speech in both directions.
    pub fn is_speech_capable(&self) -> bool {
        self.input_support(ModelModality::Speech).is_supported()
            && self.output_support(ModelModality::Speech).is_supported()
    }

    /// Flat modality tags for the roster seam (`ModelRosterCandidate::
    /// modalities`, `ModelRosterDemand::required_modalities`).
    pub fn modality_tags(&self) -> BTreeSet<String> {
        self.input_modalities
            .iter()
            .chain(self.output_modalities.iter())
            .map(|m| m.as_str().to_string())
            .collect()
    }

    /// Flat capability tags for the roster seam's `native_capabilities`:
    /// declared transform and interaction capabilities.
    pub fn capability_tags(&self) -> BTreeSet<String> {
        self.transforms
            .keys()
            .map(|t| t.as_str().to_string())
            .chain(self.interaction.iter().map(|i| i.as_str().to_string()))
            .collect()
    }
}

/// The fully explicit answer to "can this resolved body do X?". Supported,
/// degraded, unsupported and unknown are four different facts and never
/// collapse into two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ModalitySupport {
    Supported,
    Degraded { reason: String },
    Unsupported { reason: String },
    /// No modality contract was declared for the surface, so nothing is
    /// known. This is not unsupported: unproven and proven-absent stay
    /// distinct.
    Unknown { reason: String },
}

impl ModalitySupport {
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Supported)
    }

    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Supported | Self::Degraded { .. })
    }
}

/// Query a surface reading that may not have declared a modality contract at
/// all (a plain text harness surface legitimately declares none).
pub fn surface_interaction_support(
    contract: Option<&ModelModalityContract>,
    capability: InteractionCapability,
) -> ModalitySupport {
    match contract {
        Some(contract) => contract.interaction_support(capability),
        None => ModalitySupport::Unknown {
            reason: "no modality contract was declared for this surface".to_string(),
        },
    }
}

/// Query a surface reading for one direction of one modality.
pub fn surface_modality_support(
    contract: Option<&ModelModalityContract>,
    direction: ModalityDirection,
    modality: ModelModality,
) -> ModalitySupport {
    match contract {
        Some(contract) => match direction {
            ModalityDirection::Input => contract.input_support(modality),
            ModalityDirection::Output => contract.output_support(modality),
        },
        None => ModalitySupport::Unknown {
            reason: "no modality contract was declared for this surface".to_string(),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalityDirection {
    Input,
    Output,
}

impl ModalityDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

/// The composed modality view of a multi-stage body (for example
/// speech-to-text stage, text reasoning harness, text-to-speech stage).
///
/// Per-stage facts stay the load-bearing truth; this view only derives the
/// body-level answer, and it derives it strictly: a body-level interaction
/// capability is supported only where every declared stage supports it. That
/// is the opposite of a lowest-common-denominator lift — no capability is
/// ever granted that a stage withheld, and every derived answer names the
/// stages it rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposedModalityView {
    /// The first declared stage's input modalities: what enters the body.
    pub input_modalities: BTreeSet<ModelModality>,
    /// The last declared stage's output modalities: what leaves the body.
    pub output_modalities: BTreeSet<ModelModality>,
    pub interaction: BTreeMap<InteractionCapability, ModalitySupport>,
    pub speech_capable: bool,
    /// True when every stage declared a modality contract. When false,
    /// body-level claims about capabilities outside the derived map answer
    /// unknown rather than unsupported.
    pub complete: bool,
    /// Why each derived answer is what it is: which stage carried which
    /// fact. Ordered, stage-named strings.
    pub basis: Vec<String>,
}

impl ComposedModalityView {
    /// Body-level answer for one interaction capability. Capabilities the
    /// map carries were derivable from fully-declared stages; anything else
    /// answers unknown while stages remain opaque, and unsupported only
    /// once every stage has spoken.
    pub fn interaction_support(&self, capability: InteractionCapability) -> ModalitySupport {
        if let Some(support) = self.interaction.get(&capability) {
            return support.clone();
        }
        if !self.complete {
            return ModalitySupport::Unknown {
                reason: "at least one stage of this body declares no modality contract, so \
                         absence cannot be proven"
                    .to_string(),
            };
        }
        ModalitySupport::Unsupported {
            reason: format!(
                "no stage of this body declares interaction `{}`",
                capability.as_str()
            ),
        }
    }

    /// What enters the first stage and what leaves the last, for bodies
    /// whose stages form a pipeline.
    pub fn pipeline_modalities(&self) -> (BTreeSet<ModelModality>, BTreeSet<ModelModality>) {
        (self.input_modalities.clone(), self.output_modalities.clone())
    }
}

/// Derive the body-level modality view from ordered per-stage contracts.
/// `stages` are `(stage component, contract)` pairs in body order.
///
/// Derivation rules, deliberately strict:
///
/// * body input modalities are the **first** declared stage's inputs; body
///   output modalities are the **last** declared stage's outputs (pipeline
///   semantics — what enters the body and what leaves it);
/// * a body-level interaction capability is carried only where **every
///   fully-declared stage** supports it, and only when no stage is opaque;
/// * a stage that declares no modality contract is opaque: it neither
///   grants nor vetoes, and it turns body-level claims about capabilities
///   outside the declared intersection into unknowns rather than letting a
///   resolution overclaim. Per-stage facts remain the load-bearing truth.
pub fn compose_stage_modalities(
    stages: &[(&ResourceRef, Option<&ModelModalityContract>)],
) -> ComposedModalityView {
    let mut input_modalities: BTreeSet<ModelModality> = BTreeSet::new();
    let mut basis = Vec::new();
    let mut declared_capabilities: BTreeSet<InteractionCapability> = BTreeSet::new();
    let mut opaque_stages: Vec<String> = Vec::new();
    let mut first_inputs_seen = false;
    let mut last_declared: Option<(&ResourceRef, &ModelModalityContract)> = None;

    for (stage, contract) in stages {
        let Some(contract) = contract else {
            opaque_stages.push(stage.to_string());
            basis.push(format!(
                "stage {stage} declares no modality contract; body-level claims do not rest on it"
            ));
            continue;
        };
        if !first_inputs_seen {
            for modality in &contract.input_modalities {
                basis.push(format!(
                    "input `{}` enters at stage {stage} (surface {})",
                    modality.as_str(),
                    contract.provider_native_surface
                ));
            }
            input_modalities = contract.input_modalities.clone();
            first_inputs_seen = true;
        }
        declared_capabilities.extend(contract.interaction.iter().copied());
        last_declared = Some((stage, contract));
    }

    // The pipeline's output modalities are the last declared stage's.
    let mut output_modalities: BTreeSet<ModelModality> = BTreeSet::new();
    if let Some((stage, contract)) = last_declared {
        output_modalities = contract.output_modalities.clone();
        for modality in &output_modalities {
            basis.push(format!(
                "output `{}` leaves at stage {stage} (surface {})",
                modality.as_str(),
                contract.provider_native_surface
            ));
        }
    }

    let mut interaction = BTreeMap::new();
    for capability in declared_capabilities {
        let mut supporting = Vec::new();
        let mut degraded_reasons = Vec::new();
        let mut withholding = Vec::new();
        for (stage, contract) in stages {
            let Some(contract) = contract else {
                continue;
            };
            match contract.interaction_support(capability) {
                ModalitySupport::Supported => supporting.push(stage.to_string()),
                ModalitySupport::Degraded { reason } => {
                    supporting.push(stage.to_string());
                    degraded_reasons.push(format!("{stage}: {reason}"));
                }
                // A declared surface's absence is an explicit refusal, and
                // one proven-absent stage refutes the body-level claim no
                // matter what an opaque stage might have contributed.
                ModalitySupport::Unsupported { .. } => withholding.push(stage.to_string()),
                ModalitySupport::Unknown { .. } => {}
            }
        }
        if !withholding.is_empty() {
            interaction.insert(
                capability,
                ModalitySupport::Unsupported {
                    reason: format!(
                        "not carried by every stage; withheld by {}",
                        withholding.join(", ")
                    ),
                },
            );
        } else if opaque_stages.is_empty() {
            let support = if degraded_reasons.is_empty() {
                ModalitySupport::Supported
            } else {
                ModalitySupport::Degraded {
                    reason: degraded_reasons.join("; "),
                }
            };
            basis.push(format!(
                "interaction `{}` rests on stages {}",
                capability.as_str(),
                supporting.join(", ")
            ));
            interaction.insert(capability, support);
        }
        // Opaque stages present and nothing explicitly withheld: the claim
        // is merely unproven, so leave the capability out of the map and
        // let `interaction_support` answer unknown.
    }

    let speech_in = input_modalities.contains(&ModelModality::Speech);
    let speech_out = output_modalities.contains(&ModelModality::Speech);
    ComposedModalityView {
        input_modalities,
        output_modalities,
        interaction,
        speech_capable: speech_in && speech_out,
        complete: opaque_stages.is_empty(),
        basis,
    }
}

/// Explain why a resolved single-body session reads as it does. This is the
/// effective-state explanation of the modality half of the body: which
/// provider/native surface declared what, why body-level answers hold, and
/// which capabilities are honestly absent. Facts only classify what the read
/// model already resolved; nothing here re-resolves or overstates.
pub fn explain_model_modality(read_model: &ModelRuntimeReadModel) -> ExplainEvidence {
    let subject = read_model.harness.clone();
    let mut facts = Vec::new();
    let contract = read_model.relation.model_surface.modality.as_ref();
    let Some(contract) = contract else {
        facts.push(ExplainFact {
            relation: "body-modality".into(),
            authority: Some(crate::resource::SourceAuthority::Derived),
            summary: "no modality contract was declared; the surface is a plain text body"
                .into(),
            canonical_refs: Vec::new(),
            provenance: Vec::new(),
        });
        return ExplainEvidence {
            schema: crate::EXPLAIN_HISTORY_VERSION.into(),
            subject,
            facts,
        };
    };

    let provenance = vec![EvidenceProvenance {
        provider: ResourceRef::parse(contract.provider.as_str()).ok(),
        native_id: Some(contract.provider_native_surface.clone()),
        revision: contract.provider_revision.clone(),
        ..EvidenceProvenance::default()
    }];
    for (relation, modalities, direction) in [
        (
            "body-input-modality",
            &contract.input_modalities,
            ModalityDirection::Input,
        ),
        (
            "body-output-modality",
            &contract.output_modalities,
            ModalityDirection::Output,
        ),
    ] {
        if modalities.is_empty() {
            continue;
        }
        let names = modalities
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        facts.push(ExplainFact {
            relation: relation.into(),
            authority: Some(crate::resource::SourceAuthority::Observed),
            summary: format!(
                "{} declared as [{names}] on surface {} via {} ({})",
                direction.as_str(),
                contract.provider_native_surface,
                contract.provider,
                contract.transport.as_str()
            ),
            canonical_refs: Vec::new(),
            provenance: provenance.clone(),
        });
    }
    for capability in &contract.transforms {
        facts.push(ExplainFact {
            relation: "body-transform".into(),
            authority: Some(crate::resource::SourceAuthority::Observed),
            summary: format!(
                "transform `{}` declared on surface {}",
                capability.0.as_str(),
                contract.provider_native_surface
            ),
            canonical_refs: Vec::new(),
            provenance: provenance.clone(),
        });
    }
    for (capability, label) in [
        (
            InteractionCapability::FullDuplexRealtime,
            "full-duplex realtime",
        ),
        (InteractionCapability::BargeIn, "barge-in"),
        (InteractionCapability::VadTurnDetection, "turn detection"),
    ] {
        let statement = match contract.interaction_support(capability) {
            ModalitySupport::Supported => format!("{label} is supported"),
            ModalitySupport::Degraded { reason } => format!("{label} is degraded: {reason}"),
            ModalitySupport::Unsupported { reason } => {
                format!("{label} is unsupported: {reason}")
            }
            ModalitySupport::Unknown { reason } => format!("{label} is unknown: {reason}"),
        };
        facts.push(ExplainFact {
            relation: "body-interaction".into(),
            authority: Some(crate::resource::SourceAuthority::Observed),
            summary: statement,
            canonical_refs: Vec::new(),
            provenance: provenance.clone(),
        });
    }
    facts.push(ExplainFact {
        relation: "body-transport".into(),
        authority: Some(crate::resource::SourceAuthority::Observed),
        summary: format!(
            "transport {} over {:?} with credential scope {:?}",
            contract.transport.as_str(),
            contract.connection,
            contract.credential_scope
        ),
        canonical_refs: Vec::new(),
        provenance: provenance.clone(),
    });
    for (capability, reason) in &contract.degraded_interaction {
        facts.push(ExplainFact {
            relation: "body-interaction-degraded".into(),
            authority: Some(crate::resource::SourceAuthority::Observed),
            summary: format!(
                "interaction `{}` is degraded: {reason}",
                capability.as_str()
            ),
            canonical_refs: Vec::new(),
            provenance: provenance.clone(),
        });
    }

    ExplainEvidence {
        schema: crate::EXPLAIN_HISTORY_VERSION.into(),
        subject,
        facts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn realtime_contract() -> ModelModalityContract {
        let mut contract = ModelModalityContract::new(
            ProviderRef::parse("provider:openai").unwrap(),
            "gpt-realtime",
        );
        contract.input_modalities =
            BTreeSet::from([ModelModality::Audio, ModelModality::Speech, ModelModality::Text]);
        contract.output_modalities =
            BTreeSet::from([ModelModality::Audio, ModelModality::Speech, ModelModality::Text]);
        contract.transforms.insert(
            TransformCapability::SpeechToSpeech,
            DeclaredSupport::Supported,
        );
        contract.transforms.insert(
            TransformCapability::AudioUnderstanding,
            DeclaredSupport::Supported,
        );
        contract.interaction = BTreeSet::from([
            InteractionCapability::StreamingInput,
            InteractionCapability::StreamingOutput,
            InteractionCapability::FullDuplexRealtime,
            InteractionCapability::StructuredEvents,
            InteractionCapability::ToolRequests,
            InteractionCapability::VadTurnDetection,
            InteractionCapability::BargeIn,
        ]);
        contract.transport = TransportKind::WebSocket;
        contract.connection = ConnectionSemantics::Connected {
            reconnect: ReconnectSupport::ReconnectWithoutSession,
        };
        contract
    }

    #[test]
    fn an_undeclared_capability_answers_unsupported_with_a_reason_never_supported() {
        let contract = realtime_contract();
        match contract.interaction_support(InteractionCapability::PartialTranscripts) {
            ModalitySupport::Unsupported { reason } => {
                assert!(reason.contains("partial-transcripts"));
                assert!(reason.contains("gpt-realtime"));
            }
            other => panic!("an undeclared capability must not read as {other:?}"),
        }
        assert!(matches!(
            contract.transform_support(TransformCapability::TextToSpeech),
            ModalitySupport::Unsupported { .. }
        ));
    }

    #[test]
    fn a_text_only_surface_with_no_contract_answers_unknown_not_unsupported() {
        // Unproven and proven-absent are different facts: a surface that
        // declared nothing cannot yet be held to have refused.
        let support = surface_interaction_support(None, InteractionCapability::BargeIn);
        assert!(matches!(support, ModalitySupport::Unknown { .. }));
        let text_in =
            surface_modality_support(None, ModalityDirection::Input, ModelModality::Speech);
        assert!(matches!(text_in, ModalitySupport::Unknown { .. }));
    }

    #[test]
    fn degradation_travels_with_its_own_reason_and_never_flattens() {
        let mut contract = realtime_contract();
        contract.availability = SurfaceAvailability::Degraded {
            reason: "region failover active".into(),
        };
        assert_eq!(
            contract.input_support(ModelModality::Speech),
            ModalitySupport::Degraded {
                reason: "region failover active".into()
            }
        );
        contract
            .degraded_interaction
            .insert(InteractionCapability::BargeIn, "half-duplex until resume".into());
        assert_eq!(
            contract.interaction_support(InteractionCapability::BargeIn),
            ModalitySupport::Degraded {
                reason: "half-duplex until resume".into()
            }
        );
        // A capability the surface never declared still says unsupported.
        assert!(matches!(
            contract.interaction_support(InteractionCapability::FinalTranscripts),
            ModalitySupport::Unsupported { .. }
        ));
    }

    #[test]
    fn a_unavailable_surface_reports_its_declared_modalities_as_unavailable() {
        let mut contract = realtime_contract();
        contract.availability = SurfaceAvailability::Unavailable {
            reason: "provider decommitted the realtime surface".into(),
        };
        assert!(matches!(
            contract.input_support(ModelModality::Speech),
            ModalitySupport::Unsupported { .. }
        ));
        assert!(matches!(
            contract.interaction_support(InteractionCapability::FullDuplexRealtime),
            ModalitySupport::Unsupported { .. }
        ));
        assert!(!contract.is_speech_capable());
    }

    #[test]
    fn a_transform_is_refused_when_its_modalities_are_not_carried() {
        let mut contract = realtime_contract();
        contract.output_modalities.clear();
        let error = contract.validate().unwrap_err();
        assert_eq!(error.code(), "model_modality.transform_without_output_modality");

        let mut text_only = ModelModalityContract::new(
            ProviderRef::parse("provider:example").unwrap(),
            "text-chat",
        );
        text_only
            .transforms
            .insert(TransformCapability::SpeechToText, DeclaredSupport::Supported);
        let error = text_only.validate().unwrap_err();
        assert_eq!(error.code(), "model_modality.transform_without_input_modality");
    }

    #[test]
    fn a_degradation_recorded_against_an_undeclared_capability_is_a_contract_error() {
        let mut contract = realtime_contract();
        contract
            .degraded_interaction
            .insert(InteractionCapability::Timestamps, "no timestamps".into());
        let error = contract.validate().unwrap_err();
        assert_eq!(error.code(), "model_modality.degraded_without_capability");
    }

    #[test]
    fn roster_tags_expose_modalities_and_capabilities_as_flat_membership() {
        let contract = realtime_contract();
        let modalities = contract.modality_tags();
        assert!(modalities.contains("speech"));
        assert!(modalities.contains("text"));
        assert!(modalities.contains("audio"));
        let capabilities = contract.capability_tags();
        assert!(capabilities.contains("speech-to-speech"));
        assert!(capabilities.contains("full-duplex-realtime"));
        assert!(capabilities.contains("barge-in"));
        assert!(!capabilities.contains("text"), "modalities are not capabilities");
    }

    #[test]
    fn a_cascade_body_derives_strictly_and_names_its_stages() {
        let mut stt = ModelModalityContract::new(
            ProviderRef::parse("provider:example-stt").unwrap(),
            "transcribe-v2",
        );
        stt.input_modalities = BTreeSet::from([ModelModality::Audio, ModelModality::Speech]);
        stt.output_modalities = BTreeSet::from([ModelModality::Text]);
        stt.transforms
            .insert(TransformCapability::SpeechToText, DeclaredSupport::Supported);
        stt.interaction = BTreeSet::from([
            InteractionCapability::RequestResponse,
            InteractionCapability::FinalTranscripts,
            InteractionCapability::VadTurnDetection,
        ]);
        let mut tts = ModelModalityContract::new(
            ProviderRef::parse("provider:example-tts").unwrap(),
            "speak-v1",
        );
        tts.input_modalities = BTreeSet::from([ModelModality::Text]);
        tts.output_modalities = BTreeSet::from([ModelModality::Audio, ModelModality::Speech]);
        tts.transforms
            .insert(TransformCapability::TextToSpeech, DeclaredSupport::Supported);
        tts.interaction = BTreeSet::from([
            InteractionCapability::StreamingOutput,
            InteractionCapability::StructuredEvents,
        ]);
        stt.interaction.insert(InteractionCapability::StructuredEvents);
        let text_stage: Option<&ModelModalityContract> = None;

        let stages = [
            (r("component/stt"), Some(&stt) as Option<&ModelModalityContract>),
            (r("component/text"), text_stage),
            (r("component/tts"), Some(&tts)),
        ];
        let stage_refs: Vec<(&ResourceRef, Option<&ModelModalityContract>)> =
            stages.iter().map(|(component, contract)| (component, *contract)).collect();
        let view = compose_stage_modalities(&stage_refs);

        assert!(view.speech_capable);
        let (pipeline_in, pipeline_out) = view.pipeline_modalities();
        assert!(pipeline_in.contains(&ModelModality::Speech));
        assert!(pipeline_out.contains(&ModelModality::Speech));
        assert!(!pipeline_out.contains(&ModelModality::Text),
            "body output is the last stage's output; the STT stage's text must not leak");
        // An explicit refusal by a declared stage refutes the body claim and
        // names who withheld it.
        match view.interaction_support(InteractionCapability::RequestResponse) {
            ModalitySupport::Unsupported { reason } => {
                assert!(reason.contains("component/tts"), "reason names who withheld: {reason}");
            }
            other => panic!("request-response must not read as {other:?}"),
        }
        // Capabilities every declared stage carries, with an opaque stage in
        // the middle, are unknown rather than claimed: the opaque stage never
        // said whether it carries them.
        match view.interaction_support(InteractionCapability::StructuredEvents) {
            ModalitySupport::Unknown { reason } => {
                assert!(reason.contains("no modality contract"));
            }
            other => panic!(
                "body-level structured-events must not read as {other:?} while a stage is opaque"
            ),
        }
        assert!(!view.complete);
        assert!(view
            .basis
            .iter()
            .any(|line| line.contains("component/stt") && line.contains("transcribe-v2")));
        assert!(view
            .basis
            .iter()
            .any(|line| line.contains("component/text") && line.contains("no modality contract")));
        assert!(view
            .basis
            .iter()
            .any(|line| line.contains("component/tts") && line.contains("speak-v1")));
        // The per-stage facts stay exact: the TTS stage does stream.
        assert!(tts
            .interaction_support(InteractionCapability::StreamingOutput)
            .is_supported());
    }

    #[test]
    fn an_opaque_stage_turns_shared_capability_claims_into_unknowns() {
        let mut stt = ModelModalityContract::new(
            ProviderRef::parse("provider:example-stt").unwrap(),
            "transcribe-v2",
        );
        stt.input_modalities = BTreeSet::from([ModelModality::Speech]);
        stt.output_modalities = BTreeSet::from([ModelModality::Text]);
        stt.interaction.insert(InteractionCapability::BargeIn);
        let mut tts = ModelModalityContract::new(
            ProviderRef::parse("provider:example-tts").unwrap(),
            "speak-v1",
        );
        tts.input_modalities = BTreeSet::from([ModelModality::Text]);
        tts.output_modalities = BTreeSet::from([ModelModality::Speech]);
        tts.interaction.insert(InteractionCapability::BargeIn);
        let text_stage: Option<&ModelModalityContract> = None;

        let stages = [
            (r("component/stt"), Some(&stt) as Option<&ModelModalityContract>),
            (r("component/text"), text_stage),
            (r("component/tts"), Some(&tts)),
        ];
        let stage_refs: Vec<(&ResourceRef, Option<&ModelModalityContract>)> =
            stages.iter().map(|(component, contract)| (component, *contract)).collect();
        let view = compose_stage_modalities(&stage_refs);
        // Both declared stages carry barge-in, but the opaque text stage
        // never spoke, so the body cannot claim it end-to-end.
        assert!(matches!(
            view.interaction_support(InteractionCapability::BargeIn),
            ModalitySupport::Unknown { .. }
        ));
    }

    #[test]
    fn a_fully_declared_two_stage_body_supports_only_shared_interaction() {
        let mut stt = ModelModalityContract::new(
            ProviderRef::parse("provider:example-stt").unwrap(),
            "transcribe-v2",
        );
        stt.input_modalities = BTreeSet::from([ModelModality::Speech]);
        stt.output_modalities = BTreeSet::from([ModelModality::Text]);
        stt.interaction.insert(InteractionCapability::BargeIn);
        let mut s2s = realtime_contract();
        s2s.provider = ProviderRef::parse("provider:example-realtime").unwrap();

        let view = compose_stage_modalities(&[
            (&r("component/stt"), Some(&stt)),
            (&r("component/realtime"), Some(&s2s)),
        ]);
        assert!(view.complete);
        assert!(view.interaction_support(InteractionCapability::BargeIn).is_supported());
        stt.interaction.remove(&InteractionCapability::BargeIn);
        let view = compose_stage_modalities(&[
            (&r("component/stt"), Some(&stt)),
            (&r("component/realtime"), Some(&s2s)),
        ]);
        match view.interaction_support(InteractionCapability::BargeIn) {
            ModalitySupport::Unsupported { reason } => {
                assert!(reason.contains("component/stt"), "the reason names who withheld: {reason}");
            }
            other => panic!("barge-in must not survive one stage withholding it: {other:?}"),
        }
        // A capability no stage declares, with all stages declared, is
        // proven-absent at body level.
        assert!(matches!(
            view.interaction_support(InteractionCapability::PartialTranscripts),
            ModalitySupport::Unsupported { .. }
        ));
    }

    #[test]
    fn the_explanation_names_provider_native_provenance_and_honest_absence() {
        let read = crate::model_runtime::ModelRuntimeReadModel {
            version: crate::model_runtime::MODEL_RUNTIME_RELATION_VERSION.into(),
            project: None,
            agent: None,
            agency: None,
            harness: r("harness/realtime"),
            agent_session: Some("agent-session-1".into()),
            harness_composition_fingerprint: "fp".into(),
            relation: crate::model_runtime::ModelRuntimeRelation {
                model: crate::model_runtime::ModelVariantReading {
                    model: r("model:gpt-realtime"),
                    variant: "gpt-realtime".into(),
                },
                engine: crate::model_runtime::InferenceEngineReading {
                    engine: r("engine/openai-realtime"),
                    provider: ProviderRef::parse("provider:openai").unwrap(),
                    form: crate::model_runtime::InferenceEngineForm::ManagedService,
                    revision: None,
                    provider_native: BTreeMap::new(),
                },
                materialisation: crate::model_runtime::ModelMaterialisationReading {
                    binding_ref: "binding/realtime-1".into(),
                    workcell_ref: None,
                    placement: crate::model_runtime::PlacementObservation::Remote,
                    endpoint: Some("wss://example.invalid/v1/realtime".into()),
                    provider_native: BTreeMap::new(),
                    resources: crate::model_runtime::MaterialResourceReading::default(),
                    lifetime_owner: "caller".into(),
                    retraction: crate::composition::RetractionMode::Live,
                },
                model_surface: crate::model_runtime::ModelSurfaceReading {
                    contract: None,
                    protocol: "openai-realtime-websocket".into(),
                    capabilities: BTreeSet::new(),
                    access: crate::model_runtime::ModelAccessReading {
                        inference: crate::model_runtime::AccessFieldReading::available([
                            "invoke",
                        ]),
                        material_control: crate::model_runtime::AccessFieldReading::unavailable(
                            "provider owns lifecycle",
                        ),
                        interior: crate::model_runtime::AccessFieldReading::unavailable(
                            "no model-interior seam",
                        ),
                    },
                    modality: Some(realtime_contract()),
                },
                change_application: crate::model_runtime::RuntimeChangeApplication::Live,
            },
            components: Vec::new(),
            contracts: Vec::new(),
            surfaces: Vec::new(),
            unavailable: Vec::new(),
        };

        let evidence = explain_model_modality(&read);
        assert_eq!(evidence.subject, r("harness/realtime"));
        let summaries: Vec<&str> = evidence
            .facts
            .iter()
            .map(|fact| fact.summary.as_str())
            .collect();
        assert!(summaries.iter().any(|s| s.contains("input declared as")));
        assert!(summaries.iter().any(|s| s.contains("gpt-realtime")));
        assert!(summaries
            .iter()
            .any(|s| s.contains("full-duplex realtime is supported")));
        let barge_in = evidence
            .facts
            .iter()
            .find(|fact| fact.summary.contains("barge-in"))
            .unwrap();
        assert!(barge_in.summary.contains("is supported"));
        let provenance = &barge_in.provenance[0];
        assert_eq!(provenance.native_id.as_deref(), Some("gpt-realtime"));
        assert!(evidence
            .facts
            .iter()
            .any(|fact| fact.summary.contains("turn detection is supported")));
        let degradation = evidence
            .facts
            .iter()
            .find(|fact| fact.relation == "body-interaction-degraded");
        assert!(degradation.is_none(), "nothing was declared degraded");
    }

    #[test]
    fn the_explanation_of_a_contractless_body_says_so_instead_of_guessing() {
        let read = crate::model_runtime::ModelRuntimeReadModel {
            version: crate::model_runtime::MODEL_RUNTIME_RELATION_VERSION.into(),
            project: None,
            agent: None,
            agency: None,
            harness: r("harness/text"),
            agent_session: None,
            harness_composition_fingerprint: "fp".into(),
            relation: crate::model_runtime::ModelRuntimeRelation {
                model: crate::model_runtime::ModelVariantReading {
                    model: r("model:llama3.2"),
                    variant: "llama3.2:latest".into(),
                },
                engine: crate::model_runtime::InferenceEngineReading {
                    engine: r("engine/ollama"),
                    provider: ProviderRef::parse("provider:ollama").unwrap(),
                    form: crate::model_runtime::InferenceEngineForm::ManagedService,
                    revision: None,
                    provider_native: BTreeMap::new(),
                },
                materialisation: crate::model_runtime::ModelMaterialisationReading {
                    binding_ref: "binding/1".into(),
                    workcell_ref: None,
                    placement: crate::model_runtime::PlacementObservation::Local,
                    endpoint: None,
                    provider_native: BTreeMap::new(),
                    resources: crate::model_runtime::MaterialResourceReading::default(),
                    lifetime_owner: "caller".into(),
                    retraction: crate::composition::RetractionMode::Restart,
                },
                model_surface: crate::model_runtime::ModelSurfaceReading {
                    contract: None,
                    protocol: "openai-compatible-chat".into(),
                    capabilities: BTreeSet::new(),
                    access: crate::model_runtime::ModelAccessReading {
                        inference: crate::model_runtime::AccessFieldReading::available([
                            "invoke",
                        ]),
                        material_control: crate::model_runtime::AccessFieldReading::unavailable(
                            "provider owns lifecycle",
                        ),
                        interior: crate::model_runtime::AccessFieldReading::unavailable(
                            "no model-interior seam",
                        ),
                    },
                    modality: None,
                },
                change_application: crate::model_runtime::RuntimeChangeApplication::NextSession,
            },
            components: Vec::new(),
            contracts: Vec::new(),
            surfaces: Vec::new(),
            unavailable: Vec::new(),
        };
        let evidence = explain_model_modality(&read);
        assert_eq!(evidence.facts.len(), 1);
        assert!(evidence.facts[0].summary.contains("no modality contract"));
        assert!(evidence.facts[0].summary.contains("plain text body"));
    }

    #[test]
    fn modality_support_serialises_with_explicit_states() {
        let supported = serde_json::to_value(ModalitySupport::Supported).unwrap();
        assert_eq!(supported["state"], "supported");
        let degraded = serde_json::to_value(ModalitySupport::Degraded {
            reason: "half rate".into(),
        })
        .unwrap();
        assert_eq!(degraded["state"], "degraded");
        assert_eq!(degraded["reason"], "half rate");
    }
}
