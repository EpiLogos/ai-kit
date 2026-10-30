//! Saved operations, end to end through the real binary.
//!
//! A command, a script or an alias becomes a saved operation by travelling one
//! road: **capture → promote → (review) → apply → found → run → counted**. Every
//! stage is a verb a person or an agent types, and every claim here is made by
//! running those verbs against a temporary AIKit home — nothing is faked and no
//! internals are called.
//!
//! The home, the user's `HOME` and the project are all temporary: `apply` also
//! projects into the clients' own configuration under `$HOME`, so isolating only
//! `AIKIT_HOME` would let a test rewrite the real `~/.claude.json`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

const PANE_VARIABLES: [&str; 7] = [
    "TMUX",
    "TMUX_PANE",
    "HERDR_PANE_ID",
    "HERDR_SOCKET_PATH",
    "CMUX_WORKSPACE_ID",
    "CMUX_SURFACE_ID",
    "CMUX_SOCKET_PATH",
];

struct World {
    _tmp: TempDir,
    aikit_home: PathBuf,
    user_home: PathBuf,
    project: PathBuf,
    bin_dir: PathBuf,
}

impl World {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let aikit_home = tmp.path().join("aikit");
        let user_home = tmp.path().join("home");
        let project = tmp.path().join("project");
        for dir in [&aikit_home, &user_home, &project.join(".aikit")] {
            fs::create_dir_all(dir).unwrap();
        }
        // A project that declares nothing: whatever is active, a test enabled.
        fs::write(project.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
        let bin_dir = assert_cmd::cargo::cargo_bin("aikit")
            .parent()
            .unwrap()
            .to_path_buf();
        World {
            _tmp: tmp,
            aikit_home,
            user_home,
            project,
            bin_dir,
        }
    }

    fn command(&self, program: &Path) -> Command {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![self.bin_dir.clone()];
        paths.extend(std::env::split_paths(&path));
        let mut command = Command::new(program);
        command
            .env("AIKIT_HOME", &self.aikit_home)
            .env("HOME", &self.user_home)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env_remove("AIKIT_CONTEXT_ID")
            .env_remove("AIKIT_SESSION_ID")
            .current_dir(&self.project);
        // A test never names the pane its runner happens to sit in, and never
        // asks the person's cmux where that is.
        for name in PANE_VARIABLES {
            command.env_remove(name);
        }
        command
    }

    fn aikit_raw(&self, args: &[&str]) -> Output {
        self.command(&assert_cmd::cargo::cargo_bin("aikit"))
            .args(args)
            .output()
            .expect("aikit runs")
    }

    /// Run a verb with `input` on its stdin; the output is returned whatever its status.
    fn with_stdin(&self, args: &[&str], input: &str, env: &[(&str, &str)]) -> Output {
        use std::io::Write;
        use std::process::Stdio;
        let mut command = self.command(&assert_cmd::cargo::cargo_bin("aikit"));
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("aikit spawns");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    /// Run a verb that must succeed and return its JSON envelope.
    fn json(&self, args: &[&str]) -> Value {
        let mut args = args.to_vec();
        args.push("--json");
        let output = self.aikit_raw(&args);
        assert!(
            output.status.success(),
            "aikit {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "aikit {args:?} did not print JSON ({e}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
    }

    /// Run a verb that must be refused; return its stderr.
    fn refused(&self, args: &[&str]) -> String {
        let output = self.aikit_raw(args);
        assert!(
            !output.status.success(),
            "aikit {args:?} should have been refused, printed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        String::from_utf8_lossy(&output.stderr).to_string()
    }

    fn capsule_dir(&self, id: &str) -> PathBuf {
        self.aikit_home
            .join("registries/personal/capsules")
            .join(id)
    }

    /// The `bin/<name>` shim a given `apply` generated.
    fn shim(&self, applied: &Value, name: &str) -> PathBuf {
        let context = applied["context"]["context_id"].as_str().unwrap();
        let generation = applied["data"]["generation"].as_str().unwrap();
        let shim = self
            .aikit_home
            .join("state/contexts")
            .join(context)
            .join("generations")
            .join(generation)
            .join("bin")
            .join(name);
        assert!(shim.is_file(), "no shim at {}", shim.display());
        shim
    }

    fn run_shim(&self, shim: &Path, args: &[&str]) -> Output {
        self.command(Path::new("/bin/sh"))
            .arg(shim)
            .args(args)
            .output()
            .expect("the shim runs")
    }

    /// Events of one capability on the log, newest first: `(action, outcome)`.
    fn events_of(&self, capability: &str) -> Vec<(String, String)> {
        self.json(&["recent", "--limit", "50"])["data"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["capability"] == capability)
            .map(|e| {
                (
                    e["action"].as_str().unwrap().to_string(),
                    e["outcome"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn search_ids(world: &World, query: &str) -> Vec<String> {
    world.json(&["search", query])["data"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// A script, captured and promoted, found and run and counted
// ---------------------------------------------------------------------------

#[test]
fn a_captured_script_travels_from_capture_to_a_counted_run() {
    let world = World::new();
    let id = "script/personal/deploy";

    // -- capture ------------------------------------------------------------
    let captured = world.json(&[
        "capture",
        "deploy staging",
        "--body",
        "echo deploying \"$@\"",
        "--kind",
        "script",
        "--export",
        "deploy",
        "--by",
        "human",
        "--source-ref",
        "terminal:pane-1",
    ]);
    let data = &captured["data"];
    assert_eq!(data["state"], "ready");
    assert_eq!(data["kind"], "script");
    assert_eq!(data["confidence"], "declared");
    assert_eq!(data["exports"][0], "deploy");
    assert_eq!(data["by"], "human");
    assert_eq!(data["source_ref"], "terminal:pane-1");
    assert_eq!(
        data["proposed_id"], id,
        "the reply says what accept would save it as"
    );
    let candidate = data["candidate"].as_str().unwrap().to_string();

    // -- promote ------------------------------------------------------------
    let promoted = world.json(&["promote", &candidate, "--id", id]);
    assert_eq!(promoted["data"]["id"], id);
    assert_eq!(promoted["data"]["exports"][0], "deploy");
    let manifest = fs::read_to_string(world.capsule_dir(id).join("manifest.toml")).unwrap();
    assert!(manifest.contains("exports = [\"deploy\"]"), "{manifest}");
    assert!(manifest.contains("author = \"human\""), "{manifest}");
    assert!(
        manifest.contains("source_ref = \"terminal:pane-1\""),
        "{manifest}"
    );

    // -- found: by search, by z, and as a capability an act can describe -----
    assert_eq!(search_ids(&world, "kind:script deploy"), [id]);
    let found = &world.json(&["search", "kind:script deploy"])["data"]["rows"][0];
    assert_eq!(
        found["source"], "personal",
        "inactive, and still from somewhere"
    );
    let z = world.json(&["z", "deploy"]);
    assert_eq!(z["data"]["decision"], "act");
    assert_eq!(z["data"]["capability"], id);
    assert_eq!(z["data"]["action"], "run");
    assert_eq!(
        z["data"]["activated"], false,
        "z proposes; it never enables"
    );
    let described = world.json(&["act", "describe", id]);
    assert_eq!(described["data"]["kind"], "capability");
    assert_eq!(described["data"]["input"]["confirm_required"], true);

    // -- an unreviewed script is refused, and the refusal is on the record ----
    let refusal = world.refused(&["run", id]);
    assert!(refusal.contains("has not been reviewed"), "{refusal}");
    assert_eq!(
        world.events_of(id),
        [("run".to_string(), "denied".to_string())],
        "a refusal is a policy denial, recorded as one"
    );
    let failures = world.json(&["failures"]);
    assert_eq!(failures["data"]["failures"][0]["outcome"], "denied");
    assert_eq!(failures["data"]["failures"][0]["detail"], "trust.required");

    // -- confirmed once: runs, and is counted ---------------------------------
    let confirmed = world.aikit_raw(&["run", id, "--confirm", "one", "two"]);
    assert!(confirmed.status.success());
    assert_eq!(stdout(&confirmed).trim(), "deploying one two");

    assert_eq!(
        world.events_of(id)[0],
        ("run".to_string(), "success".to_string())
    );
    let stats = world.json(&["stats"])["data"].clone();
    assert_eq!(stats["ever_used"], 1);
    assert_eq!(stats["successful_runs"], 1);
    assert_eq!(stats["by_kind"][0]["kind"], "script");
    assert_eq!(stats["by_kind"][0]["count"], 1);
    let z = world.json(&["z", "deploy"]);
    assert_eq!(
        z["data"]["candidates"][0]["successful_runs"], 1,
        "z learns from the run"
    );
    let unused = world.json(&["unused"])["data"].clone();
    assert_eq!(unused["count"], 0, "a script that ran is not unused");

    // -- reviewed, enabled, applied: it is on the PATH as a command -----------
    world.json(&["trust", "record", id]);
    assert_eq!(
        stdout(&world.aikit_raw(&["run", id, "reviewed"])).trim(),
        "deploying reviewed"
    );
    world.json(&["enable", id]);
    let applied = world.json(&["apply"]);
    let shim = world.shim(&applied, "deploy");

    let by_shim = world.run_shim(&shim, &["x", "y"]);
    assert!(
        by_shim.status.success(),
        "{}",
        String::from_utf8_lossy(&by_shim.stderr)
    );
    assert_eq!(
        stdout(&by_shim).trim(),
        "deploying x y",
        "the shim's own bookkeeping never reaches the script's arguments"
    );
    let runs = world
        .events_of(id)
        .into_iter()
        .filter(|(action, outcome)| action == "run" && outcome == "success")
        .count();
    assert_eq!(
        runs, 3,
        "confirmed, reviewed and shimmed runs are all counted"
    );

    // -- revised: the new revision is unreviewed and needs confirming again ---
    let payload = world.capsule_dir(id).join("payload/run.sh");
    let mut text = fs::read_to_string(&payload).unwrap();
    text.push_str("\necho revised\n");
    fs::write(&payload, text).unwrap();

    let revised = world.refused(&["run", id]);
    assert!(revised.contains("has not been reviewed"), "{revised}");
    assert_eq!(
        world.json(&["trust", "show", id])["data"]["state"],
        "unseen"
    );
    let after = world.aikit_raw(&["run", id, "--confirm"]);
    assert!(stdout(&after).contains("revised"));
}

// ---------------------------------------------------------------------------
// An alias, the same road, through its generated shim
// ---------------------------------------------------------------------------

#[test]
fn a_captured_alias_runs_through_its_shim_and_is_counted() {
    let world = World::new();
    let id = "alias/personal/say";

    let captured = world.json(&[
        "capture",
        "say with angle brackets",
        "--body",
        "printf '<%s>' status",
        "--kind",
        "alias",
        "--export",
        "say",
    ]);
    assert_eq!(captured["data"]["kind"], "alias");
    assert_eq!(captured["data"]["proposed_id"], id);
    let candidate = captured["data"]["candidate"].as_str().unwrap().to_string();

    // Promoted under the name the capture proposed; the command line is the
    // manifest's body — an alias is never promoted empty.
    let promoted = world.json(&["promote", &candidate]);
    assert_eq!(promoted["data"]["id"], id);
    assert_eq!(promoted["data"]["kind"], "alias");
    let manifest = fs::read_to_string(world.capsule_dir(id).join("manifest.toml")).unwrap();
    assert!(
        manifest.contains("body = \"printf '<%s>' status\""),
        "{manifest}"
    );

    assert_eq!(search_ids(&world, "kind:alias say"), [id]);
    let z = world.json(&["z", "say"]);
    assert_eq!(z["data"]["capability"], id);
    assert_eq!(z["data"]["action"], "run");

    // Like a script, an alias is executable and is reviewed before it runs.
    let refusal = world.refused(&["run", id]);
    assert!(refusal.contains("has not been reviewed"), "{refusal}");
    let confirmed = world.aikit_raw(&["run", id, "--confirm", "more"]);
    assert!(
        confirmed.status.success(),
        "{}",
        String::from_utf8_lossy(&confirmed.stderr)
    );
    assert_eq!(stdout(&confirmed).trim(), "<status><more>");

    world.json(&["trust", "record", id]);
    world.json(&["enable", id]);
    let applied = world.json(&["apply"]);
    let shim = world.shim(&applied, "say");
    let by_shim = world.run_shim(&shim, &["a b", "c"]);
    assert!(
        by_shim.status.success(),
        "{}",
        String::from_utf8_lossy(&by_shim.stderr)
    );
    assert_eq!(stdout(&by_shim).trim(), "<status><a b><c>");

    let successes = world
        .events_of(id)
        .into_iter()
        .filter(|(action, outcome)| action == "run" && outcome == "success")
        .count();
    assert_eq!(successes, 2);
    let stats = world.json(&["stats"])["data"].clone();
    assert_eq!(stats["ever_used"], 1);
    assert_eq!(stats["by_kind"][0]["kind"], "alias");

    // Editing the saved command line is a new revision, and drops it back to
    // needing review.
    let manifest_path = world.capsule_dir(id).join("manifest.toml");
    let edited = fs::read_to_string(&manifest_path)
        .unwrap()
        .replace("' status\"", "' changed\"");
    fs::write(&manifest_path, edited).unwrap();
    assert_eq!(
        world.json(&["trust", "show", id])["data"]["state"],
        "unseen"
    );
    assert!(world
        .refused(&["run", id])
        .contains("has not been reviewed"));
}

// ---------------------------------------------------------------------------
// Capture carries intent
// ---------------------------------------------------------------------------

#[test]
fn a_command_captured_with_a_kind_and_an_export_promotes_without_a_name() {
    let world = World::new();

    let captured = world.json(&[
        "capture",
        "deploy",
        "--body",
        "echo shipped",
        "--kind",
        "script",
        "--export",
        "deploy",
    ]);
    let candidate = captured["data"]["candidate"].as_str().unwrap().to_string();

    let promoted = world.json(&["promote", &candidate]);

    assert_eq!(promoted["data"]["id"], "script/personal/deploy");
    assert_eq!(promoted["data"]["exports"][0], "deploy");
    let run = world.aikit_raw(&["run", "script/personal/deploy", "--confirm"]);
    assert_eq!(
        stdout(&run).trim(),
        "shipped",
        "a captured command line runs as written"
    );
}

#[test]
fn prose_is_filed_as_a_question_not_as_a_script() {
    let world = World::new();

    let captured = world.json(&[
        "capture",
        "a note",
        "--body",
        "Remember to ask the owner before changing the schema.",
    ]);

    let data = &captured["data"];
    assert_eq!(data["kind"], "guidance");
    assert_eq!(data["confidence"], "low");
    assert!(
        data["kind_reason"].as_str().unwrap().contains("prose"),
        "{data}"
    );
    assert!(data["exports"].as_array().unwrap().is_empty());
    assert_eq!(data["scope"], "home");
    assert!(data["by"].is_null() && data["source_ref"].is_null());
}

#[test]
fn the_existing_capture_reply_keeps_every_field_it_had() {
    let world = World::new();

    let captured = world.json(&["capture", "ci", "--body", "#!/bin/sh\ncargo test\n"]);

    for field in [
        "candidate",
        "kind",
        "state",
        "quarantined",
        "findings",
        "duplicate_of",
        "similar",
    ] {
        assert!(
            captured["data"].get(field).is_some(),
            "`{field}` is read by the Capture skill device and must stay"
        );
    }
    assert_eq!(captured["data"]["kind"], "script");
    assert_eq!(captured["data"]["confidence"], "high");
}

#[test]
fn a_capture_that_contradicts_itself_is_refused_before_anything_is_stored() {
    let world = World::new();

    let exports_on_prose = world.refused(&[
        "capture",
        "note",
        "--body",
        "Ask the owner first.",
        "--export",
        "note",
    ]);
    assert!(
        exports_on_prose.contains("--kind script"),
        "{exports_on_prose}"
    );

    let bad_name = world.refused(&[
        "capture",
        "x",
        "--body",
        "echo hi && true",
        "--kind",
        "script",
        "--export",
        "a b",
    ]);
    assert!(bad_name.contains("cannot be a command name"), "{bad_name}");

    let bad_kind = world.refused(&["capture", "x", "--body", "echo hi", "--kind", "banana"]);
    assert!(bad_kind.contains("banana"), "{bad_kind}");
    let bad_scope = world.refused(&["capture", "x", "--body", "echo hi", "--scope", "galaxy"]);
    assert!(bad_scope.contains("galaxy"), "{bad_scope}");
    let session_without_one =
        world.refused(&["capture", "x", "--body", "echo hi", "--scope", "session"]);
    assert!(
        session_without_one.contains("session"),
        "{session_without_one}"
    );
}

#[test]
fn a_project_scoped_capture_is_saved_in_the_project_not_in_the_personal_registry() {
    let world = World::new();

    let captured = world.json(&[
        "capture",
        "lint",
        "--body",
        "echo linting && true",
        "--kind",
        "script",
        "--export",
        "lint",
        "--scope",
        "project",
    ]);
    assert_eq!(captured["data"]["scope"], "project");
    assert_eq!(captured["data"]["proposed_id"], "script/project/lint");
    let candidate = captured["data"]["candidate"].as_str().unwrap().to_string();

    let promoted = world.json(&["promote", &candidate]);

    assert_eq!(promoted["data"]["scope"], "project");
    let in_project = world
        .project
        .join(".aikit/capsules/script/project/lint/manifest.toml");
    assert!(in_project.is_file(), "{}", in_project.display());
    assert!(!world.capsule_dir("script/project/lint").exists());
    assert_eq!(
        search_ids(&world, "kind:script lint"),
        ["script/project/lint"]
    );
}

// ---------------------------------------------------------------------------
// One inbox
// ---------------------------------------------------------------------------

fn inbox_items(world: &World, all: bool) -> Vec<Value> {
    let mut args = vec!["inbox"];
    if all {
        args.push("--all");
    }
    world.json(&args)["data"]["items"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn a_captured_example_shows_in_the_inbox_and_accepting_it_saves_it_and_resolves_the_message() {
    let world = World::new();
    assert!(inbox_items(&world, false).is_empty());

    let captured = world.json(&[
        "capture",
        "deploy staging",
        "--body",
        "echo deploying \"$@\"",
        "--kind",
        "script",
        "--export",
        "deploy",
        "--by",
        "claude-code:integrator",
    ]);
    let candidate = captured["data"]["candidate"].as_str().unwrap().to_string();
    let item_id = captured["data"]["inbox_item"].as_str().unwrap().to_string();
    let proposal = captured["data"]["proposal"].as_str().unwrap().to_string();

    // -- the example is on the inbox, as an agent reading the broker sees it --
    let items = inbox_items(&world, false);
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item["id"], item_id.as_str());
    assert_eq!(item["kind"], "capture-candidate");
    assert_eq!(item["proposal"], proposal.as_str());
    assert!(item["title"].as_str().unwrap().contains("deploy staging"));
    assert!(item["body"]
        .as_str()
        .unwrap()
        .contains("script/personal/deploy"));
    let evidence = item["evidence"].as_array().unwrap();
    assert!(
        evidence
            .iter()
            .any(|e| e["label"] == "candidate" && e["value"] == candidate.as_str()),
        "the evidence is the candidate: {evidence:?}"
    );
    assert!(
        !world.capsule_dir("script/personal/deploy").exists(),
        "announcing writes nothing"
    );

    // -- accept: runs the planned Procedure, resolves both sides --------------
    let accepted = world.json(&["inbox", "accept", &item_id]);
    let data = &accepted["data"];
    assert_eq!(data["id"], "script/personal/deploy");
    assert_eq!(
        data["procedure"],
        proposal.as_str(),
        "it ran the plan it showed"
    );
    assert!(data["applied"].as_u64().unwrap() > 0);
    assert!(world
        .capsule_dir("script/personal/deploy")
        .join("manifest.toml")
        .is_file());

    assert!(
        inbox_items(&world, false).is_empty(),
        "nothing left to decide"
    );
    let resolved = &inbox_items(&world, true)[0];
    assert_eq!(resolved["state"]["state"], "resolved");
    assert!(resolved["state"]["decision"]
        .as_str()
        .unwrap()
        .contains("script/personal/deploy"));
    // The example is decided too: promoting it again says so.
    let again = world.refused(&["inbox", "accept", &candidate]);
    assert!(again.contains("already saved"), "{again}");

    // -- and it can be taken back by the Procedure that wrote it ---------------
    let undo = data["undo"].as_str().unwrap().to_string();
    let procedure = undo.rsplit(' ').next().unwrap();
    let undone = world.aikit_raw(&["procedure", "undo", procedure]);
    assert!(
        undone.status.success(),
        "{}",
        String::from_utf8_lossy(&undone.stderr)
    );
    assert!(!world.capsule_dir("script/personal/deploy").exists());
}

#[test]
fn rejecting_from_the_inbox_resolves_the_message_and_the_example() {
    let world = World::new();
    let captured = world.json(&["capture", "ci", "--body", "#!/bin/sh\ncargo test\n"]);
    let candidate = captured["data"]["candidate"].as_str().unwrap().to_string();

    let rejected = world.json(&["inbox", "reject", &candidate, "--reason", "one-off"]);

    assert_eq!(rejected["data"]["state"], "rejected");
    assert!(inbox_items(&world, false).is_empty());
    assert_eq!(
        inbox_items(&world, true)[0]["state"]["decision"],
        "rejected: one-off"
    );
    let promoted = world.refused(&["promote", &candidate]);
    assert!(promoted.contains("turned down"), "{promoted}");
    assert!(
        !world.capsule_dir("script/personal/ci").exists(),
        "a rejected example is never written"
    );
}

#[test]
fn promoting_straight_from_the_candidate_resolves_the_message_too() {
    let world = World::new();
    let captured = world.json(&[
        "capture",
        "say",
        "--body",
        "printf hi && true",
        "--kind",
        "script",
        "--export",
        "hi",
    ]);
    let candidate = captured["data"]["candidate"].as_str().unwrap().to_string();
    assert_eq!(inbox_items(&world, false).len(), 1);

    world.json(&["promote", &candidate]);

    assert!(inbox_items(&world, false).is_empty());
}

#[test]
fn a_capture_holding_a_secret_is_on_the_inbox_as_held_back_and_cannot_be_accepted() {
    let world = World::new();
    let captured = world.json(&[
        "capture",
        "publish",
        "--body",
        "#!/bin/sh\nexport GITHUB_TOKEN=ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8\ncargo publish\n",
    ]);
    assert_eq!(captured["data"]["quarantined"], true);
    assert!(captured["data"]["proposal"].is_null());
    let candidate = captured["data"]["candidate"].as_str().unwrap().to_string();

    let items = inbox_items(&world, false);
    assert_eq!(items.len(), 1);
    assert!(items[0]["title"].as_str().unwrap().starts_with("Held back"));
    assert!(items[0]["proposal"].is_null());
    let text = serde_json::to_string(&items[0]).unwrap();
    assert!(
        !text.contains("ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8"),
        "the secret never reaches the inbox: {text}"
    );

    let refused = world.refused(&["inbox", "accept", &candidate]);
    assert!(refused.contains("quarantined"), "{refused}");
}

#[test]
fn only_a_captured_example_can_be_accepted_or_rejected_from_the_inbox() {
    let world = World::new();

    let unknown = world.refused(&["inbox", "accept", "inb_does_not_exist"]);

    assert!(unknown.contains("no inbox item"), "{unknown}");
}

// ---------------------------------------------------------------------------
// The recurrence observer
// ---------------------------------------------------------------------------

fn observe(world: &World, command: &str, by: &str) -> Value {
    world.json(&["observe", "run", "--command", command, "--by", by])
}

#[test]
fn the_third_identical_agent_command_is_offered_on_the_inbox_and_can_be_accepted_and_run() {
    let world = World::new();
    let command = "sh -c 'echo built'";

    let first = observe(&world, command, "claude-code:saved-ops");
    let second = observe(&world, command, "claude-code:saved-ops");
    assert_eq!(first["data"]["runs"][0]["count"], 1);
    assert_eq!(second["data"]["runs"][0]["count"], 2);
    assert!(
        inbox_items(&world, false).is_empty(),
        "two is not a pattern"
    );

    let third = observe(&world, command, "claude-code:saved-ops");

    let run = &third["data"]["runs"][0];
    assert_eq!(run["count"], 3);
    assert_eq!(run["threshold"], 3);
    let offered = &run["offered"];
    assert_eq!(offered["new"], true);
    assert_eq!(third["data"]["offered"][0], offered["candidate"]);

    // On the inbox, carrying the agent's name, with a plan to save it.
    let items = inbox_items(&world, false);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "capture-candidate");
    assert_eq!(items[0]["id"], offered["inbox_item"]);
    assert!(items[0]["body"]
        .as_str()
        .unwrap()
        .contains("claude-code:saved-ops"));
    assert_eq!(items[0]["proposal"], offered["proposal"]);

    // The fourth run offers nothing more.
    let fourth = observe(&world, command, "claude-code:saved-ops");
    assert!(fourth["data"]["runs"][0]["offered"].is_null());
    assert_eq!(inbox_items(&world, false).len(), 1);

    // Accept it and run it: the habit became a saved operation.
    let accepted = world.json(&["inbox", "accept", offered["inbox_item"].as_str().unwrap()]);
    let id = accepted["data"]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("script/personal/sh-c-echo-built"), "{id}");
    let ran = world.aikit_raw(&["run", &id, "--confirm"]);
    assert_eq!(stdout(&ran).trim(), "built");
}

#[test]
fn a_human_typed_run_is_attributed_to_the_human() {
    let world = World::new();

    for _ in 0..3 {
        observe(&world, "make -j8 check && make doc", "human");
    }

    let items = inbox_items(&world, false);
    assert_eq!(items.len(), 1);
    assert!(items[0]["body"]
        .as_str()
        .unwrap()
        .contains("Observed by human"));
}

#[test]
fn a_terminal_or_factory_feed_reports_a_batch_as_json_lines() {
    let world = World::new();
    let secret = "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8";
    let line = |command: &str, at: &str| {
        serde_json::json!({
            "command": command, "cwd": "/work/app", "by": "human",
            "at": at, "session": "ses_1"
        })
        .to_string()
    };
    let batch = [
        line("terraform plan -out=tf.plan", "2026-10-16T09:00:00Z"),
        line("ls -la", "2026-10-16T09:01:00Z"),
        line("terraform plan -out=tf.plan", "2026-10-15T09:00:00Z"),
        line(
            &format!("curl -H 'Authorization: token {secret}' https://x"),
            "2026-10-16T09:02:00Z",
        ),
        line("terraform plan -out=tf.plan", "2026-10-14T09:00:00Z"),
    ]
    .join("\n");

    let output = world.with_stdin(&["observe", "run", "--stdin-jsonl", "--json"], &batch, &[]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = stdout(&output);
    assert!(
        !text.contains(secret),
        "the reply never repeats a secret: {text}"
    );
    let reply: Value = serde_json::from_str(&text).unwrap();
    let data = &reply["data"];
    assert_eq!(data["recorded"], 3);
    assert_eq!(data["skipped"], 2);
    assert_eq!(data["offered"].as_array().unwrap().len(), 1);
    let skipped: Vec<&str> = data["runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|run| run["skipped"].as_str())
        .collect();
    assert_eq!(skipped, ["noise", "secret"]);
    assert_eq!(
        data["runs"][4]["count"], 3,
        "the window is anchored on the newest run, so a batch arriving out of order still counts"
    );
}

#[test]
fn an_agent_harnesss_post_tool_use_event_feeds_the_observer_and_never_fails_the_hook() {
    let world = World::new();
    let bash = |command: &str| {
        serde_json::json!({
            "session_id": "ses_9", "transcript_path": "/tmp/t.jsonl", "cwd": "/work/app",
            "hook_event_name": "PostToolUse", "tool_name": "Bash",
            "tool_input": { "command": command, "description": "build" },
            "tool_response": { "stdout": "ok", "stderr": "", "interrupted": false }
        })
        .to_string()
    };
    let feed = |payload: &str| {
        world.with_stdin(
            &["observe", "run", "--from-hook", "--json"],
            payload,
            &[("AIKIT_ACTOR", "claude-code:hooked")],
        )
    };

    let one = feed(&bash("cargo build --release --locked"));
    assert!(one.status.success());
    let one: Value = serde_json::from_slice(&one.stdout).unwrap();
    assert_eq!(one["data"]["runs"][0]["by"], "claude-code:hooked");
    feed(&bash("cargo build --release --locked"));

    // Another tool's event is not a run; garbage is a warning, not a failure.
    let read =
        serde_json::json!({"tool_name": "Read", "tool_input": {"file_path": "/x"}}).to_string();
    let ignored: Value = serde_json::from_slice(&feed(&read).stdout).unwrap();
    assert_eq!(ignored["data"]["ignored"], "not a shell command");
    assert!(ignored["data"]["runs"].as_array().unwrap().is_empty());
    let garbage = feed("this is not json");
    assert!(
        garbage.status.success(),
        "a hook must never fail on the observer's account"
    );
    let garbage: Value = serde_json::from_slice(&garbage.stdout).unwrap();
    assert!(garbage["warnings"][0]
        .as_str()
        .unwrap()
        .contains("not JSON"));

    let third: Value =
        serde_json::from_slice(&feed(&bash("cargo build --release --locked")).stdout).unwrap();
    assert_eq!(third["data"]["runs"][0]["count"], 3);
    assert_eq!(third["data"]["offered"].as_array().unwrap().len(), 1);
    let items = inbox_items(&world, false);
    assert!(items[0]["body"]
        .as_str()
        .unwrap()
        .contains("claude-code:hooked"));
}

#[test]
fn a_codex_post_tool_use_event_is_read_the_way_its_documentation_sends_it() {
    // Codex's documented PostToolUse: JSON on stdin; tool_name Bash with a string
    // tool_input.command; cwd, session_id, turn_id, permission_mode and model;
    // tool_response is the tool's model-facing output (here a plain string).
    let world = World::new();
    let bash = |command: &str| {
        serde_json::json!({
            "session_id": "thr_123", "turn_id": "turn_7", "transcript_path": "/w/.codex/rollout.jsonl",
            "cwd": "/work/app", "hook_event_name": "PostToolUse", "model": "gpt-5",
            "permission_mode": "default", "tool_name": "Bash", "tool_use_id": "call_1",
            "tool_input": { "command": command },
            "tool_response": "Compiling app v0.1.0\nFinished dev [unoptimized] target(s)"
        })
        .to_string()
    };
    let feed = |payload: &str| {
        let out = world.with_stdin(
            &["observe", "run", "--from-hook", "--json"],
            payload,
            &[("AIKIT_ACTOR", "codex:thr_123")],
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };

    let first = feed(&bash("cargo build --workspace --locked"));
    assert_eq!(first["data"]["runs"][0]["by"], "codex:thr_123");
    assert_eq!(
        first["data"]["runs"][0]["recorded"], true,
        "no exit code means not marked failed"
    );
    feed(&bash("cargo build --workspace --locked"));

    // apply_patch and MCP tools are not shell commands.
    let patch = serde_json::json!({
        "tool_name": "apply_patch", "tool_input": { "command": "*** Begin Patch" }, "cwd": "/w"
    })
    .to_string();
    assert_eq!(feed(&patch)["data"]["ignored"], "not a shell command");
    let mcp = serde_json::json!({
        "tool_name": "mcp__fs__read", "tool_input": { "path": "/x" }, "cwd": "/w"
    })
    .to_string();
    assert_eq!(feed(&mcp)["data"]["ignored"], "not a shell command");

    let third = feed(&bash("cargo build --workspace --locked"));
    assert_eq!(third["data"]["runs"][0]["count"], 3);
    assert_eq!(third["data"]["offered"].as_array().unwrap().len(), 1);
    let body = inbox_items(&world, false)[0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains("Observed by codex:thr_123"), "{body}");
}

#[test]
fn the_shipped_hook_capsule_carries_the_real_post_tool_use_stream_to_the_observer() {
    let world = World::new();
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../registry/capsules/hook/aikit/observe-runs");
    let installed = world.capsule_dir("hook/aikit/observe-runs");
    fs::create_dir_all(installed.join("payload")).unwrap();
    fs::copy(
        shipped.join("manifest.toml"),
        installed.join("manifest.toml"),
    )
    .unwrap();
    fs::copy(
        shipped.join("payload/observe-runs"),
        installed.join("payload/observe-runs"),
    )
    .unwrap();

    // Opt-in is the capsule machinery's: reviewed, then enabled.
    world.json(&["trust", "record", "hook/aikit/observe-runs"]);
    world.json(&["enable", "hook/aikit/observe-runs", "--scope", "project"]);

    let payload = |tool: &str, command: &str| {
        serde_json::json!({
            "session_id": "ses_e2e", "cwd": world.project, "hook_event_name": "PostToolUse",
            "tool_name": tool, "tool_input": { "command": command }
        })
        .to_string()
    };
    let dispatch = |payload: String| {
        world.with_stdin(
            &["hook", "dispatch", "claude", "PostToolUse", "--json"],
            &payload,
            &[("AIKIT_ACTOR", "claude-code:e2e")],
        )
    };

    // A non-shell tool is not routed to the capsule at all (the matcher).
    let read = dispatch(payload("Read", "ignored"));
    assert!(read.status.success());
    for _ in 0..3 {
        let out = dispatch(payload("Bash", "sh -c 'echo through the hook'"));
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            reply["data"]["allowed"], true,
            "an observer never denies: {reply}"
        );
    }

    let items = inbox_items(&world, false);
    assert_eq!(items.len(), 1, "three Bash events through the dispatcher");
    assert_eq!(items[0]["kind"], "capture-candidate");
    assert!(items[0]["title"]
        .as_str()
        .unwrap()
        .contains("echo through the hook"));
    // Attribution crossed the dispatcher: the actor and the session are named.
    let body = items[0]["body"].as_str().unwrap();
    assert!(body.contains("Observed by claude-code:e2e"), "{body}");
    assert!(body.contains("ses_e2e"), "{body}");
}

#[test]
fn status_lists_what_is_repeating_and_whether_it_was_offered() {
    let world = World::new();
    observe(&world, "npm run build:all", "human");
    observe(&world, "npm run build:all", "human");

    let status = world.json(&["observe", "status"])["data"].clone();
    assert_eq!(status["count"], 1);
    assert_eq!(status["repeating"][0]["command"], "npm run build:all");
    assert_eq!(status["repeating"][0]["count"], 2);
    assert!(status["repeating"][0]["offered"].is_null());

    observe(&world, "npm run build:all", "human");
    let status = world.json(&["observe", "status"])["data"].clone();
    assert!(status["repeating"][0]["offered"]
        .as_str()
        .unwrap()
        .starts_with("cnd_"));
    assert_eq!(status["repeating"][0]["waiting"], true);
}

#[test]
fn a_feed_must_say_what_it_is_feeding_and_a_bad_span_is_refused() {
    let world = World::new();

    let nothing = world.refused(&["observe", "run"]);
    assert!(nothing.contains("--command"), "{nothing}");
    let span = world.refused(&[
        "observe",
        "run",
        "--command",
        "make all",
        "--window",
        "soon",
    ]);
    assert!(span.contains("not a span"), "{span}");
    let time = world.refused(&[
        "observe",
        "run",
        "--command",
        "make all",
        "--at",
        "yesterday",
    ]);
    assert!(time.contains("not a time"), "{time}");
}

// ---------------------------------------------------------------------------
// Search aliases: `@name = expression`
// ---------------------------------------------------------------------------

#[test]
fn a_search_alias_is_defined_found_and_expanded_by_search() {
    let world = World::new();

    let defined = world.json(&["alias", "define", "@gs = @# ( @ ? )", "--by", "human"]);
    let data = &defined["data"];
    assert_eq!(data["id"], "alias/personal/gs");
    assert_eq!(data["alias"], "@gs");
    assert_eq!(data["takes"], 1);
    assert_eq!(data["replaced"], false);
    let manifest =
        fs::read_to_string(world.capsule_dir("alias/personal/gs").join("manifest.toml")).unwrap();
    assert!(manifest.contains("syntax = \"operative\""), "{manifest}");
    assert!(manifest.contains("author = \"human\""), "{manifest}");

    // It is a capsule like any other: found by the kind filter, no second registry.
    assert_eq!(search_ids(&world, "kind:alias gs"), ["alias/personal/gs"]);

    // Search expands it; the reply says how, and the expression is the one the
    // expanded text would have given if it had been typed out.
    let aliased = world.json(&["search", "@gs orient"]);
    assert_eq!(
        aliased["data"]["alias_expansion"]["expanded"],
        "@# ( @ orient )"
    );
    assert_eq!(aliased["data"]["alias_expansion"]["aliases"][0], "gs");
    let written = world.json(&["search", "@# ( @ orient )"]);
    assert_eq!(aliased["data"]["expression"], written["data"]["expression"]);
    assert!(written["data"].get("alias_expansion").is_none());

    // A query that names no alias is passed on exactly as typed.
    let plain = world.json(&["search", "release notes"]);
    assert!(plain["data"].get("alias_expansion").is_none());

    let shown = world.json(&["alias", "expand", "@gs", "orient"]);
    assert_eq!(shown["data"]["expanded"], "@# ( @ orient )");
    assert_eq!(shown["data"]["input"], "@gs orient");
}

#[test]
fn a_search_alias_is_a_name_not_a_command() {
    let world = World::new();
    world.json(&["alias", "define", "@gs = @# ( @ ? )"]);

    // It cannot be run, and is not asked to be reviewed first.
    let run = world.refused(&["run", "alias/personal/gs"]);
    assert!(run.contains("operative"), "{run}");
    assert!(!run.contains("has not been reviewed"), "{run}");
    // z shows it rather than proposing a run that could only refuse.
    let z = world.json(&["z", "gs"]);
    assert_eq!(z["data"]["capability"], "alias/personal/gs");
    assert_eq!(z["data"]["action"], "open");

    // Enabled and applied, it puts nothing on the PATH.
    world.json(&["enable", "alias/personal/gs"]);
    let applied = world.json(&["apply"]);
    let context = applied["context"]["context_id"].as_str().unwrap();
    let generation = applied["data"]["generation"].as_str().unwrap();
    let bin = world
        .aikit_home
        .join("state/contexts")
        .join(context)
        .join("generations")
        .join(generation)
        .join("bin");
    assert!(!bin.join("gs").exists(), "an operative alias has no shim");
}

#[test]
fn definitions_are_checked_before_they_are_saved() {
    let world = World::new();
    world.json(&["alias", "define", "@a = @# ( @ ? )"]);

    // Redefining needs saying so, and keeps working afterwards.
    let again = world.refused(&["alias", "define", "@a = @ ?"]);
    assert!(again.contains("--replace"), "{again}");
    let replaced = world.json(&["alias", "define", "@a = @ ?", "--replace"]);
    assert_eq!(replaced["data"]["replaced"], true);
    assert_eq!(
        world.json(&["alias", "expand", "@a", "x"])["data"]["expanded"],
        "@ x"
    );

    // A loop through another alias is refused, naming the chain; nothing changes.
    world.json(&["alias", "define", "@b = @a ?"]);
    let before =
        fs::read_to_string(world.capsule_dir("alias/personal/a").join("manifest.toml")).unwrap();
    let cycle = world.refused(&["alias", "define", "@a = @b ?", "--replace"]);
    assert!(cycle.contains("@a -> @b -> @a"), "{cycle}");
    let after =
        fs::read_to_string(world.capsule_dir("alias/personal/a").join("manifest.toml")).unwrap();
    assert_eq!(before, after);

    for (bad, why) in [
        (&["alias", "define", "gs = x"][..], "@name = expression"),
        (&["alias", "define", "@Gs = x"][..], "@name = expression"),
        (&["alias", "define", "@c = ( @ ?"][..], "does not expand"),
        (&["alias", "define", "@d = ? ? ? ? ?"][..], "at most 4"),
    ] {
        let refusal = world.refused(bad);
        assert!(refusal.contains(why), "{bad:?}: {refusal}");
    }

    // A name a shell alias already holds is not taken twice.
    let captured = world.json(&[
        "capture",
        "say",
        "--body",
        "printf hi",
        "--kind",
        "alias",
        "--export",
        "say",
    ]);
    world.json(&["promote", captured["data"]["candidate"].as_str().unwrap()]);
    let taken = world.refused(&["alias", "define", "@say = @ ?"]);
    assert!(taken.contains("shell alias"), "{taken}");
}

#[test]
fn using_an_alias_without_the_word_it_wants_says_so() {
    let world = World::new();
    world.json(&["alias", "define", "@gs = @# ( @ ? )"]);

    let missing = world.refused(&["search", "@gs"]);

    assert!(missing.contains("takes 1 word"), "{missing}");
}

// ---------------------------------------------------------------------------
// One reading of what is saved
// ---------------------------------------------------------------------------

#[test]
fn saved_reads_what_is_saved_from_where_by_whom_at_which_revision_and_how_often_used() {
    let world = World::new();

    // A script an agent captured with its source, promoted and used twice.
    let c = world.json(&[
        "capture",
        "deploy staging",
        "--body",
        "echo deploying \"$@\"",
        "--kind",
        "script",
        "--export",
        "deploy",
        "--by",
        "claude-code:saved-ops",
        "--source-ref",
        "terminal:pane-1",
    ]);
    world.json(&["promote", c["data"]["candidate"].as_str().unwrap()]);
    world.aikit_raw(&["run", "script/personal/deploy", "--confirm", "a"]);
    world.aikit_raw(&["run", "script/personal/deploy", "--confirm", "b"]);
    // A shell alias a person captured, never used.
    let c = world.json(&[
        "capture",
        "say",
        "--body",
        "printf hi",
        "--kind",
        "alias",
        "--export",
        "say",
        "--by",
        "human",
    ]);
    world.json(&["promote", c["data"]["candidate"].as_str().unwrap()]);
    // A search alias, a project script, and a capture still waiting.
    world.json(&["alias", "define", "@gs = @# ( @ ? )", "--by", "human"]);
    let c = world.json(&[
        "capture",
        "lint",
        "--body",
        "echo linting && true",
        "--kind",
        "script",
        "--export",
        "lint",
        "--scope",
        "project",
    ]);
    world.json(&["promote", c["data"]["candidate"].as_str().unwrap()]);
    world.json(&[
        "capture",
        "later",
        "--body",
        "make -j8 check && make doc",
        "--kind",
        "script",
        "--export",
        "later",
        "--by",
        "claude-code:other",
    ]);
    let events_before = world.json(&["stats"])["data"]["events"].clone();

    let saved = world.json(&["saved"]);
    let data = &saved["data"];

    // -- the saved operations, most used first ------------------------------
    let rows = data["saved"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["id"], "script/personal/deploy", "most used leads");
    let deploy = &rows[0];
    assert_eq!(deploy["kind"], "script");
    assert_eq!(deploy["command"], "deploy");
    assert_eq!(deploy["exports"][0], "deploy");
    assert_eq!(deploy["registry"], "personal");
    assert_eq!(deploy["scope"], "home");
    assert_eq!(deploy["by"], "claude-code:saved-ops");
    assert_eq!(deploy["source_ref"], "terminal:pane-1");
    assert_eq!(deploy["origin"], "harvested");
    assert!(deploy["candidate"].as_str().unwrap().starts_with("cnd_"));
    assert!(deploy["saved_at"].as_str().is_some());
    assert!(deploy["revision"].as_str().unwrap().len() >= 32);
    assert_eq!(deploy["use"]["successful_runs"], 2);
    assert!(deploy["use"]["last_success_seconds_ago"].as_u64().is_some());
    assert_eq!(deploy["trust"], "unseen");
    assert_eq!(deploy["reviewed"], false);
    assert!(deploy["manifest"]
        .as_str()
        .unwrap()
        .ends_with("manifest.toml"));

    let by_id = |id: &str| rows.iter().find(|r| r["id"] == id).unwrap();
    let say = by_id("alias/personal/say");
    assert_eq!(
        (say["syntax"].as_str(), say["command"].as_str()),
        (Some("shell"), Some("say"))
    );
    assert_eq!(say["use"]["successful_runs"], 0);
    assert!(say["use"]["last_success_seconds_ago"].is_null());
    let gs = by_id("alias/personal/gs");
    assert_eq!(
        (
            gs["syntax"].as_str(),
            gs["command"].as_str(),
            gs["origin"].as_str()
        ),
        (Some("operative"), Some("@gs"), Some("authored"))
    );
    assert!(
        gs["exports"].as_array().unwrap().is_empty(),
        "a search alias is no command"
    );
    let lint = by_id("script/project/lint");
    assert_eq!(
        (lint["scope"].as_str(), lint["registry"].as_str()),
        (Some("project"), Some("project-local"))
    );

    // -- what is waiting ----------------------------------------------------
    let waiting = data["waiting"].as_array().unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0]["proposed_id"], "script/personal/later");
    assert_eq!(waiting[0]["by"], "claude-code:other");
    assert!(waiting[0]["accept"]
        .as_str()
        .unwrap()
        .starts_with("aikit inbox accept cnd_"));

    // -- the summary --------------------------------------------------------
    let summary = &data["summary"];
    assert_eq!(summary["saved"], 4);
    assert_eq!(summary["scripts"], 2);
    assert_eq!(summary["aliases"]["shell"], 1);
    assert_eq!(summary["aliases"]["operative"], 1);
    assert_eq!(summary["ever_used"], 1);
    assert_eq!(summary["never_used"], 3);
    assert_eq!(summary["waiting"], 1);

    // -- filters, and it changes nothing ------------------------------------
    assert_eq!(
        world.json(&["saved", "--kind", "alias"])["data"]["saved"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        world.json(&["saved", "--limit", "1"])["data"]["saved"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        world.json(&["stats"])["data"]["events"],
        events_before,
        "a reading records nothing"
    );

    // Reviewing is reflected, because it is read, not remembered.
    world.json(&["trust", "record", "script/personal/deploy"]);
    let again = world.json(&["saved", "--kind", "script"]);
    let deploy = again["data"]["saved"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "script/personal/deploy")
        .unwrap();
    assert_eq!(deploy["reviewed"], true);
}

#[test]
fn a_hook_run_inside_a_terminal_pane_is_kept_with_that_pane() {
    let world = World::new();
    let bash = |command: &str| {
        serde_json::json!({
            "session_id": "ses_9", "cwd": "/work/app", "hook_event_name": "PostToolUse",
            "tool_name": "Bash", "tool_input": { "command": command },
            "tool_response": { "stdout": "", "stderr": "", "interrupted": false }
        })
        .to_string()
    };
    let feed = |payload: &str, env: &[(&str, &str)]| -> Value {
        let out = world.with_stdin(
            &["observe", "run", "--from-hook", "--json"],
            payload,
            &[&[("AIKIT_ACTOR", "agent-session/av")], env].concat(),
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    };

    // A harness in a tmux pane: the pane, on the server TMUX names.
    let tmux = feed(
        &bash("make lane-av-proof"),
        &[
            ("TMUX", "/private/tmp/tmux-501/aikit-walk,4242,0"),
            ("TMUX_PANE", "%3"),
        ],
    );
    assert_eq!(
        tmux["data"]["runs"][0]["panes"],
        serde_json::json!(["tmux:%3@/private/tmp/tmux-501/aikit-walk,4242"])
    );
    // Even a shell staple is kept for its pane, though it never counts.
    let staple = feed(
        &bash("ls -la"),
        &[
            ("HERDR_PANE_ID", "w1:p1"),
            ("HERDR_SOCKET_PATH", "/h/herdr.sock"),
        ],
    );
    assert_eq!(staple["data"]["runs"][0]["skipped"], "noise");
    assert_eq!(
        staple["data"]["runs"][0]["panes"],
        serde_json::json!(["herdr:w1:p1@/h/herdr.sock"])
    );
    // A harness in no pane names none.
    let nowhere = feed(&bash("make lane-av-proof"), &[]);
    assert_eq!(nowhere["data"]["runs"][0]["panes"], serde_json::json!([]));

    // A feed names a run's pane itself, by flag or by field.
    let flagged = world.json(&[
        "observe",
        "run",
        "--command",
        "make lane-av-proof",
        "--by",
        "agent-session/av",
        "--pane",
        "cmux:surface:20",
    ]);
    assert_eq!(
        flagged["data"]["runs"][0]["panes"],
        serde_json::json!(["cmux:surface:20"])
    );
    let fed = world.with_stdin(
        &["observe", "run", "--stdin-jsonl", "--json"],
        r#"{"command":"make lane-av-proof","by":"agent-session/av","pane":"tmux:%3@/private/tmp/tmux-501/aikit-walk,4242"}"#,
        &[],
    );
    let fed: Value = serde_json::from_slice(&fed.stdout).unwrap();
    assert_eq!(
        fed["data"]["runs"][0]["panes"],
        serde_json::json!(["tmux:%3@/private/tmp/tmux-501/aikit-walk,4242"])
    );
}
