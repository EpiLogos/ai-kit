//! `aikit source list` through the real binary: every registered source's
//! state, each exactly as `source show` prints it, so a caller can name the
//! sources it may sync, promote or roll back without knowing an id first.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::cargo_bin;
use serde_json::{json, Value};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .current_dir(cwd)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("invalid JSON ({error}): {stdout:?}"));
    (output, value)
}

#[test]
fn every_registered_source_is_listed_as_show_reads_it() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");

    let (listed, reply) = run(&home, temp.path(), &["source", "list"]);
    assert!(listed.status.success(), "{reply}");
    assert_eq!(
        reply["data"]["sources"],
        json!([]),
        "nothing registered yet"
    );

    for (id, skill) in [("zeta-pack", "zeta"), ("alpha-pack", "alpha")] {
        let pack = temp.path().join(id);
        write(
            &pack.join(format!("{skill}/SKILL.md")),
            &format!("---\nname: {skill}\ndescription: A listed skill.\n---\n\n{skill}\n"),
        );
        let (added, reply) = run(
            &home,
            temp.path(),
            &["source", "add-directory", id, pack.to_str().unwrap()],
        );
        assert!(added.status.success(), "{reply}");
    }
    let (synced, reply) = run(&home, temp.path(), &["source", "sync", "alpha-pack"]);
    assert!(synced.status.success(), "{reply}");

    let (listed, reply) = run(&home, temp.path(), &["source", "list"]);
    assert!(listed.status.success(), "{reply}");
    let sources = reply["data"]["sources"].as_array().unwrap();
    let ids: Vec<&str> = sources.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["alpha-pack", "zeta-pack"], "in id order");
    assert_eq!(reply["data"]["unreadable"], json!([]));
    // Each entry is exactly `source show`'s reading.
    for id in ids {
        let (shown, show) = run(&home, temp.path(), &["source", "show", id]);
        assert!(shown.status.success());
        let listed = sources.iter().find(|s| s["id"] == id).unwrap();
        assert_eq!(listed, &show["data"], "{id}");
    }
    assert!(
        sources[0]["candidate_snapshot"].is_string(),
        "alpha was synced"
    );
    assert!(sources[1]["candidate_snapshot"].is_null(), "zeta was not");

    // A registration that cannot be read is named with AIKit's words, not dropped.
    fs::create_dir_all(home.join("sources").join("broken")).unwrap();
    let (listed, reply) = run(&home, temp.path(), &["source", "list"]);
    assert!(listed.status.success(), "{reply}");
    assert_eq!(reply["data"]["sources"].as_array().unwrap().len(), 2);
    let unreadable = &reply["data"]["unreadable"][0];
    assert_eq!(unreadable["id"], "broken");
    assert!(
        unreadable["code"].as_str().unwrap().starts_with("source."),
        "{unreadable}"
    );
}
