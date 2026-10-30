//! `aikit observe` — the feed contract for the recurrence observer.
//!
//! Sources report attributed command runs; AIKit (`aikit_store::observer`) decides
//! what repeats. This module is only the reading of what a source sends:
//!
//! * **flags** — one run: `--command`, `--cwd`, `--by`, `--at`, `--session`.
//! * **JSON lines on stdin** — many runs, one object per line with the same names
//!   (`command`, `cwd`, `by`, `at`, `session`, `exit_code`, and `pane` or `panes`).
//!   This is the contract a terminal pane or a Factory frame feeds.
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
//!
//! ## The pane a run ran in
//!
//! A hook runs inside the harness's own process tree, so it carries the
//! variables each multiplexer hands the processes in its panes: tmux's
//! `TMUX_PANE` (and `TMUX`, naming its server), herdr's `HERDR_PANE_ID` (and
//! `HERDR_SOCKET_PATH`), cmux's `CMUX_SURFACE_ID`. The hook names each pane
//! present ([`panes_from_env`]) the way a working Surface's capture names its
//! own ([`crate::working_environment_field::pane_key`]), so the capture can
//! say which commands on its screen an agent ran. A flag or JSON-lines feed
//! names its runs' panes itself (`--pane`, `pane`/`panes`).

use std::time::Duration;

use serde_json::Value;

use aikit_core::{AikitError, Result};
use aikit_store::observer::ObservedRun;
use aikit_store::Timestamp;

use crate::working_environment_field::pane_key;

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
        panes: match (object.get("panes"), text("pane")) {
            (Some(Value::Array(panes)), _) => panes
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            (_, Some(pane)) => vec![pane],
            _ => Vec::new(),
        },
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
        // The hook's caller names the pane from its environment.
        panes: Vec::new(),
    })
}

/// The terminal panes a process runs in, named from the variables each
/// multiplexer hands the processes in its panes, innermost or not: a harness
/// inside one multiplexer inside another is in both, and each pane's screen
/// shows its commands. tmux names a pane on its server (`TMUX`'s socket and
/// pid); herdr on its socket. A cmux surface already named by its ref
/// (`surface:N`) needs no question; one named any other way (a UUID) is
/// resolved to its ref through `resolve_cmux(workspace, surface)`, and kept as
/// named when cmux cannot say.
pub fn panes_from_env<F, C>(env: F, resolve_cmux: C) -> Vec<String>
where
    F: Fn(&str) -> Option<String>,
    C: Fn(&str, &str) -> Option<String>,
{
    let set = |name: &str| env(name).filter(|value| !value.trim().is_empty());
    let mut panes = Vec::new();
    if let Some(pane) = set("TMUX_PANE") {
        // `TMUX` is `<socket>,<server pid>,<session>`: the server is the first two.
        let server = set("TMUX").map(|tmux| {
            let mut parts = tmux.rsplitn(2, ',');
            let _session = parts.next();
            parts.next().unwrap_or_default().to_string()
        });
        panes.push(pane_key("tmux", pane.trim(), server.as_deref()));
    }
    if let Some(pane) = set("HERDR_PANE_ID") {
        panes.push(pane_key(
            "herdr",
            pane.trim(),
            set("HERDR_SOCKET_PATH").as_deref(),
        ));
    }
    if let Some(surface) = set("CMUX_SURFACE_ID") {
        let surface = surface.trim();
        let named = if surface.starts_with("surface:") {
            surface.to_string()
        } else {
            resolve_cmux(&set("CMUX_WORKSPACE_ID").unwrap_or_default(), surface)
                .unwrap_or_else(|| surface.to_string())
        };
        panes.push(pane_key("cmux", &named, None));
    }
    panes
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

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn a_hook_names_the_terminal_pane_it_runs_in_from_the_multiplexers_own_variables() {
        let never = |_: &str, _: &str| -> Option<String> { panic!("no cmux is asked here") };
        // tmux: the pane, on the server the TMUX variable names (socket, pid).
        assert_eq!(
            panes_from_env(
                env(&[
                    ("TMUX", "/private/tmp/tmux-501/aikit,4242,0"),
                    ("TMUX_PANE", "%3")
                ]),
                never
            ),
            vec!["tmux:%3@/private/tmp/tmux-501/aikit,4242".to_string()]
        );
        // herdr: the pane, in the session whose socket the pane carries.
        assert_eq!(
            panes_from_env(
                env(&[
                    ("HERDR_PANE_ID", "w1:p1"),
                    ("HERDR_SOCKET_PATH", "/x/sessions/a/herdr.sock")
                ]),
                never
            ),
            vec!["herdr:w1:p1@/x/sessions/a/herdr.sock".to_string()]
        );
        // cmux: a surface already named by its ref needs no question.
        assert_eq!(
            panes_from_env(
                env(&[
                    ("CMUX_WORKSPACE_ID", "workspace:88"),
                    ("CMUX_SURFACE_ID", "surface:20")
                ]),
                never
            ),
            vec!["cmux:surface:20".to_string()]
        );
        // Nowhere: no pane.
        assert!(panes_from_env(env(&[]), never).is_empty());
    }

    #[test]
    fn a_cmux_surface_named_by_its_uuid_is_resolved_to_its_ref_and_kept_raw_when_cmux_cannot_say() {
        let pairs = [
            ("CMUX_WORKSPACE_ID", "8A2C1F0E-0000-4000-8000-000000000001"),
            ("CMUX_SURFACE_ID", "6B1D0E9F-0000-4000-8000-000000000002"),
        ];
        let resolved = panes_from_env(env(&pairs), |workspace, surface| {
            assert_eq!(workspace, "8A2C1F0E-0000-4000-8000-000000000001");
            assert_eq!(surface, "6B1D0E9F-0000-4000-8000-000000000002");
            Some("surface:20".into())
        });
        assert_eq!(resolved, vec!["cmux:surface:20".to_string()]);
        let unresolved = panes_from_env(env(&pairs), |_, _| None);
        assert_eq!(
            unresolved,
            vec!["cmux:6B1D0E9F-0000-4000-8000-000000000002".to_string()]
        );
    }

    #[test]
    fn a_harness_inside_one_multiplexer_inside_another_is_in_both_panes() {
        let panes = panes_from_env(
            env(&[
                ("TMUX", "/private/tmp/tmux-501/default,99,1"),
                ("TMUX_PANE", "%0"),
                ("HERDR_PANE_ID", "w2:p3"),
                ("HERDR_SOCKET_PATH", "/h.sock"),
            ]),
            |_, _| None,
        );
        assert_eq!(
            panes,
            vec![
                "tmux:%0@/private/tmp/tmux-501/default,99".to_string(),
                "herdr:w2:p3@/h.sock".to_string()
            ]
        );
    }

    #[test]
    fn a_feed_names_its_runs_panes() {
        let one = run_from_object(
            &serde_json::json!({"command": "make proof", "pane": "tmux:%1@/s,1"}),
            "agent",
        )
        .unwrap();
        assert_eq!(one.panes, vec!["tmux:%1@/s,1".to_string()]);
        let many = run_from_object(
            &serde_json::json!({"command": "make proof", "panes": ["tmux:%1@/s,1", "herdr:w1:p1@/h"]}),
            "agent",
        )
        .unwrap();
        assert_eq!(many.panes.len(), 2);
    }
}
