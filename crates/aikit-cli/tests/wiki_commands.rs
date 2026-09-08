//! `aikit wiki` end to end.
//!
//! The acceptance criteria are the framing law itself: a command names the file
//! it writes, the whole validates before anything is persisted, a refusal
//! leaves the file byte-identical, a dry run writes nothing, and the root
//! commands see the Central layout without ever inventing state.

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// Run `aikit wiki <args>` with no AIKit home at all: these commands name the
/// file they touch, and must not depend on a resolved context to run.
fn wiki(cwd: &Path, args: &[&str]) -> (i32, Value) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let output = Command::new(&bin)
        .args(args)
        .arg("--json")
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("aikit {args:?} should run: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} must emit a JSON envelope; got stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(-1), envelope)
}

/// The same, for `--stdin` commands.
fn wiki_stdin(cwd: &Path, args: &[&str], body: &str) -> (i32, Value) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let mut child = Command::new(&bin)
        .args(args)
        .arg("--json")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("aikit {args:?} should run: {e}"));
    child
        .stdin
        .as_mut()
        .expect("piped")
        .write_all(body.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} must emit a JSON envelope; got stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(-1), envelope)
}

#[allow(clippy::too_many_arguments)]
fn space(
    ref_id: &str,
    revision: u64,
    parents: &[&str],
    children: &[&str],
    nodes: &[&str],
) -> String {
    format!(
        r#"{{
  "child_space_refs": {children:?},
  "node_refs": {nodes:?},
  "object": "space",
  "parent_space_refs": {parents:?},
  "profile": "okf-wiki/v1",
  "provenance": [],
  "ref": "{ref_id}",
  "revision": {revision},
  "title": "{ref_id}"
}}"#
    )
}

/// A project Space under the Central root: the parent lives in a peer file,
/// which is the federated norm, not an error.
fn project_space(ref_id: &str, revision: u64, parent: &str) -> String {
    format!(
        r#"{{
  "child_space_refs": [],
  "node_refs": [],
  "object": "space",
  "parent_space_refs": ["{parent}"],
  "profile": "okf-wiki/v1",
  "provenance": [],
  "ref": "{ref_id}",
  "revision": {revision},
  "title": "{ref_id}"
}}"#
    )
}

fn node(ref_id: &str, revision: u64, spaces: &[&str]) -> String {
    format!(
        r#"{{
  "object": "node",
  "profile": "okf-wiki/v1",
  "provenance": [],
  "ref": "{ref_id}",
  "revision": {revision},
  "space_refs": {spaces:?},
  "title": "{ref_id}",
  "type": "Note"
}}"#
    )
}

/// Two federated Spaces and one node: the smallest Wiki that exercises every
/// write without touching a federation boundary.
fn document() -> String {
    format!(
        "{{\n  \"objects\": [\n    {},\n    {},\n    {}\n  ]\n}}\n",
        space("wiki:space:root", 2, &[], &["wiki:space:child"], &[]),
        space(
            "wiki:space:child",
            2,
            &["wiki:space:root"],
            &[],
            &["wiki:node:a"],
        ),
        node("wiki:node:a", 1, &["wiki:space:root", "wiki:space:child"]),
    )
}

fn root_document(children: &[&str]) -> String {
    format!(
        "{{\n  \"objects\": [\n    {}\n  ]\n}}\n",
        space("central:wiki:root", 2, &[], children, &[]),
    )
}

fn fixture() -> (TempDir, TempDir) {
    let work = TempDir::new().unwrap();
    let scratch = TempDir::new().unwrap();
    write(&work.path().join("wiki.json"), &document());
    (work, scratch)
}

#[test]
fn a_healthy_document_validates_and_a_broken_one_fails_with_its_findings() {
    let (work, scratch) = fixture();
    let wiki_json = work.path().join("wiki.json");

    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "validate", wiki_json.to_str().unwrap()],
    );
    assert_eq!(code, 0);
    assert_eq!(envelope["ok"], Value::Bool(true));
    assert_eq!(envelope["data"]["valid"], Value::Bool(true));
    assert_eq!(envelope["data"]["objects"], Value::from(3));
    assert_eq!(envelope["data"]["errors"], Value::Array(Vec::new()));

    // The federated norm: a Space pointing at its peer file is a finding,
    // published, and still a valid document. The parent is simply not in this
    // file, exactly as ctrl writes a project Wiki.
    write(
        &work.path().join("federated.json"),
        &format!(
            "{{\n  \"objects\": [\n    {}\n  ]\n}}\n",
            project_space("wiki:space:child", 1, "central:wiki:root"),
        ),
    );
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "validate",
            work.path().join("federated.json").to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "a dangling peer ref is the norm: {envelope}");
    assert_eq!(envelope["data"]["dangling"].as_array().unwrap().len(), 1);
    assert_eq!(
        envelope["data"]["dangling"][0]["field"],
        "parent_space_refs"
    );

    // The hard case the index exists for: an in-file link the other side does
    // not return.
    write(
        &work.path().join("asymmetric.json"),
        &format!(
            "{{\n  \"objects\": [\n    {},\n    {}\n  ]\n}}\n",
            space("wiki:space:root", 1, &[], &["wiki:space:child"], &[]),
            space("wiki:space:child", 1, &[], &[], &[]),
        ),
    );
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "validate",
            work.path().join("asymmetric.json").to_str().unwrap(),
        ],
    );
    assert_eq!(code, 1, "a broken whole reports findings and fails");
    assert_eq!(
        envelope["ok"],
        Value::Bool(true),
        "the findings still print in a success envelope"
    );
    assert_eq!(envelope["data"]["valid"], Value::Bool(false));
    assert_eq!(
        envelope["data"]["errors"][0]["code"], "knowledge.wiki_space_asymmetry",
        "{envelope}"
    );
}

#[test]
fn a_node_written_through_the_cli_reparses_and_reindexes_identically() {
    let (work, scratch) = fixture();
    let wiki_json = work.path().join("wiki.json");

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "node",
            "create",
            "wiki:node:b",
            "--file",
            wiki_json.to_str().unwrap(),
            "--space",
            "wiki:space:child",
            "--type",
            "Claim",
            "--title",
            "B",
            "--source",
            "source:test:one",
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["outcome"]["changed"], Value::Bool(true));
    let proposals = envelope["data"]["outcome"]["proposals"].as_array().unwrap();
    assert_eq!(
        proposals.len(),
        2,
        "the node and its membership are proposed"
    );
    assert_eq!(proposals[0]["object"]["resource"], "wiki:node:b");
    assert_eq!(proposals[1]["object"]["resource"], "wiki:space:child");

    // The round trip the whole design rests on: what the CLI wrote is what the
    // reader parses and the index rebuilds.
    let written = aikit_core::knowledge_wiki_write::WikiDocument::parse(&read(&wiki_json)).unwrap();
    let node_ref = aikit_core::ResourceRef::parse("wiki:node:b").unwrap();
    let Some(aikit_core::knowledge_wiki::WikiObject::Node(added)) = written.object(&node_ref)
    else {
        panic!("the written file holds wiki:node:b");
    };
    assert_eq!(added.node_type, "Claim");
    assert_eq!(added.title.as_deref(), Some("B"));
    assert_eq!(
        added.source_refs,
        vec![aikit_core::SourceRef::parse("source:test:one").unwrap()]
    );
    let child = aikit_core::ResourceRef::parse("wiki:space:child").unwrap();
    let Some(aikit_core::knowledge_wiki::WikiObject::Space(space)) = written.object(&child) else {
        panic!("the written file holds wiki:space:child");
    };
    assert!(
        space.node_refs.contains(&node_ref),
        "the Space records the membership the node claims"
    );
    assert_eq!(space.revision, 3, "one membership, one revision");

    // And the file the pipeline left behind is one the index accepts.
    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "validate", wiki_json.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{envelope}");

    // The stdin path replaces the body wholesale and keeps identity with the
    // file: same ref, revision advanced by exactly one.
    let body = "{\"object\": \"node\", \"profile\": \"okf-wiki/v1\", \"ref\": \"wiki:node:a\", \
         \"type\": \"Definition\", \"title\": \"A, restated\", \"space_refs\": [\"wiki:space:child\"]}";
    let (code, envelope) = wiki_stdin(
        scratch.path(),
        &[
            "wiki",
            "node",
            "update",
            "wiki:node:a",
            "--file",
            wiki_json.to_str().unwrap(),
            "--stdin",
        ],
        body,
    );
    assert_eq!(code, 0, "{envelope}");
    let touched = envelope["data"]["outcome"]["touched"].as_array().unwrap();
    assert_eq!(touched[0]["revision_before"], Value::from(1));
    assert_eq!(touched[0]["revision_after"], Value::from(2));
    let written = aikit_core::knowledge_wiki_write::WikiDocument::parse(&read(&wiki_json)).unwrap();
    let Some(aikit_core::knowledge_wiki::WikiObject::Node(updated)) =
        written.object(&aikit_core::ResourceRef::parse("wiki:node:a").unwrap())
    else {
        panic!("the file still holds wiki:node:a");
    };
    assert_eq!(updated.node_type, "Definition");
    assert_eq!(
        updated.space_refs.len(),
        1,
        "the body replaced the memberships"
    );

    // A body may not rename itself: identity is not rewritten by a write.
    let renamed = body.replace("wiki:node:a", "wiki:node:other");
    let (code, envelope) = wiki_stdin(
        scratch.path(),
        &[
            "wiki",
            "node",
            "update",
            "wiki:node:a",
            "--file",
            wiki_json.to_str().unwrap(),
            "--stdin",
        ],
        &renamed,
    );
    // A usage refusal is exit code 2, per the published table.
    assert_eq!(code, 2);
    assert_eq!(envelope["error"]["code"], "cli.usage", "{envelope}");
}

#[test]
fn a_refused_write_leaves_the_file_byte_identical() {
    let (work, scratch) = fixture();
    let wiki_json = work.path().join("wiki.json");
    let before = fs::read(&wiki_json).unwrap();

    // Refused before the gate: the ref already exists.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "node",
            "create",
            "wiki:node:a",
            "--file",
            wiki_json.to_str().unwrap(),
            "--type",
            "Note",
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(
        envelope["error"]["code"], "knowledge.wiki_ref_exists",
        "{envelope}"
    );
    assert_eq!(
        fs::read(&wiki_json).unwrap(),
        before,
        "a refused command writes nothing"
    );

    // Refused because the document it would join is broken. The mutation runs,
    // the whole refuses to validate, and nothing is rendered.
    write(
        &work.path().join("asymmetric.json"),
        &format!(
            "{{\n  \"objects\": [\n    {},\n    {},\n    {}\n  ]\n}}\n",
            space("wiki:space:root", 1, &[], &["wiki:space:child"], &[]),
            space("wiki:space:child", 1, &[], &[], &[]),
            node("wiki:node:a", 1, &[]),
        ),
    );
    let asymmetric = work.path().join("asymmetric.json");
    let before_gate = fs::read(&asymmetric).unwrap();
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "node",
            "update",
            "wiki:node:a",
            "--file",
            asymmetric.to_str().unwrap(),
            "--title",
            "Rewritten",
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(
        envelope["error"]["code"], "knowledge.wiki_space_asymmetry",
        "{envelope}"
    );
    assert_eq!(
        fs::read(&asymmetric).unwrap(),
        before_gate,
        "a refused mutation writes nothing"
    );
}

#[test]
fn a_prune_dry_run_writes_nothing_and_apply_removes_exactly_one_ref() {
    let central = TempDir::new().unwrap();
    let scratch = TempDir::new().unwrap();
    write(
        &central.path().join("Control/agents/wiki/wiki.json"),
        &root_document(&["central:wiki:project:alpha", "central:wiki:project:beta"]),
    );
    let root = central.path().join("Control/agents/wiki/wiki.json");
    let before = read(&root);

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "prune",
            "central:wiki:project:beta",
            "--root",
            central.path().to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(
        envelope["data"]["applied"],
        Value::Bool(false),
        "prune is a dry run unless --apply"
    );
    assert_eq!(envelope["data"]["would_change"], Value::Bool(true));
    assert_eq!(read(&root), before, "the dry run left the file untouched");

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "prune",
            "central:wiki:project:beta",
            "--apply",
            "--root",
            central.path().to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["applied"], Value::Bool(true));

    let document = aikit_core::knowledge_wiki_write::WikiDocument::parse(&read(&root)).unwrap();
    let root_ref = aikit_core::ResourceRef::parse("central:wiki:root").unwrap();
    let Some(aikit_core::knowledge_wiki::WikiObject::Space(space)) = document.object(&root_ref)
    else {
        panic!("the root file still holds the root Space");
    };
    assert_eq!(
        space.child_space_refs,
        vec![aikit_core::ResourceRef::parse("central:wiki:project:alpha").unwrap()],
        "exactly one ref was retracted"
    );
    assert_eq!(space.revision, 3, "one retraction, one revision");
    assert_ne!(read(&root), before);

    // A ref the root does not hold is a refusal, not a silent no-op.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "prune",
            "central:wiki:project:beta",
            "--apply",
            "--root",
            central.path().to_str().unwrap(),
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(
        envelope["error"]["code"], "knowledge.wiki_ref_missing",
        "{envelope}"
    );
}

#[test]
fn the_root_doctor_reports_the_dangling_and_healthy_sets_and_writes_nothing() {
    let central = TempDir::new().unwrap();
    let scratch = TempDir::new().unwrap();

    // alpha is federated and real; beta is a ref with no project behind it.
    write(
        &central.path().join("Control/agents/wiki/wiki.json"),
        &root_document(&["central:wiki:project:alpha", "central:wiki:project:beta"]),
    );
    write(
        &central
            .path()
            .join("Work/alpha/ProjectCentral/agents/wiki/wiki.json"),
        &format!(
            "{{\n  \"objects\": [\n    {}\n  ]\n}}\n",
            project_space("central:wiki:project:alpha", 1, "central:wiki:root"),
        ),
    );
    write(
        &central
            .path()
            .join("Work/alpha/ProjectCentral/project.json"),
        "{\"schema\": 1, \"project_id\": \"alpha\"}",
    );
    let root = central.path().join("Control/agents/wiki/wiki.json");
    let before = read(&root);

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "doctor",
            "--root",
            central.path().to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let healthy = envelope["data"]["healthy"].as_array().unwrap();
    let dangling = envelope["data"]["dangling"].as_array().unwrap();
    assert_eq!(healthy.len(), 1, "{envelope}");
    assert_eq!(healthy[0]["project"], "alpha");
    assert_eq!(dangling.len(), 1, "{envelope}");
    assert_eq!(dangling[0]["project"], "beta");
    assert_eq!(read(&root), before, "the doctor is read-only");
}

#[test]
fn adopt_federates_an_authored_project_wiki_idempotently() {
    let central = TempDir::new().unwrap();
    let scratch = TempDir::new().unwrap();
    write(
        &central.path().join("Control/agents/wiki/wiki.json"),
        &root_document(&[]),
    );
    let project = central.path().join("Work/alpha");
    write(
        &project.join("ProjectCentral/project.json"),
        "{\"schema\": 1, \"project_id\": \"alpha\"}",
    );
    write(
        &project.join("ProjectCentral/agents/wiki/wiki.json"),
        &format!(
            "{{\n  \"objects\": [\n    {}\n  ]\n}}\n",
            project_space("central:wiki:project:alpha", 1, "central:wiki:root"),
        ),
    );
    let root = central.path().join("Control/agents/wiki/wiki.json");
    let adopt = [
        "wiki",
        "root",
        "adopt",
        project.to_str().unwrap(),
        "--root",
        central.path().to_str().unwrap(),
    ];

    let (code, envelope) = wiki(scratch.path(), &adopt);
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["changed"], Value::Bool(true));

    let document = aikit_core::knowledge_wiki_write::WikiDocument::parse(&read(&root)).unwrap();
    let root_ref = aikit_core::ResourceRef::parse("central:wiki:root").unwrap();
    let Some(aikit_core::knowledge_wiki::WikiObject::Space(space)) = document.object(&root_ref)
    else {
        panic!("the root file still holds the root Space");
    };
    assert!(
        space
            .child_space_refs
            .contains(&aikit_core::ResourceRef::parse("central:wiki:project:alpha").unwrap()),
        "the root now federates the project"
    );
    let after_first = read(&root);

    let (code, envelope) = wiki(scratch.path(), &adopt);
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(
        envelope["data"]["changed"],
        Value::Bool(false),
        "adopt is idempotent: {envelope}"
    );
    assert_eq!(read(&root), after_first, "the second adopt wrote nothing");
}

#[test]
fn an_adopt_refuses_a_project_that_has_no_authored_wiki() {
    let central = TempDir::new().unwrap();
    let scratch = TempDir::new().unwrap();
    write(
        &central.path().join("Control/agents/wiki/wiki.json"),
        &root_document(&[]),
    );
    let project = central.path().join("Work/naked");
    write(
        &project.join("ProjectCentral/project.json"),
        "{\"schema\": 1, \"project_id\": \"naked\"}",
    );
    let root = central.path().join("Control/agents/wiki/wiki.json");
    let before = read(&root);

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "adopt",
            project.to_str().unwrap(),
            "--root",
            central.path().to_str().unwrap(),
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(
        envelope["error"]["code"], "knowledge.wiki_project_wiki_missing",
        "adopt federates an authored Wiki, it does not author one: {envelope}"
    );
    assert_eq!(read(&root), before);
}

// ---------------------------------------------------------------------------
// stage
// ---------------------------------------------------------------------------

const GOVERNANCE_SOURCE: &str = "\
---
ql:
  position: 3
  unit: documentation
  face: direct
---
# Propose, not write

You may propose a change to my source. The proposal is yours; the source is
mine. Return can reach me without rewriting me.
";

#[test]
fn stage_records_the_authored_alignment_and_leaves_the_prose_alone() {
    let (work, scratch) = fixture();
    let wiki_json = work.path().join("wiki.json");
    let source = work.path().join("propose-not-write.md");
    write(&source, GOVERNANCE_SOURCE);

    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "stage", source.to_str().unwrap(), "--file", wiki_json.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["ref"], Value::from("wiki:node:staged/propose-not-write"));
    assert_eq!(envelope["data"]["alignment"]["position"], Value::from(3));
    assert_eq!(envelope["data"]["alignment"]["unit"], Value::from("documentation"));
    assert_eq!(envelope["data"]["alignment"]["face"], Value::from("direct"));

    // The written node: alignment rides as the `ql` extension, the title from
    // the heading, provenance pointing back at the staged source, and the
    // prose body never copied into the Wiki.
    let staged: Value = serde_json::from_str(&read(&wiki_json)).unwrap();
    let node = staged["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["ref"] == "wiki:node:staged/propose-not-write")
        .expect("the staged node is held");
    assert_eq!(node["type"], Value::from("staged-source"));
    assert_eq!(node["title"], Value::from("Propose, not write"));
    assert_eq!(node["ql"]["position"], Value::from(3));
    assert_eq!(node["ql"]["unit"], Value::from("documentation"));
    assert!(node["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry == &Value::from("staging/propose-not-write")));

    // The source file itself is byte-identical: staging records, it never
    // rewrites the handwriting.
    assert_eq!(read(&source), GOVERNANCE_SOURCE);
}

#[test]
fn stage_without_an_authored_alignment_is_a_refusal_not_a_guess() {
    let (work, scratch) = fixture();
    let wiki_json = work.path().join("wiki.json");
    let before = read(&wiki_json);
    let source = work.path().join("plain.md");
    write(&source, "# Plain\n\nNo alignment declared.\n");

    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "stage", source.to_str().unwrap(), "--file", wiki_json.to_str().unwrap()],
    );
    assert_ne!(code, 0);
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("declares no `ql:` frontmatter"));
    assert_eq!(read(&wiki_json), before, "a refusal leaves the file byte-identical");
}

#[test]
fn stage_refuses_positions_outside_the_local_sixfold_and_units_absent() {
    let (work, scratch) = fixture();
    let wiki_json = work.path().join("wiki.json");
    let beyond = work.path().join("beyond.md");
    write(&beyond, "---\nql:\n  position: 7\n  unit: documentation\n---\n# Beyond\n");
    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "stage", beyond.to_str().unwrap(), "--file", wiki_json.to_str().unwrap()],
    );
    assert_ne!(code, 0);
    assert!(envelope["error"]["message"].as_str().unwrap().contains("0–5"));

    let orphan = work.path().join("orphan.md");
    write(&orphan, "---\nql:\n  position: 2\n---\n# Orphan\n");
    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "stage", orphan.to_str().unwrap(), "--file", wiki_json.to_str().unwrap()],
    );
    assert_ne!(code, 0);
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("a position requires its unit"));
}

#[test]
fn stage_replaces_only_when_told_and_advances_the_revision() {
    let (work, scratch) = fixture();
    let wiki_json = work.path().join("wiki.json");
    let source = work.path().join("flow-note.md");
    write(
        &source,
        "---\nql:\n  position: 0\n  unit: documentation\n  type: flow\n---\n# Flow note\n",
    );

    let args = [
        "wiki",
        "stage",
        source.to_str().unwrap(),
        "--file",
        wiki_json.to_str().unwrap(),
    ];
    let (code, _) = wiki(scratch.path(), &args);
    assert_eq!(code, 0);

    // Held without --update: refusal, byte-identical.
    let before = read(&wiki_json);
    let (code, envelope) = wiki(scratch.path(), &args);
    assert_ne!(code, 0);
    assert!(envelope["error"]["message"].as_str().unwrap().contains("--update"));
    assert_eq!(read(&wiki_json), before);

    // With --update: the revision advances, the type label rides through.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "stage",
            source.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
            "--update",
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let staged: Value = serde_json::from_str(&read(&wiki_json)).unwrap();
    let node = staged["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["ref"] == "wiki:node:staged/flow-note")
        .unwrap();
    assert_eq!(node["revision"], Value::from(2));
    assert_eq!(node["type"], Value::from("flow"));
}

// ---------------------------------------------------------------------------
// root anchor
// ---------------------------------------------------------------------------

#[test]
fn anchor_root_creates_a_minimal_identity_node_and_is_idempotent() {
    let (work, scratch) = fixture();
    let central = work.path().join("Central");
    let root_wiki = central.join("Control/agents/wiki/wiki.json");
    write(&root_wiki, &root_document(&[]));

    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "root", "anchor", "--root", central.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["anchor"], Value::from("wiki:node:identity"));
    assert_eq!(envelope["data"]["title"], Value::from("User identity"));

    let held: Value = serde_json::from_str(&read(&root_wiki)).unwrap();
    let space = held["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["ref"] == "central:wiki:root")
        .unwrap();
    assert_eq!(space["anchor_ref"], Value::from("wiki:node:identity"));
    let identity = held["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["ref"] == "wiki:node:identity")
        .unwrap();
    assert_eq!(identity["type"], Value::from("identity"));

    // Second run: nothing changes, the anchored Space reports as such.
    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "root", "anchor", "--root", central.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["outcome"]["changed"], Value::Bool(false));

    // An authored identity node is never rewritten to become an anchor.
    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "node", "update", "wiki:node:identity", "--file", root_wiki.to_str().unwrap(), "--title", "Mine"],
    );
    assert_eq!(code, 0, "{envelope}");
    wiki(
        scratch.path(),
        &["wiki", "root", "anchor", "--root", central.to_str().unwrap()],
    );
    let held: Value = serde_json::from_str(&read(&root_wiki)).unwrap();
    let identity = held["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["ref"] == "wiki:node:identity")
        .unwrap();
    assert_eq!(identity["title"], Value::from("Mine"));
}

#[test]
fn anchor_project_names_the_root_node_from_the_project() {
    let (work, scratch) = fixture();
    let project = work.path().join("My-Project");
    write(
        &project.join("ProjectCentral/project.json"),
        r#"{ "project_id": "project:my-project" }"#,
    );
    write(
        &project.join("ProjectCentral/agents/wiki/wiki.json"),
        &format!(
            "{{\n  \"objects\": [\n    {}\n  ]\n}}\n",
            project_space("central:wiki:project:project:my-project", 1, "central:wiki:root"),
        ),
    );

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "anchor",
            "--project",
            project.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(
        envelope["data"]["anchor"],
        Value::from("wiki:node:project-root/my-project")
    );
    assert_eq!(envelope["data"]["title"], Value::from("My-Project"));

    let held: Value =
        serde_json::from_str(&read(&project.join("ProjectCentral/agents/wiki/wiki.json")))
            .unwrap();
    let space = held["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["ref"] == "central:wiki:project:project:my-project")
        .unwrap();
    assert_eq!(
        space["anchor_ref"],
        Value::from("wiki:node:project-root/my-project")
    );
    assert!(space["node_refs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry == &Value::from("wiki:node:project-root/my-project")));
}

// ---------------------------------------------------------------------------
// wiki ingest / wiki query
//
// The fixture below mirrors the real Return of Zero corpus in miniature:
// a non-record README, an etymology whole-field that cites its argument
// consumer by markdown link (the corpus's dominant citation form) with a
// relative path that only resolves from the citing file's own directory,
// a tagged argument record, and a stale working-tree snapshot that
// re-declares the etymology's own `record_id` — the exact collision the
// live corpus carries in `working/…/snapshots/…/before/`.
// ---------------------------------------------------------------------------

fn ingest_corpus_fixture(root: &Path) {
    write(&root.join("README.md"), "# Not a record\n\nNo frontmatter, no record_id.\n");
    write(
        &root.join("symbolon/episteme/etymologies/arbitration/WHOLE-FIELD.md"),
        "---\nrecord_id: etymology-arbitration\nrecord_type: etymology-whole\nregister: episteme\n---\n\n# Whole Field\n\nSee [A24](../../arguments/A24-Arbitration.md) for the consequential development.\n",
    );
    write(
        &root.join("symbolon/episteme/arguments/A24-Arbitration.md"),
        "---\nrecord_id: A24\nrecord_type: argument\nregister: episteme\nclaim_status: \"Argued\"\ntags:\n  - arbitration\n  - measure\n---\n\n# A24 — Arbitration and the Usurpation of Measure\n",
    );
    // The stale checkpoint: same record_id as the canonical whole-field,
    // sorting after it lexicographically (`submission` < `working`, mirrored
    // here by `symbolon` < `working`), so it is the one set aside.
    write(
        &root.join("working/snapshots/before/expanded-E2.md"),
        "---\nrecord_id: etymology-arbitration\nrecord_type: etymology-whole\n---\n\n# Snapshot copy, not canonical\n",
    );
}

#[test]
fn ingest_dry_run_reports_the_real_mixed_tree_and_writes_nothing() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\n  \"objects\": []\n}\n");
    let before = read(&wiki_json);

    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "ingest", corpus.to_str().unwrap(), "--file", wiki_json.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{envelope}");
    let data = &envelope["data"];
    assert_eq!(data["applied"], Value::from(false));
    // README.md is the one non-record file; the snapshot re-declares
    // etymology-arbitration's id and is set aside, not ingested.
    assert_eq!(data["skipped_no_record_id"], Value::from(1));
    assert_eq!(data["duplicate_record_id"], Value::from(1));
    assert_eq!(data["records_selected"], Value::from(2));
    // Two record nodes, two tag nodes (arbitration, measure), one room space
    // (`symbolon`), and the one resolved markdown-link edge (A24 -> whole
    // field is not asserted; the whole field cites A24, so the edge is
    // whole-field -> A24) plus two tagged edges.
    assert_eq!(data["nodes"], Value::from(4));
    assert_eq!(data["edges"], Value::from(3));
    assert_eq!(data["spaces"], Value::from(1));
    assert_eq!(data["absences"], Value::from(0));

    assert!(envelope["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap().contains("etymology-arbitration")
            && w.as_str().unwrap().contains("expanded-E2.md")));
    assert_eq!(read(&wiki_json), before, "a dry run writes nothing");
}

#[test]
fn ingest_apply_writes_objects_then_refuses_a_rerun_without_update() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\n  \"objects\": []\n}\n");

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "ingest",
            corpus.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
            "--apply",
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["applied"], Value::from(true));

    let held: Value = serde_json::from_str(&read(&wiki_json)).unwrap();
    let objects = held["objects"].as_array().unwrap();
    assert!(objects
        .iter()
        .any(|o| o["ref"] == "wiki:node:record/A24" && o["type"] == "argument"));
    assert!(objects
        .iter()
        .any(|o| o["ref"] == "wiki:node:record/etymology-arbitration"));
    assert!(objects.iter().any(|o| o["ref"] == "wiki:node:tag/arbitration"));

    // A second apply without --update refuses and leaves the file untouched.
    let before = read(&wiki_json);
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "ingest",
            corpus.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
            "--apply",
        ],
    );
    assert_ne!(code, 0);
    assert!(envelope["error"]["message"].as_str().unwrap().contains("--update"));
    assert_eq!(read(&wiki_json), before);

    // With --update the rerun succeeds and advances every touched revision.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "ingest",
            corpus.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
            "--apply",
            "--update",
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let held: Value = serde_json::from_str(&read(&wiki_json)).unwrap();
    let a24 = held["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["ref"] == "wiki:node:record/A24")
        .unwrap();
    assert_eq!(a24["revision"], Value::from(2));
}

/// The capability proof: after ingesting, `wiki query backlinks` and
/// `wiki query search` answer real questions over the ingested field
/// through the ordinary semantic-index surface — backlinks and tags are
/// first-class results, not merely present on the objects.
#[test]
fn query_backlinks_and_search_see_the_ingested_field_as_first_class_results() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\n  \"objects\": []\n}\n");
    let (code, _) = wiki(
        scratch.path(),
        &[
            "wiki",
            "ingest",
            corpus.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
            "--apply",
        ],
    );
    assert_eq!(code, 0);

    // A24's backlinks include the whole-field's markdown-link citation.
    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "query", "backlinks", "wiki:node:record/A24", "--file", wiki_json.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{envelope}");
    let backlinks = envelope["data"]["backlinks"].as_array().unwrap();
    assert!(backlinks
        .iter()
        .any(|n| n["resource"] == "wiki:node:record/etymology-arbitration" && n["relation"] == "references"));

    // The `arbitration` tag's backlinks are the tagged records — first-class,
    // not a side channel.
    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "query", "backlinks", "wiki:node:tag/arbitration", "--file", wiki_json.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{envelope}");
    let backlinks = envelope["data"]["backlinks"].as_array().unwrap();
    assert!(backlinks
        .iter()
        .any(|n| n["resource"] == "wiki:node:record/A24" && n["relation"] == "tagged"));

    // Search finds the ingested record by its title.
    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "query", "search", "Usurpation of Measure", "--file", wiki_json.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{envelope}");
    let hits = envelope["data"]["hits"].as_array().unwrap();
    assert!(hits.iter().any(|h| h["resource"] == "wiki:node:record/A24"));

    // Neighbours from the whole-field show its outgoing reference to A24.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "neighbours",
            "wiki:node:record/etymology-arbitration",
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let neighbours = envelope["data"]["neighbours"].as_array().unwrap();
    assert!(neighbours
        .iter()
        .any(|n| n["resource"] == "wiki:node:record/A24" && n["direction"] == "outgoing"));
}

#[test]
fn ingest_refuses_a_corpus_path_that_is_not_a_directory() {
    let (work, scratch) = fixture();
    let not_a_dir = work.path().join("wiki.json");
    let (code, envelope) = wiki(
        scratch.path(),
        &["wiki", "ingest", not_a_dir.to_str().unwrap(), "--file", not_a_dir.to_str().unwrap()],
    );
    assert_ne!(code, 0);
    assert!(envelope["error"]["message"].as_str().unwrap().contains("not a directory"));
}
