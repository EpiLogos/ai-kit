use std::cell::RefCell;

use super::*;
use crate::resource::{parse_resolve_expression, MemoryResourceIndex, ResourceDescriptor, ResourceKind};

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
        expression: parse_resolve_expression("( @4 project/one / + @5 action/verify )").unwrap(),
        scopes: vec![ExpressionScope { node: vec![], binding: binding() }],
    }
}
fn index() -> MemoryResourceIndex {
    let mut index = MemoryResourceIndex::default();
    for (id, kind) in [("project/one", ResourceKind::Project), ("action/verify", ResourceKind::Action)] {
        index.insert(ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse(id).unwrap(), kind, id, id,
        )));
    }
    index
}

#[test]
fn source_qualified_identity_is_not_the_rendered_text() {
    let original = request();
    let identity = original.identity().unwrap();
    let rendered = original.expression.render();
    let changes: Vec<Box<dyn Fn(&mut OperativeScope)>> = vec![
        Box::new(|s| s.world = ResourceRef::parse("world/two").unwrap()),
        Box::new(|s| s.whole = ResourceRef::parse("whole/two").unwrap()),
        Box::new(|s| s.subject = ResourceRef::parse("project/two").unwrap()),
        Box::new(|s| s.owner = OwnerRef::parse("another-owner").unwrap()),
        Box::new(|s| s.owner_revision = SourceRevision::parse("producer-v2").unwrap()),
        Box::new(|s| s.generation = SourceRevision::parse("generation-2").unwrap()),
        Box::new(|s| s.interpretation_revision = SourceRevision::parse("profile-v2").unwrap()),
        Box::new(|s| s.sources[0].revision = SourceRevision::parse("source-revision-2").unwrap()),
        Box::new(|s| s.method_skill = Some(ResourceRef::parse("skill/recognised").unwrap())),
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
    let scoped = resolve_scoped_expression(&ScopedResolveExpression::unbound(expression), &index, 16).unwrap();
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
    request.scopes.push(ExpressionScope { node: child_path.clone(), binding: child.clone() });
    let left = vec![ExpressionEdge::Operand, ExpressionEdge::Left];
    assert_eq!(request.effective_scopes(&left).unwrap()[0].binding, binding());
    assert_eq!(request.effective_scopes(&child_path).unwrap()[0].binding, child);
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
    request.scopes[0].binding.sources.push(request.scopes[0].binding.sources[0].clone());
    assert!(request.canonical().is_err());
    let mut encoded = serde_json::to_value(super::tests::request()).unwrap();
    encoded["scopes"][0]["binding"]["world"] = serde_json::json!("");
    let invalid: ScopedResolveExpression = serde_json::from_value(encoded).unwrap();
    assert!(invalid.identity().is_err());
}

#[test]
fn qualified_familiarity_uses_the_same_native_ranking_law() {
    struct RecordingIndex { index: MemoryResourceIndex, paths: RefCell<Vec<String>> }
    impl ResourceIndex for RecordingIndex {
        fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord> { self.index.resource(id) }
        fn resources(&self) -> Vec<&ResourceRecord> { self.index.resources() }
        fn resolve_path_ranking(&self, path: &str, _: &ResourceRef) -> ResolveRankingSignals {
            self.paths.borrow_mut().push(path.into());
            ResolveRankingSignals::default()
        }
    }
    let index = RecordingIndex { index: index(), paths: RefCell::new(vec![]) };
    let request = request();
    let resolved = resolve_scoped_expression(&request, &index, 16).unwrap();
    assert!(!index.paths.borrow().is_empty());
    assert!(index.paths.borrow().iter().all(|id| id == &resolved.path.identity));
    assert_eq!(resolved.path.candidates, resolve_expression(&request.expression, &index.index, 16).candidates);
}

#[test]
fn structural_budget_covers_structured_clients_too() {
    let mut expression = ResolveExpression::subject("subject");
    for _ in 0..66 { expression = ResolveExpression::Frame { expression: Box::new(expression) }; }
    assert!(ScopedResolveExpression::unbound(expression).canonical().is_err());
    let mut request = request();
    request.scopes[0].binding.sources.clear();
    assert!(request.canonical().is_err());
}
