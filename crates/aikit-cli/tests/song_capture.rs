//! O-I #844: Capture on the Skillset rack's Capture page writes a Method from a song's clip and sends it to the person to accept
//! (DLW manual §19.1 Songs; §19.7 Making a skill: the Capture page — "Source: The last minutes · A selected clip", "The member
//! arrives as a proposal. It is kept as a draft only when you accept it", "A proposal that holds a secret is held").
//!
//! Proves, through the real binary against a temporary AIKit home and `HOME`: `aikit capture --from-song` reads Software Factory's
//! clip (`factory.song-clip/v1`, `factory workflow export`) on stdin and files one capture candidate whose text is a Method — a
//! Skill whose description starts `METHOD:`, its steps the song's units in the order they run — on the one capture road: the
//! inbox announces it with its proposal and nothing is promoted; the candidate keeps the clip's ref as where it came from;
//! promoting it (the person's Accept) keeps it as a draft Skill whose form is Method; a clip that holds a secret is held; anything
//! but a song's clip is refused in AIKit's words, and nothing is filed.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};
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
        fs::write(project.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
        World {
            _tmp: tmp,
            aikit_home,
            user_home,
            project,
        }
    }

    fn run(&self, args: &[&str], input: Option<&str>) -> Output {
        use std::io::Write;
        let mut command = Command::new(assert_cmd::cargo::cargo_bin("aikit"));
        command
            .env("AIKIT_HOME", &self.aikit_home)
            .env("HOME", &self.user_home)
            .env("HISTFILE", self.user_home.join(".history"))
            .env_remove("AIKIT_CONTEXT_ID")
            .env_remove("AIKIT_SESSION_ID")
            .current_dir(&self.project)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in PANE_VARIABLES {
            command.env_remove(name);
        }
        let mut child = command.spawn().expect("aikit spawns");
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.unwrap_or("").as_bytes()).unwrap();
        drop(stdin);
        child.wait_with_output().unwrap()
    }

    fn json(&self, args: &[&str], input: Option<&str>) -> Value {
        let mut args = args.to_vec();
        args.push("--json");
        let output = self.run(&args, input);
        assert!(
            output.status.success(),
            "aikit {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

/// A song's clip as `factory workflow export` writes it (Software Factory song_clip.rs), its scratch paths redacted.
fn clip() -> Value {
    json!({
        "contract": "factory.song-clip/v1",
        "clipRef": "factory-song:song-probe/release-check",
        "name": "release-check",
        "project": {"projectKey": "song-probe", "projectRef": "project:0000000000ZBHKXNKK9QASSKZE"},
        "set": {"entry": "release-check.workflow.ts", "sourceRef": "workflow-source:01M399B6B0R8CJY3Y2RTG5EPWS",
            "sourceRevision": "release-check-v1", "workflowKey": "release-check", "byteRevision": "blake3:c1948266", "semanticDigest": "ae25c1dd"},
        "selection": {"rows": null, "units": null},
        "score": {
            "workflowKey": "release-check",
            "rows": [{"ref": "agent/field-guardian", "kind": "agent"}, {"ref": "agent-set/guardians", "kind": "agent-set"}],
            "units": [
                {"key": "gather", "row": "agent/field-guardian", "rowKind": "agent", "dependencies": [], "heads": null,
                    "concern": "Gather what the release changes", "returns": "The release's changes, product by product",
                    "success": ["every product the release touches is named"], "stop": "Stop when nothing has changed"},
                {"key": "check", "row": "agent-set/guardians", "rowKind": "agent-set", "dependencies": ["gather"], "heads": null,
                    "concern": "Check each product agrees", "returns": "Each guardian's word", "success": ["each guardian answers"]},
                {"key": "report", "row": "agent/field-guardian", "rowKind": "agent", "dependencies": ["check"], "heads": null,
                    "concern": "Report where the guardians disagree", "returns": "The report"}
            ],
            "joins": [], "cut": []
        },
        "render": {"by": "factory workflow export", "renders": 1, "previousByteRevision": null},
        "importedFrom": null,
        "execution": "not-requested"
    })
}

#[test]
fn a_song_s_clip_is_captured_as_a_method_that_waits_for_the_person() {
    let world = World::new();
    let captured = world.json(
        &[
            "capture",
            "Release check",
            "--from-song",
            "--scope",
            "project",
            "--by",
            "human",
        ],
        Some(&clip().to_string()),
    );
    let data = &captured["data"];
    assert_eq!(
        data["kind"], "skill",
        "a Method is a Skill whose text states its form"
    );
    assert_eq!(data["state"], "ready");
    assert_eq!(
        data["source_ref"], "factory-song:song-probe/release-check",
        "the example stays with the Method"
    );
    assert_eq!(data["proposed_id"], "skill/project/method-release-check");
    assert!(
        data["proposal"].is_string(),
        "the inbox carries the planned promotion: {data}"
    );
    assert_eq!(data["song"]["steps"].as_array().unwrap().len(), 3);
    assert!(data["song"]["steps"][0]
        .as_str()
        .unwrap()
        .starts_with("1. **gather**"));
    assert!(data["song"]["steps"][2]
        .as_str()
        .unwrap()
        .starts_with("3. **report**"));
    let candidate = data["candidate"].as_str().unwrap().to_owned();

    // It waits: the inbox lists it, and nothing is in the project's registry until the person accepts it.
    let inbox = world.json(&["inbox"], None);
    let items = inbox["data"]["items"].as_array().unwrap();
    let waiting = items
        .iter()
        .find(|item| item["kind"] == "capture-candidate")
        .expect("the capture waits in the inbox");
    assert!(
        waiting["title"]
            .as_str()
            .unwrap()
            .contains("METHOD: Release check"),
        "{waiting}"
    );
    assert!(
        waiting["body"]
            .as_str()
            .unwrap()
            .contains("skill/project/method-release-check"),
        "{waiting}"
    );
    assert!(
        !world.project.join(".aikit/registry").exists()
            || fs::read_dir(world.project.join(".aikit/registry"))
                .unwrap()
                .next()
                .is_none(),
        "nothing is promoted by the capture itself"
    );

    // The person's Accept keeps it as a draft Skill, and its description names it a Method.
    let promoted = world.json(&["promote", &candidate], None);
    assert_eq!(promoted["data"]["kind"], "skill");
    let manifest = fs::read_to_string(promoted["data"]["manifest"].as_str().unwrap()).unwrap();
    assert!(
        manifest.contains("description = \"METHOD: Release check\""),
        "{manifest}"
    );
    assert!(manifest.contains("maturity = \"draft\""), "{manifest}");
    assert!(
        manifest.contains("source_ref = \"factory-song:song-probe/release-check\""),
        "{manifest}"
    );
    let skill = fs::read_to_string(
        PathBuf::from(promoted["data"]["root"].as_str().unwrap()).join("payload/SKILL.md"),
    )
    .unwrap();
    assert!(skill.contains("## Steps") && skill.contains("**check** — Check each product agrees. Taken by the agent set agent-set/guardians; after gather"), "{skill}");
}

#[test]
fn a_song_that_holds_a_secret_is_held_and_anything_else_is_refused() {
    let world = World::new();
    let mut secret = clip();
    secret["score"]["units"][0]["concern"] = json!("Gather with AKIAIOSFODNN7EXAMPLE and aws_secret_access_key=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
    let held = world.json(
        &["capture", "Leaky", "--from-song"],
        Some(&secret.to_string()),
    );
    assert_eq!(held["data"]["quarantined"], true, "{}", held["data"]);
    assert!(
        held["data"]["proposal"].is_null(),
        "a held proposal offers no promotion"
    );

    let output = world.run(
        &["capture", "Not a song", "--from-song", "--json"],
        Some(r#"{"contract":"factory.workflow-score/v1"}"#),
    );
    assert!(!output.status.success());
    // The refusal is AIKit's, in its words (the JSON envelope carries it under --json).
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        said.contains("capture.song_invalid") && said.contains("factory.song-clip/v1"),
        "{said}"
    );
    let inbox = world.json(&["inbox"], None);
    assert_eq!(
        inbox["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["kind"] == "capture-candidate")
            .count(),
        1,
        "the refused capture filed nothing"
    );
}
