//! Actual Central file-map and SourcePool boundary exercises. Explicit native
//! tests require the qualification job's built/pinned owner binary; no fake
//! owner, generated response, guessed SourceRef or unavailable-as-green skip.
use aikit_adapters::central_file_map::{call, CentralFileMapProvider};
use aikit_adapters::runner::{CommandRunner, Output, SystemRunner};
use aikit_core::context_source::{ContextSourcePrivacy, RetrievalTarget};
use aikit_core::knowledge_source_pool::{SourceOriginKind, SourcePoolProvider};
use aikit_core::knowledge_navigation::{KnowledgeAddress, KnowledgeApplication};
use aikit_core::familiarity::FamiliarityContext;
use aikit_core::resource::SourceRef;
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, sync::Mutex, time::Duration};

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
        if argv.iter().any(|argument| argument == "central.file-map.search") {
            let mut receipt = self.receipt.lock().unwrap();
            if receipt.is_none() {
                *receipt = Some(serde_json::from_str(&output.stdout)
                    .expect("the real owner must return a native JSON search receipt"));
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
    tempfile::Builder::new().prefix("native-file-map-").tempdir_in(root).unwrap()
}

fn owner() -> PathBuf {
    let path = PathBuf::from(std::env::var_os("AIKIT_CENTRAL_REAL_BIN")
        .expect("explicit native qualification requires AIKIT_CENTRAL_REAL_BIN for the built/pinned ctrl"));
    assert!(path.is_file(), "actual owner binary is required: {}", path.display());
    path
}

fn action_envelope(ctrl: &Path, root: &Path, name: &str, input: Value) -> (Output, Value) {
    let mut command = std::process::Command::new(ctrl);
    command.args(["--json", "--root"]).arg(root)
        .args(["action", "run", name]).arg(input.to_string());
    let output = SystemRunner::new().with_timeout(Duration::from_secs(30))
        .with_strict_utf8()
        .capture_command(&mut command).unwrap();
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
    assert!(output.ok() && value["ok"] == true, "{value}; stderr={}", output.stderr);
    if let Some(operation) = name.strip_prefix("central.file-map.") {
        return file_map_result(&value["data"], operation);
    }
    value["data"].clone()
}

fn native_withdrawal(ctrl: &Path, root: &Path, source: &SourceRef) -> Value {
    let (output, native) = action_envelope(ctrl, root, "central.file-map.resolve", json!({
        "project":null,"federated":true,"resources":false,"source_ref":source,"binding_only":true,
    }));
    assert!(!output.ok() && native["ok"] == false, "the actual withdrawal must refuse: {native}; stderr={}", output.stderr);
    assert!(native["error"]["code"].as_str().is_some_and(|code| !code.is_empty()), "{native}");
    assert!(native["data"].is_null(), "withdrawal may not return source body: {native}");
    native
}

fn same_native_failure(error: &aikit_core::AikitError, native: &Value) {
    assert_eq!(error.code(), native["error"]["code"].as_str().unwrap());
    assert_eq!(serde_json::from_str::<Value>(&error.details()["native_result"]).unwrap(), *native);
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
    assert!(provider.descriptors().is_empty(), "no copied owner roster at attachment");
    assert!(provider.rebuild(&[]).is_err(), "persistent owner map is never rebuilt by this consumer");
    assert!(!provider.capabilities().fulltext, "a fresh uninitialised owner map is honestly unavailable");
    let held = provider.read(&source).unwrap().unwrap();
    assert_eq!(held.body, "# Initial owner source\n");
    assert_eq!(held.binding.media_type, "text/markdown");
    assert_eq!(held.binding.source.as_str(), reading["source"]["ref"].as_str().unwrap());
    assert_eq!(held.binding.revision.as_str(), reading["revision"].as_str().unwrap());
    let origin = held.binding.source_origin().unwrap().unwrap();
    match origin.origin {
        SourceOriginKind::NativeSource { world_ref, source: native, observed_binding } => {
            assert_eq!(world_ref, reading["world_ref"].as_str().unwrap());
            assert_eq!(native.source, source);
            assert_eq!(native.revision.unwrap(), held.binding.revision);
            assert!(native.locator.is_none());
            assert_eq!(observed_binding.roles, serde_json::from_value::<Vec<String>>(reading["source"]["roles"].clone()).unwrap());
            assert_eq!(observed_binding.provenance, reading["source"]["provenance"].as_str().unwrap());
            assert_eq!(observed_binding.standing, reading["source"]["standing"].as_str().unwrap());
            assert_eq!(observed_binding.treatment, reading["source"]["treatment"].as_str().unwrap());
        }
        other => panic!("actual native source cannot be a standalone declaration: {other:?}"),
    }
    let local = provider.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    assert_eq!(local.privacy, ContextSourcePrivacy::default());
    assert_eq!(local.material, held);
    let human = provider.read_for(&source, RetrievalTarget::Human).unwrap().unwrap();
    assert_eq!(human.material, held);
    assert_eq!(provider.read_for(&source, RetrievalTarget::ExternalProvider).unwrap_err().code(), "knowledge.source_target_withheld");
    let address = KnowledgeAddress::Source(source.clone());
    for target in [RetrievalTarget::Human, RetrievalTarget::LocalAgent] {
        let unheld = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &[]).with_retrieval_target(target);
        let actual = unheld.read(&address).unwrap();
        assert_eq!(actual.content.as_deref(), Some(held.body.as_str()));
        assert_eq!(actual.revision.as_deref(), Some(held.binding.revision.as_str()));
        assert!(unheld.explain(&address).unwrap().detail.unwrap().get("locator").is_none());
        assert_eq!(unheld.route(None, std::slice::from_ref(&address)).unwrap().steps[0].revision.as_deref(), Some(held.binding.revision.as_str()));
    }
    let unheld_external = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&provider, &[]).with_retrieval_target(RetrievalTarget::ExternalProvider);
    assert_eq!(unheld_external.read(&address).unwrap_err().code(), "knowledge.source_target_withheld");
    assert_eq!(unheld_external.explain(&address).unwrap_err().code(), "knowledge.source_target_withheld");
    assert_eq!(unheld_external.route(None, std::slice::from_ref(&address)).unwrap_err().code(), "knowledge.source_target_withheld");
    let materials = vec![held.clone()];
    let external = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&provider, &materials).with_retrieval_target(RetrievalTarget::ExternalProvider);
    assert_eq!(external.read(&address).unwrap_err().code(), "knowledge.source_target_withheld");
    assert_eq!(external.explain(&address).unwrap_err().code(), "knowledge.source_target_withheld");
    assert_eq!(external.route(None, std::slice::from_ref(&address)).unwrap_err().code(), "knowledge.source_target_withheld");
    let bound = KnowledgeApplication::new(FamiliarityContext::default()).with_source_pool(&provider, &materials);
    let explanation = bound.explain(&KnowledgeAddress::Source(source.clone())).unwrap().detail.unwrap();
    assert!(explanation.get("locator").is_none());
    assert!(explanation["metadata"].get("central").is_none());
    assert!(!explanation.to_string().contains(root.to_str().unwrap()));
    assert_eq!(bound.read(&KnowledgeAddress::Source(source.clone())).unwrap().content.as_deref(), Some(held.body.as_str()));
    fs::write(&note, "# Current owner source\n").unwrap();
    let stale = bound.read(&KnowledgeAddress::Source(source.clone())).unwrap_err();
    assert_eq!(stale.code(), "knowledge.source_origin_revision_conflict");
    assert_eq!(bound.explain(&KnowledgeAddress::Source(source.clone())).unwrap_err().code(), stale.code());
    assert_eq!(bound.route(None, &[KnowledgeAddress::Source(source.clone())]).unwrap_err().code(), stale.code());
    let current = provider.read(&source).unwrap().unwrap();
    assert_eq!(current.body, "# Current owner source\n");
    assert_ne!(current.binding.revision, held.binding.revision);
    let late = root.join("late-source.md");
    fs::write(&late, "# Added after attachment\n").unwrap();
    let basis = call(&SystemRunner::new(), &ctrl, root, "inspect", &json!({"resources":false})).unwrap();
    let registered = action(&ctrl, root, "central.file-map.register", json!({
        "path":"late-source.md", "expected_revision":basis["revision"],
    }));
    let late_ref = SourceRef::parse(registered["source_ref"].as_str().unwrap()).unwrap();
    assert_eq!(provider.read(&late_ref).unwrap().unwrap().body, "# Added after attachment\n");
    fs::write(root.join("Control/user/.no-agent-retrieval"), "withdrawal\n").unwrap();
    let native = native_withdrawal(&ctrl, root, &source);
    let denied = bound.read(&KnowledgeAddress::Source(source.clone())).unwrap_err();
    same_native_failure(&denied, &native);
    for target in [RetrievalTarget::Human, RetrievalTarget::LocalAgent] {
        let unheld = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &[]).with_retrieval_target(target);
        assert_eq!(unheld.read(&address).unwrap_err().details()["native_result"], denied.details()["native_result"]);
        assert_eq!(unheld.explain(&address).unwrap_err().details()["native_result"], denied.details()["native_result"]);
        assert_eq!(unheld.route(None, std::slice::from_ref(&address)).unwrap_err().details()["native_result"], denied.details()["native_result"]);
    }
    let denied_explanation = bound.explain(&KnowledgeAddress::Source(source.clone())).unwrap_err();
    let denied_route = bound.route(None, &[KnowledgeAddress::Source(source.clone())]).unwrap_err();
    assert_eq!(denied_route.details()["native_result"], denied.details()["native_result"]);
    assert_eq!(denied_explanation.details()["native_result"], denied.details()["native_result"]);
    let envelope: Value = serde_json::from_str(&denied.details()["native_result"]).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["action"], "central.file-map.resolve");
    assert_eq!(denied.details()["native_error_code"], envelope["error"]["code"].as_str().unwrap());
    assert_eq!(serde_json::from_str::<Value>(&denied.details()["native_error"]).unwrap(), envelope["error"]);
    assert_eq!(fs::read_to_string(&note).unwrap(), "# Current owner source\n");
    assert_eq!(provider.read(&late_ref).unwrap().unwrap().body, "# Added after attachment\n");
}

#[test]
fn absent_owner_binary_is_genuine_unavailability_not_a_capability() {
    let owned = scratch();
    let missing = owned.path().join("no-owner-executable");
    assert!(!missing.exists());
    assert!(CentralFileMapProvider::connect(SystemRunner::new(), missing, owned.path(), None).is_err());
}

#[test]
fn disposable_provider_refuses_central_owned_database() {
    use aikit_adapters::bkmr::BkmrSourcePoolProvider;
    let owned = scratch();
    let database = owned.path().join(".central/bkmr/map.db");
    fs::create_dir_all(database.parent().unwrap()).unwrap();
    fs::write(&database, b"persistent owner sentinel").unwrap();
    let mut provider = BkmrSourcePoolProvider::with_binary(SystemRunner::new(), "missing-bkmr", &database, false);
    assert!(provider.rebuild(&[]).is_err());
    assert_eq!(fs::read(&database).unwrap(), b"persistent owner sentinel");
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and CENTRAL_BKMR_BIN"]
fn actual_indexed_native_search_and_retained_routes_withhold_withdrawn_or_changed_origins() {
    let ctrl = owner();
    let bkmr = std::env::var_os("CENTRAL_BKMR_BIN")
        .expect("explicit native search qualification requires the real pinned bkmr binary");
    assert!(Path::new(&bkmr).is_file(), "the actual search provider is required");
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let note = root.join("Control/user/origin-search-needle.md");
    fs::write(&note, "# Originneedle qualification\nActual original body.\n").unwrap();
    let actual = action(&ctrl, &root, "central.file-map.locate", json!({"path": note}));
    let source = SourceRef::parse(actual["source"]["ref"].as_str().unwrap()).unwrap();
    // This is the real owner refresh and provider index, with embeddings
    // explicitly absent. No generated row or owner reply stands in for it.
    action(&ctrl, &root, "central.file-map.refresh", json!({"embeddings":false}));
    let provider = CentralFileMapProvider::connect(SystemRunner::new().with_timeout(Duration::from_secs(30)), &ctrl, &root, None).unwrap();
    assert!(provider.capabilities().fulltext, "the actual index must be available for this selected qualification");
    let materials = vec![provider.read(&source).unwrap().unwrap()];
    let address = KnowledgeAddress::Source(source.clone());
    let bound = KnowledgeApplication::new(FamiliarityContext::default()).with_source_pool(&provider, &materials);
    assert!(bound.search("Originneedle", 8).hits.iter().any(|hit| hit.address == address));
    let unheld = KnowledgeApplication::new(FamiliarityContext::default()).with_source_pool(&provider, &[]);
    assert!(unheld.search("Originneedle", 8).hits.iter().any(|hit| hit.address == address));
    for held in [&materials[..], &[][..]] {
        let external = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, held).with_retrieval_target(RetrievalTarget::ExternalProvider);
        let withheld = external.search("Originneedle", 8);
        assert!(withheld.hits.iter().all(|hit| hit.address != address));
        let absence = withheld.absences.join("\n");
        assert!(!absence.contains(source.as_str()) && !absence.contains("Originneedle"));
        assert!(!absence.contains(note.to_str().unwrap()));
    }
    assert_eq!(bound.route(None, std::slice::from_ref(&address)).unwrap().steps[0].revision.as_deref(), Some(materials[0].binding.revision.as_str()));
    fs::write(&note, "# Originneedle qualification\nActual changed body.\n").unwrap();
    assert!(bound.search("Originneedle", 8).hits.iter().all(|hit| hit.address != address));
    assert_eq!(bound.route(None, std::slice::from_ref(&address)).unwrap_err().code(), "knowledge.source_origin_revision_conflict");
    fs::write(root.join("Control/user/.no-agent-retrieval"), "withdrawal\n").unwrap();
    let native = native_withdrawal(&ctrl, &root, &source);
    let denied = bound.read(&address).unwrap_err();
    same_native_failure(&denied, &native);
    assert!(bound.search("Originneedle", 8).hits.iter().all(|hit| hit.address != address));
    assert_eq!(bound.route(None, std::slice::from_ref(&address)).unwrap_err().details()["native_result"], denied.details()["native_result"]);
    assert!(unheld.search("Originneedle", 8).hits.iter().all(|hit| hit.address != address));
    assert_eq!(unheld.read(&address).unwrap_err().details()["native_result"], denied.details()["native_result"]);
    let absences = bound.search("Originneedle", 8).absences.join("\n");
    assert!(!absences.contains(source.as_str()));
    assert!(!absences.contains("Originneedle"));
    assert!(!absences.contains(note.to_str().unwrap()));
    assert_eq!(fs::read_to_string(&note).unwrap(), "# Originneedle qualification\nActual changed body.\n");
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
        let actual = action(&ctrl, &root, "central.file-map.locate", json!({"path": note}));
        let source = SourceRef::parse(actual["source"]["ref"].as_str().unwrap()).unwrap();
        action(&ctrl, &root, "central.file-map.refresh", json!({"embeddings":false}));
        let runner = SearchReceiptInterleaving {
            runner: SystemRunner::new().with_timeout(Duration::from_secs(30)),
            source_path: note.clone(),
            replacement: changed.then_some("# Current replacement\nThe former query is absent.\n"),
            receipt: Mutex::new(None),
        };
        let provider = CentralFileMapProvider::connect(&runner, &ctrl, &root, None).unwrap();
        assert!(provider.capabilities().fulltext, "the actual owner index is required");
        let app = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &[]);
        let result = app.search("Receiptneedle", 8);
        let receipt = runner.receipt.lock().unwrap();
        let receipt = receipt.as_ref().expect("actual completed native search receipt");
        assert_eq!(receipt["ok"], true, "{receipt}");
        let native_hit = receipt["data"]["result"]["hits"].as_array().unwrap().iter()
            .find(|hit| hit["source"]["ref"] == source.as_str())
            .expect("the original owner search must genuinely match the selected file");
        assert_eq!(native_hit["revision"], actual["revision"]);
        let current = provider.read(&source).unwrap().unwrap();
        let address = KnowledgeAddress::Source(source.clone());
        if changed {
            assert_ne!(current.binding.revision.as_str(), native_hit["revision"].as_str().unwrap());
            assert!(!current.body.contains("Receiptneedle"));
            assert!(!result.hits.iter().any(|hit| hit.address == address), "old match must be withheld: {result:?}");
            assert!(result.absences.iter().any(|absence| absence.contains("knowledge.source_origin_revision_conflict")));
            assert!(result.absences.iter().all(|absence| !absence.contains(note.to_str().unwrap()) && !absence.contains(source.as_str())));
        } else {
            assert_eq!(current.body, original);
            assert_eq!(current.binding.revision.as_str(), native_hit["revision"].as_str().unwrap());
            let returned = result.hits.iter().find(|hit| hit.address == address).expect("unchanged real native match");
            assert_eq!(returned.label, native_hit["title"].as_str().unwrap());
            assert_eq!(returned.snippet, native_hit["snippet"].as_str().unwrap());
            assert_eq!(returned.score, native_hit["score"].as_f64().unwrap_or(0.5));
        }
    }
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
fn nonowners_decline_before_current_central_and_independent_work_owners() {
    use aikit_adapters::work_repos::{discover_native_work_projects, work_file_source_ref, NativeWorkProjectEntry, WorkReposSourcePoolProvider};
    use aikit_core::knowledge_source_pool::NativeSourcePoolProvider;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let project = root.join("Work/composition");
    fs::create_dir_all(&project).unwrap();
    action(&ctrl, &root, "projectcentral.init", json!({"project":"composition", "project_id":"composition/project"}));
    let note = project.join("README.md");
    fs::write(&note, "# Independent live Work source\nCompositionneedle.\n").unwrap();
    let projects = discover_native_work_projects(&root).unwrap().into_iter().map(|entry| match entry {
        NativeWorkProjectEntry::Project(project) => project,
        NativeWorkProjectEntry::Absence { name, error } => panic!("actual Project discovery failed for {name}: {error}"),
    }).collect::<Vec<_>>();
    assert_eq!(projects.len(), 1);
    let source = work_file_source_ref(&projects[0].project().project_id,Path::new("README.md")).unwrap();
    let work = WorkReposSourcePoolProvider::connect_native(
        SystemRunner::new().with_timeout(Duration::from_secs(30)),
        aikit_adapters::ripgrep::executable(), projects,
    ).unwrap();
    assert!(work.capabilities().fulltext, "real ripgrep is required for the selected gate");
    let central = CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)), &ctrl, &root, None,
    ).unwrap();
    let index = NativeSourcePoolProvider::default();
    let app = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&index, &[]).with_source_pool(&central, &[]).with_source_pool(&work, &[]);
    assert_eq!(app.read(&KnowledgeAddress::Source(source.clone())).unwrap().content.as_deref(), Some(fs::read_to_string(&note).unwrap().as_str()));
    let native_matches = work.search("Compositionneedle", aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext, &[], 8).unwrap();
    let actual_match = native_matches.iter().find(|hit| hit.source == source).expect("real native Work query");
    assert_eq!(actual_match.revision.as_ref(), Some(&work.read(&source).unwrap().unwrap().binding.revision));
    assert!(actual_match.snippet.contains("Compositionneedle"));
    assert!(app.search("Compositionneedle", 8).hits.iter()
        .any(|hit| hit.address == KnowledgeAddress::Source(source.clone())));
    let native_path = root.join("Control/user/native-composition.md");
    fs::write(&native_path, "# Actual Central owner\n").unwrap();
    let actual = action(&ctrl, &root, "central.file-map.locate", json!({"path":native_path}));
    let native_ref = SourceRef::parse(actual["source"]["ref"].as_str().unwrap()).unwrap();
    assert_eq!(app.read(&KnowledgeAddress::Source(native_ref.clone())).unwrap().content.as_deref(), Some("# Actual Central owner\n"));
    let external = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&index, &[]).with_source_pool(&central, &[]).with_source_pool(&work, &[])
        .with_retrieval_target(RetrievalTarget::ExternalProvider);
    assert_eq!(external.read(&KnowledgeAddress::Source(source)).unwrap_err().code(), "knowledge.source_target_withheld");
    assert_eq!(external.read(&KnowledgeAddress::Source(native_ref.clone())).unwrap_err().code(), "knowledge.source_target_withheld");
    fs::write(root.join("Control/user/.no-agent-retrieval"), "withdrawal\n").unwrap();
    let native = native_withdrawal(&ctrl, &root, &native_ref);
    let denied = app.read(&KnowledgeAddress::Source(native_ref)).unwrap_err();
    same_native_failure(&denied, &native);
    assert_eq!(fs::read_to_string(native_path).unwrap(), "# Actual Central owner\n");
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
fn attached_now_preserves_registered_project_identity_current_payload_and_native_failure() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider, ScopeInclude};
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    fs::create_dir_all(root.join("Work/native-now")).unwrap();
    action(&ctrl, &root, "projectcentral.init", json!({"project":"native-now", "project_id":"native/now"}));
    let path = root.join("Work/native-now/ProjectCentral/now/agents/current.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{\"native-now-query\":\"current selected record\"}\n").unwrap();
    let inspected = action(&ctrl, &root, "central.file-map.inspect", json!({"project":"native-now", "resources":false}));
    let registered = action(&ctrl, &root, "central.file-map.register", json!({
        "project":"native-now", "path":"ProjectCentral/now/agents/current.json",
        "expected_revision":inspected["revision"],
    }));
    let source = SourceRef::parse(registered["source_ref"].as_str().unwrap()).unwrap();
    assert!(source.as_str().starts_with("central:source:project:native/now:"));
    let relay = root.join("native-owner-relay");
    fs::write(&relay, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$0.args\"\nIFS= read -r native_ctrl < \"$0.owner\"\nexec \"$native_ctrl\" \"$@\"\n").unwrap();
    fs::write(root.join("native-owner-relay.owner"), format!("{}\n", ctrl.display())).unwrap();
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&relay, fs::Permissions::from_mode(0o700)).unwrap(); }
    let central = Arc::new(CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)), &relay, &root, None,
    ).unwrap());
    fs::write(root.join("native-owner-relay.args"), "").unwrap();
    let scope = NowFieldScope { central_root:root.clone(),
        includes:vec![ScopeInclude { glob:"Work/native-now/ProjectCentral/now/**/*.json".into(), family:"projectcentral" }],
        excludes:vec![], pruned:vec![],
    };
    let now = NowFieldSourcePoolProvider::connect(default_runner(&root), aikit_adapters::ripgrep::executable(), scope).unwrap()
        .with_native_owner(Arc::clone(&central));
    assert!(now.descriptors().is_empty(), "native construction must not preload unselected Source bodies");
    assert_eq!(fs::read_to_string(root.join("native-owner-relay.args")).unwrap(), "",
        "the real native transport observer sees no owner body read during construction/descriptors");
    let local = now.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    let native = central.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    assert_eq!(local, native, "NOW must not substitute a Root alias or binding");
    assert_eq!(local.material.body, fs::read_to_string(&path).unwrap());
    let hits = now.search("native-now-query", aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext, &[], 8).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].source, source);
    assert_eq!(hits[0].revision.as_ref(), Some(&native.material.binding.revision));
    assert_eq!(hits[0].provider_binding.as_deref(), Some("line:1"));
    assert!(hits[0].tags.contains(&"native-now".to_string()));
    assert!(hits[0].tags.contains(&"projectcentral".to_string()));
    assert_eq!(now.read_for(&source, RetrievalTarget::ExternalProvider).unwrap_err().code(), "knowledge.source_target_withheld");
    fs::write(&path, "{\"native-now-current\":\"changed selected record\"}\n").unwrap();
    let changed = now.read(&source).unwrap().unwrap();
    assert_ne!(changed.binding.revision, local.material.binding.revision);
    assert_eq!(changed.binding.source, source);
    fs::write(path.parent().unwrap().join(".no-agent-retrieval"), "current native withdrawal").unwrap();
    let native = native_withdrawal(&ctrl, &root, &source);
    let denied = now.read(&source).unwrap_err();
    same_native_failure(&denied, &native);
    assert_eq!(fs::read_to_string(&path).unwrap(), changed.body);
}

#[test]
#[ignore = "explicit native roster capacity qualification: pinned ctrl and real ripgrep"]
fn selected_now_roster_is_bounded_without_capping_unselected_world_sources() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider, ScopeInclude, MAX_ROSTER_FILES};
    use std::sync::Arc;
    for selected_overflow in [false, true] {
        let ctrl = owner();
        let owned = scratch();
        let root = fs::canonicalize(owned.path()).unwrap();
        action(&ctrl, &root, "central.init", json!({}));
        let selected_count = if selected_overflow { MAX_ROSTER_FILES + 1 } else { 1 };
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
        let actual = action(&ctrl, &root, "central.file-map.inspect", json!({"resources":true,"federated":true}));
        assert!(actual["resources"].as_array().unwrap().len() > MAX_ROSTER_FILES,
            "the actual native owner must supply a World larger than the selected NOW bound");
        let relay = root.join("actual-rg-observer");
        fs::write(&relay, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$0.args\"\nIFS= read -r native_rg < \"$0.binary\"\nexec \"$native_rg\" \"$@\"\n").unwrap();
        fs::write(root.join("actual-rg-observer.binary"), format!("{}\n", aikit_adapters::ripgrep::executable().display())).unwrap();
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&relay, fs::Permissions::from_mode(0o700)).unwrap(); }
        let central = Arc::new(CentralFileMapProvider::connect(
            SystemRunner::new().with_timeout(Duration::from_secs(30)), &ctrl, &root, None,
        ).unwrap());
        let scope = NowFieldScope { central_root:root.clone(), includes:vec![ScopeInclude {
            glob:"Control/agents/now/clearings/**/*.json".into(),family:"now",
        }], excludes:vec![], pruned:vec![] };
        let now = NowFieldSourcePoolProvider::connect(default_runner(&root), &relay, scope).unwrap().with_native_owner(central);
        assert!(now.capabilities().fulltext, "the observer delegates to actual ripgrep");
        fs::write(root.join("actual-rg-observer.args"), "").unwrap();
        let query = now.search("Actualrosterneedle", aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext, &[], 8);
        if selected_overflow {
            let error = query.unwrap_err();
            assert_eq!(error.code(), "now_field.source_roster_budget");
            assert_eq!(error.details()["capacity"], "selected_native_sources");
            assert_eq!(error.details()["remaining_roster"], "unknown");
            assert_eq!(fs::read_to_string(root.join("actual-rg-observer.args")).unwrap(), "",
                "capacity refusal precedes every body query, not final hit suppression");
        } else {
            let hits = query.unwrap();
            assert_eq!(hits.len(), 1);
            let current = now.read(&hits[0].source).unwrap().unwrap();
            assert_eq!(hits[0].revision.as_ref(), Some(&current.binding.revision));
            assert!(current.body.contains("Actualrosterneedle"));
            assert!(fs::read_to_string(root.join("actual-rg-observer.args")).unwrap().contains("--json"));
        }
        assert_eq!(fs::read_to_string(root.join("Control/agents/now/clearings/actual-0000/current.json")).unwrap(),
            "{\"Actualrosterneedle\":\"current owned record\"}\n");
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
    fs::write(root.join("native-delayed-owner.owner"), format!("{}\n", ctrl.display())).unwrap();
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&relay, fs::Permissions::from_mode(0o700)).unwrap(); }
    let central = Arc::new(CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(2)), &relay, &root, None,
    ).unwrap());
    let note = root.join("Control/user/day/2026-10-02/day.md");
    fs::create_dir_all(note.parent().unwrap()).unwrap();
    fs::write(&note, "Existing usable Control budgetneedle\n").unwrap();
    let now = NowFieldSourcePoolProvider::connect(default_runner(&root), aikit_adapters::ripgrep::executable(), NowFieldScope::standard(&root)).unwrap()
        .with_native_owner(central);
    fs::write(root.join("native-delayed-owner.delay"), "actual OS delay enabled after successful connection\n").unwrap();
    let started = std::time::Instant::now();
    let error = now.search("budgetneedle", aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext, &[], 8).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(4), "the real five-second child must not consume an unbounded query");
    assert_eq!(error.code(), "central.file_map_unavailable");
    let transport: Value = serde_json::from_str(&error.details()["transport_error"]).unwrap();
    assert_eq!(transport["code"], "mux.command_timeout");
    assert_eq!(error.details()["owner_operation"], "central.file-map.inspect");
    assert_eq!(fs::read_to_string(&note).unwrap(), "Existing usable Control budgetneedle\n");
}

#[test]
#[cfg(unix)]
fn non_utf8_coordinates_cannot_address_a_real_replacement_named_owner_or_world() {
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    let invalid_executable = root.join(std::ffi::OsString::from_vec(b"owner-\xff".to_vec()));
    let replacement_executable = PathBuf::from(invalid_executable.to_string_lossy().into_owned());
    fs::write(&replacement_executable, "#!/bin/sh\nprintf invoked > \"$0.invoked\"\n").unwrap();
    fs::set_permissions(&replacement_executable, fs::Permissions::from_mode(0o700)).unwrap();
    let runner = SystemRunner::new().with_timeout(Duration::from_secs(2));
    let error = call(&runner, &invalid_executable, &root, "inspect", &json!({})).unwrap_err();
    assert_eq!(error.code(), "central.file_map_coordinate_invalid");
    assert_eq!(error.details()["coordinate"], "executable");
    let invoked = PathBuf::from(format!("{}.invoked", replacement_executable.to_str().unwrap()));
    assert!(!invoked.exists(), "lossy executable conversion must not launch the other real file");
    let invalid_root = root.join(std::ffi::OsString::from_vec(b"world-\xff".to_vec()));
    let replacement_root = PathBuf::from(invalid_root.to_string_lossy().into_owned());
    fs::create_dir(&replacement_root).unwrap();
    fs::write(replacement_root.join("retained.txt"), "a distinct real World coordinate").unwrap();
    let error = call(&runner, &replacement_executable, &invalid_root, "inspect", &json!({})).unwrap_err();
    assert_eq!(error.code(), "central.file_map_coordinate_invalid");
    assert_eq!(error.details()["coordinate"], "root");
    assert!(!invoked.exists(), "lossy World conversion must not contact another real owner");
    assert_eq!(fs::read_to_string(replacement_root.join("retained.txt")).unwrap(), "a distinct real World coordinate");
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
                fs::write(&self.source_path, "Replacement record without the original query match\n")
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
    fn configured_timeout(&self) -> Option<Duration> { self.runner.configured_timeout() }
    fn run_with_timeout(&self, argv: &[String], timeout: Duration) -> aikit_core::Result<Output> {
        self.after_query(argv, self.runner.run_with_timeout(argv, timeout)?)
    }
}

#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
fn native_now_does_not_stamp_a_later_revision_on_an_original_rg_match() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider, ScopeInclude};
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let path = root.join("Control/user/day/2026-10-02/day.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "Actual original interleaved-queryneedle record\n").unwrap();
    let central = Arc::new(CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)), &ctrl, &root, None,
    ).unwrap());
    let located = call(&SystemRunner::new(), &ctrl, &root, "locate", &json!({"path":path,"content":false})).unwrap();
    let source = SourceRef::parse(located["source"]["ref"].as_str().unwrap()).unwrap();
    let original = central.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    let transport = RgReceiptInterleaving { runner:default_runner(&root), source_path:path.clone(), receipt:Mutex::new(None) };
    let scope = NowFieldScope { central_root:root,
        includes:vec![ScopeInclude { glob:"Control/user/day/*/day.md".into(), family:"day" }],
        excludes:vec![], pruned:vec![],
    };
    let now = NowFieldSourcePoolProvider::connect(&transport, aikit_adapters::ripgrep::executable(), scope).unwrap()
        .with_native_owner(Arc::clone(&central));
    let error = now.search("interleaved-queryneedle", aikit_core::knowledge_source_pool::SourceSearchMode::Fulltext, &[], 8).unwrap_err();
    assert_eq!(error.code(), "now_field.source_search_basis_conflict");
    let receipt = transport.receipt.lock().unwrap();
    let receipt = receipt.as_ref().expect("the original real rg receipt must be retained");
    assert!(receipt.ok());
    assert!(receipt.stdout.contains("interleaved-queryneedle"));
    let current = central.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    assert_eq!(current.material.binding.source, original.material.binding.source);
    assert_ne!(current.material.binding.revision, original.material.binding.revision);
    assert_eq!(current.material.body, "Replacement record without the original query match\n");
    assert_eq!(fs::read_to_string(&path).unwrap(), current.material.body);
}


#[test]
#[ignore = "explicit native integration: requires built/pinned AIKIT_CENTRAL_REAL_BIN and real ripgrep"]
fn native_work_exact_ids_collisions_current_declaration_and_historical_copies_remain_distinct() {
    use aikit_adapters::work_repos::{decode_work_file_source_ref, discover_native_work_projects,
        work_file_source_ref, NativeWorkProjectEntry, NativeWorkRepoProject, WorkReposSourcePoolProvider};
    use aikit_core::knowledge_source_pool::{NativeSourcePoolProvider, SourceSearchMode};
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl,&root,"central.init",json!({}));
    for (name,id,member) in [("left","a:b","c.md"),("right","a","b:c.md")] {
        fs::create_dir_all(root.join("Work").join(name)).unwrap();
        action(&ctrl,&root,"projectcentral.init",json!({"project":name,"project_id":id}));
        fs::write(root.join("Work").join(name).join(member),format!("# {name}\nWorkidentityneedle.\n")).unwrap();
    }
    let attach = || {
        let projects = discover_native_work_projects(&root).unwrap().into_iter().map(|entry| match entry {
            NativeWorkProjectEntry::Project(project) => project,
            NativeWorkProjectEntry::Absence {name,error} => panic!("{name}: {error}"),
        }).collect();
        WorkReposSourcePoolProvider::connect_native(SystemRunner::new().with_timeout(Duration::from_secs(30)),
            aikit_adapters::ripgrep::executable(),projects).unwrap()
    };
    let work = attach();
    assert!(work.status().available,"real rg is required for this selected gate");
    let left = work_file_source_ref("a:b",Path::new("c.md")).unwrap();
    let right = work_file_source_ref("a",Path::new("b:c.md")).unwrap();
    assert_ne!(left,right);
    let hits = work.search("Workidentityneedle",SourceSearchMode::Fulltext,&[],8).unwrap();
    assert_eq!(hits.len(),2,"{hits:?}");
    for (source,id,member,body) in [(&left,"a:b","c.md","# left\nWorkidentityneedle.\n"),(&right,"a","b:c.md","# right\nWorkidentityneedle.\n")] {
        let address = decode_work_file_source_ref(source).unwrap().unwrap();
        assert_eq!(address.project_id,id);
        assert_eq!(address.member,Path::new(member));
        let material = work.read(source).unwrap().unwrap();
        assert_eq!(material.body,body);
        let hit = hits.iter().find(|hit| &hit.source==source).unwrap();
        assert_eq!(hit.revision.as_ref(),Some(&material.binding.revision));
    }
    let native = aikit_adapters::ProjectCentralFilesystemBinding::inspect(&root.join("Work/left"),Some(&root)).unwrap();
    assert_eq!(native.semantic.native_project_root.as_str(),"source:project:a:b:root");
    assert!(work.read(&native.semantic.native_project_root).unwrap().is_none(),"native root remains the native owner's distinct role");
    let legacy = SourceRef::parse("source:project:a:b:c.md").unwrap();
    assert!(work.read(&legacy).unwrap().is_none());
    let mut retained = work.read(&left).unwrap().unwrap();
    retained.binding.source = legacy.clone();
    retained.binding.metadata.insert("owner_read_required".into(),json!(false));
    let retained_body = retained.body.clone();
    let historical = vec![retained];
    let mut index = NativeSourcePoolProvider::default();
    index.rebuild(&historical).unwrap();
    let app = KnowledgeApplication::new(FamiliarityContext::default()).with_source_pool(&index,&historical)
        .with_source_pool(&work,&[]);
    let old = KnowledgeAddress::Source(legacy.clone());
    assert_eq!(app.read(&old).unwrap_err().code(),"knowledge.source_origin_unavailable");
    assert_eq!(app.explain(&old).unwrap_err().code(),"knowledge.source_origin_unavailable");
    assert_eq!(app.route(None,std::slice::from_ref(&old)).unwrap_err().code(),"knowledge.source_origin_unavailable");
    assert_eq!(historical[0].body,retained_body,"history is retained, never reattributed");
    let restart = attach();
    assert_eq!(restart.read(&left).unwrap().unwrap().body,"# left\nWorkidentityneedle.\n");
    assert!(restart.read(&legacy).unwrap().is_none());
    let manifest = root.join("Work/left/ProjectCentral/project.json");
    let original = fs::read(&manifest).unwrap();
    let same = root.join("Work/left/ProjectCentral/same.json");
    fs::write(&same,&original).unwrap(); fs::rename(&same,&manifest).unwrap();
    assert!(work.read(&left).unwrap().is_some(),"native identical-byte replacement is compatible");
    let mut changed:Value=serde_json::from_slice(&original).unwrap();
    changed["native-extension"]=json!({"retained":true});
    fs::write(&manifest,serde_json::to_vec(&changed).unwrap()).unwrap();
    assert_eq!(work.read(&left).unwrap_err().code(),"work_repos.native_declaration_changed");
    let fresh = attach();
    assert!(fresh.read(&left).unwrap().is_some());
    fs::write(root.join("Work/left/ProjectCentral/.no-agent-retrieval"),"withdraw\n").unwrap();
    assert_eq!(fresh.read(&left).unwrap_err().code(),"work_repos.source_unauthorised");
    let survivor = WorkReposSourcePoolProvider::connect_native(SystemRunner::probe(),
        "work-test-deliberately-unavailable-rg",vec![NativeWorkRepoProject::inspect(&root.join("Work/right"),"right",Some(&root)).unwrap()]).unwrap();
    assert!(survivor.read(&legacy).unwrap().is_none(),"a current survivor cannot certify an old issuing tuple");
    assert_eq!(survivor.read(&right).unwrap().unwrap().body,"# right\nWorkidentityneedle.\n");
    assert_eq!(fs::read_to_string(root.join("Work/left/c.md")).unwrap(),"# left\nWorkidentityneedle.\n");
}

// The fixture declares its controlled native Source through the actual owning
// ground grammar. Registration can preserve that issued identity, never mint it.
fn declare_custom_source(ctrl: &Path, root: &Path, reference: &str) -> PathBuf {
    action(ctrl, root, "central.init", json!({}));
    let relative = "Control/user/issued-source.md";
    let path = root.join(relative);
    fs::write(&path, "# Actual custom-issued Source\nInitial current payload.\n").unwrap();
    let relations = root.join("Control/relations/source-relations.json");
    fs::create_dir_all(relations.parent().unwrap()).unwrap();
    fs::write(&relations, serde_json::to_vec_pretty(&json!({
        "schema":"central.control.ground-relations/v1","project_id":"control:root",
        "relations":[{"ref":reference,"path":relative,"roles":["controlled-qualification-source"],
            "provenance":"human-adopted","standing":"architecture-contract","treatment":"control-user"}],
        "qualification_scope":"owned fixture, never personal adoption",
    })).unwrap()).unwrap();
    let metadata = action(ctrl, root, "central.file-map.resolve", json!({
        "source_ref":reference,"binding_only":true,
    }));
    assert_eq!(metadata["ownership"], "owned", "{metadata}");
    assert_eq!(metadata["source"]["ref"], reference);
    assert_eq!(metadata["world_ref"], "control:root");
    assert!(metadata.get("revision").is_none() && metadata.get("content").is_none());
    let basis = action(ctrl, root, "central.file-map.inspect", json!({"resources":false}));
    let registered = action(ctrl, root, "central.file-map.register", json!({
        "path":relative,"source_ref":reference,"expected_revision":basis["revision"],
    }));
    assert_eq!(registered["source_ref"], reference, "registration must retain owner-issued opaque identity");
    path
}

#[test]
#[ignore = "explicit native integration: requires built/pinned corrected binding-only AIKIT_CENTRAL_REAL_BIN"]
fn actual_custom_issued_source_is_read_current_without_prefix_authority() {
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    let reference = "issued:controlled-qualification:source-A";
    assert!(!reference.starts_with("central:source:"));
    let path = declare_custom_source(&ctrl, &root, reference);
    let relations = root.join("Control/relations/source-relations.json");
    let retained_relations = fs::read(&relations).unwrap();
    let source = SourceRef::parse(reference).unwrap();
    let provider = CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)), &ctrl, &root, None,
    ).unwrap();
    let actual = action(&ctrl, &root, "central.file-map.resolve", json!({"source_ref":reference,"content":true}));
    let held = provider.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap().material;
    assert_eq!(held.binding.source, source);
    assert_eq!(held.binding.revision.as_str(), actual["revision"].as_str().unwrap());
    assert_eq!(held.body, actual["content"].as_str().unwrap());
    let app = KnowledgeApplication::new(FamiliarityContext::default())
        .with_source_pool(&provider, &[]);
    assert_eq!(app.read(&KnowledgeAddress::Source(source.clone())).unwrap().content.as_deref(), Some(held.body.as_str()));
    assert_eq!(provider.read_for(&source, RetrievalTarget::ExternalProvider).unwrap_err().code(), "knowledge.source_target_withheld");
    fs::write(&path, "# Actual custom-issued Source\nChanged current payload.\n").unwrap();
    let latest = action(&ctrl, &root, "central.file-map.resolve", json!({"source_ref":reference,"content":true}));
    let current = provider.read(&source).unwrap().unwrap();
    assert_eq!(current.binding.source, source);
    assert_ne!(current.binding.revision, held.binding.revision);
    assert_eq!(current.binding.revision.as_str(), latest["revision"].as_str().unwrap());
    assert_eq!(current.body, latest["content"].as_str().unwrap());
    fs::write(root.join("Control/user/.no-agent-retrieval"), "actual fixture withdrawal\n").unwrap();
    let failure = native_withdrawal(&ctrl, &root, &source);
    assert_eq!(failure["error"]["details"]["ownership"], "known", "{failure}");
    same_native_failure(&provider.read(&source).unwrap_err(), &failure);
    fs::remove_file(root.join("Control/user/.no-agent-retrieval")).unwrap();
    assert_eq!(provider.read(&source).unwrap().unwrap().binding.revision, current.binding.revision);
    assert_eq!(fs::read(&relations).unwrap(), retained_relations);
    assert_eq!(fs::read_to_string(&path).unwrap(), current.body);
}

#[test]
#[ignore = "explicit native integration: requires built/pinned corrected binding-only AIKIT_CENTRAL_REAL_BIN"]
fn actual_healthy_unregistered_declines_but_known_missing_source_preserves_native_failure() {
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    let source = SourceRef::parse("issued:controlled-qualification:missing-source").unwrap();
    let path = declare_custom_source(&ctrl, &root, source.as_str());
    let provider = CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)), &ctrl, &root, None,
    ).unwrap();
    // Even a prefix-shaped reference is not ownership. Use the actual healthy
    // native disposition, not a NotFound catch or a fabricated provider reply.
    let unknown = SourceRef::parse("central:source:unregistered-controlled-qualification").unwrap();
    let observed = action(&ctrl, &root, "central.file-map.resolve", json!({
        "project":null,"federated":true,"resources":false,"source_ref":unknown,"binding_only":true,
    }));
    assert_eq!(observed, json!({"ownership":"unregistered","binding_only":true,"source_ref":unknown}));
    for target in [RetrievalTarget::Human, RetrievalTarget::LocalAgent, RetrievalTarget::ExternalProvider] {
        assert!(provider.read_for(&unknown, target).unwrap().is_none());
    }
    fs::remove_file(&path).unwrap();
    let failure = native_withdrawal(&ctrl, &root, &source);
    assert_eq!(failure["error"]["code"], "central.file_map_not_found", "{failure}");
    assert_eq!(failure["error"]["details"]["ownership"], "known", "{failure}");
    assert_eq!(failure["error"]["details"]["material_state"], "missing", "{failure}");
    same_native_failure(&provider.read(&source).unwrap_err(), &failure);
    assert!(!path.exists());
    fs::write(&path, "# Restored actual owner source\n").unwrap();
    assert_eq!(provider.read(&source).unwrap().unwrap().body, "# Restored actual owner source\n");
}

#[cfg(unix)]
#[test]
#[ignore = "explicit native integration: requires nonroot Unix and built/pinned corrected binding-only AIKIT_CENTRAL_REAL_BIN"]
fn actual_unreadable_payload_allows_only_metadata_and_target_denial_before_body() {
    use std::os::unix::fs::PermissionsExt;
    let runner = SystemRunner::new().with_timeout(Duration::from_secs(30));
    let uid = runner.run(&["id".into(), "-u".into()]).unwrap();
    assert!(uid.ok());
    assert_ne!(uid.stdout.trim(), "0", "actual EACCES requires a nonroot hosted qualification process");
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    let source = SourceRef::parse("issued:controlled-qualification:unreadable-source").unwrap();
    let path = declare_custom_source(&ctrl, &root, source.as_str());
    let provider = CentralFileMapProvider::connect(runner, &ctrl, &root, None).unwrap();
    let original = fs::metadata(&path).unwrap().permissions();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
    // Collect actual results, then restore our owned fixture before asserting
    // so a red test does not strand an unreadable source during unwinding.
    let metadata = action_envelope(&ctrl, &root, "central.file-map.resolve", json!({
        "project":null,"federated":true,"resources":false,"source_ref":source,"binding_only":true,
    }));
    let external = provider.read_for(&source, RetrievalTarget::ExternalProvider);
    let local = provider.read(&source);
    let native_payload = action_envelope(&ctrl, &root, "central.file-map.resolve", json!({
        "project":null,"federated":true,"resources":false,"source_ref":source,"content":true,
    }));
    fs::set_permissions(&path, original).unwrap();
    assert!(metadata.0.ok() && metadata.1["ok"] == true, "{}", metadata.1);
    let descriptor = file_map_result(&metadata.1["data"], "resolve");
    assert_eq!(descriptor["ownership"], "owned");
    assert!(descriptor.get("revision").is_none() && descriptor.get("content").is_none());
    assert_eq!(external.unwrap_err().code(), "knowledge.source_target_withheld");
    assert!(!native_payload.0.ok() && native_payload.1["ok"] == false, "{}", native_payload.1);
    assert_eq!(native_payload.1["error"]["details"]["ownership"], "known");
    assert_eq!(native_payload.1["error"]["details"]["io_error"]["kind"], "PermissionDenied");
    same_native_failure(&local.unwrap_err(), &native_payload.1);
    assert_eq!(provider.read(&source).unwrap().unwrap().body, fs::read_to_string(path).unwrap());
}

// This relay runs the real owner, preserves its complete receipt, and changes
// only actual owned ground between metadata admission and the selected read.
struct BindingReceiptInterleaving {
    runner: SystemRunner,
    relations: PathBuf,
    admitted: Mutex<Option<Value>>,
}
impl BindingReceiptInterleaving {
    fn after_binding(&self, argv: &[String], output: Output) -> aikit_core::Result<Output> {
        if argv.iter().any(|argument| argument == "central.file-map.resolve")
            && argv.last().and_then(|input| serde_json::from_str::<Value>(input).ok())
                .is_some_and(|input| input["binding_only"] == true)
        {
            let mut admitted = self.admitted.lock().unwrap();
            if admitted.is_none() {
                let receipt: Value = serde_json::from_str(&output.stdout).expect("real native binding receipt");
                assert!(output.ok() && receipt["ok"] == true, "{receipt}");
                let result = file_map_result(&receipt["data"], "resolve");
                assert_eq!(result["ownership"], "owned", "{result}");
                *admitted = Some(result);
                let mut ground: Value = serde_json::from_slice(&fs::read(&self.relations).unwrap()).unwrap();
                ground["relations"][0]["standing"] = json!("durable-source");
                fs::write(&self.relations, serde_json::to_vec_pretty(&ground).unwrap()).unwrap();
            }
        }
        Ok(output)
    }
}
impl CommandRunner for BindingReceiptInterleaving {
    fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
        self.after_binding(argv, self.runner.run(argv)?)
    }
    fn configured_timeout(&self) -> Option<Duration> { self.runner.configured_timeout() }
    fn run_with_timeout(&self, argv: &[String], timeout: Duration) -> aikit_core::Result<Output> {
        self.after_binding(argv, self.runner.run_with_timeout(argv, timeout)?)
    }
}

#[test]
#[ignore = "explicit native integration: requires built/pinned corrected binding-only AIKIT_CENTRAL_REAL_BIN"]
fn actual_binding_change_after_metadata_preserves_both_native_readings_and_refuses_old_admission() {
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    let source = SourceRef::parse("issued:controlled-qualification:binding-change").unwrap();
    let path = declare_custom_source(&ctrl, &root, source.as_str());
    let relations = root.join("Control/relations/source-relations.json");
    let body = fs::read(&path).unwrap();
    let provider = CentralFileMapProvider::connect(BindingReceiptInterleaving {
        runner: SystemRunner::new().with_timeout(Duration::from_secs(30)),
        relations: relations.clone(), admitted: Mutex::new(None),
    }, &ctrl, &root, None).unwrap();
    let refusal = provider.read(&source).unwrap_err();
    assert_eq!(refusal.code(), "central.file_map_conflict");
    let admitted: Value = serde_json::from_str(&refusal.details()["native_binding"]).unwrap();
    let read: Value = serde_json::from_str(&refusal.details()["native_reading"]).unwrap();
    assert_eq!(admitted["source"]["ref"], source.as_str());
    assert_eq!(read["source"]["ref"], source.as_str());
    assert_eq!(admitted["source"]["standing"], "architecture-contract");
    assert_eq!(read["source"]["standing"], "durable-source");
    let current = action(&ctrl, &root, "central.file-map.resolve", json!({
        "project":null,"federated":true,"resources":false,"source_ref":source,"content":true,
    }));
    assert_eq!(read, current, "retain the actual new native reading, not a rewritten earlier receipt");
    assert_eq!(fs::read(&path).unwrap(), body);
    let stable = provider.read(&source).unwrap().unwrap();
    assert_eq!(stable.binding.source, source);
    assert_eq!(stable.binding.revision.as_str(), current["revision"].as_str().unwrap());
    assert_eq!(stable.body.as_bytes(), body.as_slice());
    let ground: Value = serde_json::from_slice(&fs::read(relations).unwrap()).unwrap();
    assert_eq!(ground["qualification_scope"], "owned fixture, never personal adoption");
}

#[test]
#[ignore = "explicit native integration: requires built/pinned corrected binding-only ctrl and real ripgrep"]
fn native_now_custom_source_uses_current_owner_before_independent_control_grammar() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider};
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let relative = "Control/user/day/2026-01-03/day.md";
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "Actual custom NOW initial body.\n").unwrap();
    let reference = "issued:controlled-now:literal:source";
    let source = SourceRef::parse(reference).unwrap();
    let relations = root.join("Control/relations/source-relations.json");
    fs::create_dir_all(relations.parent().unwrap()).unwrap();
    fs::write(&relations, serde_json::to_vec_pretty(&json!({
        "schema":"central.control.ground-relations/v1", "project_id":"control:root",
        "relations":[{"ref":reference,"path":relative,"roles":["controlled-qualification-source"],
            "provenance":"human-adopted","standing":"architecture-contract","treatment":"control-user"}],
        "qualification_scope":"owned fixture only, never personal adoption",
    })).unwrap()).unwrap();
    let retained_relations = fs::read(&relations).unwrap();
    let basis = action(&ctrl, &root, "central.file-map.inspect", json!({"resources":false}));
    let registered = action(&ctrl, &root, "central.file-map.register", json!({
        "path":relative,"source_ref":reference,"expected_revision":basis["revision"],
    }));
    assert_eq!(registered["source_ref"], reference);
    let central = Arc::new(CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)), &ctrl, &root, None,
    ).unwrap());
    let now = NowFieldSourcePoolProvider::connect(
        default_runner(&root), aikit_adapters::ripgrep::executable(), NowFieldScope::standard(&root),
    ).unwrap().with_native_owner(Arc::clone(&central));
    assert!(now.descriptors().is_empty());
    let local = now.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    let actual = central.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    assert_eq!(local, actual);
    assert_eq!(local.material.binding.source, source);
    assert_eq!(local.material.body, fs::read_to_string(&path).unwrap());
    assert_eq!(now.read_for(&source, RetrievalTarget::ExternalProvider).unwrap_err().code(),
        "knowledge.source_target_withheld");
    let unknown = SourceRef::parse("issued:controlled-now:never-registered").unwrap();
    assert!(now.read_for(&unknown, RetrievalTarget::ExternalProvider).unwrap().is_none());
    fs::write(&path, "Actual custom NOW changed body.\n").unwrap();
    let changed = now.read(&source).unwrap().unwrap();
    assert_eq!(changed.binding.source, source);
    assert_ne!(changed.binding.revision, local.material.binding.revision);
    assert_eq!(changed.body, fs::read_to_string(&path).unwrap());
    fs::write(path.parent().unwrap().join(".no-agent-retrieval"), "actual current native withdrawal").unwrap();
    let failure = native_withdrawal(&ctrl, &root, &source);
    same_native_failure(&now.read(&source).unwrap_err(), &failure);
    assert_eq!(fs::read(&relations).unwrap(), retained_relations);
    assert_eq!(fs::read_to_string(&path).unwrap(), changed.body);
}


#[test]
#[ignore = "explicit native integration requires pinned corrected Ctrl, real BKMR 7.6.7 and real ripgrep"]
fn actual_borrowed_project_owner_scope_queries_two_projects_and_only_native_linked_common_source() {
    use aikit_core::knowledge_source_pool::SourceSearchMode;
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider, ScopeInclude};
    use std::sync::Arc;
    let ctrl = owner(); let owned = scratch(); let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let mut sources = Vec::new(); let mut now_sources = Vec::new();
    for (member, id) in [("alpha", "scope:alpha"), ("beta", "scope:beta")] {
        fs::create_dir_all(root.join("Work").join(member)).unwrap();
        action(&ctrl, &root, "projectcentral.init", json!({"project":member,"project_id":id}));
        let relative = "ProjectCentral/user/current-scope.md";
        let path = root.join("Work").join(member).join(relative);
        fs::write(&path, format!("# Current {member} Source\nR4scopeCedar real selected {member}.\n")).unwrap();
        let located = action(&ctrl, &root, "central.file-map.locate", json!({"project":member,"federated":false,"path":path,"binding_only":true}));
        assert_eq!(located["ownership"], "owned"); assert_eq!(located["world_ref"], format!("project:{id}"));
        sources.push(SourceRef::parse(located["source"]["ref"].as_str().unwrap()).unwrap());
        let relative_now = "ProjectCentral/now/agents/current-scope.json";
        let now_path = root.join("Work").join(member).join(relative_now);
        fs::create_dir_all(now_path.parent().unwrap()).unwrap();
        fs::write(&now_path, format!("{{\"scope\":\"{member} R4scopeNowCedar real selected record\"}}\n")).unwrap();
        let inspected = action(&ctrl, &root, "central.file-map.inspect", json!({"project":member,"resources":false}));
        let registered = action(&ctrl, &root, "central.file-map.register", json!({"project":member,"path":relative_now,"expected_revision":inspected["revision"]}));
        now_sources.push(SourceRef::parse(registered["source_ref"].as_str().unwrap()).unwrap());
    }
    let common = root.join("Control/user/current-scope-common.md");
    fs::write(&common, "# Actual common Control source\nR4scopeCedar real common Control.\n").unwrap();
    let descriptor = action(&ctrl, &root, "central.file-map.locate", json!({"path":common,"binding_only":true}));
    let common_ref = SourceRef::parse(descriptor["source"]["ref"].as_str().unwrap()).unwrap();
    let inspected = action(&ctrl, &root, "central.file-map.inspect", json!({"project":"alpha","resources":false}));
    let link = action(&ctrl, &root, "central.file-map.link", json!({"project":"alpha","path":"common/current.md",
        "source_ref":common_ref,"owner":"test:r4-current-scope","expected_revision":inspected["revision"]}));
    assert_eq!(link["source_ref"], common_ref.as_str());
    assert!(fs::symlink_metadata(root.join("Work/alpha/common/current.md")).unwrap().file_type().is_symlink());
    for project in [None, Some("alpha"), Some("beta")] {
        action(&ctrl, &root, "central.file-map.refresh", json!({"project":project,"embeddings":false}));
    }
    let owner = Arc::new(CentralFileMapProvider::connect(SystemRunner::new().with_timeout(Duration::from_secs(30))
        .with_strict_utf8(), &ctrl, &root, None).unwrap());
    assert!(owner.capabilities().fulltext, "real current native BKMR query owner must be available");
    let view = owner.for_project("alpha").unwrap();
    assert_eq!(view.capabilities(), owner.capabilities(), "borrowed scope preserves provider identity/capabilities");
    let global = owner.search("R4scopeCedar", SourceSearchMode::Fulltext, &[], 16).unwrap();
    for source in &sources { assert!(global.iter().any(|hit| &hit.source == source)); }
    assert!(global.iter().any(|hit| hit.source == common_ref));
    let selected = view.search("R4scopeCedar", SourceSearchMode::Fulltext, &[], 16).unwrap();
    assert!(selected.iter().any(|hit| hit.source == sources[0]));
    assert!(selected.iter().any(|hit| hit.source == common_ref));
    assert!(!selected.iter().any(|hit| hit.source == sources[1]));
    for source in [&sources[0], &common_ref] {
        let actual = action(&ctrl, &root, "central.file-map.resolve", json!({"project":"alpha","federated":false,"source_ref":source,"content":true}));
        let current = view.read_for(source, RetrievalTarget::LocalAgent).unwrap().unwrap();
        assert_eq!(current.material.body, actual["content"].as_str().unwrap());
        assert_eq!(current.material.binding.revision.as_str(), actual["revision"].as_str().unwrap());
    }
    // The native owner selects scope before content. Neither a successful
    // unknown lookup nor a native refusal may cause a root-view body fallback.
    match view.read(&sources[1]) {
        Ok(reading) => assert!(reading.is_none()),
        Err(error) => {
            let actual = action_envelope(&ctrl, &root, "central.file-map.resolve", json!({"project":"alpha","federated":false,"source_ref":sources[1],"binding_only":true}));
            assert!(!actual.0.ok() && actual.1["ok"] == false);
            same_native_failure(&error, &actual.1);
        }
    }
    let scope = NowFieldScope { central_root:root.clone(), includes:vec![
        ScopeInclude {glob:"Work/alpha/ProjectCentral/now/**/*.json".into(),family:"projectcentral"},
    ], excludes:vec![],pruned:vec![] };
    let now = NowFieldSourcePoolProvider::connect(default_runner(&root), aikit_adapters::ripgrep::executable(), scope).unwrap()
        .with_native_owner(Arc::clone(&owner)).with_native_project("alpha").unwrap();
    assert!(now.descriptors().is_empty());
    let now_hits = now.search("R4scopeNowCedar", SourceSearchMode::Fulltext, &[], 16).unwrap();
    assert!(now_hits.iter().any(|hit| hit.source == now_sources[0]));
    assert!(!now_hits.iter().any(|hit| hit.source == now_sources[1]));
    let outside_now = action_envelope(&ctrl, &root, "central.file-map.resolve", json!({
        "project":"alpha", "federated":false, "source_ref":now_sources[1], "binding_only":true,
    }));
    match now.read(&now_sources[1]) {
        Ok(reading) => {
            assert!(reading.is_none());
            assert!(outside_now.0.ok());
            assert_eq!(outside_now.1["data"]["result"]["ownership"], "unregistered");
        }
        Err(error) => {
            assert!(!outside_now.0.ok() && outside_now.1["ok"] == false);
            same_native_failure(&error, &outside_now.1);
        }
    }
    let admitted = now.read(&now_sources[0]).unwrap().unwrap();
    assert_eq!(admitted, view.read(&now_sources[0]).unwrap().unwrap());
    fs::write(root.join("Work/alpha/ProjectCentral/now/agents/.no-agent-retrieval"), "actual current owner withdrawal\n").unwrap();
    let actual = action_envelope(&ctrl, &root, "central.file-map.resolve", json!({"project":"alpha","federated":false,"source_ref":now_sources[0],"binding_only":true}));
    assert!(!actual.0.ok() && actual.1["ok"] == false);
    same_native_failure(&now.read(&now_sources[0]).unwrap_err(), &actual.1);
    assert!(owner.read(&sources[1]).unwrap().is_some(), "root/global relation remains independently useful");
}

#[test]
#[cfg(unix)]
#[ignore = "explicit native integration: corrected binding-only ctrl and real ripgrep required"]
fn actual_custom_temporal_return_source_keeps_owner_identity_and_declines_other_families_before_body() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    fs::create_dir_all(root.join("Work/temporal")).unwrap();
    action(&ctrl, &root, "projectcentral.init", json!({"project":"temporal", "project_id":"native/temporal"}));
    action(&ctrl, &root, "projectcentral.now.init", json!({"project":"temporal"}));
    let returned = action(&ctrl, &root, "projectcentral.now.return", json!({
        "project":"temporal", "id":"native-temporal-source", "actor":"agent:controlled-native-qualification",
        "kind":"handoff", "subject":"Actual temporal Source", "result":"Retain this genuine native Return.", "status":"active",
    }));
    let member = returned["source"].as_str().unwrap();
    assert!(Path::new(member).starts_with("ProjectCentral/now/agents"));
    let project_root = root.join("Work/temporal");
    let path = project_root.join(member);
    let retained = fs::read(&path).unwrap();
    // This is a temporal Return Source, not an allocated NowRecord. Its
    // accepted Source relation can use an opaque identity without reminting
    // any native now_ref/source_ref co-reference inside an allocated record.
    let actual = action(&ctrl, &root, "central.file-map.locate", json!({"path":path,"binding_only":true}));
    let reference = "issued:controlled-qualification:temporal-return";
    let relation = json!({"ref":reference,"path":member,"roles":actual["source"]["roles"],
        "provenance":actual["source"]["provenance"],"standing":actual["source"]["standing"],
        "treatment":actual["source"]["treatment"]});
    let relations_path = project_root.join("ProjectCentral/relations/source-relations.json");
    fs::create_dir_all(relations_path.parent().unwrap()).unwrap();
    let mut relations = if relations_path.is_file() {
        serde_json::from_slice::<Value>(&fs::read(&relations_path).unwrap()).unwrap()
    } else {
        json!({"schema":"central.project.ground-relations/v1","project_id":"native/temporal","relations":[]})
    };
    relations["relations"].as_array_mut().unwrap().push(relation);
    fs::write(&relations_path, serde_json::to_vec_pretty(&relations).unwrap()).unwrap();
    let basis = action(&ctrl, &root, "central.file-map.inspect", json!({"project":"temporal","resources":false}));
    let registered = action(&ctrl, &root, "central.file-map.register", json!({
        "project":"temporal","path":member,"source_ref":reference,"expected_revision":basis["revision"],
    }));
    assert_eq!(registered["source_ref"], reference);
    let source = SourceRef::parse(reference).unwrap();
    let relay = root.join("temporal-owner-observer");
    fs::write(&relay, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$0.args\"\nIFS= read -r native_ctrl < \"$0.owner\"\nexec \"$native_ctrl\" \"$@\"\n").unwrap();
    fs::write(root.join("temporal-owner-observer.owner"), format!("{}\n", ctrl.display())).unwrap();
    fs::set_permissions(&relay, fs::Permissions::from_mode(0o700)).unwrap();
    let central = Arc::new(CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)), &relay, &root, None,
    ).unwrap());
    let now = NowFieldSourcePoolProvider::connect(default_runner(&root), aikit_adapters::ripgrep::executable(), NowFieldScope::standard(&root)).unwrap()
        .with_native_owner(Arc::clone(&central));
    let current = now.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    let native = central.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    assert_eq!(current, native);
    assert_eq!(current.material.binding.source, source);
    assert_eq!(current.material.body.as_bytes(), retained);
    fs::write(root.join("temporal-owner-observer.args"), "").unwrap();
    assert_eq!(now.read_for(&source, RetrievalTarget::ExternalProvider).unwrap_err().code(), "knowledge.source_target_withheld");
    let observed = fs::read_to_string(root.join("temporal-owner-observer.args")).unwrap();
    assert!(!observed.contains("\"content\":true"), "native target refusal precedes all payload invocations: {observed}");
    let outside = root.join("Control/user/outside-temporal.md");
    fs::write(&outside, "# Real Source outside NOW\n").unwrap();
    let outside_meta = action(&ctrl, &root, "central.file-map.locate", json!({"path":outside,"binding_only":true}));
    let outside_ref = SourceRef::parse(outside_meta["source"]["ref"].as_str().unwrap()).unwrap();
    fs::write(root.join("temporal-owner-observer.args"), "").unwrap();
    assert!(now.read_for(&outside_ref, RetrievalTarget::ExternalProvider).unwrap().is_none());
    let unknown = SourceRef::parse("issued:controlled-qualification:healthy-unregistered").unwrap();
    let verdict = action(&ctrl, &root, "central.file-map.resolve", json!({"source_ref":unknown,"binding_only":true}));
    assert_eq!(verdict["ownership"], "unregistered");
    assert!(now.read_for(&unknown, RetrievalTarget::ExternalProvider).unwrap().is_none());
    let observed = fs::read_to_string(root.join("temporal-owner-observer.args")).unwrap();
    assert!(!observed.contains("\"content\":true"), "nonowner/outside family must not enter payload transport: {observed}");
    // The temporal directory is native material too, but is not a NOW
    // payload file. Actual kind evidence must precede family/target/body.
    let basis = action(&ctrl, &root, "central.file-map.inspect", json!({"project":"temporal","resources":false}));
    let directory = action(&ctrl, &root, "central.file-map.register", json!({
        "project":"temporal","path":"ProjectCentral/now/agents","expected_revision":basis["revision"],
    }));
    let directory_ref = SourceRef::parse(directory["source_ref"].as_str().unwrap()).unwrap();
    let directory_meta = action(&ctrl, &root, "central.file-map.resolve", json!({"source_ref":directory_ref,"binding_only":true}));
    assert_eq!(directory_meta["kind"], "directory");
    fs::write(root.join("temporal-owner-observer.args"), "").unwrap();
    assert!(now.read_for(&directory_ref, RetrievalTarget::ExternalProvider).unwrap().is_none());
    let observed = fs::read_to_string(root.join("temporal-owner-observer.args")).unwrap();
    assert!(!observed.contains("\"content\":true"), "nonfile temporal material must not enter payload transport: {observed}");
    fs::write(path.parent().unwrap().join(".no-agent-retrieval"), "actual temporal withdrawal\n").unwrap();
    let refusal = native_withdrawal(&ctrl, &root, &source);
    same_native_failure(&now.read_for(&source, RetrievalTarget::LocalAgent).unwrap_err(), &refusal);
    fs::remove_file(path.parent().unwrap().join(".no-agent-retrieval")).unwrap();
    assert_eq!(fs::read(&path).unwrap(), retained);
    fs::remove_file(&path).unwrap();
    let missing = native_withdrawal(&ctrl, &root, &source);
    same_native_failure(&now.read_for(&source, RetrievalTarget::LocalAgent).unwrap_err(), &missing);
    fs::write(&path, &retained).unwrap();
    assert_eq!(now.read(&source).unwrap().unwrap().binding.source, source);
}

#[test]
#[ignore = "explicit native integration: actual native placement policy/allocation/read and corrected ctrl required"]
fn actual_allocated_now_source_keeps_its_native_record_coreference_and_revision() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider};
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let policy_path = "Control/user/controlled-placement.json";
    fs::write(root.join(policy_path), serde_json::to_vec_pretty(&json!({
        "schema":"central.work-placement-policy/v1","scope_ref":"control:root","writable":[],"protected":[],
        "enforcement":"native-actions","required_coverage":["file-content"],"lease_seconds":300,
    })).unwrap()).unwrap();
    let located = action(&ctrl, &root, "central.file-map.locate", json!({"path":root.join(policy_path),"binding_only":true}));
    let policy_ref = located["source"]["ref"].as_str().unwrap();
    let relations_path = root.join("Control/relations/source-relations.json");
    fs::create_dir_all(relations_path.parent().unwrap()).unwrap();
    fs::write(&relations_path, serde_json::to_vec_pretty(&json!({
        "schema":"central.control.ground-relations/v1","project_id":"control:root","relations":[{
            "ref":policy_ref,"path":policy_path,"roles":["work-placement-policy"],"provenance":"human-adopted",
            "standing":"architecture-contract","treatment":"control-user",
            "recognition":"explicit-owned-native-test-fixture-not-personal-adoption","recorded_at_unix_seconds":1,
        }],
    })).unwrap()).unwrap();
    let policy = action(&ctrl, &root, "central.work.policy", json!({}));
    let allocation = action(&ctrl, &root, "central.now.allocate", json!({
        "task_ref":"task:controlled-native-now","purpose":"Verify actual native identity without reminting",
        "participant_refs":[],"source_refs":[],"expected_policy_revision":policy["revision"],
    }));
    assert_eq!(allocation["schema"], "central.now-allocation/v1");
    assert_eq!(allocation["created"], true);
    let now_ref = allocation["now_ref"].as_str().unwrap();
    let native_now = action(&ctrl, &root, "central.now.read", json!({"now_ref":now_ref}));
    assert_eq!(native_now["record"]["now_ref"], now_ref);
    assert_eq!(native_now["record"]["source_ref"], native_now["source"]["ref"]);
    let source = SourceRef::parse(native_now["record"]["source_ref"].as_str().unwrap()).unwrap();
    let path = root.join(native_now["source"]["path"].as_str().unwrap());
    let retained = fs::read(&path).unwrap();
    let central = Arc::new(CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(30)), &ctrl, &root, None,
    ).unwrap());
    let now = NowFieldSourcePoolProvider::connect(default_runner(&root), aikit_adapters::ripgrep::executable(), NowFieldScope::standard(&root)).unwrap()
        .with_native_owner(Arc::clone(&central));
    let reading = now.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    let actual_source = central.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    assert_eq!(reading, actual_source);
    assert_eq!(reading.material.binding.source, source);
    assert_eq!(reading.material.binding.revision.as_str(), native_now["revision"]["revision"].as_str().unwrap());
    assert_eq!(reading.material.body.as_bytes(), retained);
    assert_eq!(fs::read(&path).unwrap(), retained);
    let latest = action(&ctrl, &root, "central.now.read", json!({"now_ref":now_ref}));
    assert_eq!(latest["record"]["now_ref"], now_ref);
    assert_eq!(latest["record"]["source_ref"], source.as_str());
    assert_eq!(latest["revision"], native_now["revision"]);
}

#[test]
#[cfg(unix)]
#[ignore = "explicit native read-deadline qualification: real corrected ctrl, ripgrep and owned OS subprocesses required"]
fn native_now_selected_read_preserves_configured_transport_deadline_and_original_failure() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let note = root.join("Control/agents/now/day/read-budget.md");
    fs::create_dir_all(note.parent().unwrap()).unwrap();
    fs::write(&note, "Actual selected native temporal material.\n").unwrap();
    let binding = action(&ctrl, &root, "central.file-map.locate", json!({"path":note,"binding_only":true}));
    assert_eq!(binding["ownership"], "owned");
    let source = SourceRef::parse(binding["source"]["ref"].as_str().unwrap()).unwrap();
    let relay = root.join("native-read-deadline-owner");
    fs::write(&relay, "#!/bin/sh\nif [ -f \"$0.delay\" ]; then /bin/sleep 10; fi\nIFS= read -r native_ctrl < \"$0.owner\"\nexec \"$native_ctrl\" \"$@\"\n").unwrap();
    fs::write(root.join("native-read-deadline-owner.owner"), format!("{}\n", ctrl.display())).unwrap();
    fs::set_permissions(&relay, fs::Permissions::from_mode(0o700)).unwrap();
    let central = Arc::new(CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(3)), &relay, &root, None,
    ).unwrap());
    let now = NowFieldSourcePoolProvider::connect(default_runner(&root), aikit_adapters::ripgrep::executable(), NowFieldScope::standard(&root)).unwrap()
        .with_native_owner(central);
    fs::write(root.join("native-read-deadline-owner.delay"), "Enable actual delay after successful native connection.\n").unwrap();
    let started = std::time::Instant::now();
    let failure = now.read_for(&source, RetrievalTarget::LocalAgent).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(8), "configured native read budget and owned cleanup must retire the actual delayed transport");
    assert_eq!(failure.code(), "central.file_map_unavailable");
    assert_eq!(failure.details()["owner_operation"], "central.file-map.resolve");
    let transport: Value = serde_json::from_str(&failure.details()["transport_error"]).unwrap();
    assert_eq!(transport["code"], "mux.command_timeout");
    assert_eq!(transport["details"]["execution_started"], "true");
    assert_eq!(transport["details"]["direct_child_reaped"], "true");
    assert_eq!(fs::read_to_string(&note).unwrap(), "Actual selected native temporal material.\n");
    fs::remove_file(root.join("native-read-deadline-owner.delay")).unwrap();
    assert_eq!(now.read(&source).unwrap().unwrap().binding.source, source);
}

#[test]
#[cfg(unix)]
#[ignore = "explicit native aggregate deadline: built/pinned corrected ctrl, ripgrep and actual owned OS transport required"]
fn actual_native_now_metadata_and_payload_share_one_configured_read_deadline() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    let ctrl = owner();
    let owned = scratch();
    let root = fs::canonicalize(owned.path()).unwrap();
    action(&ctrl, &root, "central.init", json!({}));
    let note = root.join("Control/agents/now/day/shared-read-budget.md");
    fs::create_dir_all(note.parent().unwrap()).unwrap();
    let bytes = b"Actual temporal Source under one native read deadline.\n";
    fs::write(&note, bytes).unwrap();
    let binding = action(&ctrl, &root, "central.file-map.locate", json!({"path":note,"binding_only":true}));
    assert_eq!(binding["ownership"], "owned");
    assert_eq!(binding["kind"], "file");
    let source = SourceRef::parse(binding["source"]["ref"].as_str().unwrap()).unwrap();
    let relay = root.join("native-now-shared-budget-owner");
    // Every receipt is the same actual ctrl's output. Only a real transport
    // delay is added after a successful current owner/read control. No native
    // envelope, ownership, body, status or errno is fabricated by the relay.
    fs::write(&relay, "#!/bin/sh\nif [ -f \"$0.delay\" ]; then\n  case \"$*\" in\n    *'\"binding_only\":true'*) phase=metadata ;;\n    *'\"content\":true'*) phase=payload ;;\n    *) phase=other ;;\n  esac\n  printf '%s\\n' \"$phase\" >> \"$0.phases\"\n  /bin/sleep 1.5\nfi\nIFS= read -r native_ctrl < \"$0.owner\"\nexec \"$native_ctrl\" \"$@\"\n").unwrap();
    fs::write(root.join("native-now-shared-budget-owner.owner"), format!("{}\n", ctrl.display())).unwrap();
    fs::set_permissions(&relay, fs::Permissions::from_mode(0o700)).unwrap();
    let central = Arc::new(CentralFileMapProvider::connect(
        SystemRunner::new().with_timeout(Duration::from_secs(4)).with_strict_utf8(),
        &relay, &root, None,
    ).unwrap());
    let now = NowFieldSourcePoolProvider::connect(
        default_runner(&root), aikit_adapters::ripgrep::executable(), NowFieldScope::standard(&root),
    ).unwrap().with_native_owner(Arc::clone(&central));
    let before = now.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    assert_eq!(before, central.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap());
    assert_eq!(before.material.body.as_bytes(), bytes);
    fs::write(root.join("native-now-shared-budget-owner.delay"), "Actual same-owner metadata and payload transport delay.\n").unwrap();
    let started = std::time::Instant::now();
    let failure = now.read_for(&source, RetrievalTarget::LocalAgent).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(9), "one configured read deadline plus bounded owned retirement");
    assert_eq!(failure.code(), "central.file_map_unavailable");
    assert_eq!(failure.details()["owner_operation"], "central.file-map.resolve");
    let transport: Value = serde_json::from_str(&failure.details()["transport_error"]).unwrap();
    assert_eq!(transport["code"], "mux.command_timeout");
    assert_eq!(transport["details"]["execution_started"], "true");
    assert_eq!(transport["details"]["direct_child_reaped"], "true");
    let phases = fs::read_to_string(root.join("native-now-shared-budget-owner.phases")).unwrap();
    assert_eq!(phases.lines().collect::<Vec<_>>(), vec!["metadata", "metadata", "payload"],
        "the actual outer and current metadata receipts completed before payload transport exhausted their shared remaining deadline");
    assert_eq!(fs::read(&note).unwrap(), bytes);
    fs::remove_file(root.join("native-now-shared-budget-owner.delay")).unwrap();
    let after = now.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
    assert_eq!(after, before);
    assert_eq!(after.material.binding.source, source);
}
