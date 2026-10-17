//! `aikit observe` — the feed contract for the recurrence observer.
//!
//! Sources report attributed command runs; AIKit (`aikit_store::observer`) decides
//! what repeats. This module is only the reading of what a source sends:
//!
//! * **flags** — one run: `--command`, `--cwd`, `--by`, `--at`, `--session`.
//! * **JSON lines on stdin** — many runs, one object per line with the same names
//!   (`command`, `cwd`, `by`, `at`, `session`, `exit_code`). This is the contract a
//!   terminal pane or a Factory frame feeds.
//! * **a hook payload on stdin** — an agent harness's `PostToolUse` event. The
//!   shell tool's call carries the command line, the working directory and the
//!   session; nothing in it names the agent, so the actor comes from the
//!   environment (`OI_POSITION_REF`, then `AIKIT_ACTOR`) or `--by`.
//!
//! The hook shapes handled are the harnesses' documented ones. Claude Code and
//! Codex both send JSON on stdin with `tool_name: "Bash"` and a string
//! `tool_input.command`, plus `cwd` and `session_id` (Codex adds `turn_id`,
//! `permission_mode` and `model`; its `tool_response` is the tool's model-facing
//! output, with no exit code, so a Codex run is never marked failed). Codex's
//! `apply_patch` and MCP tools (`mcp__*`) are not shell commands and are ignored.
//! The argv array form some harnesses send (`["bash", "-lc", "<script>"]`) is
//! handled defensively: the script is the command.

use std::time::Duration;

use serde_json::Value;

use aikit_core::{AikitError, Result};
use aikit_store::observer::ObservedRun;
use aikit_store::Timestamp;

/// The tools whose calls are a shell command line.
const SHELL_TOOLS: [&str; 5] = ["Bash", "bash", "shell", "local_shell", "run_shell_command"];

/// A span such as `7d`, `36h`, `90m` or `45s`, or a sum of them (`1d12h`).
pub fn parse_span(raw: &str) -> Result<Duration> {
    let bad = || {
        AikitError::new(
            "observe.bad_span",
            format!("`{raw}` is not a span; write it like 7d, 36h or 90m"),
        )
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(bad());
    }
    let mut total = 0u64;
    let mut digits = String::new();
    for c in raw.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let value: u64 = digits.parse().map_err(|_| bad())?;
        digits.clear();
        total += value
            * match c {
                'd' => 86_400,
                'h' => 3_600,
                'm' => 60,
                's' => 1,
                _ => return Err(bad()),
            };
    }
    if !digits.is_empty() || total == 0 {
        return Err(bad());
    }
    Ok(Duration::from_secs(total))
}

/// When a source says a run happened: RFC 3339 text, or a number (seconds, or
/// milliseconds when it is too large to be seconds). No time means now.
fn parse_time(value: Option<&Value>) -> Result<Timestamp> {
    match value {
        None | Some(Value::Null) => Ok(Timestamp::now()),
        Some(Value::String(text)) => text.parse().map_err(|_| bad_time(text)),
        Some(Value::Number(number)) => {
            let n = number
                .as_f64()
                .ok_or_else(|| bad_time(&number.to_string()))?;
            let millis = if n >= 1.0e11 { n } else { n * 1000.0 };
            Ok(Timestamp::from_nanos((millis * 1_000_000.0) as i64))
        }
        Some(other) => Err(bad_time(&other.to_string())),
    }
}

fn bad_time(text: &str) -> AikitError {
    AikitError::new(
        "observe.bad_time",
        format!("`{text}` is not a time; use RFC 3339 (2026-10-16T21:30:00Z) or epoch seconds"),
    )
}

/// One run from a JSON object with the contract's names.
pub fn run_from_object(object: &Value, default_by: &str) -> Result<ObservedRun> {
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let command = text("command").ok_or_else(|| {
        AikitError::new(
            "observe.no_command",
            "a run needs a `command` (the line that was run)",
        )
    })?;
    Ok(ObservedRun {
        command,
        cwd: text("cwd"),
        by: text("by").unwrap_or_else(|| default_by.to_string()),
        at: parse_time(object.get("at"))?,
        session: text("session"),
        exit_code: object
            .get("exit_code")
            .and_then(Value::as_i64)
            .map(|code| code as i32),
    })
}

/// Runs from JSON lines. A blank line is skipped; a line that is not an object
/// with a command is an error naming its line, so a feed learns it is malformed
/// rather than being counted as having sent nothing.
pub fn runs_from_jsonl(text: &str, default_by: &str) -> Result<Vec<ObservedRun>> {
    let mut runs = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let object: Value = serde_json::from_str(line).map_err(|e| {
            AikitError::new(
                "observe.bad_line",
                format!("line {} is not JSON: {e}", number + 1),
            )
        })?;
        runs.push(run_from_object(&object, default_by).map_err(|e| {
            AikitError::new(
                "observe.bad_line",
                format!("line {}: {}", number + 1, e.message()),
            )
        })?);
    }
    Ok(runs)
}

/// The run a harness's `PostToolUse` event reports, when it is a shell command.
/// Any other tool's event is not a run, and yields `None`.
pub fn run_from_hook(payload: &Value, by: &str) -> Option<ObservedRun> {
    let tool = payload
        .get("tool_name")
        .or_else(|| payload.get("tool"))
        .and_then(Value::as_str)?;
    if !SHELL_TOOLS.contains(&tool) {
        return None;
    }
    let input = payload
        .get("tool_input")
        .or_else(|| payload.get("input"))
        .unwrap_or(payload);
    let command = match input.get("command").or_else(|| input.get("cmd"))? {
        Value::String(line) => line.clone(),
        Value::Array(argv) => {
            let words: Vec<&str> = argv.iter().filter_map(Value::as_str).collect();
            match words.as_slice() {
                // ["bash", "-lc", "<script>"]: the script is the command.
                [_, flag, script] if flag.starts_with('-') && flag.ends_with('c') => {
                    (*script).to_string()
                }
                _ => words.join(" "),
            }
        }
        _ => return None,
    };
    let response = payload
        .get("tool_response")
        .or_else(|| payload.get("tool_output"));
    let exit_code = response
        .and_then(|r| r.get("exit_code").or_else(|| r.get("exitCode")))
        .and_then(Value::as_i64)
        .map(|code| code as i32);
    Some(ObservedRun {
        command,
        cwd: payload
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_string),
        by: by.to_string(),
        at: Timestamp::now(),
        session: payload
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        exit_code,
    })
}

/// Who a feed that does not say is, taken from the environment: the Position an
/// agent was launched as, then an explicit actor. Never `human` — a run nobody
/// attributed is not safe to call a person's.
pub fn actor_from_env<F: Fn(&str) -> Option<String>>(env: F, fallback: &str) -> String {
    ["OI_POSITION_REF", "AIKIT_ACTOR"]
        .iter()
        .find_map(|name| env(name).filter(|value| !value.trim().is_empty()))
        .unwrap_or_else(|| fallback.to_string())
}
