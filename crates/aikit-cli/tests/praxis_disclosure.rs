//! Praxis forms, Agent praxis disclosure and the A2A card projection, driven
//! through the real binary against a temporary home.
//!
//! The fixture is a small Factory-style Agent carrying two SkillSets: a home
//! set (`core`) with Wayfinder (Methodology), grilling (Method) and research
//! (Skill), and a registry set (`demo:documentation`) with the Documentation
//! Methodology and a UI Method, which itself carries `demo:accounts` by
//! reference.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn skill(home: &Path, id: &str, description: &str) {
    let base = home.join("registries/personal/capsules").join(id);
    let name = id.rsplit('/').next().unwrap();
    write(
        &base.join("manifest.toml"),
        &format!(
            "schema = 1\nid = \"{id}\"\nkind = \"skill\"\nname = \"{name}\"\ndescription = \"{description}\"\n\n[skill]\nroot = \"payload\"\n"
        ),
    );
    write(
        &base.join("payload/SKILL.md"),
        &format!("---\nname: {name}\ndescription: \"{description}\"\n---\n\n# {name}\n"),
    );
}

fn fixture() -> (TempDir, TempDir) {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let h = home.path();
    skill(
        h,
        "skill/demo/wayfinder",
        "METHODOLOGY: chart the developmental field",
    );
    skill(
        h,
        "skill/demo/grilling",
        "METHOD: grill a decision one question at a time",
    );
    skill(
        h,
        "skill/demo/research",
        "Investigate against primary sources.",
    );
    skill(
        h,
        "skill/demo/docs-methodology",
        "METHODOLOGY: orient the documentation field",
    );
    skill(
        h,
        "skill/demo/ui-development",
        "METHOD: carry a UI change from Design to evidence",
    );
    skill(
        h,
        "skill/demo/html-account",
        "Author a self-contained HTML account.",
    );

    write(
        &h.join("skillsets/core/members"),
        "skill/demo/wayfinder\nskill/demo/grilling\nskill/demo/research\n",
    );
    write(
        &h.join("registries/personal/skillsets/index.toml"),
        "schema = 1\n\n[[skillset]]\nsemantic_ref = \"demo:documentation\"\ndirectory = \"documentation\"\ndescription = \"Documentation field\"\nchild_refs = [\"demo:accounts\"]\n\n[[skillset]]\nsemantic_ref = \"demo:accounts\"\ndirectory = \"accounts\"\n",
    );
    write(
        &h.join("registries/personal/skillsets/documentation/members"),
        "skill/demo/docs-methodology\nskill/demo/ui-development\n",
    );
    write(
        &h.join("registries/personal/skillsets/accounts/members"),
        "skill/demo/html-account\n",
    );
    write(&project.path().join(".aikit/profile.toml"), "schema = 1\n");
    (home, project)
}

fn run(home: &Path, project: &Path, args: &[&str]) -> Value {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = std::process::Command::new(&bin)
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .output()
        .unwrap_or_else(|e| panic!("aikit {args:?} should run: {e}"));
    assert!(
        output.status.success(),
        "aikit {args:?} failed: {} {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).expect("JSON envelope")
}

fn profile(project: &Path) -> String {
    let path = project.join("profile.json");
    let value = json!({
        "schema": "central.agent-profile/v1",
        "ref": "profile/factory-builder",
        "revision": "r2",
        "agent_ref": "agent/factory-builder",
        "name": "Factory builder",
        "scope": "project",
        "world_ref": "project:Factory",
        "purpose": "Develop Factory capabilities from authored intent",
        "skill_refs": [],
        "skill_set_refs": ["core", "demo:documentation"],
        "method_refs": [],
        "ratified_world_refs": ["project:Factory", "project:O-I"],
        "intent_provenance": {
            "schema": "central.agent-profile-provenance/v1",
            "intent_expression": "Build what the owner intends.",
            "origin_action": "agent-profile.express",
            "authorship": "generated-proposal",
            "recognition": "unrecognised"
        }
    });
    // Central returns the profile inside its action envelope; accept that.
    fs::write(
        &path,
        serde_json::to_string(&json!({"ok": true, "data": {"profile": value}})).unwrap(),
    )
    .unwrap();
    format!("@{}", path.display())
}

#[test]
fn praxis_list_classifies_three_forms_and_method_list_is_unchanged() {
    let (home, project) = fixture();
    let all = run(home.path(), project.path(), &["praxis", "list"]);
    assert_eq!(all["data"]["counts"]["methodology"], 2);
    assert_eq!(all["data"]["counts"]["method"], 2);
    assert_eq!(all["data"]["counts"]["skill"], 2);

    let orient = run(
        home.path(),
        project.path(),
        &["praxis", "list", "--form", "methodology"],
    );
    assert_eq!(orient["data"]["count"], 2);
    assert!(orient["data"]["praxis"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["position"] == 3));

    // `aikit method list` keeps its exact contract: Methodologies are not Methods.
    let methods = run(home.path(), project.path(), &["method", "list"]);
    assert_eq!(methods["data"]["count"], 2);
}

#[test]
fn disclosure_reads_profile_to_nested_sets_to_classified_praxis() {
    let (home, project) = fixture();
    let profile = profile(project.path());
    let reading = run(
        home.path(),
        project.path(),
        &["praxis", "disclose", "--profile-json", &profile],
    );
    let data = &reading["data"];
    assert_eq!(data["schema"], "aikit.agent-praxis-disclosure/v1");
    assert_eq!(data["agent"]["agent_ref"], "agent/factory-builder");
    let effective: Vec<&str> = data["repertoire"]["effective_skill_sets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(effective.contains(&"demo:accounts"), "{effective:?}");
    assert_eq!(data["repertoire"]["unresolved"], json!([]));
    let praxis = data["praxis"].as_array().unwrap();
    assert_eq!(praxis.len(), 6);
    let orientations: Vec<&str> = praxis
        .iter()
        .filter(|entry| entry["form"] == "methodology")
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    assert_eq!(orientations, vec!["docs-methodology", "wayfinder"]);
    for entry in praxis {
        // Carried and catalogued; nothing claimed loaded or invoked without evidence.
        assert_eq!(entry["involvement"]["carried"], true);
        assert_eq!(entry["involvement"]["catalogued"], true);
        assert_eq!(entry["involvement"]["loaded"], Value::Null);
        assert_eq!(entry["involvement"]["invoked"], Value::Null);
        assert!(entry["revision"].is_string(), "exact revision disclosed");
    }
    assert_eq!(data["return"]["source_rewritten"], false);
    assert!(data["answers"]["why_am_i_here"]
        .as_str()
        .unwrap()
        .contains("unrecognised"));
}

#[test]
fn activity_evidence_separates_selected_loaded_and_invoked() {
    let (home, project) = fixture();
    let profile = profile(project.path());
    let activity = project.path().join("activity.json");
    fs::write(
        &activity,
        json!({
            "schema": "aikit.praxis-activity/v1",
            "loaded": ["skill/demo/ui-development", "skill/demo/docs-methodology"],
            "invoked": ["skill/demo/ui-development"],
            "evidence_refs": ["factory:evidence:run-1:walk"],
            "return_destinations": ["central:documentation-field:design"],
            "activity_refs": ["factory:run:run-1"]
        })
        .to_string(),
    )
    .unwrap();
    let activity_arg = format!("@{}", activity.display());
    let reading = run(
        home.path(),
        project.path(),
        &[
            "praxis",
            "disclose",
            "--profile-json",
            &profile,
            "--activity-json",
            &activity_arg,
            "--select",
            "skill/demo/ui-development",
        ],
    );
    let praxis = reading["data"]["praxis"].as_array().unwrap();
    let find = |id: &str| praxis.iter().find(|entry| entry["id"] == id).unwrap();
    let ui = find("skill/demo/ui-development");
    assert_eq!(ui["involvement"]["selected"], true);
    assert_eq!(ui["involvement"]["loaded"], true);
    assert_eq!(ui["involvement"]["invoked"], true);
    let methodology = find("skill/demo/docs-methodology");
    assert_eq!(methodology["involvement"]["loaded"], true);
    assert_eq!(methodology["involvement"]["invoked"], false);
    // A small repair would not have loaded Wayfinder; here it is observed unloaded.
    assert_eq!(find("skill/demo/wayfinder")["involvement"]["loaded"], false);
    assert_eq!(
        reading["data"]["return"]["destinations"],
        json!(["central:documentation-field:design"])
    );
}

#[test]
fn a2a_card_publishes_only_public_capabilities() {
    let (home, project) = fixture();
    let participation = project.path().join("participation.json");
    fs::write(
        &participation,
        json!({
            "schema": "oi.agent-world-participation/v1",
            "participation_ref": "oi:participation:agent/factory-builder@project:Factory",
            "agent_ref": "agent/factory-builder",
            "world_ref": "project:Factory",
            "profile": {"ref": "profile/factory-builder", "revision": "r2", "name": "Factory builder"},
            "expression": {"name": "Factory builder", "purpose": "Develop Factory capabilities from authored intent"},
            "repertoire": {"skill_sets": ["core", "demo:documentation"]},
            "public_capabilities": [{"id": "ui-development", "name": "UI development",
                "description": "Carry a UI change from Design to evidence.", "tags": ["ui"]}],
            "disclosure": {"public_basis": "profile-declared", "extended_card_served": false}
        })
        .to_string(),
    )
    .unwrap();
    let out = project.path().join("site/.well-known/agent-card.json");
    let arg = format!("@{}", participation.display());
    let reply = run(
        home.path(),
        project.path(),
        &[
            "a2a",
            "card",
            "--participation-json",
            &arg,
            "--interface-url",
            "https://agents.example/factory-builder/a2a",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(reply["data"]["public_skill_count"], 1);
    let card: Value = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(card["supportedInterfaces"][0]["protocolVersion"], "1.0");
    let text = card.to_string();
    assert!(
        !text.contains("skill/demo/grilling"),
        "internal repertoire leaked"
    );
    assert!(!text.contains("demo:documentation"), "internal set leaked");
}
