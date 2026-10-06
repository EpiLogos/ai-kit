//! Actual folded CLI configuration and native on-disk provider admission.
//! This boundary validates file/directory existence, not acting-body ABI. The
//! real compiled AIKit image supplies file witnesses; no provider is launched,
//! emulated, or reported as a working Actuation/QL/Prime body.
#![cfg(unix)]

use aikit_adapters::runner::{Output, SystemRunner};
use aikit_cli::encounter_service::EncounterProvider;
use serde_json::{json, Value};
use std::{
    error::Error,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const QL_REVISION: &str = "6a81fc441e4dda477f4de3a7ebd59c368cb28f37";
const BODY_REVISION: &str = "39c28eb6e74fe87d10f70910be91005960ff4bf1";

struct Fixture {
    root: PathBuf,
    identity: (u64, u64),
    calls: usize,
    disposed: bool,
}

impl Fixture {
    fn new() -> Self {
        let product = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let scratch = product.join("ProjectCentral/now/tmp");
        fs::create_dir_all(&scratch).unwrap();
        let root = tempfile::Builder::new()
            .prefix("epi-config-")
            .tempdir_in(scratch.canonicalize().unwrap())
            .unwrap()
            .keep();
        let metadata = fs::symlink_metadata(&root).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        let mut fixture = Self {
            root,
            identity: (metadata.dev(), metadata.ino()),
            calls: 0,
            disposed: false,
        };
        for member in ["Central/Control", "Central/Work", "user"] {
            fs::create_dir_all(fixture.root.join(member)).unwrap();
        }
        // Establish the disposable home/layout using the same native CLI. A
        // later refusal must preserve an existing real provider, not a hand-
        // written record or a fabricated successful configuration response.
        let output = fixture.configure(&[]);
        fixture.require_configured(&output);
        fixture
    }

    fn require_owned(&self) {
        let metadata = fs::symlink_metadata(&self.root).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!((metadata.dev(), metadata.ino()), self.identity);
    }

    fn central(&self) -> PathBuf {
        self.root.join("Central")
    }

    fn provider_path(&self) -> PathBuf {
        self.root
            .join("home/state/encounter-providers/epi-prime-ql.json")
    }

    fn configure(&mut self, owner: &[(&str, &Path)]) -> Output {
        self.require_owned();
        let binary = Path::new(env!("CARGO_BIN_EXE_aikit"));
        let before = fs::metadata(binary).unwrap();
        assert!(before.is_file());
        let mut command = Command::new(binary);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("user"))
            .env("AIKIT_HOME", self.root.join("home"))
            .env("CENTRAL_ROOT", self.central())
            .current_dir(self.central())
            .args(["session-space", "-C"])
            .arg(self.central())
            .arg("encounter-epi-prime-configure");
        for flag in [
            "--launcher",
            "--prime-bin",
            "--ql-bin",
            "--research-bin",
            "--faculty-config",
        ] {
            command.arg(flag).arg(binary);
        }
        command
            .args(["--ql-revision", QL_REVISION])
            .args(["--body-revision", BODY_REVISION])
            .arg("--skill-path")
            .arg(self.central());
        for (flag, path) in owner {
            command.arg(flag).arg(path);
        }
        let observed = SystemRunner::new()
            .with_timeout(Duration::from_secs(20))
            .with_output_limit_bytes(512 * 1024)
            .with_strict_utf8()
            .capture_command(&mut command);
        self.calls += 1;
        let prefix = self.root.join(format!("capture-{}", self.calls));
        let output = match observed {
            Ok(output) => output,
            Err(error) => {
                if let Some(capture) = error.native_capture() {
                    fs::write(prefix.with_extension("stdout"), &capture.stdout).unwrap();
                    fs::write(prefix.with_extension("stderr"), &capture.stderr).unwrap();
                }
                let original = error
                    .source()
                    .and_then(|source| source.downcast_ref::<std::io::Error>());
                let facts = json!({
                    "code": error.code(), "message": error.message(), "details": error.details(),
                    "original_io": original.map(|source| json!({
                        "kind": format!("{:?}", source.kind()), "errno": source.raw_os_error()
                    })),
                    "secondary_io": error.secondary_io_sources().map(|source| json!({
                        "kind": format!("{:?}", source.kind()), "errno": source.raw_os_error()
                    })).collect::<Vec<_>>()
                });
                fs::write(prefix.with_extension("failure.json"), facts.to_string()).unwrap();
                panic!("Actual CLI capture/retirement failed; owned fixture retained: {error:?}");
            }
        };
        fs::write(prefix.with_extension("stdout"), &output.stdout).unwrap();
        fs::write(prefix.with_extension("stderr"), &output.stderr).unwrap();
        fs::write(
            prefix.with_extension("result.json"),
            json!({"status": output.status, "stdout_bytes": output.stdout.len(),
                "stderr_bytes": output.stderr.len(), "standing": "actual completed strict capture"})
            .to_string(),
        )
        .unwrap();
        let after = fs::metadata(binary).unwrap();
        assert_eq!(
            (
                before.dev(),
                before.ino(),
                before.len(),
                before.mtime(),
                before.mtime_nsec()
            ),
            (
                after.dev(),
                after.ino(),
                after.len(),
                after.mtime(),
                after.mtime_nsec()
            ),
            "Actual configured/compiled image changed across the invocation"
        );
        self.require_owned();
        output
    }

    fn require_configured(&self, output: &Output) -> EncounterProvider {
        assert_eq!(output.status, 0, "{}", output.stderr);
        assert!(output.stderr.is_empty());
        let reply: Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(reply["configured"], true);
        assert_eq!(reply["standing"], "configured-not-started");
        let provider: EncounterProvider =
            serde_json::from_slice(&fs::read(self.provider_path()).unwrap()).unwrap();
        assert_eq!(provider.id, "epi-prime-ql");
        assert_eq!(
            provider.body_ref.as_deref(),
            Some("agent-body/epi-prime-ql")
        );
        assert_eq!(provider.body_revision.as_deref(), Some(BODY_REVISION));
        assert!(provider.now_context.is_none());
        assert!(provider.model_policy.is_none());
        // Configure never constructs the resident store or starts an owner.
        assert!(!self.root.join("home/state/encounters.sqlite3").exists());
        assert!(!self.root.join("home/state/encounter-owner.log").exists());
        provider
    }

    fn finish(mut self) {
        self.require_owned();
        fs::remove_dir_all(&self.root).unwrap_or_else(|error| {
            panic!(
                "Actual completed fixture cleanup failed at {}: {error:?}",
                self.root.display()
            )
        });
        self.disposed = true;
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.disposed {
            eprintln!(
                "Retained actual configuration/capture fixture: {}",
                self.root.display()
            );
        }
    }
}

fn value<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    let positions: Vec<_> = argv
        .iter()
        .enumerate()
        .filter(|(_, arg)| arg.as_str() == flag)
        .collect();
    assert!(
        positions.len() <= 1,
        "Duplicate native owner argument: {flag}"
    );
    positions
        .first()
        .map(|(index, _)| argv[*index + 1].as_str())
}

#[test]
fn actual_root_configure_preserves_absent_project_and_native_configuration() {
    let mut fixture = Fixture::new();
    let binary = Path::new(env!("CARGO_BIN_EXE_aikit"));
    let root = fixture.central();
    let output = fixture.configure(&[("--central-ctrl-bin", binary), ("--central-root", &root)]);
    let provider = fixture.require_configured(&output);
    assert_eq!(
        value(&provider.argv, "--central-ctrl-bin"),
        binary.canonicalize().unwrap().to_str()
    );
    assert_eq!(
        value(&provider.argv, "--central-root"),
        root.canonicalize().unwrap().to_str()
    );
    assert_eq!(value(&provider.argv, "--central-project"), None);
    fixture.finish();
}

#[test]
fn actual_project_configure_preserves_exact_literal_project() {
    let mut fixture = Fixture::new();
    let binary = Path::new(env!("CARGO_BIN_EXE_aikit"));
    let root = fixture.central();
    let output = fixture.configure(&[
        ("--central-ctrl-bin", binary),
        ("--central-root", &root),
        ("--central-project", Path::new("literal-project/key")),
    ]);
    let provider = fixture.require_configured(&output);
    assert_eq!(
        value(&provider.argv, "--central-project"),
        Some("literal-project/key")
    );
    assert_eq!(
        value(&provider.argv, "--central-root"),
        root.canonicalize().unwrap().to_str()
    );
    fixture.finish();
}

#[test]
fn actual_no_central_configuration_stays_optional() {
    let mut fixture = Fixture::new();
    let output = fixture.configure(&[]);
    let provider = fixture.require_configured(&output);
    for flag in ["--central-ctrl-bin", "--central-root", "--central-project"] {
        assert_eq!(value(&provider.argv, flag), None);
    }
    fixture.finish();
}

fn require_refusal_preserves_provider(fixture: &mut Fixture, owner: &[(&str, &Path)]) {
    let path = fixture.provider_path();
    let before = fs::read(&path).unwrap();
    let metadata = fs::metadata(&path).unwrap();
    let output = fixture.configure(owner);
    assert_eq!(output.status, 1);
    assert!(output.stdout.is_empty());
    assert!(
        output.stderr.starts_with("encounter.prime_configuration:"),
        "{}",
        output.stderr
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let after = fs::metadata(&path).unwrap();
    assert_eq!((metadata.dev(), metadata.ino()), (after.dev(), after.ino()));
}

#[test]
fn actual_partial_central_owner_refuses_before_provider_replacement() {
    let mut fixture = Fixture::new();
    let binary = Path::new(env!("CARGO_BIN_EXE_aikit"));
    let root = fixture.central();
    let project = Path::new("literal-project/key");
    for owner in [
        vec![("--central-ctrl-bin", binary)],
        vec![("--central-root", root.as_path())],
        vec![("--central-project", project)],
        vec![
            ("--central-ctrl-bin", binary),
            ("--central-project", project),
        ],
        vec![
            ("--central-root", root.as_path()),
            ("--central-project", project),
        ],
    ] {
        require_refusal_preserves_provider(&mut fixture, &owner);
    }
    fixture.finish();
}

#[test]
fn actual_missing_or_wrong_form_central_owner_refuses_before_provider_replacement() {
    let mut fixture = Fixture::new();
    let binary = Path::new(env!("CARGO_BIN_EXE_aikit"));
    let root = fixture.central();
    let absent = fixture.root.join("actually-absent");
    assert_eq!(
        fs::symlink_metadata(&absent).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    for owner in [
        vec![
            ("--central-ctrl-bin", absent.as_path()),
            ("--central-root", root.as_path()),
        ],
        vec![
            ("--central-ctrl-bin", binary),
            ("--central-root", absent.as_path()),
        ],
        vec![
            ("--central-ctrl-bin", root.as_path()),
            ("--central-root", root.as_path()),
        ],
        vec![("--central-ctrl-bin", binary), ("--central-root", binary)],
    ] {
        require_refusal_preserves_provider(&mut fixture, &owner);
    }
    fixture.finish();
}
