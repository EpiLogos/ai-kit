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
            Some("provider:controlled-native")
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
        .realise_model(&incomplete, "model:controlled-caw", None)
        .is_err());
    assert!(service
        .realise_model(&composed, "model:unrelated", None)
        .is_err());
    assert!(!w.temp.path().join("root.log").exists());
    let result = service
        .realise_model(
            &composed,
            "model:controlled-caw",
            Some("provider:controlled-native"),
        )
        .unwrap();
    assert_eq!(result["schema"], "aikit.model-realisation/v2");
    assert_eq!(result["selected"], true);
    assert_eq!(result["executed"], false);
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
        .realise_model(&composed, "model:controlled-caw", None)
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
    assert_eq!(reading["data"]["project_binding"]["project"], "central:root");
    assert_eq!(reading["data"]["project_binding"]["locator"]["kind"], "native-world");
    assert_eq!(reading["data"]["project_binding"]["locator"]["binding"], "binding:root");
    assert_eq!(reading["data"]["project_binding"]["locator"]["scope"], "scope:root");
    assert_eq!(reading["data"]["plan"]["project"], reading["data"]["project_binding"]);
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
    let ctrl = PathBuf::from(std::env::var_os("AIKIT_CAW_CTRL_BIN").expect("pinned native Central"));
    let initialized = Command::new(&ctrl).args(["--json", "--root"]).arg(&central).arg("init").output().unwrap();
    assert!(initialized.status.success(), "{} {}", String::from_utf8_lossy(&initialized.stdout), String::from_utf8_lossy(&initialized.stderr));
    let native = actuation();
    let mut paths = vec![native.parent().unwrap().to_path_buf(), ctrl.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    for cwd in [&central, &central.join("Control"), &central.join("Work")] {
        let out = Command::new(env!("CARGO_BIN_EXE_aikit"))
            .env("AIKIT_HOME", w.home.root())
            .env("HOME", w.temp.path())
            .env("CENTRAL_ROOT", &central)
            .env("CENTRAL_CTRL_BIN", &ctrl)
            .env("PATH", std::env::join_paths(&paths).unwrap())
            .arg("--json").arg("-C").arg(cwd).arg("compose").output().unwrap();
        assert!(out.status.success(), "{} {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let reading: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(reading["data"]["project_binding"]["project"], "control:root");
        assert_eq!(reading["data"]["root_meta_project"], true);
        assert_eq!(reading["data"]["plan"]["project"], reading["data"]["project_binding"]);
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
