//! Frecency: you should never have to type an id.
//!
//! zoxide's insight is that you do not need aliases if the tool learns which
//! partial match you meant — the substring you happen to type *is* the alias, and
//! it costs nothing to create because you did not create it. That transfers here
//! almost unchanged, because capsule ids are paths.
//!
//! ## The correction this module exists to encode
//!
//! An earlier draft had one `score()` blending match quality *and* usage. That is
//! wrong, and fzf and nucleo are both right not to do it:
//!
//! > **`score` is match quality alone. Usage lives in an ordered tiebreak.**
//!
//! A single blended number is unstable — the same query returns a different order
//! tomorrow because a counter moved — and it is unexplainable, because "why did
//! this rank first" has no answer you can show a user. It also quietly violates
//! Part I's explainability requirement.
//!
//! The tiebreak is ordered, and it ends in [`Tiebreak::CapsuleId`] for a **total
//! order**. That last rung matters more than it looks: without it, equally-scored
//! equally-used candidates swap places between keystrokes, which reads as a broken
//! UI.
//!
//! ## What frecency may never do
//!
//! It ranks. It does not activate. A frecent capsule that no scope selects is still
//! inactive, and `z` on it proposes *running* it, not enabling it — running and
//! being active are different acts (Part I rule 6).

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::id::CapsuleId;
use crate::search::UsageStats;

/// The tiebreaks, applied **in order**, only between candidates of equal score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tiebreak {
    /// You typed the command's actual name.
    ExactExportName,
    /// A match here beats a match elsewhere.
    CurrentProject,
    /// Already active beats merely catalogued.
    ActiveInContext,
    /// Successful uses, recency-decayed.
    Frecency,
    /// A total order, so results never jitter.
    CapsuleId,
}

impl Tiebreak {
    /// The ladder, highest priority first. Scope beats globality (`CurrentProject`
    /// above `Frecency`) — a match in the project you are in outranks a more
    /// frecent match from somewhere else.
    pub const ORDER: [Tiebreak; 5] = [
        Tiebreak::ExactExportName,
        Tiebreak::CurrentProject,
        Tiebreak::ActiveInContext,
        Tiebreak::Frecency,
        Tiebreak::CapsuleId,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Tiebreak::ExactExportName => "exact-export-name",
            Tiebreak::CurrentProject => "current-project",
            Tiebreak::ActiveInContext => "active-in-context",
            Tiebreak::Frecency => "frecency",
            Tiebreak::CapsuleId => "capsule-id",
        }
    }
}

/// One ranked candidate: its match quality, and the facts the tiebreaks read.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: CapsuleId,
    /// Match quality **alone**, in `[0, 1]`. Never blended with usage.
    pub score: f32,
    pub exact_export_name: bool,
    pub in_current_project: bool,
    pub active_in_context: bool,
    pub usage: UsageStats,
}

impl Candidate {
    pub fn new(id: CapsuleId, score: f32) -> Self {
        Self {
            id,
            score,
            exact_export_name: false,
            in_current_project: false,
            active_in_context: false,
            usage: UsageStats::default(),
        }
    }

    /// Recency-decayed successful uses. **Success, not invocation**: a script you
    /// run and abort five times a day should not become your top match.
    pub fn frecency(&self, half_life: std::time::Duration) -> f32 {
        if self.usage.successful_runs == 0 {
            return 0.0;
        }
        let Some(age) = self.usage.last_success_age else {
            return 0.0;
        };
        let half_life = half_life.as_secs_f32().max(f32::EPSILON);
        let decay = 0.5f32.powf(age.as_secs_f32() / half_life);
        self.usage.successful_runs as f32 * decay
    }

    /// Which rung of the ladder decided this candidate against `other`.
    ///
    /// Returned so the UI can *show* why one row beat another, which is the whole
    /// point of keeping the tiebreak ordered rather than blending it into a number.
    pub fn deciding_tiebreak(
        &self,
        other: &Self,
        half_life: std::time::Duration,
    ) -> Option<Tiebreak> {
        if self.exact_export_name != other.exact_export_name {
            return Some(Tiebreak::ExactExportName);
        }
        if self.in_current_project != other.in_current_project {
            return Some(Tiebreak::CurrentProject);
        }
        if self.active_in_context != other.active_in_context {
            return Some(Tiebreak::ActiveInContext);
        }
        if self.frecency(half_life) != other.frecency(half_life) {
            return Some(Tiebreak::Frecency);
        }
        if self.id != other.id {
            return Some(Tiebreak::CapsuleId);
        }
        None
    }
}

/// The one tunable, stated as a documented half-life rather than a magic constant.
pub const DEFAULT_HALF_LIFE: std::time::Duration =
    std::time::Duration::from_secs(14 * 24 * 60 * 60);

/// Rank candidates: by score, then down the ordered tiebreak ladder.
///
/// Deterministic and total. Two runs over the same inputs give the same order, and
/// no two distinct candidates ever compare equal, because the ladder ends in the
/// capsule id.
pub fn rank(candidates: &mut [Candidate], half_life: std::time::Duration) {
    candidates.sort_by(|a, b| compare(a, b, half_life));
}

/// The comparison `rank` uses. Higher is better, so this is a *descending* order.
pub fn compare(a: &Candidate, b: &Candidate, half_life: std::time::Duration) -> Ordering {
    // Match quality first, and alone.
    match b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal) {
        Ordering::Equal => {}
        other => return other,
    }
    // Then the ladder, in order.
    b.exact_export_name
        .cmp(&a.exact_export_name)
        .then_with(|| b.in_current_project.cmp(&a.in_current_project))
        .then_with(|| b.active_in_context.cmp(&a.active_in_context))
        .then_with(|| {
            b.frecency(half_life)
                .partial_cmp(&a.frecency(half_life))
                .unwrap_or(Ordering::Equal)
        })
        // The total order at the bottom: ascending id, so it is stable and
        // predictable rather than merely deterministic.
        .then_with(|| a.id.cmp(&b.id))
}

/// Segment-aware match quality: **match on the tail first.**
///
/// `nextest` should beat a capsule whose *group* is `nextest`, the same way
/// `z docs` prefers a directory named `docs` over one merely containing it. This is
/// match quality, not usage, so it belongs in the score.
///
/// The constants are expressed as *relationships* rather than magic numbers
/// (fzf's discipline), so the tuning stays legible when someone changes one:
/// an exact leaf is the ceiling, a leaf prefix is most of it, and matching only
/// in an earlier segment is worth distinctly less than matching the tail.
pub fn match_quality(query: &str, id: &CapsuleId) -> f32 {
    const EXACT_LEAF: f32 = 1.0;
    const LEAF_PREFIX: f32 = EXACT_LEAF * 0.9;
    // Matching the END of the leaf is tighter than matching its middle: typing
    // `nextest` means `cargo-nextest`, not `cargo-nextest-helper`. This is the
    // same "match on the tail first" instinct as preferring the leaf over the
    // group, applied one level further in — and it is what lets `z` decide
    // instead of asking.
    const LEAF_SUFFIX: f32 = EXACT_LEAF * 0.8;
    const LEAF_SUBSTRING: f32 = EXACT_LEAF * 0.7;
    // Matching away from the tail is worth meaningfully less than matching it.
    const OTHER_SEGMENT: f32 = LEAF_SUBSTRING * 0.5;
    const SUBSEQUENCE: f32 = OTHER_SEGMENT * 0.5;

    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return 0.0;
    }
    let rendered = id.to_string().to_lowercase();
    let leaf = id.leaf().to_lowercase();

    if leaf == query {
        return EXACT_LEAF;
    }
    if leaf.starts_with(&query) {
        return LEAF_PREFIX;
    }
    if leaf.ends_with(&query) {
        return LEAF_SUFFIX;
    }
    if leaf.contains(&query) {
        return LEAF_SUBSTRING;
    }
    if rendered.contains(&query) {
        return OTHER_SEGMENT;
    }
    if is_subsequence(&query, &rendered) {
        return SUBSEQUENCE;
    }
    0.0
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut chars = needle.chars();
    let mut current = chars.next();
    for c in haystack.chars() {
        if Some(c) == current {
            current = chars.next();
            if current.is_none() {
                return true;
            }
        }
    }
    current.is_none()
}

/// What `aikit z` decided to do.
///
/// Ambiguity is never an error message — it is the interactive case, one keystroke
/// from resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum Jump {
    /// Exactly one candidate stands out: act on it.
    Act { capsule: CapsuleId },
    /// Several plausible candidates: open the palette pre-filtered.
    Disambiguate { candidates: Vec<CapsuleId> },
    /// Nothing matched.
    Nothing,
}

/// Decide what `z` should do with a ranked list.
///
/// "Unambiguous" is deliberately not "exactly one result": it is *one clear
/// winner*. A top score that ties with the runner-up is ambiguity even though both
/// matched, and pretending otherwise would run the wrong thing on a coin toss.
pub fn decide(ranked: &[Candidate]) -> Jump {
    let matched: Vec<&Candidate> = ranked.iter().filter(|c| c.score > 0.0).collect();
    match matched.split_first() {
        None => Jump::Nothing,
        Some((best, rest)) => {
            let contested = rest.first().is_some_and(|next| {
                (next.score - best.score).abs() < f32::EPSILON && !best.exact_export_name
            });
            if contested {
                Jump::Disambiguate {
                    candidates: matched.iter().map(|c| c.id.clone()).collect(),
                }
            } else {
                Jump::Act {
                    capsule: best.id.clone(),
                }
            }
        }
    }
}
