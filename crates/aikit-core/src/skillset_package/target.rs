//! The target adapter seam: capabilities as data, a classified plan, and a
//! pure render to an in-memory file list.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{AikitError, Result};

use super::digest::sha256_hex;
use super::model::{PackageMember, PortableSkillPackage};

pub const PLAN_SCHEMA: &str = "aikit.skillset-package-plan/v1";
pub const PROVENANCE_SCHEMA: &str = "aikit.skillset-package-provenance/v1";
/// Provenance file at the package root. `claude plugin validate --strict`,
/// Agent Plugins and pi all tolerate an extra root file.
pub const PROVENANCE_FILE: &str = "aikit-package.json";
pub const GENERATOR: &str = concat!("aikit ", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetId {
    Openai,
    Codex,
    Claude,
    Pi,
}

impl TargetId {
    pub const ALL: [TargetId; 4] = [Self::Openai, Self::Codex, Self::Claude, Self::Pi];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Pi => "pi",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.as_str() == raw)
            .ok_or_else(|| {
                AikitError::new(
                    "skillset.package.unknown_target",
                    format!("unknown package target `{raw}`; use openai, codex, claude or pi"),
                )
            })
    }
}

/// The §5 questions, answered as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetCapabilities {
    pub target: TargetId,
    pub format: String,
    pub format_version: String,
    pub package_identity: String,
    pub skills_location: String,
    pub mcp: String,
    pub hooks_and_extensions: String,
    pub ui_contribution: String,
    pub install_discovery: String,
    pub validation: String,
    pub no_analogue: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "kebab-case")]
pub enum PlanClass {
    /// Carried byte-for-byte (a Skill payload).
    Portable,
    /// Expressed in the target's own vocabulary.
    Translated,
    /// Generated material with no source counterpart.
    TargetAddition,
    /// No analogue in the target; recorded, never dropped silently.
    Unsupported { reason: String },
}

impl PlanClass {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Portable => "portable",
            Self::Translated => "translated",
            Self::TargetAddition => "target-addition",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanEntry {
    /// `member:<id>`, `mcp:<name>`, `hook:<event>`, `command:<name>`,
    /// `environment:<NAME>`, `tool:<name>`, `presentation`, `overlay:<target>`,
    /// `manifest`, `provenance`.
    pub relation: String,
    #[serde(flatten)]
    pub class: PlanClass,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

impl PlanEntry {
    pub fn new(relation: impl Into<String>, class: PlanClass) -> Self {
        Self {
            relation: relation.into(),
            class,
            paths: Vec::new(),
            detail: String::new(),
        }
    }

    #[must_use]
    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.paths.push(path.into());
        self
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn unsupported(relation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(
            relation,
            PlanClass::Unsupported {
                reason: reason.into(),
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackagePlan {
    pub schema: String,
    pub target: TargetId,
    pub format_version: String,
    pub package: String,
    pub version: String,
    pub skillset_ref: String,
    pub source_revision: String,
    pub entries: Vec<PlanEntry>,
}

impl PackagePlan {
    pub fn new(target: &dyn PackageTarget, pkg: &PortableSkillPackage) -> Self {
        Self {
            schema: PLAN_SCHEMA.to_string(),
            target: target.id(),
            format_version: target.format_version().to_string(),
            package: pkg.identity.name.clone(),
            version: pkg.version.clone(),
            skillset_ref: pkg.skillset_ref.clone(),
            source_revision: pkg.source_revision.clone(),
            entries: Vec::new(),
        }
    }

    pub fn push(&mut self, entry: PlanEntry) {
        self.entries.push(entry);
    }

    pub fn of_class(&self, label: &str) -> Vec<&PlanEntry> {
        self.entries
            .iter()
            .filter(|e| e.class.label() == label)
            .collect()
    }

    pub fn has(&self, relation: &str) -> Option<&PlanEntry> {
        self.entries.iter().find(|e| e.relation == relation)
    }

    /// Members the target will carry, with their exported directory name.
    pub fn carried_members(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .filter(|e| e.relation.starts_with("member:") && e.class == PlanClass::Portable)
            .map(|e| e.relation["member:".len()..].to_string())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderedContent {
    Bytes(Vec<u8>),
    /// Copy a canonical source file byte-for-byte.
    Copy {
        source: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    /// Relative to the package root, `/`-separated.
    pub path: String,
    pub content: RenderedContent,
    /// sha256 of the bytes that will be written.
    pub sha256: String,
    /// Mark executable (scripts carried from a payload keep their bit via copy;
    /// this is for generated files).
    pub executable: bool,
}

impl RenderedFile {
    pub fn bytes(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes.into();
        Self {
            path: path.into(),
            sha256: sha256_hex(&bytes),
            content: RenderedContent::Bytes(bytes),
            executable: false,
        }
    }

    pub fn json(path: impl Into<String>, value: &Value) -> Self {
        let mut text = serde_json::to_string_pretty(value).expect("json value serialises");
        text.push('\n');
        Self::bytes(path, text.into_bytes())
    }
}

/// A native command that proves a rendered package usable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationCommand {
    pub program: String,
    pub args: Vec<String>,
    /// Bytes written to stdin, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    /// What a pass looks like, in words.
    pub expectation: String,
}

impl ValidationCommand {
    pub fn display(&self) -> String {
        let mut out = self.program.clone();
        for arg in &self.args {
            out.push(' ');
            if arg.contains(' ') || arg.is_empty() {
                out.push_str(&format!("'{arg}'"));
            } else {
                out.push_str(arg);
            }
        }
        if let Some(stdin) = &self.stdin {
            out = format!("echo '{}' | {out}", stdin.trim());
        }
        out
    }
}

/// A structural finding against the target's field rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub path: String,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Error,
    Warning,
}

pub type FileMap = BTreeMap<String, Vec<u8>>;

pub trait PackageTarget {
    fn id(&self) -> TargetId;
    fn format_version(&self) -> &'static str;
    fn capabilities(&self) -> TargetCapabilities;
    fn plan(&self, pkg: &PortableSkillPackage) -> PackagePlan;
    fn render(&self, pkg: &PortableSkillPackage, plan: &PackagePlan) -> Result<Vec<RenderedFile>>;
    /// The native validation command, when the target ships one.
    fn native_validation(&self, dir: &Path) -> Option<ValidationCommand>;
    /// Structural validation against the target's own field rules, over an
    /// exported tree read into memory.
    fn structural_validation(&self, files: &FileMap) -> Vec<Finding>;
}

/// The adapter for a target id. `codex_overlay` adds the
/// `.codex-plugin/plugin.json` compatibility overlay to `openai`.
pub fn target_for(id: TargetId, codex_overlay: bool) -> Box<dyn PackageTarget> {
    match id {
        TargetId::Openai => Box::new(super::openai::OpenAiTarget {
            id: TargetId::Openai,
            codex_overlay,
        }),
        TargetId::Codex => Box::new(super::openai::OpenAiTarget {
            id: TargetId::Codex,
            codex_overlay: true,
        }),
        TargetId::Claude => Box::new(super::claude::ClaudeTarget),
        TargetId::Pi => Box::new(super::pi::PiTarget),
    }
}

// ---------------------------------------------------------------------------
// Shared planning / rendering helpers
// ---------------------------------------------------------------------------

/// Plan every member as a portable Skill under `skills/<name>/`, and every
/// unresolved or colliding member as unsupported.
pub(crate) fn plan_members(plan: &mut PackagePlan, pkg: &PortableSkillPackage) {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for member in &pkg.members {
        let relation = format!("member:{}", member.id);
        if let Some(first) = seen.get(&member.name) {
            plan.push(PlanEntry::unsupported(
                relation,
                format!(
                    "Skill name `{}` is already carried by `{first}`; a package holds one Skill per name",
                    member.name
                ),
            ));
            continue;
        }
        if !is_usable_skill_dir(&member.name) {
            plan.push(PlanEntry::unsupported(
                relation,
                format!(
                    "Skill name `{}` cannot become a directory under skills/",
                    member.name
                ),
            ));
            continue;
        }
        seen.insert(member.name.clone(), member.id.clone());
        let mut entry = PlanEntry::new(relation, PlanClass::Portable)
            .at(format!("skills/{}/", member.name))
            .detail(format!(
                "{} `{}` @ {} ({} files)",
                member.form.as_str(),
                member.name,
                short(&member.revision),
                member.files.len()
            ));
        if !super::model::is_kebab(&member.name) {
            entry
                .detail
                .push_str("; name is outside the Agent Skills [a-z0-9-] rule — hosts may warn");
        }
        plan.push(entry);
    }
    for unresolved in &pkg.unresolved {
        plan.push(PlanEntry::unsupported(
            format!("member:{}", unresolved.id),
            format!("unresolved: {}", unresolved.reason),
        ));
    }
}

fn is_usable_skill_dir(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\'])
}

pub(crate) fn short(revision: &str) -> String {
    let body = revision.strip_prefix("sha256:").unwrap_or(revision);
    body.chars().take(12).collect()
}

/// The members a plan carries, in package order.
pub(crate) fn carried<'a>(
    pkg: &'a PortableSkillPackage,
    plan: &PackagePlan,
) -> Vec<&'a PackageMember> {
    let carried = plan.carried_members();
    pkg.members
        .iter()
        .filter(|m| carried.contains(&m.id))
        .collect()
}

/// Render every carried member's payload under `skills/<name>/`.
pub(crate) fn render_members(
    pkg: &PortableSkillPackage,
    plan: &PackagePlan,
) -> Result<Vec<RenderedFile>> {
    let mut out = Vec::new();
    for member in carried(pkg, plan) {
        for file in &member.files {
            let path = format!("skills/{}/{}", member.name, file.path);
            let content = match (&file.inline, &file.source) {
                (Some(bytes), _) => RenderedContent::Bytes(bytes.clone()),
                (None, Some(source)) => RenderedContent::Copy {
                    source: source.clone(),
                },
                (None, None) => {
                    return Err(AikitError::new(
                        "skillset.package.missing_bytes",
                        format!("{} has no source bytes for {}", member.id, file.path),
                    ))
                }
            };
            out.push(RenderedFile {
                path,
                content,
                sha256: file.sha256.clone(),
                executable: false,
            });
        }
    }
    Ok(out)
}

/// The provenance carried inside every rendered package.
pub fn provenance(pkg: &PortableSkillPackage, plan: &PackagePlan) -> Value {
    let carried = plan.carried_members();
    json!({
        "schema": PROVENANCE_SCHEMA,
        "generator": GENERATOR,
        "target": plan.target.as_str(),
        "format_version": plan.format_version,
        "package": {"name": pkg.identity.name, "version": pkg.version},
        "skillset_ref": pkg.skillset_ref,
        "source_revision": pkg.source_revision,
        "members": pkg.members.iter().map(|m| json!({
            "id": m.id,
            "name": m.name,
            "form": m.form.as_str(),
            "revision": m.revision,
            "exported": carried.contains(&m.id),
        })).collect::<Vec<_>>(),
        "unresolved": pkg.unresolved.iter().map(|u| json!({"id": u.id, "reason": u.reason})).collect::<Vec<_>>(),
        "tool_dependencies": pkg.tool_dependencies,
        "environment": pkg.environment.iter().map(|e| json!({
            "name": e.name, "purpose": e.purpose, "required": e.required
        })).collect::<Vec<_>>(),
        "note": "Generated projection of an AIKit SkillSet. The SkillSet is the source; edit it there and re-export.",
    })
}

pub(crate) fn plan_provenance(plan: &mut PackagePlan) {
    plan.push(
        PlanEntry::new("provenance", PlanClass::TargetAddition)
            .at(PROVENANCE_FILE)
            .detail("source SkillSet ref, source revision and member revisions"),
    );
}

/// Tool dependencies and environment names have no manifest analogue in any
/// current target; they stay visible in the provenance file.
pub(crate) fn plan_requirements(plan: &mut PackagePlan, pkg: &PortableSkillPackage, target: &str) {
    for tool in &pkg.tool_dependencies {
        plan.push(
            PlanEntry::unsupported(
                format!("tool:{tool}"),
                format!("{target} manifests have no field declaring required host tools"),
            )
            .at(PROVENANCE_FILE)
            .detail("recorded in the provenance file"),
        );
    }
    for env in &pkg.environment {
        let referenced = pkg
            .mcp_dependencies
            .iter()
            .any(|m| m.env.contains(&env.name));
        let reason = if referenced {
            format!(
                "{target} has no package-level environment declaration; the name is referenced by an MCP server entry"
            )
        } else {
            format!("{target} has no package-level environment declaration")
        };
        plan.push(
            PlanEntry::unsupported(format!("environment:{}", env.name), reason)
                .at(PROVENANCE_FILE)
                .detail("name recorded in the provenance file; no value is ever exported"),
        );
    }
}

/// Deep-merge `overlay` into `base` (objects merge, everything else replaces).
pub fn merge_json(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

/// Rewrite the neutral `${PACKAGE_ROOT}` to the target's root variable.
pub(crate) fn with_root(command: &str, root_var: &str) -> String {
    command.replace("${PACKAGE_ROOT}", root_var)
}

/// Claude / Codex hook event names (the two share the vocabulary).
pub(crate) fn claude_event(event: super::model::HookEvent) -> &'static str {
    use super::model::HookEvent::*;
    match event {
        SessionStart => "SessionStart",
        PreTool => "PreToolUse",
        PostTool => "PostToolUse",
        PromptSubmit => "UserPromptSubmit",
        Stop => "Stop",
    }
}

/// `{"hooks": {"<Event>": [{"matcher"?, "hooks": [{"type":"command","command"}]}]}}`
pub(crate) fn claude_hooks_file(pkg: &PortableSkillPackage, root_var: &str) -> Value {
    let mut events: BTreeMap<&'static str, Vec<Value>> = BTreeMap::new();
    for hook in &pkg.hook_requirements {
        let mut group = serde_json::Map::new();
        if let Some(matcher) = &hook.matcher {
            group.insert("matcher".into(), Value::String(matcher.clone()));
        }
        group.insert(
            "hooks".into(),
            json!([{ "type": "command", "command": with_root(&hook.command, root_var) }]),
        );
        events
            .entry(claude_event(hook.event))
            .or_default()
            .push(Value::Object(group));
    }
    json!({ "hooks": events })
}

/// `{"<name>": {command,args,env} | {url}}` with env as `${NAME}` references.
pub(crate) fn mcp_servers(
    pkg: &PortableSkillPackage,
    typed: bool,
) -> serde_json::Map<String, Value> {
    let mut servers = serde_json::Map::new();
    for dep in &pkg.mcp_dependencies {
        let mut server = serde_json::Map::new();
        if let Some(command) = &dep.command {
            if typed {
                server.insert("type".into(), json!("stdio"));
            }
            server.insert("command".into(), json!(command));
            if !dep.args.is_empty() {
                server.insert("args".into(), json!(dep.args));
            }
            if !dep.env.is_empty() {
                let env: serde_json::Map<String, Value> = dep
                    .env
                    .iter()
                    .map(|name| (name.clone(), Value::String(format!("${{{name}}}"))))
                    .collect();
                server.insert("env".into(), Value::Object(env));
            }
        } else if let Some(url) = &dep.url {
            if typed {
                server.insert("type".into(), json!("streamable-http"));
            } else {
                server.insert("type".into(), json!("http"));
            }
            server.insert("url".into(), json!(url));
        }
        servers.insert(dep.name.clone(), Value::Object(server));
    }
    servers
}

/// Parse a JSON file out of a file map, recording a finding when it fails.
pub(crate) fn read_json(files: &FileMap, path: &str, findings: &mut Vec<Finding>) -> Option<Value> {
    let Some(bytes) = files.get(path) else {
        findings.push(error(path, "missing"));
        return None;
    };
    match serde_json::from_slice::<Value>(bytes) {
        Ok(value) => Some(value),
        Err(e) => {
            findings.push(error(path, &format!("not valid JSON: {e}")));
            None
        }
    }
}

pub(crate) fn error(path: &str, message: &str) -> Finding {
    Finding {
        path: path.to_string(),
        severity: Severity::Error,
        message: message.to_string(),
    }
}

pub(crate) fn warning(path: &str, message: &str) -> Finding {
    Finding {
        path: path.to_string(),
        severity: Severity::Warning,
        message: message.to_string(),
    }
}

/// Every `skills/<dir>/SKILL.md` must exist for each provenance member marked
/// exported, and carry a `name` + `description` frontmatter.
pub(crate) fn validate_skills(files: &FileMap, findings: &mut Vec<Finding>) {
    let skill_files: Vec<&String> = files
        .keys()
        .filter(|p| {
            p.starts_with("skills/") && p.ends_with("/SKILL.md") && p.matches('/').count() == 2
        })
        .collect();
    if skill_files.is_empty() {
        findings.push(warning("skills/", "the package carries no Skills"));
    }
    for path in skill_files {
        let text = String::from_utf8_lossy(&files[path]);
        if let Some(problem) = unquoted_colon_scalar(&text) {
            findings.push(error(path, &problem));
        }
        let front = frontmatter(&text);
        for key in ["name", "description"] {
            if !front.contains_key(key) {
                findings.push(error(path, &format!("SKILL.md frontmatter has no `{key}`")));
            }
        }
        if let Some(desc) = front.get("description") {
            if desc.chars().count() > 1024 {
                findings.push(warning(
                    path,
                    "description is longer than the 1024-character Agent Skills limit",
                ));
            }
        }
    }
    if let Some(bytes) = files.get(PROVENANCE_FILE) {
        if let Ok(prov) = serde_json::from_slice::<Value>(bytes) {
            for member in prov["members"].as_array().into_iter().flatten() {
                if member["exported"] == Value::Bool(true) {
                    let name = member["name"].as_str().unwrap_or_default();
                    let path = format!("skills/{name}/SKILL.md");
                    if !files.contains_key(&path) {
                        findings.push(error(
                            &path,
                            &format!(
                                "provenance names exported member {} but its SKILL.md is missing",
                                member["id"]
                            ),
                        ));
                    }
                }
            }
        } else {
            findings.push(error(PROVENANCE_FILE, "not valid JSON"));
        }
    } else {
        findings.push(error(PROVENANCE_FILE, "missing provenance file"));
    }
}

/// Minimal frontmatter reader: `key: value` lines between leading `---`.
pub fn frontmatter(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return out;
    }
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            out.insert(
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            );
        }
    }
    out
}

/// A plain (unquoted) YAML scalar containing `": "` is a parse error for
/// strict YAML hosts: pi drops the Skill entirely. `METHOD: …` descriptions
/// must be quoted.
pub fn unquoted_colon_scalar(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return None;
    }
    for line in lines {
        if line.trim() == "---" {
            return None;
        }
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            let quoted = value.starts_with(['"', '\'', '>', '|', '[', '{']);
            if !quoted && (value.contains(": ") || value.ends_with(':')) {
                return Some(format!(
                    "frontmatter `{}` is an unquoted YAML scalar containing `: `; strict YAML hosts \
                     reject it (pi does not load the Skill) — quote the value at the source",
                    key.trim()
                ));
            }
        }
    }
    None
}
