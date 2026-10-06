//! Actual Central file-map and SourcePool boundary exercises. Explicit native
//! tests require the qualification job's built/pinned owner binary; no fake
//! owner, generated response, guessed SourceRef or unavailable-as-green skip.
use aikit_adapters::central_file_map::{call, CentralFileMapProvider};
use aikit_adapters::runner::{CommandRunner, Output, SystemRunner};
use aikit_core::context_source::{ContextSourcePrivacy, RetrievalTarget};
use aikit_core::familiarity::FamiliarityContext;
use aikit_core::knowledge_navigation::{KnowledgeAddress, KnowledgeApplication};
use aikit_core::knowledge_source_pool::{SourceOriginKind, SourcePoolProvider};
use aikit_core::resource::SourceRef;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

// Delegate every invocation to the real owner. The only interleaving is an
// actual fixture-file change after its completed search receipt; the original
// native bytes, exit status and errors are returned unchanged.
struct SearchReceiptInterleaving {
    runner: SystemRunner,
    source_path: PathBuf,
    replacement: Option<&'static str>,
    receipt: Mutex<Option<Value>>,
}

impl SearchReceiptInterleaving {
    fn after_search(&self, argv: &[String], output: Output) -> aikit_core::Result<Output> {
        if argv
            .iter()
            .any(|argument| argument == "central.file-map.search")
        {
            let mut receipt = self.receipt.lock().unwrap();
            if receipt.is_none() {
                *receipt = Some(
                    serde_json::from_str(&output.stdout)
                        .expect("the real owner must return a native JSON search receipt"),
                );
                if let Some(body) = self.replacement {
                    fs::write(&self.source_path, body).expect("actual owned-file interleaving");
                }
            }
        }
        Ok(output)
    }
}

impl CommandRunner for SearchReceiptInterleaving {
    fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
        self.after_search(argv, self.runner.run(argv)?)
    }

    fn configured_timeout(&self) -> Option<Duration> {
        self.runner.configured_timeout()
    }

    fn run_with_timeout(&self, argv: &[String], timeout: Duration) -> aikit_core::Result<Output> {
        self.after_search(argv, self.runner.run_with_timeout(argv, timeout)?)
    }
}

fn scratch() -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
    fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("native-file-map-")
        .tempdir_in(root)
        .unwrap()
}

fn owner() -> PathBuf {
    let path = PathBuf::from(std::env::var_os("AIKIT_CENTRAL_REAL_BIN").expect(
        "explicit native qualification requires AIKIT_CENTRAL_REAL_BIN for the built/pinned ctrl",
    ));
    assert!(
        path.is_file(),
        "actual owner binary is required: {}",
        path.display()
    );
    path
}

fn action_envelope(ctrl: &Path, root: &Path, name: &str, input: Value) -> (Output, Value) {
    let mut command = std::process::Command::new(ctrl);
    command
        .args(["--json", "--root"])
        .arg(root)
        .args(["action", "run", name])
        .arg(input.to_string());
    let output = SystemRunner::new()
        .with_timeout(Duration::from_secs(30))
        .with_strict_utf8()
        .capture_command(&mut command)
        .unwrap();
    let value: Value = serde_json::from_str(&output.stdout).unwrap();
    (output, value)
}

fn file_map_result(data: &Value, operation: &str) -> Value {
    assert_eq!(data["schema"], "central.file-map/v1", "{data}");
    assert_eq!(data["operation"], operation, "{data}");
    assert!(data["result"].is_object(), "{data}");
    data["result"].clone()
}

fn action(ctrl: &Path, root: &Path, name: &str, input: Value) -> Value {
    let (output, value) = action_envelope(ctrl, root, name, input);
    assert!(
        output.ok() && value["ok"] == true,
        "{value}; stderr={}",
        output.stderr
    );
    if let Some(operation) = name.strip_prefix("central.file-map.") {
        return file_map_result(&value["data"], operation);
    }
    value["data"].clone()
}

fn native_withdrawal(ctrl: &Path, root: &Path, source: &SourceRef, content: bool) -> Value {
    let (output, native) = action_envelope(
        ctrl,
        root,
        "central.file-map.resolve",
        json!({
            "project":null,"federated":true,"resources":false,"source_ref":source,"content":content,
        }),
    );
    assert!(
        !output.ok() && native["ok"] == false,
        "the actual withdrawal must refuse: {native}; stderr={}",
        output.stderr
    );
    assert!(
        native["error"]["code"]
            .as_str()
            .is_some_and(|code| !code.is_empty()),
        "{native}"
    );
    assert!(
        native["data"].is_null(),
        "withdrawal may not return source body: {native}"
    );
    native
}

fn same_native_failure(error: &aikit_core::AikitError, native: &Value) {
    assert_eq!(error.code(), native["error"]["code"].as_str().unwrap());
    assert_eq!(
        serde_json::from_str::<Value>(&error.details()["native_result"]).unwrap(),
        *native
    );
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN"]
fn live_native_identity_revision_withdrawal_and_new_source_survive_one_attachment() {
    let ctrl = owner();
    let owned = scratch();
    let canonical_root = fs::canonicalize(owned.path()).unwrap();
    let root = canonical_root.as_path();
    action(&ctrl, root, "central.init", json!({}));
    let note = root.join("Control/user/note.md");
    fs::write(&note, "# Initial owner source\n").unwrap();
    let runner = SystemRunner::new().with_timeout(Duration::from_secs(30));
    let reading = call(&runner, &ctrl, root, "locate", &json!({"path":note})).unwrap();
    let source = SourceRef::parse(reading["source"]["ref"].as_str().unwrap()).unwrap();
    let mut provider = CentralFileMapProvider::connect(runner, &ctrl, root, None).unwrap();
    assert!(
        provider.descriptors().is_empty(),
        "no copied owner roster at attachment"
    );
    assert!(
        provider.rebuild(&[]).is_err(),
        "persistent owner map is never rebuilt by this consumer"
    );
    assert!(
        !provider.capabilities().fulltext,
        "a fresh uninitialised owner map is honestly unavailable"
    );
    let held = provider.read(&source).unwrap().unwrap();
    assert_eq!(held.body, "# Initial owner source\n");
    assert_eq!(held.binding.media_type, "text/markdown");
    assert_eq!(
        held.binding.source.as_str(),
        reading["source"]["ref"].as_str().unwrap()
    );
    assert_eq!(
        held.binding.revision.as_str(),
        reading["revision"].as_str().unwrap()
    );
    let origin = held.binding.source_origin().unwrap().unwrap();
    match origin.origin {
        SourceOriginKind::NativeSource {
            world_ref,
            source: native,
            observed_binding,
        } => {
            assert_eq!(world_ref, reading["world_ref"].as_str().unwrap());
            assert_eq!(native.source, source);
            assert_eq!(native.revision.unwrap(), held.binding.revision);
            assert!(native.locator.is_none());
            assert_eq!(
                observed_binding.roles,
                serde_json::from_value::<Vec<String>>(reading["source"]["roles"].clone()).unwrap()
            );
            assert_eq!(
                observed_binding.provenance,
                reading["source"]["provenance"].as_str().unwrap()
            );
            assert_eq!(
                observed_binding.standing,
                reading["source"]["standing"].as_str().unwrap()
            );
            assert_eq!(
                observed_binding.treatment,
                reading["source"]["treatment"].as_str().unwrap()
            );
        }
        other => panic!("actual native source cannot be a standalone declaration: {other:?}"),
    }
    let local = provider
        .read_for(&source, RetrievalTarget::LocalAgent)
        .unwrap()
        .unwrap();
    assert_eq!(local.privacy, ContextSourcePrivacy::default());
    assert_eq!(local.material, held);
    let human = provider
        .read_for(&source, RetrievalTarget::Human)
        .unwrap()
        .unwrap();
    assert_eq!(human.material, held);
    assert_eq!(
        provider
            .read_for(&source, RetrievalTarget::ExternalProvider)
            .unwrap_err()
            .code(),
        "knowledge.source_target_withheld"
    );
    let address = KnowledgeAddress::Source(source.clone());
    for target in [RetrievalTarget::Human, RetrievalTarget::LocalAgent] {
        let unheld = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &[])
            .with_retrieval_target(target);
        let actual = unheld.read(&address).unwrap();
        assert_eq!(actual.content.as_deref(), Some(held.body.as_str()));
        assert_eq!(
            actual.revision.as_deref(),
            Some(held.binding.revision.as_str())
        );
        assert!(unheld
            .explain(&address)
            .unwrap()
            .detail
            .unwrap()
            .get("locator")
            .is_none());
        assert_eq!(
            unheld
                .route(None, std::slice::from_ref(&address))
                .unwrap()
                .steps[0]
                .revision
                .as_deref(),
            Some(held.binding.revision.as_str())
        );
    }
    let unheld_external = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&provider, &[])
        .with_retrieval_target(RetrievalTarget::ExternalProvider);
    assert_eq!(
        unheld_external.read(&address).unwrap_err().code(),
        "knowledge.source_target_withheld"
    );
    assert_eq!(
        unheld_external.explain(&address).unwrap_err().code(),
        "knowledge.source_target_withheld"
    );
    assert_eq!(
        unheld_external
            .route(None, std::slice::from_ref(&address))
            .unwrap_err()
            .code(),
        "knowledge.source_target_withheld"
    );
    let materials = vec![held.clone()];
    let external = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&provider, &materials)
        .with_retrieval_target(RetrievalTarget::ExternalProvider);
    assert_eq!(
        external.read(&address).unwrap_err().code(),
        "knowledge.source_target_withheld"
    );
    assert_eq!(
        external.explain(&address).unwrap_err().code(),
        "knowledge.source_target_withheld"
    );
    assert_eq!(
        external
            .route(None, std::slice::from_ref(&address))
            .unwrap_err()
            .code(),
        "knowledge.source_target_withheld"
    );
    let bound = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&provider, &materials);
    let explanation = bound
        .explain(&KnowledgeAddress::Source(source.clone()))
        .unwrap()
        .detail
        .unwrap();
    assert!(explanation.get("locator").is_none());
    assert!(explanation["metadata"].get("central").is_none());
    assert!(!explanation.to_string().contains(root.to_str().unwrap()));
    assert_eq!(
        bound
            .read(&KnowledgeAddress::Source(source.clone()))
            .unwrap()
            .content
            .as_deref(),
        Some(held.body.as_str())
    );
    fs::write(&note, "# Current owner source\n").unwrap();
    let stale = bound
        .read(&KnowledgeAddress::Source(source.clone()))
        .unwrap_err();
    assert_eq!(stale.code(), "knowledge.source_origin_revision_conflict");
    assert_eq!(
        bound
            .explain(&KnowledgeAddress::Source(source.clone()))
            .unwrap_err()
            .code(),
        stale.code()
    );
    assert_eq!(
        bound
            .route(None, &[KnowledgeAddress::Source(source.clone())])
            .unwrap_err()
            .code(),
        stale.code()
    );
    let current = provider.read(&source).unwrap().unwrap();
    assert_eq!(current.body, "# Current owner source\n");
    assert_ne!(current.binding.revision, held.binding.revision);
    let late = root.join("late-source.md");
    fs::write(&late, "# Added after attachment\n").unwrap();
    let basis = call(
        &SystemRunner::new(),
        &ctrl,
        root,
        "inspect",
        &json!({"resources":false}),
    )
    .unwrap();
    let registered = action(
        &ctrl,
        root,
        "central.file-map.register",
        json!({
            "path":"late-source.md", "expected_revision":basis["revision"],
        }),
    );
    let late_ref = SourceRef::parse(registered["source_ref"].as_str().unwrap()).unwrap();
    assert_eq!(
        provider.read(&late_ref).unwrap().unwrap().body,
        "# Added after attachment\n"
    );
    fs::write(
        root.join("Control/user/.no-agent-retrieval"),
        "withdrawal\n",
    )
    .unwrap();
    let native = native_withdrawal(&ctrl, root, &source, true);
    let denied = bound
        .read(&KnowledgeAddress::Source(source.clone()))
        .unwrap_err();
    same_native_failure(&denied, &native);
    for target in [RetrievalTarget::Human, RetrievalTarget::LocalAgent] {
        let unheld = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &[])
            .with_retrieval_target(target);
        assert_eq!(
            unheld.read(&address).unwrap_err().details()["native_result"],
            denied.details()["native_result"]
        );
        assert_eq!(
            unheld.explain(&address).unwrap_err().details()["native_result"],
            denied.details()["native_result"]
        );
        assert_eq!(
            unheld
                .route(None, std::slice::from_ref(&address))
                .unwrap_err()
                .details()["native_result"],
            denied.details()["native_result"]
        );
    }
    let denied_explanation = bound
        .explain(&KnowledgeAddress::Source(source.clone()))
        .unwrap_err();
    let denied_route = bound
        .route(None, &[KnowledgeAddress::Source(source.clone())])
        .unwrap_err();
    assert_eq!(
        denied_route.details()["native_result"],
        denied.details()["native_result"]
    );
    assert_eq!(
        denied_explanation.details()["native_result"],
        denied.details()["native_result"]
    );
    let envelope: Value = serde_json::from_str(&denied.details()["native_result"]).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["action"], "central.file-map.resolve");
    assert_eq!(
        denied.details()["native_error_code"],
        envelope["error"]["code"].as_str().unwrap()
    );
    assert_eq!(
        serde_json::from_str::<Value>(&denied.details()["native_error"]).unwrap(),
        envelope["error"]
    );
    assert_eq!(
        fs::read_to_string(&note).unwrap(),
        "# Current owner source\n"
    );
    assert_eq!(
        provider.read(&late_ref).unwrap().unwrap().body,
        "# Added after attachment\n"
    );
}

#[test]
fn absent_owner_binary_is_genuine_unavailability_not_a_capability() {
    let owned = scratch();
    let missing = owned.path().join("no-owner-executable");
    assert!(!missing.exists());
    assert!(
        CentralFileMapProvider::connect(SystemRunner::new(), missing, owned.path(), None).is_err()
    );
}

#[test]
fn disposable_provider_refuses_central_owned_database() {
    use aikit_adapters::bkmr::BkmrSourcePoolProvider;
    let owned = scratch();
    let database = owned.path().join(".central/bkmr/map.db");
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    fs::write(&database, b"persistent owner sentinel").unwrap();
    let mut provider =
        BkmrSourcePoolProvider::with_binary(SystemRunner::new(), "missing-bkmr", &database, false);
    assert!(provider.rebuild(&[]).is_err());
    assert_eq!(fs::read(&database).unwrap(), b"persistent owner sentinel");
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and CENTRAL_BKMR_BIN"]
fn actual_indexed_native_search_and_retained_routes_withhold_withdrawn_or_changed_origins() {
    let ctrl = owner();
    let bkmr = std::env::var_os("CENTRAL_BKMR_BIN")
        .expect("explicit native search qualification requires the real pinned bkmr binary");
    assert!(
        Path::new(&bkmr).is_file(),
        "the actual search provider is required"
    );
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let note = root.join("Control/user/origin-search-needle.md");
    fs::write(
        &note,
        "# Originneedle qualification\nActual original body.\n",
    )
    .unwrap();
    let actual = action(
        &ctrl,
        &root,
        "central.file-map.locate",
        json!({"path": note}),
    );
    let source = SourceRef::parse(actual["source"]["ref"].as_str().unwrap()).unwrap();
    // This is the real owner refresh and provider index, with embeddings
    // explicitly absent. No generated row or owner reply stands in for it.
    action(
        &ctrl,
        &root,
        "central.file-map.refresh",
        json!({"embeddings":false}),
    );
    let provider = CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)),
        &ctrl,
        &root,
        None,
    )
    .unwrap();
    assert!(
        provider.capabilities().fulltext,
        "the actual index must be available for this selected qualification"
    );
    let materials = vec![provider.read(&source).unwrap().unwrap()];
    let address = KnowledgeAddress::Source(source.clone());
    let bound = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&provider, &materials);
    assert!(bound
        .search("Originneedle", 8)
        .hits
        .iter()
        .any(|hit| hit.address == address));
    let unheld =
        KnowledgeApplication::new(FamiliarityContext::default()).with_source_pool(&provider, &[]);
    assert!(unheld
        .search("Originneedle", 8)
        .hits
        .iter()
        .any(|hit| hit.address == address));
    for held in [&materials[..], &[][..]] {
        let external = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, held)
            .with_retrieval_target(RetrievalTarget::ExternalProvider);
        let withheld = external.search("Originneedle", 8);
        assert!(withheld.hits.iter().all(|hit| hit.address != address));
        let absence = withheld.absences.join("\n");
        assert!(!absence.contains(source.as_str()) && !absence.contains("Originneedle"));
        assert!(!absence.contains(note.to_str().unwrap()));
    }
    assert_eq!(
        bound
            .route(None, std::slice::from_ref(&address))
            .unwrap()
            .steps[0]
            .revision
            .as_deref(),
        Some(materials[0].binding.revision.as_str())
    );
    fs::write(
        &note,
        "# Originneedle qualification\nActual changed body.\n",
    )
    .unwrap();
    assert!(bound
        .search("Originneedle", 8)
        .hits
        .iter()
        .all(|hit| hit.address != address));
    assert_eq!(
        bound
            .route(None, std::slice::from_ref(&address))
            .unwrap_err()
            .code(),
        "knowledge.source_origin_revision_conflict"
    );
    fs::write(
        root.join("Control/user/.no-agent-retrieval"),
        "withdrawal\n",
    )
    .unwrap();
    let native = native_withdrawal(&ctrl, &root, &source, true);
    let denied = bound.read(&address).unwrap_err();
    same_native_failure(&denied, &native);
    assert!(bound
        .search("Originneedle", 8)
        .hits
        .iter()
        .all(|hit| hit.address != address));
    assert_eq!(
        bound
            .route(None, std::slice::from_ref(&address))
            .unwrap_err()
            .details()["native_result"],
        denied.details()["native_result"]
    );
    assert!(unheld
        .search("Originneedle", 8)
        .hits
        .iter()
        .all(|hit| hit.address != address));
    assert_eq!(
        unheld.read(&address).unwrap_err().details()["native_result"],
        denied.details()["native_result"]
    );
    let absences = bound.search("Originneedle", 8).absences.join("\n");
    assert!(!absences.contains(source.as_str()));
    assert!(!absences.contains("Originneedle"));
    assert!(!absences.contains(note.to_str().unwrap()));
    assert_eq!(
        fs::read_to_string(&note).unwrap(),
        "# Originneedle qualification\nActual changed body.\n"
    );
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and CENTRAL_BKMR_BIN"]
fn completed_native_search_receipt_cannot_validate_changed_current_payload() {
    let ctrl = owner();
    let bkmr = std::env::var_os("CENTRAL_BKMR_BIN")
        .expect("explicit native search qualification requires the real pinned bkmr binary");
    assert!(Path::new(&bkmr).is_file());
    for changed in [false, true] {
        let owned = scratch();
        let root = fs::canonicalize(owned.path()).unwrap();
        action(&ctrl, &root, "central.init", json!({}));
        let note = root.join("Control/user/receipt-needle.md");
        let original = "# Receiptneedle\nActual indexed owner evidence.\n";
        fs::write(&note, original).unwrap();
        let actual = action(
            &ctrl,
            &root,
            "central.file-map.locate",
            json!({"path": note}),
        );
        let source = SourceRef::parse(actual["source"]["ref"].as_str().unwrap()).unwrap();
        action(
            &ctrl,
            &root,
            "central.file-map.refresh",
            json!({"embeddings":false}),
        );
        let runner = SearchReceiptInterleaving {
            runner: SystemRunner::new().with_timeout(Duration::from_secs(30)),
            source_path: note.clone(),
            replacement: changed.then_some("# Current replacement\nThe former query is absent.\n"),
            receipt: Mutex::new(None),
        };
        let provider = CentralFileMapProvider::connect(&runner, &ctrl, &root, None).unwrap();
        assert!(
            provider.capabilities().fulltext,
            "the actual owner index is required"
        );
        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &[]);
        let result = app.search("Receiptneedle", 8);
        let receipt = runner.receipt.lock().unwrap();
        let receipt = receipt
            .as_ref()
            .expect("actual completed native search receipt");
        assert_eq!(receipt["ok"], true, "{receipt}");
        let native_hit = receipt["data"]["result"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|hit| hit["source"]["ref"] == source.as_str())
            .expect("the original owner search must genuinely match the selected file");
        assert_eq!(native_hit["revision"], actual["revision"]);
        let current = provider.read(&source).unwrap().unwrap();
        let address = KnowledgeAddress::Source(source.clone());
        if changed {
            assert_ne!(
                current.binding.revision.as_str(),
                native_hit["revision"].as_str().unwrap()
            );
            assert!(!current.body.contains("Receiptneedle"));
            assert!(
                !result.hits.iter().any(|hit| hit.address == address),
                "old match must be withheld: {result:?}"
            );
            assert!(result
                .absences
                .iter()
                .any(|absence| absence.contains("knowledge.source_origin_revision_conflict")));
            assert!(result
                .absences
                .iter()
                .all(|absence| !absence.contains(note.to_str().unwrap())
                    && !absence.contains(source.as_str())));
        } else {
            assert_eq!(current.body, original);
            assert_eq!(
                current.binding.revision.as_str(),
                native_hit["revision"].as_str().unwrap()
            );
            let returned = result
                .hits
                .iter()
                .find(|hit| hit.address == address)
                .expect("unchanged real native match");
            assert_eq!(returned.label, native_hit["title"].as_str().unwrap());
            assert_eq!(returned.snippet, native_hit["snippet"].as_str().unwrap());
            assert_eq!(returned.score, native_hit["score"].as_f64().unwrap_or(0.5));
        }
    }
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
fn nonowners_decline_before_current_central_and_independent_work_owners() {
    use aikit_adapters::work_repos::{
        discover_native_work_projects, work_file_source_ref, NativeWorkProjectEntry,
        WorkReposSourcePoolProvider,
    };
    use aikit_core::knowledge_source_pool::NativeSourcePoolProvider;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let project = root.join("Work/composition");
    fs::create_dir_all(&project).unwrap();
    action(
        &ctrl,
        &root,
        "projectcentral.init",
        json!({"project":"composition", "project_id":"composition/project"}),
    );
    let note = project.join("README.md");
    fs::write(
        &note,
        "# Independent live Work source\nCompositionneedle.\n",
    )
    .unwrap();
    let projects = discover_native_work_projects(&root)
        .unwrap()
        .into_iter()
        .map(|entry| match entry {
            NativeWorkProjectEntry::Project(project) => project,
            NativeWorkProjectEntry::Absence { name, error } => {
                panic!("actual Project discovery failed for {name}: {error}")
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(projects.len(), 1);
    let source =
        work_file_source_ref(&projects[0].project().project_id, Path::new("README.md")).unwrap();
    let work = WorkReposSourcePoolProvider::connect_native(
        SystemRunner::new().with_timeout(Duration::from_secs(30)),
        aikit_adapters::ripgrep::executable(),
        projects,
    )
    .unwrap();
    assert!(
        work.capabilities().fulltext,
        "real ripgrep is required for the selected gate"
    );
    let central = CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)),
        &ctrl,
        &root,
        None,
    )
    .unwrap();
    let index = NativeSourcePoolProvider::default();
    let app = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&index, &[])
        .with_source_pool(&central, &[])
        .with_source_pool(&work, &[]);
    assert_eq!(
        app.read(&KnowledgeAddress::Source(source.clone()))
            .unwrap()
            .content
            .as_deref(),
        Some(fs::read_to_string(&note).unwrap().as_str())
    );
    let native_matches = work
        .search(
            "Compositionneedle",
            aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext,
            &[],
            8,
        )
        .unwrap();
    let actual_match = native_matches
        .iter()
        .find(|hit| hit.source == source)
        .expect("real native Work query");
    assert_eq!(
        actual_match.revision.as_ref(),
        Some(&work.read(&source).unwrap().unwrap().binding.revision)
    );
    assert!(actual_match.snippet.contains("Compositionneedle"));
    assert!(app
        .search("Compositionneedle", 8)
        .hits
        .iter()
        .any(|hit| hit.address == KnowledgeAddress::Source(source.clone())));
    let native_path = root.join("Control/user/native-composition.md");
    fs::write(&native_path, "# Actual Central owner\n").unwrap();
    let actual = action(
        &ctrl,
        &root,
        "central.file-map.locate",
        json!({"path":native_path}),
    );
    let native_ref = SourceRef::parse(actual["source"]["ref"].as_str().unwrap()).unwrap();
    assert_eq!(
        app.read(&KnowledgeAddress::Source(native_ref.clone()))
            .unwrap()
            .content
            .as_deref(),
        Some("# Actual Central owner\n")
    );
    let external = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&index, &[])
        .with_source_pool(&central, &[])
        .with_source_pool(&work, &[])
        .with_retrieval_target(RetrievalTarget::ExternalProvider);
    assert_eq!(
        external
            .read(&KnowledgeAddress::Source(source))
            .unwrap_err()
            .code(),
        "knowledge.source_target_withheld"
    );
    assert_eq!(
        external
            .read(&KnowledgeAddress::Source(native_ref.clone()))
            .unwrap_err()
            .code(),
        "knowledge.source_target_withheld"
    );
    fs::write(
        root.join("Control/user/.no-agent-retrieval"),
        "withdrawal\n",
    )
    .unwrap();
    let native = native_withdrawal(&ctrl, &root, &native_ref, true);
    let denied = app.read(&KnowledgeAddress::Source(native_ref)).unwrap_err();
    same_native_failure(&denied, &native);
    assert_eq!(
        fs::read_to_string(native_path).unwrap(),
        "# Actual Central owner\n"
    );
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
fn attached_now_preserves_registered_project_identity_current_payload_and_native_failure() {
    use aikit_adapters::now_field::{
        default_runner, NowFieldScope, NowFieldSourcePoolProvider, ScopeInclude,
    };
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    fs::create_dir_all(root.join("Work/native-now")).unwrap();
    action(
        &ctrl,
        &root,
        "projectcentral.init",
        json!({"project":"native-now", "project_id":"native/now"}),
    );
    let path = root.join("Work/native-now/ProjectCentral/now/agents/current.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "{\"native-now-query\":\"current selected record\"}\n",
    )
    .unwrap();
    let inspected = action(
        &ctrl,
        &root,
        "central.file-map.inspect",
        json!({"project":"native-now", "resources":false}),
    );
    let registered = action(
        &ctrl,
        &root,
        "central.file-map.register",
        json!({
            "project":"native-now", "path":"ProjectCentral/now/agents/current.json",
            "expected_revision":inspected["revision"],
        }),
    );
    let source = SourceRef::parse(registered["source_ref"].as_str().unwrap()).unwrap();
    assert!(source
        .as_str()
        .starts_with("central:source:project:native/now:"));
    let relay = root.join("native-owner-relay");
    fs::write(&relay, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$0.args\"\nIFS= read -r native_ctrl < \"$0.owner\"\nexec \"$native_ctrl\" \"$@\"\n").unwrap();
    fs::write(
        root.join("native-owner-relay.owner"),
        format!("{}\n", ctrl.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&relay, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let central = Arc::new(
        CentralFileMapProvider::connect(
            SystemRunner::new().with_timeout(Duration::from_secs(30)),
            &relay,
            &root,
            None,
        )
        .unwrap(),
    );
    fs::write(root.join("native-owner-relay.args"), "").unwrap();
    let scope = NowFieldScope {
        central_root: root.clone(),
        includes: vec![ScopeInclude {
            glob: "Work/native-now/ProjectCentral/now/**/*.json".into(),
            family: "projectcentral",
        }],
        excludes: vec![],
        pruned: vec![],
    };
    let now = NowFieldSourcePoolProvider::connect(
        default_runner(&root),
        aikit_adapters::ripgrep::executable(),
        scope,
    )
    .unwrap()
    .with_native_owner(Arc::clone(&central));
    assert!(
        now.descriptors().is_empty(),
        "native construction must not preload unselected Source bodies"
    );
    assert_eq!(fs::read_to_string(root.join("native-owner-relay.args")).unwrap(), "",
        "the real native transport observer sees no owner body read during construction/descriptors");
    let local = now
        .read_for(&source, RetrievalTarget::LocalAgent)
        .unwrap()
        .unwrap();
    let native = central
        .read_for(&source, RetrievalTarget::LocalAgent)
        .unwrap()
        .unwrap();
    assert_eq!(
        local, native,
        "NOW must not substitute a Root alias or binding"
    );
    assert_eq!(local.material.body, fs::read_to_string(&path).unwrap());
    let hits = now
        .search(
            "native-now-query",
            aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext,
            &[],
            8,
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].source, source);
    assert_eq!(
        hits[0].revision.as_ref(),
        Some(&native.material.binding.revision)
    );
    assert_eq!(hits[0].provider_binding.as_deref(), Some("line:1"));
    assert!(hits[0].tags.contains(&"native-now".to_string()));
    assert!(hits[0].tags.contains(&"projectcentral".to_string()));
    assert_eq!(
        now.read_for(&source, RetrievalTarget::ExternalProvider)
            .unwrap_err()
            .code(),
        "knowledge.source_target_withheld"
    );
    fs::write(
        &path,
        "{\"native-now-current\":\"changed selected record\"}\n",
    )
    .unwrap();
    let changed = now.read(&source).unwrap().unwrap();
    assert_ne!(changed.binding.revision, local.material.binding.revision);
    assert_eq!(changed.binding.source, source);
    fs::write(
        path.parent().unwrap().join(".no-agent-retrieval"),
        "current native withdrawal",
    )
    .unwrap();
    let native = native_withdrawal(&ctrl, &root, &source, false);
    let denied = now.read(&source).unwrap_err();
    same_native_failure(&denied, &native);
    assert_eq!(fs::read_to_string(&path).unwrap(), changed.body);
}

#[test]
#[ignore = "explicit native roster capacity qualification: pinned ctrl and real ripgrep"]
fn selected_now_roster_is_bounded_without_capping_unselected_world_sources() {
    use aikit_adapters::now_field::{
        default_runner, NowFieldScope, NowFieldSourcePoolProvider, ScopeInclude, MAX_ROSTER_FILES,
    };
    use std::sync::Arc;
    for selected_overflow in [false, true] {
        let ctrl = owner();
        let owned = scratch();
        let root = fs::canonicalize(owned.path()).unwrap();
        action(&ctrl, &root, "central.init", json!({}));
        let selected_count = if selected_overflow {
            MAX_ROSTER_FILES + 1
        } else {
            1
        };
        let mut relations = Vec::new();
        for index in 0..selected_count {
            let relative = format!("Control/agents/now/clearings/actual-{index:04}/current.json");
            let path = root.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "{\"Actualrosterneedle\":\"current owned record\"}\n").unwrap();
            relations.push(json!({
                "ref":format!("central:source:control:root:{relative}"),"path":relative,
                "roles":["agent-continuation-source"],"provenance":"agent-maintained",
                "standing":"current-development-state","treatment":"retain-native-in-place",
            }));
        }
        if !selected_overflow {
            for index in 0..MAX_ROSTER_FILES + 1 {
                let path = root.join(format!("Control/user/unselected-{index:04}.md"));
                fs::write(path, "retained actual unselected source\n").unwrap();
            }
        }
        fs::create_dir_all(root.join("Control/relations")).unwrap();
        fs::write(root.join("Control/relations/source-relations.json"), serde_json::to_vec(&json!({
            "schema":"central.control.ground-relations/v1","project_id":"control:root","relations":relations,
        })).unwrap()).unwrap();
        let actual = action(
            &ctrl,
            &root,
            "central.file-map.inspect",
            json!({"resources":true,"federated":true}),
        );
        assert!(
            actual["resources"].as_array().unwrap().len() > MAX_ROSTER_FILES,
            "the actual native owner must supply a World larger than the selected NOW bound"
        );
        let relay = root.join("actual-rg-observer");
        fs::write(&relay, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$0.args\"\nIFS= read -r native_rg < \"$0.binary\"\nexec \"$native_rg\" \"$@\"\n").unwrap();
        fs::write(
            root.join("actual-rg-observer.binary"),
            format!("{}\n", aikit_adapters::ripgrep::executable().display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&relay, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let central = Arc::new(
            CentralFileMapProvider::connect(
                SystemRunner::new().with_timeout(Duration::from_secs(30)),
                &ctrl,
                &root,
                None,
            )
            .unwrap(),
        );
        let scope = NowFieldScope {
            central_root: root.clone(),
            includes: vec![ScopeInclude {
                glob: "Control/agents/now/clearings/**/*.json".into(),
                family: "now",
            }],
            excludes: vec![],
            pruned: vec![],
        };
        let now = NowFieldSourcePoolProvider::connect(default_runner(&root), &relay, scope)
            .unwrap()
            .with_native_owner(central);
        assert!(
            now.capabilities().fulltext,
            "the observer delegates to actual ripgrep"
        );
        fs::write(root.join("actual-rg-observer.args"), "").unwrap();
        let query = now.search(
            "Actualrosterneedle",
            aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext,
            &[],
            8,
        );
        if selected_overflow {
            let error = query.unwrap_err();
            assert_eq!(error.code(), "now_field.source_roster_budget");
            assert_eq!(error.details()["capacity"], "selected_native_sources");
            assert_eq!(error.details()["remaining_roster"], "unknown");
            assert_eq!(
                fs::read_to_string(root.join("actual-rg-observer.args")).unwrap(),
                "",
                "capacity refusal precedes every body query, not final hit suppression"
            );
        } else {
            let hits = query.unwrap();
            assert_eq!(hits.len(), 1);
            let current = now.read(&hits[0].source).unwrap().unwrap();
            assert_eq!(hits[0].revision.as_ref(), Some(&current.binding.revision));
            assert!(current.body.contains("Actualrosterneedle"));
            assert!(fs::read_to_string(root.join("actual-rg-observer.args"))
                .unwrap()
                .contains("--json"));
        }
        assert_eq!(
            fs::read_to_string(root.join("Control/agents/now/clearings/actual-0000/current.json"))
                .unwrap(),
            "{\"Actualrosterneedle\":\"current owned record\"}\n"
        );
    }
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
fn native_now_operation_budget_preserves_actual_transport_failure_without_fallback() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider};
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let relay = root.join("native-delayed-owner");
    fs::write(&relay, "#!/bin/sh\nif [ -f \"$0.delay\" ]; then /bin/sleep 5; fi\nIFS= read -r native_ctrl < \"$0.owner\"\nexec \"$native_ctrl\" \"$@\"\n").unwrap();
    fs::write(
        root.join("native-delayed-owner.owner"),
        format!("{}\n", ctrl.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&relay, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let central = Arc::new(
        CentralFileMapProvider::connect(
            SystemRunner::new().with_timeout(Duration::from_secs(2)),
            &relay,
            &root,
            None,
        )
        .unwrap(),
    );
    let note = root.join("Control/user/day/2026-10-02/day.md");
    fs::create_dir_all(note.parent().unwrap()).unwrap();
    fs::write(&note, "Existing usable Control budgetneedle\n").unwrap();
    let now = NowFieldSourcePoolProvider::connect(
        default_runner(&root),
        aikit_adapters::ripgrep::executable(),
        NowFieldScope::standard(&root),
    )
    .unwrap()
    .with_native_owner(central);
    fs::write(
        root.join("native-delayed-owner.delay"),
        "actual OS delay enabled after successful connection\n",
    )
    .unwrap();
    let started = std::time::Instant::now();
    let error = now
        .search(
            "budgetneedle",
            aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext,
            &[],
            8,
        )
        .unwrap_err();
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the real five-second child must not consume an unbounded query"
    );
    assert_eq!(error.code(), "central.file_map_unavailable");
    let transport: Value = serde_json::from_str(&error.details()["transport_error"]).unwrap();
    assert_eq!(transport["code"], "mux.command_timeout");
    assert_eq!(
        error.details()["owner_operation"],
        "central.file-map.inspect"
    );
    assert_eq!(
        fs::read_to_string(&note).unwrap(),
        "Existing usable Control budgetneedle\n"
    );
}

#[test]
#[cfg(unix)]
fn non_utf8_coordinates_cannot_address_a_real_replacement_named_owner_or_world() {
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    let invalid_executable = root.join(std::ffi::OsString::from_vec(b"owner-\xff".to_vec()));
    let replacement_executable = PathBuf::from(invalid_executable.to_string_lossy().into_owned());
    fs::write(
        &replacement_executable,
        "#!/bin/sh\nprintf invoked > \"$0.invoked\"\n",
    )
    .unwrap();
    fs::set_permissions(&replacement_executable, fs::Permissions::from_mode(0o700)).unwrap();
    let runner = SystemRunner::new().with_timeout(Duration::from_secs(2));
    let error = call(&runner, &invalid_executable, &root, "inspect", &json!({})).unwrap_err();
    assert_eq!(error.code(), "central.file_map_coordinate_invalid");
    assert_eq!(error.details()["coordinate"], "executable");
    let invoked = PathBuf::from(format!(
        "{}.invoked",
        replacement_executable.to_str().unwrap()
    ));
    assert!(
        !invoked.exists(),
        "lossy executable conversion must not launch the other real file"
    );
    let invalid_root = root.join(std::ffi::OsString::from_vec(b"world-\xff".to_vec()));
    let replacement_root = PathBuf::from(invalid_root.to_string_lossy().into_owned());
    fs::create_dir(&replacement_root).unwrap();
    fs::write(
        replacement_root.join("retained.txt"),
        "a distinct real World coordinate",
    )
    .unwrap();
    let error = call(
        &runner,
        &replacement_executable,
        &invalid_root,
        "inspect",
        &json!({}),
    )
    .unwrap_err();
    assert_eq!(error.code(), "central.file_map_coordinate_invalid");
    assert_eq!(error.details()["coordinate"], "root");
    assert!(
        !invoked.exists(),
        "lossy World conversion must not contact another real owner"
    );
    assert_eq!(
        fs::read_to_string(replacement_root.join("retained.txt")).unwrap(),
        "a distinct real World coordinate"
    );
}

// Observe an actual rg receipt and change the owned file afterward. The native
// search output remains byte-for-byte the real command's; no owner is simulated.
struct RgReceiptInterleaving {
    runner: SystemRunner,
    source_path: PathBuf,
    receipt: Mutex<Option<Output>>,
}
impl RgReceiptInterleaving {
    fn after_query(&self, argv: &[String], output: Output) -> aikit_core::Result<Output> {
        if argv.iter().any(|argument| argument == "--json") && !output.stdout.is_empty() {
            let mut receipt = self.receipt.lock().unwrap();
            if receipt.is_none() {
                *receipt = Some(output.clone());
                fs::write(
                    &self.source_path,
                    "Replacement record without the original query match\n",
                )
                .expect("actual owned Source interleaving");
            }
        }
        Ok(output)
    }
}
impl CommandRunner for RgReceiptInterleaving {
    fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
        self.after_query(argv, self.runner.run(argv)?)
    }
    fn configured_timeout(&self) -> Option<Duration> {
        self.runner.configured_timeout()
    }
    fn run_with_timeout(&self, argv: &[String], timeout: Duration) -> aikit_core::Result<Output> {
        self.after_query(argv, self.runner.run_with_timeout(argv, timeout)?)
    }
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
fn native_now_does_not_stamp_a_later_revision_on_an_original_rg_match() {
    use aikit_adapters::now_field::{
        default_runner, NowFieldScope, NowFieldSourcePoolProvider, ScopeInclude,
    };
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let path = root.join("Control/user/day/2026-10-02/day.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "Actual original interleaved-queryneedle record\n").unwrap();
    let central = Arc::new(
        CentralFileMapProvider::connect(
            SystemRunner::new().with_timeout(Duration::from_secs(30)),
            &ctrl,
            &root,
            None,
        )
        .unwrap(),
    );
    let located = call(
        &SystemRunner::new(),
        &ctrl,
        &root,
        "locate",
        &json!({"path":path,"content":false}),
    )
    .unwrap();
    let source = SourceRef::parse(located["source"]["ref"].as_str().unwrap()).unwrap();
    let original = central
        .read_for(&source, RetrievalTarget::LocalAgent)
        .unwrap()
        .unwrap();
    let transport = RgReceiptInterleaving {
        runner: default_runner(&root),
        source_path: path.clone(),
        receipt: Mutex::new(None),
    };
    let scope = NowFieldScope {
        central_root: root,
        includes: vec![ScopeInclude {
            glob: "Control/user/day/*/day.md".into(),
            family: "day",
        }],
        excludes: vec![],
        pruned: vec![],
    };
    let now = NowFieldSourcePoolProvider::connect(
        &transport,
        aikit_adapters::ripgrep::executable(),
        scope,
    )
    .unwrap()
    .with_native_owner(Arc::clone(&central));
    let error = now
        .search(
            "interleaved-queryneedle",
            aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext,
            &[],
            8,
        )
        .unwrap_err();
    assert_eq!(error.code(), "now_field.source_search_basis_conflict");
    let receipt = transport.receipt.lock().unwrap();
    let receipt = receipt
        .as_ref()
        .expect("the original real rg receipt must be retained");
    assert!(receipt.ok());
    assert!(receipt.stdout.contains("interleaved-queryneedle"));
    let current = central
        .read_for(&source, RetrievalTarget::LocalAgent)
        .unwrap()
        .unwrap();
    assert_eq!(
        current.material.binding.source,
        original.material.binding.source
    );
    assert_ne!(
        current.material.binding.revision,
        original.material.binding.revision
    );
    assert_eq!(
        current.material.body,
        "Replacement record without the original query match\n"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), current.material.body);
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
fn native_work_exact_ids_collisions_current_declaration_and_historical_copies_remain_distinct() {
    use aikit_adapters::work_repos::{
        decode_work_file_source_ref, discover_native_work_projects, work_file_source_ref,
        NativeWorkProjectEntry, NativeWorkRepoProject, WorkReposSourcePoolProvider,
    };
    use aikit_core::knowledge_source_pool::{NativeSourcePoolProvider, SourceSearchMode};
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    for (name, id, member) in [("left", "a:b", "c.md"), ("right", "a", "b:c.md")] {
        fs::create_dir_all(root.join("Work").join(name)).unwrap();
        action(
            &ctrl,
            &root,
            "projectcentral.init",
            json!({"project":name,"project_id":id}),
        );
        fs::write(
            root.join("Work").join(name).join(member),
            format!("# {name}\nWorkidentityneedle.\n"),
        )
        .unwrap();
    }
    let attach = || {
        let projects = discover_native_work_projects(&root)
            .unwrap()
            .into_iter()
            .map(|entry| match entry {
                NativeWorkProjectEntry::Project(project) => project,
                NativeWorkProjectEntry::Absence { name, error } => panic!("{name}: {error}"),
            })
            .collect();
        WorkReposSourcePoolProvider::connect_native(
            SystemRunner::new().with_timeout(Duration::from_secs(30)),
            aikit_adapters::ripgrep::executable(),
            projects,
        )
        .unwrap()
    };
    let work = attach();
    assert!(
        work.status().available,
        "real rg is required for this selected gate"
    );
    let left = work_file_source_ref("a:b", Path::new("c.md")).unwrap();
    let right = work_file_source_ref("a", Path::new("b:c.md")).unwrap();
    assert_ne!(left, right);
    let hits = work
        .search("Workidentityneedle", SourceSearchMode::Fulltext, &[], 8)
        .unwrap();
    assert_eq!(hits.len(), 2, "{hits:?}");
    for (source, id, member, body) in [
        (&left, "a:b", "c.md", "# left\nWorkidentityneedle.\n"),
        (&right, "a", "b:c.md", "# right\nWorkidentityneedle.\n"),
    ] {
        let address = decode_work_file_source_ref(source).unwrap().unwrap();
        assert_eq!(address.project_id, id);
        assert_eq!(address.member, Path::new(member));
        let material = work.read(source).unwrap().unwrap();
        assert_eq!(material.body, body);
        let hit = hits.iter().find(|hit| &hit.source == source).unwrap();
        assert_eq!(hit.revision.as_ref(), Some(&material.binding.revision));
    }
    let native = aikit_adapters::ProjectCentralFilesystemBinding::inspect(
        &root.join("Work/left"),
        Some(&root),
    )
    .unwrap();
    assert_eq!(
        native.semantic.native_project_root.as_str(),
        "source:project:a:b:root"
    );
    assert!(
        work.read(&native.semantic.native_project_root)
            .unwrap()
            .is_none(),
        "native root remains the native owner's distinct role"
    );
    let legacy = SourceRef::parse("source:project:a:b:c.md").unwrap();
    assert!(work.read(&legacy).unwrap().is_none());
    let mut retained = work.read(&left).unwrap().unwrap();
    retained.binding.source = legacy.clone();
    retained
        .binding
        .metadata
        .insert("owner_read_required".into(), json!(false));
    let retained_body = retained.body.clone();
    let historical = vec![retained];
    let mut index = NativeSourcePoolProvider::default();
    index.rebuild(&historical).unwrap();
    let app = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&index, &historical)
        .with_source_pool(&work, &[]);
    let old = KnowledgeAddress::Source(legacy.clone());
    assert_eq!(
        app.read(&old).unwrap_err().code(),
        "knowledge.source_origin_unavailable"
    );
    assert_eq!(
        app.explain(&old).unwrap_err().code(),
        "knowledge.source_origin_unavailable"
    );
    assert_eq!(
        app.route(None, std::slice::from_ref(&old))
            .unwrap_err()
            .code(),
        "knowledge.source_origin_unavailable"
    );
    assert_eq!(
        historical[0].body, retained_body,
        "history is retained, never reattributed"
    );
    let restart = attach();
    assert_eq!(
        restart.read(&left).unwrap().unwrap().body,
        "# left\nWorkidentityneedle.\n"
    );
    assert!(restart.read(&legacy).unwrap().is_none());
    let manifest = root.join("Work/left/ProjectCentral/project.json");
    let original = fs::read(&manifest).unwrap();
    let same = root.join("Work/left/ProjectCentral/same.json");
    fs::write(&same, &original).unwrap();
    fs::rename(&same, &manifest).unwrap();
    assert!(
        work.read(&left).unwrap().is_some(),
        "native identical-byte replacement is compatible"
    );
    let mut changed: Value = serde_json::from_slice(&original).unwrap();
    changed["native-extension"] = json!({"retained":true});
    fs::write(&manifest, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert_eq!(
        work.read(&left).unwrap_err().code(),
        "work_repos.native_declaration_changed"
    );
    let fresh = attach();
    assert!(fresh.read(&left).unwrap().is_some());
    fs::write(
        root.join("Work/left/ProjectCentral/.no-agent-retrieval"),
        "withdraw\n",
    )
    .unwrap();
    assert_eq!(
        fresh.read(&left).unwrap_err().code(),
        "work_repos.source_unauthorised"
    );
    let survivor = WorkReposSourcePoolProvider::connect_native(
        SystemRunner::probe(),
        "work-test-deliberately-unavailable-rg",
        vec![
            NativeWorkRepoProject::inspect(&root.join("Work/right"), "right", Some(&root)).unwrap(),
        ],
    )
    .unwrap();
    assert!(
        survivor.read(&legacy).unwrap().is_none(),
        "a current survivor cannot certify an old issuing tuple"
    );
    assert_eq!(
        survivor.read(&right).unwrap().unwrap().body,
        "# right\nWorkidentityneedle.\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("Work/left/c.md")).unwrap(),
        "# left\nWorkidentityneedle.\n"
    );
}
