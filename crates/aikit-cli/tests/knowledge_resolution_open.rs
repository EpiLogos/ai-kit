//! U3.1 owner-side resolution rows and successful-use familiarity.
//!
//! One canonical owner resolution operation (`knowledge resolve`) must return
//! real seeded files, a real Flow and a real skill, each row carrying its
//! available canonical Actions. Only an explicit `knowledge open` records
//! familiarity — query, display and failed opens record nothing.

use std::fs;

use aikit_cli::app::Service;
use aikit_core::resource::{parse_or_search_expression, ResourceRef};
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
fn one_query_returns_a_real_file_and_a_real_flow_through_the_canonical_path() {
    let temp = TempDir::new().unwrap();
    let service = open_service(&temp);

    // A plain typed string is legitimate input; the resolver lowers it into
    // the Vāk contract and returns typed hits (PR #258 one-query-path law).
    let expression = parse_or_search_expression("test").unwrap();
    let result = service.knowledge_resolve(&expression, 50).unwrap();
    assert!(
        !result.hits.is_empty(),
        "search returns the seeded material"
    );
    let resources: Vec<String> = result
        .hits
        .iter()
        .map(|hit| hit.resource.as_str().to_string())
        .collect();
    assert!(
        resources.iter().any(|r| r == FILE_REF),
        "the seeded file resolves through the canonical path; got {resources:?}"
    );
    assert!(
        resources.iter().any(|r| r == FLOW_REF),
        "the seeded Flow resolves through the canonical path"
    );
}

#[test]
fn resolve_query_display_and_refresh_record_no_familiarity_open_records_exactly_one() {
    let temp = TempDir::new().unwrap();
    let service = open_service(&temp);

    // Query: resolution itself records nothing.
    let expression = parse_or_search_expression("test").unwrap();
    let resolution = service.knowledge_resolve(&expression, 50).unwrap();
    assert!(!resolution.hits.is_empty());

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
