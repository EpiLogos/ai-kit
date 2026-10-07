//! Public application -> native IPC -> selected protocol model -> real turn.
//! Removing the running owner must fail this path, not return a plan receipt.
use super::*;

fn composition(w: &World, target: &Value) -> Value {
    json!({
        "agency_admission":target["request"]["expected_agency"],
        "resident_target":{
            "space":"session-space/root", "agent_session":"agent-session/root",
            "socket":w.socket, "body":"root"
        }
    })
}

fn actual_pi_setup(w: &World) -> Value {
    let pi = std::env::var_os("AIKIT_CAW_PI_BIN")
        .map(PathBuf::from)
        .expect("Run with the exact installed Pi executable");
    assert_eq!(pi.file_name().and_then(|name| name.to_str()), Some("pi"));
    let mut binding = w.attach("root", "pi");
    let mut source: Value =
        serde_json::from_slice(&fs::read(&binding.agency_source.path).unwrap()).unwrap();
    source["determination"]["delegated_autonomy"]["allowed_action_refs"] =
        json!(["action/aikit/encounter-send", "action/aikit/model-realise"]);
    let bytes = serde_json::to_vec(&source).unwrap();
    fs::write(&binding.agency_source.path, &bytes).unwrap();
    binding.revision = rev("rev/2");
    binding.agency_source.revision = rev("rev/native-2");
    binding.agency_source.content_digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    w.cli(&[
        "encounter-agency-configure".into(),
        "--agent-session".into(),
        "agent-session/root".into(),
        "--binding-json".into(),
        serde_json::to_string(&binding).unwrap(),
        "--expected-revision".into(),
        "rev/1".into(),
    ]);
    publish_catalogue_fixture(
        w,
        &ModelCatalogueEntry {
            model: r("model:deepseek-v4-pro"),
            name: "DeepSeek V4 Pro".into(),
            description: "Exact native Pi selection receipt proof".into(),
            superseded_refs: Default::default(),
            routes: vec![DeclaredRoute {
                provider: ProviderRef::parse("provider:openrouter").unwrap(),
                kind: ModelRouteKind::ProviderNative,
                provider_native_ids: ["deepseek/deepseek-v4-pro".to_string()].into(),
                endpoint: None,
                credential: CredentialCondition::NotRequired,
            }],
            source: SourceRef::parse("source/actual-pi-selection-test").unwrap(),
            freshness: None,
            book: None,
        },
    );
    let policy = json!({
        "schema":"aikit.model-dispatch-policy/v1",
        "agent_ref":"agent:root",
        "world_ref":"central:root",
        "authority_ref":"authority:project:delegation",
        "bounds_refs":["bound:project:delegation"],
        "model_ref":"model:deepseek-v4-pro",
        "provider_ref":"provider:openrouter",
        "native_provider":"openrouter",
        "provider_native_id":"deepseek/deepseek-v4-pro",
        "expires_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() + 300_000,
        "credential":null,
    });
    let policy_path = w.temp.path().join("actual-pi-model-policy.json");
    let policy_bytes = serde_json::to_vec(&policy).unwrap();
    fs::write(&policy_path, &policy_bytes).unwrap();
    let provider = json!({
        "id":"root",
        "label":"Actual installed Pi, no inference",
        "protocol":"pi-rpc",
        "argv":[pi,"--mode","rpc","--no-extensions","--session-dir",w.temp.path().join("pi-sessions")],
        "model_policy":{
            "source":"source/actual-pi-model-policy",
            "revision":"rev/model-1",
            "path":policy_path,
            "content_digest":format!("blake3:{}",blake3::hash(&policy_bytes).to_hex()),
        }
    });
    w.cli(&[
        "encounter-configure".into(),
        "--provider-json".into(),
        provider.to_string(),
    ]);
    let admitted = admit_agency(
        &SystemRunner::new(),
        actuation().to_str().unwrap(),
        &binding.agency_source,
        &binding.agent_ref,
        &binding.world_ref,
    )
    .unwrap();
    json!({"action":"open-model","request":{
        "space":"session-space/root",
        "agent_session":"agent-session/root",
        "cwd":w.temp.path(),
        "model_ref":"model:deepseek-v4-pro",
        "provider_ref":"provider:openrouter",
        "body":"root",
        "expected_agency":admitted,
    }})
}

#[cfg(feature = "codex-account-native")]
fn actual_codex_setup(w: &World) -> EncounterAgencyBinding {
    let mut binding = w.attach("root", "acp");
    let mut source: Value =
        serde_json::from_slice(&fs::read(&binding.agency_source.path).unwrap()).unwrap();
    source["determination"]["delegated_autonomy"]["allowed_action_refs"] =
        json!(["action/aikit/encounter-send", "action/aikit/model-realise"]);
    let bytes = serde_json::to_vec(&source).unwrap();
    fs::write(&binding.agency_source.path, &bytes).unwrap();
    binding.revision = rev("rev/2");
    binding.agency_source.revision = rev("rev/native-2");
    binding.agency_source.content_digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    w.cli(&[
        "encounter-agency-configure".into(),
        "--agent-session".into(),
        "agent-session/root".into(),
        "--binding-json".into(),
        serde_json::to_string(&binding).unwrap(),
        "--expected-revision".into(),
        "rev/1".into(),
    ]);
    publish_catalogue_fixture(
        w,
        &ModelCatalogueEntry {
            model: r("model:gpt-5.6-luna"),
            name: "GPT-5.6 Luna".into(),
            description: "Actual Codex ACP selected-model receipt proof".into(),
            superseded_refs: Default::default(),
            routes: vec![DeclaredRoute {
                provider: ProviderRef::parse("provider:openai").unwrap(),
                kind: ModelRouteKind::ProviderNative,
                provider_native_ids: ["gpt-5.6-luna".to_string()].into(),
                endpoint: None,
                credential: CredentialCondition::Required {
                    hint: "Codex ChatGPT own-login, with no API key delivery".into(),
                },
            }],
            source: SourceRef::parse("source/actual-codex-selection-test").unwrap(),
            freshness: None,
            book: None,
        },
    );
    let policy = json!({
        "schema":"aikit.model-dispatch-policy/v1",
        "agent_ref":"agent:root",
        "world_ref":"central:root",
        "authority_ref":"authority:project:delegation",
        "bounds_refs":["bound:project:delegation"],
        "model_ref":"model:gpt-5.6-luna",
        "provider_ref":"provider:openai",
        "native_provider":"openai",
        "provider_native_id":"gpt-5.6-luna",
        "expires_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() + 300_000,
        "credential":null,
    });
    let policy_path = w.temp.path().join("actual-codex-model-policy.json");
    let policy_bytes = serde_json::to_vec(&policy).unwrap();
    fs::write(&policy_path, &policy_bytes).unwrap();
    w.cli(&[
        "encounter-configure".into(),
        "--provider-json".into(),
        json!({
            "id":"root",
            "label":"Actual installed Codex ACP, no inference",
            "protocol":"acp",
            "from_profile":"codex",
            "model_policy":{
                "source":"source/actual-codex-model-policy",
                "revision":"rev/model-1",
                "path":policy_path,
                "content_digest":format!("blake3:{}",blake3::hash(&policy_bytes).to_hex()),
            }
        })
        .to_string(),
    ]);
    binding
}

#[test]
#[cfg(feature = "codex-account-native")]
#[ignore = "requires real Codex ACP, an existing ChatGPT login and pinned Actuation; selects only, never prompts"]
fn actual_codex_acp_open_emits_factory_selection_without_inference() {
    let mut w = World::new();
    let binding = actual_codex_setup(&w);
    let service = aikit_cli::app::Service::open(w.home.clone(), w.temp.path(), |_| None).unwrap();
    start_model(&mut w, false);
    let admitted = admit_agency(
        &SystemRunner::new(),
        actuation().to_str().unwrap(),
        &binding.agency_source,
        &binding.agent_ref,
        &binding.world_ref,
    )
    .unwrap();
    let composed = json!({
        "agency_admission":admitted,
        "resident_target":{
            "space":"session-space/root",
            "agent_session":"agent-session/root",
            "socket":w.socket,
            "body":"root",
        }
    });
    let result = service
        .realise_model(
            &composed,
            "model:gpt-5.6-luna",
            Some("provider:openai"),
            None,
        )
        .unwrap();
    assert_eq!(result["selected"], true);
    assert_eq!(result["executed"], false);
    assert_eq!(result["resident"]["inference_observed"], false);
    assert_eq!(result["resident"]["protocol"], "acp");
    assert_eq!(result["resident"]["body_basis"]["harness_profile"], "codex");
    assert_eq!(
        result["resident"]["model_selection"]["credential_mode"],
        "codex-chatgpt-own-login"
    );
    assert!(
        result["resident"]["model_selection"]["codex_login_basis"]["program"]
            .as_str()
            .is_some_and(|program| std::path::Path::new(program).is_absolute())
    );
    assert_eq!(
        result["resident"]["model_observation"]["current_model_id"],
        "gpt-5.6-luna"
    );
    let selection = &result["factory_selection"];
    assert_eq!(selection["ranking_policy"], "EXPLICIT_PIN");
    assert_eq!(
        selection["ranking_explanation"]["harness_ref"],
        "harness/codex"
    );
    assert_eq!(
        selection["ranking_explanation"]["basis"]["composition_scope"]["kind"],
        "thin-native-codex-acp"
    );
    assert_eq!(
        selection["ranking_explanation"]["basis"]["native"]["native_session_id"],
        result["resident"]["native_session_id"]
    );
    assert_eq!(
        selection["ranking_explanation"]["basis"]["native"]["model_observation"],
        result["resident"]["model_observation"]
    );
    if let Some(path) = std::env::var_os("AIKIT_FACTORY_CODEX_SELECTION_FIXTURE_OUT") {
        fs::write(path, serde_json::to_vec_pretty(selection).unwrap()).unwrap();
    }
    w.stop();
    println!("ACTUAL_CODEX_ACP_FACTORY_SELECTION_EMITTED_WITHOUT_INFERENCE");
}

#[test]
#[ignore = "requires pinned Actuation and actual installed Pi; opens/configures only, never prompts"]
fn actual_pi_open_emits_factory_selection_without_inference() {
    let mut w = World::new();
    let target = actual_pi_setup(&w);
    let service = aikit_cli::app::Service::open(w.home.clone(), w.temp.path(), |_| None).unwrap();
    start_model(&mut w, false);
    let result = service
        .realise_model(
            &composition(&w, &target),
            "model:deepseek-v4-pro",
            Some("provider:openrouter"),
            None,
        )
        .unwrap();
    let selection = &result["factory_selection"];
    assert_eq!(selection["ranking_policy"], "EXPLICIT_PIN");
    assert_eq!(result["resident"]["inference_observed"], false);
    assert_eq!(
        selection["ranking_explanation"]["basis"]["composition_target_basis"]
            ["resident_body_basis"]["harness_profile"],
        "pi"
    );
    if let Some(path) = std::env::var_os("AIKIT_FACTORY_SELECTION_FIXTURE_OUT") {
        fs::write(path, serde_json::to_vec_pretty(selection).unwrap()).unwrap();
    }
    w.stop();
    println!("ACTUAL_PI_FACTORY_SELECTION_EMITTED_WITHOUT_INFERENCE");
}

#[test]
#[ignore = "requires pinned Actuation and actual installed Pi; explicit CLI native preparation, never prompts"]
fn explicit_compose_cli_admits_actual_pi_without_ambient_world_composition() {
    let mut w = World::new();
    let target = actual_pi_setup(&w);
    let composition = composition(&w, &target);
    let basis_path = w.temp.path().join("actual-pi-agency-basis.json");
    let target_path = w.temp.path().join("actual-pi-resident-target.json");
    let basis = &target["request"]["expected_agency"]["basis"];
    fs::write(&basis_path, serde_json::to_vec(basis).unwrap()).unwrap();
    fs::write(
        &target_path,
        serde_json::to_vec(&composition["resident_target"]).unwrap(),
    )
    .unwrap();
    let native = actuation();
    let mut paths = vec![native.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path = std::env::join_paths(paths).unwrap();
    start_model(&mut w, false);
    let invoke = |selected_target: &Path, selected_basis: &Path, model: &str, provider: &str| {
        Command::new(env!("CARGO_BIN_EXE_aikit"))
            .env("AIKIT_HOME", w.home.root())
            .env("PATH", &path)
            .env_remove("CAW_SOURCE_API_KEY")
            .args(["--json", "-C"])
            .arg(w.temp.path())
            .args(["compose", "--agency-source"])
            .arg(selected_basis)
            .args([
                "--agent",
                "agent:root",
                "--world",
                "central:root",
                "--realise",
                "--model",
                model,
                "--provider",
                provider,
                "--resident-target",
            ])
            .arg(selected_target)
            .output()
            .unwrap()
    };
    let output = invoke(
        &target_path,
        &basis_path,
        "model:deepseek-v4-pro",
        "provider:openrouter",
    );
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reading: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        reading["data"]["composition_scope"]["kind"],
        "explicit-native-resident"
    );
    assert_eq!(
        reading["data"]["composition_scope"]["ambient_components_claimed"],
        false
    );
    for field in [
        "basis",
        "agent_ref",
        "agency_ref",
        "world_binding_ref",
        "world_ref",
        "scope_ref",
    ] {
        assert_eq!(
            reading["data"]["agency_admission"][field],
            target["request"]["expected_agency"][field]
        );
    }
    assert!(reading["data"].get("project_binding").is_none());
    assert!(reading["data"].get("plan").is_none());
    let realisation = &reading["data"]["realisation"];
    assert_eq!(realisation["selected"], true);
    assert_eq!(realisation["executed"], false);
    assert_eq!(realisation["resident"]["inference_observed"], false);
    let selection = &realisation["factory_selection"];
    assert_eq!(selection["model_ref"], "model:deepseek-v4-pro");
    assert_eq!(selection["provider_ref"], "provider:openrouter");
    assert_eq!(selection["ranking_policy"], "EXPLICIT_PIN");
    let actual_basis = &selection["ranking_explanation"]["basis"];
    assert_eq!(
        actual_basis["composition_target_basis"]["agency_source"],
        *basis
    );
    assert_eq!(
        actual_basis["composition_target_basis"]["resident_body_basis"]["harness_profile"],
        "pi"
    );
    assert_eq!(
        actual_basis["native"]["model_observation"]["current_model_id"],
        "deepseek/deepseek-v4-pro"
    );
    let native_session = &actual_basis["native"]["native_session_id"];
    assert!(native_session.as_str().is_some_and(|s| !s.is_empty()));

    // Each refusal reaches the same actual owner/basis, without a turn.
    let wrong_model = invoke(
        &target_path,
        &basis_path,
        "model:unrelated",
        "provider:openrouter",
    );
    assert!(!wrong_model.status.success());
    let wrong_provider = invoke(
        &target_path,
        &basis_path,
        "model:deepseek-v4-pro",
        "provider:openai",
    );
    assert!(!wrong_provider.status.success());
    let wrong_target_path = w.temp.path().join("foreign-body-target.json");
    let mut wrong_target = composition["resident_target"].clone();
    wrong_target["body"] = json!("unconfigured-foreign-body");
    fs::write(
        &wrong_target_path,
        serde_json::to_vec(&wrong_target).unwrap(),
    )
    .unwrap();
    assert!(!invoke(
        &wrong_target_path,
        &basis_path,
        "model:deepseek-v4-pro",
        "provider:openrouter"
    )
    .status
    .success());
    let wrong_basis_path = w.temp.path().join("stale-source-basis.json");
    let mut wrong_basis = basis.clone();
    wrong_basis["content_digest"] = json!(format!("blake3:{}", "0".repeat(64)));
    fs::write(&wrong_basis_path, serde_json::to_vec(&wrong_basis).unwrap()).unwrap();
    assert!(!invoke(
        &target_path,
        &wrong_basis_path,
        "model:deepseek-v4-pro",
        "provider:openrouter"
    )
    .status
    .success());
    let repeat = invoke(
        &target_path,
        &basis_path,
        "model:deepseek-v4-pro",
        "provider:openrouter",
    );
    assert!(
        repeat.status.success(),
        "{} {}",
        String::from_utf8_lossy(&repeat.stdout),
        String::from_utf8_lossy(&repeat.stderr)
    );
    let repeated: Value = serde_json::from_slice(&repeat.stdout).unwrap();
    assert_eq!(
        repeated["data"]["realisation"]["resident"]["native_session_id"],
        *native_session
    );
    assert_eq!(
        repeated["data"]["realisation"]["resident"]["inference_observed"],
        false
    );
    assert!(!w.temp.path().join(".aikit").exists());
    assert!(!w.temp.path().join("ProjectCentral").exists());
    w.stop();
    println!("ACTUAL_PI_COMPOSE_CLI_SELECTION_WITHOUT_AMBIENT_COMPOSITION_OR_INFERENCE");
}

#[test]
#[ignore = "requires pinned Actuation and native protocol owner; mandatory CAW lane"]
fn application_realisation_dispatches_native_selected_model_and_requires_the_owner() {
    let mut w = World::new();
    let target = model_setup(&w, "normal", true);
    let service = aikit_cli::app::Service::open(w.home.clone(), w.temp.path(), |_| None).unwrap();
    let composed = composition(&w, &target);
    assert!(service
        .realise_model(
            &composed,
            "model:controlled-caw",
            Some("provider:controlled-native"),
            None,
        )
        .is_err());
    assert!(!w.temp.path().join("root.log").exists());
    start_model(&mut w, true);
    let mut incomplete = composed.clone();
    incomplete
        .as_object_mut()
        .unwrap()
        .remove("resident_target");
    assert!(service
        .realise_model(&incomplete, "model:controlled-caw", None, None)
        .is_err());
    assert!(service
        .realise_model(&composed, "model:unrelated", None, None)
        .is_err());
    assert!(!w.temp.path().join("root.log").exists());
    let result = service
        .realise_model(
            &composed,
            "model:controlled-caw",
            Some("provider:controlled-native"),
            None,
        )
        .unwrap();
    assert_eq!(result["schema"], "aikit.model-realisation/v2");
    assert_eq!(result["selected"], true);
    assert_eq!(result["executed"], false);
    assert!(result.get("factory_selection").is_none());
    assert_eq!(result["resident"]["body_basis"]["protocol"], "pi-rpc");
    assert_eq!(
        result["resident"]["body_basis"]["harness_profile"],
        Value::Null,
        "a controlled protocol fixture must not be presented as an actual Pi composition"
    );
    assert_eq!(
        result["resident"]["model_observation"]["current_model_id"],
        "controlled-model-v1"
    );
    assert_eq!(prompts(&w), 0);
    assert_eq!(send(&w, "application-model")["ok"], true);
    assert_eq!(
        w.returned("root", "application-model")["data"]["phase"],
        "returned"
    );
    assert_eq!(prompts(&w), 1);
    assert_no_secret(w.home.root());
    w.stop();
    assert!(service
        .realise_model(&composed, "model:controlled-caw", None, None)
        .is_err());
    assert_eq!(prompts(&w), 1);
    println!("MODEL_APPLICATION_NATIVE_CONNECTION_EXECUTED");
}

#[test]
#[ignore = "requires pinned Actuation and native protocol owner; mandatory CAW lane"]
fn compose_cli_realisation_uses_the_same_existing_resident_target() {
    let mut w = World::new();
    let target = model_setup(&w, "normal", true);
    let basis = w.temp.path().join("selected-basis.json");
    let destination = w.temp.path().join("resident-target.json");
    fs::write(
        &basis,
        target["request"]["expected_agency"]["basis"].to_string(),
    )
    .unwrap();
    fs::write(
        &destination,
        composition(&w, &target)["resident_target"].to_string(),
    )
    .unwrap();
    let isolated_home = w.temp.path().join("client-home");
    fs::create_dir_all(&isolated_home).unwrap();
    let native = actuation();
    let mut paths = vec![native.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    start_model(&mut w, true);
    let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
        .env("AIKIT_HOME", w.home.root())
        .env("HOME", isolated_home)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env_remove("CAW_SOURCE_API_KEY")
        .arg("--json")
        .arg("-C")
        .arg(w.temp.path())
        .args(["compose", "--agency-source"])
        .arg(basis)
        .args([
            "--agent",
            "agent:root",
            "--world",
            "central:root",
            "--realise",
            "--model",
            "model:controlled-caw",
            "--provider",
            "provider:controlled-native",
            "--resident-target",
        ])
        .arg(destination)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reading: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        reading["data"]["composition_scope"]["kind"],
        "explicit-native-resident"
    );
    for field in [
        "basis",
        "agent_ref",
        "agency_ref",
        "world_binding_ref",
        "world_ref",
        "scope_ref",
    ] {
        assert_eq!(
            reading["data"]["agency_admission"][field],
            target["request"]["expected_agency"][field]
        );
    }
    assert!(reading["data"].get("project_binding").is_none());
    assert!(reading["data"].get("plan").is_none());
    assert!(!w.temp.path().join(".aikit").exists());
    assert!(!w.temp.path().join("ProjectCentral").exists());
    assert_eq!(
        reading["data"]["realisation"]["selected"], true,
        "{reading}"
    );
    assert_eq!(reading["data"]["realisation"]["executed"], false);
    assert_eq!(send(&w, "compose-model")["ok"], true);
    assert_eq!(
        w.returned("root", "compose-model")["data"]["phase"],
        "returned"
    );
    assert_no_secret(w.home.root());
    w.stop();
    println!("MODEL_COMPOSE_NATIVE_CONNECTION_EXECUTED");
}

#[test]
#[ignore = "requires pinned Central and Actuation; mandatory CAW lane"]
fn native_central_root_composes_without_a_profile_or_child_project() {
    let w = World::new();
    let central = w.temp.path().join("Central");
    let ctrl =
        PathBuf::from(std::env::var_os("AIKIT_CAW_CTRL_BIN").expect("pinned native Central"));
    let initialized = Command::new(&ctrl)
        .args(["--json", "--root"])
        .arg(&central)
        .arg("init")
        .output()
        .unwrap();
    assert!(
        initialized.status.success(),
        "{} {}",
        String::from_utf8_lossy(&initialized.stdout),
        String::from_utf8_lossy(&initialized.stderr)
    );
    let native = actuation();
    let mut paths = vec![
        native.parent().unwrap().to_path_buf(),
        ctrl.parent().unwrap().to_path_buf(),
    ];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    for cwd in [&central, &central.join("Control"), &central.join("Work")] {
        let out = Command::new(env!("CARGO_BIN_EXE_aikit"))
            .env("AIKIT_HOME", w.home.root())
            .env("HOME", w.temp.path())
            .env("CENTRAL_ROOT", &central)
            .env("CENTRAL_CTRL_BIN", &ctrl)
            .env("PATH", std::env::join_paths(&paths).unwrap())
            .arg("--json")
            .arg("-C")
            .arg(cwd)
            .arg("compose")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let reading: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            reading["data"]["project_binding"]["project"],
            "control:root"
        );
        assert_eq!(reading["data"]["root_meta_project"], true);
        assert_eq!(
            reading["data"]["plan"]["project"],
            reading["data"]["project_binding"]
        );
        assert!(reading["data"]["model_routes"].is_array());
    }
    assert!(!central.join(".aikit").exists());
    assert!(!central.join("ProjectCentral").exists());
    println!("CENTRAL_ROOT_META_PROJECT_COMPOSE_EXECUTED");
}

#[test]
#[ignore = "requires pinned Actuation; mandatory CAW lane"]
fn canonical_root_agency_composes_from_a_nested_material_checkout() {
    let w = World::new();
    let central = w.temp.path().join("Central");
    let checkout = central.join("worktrees/env-2/o-i");
    for directory in [
        central.join(".aikit"),
        central.join("Control"),
        central.join("Work"),
        checkout.join(".aikit"),
    ] {
        fs::create_dir_all(directory).unwrap();
    }
    let central = central.canonicalize().unwrap();
    let checkout = checkout.canonicalize().unwrap();

    let source_path = w.temp.path().join("canonical-root-agency.json");
    let mut source: Value =
        serde_json::from_str(include_str!("../fixtures/caw-agency-request.json")).unwrap();
    source["differentiated_binding"]["world_ref"] = json!("control:root");
    source["differentiated_binding"]["scope_ref"] = json!("control:root");
    let source_bytes = serde_json::to_vec(&source).unwrap();
    fs::write(&source_path, &source_bytes).unwrap();
    let basis = AgencySourceBasis {
        source_ref: r("source/canonical-root-agency"),
        revision: rev("rev/canonical-root-1"),
        path: source_path.canonicalize().unwrap(),
        content_digest: format!("blake3:{}", blake3::hash(&source_bytes).to_hex()),
    };
    let basis_path = w.temp.path().join("canonical-root-basis.json");
    fs::write(&basis_path, serde_json::to_vec(&basis).unwrap()).unwrap();

    let native = actuation();
    let ctrl =
        PathBuf::from(std::env::var_os("AIKIT_CAW_CTRL_BIN").expect("pinned native Central"));
    let mut paths = vec![
        native.parent().unwrap().to_path_buf(),
        ctrl.parent().unwrap().to_path_buf(),
    ];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
        .env("AIKIT_HOME", w.home.root())
        .env("CENTRAL_ROOT", &central)
        .env("CENTRAL_CTRL_BIN", &ctrl)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .arg("--json")
        .arg("-C")
        .arg(&checkout)
        .args(["compose", "--agency-source"])
        .arg(&basis_path)
        .args(["--agent", "agent:existing-1", "--world", "control:root"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reading: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        reading["data"]["project_binding"]["project"],
        "control:root"
    );
    assert_eq!(
        reading["data"]["project_binding"]["locator"]["kind"],
        "native-world"
    );
    assert_eq!(
        reading["data"]["project_binding"]["locator"]["world"],
        "control:root"
    );
    assert_eq!(
        reading["data"]["project_binding"]["locator"]["scope"],
        "control:root"
    );
    assert_eq!(reading["data"]["root_meta_project"], true);
    assert_eq!(
        reading["data"]["project_root"],
        central.display().to_string()
    );
    assert_eq!(
        reading["data"]["invocation_cwd"],
        checkout.display().to_string()
    );
    assert_eq!(
        reading["data"]["plan"]["project"],
        reading["data"]["project_binding"]
    );
    assert!(reading["data"]["realisation"].is_null());
    println!("CANONICAL_ROOT_AGENCY_NESTED_MATERIAL_COMPOSED_WITHOUT_INFERENCE");
}

#[test]
#[ignore = "requires pinned Actuation; mandatory CAW lane"]
fn root_admission_cannot_borrow_an_unrelated_child_context() {
    let w = World::new();
    let target = model_setup(&w, "normal", true);
    let admitted: aikit_adapters::agency_admission::AdmittedAgency =
        serde_json::from_value(target["request"]["expected_agency"].clone()).unwrap();
    let child = w.temp.path().join("child");
    fs::create_dir_all(child.join(".aikit")).unwrap();
    let service = aikit_cli::app::Service::open(w.home.clone(), &child, |_| None).unwrap();
    let error = service.compose_selected_plan(Some(&admitted)).unwrap_err();
    assert_eq!(error.code(), "compose.root_world_child_context");
    assert!(!w.temp.path().join("root.log").exists());
}
