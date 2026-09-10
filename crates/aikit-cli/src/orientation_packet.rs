//! Orientation packet (W2/W1, CASE 02 + CASE 09): the composed continuity
//! capability that meets a fresh session in a project with that project's
//! own NOW field — assembled through
//! `ctrl --json action run projectcentral.now.inspect`, bounded, and closed
//! to horizons the composition does not explicitly authorize.
//!
//! Descope law: this runs only when the active composition selected
//! `hook/continuity/orientation-packet` — never ambient. The content is the
//! project's already-recorded field, never invented here:
//!
//! * **Continuation** — the newest open handoff return (subject + bounded
//!   result), so a fresh session resumes the record instead of the chat
//!   (CASE 09's continuation-in-packet).
//! * **Open work** — remaining active returns and open questions, subjects
//!   only, within the budget.
//! * **Warnings** — invalid items disclosed, never silently dropped.
//!
//! Horizon apertures are closed by default: the field's human side
//! (`human_scratch`, the @1 personal horizon) is injected only when the
//! composition explicitly authorizes it (`include_human_scratch = true` in
//! the capsule's config). Other projects are structurally absent — the
//! assembly inspects exactly the project the session stands in, nothing else.
//!
//! Fail-open: no project, no field, or a failed ctrl call downgrades to an
//! honest absence or a warning on the decision; the session proceeds.

use std::path::{Path, PathBuf};

use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_core::hooks::HookEvent;
use serde_json::Value;

use crate::temporal::process_central_root;

/// The aperture over the @1 personal horizon, closed by default.
const DEFAULT_INCLUDE_HUMAN_SCRATCH: bool = false;
/// How many open items (the continuation included) survive the budget.
const DEFAULT_MAX_ITEMS: usize = 5;
/// How many characters of a returned result survive the budget.
const DEFAULT_MAX_RESULT_CHARS: usize = 280;

/// The tunings the composition may adjust for this packet. Missing keys
/// keep the closed, bounded defaults — the aperture law.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrientationConfig {
    pub include_human_scratch: bool,
    pub max_items: usize,
    pub max_result_chars: usize,
}

impl Default for OrientationConfig {
    fn default() -> Self {
        Self {
            include_human_scratch: DEFAULT_INCLUDE_HUMAN_SCRATCH,
            max_items: DEFAULT_MAX_ITEMS,
            max_result_chars: DEFAULT_MAX_RESULT_CHARS,
        }
    }
}

impl OrientationConfig {
    /// Read the tunings from the composed capsule's config table (the
    /// `[config.hook/continuity/orientation-packet]` section of the active
    /// composition). Absent table, absent keys, or wrong types keep the
    /// closed defaults; the values in effect are always the composition's.
    pub fn from_config(config: Option<&toml::value::Table>) -> Self {
        let mut tuned = Self::default();
        let Some(config) = config else {
            return tuned;
        };
        if let Some(value) = config.get("include_human_scratch").and_then(|v| v.as_bool()) {
            tuned.include_human_scratch = value;
        }
        if let Some(value) = config.get("max_items").and_then(|v| v.as_integer()) {
            tuned.max_items = value.max(0) as usize;
        }
        if let Some(value) = config.get("max_result_chars").and_then(|v| v.as_integer()) {
            tuned.max_result_chars = value.max(0) as usize;
        }
        tuned
    }
}

/// The project the event stands in: `Work/<Name>` under the Central root.
/// Anywhere else (the root itself, outside the world) has no project NOW
/// field to assemble, and the packet is honestly absent.
pub fn project_of(central_root: &Path, cwd: &Path) -> Option<String> {
    let relative = cwd.strip_prefix(central_root).ok()?;
    let mut parts = relative.components();
    if parts.next()?.as_os_str() != "Work" {
        return None;
    }
    parts.next()?.as_os_str().to_str().map(str::to_owned)
}

fn ctrl_executable() -> PathBuf {
    std::env::var_os("CENTRAL_CTRL_BIN")
        .or_else(|| std::env::var_os("OI_CENTRAL_CTRL_BIN"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ctrl"))
}

/// The packet for this event's context, or `None` when there is nothing to
/// orient (not a project under the world, no field, nothing open).
pub fn orientation_packet(
    event: &HookEvent,
    config: &OrientationConfig,
) -> Result<Option<String>, String> {
    let central_root = process_central_root(event.cwd.as_deref());
    orientation_packet_in(
        &SystemRunner::new(),
        central_root.as_deref(),
        event.cwd.as_deref(),
        config,
    )
}

/// The injectable core: assemble the packet for `cwd` as seen from the
/// Central world `central_root`.
pub fn orientation_packet_in<R: CommandRunner>(
    runner: &R,
    central_root: Option<&Path>,
    cwd: Option<&Path>,
    config: &OrientationConfig,
) -> Result<Option<String>, String> {
    let (Some(central_root), Some(cwd)) = (central_root, cwd) else {
        return Ok(None);
    };
    let Some(project) = project_of(central_root, cwd) else {
        return Ok(None);
    };

    let field = inspect_project_now(runner, central_root, &project)?;
    if field["exists"] != true {
        return Ok(None);
    }
    render_packet(&project, &field, config)
}

/// One ctrl call, for exactly this project: no other project's field is ever
/// requested, so no other project's material can arrive.
///
/// Shared with the close-out verification, which asks the same owner the same
/// question: what does this project's NOW field actually hold? Two readers of
/// one call site cannot drift apart in how they address the field.
pub fn inspect_project_now<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: &str,
) -> Result<Value, String> {
    inspect_project_now_for(runner, central_root, project, "continuity/orientation-packet")
}

/// The same call, with the caller's own name on any failure it reports.
pub fn inspect_project_now_for<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project: &str,
    reader: &str,
) -> Result<Value, String> {
    let executable = ctrl_executable();
    let input = format!(r#"{{"project":"{project}"}}"#);
    let argv = vec![
        executable.to_string_lossy().into_owned(),
        "--json".into(),
        "--root".into(),
        central_root.to_string_lossy().into_owned(),
        "action".into(),
        "run".into(),
        "projectcentral.now.inspect".into(),
        input,
    ];
    let output = runner
        .run(&argv)
        .map_err(|error| format!("{reader} ctrl unavailable: {error}"))?;
    if output.status != 0 {
        return Err(format!(
            "{reader} ctrl failed ({}): {}",
            output.status,
            output.stderr.trim()
        ));
    }
    let envelope: Value = serde_json::from_str(&output.stdout)
        .map_err(|error| format!("{reader} unreadable ctrl reply: {error}"))?;
    if envelope["ok"] != true {
        return Err(format!(
            "{reader} ctrl refused: {}",
            envelope["error"]["message"]
                .as_str()
                .unwrap_or("unknown error")
        ));
    }
    Ok(envelope["data"].clone())
}

fn bounded_result(result: &str, config: &OrientationConfig) -> String {
    let flat = result.trim();
    if flat.chars().count() <= config.max_result_chars {
        return flat.to_owned();
    }
    let mut cut: String = flat.chars().take(config.max_result_chars).collect();
    cut.push('…');
    cut
}

fn render_packet(
    project: &str,
    field: &Value,
    config: &OrientationConfig,
) -> Result<Option<String>, String> {
    let empty = Vec::new();
    let active = field["active_items"].as_array().unwrap_or(&empty);
    let questions = field["open_questions"].as_array().unwrap_or(&empty);
    let invalid = field["invalid_items"].as_array().unwrap_or(&empty);
    let scratch = field["human_scratch"].as_array().unwrap_or(&empty);

    if active.is_empty() && questions.is_empty() && invalid.is_empty() {
        // Nothing open to orient with: honest absence rather than a packet
        // about nothing.
        return Ok(None);
    }

    // Continuation first: the newest open handoff return, result bounded.
    let newest_handoff = active
        .iter()
        .filter(|item| item["kind"].as_str() == Some("handoff"))
        .max_by_key(|item| item["recorded_at_unix_seconds"].as_i64().unwrap_or(0));

    let mut lines = Vec::new();
    let mut shown = 0usize;
    if let Some(item) = newest_handoff {
        let subject = item["subject"].as_str().unwrap_or("(untitled)");
        let actor = item["actor"].as_str().unwrap_or("unknown actor");
        let run = item["run_ref"]
            .as_str()
            .map(|run| format!(", run: {run}"))
            .unwrap_or_default();
        lines.push(format!("- continuation: {subject} (actor: {actor}{run})"));
        if let Some(result) = item["result"].as_str() {
            for line in bounded_result(result, config).lines() {
                lines.push(format!("  {line}"));
            }
        }
        shown += 1;
    }

    // Remaining open returns: subjects only, within the budget. `ctrl`'s real
    // `projectcentral.now.inspect` pushes every active record into
    // `active_items` regardless of kind, and *additionally* pushes
    // `question`-kind records into `open_questions` — the same record
    // surfaced through two fields, not two records. Question-kind entries
    // are skipped here and rendered once, from the `open_questions` loop
    // below, so a question never charges the budget or the packet twice.
    let continuation_id = newest_handoff.and_then(|item| item["id"].as_str());
    for item in active {
        if shown >= config.max_items {
            break;
        }
        if Some(item["id"].as_str().unwrap_or("")) == continuation_id {
            continue;
        }
        let kind = item["kind"].as_str().unwrap_or("item");
        if kind == "question" {
            continue;
        }
        let subject = item["subject"].as_str().unwrap_or("(untitled)");
        lines.push(format!("- {kind}: {subject}"));
        shown += 1;
    }

    // Open questions fill the remaining budget (they are handoff records of
    // kind `question`, already counted once in `active_items` above — this
    // is their one and only render).
    for question in questions {
        if shown >= config.max_items {
            break;
        }
        let subject = question["subject"].as_str().unwrap_or("(open question)");
        lines.push(format!("- question: {subject}"));
        shown += 1;
    }

    // `open_questions` is a subset of `active_items` (every question-kind
    // record lives in both), so the distinct-item total is `active.len()`
    // alone — summing the two would double-count each open question.
    let total_open = active.len();
    if total_open > shown {
        lines.push(format!(
            "- … {} further open item(s) withheld by the orientation budget",
            total_open - shown
        ));
    }

    for item in invalid {
        let id = item["id"]
            .as_str()
            .or_else(|| item.as_str())
            .unwrap_or("(invalid record)");
        lines.push(format!("- warning: invalid NOW record disclosed: {id}"));
    }

    // The @1 personal horizon: closed unless the composition opened it.
    if config.include_human_scratch && !scratch.is_empty() {
        lines.push("- human scratch (aperture open by composition):".to_owned());
        for entry in scratch.iter().take(config.max_items) {
            let subject = entry.as_str().unwrap_or("(scratch entry)");
            lines.push(format!("  - {subject}"));
        }
    }

    // Header counts: `active.len()` already includes every question-kind
    // record (they live in both fields), so "N open item(s)" is the honest
    // total distinct open records, and "M open question(s)" names how many
    // of those N are questions — a subset call-out, not an addend. The two
    // numbers are never summed anywhere in this render, matching `total_open`
    // above.
    Ok(Some(format!(
        "[continuity/orientation-packet] project {project} — {} open item(s), {} open question(s) (composed):\n{}",
        active.len(),
        questions.len(),
        lines.join("\n")
    )))
}
