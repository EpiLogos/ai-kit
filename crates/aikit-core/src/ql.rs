//! Optional client-side QL/MEF interoperability.
//!
//! AIKit does not own QL semantics and does not require a QL provider in order to
//! resolve or operate ordinary contexts. These wire-facing types consume the
//! standalone QL-MEF Q3/Q4 contracts: capabilities are discovered explicitly,
//! client identity/revision remain opaque, and QL results are additive attachments
//! whose target and provenance must preserve the exact client subject supplied by
//! AIKit.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context_resolution::ContextResolution;
use crate::resource::ResourceRef;
use crate::{AikitError, Result};

pub const QL_PROVENANCE_SCHEMA_VERSION: &str = "1.1.0";
pub const QL_MEF_REGISTRY_VERSION: &str = "1.0.0-q2";
pub const QL_OUTPUT_SCHEMA_VERSION: &str = "ql-contract/1.1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QlMode {
    Disabled,
    Optional,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QlProviderState {
    Absent,
    Available,
    Degraded,
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlProviderHealth {
    pub state: QlProviderState,
    #[serde(default)]
    pub detail: Option<String>,
}

impl QlProviderHealth {
    pub fn absent() -> Self {
        Self {
            state: QlProviderState::Absent,
            detail: Some("QL provider not supplied".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlProviderRef {
    pub provider: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QlProviderClass {
    FormalKernel,
    SemanticRefraction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QlOperation {
    Capabilities,
    Locate,
    Refract,
    Relate,
    Synthesise,
}

impl QlOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::Locate => "locate",
            Self::Refract => "refract",
            Self::Relate => "relate",
            Self::Synthesise => "synthesise",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QlResultClass {
    Canonical,
    Deterministic,
    SemanticStochastic,
    Research,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlInputLimits {
    pub max_relation_subjects: usize,
    pub max_synthesis_readings: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlProviderCapabilities {
    pub provider: QlProviderRef,
    pub health: QlProviderHealth,
    pub classes: Vec<QlProviderClass>,
    pub supported_forms: Vec<String>,
    pub supported_lenses: Vec<String>,
    pub operations: Vec<QlOperation>,
    pub extension_namespaces: Vec<String>,
    pub deterministic_operations: Vec<QlOperation>,
    pub input_limits: QlInputLimits,
    pub output_schema_versions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlClientSubject {
    pub reference: ResourceRef,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub subject_type: Option<String>,
    #[serde(default)]
    pub frame_ref: Option<String>,
    #[serde(default)]
    pub context_refs: Vec<ResourceRef>,
}

impl QlClientSubject {
    pub fn new(reference: ResourceRef, revision: Option<String>) -> Self {
        Self {
            reference,
            revision,
            subject_type: None,
            frame_ref: None,
            context_refs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlRefractionRequest {
    pub subject: QlClientSubject,
    pub lens: String,
    #[serde(default)]
    pub sublens: Option<String>,
    #[serde(default)]
    pub frame: Option<String>,
}

impl QlRefractionRequest {
    pub fn new(subject: QlClientSubject, lens: impl Into<String>) -> Self {
        Self {
            subject,
            lens: lens.into(),
            sublens: None,
            frame: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlTargetView {
    pub subject: ResourceRef,
    #[serde(default)]
    pub subject_type: Option<String>,
    #[serde(default)]
    pub frame_ref: Option<String>,
    #[serde(default)]
    pub context_refs: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlInputRefRevision {
    pub reference: ResourceRef,
    #[serde(default)]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlProvenance {
    pub schema_version: String,
    pub mef_registry_version: String,
    pub provider: QlProviderRef,
    pub operation: String,
    pub input_refs: Vec<QlInputRefRevision>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub config_ref: Option<ResourceRef>,
    pub result_class: QlResultClass,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Consumer view of a QL-MEF semantic reading. Identity-bearing envelope fields
/// are explicit; the semantic disclosure itself stays provider-owned JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QlReading {
    pub id: ResourceRef,
    pub target: QlTargetView,
    pub operation: String,
    #[serde(default)]
    pub ql_form: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub lens: Option<String>,
    pub reading: Value,
    #[serde(default)]
    pub evidence_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub provenance: QlProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlProviderFailure {
    pub code: String,
    pub message: String,
}

impl QlProviderFailure {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Transport-neutral client seam. A process, RPC, MCP, embedded adapter, or test
/// provider can implement this without changing AIKit's ContextResolution logic.
pub trait QlProviderClient {
    fn capabilities(&self) -> QlProviderCapabilities;

    fn refract(
        &self,
        request: &QlRefractionRequest,
    ) -> std::result::Result<QlReading, QlProviderFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlProviderDiscovery {
    pub mode: QlMode,
    pub health: QlProviderHealth,
    #[serde(default)]
    pub capabilities: Option<QlProviderCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum QlAttachment {
    Disabled,
    Reading {
        health: QlProviderHealth,
        reading: Box<QlReading>,
    },
    Unavailable {
        health: QlProviderHealth,
        reason: String,
    },
    Failed {
        #[serde(default)]
        health: Option<QlProviderHealth>,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlProjectionRequest {
    pub mode: QlMode,
    pub refractions: Vec<QlRefractionRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QlProjectedRefraction {
    pub request: QlRefractionRequest,
    pub attachment: QlAttachment,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QlProjectedContext {
    /// The ordinary V2 context is retained unchanged. QL is an attachment field,
    /// not a replacement context ontology.
    pub context: ContextResolution,
    pub discovery: QlProviderDiscovery,
    pub refractions: Vec<QlProjectedRefraction>,
}

pub fn project_context_with_ql(
    context: &ContextResolution,
    provider: Option<&dyn QlProviderClient>,
    projection: QlProjectionRequest,
) -> Result<QlProjectedContext> {
    for request in &projection.refractions {
        validate_request(request)?;
    }

    if projection.mode == QlMode::Disabled {
        return Ok(QlProjectedContext {
            context: context.clone(),
            discovery: QlProviderDiscovery {
                mode: QlMode::Disabled,
                health: QlProviderHealth::absent(),
                capabilities: None,
            },
            refractions: projection
                .refractions
                .into_iter()
                .map(|request| QlProjectedRefraction {
                    request,
                    attachment: QlAttachment::Disabled,
                })
                .collect(),
        });
    }

    let Some(provider) = provider else {
        let health = QlProviderHealth::absent();
        if projection.mode == QlMode::Required && !projection.refractions.is_empty() {
            return Err(required_error(
                "QL provider is required but not supplied",
                &health,
            ));
        }
        return Ok(QlProjectedContext {
            context: context.clone(),
            discovery: QlProviderDiscovery {
                mode: projection.mode,
                health: health.clone(),
                capabilities: None,
            },
            refractions: projection
                .refractions
                .into_iter()
                .map(|request| QlProjectedRefraction {
                    request,
                    attachment: QlAttachment::Unavailable {
                        health: health.clone(),
                        reason: "QL provider not supplied".into(),
                    },
                })
                .collect(),
        });
    };

    let capabilities = provider.capabilities();
    let health = capabilities.health.clone();
    let discovery = QlProviderDiscovery {
        mode: projection.mode,
        health: health.clone(),
        capabilities: Some(capabilities.clone()),
    };

    if matches!(
        health.state,
        QlProviderState::Absent | QlProviderState::Incompatible
    ) {
        let reason = health
            .detail
            .clone()
            .unwrap_or_else(|| format!("QL provider is {}", provider_state_name(health.state)));
        if projection.mode == QlMode::Required && !projection.refractions.is_empty() {
            return Err(required_error(&reason, &health));
        }
        return Ok(QlProjectedContext {
            context: context.clone(),
            discovery,
            refractions: projection
                .refractions
                .into_iter()
                .map(|request| QlProjectedRefraction {
                    request,
                    attachment: QlAttachment::Unavailable {
                        health: health.clone(),
                        reason: reason.clone(),
                    },
                })
                .collect(),
        });
    }

    let mut refractions = Vec::with_capacity(projection.refractions.len());
    for request in projection.refractions {
        let attachment = match compatibility_reason(&capabilities, &request) {
            Some(reason) if projection.mode == QlMode::Required => {
                return Err(required_error(&reason, &health));
            }
            Some(reason) => QlAttachment::Unavailable {
                health: health.clone(),
                reason,
            },
            None => match provider.refract(&request) {
                Ok(reading) => match validate_reading(&reading, &request, &capabilities) {
                    Ok(()) => QlAttachment::Reading {
                        health: health.clone(),
                        reading: Box::new(reading),
                    },
                    Err(error) if projection.mode == QlMode::Required => return Err(error),
                    Err(error) => QlAttachment::Failed {
                        health: Some(health.clone()),
                        message: error.to_string(),
                    },
                },
                Err(failure) if projection.mode == QlMode::Required => {
                    return Err(AikitError::new(
                        "ql.required_provider_failure",
                        format!("required QL provider failed: {}", failure.message),
                    )
                    .with("provider_code", failure.code));
                }
                Err(failure) => QlAttachment::Failed {
                    health: Some(health.clone()),
                    message: format!("{}: {}", failure.code, failure.message),
                },
            },
        };
        refractions.push(QlProjectedRefraction {
            request,
            attachment,
        });
    }

    Ok(QlProjectedContext {
        context: context.clone(),
        discovery,
        refractions,
    })
}

fn compatibility_reason(
    capabilities: &QlProviderCapabilities,
    request: &QlRefractionRequest,
) -> Option<String> {
    if !capabilities
        .classes
        .contains(&QlProviderClass::SemanticRefraction)
    {
        return Some("provider does not advertise semantic-refraction capability".into());
    }
    if !capabilities.operations.contains(&QlOperation::Refract) {
        return Some("provider does not advertise refract".into());
    }
    if !capabilities
        .output_schema_versions
        .iter()
        .any(|version| version == QL_OUTPUT_SCHEMA_VERSION)
    {
        return Some(format!(
            "provider does not advertise required output schema {QL_OUTPUT_SCHEMA_VERSION}"
        ));
    }
    if !capabilities
        .supported_lenses
        .iter()
        .any(|lens| lens == &request.lens)
    {
        return Some(format!("provider does not advertise lens {}", request.lens));
    }
    if let Some(frame) = &request.frame {
        if !capabilities
            .supported_forms
            .iter()
            .any(|value| value == frame)
        {
            return Some(format!("provider does not advertise QL form {frame}"));
        }
    }
    None
}

fn validate_request(request: &QlRefractionRequest) -> Result<()> {
    if !lens_regex().is_match(&request.lens) {
        return Err(AikitError::new(
            "ql.invalid_lens_ref",
            format!("`{}` is not a canonical QL-MEF LensRef", request.lens),
        ));
    }
    if let Some(sublens) = &request.sublens {
        let captures = sublens_regex().captures(sublens).ok_or_else(|| {
            AikitError::new(
                "ql.invalid_sublens_ref",
                format!("`{sublens}` is not a canonical QL-MEF SublensRef"),
            )
        })?;
        let parent = captures
            .get(1)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let lens_parent = request
            .lens
            .strip_prefix("mef:lens:")
            .and_then(|value| value.strip_suffix("@1"))
            .unwrap_or_default();
        if parent != lens_parent {
            return Err(AikitError::new(
                "ql.sublens_lens_mismatch",
                format!("sublens {sublens} does not belong to lens {}", request.lens),
            ));
        }
    }
    if let Some(frame) = &request.frame {
        if !form_regex().is_match(frame) {
            return Err(AikitError::new(
                "ql.invalid_form_ref",
                format!("`{frame}` is not a canonical QL FormRef"),
            ));
        }
    }
    Ok(())
}

fn validate_reading(
    reading: &QlReading,
    request: &QlRefractionRequest,
    capabilities: &QlProviderCapabilities,
) -> Result<()> {
    let provenance = &reading.provenance;
    if reading.target.subject != request.subject.reference {
        return Err(AikitError::new(
            "ql.provider_identity_drift",
            "QL reading target does not preserve the requested client Ref",
        )
        .with("requested", request.subject.reference.as_str())
        .with("observed", reading.target.subject.as_str()));
    }
    if provenance.schema_version != QL_PROVENANCE_SCHEMA_VERSION {
        return Err(AikitError::new(
            "ql.provider_contract_violation",
            "QL reading provenance schema version mismatch",
        )
        .with("observed", provenance.schema_version.clone())
        .with("expected", QL_PROVENANCE_SCHEMA_VERSION));
    }
    if provenance.mef_registry_version != QL_MEF_REGISTRY_VERSION {
        return Err(AikitError::new(
            "ql.provider_contract_violation",
            "QL reading MEF registry version mismatch",
        )
        .with("observed", provenance.mef_registry_version.clone())
        .with("expected", QL_MEF_REGISTRY_VERSION));
    }
    if provenance.provider != capabilities.provider {
        return Err(AikitError::new(
            "ql.provider_contract_violation",
            "QL reading provenance provider/version differs from discovered provider",
        ));
    }
    if reading.operation != QlOperation::Refract.as_str()
        || provenance.operation != QlOperation::Refract.as_str()
    {
        return Err(AikitError::new(
            "ql.provider_contract_violation",
            "QL reading operation/provenance is not refract",
        ));
    }
    let expected_revision = request.subject.revision.as_deref();
    let exact_input = provenance.input_refs.iter().any(|input| {
        input.reference == request.subject.reference
            && input.revision.as_deref() == expected_revision
    });
    if !exact_input {
        return Err(AikitError::new(
            "ql.provider_identity_drift",
            "QL reading provenance does not preserve the requested client Ref/revision",
        )
        .with("reference", request.subject.reference.as_str())
        .with("revision", expected_revision.unwrap_or("<none>")));
    }
    Ok(())
}

fn required_error(message: &str, health: &QlProviderHealth) -> AikitError {
    AikitError::new("ql.required_unavailable", message)
        .with("provider_state", provider_state_name(health.state))
}

const fn provider_state_name(state: QlProviderState) -> &'static str {
    match state {
        QlProviderState::Absent => "absent",
        QlProviderState::Available => "available",
        QlProviderState::Degraded => "degraded",
        QlProviderState::Incompatible => "incompatible",
    }
}

fn lens_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| Regex::new(r"^mef:lens:L[0-5]'?@1$").expect("canonical LensRef regex"))
}

fn sublens_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"^mef:sublens:(L[0-5]'?)\.[0-5]@1$").expect("canonical SublensRef regex")
    })
}

fn form_regex() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"^qlform:(sixfold|four-plus-two|direct-conjugate)@1$")
            .expect("canonical QlFormRef regex")
    })
}
