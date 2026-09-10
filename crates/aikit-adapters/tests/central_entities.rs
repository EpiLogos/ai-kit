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
    fs::create_dir_all(central.join("Control/agents/agent-sets")).unwrap();
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
            // Central's serialised shape: ctrl's `AgentProfile` renames the
            // identifier to `ref` (`ctrl/src/agent_profile.rs:113`). A fixture
            // that spells it `profile_ref` proves nothing about the real
            // record — that mismatch is exactly what this file guards.
            "schema": "central.agent-profile/v1",
            "ref": "profile/hermes",
            "agent_ref": "agent:hermes",
            "revision": "p1",
            "scope": "personal",
            "intent_provenance": {
                "schema": "central.agent-profile-provenance/v1",
                "intent_expression": "be hermes, and hold it",
                "origin_action": "agent-profile.propose",
                "authorship": "generated-proposal",
                "recognition": "unrecognised"
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        central.join("Control/agents/agent-sets/agent-set-operators.json"),
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
            "ref": "profile/hermes",
            "agent_ref": "agent:hermes",
            "revision": "p2",
            "scope": "personal",
            "intent_provenance": {
                "schema": "central.agent-profile-provenance/v1",
                "intent_expression": "be hermes, and hold it",
                "origin_action": "agent-profile.propose",
                "authorship": "generated-proposal",
                "recognition": "unrecognised"
            }
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

/// Central serialises the profile identifier as `ref`
/// (`ctrl/src/agent_profile.rs:113`). Reading the consumer's own spelling would
/// leave every real record marked `unprofiled`, so the identifier AND the
/// generated-proposal block must survive from Central's actual shape.
#[test]
fn the_profile_identifier_and_intent_survive_from_centrals_own_shape() {
    let central = fixture_central();
    let reading = materialise_central_entities(&central);
    let hermes = reading
        .objects
        .iter()
        .find_map(|object| match object {
            WikiObject::Node(node) if node.ref_id.as_str() == "wiki:node:pasu:agent:agent:hermes" => {
                Some(node.clone())
            }
            _ => None,
        })
        .expect("agent entity");
    let profile = &hermes.extensions["aikit.pasu/v1"]["extra"]["profiles"][0];

    assert_eq!(
        profile["profile_ref"].as_str(),
        Some("profile/hermes"),
        "the real identifier, never the placeholder"
    );
    assert_eq!(profile["revision"].as_str(), Some("p1"));
    assert_eq!(profile["scope"].as_str(), Some("personal"));
    // The intent travels verbatim, and its standing travels with it.
    assert_eq!(
        profile["intent_provenance"]["intent_expression"].as_str(),
        Some("be hermes, and hold it")
    );
    assert_eq!(
        profile["intent_provenance"]["recognition"].as_str(),
        Some("unrecognised")
    );
}

/// The legacy `profile_ref` spelling still reads (records generated before the
/// rename), and the placeholder marks only a record that names no profile.
#[test]
fn legacy_profile_ref_records_still_read_and_absence_is_marked() {
    let central = fixture_central();
    fs::write(
        central.join("Control/agents/profiles/profile-legacy.json"),
        json!({
            "schema": "central.agent-profile/v1",
            "profile_ref": "profile/legacy",
            "agent_ref": "agent:legacy",
            "revision": "r1",
            "scope": "personal"
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        central.join("Control/agents/profiles/profile-nameless.json"),
        json!({
            "schema": "central.agent-profile/v1",
            "agent_ref": "agent:nameless",
            "revision": "r1",
            "scope": "personal"
        })
        .to_string(),
    )
    .unwrap();

    let reading = materialise_central_entities(&central);
    let profile_ref_of = |agent_ref: &str| -> Option<String> {
        reading.objects.iter().find_map(|object| match object {
            WikiObject::Node(node)
                if node.ref_id.as_str() == format!("wiki:node:pasu:agent:{agent_ref}") =>
            {
                node.extensions["aikit.pasu/v1"]["extra"]["profiles"][0]["profile_ref"]
                    .as_str()
                    .map(str::to_owned)
            }
            _ => None,
        })
    };
    assert_eq!(profile_ref_of("agent:legacy").as_deref(), Some("profile/legacy"));
    assert_eq!(profile_ref_of("agent:nameless").as_deref(), Some("unprofiled"));
}

/// Central permits colon-bearing set refs (`ctrl/src/agent_set_store.rs:533`
/// rejects only empty, untrimmed and NUL values). Deriving the local space from
/// the final colon-separated fragment would collapse two distinct permitted
/// sets onto one space — and two spaces sharing a ref would corrupt the graph.
#[test]
fn colon_bearing_set_refs_get_distinct_local_spaces() {
    let central = fixture_central();
    for (file, set_ref) in [
        ("agent-set-team.json", "team:review"),
        ("agent-set-other.json", "other:review"),
    ] {
        fs::write(
            central.join("Control/agents/agent-sets").join(file),
            json!({
                "schema": "central.agent-set/v1",
                "ref": set_ref,
                "revision": "r1",
                "members": [{"kind": "agent", "agent_ref": "agent:hermes"}]
            })
            .to_string(),
        )
        .unwrap();
    }

    let reading = materialise_central_entities(&central);
    let spaces: Vec<String> = reading
        .objects
        .iter()
        .filter_map(|object| match object {
            WikiObject::Space(space) => Some(space.ref_id.as_str().to_owned()),
            _ => None,
        })
        .collect();

    assert!(
        spaces.iter().any(|s| s == "wiki:space:pasu-local:team:review"),
        "{spaces:?}"
    );
    assert!(
        spaces.iter().any(|s| s == "wiki:space:pasu-local:other:review"),
        "{spaces:?}"
    );
    let mut unique = spaces.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        spaces.len(),
        "no two local spaces may share a ref: {spaces:?}"
    );
}

/// A real Central-produced record, committed byte-exact from the live store
/// (`Control/agents/profiles/`), consumed end to end. A fixture that restates
/// the consumer's own spelling can only ever prove the consumer agrees with
/// itself; this one fails when Central's serialisation moves.
#[test]
fn the_committed_real_central_record_is_consumed_faithfully() {
    let central = fixture_central();
    let real = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/central/profile-factory-bounded-acceptance.json");
    let body = fs::read_to_string(&real).expect("committed real record");
    fs::write(central.join("Control/agents/profiles/profile-real.json"), &body).unwrap();

    let reading = materialise_central_entities(&central);
    let node = reading
        .objects
        .iter()
        .find_map(|object| match object {
            WikiObject::Node(node)
                if node.ref_id.as_str()
                    == "wiki:node:pasu:agent:agent/factory-bounded-acceptance" =>
            {
                Some(node.clone())
            }
            _ => None,
        })
        .expect("the real record materialises as an agent entity");
    let profile = &node.extensions["aikit.pasu/v1"]["extra"]["profiles"][0];

    assert_eq!(
        profile["profile_ref"].as_str(),
        Some("profile/factory-bounded-acceptance")
    );
    assert_eq!(profile["revision"].as_str(), Some("commission-v1"));
    assert_eq!(profile["scope"].as_str(), Some("personal"));
    // This record was not authored from an expressed intent. No provenance
    // block is invented for it, and no absence is dressed up as one.
    assert!(
        profile.get("intent_provenance").is_none(),
        "no provenance is fabricated: {profile}"
    );

    // The record's own bytes are what is read. Central names the identifier
    // `ref`; `source_profile_ref` is a different field and must not be
    // mistaken for it.
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        value.get("profile_ref").is_none(),
        "the live record carries no `profile_ref`"
    );
    assert_eq!(value["ref"], profile["profile_ref"]);
}

/// F5a: the native AgentRef, the canonical paśu subject and the materialised
/// WikiNode must co-refer. The producer writes the subject and the addressing
/// grammar resolves it — proven against the producer's own output, so the
/// producer and the resolver cannot drift apart into different
/// representations.
#[test]
fn produced_agent_subjects_coreference_through_the_addressing_grammar() {
    let central = fixture_central();
    let reading = materialise_central_entities(&central);

    let agent = reading
        .objects
        .iter()
        .find_map(|object| match object {
            WikiObject::Node(node)
                if node.ref_id.as_str() == "wiki:node:pasu:agent:agent:hermes" =>
            {
                Some(node.clone())
            }
            _ => None,
        })
        .expect("agent entity");
    let subject = agent.extensions["aikit.pasu/v1"]["subject_ref"]
        .as_str()
        .expect("the entity carries its canonical subject")
        .to_owned();

    // The subject is the paśu form of the agent ref (PasuRef::for_agent —
    // ctrl/src/pasu.rs:121): a canonical prefix, then the agent ref opaque.
    assert_eq!(subject, "central:pasu:agent:agent:hermes");

    let index = SemanticWikiIndex::rebuild(reading.objects).expect("rebuild");

    // And that subject is a working address, resolving back to this entity.
    let resolved = aikit_core::knowledge_entity_address::resolve_participant_expression(
        &index,
        "@central:pasu:agent:agent:hermes",
    )
    .expect("the canonical subject is an addressable participant");
    assert_eq!(
        resolved[0].as_str(),
        "wiki:node:pasu:agent:agent:hermes",
        "producer subject and resolver agree on one entity"
    );
}
