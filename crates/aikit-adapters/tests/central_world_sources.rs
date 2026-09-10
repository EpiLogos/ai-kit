//! W10 V5 — root→project contextualisation: project contexts bind the same
//! entity refs through Central's effective world sources; declared
//! exclusions withhold in-context; a project never mints a second subject;
//! unavailable world relations degrade fail-open.

use aikit_adapters::central_world_sources::{
    bind_project_context, project_world_ref, read_project_binding, EffectiveSource, WorldBinding,
    BINDING_EXTENSION,
};
use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::{ResourceRef, SemanticRevision, SourceRef, WikiNode, WikiObject, WikiProvenanceRef};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

fn resource_ref(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}

fn source_ref(value: &str) -> SourceRef {
    SourceRef::parse(value).unwrap()
}

fn pasu_node(ref_id: &str, subject_ref: &str, source_refs: &[&str]) -> WikiNode {
    let mut extensions = std::collections::BTreeMap::new();
    extensions.insert(
        "aikit.pasu/v1".to_owned(),
        json!({"form": "nara", "subject_ref": subject_ref}),
    );
    WikiNode {
        profile: "okf-wiki/v1".into(),
        ref_id: resource_ref(ref_id),
        revision: 1,
        provenance: Vec::new(),
        node_type: "pasu".into(),
        title: None,
        space_refs: Vec::new(),
        source_refs: source_refs.iter().map(|s| source_ref(s)).collect(),
        local_space_ref: None,
        extensions,
    }
}

/// Mark a fixture as produced by the entity materialisation (only these may
/// claim a pasu subject).
fn materialised(mut node: WikiNode) -> WikiNode {
    node.provenance.push(WikiProvenanceRef {
        source_ref: source_ref("central:pasu:fixture"),
        source_revision: Some(SemanticRevision::Text("1".into())),
        producer_ref: Some(resource_ref("aikit/central-entity-materialisation/v1")),
        generation_ref: None,
        extensions: std::collections::BTreeMap::new(),
    });
    node
}

/// Answers each call by looking up the invoked world_ref in a scripted map;
/// records every argv so the Action contract is pinned.
struct WorldRunner {
    answers: BTreeMap<String, Value>,
    fail_on: BTreeMap<String, ()>,
    unreadable_on: BTreeMap<String, String>,
    seen: Mutex<Vec<Vec<String>>>,
}

/// Central's real answer for a world ref with no authored record. The result
/// status IS the code (`ctrl/src/result.rs:81`) and the absence is named only
/// in the message (`missing World <ref>`, `ctrl/src/world.rs:583`).
fn absent_envelope(world_ref: &str) -> String {
    json!({
        "ok": false,
        "status": "invalid_input",
        "error": {
            "code": "invalid_input",
            "message": format!("missing World {world_ref}"),
        }
    })
    .to_string()
}

/// A declaration that exists but cannot be read or validated. Same status and
/// code as the absent case — deliberately, so this pins that the consumer
/// separates them by the message and not by the code alone.
fn unreadable_envelope(message: &str) -> String {
    json!({
        "ok": false,
        "status": "invalid_input",
        "error": {"code": "invalid_input", "message": message}
    })
    .to_string()
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

    fn envelope(&self, world_ref: &str, sources: Value) -> String {
        json!({"ok": true, "data": {"world_ref": world_ref, "sources": sources}}).to_string()
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
        let stdout = if let Some(message) = self.unreadable_on.get(&world_ref) {
            unreadable_envelope(message)
        } else if self.fail_on.contains_key(&world_ref) {
            absent_envelope(&world_ref)
        } else {
            self.answers
                .get(&world_ref)
                .map(|sources| self.envelope(&world_ref, sources.clone()))
                .unwrap_or_else(|| absent_envelope(&world_ref))
        };
        Ok(Output {
            status: 0,
            stdout,
            stderr: String::new(),
        })
    }
}

fn binding() -> WorldBinding {
    WorldBinding {
        world_ref: "project:alpha".into(),
        inherited_root_lineage: false,
        sources: vec![EffectiveSource {
            source_ref: "central:source:control:root:Control/user/identity".into(),
            state: "available".into(),
            effective_revision: "1".into(),
            propagation_path: vec!["project:alpha".into(), "control:root".into()],
        }],
    }
}

#[test]
fn binding_annotates_entities_with_propagation_and_keeps_refs() {
    let mut objects = vec![WikiObject::Node(materialised(pasu_node(
        "wiki:node:identity",
        "central:pasu:nara:local",
        &["central:source:control:root:Control/user/identity/formation.md"],
    )))];
    let mut absences = Vec::new();
    bind_project_context(&mut objects, &binding(), &mut absences);
    assert_eq!(objects.len(), 1, "the entity binds, it is not replaced");
    assert_eq!(objects[0].ref_id().as_str(), "wiki:node:identity", "same ref — no second human");
    let WikiObject::Node(node) = &objects[0] else {
        panic!("entity node");
    };
    let annotation = &node.extensions[BINDING_EXTENSION];
    assert_eq!(annotation["world_ref"], "project:alpha");
    assert_eq!(
        annotation["bindings"][0]["propagation_path"],
        json!(["project:alpha", "control:root"]),
        "per-hop provenance is recorded"
    );
    assert!(absences.is_empty(), "{absences:?}");
}

#[test]
fn excluded_source_withholds_the_entity_from_the_context_and_discloses() {
    let mut world = binding();
    world.sources[0].state = "excluded".into();
    let mut objects = vec![
        WikiObject::Node(materialised(pasu_node(
            "wiki:node:identity",
            "central:pasu:nara:local",
            &["central:source:control:root:Control/user/identity/formation.md"],
        ))),
        WikiObject::Node(materialised(pasu_node(
            "wiki:node:pasu:agent:agent/x",
            "central:pasu:agent:agent/x",
            &["central:source:control:root:Control/agents/profiles/x.json"],
        ))),
    ];
    let mut absences = Vec::new();
    bind_project_context(&mut objects, &world, &mut absences);
    let refs: Vec<_> = objects.iter().map(|o| o.ref_id().as_str()).collect();
    assert!(!refs.contains(&"wiki:node:identity"), "excluded source withholds the entity in this context");
    assert!(refs.contains(&"wiki:node:pasu:agent:agent/x"), "unrelated entities still bind");
    assert!(
        absences.iter().any(|a| a.contains("withheld") && a.contains("excluded")),
        "{absences:?}"
    );
}

#[test]
fn project_stand_in_redeclaring_a_subject_is_refused() {
    let mut objects = vec![
        WikiObject::Node(materialised(pasu_node(
            "wiki:node:identity",
            "central:pasu:nara:local",
            &["central:source:control:root:Control/user/identity/formation.md"],
        ))),
        // No entity producer: a project-wiki stand-in, not a materialisation.
        WikiObject::Node(pasu_node(
            "wiki:node:project-local-human",
            "central:pasu:nara:local",
            &["central:source:project:alpha:ProjectCentral/user/human.md"],
        )),
    ];
    let mut absences = Vec::new();
    bind_project_context(&mut objects, &binding(), &mut absences);
    let refs: Vec<_> = objects.iter().map(|o| o.ref_id().as_str()).collect();
    assert_eq!(refs, vec!["wiki:node:identity"], "the materialised entity keeps the subject");
    assert!(
        absences.iter().any(|a| a.contains("re-declares") && a.contains("kept the materialised entity")),
        "{absences:?}"
    );
}

#[test]
fn empty_binding_leaves_objects_untouched() {
    let mut objects = vec![WikiObject::Node(materialised(pasu_node(
        "wiki:node:identity",
        "central:pasu:nara:local",
        &["central:source:control:root:Control/user/identity/formation.md"],
    )))];
    let before = format!("{:?}", objects[0].ref_id());
    let mut absences = Vec::new();
    bind_project_context(
        &mut objects,
        &WorldBinding { world_ref: "project:alpha".into(), inherited_root_lineage: true, sources: Vec::new() },
        &mut absences,
    );
    assert_eq!(objects.len(), 1);
    assert_eq!(format!("{:?}", objects[0].ref_id()), before);
    assert!(absences.is_empty());
}

#[test]
fn effective_sources_action_contract_is_pinned_and_parsed() {
    let runner = WorldRunner::with_answer(
        "project:alpha",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "2",
                "propagation_path": ["project:alpha", "control:root"]}]),
    );
    let root = PathBuf::from("/tmp/central");
    let executable = Path::new("ctrl");
    let world = read_project_binding(&runner, executable, &root, "alpha", &mut Vec::new())
        .expect("project world relations resolve");
    assert_eq!(world.world_ref, "project:alpha");
    assert!(!world.inherited_root_lineage);
    assert_eq!(world.sources[0].effective_revision, "2");
    let seen = runner.seen.lock().unwrap();
    let argv = &seen[0];
    assert_eq!(argv[0], "ctrl");
    assert!(argv.windows(2).any(|pair| pair[0] == "--root" && pair[1] == "/tmp/central"));
    assert!(argv.windows(3).any(|pair| pair[0] == "action"
        && pair[1] == "run"
        && pair[2] == "central.world.effective-sources"));
    let input: Value = serde_json::from_str(argv.last().unwrap()).unwrap();
    assert_eq!(input["scope"], "project");
    assert_eq!(input["project"], "alpha");
    assert_eq!(input["world_ref"], "project:alpha");
}

#[test]
fn project_without_world_relations_falls_back_to_the_root_lineage() {
    let runner = WorldRunner::with_answer(
        "control:root",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "1",
                "propagation_path": ["control:root"]}]),
    )
    .failing_on("project:beta");
    let mut absences = Vec::new();
    let world = read_project_binding(&runner, Path::new("ctrl"), &PathBuf::from("/tmp/central"), "Beta", &mut absences)
        .expect("root lineage applies when the project declares no world");
    assert!(world.inherited_root_lineage, "the convention is disclosed, never silent");
    assert_eq!(world.sources[0].effective_revision, "1");
    assert!(absences.iter().any(|a| a.contains("root lineage applies")), "{absences:?}");
}

#[test]
fn world_relations_unavailable_degrades_to_uncontextualised() {
    let runner = WorldRunner::with_answer("nothing", json!([]))
        .unreadable_on("project:gamma", "world relation record is malformed");
    let mut absences = Vec::new();
    let world = read_project_binding(
        &runner,
        Path::new("ctrl"),
        &PathBuf::from("/tmp/central"),
        "Gamma",
        &mut absences,
    );
    assert!(world.is_none(), "fail-open: no binding, composition proceeds");
    assert!(
        absences.iter().any(|a| a.contains("uncontextualised")),
        "{absences:?}"
    );
}

/// The distinction the failure policy turns on: a declaration that exists but
/// cannot be read is NOT an undeclared world. It must not inherit the root
/// lineage, and the root must not even be consulted — an unreadable exclusion
/// must never broaden what a turn receives.
#[test]
fn an_unreadable_declaration_does_not_inherit_the_root_lineage() {
    let runner = WorldRunner::with_answer(
        "control:root",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "1",
                "propagation_path": ["control:root"]}]),
    )
    .unreadable_on("project:Delta", "world relation record is malformed");
    let mut absences = Vec::new();
    let world = read_project_binding(
        &runner,
        Path::new("ctrl"),
        &PathBuf::from("/tmp/central"),
        "Delta",
        &mut absences,
    );
    assert!(world.is_none(), "no binding rather than a widened one");
    assert!(
        absences.iter().any(|a| a.contains("could not be read or validated")),
        "{absences:?}"
    );
    assert!(
        !absences.iter().any(|a| a.contains("root lineage applies")),
        "the root lineage is not assumed: {absences:?}"
    );
    // The root was never asked — the inheritance path was not entered at all.
    assert_eq!(
        runner.seen.lock().unwrap().len(),
        1,
        "only the project declaration was read"
    );
}

#[test]
fn project_world_ref_prefers_the_declared_project_id() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("Work/Alpha");
    std::fs::create_dir_all(project.join("ProjectCentral")).unwrap();
    std::fs::write(
        project.join("ProjectCentral/project.json"),
        json!({"schema": "central.project/v1", "project_id": "project:alpha-native"}).to_string(),
    )
    .unwrap();
    assert_eq!(
        project_world_ref(temp.path(), "Alpha"),
        "project:alpha-native",
        "the project's declared stable identity wins over the convention"
    );
    assert_eq!(project_world_ref(temp.path(), "Beta"), "project:Beta");
}
