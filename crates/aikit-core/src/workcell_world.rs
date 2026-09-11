//! Application-boundary read model for Workcell (body materialisation).
//!
//! Workcell is an external owner with its own binary; AIKit only observes it in
//! its native terms. `aikit-adapters` already has the observer —
//! `intake_workcell_instances` runs `workcell instances list --json` and
//! returns `Records | Unavailable` — but nothing consumed it. This module is the
//! I/O-free read model a surface renders, built the same way `credential_world`
//! and `doctor_world` are.
//!
//! Three absences stay apart, the discipline this whole family shares:
//! `NotAttempted` is "no producer looked" (the default a reading carries);
//! `Unavailable` is "the `workcell` binary could not be read" (the adapter's own
//! `Unavailable` reason, carried through); and `Observed` with an empty
//! `instances` list is a confirmed "the registry exists and holds nothing".

use serde::{Deserialize, Serialize};

pub const WORKCELL_WORLD_VERSION: &str = "aikit.workcell-world/v1";

/// One Workcell instance, projected to the facts a disclosure carries. The full
/// `workcell.harness-instance/v1` record (pids, executable digest, seams) stays
/// in the adapter; only identity and observed liveness cross this boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkcellInstanceDisclosure {
    pub instance_ref: String,
    pub harness_ref: String,
    /// Evidence was observed, not merely declared (a live pid or a
    /// gateway-confirmed instance), as opposed to `declared-unverified`.
    pub detected: bool,
    /// Live at the last scan, as opposed to `stale` (missed scans, kept in the
    /// registry rather than silently deleted).
    pub live: bool,
}

/// Whether Workcell was observed at all, and if so, what its registry held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum WorkcellKnowledge {
    /// No producer looked — the reading's honest default, not "no Workcell".
    NotAttempted { reason: String },
    /// The `workcell` binary could not be read (absent, errored, or its output
    /// unparsable). Carries the observer's own reason; distinct from an empty
    /// registry.
    Unavailable { reason: String },
    /// The registry was read. An empty `instances` list is a confirmed "nothing
    /// registered here", not a stand-in for missing data.
    Observed {
        instances: Vec<WorkcellInstanceDisclosure>,
    },
}

/// Application-boundary read model answering "is Workcell present here, and what
/// has it materialised".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkcellDisclosure {
    pub version: String,
    pub knowledge: WorkcellKnowledge,
}

impl WorkcellDisclosure {
    /// The honest default: no producer looked. Never "Workcell is absent".
    pub fn not_attempted(reason: impl Into<String>) -> Self {
        Self {
            version: WORKCELL_WORLD_VERSION.to_string(),
            knowledge: WorkcellKnowledge::NotAttempted {
                reason: reason.into(),
            },
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            version: WORKCELL_WORLD_VERSION.to_string(),
            knowledge: WorkcellKnowledge::Unavailable {
                reason: reason.into(),
            },
        }
    }

    pub fn observed(instances: Vec<WorkcellInstanceDisclosure>) -> Self {
        Self {
            version: WORKCELL_WORLD_VERSION.to_string(),
            knowledge: WorkcellKnowledge::Observed { instances },
        }
    }

    /// `Some` only when the registry was actually read (possibly empty). `None`
    /// covers both "nobody looked" and "could not read" — a caller must not read
    /// either as an empty registry.
    pub fn instances(&self) -> Option<&[WorkcellInstanceDisclosure]> {
        match &self.knowledge {
            WorkcellKnowledge::Observed { instances } => Some(instances),
            _ => None,
        }
    }

    pub fn was_observed(&self) -> bool {
        matches!(self.knowledge, WorkcellKnowledge::Observed { .. })
    }

    /// Count of instances backed by observed evidence. `None` when the registry
    /// was not read.
    pub fn detected_count(&self) -> Option<usize> {
        self.instances()
            .map(|instances| instances.iter().filter(|i| i.detected).count())
    }
}

impl Default for WorkcellDisclosure {
    fn default() -> Self {
        Self::not_attempted("Workcell was not observed for this reading")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(id: &str, detected: bool, live: bool) -> WorkcellInstanceDisclosure {
        WorkcellInstanceDisclosure {
            instance_ref: id.into(),
            harness_ref: format!("harness:{id}"),
            detected,
            live,
        }
    }

    #[test]
    fn not_attempted_is_not_an_empty_registry() {
        let disclosure = WorkcellDisclosure::default();
        assert!(!disclosure.was_observed());
        assert_eq!(disclosure.instances(), None);
        assert_eq!(disclosure.detected_count(), None);
    }

    #[test]
    fn unavailable_is_not_an_empty_registry() {
        let disclosure = WorkcellDisclosure::unavailable("could not run workcell");
        assert!(!disclosure.was_observed());
        assert_eq!(disclosure.instances(), None);
    }

    #[test]
    fn observed_empty_is_a_confirmed_empty_registry() {
        let disclosure = WorkcellDisclosure::observed(Vec::new());
        assert!(disclosure.was_observed());
        assert_eq!(disclosure.instances(), Some(&[][..]));
        assert_eq!(disclosure.detected_count(), Some(0));
    }

    #[test]
    fn detected_count_ignores_declared_but_unverified_instances() {
        let disclosure = WorkcellDisclosure::observed(
            vec![instance("a", true, true), instance("b", false, false)],
        );
        assert_eq!(disclosure.detected_count(), Some(1));
    }

    #[test]
    fn disclosure_round_trips_through_json() {
        let disclosure = WorkcellDisclosure::observed(
            vec![instance("a", true, false)],
        );
        let json = serde_json::to_string(&disclosure).unwrap();
        let back: WorkcellDisclosure = serde_json::from_str(&json).unwrap();
        assert_eq!(disclosure, back);
    }
}
