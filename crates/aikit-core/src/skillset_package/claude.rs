//! `claude`: a Claude Code plugin (`.claude-plugin/plugin.json`).
//!
//! Vendor facts (Claude Code 2.1.263, plugins-reference): only `name`
//! (kebab-case) is required; `skills/`, `.mcp.json`, `hooks/hooks.json` are
//! default component locations; unknown manifest fields are errors under
//! `claude plugin validate --strict`. Commands and agents are generated only
//! when the package explicitly declares them.

use std::path::Path;

use serde_json::{json, Value};

use crate::Result;

use super::model::{is_kebab, PortableSkillPackage};
use super::target::*;

const MANIFEST: &str = ".claude-plugin/plugin.json";
const MCP_PATH: &str = ".mcp.json";
const HOOKS_PATH: &str = "hooks/hooks.json";
const ROOT_VAR: &str = "${CLAUDE_PLUGIN_ROOT}";

/// Top-level plugin.json fields Claude Code recognises.
pub const CLAUDE_MANIFEST_FIELDS: &[&str] = &[
    "name",
    "displayName",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "metadata",
    "defaultEnabled",
    "skills",
    "commands",
    "agents",
    "workflows",
    "outputStyles",
    "hooks",
    "mcpServers",
    "lspServers",
    "experimental",
    "userConfig",
    "channels",
    "dependencies",
];

pub struct ClaudeTarget;

impl ClaudeTarget {
    fn manifest(&self, pkg: &PortableSkillPackage) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("name".into(), json!(pkg.identity.name));
        if let Some(display) = &pkg.presentation.display_name {
            m.insert("displayName".into(), json!(display));
        }
        m.insert("version".into(), json!(pkg.version));
        m.insert("description".into(), json!(pkg.description));
        if let Some(author) = &pkg.attribution {
            m.insert(
                "author".into(),
                serde_json::to_value(author).expect("author"),
            );
        } else if let Some(developer) = &pkg.presentation.developer_name {
            m.insert("author".into(), json!({ "name": developer }));
        }
        if let Some(homepage) = pkg
            .homepage
            .clone()
            .or(pkg.presentation.website_url.clone())
        {
            m.insert("homepage".into(), json!(homepage));
        }
        if let Some(repository) = &pkg.repository {
            m.insert("repository".into(), json!(repository));
        }
        if let Some(license) = &pkg.license {
            m.insert("license".into(), json!(license));
        }
        if !pkg.keywords.is_empty() {
            m.insert("keywords".into(), json!(pkg.keywords));
        }
        let mut manifest = Value::Object(m);
        if let Some(overlay) = pkg.target_overlays.get("claude") {
            let mut allowed = serde_json::Map::new();
            for (k, v) in overlay.as_object().into_iter().flatten() {
                if CLAUDE_MANIFEST_FIELDS.contains(&k.as_str()) && k != "name" {
                    allowed.insert(k.clone(), v.clone());
                }
            }
            merge_json(&mut manifest, &Value::Object(allowed));
        }
        manifest
    }
}

impl PackageTarget for ClaudeTarget {
    fn id(&self) -> TargetId {
        TargetId::Claude
    }

    fn format_version(&self) -> &'static str {
        "claude-code-plugin/1"
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            target: TargetId::Claude,
            format: "Claude Code plugin".into(),
            format_version: self.format_version().into(),
            package_identity: ".claude-plugin/plugin.json `name` (kebab-case, required)".into(),
            skills_location: "skills/<name>/SKILL.md (default; `skills` adds paths)".into(),
            mcp: ".mcp.json {mcpServers} (stdio command/args/env or http url)".into(),
            hooks_and_extensions: "hooks/hooks.json {hooks: {<Event>: [{matcher?, hooks: [{type: command}]}]}}; commands/*.md and agents/*.md only when explicitly declared".into(),
            ui_contribution: "displayName in plugin.json; category/tags belong to a marketplace entry".into(),
            install_discovery: "claude plugin marketplace add <marketplace> ; claude plugin install <name>@<marketplace> ; or --plugin-dir <dir>".into(),
            validation: "claude plugin validate --strict --json <dir>".into(),
            no_analogue: vec![
                "required host tools".into(),
                "package-level environment declarations (userConfig is per-key user input, not a required env name)".into(),
                "presentation category / brand colour / default prompts".into(),
            ],
        }
    }

    fn plan(&self, pkg: &PortableSkillPackage) -> PackagePlan {
        let mut plan = PackagePlan::new(self, pkg);
        if pkg.attribution.is_none() && pkg.presentation.developer_name.is_none() {
            plan.push(PlanEntry::unsupported(
                "author",
                "no attribution declared; Claude strict validation requires plugin.json `author` — declare [package.author]",
            ));
        }
        plan.push(if is_kebab(&pkg.identity.name) {
            PlanEntry::new("manifest", PlanClass::Translated)
                .at(MANIFEST)
                .detail("name, version, description, author, license, keywords → plugin.json")
        } else {
            PlanEntry::unsupported("manifest", "Claude plugin names must be kebab-case")
        });
        plan_members(&mut plan, pkg);
        for dep in &pkg.mcp_dependencies {
            plan.push(
                PlanEntry::new(format!("mcp:{}", dep.name), PlanClass::Translated)
                    .at(MCP_PATH)
                    .detail(if dep.env.is_empty() {
                        "mcpServers entry".to_string()
                    } else {
                        format!(
                            "mcpServers entry; env expanded from ${{NAME}} ({})",
                            dep.env.join(", ")
                        )
                    }),
            );
        }
        for hook in &pkg.hook_requirements {
            plan.push(
                PlanEntry::new(
                    format!("hook:{}", hook.event.as_str()),
                    PlanClass::Translated,
                )
                .at(HOOKS_PATH)
                .detail(format!("{} command hook", claude_event(hook.event))),
            );
        }
        for command in &pkg.commands {
            plan.push(
                PlanEntry::new(
                    format!("command:{}", command.name),
                    PlanClass::TargetAddition,
                )
                .at(format!("commands/{}.md", command.name))
                .detail("explicitly declared command → Claude slash command"),
            );
        }
        let p = &pkg.presentation;
        if p.display_name.is_some() {
            plan.push(
                PlanEntry::new("presentation.display_name", PlanClass::Translated)
                    .at(MANIFEST)
                    .detail("displayName"),
            );
        }
        let unsupported_presentation = [
            ("short_description", p.short_description.is_some()),
            ("long_description", p.long_description.is_some()),
            ("category", p.category.is_some()),
            ("developer_name", p.developer_name.is_some()),
            ("brand_color", p.brand_color.is_some()),
            ("default_prompt", !p.default_prompt.is_empty()),
        ];
        for (field, present) in unsupported_presentation {
            if present {
                plan.push(PlanEntry::unsupported(
                    format!("presentation.{field}"),
                    "Claude plugin.json has no such field (category and tags live on a marketplace entry)",
                ));
            }
        }
        if p.website_url.is_some() && pkg.homepage.is_none() {
            plan.push(
                PlanEntry::new("presentation.website_url", PlanClass::Translated)
                    .at(MANIFEST)
                    .detail("homepage"),
            );
        }
        plan_requirements(&mut plan, pkg, "Claude Code");
        if let Some(overlay) = pkg.target_overlays.get("claude") {
            plan.push(
                PlanEntry::new("overlay:claude", PlanClass::TargetAddition)
                    .at(MANIFEST)
                    .detail("targets.claude merged into plugin.json"),
            );
            for key in overlay.as_object().into_iter().flat_map(|o| o.keys()) {
                if !CLAUDE_MANIFEST_FIELDS.contains(&key.as_str()) || key == "name" {
                    plan.push(PlanEntry::unsupported(
                        format!("overlay:claude.{key}"),
                        "not a Claude plugin.json field (or identity, which the SkillSet owns); strict validation would fail",
                    ));
                }
            }
        }
        plan_provenance(&mut plan);
        plan
    }

    fn render(&self, pkg: &PortableSkillPackage, plan: &PackagePlan) -> Result<Vec<RenderedFile>> {
        let mut files = vec![RenderedFile::json(MANIFEST, &self.manifest(pkg))];
        files.extend(render_members(pkg, plan)?);
        if !pkg.mcp_dependencies.is_empty() {
            files.push(RenderedFile::json(
                MCP_PATH,
                &json!({ "mcpServers": Value::Object(mcp_servers(pkg, false)) }),
            ));
        }
        if !pkg.hook_requirements.is_empty() {
            files.push(RenderedFile::json(
                HOOKS_PATH,
                &claude_hooks_file(pkg, ROOT_VAR),
            ));
        }
        for command in &pkg.commands {
            let description = if command.description.trim().is_empty() {
                format!("Run the {} package command", command.name)
            } else {
                command.description.trim().to_string()
            };
            let body = format!(
                "---\ndescription: {}\n---\n\nRun this command from the `{}` package with the Bash tool and report its result:\n\n```sh\n{}\n```\n",
                description.replace('\n', " "),
                pkg.identity.name,
                with_root(&command.command, ROOT_VAR)
            );
            files.push(RenderedFile::bytes(
                format!("commands/{}.md", command.name),
                body.into_bytes(),
            ));
        }
        files.push(RenderedFile::json(PROVENANCE_FILE, &provenance(pkg, plan)));
        Ok(files)
    }

    fn native_validation(&self, dir: &Path) -> Option<ValidationCommand> {
        Some(ValidationCommand {
            program: "claude".into(),
            args: vec![
                "plugin".into(),
                "validate".into(),
                "--strict".into(),
                "--json".into(),
                dir.display().to_string(),
            ],
            stdin: None,
            expectation: "exit 0 with {\"success\": true} and no strict errors".into(),
        })
    }

    fn structural_validation(&self, files: &FileMap) -> Vec<Finding> {
        let mut findings = Vec::new();
        if let Some(manifest) = read_json(files, MANIFEST, &mut findings) {
            match manifest["name"].as_str() {
                Some(name) if is_kebab(name) => {}
                Some(name) => findings.push(error(
                    MANIFEST,
                    &format!("`name` `{name}` must be kebab-case"),
                )),
                None => findings.push(error(MANIFEST, "`name` is required")),
            }
            for key in manifest.as_object().into_iter().flat_map(|o| o.keys()) {
                if !CLAUDE_MANIFEST_FIELDS.contains(&key.as_str()) {
                    findings.push(error(
                        MANIFEST,
                        &format!("unknown field `{key}` (fails --strict)"),
                    ));
                }
            }
            if manifest.get("author").is_none() {
                findings.push(error(
                    MANIFEST,
                    "no `author`: `claude plugin validate --strict` fails on the missing-author \
                     warning — declare [package.author] (or presentation.developer_name)",
                ));
            }
        }
        if files.contains_key(MCP_PATH) {
            if let Some(mcp) = read_json(files, MCP_PATH, &mut findings) {
                if !mcp["mcpServers"].is_object() {
                    findings.push(error(MCP_PATH, "`mcpServers` object is required"));
                }
            }
        }
        if files.contains_key(HOOKS_PATH) {
            if let Some(hooks) = read_json(files, HOOKS_PATH, &mut findings) {
                if !hooks["hooks"].is_object() {
                    findings.push(error(HOOKS_PATH, "`hooks` object is required"));
                }
            }
        }
        validate_skills(files, &mut findings);
        findings
    }
}
