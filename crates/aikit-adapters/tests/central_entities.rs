//! W10 V3 — compiled entity materialisation regression halves.
//!
//! CASE 15 half: an identity-source edit changes the nara entity's sourced
//! relations and never mints a second subject.
//! CASE 16 half: an AgentProfile revision change re-relates the same agent
//! entity. CASE 17 half: an authored AgentSet materialises with Compiled
//! membership edges that survive an availability-shaped world (the authored
//! record is untouched by resolution).

use aikit_adapters::central_entities::materialise_central_entities;
use aikit_core::{SemanticWikiIndex, WikiObject};
use serde_json::json;
use std::{fs, path::PathBuf, sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};

fn fixture_central() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "aikit-central-entities-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    let central = root.join("Central");
    fs::create_dir_all(central.join("Control/user/identity/sources")).unwrap();
    fs::create_dir_all(central.join("Control/agents/profiles")).unwrap();
    fs::create_dir_all(central.join("Control/relations/agent-sets")).unwrap();
    fs::write(
        central.join("Control/user/identity/present.md"),
        "who I am now\n",
    )
    .unwrap();
    fs::write(
        central.join("Control/user/identity/manifest.json"),
        json!({
            "schema": "central.pasu.identity-manifest/v1",
            "revision": "1",
            "subject": {"ref": "central:pasu:nara:local", "title": "Nara identity source"},
            "identity_source": {
                "path": "Control/user/identity",
                "provenance_law": "vault-first",
                "sources": [
                    {"path": "Control/user/identity/present.md", "standing": "authored-ground"}
                ]
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        central.join("Control/agents/profiles/profile-hermes.json"),
        json!({
            "schema": "central.agent-profile/v1",
            "profile_ref": "agent-profile:hermes",
            "agent_ref": "agent:hermes",
            "revision": "p1",
            "scope": "personal"
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        central.join("Control/relations/agent-sets/agent-set-operators.json"),
        json!({
            "schema": "central.agent-set/v1",
            "ref": "central-operators",
            "revision": "r1",
            "members": [
                {"kind": "agent", "agent_ref": "agent:hermes"},
                {"kind": "agent", "agent_ref": "agent:unprofiled"}
            ]
        })
        .to_string(),
    )
    .unwrap();
    central
}

fn node_refs(objects: &[WikiObject]) -> Vec<String> {
    objects
        .iter()
        .filter_map(|object| match object {
            WikiObject::Node(node) => Some(node.ref_id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn case15_half_identity_source_edit_changes_relations_never_the_entity_ref() {
    let central = fixture_central();

    let first = materialise_central_entities(&central);
    // Member disclosures are honest absence data; carrier errors are not.
    assert!(!first
        .absences
        .iter()
        .any(|absence| absence.contains("carrier")), "{:?}", first.absences);
    let nara_first = first
        .objects
        .iter()
        .find_map(|object| match object {
            WikiObject::Node(node) if node.ref_id.as_str() == "wiki:node:identity" => {
                Some(node.clone())
            }
            _ => None,
        })
        .expect("nara entity materialises");
    assert_eq!(nara_first.node_type, "pasu");
    let form = nara_first.extensions["aikit.pasu/v1"]["form"].as_str().unwrap();
    assert_eq!(form, "nara");
    let revision_first = nara_first.extensions["aikit.pasu/v1"]["extra"]["sourced"][0]
        ["content_revision"]
        .as_str()
        .unwrap()
        .to_owned();

    // The identity source is edited in place; the manifest revision is
    // untouched. Same subject, changed sourced relations.
    fs::write(central.join("Control/user/identity/present.md"), "who I am now, revised\n").unwrap();
    let second = materialise_central_entities(&central);
    let nara_second = second
        .objects
        .iter()
        .find_map(|object| match object {
            WikiObject::Node(node) if node.ref_id.as_str() == "wiki:node:identity" => {
                Some(node.clone())
            }
            _ => None,
        })
        .expect("nara entity still materialises");
    let revision_second = nara_second.extensions["aikit.pasu/v1"]["extra"]["sourced"][0]
        ["content_revision"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(nara_second.ref_id, nara_first.ref_id, "no second subject");
    assert_ne!(revision_second, revision_first, "relations changed");
    assert_eq!(node_refs(&second.objects), node_refs(&first.objects));
}

#[test]
fn case16_half_profile_revision_change_relinks_the_same_agent_entity() {
    let central = fixture_central();
    let first = materialise_central_entities(&central);
    assert!(first
        .objects
        .iter()
        .any(|object| matches!(object,
            WikiObject::Node(node) if node.ref_id.as_str() == "wiki:node:pasu:agent:agent:hermes")));

    // Profile revision advances; the agent entity ref is untouched.
    fs::write(
        central.join("Control/agents/profiles/profile-hermes.json"),
        json!({
            "schema": "central.agent-profile/v1",
            "profile_ref": "agent-profile:hermes",
            "agent_ref": "agent:hermes",
            "revision": "p2",
            "scope": "personal"
        })
        .to_string(),
    )
    .unwrap();
    let second = materialise_central_entities(&central);
    let hermes = second
        .objects
        .iter()
        .find_map(|object| match object {
            WikiObject::Node(node) if node.ref_id.as_str() == "wiki:node:pasu:agent:agent:hermes" => {
                Some(node.clone())
            }
            _ => None,
        })
        .expect("same agent entity");
    let stored_revision = hermes.extensions["aikit.pasu/v1"]["extra"]["profiles"][0]["revision"]
        .as_str()
        .unwrap();
    assert_eq!(stored_revision, "p2");
}

#[test]
fn case17_half_agent_set_materialises_with_compiled_member_edges() {
    let central = fixture_central();
    let reading = materialise_central_entities(&central);
    // The unprofiled member is disclosed, not silently dropped.
    assert!(reading
        .absences
        .iter()
        .any(|absence| absence.contains("agent:unprofiled")));
    // The profiled member edge compiles and the whole set rebuilds into the index.
    let index = SemanticWikiIndex::rebuild(reading.objects).expect("entity materialisation rebuilds");
    let members = index.search("central-operators", 5);
    assert!(!members.is_empty(), "agent-set entity participates in search");
}

/// W10 V4: the agent-set entity is a bounded local whole — `local_space_ref`
/// resolves against a materialised space anchored on the entity, and
/// navigation traverses the membership through the relation faculty.
#[test]
fn case17_local_whole_resolves_and_navigation_traverses_membership() {
    use aikit_core::RelationQuery;
    let central = fixture_central();
    let reading = materialise_central_entities(&central);
    let index = SemanticWikiIndex::rebuild(reading.objects).expect("rebuild");

    let set_ref = aikit_core::ResourceRef::parse("wiki:node:pasu:agent-set:central-operators").unwrap();
    let whole = index.local_whole(&set_ref).expect("local whole resolves");
    assert!(whole.local_space.is_some(), "the local space is materialised");
    assert_eq!(whole.members.len(), 1, "only materialised members ride the whole");

    let provider = aikit_core::SemanticWikiProvider::new(&index);
    let query = RelationQuery {
        focus: set_ref.clone(),
        depth: 2,
        max_nodes: 32,
        max_edges: 32,
        filters: Vec::new(),
    };
    let view = provider.relations(query).expect("relations view");
    assert!(view.edges.iter().any(|edge| edge.relation == "local-member"),
        "navigation traverses the local whole: {:?}", view.edges.iter().map(|e| e.relation.clone()).collect::<Vec<_>>());
}
