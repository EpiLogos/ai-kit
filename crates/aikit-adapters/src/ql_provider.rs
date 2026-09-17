//! Production QL-MEF client binding over AIKit's existing provider seams.
//!
//! AIKit invokes the installed `ql` owner for discovery/negotiation and consumes
//! provider-owned readings when a transport exposes them. Generic MEF refraction
//! and source-qualified operative-scope currentness are distinct QL operations:
//! neither is emulated by overloading the other's lens/form fields. AIKit does
//! not parse QL expressions, mirror the QL registry, or create a second context
//! store.

use std::path::{Path, PathBuf};
use std::process::Command;

use aikit_core::ql::{
    QlInputLimits, QlOperation, QlProviderCapabilities, QlProviderClass, QlProviderClient,
    QlProviderFailure, QlProviderHealth, QlProviderRef, QlProviderState, QlReading,
    QlRefractionRequest, QL_OUTPUT_SCHEMA_VERSION,
};
use aikit_core::resource::operative_scope::{
    OperativeScope, ScopeAwareOperativeProvider, ScopeObservation,
};
use aikit_core::resource::{
    ActionSemanticProfile, AddressHorizon, OperativeSemanticProvider,
    OperativeSemanticProviderCapabilities, OperativeSemanticProviderDescriptor,
    OperativeSemanticProviderStatus, ProviderRef, RelationOp, ResolveExpression, ResolvePath,
    ResourceRef,
};
use aikit_core::{AikitError, Result};
use serde::Deserialize;
use serde_json::Value;

pub const QL_CLI_PROVIDER_VERSION: &str = "aikit.ql-cli-provider/v1";
pub const QL_OPERATIVE_SCOPE_CLIENT_VERSION: &str = "aikit.ql-operative-scope-client/v1";

#[derive(Debug, Clone)]
pub struct QlCliClient {
    executable: PathBuf,
}

impl Default for QlCliClient {
    fn default() -> Self {
        Self::new("ql")
    }
}

impl QlCliClient {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    fn json(&self, args: &[&str]) -> std::result::Result<Value, QlProviderFailure> {
        let output = Command::new(&self.executable)
            .args(args)
            .arg("--json")
            .output()
            .map_err(|error| {
                QlProviderFailure::new(
                    "ql.cli_spawn_failed",
                    format!("could not invoke {}: {error}", self.executable.display()),
                )
            })?;
        if !output.status.success() {
            return Err(QlProviderFailure::new(
                "ql.cli_failed",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        serde_json::from_slice(&output.stdout).map_err(|error| {
            QlProviderFailure::new(
                "ql.cli_invalid_json",
                format!("QL CLI returned invalid JSON: {error}"),
            )
        })
    }

    fn observed_capabilities(
        &self,
    ) -> std::result::Result<QlProviderCapabilities, QlProviderFailure> {
        let value = self.json(&["capabilities"])?;
        let version = value
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let service = value.get("service").cloned().unwrap_or(Value::Null);
        let state = service
            .get("providerState")
            .and_then(Value::as_str)
            .unwrap_or("absent");
        let detail = service
            .get("detail")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let health = QlProviderHealth {
            state: match state {
                "available" => QlProviderState::Available,
                "degraded" => QlProviderState::Degraded,
                "incompatible" => QlProviderState::Incompatible,
                _ => QlProviderState::Absent,
            },
            detail,
        };
        let mut operations = Vec::new();
        let mut deterministic_operations = Vec::new();
        if let Some(items) = service.get("operations").and_then(Value::as_array) {
            for item in items {
                if !item
                    .get("supported")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    continue;
                }
                let Some(operation) = item
                    .get("operation")
                    .and_then(Value::as_str)
                    .and_then(parse_operation)
                else {
                    continue;
                };
                operations.push(operation);
                if item
                    .get("deterministic")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    deterministic_operations.push(operation);
                }
            }
        }
        Ok(QlProviderCapabilities {
            provider: QlProviderRef {
                provider: "ql-mef".into(),
                version,
            },
            health,
            classes: vec![
                QlProviderClass::FormalKernel,
                QlProviderClass::SemanticRefraction,
            ],
            supported_forms: value
                .get("kernel")
                .and_then(|kernel| kernel.get("supportedForms"))
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            supported_lenses: Vec::new(),
            operations,
            extension_namespaces: vec!["vak".into(), "mef".into()],
            deterministic_operations,
            input_limits: QlInputLimits {
                max_relation_subjects: 4096,
                max_synthesis_readings: 4096,
            },
            output_schema_versions: vec![QL_OUTPUT_SCHEMA_VERSION.into()],
        })
    }
}

impl QlProviderClient for QlCliClient {
    fn capabilities(&self) -> QlProviderCapabilities {
        self.observed_capabilities()
            .unwrap_or_else(|failure| QlProviderCapabilities {
                provider: QlProviderRef {
                    provider: "ql-mef".into(),
                    version: "unobserved".into(),
                },
                health: QlProviderHealth {
                    state: QlProviderState::Absent,
                    detail: Some(format!("{}: {}", failure.code, failure.message)),
                },
                classes: Vec::new(),
                supported_forms: Vec::new(),
                supported_lenses: Vec::new(),
                operations: Vec::new(),
                extension_namespaces: Vec::new(),
                deterministic_operations: Vec::new(),
                input_limits: QlInputLimits {
                    max_relation_subjects: 0,
                    max_synthesis_readings: 0,
                },
                output_schema_versions: Vec::new(),
            })
    }

    fn refract(
        &self,
        _request: &QlRefractionRequest,
    ) -> std::result::Result<QlReading, QlProviderFailure> {
        let decision = self.json(&["service", "negotiate", "refract"])?;
        if !decision
            .get("supported")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(QlProviderFailure::new(
                "ql.refract_unavailable",
                "the installed QL owner does not currently expose provider-backed refract",
            ));
        }
        Err(QlProviderFailure::new(
            "ql.cli_refract_transport_unexposed",
            "QL negotiated refract but the installed CLI exposes no refraction dispatch command; use a QlProviderClient transport that owns refract rather than fabricating it in AIKit",
        ))
    }
}

/// Transport capability for the QL-owned operative binding. It is intentionally
/// separate from generic MEF `refract`: an operative interpretation such as
/// `ql/interpretation/c-prime` is not a LensRef and a World is not a QL FormRef.
/// The outer AIKit provider architecture remains `ScopeAwareOperativeProvider`;
/// this trait only describes what the concrete QL transport must be able to read.
pub trait QlOperativeScopeClient {
    fn observe_operative_scope(
        &self,
        requested: &OperativeScope,
    ) -> std::result::Result<QlReading, QlProviderFailure>;
}

impl QlOperativeScopeClient for QlCliClient {
    fn observe_operative_scope(
        &self,
        _requested: &OperativeScope,
    ) -> std::result::Result<QlReading, QlProviderFailure> {
        Err(QlProviderFailure::new(
            "ql.operative_scope_transport_unexposed",
            "the installed QL CLI has no source-backed operative-scope observation endpoint; do not substitute generic refract or echo the requested scope",
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QlOperativeScopeReading {
    operative_scope: OperativeScope,
    #[serde(default)]
    evidence: Vec<ResourceRef>,
}

/// Adapter from the existing transport-neutral QL client into the existing
/// operative semantic/scope provider architecture. The reading body remains QL-
/// owned JSON; AIKit only consumes the identity/evidence projection needed to
/// revalidate a scope at an effect point.
pub struct QlOperativeProvider<C> {
    client: C,
}

impl<C> QlOperativeProvider<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &C {
        &self.client
    }
}

impl<C: QlProviderClient> OperativeSemanticProvider for QlOperativeProvider<C> {
    type SemanticRef = Value;
    type ResourceReading = Value;
    type ActionProfile = Value;
    type Path = Value;

    fn descriptor(&self) -> OperativeSemanticProviderDescriptor {
        let capabilities = self.client.capabilities();
        let status = match capabilities.health.state {
            QlProviderState::Available => OperativeSemanticProviderStatus::Available,
            QlProviderState::Degraded => OperativeSemanticProviderStatus::Degraded {
                reason: capabilities
                    .health
                    .detail
                    .unwrap_or_else(|| "QL provider is degraded".into()),
            },
            QlProviderState::Absent | QlProviderState::Incompatible => {
                OperativeSemanticProviderStatus::Unavailable {
                    reason: capabilities
                        .health
                        .detail
                        .unwrap_or_else(|| "QL provider is unavailable".into()),
                }
            }
        };
        OperativeSemanticProviderDescriptor::new(
            ProviderRef::parse("provider/ql-mef").expect("static QL provider ref is valid"),
            status,
            OperativeSemanticProviderCapabilities::default(),
        )
    }

    fn bind_horizon(&self, _: AddressHorizon) -> Result<Option<Self::SemanticRef>> {
        Ok(None)
    }

    fn bind_relation(&self, _: RelationOp) -> Result<Option<Self::SemanticRef>> {
        Ok(None)
    }

    fn resource_readings(
        &self,
        _: &ResourceRef,
        _: Option<&ResolveExpression>,
    ) -> Result<Vec<Self::ResourceReading>> {
        Ok(Vec::new())
    }

    fn enrich_action(&self, _: &ActionSemanticProfile) -> Result<Option<Self::ActionProfile>> {
        Ok(None)
    }

    fn enrich_path(&self, _: &ResolvePath) -> Result<Option<Self::Path>> {
        Ok(None)
    }
}

impl<C: QlProviderClient + QlOperativeScopeClient> ScopeAwareOperativeProvider
    for QlOperativeProvider<C>
{
    fn observe_scope(
        &self,
        requested: &OperativeScope,
        _: &aikit_core::context_resolution::ContextResolution,
    ) -> Result<ScopeObservation> {
        let reading = self
            .client
            .observe_operative_scope(requested)
            .map_err(|failure| {
                AikitError::new(
                    "resolve.ql_scope_observation_failed",
                    format!("{}: {}", failure.code, failure.message),
                )
            })?;
        if reading.target.subject != requested.binding {
            return Ok(ScopeObservation::Stale {
                observed: requested.clone(),
                reason: "QL returned a scope reading for another binding subject".into(),
            });
        }
        let projection: QlOperativeScopeReading =
            serde_json::from_value(reading.reading).map_err(|error| {
                AikitError::new(
                    "resolve.ql_scope_reading_invalid",
                    format!(
                        "QL operative-scope reading does not expose the source-qualified binding: {error}"
                    ),
                )
            })?;
        let mut evidence = projection.evidence;
        evidence.extend(reading.evidence_refs);
        evidence.extend(
            reading
                .provenance
                .input_refs
                .into_iter()
                .map(|input| input.reference),
        );
        evidence.sort();
        evidence.dedup();
        Ok(ScopeObservation::Current {
            binding: projection.operative_scope,
            evidence,
        })
    }
}

fn parse_operation(value: &str) -> Option<QlOperation> {
    Some(match value {
        "capabilities" => QlOperation::Capabilities,
        "locate" => QlOperation::Locate,
        "refract" => QlOperation::Refract,
        "relate" => QlOperation::Relate,
        "synthesise" => QlOperation::Synthesise,
        _ => return None,
    })
}
