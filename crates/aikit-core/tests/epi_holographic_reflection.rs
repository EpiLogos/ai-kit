use aikit_core::{
    project_reflection, verify_reflection_law, CodeReference, ProjectLens, ProjectMap,
    ProjectMapBinding, ProjectMapEndpoint, ReflectionLaw, ReflectionMapping, ResourceKind,
    ResourceRef, SourceAuthority, SourceRef, SourceRevision,
};

const EPI_REVISION: &str = "daa660cbc1b8c5da83828698665a753852cb0287";
const QL_HEAD: &str = "de7d50c9f7dcfec33cfa0fd5f8a8a1068b4fbe84";
const BIMBA_COORDINATE: &str = "#1-4.2";
const BIMBA_NODE_PATH: &str = "Idea/Bimba/Map/datasets/low-detail/nodes_paramasiva.json";

fn endpoint(
    resource: ResourceRef,
    lens: ProjectLens,
    kind: ResourceKind,
    authority: SourceAuthority,
    revision: Option<&str>,
) -> ProjectMapEndpoint {
    ProjectMapEndpoint {
        resource,
        kind,
        lens,
        authority,
        provider: None,
        revision: revision.map(str::to_owned),
        label: None,
    }
}

fn bind(
    map: &mut ProjectMap,
    from: &ResourceRef,
    to: &ResourceRef,
    relation: &str,
    provenance: &[ResourceRef],
) {
    map.bind(ProjectMapBinding {
        from: from.clone(),
        to: to.clone(),
        relation: relation.into(),
        reversible: true,
        authority: SourceAuthority::Authored,
        provider: None,
        provenance: provenance.to_vec(),
    })
    .unwrap();
}

/// Epi is deliberately only a conformance subject here. AIKit never parses a
/// Bimba coordinate, C category, S/S′ relation, M′ form, or VĀK meaning.
///
/// The frozen Epi Map now supplies the exact source-owned identity for this
/// specimen: `#1-4.2`, "Principle of Inversion". Its source formulation names
/// the six-position complement pairs 0↔5, 1↔4 and 2↔3, while the Map relation
/// `#1-4.1 INVERTS_INTO #1-4.2` preserves its source lineage. AIKit carries the
/// coordinate only as an opaque target-owned ReflectionLaw coordinate. Live
/// Bimba MCP/Neo4j verification remains a separate source-owner evidence leg.
#[test]
fn epi_manifest_subject_round_trips_to_exact_ql_c_symbol_and_evidence() {
    let semantic = ResourceRef::parse("epi:specimen:position-inversion").unwrap();
    let manifest = ResourceRef::parse("ql-mef:manifest:epi-holographic-kernel-v1").unwrap();
    let verification = ResourceRef::parse("ql-mef:evidence:pr59-position-inversion").unwrap();

    let code = CodeReference {
        source: SourceRef::parse("github:EpiLogos/QL-MEF").unwrap(),
        revision: Some(SourceRevision::parse(QL_HEAD).unwrap()),
        path: "c/src/primitive.c".into(),
        symbol: Some("ql_position_invert".into()),
        kind: Some("function".into()),
        line: None,
    };
    let code_resource = code.resource_ref();

    // Provider/index revisions are not code identity. If the exact symbol moves,
    // the new path/symbol produces another ResourceRef and the declared law below
    // reports the old mapping missing/stale instead of silently retargeting it.
    let mut other_revision = code.clone();
    other_revision.revision = Some(SourceRevision::parse("future-reindex").unwrap());
    assert_eq!(code_resource, other_revision.resource_ref());

    let mut map = ProjectMap::new();
    map.add_endpoint(endpoint(
        semantic.clone(),
        ProjectLens::Canon,
        ResourceKind::KnowledgeNode,
        SourceAuthority::Authored,
        Some(EPI_REVISION),
    ))
    .unwrap();
    map.add_endpoint(endpoint(
        manifest.clone(),
        ProjectLens::SourcePool,
        ResourceKind::KnowledgeSource,
        SourceAuthority::Authored,
        Some(QL_HEAD),
    ))
    .unwrap();
    map.add_endpoint(endpoint(
        code_resource.clone(),
        ProjectLens::Code,
        ResourceKind::CodeReference,
        SourceAuthority::Derived,
        Some(QL_HEAD),
    ))
    .unwrap();
    map.add_endpoint(endpoint(
        verification.clone(),
        ProjectLens::Verification,
        ResourceKind::KnowledgeSource,
        SourceAuthority::Observed,
        Some(QL_HEAD),
    ))
    .unwrap();

    bind(
        &mut map,
        &semantic,
        &manifest,
        "described-by",
        std::slice::from_ref(&manifest),
    );
    bind(
        &mut map,
        &manifest,
        &code_resource,
        "describes",
        std::slice::from_ref(&manifest),
    );
    bind(
        &mut map,
        &semantic,
        &code_resource,
        "implemented-by",
        std::slice::from_ref(&manifest),
    );
    bind(
        &mut map,
        &code_resource,
        &verification,
        "verified-by",
        std::slice::from_ref(&manifest),
    );

    let law = ReflectionLaw {
        id: "epi-holographic-specimen/position-inversion/v1".into(),
        source: Some(
            SourceRef::parse(&format!(
                "github:EpiLogos/Epi-Logos-C-Experiments:{BIMBA_NODE_PATH}"
            ))
            .unwrap(),
        ),
        source_revision: Some(EPI_REVISION.into()),
        unique_implementation: true,
        mappings: vec![ReflectionMapping {
            // Exact target-owned Bimba source identity. AIKit does not interpret
            // its syntax or derive its parentage/relation semantics.
            coordinate: BIMBA_COORDINATE.into(),
            semantic: semantic.clone(),
            implementation: code_resource.clone(),
            relation: "implemented-by".into(),
            description: Some(manifest.clone()),
            description_relation: Some("describes".into()),
            expected_implementation_revision: Some(QL_HEAD.into()),
        }],
        constitutive_relations: vec![],
    };

    let verified = verify_reflection_law(&map, &law);
    assert!(verified.passed, "reflection issues: {:?}", verified.issues);

    let from_semantic = project_reflection(&map, &semantic, 3, 12);
    assert!(from_semantic
        .code
        .iter()
        .any(|item| item.endpoint.resource == code_resource));
    assert!(from_semantic
        .verification
        .iter()
        .any(|item| item.endpoint.resource == verification));

    let from_code = project_reflection(&map, &code_resource, 3, 12);
    assert!(from_code
        .meaning
        .iter()
        .any(|item| item.endpoint.resource == semantic));
    assert!(from_code
        .descriptions
        .iter()
        .any(|item| item.endpoint.resource == manifest));

    assert_eq!(law.mappings[0].coordinate, BIMBA_COORDINATE);
    assert_eq!(code.path, "c/src/primitive.c");
    assert_eq!(code.symbol.as_deref(), Some("ql_position_invert"));
    assert_eq!(code.revision.as_ref().map(SourceRevision::as_str), Some(QL_HEAD));
}
