//! The resolution hash.
//!
//! A generation is content-addressed by this value, so it must be stable across
//! anything the user would call semantically irrelevant — declaration order,
//! which file a layer came from, how many redundant profile references there were
//! — and must move for anything that changes what the context actually gets.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::context::ContextDescriptor;
use crate::id::CapsuleId;
use crate::policy::ManagedPolicy;
use crate::profile::canonical_config;

use super::{ActiveCapability, AppliedSkillUsageOverlay};

/// Bumped whenever the canonical encoding below changes, so that old generations
/// are recognised as stale rather than silently reused.
const HASH_DOMAIN: &str = "aikit-resolution-v3";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResolutionHash(String);

impl ResolutionHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn short(&self) -> &str {
        &self.0[..self.0.len().min(12)]
    }

    pub fn generation_id(&self) -> crate::id::GenerationId {
        // A `ResolutionHash` is only ever constructed from `blake3::hash(...)`
        // rendered as 64 hex chars, so a 16-char prefix always exists and is
        // always a valid generation id. `char_indices` keeps that true even if
        // the length invariant were ever loosened, rather than panicking.
        let end = self
            .0
            .char_indices()
            .nth(16)
            .map_or(self.0.len(), |(b, _)| b);
        crate::id::GenerationId::parse(&format!("gen_{}", &self.0[..end]))
            .expect("a hex hash prefix is always a valid generation id")
    }
}

impl fmt::Display for ResolutionHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Build the canonical byte string, then hash it.
///
/// Deliberately excluded: context id, session id, selection/config layer origins,
/// warnings, and the order anything was declared in. Skill-overlay provenance is
/// included because it is rendered into the Effective Skill. Also included: the
/// platform, target list, isolation mode (it changes which projections are
/// possible), policy digest, and every active capsule's revision and effective
/// config.
pub fn resolution_hash(
    context: &ContextDescriptor,
    policy: &ManagedPolicy,
    active: &std::collections::BTreeMap<CapsuleId, ActiveCapability>,
    skill_usage_overlays: &std::collections::BTreeMap<CapsuleId, Vec<AppliedSkillUsageOverlay>>,
) -> ResolutionHash {
    let mut canonical = String::with_capacity(256 + active.len() * 128);
    canonical.push_str(HASH_DOMAIN);
    canonical.push('\n');
    canonical.push_str("platform=");
    canonical.push_str(context.platform.as_str());
    canonical.push('\n');

    let mut targets: Vec<&str> = context.targets.iter().map(|t| t.as_str()).collect();
    targets.sort_unstable();
    targets.dedup();
    canonical.push_str("targets=");
    canonical.push_str(&targets.join(","));
    canonical.push('\n');

    canonical.push_str("isolation=");
    canonical.push_str(context.isolation.as_str());
    canonical.push('\n');

    canonical.push_str("policy=");
    canonical.push_str(&policy.digest());
    canonical.push('\n');

    // `active` is a BTreeMap, so this walk is already in canonical id order.
    for (id, capability) in active {
        canonical.push_str("capsule=");
        canonical.push_str(&id.to_string());
        canonical.push('@');
        canonical.push_str(
            capability
                .revision
                .as_ref()
                .map(|r| r.as_str())
                .unwrap_or("unstamped"),
        );
        canonical.push('|');
        let mut caps_targets: Vec<&str> = capability.targets.iter().map(|t| t.as_str()).collect();
        caps_targets.sort_unstable();
        canonical.push_str(&caps_targets.join(","));
        canonical.push('|');
        canonical.push_str(&canonical_config(&capability.config));
        canonical.push('\n');
    }

    for (id, overlays) in skill_usage_overlays {
        for overlay in overlays {
            canonical.push_str("skill-overlay=");
            canonical.push_str(&id.to_string());
            canonical.push('|');
            canonical.push_str(
                &serde_json::to_string(overlay)
                    .expect("a resolved skill overlay always serializes"),
            );
            canonical.push('\n');
        }
    }

    ResolutionHash(blake3::hash(canonical.as_bytes()).to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Isolation;
    use std::collections::BTreeMap;

    fn ctx() -> ContextDescriptor {
        ContextDescriptor::for_project("/work/payments")
    }

    #[test]
    fn an_empty_view_still_hashes_to_something_stable() {
        let a = resolution_hash(
            &ctx(),
            &ManagedPolicy::default(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let b = resolution_hash(
            &ctx(),
            &ManagedPolicy::default(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(a, b);
        assert!(!a.as_str().is_empty());
    }

    #[test]
    fn the_context_id_does_not_affect_the_hash() {
        let a = resolution_hash(
            &ctx(),
            &ManagedPolicy::default(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let b = resolution_hash(
            &ctx(),
            &ManagedPolicy::default(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        // Two `for_project` calls generate different context ids.
        assert_eq!(a, b);
    }

    #[test]
    fn isolation_changes_the_hash_because_it_changes_projection() {
        let mut isolated = ctx();
        isolated.isolation = Isolation::Worktree;
        assert_ne!(
            resolution_hash(
                &ctx(),
                &ManagedPolicy::default(),
                &BTreeMap::new(),
                &BTreeMap::new(),
            ),
            resolution_hash(
                &isolated,
                &ManagedPolicy::default(),
                &BTreeMap::new(),
                &BTreeMap::new(),
            )
        );
    }

    #[test]
    fn a_generation_id_can_be_derived_from_any_hash() {
        let h = resolution_hash(
            &ctx(),
            &ManagedPolicy::default(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        let gen = h.generation_id();
        assert!(gen.as_str().starts_with("gen_"));
        assert_eq!(gen.as_str().len(), 20);
    }
}
