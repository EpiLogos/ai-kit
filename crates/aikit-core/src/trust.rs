//! Trust.
//!
//! Trust never comes from a self-declaration inside a manifest, and it never
//! comes from mere presence in a registry. Being catalogued is not being
//! reviewed. That distinction is the whole point.
//!
//! ## The keying is deliberately asymmetric
//!
//! **Approval is keyed on content.** `(source, capsule, revision)`. Editing a
//! capsule yields the same id, a new revision, and a state that requires review
//! again — because the thing you approved is not the thing that would now run.
//!
//! **Refusal is keyed on identity.** `(source, capsule)`, revision excluded. A
//! block that a version bump clears is not a block: an unwanted capsule would
//! only have to change a byte to come back. direnv reaches the same conclusion
//! from the other direction — its allow list hashes path *and* contents, while
//! its deny list is keyed on the path alone.
//!
//! Getting this symmetric in either direction is a security bug. Symmetric on
//! content means "no" expires silently; symmetric on identity means "yes"
//! survives an edit you never saw.
//!
//! A **standing verdict** (block or dismissal) is therefore stored separately
//! from per-revision approvals, and consulted first.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{err, AikitError, Result};
use crate::id::{CapsuleId, RegistrySource, Revision};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum TrustState {
    /// Catalogued, never looked at.
    #[default]
    Unseen,
    /// The user declined to review it for now.
    ///
    /// Distinct from both `Unseen` and `Blocked`, and the distinction earns its
    /// keep: without it, every prompt is a choice between "yes" and "forever",
    /// so users learn to say yes. Dismissal stops the asking without becoming a
    /// refusal. (mise keeps `ignored` separate from `trusted` for this reason.)
    Dismissed,
    /// Held back — usually because capture found a possible secret.
    Quarantined,
    /// A human has read this revision.
    Reviewed,
    /// Reviewed and explicitly promoted; may be activated without further prompting.
    Trusted,
    /// Explicitly refused. Keyed on identity, not content — see the module header.
    Blocked,
    /// A newer revision has been reviewed; this one is retained for audit only.
    Superseded,
}


impl TrustState {
    pub fn as_str(self) -> &'static str {
        match self {
            TrustState::Unseen => "unseen",
            TrustState::Dismissed => "dismissed",
            TrustState::Quarantined => "quarantined",
            TrustState::Reviewed => "reviewed",
            TrustState::Trusted => "trusted",
            TrustState::Blocked => "blocked",
            TrustState::Superseded => "superseded",
        }
    }

    /// Should the palette stop offering this for review?
    ///
    /// True for every state where the user has already answered the question,
    /// in either direction. Only `Unseen` is an open question.
    pub fn suppresses_prompting(self) -> bool {
        !matches!(self, TrustState::Unseen)
    }

    /// Is this a standing verdict — one that must survive a content change?
    pub fn is_standing(self) -> bool {
        matches!(self, TrustState::Blocked | TrustState::Dismissed)
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
    /// The per-revision approval record. Keyed on content.
    fn state(&self, key: &TrustKey) -> TrustState;

    /// A verdict that holds for *every* revision of a capsule.
    ///
    /// Only refusals live here — `Blocked` and `Dismissed`. Implementations that
    /// have no standing ledger return `None` and get ordinary per-revision
    /// behaviour, which is safe but means a block can be cleared by an edit; a
    /// persistent oracle must override this.
    fn standing_verdict(
        &self,
        source: &RegistrySource,
        capsule: &CapsuleId,
    ) -> Option<TrustState> {
        let _ = (source, capsule);
        None
    }

    /// The effective state, consulting the standing verdict first.
    ///
    /// An unstamped capsule — one the store has not yet given a source and a
    /// revision — is `Unseen`, never trusted by default.
    fn state_for(
        &self,
        source: Option<&RegistrySource>,
        capsule: &CapsuleId,
        revision: Option<&Revision>,
    ) -> TrustState {
        let Some(source) = source else {
            return TrustState::Unseen;
        };
        if let Some(standing) = self.standing_verdict(source, capsule) {
            // A block is final. A dismissal is a "not now", so an explicit review
            // of *this* revision answers it and takes precedence.
            if standing == TrustState::Blocked {
                return standing;
            }
            let per_revision = revision
                .map(|r| self.state(&TrustKey::new(source.clone(), capsule.clone(), r.clone())))
                .unwrap_or(TrustState::Unseen);
            return if per_revision == TrustState::Unseen {
                standing
            } else {
                per_revision
            };
        }
        match revision {
            Some(r) => self.state(&TrustKey::new(source.clone(), capsule.clone(), r.clone())),
            None => TrustState::Unseen,
        }
    }
}

/// One row of the trust ledger, carrying human-readable identity alongside the key.
///
/// direnv names its grant files by hash but stores the path inside them, which is
/// the only reason `direnv status` and `direnv prune` can exist at all. A ledger
/// you cannot enumerate in human terms cannot be audited or pruned, so identity
/// travels with every entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustEntry {
    pub source: RegistrySource,
    pub capsule: CapsuleId,
    /// `None` for a standing verdict, which applies to every revision.
    pub revision: Option<Revision>,
    pub state: TrustState,
}

/// An in-memory oracle. The store provides the persistent one.
#[derive(Debug, Clone, Default)]
pub struct MemoryTrust {
    /// Per-revision approvals, keyed on content.
    entries: BTreeMap<TrustKey, TrustState>,
    /// Standing refusals, keyed on identity so an edit cannot clear them.
    standing: BTreeMap<(RegistrySource, CapsuleId), TrustState>,
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

    /// Refuse a capsule for every revision, present and future.
    pub fn block(&mut self, source: RegistrySource, capsule: CapsuleId) {
        self.standing
            .insert((source, capsule), TrustState::Blocked);
    }

    /// Stop asking about a capsule without refusing it.
    pub fn dismiss(&mut self, source: RegistrySource, capsule: CapsuleId) {
        self.standing
            .insert((source, capsule), TrustState::Dismissed);
    }

    /// Lift a standing verdict.
    ///
    /// This restores ordinary per-revision keying; it does not grant approval,
    /// because "I no longer refuse this" and "I have reviewed this" are
    /// different statements and only the user makes the second one.
    pub fn unblock(&mut self, source: &RegistrySource, capsule: &CapsuleId) {
        self.standing.remove(&(source.clone(), capsule.clone()));
    }

    /// Every recorded decision, in a form a person can read and prune.
    pub fn ledger(&self) -> Vec<TrustEntry> {
        let mut out: Vec<TrustEntry> = self
            .standing
            .iter()
            .map(|((source, capsule), state)| TrustEntry {
                source: source.clone(),
                capsule: capsule.clone(),
                revision: None,
                state: *state,
            })
            .collect();
        out.extend(self.entries.iter().map(|(key, state)| TrustEntry {
            source: key.source.clone(),
            capsule: key.capsule.clone(),
            revision: Some(key.revision.clone()),
            state: *state,
        }));
        out
    }
}

impl TrustOracle for MemoryTrust {
    fn state(&self, key: &TrustKey) -> TrustState {
        self.entries.get(key).copied().unwrap_or_default()
    }

    fn standing_verdict(
        &self,
        source: &RegistrySource,
        capsule: &CapsuleId,
    ) -> Option<TrustState> {
        self.standing
            .get(&(source.clone(), capsule.clone()))
            .copied()
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
