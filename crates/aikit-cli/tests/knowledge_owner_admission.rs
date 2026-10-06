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
        .tempdir()
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

fn assert_native_withheld(service: &mut Service, address: &KnowledgeAddress, needle: &str) {
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
    let mut service = open_service();
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
    assert_native_withheld(&mut service, &address, "R4newAlder");
    assert_native_withheld(&mut open_service(), &address, "R4newAlder");
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
    fs::set_permissions(&owner, fs::Permissions::from_mode(0)).unwrap();
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
