//! Trust.
//!
//! Trust attaches to a *revision*, never to an id and never to a self-declaration
//! inside a manifest. Updating a trusted capsule therefore yields the same id, a
//! new revision, and a state that requires review again.
//!
//! Being catalogued is not being reviewed. That distinction is the whole point.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{err, AikitError, Result};
use crate::id::{CapsuleId, RegistrySource, Revision};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrustState {
    /// Catalogued, never looked at.
    Unseen,
    /// Held back — usually because capture found a possible secret.
    Quarantined,
    /// A human has read this revision.
    Reviewed,
    /// Reviewed and explicitly promoted; may be activated without further prompting.
    Trusted,
    /// Explicitly refused.
    Blocked,
    /// A newer revision has been reviewed; this one is retained for audit only.
    Superseded,
}

impl Default for TrustState {
    fn default() -> Self {
        Self::Unseen
    }
}

impl TrustState {
    pub fn as_str(self) -> &'static str {
        match self {
            TrustState::Unseen => "unseen",
            TrustState::Quarantined => "quarantined",
            TrustState::Reviewed => "reviewed",
            TrustState::Trusted => "trusted",
            TrustState::Blocked => "blocked",
            TrustState::Superseded => "superseded",
        }
    }

    /// May a capsule in this state be projected into a client at all?
    pub fn may_project(self) -> bool {
        matches!(self, TrustState::Reviewed | TrustState::Trusted)
    }

    /// May an executable payload in this state run without a one-shot confirmation?
    pub fn may_run_unattended(self) -> bool {
        matches!(self, TrustState::Trusted | TrustState::Reviewed)
    }

    /// Quarantine is a hard stop; it is never merely "unreviewed".
    pub fn is_withheld(self) -> bool {
        matches!(self, TrustState::Quarantined | TrustState::Blocked)
    }
}

impl fmt::Display for TrustState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TrustState {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "unseen" => TrustState::Unseen,
            "quarantined" => TrustState::Quarantined,
            "reviewed" => TrustState::Reviewed,
            "trusted" => TrustState::Trusted,
            "blocked" => TrustState::Blocked,
            "superseded" => TrustState::Superseded,
            other => return err("trust.unknown_state", format!("`{other}` is not a trust state")),
        })
    }
}

/// The full trust key. All three parts matter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TrustKey {
    pub source: RegistrySource,
    pub capsule: CapsuleId,
    pub revision: Revision,
}

impl TrustKey {
    pub fn new(source: RegistrySource, capsule: CapsuleId, revision: Revision) -> Self {
        Self {
            source,
            capsule,
            revision,
        }
    }
}

/// Where trust decisions are read from.
pub trait TrustOracle {
    fn state(&self, key: &TrustKey) -> TrustState;

    /// Convenience: an unstamped capsule (no source/revision yet) is unseen.
    fn state_for(
        &self,
        source: Option<&RegistrySource>,
        capsule: &CapsuleId,
        revision: Option<&Revision>,
    ) -> TrustState {
        match (source, revision) {
            (Some(s), Some(r)) => self.state(&TrustKey::new(s.clone(), capsule.clone(), r.clone())),
            _ => TrustState::Unseen,
        }
    }
}

/// An in-memory oracle. The store provides the persistent one.
#[derive(Debug, Clone, Default)]
pub struct MemoryTrust {
    entries: BTreeMap<TrustKey, TrustState>,
}

impl MemoryTrust {
    pub fn set(
        &mut self,
        source: RegistrySource,
        capsule: CapsuleId,
        revision: Revision,
        state: TrustState,
    ) {
        self.entries
            .insert(TrustKey::new(source, capsule, revision), state);
    }

    pub fn entries(&self) -> &BTreeMap<TrustKey, TrustState> {
        &self.entries
    }
}

impl TrustOracle for MemoryTrust {
    fn state(&self, key: &TrustKey) -> TrustState {
        self.entries.get(key).copied().unwrap_or_default()
    }
}

/// An oracle that trusts everything. Only for tests and `--no-trust-checks` tooling.
#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysTrusted;

impl TrustOracle for AlwaysTrusted {
    fn state(&self, _key: &TrustKey) -> TrustState {
        TrustState::Trusted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(rev: &str) -> TrustKey {
        TrustKey::new(
            RegistrySource::personal(),
            CapsuleId::parse("hook/gate/thing").unwrap(),
            Revision::from_raw(rev),
        )
    }

    #[test]
    fn an_unrecorded_capsule_is_unseen_rather_than_trusted() {
        let trust = MemoryTrust::default();
        assert_eq!(trust.state(&key("aaa")), TrustState::Unseen);
        assert!(!trust.state(&key("aaa")).may_project());
    }

    #[test]
    fn trust_does_not_carry_across_a_revision_change() {
        let mut trust = MemoryTrust::default();
        trust.set(
            RegistrySource::personal(),
            CapsuleId::parse("hook/gate/thing").unwrap(),
            Revision::from_raw("aaa"),
            TrustState::Trusted,
        );
        assert_eq!(trust.state(&key("aaa")), TrustState::Trusted);
        assert_eq!(
            trust.state(&key("bbb")),
            TrustState::Unseen,
            "editing a capsule must drop it back to review"
        );
    }

    #[test]
    fn trust_does_not_carry_across_a_registry_source_change() {
        let mut trust = MemoryTrust::default();
        trust.set(
            RegistrySource::personal(),
            CapsuleId::parse("hook/gate/thing").unwrap(),
            Revision::from_raw("aaa"),
            TrustState::Trusted,
        );
        let project_key = TrustKey::new(
            RegistrySource::project_local(),
            CapsuleId::parse("hook/gate/thing").unwrap(),
            Revision::from_raw("aaa"),
        );
        assert_eq!(trust.state(&project_key), TrustState::Unseen);
    }

    #[test]
    fn quarantine_is_a_hard_stop_and_not_merely_unreviewed() {
        assert!(TrustState::Quarantined.is_withheld());
        assert!(!TrustState::Quarantined.may_project());
        assert!(!TrustState::Unseen.is_withheld());
    }

    #[test]
    fn trust_states_round_trip_through_their_names() {
        for state in [
            TrustState::Unseen,
            TrustState::Quarantined,
            TrustState::Reviewed,
            TrustState::Trusted,
            TrustState::Blocked,
            TrustState::Superseded,
        ] {
            assert_eq!(state.as_str().parse::<TrustState>().unwrap(), state);
        }
    }
}
