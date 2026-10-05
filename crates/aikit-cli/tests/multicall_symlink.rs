//! The multicall shim, end to end through the real binary.
//!
//! A registry, a project, a resolved generation and a `bin/` export are all built
//! on a real filesystem; then a **real symlink** named after the export points at
//! the `aikit` binary and is invoked. The binary must notice it was not called as
//! `aikit`, find the context's current generation, locate the capsule that owns
//! the export at the applied revision, and run its real payload.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;

use aikit_cli::app::{AikitApplication, ApplyRequest, Service};
use aikit_core::scope::ScopeKind;
use aikit_store::home::AikitHome;
use tempfile::TempDir;

fn native_tempdir() -> TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
    fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("native-multicall-")
        .tempdir_in(root)
        .unwrap()
}

const CONTEXT_ID: &str = "ctx_01HZYMULTICALL0000000000";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A personal registry with one script capsule that exports `greet`.
fn seed_registry(home: &Path) {
    let base = home.join("registries/personal/capsules/script/demo/greet");
    write(
        &base.join("manifest.toml"),
        r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "Greets the world for the multicall test."

[script]
entry = "payload/run.sh"
interpreter = ["/bin/sh"]
exports = ["greet"]
"#,
    );
    let run = base.join("payload/run.sh");
    write(&run, "#!/bin/sh\necho \"hello $1\"\n");
    let mut perms = fs::metadata(&run).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&run, perms).unwrap();
}

#[test]
fn a_symlink_named_after_an_export_runs_the_capsule_that_owns_it() {
    let home_dir = native_tempdir();
    let project = native_tempdir();
    let home_path = home_dir.path();

    seed_registry(home_path);

    // The project enables the capability.
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    // Build and commit a generation for a fixed context, in process, using the
    // real service — this is the same code `aikit apply` runs.
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let home = AikitHome::at(home_path);
    let mut service =
        Service::open(home, project.path(), |k| env.get(k).cloned()).expect("service opens");
    service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .expect("apply builds a generation");

    // The multicall path gates on trust, so review the capsule before invoking.
    review(home_path, project.path(), "script/demo/greet");

    // Symlink `greet` -> the aikit binary.
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let link = project.path().join("greet");
    symlink(&bin, &link).unwrap();

    // Invoke through the symlink.
    let output = std::process::Command::new(&link)
        .arg("world")
        .env("AIKIT_HOME", home_path)
        .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
        .current_dir(project.path())
        .output()
        .expect("the symlinked binary runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "exit={:?} stdout={stdout:?} stderr={:?}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("hello world"),
        "the capsule's real payload should have run; got {stdout:?}"
    );
}

/// Review a capsule in the store the running binary will read, so a symlink
/// invocation of it passes the trust gate.
fn review(home_path: &Path, project: &Path, id: &str) {
    use aikit_core::catalog::Catalog;
    use aikit_core::id::CapsuleId;
    use aikit_core::trust::{TrustKey, TrustState};
    use aikit_store::index::Index;
    use aikit_store::trust::TrustStore;

    let home = AikitHome::at(home_path);
    let load = aikit_cli::app::load_catalog(&home, Some(project)).unwrap();
    let cid = CapsuleId::parse(id).unwrap();
    let capsule = Catalog::get(&load.catalog, &cid).expect("capsule is catalogued");
    let key = TrustKey::new(
        capsule.source.clone().unwrap(),
        capsule.id.clone(),
        capsule.revision.clone().unwrap(),
    );
    let index = Index::open(&home.database()).unwrap();
    TrustStore::new(&index)
        .record(&key, TrustState::Reviewed, None)
        .unwrap();
}

#[test]
fn an_unreviewed_export_invoked_by_symlink_refuses_with_trust_required() {
    // The multicall shim is on the PATH, so a symlink named after an export runs
    // the capsule unattended. That must be gated by the same trust check the
    // interactive `aikit run` uses: an unreviewed executable REFUSES rather than
    // silently running whatever a `git pull` left in a bin/ shim.
    let home_dir = native_tempdir();
    let project = native_tempdir();
    let home_path = home_dir.path();
    seed_registry(home_path);
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    // Apply the generation, but do NOT review the capsule: it stays `Unseen`.
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let home = AikitHome::at(home_path);
    let mut service = Service::open(home, project.path(), |k| env.get(k).cloned()).unwrap();
    service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .unwrap();

    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let link = project.path().join("greet");
    symlink(&bin, &link).unwrap();

    let output = std::process::Command::new(&link)
        .arg("world")
        .env("AIKIT_HOME", home_path)
        .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
        .current_dir(project.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "an unreviewed export must refuse to run"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--confirm") || stderr.contains("review"),
        "the refusal must tell the user how to proceed; got {stderr:?}"
    );
    // It must NOT have run the payload.
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("hello world"),
        "the unreviewed payload must not have executed"
    );
}

#[test]
fn a_reviewed_export_invoked_by_symlink_runs() {
    // The other side of the gate: once the capsule is reviewed, the same symlink
    // invocation runs its real payload.
    let home_dir = native_tempdir();
    let project = native_tempdir();
    let home_path = home_dir.path();
    seed_registry(home_path);
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let home = AikitHome::at(home_path);
    let mut service = Service::open(home, project.path(), |k| env.get(k).cloned()).unwrap();
    service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .unwrap();

    review(home_path, project.path(), "script/demo/greet");

    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let link = project.path().join("greet");
    symlink(&bin, &link).unwrap();

    let output = std::process::Command::new(&link)
        .arg("world")
        .env("AIKIT_HOME", home_path)
        .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
        .current_dir(project.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "a reviewed export runs; stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("hello world"));
}

#[test]
fn an_unknown_export_name_is_reported_not_silently_ignored() {
    let home_dir = native_tempdir();
    let project = native_tempdir();
    seed_registry(home_dir.path());
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    let home = AikitHome::at(home_dir.path());
    let mut service = Service::open(home, project.path(), |k| env.get(k).cloned()).unwrap();
    service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .unwrap();

    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let link = project.path().join("nonexistent-export");
    symlink(&bin, &link).unwrap();

    let output = std::process::Command::new(&link)
        .env("AIKIT_HOME", home_dir.path())
        .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
        .current_dir(project.path())
        .output()
        .unwrap();

    assert!(!output.status.success(), "an unknown export must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nonexistent-export"),
        "the error should name the export; got {stderr:?}"
    );
}

fn captured_export(body: &str, extra: &str, reviewed: bool) -> (TempDir, TempDir) {
    let home = native_tempdir();
    let project = native_tempdir();
    seed_registry(home.path());
    let capsule = home
        .path()
        .join("registries/personal/capsules/script/demo/greet");
    let manifest = capsule.join("manifest.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, format!("{text}mode = \"capture\"\n{extra}")).unwrap();
    fs::write(capsule.join("payload/run.sh"), body).unwrap();
    write(
        &project.path().join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );
    let env = BTreeMap::from([("AIKIT_CONTEXT_ID".to_owned(), CONTEXT_ID.to_owned())]);
    let mut service = Service::open(AikitHome::at(home.path()), project.path(), |key| {
        env.get(key).cloned()
    })
    .unwrap();
    service
        .apply(ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: None,
        })
        .unwrap();
    if reviewed {
        review(home.path(), project.path(), "script/demo/greet");
    }
    symlink(
        assert_cmd::cargo::cargo_bin("aikit"),
        project.path().join("greet"),
    )
    .unwrap();
    (home, project)
}

fn capture_native_cli(
    home: &Path,
    project: &Path,
    export: bool,
    args: &[&str],
) -> aikit_adapters::runner::Output {
    let binary = if export {
        project.join("greet")
    } else {
        assert_cmd::cargo::cargo_bin("aikit")
    };
    let mut command = std::process::Command::new(binary);
    command
        .args(args)
        .env("AIKIT_HOME", home)
        .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
        .env_remove("CENTRAL_ROOT")
        .env_remove("CENTRAL_CTRL_BIN")
        .current_dir(project);
    aikit_adapters::runner::SystemRunner::new()
        .with_strict_utf8()
        .with_timeout(std::time::Duration::from_secs(20))
        .with_output_limit_bytes(1024 * 1024)
        .capture_command(&mut command)
        .expect("actual CLI must complete bounded native capture")
}

#[test]
fn actual_applied_capture_export_and_run_cli_deliver_exact_streams_and_status() {
    let (home, project) = captured_export(
        "#!/bin/sh\nprintf '[%s][%s]' \"$1\" \"$RETURN_TOKEN\"\nprintf 'native-error-tail' >&2\nexit 7\n",
        "env = { RETURN_TOKEN = \"native-token\" }\n", true);
    for (export, args) in [
        (true, vec!["arg with spaces"]),
        (
            false,
            vec!["run", "script/demo/greet", "--", "arg with spaces"],
        ),
    ] {
        let output = capture_native_cli(home.path(), project.path(), export, &args);
        assert_eq!(output.status, 7, "{output:?}");
        assert_eq!(output.stdout, "[arg with spaces][native-token]");
        assert_eq!(output.stderr, "native-error-tail");
    }
}

#[test]
fn actual_capture_export_still_refuses_unreviewed_and_changed_applied_revisions() {
    let (home, project) = captured_export("#!/bin/sh\nprintf 'must-not-run'\n", "", false);
    let output = capture_native_cli(home.path(), project.path(), true, &[]);
    assert_ne!(output.status, 0);
    assert!(!output.stdout.contains("must-not-run"));
    assert!(
        output.stderr.contains("review") || output.stderr.contains("--confirm"),
        "{output:?}"
    );
    review(home.path(), project.path(), "script/demo/greet");
    let output = capture_native_cli(home.path(), project.path(), true, &[]);
    assert_eq!(output.status, 0, "{output:?}");
    assert_eq!(output.stdout, "must-not-run");
    fs::write(
        home.path()
            .join("registries/personal/capsules/script/demo/greet/payload/run.sh"),
        "#!/bin/sh\nprintf 'changed-must-not-run'\n",
    )
    .unwrap();
    let output = capture_native_cli(home.path(), project.path(), true, &[]);
    assert_ne!(output.status, 0);
    assert!(!output.stdout.contains("changed-must-not-run"));
    assert!(
        output.stderr.contains("changed") && output.stderr.contains("re-apply"),
        "{output:?}"
    );
}

#[test]
fn actual_capture_export_reuses_the_declared_deadline_and_scope() {
    let (home, project) = captured_export(
        "#!/bin/sh\nprintf '%s' \"$1\"\nprintf 'private-export-stderr-canary' >&2\nsleep 30\n",
        "timeout = \"1s\"\n",
        true,
    );
    let argument = "private-export-input-canary";
    let output = capture_native_cli(home.path(), project.path(), true, &[argument]);
    assert_eq!(output.status, aikit_cli::json::EXIT_GENERIC, "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "partial stdout cannot become a completed export result"
    );
    assert!(
        output.stderr.contains("did not finish within"),
        "{output:?}"
    );
    assert!(!output.stderr.contains(argument));
    assert!(!output.stderr.contains("private-export-stderr-canary"));
    assert!(!output.stderr.contains(home.path().to_str().unwrap()));
    assert!(!output.stderr.contains(project.path().to_str().unwrap()));
    let json_output = capture_native_cli(
        home.path(),
        project.path(),
        false,
        &["--json", "run", "script/demo/greet", "--", argument],
    );
    assert_eq!(
        json_output.status,
        aikit_cli::json::EXIT_GENERIC,
        "{json_output:?}"
    );
    assert!(json_output.stderr.is_empty());
    let diagnostic: serde_json::Value = serde_json::from_str(&json_output.stdout).unwrap();
    assert_eq!(diagnostic["ok"], false);
    assert_eq!(diagnostic["error"]["code"], "mux.command_timeout");
    assert_eq!(
        diagnostic["error"]["details"]["captured_stdout_bytes"],
        argument.len().to_string()
    );
    assert_eq!(
        diagnostic["error"]["details"]["captured_stderr_bytes"],
        "private-export-stderr-canary".len().to_string()
    );
    assert!(!json_output.stdout.contains(argument));
    assert!(!json_output.stdout.contains("private-export-stderr-canary"));
    assert!(!json_output.stdout.contains(home.path().to_str().unwrap()));
    assert!(!json_output
        .stdout
        .contains(project.path().to_str().unwrap()));
    // The actual selected context still resolves the applied capsule. A foreign
    // context has no generation and cannot use this export or run its payload.
    let mut command = std::process::Command::new(project.path().join("greet"));
    command
        .env("AIKIT_HOME", home.path())
        .env("AIKIT_CONTEXT_ID", "ctx_01HZYMULTICALL0000000001")
        .env_remove("CENTRAL_ROOT")
        .env_remove("CENTRAL_CTRL_BIN")
        .current_dir(project.path());
    let foreign = aikit_adapters::runner::SystemRunner::new()
        .with_strict_utf8()
        .with_timeout(std::time::Duration::from_secs(20))
        .with_output_limit_bytes(1024 * 1024)
        .capture_command(&mut command)
        .unwrap();
    assert_ne!(foreign.status, 0);
    assert!(
        foreign.stderr.contains("no applied generation"),
        "{foreign:?}"
    );
    assert!(!foreign.stdout.contains(argument));
    assert!(!foreign.stderr.contains("private-export-stderr-canary"));
    assert!(
        !foreign.stderr.contains("did not finish within"),
        "{foreign:?}"
    );
}

#[test]
fn native_looking_arguments_do_not_replace_an_applied_multicall_export() {
    use std::error::Error;
    for state in ["reviewed", "unreviewed", "changed-after-apply"] {
        let mut home_dir = native_tempdir();
        let mut project = native_tempdir();
        // Retained before the first native effect: failed capture or unknown
        // process retirement must not delete the actual applied generation.
        home_dir.disable_cleanup(true);
        project.disable_cleanup(true);
        let home_path = home_dir.path();
        seed_registry(home_path);
        write(
            &project.path().join(".aikit/profile.toml"),
            "schema = 1\nenable = [\"script/demo/greet\"]\n",
        );
        let mut env = BTreeMap::new();
        env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
        let home = AikitHome::at(home_path);
        let mut service = Service::open(home, project.path(), |key| env.get(key).cloned()).unwrap();
        service
            .apply(ApplyRequest {
                scope: ScopeKind::Project,
                toggles: vec![],
                label: None,
            })
            .unwrap();
        if state != "unreviewed" {
            review(home_path, project.path(), "script/demo/greet");
        }
        if state == "changed-after-apply" {
            write(
                &home_path.join("registries/personal/capsules/script/demo/greet/payload/run.sh"),
                "#!/bin/sh\necho SOURCE_CHANGED\n",
            );
        }
        let link = project.path().join("greet");
        symlink(assert_cmd::cargo::cargo_bin("aikit"), &link).unwrap();
        let mut command = std::process::Command::new(&link);
        command
            .args(["session-space", "encounter-start"])
            .env("AIKIT_HOME", home_path)
            .env("AIKIT_CONTEXT_ID", CONTEXT_ID)
            .current_dir(project.path());
        let result = aikit_adapters::runner::SystemRunner::new()
            .with_strict_utf8()
            .with_body_free_diagnostics()
            .with_timeout(std::time::Duration::from_secs(20))
            .with_output_limit_bytes(1024 * 1024)
            .capture_command(&mut command);
        let capture = match &result {
            Ok(output) => {
                fs::write(
                    project.path().join("actual-native-export.stdout"),
                    &output.stdout,
                )
                .unwrap();
                fs::write(
                    project.path().join("actual-native-export.stderr"),
                    &output.stderr,
                )
                .unwrap();
                serde_json::json!({"status":output.status,"capture":"completed", "case":state,
                    "home":home_path,"project":project.path(),"automaticRetry":false})
            }
            Err(error) => {
                if let Some(raw) = error.native_capture() {
                    fs::write(
                        project.path().join("actual-native-export.stdout"),
                        &raw.stdout,
                    )
                    .unwrap();
                    fs::write(
                        project.path().join("actual-native-export.stderr"),
                        &raw.stderr,
                    )
                    .unwrap();
                }
                serde_json::json!({"code":error.code(),"message":error.message(),
                    "details":error.details(),"capture":"refused", "case":state,
                    "cause":error.source().map(ToString::to_string),
                    "secondaryCauses":error.secondary_io_sources().map(ToString::to_string).collect::<Vec<_>>(),
                    "privatePartialBytesRetained":error.native_capture().is_some(),
                    "home":home_path,"project":project.path(),"automaticRetry":false})
            }
        };
        fs::write(
            project.path().join("actual-native-export-outcome.json"),
            serde_json::to_vec_pretty(&capture).unwrap(),
        )
        .unwrap();
        let output =
            result.expect("actual finite capture refused; original bytes and fixtures retained");
        let stdout = &output.stdout;
        let stderr = &output.stderr;
        match state {
            "reviewed" => {
                assert_eq!(output.status, 0, "{stderr}");
                assert!(stdout.contains("hello session-space"), "{stdout}");
            }
            "unreviewed" => {
                assert_ne!(output.status, 0);
                assert!(
                    stderr.contains("review") || stderr.contains("--confirm"),
                    "{stderr}"
                );
                assert!(!stdout.contains("hello"));
            }
            "changed-after-apply" => {
                assert_ne!(output.status, 0);
                assert!(
                    stderr.contains("has changed since the generation was applied"),
                    "{stderr}"
                );
                assert!(!stdout.contains("SOURCE_CHANGED"));
            }
            _ => unreachable!(),
        }
        assert!(
            !home_path.join("state/encounter-owner.log").exists(),
            "a real export or its refusal may not become native owner startup"
        );
    }
}
