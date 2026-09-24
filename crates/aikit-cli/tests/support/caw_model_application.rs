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
        reading["data"]["project_binding"]["project"],
        "central:root"
    );
    assert_eq!(
        reading["data"]["project_binding"]["locator"]["kind"],
        "native-world"
    );
    assert_eq!(
        reading["data"]["project_binding"]["locator"]["binding"],
        "binding:root"
    );
    assert_eq!(
        reading["data"]["project_binding"]["locator"]["scope"],
        "scope:root"
    );
    assert_eq!(
        reading["data"]["plan"]["project"],
        reading["data"]["project_binding"]
    );
    assert_eq!(reading["data"]["root_meta_project"], true);
    assert_eq!(reading["data"]["local_project_directory_present"], false);
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
