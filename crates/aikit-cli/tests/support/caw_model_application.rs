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
    assert!(service.realise_model(&composed, "model:controlled-caw", Some("provider:controlled-native")).is_err());
    assert!(!w.temp.path().join("root.log").exists());
    start_model(&mut w, true);
    let mut incomplete = composed.clone();
    incomplete.as_object_mut().unwrap().remove("resident_target");
    assert!(service.realise_model(&incomplete, "model:controlled-caw", None).is_err());
    assert!(service.realise_model(&composed, "model:unrelated", None).is_err());
    assert!(!w.temp.path().join("root.log").exists());
    let result = service.realise_model(&composed, "model:controlled-caw", Some("provider:controlled-native")).unwrap();
    assert_eq!(result["schema"], "aikit.model-realisation/v2");
    assert_eq!(result["selected"], true);
    assert_eq!(result["executed"], false);
    assert_eq!(result["resident"]["model_observation"]["current_model_id"], "controlled-model-v1");
    assert_eq!(prompts(&w), 0);
    assert_eq!(send(&w, "application-model")["ok"], true);
    assert_eq!(w.returned("root", "application-model")["data"]["phase"], "returned");
    assert_eq!(prompts(&w), 1);
    assert_no_secret(w.home.root());
    w.stop();
    assert!(service.realise_model(&composed, "model:controlled-caw", None).is_err());
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
    fs::write(&basis, target["request"]["expected_agency"]["basis"].to_string()).unwrap();
    fs::write(&destination, composition(&w, &target)["resident_target"].to_string()).unwrap();
    let isolated_home = w.temp.path().join("client-home");
    fs::create_dir_all(&isolated_home).unwrap();
    let native = actuation();
    let mut paths = vec![native.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    start_model(&mut w, true);
    let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
        .env("AIKIT_HOME", w.home.root()).env("HOME", isolated_home)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env_remove("CAW_SOURCE_API_KEY")
        .arg("--json").arg("-C").arg(w.temp.path())
        .args(["compose", "--agency-source"]).arg(basis)
        .args(["--agent", "agent:root", "--world", "central:root", "--realise",
            "--model", "model:controlled-caw", "--provider", "provider:controlled-native", "--resident-target"])
        .arg(destination).output().unwrap();
    assert!(output.status.success(), "{} {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let reading: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reading["data"]["realisation"]["selected"], true, "{reading}");
    assert_eq!(reading["data"]["realisation"]["executed"], false);
    assert_eq!(send(&w, "compose-model")["ok"], true);
    assert_eq!(w.returned("root", "compose-model")["data"]["phase"], "returned");
    assert_no_secret(w.home.root());
    w.stop();
    println!("MODEL_COMPOSE_NATIVE_CONNECTION_EXECUTED");
}
