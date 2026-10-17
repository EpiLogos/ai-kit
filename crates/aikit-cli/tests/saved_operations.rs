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
        command
    }

    fn aikit_raw(&self, args: &[&str]) -> Output {
        self.command(&assert_cmd::cargo::cargo_bin("aikit"))
            .args(args)
            .output()
            .expect("aikit runs")
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
