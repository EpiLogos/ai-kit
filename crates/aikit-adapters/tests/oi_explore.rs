//! O:I Explore discovery materialisation — the SharedField SF4 contract.
//!
//! The discovery seed (`oi.explore-discovery/v1`) is what one O:I world's
//! rebuildable Explore index exports; these cases assert that AIKit
//! materialises it as ordinary SemanticWiki objects over the SAME stable
//! semantic refs — entries as nodes, admitted relations as edges, the
//! structured presentation join as derived `presents` edges, SharedField
//! membership as field nodes with `projected-in` edges — and that malformed
//! or absent material is disclosed, never invented.

use aikit_adapters::oi_explore::{
    discovery_seed_path, materialise_explore_discovery, read_explore_discovery,
    EXPLORE_DISCOVERY_SCHEMA, EXPLORE_EXTENSION, RELATION_PRESENTS, RELATION_PROJECTED_IN,
};
use aikit_core::{SemanticWikiIndex, WikiObject};
use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;

const SUBJECT: &str = "wiki:node:harbour:quay";
const BEING: &str = "central:pasu:agent:epii";
const EXPRESSION: &str = "expression:harbour:quay-light";
const PRESENTATION: &str = "presentation:harbour:quay-light";
const FIELD: &str = "oi:field:harbour";
const WORLD: &str = "world:harbour";

fn discovery_seed() -> Value {
    json!({
        "schema": EXPLORE_DISCOVERY_SCHEMA,
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
}

fn node_refs(objects: &[WikiObject]) -> Vec<String> {
    objects
        .iter()
        .filter_map(|object| match object {
            WikiObject::Node(node) => Some(node.ref_id.to_string()),
            _ => None,
        })
        .collect()
}

fn edges(objects: &[WikiObject]) -> Vec<&aikit_core::WikiEdge> {
    objects
        .iter()
        .filter_map(|object| match object {
            WikiObject::Edge(edge) => Some(edge),
            _ => None,
        })
        .collect()
}

#[test]
fn entries_materialise_as_nodes_carrying_their_own_stable_refs() {
    let reading = materialise_explore_discovery(&discovery_seed());
    assert!(
        reading.absences.is_empty(),
        "a well-formed seed discloses no absences: {:?}",
        reading.absences
    );
    let refs = node_refs(&reading.objects);
    for expected in [WORLD, SUBJECT, BEING, EXPRESSION, PRESENTATION, FIELD] {
        assert!(
            refs.contains(&expected.to_string()),
            "stable ref {expected} is addressable verbatim: {refs:?}"
        );
    }
    // The entry's own kind is the node grammar; explore facts ride the extension.
    let quay = reading
        .objects
        .iter()
        .find_map(|object| match object {
            WikiObject::Node(node) if node.ref_id.as_str() == SUBJECT => Some(node),
            _ => None,
        })
        .expect("quay node");
    assert_eq!(quay.node_type, "wiki-node");
    assert_eq!(quay.title.as_deref(), Some("The Quay wall"));
    let extension = quay.extensions.get(EXPLORE_EXTENSION).expect("extension");
    assert_eq!(extension["kind"], "wiki-node");
    assert_eq!(extension["aliases"][0], "quay-wall");
    // The admitted alias also rides the conventional top-level `aliases`
    // extension, so SemanticWikiIndex alias search resolves the entry.
    assert_eq!(quay.extensions["aliases"][0], "quay-wall");
    let expression = reading
        .objects
        .iter()
        .find_map(|object| match object {
            WikiObject::Node(node) if node.ref_id.as_str() == EXPRESSION => Some(node),
            _ => None,
        })
        .expect("expression node");
    assert_eq!(expression.node_type, "expression");
    let extension = expression.extensions.get(EXPLORE_EXTENSION).expect("extension");
    assert_eq!(
        extension["projection_ref"],
        "projection:harbour:quay-light",
        "the Projection identity an entry names stays disclosed"
    );
}

#[test]
fn an_admitted_alias_is_searchable_over_the_canonical_entry_ref() {
    let reading = materialise_explore_discovery(&discovery_seed());
    let index = SemanticWikiIndex::rebuild(reading.objects.clone())
        .expect("materialised seed rebuilds into the SemanticWiki");

    let hits = index.search("quay-wall", 10);
    assert_eq!(hits.len(), 1, "exactly the canonical entry matches: {:?}", hits
        .iter()
        .map(|hit| hit.label.clone())
        .collect::<Vec<_>>());
    assert_eq!(
        hits[0].address.as_curated().expect("curated hit").as_str(),
        SUBJECT,
        "the alias resolves the canonical entry, never a second identity"
    );
}

#[test]
fn the_presentation_join_becomes_derived_typed_presents_edges() {
    let reading = materialise_explore_discovery(&discovery_seed());
    let presents = edges(&reading.objects)
        .into_iter()
        .filter(|edge| edge.relation == RELATION_PRESENTS)
        .collect::<Vec<_>>();
    assert_eq!(
        presents.len(),
        3,
        "one presents-edge per bound subject plus the presentation's own world whole"
    );
    assert!(
        presents
            .iter()
            .any(|edge| edge.to_ref.as_str() == EXPRESSION),
        "the Expression whole its WorldPresentation presents is traversable"
    );
    let roles: BTreeMap<String, String> = presents
        .iter()
        .filter_map(|edge| {
            let extension = edge.extensions.get(EXPLORE_EXTENSION).expect("extension");
            let role = extension["role"].as_str()?;
            Some((edge.to_ref.to_string(), role.to_owned()))
        })
        .collect();
    assert_eq!(roles.get(SUBJECT).map(String::as_str), Some("thing"));
    assert_eq!(roles.get(BEING).map(String::as_str), Some("being"));
    for edge in &presents {
        assert_eq!(edge.from_ref.as_str(), PRESENTATION);
        let extension = edge.extensions.get(EXPLORE_EXTENSION).expect("extension");
        assert_eq!(extension["projection_ref"], "projection:harbour:quay-light");
        assert_eq!(extension["projection_state"], "published");
    }
}

#[test]
fn shared_field_membership_becomes_field_nodes_and_projected_in_edges() {
    let reading = materialise_explore_discovery(&discovery_seed());
    let membership = edges(&reading.objects)
        .into_iter()
        .filter(|edge| edge.relation == RELATION_PROJECTED_IN)
        .collect::<Vec<_>>();
    assert_eq!(membership.len(), 3, "one projected-in edge per hosted entry");
    assert!(membership.iter().all(|edge| edge.to_ref.as_str() == FIELD));
    let field = reading
        .objects
        .iter()
        .find_map(|object| match object {
            WikiObject::Node(node) if node.ref_id.as_str() == FIELD => Some(node),
            _ => None,
        })
        .expect("field node");
    assert_eq!(field.node_type, "shared-field");
    assert_eq!(field.title.as_deref(), Some("Harbour field"));
}

#[test]
fn unavailable_or_malformed_material_is_disclosed_never_invented() {
    let mut seed = discovery_seed();
    // A presentation naming a subject with no indexed entry.
    seed["presentations"][0]["subjects"][0]["ref"] = json!("wiki:node:ghost");
    // A relation with an unavailable endpoint.
    seed["relations"][0]["to"] = json!("wiki:node:ghost");
    // An entry with an invalid ref.
    seed["entries"][0]["ref"] = json!(" ");
    let reading = materialise_explore_discovery(&seed);
    assert!(
        reading.absences.len() >= 3,
        "each refusal is disclosed: {:?}",
        reading.absences
    );
    assert!(
        !node_refs(&reading.objects).iter().any(|r| r == "wiki:node:ghost"),
        "a ghost subject is never materialised"
    );
    assert!(
        !edges(&reading.objects)
            .iter()
            .any(|edge| edge.relation == RELATION_PRESENTS && edge.to_ref.as_str() == "wiki:node:ghost"),
        "the ghost presents-edge is omitted"
    );
}

#[test]
fn a_seed_without_the_explore_contract_is_refused_honestly() {
    let seed = json!({"schema": "something/else/v1", "entries": []});
    let reading = materialise_explore_discovery(&seed);
    assert!(reading.objects.is_empty());
    assert_eq!(reading.absences.len(), 1);
    assert!(reading.absences[0].contains(EXPLORE_DISCOVERY_SCHEMA));
}

#[test]
fn an_absent_seed_is_an_ordinary_absence_and_an_unreadable_one_is_disclosed() {
    let temp = TempDir::new().unwrap();
    // No environment override, no seed on the default path: None.
    std::env::remove_var("OI_EXPLORE_DISCOVERY");
    assert!(discovery_seed_path(temp.path()).is_none());

    // An explicit override pointing at unreadable bytes discloses, never panics.
    let seed_path = temp.path().join("explore-discovery.json");
    fs::write(&seed_path, "{not json").unwrap();
    std::env::set_var("OI_EXPLORE_DISCOVERY", &seed_path);
    assert_eq!(discovery_seed_path(temp.path()), Some(seed_path.clone()));
    let error = match read_explore_discovery(&seed_path) {
        Err(error) => error,
        Ok(_) => panic!("unreadable seed must be disclosed, not parsed"),
    };
    assert!(error.contains("not valid JSON"), "{error}");
    std::env::remove_var("OI_EXPLORE_DISCOVERY");
}

use std::collections::BTreeMap;
