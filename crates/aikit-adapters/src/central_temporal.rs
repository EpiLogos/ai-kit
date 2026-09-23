//! Central temporal-ground adapter.
//!
//! Central owns ProjectCentral NOW and the Day. AIKit consumes that owner surface
//! through `ctrl --json action run ...`; it does not parse Central's files or mint
//! a parallel temporal model. The provider is deliberately a normal external
//! process adapter so scripted argv tests and real-binary conformance use the same
//! call path.
//!
//! The legacy Flow store (`projectcentral.flow.*`) was retired by Central #177;
//! the Day is the integrated writing medium that replaced it, and it is named
//! here by reference (`central.day.read` → day ref + revision), never inlined.

use std::path::{Component, Path};

use aikit_core::{AikitError, Result};
use serde_json::{json, Value};

use crate::runner::{CommandRunner, Output};

pub const NOW_INSPECT_ACTION: &str = "projectcentral.now.inspect";
pub const DAY_READ_ACTION: &str = "central.day.read";
pub const CENTRAL_TEMPORAL_PROVIDER_VERSION: &str = "aikit.central-temporal/v1";

const MAX_ACTIVE_ITEMS: usize = 8;
const MAX_HUMAN_REFS: usize = 8;
const MAX_DAY_REFS: usize = 3;
const MAX_ITEM_RESULT_CHARS: usize = 600;
const MAX_RENDERED_CHARS: usize = 9_000;

/// The current root Day, by reference. `None` when Central has no open Day or
/// the read failed: the floor says so rather than inventing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CentralDayPointer {
    pub day_ref: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CentralTemporalGround {
    pub project: String,
    pub now: Value,
    pub day: Option<CentralDayPointer>,
}

impl CentralTemporalGround {
    /// Render an orientation fragment, not an authority or activation grant.
    pub fn render(&self) -> String {
        let mut lines = vec![
            "[Central temporal ground]".to_owned(),
            format!("owner: Central ({CENTRAL_TEMPORAL_PROVIDER_VERSION})"),
            format!("project: {}", self.project),
            "standing: current orientation only; this does not grant authority, invoke an Agent/model, or activate a capability.".to_owned(),
        ];

        let exists = self
            .now
            .get("exists")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        lines.push(format!(
            "NOW: {}",
            if exists { "present" } else { "not initialized" }
        ));

        if let Some(refs) = self.now.get("human_scratch").and_then(Value::as_array) {
            let refs = refs
                .iter()
                .filter_map(Value::as_str)
                .take(MAX_HUMAN_REFS)
                .collect::<Vec<_>>();
            if !refs.is_empty() {
                lines.push(format!("human-current-source refs: {}", refs.join(", ")));
            }
        }

        if let Some(items) = self.now.get("active_items").and_then(Value::as_array) {
            for item in items.iter().take(MAX_ACTIVE_ITEMS) {
                let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
                let kind = item.get("kind").and_then(Value::as_str).unwrap_or("item");
                let actor = item
                    .get("actor")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let status = item
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let subject = item.get("subject").and_then(Value::as_str).unwrap_or("");
                let result = item
                    .get("result")
                    .and_then(Value::as_str)
                    .map(|text| bounded(text, MAX_ITEM_RESULT_CHARS))
                    .unwrap_or_default();
                lines.push(format!(
                    "NOW {kind} {id} [{status}] by {actor}: {subject}{}",
                    if result.is_empty() {
                        String::new()
                    } else {
                        format!(" — {result}")
                    }
                ));
            }
        }

        if let Some(days) = self.now.get("day_records").and_then(Value::as_array) {
            let mut refs = days.iter().filter_map(Value::as_str).collect::<Vec<_>>();
            refs.sort();
            let start = refs.len().saturating_sub(MAX_DAY_REFS);
            if !refs[start..].is_empty() {
                lines.push(format!("recent DAY refs: {}", refs[start..].join(", ")));
            }
        }

        lines.push(match &self.day {
            Some(day) => format!(
                "Day: {} @ {} (read it with `ctrl --json action run {DAY_READ_ACTION} '{{}}'`)",
                day.day_ref, day.revision
            ),
            None => "Day: no open root Day".to_owned(),
        });

        bounded(&lines.join("\n"), MAX_RENDERED_CHARS)
    }
}

/// Consume Central's public Action surface for one Project physically resident in
/// `Central/Work`. Returns `Ok(None)` when this Project is not in that world.
pub fn read_central_temporal_ground<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    project_root: &Path,
) -> Result<Option<CentralTemporalGround>> {
    let Some(project) = project_member(central_root, project_root) else {
        return Ok(None);
    };

    let now = action(
        runner,
        central_root,
        NOW_INSPECT_ACTION,
        json!({"project": project}),
    )?;
    // The Day is a pointer, and optional: a world with no open Day (or an
    // older Central) still gets its NOW ground.
    let day = action(runner, central_root, DAY_READ_ACTION, json!({}))
        .ok()
        .and_then(|day| {
            Some(CentralDayPointer {
                day_ref: day.get("day_ref")?.as_str()?.to_owned(),
                revision: day.get("revision")?.as_str()?.to_owned(),
            })
        });

    Ok(Some(CentralTemporalGround { project, now, day }))
}

fn action<R: CommandRunner>(
    runner: &R,
    central_root: &Path,
    id: &str,
    input: Value,
) -> Result<Value> {
    let argv = vec![
        "ctrl".to_owned(),
        "--json".to_owned(),
        "--root".to_owned(),
        central_root.display().to_string(),
        "action".to_owned(),
        "run".to_owned(),
        id.to_owned(),
        input.to_string(),
    ];
    let output = runner.run(&argv)?;
    decode_action(output, &argv, id)
}

fn decode_action(output: Output, argv: &[String], id: &str) -> Result<Value> {
    if !output.ok() {
        return Err(AikitError::new(
            "central.action_failed",
            format!("Central Action {id} exited with status {}", output.status),
        )
        .with("action", id)
        .with("command", argv.join(" "))
        .with("stderr", output.stderr.trim().to_owned()));
    }
    let result: Value = serde_json::from_str(output.stdout.trim()).map_err(|error| {
        AikitError::new(
            "central.action_invalid_output",
            format!("Central Action {id} returned invalid JSON: {error}"),
        )
        .with("action", id)
    })?;
    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = result
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Central Action failed");
        return Err(AikitError::new("central.action_failed", message)
            .with("action", id)
            .with(
                "status",
                result
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
            ));
    }
    result.get("data").cloned().ok_or_else(|| {
        AikitError::new(
            "central.action_invalid_output",
            format!("Central Action {id} succeeded without data"),
        )
        .with("action", id)
    })
}

fn project_member(central_root: &Path, project_root: &Path) -> Option<String> {
    let relative = project_root.strip_prefix(central_root.join("Work")).ok()?;
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn bounded(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::ScriptedRunner;

    fn success(data: Value) -> String {
        json!({"ok": true, "status": "success", "action": "fixture", "data": data}).to_string()
    }

    #[test]
    fn non_central_project_is_not_invented_into_central() {
        let runner = ScriptedRunner::new();
        let value = read_central_temporal_ground(
            &runner,
            Path::new("/home/me/Central"),
            Path::new("/tmp/project"),
        )
        .unwrap();
        assert!(value.is_none());
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn owner_actions_supply_now_and_the_day_by_reference_only() {
        let now = success(json!({
            "exists": true,
            "human_scratch": ["ProjectCentral/now/user/current.md"],
            "active_items": [{"id":"h1","kind":"handoff","actor":"agent:Epii","status":"active","subject":"Current work","result":"continue from live state"}],
            "day_records": ["ProjectCentral/now/day/2026-09-01.md"]
        }));
        let day = success(json!({
            "day_ref": "central:day:control:root:2026-09-23",
            "revision": "central.content-fnv1a64/v1:1:abc",
            "content": "THE WHOLE DAY DOCUMENT",
            "automatic_agent_or_model_invocation": false
        }));
        let runner = ScriptedRunner::new()
            .on(NOW_INSPECT_ACTION, &now)
            .on(DAY_READ_ACTION, &day);

        let ground = read_central_temporal_ground(
            &runner,
            Path::new("/home/me/Central"),
            Path::new("/home/me/Central/Work/example"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(ground.project, "example");
        let rendered = ground.render();
        assert!(rendered.contains("central:day:control:root:2026-09-23"));
        assert!(
            !rendered.contains("THE WHOLE DAY DOCUMENT"),
            "a pointer, never the body"
        );
        assert!(
            !runner
                .call_lines()
                .iter()
                .any(|line| line.contains("projectcentral.flow.")),
            "the retired Flow store is never called"
        );
        assert!(runner
            .call_lines()
            .iter()
            .all(|line| line.contains("--json --root /home/me/Central action run")));
    }

    #[test]
    fn a_missing_day_is_said_not_fatal() {
        let now = success(json!({"exists": true}));
        let runner = ScriptedRunner::new().on(NOW_INSPECT_ACTION, &now).failing(
            DAY_READ_ACTION,
            1,
            "no DayRef or current today pointer",
        );
        let ground = read_central_temporal_ground(
            &runner,
            Path::new("/home/me/Central"),
            Path::new("/home/me/Central/Work/example"),
        )
        .unwrap()
        .unwrap();
        assert!(ground.day.is_none());
        assert!(ground.render().contains("Day: no open root Day"));
    }

    #[test]
    fn rendered_ground_is_bounded_and_non_authorising() {
        let ground = CentralTemporalGround {
            project: "example".into(),
            now: json!({"exists": true, "active_items": (0..20).map(|i| json!({
                "id": format!("h{i}"), "kind": "handoff", "actor": "agent", "status": "active",
                "subject": "s", "result": "x".repeat(2_000)
            })).collect::<Vec<_>>()}),
            day: None,
        };
        let rendered = ground.render();
        assert!(rendered.chars().count() <= MAX_RENDERED_CHARS);
        assert!(rendered.contains("does not grant authority"));
    }
}
