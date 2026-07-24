//! The hook dispatcher.
//!
//! One permanent entry per client event routes into `aikit hook dispatch <client>
//! <event>`. This module normalises the client's event, runs the immutable chain
//! with **real subprocesses** honouring per-step timeouts, spends a bypass token
//! if one applied, and hands back the decision for the caller to translate into
//! the client's protocol.
//!
//! The decision logic itself is not here — it is [`aikit_core::hooks::Dispatcher`],
//! which folds the chain deterministically and decides bypass application. This
//! module supplies the two things core cannot: a real step runner (a child
//! process fed the event on stdin) and the persistent bypass ledger (issue once,
//! spend once, and the next event is gated again).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aikit_core::hooks::{
    BypassScope, BypassToken, Dispatcher, HookChain, HookDecision, HookEvent, HookEventKind,
    HookStep, StepResult,
};
use aikit_core::id::{CapsuleId, ContextId};
use aikit_core::{AikitError, Result};

use aikit_store::index::Index;

/// Run `chain` against `event` for `context`, consuming a bypass token if one
/// applied.
///
/// The bypass ledger is read *before* the run and written *after* it: the token
/// that was in force when the event arrived is the one that can be spent, and it
/// is spent only if [`aikit_core::hooks::Dispatcher`] actually applied it (a
/// `next-event` token that never matched a step is not burned).
pub fn dispatch(
    index: &Index,
    context: &ContextId,
    chain: &HookChain,
    event: &HookEvent,
    roots: &BTreeMap<CapsuleId, PathBuf>,
) -> Result<HookDecision> {
    let open = index.open_bypasses(context)?;
    let active = open.into_iter().next();

    let dispatcher = match &active {
        Some(record) => Dispatcher::with_bypass(record.token.clone()),
        None => Dispatcher::new(),
    };

    let mut runner = |step: &HookStep, ev: &HookEvent| -> StepResult {
        match roots.get(&step.capsule) {
            Some(root) => run_hook_step(step, ev, root),
            None => StepResult::system_failure(format!(
                "no payload on this machine for {}",
                step.capsule
            )),
        }
    };

    let decision = dispatcher.run(chain, event, &mut runner);

    if decision.bypass_consumed {
        if let Some(record) = &active {
            index.spend_bypass(&record.bypass_id)?;
        }
    }

    Ok(decision)
}

/// Execute one hook step as a real child process.
///
/// The event is handed to the child on stdin as JSON. The exit status is mapped
/// to a verdict: zero allows, non-zero denies in a phase that can deny (and is a
/// recorded system failure otherwise, so the step's failure policy decides). A
/// step that outruns its timeout is killed and reported as a system failure, not
/// left to hang the client.
pub fn run_hook_step(step: &HookStep, event: &HookEvent, root: &Path) -> StepResult {
    let entry = root.join(&step.entry);
    let started = Instant::now();

    let mut child = match Command::new(&entry)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return StepResult::system_failure(format!("could not start {}: {e}", entry.display()))
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let payload = serde_json::to_vec(&event.payload).unwrap_or_default();
        let _ = stdin.write_all(&payload);
        // Dropping stdin closes it, so a child reading to EOF is not left waiting.
    }

    let timeout = step.timeout.as_ref().map(|d| d.as_duration());
    let status = match wait_with_timeout(&mut child, timeout) {
        WaitResult::Exited(status) => status,
        WaitResult::TimedOut => {
            let _ = child.kill();
            let _ = child.wait();
            return StepResult::system_failure(format!(
                "{} exceeded its {:?} timeout",
                step.capsule, timeout
            ))
            .taking(started.elapsed());
        }
        WaitResult::Error(e) => {
            return StepResult::system_failure(format!("{} failed to run: {e}", step.capsule))
                .taking(started.elapsed());
        }
    };

    let result = if status.success() {
        StepResult::allow()
    } else {
        // A non-zero exit is a denial. Whether that denial has teeth is the
        // dispatcher's call, based on the step's phase and failure policy; here we
        // only report what the process said.
        let mut reason = String::new();
        if let Some(mut err) = child.stderr.take() {
            use std::io::Read;
            let _ = err.read_to_string(&mut reason);
        }
        let reason = reason.trim();
        let reason = if reason.is_empty() {
            format!("{} exited with a non-zero status", step.capsule)
        } else {
            reason.to_string()
        };
        StepResult::deny(reason)
    };

    result.taking(started.elapsed())
}

enum WaitResult {
    Exited(std::process::ExitStatus),
    TimedOut,
    Error(std::io::Error),
}

/// Wait for a child, killing it if it outruns `timeout`.
///
/// A `None` timeout waits indefinitely. A `Some` timeout polls, which is coarse
/// but correct and needs no extra threads — a hook that has to be killed is
/// already the slow path, so the polling granularity does not matter.
fn wait_with_timeout(child: &mut std::process::Child, timeout: Option<Duration>) -> WaitResult {
    let Some(timeout) = timeout else {
        return match child.wait() {
            Ok(status) => WaitResult::Exited(status),
            Err(e) => WaitResult::Error(e),
        };
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return WaitResult::Exited(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    return WaitResult::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return WaitResult::Error(e),
        }
    }
}

/// Normalise a client event read from stdin into a [`HookEvent`].
///
/// The payload is passed through verbatim; only the fields AIKit routes on — the
/// event kind and, where the event carries one, the tool name — are lifted out so
/// a hook's matcher can be evaluated without every hook re-parsing the client's
/// JSON.
pub fn normalize(client: &str, event: &str, payload: serde_json::Value) -> HookEvent {
    let kind = HookEventKind::parse(event);
    let carries_tool_name = kind.carries_tool_name();
    let mut normalized = HookEvent::new(client, kind, payload.clone());
    if carries_tool_name {
        if let Some(tool) = payload
            .get("tool_name")
            .or_else(|| payload.get("tool"))
            .and_then(|v| v.as_str())
        {
            normalized = normalized.with_tool_name(tool);
        }
    }
    normalized
}

/// Parse a bypass scope string as `aikit bypass issue --scope` accepts it.
pub fn parse_bypass_scope(raw: &str) -> Result<BypassScope> {
    match raw {
        "next-event" | "next" => Ok(BypassScope::NextEvent),
        "session" => Ok(BypassScope::Session),
        other => Err(AikitError::new(
            "cli.usage",
            format!("`{other}` is not a bypass scope; use `next-event` or `session`"),
        )
        .with("scope", other.to_string())),
    }
}

/// Mint and persist a bypass token, returning its id.
pub fn issue_bypass(
    index: &Index,
    context: &ContextId,
    scope: &str,
    reason: Option<&str>,
    capability: Option<&str>,
) -> Result<String> {
    let mut token = BypassToken::new(parse_bypass_scope(scope)?);
    token.reason = reason.map(|r| r.to_string());
    if let Some(capability) = capability {
        token.issued_for = Some(CapsuleId::parse(capability)?);
    }
    index.issue_bypass(context, &token)
}
