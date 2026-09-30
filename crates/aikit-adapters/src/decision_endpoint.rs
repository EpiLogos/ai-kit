//! Transport for local and self-hosted SystemOne-compatible decision
//! endpoints (the typed Noul/Choice/Score decision protocol served by, for
//! example, a managed local Kev server or an operator's own deployment).
//!
//! This is production transport, not a test stand-in: the same bounded-process
//! curl discipline as the hosted Jev transport (no credential or source text
//! in argv/env/temp files, bounded retries only on explicit overload,
//! cancellation, deadline), with the laws a local endpoint actually has:
//!
//! - an explicitly unauthenticated local server is served without inventing a
//!   bearer credential or a per-token tariff; an optional native credential is
//!   sent as an ordinary bearer token when configured;
//! - plain HTTP is loopback-only; anything beyond loopback requires both an
//!   explicit `allow_remote` and HTTPS;
//! - there is no spend reservation or tariff arithmetic, because there is no
//!   price source; usage is still required and recorded on success;
//! - the answer must echo the explicitly selected model identity — a mismatch
//!   means the served artifact changed and the determination is refused.

use aikit_core::jev::{DecisionLimits, JevRequest, JevResponse, TokenUsage, MAX_RESPONSE_BYTES};
use aikit_core::{AikitError, ResourceRef, Result, SecretValue};
use serde::{Deserialize, Serialize};
use std::{
    net::{SocketAddr, ToSocketAddrs},
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use crate::jev::{
    boundary, bounded_process, error, failure, millis, parse_http, pause, quote, JevBoundary,
    JevCancellation, JevFailure, JevOutcome,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionStanding {
    /// Loopback endpoint on this machine (the managed local service).
    LocalProtocol,
    /// An operator-supplied endpoint beyond this process's own loopback
    /// (self-hosted on another host, admitted explicitly with TLS).
    SelfHostedProtocol,
}

/// A resolved decision endpoint: the base URL and the standing it proves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionEndpoint {
    base_url: String,
    standing: DecisionStanding,
}

impl DecisionEndpoint {
    /// Resolve `host:port` into a loopback/self-hosted endpoint. Plain HTTP is
    /// accepted only on loopback; beyond loopback `allow_remote` and HTTPS are
    /// both required, so an ordinary local selection cannot widen itself into
    /// an unencrypted network boundary.
    pub fn resolve(address: &str, allow_remote: bool) -> Result<Self> {
        let address = address.trim();
        let ok_address = !address.is_empty()
            && address.len() <= 512
            && !address.contains("://")
            && !address.contains('/');
        if !ok_address {
            return Err(error(
                "decision.endpoint_invalid",
                "Decision endpoint must be a host:port address without a scheme or path",
            ));
        }
        let resolved = address
            .to_socket_addrs()
            .map_err(|_| {
                error(
                    "decision.endpoint_invalid",
                    "Decision endpoint does not resolve to a socket address",
                )
            })?
            .next()
            .ok_or_else(|| {
                error(
                    "decision.endpoint_invalid",
                    "Decision endpoint resolved to no socket address",
                )
            })?;
        let is_loopback = match resolved {
            SocketAddr::V4(v4) => v4.ip().is_loopback(),
            SocketAddr::V6(v6) => v6.ip().is_loopback(),
        };
        if is_loopback {
            return Ok(Self {
                base_url: format!("http://{address}/v1/systemone"),
                standing: DecisionStanding::LocalProtocol,
            });
        }
        if !allow_remote {
            return Err(error(
                "decision.remote_refused",
                "A decision endpoint beyond loopback requires the explicit allow_remote election",
            ));
        }
        Ok(Self {
            base_url: format!("https://{address}/v1/systemone"),
            standing: DecisionStanding::SelfHostedProtocol,
        })
    }
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
    pub fn standing(&self) -> &DecisionStanding {
        &self.standing
    }
    pub fn is_loopback(&self) -> bool {
        self.standing == DecisionStanding::LocalProtocol
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionAttempt {
    pub ordinal: u32,
    pub elapsed_ms: u64,
    pub http_status: Option<u16>,
    pub model: Option<String>,
    pub usage: Option<TokenUsage>,
    pub effect_uncertain: bool,
    pub failure: Option<JevFailure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionInvocation {
    pub schema: String,
    pub protocol: String,
    pub standing: DecisionStanding,
    pub endpoint: String,
    pub invocation_ref: ResourceRef,
    pub request_digest: String,
    pub requested_model: String,
    pub outcome: JevOutcome,
    pub limits: DecisionLimits,
    pub elapsed_ms: u64,
    pub attempts: Vec<DecisionAttempt>,
    pub answer: Option<JevResponse>,
    pub failure: Option<JevFailure>,
}
impl DecisionInvocation {
    fn fail(&mut self, e: AikitError) {
        self.outcome = if e.code() == "jev.cancelled" {
            JevOutcome::Cancelled
        } else {
            JevOutcome::Failed
        };
        self.answer = None;
        self.failure = Some(failure(&e));
    }
}

pub struct EndpointDecisionProvider {
    curl: PathBuf,
    endpoint: DecisionEndpoint,
}

impl EndpointDecisionProvider {
    pub fn new(curl: impl Into<PathBuf>, endpoint: DecisionEndpoint) -> Self {
        Self {
            curl: curl.into(),
            endpoint,
        }
    }

    pub fn endpoint(&self) -> &DecisionEndpoint {
        &self.endpoint
    }

    /// One bounded typed decision against the endpoint. The caller revalidates
    /// native authority and disclosure before making the answer available; the
    /// optional bearer credential is resolved by the caller and may be absent
    /// for an explicitly unauthenticated local server.
    #[allow(clippy::too_many_arguments)]
    pub fn invoke(
        &self,
        invocation_ref: ResourceRef,
        request: &JevRequest,
        limits: &DecisionLimits,
        secret: Option<&SecretValue>,
        cancellation: &JevCancellation,
        guard: &mut dyn FnMut(JevBoundary) -> Result<()>,
    ) -> Result<DecisionInvocation> {
        limits.validate(request)?;
        if let Some(secret) = secret {
            if secret.expose().len() > 16384
                || !secret.expose().bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(error(
                    "decision.credential_invalid",
                    "The endpoint credential, when configured, must be a bounded printable bearer token",
                ));
            }
        }
        let started = Instant::now();
        let deadline = started + Duration::from_millis(limits.timeout_ms);
        let mut receipt = DecisionInvocation {
            schema: "aikit.decision-invocation/v1".into(),
            protocol: "systemone-compatible/v1".into(),
            standing: self.endpoint.standing().clone(),
            endpoint: redact_endpoint(&self.endpoint),
            invocation_ref,
            request_digest: request.digest()?,
            requested_model: request.model.clone(),
            outcome: JevOutcome::Failed,
            limits: limits.clone(),
            elapsed_ms: 0,
            attempts: Vec::new(),
            answer: None,
            failure: None,
        };
        for ordinal in 1..=limits.max_attempts {
            if let Err(e) = boundary(cancellation, deadline) {
                receipt.fail(e);
                break;
            }
            if let Err(e) = guard(JevBoundary::BeforeAttempt {
                ordinal,
                reserved_microusd: 0,
            }) {
                receipt.fail(e);
                break;
            }
            if let Err(e) = boundary(cancellation, deadline) {
                receipt.fail(e);
                break;
            }
            let attempt_started = Instant::now();
            let mut attempt = DecisionAttempt {
                ordinal,
                elapsed_ms: 0,
                http_status: None,
                model: None,
                usage: None,
                effect_uncertain: true,
                failure: None,
            };
            let transport = self.exchange(request, secret, cancellation, deadline);
            attempt.elapsed_ms = millis(attempt_started.elapsed());
            let mut retry_after = None;
            let result = match transport {
                Err(e) => Err(e),
                Ok(http) => {
                    attempt.http_status = Some(http.status);
                    if http.status == 200 {
                        JevResponse::parse(&http.body).and_then(|answer| {
                            // The endpoint standing: the served artifact's
                            // numeric precision (bf16 backbones drift ~1e-4
                            // on distribution sums) cannot satisfy the hosted
                            // 1e-5 bound, so the same strict typed law runs
                            // with the documented endpoint tolerance.
                            answer
                                .validate_for_with_tolerance(
                                    request,
                                    aikit_core::jev::ENDPOINT_TOLERANCE,
                                )?;
                            if answer.model != request.model {
                                return Err(error(
                                    "decision.model_mismatch",
                                    "The endpoint answered under a different model identity than the explicitly selected one; the determination is refused",
                                ));
                            }
                            if answer.usage.input_tokens > limits.max_input_tokens_per_attempt
                                || answer.usage.output_tokens > limits.max_output_tokens_per_attempt
                            {
                                return Err(error(
                                    "decision.token_ceiling_exceeded",
                                    "Reported usage exceeds the admitted token ceilings; no further calls",
                                ));
                            }
                            attempt.model = Some(answer.model.clone());
                            attempt.usage = Some(answer.usage.clone());
                            attempt.effect_uncertain = false;
                            boundary(cancellation, deadline)?;
                            guard(JevBoundary::BeforeReturn)?;
                            Ok(answer)
                        })
                    } else {
                        // Retry only the endpoint's explicit overload responses;
                        // a timed-out request may already have been evaluated.
                        if (http.status == 429 || http.status == 529)
                            && ordinal < limits.max_attempts
                        {
                            retry_after = match http.retry_after {
                                crate::jev::RetryAfter::Absent => Some(Duration::from_millis(
                                    200u64.saturating_mul(1 << (ordinal - 1)),
                                )),
                                crate::jev::RetryAfter::Seconds(seconds) => {
                                    Some(Duration::from_secs(seconds))
                                }
                                crate::jev::RetryAfter::Invalid => None,
                            };
                        }
                        Err(error(
                            "decision.endpoint_http",
                            format!(
                                "Decision endpoint refused the invocation with HTTP {}; response payload withheld",
                                http.status
                            ),
                        ))
                    }
                }
            };
            match result {
                Ok(answer) => {
                    receipt.attempts.push(attempt);
                    receipt.answer = Some(answer);
                    receipt.outcome = JevOutcome::Completed;
                    receipt.failure = None;
                    break;
                }
                Err(e) => {
                    attempt.failure = Some(failure(&e));
                    receipt.attempts.push(attempt);
                    receipt.fail(e);
                    if let Some(delay) = retry_after {
                        if let Err(e) = pause(delay, cancellation, deadline) {
                            receipt.fail(e);
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }
        receipt.elapsed_ms = millis(started.elapsed());
        Ok(receipt)
    }

    fn exchange(
        &self,
        request: &JevRequest,
        secret: Option<&SecretValue>,
        cancel: &JevCancellation,
        deadline: Instant,
    ) -> Result<crate::jev::HttpResponse> {
        boundary(cancel, deadline)?;
        let json = serde_json::to_string(request)
            .map_err(|_| error("jev.invalid_request", "Could not encode typed questions"))?;
        let authorization = secret
            .map(|secret| format!("Authorization: Bearer {}", secret.expose()))
            .unwrap_or_default();
        let config = format!(
            "url = {}\n{authorization_header}header = \"Content-Type: application/json\"\nheader = \"Expect:\"\ndata-binary = {}\n",
            quote(&self.endpoint.base_url),
            quote(&json),
            authorization_header = if secret.is_some() {
                format!("header = {}\n", quote(&authorization))
            } else {
                String::new()
            },
        );
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_secs_f64()
            .max(0.001);
        let protocol = if self.endpoint.is_loopback() {
            "=http"
        } else {
            "=https"
        };
        let mut command = Command::new(&self.curl);
        command.env_clear().args([
            "-q",
            "--silent",
            "--show-error",
            "--include",
            "--http1.1",
            "--proxy",
            "",
            "--noproxy",
            "*",
            "--proto",
            protocol,
            "--max-redirs",
            "0",
            "--max-filesize",
            "1048576",
            "--max-time",
            &format!("{remaining:.3}"),
            "--connect-timeout",
            &format!("{:.3}", remaining.min(15.0)),
            "--config",
            "-",
        ]);
        let bytes = bounded_process(command, config.into_bytes(), cancel, deadline)?;
        parse_http(&bytes)
    }
}

/// The endpoint string carried in receipts names the loopback/self-hosted
/// standing and port but never a credential or a hostname's userinfo.
fn redact_endpoint(endpoint: &DecisionEndpoint) -> String {
    endpoint.base_url().to_string()
}

/// Diagnostic read of the endpoint's model card (`GET /v1/models`), used by
/// the operator status surface. Returns the parsed JSON body; absence of a
/// field is reported to the operator rather than guessed.
pub fn probe_models(
    curl: impl Into<PathBuf>,
    endpoint: &DecisionEndpoint,
    timeout_ms: u64,
    secret: Option<&SecretValue>,
) -> Result<serde_json::Value> {
    let timeout_ms = timeout_ms.clamp(100, 30_000);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let authorization = secret
        .map(|secret| format!("Authorization: Bearer {}", secret.expose()))
        .unwrap_or_default();
    let models_url = format!(
        "{}/models",
        endpoint.base_url().trim_end_matches("/systemone")
    );
    let config = format!(
        "url = {models_url}\n{}header = \"Expect:\"\n",
        if secret.is_some() {
            format!("header = {}\n", quote(&authorization))
        } else {
            String::new()
        }
    );
    let mut command = Command::new(curl.into());
    command.env_clear().args([
        "-q",
        "--silent",
        "--show-error",
        "--include",
        "--http1.1",
        "--proxy",
        "",
        "--noproxy",
        "*",
        "--proto",
        if endpoint.is_loopback() {
            "=http"
        } else {
            "=https"
        },
        "--max-redirs",
        "0",
        "--max-filesize",
        "1048576",
        "--max-time",
        &format!("{:.3}", timeout_ms as f64 / 1000.0),
        "--connect-timeout",
        &format!("{:.3}", (timeout_ms as f64 / 1000.0).min(5.0)),
        "--config",
        "-",
    ]);
    let cancellation = JevCancellation::default();
    let bytes = bounded_process(command, config.into_bytes(), &cancellation, deadline)?;
    let http = parse_http(&bytes)?;
    if http.status != 200 {
        return Err(error(
            "decision.endpoint_http",
            format!("Model-card probe received HTTP {}", http.status),
        ));
    }
    aikit_core::jev::unique_json(&http.body, MAX_RESPONSE_BYTES).map_err(|_| {
        error(
            "decision.endpoint_invalid",
            "Model card is not bounded unique JSON",
        )
    })
}
