//! Application-boundary read model for installation health ("doctor").
//!
//! `aikit-cli` owns the health checks (`doctor::run`) and the fixes
//! (`doctor::plan_fixes`), because running them is I/O — it probes the OS
//! secure store, reads harness config, asks a gateway socket, lists registries.
//! But the *findings* are just facts, and a surface that wants to disclose
//! "what does this installation depend on, and is any of it broken" needs a
//! typed, owned, round-trippable projection of them that does not drag the
//! Procedure engine or the CLI into `aikit-core`.
//!
//! This module is that projection, built the same way `credential_world.rs` is:
//! a read model over an already-produced result, keeping absence honest.
//! `DoctorKnowledge::NotAttempted` is "nobody ran the checks" — the default a
//! reading carries when no producer is attached. `DoctorKnowledge::Observed`
//! with an empty `findings` list is a real, confirmed "the checks ran and found
//! nothing to report", which is a different fact and must never be collapsed
//! into the first.
//!
//! The disclosure carries whether a finding is *fixable*, never the fix itself:
//! a `Fix`/`Procedure` is a CLI-owned mutation, and a read model has no business
//! holding one. A surface shows that a repair exists; applying it stays with the
//! Procedure engine.

use serde::{Deserialize, Serialize};

pub const DOCTOR_WORLD_VERSION: &str = "aikit.doctor-world/v1";

/// How much a finding matters. Mirrors the CLI's own `Severity`, owned here so
/// the read model does not depend on the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DoctorSeverity {
    /// Something is broken now.
    Error,
    /// Something will bite later.
    Warning,
    /// Worth knowing; nothing is wrong.
    Note,
}

impl DoctorSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            DoctorSeverity::Error => "error",
            DoctorSeverity::Warning => "warning",
            DoctorSeverity::Note => "note",
        }
    }
}

/// One thing the health checks noticed, projected as owned data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorFinding {
    /// The check that produced it (e.g. `gateway.service`, `home.layout`).
    pub check: String,
    pub severity: DoctorSeverity,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Whether an automatic repair exists for this finding. The repair itself
    /// is a CLI-owned Procedure and never crosses this boundary; a surface
    /// shows only that one is available.
    pub fixable: bool,
}

/// Whether the health checks were run at all for this reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum DoctorKnowledge {
    /// The checks were not run — no producer is attached. This is not "healthy";
    /// it is "unknown", and a surface must render it as such.
    NotAttempted { reason: String },
    /// The checks ran. An empty `findings` list is a confirmed clean bill, not
    /// a stand-in for missing data.
    Observed { findings: Vec<DoctorFinding> },
}

/// Application-boundary read model answering "is this installation healthy, and
/// what does it depend on" with typed values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorDisclosure {
    pub version: String,
    pub knowledge: DoctorKnowledge,
}

impl DoctorDisclosure {
    /// An honest "the checks were not run" reading — the default when no
    /// producer is attached. Never confused with a clean bill of health.
    pub fn not_attempted(reason: impl Into<String>) -> Self {
        Self {
            version: DOCTOR_WORLD_VERSION.to_string(),
            knowledge: DoctorKnowledge::NotAttempted {
                reason: reason.into(),
            },
        }
    }

    /// A reading composed from findings the checks actually produced.
    pub fn observed(findings: Vec<DoctorFinding>) -> Self {
        Self {
            version: DOCTOR_WORLD_VERSION.to_string(),
            knowledge: DoctorKnowledge::Observed { findings },
        }
    }

    /// `Some` only when the checks were actually run (findings possibly empty).
    /// `None` means they were not — a caller must not read that as healthy.
    pub fn findings(&self) -> Option<&[DoctorFinding]> {
        match &self.knowledge {
            DoctorKnowledge::Observed { findings } => Some(findings),
            DoctorKnowledge::NotAttempted { .. } => None,
        }
    }

    pub fn was_attempted(&self) -> bool {
        matches!(self.knowledge, DoctorKnowledge::Observed { .. })
    }

    /// Count of findings at a given severity. `None` when the checks were not
    /// run — zero errors "observed" and "not looked for" are different facts.
    pub fn count(&self, severity: DoctorSeverity) -> Option<usize> {
        self.findings()
            .map(|findings| findings.iter().filter(|f| f.severity == severity).count())
    }
}

impl Default for DoctorDisclosure {
    fn default() -> Self {
        Self::not_attempted("no health checks have been run for this reading yet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(check: &str, severity: DoctorSeverity) -> DoctorFinding {
        DoctorFinding {
            check: check.into(),
            severity,
            summary: format!("{check} summary"),
            detail: None,
            fixable: false,
        }
    }

    #[test]
    fn not_attempted_is_not_a_clean_bill_of_health() {
        let disclosure = DoctorDisclosure::not_attempted("no producer attached");
        assert!(!disclosure.was_attempted());
        assert_eq!(disclosure.findings(), None);
        // The distinction the module exists to keep: "no errors" is unknown,
        // not zero.
        assert_eq!(disclosure.count(DoctorSeverity::Error), None);
    }

    #[test]
    fn observed_empty_is_a_confirmed_clean_bill() {
        let disclosure = DoctorDisclosure::observed(Vec::new());
        assert!(disclosure.was_attempted());
        assert_eq!(disclosure.findings(), Some(&[][..]));
        assert_eq!(disclosure.count(DoctorSeverity::Error), Some(0));
    }

    #[test]
    fn counts_are_per_severity() {
        let disclosure = DoctorDisclosure::observed(vec![
            finding("a", DoctorSeverity::Error),
            finding("b", DoctorSeverity::Warning),
            finding("c", DoctorSeverity::Warning),
            finding("d", DoctorSeverity::Note),
        ]);
        assert_eq!(disclosure.count(DoctorSeverity::Error), Some(1));
        assert_eq!(disclosure.count(DoctorSeverity::Warning), Some(2));
        assert_eq!(disclosure.count(DoctorSeverity::Note), Some(1));
    }

    #[test]
    fn severity_orders_error_before_warning_before_note() {
        assert!(DoctorSeverity::Error < DoctorSeverity::Warning);
        assert!(DoctorSeverity::Warning < DoctorSeverity::Note);
    }

    #[test]
    fn disclosure_round_trips_through_json() {
        let disclosure = DoctorDisclosure::observed(vec![DoctorFinding {
            check: "gateway.service".into(),
            severity: DoctorSeverity::Note,
            summary: "no agency gateway is running at the default endpoint".into(),
            detail: Some("optional; start one with `aikit gateway serve`".into()),
            fixable: false,
        }]);
        let json = serde_json::to_string(&disclosure).unwrap();
        let back: DoctorDisclosure = serde_json::from_str(&json).unwrap();
        assert_eq!(disclosure, back);
    }

    #[test]
    fn not_attempted_round_trips_through_json() {
        let disclosure = DoctorDisclosure::default();
        let json = serde_json::to_string(&disclosure).unwrap();
        let back: DoctorDisclosure = serde_json::from_str(&json).unwrap();
        assert_eq!(disclosure, back);
        assert!(!back.was_attempted());
    }
}
