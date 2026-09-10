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
        return (Reading::from_reported(brackets, fraction, detail), Vec::new());
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
        None => Bounded {
            blocks: blocks
                .iter()
                .map(Block::render)
                .filter(|text| !text.is_empty())
                .collect(),
            withheld_lines: 0,
            withheld_blocks: 0,
            notice: None,
        },
    }
}
