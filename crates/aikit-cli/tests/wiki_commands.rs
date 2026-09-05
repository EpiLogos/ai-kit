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
