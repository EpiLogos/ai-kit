//! Capability lifecycle — a capability's place in its life, **derived**, never
//! authored.
//!
//! Hermes proves that a capability tree wants a lifecycle: skills age, go quiet,
//! and eventually want a look (PRIOR-ART-ACTIONS L1). AIKit adds the resolution
//! and the guardrail Hermes lacks — the lifecycle is *observed*, and the only
//! thing that ever acts on it is a curator that **proposes**, never archives on a
//! timer (L4, in `aikit-store`).
//!
//! The state is derived, so a manifest cannot declare it: usefulness is evidence
//! of usefulness and nothing else, and freezing "stable" into a file the day it
//! was written would be exactly the stale claim this avoids. The one input a human
//! controls is `archived` — set by a confirmed curation, never by this crate.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::search::UsageStats;

/// Days as seconds, for the documented thresholds below.
const DAY: u64 = 24 * 60 * 60;

/// Where a capability sits in its life. Ordered from most to least alive so a
/// listing can sort by concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityLifecycle {
    /// Used recently, or newly catalogued and not yet aged out.
    Active,
    /// Idle for a while — quiet, not yet a concern.
    Quiet,
    /// Idle long enough to be a candidate for review.
    Stale,
    /// Archived by a human-confirmed curation; still catalogued for audit.
    Retired,
}

impl CapabilityLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            CapabilityLifecycle::Active => "active",
            CapabilityLifecycle::Quiet => "quiet",
            CapabilityLifecycle::Stale => "stale",
            CapabilityLifecycle::Retired => "retired",
        }
    }

    /// Whether this state is one the curator should surface for review.
    pub fn wants_review(self) -> bool {
        matches!(self, CapabilityLifecycle::Stale)
    }
}

/// The idle thresholds that turn "how long since it was last useful" into a
/// lifecycle state.
///
/// Documented defaults: **quiet after 30 idle days, stale after 90.** Chosen as
/// relationships a reader can see rather than magic numbers scattered across call
/// sites — `stale_after` is three times `quiet_after`, the Hermes default ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleThresholds {
    pub quiet_after: Duration,
    pub stale_after: Duration,
}

impl Default for LifecycleThresholds {
    fn default() -> Self {
        Self {
            quiet_after: Duration::from_secs(30 * DAY),
            stale_after: Duration::from_secs(90 * DAY),
        }
    }
}

/// Derive the lifecycle from the idle time (time since the capability was last
/// useful) and whether a human has archived it.
///
/// `archived` short-circuits to [`CapabilityLifecycle::Retired`] — a retired
/// capability that suddenly runs is a decision for the curator, not something this
/// function silently reverses.
pub fn derive(
    idle: Duration,
    archived: bool,
    thresholds: &LifecycleThresholds,
) -> CapabilityLifecycle {
    if archived {
        return CapabilityLifecycle::Retired;
    }
    if idle < thresholds.quiet_after {
        CapabilityLifecycle::Active
    } else if idle < thresholds.stale_after {
        CapabilityLifecycle::Quiet
    } else {
        CapabilityLifecycle::Stale
    }
}

/// Derive the lifecycle from a usage record and how long the capability has been
/// catalogued.
///
/// The idle clock is the time since the last successful run; a capability that
/// has *never* succeeded is aged by how long it has been catalogued instead —
/// "catalogued three months ago and never once used" is exactly the stale case,
/// while "catalogued this morning" is legitimately still active.
pub fn from_usage(
    usage: &UsageStats,
    catalog_age: Duration,
    archived: bool,
    thresholds: &LifecycleThresholds,
) -> CapabilityLifecycle {
    let idle = usage.last_success_age.unwrap_or(catalog_age);
    derive(idle, archived, thresholds)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn days(n: u64) -> Duration {
        Duration::from_secs(n * DAY)
    }

    #[test]
    fn recent_use_is_active_and_long_idle_is_stale() {
        let t = LifecycleThresholds::default();
        assert_eq!(derive(days(1), false, &t), CapabilityLifecycle::Active);
        assert_eq!(derive(days(45), false, &t), CapabilityLifecycle::Quiet);
        assert_eq!(derive(days(120), false, &t), CapabilityLifecycle::Stale);
    }

    #[test]
    fn archiving_wins_over_any_idle_time() {
        let t = LifecycleThresholds::default();
        // Even a capability used one second ago reads as retired once archived.
        assert_eq!(derive(days(0), true, &t), CapabilityLifecycle::Retired);
    }

    #[test]
    fn a_never_used_capability_is_aged_by_how_long_it_has_been_catalogued() {
        let t = LifecycleThresholds::default();
        let never = UsageStats::default(); // last_success_age = None
        assert_eq!(
            from_usage(&never, days(2), false, &t),
            CapabilityLifecycle::Active,
            "catalogued two days ago and unused is still active"
        );
        assert_eq!(
            from_usage(&never, days(200), false, &t),
            CapabilityLifecycle::Stale,
            "catalogued long ago and never once used is stale"
        );
    }

    #[test]
    fn only_stale_wants_review() {
        assert!(CapabilityLifecycle::Stale.wants_review());
        assert!(!CapabilityLifecycle::Quiet.wants_review());
        assert!(!CapabilityLifecycle::Active.wants_review());
        assert!(!CapabilityLifecycle::Retired.wants_review());
    }
}
