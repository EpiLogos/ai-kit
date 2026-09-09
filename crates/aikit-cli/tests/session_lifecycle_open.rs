//! U3 session lifecycle owner cell: durable history, thinking, cancellation
//! and permission events with stable identities.
//!
//! The in-process tests drive the canonical owner operations through the
//! injected `Service` home; the binary test drives the same owner surface
//! through the real built CLI against an isolated `AIKIT_HOME`, including
//! the permission join across separate process invocations — the exact
//! shape Actuation, an O:I kernel and a UI must be able to agree on.

use std::fs;
use std::path::Path;

use aikit_cli::app::Service;
use aikit_cli::SessionLifecycleServiceOps;
use aikit_core::session_lifecycle::{SessionLifecycleRecord, SessionLifecycleState};
use aikit_core::{PermissionRequestId, SessionActivityId, SessionId};
use aikit_store::home::AikitHome;
use aikit_store::SessionLifecycleStore;
use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::TempDir;

const SESSION: &str = "ses_ownerlifecycle";
const ACTIVITY: &str = "act_ownerjoin";
const REQUEST: &str = "prq_ownerjoin1";

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn scene() -> (TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    (home, project)
}

fn open_service(home: &TempDir, project: &TempDir) -> Service {
    let home = AikitHome::at(home.path());
    home.ensure_layout().unwrap();
    Service::open(home, project.path(), |_| None).expect("open production application service")
}

fn sid() -> SessionId {
    SessionId::parse(SESSION).unwrap()
}

fn activity() -> SessionActivityId {
    SessionActivityId::parse(ACTIVITY).unwrap()
}

fn request() -> PermissionRequestId {
    PermissionRequestId::parse(REQUEST).unwrap()
}

#[test]
fn full_lifecycle_is_durable_and_carries_stable_identities_across_reopen() {
    let (home_dir, project_dir) = scene();
    let service = open_service(&home_dir, &project_dir);

    // A complete lifecycle on one explicit activity identity.
    let started = service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::Start,
            Some(activity()),
            "operator",
        )
        .unwrap();
    assert_eq!(started.kind.as_str(), "session-started");
    assert_eq!(started.activity, activity());

    service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::Thinking {
                state: "awaiting-tool".into(),
            },
            Some(activity()),
            "agent:worker",
        )
        .unwrap();
    let issued = service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::PermissionRequest {
                tool: "shell/exec".into(),
                activity: activity(),
                request: Some(request()),
            },
            None,
            "agent:worker",
        )
        .unwrap();
    assert_eq!(issued.permission_request, Some(request()));
    assert_eq!(issued.tool.as_deref(), Some("shell/exec"));

    // The owner answers by quoting the stable request identity verbatim; it
    // recovers the activity identity from the request event, so all three
    // sides of the join quote one activity.
    let granted = service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::PermissionGrant { request: request() },
            None,
            "operator",
        )
        .unwrap();
    assert_eq!(granted.activity, activity());
    assert_eq!(granted.permission_request, Some(request()));

    let cancelled = service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::Cancel {
                reason: "operator interrupt".into(),
            },
            Some(activity()),
            "operator",
        )
        .unwrap();
    assert_eq!(cancelled.reason.as_deref(), Some("operator interrupt"));

    // Durability across a full Service reopen over the same home.
    drop(service);
    let service = open_service(&home_dir, &project_dir);
    let history = service.session_lifecycle_history(&sid()).unwrap();
    assert_eq!(history.len(), 5);
    assert_eq!(history[0].kind.as_str(), "session-started");
    assert_eq!(history[2].permission_request, Some(request()));
    assert_eq!(history[3].permission_request, Some(request()));
    assert!(history.iter().all(|event| event.activity == activity()));

    let model = service.session_lifecycle_read_model(&sid()).unwrap();
    assert_eq!(model.version, "aikit.session-lifecycle/v1");
    assert_eq!(model.state, SessionLifecycleState::Cancelled);
    assert_eq!(model.activities, vec![activity()]);
    assert!(model.open_permission_requests.is_empty());

    // A cancelled session records nothing further.
    let error = service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::End,
            Some(activity()),
            "operator",
        )
        .unwrap_err();
    assert_eq!(error.code(), "session_lifecycle.already_cancelled");
}

#[test]
fn unknown_sessions_are_explicit_and_reads_never_write() {
    let (home_dir, project_dir) = scene();
    let service = open_service(&home_dir, &project_dir);
    service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::Start,
            Some(activity()),
            "operator",
        )
        .unwrap();

    let store = SessionLifecycleStore::new(service.home().clone());

    let snapshot = |store: &SessionLifecycleStore| -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let root = store.root();
        let mut files = Vec::new();
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push((path.clone(), fs::read(&path).unwrap()));
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    };
    let before = snapshot(&store);

    let unknown = SessionId::parse("ses_absentowner").unwrap();
    let error = service.session_lifecycle_history(&unknown).unwrap_err();
    assert_eq!(error.code(), "session_lifecycle.unknown_session");
    let error = service.session_lifecycle_read_model(&unknown).unwrap_err();
    assert_eq!(error.code(), "session_lifecycle.unknown_session");

    // Reads are reads: the whole canonical store is byte-identical after
    // every read above.
    service.session_lifecycle_history(&sid()).unwrap();
    service.session_lifecycle_read_model(&sid()).unwrap();
    service.session_lifecycle_list().unwrap();
    assert_eq!(
        snapshot(&store),
        before,
        "reads must not rewrite the canonical history"
    );

    // Granting a request this history never issued is explicit.
    let error = service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::PermissionGrant {
                request: PermissionRequestId::parse("prq_neverissued").unwrap(),
            },
            None,
            "operator",
        )
        .unwrap_err();
    assert_eq!(error.code(), "session_lifecycle.unknown_permission_request");
}

#[test]
fn end_while_a_permission_request_is_open_is_an_explicit_state() {
    let (home_dir, project_dir) = scene();
    let service = open_service(&home_dir, &project_dir);
    service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::Start,
            Some(activity()),
            "operator",
        )
        .unwrap();
    service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::PermissionRequest {
                tool: "fs/write".into(),
                activity: activity(),
                request: Some(request()),
            },
            None,
            "agent:worker",
        )
        .unwrap();
    let error = service
        .session_lifecycle_record(
            sid(),
            SessionLifecycleRecord::End,
            Some(activity()),
            "operator",
        )
        .unwrap_err();
    assert_eq!(error.code(), "session_lifecycle.permission_request_pending");
}

// ---------------------------------------------------------------------------
// Binary contract: the same owner surface through the real built CLI, with
// the permission join happening across separate process invocations.
// ---------------------------------------------------------------------------

fn run_json(home: &Path, project: &Path, args: &[&str]) -> (std::process::Output, Value) {
    let output = std::process::Command::new(cargo_bin("aikit"))
        .args(args)
        .env("AIKIT_HOME", home)
        .current_dir(project)
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout:?}"));
    (output, value)
}

#[test]
fn binary_lifecycle_records_and_reopens_with_the_same_identities() {
    let (home, project) = scene();
    let session = "ses_binlifecycle";

    let (output, started) = run_json(
        home.path(),
        project.path(),
        &[
            "session",
            "lifecycle",
            "start",
            session,
            "--activity",
            ACTIVITY,
            "--json",
        ],
    );
    assert!(output.status.success(), "{started}");
    assert_eq!(started["data"]["kind"], "session-started");
    assert_eq!(started["data"]["activity"], ACTIVITY);

    // A permission request mints no identity of its own when the correlator
    // supplies it; each answer quotes the same strings.
    let (output, issued) = run_json(
        home.path(),
        project.path(),
        &[
            "session",
            "lifecycle",
            "permission",
            "request",
            session,
            "--tool",
            "shell/exec",
            "--activity",
            ACTIVITY,
            "--request",
            REQUEST,
            "--origin",
            "agent:worker",
            "--json",
        ],
    );
    assert!(output.status.success(), "{issued}");
    assert_eq!(issued["data"]["permission_request"], REQUEST);
    assert_eq!(issued["data"]["activity"], ACTIVITY);

    let (output, granted) = run_json(
        home.path(),
        project.path(),
        &[
            "session",
            "lifecycle",
            "permission",
            "grant",
            session,
            "--request",
            REQUEST,
            "--json",
        ],
    );
    assert!(output.status.success(), "{granted}");
    assert_eq!(granted["data"]["kind"], "permission-granted");
    assert_eq!(granted["data"]["permission_request"], REQUEST);
    assert_eq!(granted["data"]["activity"], ACTIVITY);

    // A fresh process over the same AIKIT_HOME — the durable read model
    // quotes the identities unchanged: the four-sided join contract.
    let (output, model) = run_json(
        home.path(),
        project.path(),
        &["session", "lifecycle", "show", session, "--json"],
    );
    assert!(output.status.success(), "{model}");
    assert_eq!(model["data"]["version"], "aikit.session-lifecycle/v1");
    assert_eq!(model["data"]["state"], "running");
    assert_eq!(model["data"]["activities"], serde_json::json!([ACTIVITY]));
    assert!(model["data"]["open_permission_requests"]
        .as_array()
        .unwrap()
        .is_empty());

    let (output, history) = run_json(
        home.path(),
        project.path(),
        &["session", "lifecycle", "history", session, "--json"],
    );
    assert!(output.status.success(), "{history}");
    assert_eq!(history["data"]["count"], 3);
    let events = history["data"]["events"].as_array().unwrap();
    assert_eq!(events[0]["kind"], "session-started");
    assert_eq!(events[1]["kind"], "permission-requested");
    assert_eq!(events[2]["kind"], "permission-granted");
    assert!(events.iter().all(|event| event["activity"] == ACTIVITY));

    // An unknown session is an explicit error envelope, not an empty list.
    let (output, value) = run_json(
        home.path(),
        project.path(),
        &["session", "lifecycle", "history", "ses_nobinary", "--json"],
    );
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "session_lifecycle.unknown_session");
    assert_eq!(output.status.code(), Some(1));
}
