//! Wiki reader/constructive-owner boundary. Actual CLI and files; no model,
//! installed-world claims, Markdown re-parser, or renderer-owned knowledge.
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::TempDir;

fn run(root: &Path, args: &[&str], input: Option<Value>) -> (bool, Value) {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .current_dir(root)
        .env("AIKIT_HOME", root.join("isolated-home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("native CLI starts");
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    let output = child.wait_with_output().unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{args:?}: {error}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), value)
}
fn apply(root: &Path, revision: u64, operation: &str, changes: Value) -> (bool, Value) {
    let basis = fs::read_to_string(root.join("wiki.json")).unwrap();
    run(
        root,
        &["wiki-construct", "apply", "--file", "wiki.json"],
        Some(json!({
            "basis_content": basis,
            "request": {"schema":"aikit.constellation-action/v1", "frame_ref":"wiki:frame:inquiry", "expected_revision":revision,
                "actor_ref":"human:source-author", "operation_ref":operation, "changes":changes}
        })),
    )
}
fn provenance(start: usize, end: usize, quote: &str) -> Value {
    json!({"source_ref":"source:ordinary-notes", "source_revision":"source-r1",
        "aikit.techne-facet/v1":{"contract":"aikit.techne-facet/v1", "selector":{"unit":"other", "kind":"markdown-utf8-span",
            "value":json!({"start_byte":start, "end_byte":end, "quote":quote}).to_string()}}})
}

#[test]
fn two_passages_from_one_source_keep_distinct_participation_and_return_without_source_mutation() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let text = "# Notes\n\n🌱 An initial distinction.\n\nA different interpretation.\n";
    fs::write(root.join("notes.md"), text).unwrap();
    fs::write(root.join("wiki.json"), json!({"profile":"okf-wiki/v1", "objects":[
        {"object":"space", "profile":"okf-wiki/v1", "ref":"wiki:space:ordinary", "revision":1, "node_refs":[]}
    ]}).to_string()).unwrap();
    let first = text.find('🌱').unwrap();
    let second = text.find("A different").unwrap();
    let sources = [
        provenance(first, second - 2, "An initial distinction."),
        provenance(second, text.len() - 1, "A different interpretation."),
    ];
    let mut changes = vec![
        json!({"change":"create", "anchor_ref":"wiki:anchor:inquiry", "title":"Source inquiry",
        "inquiry":{"question":"How do these passages qualify each other?"}, "space_refs":["wiki:space:ordinary"]}),
    ];
    for (index, source) in sources.iter().enumerate() {
        changes.push(json!({"change":"member_add", "member":{"subject_ref":"source:ordinary-notes", "participation":{
            "participation_ref":format!("participation:passage-{index}"), "sources":[source], "note":format!("Passage {index}")}}}));
    }
    changes.push(json!({"change":"relation_put", "relation":{"relation_ref":"wiki:relation:qualifies", "expected_revision":null,
        "from_participation_ref":"participation:passage-0", "to_participation_ref":"participation:passage-1",
        "relation":"qualifies", "direction":"directed", "standing":"proposed", "evidence":sources}}));
    let (ok, saved) = apply(root, 0, "operation:source-inquiry", json!(changes));
    assert!(ok, "{saved}");
    assert_eq!(saved["data"]["persisted"], true);
    assert_eq!(fs::read_to_string(root.join("notes.md")).unwrap(), text);
    let reading = &saved["data"]["reading"];
    let members = reading["frame"]["constellations"][0]["members"]
        .as_array()
        .unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0]["ref"], members[1]["ref"]);
    assert_ne!(
        members[0]["aikit.constellation-participation/v1"]["participation_ref"],
        members[1]["aikit.constellation-participation/v1"]["participation_ref"]
    );
    let selector = &members[0]["aikit.constellation-participation/v1"]["sources"][0]
        ["aikit.techne-facet/v1"]["selector"];
    assert_eq!(selector["unit"], "other");
    assert_eq!(selector["kind"], "markdown-utf8-span");
    let span: Value = serde_json::from_str(selector["value"].as_str().unwrap()).unwrap();
    assert_eq!(span["start_byte"], first);
    assert_eq!(reading["relations"][0]["relation"], "qualifies");
    assert_eq!(
        reading["relations"][0]["aikit.constellation-relation/v1"]["standing"],
        "proposed"
    );
    let (ok, returned) = apply(
        root,
        1,
        "operation:return-expression",
        json!([{"change":"composition_attach", "composition":{
        "reference":"expression:source-inquiry", "revision":"3", "kind":"expression", "source":sources[0], "derivation_refs":["source:ordinary-notes"]}}]),
    );
    assert!(ok, "{returned}");
    let (ok, reopened) = run(
        root,
        &[
            "wiki-construct",
            "inspect",
            "wiki:frame:inquiry",
            "--file",
            "wiki.json",
        ],
        None,
    );
    assert!(ok, "{reopened}");
    assert_eq!(reopened["data"]["reading"], returned["data"]["reading"]);
    assert_eq!(
        reopened["data"]["reading"]["construction"]["compositions"][0]["reference"],
        "expression:source-inquiry"
    );
    assert_eq!(
        reopened["data"]["reading"]["frame"]["constellations"][0]["returns"][0]
            ["through_anchor_ref"],
        "wiki:anchor:inquiry"
    );
    assert_eq!(fs::read_to_string(root.join("notes.md")).unwrap(), text);
    let before = fs::read(root.join("wiki.json")).unwrap();
    let (ok, refused) = apply(
        root,
        1,
        "operation:late-source-edit",
        json!([{"change":"inquiry_set", "title":"Obsolete", "inquiry":{"question":"Old state"}}]),
    );
    assert!(!ok, "{refused}");
    assert_eq!(fs::read(root.join("wiki.json")).unwrap(), before);
}
