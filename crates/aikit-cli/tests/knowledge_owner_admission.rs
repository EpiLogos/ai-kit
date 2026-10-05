//! Current-source regressions through the production Service and CLI.
//! These own real filesystem sources; they do not mock native owner replies.
//! The selected Redis gate creates its record through the actual pinned
//! Central owner and resolves its real Source identity/basis. It does not
//! claim allocation, model execution or Factory worker Return. Unavailable
//! native/Redis providers fail that selected gate, never a green skip.

use aikit_adapters::runner::{Output, SystemRunner};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use aikit_cli::app::Service;
use aikit_core::resource::{ResourceRef, SourceRef, SourceRevision};
use aikit_core::{FlowChangedSinceState, FlowThoughtRecord, KnowledgeAddress};
use aikit_store::knowledge_cache::KnowledgeCacheStore;
use aikit_store::now_context::{RedisNowConfig, NOW_REDIS_CONFIG_SCHEMA};
use aikit_store::AikitHome;
use tempfile::TempDir;

const SOURCE: &str = "source:paper:r4-current-owner";
const OLD_BODY: &str = "R4oldCedar current source before its owner changes it.";
const NEW_BODY: &str = "R4newAlder current source after its owner changes it.";

fn native_tempdir() -> TempDir {
    let scratch =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
    fs::create_dir_all(&scratch).unwrap();
    tempfile::Builder::new()
        .prefix("knowledge-current-admission-")
        .tempdir_in(&scratch)
        .unwrap()
}

struct Ground {
    owned: TempDir,
    project: PathBuf,
    home: PathBuf,
    source: PathBuf,
}

impl Ground {
    fn standalone() -> Self {
        let owned = native_tempdir();
        let project = owned.path().join("project");
        let home = owned.path().join("aikit-home");
        fs::create_dir_all(project.join(".aikit")).unwrap();
        fs::write(project.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
        let source = project.join("source-material.json");
        let ground = Self {
            owned,
            project,
            home,
            source,
        };
        ground.write_source(OLD_BODY);
        ground
    }

    fn write_source(&self, body: &str) {
        let material = serde_json::json!({
            "binding": {
                "source": SOURCE,
                "revision": aikit_core::knowledge_ingest::corpus_content_revision(body.as_bytes()),
                "title": "Retained copy without an operative origin",
                "tags": ["R4current"],
                "visibility": "public",
                "owners": [],
                "media_type": "text/markdown",
                "metadata": {}
            },
            "body": body
        });
        fs::write(&self.source, serde_json::to_vec(&material).unwrap()).unwrap();
    }

    fn service(&self) -> Service {
        Service::open(AikitHome::at(&self.home), &self.project, |_| None).unwrap()
    }

    fn cli(&self, address: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
        let address = format!("source={address}");
        command
            .args(["--json", "knowledge", "read", address.as_str()])
            .current_dir(&self.project)
            .env("AIKIT_HOME", &self.home)
            .env("HOME", self.owned.path())
            .env_remove("CENTRAL_ROOT")
            .env_remove("AIKIT_CENTRAL_ROOT")
            .env_remove("OI_CENTRAL_ROOT")
            .env_remove("AIKIT_WORLD_REDIS_CONFIG")
            .env_remove("AIKIT_KNOWLEDGE_RESULT_CACHE");
        bounded_output(&mut command)
    }
}

const CLI_OUTPUT_BUDGET: u64 = 8 * 1024 * 1024;

fn bounded_output(command: &mut Command) -> Output {
    // The SAME real production lifecycle owns pipes, cancellation and reaping.
    // No test-only reader thread, spool or process-group implementation.
    SystemRunner::new()
        .with_timeout(Duration::from_secs(20))
        .with_output_limit_bytes(CLI_OUTPUT_BUDGET)
        .capture_command(command)
        .expect("actual native CLI capture must complete within its real bounds")
}

fn address() -> KnowledgeAddress {
    KnowledgeAddress::Source(SourceRef::parse(SOURCE).unwrap())
}

fn assert_withheld(service: &mut Service, address: &KnowledgeAddress, needle: &str) {
    if let Ok(reading) = service.knowledge_read(address) {
        assert!(
            reading.content.is_none(),
            "withdrawn source returned a body: {reading:?}"
        );
    }
    assert!(service.knowledge_read_document(address).is_err());
    assert!(service.knowledge_relations(address, 1, 32, 64).is_err());
    let search = service.knowledge_search(needle, 32).unwrap();
    assert!(!search
        .hits
        .iter()
        .any(|hit| hit.resource == address.resource_ref()));
    let frame = service.knowledge_frame(None, &[address.clone()]).unwrap();
    assert!(!serde_json::to_string(&frame).unwrap().contains(needle));
}

#[test]
fn explicit_authorised_pure_source_pool_stays_usable_without_owner_or_redis_composition() {
    use aikit_core::knowledge_source_pool::{
        NativeSourcePoolProvider, SourceMaterial, SourcePoolProvider,
    };
    use aikit_core::{FamiliarityContext, KnowledgeApplication};
    // This public API expressly accepts already-authorised material supplied
    // by its caller. It is not a fabricated native owner response or a claim
    // that generic copies discovered on disk acquire that same standing.
    for body in [OLD_BODY, NEW_BODY] {
        let revision = aikit_core::knowledge_ingest::corpus_content_revision(body.as_bytes());
        let material: SourceMaterial = serde_json::from_value(serde_json::json!({
            "binding":{"source":SOURCE,"revision":revision,"title":"Explicit authorised material",
                "tags":[],"visibility":"public","owners":[],"media_type":"text/markdown","metadata":{}},
            "body":body,
        })).unwrap();
        let material = vec![material];
        let mut provider = NativeSourcePoolProvider::new();
        provider.rebuild(&material).unwrap();
        let application = KnowledgeApplication::new(FamiliarityContext::default())
            .with_source_pool(&provider, &material);
        let read = application.read(&address()).unwrap();
        assert_eq!(read.content.as_deref(), Some(body));
        assert_eq!(read.revision.as_deref(), Some(revision.as_str()));
        assert!(!application
            .search(body.split_whitespace().next().unwrap(), 8)
            .hits
            .is_empty());
    }
}

#[test]
fn unknown_legacy_generic_copy_is_withheld_same_service_fresh_service_and_actual_cli() {
    let ground = Ground::standalone();
    let original = fs::read(&ground.source).unwrap();
    let mut service = ground.service();
    assert_withheld(&mut service, &address(), "R4oldCedar");
    let before = ground.cli(SOURCE);
    assert!(!before.ok());
    assert!(!before.stdout.contains(OLD_BODY));
    ground.write_source(NEW_BODY);
    let current = fs::read(&ground.source).unwrap();
    assert_ne!(current, original);
    assert_withheld(&mut service, &address(), "R4newAlder");
    assert_withheld(&mut ground.service(), &address(), "R4newAlder");
    let after = ground.cli(SOURCE);
    assert!(!after.ok());
    assert!(!after.stdout.contains(NEW_BODY));
    assert_eq!(fs::read(&ground.source).unwrap(), current);
    fs::write(
        ground.project.join(".no-agent-retrieval"),
        "owner withdrew this aperture",
    )
    .unwrap();
    assert_withheld(&mut service, &address(), "R4newAlder");
    assert_withheld(&mut ground.service(), &address(), "R4newAlder");
    let withdrawn = ground.cli(SOURCE);
    assert!(!withdrawn.ok());
    assert!(!withdrawn.stdout.contains(NEW_BODY));
    assert!(!withdrawn.stderr.contains(NEW_BODY));
    assert_eq!(fs::read(&ground.source).unwrap(), current);
}

#[test]
#[cfg(unix)]
fn actual_descendant_output_descriptor_cannot_hold_bounded_capture_until_its_sleep_finishes() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 30 & printf 'actual-owned-parent-exit\n'"]);
    let start = Instant::now();
    let output = bounded_output(&mut command);
    assert!(output.ok());
    assert_eq!(output.stdout, "actual-owned-parent-exit\n");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn world_nested_flow_read_returns_actual_unavailable_state_without_shared_runtime_borrow() {
    let owned = native_tempdir();
    let world = owned.path().join("world");
    fs::create_dir_all(world.join("Control/agents/now")).unwrap();
    fs::create_dir_all(world.join("Work")).unwrap();
    let mut service = Service::open(AikitHome::at(owned.path().join("home")), &world, |key| {
        (key == "CENTRAL_ROOT").then(|| world.display().to_string())
    })
    .unwrap();
    // This is a typed retained input, not a claimed model execution. With no
    // owner horizon the real nested descriptor read must return Unavailable.
    let thought = FlowThoughtRecord {
        version: aikit_core::flow_cognition::FLOW_COGNITION_VERSION.into(),
        invocation_ref: ResourceRef::parse("flow-contemplate/r4-prior-reading").unwrap(),
        flow_ref: ResourceRef::parse("wiki:node:r4-flow").unwrap(),
        source_ref: SourceRef::parse("source:flow:r4").unwrap(),
        basis_revision: SourceRevision::parse("owner-r1").unwrap(),
        horizon_cursor: 0,
        outcome: None,
    };
    let receipt = service.flow_changed_since(&thought, None).unwrap();
    assert_eq!(receipt.flow, thought.flow_ref);
    assert_eq!(receipt.thought, thought.invocation_ref);
    assert_eq!(receipt.reading.state, FlowChangedSinceState::Unavailable);
    assert!(receipt.reading.changed_sources.is_empty());
    assert!(!receipt.reading.automatic_agent_or_model_invocation);
    assert!(service.knowledge_history(None).unwrap().is_empty());
}

fn selected_native_owner() -> PathBuf {
    let pinned = PathBuf::from(
        std::env::var_os("AIKIT_CENTRAL_REAL_BIN")
            .expect("selected native Redis gate requires its built/pinned Central owner"),
    );
    assert!(
        pinned.is_file(),
        "actual pinned owner binary is required: {}",
        pinned.display()
    );
    let configured = PathBuf::from(
        std::env::var_os("CENTRAL_CTRL_BIN")
            .expect("selected native Redis gate must configure the Service's actual owner"),
    );
    assert_eq!(
        fs::canonicalize(&pinned).unwrap(),
        fs::canonicalize(&configured).unwrap(),
        "the native fixture and the production Service must use the SAME pinned owner"
    );
    assert_eq!(
        fs::canonicalize(aikit_adapters::central_file_map::executable()).unwrap(),
        fs::canonicalize(&pinned).unwrap()
    );
    pinned
}

fn native_owner_action(
    ctrl: &std::path::Path,
    root: &std::path::Path,
    operation: &str,
    input: serde_json::Value,
) -> serde_json::Value {
    use aikit_adapters::runner::CommandRunner;
    let output = SystemRunner::new()
        .with_timeout(Duration::from_secs(30))
        .with_output_limit_bytes(CLI_OUTPUT_BUDGET)
        .run(&[
            ctrl.display().to_string(),
            "--json".into(),
            "--root".into(),
            root.display().to_string(),
            "action".into(),
            "run".into(),
            operation.into(),
            input.to_string(),
        ])
        .expect("actual native fixture owner must execute, never an unavailable-as-green skip");
    let envelope: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert!(
        output.ok() && envelope["ok"] == true,
        "{envelope}; stderr={}",
        output.stderr
    );
    if let Some(name) = operation.strip_prefix("central.file-map.") {
        assert_eq!(envelope["data"]["schema"], "central.file-map/v1");
        assert_eq!(envelope["data"]["operation"], name);
        assert!(envelope["data"]["result"].is_object());
        envelope["data"]["result"].clone()
    } else {
        envelope["data"].clone()
    }
}

fn assert_current_native_read(
    service: &Service,
    address: &KnowledgeAddress,
    current: &serde_json::Value,
) {
    let reading = service.knowledge_read(address).unwrap();
    assert_eq!(reading.resource, address.resource_ref());
    assert_eq!(reading.content.as_deref(), current["content"].as_str());
    assert_eq!(reading.revision.as_deref(), current["revision"].as_str());
}

fn assert_native_withheld(service: &Service, address: &KnowledgeAddress, needle: &str) {
    // A truthful current-owner refusal may remain an error. It must never
    // become a successful copied payload or a declaration of Source absence.
    if let Ok(reading) = service.knowledge_read(address) {
        assert!(reading.content.is_none(), "{reading:?}");
    }
    assert!(service.knowledge_read_document(address).is_err());
    assert!(service.knowledge_relations(address, 1, 32, 64).is_err());
    if let Ok(search) = service.knowledge_search(needle, 32) {
        assert!(!search
            .hits
            .iter()
            .any(|hit| hit.resource == address.resource_ref()));
        assert!(!serde_json::to_string(&search).unwrap().contains(needle));
    }
    if let Ok(frame) = service.knowledge_frame(None, &[address.clone()]) {
        assert!(!serde_json::to_string(&frame).unwrap().contains(needle));
    }
}

#[test]
#[ignore = "explicit native integration requires SAME pinned ctrl, selected real Redis and actual ripgrep"]
fn real_redis_cannot_override_current_now_provider_read_or_withdrawal() {
    assert!(
        aikit_adapters::ripgrep::available(),
        "actual ripgrep is required, never a green skip"
    );
    let ctrl = selected_native_owner();
    let redis_address = std::env::var("AIKIT_TEST_REDIS_ADDR")
        .expect("selected real Redis gate requires its address");
    assert!(
        std::env::var_os("AIKIT_WORLD_REDIS_CONFIG").is_none(),
        "owned gate must not inherit another Redis config"
    );
    assert!(
        std::env::var_os("AIKIT_KNOWLEDGE_RESULT_CACHE").is_none(),
        "retired result-cache switch is not an admission input"
    );
    let owned = native_tempdir();
    let world = owned.path().join("world");
    fs::create_dir(&world).unwrap();
    // These commands own only this native test scratch. They establish actual
    // owner state/identities, not a model launch, allocation or Factory worker.
    native_owner_action(&ctrl, &world, "central.init", serde_json::json!({}));
    let returned = native_owner_action(
        &ctrl,
        &world,
        "projectcentral.now.return",
        serde_json::json!({
            "actor":"test:knowledge-current-native-owner", "kind":"note",
            "subject":"Native R4 current owner source", "result":OLD_BODY, "status":"active",
        }),
    );
    let relative = returned["source"]
        .as_str()
        .expect("native Return supplies its actual path");
    let file = world.join(relative);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&file).unwrap()).unwrap(),
        returned["handoff"]
    );
    let inspected = native_owner_action(
        &ctrl,
        &world,
        "central.file-map.inspect",
        serde_json::json!({"resources":false}),
    );
    let registered = native_owner_action(
        &ctrl,
        &world,
        "central.file-map.register",
        serde_json::json!({
            "path":relative, "expected_revision":inspected["revision"],
        }),
    );
    let native_ref = registered["source_ref"]
        .as_str()
        .expect("actual registration supplies Source identity")
        .to_owned();
    let located = native_owner_action(
        &ctrl,
        &world,
        "central.file-map.locate",
        serde_json::json!({"path":file}),
    );
    assert_eq!(located["source"]["ref"], native_ref);
    let old = native_owner_action(
        &ctrl,
        &world,
        "central.file-map.resolve",
        serde_json::json!({"source_ref":native_ref,"content":true}),
    );
    assert_eq!(old["source"]["ref"], native_ref);
    assert_eq!(old["revision"], located["revision"]);
    assert_eq!(old["source"]["agent_retrieval_allowed"], true);
    assert!(old["content"].as_str().unwrap().contains("R4oldCedar"));
    let address = KnowledgeAddress::Source(SourceRef::parse(&native_ref).unwrap());
    let home = AikitHome::at(owned.path().join("home"));
    home.ensure_layout().unwrap();
    let config = RedisNowConfig {
        schema: NOW_REDIS_CONFIG_SCHEMA.into(),
        address: redis_address,
        database: 0,
        key_prefix: format!("aikit-r4-current-{}", ulid::Ulid::generate()),
        username: None,
        credential_ref: None,
        allow_remote: false,
        connect_timeout_ms: 1000,
        io_timeout_ms: 1000,
        prepared_ttl_seconds: 3600,
        coordination_retention_seconds: 3600,
    };
    let store = KnowledgeCacheStore::new(config.clone()).unwrap();
    assert!(
        store
            .status(None)
            .expect("real Redis must be available")
            .available
    );
    fs::write(
        home.root().join("redis-now.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let open_service = || {
        Service::open(home.clone(), &world, |key| {
            (key == "CENTRAL_ROOT").then(|| world.display().to_string())
        })
        .unwrap()
    };
    let service = open_service();
    assert_current_native_read(&service, &address, &old);
    let old_search = service.knowledge_search("R4oldCedar", 32).unwrap();
    assert!(
        old_search
            .hits
            .iter()
            .any(|hit| hit.resource == address.resource_ref()),
        "actual native NOW rg discovery must select its current Source: {old_search:?}"
    );
    let cli_read = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
        command
            .args([
                "--json",
                "knowledge",
                "read",
                &format!("source={native_ref}"),
            ])
            .current_dir(&world)
            .env("AIKIT_HOME", home.root())
            .env("HOME", owned.path())
            .env("CENTRAL_ROOT", &world)
            .env("CENTRAL_CTRL_BIN", &ctrl)
            .env_remove("OI_CENTRAL_CTRL_BIN")
            .env_remove("AIKIT_CENTRAL_ROOT")
            .env_remove("OI_CENTRAL_ROOT")
            .env(
                "AIKIT_WORLD_REDIS_CONFIG",
                home.root().join("redis-now.json"),
            )
            .env_remove("AIKIT_KNOWLEDGE_RESULT_CACHE");
        bounded_output(&mut command)
    };
    let initial_cli = cli_read();
    assert!(initial_cli.ok(), "{}", initial_cli.stderr);
    assert!(initial_cli.stdout.contains("R4oldCedar"));
    // A real owned external-file edit challenges current native admission and
    // revision. It is not described as a native accepted mutation or fake reply.
    let mut changed: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    changed["result"] = serde_json::json!(NEW_BODY);
    fs::write(&file, serde_json::to_vec(&changed).unwrap()).unwrap();
    let current = native_owner_action(
        &ctrl,
        &world,
        "central.file-map.resolve",
        serde_json::json!({"source_ref":native_ref,"content":true}),
    );
    assert_eq!(current["source"]["ref"], native_ref);
    assert_ne!(current["revision"], old["revision"]);
    assert_current_native_read(&service, &address, &current);
    assert_current_native_read(&open_service(), &address, &current);
    assert!(service
        .knowledge_search("R4newAlder", 32)
        .unwrap()
        .hits
        .iter()
        .any(|hit| hit.resource == address.resource_ref()));
    let current_cli = cli_read();
    assert!(current_cli.ok(), "{}", current_cli.stderr);
    assert!(
        current_cli.stdout.contains("R4newAlder") && !current_cli.stdout.contains("R4oldCedar")
    );
    let retained = fs::read(&file).unwrap();
    let marker = file.parent().unwrap().join(".no-agent-retrieval");
    fs::write(&marker, "owner withdrawal\n").unwrap();
    assert_native_withheld(&service, &address, "R4newAlder");
    assert_native_withheld(&open_service(), &address, "R4newAlder");
    let withheld = cli_read();
    assert!(!withheld.ok() && !withheld.stdout.contains("R4newAlder"));
    assert_eq!(fs::read(&file).unwrap(), retained);
    assert!(
        store.status(None).unwrap().available,
        "source withdrawal did not disable shared Redis"
    );
    fs::remove_file(&marker).unwrap();
    assert_current_native_read(&service, &address, &current);
    assert_current_native_read(&open_service(), &address, &current);
    assert!(
        cli_read().ok(),
        "restored current admission must restore the actual native route"
    );
    assert_eq!(fs::read(&file).unwrap(), retained);
    assert!(store.status(None).unwrap().available);
}

#[test]
fn configured_missing_native_executable_cannot_be_replaced_by_independent_control_records() {
    use aikit_adapters::now_field::{default_runner, NowFieldScope, NowFieldSourcePoolProvider};
    use aikit_core::knowledge_source_pool::SourcePoolProvider;
    let owned = native_tempdir();
    let world = owned.path().join("world");
    let records = world.join("Control/agents/now/agents");
    fs::create_dir_all(&records).unwrap();
    fs::create_dir_all(world.join("Control/user")).unwrap();
    fs::create_dir_all(world.join("Work")).unwrap();
    let record = records.join("independent-record.json");
    fs::write(
        &record,
        serde_json::to_vec(&serde_json::json!({"result":OLD_BODY})).unwrap(),
    )
    .unwrap();
    let before = fs::read(&record).unwrap();
    // This explicitly declared independent Control provider remains useful.
    // Its real descriptor is no evidence of a native Central connection.
    let provider = NowFieldSourcePoolProvider::connect(
        default_runner(&world),
        aikit_adapters::ripgrep::executable(),
        NowFieldScope::standard(&world),
    )
    .unwrap();
    let material = provider
        .descriptors()
        .into_iter()
        .find(|row| row.body.contains(OLD_BODY))
        .expect("actual independent Control record is readable");
    assert_eq!(
        provider
            .read(&material.binding.source)
            .unwrap()
            .unwrap()
            .body,
        material.body
    );
    let missing = owned.path().join("missing-selected-ctrl");
    assert!(!missing.exists());
    let home = owned.path().join("home");
    let invoke = |verb: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
        command.args(["--json", "knowledge", verb]);
        if verb == "read" {
            command.arg(format!("source={}", material.binding.source));
        }
        command
            .current_dir(&world)
            .env("AIKIT_HOME", &home)
            .env("HOME", owned.path())
            .env("CENTRAL_ROOT", &world)
            .env("CENTRAL_CTRL_BIN", &missing)
            .env_remove("OI_CENTRAL_CTRL_BIN")
            .env_remove("AIKIT_CENTRAL_ROOT")
            .env_remove("OI_CENTRAL_ROOT")
            .env_remove("AIKIT_WORLD_REDIS_CONFIG")
            .env_remove("AIKIT_KNOWLEDGE_RESULT_CACHE");
        bounded_output(&mut command)
    };
    let read = invoke("read");
    assert!(!read.ok() && !read.stdout.contains(OLD_BODY));
    let status = invoke("status");
    assert!(
        status.ok(),
        "an unavailable provider is truthful status data: {}",
        status.stderr
    );
    assert!(
        status.stdout.contains("Central file map unavailable"),
        "{}",
        status.stdout
    );
    assert!(
        status
            .stdout
            .contains("NOW-field search unavailable: native Central file map is unavailable"),
        "{}",
        status.stdout
    );
    assert!(!status.stdout.contains(OLD_BODY));
    assert_eq!(fs::read(&record).unwrap(), before);
    assert!(!missing.exists());
}

#[test]
fn an_explicit_missing_native_owner_keeps_actual_notfound_instead_of_standalone() {
    use std::error::Error;
    let ground = Ground::standalone();
    let missing = ground
        .owned
        .path()
        .join("configured-native-owner-is-missing");
    let before = fs::read(&ground.source).unwrap();
    let error = match Service::open(AikitHome::at(&ground.home), &ground.project, |key| {
        (key == "CENTRAL_ROOT").then(|| missing.display().to_string())
    }) {
        Ok(_) => panic!("an unavailable explicit native owner cannot become standalone"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "central.root_context_unavailable");
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
    assert!(cause.raw_os_error().is_some());
    assert_eq!(fs::read(&ground.source).unwrap(), before);
    assert!(!missing.exists());
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
    command
        .args(["--json", "knowledge", "read", &format!("source={SOURCE}")])
        .current_dir(&ground.project)
        .env("AIKIT_HOME", &ground.home)
        .env("HOME", ground.owned.path())
        .env("CENTRAL_ROOT", &missing)
        .env_remove("OI_CENTRAL_ROOT")
        .env_remove("AIKIT_CENTRAL_ROOT");
    let output = bounded_output(&mut command);
    assert!(!output.ok());
    let envelope: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(
        envelope["error"]["code"],
        "central.root_context_unavailable"
    );
    assert!(!output.stdout.contains(OLD_BODY));
    assert_eq!(fs::read(&ground.source).unwrap(), before);
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn same_service_rechecks_removed_configured_alias_without_using_the_old_canonical_owner() {
    use std::error::Error;
    let ground = Ground::standalone();
    let owner = ground.owned.path().join("native-owner");
    fs::create_dir_all(owner.join("Control")).unwrap();
    fs::create_dir_all(owner.join("Work")).unwrap();
    let alias = ground.owned.path().join("selected-owner-alias");
    std::os::unix::fs::symlink(&owner, &alias).unwrap();
    let service = Service::open(AikitHome::at(&ground.home), &ground.project, |key| {
        (key == "CENTRAL_ROOT").then(|| alias.display().to_string())
    })
    .unwrap();
    let before = fs::read(&ground.source).unwrap();
    fs::remove_file(&alias).unwrap();
    let error = service.knowledge_status().unwrap_err();
    assert_eq!(error.code(), "central.root_context_unavailable");
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
    assert!(cause.raw_os_error().is_some());
    assert!(service.knowledge_read(&address()).is_err());
    assert_eq!(fs::read(&ground.source).unwrap(), before);
    assert!(owner.join("Control").is_dir() && owner.join("Work").is_dir());
    // This is the retained configured physical locator, not proof of a native
    // owner reply or a retained semantic Project/World identity.
}

#[test]
fn genuinely_absent_optional_home_hint_keeps_declared_standalone_service_useful() {
    let ground = Ground::standalone();
    let before = fs::read(&ground.source).unwrap();
    let service = Service::open(AikitHome::at(&ground.home), &ground.project, |key| {
        (key == "HOME").then(|| ground.owned.path().display().to_string())
    })
    .unwrap();
    assert!(service.descriptor().project_root.is_some());
    assert!(!ground.owned.path().join("Central").exists());
    // Explicit already-authorised pure material compatibility is exercised
    // above; service construction does not grant this generic copy eligibility.
    assert_eq!(fs::read(&ground.source).unwrap(), before);
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[ignore = "explicit qualification requires actual nonroot filesystem denial"]
fn actual_nonroot_known_owner_privacy_failure_retains_original_permission_cause() {
    use std::error::Error;
    use std::os::unix::fs::PermissionsExt;
    let ground = Ground::standalone();
    let owner = ground.owned.path().join("native-owner");
    fs::create_dir_all(owner.join("Control")).unwrap();
    fs::create_dir_all(owner.join("Work")).unwrap();
    let service = Service::open(AikitHome::at(&ground.home), &ground.project, |key| {
        (key == "CENTRAL_ROOT").then(|| owner.display().to_string())
    })
    .unwrap();
    struct Restore {
        path: PathBuf,
        permissions: fs::Permissions,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            fs::set_permissions(&self.path, self.permissions.clone()).unwrap();
        }
    }
    let restore = Restore {
        path: owner.clone(),
        permissions: fs::metadata(&owner).unwrap().permissions(),
    };
    let before = fs::read(&ground.source).unwrap();
    fs::set_permissions(&owner, fs::Permissions::from_mode(0o0)).unwrap();
    let actual = fs::symlink_metadata(owner.join("Control"))
        .expect_err("selected nonroot gate must observe actual denial, never green skip");
    assert_eq!(actual.kind(), std::io::ErrorKind::PermissionDenied);
    let error = service.knowledge_status().unwrap_err();
    let cause = error
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert_eq!(cause.kind(), actual.kind());
    assert_eq!(cause.raw_os_error(), actual.raw_os_error());
    assert_eq!(fs::read(&ground.source).unwrap(), before);
    drop(restore);
    assert!(owner.join("Control").is_dir() && owner.join("Work").is_dir());
}

fn current_corpus(ground: &Ground) -> (PathBuf, PathBuf, String) {
    let corpus = ground.project.join("selected-current-corpus");
    fs::create_dir(&corpus).unwrap();
    let record = corpus.join("record.md");
    let body = "---\nrecord_id: r4-current-alias\nrecord_type: note\nsource_ids: [r4-current-citation]\ntags: [current-proof]\n---\n\n# Current selected record\nR4corpusCedar exact current record.\n".to_owned();
    fs::write(&record, &body).unwrap();
    fs::write(corpus.join("source.md"), "---\nsource_id: r4-current-citation\nrecord_type: book\n---\n\n# Current citation\nR4citationAlder initial basis.\n").unwrap();
    (corpus, record, body)
}

fn corpus_service(ground: &Ground, corpus: &std::path::Path) -> Service {
    ground
        .service()
        .with_current_corpus_selection(aikit_cli::app::CurrentCorpusSelection {
            corpus: corpus.to_path_buf(),
            extension: "md".into(),
            room_depth: 1,
        })
        .unwrap()
}

fn corpus_cli(ground: &Ground, corpus: &std::path::Path, source: &SourceRef) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
    command
        .args([
            "--json",
            "knowledge",
            "--source-corpus",
            corpus.to_str().unwrap(),
            "read",
            &format!("source={source}"),
        ])
        .current_dir(&ground.project)
        .env("AIKIT_HOME", &ground.home)
        .env("HOME", ground.owned.path())
        .env_remove("CENTRAL_ROOT")
        .env_remove("AIKIT_CENTRAL_ROOT")
        .env_remove("OI_CENTRAL_ROOT")
        .env_remove("AIKIT_WORLD_REDIS_CONFIG")
        .env_remove("AIKIT_KNOWLEDGE_RESULT_CACHE");
    bounded_output(&mut command)
}

#[test]
fn explicit_current_corpus_reads_same_service_restart_cli_and_current_citation_basis() {
    let ground = Ground::standalone();
    let (corpus, record, body) = current_corpus(&ground);
    let source = SourceRef::parse("central:source:corpus:r4-current-alias").unwrap();
    let address = KnowledgeAddress::Source(source.clone());
    // A discovered alias-shaped JSON or a prior compiler shard is historical;
    // only an explicit current IO selection supplies the owner relation.
    let mut absent = ground.service();
    assert_withheld(&mut absent, &address, "R4corpusCedar");
    let selected = corpus_service(&ground, &corpus);
    let current = selected.knowledge_read(&address).unwrap();
    assert_eq!(current.content.as_deref(), Some(body.as_str()));
    assert_eq!(
        current.revision.as_deref(),
        Some(aikit_core::knowledge_ingest::corpus_content_revision(body.as_bytes()).as_str())
    );
    let search = selected.knowledge_search("R4corpusCedar", 8).unwrap();
    assert!(search
        .hits
        .iter()
        .any(|hit| hit.resource == address.resource_ref()));
    assert_eq!(
        corpus_service(&ground, &corpus)
            .knowledge_read(&address)
            .unwrap(),
        current
    );
    let cli = corpus_cli(&ground, &corpus, &source);
    assert!(cli.ok(), "{}", cli.stderr);
    let reply: serde_json::Value = serde_json::from_str(&cli.stdout).unwrap();
    assert!(reply["ok"] == true);
    assert!(cli.stdout.contains("R4corpusCedar"));
    let citation = corpus.join("source.md");
    fs::write(&citation, "---\nsource_id: r4-current-citation\nrecord_type: book\n---\n\n# Current citation\nR4citationBirch changed basis without changing the record.\n").unwrap();
    let changed = selected.knowledge_read(&address).unwrap();
    assert_eq!(changed.content.as_deref(), Some(body.as_str()));
    assert_eq!(changed.revision, current.revision);
    // The retained single-operation pool basis assertion is exercised by the
    // real-filesystem unit case; this API intentionally returns a reading.
    assert_eq!(
        corpus_service(&ground, &corpus)
            .knowledge_read(&address)
            .unwrap(),
        changed
    );
    assert_eq!(fs::read_to_string(&record).unwrap(), body);
    fs::write(
        corpus.join(".no-agent-retrieval"),
        "actual selected corpus withdrawn\n",
    )
    .unwrap();
    assert_eq!(
        selected.knowledge_read(&address).unwrap_err().code(),
        "knowledge.ingest_corpus_withheld"
    );
    let denied = corpus_cli(&ground, &corpus, &source);
    assert!(!denied.ok());
    assert!(!denied.stdout.contains("R4corpusCedar"));
    assert_eq!(fs::read_to_string(record).unwrap(), body);
}

#[test]
fn copied_native_and_declared_corpus_origin_claims_do_not_acquire_current_io_ownership() {
    let ground = Ground::standalone();
    let (corpus, _, _) = current_corpus(&ground);
    let source = SourceRef::parse("central:source:corpus:r4-current-alias").unwrap();
    let address = KnowledgeAddress::Source(source.clone());
    let admitted = corpus_service(&ground, &corpus)
        .knowledge_read(&address)
        .unwrap();
    let record_body = admitted.content.clone().unwrap();
    let citation = String::from_utf8(
        aikit_adapters::wiki_publication::material_bytes(
            &corpus.join("source.md"),
            16 * 1024 * 1024,
        )
        .unwrap(),
    )
    .unwrap();
    let selected = aikit_core::knowledge_ingest::select_ingestable_records(&[
        ("record.md".into(), record_body),
        ("source.md".into(), citation),
    ]);
    let origins = selected
        .records
        .iter()
        .chain(&selected.sources)
        .map(|(relative, body)| {
            (
                relative.clone(),
                aikit_core::knowledge_ingest::IngestOriginBinding {
                    origin: aikit_core::knowledge_source_pool::SourceOrigin::declared_corpus(),
                    content_revision: SourceRevision::parse(
                        aikit_core::knowledge_ingest::corpus_content_revision(body.as_bytes()),
                    )
                    .unwrap(),
                    visibility: aikit_core::knowledge_source_pool::SourceVisibility::Team,
                    owners: Vec::new(),
                },
            )
        })
        .collect();
    let compiled = aikit_core::knowledge_ingest::ingest_corpus_with_origins(
        &selected.records,
        &selected.sources,
        1,
        &origins,
    )
    .unwrap();
    let material = compiled
        .material
        .iter()
        .find(|item| item.binding.source == source)
        .unwrap();
    // This really generated shard has a legitimate producer/basis. Its disk
    // copy still supplies no current selected corpus attachment to this Service.
    fs::write(&ground.source, serde_json::to_vec(&material).unwrap()).unwrap();
    let copy = fs::read(&ground.source).unwrap();
    assert_withheld(&mut ground.service(), &address, "R4corpusCedar");
    assert!(!ground.cli(source.as_str()).ok());
    // A well-formed native-origin carrier copied from actual native IO is
    // covered by the selected real-Ctrl case below; a prefix is never proof.
    assert_eq!(fs::read(&ground.source).unwrap(), copy);
    let mut unselected = ground.service();
    assert_eq!(
        corpus_service(&ground, &corpus)
            .knowledge_read(&address)
            .unwrap(),
        admitted
    );
    fs::write(
        corpus.join(".no-agent-retrieval"),
        "actual selected input withdrawn",
    )
    .unwrap();
    assert_withheld(&mut unselected, &address, "R4corpusCedar");
    assert_withheld(&mut ground.service(), &address, "R4corpusCedar");
    let withdrawn = ground.cli(source.as_str());
    assert!(!withdrawn.ok());
    assert!(!withdrawn.stdout.contains("R4corpusCedar"));
    assert!(!withdrawn.stderr.contains("R4corpusCedar"));
    assert_eq!(fs::read(&ground.source).unwrap(), copy);
}

#[test]
fn selected_current_corpus_refuses_real_oversize_input_without_output_or_truncation() {
    let ground = Ground::standalone();
    let (corpus, record, _) = current_corpus(&ground);
    let file = fs::OpenOptions::new().write(true).open(&record).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    let length = file.metadata().unwrap().len();
    let service = corpus_service(&ground, &corpus);
    let source = KnowledgeAddress::Source(
        SourceRef::parse("central:source:corpus:r4-current-alias").unwrap(),
    );
    assert_eq!(
        service.knowledge_read(&source).unwrap_err().code(),
        "knowledge.wiki_publication_budget"
    );
    assert_eq!(fs::metadata(&record).unwrap().len(), length);
    assert!(!ground.project.join("output.sources").exists());
}

#[cfg(unix)]
#[test]
fn selected_current_corpus_refuses_real_fifo_without_a_writer_or_indefinite_open() {
    let ground = Ground::standalone();
    let (corpus, record, _) = current_corpus(&ground);
    fs::remove_file(&record).unwrap();
    let mut make_fifo = Command::new("mkfifo");
    make_fifo.arg(&record);
    assert!(
        bounded_output(&mut make_fifo).ok(),
        "actual supported-host FIFO fixture is required"
    );
    let started = Instant::now();
    let service = corpus_service(&ground, &corpus);
    let source = KnowledgeAddress::Source(
        SourceRef::parse("central:source:corpus:r4-current-alias").unwrap(),
    );
    assert_eq!(
        service.knowledge_read(&source).unwrap_err().code(),
        "knowledge.wiki_publication_identity"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    use std::os::unix::fs::FileTypeExt;
    assert!(fs::symlink_metadata(&record).unwrap().file_type().is_fifo());
}

#[test]
#[ignore = "explicit native integration requires SAME built/pinned corrected Ctrl and actual current file-map owner"]
fn actual_native_current_corpus_alias_retains_issued_ref_body_basis_target_and_withdrawal() {
    use aikit_core::knowledge_source_pool::SourceOriginKind;
    let ctrl = selected_native_owner();
    let owned = native_tempdir();
    let world = owned.path().join("world");
    fs::create_dir(&world).unwrap();
    native_owner_action(&ctrl, &world, "central.init", serde_json::json!({}));
    let relative = "Control/user/current-corpus/record.md";
    let record = world.join(relative);
    fs::create_dir_all(record.parent().unwrap()).unwrap();
    let body = "---\nrecord_id: native-current-alias\nrecord_type: note\n---\n\n# Actual current native record\nR4nativeAliasCedar real current body.\n";
    fs::write(&record, body).unwrap();
    let issued = "issued:qualification:opaque-current-corpus";
    let relations = world.join("Control/relations/source-relations.json");
    fs::create_dir_all(relations.parent().unwrap()).unwrap();
    fs::write(&relations, serde_json::to_vec(&serde_json::json!({
        "schema":"central.control.ground-relations/v1", "project_id":"control:root",
        "relations":[{"ref":issued,"path":relative,"roles":["controlled-corpus-input"],
            "provenance":"human-adopted","standing":"architecture-contract","treatment":"control-user"}]
    })).unwrap()).unwrap();
    let inspected = native_owner_action(
        &ctrl,
        &world,
        "central.file-map.inspect",
        serde_json::json!({"resources":false}),
    );
    let registered = native_owner_action(
        &ctrl,
        &world,
        "central.file-map.register",
        serde_json::json!({
            "path":relative,"source_ref":issued,"expected_revision":inspected["revision"]
        }),
    );
    assert_eq!(registered["source_ref"], issued);
    let located = native_owner_action(
        &ctrl,
        &world,
        "central.file-map.locate",
        serde_json::json!({"path":record,"binding_only":true}),
    );
    assert_eq!(located["source"]["ref"], issued);
    assert_eq!(located["ownership"], "owned");
    let native = native_owner_action(
        &ctrl,
        &world,
        "central.file-map.resolve",
        serde_json::json!({"source_ref":issued,"content":true}),
    );
    let open = || {
        Service::open(AikitHome::at(owned.path().join("home")), &world, |key| {
            (key == "CENTRAL_ROOT").then(|| world.display().to_string())
        })
        .unwrap()
        .with_current_corpus_selection(aikit_cli::app::CurrentCorpusSelection {
            corpus: record.parent().unwrap().to_path_buf(),
            extension: "md".into(),
            room_depth: 1,
        })
        .unwrap()
    };
    let service = open();
    let alias = KnowledgeAddress::Source(
        SourceRef::parse("central:source:corpus:native-current-alias").unwrap(),
    );
    let reading = service.knowledge_read(&alias).unwrap();
    assert_eq!(reading.content.as_deref(), native["content"].as_str());
    let owner = aikit_adapters::central_file_map::CentralFileMapProvider::connect(
        SystemRunner::new()
            .with_timeout(Duration::from_secs(30))
            .with_strict_utf8(),
        &ctrl,
        &world,
        None,
    )
    .unwrap();
    use aikit_core::knowledge_source_pool::SourcePoolProvider;
    let actual = owner
        .read_for(
            &SourceRef::parse(issued).unwrap(),
            aikit_core::context_source::RetrievalTarget::LocalAgent,
        )
        .unwrap()
        .unwrap();
    let selected = aikit_core::knowledge_ingest::select_ingestable_records(&[(
        "record.md".into(),
        actual.material.body.clone(),
    )]);
    let origins = std::collections::BTreeMap::from([(
        "record.md".into(),
        aikit_core::knowledge_ingest::IngestOriginBinding {
            origin: actual.material.binding.source_origin().unwrap().unwrap(),
            content_revision: SourceRevision::parse(
                aikit_core::knowledge_ingest::corpus_content_revision(
                    actual.material.body.as_bytes(),
                ),
            )
            .unwrap(),
            visibility: actual.material.binding.visibility,
            owners: actual.material.binding.owners.clone(),
        },
    )]);
    let privacy = std::collections::BTreeMap::from([("record.md".into(), actual.privacy)]);
    let (compiled, admitted_privacy) =
        aikit_core::knowledge_ingest::compile_corpus_for_current_read(
            &selected.records,
            &selected.sources,
            1,
            &origins,
            &privacy,
            aikit_core::context_source::RetrievalTarget::LocalAgent,
        )
        .unwrap();
    assert_eq!(admitted_privacy, actual.privacy);
    let material = &compiled.material[0];
    assert_eq!(
        reading.revision.as_deref(),
        Some(material.binding.revision.as_str())
    );
    assert_eq!(
        material.binding.revision.as_str(),
        aikit_core::knowledge_ingest::corpus_content_revision(material.body.as_bytes())
    );
    let origin = material.binding.source_origin().unwrap().unwrap();
    match origin.origin {
        SourceOriginKind::NativeSource {
            world_ref,
            source,
            observed_binding,
        } => {
            assert_eq!(world_ref, "control:root");
            assert_eq!(source.source.as_str(), issued);
            assert_eq!(
                source.revision.as_ref().unwrap().as_str(),
                native["revision"].as_str().unwrap()
            );
            assert_eq!(
                observed_binding.roles,
                serde_json::from_value::<Vec<String>>(native["source"]["roles"].clone()).unwrap()
            );
            assert_eq!(observed_binding.agent_retrieval_allowed, true);
        }
        _ => {
            panic!("actual participating native input cannot become an independent declared corpus")
        }
    }
    assert_eq!(open().knowledge_read(&alias).unwrap(), reading);
    // A genuine owner-issued carrier on disk cannot replace its current route.
    let copy = world.join("retained-native-alias.json");
    fs::write(&copy, serde_json::to_vec(&material).unwrap()).unwrap();
    let copy_bytes = fs::read(&copy).unwrap();
    let unselected = Service::open(
        AikitHome::at(owned.path().join("unselected-home")),
        &world,
        |key| (key == "CENTRAL_ROOT").then(|| world.display().to_string()),
    )
    .unwrap();
    assert!(unselected.knowledge_read(&alias).is_err());
    let unselected_cli = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
        command
            .args([
                "--json",
                "knowledge",
                "read",
                "source=central:source:corpus:native-current-alias",
            ])
            .current_dir(&world)
            .env("AIKIT_HOME", owned.path().join("unselected-cli-home"))
            .env("HOME", owned.path())
            .env("CENTRAL_ROOT", &world)
            .env("CENTRAL_CTRL_BIN", &ctrl)
            .env_remove("AIKIT_CENTRAL_ROOT")
            .env_remove("OI_CENTRAL_ROOT")
            .env_remove("AIKIT_WORLD_REDIS_CONFIG")
            .env_remove("AIKIT_KNOWLEDGE_RESULT_CACHE");
        bounded_output(&mut command)
    };
    let copied_cli = unselected_cli();
    assert!(!copied_cli.ok());
    assert!(!copied_cli.stdout.contains("R4nativeAliasCedar"));
    let before = fs::read(&record).unwrap();
    fs::write(
        record.parent().unwrap().join(".no-agent-retrieval"),
        "actual native ancestor withdrew\n",
    )
    .unwrap();
    assert_eq!(
        service.knowledge_read(&alias).unwrap_err().code(),
        "knowledge.ingest_corpus_withheld"
    );
    assert_eq!(
        open().knowledge_read(&alias).unwrap_err().code(),
        "knowledge.ingest_corpus_withheld"
    );
    assert!(unselected.knowledge_read(&alias).is_err());
    let fresh_unselected = Service::open(
        AikitHome::at(owned.path().join("fresh-unselected-home")),
        &world,
        |key| (key == "CENTRAL_ROOT").then(|| world.display().to_string()),
    )
    .unwrap();
    assert!(fresh_unselected.knowledge_read(&alias).is_err());
    let copied_cli = unselected_cli();
    assert!(!copied_cli.ok());
    assert!(!copied_cli.stdout.contains("R4nativeAliasCedar"));
    assert!(!copied_cli.stderr.contains("R4nativeAliasCedar"));
    assert_eq!(fs::read(&record).unwrap(), before);
    assert_eq!(fs::read(copy).unwrap(), copy_bytes);
}

#[test]
#[ignore = "explicit native integration requires SAME corrected Ctrl, real BKMR 7.6.7 and real ripgrep"]
fn actual_root_service_and_cli_project_query_exclude_sibling_native_payloads_before_delivery() {
    let ctrl = selected_native_owner();
    assert!(
        aikit_adapters::ripgrep::available(),
        "actual selected query requires real ripgrep"
    );
    let owned = native_tempdir();
    let world = owned.path().join("world");
    fs::create_dir(&world).unwrap();
    native_owner_action(&ctrl, &world, "central.init", serde_json::json!({}));
    let mut refs = Vec::new();
    for member in ["alpha", "beta"] {
        fs::create_dir_all(world.join("Work").join(member)).unwrap();
        native_owner_action(
            &ctrl,
            &world,
            "projectcentral.init",
            serde_json::json!({"project":member,"project_id":format!("scope:{member}")}),
        );
        let path = world
            .join("Work")
            .join(member)
            .join("ProjectCentral/user/current-query.md");
        fs::write(
            &path,
            format!("# Native current {member}\nR4serviceScopeCedar {member} actual payload.\n"),
        )
        .unwrap();
        let selected = native_owner_action(
            &ctrl,
            &world,
            "central.file-map.locate",
            serde_json::json!({"project":member,"federated":false,"path":path,"binding_only":true}),
        );
        assert_eq!(selected["ownership"], "owned");
        refs.push(selected["source"]["ref"].as_str().unwrap().to_owned());
        native_owner_action(
            &ctrl,
            &world,
            "central.file-map.refresh",
            serde_json::json!({"project":member,"embeddings":false}),
        );
    }
    let home = AikitHome::at(owned.path().join("home"));
    let service = Service::open(home.clone(), &world, |key| {
        (key == "CENTRAL_ROOT").then(|| world.display().to_string())
    })
    .unwrap();
    let global = service.knowledge_search("R4serviceScopeCedar", 32).unwrap();
    for source in &refs {
        assert!(global
            .hits
            .iter()
            .any(|hit| hit.resource.as_str() == source));
    }
    let selected = service
        .knowledge_search(": alpha R4serviceScopeCedar", 32)
        .unwrap();
    assert!(selected
        .hits
        .iter()
        .any(|hit| hit.resource.as_str() == refs[0]));
    assert!(!selected
        .hits
        .iter()
        .any(|hit| hit.resource.as_str() == refs[1]));
    assert!(!serde_json::to_string(&selected)
        .unwrap()
        .contains("beta actual payload"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
    command
        .args([
            "--json",
            "knowledge",
            "search",
            ": alpha R4serviceScopeCedar",
        ])
        .current_dir(&world)
        .env("CENTRAL_ROOT", &world)
        .env("CENTRAL_CTRL_BIN", &ctrl)
        .env("AIKIT_HOME", home.root())
        .env("HOME", owned.path())
        .env_remove("AIKIT_CENTRAL_ROOT")
        .env_remove("OI_CENTRAL_ROOT")
        .env_remove("OI_CENTRAL_CTRL_BIN")
        .env_remove("AIKIT_WORLD_REDIS_CONFIG")
        .env_remove("AIKIT_KNOWLEDGE_RESULT_CACHE");
    let output = bounded_output(&mut command);
    assert!(output.ok(), "{}", output.stderr);
    let value: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    let hits = value["data"]["hits"].as_array().unwrap();
    assert!(hits.iter().any(|hit| hit["resource"] == refs[0]));
    assert!(!hits.iter().any(|hit| hit["resource"] == refs[1]));
    assert!(!output.stdout.contains("beta actual payload"));
    // Current admission stays a native owner fact after scoped preparation.
    fs::write(
        world.join("Work/alpha/ProjectCentral/user/.no-agent-retrieval"),
        "actual selected Source withdrew\n",
    )
    .unwrap();
    let current = service
        .knowledge_search(": alpha R4serviceScopeCedar", 32)
        .unwrap();
    assert!(!current
        .hits
        .iter()
        .any(|hit| hit.resource.as_str() == refs[0]));
    assert!(!current
        .hits
        .iter()
        .any(|hit| hit.resource.as_str() == refs[1]));
    assert!(!serde_json::to_string(&current)
        .unwrap()
        .contains("alpha actual payload"));
    assert_eq!(
        fs::read_to_string(world.join("Work/beta/ProjectCentral/user/current-query.md")).unwrap(),
        "# Native current beta\nR4serviceScopeCedar beta actual payload.\n"
    );
}
