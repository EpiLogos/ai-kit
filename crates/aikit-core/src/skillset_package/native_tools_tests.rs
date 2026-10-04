use super::*;
use crate::method::PraxisForm;
use serde_json::json;

fn source() -> PackageSource {
    PackageSource {
        skillset_ref: "native-source-tools".into(),
        set_name: "native-source-tools".into(),
        set_description: "Read sources through a native target tool.".into(),
        metadata: Some(PackageMetadata {
            native_tools: vec![NativeToolContribution {
                name: "package_native_read".into(),
                target: TargetId::Pi,
                member_id: "skill/native-source-reader".into(),
                module: "native.ts".into(),
            }],
            ..Default::default()
        }),
        members: vec![PackageMember {
            id: "skill/native-source-reader".into(),
            form: PraxisForm::Skill,
            name: "native-source-reader".into(),
            description: "Read a bounded source file.".into(),
            revision: "source-reader-r1".into(),
            tools: vec!["read".into()],
            files: vec![
                PackageFile::inline("SKILL.md", b"---\nname: native-source-reader\ndescription: Read a bounded source file.\n---\n\nUse package_native_read for a source in the current working directory.\n".to_vec()),
                PackageFile::inline("native.ts", include_bytes!("../../../aikit-cli/tests/fixtures/package-native-read.ts").to_vec()),
            ],
        }],
        unresolved: vec![],
    }
}

#[test]
fn native_tool_payload_is_preserved_and_registered_as_an_extension() {
    let pkg = PortableSkillPackage::build(source()).unwrap();
    assert_eq!(pkg.tool_dependencies, ["read"]);
    let (plan, files) = plan_and_render(&pi::PiTarget, &pkg).unwrap();
    let mut map = FileMap::new();
    for file in files {
        let RenderedContent::Bytes(bytes) = file.content else {
            panic!("inline source");
        };
        map.insert(file.path, bytes);
    }
    let manifest: serde_json::Value = serde_json::from_slice(&map["package.json"]).unwrap();
    assert_eq!(
        manifest["pi"]["extensions"],
        json!(["./skills/native-source-reader/native.ts"])
    );
    assert_eq!(
        map["skills/native-source-reader/native.ts"].as_slice(),
        include_bytes!("../../../aikit-cli/tests/fixtures/package-native-read.ts")
    );
    assert!(!map.contains_key("extensions/native-source-tools-aikit.ts"));
    assert!(pi::PiTarget
        .structural_validation(&map)
        .iter()
        .all(|f| f.severity != Severity::Error));
    assert!(plan.has("native-tool:pi:package_native_read").is_some());
    assert_eq!(
        provenance(&pkg, &plan)["native_tools"][0]["member_id"],
        "skill/native-source-reader"
    );
}

#[test]
fn native_tools_do_not_replace_host_dependencies_or_claim_other_target_admission() {
    let pkg = PortableSkillPackage::build(source()).unwrap();
    for target in [TargetId::Openai, TargetId::Codex, TargetId::Claude] {
        let adapter = target_for(target, false);
        let plan = adapter.plan(&pkg);
        assert!(matches!(
            plan.has("native-tool:pi:package_native_read")
                .unwrap()
                .class,
            PlanClass::Unsupported { .. }
        ));
        assert_eq!(pkg.tool_dependencies, ["read"]);
    }
}

#[test]
fn native_module_and_registration_changes_move_package_identity() {
    let original = PortableSkillPackage::build(source()).unwrap();
    let mut changed = source();
    changed.members[0].files[1] =
        PackageFile::inline("native.ts", b"export default function (pi) {}\n".to_vec());
    assert_ne!(
        PortableSkillPackage::build(changed)
            .unwrap()
            .source_revision,
        original.source_revision
    );
    let mut changed = source();
    changed.metadata.as_mut().unwrap().native_tools[0].name = "other_native_read".into();
    assert_ne!(
        PortableSkillPackage::build(changed)
            .unwrap()
            .source_revision,
        original.source_revision
    );
    let mut plain = source();
    plain.metadata.as_mut().unwrap().native_tools.clear();
    let expected = source_revision(&plain.members, &plain.unresolved);
    assert_eq!(
        PortableSkillPackage::build(plain).unwrap().source_revision,
        expected
    );
}

#[test]
fn native_payload_paths_and_missing_or_duplicate_owners_are_refused() {
    for module in [
        "../native.ts",
        "/native.ts",
        "a/../../native.ts",
        "a\\native.ts",
        "a//native.ts",
        "./native.ts",
        "C:/native.ts",
        "SKILL.md",
        "missing.ts",
    ] {
        let mut s = source();
        s.metadata.as_mut().unwrap().native_tools[0].module = module.into();
        assert!(PortableSkillPackage::build(s).is_err(), "{module}");
    }
    let mut s = source();
    s.members.clear();
    assert!(PortableSkillPackage::build(s).is_err());
    let mut s = source();
    s.members.push(s.members[0].clone());
    assert!(PortableSkillPackage::build(s).is_err());
    let mut s = source();
    let duplicate_file = s.members[0].files[1].clone();
    s.members[0].files.push(duplicate_file);
    assert!(PortableSkillPackage::build(s).is_err());
    let mut s = source();
    let duplicate = s.metadata.as_ref().unwrap().native_tools[0].clone();
    s.metadata.as_mut().unwrap().native_tools.push(duplicate);
    assert!(PortableSkillPackage::build(s).is_err());
}

#[test]
fn overlay_cannot_remove_native_module_or_generated_hook_extension() {
    let mut s = source();
    let metadata = s.metadata.as_mut().unwrap();
    metadata
        .targets
        .insert("pi".into(), json!({ "pi": { "extensions": [] } }));
    metadata.hooks.push(HookRequirement {
        event: HookEvent::SessionStart,
        purpose: "inspect native entry".into(),
        command: "true".into(),
        matcher: None,
    });
    let pkg = PortableSkillPackage::build(s).unwrap();
    let (_, files) = plan_and_render(&pi::PiTarget, &pkg).unwrap();
    let RenderedContent::Bytes(bytes) = &files
        .iter()
        .find(|f| f.path == "package.json")
        .unwrap()
        .content
    else {
        panic!("manifest bytes");
    };
    let manifest: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(manifest["pi"]["extensions"].as_array().unwrap().len(), 2);
    let mut invalid = source();
    invalid
        .metadata
        .as_mut()
        .unwrap()
        .targets
        .insert("pi".into(), json!({ "pi": null }));
    assert!(plan_and_render(
        &pi::PiTarget,
        &PortableSkillPackage::build(invalid).unwrap()
    )
    .is_err());
}

#[test]
fn multi_tool_module_is_loaded_once_and_contribution_order_is_stable() {
    let mut s = source();
    let mut second = s.metadata.as_ref().unwrap().native_tools[0].clone();
    second.name = "package_native_stat".into();
    s.metadata.as_mut().unwrap().native_tools.push(second);
    let pkg = PortableSkillPackage::build(s.clone()).unwrap();
    s.metadata.as_mut().unwrap().native_tools.reverse();
    assert_eq!(
        PortableSkillPackage::build(s).unwrap().source_revision,
        pkg.source_revision
    );
    let (_, files) = plan_and_render(&pi::PiTarget, &pkg).unwrap();
    let RenderedContent::Bytes(bytes) = &files[0].content else {
        panic!("manifest bytes");
    };
    let manifest: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(manifest["pi"]["extensions"].as_array().unwrap().len(), 1);
}

#[test]
fn wrong_native_module_digest_and_missing_bytes_are_refused() {
    let mut s = source();
    s.members[0].files[1].sha256 = "0".repeat(64);
    assert!(PortableSkillPackage::build(s).is_err());
    let mut s = source();
    s.members[0].files[1].bytes += 1;
    assert!(PortableSkillPackage::build(s).is_err());
    let mut s = source();
    s.members[0].files[1].inline = None;
    assert!(PortableSkillPackage::build(s).is_err());
}
