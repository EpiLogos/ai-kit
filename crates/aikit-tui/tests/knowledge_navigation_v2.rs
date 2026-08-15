mod common;

use common::*;

use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::RelationQuery;
use aikit_tui::{KnowledgeNavigationService, PaletteApplicationService};

#[test]
fn capability_relation_view_uses_canonical_contextual_action_edges() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(dir.path(), vec![skill("skill/rust/review")]);
    let focus = ResourceRef::parse("skill/rust/review").unwrap();
    let service = PaletteApplicationService::new(&mut backend);

    let view = service.relation_view(RelationQuery::local(focus.clone())).unwrap();

    assert_eq!(view.query.focus, focus);
    assert_eq!(view.nodes[0].resource.as_str(), "skill/rust/review");
    assert_eq!(view.edges.len(), 2, "Explain and Toggle are contextual Actions");
    assert!(view
        .edges
        .iter()
        .all(|edge| edge.relation == "contextual-action"));
    assert!(view
        .edges
        .iter()
        .all(|edge| edge.origin.authority == SourceAuthority::Generated));
    assert!(view.edges.iter().all(|edge| {
        edge.origin.lens.as_deref() == Some("aikit-resource-index")
    }));
}

#[test]
fn resource_without_typed_relations_reports_absence_instead_of_inventing_edges() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(dir.path(), vec![skill("skill/rust/review")]);
    let host = ResourceRef::parse("host/test-host").unwrap();
    let service = PaletteApplicationService::new(&mut backend);

    let view = service.relation_view(RelationQuery::local(host)).unwrap();

    assert_eq!(view.nodes.len(), 1);
    assert!(view.edges.is_empty());
    assert!(view
        .warnings
        .iter()
        .any(|warning| warning.contains("provider-native relations were not inferred")));
}
