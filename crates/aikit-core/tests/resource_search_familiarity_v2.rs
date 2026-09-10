use aikit_core::resource::{
    NavigationEvidence, NavigationEvidenceClass, PreferenceIntent, ResourceDescriptor, ResourceKind,
    ResourceRecord, ResourceRef, ResourceSearchIndex, SourceRef,
};
use aikit_core::{FamiliarityContext, FamiliarityObservation, FamiliarityStore};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn record(id: &str, name: &str) -> ResourceRecord {
    ResourceRecord::new(ResourceDescriptor::new(
        r(id),
        ResourceKind::Capability,
        name,
        "test resource",
    ))
}

fn learned(store: &mut FamiliarityStore, id: &str, destination: &str, count: usize) {
    for index in 0..count {
        store
            .record(FamiliarityObservation::destination(
                format!("{id}/{index}"),
                r(destination),
                FamiliarityContext::default(),
                1_000 + index as u64,
            ))
            .unwrap();
    }
}

#[test]
fn text_match_quality_remains_above_learned_familiarity() {
    let mut index = ResourceSearchIndex::default();
    index.insert_resource(record("capability/exact", "deploy"), vec![]);
    index.insert_resource(record("capability/familiar", "deep deployment helper"), vec![]);
    let mut familiarity = FamiliarityStore::new();
    learned(&mut familiarity, "evt/familiar", "capability/familiar", 100);
    index.apply_familiarity(
        &familiarity,
        &FamiliarityContext::default(),
        2_000,
        1_000_000,
    );

    let hits = index.search("deploy", 10);

    assert_eq!(hits[0].resource, r("capability/exact"));
    assert_eq!(hits[1].resource, r("capability/familiar"));
    assert!(hits[1].ranking.learned_contextual_frecency_milli > 0);
}

#[test]
fn authored_preference_remains_above_learned_accessibility_on_equal_text_matches() {
    let mut preferred = record("capability/preferred", "review");
    preferred.preference = Some(PreferenceIntent {
        source: SourceRef::parse("source:user").unwrap(),
        rank: 10,
        rationale: Some("explicit user choice".into()),
    });
    let mut index = ResourceSearchIndex::default();
    index.insert_resource(preferred, vec![]);
    index.insert_resource(record("capability/familiar", "review"), vec![]);
    let mut familiarity = FamiliarityStore::new();
    learned(&mut familiarity, "evt/familiar", "capability/familiar", 100);
    index.apply_familiarity(
        &familiarity,
        &FamiliarityContext::default(),
        2_000,
        1_000_000,
    );

    let hits = index.search("review", 10);

    assert_eq!(hits[0].resource, r("capability/preferred"));
    assert_eq!(hits[0].ranking.authored_preference_rank, Some(10));
    assert_eq!(hits[1].resource, r("capability/familiar"));
    assert!(hits[1].ranking.learned_observations > 0);
}

#[test]
fn learned_accessibility_breaks_otherwise_equal_search_ties_and_explains_itself() {
    let mut index = ResourceSearchIndex::default();
    index.insert_resource(record("capability/a", "review"), vec![]);
    index.insert_resource(record("capability/b", "review"), vec![]);
    let mut familiarity = FamiliarityStore::new();
    learned(&mut familiarity, "evt/b", "capability/b", 4);
    index.apply_familiarity(
        &familiarity,
        &FamiliarityContext::default(),
        2_000,
        1_000_000,
    );

    let hits = index.search("review", 10);

    assert_eq!(hits[0].resource, r("capability/b"));
    assert!(hits[0]
        .navigation_evidence
        .iter()
        .any(|evidence| evidence.class == NavigationEvidenceClass::LearnedUsage
            && evidence
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("4 observed uses"))));
}

#[test]
fn zero_query_labels_familiarity_as_learned_evidence_instead_of_preference() {
    let mut index = ResourceSearchIndex::default();
    index.insert_resource(
        record("capability/current", "current"),
        vec![NavigationEvidence::new(
            NavigationEvidenceClass::CurrentContext,
        )],
    );
    index.insert_resource(record("capability/familiar", "familiar"), vec![]);
    let mut familiarity = FamiliarityStore::new();
    learned(&mut familiarity, "evt/familiar", "capability/familiar", 2);
    index.apply_familiarity(
        &familiarity,
        &FamiliarityContext::default(),
        2_000,
        1_000_000,
    );

    let hits = index.search("", 10);

    assert_eq!(hits[0].resource, r("capability/current"));
    let familiar = hits
        .iter()
        .find(|hit| hit.resource == r("capability/familiar"))
        .unwrap();
    assert_eq!(familiar.ranking.authored_preference_rank, None);
    assert!(familiar
        .navigation_evidence
        .iter()
        .any(|evidence| evidence.class == NavigationEvidenceClass::LearnedUsage));
    assert!(!familiar
        .navigation_evidence
        .iter()
        .any(|evidence| evidence.class == NavigationEvidenceClass::ExplicitPin));
}
