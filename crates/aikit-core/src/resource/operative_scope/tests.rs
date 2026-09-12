use std::cell::RefCell;

use super::*;
use crate::resource::{
    parse_resolve_expression, ActionSemanticProfile, AddressHorizon, MemoryResourceIndex,
    OperativeSemanticProviderCapabilities, OperativeSemanticProviderDescriptor, RelationOp,
    ResourceDescriptor, ResourceKind, OPERATIVE_SEMANTIC_PROVIDER_VERSION,
};

mod knowledge;

fn binding() -> OperativeScope {
    OperativeScope {
        provider: ProviderRef::parse("provider/ql-mef").unwrap(),
        binding: ResourceRef::parse("ql/binding/one").unwrap(),
        owner: OwnerRef::parse("QL-MEF").unwrap(),
        owner_revision: SourceRevision::parse("producer-v1").unwrap(),
        interpretation: ResourceRef::parse("ql/interpretation/c-prime").unwrap(),
        interpretation_revision: SourceRevision::parse("original-profile-v1").unwrap(),
        world: ResourceRef::parse("world/one").unwrap(),
        generation: SourceRevision::parse("generation-1").unwrap(),
        whole: ResourceRef::parse("whole/one").unwrap(),
        subject: ResourceRef::parse("project/one").unwrap(),
        sources: vec![ScopeSource {
            source: SourceRef::parse("source/original").unwrap(),
            revision: SourceRevision::parse("source-revision-1").unwrap(),
        }],
        method_skill: None,
    }
}

fn request() -> ScopedResolveExpression {
    ScopedResolveExpression {
        expression: parse_resolve_expression("( project/one / + action/verify )").unwrap(),
        scopes: vec![ExpressionScope {
            node: vec![],
            binding: binding(),
        }],
    }
}

fn index() -> MemoryResourceIndex {
    let mut index = MemoryResourceIndex::default();
    for (id, kind) in [
        ("project/one", ResourceKind::Project),
        ("action/verify", ResourceKind::Action),
    ] {
        index.insert(ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse(id).unwrap(),
            kind,
            id,
            id,
        )));
    }
    index
}

fn context(resources: &dyn ResourceIndex) -> ContextResolution {
    use crate::context::ContextDescriptor;
    use crate::context_resolution::{compose_context_resolution, RequestedActors};
    use crate::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
    use crate::resolve::ResolvedView;

    let descriptor = ContextDescriptor::for_project("/operative-scope-test");
    let project = ProjectBinding::from_legacy_context(
        ProjectRef::parse("project/one").unwrap(),
        ProjectConstituentRef::parse("constituent/one").unwrap(),
        &descriptor,
    )
    .unwrap();
    let view = ResolvedView {
        context: descriptor,
        policy: Default::default(),
        active: BTreeMap::new(),
        declared: BTreeMap::new(),
        unavailable: BTreeMap::new(),
        selection_log: Vec::new(),
        catalog_index: BTreeMap::new(),
        skill_usage_overlays: BTreeMap::new(),
        warnings: Vec::new(),
        hash: serde_json::from_value(serde_json::json!("0".repeat(64))).unwrap(),
        catalog_revision: "test-catalogue-r1".into(),
        properties: BTreeMap::new(),
    };
    compose_context_resolution(&view, project, &[], resources, RequestedActors::default())
}

struct ObservingProvider {
    current: RefCell<OperativeScope>,
    standing: RefCell<Option<ScopeObservation>>,
}

impl ObservingProvider {
    fn new() -> Self {
        Self {
            current: RefCell::new(binding()),
            standing: RefCell::new(None),
        }
    }
}

impl OperativeSemanticProvider for ObservingProvider {
    type SemanticRef = String;
    type ResourceReading = String;
    type ActionProfile = String;
    type Path = String;

    fn descriptor(&self) -> OperativeSemanticProviderDescriptor {
        OperativeSemanticProviderDescriptor {
            version: OPERATIVE_SEMANTIC_PROVIDER_VERSION.into(),
            provider: binding().provider,
            status: OperativeSemanticProviderStatus::Available,
            capabilities: OperativeSemanticProviderCapabilities::default(),
            provenance: vec![],
        }
    }
    fn bind_horizon(&self, _: AddressHorizon) -> Result<Option<Self::SemanticRef>> {
        Ok(None)
    }
    fn bind_relation(&self, _: RelationOp) -> Result<Option<Self::SemanticRef>> {
        Ok(None)
    }
    fn resource_readings(
        &self,
        _: &ResourceRef,
        _: Option<&ResolveExpression>,
    ) -> Result<Vec<Self::ResourceReading>> {
        Ok(vec![])
    }
    fn enrich_action(&self, _: &ActionSemanticProfile) -> Result<Option<Self::ActionProfile>> {
        Ok(None)
    }
    fn enrich_path(&self, _: &ResolvePath) -> Result<Option<Self::Path>> {
        Ok(None)
    }
}

impl ScopeAwareOperativeProvider for ObservingProvider {
    fn observe_scope(
        &self,
        _: &OperativeScope,
        context: &ContextResolution,
    ) -> Result<ScopeObservation> {
        assert_eq!(context.project_binding.project.as_str(), "project/one");
        Ok(self
            .standing
            .borrow()
            .clone()
            .unwrap_or_else(|| ScopeObservation::Current {
                binding: self.current.borrow().clone(),
                evidence: vec![ResourceRef::parse("source/observation/one").unwrap()],
            }))
    }
}

#[test]
fn source_qualified_identity_is_not_the_rendered_text() {
    let original = request();
    let identity = original.identity().unwrap();
    let rendered = original.expression.render();
    let changes: [fn(&mut OperativeScope); 9] = [
        |s| s.world = ResourceRef::parse("world/two").unwrap(),
        |s| s.whole = ResourceRef::parse("whole/two").unwrap(),
        |s| s.subject = ResourceRef::parse("project/two").unwrap(),
        |s| s.owner = OwnerRef::parse("another-owner").unwrap(),
        |s| s.owner_revision = SourceRevision::parse("producer-v2").unwrap(),
        |s| s.generation = SourceRevision::parse("generation-2").unwrap(),
        |s| s.interpretation_revision = SourceRevision::parse("profile-v2").unwrap(),
        |s| s.sources[0].revision = SourceRevision::parse("source-revision-2").unwrap(),
        |s| s.method_skill = Some(ResourceRef::parse("skill/recognised").unwrap()),
    ];
    for change in changes {
        let mut changed = original.clone();
        change(&mut changed.scopes[0].binding);
        assert_eq!(changed.expression.render(), rendered);
        assert_ne!(changed.identity().unwrap(), identity);
    }
}

#[test]
fn no_ql_search_and_path_identity_stay_native() {
    let index = index();
    let expression = ResolveExpression::ordinary_search("verify");
    let native = resolve_expression(&expression, &index, 16);
    let scoped =
        resolve_scoped_expression(&ScopedResolveExpression::unbound(expression), &index, 16)
            .unwrap();
    assert_eq!(scoped.path, native);
    assert!(scoped.scopes.is_empty());
}

#[test]
fn structured_binding_roundtrip_keeps_nested_node_and_source_identity() {
    let request = request();
    let encoded = serde_json::to_string(&request).unwrap();
    let decoded: ScopedResolveExpression = serde_json::from_str(&encoded).unwrap();
    assert_eq!(request, decoded);
    assert_eq!(request.identity().unwrap(), decoded.identity().unwrap());
    let path = resolve_scoped_expression(&decoded, &index(), 16).unwrap();
    assert_eq!(path.path.expression, request.expression);
    assert_eq!(path.scopes, request.scopes);
    assert_eq!(path.path.candidates.len(), 2);
}

#[test]
fn child_binding_shadows_only_its_own_branch() {
    let mut request = request();
    let child_path = vec![ExpressionEdge::Operand, ExpressionEdge::Right];
    let mut child = binding();
    child.binding = ResourceRef::parse("ql/binding/child").unwrap();
    child.whole = ResourceRef::parse("whole/child").unwrap();
    request.scopes.push(ExpressionScope {
        node: child_path.clone(),
        binding: child.clone(),
    });
    let left = vec![ExpressionEdge::Operand, ExpressionEdge::Left];
    assert_eq!(
        request.effective_scopes(&left).unwrap()[0].binding,
        binding()
    );
    assert_eq!(
        request.effective_scopes(&child_path).unwrap()[0].binding,
        child
    );
    assert_eq!(request.canonical().unwrap().scopes.len(), 2);
    let mut reordered = request.clone();
    reordered.scopes.reverse();
    assert_eq!(request.identity().unwrap(), reordered.identity().unwrap());
}

#[test]
fn invalid_node_ambiguity_and_deserialised_invalid_refs_fail_closed() {
    let mut request = request();
    request.scopes[0].node = vec![ExpressionEdge::Left];
    assert!(request.canonical().is_err());
    request.scopes[0].node.clear();
    request.scopes.push(request.scopes[0].clone());
    assert!(request.canonical().is_err());
    request.scopes.pop();
    let duplicate = request.scopes[0].binding.sources[0].clone();
    request.scopes[0].binding.sources.push(duplicate);
    assert!(request.canonical().is_err());
    let mut encoded = serde_json::to_value(super::tests::request()).unwrap();
    encoded["scopes"][0]["binding"]["world"] = serde_json::json!("");
    let invalid: ScopedResolveExpression = serde_json::from_value(encoded).unwrap();
    assert!(invalid.identity().is_err());
}

#[test]
fn qualified_familiarity_uses_the_same_native_ranking_law() {
    struct RecordingIndex {
        index: MemoryResourceIndex,
        paths: RefCell<Vec<String>>,
    }
    impl ResourceIndex for RecordingIndex {
        fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord> {
            self.index.resource(id)
        }
        fn resources(&self) -> Vec<&ResourceRecord> {
            self.index.resources()
        }
        fn resolve_path_ranking(&self, path: &str, _: &ResourceRef) -> ResolveRankingSignals {
            self.paths.borrow_mut().push(path.into());
            ResolveRankingSignals::default()
        }
    }
    let index = RecordingIndex {
        index: index(),
        paths: RefCell::new(vec![]),
    };
    let request = request();
    let resolved = resolve_scoped_expression(&request, &index, 16).unwrap();
    assert!(!index.paths.borrow().is_empty());
    assert!(index
        .paths
        .borrow()
        .iter()
        .all(|id| id == &resolved.path.identity));
    assert_eq!(
        resolved.path.candidates,
        resolve_expression(&request.expression, &index.index, 16).candidates
    );
}

#[test]
fn structural_budget_covers_structured_clients_too() {
    let mut expression = ResolveExpression::subject("subject");
    for _ in 0..66 {
        expression = ResolveExpression::Frame {
            expression: Box::new(expression),
        };
    }
    assert!(ScopedResolveExpression::unbound(expression)
        .canonical()
        .is_err());
    let mut request = request();
    request.scopes[0].binding.sources.clear();
    assert!(request.canonical().is_err());
}

#[test]
fn current_native_context_and_provider_revision_are_both_required() {
    let index = index();
    let context = context(&index);
    let provider = ObservingProvider::new();
    let resolution = compose_scoped_context(&request(), &index, &context, 16, &provider).unwrap();
    resolution.revalidate(&context, &provider).unwrap();
    let mut changed_context = context.clone();
    changed_context.deterministic.context.task = Some("another-task".into());
    assert!(resolution.revalidate(&changed_context, &provider).is_err());
    provider.current.borrow_mut().sources[0].revision = SourceRevision::parse("source-r2").unwrap();
    assert!(resolution.revalidate(&context, &provider).is_err());
    let completed = resolution.completion_observations(&context, &provider);
    assert!(matches!(
        completed[0].observation,
        ScopeObservation::Stale { .. }
    ));
    assert_eq!(resolution.observations[0].scope.binding, binding());
}

#[test]
fn missing_ambiguous_unavailable_and_unsupported_do_not_become_current() {
    let index = index();
    let context = context(&index);
    let provider = ObservingProvider::new();
    for observation in [
        ScopeObservation::Missing {
            reason: "removed".into(),
        },
        ScopeObservation::Ambiguous {
            candidates: vec![ResourceRef::parse("binding/other").unwrap()],
            reason: "two owners".into(),
        },
        ScopeObservation::Unavailable {
            reason: "offline".into(),
        },
        ScopeObservation::Unsupported {
            reason: "no capability".into(),
        },
    ] {
        *provider.standing.borrow_mut() = Some(observation.clone());
        let resolution =
            compose_scoped_context(&request(), &index, &context, 16, &provider).unwrap();
        assert_eq!(resolution.observations[0].observation, observation);
        assert!(resolution.require_current().is_err());
    }
}
