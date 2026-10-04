use super::*;
use aikit_core::method::PraxisForm;
use pkgsdk::NativeToolContribution;

fn package(extra_missing_tool: bool) -> PortableSkillPackage {
    let mut native_tools = vec![NativeToolContribution {
        name: "package_native_read".into(),
        target: TargetId::Pi,
        member_id: "skill/native-source-reader".into(),
        module: "native.ts".into(),
    }];
    if extra_missing_tool {
        native_tools.push(NativeToolContribution {
            name: "undeclared_in_module".into(),
            ..native_tools[0].clone()
        });
    }
    PortableSkillPackage::build(PackageSource {
        skillset_ref: "native-source-tools".into(), set_name: "native-source-tools".into(),
        set_description: "Read source through a native Pi tool.".into(),
        metadata: Some(PackageMetadata { native_tools, ..Default::default() }),
        members: vec![PackageMember {
            id: "skill/native-source-reader".into(), form: PraxisForm::Skill,
            name: "native-source-reader".into(), description: "Read a bounded source file.".into(),
            revision: "source-reader-r1".into(), tools: vec![],
            files: vec![
                PackageFile::inline("SKILL.md", b"---\nname: native-source-reader\ndescription: Read a bounded source file.\n---\n\nUse package_native_read for a source in the current working directory.\n".to_vec()),
                PackageFile::inline("native.ts", include_bytes!("../tests/fixtures/package-native-read.ts").to_vec()),
            ],
        }], unresolved: vec![],
    }).unwrap()
}

#[test]
#[ignore = "requires installed Pi 0.84.4, node and AIKIT_PI_SDK_ROOT; runs real native tools"]
fn native_pi_package_exports_discovers_and_invokes_real_source_tool() {
    assert!(on_path("pi"), "real Pi executable is required");
    let sdk = std::env::var("AIKIT_PI_SDK_ROOT")
        .expect("AIKIT_PI_SDK_ROOT must name the installed Pi SDK root");
    let temp = tempfile::TempDir::new().unwrap();
    let pkg = package(false);
    let target = target_for(TargetId::Pi, false);
    let (plan, files) = plan_and_render(target.as_ref(), &pkg).unwrap();
    let out = temp.path().join("export");
    write_tree(&out, &files).unwrap();
    let map = read_tree(&out).unwrap();
    assert!(
        target
            .structural_validation(&map)
            .iter()
            .all(|f| f.severity != Severity::Error)
    );
    let (native, discovery) = pi_discover(target.as_ref(), &out, &pkg, &plan);
    assert_eq!(native.status, CheckStatus::Passed, "{}", native.summary);
    assert_eq!(discovery.unwrap().status, CheckStatus::Passed);
    let working = temp.path().join("session");
    std::fs::create_dir(&working).unwrap();
    std::fs::write(working.join("source.txt"), "exact original source\n").unwrap();
    let result = Command::new("node")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/invoke-package-native-tool.mjs"),
        )
        .args([
            out.as_os_str(),
            working.as_os_str(),
            Path::new(&sdk).as_os_str(),
        ])
        .env("HOME", temp.path())
        .env("PI_CODING_AGENT_DIR", working.join("agent"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(receipt["tool"], "package_native_read");
    assert_eq!(receipt["bytes"], 22);
    assert_eq!(receipt["missing_source"], "refused");
    assert_eq!(receipt["outside_source"], "refused");
    assert_eq!(receipt["symlink_source"], "refused");
    assert_eq!(receipt["model_calls"], 0);
}

#[test]
#[ignore = "requires installed Pi; detects a genuinely unregistered declared native tool"]
fn native_pi_discovery_refuses_module_that_omits_a_declared_tool() {
    assert!(on_path("pi"), "real Pi executable is required");
    let temp = tempfile::TempDir::new().unwrap();
    let pkg = package(true);
    let target = target_for(TargetId::Pi, false);
    let (plan, files) = plan_and_render(target.as_ref(), &pkg).unwrap();
    write_tree(temp.path(), &files).unwrap();
    let (native, discovery) = pi_discover(target.as_ref(), temp.path(), &pkg, &plan);
    assert_eq!(native.status, CheckStatus::Failed);
    assert_eq!(discovery.unwrap().status, CheckStatus::Failed);
    assert!(native.summary.contains("undeclared_in_module"));
}

#[test]
fn drifted_native_module_is_refused_before_starting_pi() {
    let temp = tempfile::TempDir::new().unwrap();
    let pkg = package(false);
    let target = target_for(TargetId::Pi, false);
    let (plan, files) = plan_and_render(target.as_ref(), &pkg).unwrap();
    write_tree(temp.path(), &files).unwrap();
    std::fs::write(
        temp.path().join("skills/native-source-reader/native.ts"),
        "export default function (pi) {}\n",
    )
    .unwrap();
    let (native, discovery) = pi_discover(target.as_ref(), temp.path(), &pkg, &plan);
    assert_eq!(native.status, CheckStatus::Failed);
    assert!(
        native.command.is_none(),
        "drift is refused before native process creation"
    );
    assert!(native.summary.contains("changed after source resolution"));
    assert!(discovery.is_none());
}

#[test]
fn export_refuses_changed_source_bytes_without_native_validation() {
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("source.ts");
    let original = b"export default function (pi) { pi.registerTool({name: 'native_source'}); }\n";
    std::fs::write(&source, original).unwrap();
    let files = vec![RenderedFile {
        path: "native.ts".into(),
        content: RenderedContent::Copy {
            source: source.clone(),
        },
        sha256: sha256_hex(original),
        executable: false,
    }];
    let out = temp.path().join("export");
    write_tree(&out, &files).unwrap();
    assert_eq!(std::fs::read(out.join("native.ts")).unwrap(), original);
    std::fs::write(&source, b"changed after source resolution\n").unwrap();
    assert!(write_tree(&out, &files).is_err());
    assert_eq!(
        std::fs::read(out.join("native.ts")).unwrap(),
        original,
        "failed Source qualification must preserve the prior exact exported file"
    );
}

#[cfg(unix)]
#[test]
fn copied_payload_preserves_source_executable_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::TempDir::new().unwrap();
    let source = temp.path().join("source.sh");
    let original = b"#!/bin/sh\nprintf 'source'\n";
    std::fs::write(&source, original).unwrap();
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755)).unwrap();
    let files = vec![RenderedFile {
        path: "source.sh".into(),
        content: RenderedContent::Copy {
            source: source.clone(),
        },
        sha256: sha256_hex(original),
        executable: false,
    }];
    let out = temp.path().join("export");
    write_tree(&out, &files).unwrap();
    assert_eq!(
        std::fs::metadata(out.join("source.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(std::fs::read(out.join("source.sh")).unwrap(), original);
}
