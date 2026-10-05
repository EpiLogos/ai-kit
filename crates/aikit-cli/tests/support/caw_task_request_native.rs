//! Real pending Task recovery through native Central, Agency and Workcell.
//! These cases prepare the embedded Codex profile but never open a protocol
//! body, send a prompt, or turn a test receipt into model execution evidence.
use super::*;
use sha2::{Digest, Sha256};

const TOKEN: &str = "controlled-native-task-request-recovery";
const SLUG: &str = "native-task-request";

struct NativeRequest {
    world: World,
    state: PathBuf,
    first: PathBuf,
    second: PathBuf,
    request: Value,
}

impl NativeRequest {
    fn new() -> Self {
        let world = World::new(true);
        // This is authored authority in a disposable native World, not a
        // substituted response from an owner or a production credential.
        let authority = world.root.join("Control/user/native-action-authority.json");
        fs::write(&authority, json!({"schema":"central.native-action-authority/v1",
            "scope_ref":"control:root", "grants":[{"principal_ref":"agent:existing-1",
                "actor_kind":"agent", "token_sha256":format!("{:x}",Sha256::digest(TOKEN.as_bytes())),
                "scope_refs":["control:root"], "actions":["central.now.allocate","central.now.lifecycle"],
                "expires_at_unix_seconds":u64::MAX}]}).to_string()).unwrap();
        let relation_path = world.root.join("Control/relations/source-relations.json");
        let mut relations: Value =
            serde_json::from_slice(&fs::read(&relation_path).unwrap()).unwrap();
        relations["relations"].as_array_mut().unwrap().push(json!({
            "ref":"central:source:control:root:Control/user/native-action-authority.json",
            "path":"Control/user/native-action-authority.json", "roles":["native-action-authority"],
            "provenance":"human-adopted", "standing":"architecture-contract",
            "treatment":"projectcentral-user", "recognition":"controlled-test-only",
            "recorded_at_unix_seconds":1
        }));
        fs::write(&relation_path, relations.to_string()).unwrap();

        let original = PathBuf::from(
            std::env::var_os("AIKIT_CAW_WORKCELL_BIN").expect("actual native Workcell required"),
        )
        .canonicalize()
        .unwrap();
        let first = world.root.join("native-owner-first");
        let second = world.root.join("native-owner-second");
        let mut provenance = Vec::new();
        for directory in [&first, &second] {
            fs::create_dir(directory).unwrap();
            // Copy actual native executables, not a protocol double. The two
            // real installations exercise a changed resolved helper path.
            for name in [
                "workcell",
                "workcell-write-boundary",
                "workcell-control-service",
                "workcell-control-client",
            ] {
                let source = original.parent().unwrap().join(name);
                let target = directory.join(name);
                assert!(
                    source.is_file(),
                    "native sibling missing: {}",
                    source.display()
                );
                fs::copy(&source, &target).unwrap();
                let bytes = fs::read(&source).unwrap();
                assert_eq!(fs::read(&target).unwrap(), bytes);
                provenance.push(json!({"source":source,"selected":target,
                    "sha256":format!("{:x}",Sha256::digest(&bytes))}));
            }
        }
        fs::write(
            world.root.join("native-executable-provenance.json"),
            serde_json::to_vec_pretty(&provenance).unwrap(),
        )
        .unwrap();
        let state = world.root.join("Work/demo/material-request");
        fs::create_dir(&state).unwrap();
        fs::write(
            state.join("storage.json"),
            json!({"schema":"workcell.directory-storage/v1",
            "directories":[{"logical_ref":"source-seat:native-task-request",
                "path":world.root.join("Work/demo")}]})
            .to_string(),
        )
        .unwrap();
        fs::write(
            world.root.join("Work/demo/src/partial.txt"),
            b"RETAINED_PARTIAL",
        )
        .unwrap();
        let mut native = Self {
            world,
            state,
            first,
            second,
            request: Value::Null,
        };
        let started = native.start_run(SLUG);
        let mut request = native.world.prepare_input();
        request["provider"] = json!({"id":"native-request-codex", "label":"Preparation only",
            "protocol":"acp", "from_profile":"codex"});
        request["prepared_run_scope"] = json!({"run_slug":SLUG,
            "expected_demand_digest":started["run"]["demand_digest"]});
        request
            .as_object_mut()
            .unwrap()
            .remove("workcell_boundary_bin");
        native.request = request;
        native
    }

    fn workcell(&self, args: &[String]) -> Value {
        let output = Command::new(self.first.join("workcell"))
            .args(["--state-root", self.state.to_str().unwrap(), "--json"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], true, "{value}");
        value
    }

    fn start_run(&self, slug: &str) -> Value {
        let empty = json!({"required":[],"preferred":[],"optional":[]});
        let demand = json!({"demand_ref":format!("demand:{slug}"),
            "subjects":{"test":"native-task-request-recovery"},
            "affordances":empty,"connectivity":empty,"exposure":empty,"outputs":empty,
            "storage":{"required":[{"logical_ref":"source-seat:native-task-request",
                "access":"writable","sharing":"shared","minimum_capacity":null,"unit":null,
                "persistence":"external","retention":"preserve"}],"preferred":[],"optional":[]},
            "workspace":null,"project_runtime":null,"resources":[],"persistence":null,
            "isolation_trust":null,"retention":"preserve","extensions":{}});
        let file = self.world.root.join(format!("{slug}-demand.json"));
        fs::write(&file, demand.to_string()).unwrap();
        self.workcell(&[
            "run".into(),
            "start".into(),
            "--run".into(),
            slug.into(),
            "--demand-json".into(),
            file.display().to_string(),
        ])
    }

    fn configure(
        &self,
        binary: &Path,
        owner: &Path,
        request: &Value,
        expected: Option<&str>,
    ) -> std::process::Output {
        let mut paths = vec![owner.to_path_buf()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let mut command = Command::new(binary);
        command
            .env("AIKIT_HOME", self.world.home.root())
            .env("WORKCELL_HOME", &self.state)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env(
                "OI_ACTUATION_BIN",
                std::env::var_os("AIKIT_CAW_ACTUATION_BIN").unwrap(),
            )
            .env("CENTRAL_NATIVE_TOKEN", TOKEN)
            .arg("-C")
            .arg(&self.world.root)
            .args([
                "encounter-task-configure",
                "--agent-session",
                "agent-session/task",
                "--request-json",
            ])
            .arg(request.to_string());
        if let Some(expected) = expected {
            command.args(["--expected-revision", expected]);
        }
        command.output().unwrap()
    }

    fn current_binary() -> &'static Path {
        Path::new(env!("CARGO_BIN_EXE_aikit-session-space"))
    }

    fn read(&self) -> Value {
        self.world.cli(&[
            "encounter-task-read".into(),
            "--agent-session".into(),
            "agent-session/task".into(),
        ])
    }

    fn task_path(&self) -> PathBuf {
        self.world
            .home
            .state()
            .join("encounter-tasks")
            .join(format!(
                "{}.json",
                blake3::hash(b"agent-session/task").to_hex()
            ))
    }

    fn run(&self) -> Value {
        self.workcell(&["run".into(), "show".into(), "--run".into(), SLUG.into()])
    }

    fn assert_refusal_unchanged(&self, request: &Value, expected: &str, message: &str) {
        let bytes = fs::read(self.task_path()).unwrap();
        let run = self.run();
        let output = self.configure(
            Self::current_binary(),
            &self.second,
            request,
            Some(expected),
        );
        assert!(
            !output.status.success(),
            "changed request unexpectedly admitted"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(self.task_path()).unwrap(), bytes);
        assert_eq!(
            self.run(),
            run,
            "refusal must not change native Run effects"
        );
        self.assert_partial_and_no_body();
    }

    fn assert_partial_and_no_body(&self) {
        assert_eq!(
            fs::read(self.world.root.join("Work/demo/src/partial.txt")).unwrap(),
            b"RETAINED_PARTIAL"
        );
        assert!(!self.world.root.join("Work/demo/src/protocol.log").exists());
        assert!(
            self.world.child.is_none(),
            "no protocol owner or model body is started by this case"
        );
    }
}

impl Drop for NativeRequest {
    fn drop(&mut self) {
        // Releases only disposable native Runs; no provider or shared service
        // was launched, and external source bytes retain preserve semantics.
        for slug in [SLUG, "native-task-request-other"] {
            let _ = Command::new(self.first.join("workcell"))
                .args([
                    "--state-root",
                    self.state.to_str().unwrap(),
                    "--json",
                    "run",
                    "release",
                    "--run",
                    slug,
                ])
                .output();
        }
    }
}

fn successful(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn pending_after_missing_directory(native: &NativeRequest, binary: &Path) -> (Value, Value, Value) {
    let ready = successful(native.configure(binary, &native.first, &native.request, None));
    assert_eq!(ready["ready"], true);
    let mut missing = native.request.clone();
    missing["selected_directories"] = json!([native.world.root.join("Work/demo/src/recoverable")]);
    let run = native.run();
    let output = native.configure(binary, &native.first, &missing, ready["revision"].as_str());
    assert!(
        !output.status.success(),
        "actual missing source directory must refuse"
    );
    let pending = native.read();
    assert_eq!(pending["ready"], false);
    assert_eq!(
        pending["request"]["selected_directories"],
        missing["selected_directories"]
    );
    // A historical owner may refuse allocation replay before it reaches the
    // missing directory. Preserve that actual pending shape as well: the
    // native allocation retained by the original ready reading is the basis.
    if !pending["allocation"].is_null() {
        assert_eq!(
            pending["allocation"]["allocation"]["now_ref"],
            ready["allocation"]["allocation"]["now_ref"]
        );
    }
    assert_eq!(
        native.run(),
        run,
        "failed source selection must retain the old native scope"
    );
    native.assert_partial_and_no_body();
    (missing, pending, ready)
}

fn recover_and_check_history(
    native: &NativeRequest,
    request: &Value,
    pending: &Value,
    allocated: &Value,
) -> Value {
    let old_bytes = fs::read(native.task_path()).unwrap();
    fs::create_dir(native.world.root.join("Work/demo/src/recoverable")).unwrap();
    let recovered = successful(native.configure(
        NativeRequest::current_binary(),
        &native.second,
        request,
        pending["revision"].as_str(),
    ));
    assert_eq!(recovered["ready"], true);
    assert_eq!(
        recovered["request"], pending["request"],
        "recovery must preserve exact caller source"
    );
    assert_eq!(
        recovered["prepared_run"]["boundary_executable"],
        json!(native.second.join("workcell-write-boundary"))
    );
    assert_eq!(
        recovered["prepared_run"]["scope"]["prepared_write_boundary"],
        recovered["inspection"]
    );
    assert_eq!(
        recovered["allocation"]["allocation"]["now_ref"],
        allocated["allocation"]["allocation"]["now_ref"]
    );
    assert_eq!(
        recovered["request"]["prepared_run_scope"],
        pending["request"]["prepared_run_scope"]
    );
    let historical = native
        .task_path()
        .parent()
        .unwrap()
        .join("history")
        .join(format!("{}.json", blake3::hash(&old_bytes).to_hex()));
    assert_eq!(
        fs::read(historical).unwrap(),
        old_bytes,
        "old pending evidence is immutable"
    );
    native.assert_refusal_unchanged(
        request,
        pending["revision"].as_str().unwrap(),
        "Task revision conflict",
    );
    // A late launcher must refuse at the native revision fence, before any
    // provider setup, protected exec or body effect.
    let late = Command::new(NativeRequest::current_binary())
        .env("AIKIT_HOME", native.world.home.root())
        .current_dir(&native.world.root.join("Work/demo"))
        .args([
            "encounter-task-exec",
            "--agent-session",
            "agent-session/task",
            "--expected-revision",
            pending["revision"].as_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!late.status.success());
    assert!(
        String::from_utf8_lossy(&late.stderr)
            .contains("Task revision or actual process cwd differs"),
        "{}",
        String::from_utf8_lossy(&late.stderr)
    );
    native.assert_partial_and_no_body();
    recovered
}

#[test]
#[ignore = "requires actual native Central, Workcell storage scope and Actuation; no provider launch"]
fn native_pending_request_is_immutable_across_resolved_boundary_change() {
    let native = NativeRequest::new();
    let (missing, pending, ready) =
        pending_after_missing_directory(&native, NativeRequest::current_binary());
    assert!(pending["allocation"].is_object(), "the current native owner must retain its actual allocation before the source-directory refusal");
    assert_eq!(
        pending["request"]["workcell_boundary_bin"], "",
        "the omitted caller path must not become a selected executable path"
    );
    let revision = pending["revision"].as_str().unwrap();
    let refusal = "Uncertain preparation must recover the same request";
    let mut changed = missing.clone();
    changed["provider"] = json!({"id":"changed-native-pi","label":"Changed caller body",
        "protocol":"pi-rpc","from_profile":"pi"});
    native.assert_refusal_unchanged(&changed, revision, refusal);
    changed = missing.clone();
    changed["cwd"] = json!(native.world.root.join("Work/demo/src"));
    native.assert_refusal_unchanged(&changed, revision, refusal);
    changed = missing.clone();
    changed["central"]["source_refs"] = json!(["source/changed-caller-source"]);
    native.assert_refusal_unchanged(&changed, revision, refusal);
    let other = native.start_run("native-task-request-other");
    changed = missing.clone();
    changed["prepared_run_scope"] = json!({"run_slug":"native-task-request-other",
        "expected_demand_digest":other["run"]["demand_digest"]});
    native.assert_refusal_unchanged(&changed, revision, refusal);
    changed = missing.clone();
    changed["material_host"] = json!({"workcell_bin":native.second.join("workcell"),
        "endpoint":"127.0.0.1:1","workcell_ref":"workcell:controlled-test",
        "demand_ref":"demand:changed-material"});
    native.assert_refusal_unchanged(
        &changed,
        revision,
        "An existing prepared run cannot also allocate another material host",
    );
    changed = missing.clone();
    changed["workcell_boundary_bin"] = json!(native.first.join("workcell-write-boundary"));
    native.assert_refusal_unchanged(&changed, revision, refusal);
    recover_and_check_history(&native, &missing, &pending, &ready);
}

#[test]
#[ignore = "requires actual legacy Task CLI plus native Central, Workcell and Actuation; no provider launch"]
fn native_legacy_normalized_pending_request_recovers_only_its_retained_source() {
    let native = NativeRequest::new();
    let legacy = PathBuf::from(
        std::env::var_os("AIKIT_CAW_LEGACY_TASK_BIN")
            .expect("actual old Task owner required, not a fabricated legacy record"),
    )
    .canonicalize()
    .unwrap();
    assert!(legacy.is_file());
    let source = fs::read(&legacy).unwrap();
    fs::write(
        native.world.root.join("legacy-owner-provenance.json"),
        json!({"path":legacy,"sha256":format!("{:x}",Sha256::digest(&source))}).to_string(),
    )
    .unwrap();
    let (original_omitted, pending, ready) = pending_after_missing_directory(&native, &legacy);
    assert_eq!(pending["request"]["workcell_boundary_bin"],
        json!(native.first.join("workcell-write-boundary")),
        "the actual old owner must demonstrate its normalization before this compatibility case is useful");
    native.assert_refusal_unchanged(
        &original_omitted,
        pending["revision"].as_str().unwrap(),
        "Uncertain preparation must recover the same request",
    );
    recover_and_check_history(&native, &pending["request"], &pending, &ready);
    assert_eq!(
        fs::read(&legacy).unwrap(),
        source,
        "legacy native executable remains unchanged"
    );
}
