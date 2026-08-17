mod common;

use common::*;

use aikit_core::resource::ResourceRef;
use aikit_tui::{ApplicationService, TuiApplicationService};

#[test]
fn capability_relation_read_model_uses_canonical_contextual_actions() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(dir.path(), vec![skill("skill/rust/review")]);
    let focus = ResourceRef::parse("skill/rust/review").unwrap();
    let service = ApplicationService::new(&mut backend);

    let view = service.relations(&focus).unwrap();

    assert_eq!(view.subject, focus);
    let actions = view.value["contextualActions"]
        .as_array()
        .expect("relation model must expose contextual Actions");
    assert_eq!(
        actions.len(),
        2,
        "Explain and Toggle are contextual Actions"
    );
    assert!(actions.iter().any(|action| {
        action.get("action").and_then(|value| value.as_str()) == Some("action/capability/explain")
    }));
    assert!(actions.iter().any(|action| {
        action.get("action").and_then(|value| value.as_str()) == Some("action/capability/toggle")
    }));
}

#[test]
fn resource_without_typed_relations_does_not_invent_edges() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(dir.path(), vec![skill("skill/rust/review")]);
    let host = ResourceRef::parse("host/test-host").unwrap();
    let service = ApplicationService::new(&mut backend);

    let view = service.relations(&host).unwrap();

    assert_eq!(view.subject, host);
    assert!(view.value["contextualActions"]
        .as_array()
        .is_some_and(Vec::is_empty));
    assert!(view.value["related"].as_array().is_some_and(Vec::is_empty));
    assert!(view.value["resolverRelated"]
        .as_array()
        .is_some_and(Vec::is_empty));
}
