//! Real native actor-source integration. The selected coordinator supplies the
//! pinned Ctrl and an admitted product scratch root; no native reply is mocked.
use aikit_adapters::{
    actor_composition::{compose_live_actor_inputs, ACTUATION_MODEL_BEARING_FILE},
    runner::{Output, SystemRunner},
};
use aikit_core::AikitError;
use serde_json::{json, Value};
use std::{
    cell::Cell,
    ffi::OsStr,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::TempDir;

const CAPTURE_BYTES: u64 = 1024 * 1024;
const CAPTURE_SECONDS: u64 = 15;

struct Fixture {
    _owned: TempDir,
    evidence: PathBuf,
    sequence: Cell<u32>,
    ctrl_binary: PathBuf,
    path: String,
    root: PathBuf,
    home: PathBuf,
    project: PathBuf,
}

impl Fixture {
    fn new(label: &str, independent: bool) -> Self {
        // This checks an actual declared product source, not a guessed Project
        // identity or a path-shaped grant. The coordinator owns the existing
        // scratch; only these exclusive children belong to this test.
        let repository =
            fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
        let manifest_path = repository.join("ProjectCentral/project.json");
        let mut manifest_bytes = Vec::new();
        File::open(&manifest_path)
            .unwrap()
            .take(16 * 1024 + 1)
            .read_to_end(&mut manifest_bytes)
            .unwrap();
        assert!(
            manifest_bytes.len() <= 16 * 1024,
            "product manifest must fit its admitted test input"
        );
        let manifest: Value = serde_json::from_slice(&manifest_bytes).unwrap();
        assert_eq!(manifest["schema"], "central.project/v1");
        assert_eq!(manifest["human_source"], "ProjectCentral/user");
        assert!(manifest["project_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()));
        let scratch = fs::canonicalize(repository.join("ProjectCentral/now/tmp"))
            .expect("coordinator must prepare the declared product scratch");
        let supplied = PathBuf::from(
            std::env::var_os("AIKIT_NATIVE_TEST_ARTIFACT_ROOT")
                .expect("selected native gate requires AIKIT_NATIVE_TEST_ARTIFACT_ROOT"),
        );
        assert!(
            supplied.is_absolute(),
            "artifact root must be explicit and absolute"
        );
        let artifact = fs::canonicalize(&supplied).expect("artifact root must already exist");
        assert!(
            artifact.starts_with(&scratch),
            "artifact root must belong to this product scratch"
        );
        let held_artifact = File::open(&artifact).unwrap();
        assert!(held_artifact.metadata().unwrap().is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let identity = SystemRunner::new()
                .with_timeout(Duration::from_secs(CAPTURE_SECONDS))
                .with_output_limit_bytes(CAPTURE_BYTES)
                .with_strict_utf8()
                .capture_command(Command::new("id").arg("-u"))
                .unwrap();
            succeeded(&identity);
            let uid: u32 = identity.stdout.trim().parse().unwrap();
            assert_eq!(
                held_artifact.metadata().unwrap().uid(),
                uid,
                "coordinator scratch must be caller-owned"
            );
            let named = fs::metadata(&artifact).unwrap();
            let held = held_artifact.metadata().unwrap();
            assert_eq!((named.dev(), named.ino()), (held.dev(), held.ino()));
        }
        let owned = tempfile::Builder::new()
            .prefix(&format!("actor-{label}-"))
            .tempdir_in(&artifact)
            .unwrap();
        // Retained command facts are depth for the gate's Return and survive
        // fixture cleanup. The coordinator manages this exclusive directory.
        let evidence = tempfile::Builder::new()
            .prefix(&format!("actor-{label}-evidence-"))
            .tempdir_in(&artifact)
            .unwrap()
            .keep();
        let ctrl_binary = fs::canonicalize(
            std::env::var_os("CENTRAL_CTRL_BIN")
                .expect("selected native gate requires a built, pinned CENTRAL_CTRL_BIN"),
        )
        .unwrap();
        assert!(ctrl_binary.is_file());
        assert_eq!(
            ctrl_binary.file_name(),
            Some(OsStr::new("ctrl")),
            "adapter PATH must address the same Ctrl"
        );
        let mut paths = vec![ctrl_binary.parent().unwrap().to_path_buf()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(paths)
            .unwrap()
            .into_string()
            .expect("native test transport requires UTF-8 PATH");
        let root = owned.path().join("Central");
        let home = owned.path().join("aikit");
        let project = if independent {
            owned.path().join("independent-project")
        } else {
            root.join("Work/compose-proof")
        };
        let mut fixture = Self {
            _owned: owned,
            evidence,
            sequence: Cell::new(0),
            ctrl_binary,
            path,
            root,
            home,
            project,
        };
        fs::write(fixture.evidence.join("admission.json"), serde_json::to_vec_pretty(&json!({
            "product_manifest": manifest_path, "product": manifest, "scratch": scratch,
            "artifact_root": artifact, "ctrl_binary": fixture.ctrl_binary,
            "aikit_binary": env!("CARGO_BIN_EXE_aikit"), "standing": "declared-product-test-scratch",
            "capture_seconds": CAPTURE_SECONDS, "capture_bytes_per_stream": CAPTURE_BYTES,
            "strict_utf8": true, "independent_project": independent
        })).unwrap()).unwrap();
        fixture.ctrl_ok("init", &["init"]);
        fixture.root = fs::canonicalize(&fixture.root).unwrap();
        if !independent {
            fixture.project = fixture.root.join("Work/compose-proof");
        }
        if independent {
            // Explicit independently authored Project input. It is outside
            // this native World, so it does not claim native adoption/identity.
            fs::create_dir_all(fixture.project.join("ProjectCentral/user")).unwrap();
            fs::write(
                fixture.project.join("ProjectCentral/project.json"),
                json!({
                    "schema":"central.project/v1", "project_id":"project:compose-proof",
                    "human_source":"ProjectCentral/user",
                    "wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}
                })
                .to_string(),
            )
            .unwrap();
        } else {
            fs::create_dir_all(&fixture.project).unwrap();
            let created = fixture.ctrl_ok(
                "project-init",
                &[
                    "action",
                    "run",
                    "projectcentral.init",
                    &json!({"project":"compose-proof","project_id":"compose-proof"}).to_string(),
                ],
            );
            assert_eq!(created["data"]["project_id"], "compose-proof");
            assert_eq!(
                created["data"]["wiki_space_ref"],
                "central:wiki:project:compose-proof"
            );
            assert_eq!(
                Path::new(created["data"]["project_root"].as_str().unwrap()),
                fixture.project
            );
            assert_eq!(
                created["data"]["wiki_source"],
                "ProjectCentral/agents/wiki/wiki.json"
            );
            let declared: Value = serde_json::from_slice(
                &fs::read(fixture.project.join("ProjectCentral/project.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(declared["project_id"], "compose-proof");
        }
        let bound = fixture.aikit(
            "bind",
            &[
                "project",
                "bind",
                "compose-proof",
                "--directory",
                fixture.project.to_str().unwrap(),
                "--no-default-skill-sets",
            ],
        );
        succeeded(&bound);
        fixture
    }

    fn runner(&self) -> SystemRunner {
        SystemRunner::new()
            .with_timeout(Duration::from_secs(CAPTURE_SECONDS))
            .with_output_limit_bytes(CAPTURE_BYTES)
            .with_strict_utf8()
            .with_env("PATH", &self.path)
            .with_env("HOME", self._owned.path().to_str().unwrap())
    }

    fn record(&self, label: &str, value: Value) {
        let sequence = self.sequence.get();
        self.sequence.set(sequence + 1);
        let target = self.evidence.join(format!("{sequence:03}-{label}.json"));
        fs::write(target, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }

    fn capture(&self, label: &str, command: &mut Command) -> Output {
        let command_facts = json!({
            "program": command.get_program().to_str().expect("actual program must be representable"),
            "argv": command.get_args().map(|arg| arg.to_str().expect("actual argv must be representable")).collect::<Vec<_>>(),
            "cwd": command.get_current_dir(),
        });
        match self.runner().capture_command(command) {
            Ok(output) => {
                self.record(
                    label,
                    json!({"command":command_facts,"status":output.status,
                    "stdout":output.stdout,"stderr":output.stderr}),
                );
                output
            }
            Err(error) => {
                self.record(
                    label,
                    json!({"command":command_facts,"capture_error":{
                    "code":error.code(),"message":error.message(),"details":error.details(),
                    "debug":format!("{error:?}")}}),
                );
                panic!(
                    "actual finite native capture failed: {error:?}; evidence={}",
                    self.evidence.display()
                );
            }
        }
    }

    fn ctrl_output(&self, label: &str, args: &[&str]) -> Output {
        self.capture(
            label,
            Command::new(&self.ctrl_binary)
                .args(["--json", "--root"])
                .arg(&self.root)
                .args(args)
                .env("CENTRAL_ROOT", &self.root),
        )
    }

    fn ctrl_ok(&self, label: &str, args: &[&str]) -> Value {
        let output = self.ctrl_output(label, args);
        succeeded(&output);
        let value: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(value["ok"], true, "{value}");
        value
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
        command
            .env("AIKIT_HOME", &self.home)
            .env("CENTRAL_ROOT", &self.root)
            .env("CENTRAL_CTRL_BIN", &self.ctrl_binary)
            .env_remove("AIKIT_CONTEXT_ID")
            .env_remove("AIKIT_ISOLATION")
            .current_dir(&self.project);
        command
    }

    fn aikit(&self, label: &str, args: &[&str]) -> Output {
        self.capture(label, self.command().arg("--json").args(args))
    }

    fn project_context(&self, label: &str) -> Output {
        self.capture(
            label,
            self.command().args(["session-space", "project-context"]),
        )
    }

    fn save_profile(&self, reference: &str, agent: &str) -> (Value, PathBuf) {
        let profile = json!({"schema":"central.agent-profile/v1","ref":reference,"revision":"r1",
            "agent_ref":agent,"scope":"project","world_ref":"world/compose-proof",
            "ratified_world_refs":["world/compose-proof"],"governance_refs":["source/compose-proof/law"],
            "knowledge_source_refs":["source/compose-proof/intent"]});
        let saved = self.ctrl_ok(
            "profile-save",
            &[
                "action",
                "run",
                "agent-profile.save",
                &json!({"scope":"project","project":"compose-proof","profile":profile}).to_string(),
            ],
        );
        let source = Path::new(saved["data"]["source_path"].as_str().unwrap());
        let source = if source.is_absolute() {
            source.to_path_buf()
        } else {
            self.project.join(source)
        };
        assert!(source.starts_with(&self.project));
        (profile, source)
    }

    fn oracle_error(&self, label: &str) -> AikitError {
        let error = compose_live_actor_inputs(&self.runner(), &self.root, &self.project)
            .expect_err("actual invalid owner input must produce its native adapter error");
        self.record(
            label,
            json!({"actual_adapter_error":{"code":error.code(),
            "message":error.message(),"details":error.details(),"debug":format!("{error:?}")}}),
        );
        error
    }

    fn assert_refused_everywhere(&self, expected: &AikitError) {
        // Retain every actual result before an assertion can stop the test.
        let explicit = self.aikit("refused-compose", &["compose"]);
        let context = self.project_context("refused-project-context");
        let projection = self.aikit("refused-projection", &["context", "env"]);
        for output in [&explicit, &projection] {
            assert_ne!(
                output.status, 0,
                "invalid native source became successful empty context"
            );
            let value: Value = serde_json::from_str(&output.stdout).unwrap();
            assert_eq!(value["ok"], false);
            assert_eq!(value["error"]["code"], expected.code());
            assert_eq!(value["error"]["message"], expected.message());
            assert_eq!(
                value["error"]["details"],
                serde_json::to_value(expected.details()).unwrap()
            );
        }
        assert_ne!(
            context.status, 0,
            "invalid native source became a Context receipt"
        );
        assert!(
            context.stdout.is_empty(),
            "a refused Context must not emit a successful receipt"
        );
        assert_eq!(
            context.stderr,
            format!("{}: {}\n", expected.code(), expected.message())
        );
    }

    fn assert_absent_is_useful(&self) {
        let current = compose_live_actor_inputs(&self.runner(), &self.root, &self.project).unwrap();
        assert!(
            current.is_none(),
            "positive absence must be the actual adapter's Ok(None)"
        );
        let composed = self.aikit("absent-compose", &["compose"]);
        let context = self.project_context("absent-project-context");
        let projection = self.aikit("absent-projection", &["context", "env"]);
        succeeded(&composed);
        succeeded(&context);
        succeeded(&projection);
        let value: Value = serde_json::from_str(&composed.stdout).unwrap();
        assert_eq!(value["ok"], true);
        let context: Value = serde_json::from_str(&context.stdout).unwrap();
        assert!(context["context"]["reference"].is_string());
        // Context env is genuinely shell text and may be empty when no export
        // is selected. It does not promise an AIKIT_CONTEXT_ID export.
        assert!(serde_json::from_str::<Value>(&projection.stdout).is_err());
    }
}

fn succeeded(output: &Output) {
    assert_eq!(
        output.status, 0,
        "stdout={}\nstderr={}",
        output.stdout, output.stderr
    );
}

#[test]
#[ignore = "requires pinned native Ctrl, admitted AIKIT_NATIVE_TEST_ARTIFACT_ROOT and native discovery"]
fn explicit_compose_discloses_authored_basis_and_refuses_broken_source() {
    let fixture = Fixture::new("profile", false);
    fixture.assert_absent_is_useful();
    let (profile, source) = fixture.save_profile("profile/compose-proof", "agent/compose-proof");
    let original = fs::read(&source).unwrap();
    let composed = fixture.aikit("valid-compose", &["compose"]);
    succeeded(&composed);
    let value: Value = serde_json::from_str(&composed.stdout).unwrap();
    assert_eq!(
        value["data"]["composed_inputs"]["authored_basis"]["profile_source"]["revision"],
        "r1"
    );
    assert_eq!(
        value["data"]["composed_inputs"]["authored_basis"]["profile_source"]["governance_refs"],
        profile["governance_refs"]
    );
    assert_eq!(
        value["data"]["composed_inputs"]["authored_basis_standing"],
        "requested-source-not-effective-selection"
    );
    assert_eq!(value["data"]["plan"]["agent"]["state"], "resolved");
    let records = value["data"]["composed_inputs"]["source_resources"]
        .as_array()
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["eligibility"]["state"], "undetermined");
    assert!(records[0]["providers"].as_array().unwrap().is_empty());
    assert_eq!(records[0]["descriptor"]["sources"][0]["revision"], "r1");
    let first = fixture.project_context("valid-context");
    let repeated = fixture.project_context("repeated-context");
    let projection = fixture.aikit("valid-projection", &["context", "env"]);
    succeeded(&first);
    succeeded(&repeated);
    succeeded(&projection);
    let first: Value = serde_json::from_str(&first.stdout).unwrap();
    let repeated: Value = serde_json::from_str(&repeated.stdout).unwrap();
    assert!(first["context"]["reference"].is_string());
    assert_eq!(
        first, repeated,
        "unchanged native input has a stable receipt"
    );
    let mut changed_bytes = original.clone();
    changed_bytes.push(b'\n');
    fs::write(&source, &changed_bytes).unwrap();
    let changed = fixture.project_context("changed-context");
    succeeded(&changed);
    let changed: Value = serde_json::from_str(&changed.stdout).unwrap();
    assert_ne!(
        first["context"]["reference"],
        changed["context"]["reference"]
    );
    assert_eq!(
        first["context"]["basis"]["resolver_hash"],
        changed["context"]["basis"]["resolver_hash"]
    );
    assert_eq!(
        changed["context"]["basis"]["observed_source_resources"][0]["sources"][0]["revision"],
        "r1"
    );
    assert_eq!(fs::read(&source).unwrap(), changed_bytes);
    fs::write(&source, b"malformed owner profile").unwrap();
    let owner = fixture.ctrl_output(
        "malformed-native-list",
        &[
            "action",
            "run",
            "agent-profile.list",
            &json!({"scope":"project","project":"compose-proof"}).to_string(),
        ],
    );
    assert_ne!(
        owner.status, 0,
        "actual malformed owner source must fail its own native reader"
    );
    let expected = fixture.oracle_error("malformed-adapter-oracle");
    fixture.assert_refused_everywhere(&expected);
    assert_eq!(fs::read(&source).unwrap(), b"malformed owner profile");
    fs::write(&source, original).unwrap();
    let restored = fixture.project_context("restored-context");
    succeeded(&restored);
    assert_eq!(
        serde_json::from_str::<Value>(&restored.stdout).unwrap(),
        first
    );
}

#[cfg(unix)]
#[test]
#[ignore = "requires nonroot, pinned native Ctrl and admitted native product scratch"]
fn unreadable_authored_profile_refuses_context_and_projection_with_actual_owner_error() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    struct Restore {
        file: File,
        mode: u32,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            self.file
                .set_permissions(fs::Permissions::from_mode(self.mode))
                .unwrap();
        }
    }
    let fixture = Fixture::new("permission", false);
    let uid = fixture.capture("nonroot-prerequisite", Command::new("id").arg("-u"));
    succeeded(&uid);
    assert_ne!(
        uid.stdout.trim(),
        "0",
        "EACCES qualification requires an actual nonroot host"
    );
    let (_, source) = fixture.save_profile("profile/compose-proof", "agent/compose-proof");
    let original = fs::read(&source).unwrap();
    let before = fixture.project_context("readable-context");
    succeeded(&before);
    let file = File::open(&source).unwrap();
    let identity = (
        file.metadata().unwrap().dev(),
        file.metadata().unwrap().ino(),
    );
    let restore = Restore {
        mode: file.metadata().unwrap().permissions().mode(),
        file,
    };
    restore
        .file
        .set_permissions(fs::Permissions::from_mode(0o0))
        .unwrap();
    let native = fixture.ctrl_output(
        "unreadable-native-list",
        &[
            "action",
            "run",
            "agent-profile.list",
            &json!({"scope":"project","project":"compose-proof"}).to_string(),
        ],
    );
    assert_ne!(
        native.status, 0,
        "actual owner must observe the unreadable profile"
    );
    let expected = fixture.oracle_error("unreadable-adapter-oracle");
    fixture.assert_refused_everywhere(&expected);
    assert_eq!(
        (
            restore.file.metadata().unwrap().dev(),
            restore.file.metadata().unwrap().ino()
        ),
        identity
    );
    drop(restore);
    assert_eq!(fs::read(&source).unwrap(), original);
    let after = fixture.project_context("permissions-restored-context");
    succeeded(&after);
    assert_eq!(
        serde_json::from_str::<Value>(&after.stdout).unwrap(),
        serde_json::from_str::<Value>(&before.stdout).unwrap()
    );
}

#[test]
#[ignore = "requires pinned native Ctrl and admitted native product scratch"]
fn ambiguous_native_profiles_refuse_context_and_projection_without_guessing() {
    let fixture = Fixture::new("ambiguity", false);
    let (_, first) = fixture.save_profile("profile/first", "agent/first");
    let (_, second) = fixture.save_profile("profile/second", "agent/second");
    let first_bytes = fs::read(&first).unwrap();
    let second_bytes = fs::read(&second).unwrap();
    let native = fixture.ctrl_ok(
        "ambiguous-native-list",
        &[
            "action",
            "run",
            "agent-profile.list",
            &json!({"scope":"project","project":"compose-proof"}).to_string(),
        ],
    );
    assert_eq!(native["data"]["profiles"].as_array().unwrap().len(), 2);
    let expected = fixture.oracle_error("ambiguous-adapter-oracle");
    assert_eq!(expected.code(), "actor_composition.ambiguous_profile");
    fixture.assert_refused_everywhere(&expected);
    assert_eq!(fs::read(first).unwrap(), first_bytes);
    assert_eq!(fs::read(second).unwrap(), second_bytes);
}

#[test]
#[ignore = "requires pinned native Ctrl and admitted native product scratch"]
fn invalid_actual_model_bearing_refuses_context_and_projection_but_absence_remains_valid() {
    let fixture = Fixture::new("model-bearing", false);
    fixture.assert_absent_is_useful();
    let source = fixture.project.join(ACTUATION_MODEL_BEARING_FILE);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, b"invalid actual optional model-bearing JSON").unwrap();
    let expected = fixture.oracle_error("model-bearing-adapter-oracle");
    assert_eq!(expected.code(), "actor_composition.model_bearing_invalid");
    fixture.assert_refused_everywhere(&expected);
    assert_eq!(
        fs::read(&source).unwrap(),
        b"invalid actual optional model-bearing JSON"
    );
    fs::remove_file(source).unwrap();
    fixture.assert_absent_is_useful();
}

#[test]
#[ignore = "requires pinned native Ctrl and admitted native product scratch"]
fn explicitly_independent_project_without_actor_sources_remains_useful() {
    let fixture = Fixture::new("independent", true);
    assert!(!fixture.project.starts_with(&fixture.root));
    assert!(!fixture.project.join(ACTUATION_MODEL_BEARING_FILE).exists());
    fixture.assert_absent_is_useful();
    // Optional absence was established by the real adapter on this explicitly
    // external Project. It is not a fallback after a known owner failure.
    assert!(!fixture.project.starts_with(fixture.root.join("Work")));
}
