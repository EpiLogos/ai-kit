use aikit_core::{
    project_reflection, verify_reflection_law, ProjectLens, ProjectMap, ProjectMapBinding,
    ProjectMapEndpoint, ReflectionIssueKind, ReflectionLaw, ReflectionMapping, ResourceKind,
    ResourceRef, SourceAuthority, SourceRef,
};

fn endpoint(
    resource: ResourceRef,
    kind: ResourceKind,
    lens: ProjectLens,
    authority: SourceAuthority,
    revision: Option<&str>,
) -> ProjectMapEndpoint {
    ProjectMapEndpoint {
        label: Some(resource.to_string()),
        resource,
        kind,
        lens,
        authority,
        provider: None,
        revision: revision.map(str::to_owned),
    }
}

fn bind(
    map: &mut ProjectMap,
    from: &ResourceRef,
    to: &ResourceRef,
    relation: &str,
    authority: SourceAuthority,
) {
    map.bind(ProjectMapBinding {
        from: from.clone(),
        to: to.clone(),
        relation: relation.into(),
        reversible: true,
        authority,
        provider: None,
        provenance: vec![ResourceRef::parse("source:reflection-declaration").unwrap()],
    })
    .unwrap();
}

#[test]
fn reflection_survives_round_trip_with_authority_provenance_and_reverse_route() {
    let semantic = ResourceRef::parse("wiki:concept:auth").unwrap();
    let description = ResourceRef::parse("source:local-description:auth").unwrap();
    let code = ResourceRef::parse("code:src/auth.rs:login").unwrap();
    let verification = ResourceRef::parse("verification:test:auth").unwrap();

    let mut map = ProjectMap::new();
    for value in [
        endpoint(
            semantic.clone(),
            ResourceKind::KnowledgeNode,
            ProjectLens::SemanticWiki,
            SourceAuthority::Authored,
            Some("semantic-r1"),
        ),
        endpoint(
            description.clone(),
            ResourceKind::KnowledgeSource,
            ProjectLens::SourcePool,
            SourceAuthority::Authored,
            Some("description-r1"),
        ),
        endpoint(
            code.clone(),
            ResourceKind::CodeReference,
            ProjectLens::Code,
            SourceAuthority::Derived,
            Some("git:implementation-r1"),
        ),
        endpoint(
            verification.clone(),
            ResourceKind::Action,
            ProjectLens::Verification,
            SourceAuthority::Observed,
            Some("test-run-r1"),
        ),
    ] {
        map.add_endpoint(value).unwrap();
    }
    bind(
        &mut map,
        &semantic,
        &description,
        "described-by",
        SourceAuthority::Authored,
    );
    bind(
        &mut map,
        &description,
        &code,
        "describes",
        SourceAuthority::Authored,
    );
    bind(
        &mut map,
        &semantic,
        &code,
        "implemented-by",
        SourceAuthority::Authored,
    );
    bind(
        &mut map,
        &code,
        &verification,
        "verified-by",
        SourceAuthority::Observed,
    );

    let encoded = serde_json::to_string(&map).unwrap();
    let rebuilt: ProjectMap = serde_json::from_str(&encoded).unwrap();

    assert_eq!(
        rebuilt.endpoint(&semantic).unwrap().authority,
        SourceAuthority::Authored
    );
    assert_eq!(
        rebuilt.endpoint(&code).unwrap().authority,
        SourceAuthority::Derived
    );
    assert_eq!(
        rebuilt.endpoint(&verification).unwrap().authority,
        SourceAuthority::Observed
    );
    assert!(rebuilt.bindings().iter().any(|binding| {
        binding.from == semantic
            && binding.to == code
            && binding.relation == "implemented-by"
            && binding.provenance
                == vec![ResourceRef::parse("source:reflection-declaration").unwrap()]
    }));

    let outward = project_reflection(&rebuilt, &semantic, 3, 16);
    assert!(outward.code.iter().any(|item| item.endpoint.resource == code));
    assert!(outward
        .descriptions
        .iter()
        .any(|item| item.endpoint.resource == description));
    assert!(outward
        .verification
        .iter()
        .any(|item| item.endpoint.resource == verification));

    let reverse = project_reflection(&rebuilt, &code, 3, 16);
    assert!(reverse
        .meaning
        .iter()
        .any(|item| item.endpoint.resource == semantic));
    assert!(reverse
        .descriptions
        .iter()
        .any(|item| item.endpoint.resource == description));
    assert!(reverse
        .verification
        .iter()
        .any(|item| item.endpoint.resource == verification));

    let law = ReflectionLaw {
        id: "law:auth-reflection".into(),
        source: Some(SourceRef::parse("source:auth-reflection-law").unwrap()),
        source_revision: Some("law-r1".into()),
        unique_implementation: true,
        mappings: vec![ReflectionMapping {
            coordinate: "auth".into(),
            semantic: semantic.clone(),
            implementation: code.clone(),
            relation: "implemented-by".into(),
            description: Some(description),
            description_relation: Some("describes".into()),
            expected_implementation_revision: Some("git:implementation-r1".into()),
        }],
        constitutive_relations: vec![],
    };
    assert!(verify_reflection_law(&rebuilt, &law).is_conformant());

    let stale = ReflectionLaw {
        mappings: vec![ReflectionMapping {
            expected_implementation_revision: Some("git:implementation-r0".into()),
            ..law.mappings[0].clone()
        }],
        ..law
    };
    let result = verify_reflection_law(&rebuilt, &stale);
    assert!(!result.is_conformant());
    assert!(result
        .issues
        .iter()
        .any(|issue| issue.kind == ReflectionIssueKind::Stale));
}
