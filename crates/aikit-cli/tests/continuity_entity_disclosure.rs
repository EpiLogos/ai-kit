//! W10 V6 — entity-aware disclosure as a composed capability: the rendered
//! disclosure names the participants present in the context, honors the
//! world binding (exclusions withhold), never shows volatile state, and
//! degrades fail-open.

use aikit_adapters::runner::{CommandRunner, Output};
use aikit_cli::continuity_disclosure::entity_disclosure_in;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};

/// Answers `central.world.effective-sources` per invoked world_ref.
struct WorldRunner {
    answers: BTreeMap<String, Value>,
    fail_on: BTreeMap<String, ()>,
    seen: Mutex<Vec<Vec<String>>>,
}

impl WorldRunner {
    fn with_answer(world_ref: &str, sources: Value) -> Self {
        Self {
            answers: BTreeMap::from([(world_ref.to_owned(), sources)]),
            fail_on: BTreeMap::new(),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn failing_on(mut self, world_ref: &str) -> Self {
        self.fail_on.insert(world_ref.to_owned(), ());
        self
    }
}

impl CommandRunner for WorldRunner {
    fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
        self.seen.lock().unwrap().push(argv.to_vec());
        let world_ref: String = argv
            .last()
            .and_then(|input| serde_json::from_str::<Value>(input).ok())
            .and_then(|input| input["world_ref"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let envelope = |ok: bool, sources: Value| {
            json!({"ok": ok, "data": {"world_ref": world_ref, "sources": sources}}).to_string()
        };
        Ok(Output {
            status: 0,
            stdout: if self.fail_on.contains_key(&world_ref) {
                json!({"ok": false, "error": {"message": format!("no world {world_ref}")}})
                    .to_string()
            } else {
                match self.answers.get(&world_ref) {
                    Some(sources) => envelope(true, sources.clone()),
                    None => json!({"ok": false, "error": {"message": "missing world"}}).to_string(),
                }
            },
            stderr: String::new(),
        })
    }
}

/// An inhabited Central fixture: nara identity manifest + one sourced file,
/// one durable agent profile, two nested agent-sets.
fn fixture_root() -> PathBuf {
    let root = tempfile::tempdir().unwrap().keep();
    let identity = root.join("Control/user/identity");
    fs::create_dir_all(&identity).unwrap();
    fs::write(
        identity.join("manifest.json"),
        json!({
            "schema": "central.pasu.identity-manifest/v1",
            "revision": "1",
            "subject": {"ref": "central:pasu:nara:local", "title": "Nara identity source"},
            "identity_source": {
                "path": "Control/user/identity",
                "sources": [{"path": "Control/user/identity/formation.md", "standing": "authored-ground"}]
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(identity.join("formation.md"), "fixture identity").unwrap();
    fs::create_dir_all(root.join("Control/agents/profiles")).unwrap();
    fs::write(
        root.join("Control/agents/profiles/profile-x.json"),
        json!({
            "schema": "central.agent-profile/v1",
            "ref": "profile/x", "revision": "r1",
            "agent_ref": "agent:x", "scope": "personal"
        })
        .to_string(),
    )
    .unwrap();
    fs::create_dir_all(root.join("Control/relations/agent-sets")).unwrap();
    fs::write(
        root.join("Control/relations/agent-sets/agent-set-operators.json"),
        json!({
            "schema": "central.agent-set/v1", "ref": "world-operators", "revision": "r1",
            "members": [{"kind": "agent", "agent_ref": "agent:x"}]
        })
        .to_string(),
    )
    .unwrap();
    root
}

#[test]
fn disclosure_names_the_participants_present_in_a_project_context() {
    let root = fixture_root();
    let project = root.join("Work/Alpha");
    fs::create_dir_all(&project).unwrap();
    // The project declares no world: the root lineage applies by convention.
    let runner = WorldRunner::with_answer(
        "control:root",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "1",
                "propagation_path": ["control:root"]}]),
    )
    .failing_on("project:Alpha");
    let disclosure =
        entity_disclosure_in(&runner, Some(&root), Some(&project)).expect("fail-open disclosure");
    let disclosure = disclosure.expect("inhabited world discloses participants");
    assert!(disclosure.starts_with("[continuity/entity-disclosure]"));
    assert!(
        disclosure.contains("- nara: central:pasu:nara:local"),
        "{disclosure}"
    );
    assert!(disclosure.contains("- agent: agent:x"), "{disclosure}");
    assert!(
        disclosure.contains("- agent-set: central:pasu:agent-set:world-operators"),
        "{disclosure}"
    );
    assert!(
        disclosure.contains(
            "context binding: control:root (1 source(s) effective, root lineage by convention)"
        ),
        "{disclosure}"
    );
    // Durable facts only — no volatile availability claims.
    assert!(!disclosure.contains("current actor"), "{disclosure}");
    assert!(!disclosure.contains("resolved_agents"), "{disclosure}");
}

#[test]
fn the_root_context_discloses_without_a_binding_line() {
    let root = fixture_root();
    let runner = WorldRunner::with_answer("control:root", json!([]));
    let disclosure = entity_disclosure_in(&runner, Some(&root), Some(&root)).expect("fail-open");
    let disclosure = disclosure.expect("root context still names its inhabitants");
    assert!(
        disclosure.contains("- nara: central:pasu:nara:local"),
        "{disclosure}"
    );
    assert!(!disclosure.contains("context binding:"), "{disclosure}");
    assert!(
        runner.seen.lock().unwrap().is_empty(),
        "the root is the source side; no binding call"
    );
}

#[test]
fn an_excluded_identity_source_withholds_the_nara_from_the_disclosure() {
    let root = fixture_root();
    let project = root.join("Work/Sealed");
    fs::create_dir_all(&project).unwrap();
    let runner = WorldRunner::with_answer(
        "project:Sealed",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "excluded", "effective_revision": "1",
                "propagation_path": ["project:Sealed", "control:root"]}]),
    );
    let disclosure = entity_disclosure_in(&runner, Some(&root), Some(&project)).expect("fail-open");
    let disclosure = disclosure.expect("other participants remain");
    assert!(
        !disclosure.contains("- nara:"),
        "excluded source withholds the nara: {disclosure}"
    );
    assert!(disclosure.contains("- agent: agent:x"), "{disclosure}");
}

#[test]
fn a_failed_binding_call_degrades_the_disclosure_fail_open() {
    let root = fixture_root();
    let project = root.join("Work/Broken");
    fs::create_dir_all(&project).unwrap();
    let runner = WorldRunner::with_answer("nothing", json!([]))
        .failing_on("project:Broken")
        .failing_on("control:root");
    let disclosure =
        entity_disclosure_in(&runner, Some(&root), Some(&project)).expect("fail-open, never error");
    let disclosure = disclosure.expect("uncontextualised disclosure still names participants");
    assert!(
        disclosure.contains("- nara: central:pasu:nara:local"),
        "{disclosure}"
    );
    assert!(!disclosure.contains("context binding:"), "{disclosure}");
}

#[test]
fn an_empty_world_discloses_nothing() {
    let temp = tempfile::tempdir().unwrap().keep();
    let disclosure = entity_disclosure_in(
        &WorldRunner::with_answer("nothing", json!([])),
        Some(&temp),
        Some(&temp),
    )
    .expect("fail-open");
    assert!(disclosure.is_none(), "no identity manifest — no disclosure");
}
