use std::fs;
use std::process::Command;

use aikit_adapters::gitnexus::GitNexusCodeIndexProvider;
use aikit_adapters::runner::SystemRunner;
use aikit_core::knowledge_code::{CodeIndexProvider, GITNEXUS_TESTED_VERSION};
use aikit_core::project_map::{ProjectLens, ProjectMap, ProjectMapBinding, ProjectMapEndpoint};
use aikit_core::project_reflection::{
    project_reflection, verify_reflection_law, ReflectionIssueKind, ReflectionLaw,
    ReflectionMapping,
};
use aikit_core::resource::{
    ResourceKind, ResourceRef, SourceAuthority, SourceRef, SourceRevision,
};
use tempfile::tempdir;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git fixture command");
    assert!(status.success(), "git fixture command failed: {args:?}");
}

fn endpoint(
    resource: ResourceRef,
    kind: ResourceKind,
    lens: ProjectLens,
    authority: SourceAuthority,
    revision: Option<String>,
) -> ProjectMapEndpoint {
    ProjectMapEndpoint {
        label: Some(resource.to_string()),
        resource,
        kind,
        lens,
        authority,
        provider: None,
        revision,
    }
}

fn bind(map: &mut ProjectMap, from: &ResourceRef, to: &ResourceRef, relation: &str, authority: SourceAuthority) {
    map.bind(ProjectMapBinding {
        from: from.clone(),
        to: to.clone(),
        relation: relation.into(),
        reversible: true,
        authority,
        provider: None,
        provenance: vec![ResourceRef::parse("source:fixture:project-reflection").unwrap()],
    })
    .unwrap();
}

#[test]
fn real_gitnexus_current_cli_indexes_searches_contextualises_impacts_traces_changes_and_checks() {
    let dir = tempdir().expect("temporary GitNexus fixture");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/auth.ts"),
        r#"export function validate(token: string): boolean {
  return token.length > 3;
}

export function login(token: string): boolean {
  return validate(token);
}
"#,
    )
    .unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"aikit-gitnexus-fixture"}"#,
    )
    .unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.email=aikit@example.invalid",
            "-c",
            "user.name=AIKit CI",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let revision_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(revision_output.status.success());
    let revision = String::from_utf8(revision_output.stdout).unwrap();
    let source_revision = SourceRevision::parse(&format!("git:{}", revision.trim())).unwrap();

    let mut provider = GitNexusCodeIndexProvider::new(
        SystemRunner::new(),
        "aikit-gitnexus-fixture",
        SourceRef::parse("source:git/aikit-gitnexus-fixture").unwrap(),
        Some(source_revision.clone()),
    );
    let before = provider.status();
    if !before.available {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_GITNEXUS_REAL").is_none(),
            "AIKIT_REQUIRE_GITNEXUS_REAL is set but GitNexus is unavailable: {}",
            before.detail
        );
        return;
    }
    assert_eq!(before.version.as_deref(), Some(GITNEXUS_TESTED_VERSION));
    assert!(before.capabilities.index);
    assert!(before.capabilities.search);
    assert!(before.capabilities.context);
    assert!(before.capabilities.impact);
    assert!(before.capabilities.trace);
    assert!(before.capabilities.detect_changes);
    assert!(before.capabilities.structural_check);
    assert!(before.capabilities.cypher);
    assert!(!before.capabilities.structured_output);

    let indexed = provider.index(root, true).expect("real GitNexus analyze");
    assert!(indexed.indexed);

    let login_hits = provider.search("login", 10).expect("real GitNexus query");
    let login = login_hits
        .iter()
        .find(|hit| hit.reference.symbol.as_deref() == Some("login"))
        .expect("query must expose canonical login code reference")
        .reference
        .clone();
    assert_eq!(login.source.as_str(), "source:git/aikit-gitnexus-fixture");
    assert!(login.path.ends_with("src/auth.ts"));

    let validate_hits = provider
        .search("validate", 10)
        .expect("real GitNexus validate query");
    let validate = validate_hits
        .iter()
        .find(|hit| hit.reference.symbol.as_deref() == Some("validate"))
        .expect("query must expose canonical validate code reference")
        .reference
        .clone();

    let context = provider.context(&login).expect("real GitNexus context");
    assert!(context.detail.is_object());
    let impact = provider
        .impact(&validate, "upstream")
        .expect("real GitNexus impact");
    assert!(impact.detail.is_object());
    let trace = provider
        .trace(&login, &validate)
        .expect("real GitNexus trace");
    assert!(trace.detail.is_object());

    // Bind the exact source-owned CodeReference produced by the real provider into
    // the same ProjectMap federation used by human and Agent Knowledge Navigation.
    let semantic = ResourceRef::parse("wiki:concept:login").unwrap();
    let description = ResourceRef::parse("source:local-description:auth-module").unwrap();
    let code = login.resource_ref();
    let verification = ResourceRef::parse("verification:gitnexus:1.6.9").unwrap();
    let mut map = ProjectMap::new();
    for value in [
        endpoint(
            semantic.clone(),
            ResourceKind::KnowledgeNode,
            ProjectLens::SemanticWiki,
            SourceAuthority::Authored,
            Some("wiki-rev-1".into()),
        ),
        endpoint(
            description.clone(),
            ResourceKind::KnowledgeSource,
            ProjectLens::SourcePool,
            SourceAuthority::Authored,
            Some("description-rev-1".into()),
        ),
        endpoint(
            code.clone(),
            ResourceKind::CodeReference,
            ProjectLens::Code,
            SourceAuthority::Derived,
            Some(source_revision.to_string()),
        ),
        endpoint(
            verification.clone(),
            ResourceKind::Action,
            ProjectLens::Verification,
            SourceAuthority::Observed,
            Some(GITNEXUS_TESTED_VERSION.into()),
        ),
    ] {
        map.add_endpoint(value).unwrap();
    }
    bind(&mut map, &semantic, &description, "described-by", SourceAuthority::Authored);
    bind(&mut map, &description, &code, "describes", SourceAuthority::Authored);
    bind(&mut map, &semantic, &code, "implemented-by", SourceAuthority::Authored);
    bind(&mut map, &code, &verification, "verified-by", SourceAuthority::Observed);

    let from_meaning = project_reflection(&map, &semantic, 3, 16);
    assert!(from_meaning
        .code
        .iter()
        .any(|item| item.endpoint.resource == code));
    assert!(from_meaning
        .descriptions
        .iter()
        .any(|item| item.endpoint.resource == description));
    assert!(from_meaning
        .verification
        .iter()
        .any(|item| item.endpoint.resource == verification));

    let from_code = project_reflection(&map, &code, 3, 16);
    assert!(from_code
        .meaning
        .iter()
        .any(|item| item.endpoint.resource == semantic));
    assert!(from_code
        .descriptions
        .iter()
        .any(|item| item.endpoint.resource == description));

    let law = ReflectionLaw {
        id: "fixture:login-reflection".into(),
        source: Some(SourceRef::parse("source:fixture:login-reflection-law").unwrap()),
        source_revision: Some("law-v1".into()),
        unique_implementation: true,
        mappings: vec![ReflectionMapping {
            coordinate: "login".into(),
            semantic: semantic.clone(),
            implementation: code.clone(),
            relation: "implemented-by".into(),
            description: Some(description.clone()),
            description_relation: Some("describes".into()),
            expected_implementation_revision: Some(source_revision.to_string()),
        }],
        constitutive_relations: vec![],
    };
    assert!(verify_reflection_law(&map, &law).is_conformant());

    // A stale local/reflection assertion is evidence, never silent retargeting.
    let stale = ReflectionLaw {
        mappings: vec![ReflectionMapping {
            expected_implementation_revision: Some("git:stale-revision".into()),
            ..law.mappings[0].clone()
        }],
        ..law
    };
    let stale_result = verify_reflection_law(&map, &stale);
    assert!(!stale_result.is_conformant());
    assert!(stale_result
        .issues
        .iter()
        .any(|issue| issue.kind == ReflectionIssueKind::Stale));

    fs::write(
        root.join("src/auth.ts"),
        r#"export function validate(token: string): boolean {
  return token.length > 3;
}

export function login(token: string): boolean {
  return validate(token);
}

export function logout(): boolean {
  return true;
}
"#,
    )
    .unwrap();
    let changes = provider
        .detect_changes("unstaged", None)
        .expect("real GitNexus detect-changes");
    assert!(changes.detail.is_string());
    assert!(!changes.detail.as_str().unwrap_or_default().trim().is_empty());
    let structural = provider
        .structural_check()
        .expect("real GitNexus cycle check");
    assert!(structural.detail.is_object());
}