//! `aikit inhabit` against fake owners: the claim reaches Actuation with the
//! resolved Agent/Agency and expectation, the harness is exec'd with the
//! occupancy stamped into its environment, nothing is released on exit, and
//! `--release` ends exactly the generation the body holds.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

use tempfile::TempDir;

const POSITION: &str = "central:position:project:O-I:aikit-guardian";
const HELD: &str = "actuation:generation:00000000-0000-4000-8000-000000000000";
const CLAIMED: &str = "actuation:generation:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct World {
    _dir: TempDir,
    root: PathBuf,
    log: PathBuf,
}

fn world() -> World {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    let log = root.join("actuation.log");
    fs::create_dir_all(root.join("home")).unwrap();
    fs::create_dir_all(root.join("project")).unwrap();
    script(
        &root.join("ctrl"),
        &format!(
            r#"case "$*" in
  *central.world.here*) echo '{{"ok":true,"data":{{"local_world":{{"ref":"control:root","root":"/w"}},"project_world":{{"state":"present","ref":"project:O-I","name":"O-I","path":"Work/O-I"}},"workcells":[{{"ref":"workcell:local","role":"current"}}]}}}}' ;;
  *central.position.list*) echo '{{"ok":true,"data":{{"world_ref":"project:O-I","positions":[{{"record":{{"ref":"{POSITION}","handle":"@aikit-guardian"}},"source":{{"revision":"r1"}}}}],"inherited":[],"invalid":[]}}}}' ;;
  *central.position.read*)
    if [ -n "$FAKE_MANY_AGENTS" ]; then agents='"agent/a","agent/b"'; else agents='"agent/aikit-guardian"'; fi
    echo "{{\"ok\":true,\"data\":{{\"record\":{{\"ref\":\"{POSITION}\",\"revision\":\"r1\",\"eligible_agent_refs\":[$agents]}}}}}}" ;;
  *) echo '{{"ok":false,"error":{{"code":"invalid_input","message":"Unknown Action"}}}}'; exit 2 ;;
esac
"#
        ),
    );
    script(
        &root.join("actuation"),
        &format!(
            r#"echo "$*" >> "{log}"
case "$2" in
  claim) echo '{{"ok":true,"verb":"claim","position_ref":"{POSITION}","generation":4,"env":{{"OI_POSITION_REF":"{POSITION}","OI_OCCUPANT_GENERATION":"{CLAIMED}"}}}}' ;;
  read) echo '{{"schema":"actuation.position-occupancy/v1","position_ref":"{POSITION}","state":"occupied","current":{{"generation_ref":"{HELD}","generation_ordinal":3}},"generations":[]}}' ;;
  release) echo '{{"ok":true,"verb":"release","position_ref":"{POSITION}","generation":3,"tenure":{{"generation_ref":"{HELD}"}}}}' ;;
  verify)
    case "$*" in
      *"{HELD}"*) echo '{{"ok":true,"verb":"verify","position_ref":"{POSITION}","current":true}}' ;;
      *) echo '{{"ok":false,"error":{{"code":"occupancy.superseded","fact":"superseded","consequence":"nothing","action":"actuation occupancy read --position {POSITION}"}}}}'; exit 2 ;;
    esac ;;
  *) echo "actuation: unknown command $2" >&2; exit 2 ;;
esac
"#,
            log = log.display()
        ),
    );
    World {
        _dir: dir,
        root,
        log,
    }
}

impl World {
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"));
        command
            .args(args)
            .current_dir(self.root.join("project"))
            .env("AIKIT_HOME", self.root.join("home"))
            .env("HOME", self.root.join("home"))
            .env("CENTRAL_CTRL_BIN", self.root.join("ctrl"))
            .env("ACTUATION_BIN", self.root.join("actuation"))
            .env("KEEP_ME", "session-env-survives")
            .env_remove("OI_POSITION_REF")
            .env_remove("OI_OCCUPANT_GENERATION");
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    fn actuation_calls(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn seed_agency(&self, agent: &str, agency: &str) {
        let dir = self.root.join("home/state/encounter-agencies");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("seed.json"),
            format!(r#"{{"active":true,"agent_ref":"{agent}","agency_ref":"{agency}","world_ref":"O-I"}}"#),
        )
        .unwrap();
    }
}

const HARNESS: &[&str] = &[
    "--",
    "sh",
    "-c",
    "echo POS=$OI_POSITION_REF; echo GEN=$OI_OCCUPANT_GENERATION; echo KEEP=$KEEP_ME",
];

fn args<'a>(head: &[&'a str]) -> Vec<&'a str> {
    head.iter()
        .copied()
        .chain(HARNESS.iter().copied())
        .collect()
}

#[test]
fn inhabit_claims_then_execs_the_harness_with_the_occupancy_stamped() {
    let world = world();
    world.seed_agency("agent/aikit-guardian", "agency:aikit-mint-o-i");
    let output = world.run(
        &args(&[
            "inhabit",
            "--position",
            "@aikit-guardian",
            "--reason",
            "fixture",
        ]),
        &[],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(&format!("POS={POSITION}")), "{stdout}");
    assert!(stdout.contains(&format!("GEN={CLAIMED}")), "{stdout}");
    assert!(
        stdout.contains("KEEP=session-env-survives"),
        "the existing env rides along"
    );
    let calls = world.actuation_calls();
    assert_eq!(
        calls.len(),
        1,
        "one claim, and no release when the harness exits: {calls:?}"
    );
    let claim = &calls[0];
    assert!(
        claim.starts_with("occupancy claim --position central:position:project:O-I:aikit-guardian")
    );
    assert!(
        claim.contains("--agent agent/aikit-guardian"),
        "defaulted from the single eligible Agent"
    );
    assert!(
        claim.contains("--agency agency:aikit-mint-o-i"),
        "defaulted from the admitted agency"
    );
    assert!(claim.contains("--workcell workcell:local"));
    assert!(claim.contains("--expect-vacant"));
    assert!(claim.ends_with("--json"));
}

#[test]
fn handover_expects_the_current_generation() {
    let world = world();
    world.seed_agency("agent/aikit-guardian", "agency:x");
    let output = world.run(
        &args(&[
            "inhabit",
            "--position",
            POSITION,
            "--agency",
            "agency:x",
            "--handover",
            "--reason",
            "take over",
        ]),
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = world.actuation_calls();
    assert!(calls[0].starts_with("occupancy read --position"));
    assert!(
        calls[1].contains(&format!("--expect-generation {HELD}")),
        "{calls:?}"
    );
    assert!(!calls[1].contains("--expect-vacant"));
}

#[test]
fn supplied_agent_or_agency_cannot_bypass_eligibility_or_admission() {
    let world = world();
    world.seed_agency("agent/aikit-guardian", "agency:aikit-mint-o-i");
    let output = world.run(
        &args(&[
            "inhabit",
            "--position",
            POSITION,
            "--agent",
            "agent/someone-else",
            "--agency",
            "agency:aikit-mint-o-i",
            "--reason",
            "bypass",
        ]),
        &[],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not name agent/someone-else as an eligible Agent"),
        "{stderr}"
    );
    assert!(stderr.contains("Nothing was claimed."), "{stderr}");
    assert!(
        world.actuation_calls().is_empty(),
        "ineligible agent claimed nothing"
    );

    let output = world.run(
        &args(&[
            "inhabit",
            "--position",
            POSITION,
            "--agent",
            "agent/aikit-guardian",
            "--agency",
            "agency:not-admitted",
            "--reason",
            "bypass",
        ]),
        &[],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is not an admitted Agency"), "{stderr}");
    assert!(
        world.actuation_calls().is_empty(),
        "unadmitted agency claimed nothing"
    );
}

#[test]
fn an_unresolvable_agent_or_agency_is_a_three_part_refusal_and_claims_nothing() {
    let world = world();
    let output = world.run(
        &args(&[
            "inhabit",
            "--position",
            POSITION,
            "--agency",
            "agency:x",
            "--reason",
            "r",
        ]),
        &[("FAKE_MANY_AGENTS", "1")],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("names 2 eligible Agents"), "{stderr}");
    assert!(stderr.contains("Nothing was claimed."));
    assert!(stderr.contains("--agent <one of them>"));

    let output = world.run(
        &args(&["inhabit", "--position", POSITION, "--reason", "r"]),
        &[],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("AIKit holds no admitted Agency for agent/aikit-guardian"),
        "{stderr}"
    );
    assert!(world.actuation_calls().is_empty(), "no claim was attempted");
}

#[test]
fn release_ends_exactly_the_generation_this_body_holds() {
    let world = world();
    let output = world.run(
        &["inhabit", "--release", "--position", POSITION],
        &[
            ("OI_POSITION_REF", POSITION),
            ("OI_OCCUPANT_GENERATION", HELD),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = world.actuation_calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].starts_with(&format!(
        "occupancy release --position {POSITION} --generation {HELD} --reason"
    )));

    // A body without a stamped generation holds nothing to release.
    let output = world.run(&["inhabit", "--release", "--position", POSITION], &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Nothing was released."));
    assert_eq!(world.actuation_calls().len(), 1);
}

#[test]
fn without_a_harness_the_claim_prints_the_exports() {
    let world = world();
    world.seed_agency("agent/aikit-guardian", "agency:x");
    let output = world.run(
        &[
            "inhabit",
            "--position",
            POSITION,
            "--agency",
            "agency:x",
            "--reason",
            "r",
        ],
        &[],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains(&format!("export OI_OCCUPANT_GENERATION={CLAIMED}")),
        "{stdout}"
    );
}

#[test]
fn attach_continues_only_the_current_generation_and_claims_nothing() {
    let world = world();
    let output = world.run(
        &args(&["inhabit", "--attach", "--position", POSITION]),
        &[
            ("OI_POSITION_REF", POSITION),
            ("OI_OCCUPANT_GENERATION", HELD),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("POS={POSITION}")), "{stdout}");
    assert!(stdout.contains(&format!("GEN={HELD}")), "{stdout}");
    assert!(stdout.contains("KEEP=session-env-survives"), "{stdout}");
    let calls = world.actuation_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].starts_with(&format!(
        "occupancy verify --position {POSITION} --generation {HELD}"
    )));

    // A superseded generation is refused and nothing is launched.
    let output = world.run(
        &args(&[
            "inhabit",
            "--attach",
            "--position",
            POSITION,
            "--generation",
            CLAIMED,
        ]),
        &[],
    );
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("POS="));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Nothing was launched"));
    assert!(world
        .actuation_calls()
        .iter()
        .all(|call| !call.contains(" claim ")));
}
