//! Environmental file-map refresh through the gateway dispatcher, proven in a
//! disposable Central root with the real `ctrl`, the real `aikit` binary, and
//! a stateful fake `bkmr` (via `CENTRAL_BKMR_BIN`) so no embedding model is
//! ever downloaded: the Routine must depend on Central's refresh law, not on
//! fastembed's network.
//!
//! The Routine is the first-party `skill/aikit/central-file-map-refresh`
//! Method, whose body is native: one `aikit gateway tick` admits the due
//! occurrence and the native runner probes every scope's persistent map
//! through `central.file-map.search`, then refreshes exactly the
//! uninitialized or stale ones. Nothing touches the owner's Central.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const SERVICE_TOKEN: &str = "disposable-file-map-service-token-0123456789";
const METHOD: &str = "skill/aikit/central-file-map-refresh";
const ROUTINE: &str = "routine/central-file-map-refresh";
const ACTIONS: [&str; 3] = [
    "central:action/central.file-map.search",
    "central:action/central.world",
    "central:action/central.file-map.refresh",
];

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
            "skip: no native `ctrl` ({}); the file-map refresh needs Central's own Actions",
            candidate.display()
        );
        return None;
    }
    Some(candidate)
}

fn ctrl_run(ctrl: &Path, root: &Path, action: &str, input: Value, bkmr: Option<&Path>) -> Value {
    let mut command = Command::new(ctrl);
    command
        .args(["--json", "--root"])
        .arg(root)
        .args(["action", "run", action, &input.to_string()])
        .env_remove("CENTRAL_NATIVE_TOKEN");
    if let Some(bkmr) = bkmr {
        command.env("CENTRAL_BKMR_BIN", bkmr);
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

/// A stateful fake `bkmr` the backend drives through `CENTRAL_BKMR_BIN`.
/// Rows live in a JSON file so `add` → `search` round-trips exactly the way
/// Central's refresh reconciles rows.
fn fake_bkmr(dir: &Path) -> PathBuf {
    let state = dir.join("bkmr-fake-state.json");
    std::fs::write(&state, "[]").unwrap();
    let script = dir.join("fake-bkmr.py");
    std::fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env python3
import json, sys

STATE = {state:?}
SUBCOMMANDS = {{"search", "hsearch", "add", "update", "delete", "import-files", "create-db"}}

def rows():
    with open(STATE) as handle:
        return json.load(handle)

def save(rows_):
    with open(STATE, "w") as handle:
        json.dump(rows_, handle)

args = sys.argv[1:]
if "--version" in args:
    print("bkmr 7.6.7")
    raise SystemExit(0)
command = next((arg for arg in args if arg in SUBCOMMANDS), "search")
if command == "create-db":
    for index, arg in enumerate(args):
        if arg == "create-db" and index + 1 < len(args):
            open(args[index + 1], "w").close()
            break
elif command in {{"search", "hsearch"}}:
    print(json.dumps(rows()))
elif command == "add":
    url = next(arg for arg in args if not arg.startswith("-") and "://" in arg)
    def pick(flag):
        index = args.index(flag)
        return args[index + 1] if index + 1 < len(args) else ""
    existing = [row for row in rows() if row["url"] != url]
    existing.append({{
        "id": max([row["id"] for row in existing], default=0) + 1,
        "url": url,
        "title": pick("--title"),
        "description": pick("--description"),
        "tags": [],
    }})
    save(existing)
    print(json.dumps({{"added": url}}))
elif command == "update":
    identifier = int(next(arg for arg in args if arg.isdigit()))
    def pick(flag):
        index = args.index(flag)
        return args[index + 1] if index + 1 < len(args) else None
    for row in rows():
        if row["id"] == identifier:
            if pick("--url"):
                row["url"] = pick("--url")
            if pick("--description"):
                row["description"] = pick("--description")
    save(rows())
    print(json.dumps({{"updated": identifier}}))
elif command == "delete":
    identifier = int(next((arg for arg in args if arg.isdigit()), "0"))
    save([row for row in rows() if row["id"] != identifier])
    print(json.dumps({{"deleted": identifier}}))
else:
    print(json.dumps({{}}))
"#,
            state = state.display().to_string(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

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
                 "actions":["central.day.ensure"],"expires_at_unix_seconds":4102444800u64}
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
    for member in ["Alpha", "Beta"] {
        std::fs::create_dir_all(root.join("Work").join(member)).unwrap();
        let data = ok(ctrl_run(
            ctrl,
            root,
            "projectcentral.now.init",
            json!({"project": member}),
            None,
        ));
        let _ = data;
    }
}

struct Aikit {
    home: PathBuf,
    ctrl: PathBuf,
    root: PathBuf,
    bkmr: PathBuf,
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
            .env("CENTRAL_BKMR_BIN", &self.bkmr)
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

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn a_due_file_map_occurrence_probes_every_scope_and_refreshes_the_uninitialized() {
    let Some(ctrl_bin) = native_ctrl() else {
        return;
    };
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("Central");
    std::fs::create_dir_all(&root).unwrap();
    disposable_root(&ctrl_bin, &root);
    let bkmr = fake_bkmr(dir.path());

    // Before: no persistent map anywhere.
    let probe = ok(ctrl_run(
        &ctrl_bin,
        &root,
        "central.file-map.search",
        json!({"federated": true, "limit": 1}),
        Some(&bkmr),
    ));
    let worlds: Vec<String> = probe["result"]["absences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|absence| absence.as_str().unwrap().to_owned())
        .collect();
    assert!(
        worlds.iter().any(|world| world == "control:root: map not initialized"),
        "{worlds:?}"
    );
    assert!(!root.join(".central/bkmr/index.db").exists());

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
        bkmr: bkmr.clone(),
    };
    let proof = aikit.run(&[
        "method",
        "prove",
        "--method",
        METHOD,
        "--proof-json",
        &json!({"proof_ref":"proof:disposable:file-map-refresh","context_resolution_ref":"context-resolution:disposable",
                "activity_refs":["activity:disposable:file-map-refresh"],"return_refs":["return:disposable:file-map-refresh"],
                "evidence_refs":["evidence:disposable:file-map-refresh"],"verification_refs":["verification:disposable:file-map-refresh"],
                "invocation_succeeded":true,"verification_passed":true})
            .to_string(),
    ]);
    let authority = json!({"authority_ref":"authority:disposable:file-map-refresh","revision":"authority-rev-1",
                           "action_refs":ACTIONS,"granted":true,"unattended":true})
        .to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let trigger =
        json!({"schema":"aikit.time-schedule/v1","schedule_ref":"schedule/disposable-file-map",
                         "schedule":{"kind":"once","due_unix_ms": now_ms - 2_000}})
            .to_string();
    aikit.run(&[
        "routine",
        "create",
        "--name",
        "central-file-map-refresh",
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
    let shown = aikit.run(&["routine", "show", ROUTINE]);
    assert_eq!(shown["method_body"], "native:central-file-map-refresh");

    // The boundary: one dispatcher pass.
    let tick = aikit.run(&["gateway", "tick"]);
    let dispatched = tick["dispatched"].as_array().unwrap();
    assert_eq!(dispatched.len(), 1, "{tick}");
    let record = &dispatched[0];
    assert_eq!(record["admission"], "applied");
    assert_eq!(record["method_body"], "native:central-file-map-refresh");
    assert_eq!(record["outcome"]["status"], "completed", "{record}");
    let detail: Value =
        serde_json::from_str(record["outcome"]["detail"].as_str().unwrap()).unwrap();
    assert_eq!(detail["runner"], "native");

    // Every scope the probe named was refreshed, root included.
    let receipt = read_json(Path::new(detail["receipt"].as_str().unwrap()));
    assert_eq!(receipt["method_body"], "central-file-map-refresh");
    let scopes = receipt["result"]["scopes"].as_array().unwrap();
    let refreshed = |world: &str| {
        scopes
            .iter()
            .find(|scope| scope["world"] == world)
            .unwrap_or_else(|| panic!("{world} missing from {scopes:?}"))
    };
    assert_eq!(refreshed("control:root")["outcome"], "refreshed");
    let names: Vec<&str> = scopes
        .iter()
        .filter_map(|scope| scope["project"].as_str())
        .collect();
    assert!(names.contains(&"Alpha") && names.contains(&"Beta"), "{scopes:?}");
    let calls = receipt["calls"].as_array().unwrap();
    let called: Vec<&str> = calls
        .iter()
        .map(|call| call["action"].as_str().unwrap())
        .collect();
    assert!(called.contains(&"central.file-map.search"), "{called:?}");
    assert!(called.contains(&"central.file-map.refresh"), "{called:?}");
    assert!(called.contains(&"central.world"), "{called:?}");
    assert!(
        calls
            .iter()
            .all(|call| call["credential_env"].is_null()),
        "no Action is token-gated: {calls:?}"
    );

    // After: the maps exist and the probe is clean.
    assert!(root.join(".central/bkmr/index.db").exists());
    for member in ["Alpha", "Beta"] {
        assert!(root
            .join("Work")
            .join(member)
            .join(".central/bkmr/index.db")
            .exists());
    }
    let after = ok(ctrl_run(
        &ctrl_bin,
        &root,
        "central.file-map.search",
        json!({"federated": true, "limit": 1}),
        Some(&bkmr),
    ));
    assert_eq!(
        after["result"]["absences"].as_array().unwrap().len(),
        0,
        "{after}"
    );
}
