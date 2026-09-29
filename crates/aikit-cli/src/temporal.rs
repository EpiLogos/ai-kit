//! Causal temporal re-grounding for Agent lifecycle hooks.
//!
//! Central owns NOW and the Day; AIKit owns the lifecycle delivery plane. This
//! module composes those existing responsibilities by reading Central afresh for
//! every session-start, prompt-turn and pre-compaction event, then returning the
//! bounded owner reading through AIKit's existing hook injection channel.
//!
//! A prompt turn carries the ground only when it changed since this session
//! last received it ([`FloorLedger`]): re-reading is causal, re-sending the
//! same ground every turn is not. A body occupying a World Position gets no
//! prompt-turn floor at all — its lean entry, Refocus and `aikit whoami` carry
//! orientation, so no historical NOW strap rides its turns.

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
    ledger: Option<&FloorLedger>,
) {
    if !event_needs_reground(&event.kind) {
        return;
    }
    let (Some(project_root), Some(central_root)) = (project_root, central_root) else {
        return;
    };

    match read_central_temporal_ground(runner, central_root, project_root) {
        Ok(Some(ground)) => {
            let rendered = ground.render();
            if let (Some(ledger), Some(session)) =
                (ledger, crate::refocus::hook_session(&event.payload))
            {
                let digest = blake3::hash(rendered.as_bytes()).to_hex().to_string();
                if event.kind == HookEventKind::UserPromptSubmit
                    && ledger.last(&session).as_deref() == Some(digest.as_str())
                {
                    return;
                }
                ledger.record(&session, &digest);
            }
            decision.injected.insert(0, rendered)
        }
        Ok(None) => {}
        Err(error) => decision.warnings.push(format!(
            "Central temporal re-grounding unavailable for this turn: {}",
            error.message()
        )),
    }
}

/// The lifecycle floor. With a lean World-inhabitation entry (a Position
/// occupancy resolved for this body) the entry replaces the historical NOW
/// ground at SessionStart and names where to read it on demand; an occupied
/// body's prompt turns carry no floor (Refocus owns their re-orientation).
/// Every other body re-grounds as before, prompt turns only on change.
#[allow(clippy::too_many_arguments)]
pub fn session_floor<R: CommandRunner>(
    decision: &mut HookDecision,
    event: &HookEvent,
    project_root: Option<&Path>,
    central_root: Option<&Path>,
    runner: &R,
    lean_entry: Option<&str>,
    occupied: bool,
    ledger: Option<&FloorLedger>,
) {
    match lean_entry {
        Some(entry) if event.kind == HookEventKind::SessionStart => {
            decision.injected.insert(0, entry.to_owned())
        }
        _ if occupied && event.kind == HookEventKind::UserPromptSubmit => {}
        _ => reground(decision, event, project_root, central_root, runner, ledger),
    }
}

/// Per-session record of the last Central ground a turn carried. A prompt turn
/// whose ground is byte-identical to what this session already received is
/// skipped. The record is advisory orientation state, not delivery evidence.
#[derive(Debug, Clone)]
pub struct FloorLedger {
    dir: PathBuf,
}

impl FloorLedger {
    pub fn in_state(state: &Path) -> Self {
        Self {
            dir: state.join("temporal-floor"),
        }
    }

    fn path(&self, session: &str) -> PathBuf {
        self.dir.join(format!(
            "{}.digest",
            blake3::hash(session.as_bytes()).to_hex()
        ))
    }

    pub fn last(&self, session: &str) -> Option<String> {
        std::fs::read_to_string(self.path(session))
            .ok()
            .map(|digest| digest.trim().to_owned())
    }

    pub fn record(&self, session: &str, digest: &str) {
        if std::fs::create_dir_all(&self.dir).is_ok() {
            let _ = std::fs::write(self.path(session), digest);
        }
    }
}

/// Whether this process is a body stamped into a World Position occupancy
/// (`aikit inhabit` exports `OI_POSITION_REF` into the harness).
pub fn process_is_occupied() -> bool {
    std::env::var_os("OI_POSITION_REF").is_some_and(|value| !value.is_empty())
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

/// The Central root enclosing `path`, when `path` is the root itself or
/// anywhere beneath it — the `Control` register and the `Work` tree alike.
///
/// This is a *recognition* rule, not a project claim: it answers "is this path
/// inside a Central world?". The entity disclosure needs exactly that, because
/// the entities it names are materialised from the world root and the root
/// register is where they live. Project-scoped consumers keep
/// [`process_central_root`], which additionally requires the path to be a
/// Project beneath `Work` — so `compose` and temporal re-grounding are
/// unaffected by this widening.
pub fn central_root_enclosing(path: Option<&Path>) -> Option<PathBuf> {
    let path = path?;
    if let Some(root) = std::env::var_os("CENTRAL_ROOT").filter(|value| !value.is_empty()) {
        let root = PathBuf::from(root);
        return path.starts_with(&root).then_some(root);
    }
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()))?;
    let root = PathBuf::from(home).join("Central");
    path.starts_with(&root).then_some(root)
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

    fn day() -> String {
        success(json!({"day_ref":"central:day:control:root:2026-09-23","revision":"rev-day"}))
    }

    fn now_with(subject: &str) -> String {
        success(json!({
            "exists": true,
            "active_items": [{"id":"h1","kind":"handoff","actor":"agent","status":"active","subject":subject}],
            "human_scratch": [], "day_records": []
        }))
    }

    #[test]
    fn next_prompt_reads_changed_now_instead_of_reusing_session_start() {
        let (a, b) = (now_with("state A"), now_with("state B"));
        let runner = ScriptedRunner::new()
            .sequence("projectcentral.now.inspect", &[&a, &b])
            .on("central.day.read", &day());
        let central = Path::new("/home/me/Central");
        let project = Path::new("/home/me/Central/Work/example");

        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));
        let mut first = decision(HookEventKind::SessionStart);
        reground(
            &mut first,
            &start,
            Some(project),
            Some(central),
            &runner,
            None,
        );
        assert!(first.injected_text().contains("state A"));
        assert!(first
            .injected_text()
            .contains("central:day:control:root:2026-09-23"));

        let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, json!({}));
        let mut second = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut second,
            &prompt,
            Some(project),
            Some(central),
            &runner,
            None,
        );
        assert!(second.injected_text().contains("state B"));
        assert!(!second.injected_text().contains("state A"));
        assert!(!runner
            .call_lines()
            .iter()
            .any(|line| line.contains("projectcentral.flow.")));
    }

    #[test]
    fn a_prompt_turn_carries_the_ground_only_when_it_changed() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = FloorLedger::in_state(temp.path());
        let (a, b) = (now_with("state A"), now_with("state B"));
        let runner = ScriptedRunner::new()
            .sequence("projectcentral.now.inspect", &[&a, &a, &a, &b])
            .on("central.day.read", &day());
        let central = Path::new("/home/me/Central");
        let project = Path::new("/home/me/Central/Work/example");
        let session = json!({"session_id": "sess-1"});

        let start = HookEvent::new("claude", HookEventKind::SessionStart, session.clone());
        let mut first = decision(HookEventKind::SessionStart);
        reground(
            &mut first,
            &start,
            Some(project),
            Some(central),
            &runner,
            Some(&ledger),
        );
        assert!(first.injected_text().contains("state A"));

        // Same ground: the prompt turn is silent, twice.
        for _ in 0..2 {
            let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, session.clone());
            let mut turn = decision(HookEventKind::UserPromptSubmit);
            reground(
                &mut turn,
                &prompt,
                Some(project),
                Some(central),
                &runner,
                Some(&ledger),
            );
            assert!(turn.injected.is_empty(), "unchanged ground is not re-sent");
        }

        // Changed ground: delivered once.
        let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, session.clone());
        let mut changed = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut changed,
            &prompt,
            Some(project),
            Some(central),
            &runner,
            Some(&ledger),
        );
        assert!(changed.injected_text().contains("state B"));

        // Another session starts from nothing.
        let other = HookEvent::new(
            "claude",
            HookEventKind::UserPromptSubmit,
            json!({"session_id": "sess-2"}),
        );
        let runner = ScriptedRunner::new()
            .on("projectcentral.now.inspect", &b)
            .on("central.day.read", &day());
        let mut fresh = decision(HookEventKind::UserPromptSubmit);
        reground(
            &mut fresh,
            &other,
            Some(project),
            Some(central),
            &runner,
            Some(&ledger),
        );
        assert!(fresh.injected_text().contains("state B"));
    }

    fn handoff_runner() -> ScriptedRunner {
        let now = success(json!({
            "exists": true,
            "active_items": [{"id": "h1", "kind": "handoff", "actor": "agent", "status": "active",
                              "subject": "historical handoff", "result": "a long historical NOW dump"}],
            "human_scratch": [], "day_records": []
        }));
        ScriptedRunner::new()
            .on("projectcentral.now.inspect", &now)
            .on("central.day.read", &day())
    }

    #[test]
    fn a_lean_entry_replaces_the_historical_floor_at_session_start_only() {
        let central = Path::new("/home/me/Central");
        let project = Path::new("/home/me/Central/Work/example");
        let start = HookEvent::new("claude", HookEventKind::SessionStart, json!({}));

        let runner = handoff_runner();
        let mut lean = decision(HookEventKind::SessionStart);
        session_floor(
            &mut lean,
            &start,
            Some(project),
            Some(central),
            &runner,
            Some("[O:I World inhabitation — lean entry]"),
            true,
            None,
        );
        assert_eq!(
            lean.injected,
            vec!["[O:I World inhabitation — lean entry]".to_owned()]
        );
        assert!(!lean.injected_text().contains("historical handoff"));
        assert!(runner.calls().is_empty(), "the dump is not even read");

        // Without an occupancy the floor is byte-identical to the re-ground.
        let runner = handoff_runner();
        let mut unchanged = decision(HookEventKind::SessionStart);
        session_floor(
            &mut unchanged,
            &start,
            Some(project),
            Some(central),
            &runner,
            None,
            false,
            None,
        );
        let mut direct = decision(HookEventKind::SessionStart);
        reground(
            &mut direct,
            &start,
            Some(project),
            Some(central),
            &handoff_runner(),
            None,
        );
        assert_eq!(unchanged.injected, direct.injected);
        assert!(unchanged.injected_text().contains("historical handoff"));

        // An occupied body's prompt turn carries no historical floor…
        let prompt = HookEvent::new("claude", HookEventKind::UserPromptSubmit, json!({}));
        let runner = handoff_runner();
        let mut occupied = decision(HookEventKind::UserPromptSubmit);
        session_floor(
            &mut occupied,
            &prompt,
            Some(project),
            Some(central),
            &runner,
            None,
            true,
            None,
        );
        assert!(occupied.injected.is_empty());
        assert!(runner.calls().is_empty(), "the dump is not even read");

        // …while an unoccupied body's prompt turn re-grounds as before.
        let mut turn = decision(HookEventKind::UserPromptSubmit);
        session_floor(
            &mut turn,
            &prompt,
            Some(project),
            Some(central),
            &handoff_runner(),
            None,
            false,
            None,
        );
        assert!(turn.injected_text().contains("historical handoff"));
    }

    #[test]
    fn central_failure_is_visible_but_never_becomes_hook_denial() {
        let runner =
            ScriptedRunner::new().failing("projectcentral.now.inspect", 9, "owner unavailable");
        let event = HookEvent::new("codex", HookEventKind::PreCompact, json!({}));
        let mut result = decision(HookEventKind::PreCompact);
        reground(
            &mut result,
            &event,
            Some(Path::new("/home/me/Central/Work/example")),
            Some(Path::new("/home/me/Central")),
            &runner,
            None,
        );
        assert!(result.allowed);
        assert!(result.injected.is_empty());
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("Central temporal")));
    }
}
