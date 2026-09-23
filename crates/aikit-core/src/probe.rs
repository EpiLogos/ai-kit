//! The shared probe vocabulary and budget.
//!
//! An acceptance campaign found the hang class: a live invocation of an
//! external harness binary can **silently hang** — the real case was gemini
//! 0.29.5 with an expired OAuth session, which produced no output for 30–90
//! seconds while every caller waited on it. The discipline that closes the
//! class has three parts, and this module owns the first and second:
//!
//! 1. every probe-shaped spawn is bounded by a hard budget (the budget lives
//!    here; `SystemRunner::probe` applies it);
//! 2. every bounded outcome is named with one shared vocabulary — the six
//!    words in [`ProbeOutcome`] — never an empty string, never silence;
//! 3. a surface that would perform a model call checks credential *presence*
//!    first and short-circuits to `credential-gated`, naming what is missing.
//!
//! The vocabulary is deliberately small and closed. Surfaces render it
//! verbatim (`aikit client status` carries it per row); tests assert on it.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The default hard budget for one probe-shaped spawn. Chosen to sit well
/// inside a human's patience and well outside every healthy `--version`,
/// detect, or descriptor read; a harness that needs longer than this to
/// answer a trivial question is exactly the hang the vocabulary exists to
/// name.
pub const DEFAULT_PROBE_BUDGET: Duration = Duration::from_secs(10);

/// Environment override for the probe budget, in whole seconds. An explicit
/// per-call budget always wins over this; this wins over the default.
pub const PROBE_BUDGET_VAR: &str = "AIKIT_PROBE_BUDGET_SECS";

/// The shared outcome vocabulary for a probe-shaped spawn or credential
/// pre-check. Exactly six words; every field explains itself.
///
/// The wire form tags on `outcome` with the kebab-case word, so a JSON
/// consumer reads `{"outcome": "timed-out", "bound_secs": 10}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum ProbeOutcome {
    /// The probe ran and answered.
    Ok,
    /// A required credential is absent. `missing` names what is missing and
    /// how to repair it — never a secret value, only presence facts.
    CredentialGated { missing: String },
    /// The target could not be reached at all: the binary is missing from
    /// PATH or the spawn failed.
    Unreachable { reason: String },
    /// The probe cannot answer for this target: no credential facts, no
    /// probe seam, no model path — an honest "cannot say", never a guess.
    Unsupported { reason: String },
    /// The target did not answer within its budget and was killed.
    /// `bound_secs` is the budget it violated.
    TimedOut { bound_secs: u64 },
    /// The target ran and refused: it exited non-zero. `detail` carries the
    /// refusal text (stderr first, then stdout), trimmed.
    Refused { detail: String },
}

impl ProbeOutcome {
    /// The vocabulary word for this outcome, rendered verbatim on surfaces.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::CredentialGated { .. } => "credential-gated",
            Self::Unreachable { .. } => "unreachable",
            Self::Unsupported { .. } => "unsupported",
            Self::TimedOut { .. } => "timed-out",
            Self::Refused { .. } => "refused",
        }
    }

    /// The human explanation riding the outcome, when one exists.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Ok => None,
            Self::CredentialGated { missing } => Some(missing),
            Self::Unreachable { reason } => Some(reason),
            Self::Unsupported { reason } => Some(reason),
            Self::TimedOut { .. } => None,
            Self::Refused { detail } => Some(detail),
        }
    }

    /// The budget bound, present only on the timed-out outcome.
    pub fn bound_secs(&self) -> Option<u64> {
        match self {
            Self::TimedOut { bound_secs } => Some(*bound_secs),
            _ => None,
        }
    }

    /// Whether the probe answered affirmatively. Every other word is a
    /// finding, and findings are data — never errors to be swallowed.
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }
}

impl fmt::Display for ProbeOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut { bound_secs } => write!(f, "timed-out (bound {bound_secs}s)"),
            other => write!(f, "{}", other.as_str()),
        }
    }
}

/// The effective probe budget: the `AIKIT_PROBE_BUDGET_SECS` override when it
/// parses to at least one second, otherwise [`DEFAULT_PROBE_BUDGET`]. An
/// unparseable or zero value falls back to the default rather than disabling
/// the budget — probe discipline has no off switch.
pub fn probe_budget() -> Duration {
    let secs = std::env::var(PROBE_BUDGET_VAR)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|secs| *secs >= 1);
    secs.map(Duration::from_secs)
        .unwrap_or(DEFAULT_PROBE_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vocabulary_is_exactly_six_words() {
        assert_eq!(ProbeOutcome::Ok.as_str(), "ok");
        assert_eq!(
            ProbeOutcome::CredentialGated {
                missing: "x".into()
            }
            .as_str(),
            "credential-gated"
        );
        assert_eq!(
            ProbeOutcome::Unreachable { reason: "x".into() }.as_str(),
            "unreachable"
        );
        assert_eq!(
            ProbeOutcome::Unsupported { reason: "x".into() }.as_str(),
            "unsupported"
        );
        assert_eq!(
            ProbeOutcome::TimedOut { bound_secs: 10 }.as_str(),
            "timed-out"
        );
        assert_eq!(
            ProbeOutcome::Refused { detail: "x".into() }.as_str(),
            "refused"
        );
    }

    #[test]
    fn the_wire_form_tags_on_outcome_and_carries_the_bound() {
        let value = serde_json::to_value(ProbeOutcome::TimedOut { bound_secs: 7 }).unwrap();
        assert_eq!(value["outcome"], "timed-out");
        assert_eq!(value["bound_secs"], 7);
        let back: ProbeOutcome = serde_json::from_value(value).unwrap();
        assert_eq!(back, ProbeOutcome::TimedOut { bound_secs: 7 });
    }

    #[test]
    fn the_budget_defaults_to_ten_seconds_when_nothing_is_set() {
        // The test harness does not set the override; and even a junk value
        // must fall back to the default rather than disable the budget.
        assert_eq!(probe_budget(), DEFAULT_PROBE_BUDGET);
        assert_eq!(DEFAULT_PROBE_BUDGET, Duration::from_secs(10));
    }

    #[test]
    fn the_display_names_the_bound_on_a_timeout() {
        assert_eq!(
            ProbeOutcome::TimedOut { bound_secs: 10 }.to_string(),
            "timed-out (bound 10s)"
        );
    }
}
