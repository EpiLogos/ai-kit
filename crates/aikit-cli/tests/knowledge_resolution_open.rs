//! U3.1 owner-side resolution rows and successful-use familiarity.
//!
//! One canonical owner resolution operation (`knowledge resolve`) must return
//! real seeded files, a real Flow and a real skill, each row carrying its
//! available canonical Actions. Only an explicit `knowledge open` records
//! familiarity — query, display and failed opens record nothing.

use std::fs;

use aikit_cli::app::Service;
use aikit_core::resource::ResourceRef;
use aikit_core::trust::{TrustKey, TrustState};
use aikit_core::{CapsuleId, Catalog, RegistrySource};
use aikit_store::home::AikitHome;
use aikit_store::index::Index;
use aikit_store::registry::load_registry;
use aikit_store::replay_familiarity;
use aikit_store::trust::TrustStore;
use tempfile::TempDir;

const FILE_REF: &str = "source:file:test-onboarding";
const FLOW_REF: &str = "wiki:node:staged/test-flow";
const SUBJECT_REF: &str = "wiki:node:test-subject";
const SKILL_REF: &str = "skill/test/wayfinder";

fn write(path: &std::path::Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    write(&project.join(".aikit/profile.toml"), "schema = 1\n");
    write(
        &temp.path().join("aikit-home/scopes/global/profile.toml"),
        "schema = 1\nenable = [\"skill/test/wayfinder\"]\n",
    );
    write(
        &project.join("semantic-wiki.json"),
        r#"{
          "objects": [
            {
              "profile": "okf-wiki/v1",
              "object": "node",
              "ref": "wiki:node:staged/test-flow",
              "revision": 3,
              "provenance": [{"source_ref":"source:file:test-flow-note","source_revision":"rev-2"}],
              "type": "flow",
              "title": "Test flow thread",
              "space_refs": [],
              "source_refs": ["source:file:test-flow-note"]
            },
            {
              "profile": "okf-wiki/v1",
              "object": "node",
              "ref": "wiki:node:test-subject",
              "revision": 5,
              "provenance": [{"source_ref":"source:file:test-subject-paper","source_revision":"rev-9"}],
              "type": "Concept",
              "title": "Test subject",
              "space_refs": [],
              "source_refs": ["source:file:test-subject-paper"]
            }
          ]
        }"#,
    );
    write(
        &project.join("source-material.json"),
        r#"{
          "binding": {
            "source": "source:file:test-onboarding",
            "revision": "rev-1",
            "title": "Test onboarding file",
            "tags": ["test", "onboarding"],
            "visibility": "public",
            "owners": [],
            "media_type": "text/markdown",
            "metadata": {"origin":"test-fixture"}
          },
          "body": "The test onboarding file keeps source evidence distinct from compiled knowledge."
        }"#,
    );
    write(
        &temp
            .path()
            .join("aikit-home/registries/personal/capsules/skill/test/wayfinder/manifest.toml"),
        r#"schema = 1
id = "skill/test/wayfinder"
kind = "skill"
name = "wayfinder"
description = "Plans long test work."

[skill]
root = "payload"
"#,
    );
    write(
        &temp
            .path()
            .join("aikit-home/registries/personal/capsules/skill/test/wayfinder/payload/SKILL.md"),
        "---\nname: wayfinder\ndescription: Plans long test work.\ndisable-model-invocation: true\n---\n\n# Wayfinder\n",
    );
    // A deliberate human trust review, keyed on the exact (source, capsule,
    // revision) — the same review `aikit trust record` would write. Skills
    // stay inert until this happens.
    home.ensure_layout().unwrap();
    let index = Index::open(&home.database()).unwrap();
    let load = load_registry(&home.registry("personal"), RegistrySource::new("personal")).unwrap();
    let capsule = load
        .catalog
        .get(&CapsuleId::parse(SKILL_REF).unwrap())
        .expect("the seeded registry holds the wayfinder skill");
    let revision = capsule
        .revision
        .clone()
        .expect("a loaded capsule has a revision");
    TrustStore::new(&index)
        .record(
            &TrustKey::new(
                RegistrySource::new("personal"),
                CapsuleId::parse(SKILL_REF).unwrap(),
                revision,
            ),
            TrustState::Trusted,
            Some("test-fixture review"),
        )
        .unwrap();
    Service::open(home, &project, |_| None).expect("open production application service")
}

fn observation_events(service: &Service) -> usize {
    match replay_familiarity(service.index()).expect("replay familiarity") {
        aikit_store::FamiliarityReplay::Loaded {
            observation_events, ..
        } => observation_events,
        other => panic!("familiarity replay invalidated: {other:?}"),
    }
}

#[test]
fn one_query_returns_a_real_file_a_real_flow_and_a_real_skill_with_actions() {
    let temp = TempDir::new().unwrap();
    let service = open_service(&temp);

    let resolution = service.knowledge_resolve("test", 50).unwrap();
    assert_eq!(resolution.version, "aikit.knowledge-resolution/v1");

    let file = resolution
        .rows
        .iter()
        .find(|row| row.reference.as_str() == FILE_REF)
        .expect("the seeded file resolves as a row");
    assert_eq!(file.kind.as_str(), "file");
    assert_eq!(file.owner, "provider/source-pool/native");
    assert!(file.provenance.iter().any(|entry| entry.contains("rev-1")));
    assert!(
        file.provenance
            .iter()
            .any(|entry| entry.contains("test-fixture"))
    );
    assert_eq!(
        file.actions,
        vec![
            "knowledge/read",
            "knowledge/sources",
            "knowledge/explain",
            "knowledge/open"
        ]
    );

    let flow = resolution
        .rows
        .iter()
        .find(|row| row.reference.as_str() == FLOW_REF)
        .expect("the seeded Flow resolves as a row");
    assert_eq!(flow.kind.as_str(), "flow");
    assert_eq!(flow.owner, "provider/semantic-wiki/sqlite");
    assert!(
        flow.provenance
            .iter()
            .any(|entry| entry.contains("source:file:test-flow-note@rev-2"))
    );
    assert_eq!(
        flow.actions,
        vec![
            "knowledge/read",
            "knowledge/relations",
            "action:contemplate-flow",
            "knowledge/open"
        ]
    );

    let skill = resolution
        .rows
        .iter()
        .find(|row| row.reference.as_str() == SKILL_REF)
        .expect("the seeded skill resolves as a row");
    assert_eq!(skill.kind.as_str(), "skill");
    assert!(!skill.owner.is_empty());
    assert!(
        skill
            .provenance
            .iter()
            .any(|entry| entry.contains("registry"))
    );
    assert_eq!(
        skill.actions,
        vec![
            "run",
            "skill/overlay/set",
            "knowledge/explain",
            "knowledge/open"
        ]
    );

    let subject = resolution
        .rows
        .iter()
        .find(|row| row.reference.as_str() == SUBJECT_REF)
        .expect("the seeded knowledge subject resolves as a row");
    assert_eq!(subject.kind.as_str(), "knowledge-subject");
    assert!(subject.actions.contains(&"knowledge/route".to_string()));

    // Unavailable providers are explicit states, not omissions.
    assert!(
        resolution
            .unavailable
            .iter()
            .all(|entry| entry.state == "unavailable")
    );
}

#[test]
fn resolve_query_display_and_refresh_record_no_familiarity_open_records_exactly_one() {
    let temp = TempDir::new().unwrap();
    let service = open_service(&temp);

    // Query: resolution itself records nothing.
    let resolution = service.knowledge_resolve("test", 50).unwrap();
    assert!(!resolution.rows.is_empty());

    // Display: read and explain record nothing.
    let address = service
        .knowledge_address(&ResourceRef::parse(FILE_REF).unwrap())
        .unwrap()
        .expect("the seeded file has a knowledge address");
    service.knowledge_read(&address).unwrap();
    service.knowledge_explain(&address).unwrap();

    // Refresh: a fresh Service over the same isolated home re-materialises the
    // runtime and still records nothing.
    drop(service);
    let mut service = open_service(&temp);
    let _ = service.knowledge_status().unwrap();

    assert_eq!(
        observation_events(&service),
        0,
        "query, display and refresh record no successful-use familiarity"
    );

    // One explicit open records exactly one use.
    let receipt = service
        .knowledge_open(&ResourceRef::parse(FILE_REF).unwrap())
        .unwrap();
    assert_eq!(receipt.opened.as_str(), FILE_REF);
    assert_eq!(receipt.recorded, "familiarity/resource-use");
    assert!(receipt.observation_id.starts_with("knowledge-open-use/"));
    assert_eq!(observation_events(&service), 1);

    // A second open of another row records a second, distinct use.
    service
        .knowledge_open(&ResourceRef::parse(FLOW_REF).unwrap())
        .unwrap();
    assert_eq!(observation_events(&service), 2);

    // A failed open (unresolved ref) records nothing.
    let failed = service.knowledge_open(&ResourceRef::parse("wiki:node:absent").unwrap());
    assert!(failed.is_err());
    assert_eq!(observation_events(&service), 2);
}
