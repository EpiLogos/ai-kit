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

use aikit_adapters::runner::SystemRunner;
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

    let mut decision = dispatcher.run(chain, event, &mut runner);

    // Central owns temporal continuity; AIKit owns lifecycle delivery. Re-read
    // the owner at each causal orientation event rather than caching a session
    // prompt. A missing/non-Central world remains a normal AIKit world.
    let central_root = crate::temporal::process_central_root(event.cwd.as_deref());
    crate::temporal::reground(
        &mut decision,
        event,
        event.cwd.as_deref(),
        central_root.as_deref(),
        &SystemRunner::new(),
    );

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

// ---------------------------------------------------------------------------
// The dispatch boundary: verdict -> harness protocol
// ---------------------------------------------------------------------------

/// The exit status a blocking verdict maps to. claude-code and zcode both block
/// a PreToolUse tool call when its hook process exits 2; this is the common
/// denominator of the two harnesses' protocols and needs no schema agreement.
pub const HARNESS_BLOCK_EXIT: i32 = 2;

/// The harness-facing streams of one translated dispatch verdict.
///
/// `None` means "print nothing on this stream": an empty stdout is a defined
/// pass in both harnesses, and zcode strict-parses any stdout it does see as
/// JSON (an extra key fails validation), so the default flavor never prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessVerdict {
    /// The process exit status this verdict maps to.
    pub exit_code: i32,
    /// Bytes for stdout, when the flavor prints a decision document.
    pub stdout: Option<String>,
    /// Bytes for stderr, when the flavor carries a message to the model/user.
    pub stderr: Option<String>,
}

/// Translate one dispatch verdict into the calling harness's protocol.
///
/// A denial blocks through exit [`HARNESS_BLOCK_EXIT`] with the denial message
/// on stderr — the channel claude-code feeds back to the model — and empty
/// stdout, which zcode's strict hook-output schema treats as a clean run. An
/// allowance passes with exit 0 and both streams silent. `decision_json` opts
/// into claude-code's `hookSpecificOutput.permissionDecision` document instead
/// (exit 0, decision carried in the JSON): use it only where the calling
/// harness consumes that protocol, because zcode would reject the document's
/// shape. An absent denial message still blocks, with a generic reason.
pub fn translate_verdict(
    allowed: bool,
    denial: Option<&str>,
    decision_json: bool,
    event: &str,
) -> HarnessVerdict {
    let reason = denial.unwrap_or("denied by the composed hook chain");
    match (allowed, decision_json) {
        (true, false) => HarnessVerdict {
            exit_code: 0,
            stdout: None,
            stderr: None,
        },
        (true, true) => HarnessVerdict {
            exit_code: 0,
            stdout: Some(decision_document(event, "allow", None)),
            stderr: None,
        },
        (false, false) => HarnessVerdict {
            exit_code: HARNESS_BLOCK_EXIT,
            stdout: None,
            stderr: Some(reason.to_string()),
        },
        (false, true) => HarnessVerdict {
            exit_code: 0,
            stdout: Some(decision_document(event, "deny", Some(reason))),
            stderr: None,
        },
    }
}

/// claude-code's advanced JSON decision document for one verdict.
fn decision_document(event: &str, decision: &str, reason: Option<&str>) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": event,
            "permissionDecision": decision,
            "permissionDecisionReason": reason.unwrap_or_default(),
        }
    })
    .to_string()
}

/// Normalise a client event read from stdin into a [`HookEvent`].
///
/// The payload is passed through verbatim; only the fields AIKit routes on — the
/// event kind and, where the event carries one, the tool name — are lifted out so
/// a hook's matcher can be evaluated without every hook re-parsing the client's
/// JSON. The event's current working directory is also retained because it is the
/// concrete Project-world coordinate used by context and temporal providers.
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
    let cwd = payload
        .get("cwd")
        .or_else(|| payload.get("working_directory"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    if let Some(cwd) = cwd {
        normalized = normalized.in_cwd(cwd);
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

#[cfg(test)]
mod tests {
    use super::*;

    const DENY_MESSAGE: &str = "hook/central/fs-guardrail denied this event: BLOCKED by Central filesystem guardrail: '/home/frank/rogue.txt' is outside the allowed agent write roots.";

    #[test]
    fn denial_blocks_through_exit_codes_with_empty_stdout() {
        let verdict = translate_verdict(false, Some(DENY_MESSAGE), false, "PreToolUse");
        assert_eq!(verdict.exit_code, HARNESS_BLOCK_EXIT);
        assert_eq!(verdict.stdout, None);
        assert_eq!(verdict.stderr.as_deref(), Some(DENY_MESSAGE));
    }

    #[test]
    fn allowance_passes_silently() {
        let verdict = translate_verdict(true, None, false, "PreToolUse");
        assert_eq!(verdict.exit_code, 0);
        assert_eq!(verdict.stdout, None);
        assert_eq!(verdict.stderr, None);
    }

    #[test]
    fn denial_without_a_message_still_blocks() {
        let verdict = translate_verdict(false, None, false, "PreToolUse");
        assert_eq!(verdict.exit_code, HARNESS_BLOCK_EXIT);
        assert!(verdict.stderr.is_some());
    }

    #[test]
    fn decision_json_flavor_names_the_event_and_carries_the_deny_reason() {
        let verdict = translate_verdict(false, Some(DENY_MESSAGE), true, "PreToolUse");
        assert_eq!(verdict.exit_code, 0);
        assert_eq!(verdict.stderr, None);
        let doc: serde_json::Value = serde_json::from_str(
            verdict
                .stdout
                .as_deref()
                .expect("decision document on stdout"),
        )
        .expect("valid JSON document");
        let output = &doc["hookSpecificOutput"];
        assert_eq!(output["hookEventName"], "PreToolUse");
        assert_eq!(output["permissionDecision"], "deny");
        assert!(output["permissionDecisionReason"]
            .as_str()
            .expect("reason carried")
            .contains("BLOCKED by Central filesystem guardrail"));
    }

    #[test]
    fn decision_json_flavor_allows_explicitly() {
        let verdict = translate_verdict(true, None, true, "PreToolUse");
        assert_eq!(verdict.exit_code, 0);
        let doc: serde_json::Value = serde_json::from_str(
            verdict
                .stdout
                .as_deref()
                .expect("decision document on stdout"),
        )
        .expect("valid JSON document");
        assert_eq!(doc["hookSpecificOutput"]["permissionDecision"], "allow");
    }
}
