use std::time::{Duration, Instant};

use aikit_core::resource::{
    ActionStageability, ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass,
    ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef, ResourceSearchHitKind,
    ResourceSearchIndex,
};

fn rref(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn record(id: &str, kind: ResourceKind, name: &str) -> ResourceRecord {
    ResourceRecord::new(ResourceDescriptor::new(
        rref(id),
        kind,
        name,
        format!("{name} navigation resource"),
    ))
}

#[test]
fn v2_navigation_field_includes_human_shell_and_future_knowledge_destinations() {
    let kinds = [
        ResourceKind::Project,
        ResourceKind::Profile,
        ResourceKind::SkillSet,
        ResourceKind::Capability,
        ResourceKind::Action,
        ResourceKind::Procedure,
        ResourceKind::Agent,
        ResourceKind::Agency,
        ResourceKind::ContextSource,
        ResourceKind::Model,
        ResourceKind::Harness,
        ResourceKind::Host,
        ResourceKind::ExecutionOffer,
        ResourceKind::KnowledgeSpace,
        ResourceKind::KnowledgeNode,
        ResourceKind::KnowledgeSource,
        ResourceKind::KnowledgeFrame,
        ResourceKind::KnowledgeRoute,
        ResourceKind::CodeReference,
    ];

    let names: Vec<_> = kinds.into_iter().map(ResourceKind::as_str).collect();
    assert_eq!(names.len(), 19);
    assert!(names.contains(&"project"));
    assert!(names.contains(&"procedure"));
    assert!(names.contains(&"knowledge-route"));
}

#[test]
fn zero_query_results_are_evidence_bearing_not_implicit_recommendations() {
    let mut index = ResourceSearchIndex::default();
    let learned = record("project/learned", ResourceKind::Project, "Learned project");
    assert!(learned.preference.is_none());
    index.insert_resource(
        learned,
        vec![NavigationEvidence::new(NavigationEvidenceClass::LearnedUsage)
            .with_detail("opened 7 times in this project scope")],
    );
    index.insert_resource(
        record("project/current", ResourceKind::Project, "Current project"),
        vec![NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)],
    );
    index.insert_resource(
        record("project/unseen", ResourceKind::Project, "No navigation evidence"),
        Vec::new(),
    );

    let hits = index.search("", 20);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].resource, rref("project/current"));
    assert_eq!(hits[1].resource, rref("project/learned"));
    assert_eq!(
        hits[1].navigation_evidence[0].class,
        NavigationEvidenceClass::LearnedUsage
    );
    assert!(hits.iter().all(|hit| hit.resource != rref("project/unseen")));
}

#[test]
fn contextual_actions_are_resource_derived_searchable_and_explicitly_stageable() {
    let mut index = ResourceSearchIndex::default();
    index.insert_resource(
        record("project/aikit", ResourceKind::Project, "AIKit"),
        Vec::new(),
    );
    index.insert_resource(
        record("action/project/open", ResourceKind::Action, "Open project"),
        Vec::new(),
    );
    index.insert_resource(
        record("action/project/pin", ResourceKind::Action, "Pin project"),
        Vec::new(),
    );

    index
        .insert_action(ContextualActionDescriptor::new(
            rref("action/project/open"),
            rref("project/aikit"),
            "Open AIKit workspace",
            "Enter the project without mutating composition",
            ActionStageability::NotStageable,
        ))
        .unwrap();
    index
        .insert_action(ContextualActionDescriptor::new(
            rref("action/project/pin"),
            rref("project/aikit"),
            "Pin AIKit",
            "Stage an explicit navigation pin",
            ActionStageability::Stageable,
        ))
        .unwrap();

    let hits = index.search("open workspace", 20);
    let action = hits
        .iter()
        .find(|hit| hit.hit_kind == ResourceSearchHitKind::ContextualAction)
        .expect("contextual action should be searchable");
    assert_eq!(action.resource, rref("action/project/open"));
    assert_eq!(action.subject, Some(rref("project/aikit")));
    assert_eq!(action.stageability, Some(ActionStageability::NotStageable));

    let actions = index.actions_for(&rref("project/aikit"));
    assert_eq!(actions.len(), 2);
    assert_eq!(
        actions.iter().filter(|action| action.stageability.is_stageable()).count(),
        1,
        "Space may only stage actions which declare that semantic explicitly"
    );
}

#[test]
fn contextual_action_requires_canonical_action_and_subject_resources() {
    let mut index = ResourceSearchIndex::default();
    index.insert_resource(
        record("project/aikit", ResourceKind::Project, "AIKit"),
        Vec::new(),
    );
    index.insert_resource(
        record("capability/open", ResourceKind::Capability, "Open helper"),
        Vec::new(),
    );

    let wrong_kind = index.insert_action(ContextualActionDescriptor::new(
        rref("capability/open"),
        rref("project/aikit"),
        "Open",
        "wrong kind",
        ActionStageability::NotStageable,
    ));
    assert_eq!(
        wrong_kind.unwrap_err().code(),
        "resource.search_wrong_action_kind"
    );

    let missing = index.insert_action(ContextualActionDescriptor::new(
        rref("action/missing"),
        rref("project/aikit"),
        "Missing",
        "not indexed",
        ActionStageability::NotStageable,
    ));
    assert_eq!(missing.unwrap_err().code(), "resource.search_unknown_action");
}

#[test]
fn shallow_fuzzy_search_stays_fast_over_a_large_resource_field() {
    let mut index = ResourceSearchIndex::default();
    for number in 0..20_000 {
        let id = format!("capability/synthetic/{number:05}");
        let name = if number == 19_731 {
            "wayfinder semantic atlas target".to_string()
        } else {
            format!("ordinary capability {number:05}")
        };
        index.insert_resource(record(&id, ResourceKind::Capability, &name), Vec::new());
    }

    let started = Instant::now();
    let hits = index.search("wsm atlas", 20);
    let elapsed = started.elapsed();

    assert_eq!(index.len(), 20_000);
    assert!(hits.iter().any(|hit| hit.resource == rref("capability/synthetic/19731")));
    assert!(
        elapsed < Duration::from_secs(2),
        "shallow search took {elapsed:?}; Quick must not wait on deep providers"
    );
}
