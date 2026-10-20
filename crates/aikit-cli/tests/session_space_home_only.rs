//! Which `aikit session-space` verbs build the whole Service, pinned through a home whose index cannot be opened.
//!
//! Building the Service hashes every capsule of the registry, resolves trust and the view, and discovers the project through two `git`
//! processes — 60 of the 63 ms of an `encounter-task-read` (profile, 20 Oct). Nearly every verb of the family needs only the AIKit
//! home: the store behind each `session_space_*` operation is built from it, and the encounter verbs take it and nothing else. So
//! those verbs do not build the Service, and a home whose sqlite index will not open does not stop them; the verbs that plan a Direct
//! session or resolve a project context still build it, and still fail there.
use std::{path::Path, process::Command};

fn call(home: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aikit"))
        .arg("session-space")
        .env("AIKIT_HOME", home)
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap()
}

/// A home whose index file is not a database, and a directory to stand in.
fn unopenable_index() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(home.join("state")).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(home.join("state/aikit.sqlite3"), [0x5a_u8; 4096]).unwrap();
    (temp, home, project)
}

#[test]
fn the_verbs_that_need_only_the_home_run_without_the_service() {
    let (_temp, home, project) = unopenable_index();
    let listed = call(&home, &project, &["list"]);
    assert!(
        listed.status.success(),
        "list needs only the home: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&listed.stdout).trim(), "[]");
    let discovered = call(&home, &project, &["discover"]);
    assert!(
        discovered.status.success(),
        "{}",
        String::from_utf8_lossy(&discovered.stderr)
    );
    // An encounter verb: it reaches its own refusal for a session nothing has prepared, not the index's.
    let task = call(
        &home,
        &project,
        &[
            "encounter-task-read",
            "--agent-session",
            "agent-session/never-prepared",
        ],
    );
    let said = String::from_utf8_lossy(&task.stderr);
    assert!(
        !said.contains("index.open_failed"),
        "an encounter read does not open the Service's index: {said}"
    );
    // The layout a home must have is still made, as `Service::open` made it first.
    for directory in [
        "registries",
        "inbox",
        "state/sessions",
        "state/locks",
        "cache",
    ] {
        assert!(
            home.join(directory).is_dir(),
            "{directory} is part of the layout a verb leaves behind"
        );
    }
}

#[test]
fn the_verbs_that_plan_a_session_or_resolve_a_context_still_build_the_service() {
    let (_temp, home, project) = unopenable_index();
    for args in [
        &["project-context"][..],
        &["agent-session-scope"][..],
        &["agent-session-skills"][..],
    ] {
        let output = call(&home, &project, args);
        assert!(
            !output.status.success(),
            "{args:?} needs the Service, so it meets the unopenable index"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("index.open_failed"),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
