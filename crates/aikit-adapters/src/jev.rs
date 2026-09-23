//! Bounded native transport for the general Jev protocol.
//!
//! Like AIKit's existing provider probes this uses the installed curl client,
//! but never places source text or a credential in argv, an environment variable,
//! a temporary file or a diagnostic. They travel over a private stdin pipe.
//! Curl configuration and ambient proxy variables cannot expand the egress route.
use aikit_core::{
    jev::{
        JevLimits, JevRequest, JevResponse, TokenUsage, JEV_ENDPOINT, JEV_PROTOCOL,
        MAX_RESPONSE_BYTES,
    },
    AikitError, ResourceRef, Result, SecretValue,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(10);
const HEADER_LIMIT: usize = 32 * 1024;
const CONTROLLED_KEY: &str = "aikit-controlled-protocol-only";

#[derive(Clone, Default)]
pub struct JevCancellation(Arc<AtomicBool>);
impl JevCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Production has one fixed HTTPS origin. Controlled protocol observations are
/// explicitly labelled and confined to loopback; they never accept a live key.
#[derive(Clone, Debug)]
pub enum JevEndpoint {
    Official,
    Controlled(SocketAddr),
}
impl JevEndpoint {
    fn url(&self, secret: &SecretValue) -> Result<String> {
        match self {
            Self::Official => Ok(JEV_ENDPOINT.into()),
            Self::Controlled(address)
                if address.ip().is_loopback() && secret.expose() == CONTROLLED_KEY =>
            {
                Ok(format!("http://{address}/v1/systemone"))
            }
            Self::Controlled(_) => Err(error(
                "jev.controlled_boundary",
                "Controlled transport requires loopback and its non-secret test marker",
            )),
        }
    }
    pub fn controlled_key() -> SecretValue {
        SecretValue::new(CONTROLLED_KEY).expect("nonempty test marker")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JevStanding {
    ProviderProtocol,
    ControlledProtocol,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JevOutcome {
    Completed,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JevFailure {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JevAttempt {
    pub ordinal: u32,
    pub reserved_microusd: u64,
    pub elapsed_ms: u64,
    pub http_status: Option<u16>,
    pub model_version: Option<String>,
    pub usage: Option<TokenUsage>,
    pub tariff_cost_microusd: Option<u64>,
    /// An absent provider answer is unknown usage, not zero cost.
    pub effect_uncertain: bool,
    pub failure: Option<JevFailure>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JevInvocation {
    pub schema: String,
    pub protocol: String,
    pub invocation_ref: ResourceRef,
    pub request_digest: String,
    pub requested_model: String,
    pub standing: JevStanding,
    pub outcome: JevOutcome,
    pub limits: JevLimits,
    pub total_reserved_microusd: u64,
    pub elapsed_ms: u64,
    pub attempts: Vec<JevAttempt>,
    pub answer: Option<JevResponse>,
    pub failure: Option<JevFailure>,
}
/// The caller revalidates native authority, disclosure, source and credential
/// basis before each external act and before making the answer available. A
/// native activity intent can be durably written here before the provider runs.
#[derive(Clone, Debug)]
pub enum JevBoundary {
    BeforeAttempt {
        ordinal: u32,
        reserved_microusd: u64,
    },
    BeforeReturn,
}

pub struct CurlJevProvider {
    curl: PathBuf,
    endpoint: JevEndpoint,
}
impl CurlJevProvider {
    pub fn new(curl: impl Into<PathBuf>, endpoint: JevEndpoint) -> Self {
        Self {
            curl: curl.into(),
            endpoint,
        }
    }
    pub fn invoke(
        &self,
        invocation_ref: ResourceRef,
        request: &JevRequest,
        limits: &JevLimits,
        secret: &SecretValue,
        cancellation: &JevCancellation,
        guard: &mut dyn FnMut(JevBoundary) -> Result<()>,
    ) -> Result<JevInvocation> {
        limits.validate(request)?;
        if secret.expose().len() > 16384 || !secret.expose().bytes().all(|b| b.is_ascii_graphic()) {
            return Err(error(
                "jev.credential_invalid",
                "A nonempty printable bearer credential is required",
            ));
        }
        let url = self.endpoint.url(secret)?;
        let started = Instant::now();
        let deadline = started + Duration::from_millis(limits.timeout_ms);
        let reservation = limits.tariff.reservation()?;
        let mut receipt = JevInvocation {
            schema: "aikit.jev-invocation/v1".into(),
            protocol: JEV_PROTOCOL.into(),
            invocation_ref,
            request_digest: request.digest()?,
            requested_model: request.model.clone(),
            standing: match self.endpoint {
                JevEndpoint::Official => JevStanding::ProviderProtocol,
                JevEndpoint::Controlled(_) => JevStanding::ControlledProtocol,
            },
            outcome: JevOutcome::Failed,
            limits: limits.clone(),
            total_reserved_microusd: 0,
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
            let Some(total) = receipt
                .total_reserved_microusd
                .checked_add(reservation)
                .filter(|v| *v <= limits.max_total_reserved_microusd)
            else {
                receipt.fail(error(
                    "jev.budget_exhausted",
                    "No budget remains for another bounded attempt",
                ));
                break;
            };
            if let Err(e) = guard(JevBoundary::BeforeAttempt {
                ordinal,
                reserved_microusd: reservation,
            }) {
                receipt.fail(e);
                break;
            }
            if let Err(e) = boundary(cancellation, deadline) {
                receipt.fail(e);
                break;
            }
            receipt.total_reserved_microusd = total;
            let attempt_started = Instant::now();
            let mut attempt = JevAttempt {
                ordinal,
                reserved_microusd: reservation,
                elapsed_ms: 0,
                http_status: None,
                model_version: None,
                usage: None,
                tariff_cost_microusd: None,
                effect_uncertain: true,
                failure: None,
            };
            let transport = self.exchange(&url, request, secret, cancellation, deadline);
            attempt.elapsed_ms = millis(attempt_started.elapsed());
            let mut retry_after = None;
            let result = match transport {
                Err(e) => Err(e),
                Ok(http) => {
                    attempt.http_status = Some(http.status);
                    if http.status == 200 {
                        JevResponse::parse_for(&http.body, request).and_then(|answer| {
                            attempt.model_version = Some(answer.model.clone());
                            attempt.usage = Some(answer.usage.clone());
                            attempt.tariff_cost_microusd = Some(limits.tariff.cost_microusd(&answer.usage)?);
                            attempt.effect_uncertain = false;
                            if answer.model != limits.tariff.model_version || answer.usage.input_tokens > limits.tariff.max_input_tokens_per_attempt || answer.usage.output_tokens > limits.tariff.max_output_tokens_per_attempt {
                                return Err(error("jev.provider_budget_basis_changed", "Returned model or usage exceeds the admitted tariff/token basis; no further calls"));
                            }
                            boundary(cancellation, deadline)?;
                            guard(JevBoundary::BeforeReturn)?;
                            Ok(answer)
                        })
                    } else {
                        // Retry only the provider's explicit overload responses.
                        // A disconnected/timed-out request may already be billed;
                        // it is never automatically replayed as if nothing happened.
                        if (http.status == 429 || http.status == 529)
                            && ordinal < limits.max_attempts
                        {
                            retry_after = match http.retry_after {
                                RetryAfter::Absent => Some(Duration::from_millis(
                                    200u64.saturating_mul(1 << (ordinal - 1)),
                                )),
                                RetryAfter::Seconds(seconds) => Some(Duration::from_secs(seconds)),
                                RetryAfter::Invalid => None,
                            };
                        }
                        Err(error("jev.provider_http", format!("Provider refused the invocation with HTTP {}; response payload withheld", http.status)))
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
        url: &str,
        request: &JevRequest,
        secret: &SecretValue,
        cancel: &JevCancellation,
        deadline: Instant,
    ) -> Result<HttpResponse> {
        boundary(cancel, deadline)?;
        let json = serde_json::to_string(request)
            .map_err(|_| error("jev.invalid_request", "Could not encode typed questions"))?;
        let config = format!("url = {}\nheader = {}\nheader = \"Content-Type: application/json\"\nheader = \"Expect:\"\ndata-binary = {}\n", quote(url), quote(&format!("Authorization: Bearer {}", secret.expose())), quote(&json));
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_secs_f64()
            .max(0.001);
        let protocol = match self.endpoint {
            JevEndpoint::Official => "=https",
            JevEndpoint::Controlled(_) => "=http",
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
    pub fn executable(&self) -> &Path {
        &self.curl
    }
}
impl JevInvocation {
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
fn failure(e: &AikitError) -> JevFailure {
    JevFailure {
        code: e.code().into(),
        message: e.message().into(),
    }
}
fn error(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}
fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
fn boundary(cancel: &JevCancellation, deadline: Instant) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(error(
            "jev.cancelled",
            "Invocation cancelled; any unreturned provider usage remains unknown",
        ));
    }
    if Instant::now() >= deadline {
        return Err(error(
            "jev.timeout",
            "Invocation deadline reached; any unreturned provider usage remains unknown",
        ));
    }
    Ok(())
}
fn pause(delay: Duration, cancel: &JevCancellation, deadline: Instant) -> Result<()> {
    let until = Instant::now()
        .checked_add(delay)
        .unwrap_or(deadline)
        .min(deadline);
    while Instant::now() < until {
        boundary(cancel, deadline)?;
        thread::sleep(POLL.min(until.saturating_duration_since(Instant::now())));
    }
    boundary(cancel, deadline)
}
fn read_bounded(
    mut pipe: impl Read,
    limit: usize,
    overflow: Arc<AtomicBool>,
    retain: bool,
) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut total = 0usize;
    let mut chunk = [0u8; 8192];
    loop {
        let n = pipe.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n);
        if total > limit {
            overflow.store(true, Ordering::SeqCst);
            break;
        }
        if retain {
            data.extend_from_slice(&chunk[..n]);
        }
    }
    Ok(data)
}
fn bounded_process(
    mut command: Command,
    config: Vec<u8>,
    cancel: &JevCancellation,
    deadline: Instant,
) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| {
            error(
                "jev.transport_unavailable",
                "The configured native curl executable could not start",
            )
        })?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let overflow = Arc::new(AtomicBool::new(false));
    let out_overflow = overflow.clone();
    let err_overflow = overflow.clone();
    let writer = thread::spawn(move || stdin.write_all(&config));
    let reader = thread::spawn(move || {
        read_bounded(
            stdout,
            MAX_RESPONSE_BYTES + HEADER_LIMIT,
            out_overflow,
            true,
        )
    });
    let errors = thread::spawn(move || read_bounded(stderr, HEADER_LIMIT, err_overflow, false));
    let status = loop {
        if let Err(e) = boundary(cancel, deadline) {
            break Err(e);
        }
        if overflow.load(Ordering::SeqCst) {
            break Err(error(
                "jev.response_too_large",
                "Provider output exceeds the admitted byte bound",
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now()))),
            Err(_) => {
                break Err(error(
                    "jev.transport_uncertain",
                    "Could not observe provider transport completion; do not automatically replay",
                ))
            }
        }
    };
    if status.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let wrote = writer.join().ok().and_then(|v| v.ok()).is_some();
    let bytes = reader.join().ok().and_then(|v| v.ok());
    let drained = errors.join().ok().and_then(|v| v.ok()).is_some();
    let status = status?;
    if overflow.load(Ordering::SeqCst) {
        return Err(error(
            "jev.response_too_large",
            "Provider output exceeds the admitted byte bound",
        ));
    }
    if !status.success() || !wrote || !drained {
        return Err(error(
            "jev.transport_uncertain",
            "Provider transport failed; payload and credential withheld, usage may be unknown",
        ));
    }
    bytes.ok_or_else(|| {
        error(
            "jev.transport_uncertain",
            "Provider response could not be read",
        )
    })
}
enum RetryAfter {
    Absent,
    Seconds(u64),
    Invalid,
}
struct HttpResponse {
    status: u16,
    retry_after: RetryAfter,
    body: Vec<u8>,
}
fn parse_http(mut bytes: &[u8]) -> Result<HttpResponse> {
    let invalid = || {
        error(
            "jev.invalid_http",
            "Provider returned an invalid or oversized HTTP envelope",
        )
    };
    let mut header_bytes = 0;
    for _ in 0..8 {
        let split = bytes
            .windows(4)
            .position(|p| p == b"\r\n\r\n")
            .ok_or_else(invalid)?;
        header_bytes += split + 4;
        if header_bytes > HEADER_LIMIT {
            return Err(invalid());
        }
        let headers = std::str::from_utf8(&bytes[..split]).map_err(|_| invalid())?;
        let mut lines = headers.split("\r\n");
        let mut first = lines.next().ok_or_else(invalid)?.split_whitespace();
        if !matches!(first.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
            return Err(invalid());
        }
        let status: u16 = first
            .next()
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())?;
        if !(100..=599).contains(&status) {
            return Err(invalid());
        }
        bytes = &bytes[split + 4..];
        if status < 200 {
            continue;
        }
        let mut retry_after = RetryAfter::Absent;
        let mut retry_seen = false;
        let mut content_type = None;
        for line in lines {
            let (name, value) = line.split_once(':').ok_or_else(invalid)?;
            if name.eq_ignore_ascii_case("retry-after") {
                if retry_seen {
                    return Err(invalid());
                }
                retry_seen = true;
                retry_after = value
                    .trim()
                    .parse::<u64>()
                    .map(RetryAfter::Seconds)
                    .unwrap_or(RetryAfter::Invalid);
            }
            if name.eq_ignore_ascii_case("content-type") {
                if content_type.is_some() {
                    return Err(invalid());
                }
                content_type = Some(
                    value
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase(),
                );
            }
        }
        if bytes.len() > MAX_RESPONSE_BYTES
            || (status == 200 && content_type.as_deref() != Some("application/json"))
        {
            return Err(invalid());
        }
        return Ok(HttpResponse {
            status,
            retry_after,
            body: bytes.to_vec(),
        });
    }
    Err(invalid())
}
