//! Native Central owner operations and its real canonical filesystem contract.
use aikit_adapters::{central_wiki::read_central_wiki, runner::SystemRunner};
use aikit_core::SemanticWikiIndex;
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
fn action(ctrl: &Path, root: &Path, name: &str, input: Value) -> Value {
    let output = Command::new(ctrl)
        .args(["--json", "--root"])
        .arg(root)
        .args(["action", "run", name, &input.to_string()])
        .output()
        .unwrap();
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(output.status.success() && result["ok"] == true, "{result}");
    result["data"].clone()
}
#[test]
#[ignore = "requires AIKIT_CENTRAL_REAL_BIN pointing at the actual Central owner; explicit native integration"]
fn declared_wikis_ignore_copied_fixture_graphs_and_preserve_owner_relations() {
    let executable = std::env::var("AIKIT_CENTRAL_REAL_BIN").unwrap();
    let ctrl = Path::new(&executable);
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    action(ctrl, root, "central.init", json!({}));
    for (name, id) in [("Alpha", "native-alpha"), ("Beta", "native-beta")] {
        fs::create_dir(root.join("Work").join(name)).unwrap();
        action(
            ctrl,
            root,
            "projectcentral.init",
            json!({"project":name,"project_id":id}),
        );
    }
    let original = fs::read(root.join("Work/Alpha/ProjectCentral/agents/wiki/wiki.json")).unwrap();
    let fixtures = root.join("Work/Alpha/test-fixtures");
    fs::create_dir(&fixtures).unwrap();
    for i in 0..4100 {
        fs::write(fixtures.join(format!("copied-{i}.json")), &original).unwrap();
    }
    let reading = read_central_wiki(&SystemRunner::new(), ctrl, root).unwrap();
    assert!(reading.absences.is_empty(), "{:?}", reading.absences);
    let encoded = reading
        .objects
        .iter()
        .map(|object| object.ref_id().as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(encoded.contains("native-alpha") && encoded.contains("native-beta"));
    let all_refs = reading
        .objects
        .iter()
        .map(|o| o.ref_id().as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    SemanticWikiIndex::rebuild(reading.objects)
        .expect("canonical graph survives more than4096 duplicate repository fixtures");
    assert_eq!(
        fs::read(root.join("Work/Alpha/ProjectCentral/agents/wiki/wiki.json")).unwrap(),
        original,
        "native discovery never rewrites authored wiki"
    );
    let root_refs = aikit_core::parse_wiki_objects(
        &fs::read_to_string(root.join("Control/agents/wiki/wiki.json")).unwrap(),
    )
    .unwrap()
    .into_iter()
    .map(|o| o.ref_id().as_str().to_owned())
    .collect::<std::collections::BTreeSet<_>>();
    fs::remove_file(root.join("Control/agents/wiki/wiki.json")).unwrap();
    let without_root_wiki = read_central_wiki(&SystemRunner::new(), ctrl, root).unwrap();
    assert_eq!(without_root_wiki.objects.iter().map(|o|o.ref_id().as_str().to_owned()).collect::<std::collections::BTreeSet<_>>(),
        all_refs.difference(&root_refs).cloned().collect(),
        "An absent root wiki removes exactly that declaration; copied fixtures do not substitute for it");
    fs::write(
        root.join("Work/Beta/ProjectCentral/agents/wiki/wiki.json"),
        "not json",
    )
    .unwrap();
    let failed = read_central_wiki(&SystemRunner::new(), ctrl, root).unwrap();
    // Central itself may mark the malformed declaration absent with an error;
    // either way it must never substitute a fixture for that canonical source.
    assert!(failed
        .objects
        .iter()
        .all(|object| !object.ref_id().as_str().contains("native-beta")));
    assert!(
        !failed.absences.is_empty(),
        "The unavailable declared wiki must be disclosed"
    );
}
