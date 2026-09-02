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
    for expected in [
        "action/capability/explain",
        "action/capability/toggle",
        "action/aikit/explain",
        "action/aikit/history",
    ] {
        assert!(
            actions.iter().any(|action| {
                action.get("action").and_then(|value| value.as_str()) == Some(expected)
            }),
            "missing canonical contextual Action {expected}"
        );
    }
}

#[test]
fn resource_without_typed_relations_does_not_invent_edges() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(dir.path(), vec![skill("skill/rust/review")]);
    let host = ResourceRef::parse("host/test-host").unwrap();
    let service = ApplicationService::new(&mut backend);

    let view = service.relations(&host).unwrap();

    assert_eq!(view.subject, host);
    let actions = view.value["contextualActions"]
        .as_array()
        .expect("common read-only Actions remain available without typed relations");
    assert!(actions.iter().any(|action| {
        action.get("action").and_then(|value| value.as_str()) == Some("action/aikit/explain")
    }));
    assert!(actions.iter().any(|action| {
        action.get("action").and_then(|value| value.as_str()) == Some("action/aikit/history")
    }));
    assert!(view.value["related"].as_array().is_some_and(Vec::is_empty));
    assert!(view.value["resolverRelated"]
        .as_array()
        .is_some_and(Vec::is_empty));
}
