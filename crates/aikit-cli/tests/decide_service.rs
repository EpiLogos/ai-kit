//! The native Kev lifecycle, end to end through the real `aikit` binary, in an
//! environment where Workcell and Factory do not exist.
//!
//! Tripwire: `workcell`, `workcell-write-boundary` and `factory` are on PATH,
//! and every one of them records the call and fails. If any lifecycle verb
//! discovers, runs or requires one of them, the marker file appears and the
//! test fails. The only things faked are the network-bound tools the recipe
//! calls (`git`, `uv`) and the model server itself (`fake_kev.py`, a loopback
//! SystemOne-compatible process started through the recipe's real command
//! line); everything else — process start/identity/stop, health against the
//! model card, the warm decision through the elected provider, upgrade and
//! rollback — is the production code path.

use assert_cmd::cargo::cargo_bin;
use serde_json::{json, Value};
use std::{
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

const OLD_ADAPTER: &str = "9a45d25eb2ab761841196625383fa1dff0e56c1e";

fn executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn alive(pid: u64) -> bool {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .map(|o| {
            o.status.success()
                && !String::from_utf8_lossy(&o.stdout)
                    .trim_start()
                    .starts_with('Z')
        })
        .unwrap_or(false)
}

struct Scene {
    _root: TempDir,
    home: PathBuf,
    bin: PathBuf,
    tripwire_bin: PathBuf,
    mark: PathBuf,
    hf: PathBuf,
    fake: PathBuf,
    workdir: PathBuf,
}

impl Scene {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let base = root.path().to_path_buf();
        let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_kev.py");
        let bin = base.join("shims");
        let tripwire_bin = base.join("tripwire");
        let mark = base.join("TRIPWIRE-HIT");
        let fake_s = fake.display().to_string();
        // The venv interpreter every provision creates: the fake, via python3.
        let venv_python = format!("#!/bin/sh\nexec python3 \"{fake_s}\" \"$@\"\n");
        executable(
            &bin.join("uv"),
            &format!(
                "#!/bin/sh\n\
                 # uv --directory D sync --python V --extra E\n\
                 [ \"$1\" = --directory ] || exit 64\n\
                 [ -z \"$FAKE_UV_FAIL\" ] || {{ echo \"uv: induced failure\" >&2; exit 3; }}\n\
                 mkdir -p \"$2/.venv/bin\"\n\
                 printf '%s' '{venv_python}' > \"$2/.venv/bin/python\"\n\
                 chmod +x \"$2/.venv/bin/python\"\n"
            ),
        );
        executable(
            &bin.join("git"),
            "#!/bin/sh\n\
             case \"$1\" in\n\
               clone) prev=\"\"; last=\"\"; for a; do prev=\"$last\"; last=\"$a\"; done; \
                      mkdir -p \"$last/.git\"; printf '%s' \"$prev\" > \"$last/.git/FAKE_ORIGIN\"; exit 0;;\n\
               -C) dir=\"$2\"; shift 2;;\n\
               *) exit 64;;\n\
             esac\n\
             case \"$1\" in\n\
               remote) cat \"$dir/.git/FAKE_ORIGIN\"; echo;;\n\
               fetch) exit 0;;\n\
               checkout) for last; do :; done; \
                         [ \"$last\" != \"$FAKE_GIT_FAIL_PIN\" ] || { echo 'git: induced checkout failure' >&2; exit 4; }; \
                         printf '%s' \"$last\" > \"$dir/.git/FAKE_HEAD\";;\n\
               rev-parse) cat \"$dir/.git/FAKE_HEAD\"; echo;;\n\
               *) exit 64;;\n\
             esac\n",
        );
        for name in ["workcell", "workcell-write-boundary", "factory"] {
            executable(
                &tripwire_bin.join(name),
                &format!(
                    "#!/bin/sh\necho \"{name} $*\" >> \"{}\"\nexit 99\n",
                    mark.display()
                ),
            );
        }
        let workdir = base.join("work");
        fs::create_dir_all(&workdir).unwrap();
        Self {
            home: base.join("home"),
            hf: base.join("hf"),
            bin,
            tripwire_bin,
            mark,
            fake,
            workdir,
            _root: root,
        }
    }

    fn path(&self) -> String {
        format!(
            "{}:{}:/usr/bin:/bin:/usr/sbin:/sbin",
            self.bin.display(),
            self.tripwire_bin.display()
        )
    }

    /// `aikit --json decide service <args>` with a PATH holding no Workcell.
    fn run(&self, extra_env: &[(&str, &str)], args: &[&str]) -> (bool, Value, String) {
        let mut command = Command::new(cargo_bin("aikit"));
        command
            .args(["--json", "decide", "service"])
            .args(args)
            .current_dir(&self.workdir)
            .env_clear()
            .env("PATH", self.path())
            .env("HOME", self.home.display().to_string())
            .env("AIKIT_HOME", self.home.join(".aikit"))
            .env("FAKE_HF_ROOT", &self.hf)
            // Another owner's authority that must never reach the service.
            .env("WORKCELL_CONTROL_TOKEN", "must-not-leak")
            .stdin(Stdio::null());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
            panic!("no JSON envelope ({e}); stdout={stdout:?} stderr={stderr:?}")
        });
        (output.status.success(), envelope, stderr)
    }

    fn ok(&self, args: &[&str]) -> Value {
        let (success, envelope, stderr) = self.run(&[], args);
        assert!(success, "{args:?} failed: {envelope} {stderr}");
        envelope["data"].clone()
    }

    fn tripwire_hits(&self) -> String {
        fs::read_to_string(&self.mark).unwrap_or_default()
    }
}

fn pid_of(data: &Value) -> u64 {
    data["pid"].as_u64().expect("a pid")
}

/// Stop any service a failing test left behind, so the machine is left clean.
struct Cleanup(Vec<PathBuf>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for dir in &self.0 {
            if let Ok(state) = fs::read_to_string(dir.join("service.json")) {
                if let Ok(state) = serde_json::from_str::<Value>(&state) {
                    if let Some(pid) = state["process"]["pid"].as_u64() {
                        let _ = Command::new("kill").arg(pid.to_string()).status();
                    }
                }
            }
        }
    }
}

#[test]
fn the_kev_lifecycle_runs_without_workcell_and_never_adopts_a_service_it_did_not_start() {
    let scene = Scene::new();
    let service = scene._root.path().join("kev-service");
    let service_s = service.display().to_string();
    let port = free_port().to_string();
    let _cleanup = Cleanup(vec![service.clone()]);
    let base = ["--service-dir", service_s.as_str()];
    let with = |verb: &str, rest: &[&str]| -> Vec<String> {
        let mut v = vec![verb.to_string()];
        v.extend(base.iter().map(|s| s.to_string()));
        v.extend(rest.iter().map(|s| s.to_string()));
        v
    };
    let call = |verb: &str, rest: &[&str]| -> Value {
        let argv = with(verb, rest);
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        scene.ok(&refs)
    };

    // Nothing is provisioned yet, and saying so needs no Workcell.
    let status = call("status", &[]);
    assert_eq!(status["state"], "not-provisioned");
    assert_eq!(status["workcell"], "not involved");

    // provision: pinned checkout, environment and artifacts, hashed.
    let provision = call("provision", &["--port", &port]);
    assert_eq!(provision["owner"], "aikit");
    assert_eq!(provision["provider_mode"], "endpoint");
    assert_eq!(provision["state"], "stopped");
    assert_eq!(
        provision["pins"]["adapter"],
        format!("jaredpalmer/kev-0.8b@{OLD_ADAPTER}")
    );
    assert!(provision["weights_bytes"].as_u64().unwrap() > 0);
    let provider: Value =
        serde_json::from_slice(&fs::read(service.join("decision-provider.json")).unwrap()).unwrap();
    assert_eq!(
        provider["mode"], "endpoint",
        "never labelled Workcell-managed"
    );
    assert_eq!(provider["address"], format!("127.0.0.1:{port}"));
    assert_eq!(provider["decision_model"]["family"], "kev");

    // start: ready means the pinned model card answers and a warm decision ran.
    let started = call("start", &["--ready-timeout-secs", "60"]);
    assert_eq!(started["outcome"], "started");
    assert_eq!(started["state"], "running");
    assert_eq!(started["served"]["run"], "jaredpalmer/kev-0.8b");
    assert!(started["warm_ms"].as_u64().is_some(), "{started}");
    let pid = pid_of(&started);
    assert!(alive(pid), "the service outlives the start command");

    // The service did not inherit another owner's control authority.
    let env: Vec<String> =
        serde_json::from_slice(&fs::read(service.join("fake-env.json")).unwrap()).unwrap();
    assert!(
        !env.iter().any(|k| k.starts_with("WORKCELL_")),
        "service environment leaked: {env:?}"
    );
    assert!(
        env.contains(&"HF_HUB_OFFLINE".to_string()),
        "offline serving is set"
    );

    // start again adopts our own process, never a second one.
    let again = call("start", &[]);
    assert_eq!(again["outcome"], "already-running");
    assert_eq!(pid_of(&again), pid);
    assert_eq!(again["healthy"], true);

    // status: identity + model card + material verification + a real decision.
    let status = call("status", &["--verify-material", "--probe"]);
    assert_eq!(status["state"], "running");
    assert_eq!(status["process"]["standing"], "ours");
    assert_eq!(status["health"]["identity"], "pinned");
    assert_eq!(status["material"]["state"], "verified");
    assert_eq!(status["diagnostic"]["outcome"], "completed");

    // The provider election the service wrote is consumable by the ordinary
    // decide surface, with no lifecycle knowledge at all.
    let provider_file = service.join("decision-provider.json");
    let (ok, decided, stderr) = {
        let mut command = Command::new(cargo_bin("aikit"));
        let output = command
            .args(["--json", "decide", "status", "--probe", "--provider-file"])
            .arg(&provider_file)
            .env_clear()
            .env("PATH", scene.path())
            .env("HOME", scene.home.display().to_string())
            .output()
            .unwrap();
        (
            output.status.success(),
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    assert!(ok, "{decided} {stderr}");
    assert_eq!(decided["data"]["mode"], "endpoint");
    assert_eq!(decided["data"]["install"]["state"], "loaded");
    assert_eq!(decided["data"]["diagnostic"]["outcome"], "completed");

    // restart: a new process, healthy.
    let restarted = call("restart", &["--ready-timeout-secs", "60"]);
    assert_eq!(restarted["stop"]["outcome"], "stopped");
    assert_eq!(restarted["start"]["outcome"], "started");
    let new_pid = pid_of(&restarted["start"]);
    assert_ne!(new_pid, pid);
    assert!(!alive(pid) && alive(new_pid));

    // upgrade to another pinned cut while running: stop, re-provision, start.
    let mut recipe = read_default_recipe(&service);
    recipe["adapter_revision"] = json!("1111111111111111111111111111111111111111");
    let recipe_file = scene._root.path().join("recipe-next.json");
    fs::write(&recipe_file, serde_json::to_vec(&recipe).unwrap()).unwrap();
    let upgraded = call(
        "upgrade",
        &[
            "--recipe-file",
            recipe_file.to_str().unwrap(),
            "--ready-timeout-secs",
            "60",
        ],
    );
    assert_eq!(upgraded["outcome"], "upgraded");
    assert_eq!(
        upgraded["to"]["adapter"],
        "1111111111111111111111111111111111111111"
    );
    let upgraded_pid = pid_of(&upgraded["start"]);
    assert!(alive(upgraded_pid) && !alive(new_pid));
    let manifest: Value =
        serde_json::from_slice(&fs::read(service.join("decision-material-manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["adapter_revision"],
        "1111111111111111111111111111111111111111"
    );

    // upgrade to a cut that cannot be provisioned: rolled back, running again
    // on the previous cut.
    recipe["adapter_revision"] = json!("2222222222222222222222222222222222222222");
    recipe["upstream_pin"] = json!("3333333333333333333333333333333333333333");
    fs::write(&recipe_file, serde_json::to_vec(&recipe).unwrap()).unwrap();
    let argv = with(
        "upgrade",
        &[
            "--recipe-file",
            recipe_file.to_str().unwrap(),
            "--ready-timeout-secs",
            "60",
        ],
    );
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let (success, failed, _) = scene.run(
        &[(
            "FAKE_GIT_FAIL_PIN",
            "3333333333333333333333333333333333333333",
        )],
        &refs,
    );
    assert!(!success, "an unprovisionable cut must fail: {failed}");
    let message = failed["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(message.contains("rolled back"), "{failed}");
    let after = call("status", &[]);
    assert_eq!(after["state"], "running", "service restored: {after}");
    assert_eq!(after["process"]["standing"], "ours");
    let manifest: Value =
        serde_json::from_slice(&fs::read(service.join("decision-material-manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["adapter_revision"], "1111111111111111111111111111111111111111",
        "the previous cut is what is installed after rollback"
    );

    // stop: identity-checked, idempotent, and the port goes quiet.
    let running_pid = pid_of(&after["process"]);
    let stopped = call("stop", &[]);
    assert_eq!(stopped["outcome"], "stopped");
    assert!(!alive(running_pid));
    assert_eq!(call("stop", &[])["outcome"], "not-running");
    assert_eq!(call("status", &[])["state"], "stopped");

    // A listener this service did not start is never adopted, stopped or
    // restarted: it is reported as foreign, and left running.
    let foreign_port = free_port().to_string();
    let mut foreign = Command::new("python3")
        .arg(&scene.fake)
        .args([
            "-m",
            "kev.serve",
            "--run",
            "jaredpalmer/kev-0.8b",
            "--host",
            "127.0.0.1",
            "--port",
        ])
        .arg(&foreign_port)
        .current_dir({
            let d = scene._root.path().join("foreign/kev");
            fs::create_dir_all(&d).unwrap();
            d
        })
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline
        && Command::new("curl")
            .args(["-sf", &format!("http://127.0.0.1:{foreign_port}/v1/models")])
            .output()
            .map(|o| !o.status.success())
            .unwrap_or(true)
    {
        std::thread::sleep(Duration::from_millis(100));
    }
    let service_b = scene._root.path().join("kev-service-b");
    let b = service_b.display().to_string();
    let provisioned_b = scene.ok(&["provision", "--service-dir", &b, "--port", &foreign_port]);
    assert_eq!(provisioned_b["state"], "stopped");
    let (ok_b, refused, _) = scene.run(&[], &["start", "--service-dir", &b]);
    assert!(!ok_b);
    assert_eq!(
        refused["error"]["code"], "decision_service.port_occupied",
        "{refused}"
    );
    let status_b = scene.ok(&["status", "--service-dir", &b]);
    assert_eq!(status_b["state"], "foreign-listener");
    assert_eq!(
        scene.ok(&["stop", "--service-dir", &b])["outcome"],
        "not-running"
    );
    assert!(
        foreign.try_wait().unwrap().is_none(),
        "the foreign service was left running"
    );
    let _ = foreign.kill();
    let _ = foreign.wait();

    // The tripwire: nothing in the whole lifecycle reached an excluded owner.
    assert_eq!(scene.tripwire_hits(), "", "an excluded owner was reached");
}

fn read_default_recipe(service: &Path) -> Value {
    let state: Value =
        serde_json::from_slice(&fs::read(service.join("service.json")).unwrap()).unwrap();
    state["recipe"].clone()
}

#[test]
fn the_tripwire_itself_detects_a_workcell_call() {
    // Proves the detector is live: a call to the shim leaves the marker, so a
    // clean marker in the lifecycle test is evidence, not an artifact.
    let scene = Scene::new();
    let status = Command::new(scene.tripwire_bin.join("workcell"))
        .arg("plan")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(99));
    assert!(scene.tripwire_hits().contains("workcell plan"));
}

#[test]
fn provisioning_a_floating_pin_or_a_workcell_environment_is_refused_before_any_step() {
    let scene = Scene::new();
    let service = scene._root.path().join("svc");
    let mut recipe = json!({
        "schema": "aikit.decision-service-recipe/v1", "recipe": "kev-0.8b", "family": "kev",
        "upstream_repository": "https://github.com/jaredpalmer/kev.git",
        "upstream_pin": "main",
        "adapter_repo": "jaredpalmer/kev-0.8b", "adapter_revision": OLD_ADAPTER,
        "base_repo": "Qwen/Qwen3.5-0.8B-Base",
        "base_revision": "dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68",
        "python": "3.13", "extra": "serve", "serve_module": "kev.serve",
        "run": "jaredpalmer/kev-0.8b", "model": "kev-latest", "env": {},
        "license": "Apache-2.0", "trained_state_tokens": 7552,
        "limits": {"timeout_ms": 1000, "max_attempts": 1,
                   "max_input_tokens_per_attempt": 100, "max_output_tokens_per_attempt": 100,
                   "model": "kev-latest"}
    });
    let file = scene._root.path().join("recipe.json");
    fs::write(&file, serde_json::to_vec(&recipe).unwrap()).unwrap();
    let (ok, failed, _) = scene.run(
        &[],
        &[
            "provision",
            "--service-dir",
            service.to_str().unwrap(),
            "--recipe-file",
            file.to_str().unwrap(),
        ],
    );
    assert!(!ok);
    assert_eq!(failed["error"]["code"], "decision_service.recipe_invalid");
    assert!(
        !service.join("kev").exists(),
        "no step ran for an invalid recipe"
    );

    recipe["upstream_pin"] = json!("5920c5fe4ca8e0970ed4209ac2c9b8e18bea5109");
    recipe["env"] = json!({"WORKCELL_CONTROL_TOKEN": "x"});
    fs::write(&file, serde_json::to_vec(&recipe).unwrap()).unwrap();
    let (ok, failed, _) = scene.run(
        &[],
        &[
            "provision",
            "--service-dir",
            service.to_str().unwrap(),
            "--recipe-file",
            file.to_str().unwrap(),
        ],
    );
    assert!(!ok);
    assert_eq!(failed["error"]["code"], "decision_service.recipe_invalid");
    assert_eq!(scene.tripwire_hits(), "");
}
