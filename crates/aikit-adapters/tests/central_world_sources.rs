//! W10 V5 — root→project contextualisation: project contexts bind the same
//! entity refs through Central's effective world sources; declared
//! exclusions withhold in-context; a project never mints a second subject;
//! unavailable world relations degrade fail-open.

use aikit_adapters::central_world_sources::{
    bind_project_context, project_world_ref, read_project_binding, EffectiveSource, WorldBinding,
    BINDING_EXTENSION,
};
use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::{
    ResourceRef, SemanticRevision, SourceRef, WikiNode, WikiObject, WikiProvenanceRef,
};
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
    coded_absent: BTreeMap<String, ()>,
    manifest_less: BTreeMap<String, ()>,
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

/// Central naming absence in the error code. The message is deliberately not
/// the legacy prose, so nothing but the code can carry the distinction.
fn coded_absent_envelope(world_ref: &str) -> String {
    json!({
        "ok": false,
        "status": "invalid_input",
        "error": {
            "code": "central.world_declaration_absent",
            "message": format!("no authored record for {world_ref}"),
        }
    })
    .to_string()
}

/// Central's real answer (captured live 2026-09-18) for a Work member with
/// **no ProjectCentral manifest at all**: the io not-found error for the
/// absent `ProjectCentral/project.json` (ctrl/src/projectcentral.rs
/// `read_project_manifest`), wrapped by ctrl/src/agent_set_actions.rs, code
/// `invalid_input`, exit 2. Structural non-existence — there is no project
/// record, so the declaration is absent, not unreadable.
fn manifest_less_envelope() -> String {
    json!({
        "ok": false,
        "status": "invalid_input",
        "error": {
            "code": "invalid_input",
            "message": "Project does not expose a valid ProjectCentral source: No such file or directory (os error 2)",
        }
    })
    .to_string()
}

impl WorldRunner {
    fn with_answer(world_ref: &str, sources: Value) -> Self {
        Self {
            answers: BTreeMap::from([(world_ref.to_owned(), sources)]),
            fail_on: BTreeMap::new(),
            unreadable_on: BTreeMap::new(),
            coded_absent: BTreeMap::new(),
            manifest_less: BTreeMap::new(),
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

    /// Central answers that the world ref has no authored record, naming it in
    /// the error code rather than only in the message.
    fn coded_absent_on(mut self, world_ref: &str) -> Self {
        self.coded_absent.insert(world_ref.to_owned(), ());
        self
    }

    /// Central answers that the member has no ProjectCentral manifest at all —
    /// structural non-existence, modelled from the live envelope.
    fn manifest_less_on(mut self, world_ref: &str) -> Self {
        self.manifest_less.insert(world_ref.to_owned(), ());
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
        let (stdout, failure) = if self.manifest_less.contains_key(&world_ref) {
            (manifest_less_envelope(), true)
        } else if let Some(message) = self.unreadable_on.get(&world_ref) {
            (unreadable_envelope(message), true)
        } else if self.coded_absent.contains_key(&world_ref) {
            (coded_absent_envelope(&world_ref), true)
        } else if self.fail_on.contains_key(&world_ref) {
            (absent_envelope(&world_ref), true)
        } else {
            match self.answers.get(&world_ref) {
                Some(sources) => (self.envelope(&world_ref, sources.clone()), false),
                None => (absent_envelope(&world_ref), true),
            }
        };
        // Central's real contract (ctrl/src/cli.rs `exit_code`): a structured
        // envelope rides stdout even on failure, and `invalid_input` exits 2.
        // The mock models that contract; an adapter that demanded exit 0
        // before reading the envelope would misread every absence below as an
        // unavailability and withhold the inherited root lineage.
        let status = if failure { 2 } else { 0 };
        Ok(Output {
            status,
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
    assert_eq!(
        objects[0].ref_id().as_str(),
        "wiki:node:identity",
        "same ref — no second human"
    );
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
    assert!(
        !refs.contains(&"wiki:node:identity"),
        "excluded source withholds the entity in this context"
    );
    assert!(
        refs.contains(&"wiki:node:pasu:agent:agent/x"),
        "unrelated entities still bind"
    );
    assert!(
        absences
            .iter()
            .any(|a| a.contains("withheld") && a.contains("excluded")),
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
    assert_eq!(
        refs,
        vec!["wiki:node:identity"],
        "the materialised entity keeps the subject"
    );
    assert!(
        absences
            .iter()
            .any(|a| a.contains("re-declares") && a.contains("kept the materialised entity")),
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
        &WorldBinding {
            world_ref: "project:alpha".into(),
            inherited_root_lineage: true,
            sources: Vec::new(),
        },
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
    assert!(argv
        .windows(2)
        .any(|pair| pair[0] == "--root" && pair[1] == "/tmp/central"));
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
    let world = read_project_binding(
        &runner,
        Path::new("ctrl"),
        &PathBuf::from("/tmp/central"),
        "Beta",
        &mut absences,
    )
    .expect("root lineage applies when the project declares no world");
    assert!(
        world.inherited_root_lineage,
        "the convention is disclosed, never silent"
    );
    assert_eq!(world.sources[0].effective_revision, "1");
    assert!(
        absences.iter().any(|a| a.contains("root lineage applies")),
        "{absences:?}"
    );
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
    assert!(
        world.is_none(),
        "fail-open: no binding, composition proceeds"
    );
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
        absences
            .iter()
            .any(|a| a.contains("could not be read or validated")),
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
fn absence_is_read_from_the_error_code_when_central_names_it() {
    let runner = WorldRunner::with_answer(
        "control:root",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "1",
                "propagation_path": ["control:root"]}]),
    )
    .coded_absent_on("project:Epsilon");
    let mut absences = Vec::new();
    let world = read_project_binding(
        &runner,
        Path::new("ctrl"),
        &PathBuf::from("/tmp/central"),
        "Epsilon",
        &mut absences,
    )
    .expect("the code alone establishes absence");
    assert!(world.inherited_root_lineage);
    assert_eq!(world.sources[0].effective_revision, "1");
}

/// Regression (2026-09-18, owner-acknowledged): a Work member with NO
/// ProjectCentral manifest (e.g. `~/Central/Work/epi`) is not an unreadable
/// declaration — it is structural non-existence. Central answers the shared
/// `invalid_input` code with "Project does not expose a valid ProjectCentral
/// source: No such file or directory (os error 2)" and exit 2. Classified as
/// an unavailability, the binding came back None and the inherited root
/// graph was withheld from exactly the members that have no project record.
/// By the one-world convention those members inherit the root lineage, and
/// the inheritance is disclosed.
#[test]
fn a_member_with_no_projectcentral_manifest_inherits_the_root_lineage() {
    let runner = WorldRunner::with_answer(
        "control:root",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "1",
                "propagation_path": ["control:root"]}]),
    )
    .manifest_less_on("project:epi");
    let mut absences = Vec::new();
    let world = read_project_binding(
        &runner,
        Path::new("ctrl"),
        &PathBuf::from("/tmp/central"),
        "epi",
        &mut absences,
    )
    .expect("a manifest-less member has no project record; the root lineage applies");
    assert!(world.inherited_root_lineage);
    assert_eq!(world.world_ref, "control:root");
    assert_eq!(world.sources[0].effective_revision, "1");
    assert!(
        absences.iter().any(|a| a.contains("root lineage applies")),
        "{absences:?}"
    );
}

/// The distinction that keeps the widening honest: a manifest that EXISTS but
/// cannot be parsed answers with the same "Project does not expose a valid
/// ProjectCentral source" prefix and a different cause (the manifest path and
/// the parse error). That is an unreadable declaration, not structural
/// non-existence: no binding, no root lineage, and the root is never even
/// consulted.
#[test]
fn a_malformed_manifest_is_unreadable_not_absent() {
    let runner = WorldRunner::with_answer(
        "control:root",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "1",
                "propagation_path": ["control:root"]}]),
    )
    .unreadable_on(
        "project:Malformed",
        "Project does not expose a valid ProjectCentral source: /central/Work/Malformed/ProjectCentral/project.json is not a valid ProjectCentral manifest: expected ident at line 1 column 2",
    );
    let mut absences = Vec::new();
    let world = read_project_binding(
        &runner,
        Path::new("ctrl"),
        &PathBuf::from("/tmp/central"),
        "Malformed",
        &mut absences,
    );
    assert!(
        world.is_none(),
        "an unreadable manifest must still withhold, not inherit"
    );
    assert!(
        absences
            .iter()
            .any(|a| a.contains("could not be read or validated")),
        "{absences:?}"
    );
    assert!(
        !absences.iter().any(|a| a.contains("root lineage applies")),
        "the root lineage is not assumed: {absences:?}"
    );
    assert_eq!(
        runner.seen.lock().unwrap().len(),
        1,
        "only the project declaration was read"
    );
}

/// Same prefix, different cause: an io failure other than not-found
/// (permission denied) also means the manifest exists but cannot be read —
/// an unavailable declaration, never absence.
#[test]
fn an_unreadable_manifest_io_error_is_unavailable_not_absent() {
    let runner = WorldRunner::with_answer(
        "control:root",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "1",
                "propagation_path": ["control:root"]}]),
    )
    .unreadable_on(
        "project:Locked",
        "Project does not expose a valid ProjectCentral source: Permission denied (os error 13)",
    );
    let mut absences = Vec::new();
    let world = read_project_binding(
        &runner,
        Path::new("ctrl"),
        &PathBuf::from("/tmp/central"),
        "Locked",
        &mut absences,
    );
    assert!(world.is_none(), "unreadable withholds");
    assert!(
        absences
            .iter()
            .any(|a| a.contains("could not be read or validated")),
        "{absences:?}"
    );
    assert!(
        !absences.iter().any(|a| a.contains("root lineage applies")),
        "the root lineage is not assumed: {absences:?}"
    );
}

/// Regression (2026-09-17 knowledge-fitness round): the real ctrl answers
/// "no authored record for this world" with a structured `ok:false` envelope
/// AND exit status 2 (`invalid_input`, ctrl/src/cli.rs `exit_code`). The
/// adapter used to `require` exit 0 before reading the envelope, so every
/// real absence was misread as `world_sources_unavailable` — the binding came
/// back None, the inherited Central graph was withheld, and the whole
/// SemanticWiki faculty went dark for any project that declares no world.
/// The envelope, not the exit status, is the answer.
#[test]
fn a_nonzero_exit_never_hides_a_structured_absence() {
    let runner = WorldRunner::with_answer(
        "control:root",
        json!([{"ref": "central:source:control:root:Control/user/identity",
                "state": "available", "effective_revision": "1",
                "propagation_path": ["control:root"]}]),
    )
    .coded_absent_on("project:Zeta");
    let mut absences = Vec::new();
    let world = read_project_binding(
        &runner,
        Path::new("ctrl"),
        &PathBuf::from("/tmp/central"),
        "Zeta",
        &mut absences,
    )
    .expect("a structured absence with a non-zero exit is still an absence");
    assert!(
        world.inherited_root_lineage,
        "the convention discloses the inherited lineage"
    );
    assert_eq!(world.sources[0].effective_revision, "1");
    assert!(
        absences.iter().any(|a| a.contains("root lineage applies")),
        "{absences:?}"
    );
}

/// The other half of the contract: a non-zero exit with NO readable envelope
/// is a genuine unavailability — it must not inherit the root lineage and
/// must not be mistaken for absence.
#[test]
fn a_nonzero_exit_without_an_envelope_is_unavailable_not_absent() {
    struct Garbled;
    impl CommandRunner for Garbled {
        fn run(&self, _argv: &[String]) -> aikit_core::Result<Output> {
            Ok(Output {
                status: 2,
                stdout: "panic: not an envelope".into(),
                stderr: String::new(),
            })
        }
    }
    let mut absences = Vec::new();
    let world = read_project_binding(
        &Garbled,
        Path::new("ctrl"),
        &PathBuf::from("/tmp/central"),
        "Eta",
        &mut absences,
    );
    assert!(world.is_none(), "unavailable degrades to uncontextualised");
    assert!(
        absences
            .iter()
            .any(|a| a.contains("could not be read or validated")),
        "no root lineage is assumed: {absences:?}"
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
