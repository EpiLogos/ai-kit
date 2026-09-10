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
    unreadable_on: BTreeMap<String, String>,
    seen: Mutex<Vec<Vec<String>>>,
}

impl WorldRunner {
    fn with_answer(world_ref: &str, sources: Value) -> Self {
        Self {
            answers: BTreeMap::from([(world_ref.to_owned(), sources)]),
            fail_on: BTreeMap::new(),
            unreadable_on: BTreeMap::new(),
            seen: Mutex::new(Vec::new()),
        }
    }

    /// The world has no authored record at all — Central's `missing World`.
    fn failing_on(mut self, world_ref: &str) -> Self {
        self.fail_on.insert(world_ref.to_owned(), ());
        self
    }

    /// The world declares relations, but the declaration cannot be read.
    fn unreadable_on(mut self, world_ref: &str, message: &str) -> Self {
        self.unreadable_on
            .insert(world_ref.to_owned(), message.to_owned());
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
        Ok(Output {
            status: 0,
            stdout: if let Some(message) = self.unreadable_on.get(&world_ref) {
                json!({
                    "ok": false,
                    "status": "invalid_input",
                    "error": {"code": "invalid_input", "message": message}
                })
                .to_string()
            } else if self.fail_on.contains_key(&world_ref) {
                absent_envelope(&world_ref)
            } else {
                match self.answers.get(&world_ref) {
                    Some(sources) => json!({
                        "ok": true,
                        "data": {"world_ref": world_ref, "sources": sources}
                    })
                    .to_string(),
                    None => absent_envelope(&world_ref),
                }
            },
            stderr: String::new(),
        })
    }
}

/// Central's real answer for a world ref with no authored record: the result
/// status IS the code (`ctrl/src/result.rs:81`) and the absence is named only
/// in the message (`ctrl/src/world.rs:583`).
fn absent_envelope(world_ref: &str) -> String {
    json!({
        "ok": false,
        "status": "invalid_input",
        "error": {"code": "invalid_input", "message": format!("missing World {world_ref}")}
    })
    .to_string()
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
    fs::create_dir_all(root.join("Control/agents/agent-sets")).unwrap();
    fs::write(
        root.join("Control/agents/agent-sets/agent-set-operators.json"),
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
    // The Agent subject is Central's canonical paśu form
    // (`PasuRef::for_agent` — ctrl/src/pasu.rs:121), not the bare agent ref.
    assert!(
        disclosure.contains("- agent: central:pasu:agent:agent:x"),
        "{disclosure}"
    );
    // The profile relation is named by its real identifier, which Central
    // serialises as `ref` — never the `unprofiled` placeholder.
    assert!(disclosure.contains("profile/x"), "{disclosure}");
    assert!(!disclosure.contains("unprofiled"), "{disclosure}");
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
    // A withheld human is not an absent one: no "no human entity established"
    // claim is made about a World where the human merely was not disclosed.
    assert!(
        !disclosure.contains("no human entity established"),
        "the human is excluded from this context, not absent from the World: {disclosure}"
    );
    assert!(
        disclosure.contains("- agent: central:pasu:agent:agent:x"),
        "{disclosure}"
    );
}

#[test]
fn a_failed_binding_call_withholds_participants_instead_of_broadening_context() {
    let root = fixture_root();
    let project = root.join("Work/Broken");
    fs::create_dir_all(&project).unwrap();
    let runner = WorldRunner::with_answer("nothing", json!([]))
        .failing_on("project:Broken")
        .failing_on("control:root");
    let error = entity_disclosure_in(&runner, Some(&root), Some(&project))
        .expect_err("unavailable owner policy must withhold participant material");
    assert!(error.contains("participant context withheld"), "{error}");
    assert!(!error.contains("central:pasu:nara:local"), "{error}");
    assert!(!error.contains("central:pasu:agent:agent:x"), "{error}");
}

/// An unreadable declared policy is not an absent policy and cannot authorise
/// root inheritance. The composed hook reports this refusal as a warning; the
/// turn remains fail-open, but participant disclosure does not broaden.
#[test]
fn an_unreadable_world_declaration_withholds_and_never_reads_root_lineage() {
    let root = fixture_root();
    let project = root.join("Work/Corrupt");
    fs::create_dir_all(&project).unwrap();
    let runner = WorldRunner::with_answer(
        "control:root",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "1",
                "propagation_path": ["control:root"]}]),
    )
    .unreadable_on("project:Corrupt", "world relation record is malformed");
    let error = entity_disclosure_in(&runner, Some(&root), Some(&project))
        .expect_err("malformed policy must not disclose uncontextualised participants");
    assert!(error.contains("participant context withheld"), "{error}");
    let seen = runner.seen.lock().unwrap();
    assert_eq!(
        seen.len(),
        1,
        "unreadable policy must never try root fallback"
    );
    let request: Value = serde_json::from_str(seen[0].last().unwrap()).unwrap();
    assert_eq!(request["world_ref"], "project:Corrupt");
    assert!(!error.contains("central:pasu:nara:local"), "{error}");
    assert!(!error.contains("central:pasu:agent:agent:x"), "{error}");
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
    assert!(
        disclosure.is_none(),
        "no entities at all — there is nothing to disclose"
    );
}

/// The human identity source is not a precondition for disclosing the other
/// participants. An absent human must read as "no human established here",
/// never as "this World contains no Agents".
#[test]
fn agents_disclose_when_no_human_identity_is_established() {
    let root = tempfile::tempdir().unwrap().keep();
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
    let project = root.join("Work/Alpha");
    fs::create_dir_all(&project).unwrap();
    let runner = WorldRunner::with_answer("control:root", json!([])).failing_on("project:Alpha");

    let disclosure = entity_disclosure_in(&runner, Some(&root), Some(&project))
        .expect("fail-open")
        .expect("agents are disclosed even with no human identity source");

    assert!(
        disclosure.contains("- agent: central:pasu:agent:agent:x"),
        "{disclosure}"
    );
    assert!(
        disclosure.contains("- nara: no human entity established from this source"),
        "the absent human is disclosed truthfully: {disclosure}"
    );
    // An agent-set can be authored without the human source, and the
    // materialiser reports its absence rather than suppressing it.
    assert!(
        disclosure.contains("identity manifest absent"),
        "the materialisation absence is surfaced: {disclosure}"
    );
}
