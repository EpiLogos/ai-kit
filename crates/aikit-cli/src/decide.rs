//! Optional, local-first decision-provider surface for
//! `action/model/decide`.
//!
//! The owner elects exactly one placement:
//!
//! - `none` — no decision service. Ordinary operation never requires one and
//!   never falls back to hosted inference on its own;
//! - `managed-local` — the Workcell-owned local model service on loopback
//!   (recommended where installed);
//! - `endpoint` — an existing self-hosted SystemOne-compatible endpoint the
//!   operator already runs (beyond loopback this requires HTTPS and an
//!   explicit `allow_remote` election);
//! - `hosted` — the hosted TypeSafe/Jev API under its own credential, tariff
//!   and concrete-version law.
//!
//! The decision provider is independent of the acting (coding/writing)
//! models: it is never resolved from the model roster, and acting-provider
//! disclosure does not cover it. Selecting local serving of private text is
//! not an election to send that text to any cloud worker. A local or
//! self-hosted endpoint has no price source, so there is no tariff here and
//! none is invented; usage is still required and recorded on every answer.

use crate::cli::{DecideInvokeArgs, DecideStatusArgs};
use crate::jev_now::{fail, minted_invocation_ref, read_bytes, read_json};
use aikit_adapters::decision_endpoint::{
    probe_models, DecisionEndpoint, DecisionInvocation, EndpointDecisionProvider,
};
use aikit_adapters::jev::{
    CurlJevProvider, JevBoundary, JevCancellation, JevEndpoint, JevInvocation,
};
use aikit_adapters::secret_resolver::SuiteSecretResolver;
use aikit_core::jev::{DecisionLimits, JevLimits, JevRequest, JEV_ACTION_REF};
use aikit_core::secret_ref::{SecretRef, SecretResolver};
use aikit_core::{AikitError, ResourceRef, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const DECISION_PROVIDER_SCHEMA: &str = "aikit.decision-provider/v1";

/// The recorded identity of the installed decision artifact. This is
/// disclosure metadata the operator pins at install time; the live model card
/// probe reports what the endpoint actually serves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionModelIdentity {
    /// Model family/recipe (for example `kev`).
    pub family: String,
    /// Upstream artifact identity (for example `jaredpalmer/kev-0.8b`).
    pub artifact: String,
    /// Base weights identity with its pinned revision.
    pub base: String,
    pub base_revision: String,
    /// Serving runtime/backend pinned for this installation.
    pub runtime: String,
    pub backend: String,
    pub precision: String,
    /// The calibration basis of the shipped checkpoint (for a fitted
    /// temperature: what fitted it). Thresholds are never copied from another
    /// provider's calibration.
    pub calibration: String,
    pub license: String,
    /// Artifact size facts for the operator (weights + head, bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionProviderMode {
    None,
    ManagedLocal,
    Endpoint,
    Hosted,
}

/// One elected decision-provider placement. Mode-specific fields are
/// validated by `validate`, so a local election cannot quietly carry hosted
/// tariff fields and vice versa.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionProviderConfig {
    pub schema: String,
    pub mode: DecisionProviderMode,
    /// host:port of the endpoint placements. Secrets never appear here.
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub allow_remote: bool,
    /// Optional bearer credential for endpoint placements (an explicitly
    /// unauthenticated local server needs none); required for hosted.
    #[serde(default)]
    pub credential_ref: Option<SecretRef>,
    /// Endpoint placement bounds. There is no tariff by law.
    #[serde(default)]
    pub limits: Option<DecisionLimits>,
    /// Hosted placement bounds: the existing Jev law unchanged.
    #[serde(default)]
    pub jev_limits: Option<JevLimits>,
    /// Recorded installed-artifact identity for disclosure.
    #[serde(default)]
    pub decision_model: Option<DecisionModelIdentity>,
}

impl DecisionProviderConfig {
    pub fn validate(&self) -> Result<()> {
        if self.schema != DECISION_PROVIDER_SCHEMA {
            return Err(fail(
                "decision.config_schema",
                "Unsupported decision-provider configuration schema",
            ));
        }
        match self.mode {
            DecisionProviderMode::None => {
                if self.address.is_some()
                    || self.limits.is_some()
                    || self.jev_limits.is_some()
                    || self.allow_remote
                {
                    return Err(fail(
                        "decision.config_invalid",
                        "mode none carries no endpoint, limits or remote election; the ordinary path is the path",
                    ));
                }
            }
            DecisionProviderMode::ManagedLocal => {
                if self.address.as_ref().is_some_and(|a| a.trim().is_empty()) || self.limits.is_none() {
                    return Err(fail(
                        "decision.config_invalid",
                        "managed-local requires the service address and bounded limits",
                    ));
                }
                if self.allow_remote {
                    return Err(fail(
                        "decision.config_invalid",
                        "the managed local service is loopback by law; allow_remote does not apply",
                    ));
                }
            }
            DecisionProviderMode::Endpoint => {
                if self.address.as_ref().is_some_and(|a| a.trim().is_empty()) || self.limits.is_none() {
                    return Err(fail(
                        "decision.config_invalid",
                        "endpoint requires the address and bounded limits",
                    ));
                }
            }
            DecisionProviderMode::Hosted => {
                if self.credential_ref.is_none() || self.jev_limits.is_none() {
                    return Err(fail(
                        "decision.config_invalid",
                        "hosted requires a native credential reference and explicit JevLimits",
                    ));
                }
                if self.address.is_some() || self.limits.is_some() {
                    return Err(fail(
                        "decision.config_invalid",
                        "hosted uses the official TypeSafe endpoint; address/limits are endpoint-placement fields",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn endpoint(&self) -> Result<DecisionEndpoint> {
        match self.mode {
            DecisionProviderMode::ManagedLocal | DecisionProviderMode::Endpoint => {
                let address = self
                    .address
                    .as_deref()
                    .ok_or_else(|| fail("decision.config_invalid", "No endpoint address configured"))?;
                DecisionEndpoint::resolve(address, self.allow_remote)
            }
            DecisionProviderMode::None => Err(fail(
                "decision.provider_disabled",
                "No decision provider is elected; the ordinary path does not use one",
            )),
            DecisionProviderMode::Hosted => Err(fail(
                "decision.config_invalid",
                "hosted placement uses the TypeSafe transport, not a decision endpoint",
            )),
        }
    }

    pub fn limits(&self) -> Result<&DecisionLimits> {
        self.limits.as_ref().ok_or_else(|| {
            fail("decision.config_invalid", "Endpoint placement requires bounded limits")
        })
    }
}

/// A completed decision invocation under either transport family, exposing
/// the parts selection, attribution and the prepared basis need.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum DecisionReceipt {
    Endpoint(DecisionInvocation),
    Hosted(JevInvocation),
}

impl DecisionReceipt {
    pub fn answer(&self) -> Option<&aikit_core::jev::JevResponse> {
        match self {
            Self::Endpoint(receipt) => receipt.answer.as_ref(),
            Self::Hosted(receipt) => receipt.answer.as_ref(),
        }
    }
    pub fn invocation_ref(&self) -> &ResourceRef {
        match self {
            Self::Endpoint(receipt) => &receipt.invocation_ref,
            Self::Hosted(receipt) => &receipt.invocation_ref,
        }
    }
    pub fn failure_message(&self) -> Option<String> {
        match self {
            Self::Endpoint(receipt) => receipt.failure.as_ref().map(|f| f.message.clone()),
            Self::Hosted(receipt) => receipt.failure.as_ref().map(|f| f.message.clone()),
        }
    }
    pub fn outcome_completed(&self) -> bool {
        match self {
            Self::Endpoint(receipt) => {
                receipt.outcome == aikit_adapters::jev::JevOutcome::Completed
            }
            Self::Hosted(receipt) => receipt.outcome == aikit_adapters::jev::JevOutcome::Completed,
        }
    }
    /// The identity digest that joins the prepared basis: mode, standing,
    /// endpoint, selected model and the recorded artifact identity. A
    /// provider, runtime or calibration change moves it.
    pub fn provider_identity_digest(config: &DecisionProviderConfig) -> Result<String> {
        let standing = match config.mode {
            DecisionProviderMode::ManagedLocal | DecisionProviderMode::Endpoint => {
                serde_json::to_value(config.endpoint()?.standing())
                    .map_err(|e| fail("decision.encode", e.to_string()))?
            }
            _ => json!("hosted"),
        };
        let identity = json!({
            "mode": config.mode,
            "standing": standing,
            "address": config.address,
            "model": config.limits().ok().map(|l| &l.model),
            "decision_model": config.decision_model,
        });
        let bytes = serde_json::to_vec(&identity)
            .map_err(|e| fail("decision.encode", format!("provider identity: {e}")))?;
        Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
    }
}

fn resolve_optional_credential(
    config: &DecisionProviderConfig,
    allow_env_import: bool,
) -> Result<Option<aikit_core::SecretValue>> {
    config
        .credential_ref
        .as_ref()
        .map(|reference| {
            let resolver = if allow_env_import {
                SuiteSecretResolver::with_env_import()
            } else {
                SuiteSecretResolver::default()
            };
            resolver.resolve(reference)
        })
        .transpose()
}

/// Invoke the elected provider with one typed request. Endpoint placements
/// never fall back to hosted inference: if the local service is unavailable
/// the failure is the answer, visibly.
pub fn invoke_selected(
    config: &DecisionProviderConfig,
    request: &JevRequest,
    invocation_ref: ResourceRef,
    curl: Option<PathBuf>,
    allow_env_import: bool,
    revalidate: &mut dyn FnMut() -> Result<()>,
) -> Result<DecisionReceipt> {
    config.validate()?;
    let cancellation = JevCancellation::default();
    let curl = curl.unwrap_or_else(|| PathBuf::from("curl"));
    match config.mode {
        DecisionProviderMode::None => Err(fail(
            "decision.provider_disabled",
            "No decision provider is elected; the ordinary path does not use one",
        )),
        DecisionProviderMode::ManagedLocal | DecisionProviderMode::Endpoint => {
            let endpoint = config.endpoint()?;
            let limits = config.limits()?.clone();
            let secret = resolve_optional_credential(config, allow_env_import)?;
            let provider = EndpointDecisionProvider::new(curl, endpoint);
            let mut guard = |_: JevBoundary| -> Result<()> {
                revalidate()?;
                Ok(())
            };
            let receipt = provider.invoke(
                invocation_ref,
                request,
                &limits,
                secret.as_ref(),
                &cancellation,
                &mut guard,
            )?;
            Ok(DecisionReceipt::Endpoint(receipt))
        }
        DecisionProviderMode::Hosted => {
            let limits: JevLimits = config
                .jev_limits
                .clone()
                .ok_or_else(|| fail("decision.config_invalid", "hosted requires JevLimits"))?;
            let credential_ref = config
                .credential_ref
                .clone()
                .ok_or_else(|| fail("decision.config_invalid", "hosted requires a credential"))?;
            let resolver = if allow_env_import {
                SuiteSecretResolver::with_env_import()
            } else {
                SuiteSecretResolver::default()
            };
            let secret = resolver.resolve(&credential_ref)?;
            let initial_material_digest = blake3::hash(secret.expose().as_bytes());
            let provider =
                CurlJevProvider::new(curl, JevEndpoint::Official);
            let mut guard = |_: JevBoundary| -> Result<()> {
                revalidate()?;
                let current = resolver.resolve(&credential_ref)?;
                if blake3::hash(current.expose().as_bytes()) != initial_material_digest {
                    return Err(fail(
                        "jev.credential_changed",
                        "The hosted credential changed during the invocation",
                    ));
                }
                Ok(())
            };
            let receipt = provider.invoke(
                invocation_ref,
                request,
                &limits,
                &secret,
                &cancellation,
                &mut guard,
            )?;
            Ok(DecisionReceipt::Hosted(receipt))
        }
    }
}

/// `aikit decide status`: the working face of the election — actual
/// placement, selected model, install/load state, license, download/resource
/// facts and (with `--probe`) one real bounded typed diagnostic. Mode `none`
/// reports that ordinary operation uses no decision service.
pub fn decide_status(args: DecideStatusArgs) -> Result<Value> {
    let config: DecisionProviderConfig =
        read_json(&args.provider_file, "Decision provider config", 256 * 1024)?;
    config.validate()?;
    let mut status = json!({
        "schema": "aikit.decision-status/v1",
        "action_ref": JEV_ACTION_REF,
        "mode": config.mode,
        "independent_of_acting_models": true,
    });
    if config.mode == DecisionProviderMode::None {
        status["placement"] = json!(null);
        status["ordinary_path"] = json!(
            "No decision service is elected; ordinary operation does not require one and nothing falls back to hosted inference on its own."
        );
        return Ok(status);
    }
    let curl = Some(args.curl.unwrap_or_else(|| PathBuf::from("curl")));
    match config.mode {
        DecisionProviderMode::None => return Ok(status),
        DecisionProviderMode::ManagedLocal | DecisionProviderMode::Endpoint => {
            let endpoint = config.endpoint()?;
            let limits = config.limits()?;
            status["placement"] = json!(config.address);
            status["standing"] = json!(endpoint.standing());
            status["selected_model"] = json!(limits.model);
            status["limits"] = json!({
                "timeout_ms": limits.timeout_ms,
                "max_attempts": limits.max_attempts,
                "max_input_tokens_per_attempt": limits.max_input_tokens_per_attempt,
                "max_output_tokens_per_attempt": limits.max_output_tokens_per_attempt,
                "tariff": "none — a local/self-hosted endpoint has no price source; none is invented",
            });
            status["credential"] = json!(if config.credential_ref.is_some() {
                "configured bearer (native secret reference)"
            } else {
                "explicitly unauthenticated local serving"
            });
            // Load state is what the endpoint actually answers, not what was
            // declared: reachability plus its own model card.
            match probe_models(
                curl.clone().expect("curl path"),
                &endpoint,
                2000,
                resolve_optional_credential(&config, args.allow_env_import)?.as_ref(),
            ) {
                Ok(card) => {
                    status["install"] = json!({"state": "loaded", "basis": "endpoint answered its model card"});
                    status["served"] = card;
                }
                Err(unavailable) => {
                    status["install"] = json!({
                        "state": "unavailable",
                        "basis": "endpoint did not answer its model card",
                        "reason": unavailable.message(),
                    });
                }
            }
            if let Some(model) = &config.decision_model {
                status["decision_model"] = serde_json::to_value(model)
                    .map_err(|e| fail("decision.encode", e.to_string()))?;
            }
        }
        DecisionProviderMode::Hosted => {
            let limits = config
                .jev_limits
                .as_ref()
                .ok_or_else(|| fail("decision.config_invalid", "hosted requires JevLimits"))?;
            status["placement"] = json!("official TypeSafe endpoint (hosted)");
            status["selected_model"] = json!(limits.tariff.model_version);
            status["limits"] = json!({
                "timeout_ms": limits.timeout_ms,
                "max_attempts": limits.max_attempts,
                "tariff_source": limits.tariff.source,
                "reserved_microusd_bound": limits.max_total_reserved_microusd,
            });
            status["credential"] = json!("configured (native secret reference)");
        }
    }
    if args.probe {
        status["diagnostic"] = diagnostic_probe(&config, curl, args.allow_env_import)?;
    }
    Ok(status)
}

fn diagnostic_probe(
    config: &DecisionProviderConfig,
    curl: Option<PathBuf>,
    allow_env_import: bool,
) -> Result<Value> {
    let model = match config.mode {
        DecisionProviderMode::Hosted => config
            .jev_limits
            .as_ref()
            .map(|l| l.tariff.model_version.clone())
            .unwrap_or_default(),
        _ => config.limits()?.model.clone(),
    };
    let mut questions = std::collections::BTreeMap::new();
    questions.insert(
        "diagnostic".to_string(),
        aikit_core::jev::Question::Noul {
            instructions: json!({"question": "Diagnostic probe: answer true.", "purpose": "aikit decide status --probe"}),
            criteria: None,
        },
    );
    let request = JevRequest {
        model,
        state: json!({"diagnostic": true}),
        questions,
    };
    let invocation_ref = minted_invocation_ref(&request)?;
    let started = std::time::Instant::now();
    let mut no_revalidate = || -> Result<()> { Ok(()) };
    let receipt = invoke_selected(
        config,
        &request,
        invocation_ref,
        curl,
        allow_env_import,
        &mut no_revalidate,
    )?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match receipt.answer() {
        Some(answer) => Ok(json!({
            "outcome": "completed",
            "probe": "noul",
            "answer": answer.answers.get("diagnostic"),
            "returned_model": answer.model,
            "usage": answer.usage,
            "elapsed_ms": elapsed_ms,
        })),
        None => Ok(json!({
            "outcome": "failed",
            "reason": receipt.failure_message().unwrap_or_else(|| "no answer".into()),
            "elapsed_ms": elapsed_ms,
        })),
    }
}

/// `aikit decide invoke`: one real typed invocation through the elected
/// provider, returning its bounded receipt.
pub fn decide_invoke(args: DecideInvokeArgs) -> Result<Value> {
    let config: DecisionProviderConfig =
        read_json(&args.provider_file, "Decision provider config", 256 * 1024)?;
    let request = JevRequest::parse(&read_bytes(&args.request_file, "Decision request", 1024 * 1024)?)?;
    let invocation_ref = args
        .invocation_ref
        .as_deref()
        .map(ResourceRef::parse)
        .transpose()?
        .unwrap_or(minted_invocation_ref(&request)?);
    let mut no_revalidate = || -> Result<()> { Ok(()) };
    let receipt = invoke_selected(
        &config,
        &request,
        invocation_ref,
        args.curl,
        args.allow_env_import,
        &mut no_revalidate,
    )?;
    serde_json::to_value(receipt).map_err(|e| fail("decision.encode", format!("receipt: {e}")))
}

/// Parse a decision-provider configuration from bytes already read elsewhere
/// (the NOW-preparation selection path).
pub fn parse_provider_config(bytes: &[u8]) -> Result<DecisionProviderConfig> {
    serde_json::from_slice(bytes)
        .map_err(|e| fail("decision.invalid_config", format!("Decision provider config: {e}")))
}

/// A mode-none refusal carries the ordinary-path meaning, not a generic error.
pub fn provider_disabled_error() -> AikitError {
    fail(
        "decision.provider_disabled",
        "No decision provider is elected; the ordinary path does not use one",
    )
}
