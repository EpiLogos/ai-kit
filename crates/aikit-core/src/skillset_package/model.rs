//! The neutral model: package metadata authored beside a SkillSet, and the
//! resolved `aikit.portable-skill-package/v1` a target adapter reads.
//!
//! The native SkillSet stays the source. Everything here is a reading of it
//! at exact member revisions; nothing in this module writes back.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::method::{praxis_form, PraxisForm};
use crate::{AikitError, Result};

use super::digest::{sha256_hex, Sha256};

pub const PORTABLE_PACKAGE_SCHEMA: &str = "aikit.portable-skill-package/v1";

// ---------------------------------------------------------------------------
// Authored metadata (`[package]` in set.toml, `[skillset.package]` in an index)
// ---------------------------------------------------------------------------

/// Neutral package metadata authored beside a SkillSet. Every field is
/// optional: a SkillSet with no `[package]` table still exports as Skills.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageMetadata {
    /// Package name (kebab-case). Defaults to the set name, kebab-cased.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<Attribution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// Host tools the Skills rely on (in addition to each member's declared
    /// `[skill].tools`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<McpDependency>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<HookRequirement>,
    /// Explicit user commands the package contributes. Commands are generated
    /// only when declared here; a documentation SkillSet exports as Skills.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandContribution>,
    /// Environment requirements — names and purpose only, never values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<EnvironmentRequirement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<Presentation>,
    /// Shorthand for `presentation.display_name` (a flat index entry reads
    /// better this way). The `[presentation]` table wins when both are set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Shorthand for `presentation.short_description`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    /// Shorthand for `presentation.category`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Target-specific material, keyed by target id. Never mutates the set.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub targets: BTreeMap<String, serde_json::Value>,
}

impl PackageMetadata {
    /// Read the typed metadata from a TOML value (a registry index entry's
    /// `[skillset.package]`, kept untyped by the store).
    pub fn from_toml_value(value: toml::Value) -> Result<Self> {
        value.try_into().map_err(|e: toml::de::Error| {
            AikitError::new(
                "skillset.package.malformed",
                format!("the SkillSet [package] table is not readable: {e}"),
            )
        })
    }

    /// Reject anything that is not expressible as neutral, secret-free
    /// package metadata.
    pub fn validate(&self) -> Result<()> {
        if let Some(name) = &self.name {
            validate_package_name(name)?;
        }
        for dep in &self.mcp {
            dep.validate()?;
        }
        for hook in &self.hooks {
            reject_secret("hooks.command", &hook.command)?;
            if hook.command.trim().is_empty() {
                return Err(malformed(format!(
                    "hook `{}` declares no command",
                    hook.event.as_str()
                )));
            }
        }
        for command in &self.commands {
            validate_command_name(&command.name)?;
            reject_secret("commands.command", &command.command)?;
        }
        for env in &self.environment {
            validate_env_name(&env.name)?;
        }
        for (target, overlay) in &self.targets {
            if !overlay.is_object() {
                return Err(malformed(format!(
                    "targets.{target} must be a table of target-specific manifest fields"
                )));
            }
            reject_secret_in_json(&format!("targets.{target}"), overlay)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribution {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// An MCP server the package's Skills depend on. Env carries names only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpDependency {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Environment variable NAMES the server needs; values are never authored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
}

impl McpDependency {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(malformed("an MCP dependency needs a name".to_string()));
        }
        match (&self.command, &self.url) {
            (Some(_), Some(_)) | (None, None) => {
                return Err(malformed(format!(
                    "MCP dependency `{}` must declare exactly one of `command` or `url`",
                    self.name
                )))
            }
            _ => {}
        }
        if let Some(command) = &self.command {
            reject_secret("mcp.command", command)?;
        }
        if let Some(url) = &self.url {
            reject_secret("mcp.url", url)?;
        }
        for arg in &self.args {
            reject_secret("mcp.args", arg)?;
        }
        for name in &self.env {
            validate_env_name(name)?;
        }
        Ok(())
    }
}

/// Neutral hook events. Each target maps them to its own vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookEvent {
    SessionStart,
    PreTool,
    PostTool,
    PromptSubmit,
    Stop,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session-start",
            Self::PreTool => "pre-tool",
            Self::PostTool => "post-tool",
            Self::PromptSubmit => "prompt-submit",
            Self::Stop => "stop",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookRequirement {
    pub event: HookEvent,
    #[serde(default)]
    pub purpose: String,
    /// Shell command. `${PACKAGE_ROOT}` is rewritten to the target's root
    /// variable.
    pub command: String,
    /// Tool-name matcher for pre/post-tool events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandContribution {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentRequirement {
    pub name: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub developer_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brand_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_prompt: Vec<String>,
}

impl Presentation {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

// ---------------------------------------------------------------------------
// The resolved package
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageIdentity {
    pub name: String,
    /// The source SkillSet's semantic ref (`aikit:project-author`) or home
    /// set name.
    pub semantic_ref: String,
}

/// One payload file of a member Skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFile {
    /// Relative to the member's payload root, `/`-separated.
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    /// Where the bytes live on disk (a canonical capsule payload).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    /// In-memory bytes (tests, generated material). Never serialised.
    #[serde(skip)]
    pub inline: Option<Vec<u8>>,
}

impl PackageFile {
    pub fn inline(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes.into();
        Self {
            path: path.into(),
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            source: None,
            inline: Some(bytes),
        }
    }
}

/// One resolved member Skill at an exact revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageMember {
    pub id: String,
    pub form: PraxisForm,
    /// The `SKILL.md` frontmatter name: what the Skill is called everywhere.
    pub name: String,
    pub description: String,
    pub revision: String,
    /// Host tools the capsule declares (`[skill].tools`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    pub files: Vec<PackageFile>,
}

/// A member the catalogue could not resolve. Never silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedMember {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableSkillPackage {
    pub schema: String,
    pub identity: PackageIdentity,
    pub version: String,
    pub description: String,
    pub skillset_ref: String,
    /// sha256 over the sorted member `id` + `revision` pairs.
    pub source_revision: String,
    pub members: Vec<PackageMember>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved: Vec<UnresolvedMember>,
    pub tool_dependencies: Vec<String>,
    pub mcp_dependencies: Vec<McpDependency>,
    pub hook_requirements: Vec<HookRequirement>,
    pub commands: Vec<CommandContribution>,
    pub environment: Vec<EnvironmentRequirement>,
    pub presentation: Presentation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Attribution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    pub keywords: Vec<String>,
    pub target_overlays: BTreeMap<String, serde_json::Value>,
}

/// What a caller hands the builder: the set's identity plus its members
/// resolved (or not) against a catalogue.
#[derive(Debug, Clone, Default)]
pub struct PackageSource {
    /// Semantic ref or home set name.
    pub skillset_ref: String,
    /// The set's own name (default package name).
    pub set_name: String,
    pub set_description: String,
    pub metadata: Option<PackageMetadata>,
    pub members: Vec<PackageMember>,
    pub unresolved: Vec<UnresolvedMember>,
}

pub const DEFAULT_VERSION: &str = "0.1.0";

impl PortableSkillPackage {
    /// Build the neutral package from a resolved source. Validates metadata
    /// (rejecting secret-like values) and computes the source revision.
    pub fn build(source: PackageSource) -> Result<Self> {
        let metadata = source.metadata.unwrap_or_default();
        metadata.validate()?;
        let mut members = source.members;
        members.sort_by(|a, b| a.id.cmp(&b.id));
        for member in &mut members {
            member.files.sort_by(|a, b| a.path.cmp(&b.path));
            // The form is always re-read from the description: callers cannot
            // assert a classification the source does not carry.
            member.form = praxis_form(&member.description);
        }
        let mut unresolved = source.unresolved;
        unresolved.sort_by(|a, b| a.id.cmp(&b.id));

        let name = match &metadata.name {
            Some(name) => name.clone(),
            None => kebab_case(&source.set_name),
        };
        validate_package_name(&name)?;

        let mut tools: BTreeSet<String> = metadata.tools.iter().cloned().collect();
        for member in &members {
            tools.extend(member.tools.iter().cloned());
        }

        let description = metadata
            .description
            .clone()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| {
                if source.set_description.trim().is_empty() {
                    format!("Skills from the AIKit SkillSet `{}`.", source.skillset_ref)
                } else {
                    source.set_description.trim().to_string()
                }
            });

        let source_revision = source_revision(&members, &unresolved);
        Ok(Self {
            schema: PORTABLE_PACKAGE_SCHEMA.to_string(),
            identity: PackageIdentity {
                name,
                semantic_ref: source.skillset_ref.clone(),
            },
            version: metadata
                .version
                .clone()
                .unwrap_or_else(|| DEFAULT_VERSION.to_string()),
            description,
            skillset_ref: source.skillset_ref,
            source_revision,
            members,
            unresolved,
            tool_dependencies: tools.into_iter().collect(),
            mcp_dependencies: metadata.mcp,
            hook_requirements: metadata.hooks,
            commands: metadata.commands,
            environment: metadata.environment,
            presentation: {
                let mut p = metadata.presentation.unwrap_or_default();
                p.display_name = p.display_name.or(metadata.display_name);
                p.short_description = p.short_description.or(metadata.short_description);
                p.category = p.category.or(metadata.category);
                p
            },
            license: metadata.license,
            attribution: metadata.author,
            homepage: metadata.homepage,
            repository: metadata.repository,
            keywords: metadata.keywords,
            target_overlays: metadata.targets,
        })
    }

    pub fn is_complete(&self) -> bool {
        self.unresolved.is_empty()
    }
}

/// sha256 over sorted `id\0revision\n` lines; unresolved members count as
/// `id\0unresolved\n` so resolving one moves the revision.
pub fn source_revision(members: &[PackageMember], unresolved: &[UnresolvedMember]) -> String {
    let mut lines: Vec<(String, String)> = members
        .iter()
        .map(|m| (m.id.clone(), m.revision.clone()))
        .chain(
            unresolved
                .iter()
                .map(|u| (u.id.clone(), "unresolved".to_string())),
        )
        .collect();
    lines.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"aikit-portable-skill-package-source-v1\n");
    for (id, revision) in lines {
        hasher.update(id.as_bytes());
        hasher.update(b"\0");
        hasher.update(revision.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", hasher.finish_hex())
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

fn malformed(detail: String) -> AikitError {
    AikitError::new("skillset.package.malformed", detail)
}

/// Kebab-case package name, 1–64 chars — the intersection of the Claude,
/// Agent Plugins and npm name rules.
pub fn validate_package_name(name: &str) -> Result<()> {
    if is_kebab(name) && name.len() <= 64 {
        Ok(())
    } else {
        Err(AikitError::new(
            "skillset.package.bad_name",
            format!(
                "`{name}` is not a usable package name: use 1–64 lowercase letters, digits and \
                 single hyphens (kebab-case)"
            ),
        )
        .with("name", name.to_string()))
    }
}

pub fn is_kebab(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn kebab_case(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    let mut cut: String = trimmed.chars().take(64).collect();
    while cut.ends_with('-') {
        cut.pop();
    }
    cut
}

fn validate_command_name(name: &str) -> Result<()> {
    if is_kebab(name) {
        Ok(())
    } else {
        Err(malformed(format!(
            "command name `{name}` must be kebab-case"
        )))
    }
}

/// An environment requirement is a NAME. `NAME=value` or anything that is
/// not a conventional variable name is refused.
pub fn validate_env_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(AikitError::new(
            "skillset.package.secret_value",
            format!(
                "environment requirement `{}` is not a variable NAME; package metadata carries \
                 names only, never values",
                redact(name)
            ),
        ))
    }
}

/// Heuristic secret detection for authored strings. Conservative: it looks for
/// well-known credential shapes and inline `KEY=value` assignments of
/// secret-named variables.
pub fn looks_like_secret(value: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "sk-",
        "sk_live_",
        "sk_test_",
        "rk_live_",
        "ghp_",
        "gho_",
        "ghs_",
        "ghu_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "AKIA",
        "ASIA",
        "AIza",
        "ya29.",
        "hf_",
        "npm_",
        "pypi-",
    ];
    for token in value.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '=') {
        let token = token.trim_matches(|c: char| c == ',' || c == ';');
        if token.len() >= 16 && PREFIXES.iter().any(|p| token.starts_with(p)) {
            return true;
        }
        if token.starts_with("-----BEGIN") {
            return true;
        }
    }
    if value.contains("PRIVATE KEY-----") {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    if lower.contains("bearer ") {
        let after = &value[lower.find("bearer ").unwrap_or(0) + 7..];
        if after.trim().len() >= 16
            && !after.trim().starts_with("${")
            && !after.trim().starts_with('$')
        {
            return true;
        }
    }
    // KEY=value where KEY smells secret and the value is literal.
    for part in value.split_whitespace() {
        if let Some((key, val)) = part.split_once('=') {
            let key_lower = key.to_ascii_lowercase();
            let secretish = ["token", "secret", "password", "passwd", "api_key", "apikey"]
                .iter()
                .any(|s| key_lower.contains(s));
            let literal = !val.is_empty() && !val.starts_with('$');
            if secretish && literal {
                return true;
            }
        }
    }
    // URL userinfo: scheme://user:pass@host
    if let Some(rest) = value.split("://").nth(1) {
        if let Some(userinfo) = rest.split('/').next().and_then(|h| h.split_once('@')) {
            if userinfo.0.contains(':') {
                return true;
            }
        }
    }
    false
}

fn redact(value: &str) -> String {
    let shown: String = value.chars().take(4).collect();
    format!("{shown}…")
}

pub fn reject_secret(field: &str, value: &str) -> Result<()> {
    if looks_like_secret(value) {
        Err(AikitError::new(
            "skillset.package.secret_value",
            format!(
                "{field} looks like it carries a secret value (`{}`); package metadata carries \
                 environment variable names only — reference `${{NAME}}` instead",
                redact(value)
            ),
        )
        .with("field", field.to_string()))
    } else {
        Ok(())
    }
}

fn reject_secret_in_json(field: &str, value: &serde_json::Value) -> Result<()> {
    match value {
        serde_json::Value::String(s) => reject_secret(field, s),
        serde_json::Value::Array(items) => items
            .iter()
            .try_for_each(|item| reject_secret_in_json(field, item)),
        serde_json::Value::Object(map) => map
            .iter()
            .try_for_each(|(k, v)| reject_secret_in_json(&format!("{field}.{k}"), v)),
        _ => Ok(()),
    }
}
