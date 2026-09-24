//! Portable SkillSet packages (PRAXIS-ARCHITECTURE §5).
//!
//! ```text
//! native SkillSet (+ neutral [package] metadata)
//!       ↓  resolve exact member capsules + content revisions   (caller: I/O)
//! PortableSkillPackage          aikit.portable-skill-package/v1
//!       ↓  PackageTarget adapter
//! PackagePlan                   portable | translated | target-addition | unsupported
//!       ↓  render (pure)
//! Vec<RenderedFile> + Receipt   aikit.skillset-package-receipt/v1
//! ```
//!
//! The native SkillSet is the authoritative source; a provider package is a
//! target projection of it and never a new source SkillSet. This module is
//! pure: resolving payload files, writing trees and running native validators
//! belong to the CLI (`aikit set package`).

pub mod claude;
pub mod digest;
pub mod model;
pub mod openai;
pub mod pi;
pub mod receipt;
pub mod target;

pub use digest::sha256_hex;
pub use model::{
    source_revision, Attribution, CommandContribution, EnvironmentRequirement, HookEvent,
    HookRequirement, McpDependency, PackageFile, PackageIdentity, PackageMember, PackageMetadata,
    PackageSource, PortableSkillPackage, Presentation, UnresolvedMember, PORTABLE_PACKAGE_SCHEMA,
};
pub use receipt::{
    diff_provenance, CheckStatus, Discovery, NativeValidation, PackageDiff, Receipt, Validation,
    RECEIPT_SCHEMA,
};
pub use target::{
    provenance, target_for, FileMap, Finding, PackagePlan, PackageTarget, PlanClass, PlanEntry,
    RenderedContent, RenderedFile, Severity, TargetCapabilities, TargetId, ValidationCommand,
    PROVENANCE_FILE,
};

/// Plan and render in one step.
pub fn plan_and_render(
    target: &dyn PackageTarget,
    pkg: &PortableSkillPackage,
) -> crate::Result<(PackagePlan, Vec<RenderedFile>)> {
    let plan = target.plan(pkg);
    let files = target.render(pkg, &plan)?;
    Ok((plan, files))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::method::PraxisForm;
    use serde_json::Value;

    fn member(id: &str, name: &str, description: &str, revision: &str) -> PackageMember {
        let skill = format!("---\nname: {name}\ndescription: \"{description}\"\n---\n\n# {name}\n");
        PackageMember {
            id: id.to_string(),
            form: PraxisForm::Skill,
            name: name.to_string(),
            description: description.to_string(),
            revision: revision.to_string(),
            tools: Vec::new(),
            files: vec![
                PackageFile::inline("SKILL.md", skill.into_bytes()),
                PackageFile::inline("references/detail.md", b"# detail\n".to_vec()),
            ],
        }
    }

    fn source(metadata: Option<PackageMetadata>) -> PackageSource {
        PackageSource {
            skillset_ref: "demo-set".into(),
            set_name: "Demo Set".into(),
            set_description: "A demo repertoire.".into(),
            metadata,
            members: vec![
                member("skill/demo/plain", "demo-plain", "Plain skill.", "rev-a"),
                member(
                    "skill/demo/method",
                    "demo-method",
                    "METHOD: Do the demo thing.",
                    "rev-b",
                ),
                member(
                    "skill/demo/field",
                    "demo-field",
                    "METHODOLOGY: Orient in the demo field.",
                    "rev-c",
                ),
            ],
            unresolved: Vec::new(),
        }
    }

    fn pkg(metadata: Option<PackageMetadata>) -> PortableSkillPackage {
        let mut metadata = metadata.unwrap_or_default();
        if metadata.author.is_none() {
            metadata.author = Some(Attribution {
                name: "Demo Author".into(),
                email: None,
                url: None,
            });
        }
        PortableSkillPackage::build(source(Some(metadata))).unwrap()
    }

    #[test]
    fn claude_without_author_is_planned_and_fails_structure() {
        let p = PortableSkillPackage::build(source(None)).unwrap();
        let t = target_for(TargetId::Claude, false);
        let (plan, rendered) = plan_and_render(t.as_ref(), &p).unwrap();
        assert!(matches!(
            plan.has("author").unwrap().class,
            PlanClass::Unsupported { .. }
        ));
        assert!(!errors(&t.structural_validation(&files_of(&rendered))).is_empty());
    }

    fn with_mcp() -> PackageMetadata {
        PackageMetadata {
            mcp: vec![McpDependency {
                name: "demo-server".into(),
                command: Some("npx".into()),
                args: vec!["-y".into(), "@demo/server".into()],
                url: None,
                env: vec!["DEMO_TOKEN".into()],
            }],
            environment: vec![EnvironmentRequirement {
                name: "DEMO_TOKEN".into(),
                purpose: "server auth".into(),
                required: true,
            }],
            ..Default::default()
        }
    }

    fn with_hooks() -> PackageMetadata {
        PackageMetadata {
            hooks: vec![
                HookRequirement {
                    event: HookEvent::SessionStart,
                    purpose: "announce the repertoire".into(),
                    command: "echo ready".into(),
                    matcher: None,
                },
                HookRequirement {
                    event: HookEvent::PreTool,
                    purpose: "guard writes".into(),
                    command: "${PACKAGE_ROOT}/skills/demo-plain/scripts/guard.sh".into(),
                    matcher: Some("Write|Edit".into()),
                },
            ],
            ..Default::default()
        }
    }

    fn combined() -> PackageMetadata {
        let mut m = with_mcp();
        m.hooks = with_hooks().hooks;
        m.commands = vec![CommandContribution {
            name: "demo-run".into(),
            description: "Run the demo".into(),
            command: "echo run".into(),
        }];
        m.presentation = Some(Presentation {
            display_name: Some("Demo".into()),
            category: Some("Productivity".into()),
            ..Default::default()
        });
        m.author = Some(Attribution {
            name: "Demo Author".into(),
            email: None,
            url: None,
        });
        m.license = Some("MIT".into());
        m
    }

    fn files_of(rendered: &[RenderedFile]) -> FileMap {
        rendered
            .iter()
            .map(|f| match &f.content {
                RenderedContent::Bytes(b) => (f.path.clone(), b.clone()),
                RenderedContent::Copy { .. } => panic!("inline fixtures only"),
            })
            .collect()
    }

    fn json_of(files: &FileMap, path: &str) -> Value {
        serde_json::from_slice(files.get(path).unwrap_or_else(|| panic!("{path} rendered")))
            .unwrap()
    }

    fn errors(findings: &[Finding]) -> Vec<&Finding> {
        findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .collect()
    }

    fn render(id: TargetId, overlay: bool, p: &PortableSkillPackage) -> (PackagePlan, FileMap) {
        let target = target_for(id, overlay);
        let (plan, rendered) = plan_and_render(target.as_ref(), p).unwrap();
        let files = files_of(&rendered);
        let findings = target.structural_validation(&files);
        assert!(
            errors(&findings).is_empty(),
            "{id:?} structural errors: {findings:#?}"
        );
        (plan, files)
    }

    #[test]
    fn forms_are_read_from_descriptions() {
        let p = pkg(None);
        let forms: Vec<_> = p
            .members
            .iter()
            .map(|m| (m.name.as_str(), m.form))
            .collect();
        assert!(forms.contains(&("demo-plain", PraxisForm::Skill)));
        assert!(forms.contains(&("demo-method", PraxisForm::Method)));
        assert!(forms.contains(&("demo-field", PraxisForm::Methodology)));
        assert_eq!(p.identity.name, "demo-set");
        assert_eq!(p.version, "0.1.0");
    }

    #[test]
    fn skills_only_every_target() {
        let p = pkg(None);
        for id in TargetId::ALL {
            let (plan, files) = render(id, false, &p);
            assert_eq!(plan.of_class("portable").len(), 3, "{id:?}");
            assert!(plan.of_class("unsupported").is_empty(), "{id:?}: {plan:#?}");
            for name in ["demo-plain", "demo-method", "demo-field"] {
                assert!(files.contains_key(&format!("skills/{name}/SKILL.md")));
                assert!(files.contains_key(&format!("skills/{name}/references/detail.md")));
            }
            let prov = json_of(&files, PROVENANCE_FILE);
            assert_eq!(prov["source_revision"], p.source_revision);
            assert_eq!(prov["members"].as_array().unwrap().len(), 3);
            assert!(!files.keys().any(|k| k.starts_with("commands/")
                || k.starts_with("agents/")
                || k.starts_with("extensions/")));
        }
        let (_, claude) = render(TargetId::Claude, false, &p);
        assert_eq!(
            json_of(&claude, ".claude-plugin/plugin.json")["name"],
            "demo-set"
        );
        let (_, openai) = render(TargetId::Openai, false, &p);
        let manifest = json_of(&openai, "plugin.json");
        assert_eq!(manifest["$schema"], openai::AGENT_PLUGIN_SCHEMA_URI);
        assert!(manifest.get("extensions").is_none());
        assert!(!openai.contains_key(".codex-plugin/plugin.json"));
        let (_, pi) = render(TargetId::Pi, false, &p);
        let package = json_of(&pi, "package.json");
        assert_eq!(package["pi"]["skills"][0], "./skills");
        assert!(package["pi"].get("extensions").is_none());
        assert_eq!(package["keywords"][0], "pi-package");
    }

    #[test]
    fn skills_and_mcp() {
        let p = pkg(Some(with_mcp()));
        let (plan, openai) = render(TargetId::Openai, false, &p);
        assert_eq!(
            plan.has("mcp:demo-server").unwrap().class,
            PlanClass::Translated
        );
        let mcp = json_of(&openai, "mcp.json");
        assert_eq!(mcp["$schema"], openai::AGENT_PLUGIN_MCP_SCHEMA_URI);
        assert_eq!(mcp["mcpServers"]["demo-server"]["type"], "stdio");
        assert_eq!(
            mcp["mcpServers"]["demo-server"]["env"]["DEMO_TOKEN"],
            "${DEMO_TOKEN}"
        );
        assert!(!openai.contains_key(".mcp.json"));

        let (_, claude) = render(TargetId::Claude, false, &p);
        let mcp = json_of(&claude, ".mcp.json");
        assert_eq!(mcp["mcpServers"]["demo-server"]["command"], "npx");

        let (plan, pi) = render(TargetId::Pi, false, &p);
        match &plan.has("mcp:demo-server").expect("never dropped").class {
            PlanClass::Unsupported { reason } => assert!(reason.contains("no MCP host")),
            other => panic!("pi MCP must be unsupported, got {other:?}"),
        }
        assert!(!pi.keys().any(|k| k.contains("mcp")));
        // Environment names survive in provenance; never values.
        let prov = json_of(&pi, PROVENANCE_FILE);
        assert_eq!(prov["environment"][0]["name"], "DEMO_TOKEN");
        assert!(plan.has("environment:DEMO_TOKEN").is_some());
    }

    #[test]
    fn skills_and_hooks() {
        let p = pkg(Some(with_hooks()));
        let (plan, claude) = render(TargetId::Claude, false, &p);
        assert_eq!(
            plan.has("hook:session-start").unwrap().class,
            PlanClass::Translated
        );
        let hooks = json_of(&claude, "hooks/hooks.json");
        assert_eq!(
            hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "echo ready"
        );
        assert_eq!(hooks["hooks"]["PreToolUse"][0]["matcher"], "Write|Edit");
        assert!(hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .starts_with("${CLAUDE_PLUGIN_ROOT}/"));

        let (_, openai) = render(TargetId::Openai, false, &p);
        let manifest = json_of(&openai, "plugin.json");
        assert_eq!(
            manifest["extensions"]["com.openai"]["hooks"],
            "./hooks/hooks.json"
        );
        let hooks = json_of(&openai, "hooks/hooks.json");
        assert!(hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .starts_with("${PLUGIN_ROOT}/"));

        let (plan, pi) = render(TargetId::Pi, false, &p);
        let entry = plan.has("hook:session-start").unwrap();
        assert_eq!(entry.class, PlanClass::TargetAddition);
        assert_eq!(
            entry.paths,
            vec!["extensions/demo-set-aikit.ts".to_string()]
        );
        let ext = String::from_utf8(pi["extensions/demo-set-aikit.ts"].clone()).unwrap();
        assert!(ext.contains("export default function (pi: ExtensionAPI)"));
        assert!(ext.contains("pi.on(\"session_start\""));
        assert!(ext.contains("pi.on(\"tool_call\""));
        assert!(!ext.contains("SKILL.md"), "Skills never become extensions");
        let package = json_of(&pi, "package.json");
        assert_eq!(
            package["pi"]["extensions"][0],
            "./extensions/demo-set-aikit.ts"
        );
        assert_eq!(package["peerDependencies"][pi::PI_CORE_PACKAGE], "*");
    }

    #[test]
    fn combined_and_codex_overlay() {
        let p = pkg(Some(combined()));
        let (plan, codex) = render(TargetId::Codex, true, &p);
        assert!(codex.contains_key(".codex-plugin/plugin.json"));
        let overlay = json_of(&codex, ".codex-plugin/plugin.json");
        assert_eq!(overlay["interface"]["displayName"], "Demo");
        assert_eq!(overlay["interface"]["category"], "Productivity");
        assert_eq!(overlay["mcpServers"], "./mcp.json");
        let manifest = json_of(&codex, "plugin.json");
        assert_eq!(
            manifest["extensions"]["com.openai"]["interface"]["displayName"],
            "Demo"
        );
        assert!(matches!(
            plan.has("command:demo-run").unwrap().class,
            PlanClass::Unsupported { .. }
        ));
        // openai with --with-codex-overlay renders the same tree.
        let (_, openai) = render(TargetId::Openai, true, &p);
        assert_eq!(
            openai.keys().collect::<Vec<_>>(),
            codex.keys().collect::<Vec<_>>()
        );

        let (plan, claude) = render(TargetId::Claude, false, &p);
        assert!(claude.contains_key("commands/demo-run.md"));
        assert_eq!(
            plan.has("command:demo-run").unwrap().class,
            PlanClass::TargetAddition
        );
        assert!(matches!(
            plan.has("presentation.category").unwrap().class,
            PlanClass::Unsupported { .. }
        ));
        assert_eq!(
            json_of(&claude, ".claude-plugin/plugin.json")["displayName"],
            "Demo"
        );

        let (plan, pi) = render(TargetId::Pi, false, &p);
        let ext = String::from_utf8(pi["extensions/demo-set-aikit.ts"].clone()).unwrap();
        assert!(ext.contains("pi.registerCommand(\"demo-run\""));
        let receipt = Receipt::new(&p, &plan, &[], Validation::from_findings(vec![]));
        assert!(receipt
            .unsupported
            .iter()
            .any(|u| u.relation == "mcp:demo-server"));
        assert!(receipt
            .target_additions
            .iter()
            .any(|u| u.relation == "command:demo-run"));
    }

    #[test]
    fn unresolved_members_are_planned_unsupported() {
        let mut s = source(None);
        s.unresolved.push(UnresolvedMember {
            id: "skill/demo/missing".into(),
            reason: "not in the resolved catalogue".into(),
        });
        let p = PortableSkillPackage::build(s).unwrap();
        assert!(!p.is_complete());
        for id in TargetId::ALL {
            let plan = target_for(id, false).plan(&p);
            let entry = plan
                .has("member:skill/demo/missing")
                .expect("never dropped");
            assert!(
                matches!(&entry.class, PlanClass::Unsupported { reason } if reason.contains("unresolved"))
            );
        }
    }

    #[test]
    fn secret_like_values_are_rejected() {
        let mut bad_env = PackageMetadata::default();
        bad_env.environment.push(EnvironmentRequirement {
            name: "OPENAI_API_KEY=sk-live-abcdefghijklmnopqrstuvwxyz".into(),
            purpose: String::new(),
            required: true,
        });
        let err = PortableSkillPackage::build(source(Some(bad_env))).unwrap_err();
        assert_eq!(err.code(), "skillset.package.secret_value");
        assert!(
            !err.to_string().contains("abcdefghijklmnop"),
            "the value is never echoed"
        );

        let mut bad_arg = with_mcp();
        bad_arg.mcp[0]
            .args
            .push("--token=ghp_abcdefghijklmnopqrstuvwxyz0123456789".into());
        assert_eq!(
            PortableSkillPackage::build(source(Some(bad_arg)))
                .unwrap_err()
                .code(),
            "skillset.package.secret_value"
        );

        let mut bad_hook = with_hooks();
        bad_hook.hooks[0].command = "API_TOKEN=abc123 ./run.sh".into();
        assert_eq!(
            PortableSkillPackage::build(source(Some(bad_hook)))
                .unwrap_err()
                .code(),
            "skillset.package.secret_value"
        );

        let toml_with_value: toml::Value =
            toml::from_str("[[environment]]\nname = \"DEMO_TOKEN\"\nvalue = \"hunter2\"\n")
                .unwrap();
        assert!(
            PackageMetadata::from_toml_value(toml_with_value).is_err(),
            "an environment entry cannot carry a value field"
        );
        // References are fine.
        assert!(!model::looks_like_secret("Bearer ${DEMO_TOKEN}"));
        assert!(!model::looks_like_secret("npx -y @demo/server"));
    }

    #[test]
    fn source_revision_is_stable_and_moves_with_members() {
        let a = pkg(None);
        let b = pkg(None);
        assert_eq!(a.source_revision, b.source_revision);
        // Member order does not matter.
        let mut s = source(None);
        s.members.reverse();
        assert_eq!(
            PortableSkillPackage::build(s).unwrap().source_revision,
            a.source_revision
        );
        let mut s = source(None);
        s.members[1].revision = "rev-b2".into();
        let moved = PortableSkillPackage::build(s).unwrap();
        assert_ne!(moved.source_revision, a.source_revision);
        assert!(a.source_revision.starts_with("sha256:"));

        let prov = provenance(&a, &target_for(TargetId::Claude, false).plan(&a));
        let diff = diff_provenance(&prov, &moved);
        assert!(!diff.current);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].id, "skill/demo/method");
        assert!(diff_provenance(&prov, &a).current);
    }

    #[test]
    fn metadata_reads_from_toml() {
        let value: toml::Value = toml::from_str(
            r#"
name = "demo-pack"
version = "1.2.3"
keywords = ["demo"]
display_name = "Demo Pack"
[[mcp]]
name = "srv"
url = "https://example.com/mcp"
[[hooks]]
event = "session-start"
command = "echo hi"
[presentation]
category = "Docs"
[targets.claude]
defaultEnabled = false
"#,
        )
        .unwrap();
        let metadata = PackageMetadata::from_toml_value(value).unwrap();
        let p = pkg(Some(metadata));
        assert_eq!(p.identity.name, "demo-pack");
        assert_eq!(p.version, "1.2.3");
        assert_eq!(p.presentation.display_name.as_deref(), Some("Demo Pack"));
        assert_eq!(p.presentation.category.as_deref(), Some("Docs"));
        let (plan, claude) = render(TargetId::Claude, false, &p);
        assert_eq!(
            json_of(&claude, ".claude-plugin/plugin.json")["defaultEnabled"],
            false
        );
        assert!(plan.has("overlay:claude").is_some());
        assert_eq!(
            json_of(&claude, ".mcp.json")["mcpServers"]["srv"]["type"],
            "http"
        );
        let (_, openai) = render(TargetId::Openai, false, &p);
        assert_eq!(
            json_of(&openai, "mcp.json")["mcpServers"]["srv"]["type"],
            "streamable-http"
        );
    }

    #[test]
    fn unquoted_method_descriptions_fail_structural_validation() {
        let text = "---\nname: x\ndescription: METHOD: Do it.\n---\n";
        assert!(target::unquoted_colon_scalar(text).is_some());
        let quoted = "---\nname: x\ndescription: \"METHOD: Do it.\"\n---\n";
        assert!(target::unquoted_colon_scalar(quoted).is_none());
        let p = pkg(None);
        let t = target_for(TargetId::Pi, false);
        let (_, rendered) = plan_and_render(t.as_ref(), &p).unwrap();
        let mut files = files_of(&rendered);
        files.insert(
            "skills/demo-method/SKILL.md".into(),
            text.as_bytes().to_vec(),
        );
        assert!(!errors(&t.structural_validation(&files)).is_empty());
    }

    #[test]
    fn invalid_names_are_refused() {
        let metadata = PackageMetadata {
            name: Some("Bad Name".into()),
            ..Default::default()
        };
        assert_eq!(
            PortableSkillPackage::build(source(Some(metadata)))
                .unwrap_err()
                .code(),
            "skillset.package.bad_name"
        );
        assert_eq!(model::kebab_case("Demo Set!!"), "demo-set");
    }
}
