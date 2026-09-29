//! Explicit local acceptance of the real launchd environment. Run with
//! `cargo test -p aikit-cli --test gateway_service_native -- --ignored --nocapture`
//! on a Workcell with the installed Central, Factory, Actuation and gh owners.

#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use aikit_cli::gateway_install::{render_plist, ServiceEnvironment};
use aikit_store::AikitHome;
use serde_json::Value;

struct Resident(Child);

impl Drop for Resident {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(command: &str, args: &[&str], environment: &BTreeMap<String, String>) -> Output {
    let output = Command::new(command)
        .args(args)
        .env_clear()
        .envs(environment)
        .current_dir("/")
        .output()
        .unwrap_or_else(|error| {
            panic!("{command} did not start under the rendered launchd environment: {error}")
        });
    assert!(
        output.status.success(),
        "{command} failed under the rendered launchd environment: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn json(output: Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("native owner returned JSON")
}

#[test]
#[ignore = "requires the installed native Central/Factory/Actuation/GitHub Workcell"]
fn real_owner_commands_and_disposable_resident_gateway_run_under_rendered_launchd_environment() {
    let home_dir = std::env::var("HOME").expect("installed Workcell HOME");
    let home_dir = Path::new(&home_dir);
    let central_root = home_dir.join("Central");
    let state = central_root.join("Work/Factory/.factory/development-state.json");
    let policy = central_root.join("Work/Factory/ProjectCentral/user/factory-policy.json");
    assert!(
        state.is_file() && policy.is_file(),
        "real installed Factory World is required"
    );
    let isolated = tempfile::tempdir().expect("isolated AIKit home");
    let aikit_home = AikitHome::at(isolated.path());
    let environment = ServiceEnvironment::discover(home_dir, &aikit_home)
        .expect("resolve actual installed owner commands");
    let candidate = env!("CARGO_BIN_EXE_aikit");
    let plist_path = isolated.path().join("gateway.plist");
    fs::write(
        &plist_path,
        render_plist(
            Path::new(candidate),
            &isolated.path().join("gateway.log"),
            &environment,
        ),
    )
    .expect("write disposable service plan");
    run(
        "/usr/bin/plutil",
        &["-lint", plist_path.to_str().unwrap()],
        &environment.values,
    );
    let parsed = json(run(
        "/usr/bin/plutil",
        &[
            "-extract",
            "EnvironmentVariables",
            "json",
            "-o",
            "-",
            plist_path.to_str().unwrap(),
        ],
        &environment.values,
    ));
    let carried = parsed
        .as_object()
        .expect("plutil parsed the service EnvironmentVariables")
        .iter()
        .map(|(key, value)| (key.clone(), value.as_str().unwrap().to_owned()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(carried, environment.values);
    assert_eq!(
        carried["AIKIT_CENTRAL_ROOT"],
        central_root.display().to_string()
    );
    assert!(!carried.contains_key("CENTRAL_NATIVE_TOKEN"));
    assert!(!carried.contains_key("GH_TOKEN"));
    let ctrl = &carried["CENTRAL_CTRL_BIN"];
    let time = json(run(
        ctrl,
        &[
            "--json",
            "--root",
            central_root.to_str().unwrap(),
            "action",
            "run",
            "central.time.policy",
            "{}",
        ],
        &carried,
    ));
    assert_eq!(
        time["ok"], true,
        "real Central owner did not resolve its World"
    );
    let factory = &carried["FACTORY_BIN"];
    let field = json(run(
        factory,
        &[
            "telemetry",
            "field",
            state.to_str().unwrap(),
            "--policy",
            policy.to_str().unwrap(),
            "--json",
        ],
        &carried,
    ));
    assert_eq!(field["schema"], "factory.telemetry-field/v1");
    assert_eq!(field["project_world_ref"], "project:Factory");
    let position = field["owner_basis"]["positions"]
        .as_object()
        .and_then(|positions| positions.keys().next())
        .expect("real Factory field has a native Position")
        .to_owned();
    let occupancy = json(run(
        &carried["ACTUATION_BIN"],
        &["occupancy", "read", "--position", &position, "--json"],
        &carried,
    ));
    assert!(
        occupancy.is_object(),
        "real Actuation occupancy owner answered"
    );
    let github = json(run(
        "/usr/bin/env",
        &[
            "gh",
            "api",
            "repos/EpiLogos/Factory/actions/runs/35988215169",
        ],
        &carried,
    ));
    assert_eq!(
        github["html_url"],
        "https://github.com/EpiLogos/Factory/actions/runs/35988215169"
    );

    // A CLI tick proves dispatcher construction and its Central root under
    // the parsed service environment. The isolated home has no Routines, so
    // this cannot mutate a live owner or dispatch scheduled work.
    let tick = json(run(candidate, &["gateway", "tick", "--json"], &carried));
    assert_eq!(tick["ok"], true);
    assert!(isolated
        .path()
        .join("state/routine-dispatcher.json")
        .is_file());

    let child = Command::new(candidate)
        .args(["gateway", "serve"])
        .env_clear()
        .envs(&carried)
        .current_dir("/")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("candidate gateway starts under the rendered environment");
    let mut resident = Resident(child);
    let socket = isolated.path().join("state/gateway.sock");
    let until = Instant::now() + Duration::from_secs(15);
    while !socket.exists() && Instant::now() < until {
        assert!(
            resident.0.try_wait().unwrap().is_none(),
            "disposable gateway exited early"
        );
        thread::sleep(Duration::from_millis(100));
    }
    assert!(
        socket.exists(),
        "disposable resident gateway did not open its isolated socket"
    );
    let status = json(run(candidate, &["gateway", "status", "--json"], &carried));
    assert_eq!(status["ok"], true);
    assert_eq!(status["data"]["type"], "status");
    eprintln!(
        "native service proof: Central={}, Factory field={}, Actuation Position={}, GitHub run={}, isolated gateway=ready",
        time["data"]["schema"], field["source_revision"], position, github["id"]
    );
}
