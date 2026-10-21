//! Context pressure at the engine seam (W1/CASE 04).
//!
//! The composed reaction that turns a reading into a bound. Three things live
//! here that core cannot own: where the reading comes from (the event, or the
//! session's own turn count in the store), how the composition's tuning is
//! read at event time, and the tick that makes the fallback countable.
//!
//! Descope law: this runs only when the active composition selected
//! `hook/continuity/context-pressure`. Uncomposed, the engine's blocks are
//! rendered exactly as they were before this existed — no bound, no notice,
//! no tick recorded.

use aikit_core::hooks::HookEvent;
use aikit_core::pressure::{bound, Block, Bounded, PressureBrackets, Reading};
use aikit_store::index::Index;

/// The consumption figure a harness reported for this session, if it reported
/// one.
///
/// No capability descriptor declares this today, which is exactly why it is
/// read defensively and why its absence is a normal answer rather than a
/// warning: the fallback is the ordinary path, not an error path. Two shapes
/// are accepted — a fraction, or used/total token counts — because those are
/// the two ways a harness that starts reporting would plausibly report.
pub fn reported_consumption(event: &HookEvent) -> Option<(f64, String)> {
    let payload = &event.payload;
    for key in ["context_used_fraction", "context_pressure", "used_fraction"] {
        if let Some(fraction) = payload.get(key).and_then(|value| value.as_f64()) {
            return Some((fraction, format!("{key} reported by {}", event.client)));
        }
    }
    let used = ["context_used_tokens", "used_tokens", "input_tokens"]
        .iter()
        .find_map(|key| payload.get(*key).and_then(|value| value.as_f64()))?;
    let window = ["context_window_tokens", "context_window", "max_tokens"]
        .iter()
        .find_map(|key| payload.get(*key).and_then(|value| value.as_f64()))?;
    if window <= 0.0 {
        return None;
    }
    Some((
        used / window,
        format!("{used:.0}/{window:.0} tokens reported by {}", event.client),
    ))
}

/// The reading in effect for this event: the harness's figure when there is
/// one, otherwise this session's prompt count against the declared budget.
///
/// The count is read from the store, not from the process, because a hook
/// dispatcher is a fresh short-lived process every turn — an in-memory counter
/// would read 1 forever.
pub fn read(
    index: &Index,
    scope: &str,
    event: &HookEvent,
    brackets: &PressureBrackets,
) -> (Reading, Vec<String>) {
    if let Some((fraction, detail)) = reported_consumption(event) {
        return (
            Reading::from_reported(brackets, fraction, detail),
            Vec::new(),
        );
    }
    match index.prompt_count(scope) {
        Ok(prompts) => (Reading::from_prompt_count(brackets, prompts), Vec::new()),
        Err(error) => (
            Reading::from_prompt_count(brackets, 0),
            vec![format!(
                "continuity/context-pressure: turn count unavailable ({error}); \
                 reading this turn as a fresh session"
            )],
        ),
    }
}

/// Record that this session spent a prompt turn.
///
/// Fail-open: a store that cannot record a tick must not break the turn. The
/// consequence of a lost tick is a slightly optimistic pressure reading, which
/// is a smaller harm than a failed dispatch.
pub fn record_turn(index: &Index, scope: &str) -> Vec<String> {
    match index.record_prompt(scope) {
        Ok(()) => Vec::new(),
        Err(error) => vec![format!(
            "continuity/context-pressure: turn not counted ({error}); \
             the next reading will be low by one"
        )],
    }
}

/// Apply the reading to this turn's blocks, or render them whole when no
/// pressure capability is composed.
pub fn apply(blocks: &[Block], reading: Option<&Reading>) -> Bounded {
    match reading {
        Some(reading) => bound(blocks, reading.pressure),
        None => aikit_core::pressure::whole(blocks),
    }
}

/// The last bound each hook event gave one scope, kept fail-open so a reader
/// can see what the turn's context carried and what pressure withheld
/// (`aikit work continuity turn`). One document per scope under the home's
/// `state/continuity/turns/`, replaced whole (written aside, then renamed).
pub const TURN_BOUND_VERSION: &str = "aikit.turn-bound/v1";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventBound {
    pub recorded_at_unix_ms: u64,
    /// The pressure reading the bound used; absent when no pressure
    /// capability is composed (the blocks then arrive whole).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<serde_json::Value>,
    pub blocks: Vec<aikit_core::pressure::BlockAccount>,
    pub withheld_lines: usize,
    pub withheld_blocks: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TurnBound {
    pub version: String,
    pub scope: String,
    /// The last bound per hook event (UserPromptSubmit, SessionStart, …).
    pub events: std::collections::BTreeMap<String, EventBound>,
}

fn turn_path(home: &aikit_store::home::AikitHome, scope: &str) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(scope.as_bytes());
    let name: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    home.state()
        .join("continuity")
        .join("turns")
        .join(format!("{name}.json"))
}

/// The last turn bounds kept for one scope, or `None` when none was kept.
pub fn read_turn(
    home: &aikit_store::home::AikitHome,
    scope: &str,
) -> aikit_core::Result<Option<TurnBound>> {
    let path = turn_path(home, scope);
    let body = match std::fs::read(&path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(aikit_core::AikitError::new(
                "continuity.turn_unreadable",
                format!("cannot read {}: {error}", path.display()),
            ))
        }
    };
    let turn: TurnBound = serde_json::from_slice(&body).map_err(|error| {
        aikit_core::AikitError::new(
            "continuity.turn_unreadable",
            format!("{} is not a turn bound: {error}", path.display()),
        )
    })?;
    if turn.scope != scope {
        return Err(aikit_core::AikitError::new(
            "continuity.turn_unreadable",
            format!("{} keeps another scope's turn", path.display()),
        ));
    }
    Ok(Some(turn))
}

/// Keep this event's bound as the scope's last one for it. Fail-open: a
/// store that cannot keep it must not break the turn; the warning says so.
pub fn record_turn_bound(
    home: &aikit_store::home::AikitHome,
    scope: &str,
    event: &HookEvent,
    reading: Option<&Reading>,
    bounded: &Bounded,
) -> Vec<String> {
    let kept = (|| -> Result<(), String> {
        let path = turn_path(home, scope);
        let directory = path.parent().ok_or("no turn directory")?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let mut turn = match read_turn(home, scope) {
            Ok(Some(turn)) => turn,
            _ => TurnBound {
                version: TURN_BOUND_VERSION.into(),
                scope: scope.into(),
                events: Default::default(),
            },
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or_default();
        turn.events.insert(
            event.kind.as_str().to_owned(),
            EventBound {
                recorded_at_unix_ms: now,
                pressure: reading.map(Reading::describe),
                blocks: bounded.accounts.clone(),
                withheld_lines: bounded.withheld_lines,
                withheld_blocks: bounded.withheld_blocks,
                notice: bounded.notice.clone(),
            },
        );
        let body = serde_json::to_vec_pretty(&turn).map_err(|error| error.to_string())?;
        let mut aside =
            tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
        std::io::Write::write_all(&mut aside, &body).map_err(|error| error.to_string())?;
        aside.persist(&path).map_err(|error| error.to_string())?;
        Ok(())
    })();
    match kept {
        Ok(()) => Vec::new(),
        Err(error) => vec![format!(
            "continuity: this turn's bound was not kept ({error}); `aikit work continuity turn` reads the previous one"
        )],
    }
}
