//! Managed policy.
//!
//! Policy is *not* a normal override layer. It sits above the whole precedence
//! chain and can impose immutable requirements or denials. A user's explicit
//! session enable must never be able to defeat an organisational denial, and a
//! user's explicit disable must never quietly remove a required security gate —
//! though they must be *told* when that happens, which is why the resolver emits
//! a warning rather than silently winning.

use serde::{Deserialize, Serialize};

use crate::capsule::{Capsule, Kind};
use crate::effects::EffectClass;
use crate::id::CapsuleId;

/// Why a capsule was refused by policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DenyReason {
    Capsule,
    Tag(String),
    Effect(EffectClass),
    Kind(Kind),
}

impl DenyReason {
    pub fn describe(&self) -> String {
        match self {
            DenyReason::Capsule => "denied by managed policy".to_string(),
            DenyReason::Tag(t) => format!("managed policy denies capsules tagged `{t}`"),
            DenyReason::Effect(e) => {
                format!("managed policy denies capsules declaring `{}`", e.as_str())
            }
            DenyReason::Kind(k) => format!("managed policy denies `{k}` capsules"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManagedPolicy {
    /// Capsules that must be active, whatever any layer says.
    pub require: Vec<CapsuleId>,
    /// Capsules that must never be active.
    pub deny: Vec<CapsuleId>,
    pub deny_tags: Vec<String>,
    pub deny_effects: Vec<EffectClass>,
    pub deny_kinds: Vec<Kind>,
    /// Where the policy came from, for the explanation and the audit log.
    pub source: String,
}

impl ManagedPolicy {
    pub fn is_empty(&self) -> bool {
        self.require.is_empty()
            && self.deny.is_empty()
            && self.deny_tags.is_empty()
            && self.deny_effects.is_empty()
            && self.deny_kinds.is_empty()
    }

    pub fn denies(&self, capsule: &Capsule) -> Option<DenyReason> {
        if self.deny.contains(&capsule.id) {
            return Some(DenyReason::Capsule);
        }
        if self.deny_kinds.contains(&capsule.kind) {
            return Some(DenyReason::Kind(capsule.kind));
        }
        for tag in &capsule.tags {
            if self.deny_tags.contains(tag) {
                return Some(DenyReason::Tag(tag.clone()));
            }
        }
        let classes = capsule.effects.classes();
        for denied in &self.deny_effects {
            if classes.contains(denied) {
                return Some(DenyReason::Effect(*denied));
            }
        }
        None
    }

    pub fn requires(&self, id: &CapsuleId) -> bool {
        self.require.contains(id)
    }

    /// A canonical digest folded into the resolution hash, so that a policy change
    /// invalidates every generation built under the old policy.
    pub fn digest(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("source={}", self.source));
        let mut require: Vec<String> = self.require.iter().map(|c| c.to_string()).collect();
        require.sort();
        parts.push(format!("require={}", require.join(",")));
        let mut deny: Vec<String> = self.deny.iter().map(|c| c.to_string()).collect();
        deny.sort();
        parts.push(format!("deny={}", deny.join(",")));
        let mut tags = self.deny_tags.clone();
        tags.sort();
        parts.push(format!("deny_tags={}", tags.join(",")));
        let mut effects: Vec<&str> = self.deny_effects.iter().map(|e| e.as_str()).collect();
        effects.sort_unstable();
        parts.push(format!("deny_effects={}", effects.join(",")));
        let mut kinds: Vec<&str> = self.deny_kinds.iter().map(|k| k.as_str()).collect();
        kinds.sort_unstable();
        parts.push(format!("deny_kinds={}", kinds.join(",")));
        parts.join("|")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capsule(extra: &str) -> Capsule {
        let src = format!(
            r#"
schema = 1
id = "script/test/thing"
kind = "script"
name = "Thing"
description = "A thing."
{extra}

[script]
entry = "payload/run.sh"
"#
        );
        Capsule::from_toml_str(&src).unwrap()
    }

    #[test]
    fn an_empty_policy_denies_nothing() {
        assert!(ManagedPolicy::default().denies(&capsule("")).is_none());
        assert!(ManagedPolicy::default().is_empty());
    }

    #[test]
    fn a_tag_denial_matches_any_declared_tag() {
        let policy = ManagedPolicy {
            deny_tags: vec!["experimental".into()],
            ..Default::default()
        };
        let c = capsule("tags = [\"rust\", \"experimental\"]");
        assert_eq!(
            policy.denies(&c),
            Some(DenyReason::Tag("experimental".into()))
        );
    }

    #[test]
    fn an_effect_denial_uses_normalized_effect_classes() {
        let policy = ManagedPolicy {
            deny_effects: vec![EffectClass::WriteOutsideProject],
            ..Default::default()
        };
        let c = capsule("[effects]\nfilesystem = [\"write:home\"]");
        assert_eq!(
            policy.denies(&c),
            Some(DenyReason::Effect(EffectClass::WriteOutsideProject))
        );
    }

    #[test]
    fn the_digest_is_insensitive_to_declaration_order() {
        let a = ManagedPolicy {
            deny: vec![
                CapsuleId::parse("script/a/one").unwrap(),
                CapsuleId::parse("script/b/two").unwrap(),
            ],
            ..Default::default()
        };
        let b = ManagedPolicy {
            deny: vec![
                CapsuleId::parse("script/b/two").unwrap(),
                CapsuleId::parse("script/a/one").unwrap(),
            ],
            ..Default::default()
        };
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn the_digest_changes_when_the_policy_changes() {
        let a = ManagedPolicy::default();
        let b = ManagedPolicy {
            deny_tags: vec!["experimental".into()],
            ..Default::default()
        };
        assert_ne!(a.digest(), b.digest());
    }
}
