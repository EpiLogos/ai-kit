//! Profiles and pool patches.
//!
//! A profile is a reusable composition recipe, not a capsule. A pool patch is the
//! set of declarations attached to one scope. Both projects and sessions carry
//! pool patches; that symmetry is what lets a session change be promoted into a
//! project without rewriting it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{err, Result};
use crate::id::{CapsuleId, GenerationId, ProfileId, SessionId};

/// Free-form per-capsule configuration, carried through resolution untouched.
pub type ConfigTable = toml::value::Table;

/// The `enable` / `disable` / `profiles` / `[config.*]` declarations of one scope.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PoolPatch {
    #[serde(default)]
    pub profiles: Vec<ProfileId>,
    #[serde(default)]
    pub enable: Vec<CapsuleId>,
    #[serde(default)]
    pub disable: Vec<CapsuleId>,
    #[serde(default)]
    pub config: BTreeMap<CapsuleId, ConfigTable>,
}

impl PoolPatch {
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
            && self.enable.is_empty()
            && self.disable.is_empty()
            && self.config.is_empty()
    }

    /// Declaring the same capsule in both lists is a contradiction rather than a
    /// silent precedence puzzle.
    pub fn validate(&self) -> Result<()> {
        for id in &self.enable {
            if self.disable.contains(id) {
                return err(
                    "patch.contradiction",
                    format!("`{id}` is both enabled and disabled in the same scope"),
                );
            }
        }
        Ok(())
    }

    /// Toggle a capsule in place, keeping the declaration lists canonical.
    pub fn set(&mut self, id: &CapsuleId, enabled: bool) {
        self.enable.retain(|c| c != id);
        self.disable.retain(|c| c != id);
        if enabled {
            self.enable.push(id.clone());
            self.enable.sort();
        } else {
            self.disable.push(id.clone());
            self.disable.sort();
        }
    }

    /// Remove any declaration for a capsule, letting lower scopes decide again.
    pub fn clear(&mut self, id: &CapsuleId) {
        self.enable.retain(|c| c != id);
        self.disable.retain(|c| c != id);
        self.config.remove(id);
    }
}

/// A reusable declarative patch identified by a profile id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: ProfileId,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub extends: Vec<ProfileId>,
    #[serde(flatten)]
    pub patch: PoolPatch,
}

/// The on-disk form of a profile: `~/.aikit/profiles/<group>/<name>.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileFile {
    pub schema: u32,
    pub id: ProfileId,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub extends: Vec<ProfileId>,
    #[serde(flatten)]
    pub patch: PoolPatch,
}

impl ProfileFile {
    pub fn into_profile(self) -> Result<Profile> {
        if self.schema != 1 {
            return err(
                "profile.unsupported_schema",
                format!("profile schema {} is not supported", self.schema),
            );
        }
        self.patch.validate()?;
        Ok(Profile {
            id: self.id,
            description: self.description,
            extends: self.extends,
            patch: self.patch,
        })
    }
}

/// `<repo>/.aikit/profile.toml` and `<repo>/.aikit/profile.local.toml`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectProfileFile {
    #[serde(default = "one")]
    pub schema: u32,
    #[serde(flatten)]
    pub patch: PoolPatch,
}

/// `~/.aikit/state/sessions/<session-id>/overlay.toml`.
///
/// `base_generation` is what makes concurrent edits from two panes safe: an apply
/// whose base is stale is detected rather than silently clobbering the other pane.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SessionOverlayFile {
    #[serde(default = "one")]
    pub schema: u32,
    pub session_id: SessionId,
    #[serde(default)]
    pub base_generation: Option<GenerationId>,
    #[serde(flatten)]
    pub patch: PoolPatch,
}

fn one() -> u32 {
    1
}

/// Recursively merge `overlay` into `base`, key by key.
///
/// Higher scopes win on scalars; nested tables merge rather than replace, so a
/// session can change one hook option without restating the project's whole table.
pub fn merge_config(base: &mut ConfigTable, overlay: &ConfigTable) {
    for (key, value) in overlay {
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(existing)), toml::Value::Table(incoming)) => {
                merge_config(existing, incoming);
            }
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Deterministic textual form of a config table, used for hashing.
///
/// Written by hand rather than via a serializer because the hash is a stable
/// public artefact: it must not move when a dependency changes its key ordering.
pub fn canonical_config(table: &ConfigTable) -> String {
    fn write_value(out: &mut String, value: &toml::Value) {
        match value {
            toml::Value::String(s) => {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
            toml::Value::Integer(i) => out.push_str(&i.to_string()),
            toml::Value::Float(f) => out.push_str(&format!("{f:?}")),
            toml::Value::Boolean(b) => out.push_str(if *b { "true" } else { "false" }),
            toml::Value::Datetime(d) => out.push_str(&d.to_string()),
            toml::Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_value(out, item);
                }
                out.push(']');
            }
            toml::Value::Table(t) => {
                out.push('{');
                let mut keys: Vec<&String> = t.keys().collect();
                keys.sort();
                for (i, key) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(key);
                    out.push('=');
                    write_value(out, &t[*key]);
                }
                out.push('}');
            }
        }
    }

    let mut out = String::new();
    let mut keys: Vec<&String> = table.keys().collect();
    keys.sort();
    for key in keys {
        out.push_str(key);
        out.push('=');
        write_value(&mut out, &table[key]);
        out.push(';');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(pairs: &[(&str, toml::Value)]) -> ConfigTable {
        let mut t = ConfigTable::new();
        for (k, v) in pairs {
            t.insert(k.to_string(), v.clone());
        }
        t
    }

    #[test]
    fn a_patch_that_both_enables_and_disables_the_same_capsule_is_rejected() {
        let patch = PoolPatch {
            enable: vec![CapsuleId::parse("script/test/a").unwrap()],
            disable: vec![CapsuleId::parse("script/test/a").unwrap()],
            ..Default::default()
        };
        assert_eq!(patch.validate().unwrap_err().code(), "patch.contradiction");
    }

    #[test]
    fn toggling_moves_a_capsule_between_the_lists_without_duplicating_it() {
        let id = CapsuleId::parse("script/test/a").unwrap();
        let mut patch = PoolPatch::default();
        patch.set(&id, true);
        patch.set(&id, true);
        assert_eq!(patch.enable, vec![id.clone()]);
        patch.set(&id, false);
        assert!(patch.enable.is_empty());
        assert_eq!(patch.disable, vec![id.clone()]);
        patch.clear(&id);
        assert!(patch.is_empty());
    }

    #[test]
    fn nested_config_tables_merge_rather_than_replace() {
        let mut base = table(&[(
            "hook",
            toml::Value::Table(table(&[
                ("mode", toml::Value::String("changed".into())),
                ("timeout", toml::Value::String("90s".into())),
            ])),
        )]);
        let overlay = table(&[(
            "hook",
            toml::Value::Table(table(&[("mode", toml::Value::String("full".into()))])),
        )]);
        merge_config(&mut base, &overlay);

        let hook = base["hook"].as_table().unwrap();
        assert_eq!(hook["mode"].as_str(), Some("full"));
        assert_eq!(hook["timeout"].as_str(), Some("90s"));
    }

    #[test]
    fn canonical_config_is_insensitive_to_key_insertion_order() {
        let a = table(&[
            ("z", toml::Value::Integer(1)),
            ("a", toml::Value::Integer(2)),
        ]);
        let b = table(&[
            ("a", toml::Value::Integer(2)),
            ("z", toml::Value::Integer(1)),
        ]);
        assert_eq!(canonical_config(&a), canonical_config(&b));
    }

    #[test]
    fn canonical_config_distinguishes_different_values() {
        let a = table(&[("mode", toml::Value::String("ci".into()))]);
        let b = table(&[("mode", toml::Value::String("local".into()))]);
        assert_ne!(canonical_config(&a), canonical_config(&b));
    }

    #[test]
    fn a_project_profile_file_parses_the_documented_shape() {
        let src = r#"
schema = 1
profiles = [
  "profile/code/rust",
  "profile/agents/worktree-safe",
]
enable = [
  "skill/project/payments-domain",
]
disable = [
  "hook/verify/full-regression",
]
"#;
        let file: ProjectProfileFile = toml::from_str(src).unwrap();
        assert_eq!(file.patch.profiles.len(), 2);
        assert_eq!(file.patch.enable.len(), 1);
        assert_eq!(file.patch.disable.len(), 1);
    }

    #[test]
    fn a_session_overlay_file_carries_its_base_generation() {
        let src = r#"
schema = 1
session_id = "ses_01JTESTTESTTESTTESTTESTTE"
base_generation = "gen_b71f2fdeadbeef01"
enable = ["guidance/mode/research"]

[config."script/test/cargo-nextest"]
profile = "ci"
"#;
        let file: SessionOverlayFile = toml::from_str(src).unwrap();
        assert_eq!(file.base_generation.unwrap().as_str(), "gen_b71f2fdeadbeef01");
        assert_eq!(file.patch.enable.len(), 1);
        assert_eq!(file.patch.config.len(), 1);
    }

    #[test]
    fn a_profile_file_parses_the_documented_shape() {
        let src = r#"
schema = 1
id = "profile/code/rust"
description = "Rust coding baseline."
extends = [
  "profile/base/safe",
  "profile/code/general",
]
enable = [
  "skill/rust/review",
  "script/test/cargo-nextest",
]
disable = []

[config."hook/verify/cargo-check"]
mode = "changed-crates"
timeout = "90s"
"#;
        let file: ProfileFile = toml::from_str(src).unwrap();
        let profile = file.into_profile().unwrap();
        assert_eq!(profile.extends.len(), 2);
        assert_eq!(profile.patch.enable.len(), 2);
        assert_eq!(profile.patch.config.len(), 1);
    }
}
