//! Search/discovery over the projected Explore field (SharedField SF4).
//!
//! The proving chain from the state/discovery lock, walked through the real
//! application service over one discovery seed exported by a projected
//! world's rebuildable Explore index:
//!
//! search a native/projected subject → see the eligible presentation forms
//! (the Expression and its WorldPresentation as their own addressable
//! results) → open the bounded local whole → traverse the derived typed
//! relations to the presented subjects → open the Expression → and have
//! History (familiarity) record and return through the exact same refs.
//!
//! Another World resolves the same semantic refs: the seed the test plants is
//! the byte-shaped export of O-I's `discoverySeed()`, and every ref the
//! service resolves is verbatim from that seed — never a transport or
//! materialised row ID.

use std::fs;

use aikit_cli::app::Service;
use aikit_core::ResourceRef;
use aikit_store::AikitHome;
use tempfile::TempDir;

const SUBJECT: &str = "wiki:node:harbour:quay";
const BEING: &str = "central:pasu:agent:epii";
const EXPRESSION: &str = "expression:harbour:quay-light";
const PRESENTATION: &str = "presentation:harbour:quay-light";
const FIELD: &str = "oi:field:harbour";
const WORLD: &str = "world:harbour";

/// The exact shape O-I's `createExploreSurfaceModel::discoverySeed()` exports.
fn seed_json() -> String {
    serde_json::json!({
        "schema": "oi.explore-discovery/v1",
        "generated_from": "oi.explore-browser-seed/v1",
        "revision": "explore-discovery/v1:4:1:1",
        "entries": [
            {"schema": "oi.explore-entry/v1", "ref": WORLD, "kind": "central-world", "world_ref": WORLD,
             "label": "Harbour — a ProjectCentral world", "aliases": [],
             "provenance": [{"kind": "fixture", "ref": "fixture:world", "source_system": "fixture", "revision": "1"}],
             "locators": [], "meta": {"standing": "projection"}},
            {"schema": "oi.explore-entry/v1", "ref": SUBJECT, "kind": "wiki-node", "world_ref": WORLD,
             "label": "The Quay wall", "aliases": ["quay-wall"], "revision": "2",
             "provenance": [{"kind": "fixture", "ref": "fixture:quay", "source_system": "fixture", "revision": "2"}],
             "locators": [], "meta": {}},
            {"schema": "oi.explore-entry/v1", "ref": BEING, "kind": "agent", "world_ref": WORLD,
             "label": "Epii", "aliases": [],
             "provenance": [{"kind": "fixture", "ref": "fixture:epii", "source_system": "fixture"}],
             "locators": [], "meta": {}},
            {"schema": "oi.explore-entry/v1", "ref": EXPRESSION, "kind": "expression", "world_ref": WORLD,
             "label": "Quay light", "aliases": ["projection:harbour:quay-light"], "revision": "3",
             "provenance": [{"kind": "fixture", "ref": "fixture:expression", "source_system": "fixture", "revision": "3"}],
             "locators": [], "meta": {"projection_ref": "projection:harbour:quay-light"}}
        ],
        "relations": [
            {"relation_ref": format!("{WORLD}#oi.world/wiki-node#{SUBJECT}"), "from": WORLD, "to": SUBJECT,
             "relation": "oi.world/wiki-node", "origin": "projection", "direction": "forward",
             "provenance": [{"kind": "fixture-relation", "ref": "fixture:relation", "source_system": "fixture", "revision": "1"}]}
        ],
        "presentations": [
            {"presentation_ref": PRESENTATION, "kind": "expression", "world_ref": EXPRESSION,
             "revision": 2, "title": "Quay light",
             "projection_ref": "projection:harbour:quay-light", "projection_revision": 2,
             "projection_state": "published",
             "subjects": [
                 {"ref": SUBJECT, "role": "thing", "availability": "available"},
                 {"ref": BEING, "role": "being", "availability": "available"}
             ]}
        ],
        "membership": {
            "entry_fields": {SUBJECT: FIELD, BEING: FIELD, EXPRESSION: FIELD},
            "relation_fields": {}
        },
        "fields": [{"field_ref": FIELD, "kind": "explore", "visibility": "public", "title": "Harbour field"}]
    })
    .to_string()
}

fn open_service_with_projected_world() -> (Service, TempDir) {
    let temp = TempDir::new().unwrap();
    let seed_path = temp
        .path()
        .join("Work/O-I/site/public/data/explore-discovery.json");
    fs::create_dir_all(seed_path.parent().unwrap()).unwrap();
    fs::write(&seed_path, seed_json()).unwrap();
    fs::create_dir_all(temp.path().join("Work/Probe")).unwrap();
    fs::create_dir_all(temp.path().join("Control")).unwrap();
    let home = AikitHome::at(temp.path().join("aikit-home"));
    // The session stands at the ROOT register (the Central root itself), the
    // register whose concern the projected field is. A narrower Project
    // context whose owner policy is unavailable withholds central-adopted
    // material instead — the pre-existing withhold law stays in force; the
    // discovery seed is adopted inside it, not around it.
    let service = Service::open(home, temp.path(), |_| None)
        .expect("open production application service");
    (service, temp)
}

#[test]
fn search_reveals_presentations_and_history_returns_through_the_same_refs() {
    std::env::remove_var("OI_EXPLORE_DISCOVERY");
    let (mut service, _temp) = open_service_with_projected_world();

    // 1. Search the native subject: the Thing AND its eligible Expression
    //    presentation are distinct results over stable refs.
    let result = service.knowledge_search("quay", 50).unwrap();
    let resources: Vec<String> = result
        .hits
        .iter()
        .map(|hit| hit.resource.to_string())
        .collect();
    assert!(
        resources.iter().any(|ref_id| ref_id == SUBJECT),
        "the native Thing is discoverable: {resources:?}"
    );
    assert!(
        resources.iter().any(|ref_id| ref_id == EXPRESSION),
        "the Expression presenting it is its own discoverable result: {resources:?}"
    );

    // 2. Open the Expression: the ordinary operation resolves the projected
    //    ref, and the use is recorded as familiarity on THAT ref.
    let expression_ref = ResourceRef::parse(EXPRESSION).unwrap();
    let receipt = service.knowledge_open(&expression_ref).unwrap();
    assert_eq!(receipt.opened.as_str(), EXPRESSION);
    assert_eq!(receipt.recorded, "familiarity/resource-use");
    assert!(
        !receipt.observation_id.is_empty(),
        "History records the successful use of the projected ref"
    );

    // 3. The bounded local whole of the Expression carries the derived typed
    //    relations: the presentation join and the SharedField membership.
    let view = service
        .knowledge_relations(&receipt.address, 2, 96, 192)
        .unwrap();
    let relations: Vec<&str> = view
        .edges
        .iter()
        .map(|edge| edge.relation.as_str())
        .collect();
    assert!(
        relations.contains(&"oi.presentation/presents"),
        "the presentation join traverses as a typed relation: {relations:?}"
    );
    assert!(
        relations.contains(&"oi.explore/projected-in"),
        "the live SharedField occurrence traverses as a typed relation: {relations:?}"
    );
    let nodes: Vec<String> = view.nodes.iter().map(|node| node.resource.to_string()).collect();
    assert!(
        nodes.iter().any(|ref_id| ref_id == PRESENTATION),
        "the WorldPresentation is a node of the bounded whole: {nodes:?}"
    );
    assert!(
        nodes.iter().any(|ref_id| ref_id == FIELD),
        "the hosting SharedField is a node of the bounded whole: {nodes:?}"
    );

    // 4. Traverse to the presented subjects through the same adjacency.
    let subjects: Vec<String> = view
        .edges
        .iter()
        .filter(|edge| edge.relation == "oi.presentation/presents")
        .map(|edge| edge.to.to_string())
        .collect();
    assert!(
        subjects.contains(&SUBJECT.to_string()) && subjects.contains(&BEING.to_string()),
        "the presented Thing and Being are reachable one hop from the presentation: {subjects:?}"
    );

    // 5. Another World resolves the same semantic refs: every ref above came
    //    verbatim from the seed, and re-opening the subject by its stable ref
    //    records its own use — the refs are world-portable addresses.
    let subject_ref = ResourceRef::parse(SUBJECT).unwrap();
    let subject_receipt = service.knowledge_open(&subject_ref).unwrap();
    assert_eq!(subject_receipt.opened.as_str(), SUBJECT);
    assert_ne!(
        subject_receipt.observation_id, receipt.observation_id,
        "each use of a portable ref records its own History entry"
    );
}
