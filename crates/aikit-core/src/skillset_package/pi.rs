//! `pi`: a pi package (`package.json` with a `pi` key and the `pi-package`
//! keyword).
//!
//! Vendor facts (@earendil-works/pi-coding-agent 0.84.4, docs/packages.md,
//! docs/extensions.md): Skills load from `pi.skills` directories; pi has no
//! MCP host; behaviour beyond Skills is an extension module
//! (`export default function (pi: ExtensionAPI)`), loaded by jiti without a
//! build step. Declared native tools retain their member-owned module bytes.
//! A separate extension is generated only for declared hooks or commands.

use std::path::Path;

use serde_json::{json, Value};

use crate::{AikitError, Result};

use super::model::{native_tool_module_path, HookEvent, PortableSkillPackage};
use super::target::*;

const MANIFEST: &str = "package.json";
pub const PI_CORE_PACKAGE: &str = "@earendil-works/pi-coding-agent";

pub struct PiTarget;

pub fn pi_event(event: HookEvent) -> &'static str {
    match event {
        HookEvent::SessionStart => "session_start",
        HookEvent::PreTool => "tool_call",
        HookEvent::PostTool => "tool_result",
        HookEvent::PromptSubmit => "input",
        HookEvent::Stop => "agent_end",
    }
}

fn extension_path(pkg: &PortableSkillPackage) -> String {
    format!("extensions/{}-aikit.ts", pkg.identity.name)
}

fn needs_extension(pkg: &PortableSkillPackage) -> bool {
    !pkg.hook_requirements.is_empty() || !pkg.commands.is_empty()
}

fn ts_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialises")
}

impl PiTarget {
    fn manifest(&self, pkg: &PortableSkillPackage) -> Result<Value> {
        let mut m = serde_json::Map::new();
        m.insert("name".into(), json!(pkg.identity.name));
        m.insert("version".into(), json!(pkg.version));
        m.insert("description".into(), json!(pkg.description));
        let mut keywords = vec!["pi-package".to_string()];
        keywords.extend(pkg.keywords.iter().filter(|k| *k != "pi-package").cloned());
        m.insert("keywords".into(), json!(keywords));
        if let Some(license) = &pkg.license {
            m.insert("license".into(), json!(license));
        }
        if let Some(author) = &pkg.attribution {
            m.insert(
                "author".into(),
                serde_json::to_value(author).expect("author"),
            );
        }
        if let Some(homepage) = &pkg.homepage {
            m.insert("homepage".into(), json!(homepage));
        }
        if let Some(repository) = &pkg.repository {
            m.insert("repository".into(), json!(repository));
        }
        let mut pi = serde_json::Map::new();
        pi.insert("skills".into(), json!(["./skills"]));
        if needs_extension(pkg) {
            pi.insert(
                "extensions".into(),
                json!([format!("./{}", extension_path(pkg))]),
            );
            m.insert("peerDependencies".into(), json!({ PI_CORE_PACKAGE: "*" }));
        }
        m.insert("pi".into(), Value::Object(pi));
        let mut manifest = Value::Object(m);
        if let Some(overlay) = pkg.target_overlays.get("pi") {
            let mut overlay = overlay.clone();
            if let Some(o) = overlay.as_object_mut() {
                o.remove("name");
            }
            merge_json(&mut manifest, &overlay);
        }
        let mut required = std::collections::BTreeSet::new();
        if needs_extension(pkg) {
            required.insert(format!("./{}", extension_path(pkg)));
        }
        for tool in pkg.native_tools.iter().filter(|t| t.target == TargetId::Pi) {
            required.insert(format!("./{}", native_tool_module_path(pkg, tool)?));
        }
        if !required.is_empty() {
            let pi = manifest
                .get_mut("pi")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    AikitError::new(
                        "skillset.package.malformed",
                        "targets.pi must preserve the pi object",
                    )
                })?;
            let existing = pi.entry("extensions").or_insert_with(|| json!([]));
            let extensions = existing.as_array_mut().ok_or_else(|| {
                AikitError::new(
                    "skillset.package.malformed",
                    "pi.extensions must be an array",
                )
            })?;
            if extensions.iter().any(|v| !v.is_string()) {
                return Err(AikitError::new(
                    "skillset.package.malformed",
                    "pi.extensions paths must be strings",
                ));
            }
            for path in required {
                if !extensions.iter().any(|v| {
                    v.as_str().is_some_and(|p| {
                        p.trim_start_matches("./") == path.trim_start_matches("./")
                    })
                }) {
                    extensions.push(json!(path));
                }
            }
            let peers = manifest
                .as_object_mut()
                .expect("manifest object")
                .entry("peerDependencies")
                .or_insert_with(|| json!({}));
            let peers = peers.as_object_mut().ok_or_else(|| {
                AikitError::new(
                    "skillset.package.malformed",
                    "peerDependencies must be an object",
                )
            })?;
            peers.entry(PI_CORE_PACKAGE).or_insert_with(|| json!("*"));
        }
        Ok(manifest)
    }

    fn extension(&self, pkg: &PortableSkillPackage) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "// Generated by {GENERATOR} from SkillSet `{}` (source {}).\n\
             // TARGET ADDITION: the SkillSet's declared hook requirements and commands,\n\
             // expressed as a pi extension. The Skills themselves ship under ../skills and\n\
             // are never folded into this file. Edit the SkillSet, then re-export.\n",
            pkg.skillset_ref, pkg.source_revision
        ));
        out.push_str(&format!(
            "import type {{ ExtensionAPI }} from \"{PI_CORE_PACKAGE}\";\n\
             import {{ dirname, resolve }} from \"node:path\";\n\
             import {{ fileURLToPath }} from \"node:url\";\n\n\
             const PACKAGE_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), \"..\");\n\n\
             function run(pi: ExtensionAPI, command: string) {{\n\
             \x20 // ${{PACKAGE_ROOT}} in a declared command resolves to this package's root.\n\
             \x20 return pi.exec(\"sh\", [\"-c\", `PACKAGE_ROOT=\"$1\"; ${{command}}`, \"sh\", PACKAGE_ROOT]);\n\
             }}\n\n\
             export default function (pi: ExtensionAPI) {{\n"
        ));
        for hook in &pkg.hook_requirements {
            let event = pi_event(hook.event);
            let purpose = if hook.purpose.is_empty() {
                String::new()
            } else {
                format!("  // {}\n", hook.purpose.replace('\n', " "))
            };
            out.push_str(&purpose);
            let command = ts_string(&hook.command);
            match hook.event {
                HookEvent::PreTool => {
                    let guard = hook
                        .matcher
                        .as_ref()
                        .map(|m| {
                            format!(
                                "    if (!new RegExp({}, \"i\").test(event.toolName)) return;\n",
                                ts_string(&format!("^(?:{m})$"))
                            )
                        })
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "  pi.on(\"{event}\", async (event) => {{\n{guard}    const result = await run(pi, {command});\n\
                         \x20   // Exit code 2 blocks, as a Claude/Codex PreToolUse hook would.\n\
                         \x20   if (result.code === 2) return {{ block: true, reason: result.stderr || {} }};\n\
                         \x20 }});\n",
                        ts_string(&format!("blocked by the {} pre-tool hook", pkg.identity.name))
                    ));
                }
                HookEvent::PostTool => {
                    let guard = hook
                        .matcher
                        .as_ref()
                        .map(|m| {
                            format!(
                                "    if (!new RegExp({}, \"i\").test(event.toolName)) return;\n",
                                ts_string(&format!("^(?:{m})$"))
                            )
                        })
                        .unwrap_or_default();
                    out.push_str(&format!(
                        "  pi.on(\"{event}\", async (event) => {{\n{guard}    await run(pi, {command});\n  }});\n"
                    ));
                }
                _ => {
                    out.push_str(&format!(
                        "  pi.on(\"{event}\", async () => {{\n    await run(pi, {command});\n  }});\n"
                    ));
                }
            }
        }
        for command in &pkg.commands {
            let description = if command.description.trim().is_empty() {
                format!("Run the {} package command", command.name)
            } else {
                command.description.trim().to_string()
            };
            out.push_str(&format!(
                "  pi.registerCommand({}, {{\n\
                 \x20   description: {},\n\
                 \x20   handler: async (_args, ctx) => {{\n\
                 \x20     const result = await run(pi, {});\n\
                 \x20     ctx.ui.notify((result.stdout || result.stderr || `exit ${{result.code}}`).trim(), result.code === 0 ? \"info\" : \"error\");\n\
                 \x20   }},\n\
                 \x20 }});\n",
                ts_string(&command.name),
                ts_string(&description),
                ts_string(&command.command)
            ));
        }
        out.push_str("}\n");
        out
    }
}

impl PackageTarget for PiTarget {
    fn id(&self) -> TargetId {
        TargetId::Pi
    }

    fn format_version(&self) -> &'static str {
        "pi-package/1"
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            target: TargetId::Pi,
            format: "pi package".into(),
            format_version: self.format_version().into(),
            package_identity: "package.json `name` + `version`, keyword `pi-package`".into(),
            skills_location: "skills/<name>/SKILL.md via pi.skills [\"./skills\"]".into(),
            mcp: "none — pi has no MCP host".into(),
            hooks_and_extensions: "TypeScript extension (export default function (pi: ExtensionAPI)) for declared hooks/commands; member-owned native tool modules through pi.extensions".into(),
            ui_contribution: "none beyond package.json description/keywords".into(),
            install_discovery: "pi install <path|npm:…|git:…> ; pi -e <dir> for a temporary load".into(),
            validation: "pi --mode rpc --no-session --offline --no-approve -e <dir> ← get_commands lists skill:<name> for every member".into(),
            no_analogue: vec![
                "MCP servers".into(),
                "presentation metadata".into(),
                "required host tools".into(),
                "package-level environment declarations".into(),
            ],
        }
    }

    fn plan(&self, pkg: &PortableSkillPackage) -> PackagePlan {
        let mut plan = PackagePlan::new(self, pkg);
        plan.push(
            PlanEntry::new("manifest", PlanClass::Translated)
                .at(MANIFEST)
                .detail("name, version, description, license, author → package.json with pi.skills and keyword pi-package"),
        );
        plan_members(&mut plan, pkg);
        for dep in &pkg.mcp_dependencies {
            plan.push(PlanEntry::unsupported(
                format!("mcp:{}", dep.name),
                "pi has no MCP host",
            ));
        }
        for tool in pkg.native_tools.iter().filter(|t| t.target == TargetId::Pi) {
            match native_tool_module_path(pkg, tool) {
                Ok(path) if plan.carried_members().contains(&tool.member_id) => {
                    plan.push(PlanEntry::new(format!("native-tool:pi:{}", tool.name), PlanClass::Translated)
                        .at(path).detail("member-owned native Pi module admitted through pi.extensions; no shell translation"));
                }
                Ok(_) => plan.push(PlanEntry::unsupported(
                    format!("native-tool:pi:{}", tool.name),
                    "the owning member is not carried by this target",
                )),
                Err(e) => plan.push(PlanEntry::unsupported(
                    format!("native-tool:pi:{}", tool.name),
                    e.to_string(),
                )),
            }
        }
        let ext = extension_path(pkg);
        for hook in &pkg.hook_requirements {
            plan.push(
                PlanEntry::new(
                    format!("hook:{}", hook.event.as_str()),
                    PlanClass::TargetAddition,
                )
                .at(ext.clone())
                .detail(format!(
                    "pi.on(\"{}\") shelling to the declared command",
                    pi_event(hook.event)
                )),
            );
        }
        for command in &pkg.commands {
            plan.push(
                PlanEntry::new(
                    format!("command:{}", command.name),
                    PlanClass::TargetAddition,
                )
                .at(ext.clone())
                .detail("pi.registerCommand shelling to the declared command"),
            );
        }
        if !pkg.presentation.is_empty() {
            plan.push(PlanEntry::unsupported(
                "presentation",
                "pi packages carry no presentation metadata beyond package.json description",
            ));
        }
        plan_requirements(&mut plan, pkg, "pi");
        if pkg.target_overlays.contains_key("pi") {
            plan.push(
                PlanEntry::new("overlay:pi", PlanClass::TargetAddition)
                    .at(MANIFEST)
                    .detail("targets.pi merged into package.json"),
            );
        }
        plan_provenance(&mut plan);
        plan
    }

    fn render(&self, pkg: &PortableSkillPackage, plan: &PackagePlan) -> Result<Vec<RenderedFile>> {
        for tool in pkg.native_tools.iter().filter(|t| t.target == TargetId::Pi) {
            native_tool_module_path(pkg, tool)?;
            if !plan.carried_members().contains(&tool.member_id) {
                return Err(AikitError::new(
                    "skillset.package.missing_native_tool",
                    format!("native tool `{}` owner is not exported", tool.name),
                ));
            }
        }
        let mut files = vec![RenderedFile::json(MANIFEST, &self.manifest(pkg)?)];
        files.extend(render_members(pkg, plan)?);
        if needs_extension(pkg) {
            files.push(RenderedFile::bytes(
                extension_path(pkg),
                self.extension(pkg).into_bytes(),
            ));
        }
        files.push(RenderedFile::json(PROVENANCE_FILE, &provenance(pkg, plan)));
        Ok(files)
    }

    fn native_validation(&self, dir: &Path) -> Option<ValidationCommand> {
        Some(ValidationCommand {
            program: "pi".into(),
            args: vec![
                "--mode".into(),
                "rpc".into(),
                "--no-session".into(),
                "--offline".into(),
                "--no-approve".into(),
                "-e".into(),
                dir.display().to_string(),
            ],
            stdin: Some("{\"id\":\"1\",\"type\":\"get_commands\"}\n".into()),
            expectation: "get_commands response lists skill:<name> sourced from this package for every exported member".into(),
        })
    }

    fn structural_validation(&self, files: &FileMap) -> Vec<Finding> {
        let mut findings = Vec::new();
        if let Some(manifest) = read_json(files, MANIFEST, &mut findings) {
            if !manifest["name"].as_str().is_some_and(|n| !n.is_empty()) {
                findings.push(error(MANIFEST, "`name` is required"));
            }
            if !manifest["version"].is_string() {
                findings.push(error(MANIFEST, "`version` is required"));
            }
            if !manifest["keywords"]
                .as_array()
                .is_some_and(|k| k.iter().any(|v| v == "pi-package"))
            {
                findings.push(warning(
                    MANIFEST,
                    "keyword `pi-package` missing (gallery discovery)",
                ));
            }
            match manifest["pi"].as_object() {
                Some(pi) => {
                    for key in ["skills", "extensions"] {
                        for path in pi.get(key).and_then(Value::as_array).into_iter().flatten() {
                            let path = path.as_str().unwrap_or_default();
                            let rel = path.trim_start_matches("./").trim_end_matches('/');
                            let exists = files.contains_key(rel)
                                || files.keys().any(|k| k.starts_with(&format!("{rel}/")));
                            if !exists {
                                findings.push(error(
                                    MANIFEST,
                                    &format!("pi.{key} path `{path}` does not exist"),
                                ));
                            }
                        }
                    }
                    if pi.contains_key("extensions")
                        && !manifest["peerDependencies"][PI_CORE_PACKAGE].is_string()
                    {
                        findings.push(warning(
                            MANIFEST,
                            "extensions import pi core; declare it as a peerDependency \"*\"",
                        ));
                    }
                }
                None => findings.push(error(MANIFEST, "`pi` key is required")),
            }
        }
        validate_skills(files, &mut findings);
        findings
    }
}
