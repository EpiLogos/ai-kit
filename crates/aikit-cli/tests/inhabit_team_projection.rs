//! `aikit inhabit` launches an agent-set orchestrator with its team: every
//! other member of the set it orchestrates becomes a Claude Code subagent of
//! the launched session, rendered from the member's Central profile and its
//! expression file, written for this one inhabitation, handed to the harness
//! with `--plugin-dir`, and removed with the tenure.
//!
//! The owners are fakes behind the production seam: `ctrl` answers from a
//! fixture Central root on disk (agent set, profiles, expression files),
//! `actuation` opens and closes the tenure, and `claude` is a script that
//! prints the argv it was launched with.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::{json, Value};
use tempfile::TempDir;

const POSITION: &str = "central:position:control:root:anima";
const CLAIMED: &str = "actuation:generation:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct World {
    _dir: TempDir,
    root: PathBuf,
    central: PathBuf,
    log: PathBuf,
}

fn member_expression(slug: &str, member: &str) -> String {
    format!(
        "---\nname: anima-{slug}\nteam: anima\ncf: CF1\ndescription: \"{member}, one member of the fixture team: invoked for {slug} work.\"\ntools: [Read, Grep, Glob]\nskills: [skill/ql/vak-coordinate-frame, skill/personal/brainstorming]\nsource: {{repo: EpiLogos/fixture, path: \"agents/{slug}.md\", blob: 0123abcd}}\n---\n\n# {member}\n\nYou are {member}. Work only on what you are handed.\n"
    )
}

fn profile(slug: &str, member_file: &str) -> Value {
    let agent = if slug.is_empty() {
        "agent/anima".to_owned()
    } else {
        format!("agent/anima-{slug}")
    };
    json!({
        "profile": {
            "schema": "central.agent-profile/v1",
            "ref": agent.replace("agent/", "profile/"),
            "agent_ref": agent,
            "purpose": format!("Fixture purpose of {agent}."),
            "governance_refs": [
                "central:source:control:root:Control/agents/expressions/anima/TEAM.md",
                format!("central:source:control:root:Control/agents/expressions/anima/members/{member_file}.md"),
            ],
            "skill_refs": ["skill/ql/gnosis-retrieve"],
        },
        "source_path": format!("Control/agents/profiles/{slug}.json"),
    })
}

fn world() -> World {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    let central = root.join("central");
    let members = central.join("Control/agents/expressions/anima/members");
    fs::create_dir_all(&members).unwrap();
    fs::create_dir_all(root.join("home")).unwrap();
    fs::create_dir_all(root.join("project")).unwrap();
    fs::write(members.join("nous.md"), member_expression("nous", "Nous")).unwrap();
    fs::write(
        members.join("logos.md"),
        member_expression("logos", "Logos"),
    )
    .unwrap();
    fs::write(
        root.join("sets.json"),
        json!({"ok": true, "data": {"records": [
            {"kind": "agent-set", "ref": "anima", "revision": "r1",
             "source_path": "Control/agents/agent-sets/anima.json",
             "record": {"schema": "central.agent-set/v1", "ref": "anima", "revision": "r1",
                        "orchestrator_agent_ref": "agent/anima",
                        "members": [{"kind": "agent", "agent_ref": "agent/anima"},
                                    {"kind": "agent", "agent_ref": "agent/anima-nous"},
                                    {"kind": "agent", "agent_ref": "agent/anima-logos"}]}},
            {"kind": "agent-set", "ref": "world-operators", "revision": "r3",
             "source_path": "Control/agents/agent-sets/ops.json",
             "record": {"schema": "central.agent-set/v1", "ref": "world-operators", "revision": "r3",
                        "members": [{"kind": "agent", "agent_ref": "agent/oh-i"}]}}
        ]}})
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("resolved.json"),
        json!({"ok": true, "data": {"ref": "anima", "revision": "r1",
            "authored_agents": ["agent/anima", "agent/anima-logos", "agent/anima-nous"],
            "resolved_agents": ["agent/anima", "agent/anima-logos", "agent/anima-nous"],
            "unavailable_agents": [], "nested_sets": [],
            "orchestrator_agent_ref": "agent/anima"}})
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("profiles.json"),
        json!({"ok": true, "data": {"scope": "root", "profiles": [
            profile("", "anima"), profile("nous", "nous"), profile("logos", "logos"),
        ]}})
        .to_string(),
    )
    .unwrap();
    let log = root.join("actuation.log");
    script(
        &root.join("bin/ctrl"),
        &format!(
            r#"case "$*" in
  *central.world.here*) echo '{{"ok":true,"data":{{"local_world":{{"ref":"control:root","root":"{central}"}},"project_world":{{"state":"absent"}},"workcells":[{{"ref":"workcell:mac","role":"current"}}]}}}}' ;;
  *central.position.read*) echo '{{"ok":true,"data":{{"record":{{"ref":"{POSITION}","revision":"r1","eligible_agent_refs":["agent/anima"]}}}}}}' ;;
  *central.agent-set.list*) cat "{root}/sets.json" ;;
  *central.agent-set.resolve*) cat "{root}/resolved.json" ;;
  *agent-profile.list*) cat "{root}/profiles.json" ;;
  *) echo '{{"ok":false,"error":{{"code":"invalid_input","message":"Unknown Action"}}}}'; exit 2 ;;
esac
"#,
            central = central.display(),
            root = root.display(),
        ),
    );
    script(
        &root.join("bin/actuation"),
        &format!(
            r#"echo "$*" >> "{log}"
case "$2" in
  claim) echo '{{"ok":true,"verb":"claim","position_ref":"{POSITION}","generation":1,"env":{{"OI_POSITION_REF":"{POSITION}","OI_OCCUPANT_GENERATION":"{CLAIMED}"}}}}' ;;
  release) echo '{{"ok":true,"verb":"release","position_ref":"{POSITION}","generation":1,"tenure":{{"generation_ref":"{CLAIMED}"}}}}' ;;
  verify) echo '{{"ok":true,"verb":"verify","position_ref":"{POSITION}","current":true}}' ;;
  *) echo "actuation: unknown command $2" >&2; exit 2 ;;
esac
"#,
            log = log.display()
        ),
    );
    // The Claude Code stand-in: prints the argv it was launched with.
    script(
        &root.join("bin/claude"),
        r#"for arg in "$@"; do echo "ARG=$arg"; done"#,
    );
    let agencies = root.join("home/state/encounter-agencies");
    fs::create_dir_all(&agencies).unwrap();
    fs::write(
        agencies.join("seed.json"),
        r#"{"active":true,"agent_ref":"agent/anima","agency_ref":"agency:anima-mint","world_ref":"control:root"}"#,
    )
    .unwrap();
    World {
        _dir: dir,
        root,
        central,
        log,
    }
}

impl World {
    fn run(&self, args: &[&str]) -> Output {
        std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
            .args(args)
            .current_dir(self.root.join("project"))
            .env("AIKIT_HOME", self.root.join("home"))
            .env("HOME", self.root.join("home"))
            .env("CENTRAL_CTRL_BIN", self.root.join("bin/ctrl"))
            .env("ACTUATION_BIN", self.root.join("bin/actuation"))
            .env_remove("OI_POSITION_REF")
            .env_remove("OI_OCCUPANT_GENERATION")
            .env_remove("CENTRAL_ROOT")
            .env_remove("AIKIT_CENTRAL_ROOT")
            .output()
            .unwrap()
    }

    fn claude(&self) -> String {
        self.root.join("bin/claude").display().to_string()
    }

    fn inhabitations(&self) -> PathBuf {
        self.root.join("home/state/inhabitations")
    }

    fn claims(&self) -> usize {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("occupancy claim"))
            .count()
    }
}

fn text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// The rendered frontmatter, key by key, in order. `description` is a
/// JSON-quoted (so YAML double-quoted) scalar; the rest are plain.
fn frontmatter(file: &Path) -> (Vec<(String, String)>, String) {
    let contents = fs::read_to_string(file).unwrap();
    let (yaml, body) = contents
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap();
    let fields = yaml
        .lines()
        .map(|line| {
            let (key, value) = line.split_once(": ").unwrap();
            let value = if value.starts_with('"') {
                serde_json::from_str::<String>(value).unwrap()
            } else {
                value.to_owned()
            };
            (key.to_owned(), value)
        })
        .collect();
    (fields, body.to_owned())
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> &'a str {
    fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .unwrap_or_else(|| panic!("no {key} in {fields:?}"))
}

#[test]
fn an_orchestrator_is_launched_with_its_team_as_claude_subagents_and_release_removes_them() {
    let world = world();
    let claude = world.claude();
    let output = world.run(&[
        "inhabit",
        "--position",
        POSITION,
        "--reason",
        "fixture",
        "--",
        &claude,
        "-p",
        "go",
    ]);
    let (stdout, stderr) = text(&output);
    assert!(output.status.success(), "{stdout}{stderr}");

    // The harness got the team's plugin directory, before its own arguments.
    let args: Vec<&str> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("ARG="))
        .collect();
    assert_eq!(args[0], "--plugin-dir", "{stdout}");
    assert_eq!(&args[2..], ["-p", "go"]);
    let plugin = PathBuf::from(args[1]);
    assert!(
        plugin.starts_with(world.inhabitations()),
        "{}",
        plugin.display()
    );
    assert!(stderr.contains("2 members of agent set anima"), "{stderr}");

    // One subagent per member other than the orchestrator.
    let mut agents: Vec<String> = fs::read_dir(plugin.join("agents"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    agents.sort();
    assert_eq!(agents, ["anima-logos.md", "anima-nous.md"]);
    let manifest: Value =
        serde_json::from_slice(&fs::read(plugin.join(".claude-plugin/plugin.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["name"], "anima-team");

    let (fields, body) = frontmatter(&plugin.join("agents/anima-nous.md"));
    assert_eq!(
        fields
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        ["name", "description", "tools", "skills"]
    );
    assert_eq!(field(&fields, "name"), "anima-nous");
    assert_eq!(
        field(&fields, "description"),
        "Nous, one member of the fixture team: invoked for nous work."
    );
    assert_eq!(field(&fields, "tools"), "Read, Grep, Glob");
    assert_eq!(
        field(&fields, "skills"),
        "vak-coordinate-frame, brainstorming, gnosis-retrieve"
    );
    assert!(body.contains("You are Nous."));
    assert!(body.contains("profile/anima-nous"));
    assert!(body.contains("Fixture purpose of agent/anima-nous."));

    // The receipt names every file, with its digest and provenance.
    let receipt: Value = serde_json::from_slice(
        &fs::read(
            plugin
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("receipt.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["schema"], "aikit.inhabitation-team-projection/v1");
    assert_eq!(receipt["generation_ref"], CLAIMED);
    assert_eq!(receipt["orchestrator_agent_ref"], "agent/anima");
    assert_eq!(receipt["files"].as_array().unwrap().len(), 3);
    assert!(receipt["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["expression_ref"]
            == "central:source:control:root:Control/agents/expressions/anima/members/logos.md"));

    // Leaving takes the team with it.
    let released = world.run(&[
        "inhabit",
        "--release",
        "--position",
        POSITION,
        "--generation",
        CLAIMED,
        "--json",
    ]);
    let (stdout, stderr) = text(&released);
    assert!(released.status.success(), "{stdout}{stderr}");
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        envelope["data"]["team_projection"]["removed"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(!plugin.exists());
    assert!(fs::read_dir(world.inhabitations())
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn a_member_without_an_expression_refuses_the_whole_team_before_anything_is_claimed() {
    let world = world();
    fs::remove_file(
        world
            .central
            .join("Control/agents/expressions/anima/members/logos.md"),
    )
    .unwrap();
    let claude = world.claude();
    let output = world.run(&[
        "inhabit",
        "--position",
        POSITION,
        "--reason",
        "fixture",
        "--json",
        "--",
        &claude,
    ]);
    let (stdout, stderr) = text(&output);
    assert!(!output.status.success(), "{stdout}{stderr}");
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap();
    let error = &envelope["error"];
    assert_eq!(error["code"], "inhabit.team_incomplete");
    let fact = error["details"]["fact"].as_str().unwrap();
    assert!(
        fact.contains("agent/anima-logos") && fact.contains("does not exist"),
        "{fact}"
    );
    assert!(
        !fact.contains("agent/anima-nous"),
        "only the missing member is named: {fact}"
    );
    assert!(error["details"]["consequence"]
        .as_str()
        .unwrap()
        .contains("partial team"));
    assert!(error["details"]["action"]
        .as_str()
        .unwrap()
        .contains("--no-team"));
    assert_eq!(world.claims(), 0, "nothing was claimed");
    assert!(!world.inhabitations().exists());

    // The owner can still launch the orchestrator alone, knowingly.
    let alone = world.run(&[
        "inhabit",
        "--position",
        POSITION,
        "--reason",
        "fixture",
        "--no-team",
        "--",
        &claude,
    ]);
    let (stdout, stderr) = text(&alone);
    assert!(alone.status.success(), "{stdout}{stderr}");
    assert!(!stdout.contains("--plugin-dir"));
    assert!(stderr.contains("--no-team"), "{stderr}");
    assert_eq!(world.claims(), 1);
}

#[test]
fn another_harness_is_launched_without_the_team_and_told_so() {
    let world = world();
    let output = world.run(&[
        "inhabit",
        "--position",
        POSITION,
        "--reason",
        "fixture",
        "--",
        "sh",
        "-c",
        "echo LAUNCHED $#",
    ]);
    let (stdout, stderr) = text(&output);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("LAUNCHED 0"));
    assert!(
        stderr.contains("supported for Claude Code only") && stderr.contains("`sh`"),
        "{stderr}"
    );
    assert!(!world.inhabitations().exists());
}

#[test]
fn without_a_harness_the_claim_reports_the_team_it_wrote_and_attach_hands_it_on() {
    let world = world();
    let output = world.run(&[
        "inhabit",
        "--position",
        POSITION,
        "--reason",
        "fixture",
        "--json",
    ]);
    let (stdout, stderr) = text(&output);
    assert!(output.status.success(), "{stdout}{stderr}");
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap();
    let projection = &envelope["data"]["team_projection"];
    let plugin = projection["plugins"][0]["plugin_dir"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(projection["plugins"][0]["members"], 2);
    assert!(Path::new(&plugin).join("agents/anima-logos.md").is_file());

    // A continued tenure launches Claude Code with the same team.
    let claude = world.claude();
    let attached = world.run(&[
        "inhabit",
        "--attach",
        "--position",
        POSITION,
        "--generation",
        CLAIMED,
        "--",
        &claude,
    ]);
    let (stdout, stderr) = text(&attached);
    assert!(attached.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains(&format!("ARG={plugin}")), "{stdout}");
}

/// A skill capsule in the fixture home's own registry, as AIKit catalogues
/// the QL skills: manifest plus a `payload/` Agent Skill with a script.
fn seed_skill(world: &World, id: &str, leaf: &str) {
    let base = world
        .root
        .join("home/registries/personal/capsules")
        .join(id);
    fs::create_dir_all(base.join("payload/scripts")).unwrap();
    fs::write(
        base.join("manifest.toml"),
        format!(
            "schema = 1\nid = \"{id}\"\nkind = \"skill\"\nname = \"{leaf}\"\ndescription = \"Fixture skill {leaf}.\"\n\n[skill]\nroot = \"payload\"\n"
        ),
    )
    .unwrap();
    fs::write(
        base.join("payload/SKILL.md"),
        format!("---\nname: {leaf}\ndescription: Fixture skill {leaf} for the team projection test.\n---\n\n# {leaf}\n\nRead the frame first.\n"),
    )
    .unwrap();
    fs::write(base.join("payload/scripts/frame.py"), "print('frame')\n").unwrap();
}

#[test]
fn the_team_carries_the_skills_aikit_catalogues_and_names_them_as_plugin_skills() {
    let world = world();
    seed_skill(
        &world,
        "skill/ql/vak-coordinate-frame",
        "vak-coordinate-frame",
    );
    let claude = world.claude();
    let output = world.run(&[
        "inhabit",
        "--position",
        POSITION,
        "--reason",
        "fixture",
        "--",
        &claude,
        "-p",
        "go",
    ]);
    let (stdout, stderr) = text(&output);
    assert!(output.status.success(), "{stdout}{stderr}");
    let args: Vec<&str> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("ARG="))
        .collect();
    let plugin = PathBuf::from(args[1]);
    let plugin_name = plugin.file_name().unwrap().to_string_lossy().into_owned();

    // The catalogued skill travels inside the plugin, whole.
    let skill = plugin.join("skills/vak-coordinate-frame");
    assert!(fs::read_to_string(skill.join("SKILL.md"))
        .unwrap()
        .contains("Read the frame first."));
    assert_eq!(
        fs::read_to_string(skill.join("scripts/frame.py")).unwrap(),
        "print('frame')\n"
    );
    assert!(
        stderr.contains("2 members of agent set anima with 1 bundled skills"),
        "{stderr}"
    );

    // Each member names it by the plugin-qualified name Claude Code gives a
    // plugin skill; skills the catalogue lacks keep their bare names.
    let (fields, _) = frontmatter(&plugin.join("agents/anima-nous.md"));
    assert_eq!(
        field(&fields, "skills"),
        format!("{plugin_name}:vak-coordinate-frame, brainstorming, gnosis-retrieve")
    );

    // The two it could not carry are disclosed, not dropped and not refused.
    assert!(
        stderr.contains("names skills that were not bundled")
            && stderr.contains("skill/personal/brainstorming")
            && stderr.contains("skill/ql/gnosis-retrieve"),
        "{stderr}"
    );

    // The receipt records what was bundled and what was not, and every
    // bundled file with its skill provenance.
    let receipt: Value = serde_json::from_slice(
        &fs::read(
            plugin
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("receipt.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let entry = &receipt["plugins"][0];
    assert_eq!(entry["members"], 2);
    assert_eq!(
        entry["skills_bundled"],
        json!(["skill/ql/vak-coordinate-frame"])
    );
    assert_eq!(entry["skills_missing"].as_array().unwrap().len(), 2);
    assert_eq!(
        receipt["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|file| file["skill_ref"] == "skill/ql/vak-coordinate-frame")
            .count(),
        2
    );
}
