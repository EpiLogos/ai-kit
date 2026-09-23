//! Environmental DAY through the gateway dispatcher, proven in a disposable
//! Central root with the real `ctrl` and the real `aikit` binary.
//!
//! The Routine is the first-party `skill/aikit/central-day-rollover` Method,
//! whose body is native: an occurrence comes due, one `aikit gateway tick`
//! admits it through the ordinary Routine gate, and the native runner calls
//! Central's Actions — no model, no encounter. The root here is built for the
//! test (policy `automatic_day_rollover: true`, a test-only native-service
//! grant for `central.day.ensure`); nothing touches the owner's Central.
//!
//! The real `ctrl` is `$AIKIT_TEST_CTRL` or the one on PATH. Without one the
//! test says so and skips; `AIKIT_REQUIRE_NATIVE_CTRL` turns the skip into a
//! failure. The Workcell root / child NOW horizon is asserted when that `ctrl`
//! exposes `central.now.workcell-root` (Central #217), and reported as not
//! attempted otherwise.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const SERVICE_TOKEN: &str = "disposable-day-routine-service-token-0123456789";
const HUMAN_TOKEN: &str = "disposable-day-routine-human-token-9876543210ab";
const METHOD: &str = "skill/aikit/central-day-rollover";
const ROUTINE: &str = "routine/central-day-rollover";

fn native_ctrl() -> Option<PathBuf> {
    let candidate = std::env::var_os("AIKIT_TEST_CTRL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ctrl"));
    let present = Command::new(&candidate)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !present {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_NATIVE_CTRL").is_none(),
            "AIKIT_REQUIRE_NATIVE_CTRL is set but no native `ctrl` answers ({})",
            candidate.display()
        );
        eprintln!(
            "skip: no native `ctrl` ({}); the DAY rollover needs Central's own Actions",
            candidate.display()
        );
        return None;
    }
    Some(candidate)
}

fn ctrl(ctrl: &Path, root: &Path, action: &str, input: Value, token: Option<&str>) -> Value {
    let mut command = Command::new(ctrl);
    command
        .args(["--json", "--root"])
        .arg(root)
        .args(["action", "run", action, &input.to_string()])
        .env_remove("CENTRAL_NATIVE_TOKEN");
    if let Some(token) = token {
        command.env("CENTRAL_NATIVE_TOKEN", token);
    }
    let output = command.output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "{action}: not a native envelope: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn ok(value: Value) -> Value {
    assert_eq!(value["ok"], true, "{value}");
    value["data"].clone()
}

fn has_action(ctrl: &Path, action: &str) -> bool {
    let output = Command::new(ctrl)
        .args(["--json", "actions"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).contains(&format!("\"{action}\""))
}

/// A disposable Central root: the three recognised Control/user sources the
/// DAY path reads, and three Work members (two with a NOW field, one without).
fn disposable_root(ctrl: &Path, root: &Path) {
    let init = Command::new(ctrl)
        .arg("--root")
        .arg(root)
        .arg("init")
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let digest = |token: &str| format!("{:x}", Sha256::digest(token.as_bytes()));
    let mut relations = Vec::new();
    for (name, role, value) in [
        (
            "placement.json",
            "work-placement-policy",
            json!({"schema":"central.work-placement-policy/v1","scope_ref":"control:root",
                   "writable":[{"path":"Work/Alpha","class":"repository"},{"path":"Work/Beta","class":"repository"}],
                   "enforcement":"native-actions","required_coverage":["file-content"],"lease_seconds":3600}),
        ),
        (
            "civil-time-policy.json",
            "civil-time-policy",
            json!({"schema":"central.civil-time-policy/v1","scope_ref":"control:root",
                   "timezone":"Europe/London","day_boundary_minutes":0,"automatic_day_rollover":true}),
        ),
        (
            "native-action-authority.json",
            "native-action-authority",
            json!({"schema":"central.native-action-authority/v1","scope_ref":"control:root","grants":[
                {"principal_ref":"native-service:aikit/routine-dispatcher/day-rollover","actor_kind":"native-service",
                 "token_sha256":digest(SERVICE_TOKEN),"scope_refs":["control:root"],
                 "actions":["central.day.ensure"],"expires_at_unix_seconds":4102444800u64},
                {"principal_ref":"human:disposable-test","actor_kind":"human",
                 "token_sha256":digest(HUMAN_TOKEN),"scope_refs":["control:root","project:Alpha","project:Beta"],
                 "actions":["central.now.lifecycle"],"expires_at_unix_seconds":4102444800u64}
            ]}),
        ),
    ] {
        let path = format!("Control/user/{name}");
        std::fs::write(root.join(&path), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        relations.push(json!({"ref":format!("central:source:control:root:{path}"),"path":path,"roles":[role],
            "provenance":"human-adopted","standing":"architecture-contract","treatment":"control-user",
            "recognition":"disposable-test-only","recorded_at_unix_seconds":1}));
    }
    std::fs::create_dir_all(root.join("Control/relations")).unwrap();
    std::fs::write(
        root.join("Control/relations/source-relations.json"),
        serde_json::to_vec_pretty(&json!({"schema":"central.control.ground-relations/v1",
            "project_id":"control:root","relations":relations}))
        .unwrap(),
    )
    .unwrap();
    for member in ["Alpha", "Beta", "Gamma"] {
        std::fs::create_dir_all(root.join("Work").join(member)).unwrap();
    }
    for member in ["Alpha", "Beta"] {
        ok(ctrl_run(
            ctrl,
            root,
            "projectcentral.now.init",
            json!({"project": member}),
        ));
    }
}

fn ctrl_run(ctrl_bin: &Path, root: &Path, action: &str, input: Value) -> Value {
    ctrl(ctrl_bin, root, action, input, None)
}

fn handoff(ctrl_bin: &Path, root: &Path, project: &str, subject: &str, status: &str) -> String {
    let data = ok(ctrl_run(
        ctrl_bin,
        root,
        "projectcentral.now.return",
        json!({"project":project,"actor":"agent:disposable-test","kind":"handoff",
               "subject":subject,"result":format!("{subject} ({status})"),"status":status}),
    ));
    data["source"].as_str().unwrap().to_owned()
}

struct Aikit {
    home: PathBuf,
    ctrl: PathBuf,
    root: PathBuf,
}

impl Aikit {
    fn run(&self, args: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
            .args(args)
            .arg("--json")
            .env("AIKIT_HOME", &self.home)
            .env("HOME", &self.home)
            .env("AIKIT_CENTRAL_ROOT", &self.root)
            .env("CENTRAL_CTRL_BIN", &self.ctrl)
            // The credential reaches ctrl only through the Routine's binding.
            .env_remove("CENTRAL_NATIVE_TOKEN")
            .env_remove("AIKIT_ROUTINE_PROVIDER")
            .current_dir(&self.home)
            .output()
            .unwrap();
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "aikit {args:?}: {} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert!(output.status.success(), "aikit {args:?} failed: {envelope}");
        envelope["data"].clone()
    }
}

/// Every file under `dir` with its bytes, for before/after comparison.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        if entry.file_type().is_file() {
            let relative = entry
                .path()
                .strip_prefix(dir)
                .unwrap()
                .display()
                .to_string();
            files.insert(relative, std::fs::read(entry.path()).unwrap_or_default());
        }
    }
    files
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn a_due_day_occurrence_opens_the_day_and_rolls_now_fields_with_native_actions_only() {
    let Some(ctrl_bin) = native_ctrl() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("Central");
    std::fs::create_dir_all(&root).unwrap();
    disposable_root(&ctrl_bin, &root);

    // The live NOW field before the boundary.
    let live = handoff(&ctrl_bin, &root, "Alpha", "live thread", "active");
    let waiting = handoff(&ctrl_bin, &root, "Alpha", "parked question", "waiting");
    let resolved = handoff(&ctrl_bin, &root, "Alpha", "finished thread", "resolved");

    // The Workcell horizon, when this ctrl carries it (Central #217).
    let horizon = has_action(&ctrl_bin, "central.now.workcell-root");
    let mut horizon_refs = None;
    if horizon {
        let policy = ok(ctrl_run(&ctrl_bin, &root, "central.work.policy", json!({})));
        let revision = policy["revision"].as_str().unwrap().to_owned();
        let workcell_root = ok(ctrl_run(
            &ctrl_bin,
            &root,
            "central.now.workcell-root",
            json!({"workcell_ref":"workcell:disposable","expected_policy_revision":revision}),
        ));
        let root_now = workcell_root["now_ref"]
            .as_str()
            .or_else(|| {
                workcell_root
                    .pointer("/record/now_ref")
                    .and_then(Value::as_str)
            })
            .unwrap()
            .to_owned();
        let project_policy = ok(ctrl_run(
            &ctrl_bin,
            &root,
            "central.work.policy",
            json!({"project":"Alpha"}),
        ));
        let project_revision = project_policy["revision"].as_str().unwrap().to_owned();
        let child = |task: &str| {
            let allocated = ok(ctrl_run(
                &ctrl_bin,
                &root,
                "central.now.allocate",
                json!({"project":"Alpha","task_ref":format!("central:task:project:Alpha:{task}"),
                       "purpose":format!("{task} child"),"expected_policy_revision":project_revision,
                       "parent_now_ref":root_now,"workcell_ref":"workcell:disposable"}),
            ));
            let now_ref = allocated["now_ref"]
                .as_str()
                .or_else(|| allocated.pointer("/record/now_ref").and_then(Value::as_str))
                .unwrap()
                .to_owned();
            (now_ref, allocated)
        };
        let (active_child, _) = child("active");
        let (quiet_child, _) = child("quiet");
        let reading = ok(ctrl_run(
            &ctrl_bin,
            &root,
            "central.now.read",
            json!({"project":"Alpha","now_ref":quiet_child}),
        ));
        let quiet = ok(ctrl(
            &ctrl_bin,
            &root,
            "central.now.lifecycle",
            json!({"project":"Alpha","now_ref":quiet_child,
                   "expected_revision":reading["revision"]["revision"],
                   "expected_policy_revision":project_revision,"lifecycle":"quiescent"}),
            Some(HUMAN_TOKEN),
        ));
        assert!(quiet.to_string().contains("quiescent"), "{quiet}");
        horizon_refs = Some((root_now, active_child, quiet_child));
    } else {
        eprintln!(
            "not attempted: {} has no central.now.workcell-root; the Workcell root/child horizon is not asserted",
            ctrl_bin.display()
        );
    }

    // AIKit: the first-party registry, the Method proven, the Routine bound.
    let home = dir.path().join("aikit-home");
    std::fs::create_dir_all(home.join("registries")).unwrap();
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry"),
        home.join("registries/ai-kit"),
    )
    .unwrap();
    let aikit = Aikit {
        home: home.clone(),
        ctrl: ctrl_bin.clone(),
        root: root.clone(),
    };
    let proof = aikit.run(&[
        "method",
        "prove",
        "--method",
        METHOD,
        "--proof-json",
        &json!({"proof_ref":"proof:disposable:day-rollover","context_resolution_ref":"context-resolution:disposable",
                "activity_refs":["activity:disposable:day-rollover"],"return_refs":["return:disposable:day-rollover"],
                "evidence_refs":["evidence:disposable:day-rollover"],"verification_refs":["verification:disposable:day-rollover"],
                "invocation_succeeded":true,"verification_passed":true})
        .to_string(),
    ]);
    let actions = [
        "central:action/central.time.policy",
        "central:action/central.day.ensure",
        "central:action/central.world",
        "central:action/projectcentral.now.rollover",
    ];
    let authority =
        json!({"authority_ref":"authority:disposable:day-rollover","revision":"authority-rev-1",
                           "action_refs":actions,"granted":true,"unattended":true})
        .to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let trigger =
        json!({"schema":"aikit.time-schedule/v1","schedule_ref":"schedule/disposable-day",
                         "schedule":{"kind":"once","due_unix_ms": now_ms - 2_000}})
        .to_string();
    aikit.run(&[
        "routine",
        "create",
        "--name",
        "central-day-rollover",
        "--method",
        METHOD,
        "--proof-json",
        &proof.to_string(),
        "--trigger-json",
        &trigger,
        "--authority-json",
        &authority,
    ]);
    // A Routine may only be granted the Actions its Method declares.
    let widened = json!({"authority_ref":"authority:disposable:day-rollover","revision":"authority-rev-1",
                         "action_refs":["central:action/central.day.lifecycle"],"granted":true,"unattended":true});
    let refused = Command::new(env!("CARGO_BIN_EXE_aikit"))
        .args([
            "routine",
            "enable",
            ROUTINE,
            "--authority-json",
            &widened.to_string(),
            "--json",
        ])
        .env("AIKIT_HOME", &home)
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stdout).contains("routine.action_not_in_method"));

    let token_file = dir.path().join("native-token-day-routine");
    std::fs::write(&token_file, SERVICE_TOKEN).unwrap();
    std::fs::set_permissions(&token_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    aikit.run(&[
        "routine",
        "credential",
        ROUTINE,
        "--env",
        "CENTRAL_NATIVE_TOKEN",
        "--location",
        &format!("file:{}", token_file.display()),
    ]);
    aikit.run(&["routine", "enable", ROUTINE, "--authority-json", &authority]);
    let shown = aikit.run(&["routine", "show", ROUTINE]);
    assert_eq!(shown["method_body"], "native:central-day-rollover");

    let before = snapshot(&root);
    let day_before = ctrl_run(&ctrl_bin, &root, "central.day.read", json!({}));
    assert_eq!(
        day_before["ok"], false,
        "no Day is open before the boundary: {day_before}"
    );

    // The boundary: one dispatcher pass.
    let tick = aikit.run(&["gateway", "tick"]);
    let dispatched = tick["dispatched"].as_array().unwrap();
    assert_eq!(dispatched.len(), 1, "{tick}");
    let record = &dispatched[0];
    assert_eq!(record["admission"], "applied");
    assert_eq!(record["method_body"], "native:central-day-rollover");
    assert_eq!(record["outcome"]["status"], "completed", "{record}");
    let detail: Value =
        serde_json::from_str(record["outcome"]["detail"].as_str().unwrap()).unwrap();
    assert_eq!(detail["runner"], "native");

    // The Day opened, at Central's civil date, under the policy it read.
    let policy = ok(ctrl_run(&ctrl_bin, &root, "central.time.policy", json!({})));
    let today = policy["civil_date"].as_str().unwrap().to_owned();
    let day = ok(ctrl_run(&ctrl_bin, &root, "central.day.read", json!({})));
    assert_eq!(day["temporal"]["civil_date"], today.as_str());
    assert_eq!(detail["result"]["day"]["civil_date"], today.as_str());
    assert_eq!(detail["result"]["day"]["tasks_carried_or_ticked"], false);
    assert_eq!(detail["result"]["day"]["now_cleared_or_archived"], false);
    let yesterday = detail["result"]["closed_day"].as_str().unwrap().to_owned();

    // Alpha closed yesterday into today: live handoffs carried, the resolved
    // one released, nothing newly marked resolved.
    let receipt = read_json(Path::new(detail["receipt"].as_str().unwrap()));
    let projects = receipt["result"]["projects"].as_array().unwrap();
    let alpha = projects.iter().find(|p| p["project"] == "Alpha").unwrap();
    assert_eq!(alpha["outcome"], "closed");
    assert!(root
        .join(format!("Work/Alpha/ProjectCentral/now/day/{yesterday}.md"))
        .exists());
    for carried in [&live, &waiting] {
        let handoff = read_json(&root.join("Work/Alpha").join(carried));
        assert_ne!(handoff["status"], "resolved", "{handoff}");
        assert_eq!(handoff["status"], "carried");
        assert!(handoff["carried_from_days"]
            .to_string()
            .contains(&yesterday));
    }
    assert!(
        !root.join("Work/Alpha").join(&resolved).exists(),
        "the resolved handoff is released"
    );
    let beta = projects.iter().find(|p| p["project"] == "Beta").unwrap();
    assert_eq!(beta["outcome"], "closed");
    assert!(
        projects.iter().all(|p| p["project"] != "Gamma"),
        "no NOW field, no rollover"
    );
    assert!(!root.join("Work/Gamma/ProjectCentral").exists());

    // Only the four declared Central Actions ran, and only day.ensure carried
    // the credential.
    let calls = receipt["calls"].as_array().unwrap();
    let names: Vec<&str> = calls
        .iter()
        .map(|call| call["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "central.time.policy",
            "central.day.ensure",
            "central.world",
            "projectcentral.now.rollover",
            "projectcentral.now.rollover",
        ]
    );
    for call in calls {
        let expected = (call["action"] == "central.day.ensure").then_some("CENTRAL_NATIVE_TOKEN");
        assert_eq!(call["credential_env"].as_str(), expected, "{call}");
    }

    // Horizon: clearings carried or released, never closed.
    if let Some((root_now, active_child, quiet_child)) = &horizon_refs {
        let root_horizon = &receipt["result"]["day"]["now_horizon"];
        assert!(
            root_horizon["carried"]
                .to_string()
                .contains(root_now.as_str()),
            "{root_horizon}"
        );
        assert_eq!(
            root_horizon["clearings_closed_completed_or_archived"],
            false
        );
        let alpha_horizon = &alpha["now_horizon"];
        assert!(
            alpha_horizon["carried"]
                .to_string()
                .contains(active_child.as_str()),
            "{alpha_horizon}"
        );
        assert!(
            alpha_horizon["released"]
                .to_string()
                .contains(quiet_child.as_str()),
            "{alpha_horizon}"
        );
        for (project, now_ref) in [
            (None, root_now),
            (Some("Alpha"), active_child),
            (Some("Alpha"), quiet_child),
        ] {
            let mut input = json!({"now_ref": now_ref});
            if let Some(project) = project {
                input["project"] = json!(project);
            }
            let read = ok(ctrl_run(&ctrl_bin, &root, "central.now.read", input));
            let lifecycle = read["record"]["lifecycle"].as_str().unwrap();
            assert!(
                matches!(lifecycle, "active" | "quiescent"),
                "{now_ref} was {lifecycle}"
            );
        }
        let children = ok(ctrl_run(
            &ctrl_bin,
            &root,
            "central.now.children",
            json!({"now_ref": root_now}),
        ));
        assert!(
            children.to_string().contains(quiet_child.as_str()),
            "a released child stays retained"
        );
    }

    // Nothing was recognised and no Return was received.
    let after = snapshot(&root);
    let changed: Vec<&String> = after
        .keys()
        .filter(|path| before.get(*path) != after.get(*path))
        .collect();
    assert!(!changed.is_empty());
    for path in &changed {
        assert!(
            !path.contains("source-returns"),
            "a Return was written: {path}"
        );
        assert!(
            !path.to_ascii_lowercase().contains("recogni"),
            "a recognition was written: {path}"
        );
    }

    // No model ran: nothing was sent to a resident encounter owner, and the
    // bound credential appears in no AIKit record.
    assert!(!home.join("state/encounter-owner.sock").exists());
    for (path, bytes) in snapshot(&home) {
        assert!(
            !String::from_utf8_lossy(&bytes).contains(SERVICE_TOKEN),
            "the credential leaked into {path}"
        );
    }
    assert!(!tick.to_string().contains(SERVICE_TOKEN));

    // Exactly once: the next pass admits nothing new.
    let again = aikit.run(&["gateway", "tick"]);
    assert!(
        again["dispatched"].as_array().unwrap().is_empty(),
        "{again}"
    );
}

#[test]
fn a_native_run_without_a_bound_credential_refuses_before_calling_the_gated_action() {
    let Some(ctrl_bin) = native_ctrl() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("Central");
    std::fs::create_dir_all(&root).unwrap();
    disposable_root(&ctrl_bin, &root);
    let home = dir.path().join("aikit-home");
    std::fs::create_dir_all(home.join("registries")).unwrap();
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry"),
        home.join("registries/ai-kit"),
    )
    .unwrap();
    let aikit = Aikit {
        home: home.clone(),
        ctrl: ctrl_bin.clone(),
        root: root.clone(),
    };
    let proof = aikit.run(&[
        "method", "prove", "--method", METHOD, "--proof-json",
        &json!({"proof_ref":"proof:d","context_resolution_ref":"context-resolution:d","activity_refs":["activity:d"],
                "return_refs":["return:d"],"evidence_refs":["evidence:d"],"verification_refs":["verification:d"],
                "invocation_succeeded":true,"verification_passed":true}).to_string(),
    ]);
    let authority = json!({"authority_ref":"authority:d","revision":"r1","action_refs":[
        "central:action/central.time.policy","central:action/central.day.ensure",
        "central:action/central.world","central:action/projectcentral.now.rollover"],
        "granted":true,"unattended":true})
    .to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let trigger = json!({"schema":"aikit.time-schedule/v1","schedule_ref":"schedule/d",
                         "schedule":{"kind":"once","due_unix_ms": now_ms - 2_000}})
    .to_string();
    aikit.run(&[
        "routine",
        "create",
        "--name",
        "central-day-rollover",
        "--method",
        METHOD,
        "--proof-json",
        &proof.to_string(),
        "--trigger-json",
        &trigger,
        "--authority-json",
        &authority,
    ]);
    aikit.run(&["routine", "enable", ROUTINE, "--authority-json", &authority]);
    let tick = aikit.run(&["gateway", "tick"]);
    let record = &tick["dispatched"][0];
    assert_eq!(record["outcome"]["status"], "failed", "{tick}");
    let detail: Value =
        serde_json::from_str(record["outcome"]["detail"].as_str().unwrap()).unwrap();
    assert_eq!(detail["result"]["stage"], "day-ensure");
    assert!(detail["result"]["error"].as_str().unwrap().contains(
        "aikit routine credential routine/central-day-rollover --env CENTRAL_NATIVE_TOKEN"
    ));
    let day = ctrl_run(&ctrl_bin, &root, "central.day.read", json!({}));
    assert_eq!(day["ok"], false, "no Day was opened: {day}");
}
