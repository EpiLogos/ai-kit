//! Optional, local-first decision-provider surface for
//! `action/model/decide`.
//!
//! The owner elects exactly one placement:
//!
//! - `none` — no decision service. Ordinary operation never requires one and
//!   never falls back to hosted inference on its own;
//! - `managed-local` — a loopback model service whose lifecycle a product
//!   owns where that product is installed (Workcell's declared services). The
//!   election carries no Workcell requirement: validation and invocation need
//!   only the loopback address and limits, never a `workcell` executable;
//! - `endpoint` — a SystemOne-compatible endpoint (beyond loopback this
//!   requires HTTPS and an explicit `allow_remote` election). This is also the
//!   honest placement for a local service AIKit itself provisions, starts,
//!   stops, restarts and upgrades (`aikit decide service`, `decide_service.rs`),
//!   so an installation without Workcell has the complete local lifecycle
//!   without being labelled Workcell-managed;
//! - `hosted` — the hosted TypeSafe/Jev API under its own credential, tariff
//!   and concrete-version law.
//!
//! The decision provider is independent of the acting (coding/writing)
//! models: it is never resolved from the model roster, and acting-provider
//! disclosure does not cover it. Selecting local serving of private text is
//! not an election to send that text to any cloud worker. A local or
//! self-hosted endpoint has no price source, so there is no tariff here and
//! none is invented; usage is still required and recorded on every answer.

use crate::cli::{DecideAsksArgs, DecideInvokeArgs, DecideStatusArgs};
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
    /// The checkpoint's trained state envelope in tokens. Inputs beyond it
    /// work but degrade on small models, so keep shared states lean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trained_state_tokens: Option<u64>,
    /// Operator-recorded serving notes (cache, warm-up, ceiling rationale).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
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
                if self.address.as_ref().is_some_and(|a| a.trim().is_empty())
                    || self.limits.is_none()
                {
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
                if self.address.as_ref().is_some_and(|a| a.trim().is_empty())
                    || self.limits.is_none()
                {
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
                let address = self.address.as_deref().ok_or_else(|| {
                    fail("decision.config_invalid", "No endpoint address configured")
                })?;
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
            fail(
                "decision.config_invalid",
                "Endpoint placement requires bounded limits",
            )
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
            let provider = CurlJevProvider::new(curl, JevEndpoint::Official);
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
    let provider_file = crate::local_services_election::resolve_file(
        &aikit_store::AikitHome::discover()?,
        crate::local_services_election::Election::Decision,
        args.provider_file.clone(),
    )?;
    let config: DecisionProviderConfig =
        read_json(&provider_file, "Decision provider config", 256 * 1024)?;
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
            // declared: reachability plus its own model card. The probe
            // borrows the elected invocation timeout (bounded to 30s) so a
            // busy machine shows slow-but-loaded rather than flapping to
            // unavailable while the same status run's probe answers.
            match probe_models(
                curl.clone().expect("curl path"),
                &endpoint,
                limits.timeout_ms.clamp(2_000, 30_000),
                resolve_optional_credential(&config, args.allow_env_import)?.as_ref(),
            ) {
                Ok(card) => {
                    status["install"] =
                        json!({"state": "loaded", "basis": "endpoint answered its model card"});
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
/// provider, returning its bounded receipt. The ask and its answer, or why
/// none came, are kept against the asking agent session (`--agent-session`,
/// else this process's own `AIKIT_SESSION_ID`); with neither, nowhere.
pub fn decide_invoke(args: DecideInvokeArgs) -> Result<Value> {
    let home = aikit_store::AikitHome::discover()?;
    let asking = asking_session(args.agent_session.as_deref())?;
    let provider_file = crate::local_services_election::resolve_file(
        &home,
        crate::local_services_election::Election::Decision,
        args.provider_file.clone(),
    )?;
    let config: DecisionProviderConfig =
        read_json(&provider_file, "Decision provider config", 256 * 1024)?;
    let request = JevRequest::parse(&read_bytes(
        &args.request_file,
        "Decision request",
        1024 * 1024,
    )?)?;
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
        invocation_ref.clone(),
        args.curl,
        args.allow_env_import,
        &mut no_revalidate,
    );
    if let Some((session_id, agent_session)) = &asking {
        keep_ask(
            &home,
            session_id,
            agent_session.as_ref(),
            &request,
            &invocation_ref,
            receipt.as_ref(),
        )?;
    }
    serde_json::to_value(receipt?).map_err(|e| fail("decision.encode", format!("receipt: {e}")))
}

/// `aikit.decision-ask/v1`: one typed request an agent session put to the
/// decision helper, kept with that session (O-I #684; DLW manual §19.4, the
/// thread's Ask row): the questions with the labels each allowed, and the
/// answer or why none came. An answer is evidence that the engine checks: it
/// is never a receipt and never stands for what the agent saw. The shared
/// state the questions were asked over is not kept; its digest is.
pub const DECISION_ASK_SCHEMA: &str = "aikit.decision-ask/v1";
/// `aikit decide asks`: one session's kept asks, oldest first.
pub const DECISION_ASKS_SCHEMA: &str = "aikit.decision-asks/v1";
const ASKS_FILE: &str = "decision-asks.jsonl";
/// The most asks one read returns: the latest, with the count of earlier ones.
const ASKS_READ_LIMIT: usize = 256;
const ASKS_BYTES_LIMIT: u64 = 16 * 1024 * 1024;

/// The session an ask is kept against: the one named, else the one this
/// process runs as (AIKit gives an agent session's process `AIKIT_SESSION_ID`,
/// `SessionId::of_agent_session`), else none.
fn asking_session(
    named: Option<&str>,
) -> Result<Option<(aikit_core::SessionId, Option<ResourceRef>)>> {
    if let Some(named) = named {
        let session = ResourceRef::parse(named)?;
        return Ok(Some((
            aikit_core::SessionId::of_agent_session(&session),
            Some(session),
        )));
    }
    match std::env::var("AIKIT_SESSION_ID") {
        Ok(own) if !own.trim().is_empty() => {
            Ok(Some((aikit_core::SessionId::parse(own.trim())?, None)))
        }
        _ => Ok(None),
    }
}

fn asks_path(home: &aikit_store::AikitHome, session: &aikit_core::SessionId) -> PathBuf {
    home.session_dir(session).join(ASKS_FILE)
}

/// Append one ask to its session's record, under the session's ask lock.
fn keep_ask(
    home: &aikit_store::AikitHome,
    session_id: &aikit_core::SessionId,
    agent_session: Option<&ResourceRef>,
    request: &JevRequest,
    invocation_ref: &ResourceRef,
    outcome: std::result::Result<&DecisionReceipt, &AikitError>,
) -> Result<()> {
    let mut ask = json!({
        "schema": DECISION_ASK_SCHEMA,
        "asked_at": aikit_store::Timestamp::now().to_string(),
        "session_id": session_id.as_str(),
        "invocation_ref": invocation_ref.as_str(),
        "request_digest": request.digest()?,
        "model": request.model,
        "questions": request.questions,
    });
    if let Some(agent_session) = agent_session {
        ask["agent_session"] = json!(agent_session.as_str());
    }
    match outcome {
        Ok(receipt) => match receipt.answer().filter(|_| receipt.outcome_completed()) {
            Some(answer) => {
                ask["outcome"] = json!("answered");
                ask["answered_by"] = json!(answer.model);
                ask["answers"] = json!(answer.answers);
            }
            None => {
                ask["outcome"] = json!("unanswered");
                ask["failure"] = json!(receipt
                    .failure_message()
                    .unwrap_or_else(|| "The provider returned no answer".into()));
            }
        },
        Err(error) => {
            ask["outcome"] = json!("unanswered");
            ask["failure"] = json!(error.to_string());
        }
    }
    let line =
        serde_json::to_string(&ask).map_err(|e| fail("decision.encode", format!("ask: {e}")))?;
    let _lock = aikit_store::ContextLock::acquire(
        home,
        &format!("decision-asks-{}", session_id.as_str()),
        aikit_store::LockOptions::default().with_purpose("keep a decision ask"),
    )?;
    home.ensure_session_dir(session_id)?;
    let path = asks_path(home, session_id);
    let unkept = |e: std::io::Error| {
        fail(
            "decision.ask_unkept",
            format!(
                "The answer came, but it could not be kept with the session at {}: {e}",
                path.display()
            ),
        )
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(unkept)?;
    use std::io::Write as _;
    file.write_all(format!("{line}\n").as_bytes())
        .map_err(unkept)
}

/// `aikit decide asks --agent-session <ref>`: the asks kept against that
/// session, oldest first. Read-only; a session that asked nothing has none.
pub fn decide_asks(args: DecideAsksArgs) -> Result<Value> {
    let home = aikit_store::AikitHome::discover()?;
    let session = ResourceRef::parse(&args.agent_session)?;
    let session_id = aikit_core::SessionId::of_agent_session(&session);
    let path = asks_path(&home, &session_id);
    let mut asks = Vec::new();
    let mut unreadable = 0usize;
    match std::fs::File::open(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(fail(
                "decision.asks_unreadable",
                format!("{}: {error}", path.display()),
            ))
        }
        Ok(file) => {
            use std::io::{Read as _, Seek as _};
            let mut file = file;
            let length = file.metadata().map(|m| m.len()).unwrap_or(0);
            let skip = length.saturating_sub(ASKS_BYTES_LIMIT);
            file.seek(std::io::SeekFrom::Start(skip))
                .and_then(|_| {
                    let mut text = String::new();
                    file.read_to_string(&mut text).map(|_| text)
                })
                .map_err(|e| {
                    fail(
                        "decision.asks_unreadable",
                        format!("{}: {e}", path.display()),
                    )
                })
                .map(|text| {
                    // A tail read starts mid-line: its first, partial line is not an ask.
                    for line in text.lines().skip(usize::from(skip > 0)) {
                        match serde_json::from_str::<Value>(line) {
                            Ok(ask)
                                if ask["schema"] == DECISION_ASK_SCHEMA
                                    && ask["session_id"] == session_id.as_str() =>
                            {
                                asks.push(ask)
                            }
                            // A line torn by an interrupted write, or another record, is counted, never shown.
                            _ if !line.trim().is_empty() => unreadable += 1,
                            _ => {}
                        }
                    }
                })?;
        }
    }
    let earlier = asks.len().saturating_sub(ASKS_READ_LIMIT);
    let asks = asks.split_off(earlier);
    Ok(json!({
        "schema": DECISION_ASKS_SCHEMA,
        "agent_session": session.as_str(),
        "session_id": session_id.as_str(),
        "asks": asks,
        "earlier": earlier,
        "unreadable": unreadable,
    }))
}

/// Parse a decision-provider configuration from bytes already read elsewhere
/// (the NOW-preparation selection path).
pub fn parse_provider_config(bytes: &[u8]) -> Result<DecisionProviderConfig> {
    serde_json::from_slice(bytes).map_err(|e| {
        fail(
            "decision.invalid_config",
            format!("Decision provider config: {e}"),
        )
    })
}

/// A mode-none refusal carries the ordinary-path meaning, not a generic error.
pub fn provider_disabled_error() -> AikitError {
    fail(
        "decision.provider_disabled",
        "No decision provider is elected; the ordinary path does not use one",
    )
}
