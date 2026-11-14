//! Real native encounter owner, isolated home, and no provider or model call.
//! A JSON refusal is a failed Routine run even when IPC transport succeeded.
#![cfg(unix)]
use aikit_cli::{
    encounter_service::{request, socket_path, EncounterRequest},
    routine_dispatch::{ResidentEncounterRunner, RoutineRunRequest, RoutineRunner, RunStatus},
};
use aikit_core::{ResourceRef, SourceRevision};
use aikit_store::AikitHome;
use std::process::Command;

#[test]
fn a_real_owner_refusal_never_becomes_a_prepared_draft_or_completed_run() {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("work");
    std::fs::create_dir(&cwd).unwrap();
    let home = AikitHome::at(temp.path().join("aikit"));
    let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
        .args(["session-space", "encounter-start", "-C"])
        .arg(&cwd)
        .env("AIKIT_HOME", home.root())
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let started: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let pid = started["data"]["pid"].as_u64().unwrap() as u32;
    struct Owner {
        home: AikitHome,
        pid: u32,
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = request(
                &socket_path(&self.home),
                &EncounterRequest::Shutdown {
                    expected_pid: self.pid,
                },
            );
        }
    }
    let owner = Owner {
        home: home.clone(),
        pid,
    };
    let r = |value: &str| ResourceRef::parse(value).unwrap();
    let returned = ResidentEncounterRunner { home: home.clone() }.run(RoutineRunRequest {
        routine_ref: r("routine/native-refusal"),
        invocation_ref: r("routine-invocation/native-refusal"),
        trigger_observation_ref: r("trigger-observation/native-refusal"),
        method_ref: r("method/native-refusal"),
        method_revision: SourceRevision::parse("revision/native-refusal").unwrap(),
        prompt: "No inference may occur in this refusal test".into(),
        observation_payload: None,
        time_schedule: None,
        native: None,
        authorised_actions: Vec::new(),
    });
    assert_eq!(returned.status, RunStatus::Failed, "{returned:?}");
    assert!(
        returned.detail.starts_with("session open failed:"),
        "{returned:?}"
    );
    assert!(!returned.detail.contains("draft prepared"));
    drop(owner);
}

#[test]
fn a_routine_dispatch_runs_after_the_owner_it_found_has_closed_itself() {
    // An owner nobody asks closes itself after its idle grace (here one second). A dispatch after it reaches its owner
    // through the one start path, as every caller does: the run reaches an owner, which refuses its open (no provider is
    // configured here), and is never left unreturned for want of an owner another left running.
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("work");
    std::fs::create_dir(&cwd).unwrap();
    let home = AikitHome::at(temp.path().join("aikit"));
    let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
        .args(["session-space", "encounter-start", "-C"])
        .arg(&cwd)
        .env("AIKIT_HOME", home.root())
        .env("AIKIT_ENCOUNTER_IDLE_CLOSE_SECONDS", "1")
        .env_remove("AIKIT_CONTEXT_ID")
        .env_remove("AIKIT_ISOLATION")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Nothing asks it (a Health would be an ask): its socket goes when it closes.
    let socket = socket_path(&home);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while socket.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the owner nobody asked did not close itself"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // The start path starts AIKit's own binary; this test's executable is not AIKit's.
    std::env::set_var("AIKIT_ENCOUNTER_OWNER_BIN", env!("CARGO_BIN_EXE_aikit"));
    struct Started(std::path::PathBuf);
    impl Drop for Started {
        fn drop(&mut self) {
            if let Ok(health) = request(&self.0, &EncounterRequest::Health) {
                if let Some(pid) = health["data"]["pid"].as_u64() {
                    let _ = request(
                        &self.0,
                        &EncounterRequest::Shutdown {
                            expected_pid: pid as u32,
                        },
                    );
                }
            }
        }
    }
    let _started = Started(socket.clone());
    let r = |value: &str| ResourceRef::parse(value).unwrap();
    let returned = ResidentEncounterRunner { home: home.clone() }.run(RoutineRunRequest {
        routine_ref: r("routine/after-idle-close"),
        invocation_ref: r("routine-invocation/after-idle-close"),
        trigger_observation_ref: r("trigger-observation/after-idle-close"),
        method_ref: r("method/after-idle-close"),
        method_revision: SourceRevision::parse("revision/after-idle-close").unwrap(),
        prompt: "No inference may occur in this test".into(),
        observation_payload: None,
        time_schedule: None,
        native: None,
        authorised_actions: Vec::new(),
    });
    assert_eq!(returned.status, RunStatus::Failed, "{returned:?}");
    assert!(
        returned.detail.starts_with("session open failed:"),
        "{returned:?}"
    );
    assert!(
        socket.exists(),
        "the dispatch's owner stands at the home's socket"
    );
}

#[test]
fn execution_history_cannot_create_an_invocation_or_authority() {
    use aikit_store::{
        routine_invocation::{RoutineExecutionOutcome, RoutineExecutionStatus},
        RoutineInvocationStore,
    };
    let temp = tempfile::tempdir().unwrap();
    let store = RoutineInvocationStore::new(AikitHome::at(temp.path()));
    let error = store
        .record_outcome(
            &ResourceRef::parse("routine-invocation/not-admitted").unwrap(),
            RoutineExecutionOutcome {
                status: RoutineExecutionStatus::Completed,
                detail: "Unadmitted work".into(),
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), "routine.invocation_not_found");
    assert!(store.history().unwrap().is_empty());
    assert!(
        !store.path().exists(),
        "no invocation ledger is manufactured by a return"
    );
}
