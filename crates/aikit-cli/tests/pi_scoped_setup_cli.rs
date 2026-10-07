//! Real native CLI regression over an isolated source and native owner store.
//! Synthetic private bytes belong to this test; no host credential, model,
//! Task or canonical Factory owner is read or mutated.

#![cfg(unix)]

use aikit_core::credential::CredentialRef;
use aikit_store::{AikitHome, CredentialBindingStore};
use serde_json::Value;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};

struct NativeOwner {
    root: tempfile::TempDir,
}

impl NativeOwner {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("native-home/.pi/agent/auth.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Declaration and consumer projections must not parse even this
        // deliberately non-JSON source; the provider owns parsing at delivery.
        std::fs::write(&path, b"unparsed synthetic private native source").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        Self { root }
    }

    fn state(&self) -> AikitHome {
        AikitHome::at(self.root.path().join("state"))
    }

    fn source(&self) -> std::path::PathBuf {
        self.root.path().join("native-home/.pi/agent/auth.json")
    }

    fn run(&self, args: &[&str], json: bool, input: Option<&[u8]>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
        command.args(args).arg("-C").arg(self.root.path());
        if json {
            command.arg("--json");
        }
        command
            .env("HOME", self.root.path().join("native-home"))
            .env("AIKIT_HOME", self.root.path().join("state"))
            .env_remove("CENTRAL_ROOT")
            .env_remove("CENTRAL_NATIVE_TOKEN")
            .env_remove("AIKIT_CONTEXT_ID")
            .env_remove("ZAI_API_KEY")
            .env_remove("PI_CODING_AGENT_DIR")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        let mut child = command.spawn().unwrap();
        if let Some(input) = input {
            // A correct refusal may close the pipe before these synthetic
            // bytes are written. The actual refusal, not pipe timing, governs.
            let _ = child.stdin.take().unwrap().write_all(input);
        }
        child.wait_with_output().unwrap()
    }

    fn declare(&self, verb: &str, consumer: &str, expected: &str) -> Output {
        self.run(
            &[
                "credential",
                verb,
                "credential:z-ai",
                "--harness-auth",
                "pi",
                "--provider",
                "zai",
                "--consumer",
                consumer,
                "--purpose",
                "bounded native CLI source and consumer regression",
                "--expires-at",
                "2099-01-01T00:00:00Z",
                "--expected-binding",
                expected,
            ],
            true,
            None,
        )
    }

    fn retained(&self) -> aikit_core::credential::CredentialBindingState {
        CredentialBindingStore::new(&self.state())
            .load(&CredentialRef::new("credential:z-ai").unwrap())
            .unwrap()
            .unwrap()
    }
}

fn data(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "native command refused: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
}

fn refusal(output: &Output, code: &str) {
    assert!(
        !output.status.success(),
        "native command unexpectedly admitted"
    );
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(rendered.contains(code), "wrong native refusal: {rendered}");
    assert!(!rendered.contains("synthetic-would-be-imported-key"));
}

#[test]
fn plain_setup_cannot_replace_or_reopen_an_actual_scoped_native_declaration() {
    let owner = NativeOwner::new();
    let first = data(&owner.declare("setup", "agent-session/parent", "absent"));
    let retained = owner.retained();
    let source = std::fs::read(owner.source()).unwrap();
    assert_eq!(
        first["binding"]["harness_auth_source"]["consumer_ref"],
        "agent-session/parent"
    );
    for (args, json, input) in [
        (
            vec![
                "credential",
                "setup",
                "credential:z-ai",
                "--ref",
                "pass://different-source",
            ],
            true,
            None,
        ),
        (
            vec!["credential", "setup", "credential:z-ai", "--stdin"],
            true,
            Some(b"synthetic-would-be-imported-key\n".as_slice()),
        ),
        (
            vec![
                "credential",
                "setup",
                "credential:z-ai",
                "--from-env",
                "--env-var",
                "ZAI_API_KEY",
            ],
            true,
            None,
        ),
        (
            vec![
                "credential",
                "setup",
                "credential:z-ai",
                "--consumer",
                "agent-session/child",
                "--headless",
            ],
            true,
            None,
        ),
        // No JSON/headless flag: refusal must precede the interactive rebind
        // menu as well, rather than failing later because stdin is closed.
        (
            vec![
                "credential",
                "setup",
                "credential:z-ai",
                "--consumer",
                "agent-session/child",
            ],
            false,
            None,
        ),
    ] {
        let output = owner.run(&args, json, input);
        refusal(
            &output,
            if json {
                "credential.harness_auth_rotation_required"
            } else {
                "retained session-scoped source requires explicit rotation"
            },
        );
        assert_eq!(owner.retained(), retained);
        assert_eq!(std::fs::read(owner.source()).unwrap(), source);
    }
    let reuse = data(&owner.run(
        &[
            "credential",
            "setup",
            "credential:z-ai",
            "--consumer",
            "agent-session/parent",
        ],
        true,
        None,
    ));
    assert_eq!(reuse["newly_bound"], false);
    assert_eq!(reuse["binding"], first["binding"]);
    assert_eq!(owner.retained(), retained);
}

#[test]
fn sequential_native_consumers_require_explicit_rotation_of_the_exact_current_binding() {
    let owner = NativeOwner::new();
    let parent = data(&owner.declare("setup", "agent-session/parent", "absent"));
    let parent_binding = owner.retained();
    let parent_revision = parent["binding_revision"].as_str().unwrap();
    refusal(
        &owner.declare("setup", "agent-session/child", parent_revision),
        "credential.harness_auth_rotation_required",
    );
    assert_eq!(owner.retained(), parent_binding);
    let child = data(&owner.declare("rotate", "agent-session/child", parent_revision));
    let child_binding = owner.retained();
    assert_eq!(
        child_binding.bound_at_unix_seconds,
        parent_binding.bound_at_unix_seconds
    );
    assert!(child_binding.last_rotated_at_unix_seconds.is_some());
    assert_eq!(
        child_binding
            .harness_auth_source
            .as_ref()
            .unwrap()
            .consumer_ref,
        "agent-session/child"
    );
    refusal(
        &owner.declare("rotate", "agent-session/verifier", parent_revision),
        "credential.binding_stale",
    );
    assert_eq!(owner.retained(), child_binding);
    refusal(
        &owner.declare("setup", "agent-session/parent", parent_revision),
        "credential.binding_stale",
    );
    assert_eq!(owner.retained(), child_binding);
    let verifier = data(&owner.declare(
        "rotate",
        "agent-session/verifier",
        child["binding_revision"].as_str().unwrap(),
    ));
    assert_eq!(
        owner
            .retained()
            .harness_auth_source
            .as_ref()
            .unwrap()
            .consumer_ref,
        "agent-session/verifier"
    );
    let retry = data(&owner.declare(
        "setup",
        "agent-session/verifier",
        verifier["binding_revision"].as_str().unwrap(),
    ));
    assert_eq!(retry["newly_bound"], false);
    assert_eq!(retry["binding"], verifier["binding"]);
    assert_eq!(
        CredentialBindingStore::new(&owner.state())
            .list()
            .unwrap()
            .len(),
        1
    );
    for consumer in [
        "agent-session/parent",
        "agent-session/child",
        "operator:aikit",
    ] {
        let explained = data(&owner.run(
            &[
                "credential",
                "explain",
                "credential:z-ai",
                "--consumer",
                consumer,
                "--headless",
            ],
            true,
            None,
        ));
        assert!(explained["resolution"]["selected_provider_ref"].is_null());
        assert_eq!(
            explained["persisted_binding"]["harness_auth_source"]["consumer_ref"],
            "agent-session/verifier"
        );
    }
    assert_eq!(
        std::fs::read(owner.source()).unwrap(),
        b"unparsed synthetic private native source"
    );
}
