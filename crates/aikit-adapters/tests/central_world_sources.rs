//! W10 V5 — root→project contextualisation: project contexts bind the same
//! entity refs through Central's effective world sources; declared
//! exclusions withhold in-context; a project never mints a second subject;
//! unavailable World relations withhold owner-dependent material.

use aikit_adapters::central_world_sources::{
    bind_project_context, project_world_ref, read_project_binding, read_world_binding,
    EffectiveSource, WorldBinding, BINDING_EXTENSION,
};
use aikit_adapters::runner::{CommandRunner, Output, SystemRunner};
use aikit_core::{
    ResourceRef, SemanticRevision, SourceRef, WikiNode, WikiObject, WikiProvenanceRef,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
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

fn binding() -> WorldBinding {
    WorldBinding {
        world_ref: "project:alpha".into(),
        inherited_root_lineage: false,
        sources: vec![EffectiveSource {
            source_ref: "central:source:control:root:Control/user/identity".into(),
            state: "available".into(),
            effective_revision: "1".into(),
            propagation_path: vec!["project:alpha".into(), "control:root".into()],
            native_relation: None,
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

// These gates require the composed Central source cut, including typed World
// facets and requested-absence/broken-ancestry distinction. Ignored discovery
// is not acceptance; qualification must run this suite with --ignored.
struct NativeWorld {
    owned: tempfile::TempDir,
    root: PathBuf,
    executable: PathBuf,
    runner: SystemRunner,
}

impl NativeWorld {
    fn new() -> Self {
        let executable = std::env::var_os("CENTRAL_CTRL_BIN")
            .map(PathBuf::from)
            .expect(
            "Real native World qualification requires CENTRAL_CTRL_BIN for the composed owner cut",
        );
        assert!(
            executable.is_absolute(),
            "Native owner executable must be an explicit absolute path"
        );
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        fs::create_dir_all(&scratch).unwrap();
        let owned = tempfile::Builder::new()
            .prefix("aikit-native-world-")
            .tempdir()
            .unwrap();
        let root = owned.path().join("Central");
        fs::create_dir(&root).unwrap();
        let world = Self {
            owned,
            root: fs::canonicalize(root).unwrap(),
            executable,
            runner: SystemRunner::new().with_env_removed("CENTRAL_NATIVE_TOKEN"),
        };
        world.success("central.init", json!({}));
        world.save("root", None, "control:root", None, json!([
            {"ref":"central:source:control:root:identity", "revision":"identity-v1",
                "authority":"controlled-test-fixture-not-personal-adoption", "treatment":"canonical"},
            {"ref":"central:source:control:root:sealed", "revision":"sealed-v1",
                "authority":"controlled-test-fixture-not-personal-adoption", "treatment":"canonical"}
        ]), json!([]));
        world
    }

    fn invoke(&self, action: &str, input: Value) -> Value {
        let argv = vec![
            self.executable.to_str().unwrap().to_owned(),
            "--json".into(),
            "--root".into(),
            self.root.to_str().unwrap().to_owned(),
            "action".into(),
            "run".into(),
            action.into(),
            input.to_string(),
        ];
        let output = self
            .runner
            .run_with_limits(&argv, Duration::from_secs(10), 1024 * 1024, true)
            .unwrap_or_else(|error| {
                panic!(
                    "Actual native {action} transport: {}: {error}",
                    error.code()
                )
            });
        serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "Actual native {action}: {error}; status={} stdout={} stderr={}",
                output.status, output.stdout, output.stderr
            )
        })
    }

    fn success(&self, action: &str, input: Value) -> Value {
        let result = self.invoke(action, input);
        assert_eq!(result["ok"], true, "Actual native {action}: {result}");
        result["data"].clone()
    }

    fn project(&self, member: &str, id: &str) -> String {
        fs::create_dir(self.root.join("Work").join(member)).unwrap();
        self.success(
            "projectcentral.init",
            json!({"project":member,"project_id":id}),
        );
        let here = self.success("central.world.here", json!({"project":member}));
        assert_eq!(here["schema"], "central.world-here/v1", "{here}");
        assert_eq!(here["project_world"]["state"], "present", "{here}");
        here["project_world"]["ref"].as_str().unwrap().to_owned()
    }

    fn save(
        &self,
        scope: &str,
        project: Option<&str>,
        reference: &str,
        parent: Option<&str>,
        sources: Value,
        excluded: Value,
    ) {
        let mut input = json!({"scope":scope, "record":{
            "schema":"central.world-relations/v1","ref":reference,"revision":"world-v1",
            "parent":parent,"sources":sources,"excluded_sources":excluded}});
        if let Some(project) = project {
            input["project"] = json!(project);
        }
        self.success("central.world-relations.save", input);
    }

    fn binding(&self, member: &str) -> (Option<WorldBinding>, Vec<String>) {
        let mut absences = Vec::new();
        let result = read_project_binding(
            &self.runner,
            &self.executable,
            &self.root,
            member,
            &mut absences,
        );
        (result, absences)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceBasis {
    bytes: Vec<u8>,
    #[cfg(unix)]
    physical: (u64, u64, i64, i64),
}

fn tree_basis(root: &Path) -> BTreeMap<PathBuf, SourceBasis> {
    fn walk(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, SourceBasis>) {
        for item in fs::read_dir(path).unwrap() {
            let path = item.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert!(
                !metadata.is_symlink(),
                "Unexpected fixture alias {}",
                path.display()
            );
            if metadata.is_dir() {
                walk(root, &path, out);
            } else if metadata.is_file() {
                #[cfg(unix)]
                use std::os::unix::fs::MetadataExt;
                out.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    SourceBasis {
                        bytes: fs::read(&path).unwrap(),
                        #[cfg(unix)]
                        physical: (
                            metadata.dev(),
                            metadata.ino(),
                            metadata.mtime(),
                            metadata.mtime_nsec(),
                        ),
                    },
                );
            }
        }
    }
    let mut basis = BTreeMap::new();
    walk(root, root, &mut basis);
    basis
}

// This is a forwarding observer of real owner execution, not a native answer.
// It changes only a disposable source after the actual command has returned.
type NativeCheckpoint = Box<dyn FnOnce() + Send>;
struct ObservedNative {
    runner: SystemRunner,
    seen: Mutex<Vec<(String, Duration, usize, bool)>>,
    after_effective: Mutex<Option<NativeCheckpoint>>,
}

impl ObservedNative {
    fn new(after_effective: Option<NativeCheckpoint>) -> Self {
        Self {
            runner: SystemRunner::new().with_env_removed("CENTRAL_NATIVE_TOKEN"),
            seen: Mutex::new(Vec::new()),
            after_effective: Mutex::new(after_effective),
        }
    }
}

impl CommandRunner for ObservedNative {
    fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
        self.runner.run(argv)
    }
    fn run_with_limits(
        &self,
        argv: &[String],
        timeout: Duration,
        bytes: usize,
        strict: bool,
    ) -> aikit_core::Result<Output> {
        let action = argv.get(6).expect("Actual native Action argv").clone();
        self.seen
            .lock()
            .unwrap()
            .push((action.clone(), timeout, bytes, strict));
        let output = self.runner.run_with_limits(argv, timeout, bytes, strict)?;
        if action == "central.world.effective-sources" {
            if let Some(checkpoint) = self.after_effective.lock().unwrap().take() {
                checkpoint();
            }
        }
        Ok(output)
    }
}

#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN; run explicitly in native owner qualification"]
fn native_bare_prefixed_and_slash_ids_use_owner_ref_without_remint() {
    let world = NativeWorld::new();
    for (member, id) in [
        ("Bare", "alpha"),
        ("Prefixed", "project:alpha"),
        ("Slash", "domain/alpha"),
    ] {
        let native = world.project(member, id);
        assert_eq!(native, format!("project:{id}"));
        let before = tree_basis(&world.root);
        assert_eq!(project_world_ref(&world.root, member).unwrap(), native);
        let (binding, disclosures) = world.binding(member);
        let binding = binding.unwrap_or_else(|| panic!("{disclosures:?}"));
        assert_eq!(binding.world_ref, "control:root");
        assert!(binding.inherited_root_lineage);
        assert!(disclosures
            .iter()
            .any(|line| line.contains("central.world_declaration_absent")));
        assert_eq!(
            tree_basis(&world.root),
            before,
            "Read must not declare/adopt a World"
        );
    }
}

#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN; run explicitly in native owner qualification"]
fn native_source_override_exclusion_and_every_provenance_hop_reach_context() {
    let world = NativeWorld::new();
    let reference = world.project("Alpha", "alpha");
    world.save("project", Some("Alpha"), &reference, Some("control:root"), json!([
        {"ref":"central:source:control:root:identity","revision":"identity-v2",
            "authority":"controlled-child-fixture-not-personal-adoption","treatment":"retain-native"}
    ]), json!(["central:source:control:root:sealed"]));
    let expected = world.success(
        "central.world.effective-sources",
        json!({"scope":"project","project":"Alpha","world_ref":reference}),
    );
    let before = tree_basis(&world.root);
    let (binding, absences) = world.binding("Alpha");
    assert!(absences.is_empty(), "{absences:?}");
    let binding = binding.unwrap();
    assert_eq!(binding.world_ref, reference);
    assert!(!binding.inherited_root_lineage);
    for entry in expected["sources"].as_array().unwrap() {
        let actual = binding
            .sources
            .iter()
            .find(|source| source.source_ref == entry["ref"].as_str().unwrap())
            .unwrap();
        assert_eq!(actual.native_relation.as_ref(), Some(entry));
        assert!(!entry["provenance"].as_array().unwrap().is_empty());
    }
    let identity = binding
        .sources
        .iter()
        .find(|source| source.source_ref.ends_with(":identity"))
        .unwrap();
    assert_eq!(identity.effective_revision, "identity-v2");
    assert_eq!(
        identity.native_relation.as_ref().unwrap()["provenance"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let mut objects = vec![
        WikiObject::Node(materialised(pasu_node(
            "wiki:node:identity",
            "central:pasu:nara:fixture",
            &["central:source:control:root:identity"],
        ))),
        WikiObject::Node(materialised(pasu_node(
            "wiki:node:sealed",
            "central:pasu:nara:sealed",
            &["central:source:control:root:sealed"],
        ))),
    ];
    let mut output_absences = Vec::new();
    bind_project_context(&mut objects, &binding, &mut output_absences);
    assert_eq!(objects.len(), 1);
    let WikiObject::Node(node) = &objects[0] else {
        panic!("Expected actual bound entity");
    };
    assert_eq!(
        node.extensions[BINDING_EXTENSION]["bindings"][0]["native_relation"],
        identity.native_relation.as_ref().unwrap().clone()
    );
    assert_eq!(node.ref_id.as_str(), "wiki:node:identity");
    assert_eq!(tree_basis(&world.root), before);
}

#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN; run explicitly in native owner qualification"]
fn native_present_target_with_missing_ancestor_never_uses_root_fallback() {
    let world = NativeWorld::new();
    let reference = world.project("Broken", "broken");
    world.save(
        "project",
        Some("Broken"),
        &reference,
        Some("project:missing-parent"),
        json!([]),
        json!(["central:source:control:root:identity"]),
    );
    let before = tree_basis(&world.root);
    let observer = ObservedNative::new(None);
    let mut absences = Vec::new();
    assert!(read_project_binding(
        &observer,
        &world.executable,
        &world.root,
        "Broken",
        &mut absences
    )
    .is_none());
    let diagnostic = absences.join("\n");
    assert!(
        diagnostic.contains("central.world_ancestry_unavailable"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("requested_declaration_present"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("project:missing-parent"),
        "{diagnostic}"
    );
    assert_eq!(
        observer.seen.lock().unwrap().len(),
        2,
        "here + target only; no root fallback"
    );
    assert_eq!(tree_basis(&world.root), before);
}

#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN; run explicitly in native owner qualification"]
fn native_manifest_less_existing_member_differs_from_nonexistent_member() {
    let world = NativeWorld::new();
    fs::create_dir(world.root.join("Work/Bare")).unwrap();
    let before = tree_basis(&world.root);
    let (binding, absences) = world.binding("Bare");
    let binding = binding.unwrap_or_else(|| panic!("{absences:?}"));
    assert_eq!(binding.world_ref, "control:root");
    assert!(binding.inherited_root_lineage);
    assert!(
        project_world_ref(&world.root, "Bare").is_err(),
        "No fabricated Project World identity"
    );
    let (missing, absences) = world.binding("Missing");
    assert!(missing.is_none());
    assert!(
        absences.join("\n").contains("work-member-absent"),
        "{absences:?}"
    );
    assert_eq!(tree_basis(&world.root), before);
}

#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN; run explicitly in native owner qualification"]
fn native_malformed_wrong_form_and_missing_owner_do_not_become_declaration_absence() {
    let world = NativeWorld::new();
    world.project("Alpha", "alpha");
    let manifest = world.root.join("Work/Alpha/ProjectCentral/project.json");
    let original = fs::read(&manifest).unwrap();
    for invalid in [
        b"{broken".as_slice(),
        br#"{"schema":"wrong","project_id":"alpha"}"#.as_slice(),
    ] {
        fs::write(&manifest, invalid).unwrap();
        let before = tree_basis(&world.root);
        let (binding, absences) = world.binding("Alpha");
        assert!(binding.is_none());
        assert!(absences.join("\n").contains("unavailable"), "{absences:?}");
        assert_eq!(tree_basis(&world.root), before);
    }
    fs::write(&manifest, &original).unwrap();
    let retained = manifest.with_extension("retained");
    fs::rename(&manifest, &retained).unwrap();
    fs::create_dir(&manifest).unwrap();
    let (binding, absences) = world.binding("Alpha");
    assert!(binding.is_none());
    assert!(absences.join("\n").contains("io_error"), "{absences:?}");
    fs::remove_dir(&manifest).unwrap();
    fs::rename(&retained, &manifest).unwrap();
    let error = read_world_binding(
        &world.runner,
        &world.owned.path().join("missing-ctrl"),
        &world.root,
        "root",
        None,
        "control:root",
    )
    .unwrap_err();
    use std::error::Error;
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
    assert_ne!(error.code(), "central.world_declaration_absent");
    assert_eq!(fs::read(&manifest).unwrap(), original);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN and nonroot OS user; run explicitly"]
fn native_manifest_eacces_retains_actual_owner_cause_and_never_assumes_root() {
    use std::os::unix::fs::PermissionsExt;
    assert!(
        !rustix::process::geteuid().is_root(),
        "Actual EACCES qualification requires a nonroot OS user"
    );
    let world = NativeWorld::new();
    world.project("Locked", "locked");
    let manifest = world.root.join("Work/Locked/ProjectCentral/project.json");
    let original = fs::read(&manifest).unwrap();
    struct Restore(PathBuf, fs::Permissions);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Err(error) = fs::set_permissions(&self.0, self.1.clone()) {
                if std::thread::panicking() {
                    eprintln!("Owned permission restoration failed: {error}");
                } else {
                    panic!("Owned permission restoration failed: {error}");
                }
            }
        }
    }
    let restore = Restore(
        manifest.clone(),
        fs::metadata(&manifest).unwrap().permissions(),
    );
    fs::set_permissions(&manifest, fs::Permissions::from_mode(0)).unwrap();
    let actual = fs::read(&manifest).unwrap_err();
    let (binding, absences) = world.binding("Locked");
    assert!(binding.is_none());
    let diagnostic = absences.join("\n");
    assert!(diagnostic.contains("PermissionDenied"), "{diagnostic}");
    assert!(
        diagnostic.contains(&actual.raw_os_error().unwrap().to_string()),
        "{diagnostic}"
    );
    assert!(!diagnostic.contains("root lineage applies"));
    drop(restore);
    assert_eq!(fs::read(&manifest).unwrap(), original);
}

#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN; run explicitly in native owner qualification"]
fn native_shared_probe_limits_and_final_identity_reobservation_are_operative() {
    let world = NativeWorld::new();
    world.project("Alpha", "alpha");
    let observer = ObservedNative::new(None);
    let before = tree_basis(&world.root);
    let mut absences = Vec::new();
    assert!(
        read_project_binding(
            &observer,
            &world.executable,
            &world.root,
            "Alpha",
            &mut absences
        )
        .is_some(),
        "{absences:?}"
    );
    let calls = observer.seen.lock().unwrap();
    assert_eq!(calls.len(), 4);
    for call in calls.iter() {
        assert!(call.1 <= Duration::from_secs(10));
        assert!(call.2 <= 1024 * 1024);
        assert!(call.3);
    }
    for pair in calls.windows(2) {
        assert!(pair[1].1 <= pair[0].1);
        assert!(pair[1].2 < pair[0].2);
    }
    drop(calls);
    assert_eq!(tree_basis(&world.root), before);
    let manifest = world.root.join("Work/Alpha/ProjectCentral/project.json");
    let original = fs::read(&manifest).unwrap();
    let changed_path = manifest.clone();
    let change = ObservedNative::new(Some(Box::new(move || {
        let mut value: Value = serde_json::from_slice(&fs::read(&changed_path).unwrap()).unwrap();
        value["project_id"] = json!("changed-after-native-return");
        fs::write(&changed_path, serde_json::to_vec(&value).unwrap()).unwrap();
    })));
    let mut absences = Vec::new();
    assert!(read_project_binding(
        &change,
        &world.executable,
        &world.root,
        "Alpha",
        &mut absences
    )
    .is_none());
    assert!(
        absences
            .join("\n")
            .contains("central.world_binding_changed"),
        "{absences:?}"
    );
    fs::write(&manifest, original).unwrap();
    assert!(
        world.binding("Alpha").0.is_some(),
        "Fresh native inspection after fixture restoration"
    );
}

#[cfg(unix)]
#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN; run explicitly in native owner qualification"]
fn native_unchanged_root_alias_works_and_retargeted_owner_ref_is_refused() {
    use std::os::unix::fs::symlink;
    let first = NativeWorld::new();
    first.project("Alpha", "first");
    let second = NativeWorld::new();
    second.project("Alpha", "second");
    let alias = first.owned.path().join("world-alias");
    symlink(&first.root, &alias).unwrap();
    let mut absences = Vec::new();
    assert!(
        read_project_binding(
            &first.runner,
            &first.executable,
            &alias,
            "Alpha",
            &mut absences
        )
        .is_some(),
        "{absences:?}"
    );
    let second_root = second.root.clone();
    let alias_for_change = alias.clone();
    let change = ObservedNative::new(Some(Box::new(move || {
        fs::remove_file(&alias_for_change).unwrap();
        symlink(second_root, &alias_for_change).unwrap();
    })));
    let mut absences = Vec::new();
    assert!(
        read_project_binding(&change, &first.executable, &alias, "Alpha", &mut absences).is_none()
    );
    assert!(
        absences
            .join("\n")
            .contains("central.world_binding_changed"),
        "{absences:?}"
    );
    assert!(read_project_binding(
        &first.runner,
        &first.executable,
        &alias,
        "Alpha",
        &mut Vec::new()
    )
    .is_some());
}

#[cfg(unix)]
#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN; run explicitly in native owner qualification"]
fn native_non_utf8_coordinate_refuses_before_selecting_lossy_owner() {
    use std::os::unix::ffi::OsStringExt;
    let world = NativeWorld::new();
    let before = tree_basis(&world.root);
    let root = PathBuf::from(std::ffi::OsString::from_vec(b"invalid-owner-\xff".to_vec()));
    let error = read_world_binding(
        &world.runner,
        &world.executable,
        &root,
        "root",
        None,
        "control:root",
    )
    .unwrap_err();
    assert_eq!(error.code(), "central.world_transport_unsupported");
    assert_eq!(
        error.details().get("coordinate").map(String::as_str),
        Some("root")
    );
    assert_eq!(
        error.details().get("execution_started").map(String::as_str),
        Some("false")
    );
    assert_eq!(tree_basis(&world.root), before);
}

#[cfg(unix)]
#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN and mkfifo; run explicitly"]
fn native_final_manifest_alias_and_fifo_refuse_without_opening_replacement() {
    use std::os::unix::fs::symlink;
    let world = NativeWorld::new();
    world.project("Alpha", "alpha");
    let manifest = world.root.join("Work/Alpha/ProjectCentral/project.json");
    let retained = manifest.with_extension("original");
    fs::rename(&manifest, &retained).unwrap();
    let original = fs::read(&retained).unwrap();
    symlink(&retained, &manifest).unwrap();
    let (binding, absences) = world.binding("Alpha");
    assert!(binding.is_none());
    assert!(absences.join("\n").contains("io_error"), "{absences:?}");
    fs::remove_file(&manifest).unwrap();
    let argv = vec![
        "mkfifo".into(),
        "-m".into(),
        "600".into(),
        manifest.to_str().unwrap().into(),
    ];
    world
        .runner
        .run_with_limits(&argv, Duration::from_secs(2), 1024, true)
        .unwrap()
        .require(&argv, "test.mkfifo_failed")
        .unwrap();
    let (binding, absences) = world.binding("Alpha");
    assert!(binding.is_none());
    assert!(absences.join("\n").contains("io_error"), "{absences:?}");
    assert!(!absences.join("\n").contains("root lineage applies"));
    assert_eq!(fs::read(&retained).unwrap(), original);
    fs::remove_file(&manifest).unwrap();
    fs::rename(retained, manifest).unwrap();
    assert!(world.binding("Alpha").0.is_some());
}

#[cfg(unix)]
#[test]
#[ignore = "requires real composed CENTRAL_CTRL_BIN; run explicitly in native owner qualification"]
fn native_runner_decode_and_timeout_failures_never_become_absent_owner() {
    use std::os::unix::fs::PermissionsExt;
    let world = NativeWorld::new();
    world.project("Alpha", "alpha");
    let before = tree_basis(&world.root);
    let wrapper = world.owned.path().join("actual-native-wrapper");
    // Material adversary precedes a genuine native command. No native envelope
    // is scripted; the same runner owns finite capture and cancellation/reap.
    fs::write(
        &wrapper,
        "#!/bin/sh\nprintf '\\377'\nexec \"$ACTUAL_CTRL\" \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let runner = SystemRunner::new()
        .with_env("ACTUAL_CTRL", world.executable.to_str().unwrap())
        .with_env_removed("CENTRAL_NATIVE_TOKEN");
    let error = read_world_binding(&runner, &wrapper, &world.root, "root", None, "control:root")
        .unwrap_err();
    assert_eq!(error.code(), "mux.command_utf8_invalid");
    fs::write(
        &wrapper,
        "#!/bin/sh\nsleep 5\nexec \"$ACTUAL_CTRL\" \"$@\"\n",
    )
    .unwrap();
    let runner = runner.with_timeout(Duration::from_millis(120));
    let mut absences = Vec::new();
    assert!(read_project_binding(&runner, &wrapper, &world.root, "Alpha", &mut absences).is_none());
    assert!(
        absences.join("\n").contains("mux.command_timeout"),
        "{absences:?}"
    );
    assert!(!absences.join("\n").contains("root lineage applies"));
    assert_eq!(tree_basis(&world.root), before);
}
