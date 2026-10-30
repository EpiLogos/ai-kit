//! `aikit z` end to end through the real binary.
//!
//! The acceptance criterion is precise: **`z` acts, and never activates.** Running
//! a capability and making it active are different acts; a fuzzy match is consent
//! to the first and never to the second.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A registry with two scripts, neither enabled anywhere.
fn seed(home: &Path) {
    for (id, leaf) in [
        ("script/test/cargo-nextest", "cargo-nextest"),
        ("script/test/cargo-nextest-helper", "cargo-nextest-helper"),
    ] {
        let base = home.join(format!("registries/personal/capsules/{id}"));
        write(
            &base.join("manifest.toml"),
            &format!(
                r#"schema = 1
id = "{id}"
kind = "script"
name = "{leaf}"
description = "Runs tests for the z test."

[script]
entry = "payload/run.sh"
exports = ["{leaf}"]
"#
            ),
        );
        let run = base.join("payload/run.sh");
        write(&run, "#!/bin/sh\necho ran-$0\n");
        let mut perms = fs::metadata(&run).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&run, perms).unwrap();
    }
}

fn z(home: &Path, project: &Path, args: &[&str]) -> Value {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .arg("z")
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .current_dir(project)
        .output()
        .expect("aikit z runs");
    assert!(
        output.status.success(),
        "z failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("z emits JSON")
}

#[test]
fn z_finds_a_capability_by_a_fragment_of_its_name_and_never_activates_it() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    seed(home.path());
    // A project that declares NOTHING: nothing is active here.
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");

    let envelope = z(home.path(), project.path(), &["nextest"]);
    let data = &envelope["data"];

    assert_eq!(envelope["ok"], Value::Bool(true));
    assert_eq!(data["decision"], "act", "one clear winner: {data}");
    assert_eq!(
        data["capability"], "script/test/cargo-nextest",
        "the exact leaf beats the longer one: {data}"
    );
    assert_eq!(
        data["action"], "run",
        "a script's natural action is to run it"
    );
    assert_eq!(
        data["activated"],
        Value::Bool(false),
        "z proposes running; it never makes anything active"
    );

    // And nothing on disk changed: no profile was written, nothing was enabled.
    let profile = fs::read_to_string(project.path().join(".aikit/profile.toml")).unwrap();
    assert_eq!(
        profile.trim(),
        "schema = 1",
        "z must not have written a declaration: {profile}"
    );
}

#[test]
fn z_returns_ranked_candidates_for_an_agent_to_decide_from() {
    // SPEC-III §3.3: an agent gets the same affordance a human does, headless.
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    seed(home.path());
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");

    let data = z(home.path(), project.path(), &["cargo"])["data"].clone();
    let candidates = data["candidates"].as_array().expect("candidates array");

    assert!(candidates.len() >= 2, "both matched: {data}");
    let first = candidates[0]["score"].as_f64().unwrap();
    let second = candidates[1]["score"].as_f64().unwrap();
    assert!(first >= second, "candidates arrive ranked best first");
    assert!(
        candidates.iter().all(|c| c["active"] == Value::Bool(false)),
        "nothing was activated by asking"
    );
}

#[test]
fn z_reports_nothing_rather_than_running_something_unrelated() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    seed(home.path());
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");

    let data = z(home.path(), project.path(), &["zzzz-no-such-thing"])["data"].clone();
    assert_eq!(data["decision"], "nothing");
    assert!(data["candidates"].as_array().unwrap().is_empty());
}

/// A Central with two project worlds, `Work/Alpha` and `Work/Beta`, each with its
/// own ProjectCentral identity, and an owner stub that lists no agent profile and
/// answers nothing else, so no read reaches the person's real Central. Returns the
/// Central root.
fn central_with_two_worlds(temp: &Path) -> std::path::PathBuf {
    let root = temp.join("Central");
    fs::create_dir_all(root.join(".aikit")).unwrap();
    fs::create_dir_all(root.join("Control")).unwrap();
    fs::create_dir_all(temp.join("home")).unwrap();
    for name in ["Alpha", "Beta"] {
        write(
            &root.join(format!("Work/{name}/ProjectCentral/project.json")),
            &serde_json::json!({"schema": "central.project/v1", "project_id": name,
                "human_source": "ProjectCentral/user",
                "wiki": {"profile": "okf-wiki/v1", "source": "ProjectCentral/agents/wiki/wiki.json",
                    "adopted_sources": []}})
            .to_string(),
        );
    }
    let stub = temp.join("ctrl");
    write(
        &stub,
        r#"#!/bin/sh
case "$*" in
  *agent-profile.list*) printf '%s' '{"ok":true,"data":{"profiles":[]}}' ;;
  *) printf '%s' '{"ok":false,"error":{"code":"central.world_declaration_absent","message":"not stubbed"}}'; exit 2 ;;
esac
"#,
    );
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    for (id, leaf) in [
        ("script/test/deploy-red", "deploy-red"),
        ("script/test/deploy-blue", "deploy-blue"),
    ] {
        let base = temp.join(format!("aikit-home/registries/personal/capsules/{id}"));
        write(
            &base.join("manifest.toml"),
            &format!(
                "schema = 1\nid = \"{id}\"\nkind = \"script\"\nname = \"{leaf}\"\n\
                 description = \"Deploys for the world test.\"\n\n[script]\n\
                 entry = \"payload/run.sh\"\nexports = [\"{leaf}\"]\n"
            ),
        );
        let run = base.join("payload/run.sh");
        write(&run, "#!/bin/sh\nexit 0\n");
        fs::set_permissions(&run, fs::Permissions::from_mode(0o755)).unwrap();
    }
    root
}

/// `aikit --json -C <cwd> ARGS` in the scratch Central, as the O:I kernel asks it:
/// the world is the ground it is asked in, and no AIKIT_PROJECT_ID is exported.
fn aikit_in(temp: &Path, cwd: &Path, args: &[&str]) -> Value {
    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .arg("--json")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temp.join("home"))
        .env("AIKIT_HOME", temp.join("aikit-home"))
        .env("CENTRAL_ROOT", temp.join("Central"))
        .env("CENTRAL_CTRL_BIN", temp.join("ctrl"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the real aikit binary runs");
    assert!(
        output.status.success(),
        "aikit {args:?} in {}: {}{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.stdout.is_empty() {
        // `run` answers with the capability's own exit status, not an envelope.
        return Value::Null;
    }
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].take()
}

/// O-I #820 (DLW §18.4: each world you work in keeps its own uses). AIKit takes the
/// world from the folder it is asked in (#570); `z` and the palette's Search rank by
/// that world's uses too. Deploy-red is run twice in Alpha, deploy-blue three times
/// in Beta: asked in Alpha both surfaces put red first, asked in Beta blue, and the
/// person's own world, where neither was run, keeps the order it had.
#[test]
fn z_and_the_palette_rank_the_uses_of_the_world_they_are_asked_in() {
    let temp = TempDir::new().unwrap();
    let root = central_with_two_worlds(temp.path());
    let (alpha, beta) = (root.join("Work/Alpha"), root.join("Work/Beta"));
    let first = |cwd: &Path| -> (String, String) {
        let z = aikit_in(temp.path(), cwd, &["z", "deploy"]);
        let search = aikit_in(temp.path(), cwd, &["search", "deploy"]);
        (
            z["candidates"][0]["capability"]
                .as_str()
                .unwrap_or_else(|| panic!("z ranks candidates in {}: {z}", cwd.display()))
                .to_string(),
            search["rows"][0]["id"]
                .as_str()
                .unwrap_or_else(|| panic!("Search finds rows in {}: {search}", cwd.display()))
                .to_string(),
        )
    };
    let unused = first(&root);

    for _ in 0..2 {
        aikit_in(
            temp.path(),
            &alpha,
            &["run", "--confirm", "script/test/deploy-red"],
        );
    }
    for _ in 0..3 {
        aikit_in(
            temp.path(),
            &beta,
            &["run", "--confirm", "script/test/deploy-blue"],
        );
    }

    let red = "script/test/deploy-red".to_string();
    let blue = "script/test/deploy-blue".to_string();
    assert_eq!(
        first(&alpha),
        (red.clone(), red),
        "asked in Alpha, z and Search rank Alpha's two runs first"
    );
    assert_eq!(
        first(&beta),
        (blue.clone(), blue),
        "asked in Beta, z and Search rank Beta's three runs first"
    );
    assert_eq!(
        first(&root),
        unused,
        "the person's own world ran neither, so its order is unmoved"
    );
}
