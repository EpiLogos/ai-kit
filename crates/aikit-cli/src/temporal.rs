//! Causal temporal re-grounding for Agent lifecycle hooks.
//!
//! Central owns NOW/DAY/Flow; AIKit owns the lifecycle delivery plane. This module
//! composes those existing responsibilities by reading Central afresh for every
//! session-start, prompt-turn and pre-compaction event, then returning the bounded
//! owner reading through AIKit's existing hook injection channel.

use std::path::{Path, PathBuf};

use aikit_adapters::central_temporal::read_central_temporal_ground;
use aikit_adapters::runner::CommandRunner;
use aikit_core::hooks::{HookDecision, HookEvent, HookEventKind};

pub fn event_needs_reground(kind: &HookEventKind) -> bool {
    matches!(
        kind,
        HookEventKind::SessionStart | HookEventKind::UserPromptSubmit | HookEventKind::PreCompact
    )
}

/// Add current Central orientation to an already-computed hook decision.
///
/// This is fail-soft because a non-Central Project remains a valid AIKit world and
/// a temporarily unavailable `ctrl` binary must not counterfeit a hook denial.
/// When a Project is actually bound to Central and the owner read fails, the
/// warning stays visible in the decision.
pub fn reground<R: CommandRunner>(
    decision: &mut HookDecision,
    event: &HookEvent,
    project_root: Option<&Path>,
    central_root: Option<&Path>,
    runner: &R,
) {
    if !event_needs_reground(&event.kind) {
        return;
    }
    let (Some(project_root), Some(central_root)) = (project_root, central_root) else {
        return;
    };

    match read_central_temporal_ground(runner, central_root, project_root) {
        Ok(Some(ground)) => decision.injected.insert(0, ground.render()),
        Ok(None) => {}
        Err(error) => decision.warnings.push(format!(
            "Central temporal re-grounding unavailable for this turn: {}",
            error.message()
        )),
    }
}

/// Resolve the Central root without teaching AIKit Central's internal file
/// layout. `CENTRAL_ROOT` is authoritative when present; otherwise the standard
/// Central home is considered only when the current Project is physically inside
/// its `Work` tree.
pub fn process_central_root(project_root: Option<&Path>) -> Option<PathBuf> {
    let project_root = project_root?;
    if let Some(root) = std::env::var_os("CENTRAL_ROOT").filter(|value| !value.is_empty()) {
        let root = PathBuf::from(root);
        if project_root.starts_with(root.join("Work")) {
            return Some(root);
        }
        return None;
    }
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()))?;
    let root = PathBuf::from(home).join("Central");
    project_root.starts_with(root.join("Work")).then_some(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::runner::ScriptedRunner;
    use serde_json::json;

    fn success(data: serde_json::Value) -> String {
        json!({"ok":true,"status":"success","action":"fixture","data":data}).to_string()
    }

    fn decision(kind: HookEventKind) -> HookDecision {
        HookDecision {
            event: kind,
            allowed: true,
            denial: None,
            payload: json!({}),
            injected: vec![],
            warnings: vec![],
            steps: vec![],
            groups: vec![],
            bypass_consumed: false,
        }
    }

    #[test]
    fn only_causal_orientation_events_read_central() {
        assert!(event_needs_reground(&HookEventKind::SessionStart));
        assert!(event_needs_reground(&HookEventKind::UserPromptSubmit));
        assert!(event_needs_reground(&HookEventKind::PreCompact));
        assert!(!event_needs_reground(&HookEventKind::PreToolUse));
        assert!(!event_needs_reground(&HookEventKind::PostToolUse));
    }

    #[test]
    fn next_prompt_reads_changed_flow_revision_instead_of_reusing_session_start() {
        let now = success(json!({"exists":true,"active_items":[],"human_scratch":[],"day_records":[]}));
        let list = success(json!({
            "flows":[{"flow_ref":"central:flow:work","lifecycle":"active"}],
            "automatic_agent_or_model_invocation":false
        }));
        let read_a = success(json!({
            "flow":{"flow_ref":"central:flow:work","current_revision":"rev-a","lifecycle":"active","privacy":"project","title":"Work"},
            "content":"state A",
            "automatic_agent_or_model_invocation":false
        }));
        let read_b = success(json!({
            "flow":{"flow_ref":"central:flow:work","current_revision":"rev-b","lifecycle":"active","privacy":"project","title":"Work"},
            "content":"state B",
            "automatic_agent_or_model_invocation":false
        }));
        let runner = ScriptedRunner::new()
            .sequence("projectcentral.now.inspect", &[&now, &now])
            .sequence("projectcentral.flow.list", &[&list, &list])
            .sequence("projectcentral.flow.read", &[&read_a, &read_b]);
        let central = Path::new("/home/me/Central");
        let project = Path::new("/home/me/Central/Work/example");

        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));
        let mut first = decision(HookEventKind::SessionStart);
        reground(&mut first, &start, Some(project), Some(central), &runner);
        assert!(first.injected_text().contains("rev-a"));
        assert!(first.injected_text().contains("state A"));

        let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, json!({}));
        let mut second = decision(HookEventKind::UserPromptSubmit);
        reground(&mut second, &prompt, Some(project), Some(central), &runner);
        assert!(second.injected_text().contains("rev-b"));
        assert!(second.injected_text().contains("state B"));
        assert!(!second.injected_text().contains("state A"));
    }

    #[test]
    fn central_failure_is_visible_but_never_becomes_hook_denial() {
        let runner = ScriptedRunner::new().failing("projectcentral.now.inspect", 9, "owner unavailable");
        let event = HookEvent::new("codex", HookEventKind::PreCompact, json!({}));
        let mut result = decision(HookEventKind::PreCompact);
        reground(
            &mut result,
            &event,
            Some(Path::new("/home/me/Central/Work/example")),
            Some(Path::new("/home/me/Central")),
            &runner,
        );
        assert!(result.allowed);
        assert!(result.injected.is_empty());
        assert!(result.warnings.iter().any(|warning| warning.contains("Central temporal")));
    }
}
