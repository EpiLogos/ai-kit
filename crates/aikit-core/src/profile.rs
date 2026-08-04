//! Profiles and pool patches.
//!
//! A profile is a reusable composition recipe, not a capsule. A pool patch is the
//! set of declarations attached to one scope. Both projects and sessions carry
//! pool patches; that symmetry is what lets a session change be promoted into a
//! project without rewriting it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::arg::{ArgSpec, ArgType, ArgValue, Literal, PathKind};
use crate::error::{err, AikitError, Result};
use crate::id::{CapsuleId, GenerationId, ProfileId, SessionId};

/// Free-form per-capsule configuration, carried through resolution untouched.
pub type ConfigTable = toml::value::Table;

/// Additive, scoped guidance for how an immutable Agent Skill is selected and
/// applied. The source skill remains authoritative and untouched; this patch is
/// compiled into the harness-facing Effective Skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillUsageOverlayPatch {
    /// Whether lower-scope orientation remains in force. `false` starts again
    /// from the upstream skill before adding this scope's guidance.
    #[serde(default = "yes")]
    pub inherit: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub guidance: Option<String>,
    #[serde(default)]
    pub reviewed_against: Option<crate::id::Revision>,
}

impl SkillUsageOverlayPatch {
    pub fn has_content(&self) -> bool {
        self.description.as_ref().is_some_and(|v| !v.trim().is_empty())
            || self.guidance.as_ref().is_some_and(|v| !v.trim().is_empty())
    }

    pub fn validate(&self, id: &CapsuleId) -> Result<()> {
        for (field, value) in [
            ("description", self.description.as_deref()),
            ("guidance", self.guidance.as_deref()),
        ] {
            if value.is_some_and(|text| text.contains('\0')) {
                return Err(AikitError::new(
                    "skill_overlay.invalid_text",
                    format!("{id} overlay {field} contains a NUL byte"),
                ));
            }
        }
        if self.inherit && !self.has_content() && self.reviewed_against.is_none() {
            return Err(AikitError::new(
                "skill_overlay.empty",
                format!("{id} has an empty Skill Usage Overlay"),
            ));
        }
        if let Some(revision) = &self.reviewed_against {
            let raw = revision.as_str();
            if raw.len() != 64
                || !raw
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(AikitError::new(
                    "skill_overlay.invalid_revision",
                    format!(
                        "{id} reviewed_against must be an exact 64-character lowercase content digest"
                    ),
                )
                .with("capability", id.to_string())
                .with("revision", raw.to_string()));
            }
        }
        Ok(())
    }
}

fn yes() -> bool {
    true
}

/// How a capsule's `[config.*]` section combines across scope layers.
///
/// This is declared by the capsule the section configures, not by each writer of
/// the section, because whether config is a bag of independent keys or a single
/// replaceable record is a fact about the *thing being configured* — an MCP
/// server entry and a command spec are records; a set of hook options is a bag.
/// Getting this explicit is the single most common "why isn't my config taking
/// effect" fix across every surveyed tool (PRIOR-ART-ACTIONS #27): Claude MCP,
/// mise `[tasks]` and flox all replace whole records where AIKit would otherwise
/// deep-merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigMerge {
    /// Recursive deep merge: a higher scope may change one nested field without
    /// restating the rest. The correct default for key/value config.
    #[default]
    Deep,
    /// Whole-record replacement: a higher scope's table *is* the record; keys the
    /// lower scope set that the higher one omits do not bleed through.
    Replace,
}

impl ConfigMerge {
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigMerge::Deep => "deep",
            ConfigMerge::Replace => "replace",
        }
    }
}

/// The `enable` / `disable` / `profiles` / `[config.*]` declarations of one scope.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PoolPatch {
    #[serde(default)]
    pub profiles: Vec<ProfileId>,
    /// Parameterized profile references (`[[use]]`). Kept beside the original
    /// string array so existing profile files remain valid and simple profiles
    /// pay no syntax cost.
    #[serde(default, rename = "use")]
    pub uses: Vec<ProfileUse>,
    #[serde(default)]
    pub enable: Vec<CapsuleId>,
    #[serde(default)]
    pub disable: Vec<CapsuleId>,
    #[serde(default)]
    pub config: BTreeMap<CapsuleId, ConfigTable>,
    #[serde(default, rename = "skill-overlays")]
    pub skill_overlays: BTreeMap<CapsuleId, SkillUsageOverlayPatch>,
}

impl PoolPatch {
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
            && self.uses.is_empty()
            && self.enable.is_empty()
            && self.disable.is_empty()
            && self.config.is_empty()
            && self.skill_overlays.is_empty()
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
        for (id, overlay) in &self.skill_overlays {
            overlay.validate(id)?;
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
        self.skill_overlays.remove(id);
    }
}

/// One profile reference with explicit, committed parameter bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileUse {
    pub profile: ProfileId,
    #[serde(default)]
    pub params: BTreeMap<String, Literal>,
}

/// A typed profile parameter. It deliberately reuses the manifest argument
/// types and coercion rules; profile bindings are configuration, not a second
/// little type system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileParam {
    #[serde(rename = "type")]
    pub ty: ArgType,
    #[serde(default)]
    pub default: Option<Literal>,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub pattern: Option<String>,
}

impl ProfileParam {
    fn as_arg(&self, name: &str) -> ArgSpec {
        ArgSpec {
            name: name.to_string(),
            label: None,
            help: None,
            ty: self.ty,
            position: None,
            flag: None,
            required: Some(self.default.is_none()),
            default: self.default.clone(),
            default_from: None,
            choices: self.choices.clone(),
            must_exist: false,
            path_kind: PathKind::Any,
            min: self.min,
            max: self.max,
            pattern: self.pattern.clone(),
            repeatable: false,
            secret: false,
        }
    }

    fn validate(&self, profile: &ProfileId, name: &str) -> Result<()> {
        if self.ty == ArgType::Secret {
            return Err(AikitError::new(
                "profile.secret_parameter_forbidden",
                format!(
                    "{profile} parameter `{name}` cannot be secret because profile bindings are committed configuration"
                ),
            )
            .with("profile", profile.to_string())
            .with("parameter", name.to_string()));
        }
        self.as_arg(name).validate_spec().map_err(|error| {
            AikitError::new(
                "profile.invalid_parameter",
                format!(
                    "{profile} parameter `{name}` is invalid: {}",
                    error.message()
                ),
            )
            .with("profile", profile.to_string())
            .with("parameter", name.to_string())
        })
    }

    /// Coerce a CLI-style textual binding into the typed TOML literal that a
    /// committed profile reference must retain.
    pub fn coerce_binding(&self, name: &str, raw: &str) -> Result<Literal> {
        let value = self.as_arg(name).coerce(raw)?;
        Ok(match value {
            ArgValue::Bool(value) => Literal::Bool(value),
            ArgValue::Integer(value) => Literal::Integer(value),
            ArgValue::Float(value) => Literal::Float(value),
            ArgValue::List(value) => Literal::List(value),
            ArgValue::String(value) | ArgValue::Secret(value) => Literal::String(value),
            ArgValue::Duration(value) => Literal::String(value.to_string()),
            ArgValue::KeyValue(value) => Literal::String(
                value
                    .into_iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        })
    }
}

/// The profile patch before parameter substitution.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileTemplate {
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
    #[serde(default)]
    pub config: BTreeMap<String, ConfigTable>,
    #[serde(default, rename = "skill-overlays")]
    pub skill_overlays: BTreeMap<String, SkillUsageOverlayPatch>,
}

#[derive(Debug, Clone)]
struct BoundValue {
    text: String,
    typed: toml::Value,
}

/// A reusable declarative patch identified by a profile id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: ProfileId,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub extends: Vec<ProfileId>,
    /// Parameterized parent profiles. This is separate from the compact string
    /// `extends` form so ordinary inheritance remains terse while a fork can
    /// durably record the values with which its base was reviewed.
    #[serde(default, rename = "extends_use")]
    pub extends_uses: Vec<ProfileUse>,
    #[serde(default)]
    pub params: BTreeMap<String, ProfileParam>,
    #[serde(default)]
    pub template: ProfileTemplate,
    #[serde(flatten)]
    pub patch: PoolPatch,
}

impl Profile {
    /// Bind and validate parameters, then turn every placeholder into ordinary,
    /// explicit ids before the resolver layers anything.
    pub fn resolve_patch(&self, supplied: &BTreeMap<String, Literal>) -> Result<PoolPatch> {
        // Programmatically constructed profiles predate templates and already
        // carry their explicit patch. Keeping this path also makes the template
        // representation an on-disk concern rather than a tax on callers.
        if self.params.is_empty()
            && self.template.profiles.is_empty()
            && self.template.enable.is_empty()
            && self.template.disable.is_empty()
            && self.template.config.is_empty()
            && self.template.skill_overlays.is_empty()
        {
            return Ok(self.patch.clone());
        }
        for (name, declaration) in &self.params {
            declaration.validate(&self.id, name)?;
        }
        for name in supplied.keys() {
            if !self.params.contains_key(name) {
                return Err(AikitError::new(
                    "profile.unknown_parameter",
                    format!("{} declares no parameter `{name}`", self.id),
                )
                .with("profile", self.id.to_string())
                .with("parameter", name.clone()));
            }
        }

        let mut values: BTreeMap<String, BoundValue> = BTreeMap::new();
        for (name, declaration) in &self.params {
            let raw = supplied
                .get(name)
                .or(declaration.default.as_ref())
                .ok_or_else(|| {
                    AikitError::new(
                        "profile.missing_parameter",
                        format!("{} requires a value for `{name}`", self.id),
                    )
                    .with("profile", self.id.to_string())
                    .with("parameter", name.clone())
                })?
                .to_string();
            let argument = declaration.as_arg(name);
            let value = argument.coerce(&raw).map_err(|error| {
                AikitError::new(
                    "profile.invalid_parameter",
                    format!(
                        "{} parameter `{name}` is invalid: {}",
                        self.id,
                        error.message()
                    ),
                )
                .with("profile", self.id.to_string())
                .with("parameter", name.clone())
                .with("value", raw.clone())
            })?;
            values.insert(
                name.clone(),
                BoundValue {
                    text: value.to_argv_string(),
                    typed: argument_value_to_toml(value),
                },
            );
        }

        let mut patch = PoolPatch::default();
        for raw in &self.template.profiles {
            patch
                .profiles
                .push(ProfileId::parse(&substitute(raw, &values))?);
        }
        for raw in &self.template.enable {
            patch
                .enable
                .push(CapsuleId::parse(&substitute(raw, &values))?);
        }
        for raw in &self.template.disable {
            patch
                .disable
                .push(CapsuleId::parse(&substitute(raw, &values))?);
        }
        for (raw_id, table) in &self.template.config {
            let id = CapsuleId::parse(&substitute(raw_id, &values))?;
            patch.config.insert(id, substitute_table(table, &values));
        }
        for (raw_id, overlay) in &self.template.skill_overlays {
            let id = CapsuleId::parse(&substitute(raw_id, &values))?;
            let mut resolved = overlay.clone();
            resolved.description = resolved
                .description
                .as_deref()
                .map(|text| substitute(text, &values));
            resolved.guidance = resolved
                .guidance
                .as_deref()
                .map(|text| substitute(text, &values));
            patch.skill_overlays.insert(id, resolved);
        }
        patch.validate()?;
        Ok(patch)
    }
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
    #[serde(default, rename = "extends_use")]
    pub extends_uses: Vec<ProfileUse>,
    #[serde(default)]
    pub params: BTreeMap<String, ProfileParam>,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
    #[serde(default)]
    pub config: BTreeMap<String, ConfigTable>,
    #[serde(default, rename = "skill-overlays")]
    pub skill_overlays: BTreeMap<String, SkillUsageOverlayPatch>,
}

impl ProfileFile {
    pub fn into_profile(self) -> Result<Profile> {
        if self.schema != 1 {
            return err(
                "profile.unsupported_schema",
                format!("profile schema {} is not supported", self.schema),
            );
        }
        let template = ProfileTemplate {
            profiles: self.profiles,
            enable: self.enable,
            disable: self.disable,
            config: self.config,
            skill_overlays: self.skill_overlays,
        };
        let mut profile = Profile {
            id: self.id,
            description: self.description,
            extends: self.extends,
            extends_uses: self.extends_uses,
            params: self.params,
            template,
            patch: PoolPatch::default(),
        };
        profile.patch = match profile.resolve_patch(&BTreeMap::new()) {
            Ok(patch) => patch,
            // A parameterized template is catalogued before a project supplies
            // its bindings. The resolver reports these declaration errors when
            // the profile is actually referenced, preserving the specific
            // profile-domain code instead of degrading it to
            // `resolution.unknown_profile`.
            Err(error)
                if matches!(
                    error.code(),
                    "profile.missing_parameter" | "profile.secret_parameter_forbidden"
                ) =>
            {
                PoolPatch::default()
            }
            Err(error) => return Err(error),
        };
        Ok(profile)
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

fn substitute(input: &str, values: &BTreeMap<String, BoundValue>) -> String {
    let mut output = input.to_string();
    for (name, value) in values {
        output = output.replace(&format!("{{{{{name}}}}}"), &value.text);
    }
    output
}

fn substitute_table(table: &ConfigTable, values: &BTreeMap<String, BoundValue>) -> ConfigTable {
    table
        .iter()
        .map(|(key, value)| (substitute(key, values), substitute_value(value, values)))
        .collect()
}

fn substitute_value(value: &toml::Value, values: &BTreeMap<String, BoundValue>) -> toml::Value {
    match value {
        toml::Value::String(text) => {
            if let Some(name) = text
                .strip_prefix("{{")
                .and_then(|rest| rest.strip_suffix("}}"))
            {
                if let Some(bound) = values.get(name) {
                    return bound.typed.clone();
                }
            }
            toml::Value::String(substitute(text, values))
        }
        toml::Value::Array(items) => toml::Value::Array(
            items
                .iter()
                .map(|item| substitute_value(item, values))
                .collect(),
        ),
        toml::Value::Table(table) => toml::Value::Table(substitute_table(table, values)),
        other => other.clone(),
    }
}

fn argument_value_to_toml(value: ArgValue) -> toml::Value {
    match value {
        ArgValue::String(value) | ArgValue::Secret(value) => toml::Value::String(value),
        ArgValue::Integer(value) => toml::Value::Integer(value),
        ArgValue::Float(value) => toml::Value::Float(value),
        ArgValue::Bool(value) => toml::Value::Boolean(value),
        ArgValue::Duration(value) => toml::Value::String(value.to_string()),
        ArgValue::List(values) => {
            toml::Value::Array(values.into_iter().map(toml::Value::String).collect())
        }
        ArgValue::KeyValue(values) => toml::Value::Table(
            values
                .into_iter()
                .map(|(key, value)| (key, toml::Value::String(value)))
                .collect(),
        ),
    }
}

/// Fold one capsule's `[config.*]` section from a higher scope (`overlay`) into
/// the accumulated lower-scope value (`base`), honouring the capsule's declared
/// [`ConfigMerge`] mode.
///
/// This is the one place the merge algebra lives, so `Deep` versus `Replace` is a
/// single branch a reader can see rather than a boolean threaded through call
/// sites — the failure mode PRIOR-ART-ACTIONS #14/#27 warn about.
pub fn combine_config(base: &mut ConfigTable, overlay: &ConfigTable, mode: ConfigMerge) {
    match mode {
        ConfigMerge::Deep => merge_config(base, overlay),
        ConfigMerge::Replace => {
            // The higher scope's record replaces the lower one entirely; nothing
            // the lower scope set survives into the effective view.
            base.clear();
            for (key, value) in overlay {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Recursively merge `overlay` into `base`, key by key.
///
/// Higher scopes win on scalars; nested tables merge rather than replace, so a
/// session can change one hook option without restating the project's whole
/// table. This is the [`ConfigMerge::Deep`] behaviour and the default; a section
/// that is a whole replaceable record uses [`combine_config`] with
/// [`ConfigMerge::Replace`].
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
    /// Escape the characters the canonical form uses as structure.
    ///
    /// `"`, `;` and `=` are all legal *inside* a TOML string, so without this a
    /// single key whose value embedded them would render byte-for-byte identically
    /// to two separate keys — and because this string feeds the resolution hash,
    /// two genuinely different effective configs would share one generation
    /// identity and a stale generation would be silently reused. Escaping the
    /// backslash first keeps the mapping injective.
    fn escape(raw: &str, out: &mut String) {
        for c in raw.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                ';' => out.push_str("\\;"),
                '=' => out.push_str("\\="),
                other => out.push(other),
            }
        }
    }

    fn write_value(out: &mut String, value: &toml::Value) {
        match value {
            toml::Value::String(s) => {
                out.push('"');
                escape(s, out);
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
                    escape(key, out);
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
        escape(key, &mut out);
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
    fn exact_parameter_placeholders_keep_their_toml_types() {
        let file: ProfileFile = toml::from_str(
            r#"schema = 1
id = "profile/test/typed"
enable = ["script/test/tool"]

[params.retries]
type = "integer"

[params.strict]
type = "bool"

[config."script/test/tool"]
retries = "{{retries}}"
strict = "{{strict}}"
label = "strict={{strict}}"
"#,
        )
        .unwrap();
        let profile = file.into_profile().unwrap();
        let bindings = BTreeMap::from([
            ("retries".into(), Literal::Integer(3)),
            ("strict".into(), Literal::Bool(true)),
        ]);

        let patch = profile.resolve_patch(&bindings).unwrap();
        let config = &patch.config[&CapsuleId::parse("script/test/tool").unwrap()];
        assert_eq!(config["retries"], toml::Value::Integer(3));
        assert_eq!(config["strict"], toml::Value::Boolean(true));
        assert_eq!(config["label"].as_str(), Some("strict=true"));
    }

    #[test]
    fn named_profiles_can_parameterize_skill_usage_overlays() {
        let file: ProfileFile = toml::from_str(
            r#"schema = 1
id = "profile/test/wayfinder"

[params.owner]
type = "string"

[skill-overlays."skill/{{owner}}/wayfinder"]
description = "Prefer for {{owner}} work spanning agent sessions."
guidance = "Use the {{owner}} issue tracker as the shared map."
"#,
        )
        .unwrap();
        let profile = file.into_profile().unwrap();
        let bindings = BTreeMap::from([("owner".into(), Literal::String("team".into()))]);

        let patch = profile.resolve_patch(&bindings).unwrap();
        let overlay = &patch.skill_overlays
            [&CapsuleId::parse("skill/team/wayfinder").unwrap()];
        assert_eq!(
            overlay.description.as_deref(),
            Some("Prefer for team work spanning agent sessions.")
        );
        assert_eq!(
            overlay.guidance.as_deref(),
            Some("Use the team issue tracker as the shared map.")
        );
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
        assert_eq!(
            file.base_generation.unwrap().as_str(),
            "gen_b71f2fdeadbeef01"
        );
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
