//! `openai`: Agent Plugins v1 (root `plugin.json` with the agent-plugins
//! `$schema`), plus the `codex` compatibility overlay
//! (`.codex-plugin/plugin.json`).
//!
//! Vendor facts (openai/codex @ 12fd929f, agent-plugins-spec 1.0.0):
//! the root manifest admits only `$schema, name, version, description, author,
//! homepage, repository, license, keywords, extensions`; components are fixed
//! (`./skills`, `./mcp.json` — no leading dot); hooks, interface and apps come
//! only from `extensions["com.openai"]` (legacy grammar), or — when that is
//! absent — from a `.codex-plugin/plugin.json` overlay. There is no Codex
//! validate command.

use std::path::Path;

use serde_json::{json, Value};

use crate::Result;

use super::model::PortableSkillPackage;
use super::target::*;

pub const AGENT_PLUGIN_SCHEMA_URI: &str =
    "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
pub const AGENT_PLUGIN_MCP_SCHEMA_URI: &str =
    "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";
pub const AGENT_PLUGIN_FIELDS: &[&str] = &[
    "$schema",
    "name",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "extensions",
];
const EXTENSION_NAMESPACE: &str = "com.openai";
const OVERLAY_PATH: &str = ".codex-plugin/plugin.json";
const HOOKS_PATH: &str = "hooks/hooks.json";
const MCP_PATH: &str = "mcp.json";
const ROOT_VAR: &str = "${PLUGIN_ROOT}";

pub struct OpenAiTarget {
    pub id: TargetId,
    pub codex_overlay: bool,
}

/// The Agent Plugins v1 name rule: 1–64 of `[a-z0-9.-]`, alphanumeric at both
/// ends, no `--` or `..`.
pub fn is_valid_agent_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.contains("--")
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name.as_bytes()[name.len() - 1].is_ascii_alphanumeric()
}

impl OpenAiTarget {
    fn interface(&self, pkg: &PortableSkillPackage) -> Value {
        let p = &pkg.presentation;
        let mut interface = serde_json::Map::new();
        interface.insert(
            "displayName".into(),
            json!(p
                .display_name
                .clone()
                .unwrap_or_else(|| pkg.identity.name.clone())),
        );
        interface.insert(
            "shortDescription".into(),
            json!(p
                .short_description
                .clone()
                .unwrap_or_else(|| pkg.description.clone())),
        );
        if let Some(long) = &p.long_description {
            interface.insert("longDescription".into(), json!(long));
        }
        let developer = p
            .developer_name
            .clone()
            .or_else(|| pkg.attribution.as_ref().map(|a| a.name.clone()));
        if let Some(developer) = developer {
            interface.insert("developerName".into(), json!(developer));
        }
        interface.insert(
            "category".into(),
            json!(p.category.clone().unwrap_or_else(|| "Other".to_string())),
        );
        if let Some(color) = &p.brand_color {
            interface.insert("brandColor".into(), json!(color));
        }
        if let Some(url) = p.website_url.clone().or_else(|| pkg.homepage.clone()) {
            interface.insert("websiteURL".into(), json!(url));
        }
        if !p.default_prompt.is_empty() {
            let prompts: Vec<String> = p
                .default_prompt
                .iter()
                .take(3)
                .map(|s| s.chars().take(128).collect())
                .collect();
            interface.insert("defaultPrompt".into(), json!(prompts));
        }
        Value::Object(interface)
    }

    fn manifest(&self, pkg: &PortableSkillPackage) -> Value {
        let mut manifest = serde_json::Map::new();
        manifest.insert("$schema".into(), json!(AGENT_PLUGIN_SCHEMA_URI));
        manifest.insert("name".into(), json!(pkg.identity.name));
        manifest.insert("version".into(), json!(pkg.version));
        manifest.insert("description".into(), json!(pkg.description));
        if let Some(author) = &pkg.attribution {
            manifest.insert(
                "author".into(),
                serde_json::to_value(author).expect("author"),
            );
        }
        if let Some(homepage) = &pkg.homepage {
            manifest.insert("homepage".into(), json!(homepage));
        }
        if let Some(repository) = &pkg.repository {
            manifest.insert("repository".into(), json!(repository));
        }
        if let Some(license) = &pkg.license {
            manifest.insert("license".into(), json!(license));
        }
        if !pkg.keywords.is_empty() {
            manifest.insert("keywords".into(), json!(pkg.keywords));
        }
        let mut extension = serde_json::Map::new();
        if !pkg.hook_requirements.is_empty() {
            extension.insert("hooks".into(), json!(format!("./{HOOKS_PATH}")));
        }
        if !pkg.presentation.is_empty() {
            extension.insert("interface".into(), self.interface(pkg));
        }
        if !extension.is_empty() {
            manifest.insert(
                "extensions".into(),
                json!({ EXTENSION_NAMESPACE: Value::Object(extension) }),
            );
        }
        let mut manifest = Value::Object(manifest);
        if let Some(overlay) = pkg.target_overlays.get("openai") {
            // Only Agent Plugins fields survive; the rest is planned unsupported.
            merge_json(&mut manifest, &allowed_overlay(overlay));
        }
        manifest
    }

    fn codex_overlay_manifest(&self, pkg: &PortableSkillPackage) -> Value {
        let mut overlay = serde_json::Map::new();
        overlay.insert("name".into(), json!(pkg.identity.name));
        overlay.insert("version".into(), json!(pkg.version));
        overlay.insert("description".into(), json!(pkg.description));
        if !pkg.keywords.is_empty() {
            overlay.insert("keywords".into(), json!(pkg.keywords));
        }
        overlay.insert("skills".into(), json!("./skills/"));
        if !pkg.mcp_dependencies.is_empty() {
            overlay.insert("mcpServers".into(), json!(format!("./{MCP_PATH}")));
        }
        if !pkg.hook_requirements.is_empty() {
            overlay.insert("hooks".into(), json!(format!("./{HOOKS_PATH}")));
        }
        overlay.insert("interface".into(), self.interface(pkg));
        let mut overlay = Value::Object(overlay);
        if let Some(extra) = pkg.target_overlays.get("codex") {
            merge_json(&mut overlay, extra);
        }
        overlay
    }
}

fn allowed_overlay(overlay: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(map) = overlay.as_object() {
        for (k, v) in map {
            if AGENT_PLUGIN_FIELDS.contains(&k.as_str()) && k != "$schema" && k != "name" {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(out)
}

impl PackageTarget for OpenAiTarget {
    fn id(&self) -> TargetId {
        self.id
    }

    fn format_version(&self) -> &'static str {
        "agent-plugins/1.0.0"
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            target: self.id,
            format: "Agent Plugins (OpenAI / Codex)".into(),
            format_version: self.format_version().into(),
            package_identity: "root plugin.json `name` (1–64 of [a-z0-9.-]) with $schema agent-plugins 1.0.0".into(),
            skills_location: "./skills/<name>/SKILL.md (fixed)".into(),
            mcp: "./mcp.json (fixed, no leading dot; $schema mcp 1.0.0, typed servers)".into(),
            hooks_and_extensions: if self.codex_overlay {
                "hooks/hooks.json referenced from extensions[\"com.openai\"].hooks and from the .codex-plugin/plugin.json overlay".into()
            } else {
                "hooks/hooks.json referenced from extensions[\"com.openai\"].hooks".into()
            },
            ui_contribution: "extensions[\"com.openai\"].interface (displayName, shortDescription, category, …); synthesised from name/description when absent".into(),
            install_discovery: "codex plugin marketplace add <dir> ; codex plugin add <name>@<marketplace>".into(),
            validation: "structural check against the Agent Plugins v1 field rules; optional disposable Codex marketplace load under a temporary CODEX_HOME (no codex validate command exists)".into(),
            no_analogue: vec![
                "user commands (no command component in Agent Plugins v1)".into(),
                "required host tools".into(),
                "package-level environment declarations".into(),
            ],
        }
    }

    fn plan(&self, pkg: &PortableSkillPackage) -> PackagePlan {
        let mut plan = PackagePlan::new(self, pkg);
        let mut manifest = PlanEntry::new("manifest", PlanClass::Translated)
            .at("plugin.json")
            .detail("identity, version, description, author, license, keywords → Agent Plugins v1 root manifest");
        if !is_valid_agent_plugin_name(&pkg.identity.name) {
            manifest = PlanEntry::unsupported(
                "manifest",
                format!(
                    "package name `{}` violates the Agent Plugins name rule",
                    pkg.identity.name
                ),
            );
        }
        plan.push(manifest);
        plan_members(&mut plan, pkg);
        for dep in &pkg.mcp_dependencies {
            plan.push(
                PlanEntry::new(format!("mcp:{}", dep.name), PlanClass::Translated)
                    .at(MCP_PATH)
                    .detail(if dep.env.is_empty() {
                        "typed Agent Plugins MCP server".to_string()
                    } else {
                        format!(
                            "typed Agent Plugins MCP server; env passed as ${{NAME}} references ({})",
                            dep.env.join(", ")
                        )
                    }),
            );
        }
        for hook in &pkg.hook_requirements {
            let mut entry = PlanEntry::new(
                format!("hook:{}", hook.event.as_str()),
                PlanClass::Translated,
            )
            .at(HOOKS_PATH)
            .detail(format!(
                "{} command hook via extensions[\"com.openai\"].hooks",
                claude_event(hook.event)
            ));
            if self.codex_overlay {
                entry = entry.at(OVERLAY_PATH);
            }
            plan.push(entry);
        }
        for command in &pkg.commands {
            plan.push(PlanEntry::unsupported(
                format!("command:{}", command.name),
                "Agent Plugins v1 has no command component (Codex would migrate a legacy command into a Skill the SkillSet does not carry)",
            ));
        }
        if !pkg.presentation.is_empty() {
            plan.push(
                PlanEntry::new("presentation", PlanClass::Translated)
                    .at("plugin.json")
                    .detail("extensions[\"com.openai\"].interface"),
            );
        }
        plan_requirements(&mut plan, pkg, "Agent Plugins");
        if let Some(overlay) = pkg.target_overlays.get("openai") {
            let rejected: Vec<String> = overlay
                .as_object()
                .map(|m| {
                    m.keys()
                        .filter(|k| {
                            !AGENT_PLUGIN_FIELDS.contains(&k.as_str())
                                || k.as_str() == "$schema"
                                || k.as_str() == "name"
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            plan.push(
                PlanEntry::new("overlay:openai", PlanClass::TargetAddition)
                    .at("plugin.json")
                    .detail("targets.openai merged into the root manifest"),
            );
            for key in rejected {
                plan.push(PlanEntry::unsupported(
                    format!("overlay:openai.{key}"),
                    "not an Agent Plugins v1 root field (or identity, which the SkillSet owns); additionalProperties is false",
                ));
            }
        }
        if self.codex_overlay {
            plan.push(
                PlanEntry::new("codex-overlay", PlanClass::TargetAddition)
                    .at(OVERLAY_PATH)
                    .detail("legacy .codex-plugin/plugin.json compatibility overlay with interface block; reuses ./skills, ./mcp.json and ./hooks/hooks.json"),
            );
            if pkg.target_overlays.contains_key("codex") {
                plan.push(
                    PlanEntry::new("overlay:codex", PlanClass::TargetAddition)
                        .at(OVERLAY_PATH)
                        .detail("targets.codex merged into the compatibility overlay"),
                );
            }
        }
        plan_provenance(&mut plan);
        plan
    }

    fn render(&self, pkg: &PortableSkillPackage, plan: &PackagePlan) -> Result<Vec<RenderedFile>> {
        let mut files = vec![RenderedFile::json("plugin.json", &self.manifest(pkg))];
        files.extend(render_members(pkg, plan)?);
        if !pkg.mcp_dependencies.is_empty() {
            files.push(RenderedFile::json(
                MCP_PATH,
                &json!({
                    "$schema": AGENT_PLUGIN_MCP_SCHEMA_URI,
                    "mcpServers": Value::Object(mcp_servers(pkg, true)),
                }),
            ));
        }
        if !pkg.hook_requirements.is_empty() {
            files.push(RenderedFile::json(
                HOOKS_PATH,
                &claude_hooks_file(pkg, ROOT_VAR),
            ));
        }
        if self.codex_overlay {
            files.push(RenderedFile::json(
                OVERLAY_PATH,
                &self.codex_overlay_manifest(pkg),
            ));
        }
        files.push(RenderedFile::json(PROVENANCE_FILE, &provenance(pkg, plan)));
        Ok(files)
    }

    fn native_validation(&self, _dir: &Path) -> Option<ValidationCommand> {
        None
    }

    fn structural_validation(&self, files: &FileMap) -> Vec<Finding> {
        let mut findings = Vec::new();
        if let Some(manifest) = read_json(files, "plugin.json", &mut findings) {
            validate_agent_plugin_manifest(&manifest, files, &mut findings);
        }
        if files.contains_key(MCP_PATH) {
            if let Some(mcp) = read_json(files, MCP_PATH, &mut findings) {
                validate_agent_plugin_mcp(&mcp, &mut findings);
            }
        }
        if files.contains_key(".mcp.json") {
            findings.push(warning(
                ".mcp.json",
                "Agent Plugins reads ./mcp.json (no leading dot); .mcp.json is ignored by the root manifest",
            ));
        }
        if files.contains_key(OVERLAY_PATH) {
            if let Some(overlay) = read_json(files, OVERLAY_PATH, &mut findings) {
                for key in ["skills", "mcpServers", "hooks"] {
                    if let Some(path) = overlay[key].as_str() {
                        check_relative_path(OVERLAY_PATH, key, path, files, &mut findings);
                    }
                }
            }
        } else if self.codex_overlay {
            findings.push(error(
                OVERLAY_PATH,
                "codex target requires the compatibility overlay",
            ));
        }
        validate_skills(files, &mut findings);
        findings
    }
}

fn check_relative_path(
    owner: &str,
    field: &str,
    path: &str,
    files: &FileMap,
    findings: &mut Vec<Finding>,
) {
    if !path.starts_with("./") || path.contains("../") {
        findings.push(error(
            owner,
            &format!("`{field}` path `{path}` must start with ./ and stay inside the plugin"),
        ));
        return;
    }
    let rel = path.trim_start_matches("./").trim_end_matches('/');
    let exists = files.contains_key(rel) || files.keys().any(|k| k.starts_with(&format!("{rel}/")));
    if !exists {
        findings.push(error(
            owner,
            &format!("`{field}` path `{path}` does not exist"),
        ));
    }
}

fn validate_agent_plugin_manifest(manifest: &Value, files: &FileMap, findings: &mut Vec<Finding>) {
    const P: &str = "plugin.json";
    let Some(object) = manifest.as_object() else {
        findings.push(error(P, "must be a JSON object"));
        return;
    };
    if manifest["$schema"].as_str() != Some(AGENT_PLUGIN_SCHEMA_URI) {
        findings.push(error(
            P,
            &format!("`$schema` must be `{AGENT_PLUGIN_SCHEMA_URI}`"),
        ));
    }
    match manifest["name"].as_str() {
        Some(name) if is_valid_agent_plugin_name(name) => {}
        Some(name) => findings.push(error(
            P,
            &format!("`name` `{name}` violates the Agent Plugins name rule"),
        )),
        None => findings.push(error(P, "`name` is required")),
    }
    for (key, value) in object {
        if !AGENT_PLUGIN_FIELDS.contains(&key.as_str()) {
            findings.push(error(
                P,
                &format!("`{key}` is not an Agent Plugins v1 field (additionalProperties: false)"),
            ));
        }
        if value.is_null() {
            findings.push(error(P, &format!("`{key}` must not be null")));
        }
    }
    for key in [
        "version",
        "description",
        "homepage",
        "repository",
        "license",
    ] {
        if object.get(key).is_some_and(|v| !v.is_string()) {
            findings.push(error(P, &format!("`{key}` must be a string")));
        }
    }
    if let Some(author) = object.get("author") {
        match author.as_object() {
            Some(author) => {
                for (k, v) in author {
                    if !["name", "email", "url"].contains(&k.as_str()) {
                        findings.push(error(P, &format!("`author.{k}` is not allowed")));
                    } else if !v.is_string() {
                        findings.push(error(P, &format!("`author.{k}` must be a string")));
                    }
                }
            }
            None => findings.push(error(P, "`author` must be an object")),
        }
    }
    if let Some(keywords) = object.get("keywords") {
        if !keywords
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_string))
        {
            findings.push(error(P, "`keywords` must be an array of strings"));
        }
    }
    if let Some(extensions) = object.get("extensions") {
        match extensions.as_object() {
            Some(map) => {
                for (ns, value) in map {
                    if !value.is_object() {
                        findings.push(error(P, &format!("`extensions.{ns}` must be an object")));
                    }
                }
                if let Some(hooks) = map
                    .get(EXTENSION_NAMESPACE)
                    .and_then(|e| e.get("hooks"))
                    .and_then(Value::as_str)
                {
                    check_relative_path(P, "extensions.com.openai.hooks", hooks, files, findings);
                }
            }
            None => findings.push(error(P, "`extensions` must be an object")),
        }
    }
}

fn validate_agent_plugin_mcp(mcp: &Value, findings: &mut Vec<Finding>) {
    const P: &str = MCP_PATH;
    if mcp["$schema"].as_str() != Some(AGENT_PLUGIN_MCP_SCHEMA_URI) {
        findings.push(error(
            P,
            &format!("`$schema` must be `{AGENT_PLUGIN_MCP_SCHEMA_URI}`"),
        ));
    }
    if let Some(object) = mcp.as_object() {
        for key in object.keys() {
            if key != "$schema" && key != "mcpServers" {
                findings.push(error(P, &format!("`{key}` is not allowed")));
            }
        }
    }
    let Some(servers) = mcp["mcpServers"].as_object() else {
        findings.push(error(P, "`mcpServers` object is required"));
        return;
    };
    for (name, server) in servers {
        let ty = server["type"].as_str().unwrap_or_default();
        let (required, allowed): (&str, &[&str]) = match ty {
            "stdio" => ("command", &["type", "command", "args", "env", "cwd"]),
            "streamable-http" | "sse" => ("url", &["type", "url", "headers"]),
            _ => {
                findings.push(error(
                    P,
                    &format!("server `{name}` needs `type` stdio | streamable-http | sse"),
                ));
                continue;
            }
        };
        if !server[required].as_str().is_some_and(|s| !s.is_empty()) {
            findings.push(error(P, &format!("server `{name}` needs `{required}`")));
        }
        for key in server.as_object().into_iter().flat_map(|o| o.keys()) {
            if !allowed.contains(&key.as_str()) {
                findings.push(error(
                    P,
                    &format!("server `{name}`: `{key}` is not allowed for {ty}"),
                ));
            }
        }
        for key in server["env"].as_object().into_iter().flat_map(|o| o.keys()) {
            if key == "PLUGIN_ROOT" || key == "PLUGIN_DATA" {
                findings.push(error(P, &format!("server `{name}`: env may not set {key}")));
            }
        }
    }
}
