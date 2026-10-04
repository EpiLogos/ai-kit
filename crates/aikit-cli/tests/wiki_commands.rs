//! `aikit wiki` end to end.
//!
//! The acceptance criteria are the framing law itself: a command names the file
//! it writes, the whole validates before anything is persisted, a pre-effect
//! refusal leaves that file byte-identical, partial/unknown effects retain their
//! real acknowledgements, a dry run writes nothing, and the root
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

fn native_tempdir(prefix: &str) -> TempDir {
    let temporary = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
    fs::create_dir_all(&temporary).unwrap();
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(&temporary)
        .unwrap()
}

fn fixture() -> (TempDir, TempDir) {
    let work = native_tempdir("native-wiki-work-");
    let scratch = native_tempdir("native-wiki-cwd-");
    write(&work.path().join("wiki.json"), &document());
    (work, scratch)
}

#[test]
fn canonical_wiki_writers_delegate_one_publication_contract() {
    for (name, source) in [
        ("wiki", include_str!("../src/wiki.rs")),
        ("shape", include_str!("../src/wiki_shape.rs")),
        ("construction", include_str!("../src/wiki_construct.rs")),
        (
            "maintenance",
            include_str!("../../aikit-adapters/src/projectcentral.rs"),
        ),
    ] {
        assert!(
            source.contains("publication::publish_wiki("),
            "{name} must use the canonical publication contract"
        );
        assert!(
            !source.contains(".construction.lock")
                && !source.contains(".tmp-{}")
                && !source.contains("fs::rename(&temporary, path)"),
            "{name} must not retain an independent canonical publication route"
        );
    }
}

#[test]
fn concurrent_real_cli_writers_preserve_every_acknowledged_node() {
    use std::sync::{Arc, Barrier};
    let (work, scratch) = fixture();
    let path = work.path().join("wiki.json");
    let barrier = Arc::new(Barrier::new(5));
    let handles: Vec<_> = (0..4)
        .map(|index| {
            let path = path.clone();
            let cwd = scratch.path().to_path_buf();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let resource = format!("wiki:node:concurrent-{index}");
                barrier.wait();
                let (status, envelope) = wiki(
                    &cwd,
                    &[
                        "wiki",
                        "node",
                        "create",
                        &resource,
                        "--file",
                        path.to_str().unwrap(),
                        "--space",
                        "wiki:space:child",
                        "--type",
                        "Claim",
                        "--source",
                        "source:test:concurrent",
                    ],
                );
                (resource, status, envelope)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(results.iter().any(|(_, code, _)| *code == 0));
    let source = read(&path);
    let surviving = aikit_core::WikiDocument::parse(&source).unwrap();
    surviving.validate().unwrap();
    for (resource, status, envelope) in results {
        if status == 0 {
            assert!(
                surviving.holds(&aikit_core::ResourceRef::parse(&resource).unwrap()),
                "acknowledged native result was lost: {resource}"
            );
        } else {
            assert_eq!(
                envelope["error"]["code"], "knowledge.wiki_concurrent_write",
                "{envelope}"
            );
        }
    }
    assert!(surviving.holds(&aikit_core::ResourceRef::parse("wiki:node:a").unwrap()));
}

#[test]
fn query_reads_a_federated_file_without_rewriting_or_inventing_peer_objects() {
    let (work, scratch) = fixture();
    let path = work.path().join("federated.json");
    let source = format!(
        "{{\"objects\":[{},{}]}}\n",
        space(
            "wiki:space:root",
            1,
            &[],
            &["wiki:space:peer"],
            &["wiki:node:a"]
        ),
        node("wiki:node:a", 1, &["wiki:space:root"]),
    );
    write(&path, &source);

    for operation in ["search", "neighbours", "backlinks"] {
        let subject = if operation == "search" {
            "wiki:node:a"
        } else {
            "wiki:space:root"
        };
        let (code, envelope) = wiki(
            scratch.path(),
            &[
                "wiki",
                "query",
                operation,
                subject,
                "--file",
                path.to_str().unwrap(),
            ],
        );
        assert_eq!(code, 0, "{operation}: {envelope}");
        assert!(
            envelope["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning
                    .as_str()
                    .is_some_and(|text| text.contains("wiki:space:peer"))),
            "the external peer is disclosed: {envelope}"
        );
        if operation == "search" {
            assert!(
                !envelope["data"]["hits"].as_array().unwrap().is_empty(),
                "{envelope}"
            );
        }
        assert_eq!(read(&path), source, "a query must preserve canonical bytes");
    }

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "search",
            "wiki:space:peer",
            "--file",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert!(
        envelope["data"]["hits"].as_array().unwrap().is_empty(),
        "an external ref is not a fabricated local Wiki object: {envelope}"
    );
    assert_eq!(read(&path), source);
}

#[test]
fn query_still_refuses_an_unreciprocated_relation_between_local_spaces() {
    let (work, scratch) = fixture();
    let path = work.path().join("asymmetric.json");
    let source = format!(
        "{{\"objects\":[{},{}]}}\n",
        space("wiki:space:root", 1, &[], &["wiki:space:child"], &[]),
        space("wiki:space:child", 1, &[], &[], &[]),
    );
    write(&path, &source);
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "search",
            "root",
            "--file",
            path.to_str().unwrap(),
        ],
    );
    assert_ne!(code, 0, "{envelope}");
    assert_eq!(read(&path), source);
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
    let central = native_tempdir("native-wiki-root-");
    let scratch = native_tempdir("native-wiki-cwd-");
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
    let central = native_tempdir("native-wiki-root-");
    let scratch = native_tempdir("native-wiki-cwd-");

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
    // A dangling federation is a finding ops must not be able to miss: the
    // report still lands, and the exit is non-zero.
    assert_eq!(code, 1, "{envelope}");
    let healthy = envelope["data"]["healthy"].as_array().unwrap();
    let dangling = envelope["data"]["dangling"].as_array().unwrap();
    assert_eq!(healthy.len(), 1, "{envelope}");
    assert_eq!(healthy[0]["project"], "alpha");
    assert_eq!(dangling.len(), 1, "{envelope}");
    assert_eq!(dangling[0]["project"], "beta");
    assert_eq!(read(&root), before, "the doctor is read-only");
}

#[test]
fn the_root_doctor_exits_zero_when_every_child_resolves() {
    let central = native_tempdir("native-wiki-root-");
    let scratch = native_tempdir("native-wiki-cwd-");

    write(
        &central.path().join("Control/agents/wiki/wiki.json"),
        &root_document(&["central:wiki:project:alpha"]),
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
    assert_eq!(
        envelope["data"]["dangling"].as_array().unwrap().len(),
        0,
        "{envelope}"
    );
}

#[test]
fn adopt_federates_an_authored_project_wiki_idempotently() {
    let central = native_tempdir("native-wiki-root-");
    let scratch = native_tempdir("native-wiki-cwd-");
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
    let central = native_tempdir("native-wiki-root-");
    let scratch = native_tempdir("native-wiki-cwd-");
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
        &[
            "wiki",
            "stage",
            source.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(
        envelope["data"]["ref"],
        Value::from("wiki:node:staged/propose-not-write")
    );
    assert_eq!(envelope["data"]["alignment"]["position"], Value::from(3));
    assert_eq!(
        envelope["data"]["alignment"]["unit"],
        Value::from("documentation")
    );
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
        &[
            "wiki",
            "stage",
            source.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_ne!(code, 0);
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("declares no `ql:` frontmatter"));
    assert_eq!(
        read(&wiki_json),
        before,
        "a refusal leaves the file byte-identical"
    );
}

#[test]
fn stage_refuses_positions_outside_the_local_sixfold_and_units_absent() {
    let (work, scratch) = fixture();
    let wiki_json = work.path().join("wiki.json");
    let beyond = work.path().join("beyond.md");
    write(
        &beyond,
        "---\nql:\n  position: 7\n  unit: documentation\n---\n# Beyond\n",
    );
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "stage",
            beyond.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_ne!(code, 0);
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("0–5"));

    let orphan = work.path().join("orphan.md");
    write(&orphan, "---\nql:\n  position: 2\n---\n# Orphan\n");
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "stage",
            orphan.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
        ],
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
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--update"));
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

/// The anchor cites the identity that is live, not the one that was folded.
///
/// `Control/user/identity.md` was folded into `Control/user/identity/` on
/// 2026-09-03 and survives only so older named selections resolve. The entity
/// materialisation path already reads `identity/manifest.json`; the anchor read
/// the stub, so the two paths cited different sources for the same node.
#[test]
fn the_root_anchor_cites_the_live_identity_manifest_over_the_folded_stub() {
    let (work, scratch) = fixture();
    let central = work.path().join("Central");
    write(
        &central.join("Control/agents/wiki/wiki.json"),
        &root_document(&[]),
    );
    write(
        &central.join("Control/user/identity.md"),
        "# Identity\n\n**Status:** folded into `identity/`\n",
    );
    write(
        &central.join("Control/user/identity/manifest.json"),
        r#"{"schema":"central.pasu.identity-manifest/v1","subject":"central:pasu:nara:local"}"#,
    );

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "anchor",
            "--root",
            central.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");

    let held: Value =
        serde_json::from_str(&read(&central.join("Control/agents/wiki/wiki.json"))).unwrap();
    let identity = held["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["ref"] == "wiki:node:identity")
        .unwrap();
    let cited = serde_json::to_string(&identity["source_refs"]).unwrap();
    assert!(
        cited.contains("identity/manifest.json"),
        "the anchor must cite the live manifest: {cited}"
    );
    assert!(
        !cited.contains("user/identity.md"),
        "and not the folded stub: {cited}"
    );
}

/// A world that has not folded its identity yet still anchors on the stub.
#[test]
fn a_world_without_the_manifest_still_anchors_on_the_stub() {
    let (work, scratch) = fixture();
    let central = work.path().join("Central");
    write(
        &central.join("Control/agents/wiki/wiki.json"),
        &root_document(&[]),
    );
    write(
        &central.join("Control/user/identity.md"),
        "# Identity\n\nthe short seed\n",
    );

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "anchor",
            "--root",
            central.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");

    let held: Value =
        serde_json::from_str(&read(&central.join("Control/agents/wiki/wiki.json"))).unwrap();
    let identity = held["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["ref"] == "wiki:node:identity")
        .unwrap();
    assert!(
        serde_json::to_string(&identity["source_refs"])
            .unwrap()
            .contains("user/identity.md"),
        "the stub remains the fallback where nothing has been folded"
    );
}

#[test]
fn anchor_root_creates_a_minimal_identity_node_and_is_idempotent() {
    let (work, scratch) = fixture();
    let central = work.path().join("Central");
    let root_wiki = central.join("Control/agents/wiki/wiki.json");
    write(&root_wiki, &root_document(&[]));

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "anchor",
            "--root",
            central.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(
        envelope["data"]["anchor"],
        Value::from("wiki:node:identity")
    );
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
        &[
            "wiki",
            "root",
            "anchor",
            "--root",
            central.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["outcome"]["changed"], Value::Bool(false));

    // An authored identity node is never rewritten to become an anchor.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "node",
            "update",
            "wiki:node:identity",
            "--file",
            root_wiki.to_str().unwrap(),
            "--title",
            "Mine",
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    wiki(
        scratch.path(),
        &[
            "wiki",
            "root",
            "anchor",
            "--root",
            central.to_str().unwrap(),
        ],
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
            project_space(
                "central:wiki:project:project:my-project",
                1,
                "central:wiki:root"
            ),
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
        serde_json::from_str(&read(&project.join("ProjectCentral/agents/wiki/wiki.json"))).unwrap();
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

/// A room the owner marked `.no-agent-retrieval` must never reach the wiki:
/// ingest is a read, and the marker prunes the subtree before anything in it
/// is read — the same law the ProjectCentral binding and the NOW-field
/// reader honour. Regression from the 2026-09-17 knowledge-fitness round:
/// ingest copied a withheld room's record into the wiki and the faculty then
/// disclosed it.
#[test]
fn ingest_never_reads_a_room_the_owner_withheld_from_agent_retrieval() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    write(
        &corpus.join("open/record.md"),
        "---\nrecord_id: open-record\nrecord_type: note\n---\n\n# Open record\n",
    );
    write(
        &corpus.join("private/.no-agent-retrieval"),
        "This room is withheld from agent retrieval by its owner.\n",
    );
    write(
        &corpus.join("private/endpoint.md"),
        "---\nrecord_id: withheld-record\nrecord_type: note\n---\n\n# Withheld record\n",
    );
    // A real unreadable-as-text body would become an IO diagnostic if the
    // withheld subtree were read before its source eligibility was checked.
    fs::write(corpus.join("private/not-agent-readable.md"), [0xff]).unwrap();
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\n  \"objects\": []\n}\n");

    let (code, dry_run) = wiki(
        scratch.path(),
        &[
            "wiki",
            "ingest",
            corpus.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{dry_run}");
    assert_eq!(dry_run["data"]["records_selected"], 1);
    assert_eq!(dry_run["data"]["io_skipped"], 0);
    assert_eq!(read(&wiki_json), "{\n  \"objects\": []\n}\n");

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
    assert_eq!(envelope["data"]["records_selected"], Value::from(1));
    assert_eq!(envelope["data"]["io_skipped"], 0);
    let source_pool = work.path().join("ingested.sources");
    let shard = read(&source_pool.join("corpus-000.json"));
    assert!(shard.contains("open-record"));
    assert!(!shard.contains("withheld-record"));
    let material: Value = serde_json::from_str(&shard).unwrap();
    assert!(material
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["binding"]["source"] == "central:source:corpus:open-record"));

    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "search",
            "--file",
            wiki_json.to_str().unwrap(),
            "Withheld record",
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let hits = envelope["data"]["hits"].as_array().unwrap();
    assert!(
        !hits.iter().any(|hit| hit["address"]["resource"]
            .as_str()
            .unwrap_or_default()
            .contains("withheld-record")),
        "the withheld room's record is not in the wiki: {hits:?}"
    );
}

#[test]
fn selected_withheld_corpus_root_refuses_dry_run_and_apply_without_pruning_retained_material() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    write(
        &corpus.join("open/record.md"),
        "---\nrecord_id: retained-record\nrecord_type: note\n---\n\n# Retained record\n",
    );
    let wiki_json = work.path().join("ingested.json");
    let pool = work.path().join("retained.sources");
    write(&wiki_json, "{\n  \"objects\": []\n}\n");
    let ingest_args = [
        "wiki",
        "ingest",
        corpus.to_str().unwrap(),
        "--file",
        wiki_json.to_str().unwrap(),
        "--source-pool",
        pool.to_str().unwrap(),
    ];
    let mut apply_args = ingest_args.to_vec();
    apply_args.push("--apply");
    let (code, initial) = wiki(scratch.path(), &apply_args);
    assert_eq!(code, 0, "{initial}");
    assert_eq!(initial["data"]["records_selected"], 1);
    let retained_material: Value =
        serde_json::from_str(&read(&pool.join("corpus-000.json"))).unwrap();
    assert!(retained_material
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["binding"]["source"] == "central:source:corpus:retained-record"));
    let mut retained = vec![(
        wiki_json.clone(),
        fs::read(&wiki_json).unwrap(),
        fs::metadata(&wiki_json).unwrap(),
    )];
    for entry in fs::read_dir(&pool).unwrap() {
        let path = entry.unwrap().path();
        retained.push((
            path.clone(),
            fs::read(&path).unwrap(),
            fs::metadata(&path).unwrap(),
        ));
    }
    let before_names = fs::read_dir(&pool)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<std::collections::BTreeSet<_>>();
    write(
        &corpus.join(".no-agent-retrieval"),
        "The selected corpus is withheld.\n",
    );
    fs::write(corpus.join("open/not-agent-readable.md"), [0xff]).unwrap();

    for args in [&ingest_args[..], &apply_args[..]] {
        let (code, failure) = wiki(scratch.path(), args);
        assert_ne!(code, 0, "{failure}");
        assert_eq!(failure["error"]["code"], "knowledge.ingest_corpus_withheld");
        let details = &failure["error"]["details"];
        assert_eq!(details["command_effect"], "none");
        assert_eq!(details["completed_effects"], "[]");
        let failed: Value =
            serde_json::from_str(details["failed_effect"].as_str().unwrap()).unwrap();
        assert_eq!(failed["phase"], "selection");
        assert_eq!(failed["effect"], "none");
        assert_eq!(failed["source_path"], corpus.to_str().unwrap());
        for (path, bytes, metadata) in &retained {
            assert_eq!(fs::read(path).unwrap(), *bytes, "{}", path.display());
            let current = fs::metadata(path).unwrap();
            assert_eq!(
                current.modified().unwrap(),
                metadata.modified().unwrap(),
                "{}",
                path.display()
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                assert_eq!(current.ino(), metadata.ino(), "{}", path.display());
            }
        }
        assert_eq!(
            fs::read_dir(&pool)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<std::collections::BTreeSet<_>>(),
            before_names
        );
    }

    // A selected unmarked sibling remains useful; this refusal does not
    // widen the marker's meaning to neighbouring corpora or retained copies.
    let sibling = work.path().join("allowed-sibling");
    write(
        &sibling.join("record.md"),
        "---\nrecord_id: allowed-sibling\nrecord_type: note\n---\n\n# Allowed sibling\n",
    );
    let sibling_wiki = work.path().join("sibling.json");
    write(&sibling_wiki, "{\n  \"objects\": []\n}\n");
    let (code, allowed) = wiki(
        scratch.path(),
        &[
            "wiki",
            "ingest",
            sibling.to_str().unwrap(),
            "--file",
            sibling_wiki.to_str().unwrap(),
            "--apply",
        ],
    );
    assert_eq!(code, 0, "{allowed}");
    assert_eq!(allowed["data"]["records_selected"], 1);
    assert!(read(&work.path().join("sibling.sources/corpus-000.json")).contains("allowed-sibling"));
}

fn ingest_corpus_fixture(root: &Path) {
    write(
        &root.join("README.md"),
        "# Not a record\n\nNo frontmatter, no record_id.\n",
    );
    write(
        &root.join("symbolon/episteme/etymologies/arbitration/WHOLE-FIELD.md"),
        "---\nrecord_id: etymology-arbitration\nrecord_type: etymology-whole\nregister: episteme\n---\n\n# Whole Field\n\nSee [A24](../../arguments/A24-Arbitration.md) for the consequential development.\n",
    );
    write(
        &root.join("symbolon/episteme/arguments/A24-Arbitration.md"),
        "---\nrecord_id: A24\nrecord_type: argument\nregister: episteme\nclaim_status: \"Argued\"\nsource_ids:\n  - ostrom-1990-governing-commons\ntags:\n  - arbitration\n  - measure\n---\n\n# A24 — Arbitration and the Usurpation of Measure\n",
    );
    // The bibliography A24 cites, as the corpus files it: a source page
    // declaring `source_id` and the corpus's own authored tag vocabulary.
    write(
        &root.join("symbolon/episteme/sources/ostrom/SOURCE.md"),
        "---\nsource_id: ostrom-1990-governing-commons\nnode_type: source-house\nrecord_type: book\ntitle_full: \"Governing the Commons\"\ntags:\n  - source-bank/record\n  - source-bank/commons\n---\n\n# Governing the Commons\n",
    );
    // A register page: authored tag vocabulary, no identity to place it by.
    // Counted-not-named, this is what hid a whole tag vocabulary from a
    // reader who concluded the corpus declared no tags.
    write(
        &root.join("symbolon/episteme/concepts/reference-notes/measure.md"),
        "---\ntitle: \"Measure\"\nnode_type: reference\ntags:\n  - argument-map/reference\n---\n\n# Measure\n",
    );
    // The stale checkpoint: same record_id as the canonical whole-field,
    // sorting after it lexicographically (`submission` < `working`, mirrored
    // here by `symbolon` < `working`), so it is the one set aside.
    write(
        &root.join("working/snapshots/before/expanded-E2.md"),
        "---\nrecord_id: etymology-arbitration\nrecord_type: etymology-whole\n---\n\n# Snapshot copy, not canonical\n",
    );
}

/// A record's declared bibliography is findable as an authored source, and
/// asking that source for relations says what it is rather than answering an
/// empty list — which a caller cannot tell from "exists, unrelated".
#[test]
fn cited_bibliography_is_findable_and_an_empty_traversal_says_why() {
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

    // The work A24 stands on is findable, as a source and not as a node.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "search",
            "ostrom",
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let hits = envelope["data"]["hits"].as_array().unwrap();
    let hit = hits
        .iter()
        .find(|h| h["address"]["kind"] == "authored-source")
        .expect("the declared bibliography is findable");
    assert_eq!(
        hit["address"]["source"],
        "central:source:corpus:ostrom-1990-governing-commons"
    );

    // Asking it for neighbours answers with the nodes that cite it. Its
    // citations are its neighbourhood; returning an empty list and a note
    // pointing elsewhere was a lecture where an answer belonged.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "neighbours",
            "central:source:corpus:ostrom-1990-governing-commons",
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let neighbours = envelope["data"]["neighbours"].as_array().unwrap();
    assert!(
        neighbours
            .iter()
            .any(|n| n["resource"] == "wiki:node:record/A24"
                && n["relation"] == "cites"
                && n["direction"] == "incoming"),
        "the citing node is the source's neighbourhood: {envelope}"
    );
    assert!(
        envelope["warnings"].as_array().unwrap().is_empty(),
        "nothing to apologise for once the question is answered: {envelope}"
    );

    // A ref the field does not hold at all is a different answer again.
    let (_, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "neighbours",
            "wiki:node:record/absent",
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    let warnings = envelope["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .unwrap_or_default()
            .contains("not in this Wiki file")),
        "an unknown ref says it is absent, not unrelated: {warnings:?}"
    );

    // A real curated node stays quiet.
    let (_, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "neighbours",
            "wiki:node:record/A24",
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert!(envelope["warnings"].as_array().unwrap().is_empty());
}

/// Same-named bibliography files in different rooms, each with its own
/// declared `source_id`, must not collide — and the dry run must predict
/// that the apply succeeds rather than promising a write that then fails.
#[test]
fn same_named_source_files_do_not_collide_and_the_dry_run_predicts_the_apply() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    for (room, id) in [
        ("01-differentiating-mind", "01-differentiating-mind-p1"),
        ("02-return-of-zero", "02-return-of-zero-p1"),
    ] {
        write(
            &corpus.join(format!("section-rooms/{room}/P1-CANONICAL-ALIGNMENT.md")),
            &format!("---\nsource_id: {id}\ntags: [station/s0]\n---\n\n# Alignment\n"),
        );
    }
    // One curated record so the ingest has a node population too.
    write(
        &corpus.join("arguments/A24.md"),
        "---\nrecord_id: A24\nrecord_type: argument\nsource_ids:\n           - 01-differentiating-mind-p1\n---\n\n# A24\n",
    );
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
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(
        envelope["data"]["self_colliding_refs"], 0,
        "distinct source_ids in same-named files do not collide: {envelope}"
    );

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
    assert_eq!(
        code, 0,
        "the apply the dry run promised must succeed: {envelope}"
    );

    // The record's cited bibliography is reachable from the record.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "neighbours",
            "wiki:node:record/A24",
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
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
        &[
            "wiki",
            "ingest",
            corpus.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let data = &envelope["data"];
    assert_eq!(data["applied"], Value::from(false));
    // README.md carries nothing to place and is counted; the register page
    // carries an authored `tags:` vocabulary and no identity, so it is named;
    // the snapshot re-declares etymology-arbitration's id and is set aside.
    assert_eq!(data["skipped_inert"], Value::from(1));
    assert_eq!(data["skipped_unaddressable"], Value::from(1));
    assert_eq!(data["duplicate_record_id"], Value::from(1));
    assert_eq!(data["records_selected"], Value::from(2));
    assert_eq!(data["sources_selected"], Value::from(1));
    // Two record nodes — and no tag node, because a tag is not curated
    // identity. One room space (`symbolon`), and the one resolved
    // markdown-link edge (the whole field cites A24, so the edge is
    // whole-field -> A24). No `tagged` edges.
    assert_eq!(data["nodes"], Value::from(2));
    assert_eq!(data["edges"], Value::from(1));
    assert_eq!(data["spaces"], Value::from(1));
    assert_eq!(data["absences"], Value::from(0));
    // Tags ride the SourcePool: one binding per record and per source.
    assert_eq!(data["source_bindings"], Value::from(3));
    assert_eq!(data["tag_vocabulary"], Value::from(4));
    assert_eq!(data["tagged_bindings"], Value::from(2));
    assert_eq!(
        data["source_pool_files"],
        Value::from(0),
        "a dry run writes no pool"
    );

    assert!(
        envelope["warnings"].as_array().unwrap().iter().any(|w| {
            let w = w.as_str().unwrap_or_default();
            w.contains("reference-notes/measure.md") && w.contains("`tags:`")
        }),
        "a file carrying corpus metadata is named, not counted: {:?}",
        envelope["warnings"]
    );

    assert!(envelope["warnings"].as_array().unwrap().iter().any(|w| w
        .as_str()
        .unwrap()
        .contains("etymology-arbitration")
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
    assert!(
        !objects.iter().any(|o| o["ref"]
            .as_str()
            .unwrap_or_default()
            .starts_with("wiki:node:tag/")),
        "a tag is not curated Wiki identity"
    );

    // The tags landed in the SourcePool instead, as discoverable shards.
    let pool = work.path().join("ingested.sources");
    let shards: Vec<_> = std::fs::read_dir(&pool)
        .expect("the source pool directory is written")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with("corpus-") && name.ends_with(".json")
            })
        })
        .collect();
    assert_eq!(shards.len(), 1, "{shards:?}");
    let material: Value = serde_json::from_str(&read(&shards[0])).unwrap();
    let a24 = material
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["binding"]["source"] == "central:source:corpus:A24")
        .expect("the record binds its own text as source material");
    let tags: Vec<&str> = a24["binding"]["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tag| tag.as_str().unwrap())
        .collect();
    assert_eq!(
        tags,
        vec![
            "arbitration",
            "measure",
            "source-bank/record",
            "source-bank/commons"
        ],
        "its own declared tags, then the vocabulary of the source it cites"
    );

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
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--update"));
    assert_eq!(read(&wiki_json), before);

    // With --update the rerun succeeds — and because the corpus is
    // byte-identical, nothing is a content change: every revision stays
    // where it was. A touch without a byte change is not a content change.
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
    let unchanged = envelope["data"]["unchanged"].as_u64().unwrap();
    assert!(
        unchanged > 0,
        "the identical rerun changed nothing: {envelope}"
    );
    let held: Value = serde_json::from_str(&read(&wiki_json)).unwrap();
    let a24 = held["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["ref"] == "wiki:node:record/A24")
        .unwrap();
    assert_eq!(
        a24["revision"],
        Value::from(1),
        "an unchanged re-ingest advances no revision"
    );
}

/// Re-ingesting a corpus where exactly one record genuinely changed must
/// advance that record's revision and touch nothing else — update fidelity,
/// not a wholesale rewrite.
#[test]
fn ingest_update_advances_only_the_record_that_changed() {
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
            "--update",
        ],
    );
    assert_eq!(code, 0, "{envelope}");

    // A real content change to one record's source file.
    let record = corpus.join("symbolon/episteme/arguments/A24-Arbitration.md");
    let mut text = read(&record);
    text.push_str("\nAddendum: the criterion is the measure, never the usurper.\n");
    write(&record, &text);

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
    let objects = held["objects"].as_array().unwrap();
    let changed: Vec<&Value> = objects
        .iter()
        .filter(|o| o["revision"].as_u64().unwrap() > 1)
        .collect();
    assert_eq!(
        changed
            .iter()
            .map(|o| o["ref"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["wiki:node:record/A24"],
        "only the record whose bytes changed advanced"
    );
    assert!(
        envelope["data"]["unchanged"].as_u64().unwrap() > 0,
        "everything else kept its revision: {envelope}"
    );
}

/// The capability proof: after ingesting, `wiki query backlinks` and
/// `wiki query search` answer real questions over the ingested field
/// through the ordinary semantic-index surface — backlinks are first-class
/// results, not merely present on the objects. Tags are deliberately not
/// among them: they are a SourcePool property, exercised by
/// `ingest_apply_writes_objects_then_refuses_a_rerun_without_update`.
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
        &[
            "wiki",
            "query",
            "backlinks",
            "wiki:node:record/A24",
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let backlinks = envelope["data"]["backlinks"].as_array().unwrap();
    assert!(backlinks.iter().any(
        |n| n["resource"] == "wiki:node:record/etymology-arbitration"
            && n["relation"] == "references"
    ));

    // A tag ref is not in the Wiki field at all, and asking says so rather
    // than answering an empty list.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "backlinks",
            "wiki:node:tag/arbitration",
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    assert!(envelope["data"]["backlinks"].as_array().unwrap().is_empty());
    assert!(
        envelope["warnings"].as_array().unwrap().iter().any(|w| w
            .as_str()
            .unwrap_or_default()
            .contains("not in this Wiki file")),
        "{:?}",
        envelope["warnings"]
    );

    // Search finds the ingested record by its title.
    let (code, envelope) = wiki(
        scratch.path(),
        &[
            "wiki",
            "query",
            "search",
            "Usurpation of Measure",
            "--file",
            wiki_json.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{envelope}");
    let hits = envelope["data"]["hits"].as_array().unwrap();
    assert!(hits
        .iter()
        .any(|h| h["address"]["kind"] == "curated"
            && h["address"]["resource"] == "wiki:node:record/A24"));

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
        &[
            "wiki",
            "ingest",
            not_a_dir.to_str().unwrap(),
            "--file",
            not_a_dir.to_str().unwrap(),
        ],
    );
    assert_ne!(code, 0);
    assert!(envelope["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not a directory"));
}

#[test]
fn federated_link_failure_retains_the_first_publication_and_exact_cause() {
    let (work, scratch) = fixture();
    let parent = work.path().join("parent.json");
    let missing = work.path().join("missing-child.json");
    let root = "wiki:space:central-root";
    let child = "wiki:space:project/phase-return";
    write(
        &parent,
        &format!("{{\"objects\":[{}]}}", space(root, 1, &[], &[], &[])),
    );
    let args = [
        "wiki",
        "space",
        "link",
        root,
        child,
        "--file",
        parent.to_str().unwrap(),
        "--child-file",
        missing.to_str().unwrap(),
    ];
    let (code, failure) = wiki(scratch.path(), &args);
    assert_ne!(code, 0);
    assert_eq!(failure["error"]["code"], "knowledge.wiki_file_unreadable");
    let details = &failure["error"]["details"];
    assert_eq!(details["command_effect"], "present");
    assert_eq!(details["outcome"], "partial");
    assert_eq!(details["automatic_retry"], "false");
    let completed: Value =
        serde_json::from_str(details["completed_effects"].as_str().unwrap()).unwrap();
    assert_eq!(completed.as_array().unwrap().len(), 1);
    assert_eq!(completed[0]["owner"], "AIKit/Wiki");
    assert_eq!(
        completed[0]["source_path"],
        fs::canonicalize(&parent).unwrap().display().to_string()
    );
    let original: Value =
        serde_json::from_str(details["original_error"].as_str().unwrap()).unwrap();
    assert_eq!(original["code"], "knowledge.wiki_file_unreadable");
    assert_eq!(original["details"]["path"], missing.display().to_string());
    let retained: Value = serde_json::from_str(&read(&parent)).unwrap();
    assert_eq!(retained["objects"][0]["revision"], 2);
    assert_eq!(retained["objects"][0]["child_space_refs"][0], child);
    assert!(!missing.exists(), "the failed peer was never invented");

    // The same real invocation now has a no-op first leg. Its second read
    // failure cannot claim that a new publication occurred on replay.
    let before = read(&parent);
    let modified = fs::metadata(&parent).unwrap().modified().unwrap();
    let (_, failure) = wiki(scratch.path(), &args);
    assert_eq!(failure["error"]["details"]["command_effect"], "none");
    assert_eq!(failure["error"]["details"]["completed_effects"], "[]");
    assert_eq!(read(&parent), before);
    assert_eq!(fs::metadata(&parent).unwrap().modified().unwrap(), modified);
}

#[test]
fn ingest_material_preparation_failure_keeps_the_acknowledged_wiki() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    let blocked_pool = work.path().join("ordinary-file");
    write(&wiki_json, "{\"objects\":[]}");
    write(&blocked_pool, "retain this ordinary file");
    let (code, failure) = wiki(
        scratch.path(),
        &[
            "wiki",
            "ingest",
            corpus.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
            "--source-pool",
            blocked_pool.to_str().unwrap(),
            "--apply",
        ],
    );
    assert_ne!(code, 0);
    assert_eq!(
        failure["error"]["code"],
        "knowledge.ingest_source_pool_unwritable"
    );
    let details = &failure["error"]["details"];
    assert_eq!(
        details["command_effect"], "unknown",
        "an actual directory preparation attempt is not proof of no effect"
    );
    assert_eq!(details["automatic_retry"], "false");
    let completed: Value =
        serde_json::from_str(details["completed_effects"].as_str().unwrap()).unwrap();
    assert_eq!(completed.as_array().unwrap().len(), 1);
    assert_eq!(completed[0]["owner"], "AIKit/Wiki");
    let failed: Value = serde_json::from_str(details["failed_effect"].as_str().unwrap()).unwrap();
    assert_eq!(failed["owner"], "AIKit/SourcePool");
    assert_eq!(failed["phase"], "prepare_directory");
    let retained: Value = serde_json::from_str(&read(&wiki_json)).unwrap();
    assert!(retained["objects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|object| object["ref"] == "wiki:node:record/A24"));
    assert_eq!(read(&blocked_pool), "retain this ordinary file");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn actual_created_unreadable_source_pool_retains_known_wiki_and_directory_effects() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    struct CreatedPoolPermissions {
        path: std::path::PathBuf,
        identity: Option<(u64, u64)>,
    }
    impl Drop for CreatedPoolPermissions {
        fn drop(&mut self) {
            if let Some(identity) = self.identity {
                if let Ok(metadata) = fs::symlink_metadata(&self.path) {
                    if metadata.is_dir() && (metadata.dev(), metadata.ino()) == identity {
                        if let Err(error) =
                            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o700))
                        {
                            eprintln!(
                                "actual owned test directory cleanup failed for {}: {error}",
                                self.path.display()
                            );
                        }
                    }
                }
            }
        }
    }

    let uid = Command::new("/usr/bin/id").arg("-u").output().unwrap();
    assert!(
        uid.status.success(),
        "actual UID prerequisite failed: {uid:?}"
    );
    let uid = String::from_utf8(uid.stdout).unwrap();
    assert_ne!(
        uid.trim(),
        "0",
        "real directory EACCES qualification requires a nonroot Linux/Mac process"
    );
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\"objects\":[]}");
    fs::set_permissions(&wiki_json, fs::Permissions::from_mode(0o644)).unwrap();
    let before_wiki = read(&wiki_json);
    let pool = work.path().join("ingested.sources");
    assert!(!pool.exists());
    let args = [
        "wiki",
        "ingest",
        corpus.to_str().unwrap(),
        "--file",
        wiki_json.to_str().unwrap(),
        "--apply",
    ];
    // Only the native child's creation mode changes. Existing source metadata
    // is preserved by actual publication; no global test-process umask changes.
    let output = Command::new("/bin/sh")
        .args([
            "-c",
            "umask 0444 || exit 125; exec \"$@\"",
            "native-directory-partial",
        ])
        .arg(assert_cmd::cargo::cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .current_dir(scratch.path())
        .output()
        .unwrap();
    let _pool_cleanup = CreatedPoolPermissions {
        path: pool.clone(),
        identity: fs::symlink_metadata(&pool)
            .ok()
            .filter(|metadata| metadata.is_dir())
            .map(|metadata| (metadata.dev(), metadata.ino())),
    };
    assert_ne!(
        output.status.code(),
        Some(125),
        "actual subprocess-local umask must be available"
    );
    assert!(
        !output.status.success(),
        "actual unreadable directory inventory must fail"
    );
    let failure: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!("actual native error envelope missing: {error}; {output:?}")
    });
    assert_eq!(
        failure["error"]["code"],
        "knowledge.ingest_source_pool_unwritable"
    );
    let details = &failure["error"]["details"];
    assert_eq!(details["command_effect"], "present");
    assert_eq!(details["outcome"], "partial");
    assert_eq!(details["automatic_retry"], "false");
    let completed: Value =
        serde_json::from_str(details["completed_effects"].as_str().unwrap()).unwrap();
    assert_eq!(completed.as_array().unwrap().len(), 2);
    assert_eq!(completed[0]["owner"], "AIKit/Wiki");
    assert_eq!(
        completed[0]["source_path"],
        fs::canonicalize(&wiki_json).unwrap().display().to_string()
    );
    assert_eq!(completed[1]["owner"], "AIKit/SourcePool");
    assert_eq!(completed[1]["action"], "prepare_directory");
    assert_eq!(completed[1]["source_path"], pool.display().to_string());
    let failed: Value = serde_json::from_str(details["failed_effect"].as_str().unwrap()).unwrap();
    assert_eq!(failed["phase"], "inventory");
    assert_eq!(failed["effect"], "none");
    let original: Value =
        serde_json::from_str(details["original_error"].as_str().unwrap()).unwrap();
    assert_eq!(original["code"], "knowledge.ingest_source_pool_unwritable");
    assert_eq!(original["details"]["cause_kind"], "PermissionDenied");
    let actual_io = fs::read_dir(&pool).unwrap_err();
    assert_eq!(actual_io.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(actual_io.raw_os_error().is_some());
    assert_eq!(
        original["details"]["cause_raw_os_error"],
        serde_json::json!(actual_io.raw_os_error()).to_string()
    );
    assert_ne!(
        read(&wiki_json),
        before_wiki,
        "actual acknowledged Wiki content must survive"
    );
    let acknowledged: Value = serde_json::from_str(&read(&wiki_json)).unwrap();
    assert!(acknowledged["objects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|object| object["ref"] == "wiki:node:record/A24"));
    assert_eq!(
        fs::metadata(&wiki_json).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert_eq!(
        fs::metadata(&pool).unwrap().permissions().mode() & 0o777,
        0o333
    );
    // Cleanup is explicit after the retained physical state and real error
    // have been inspected. It changes no published Wiki source.
    drop(_pool_cleanup);
    assert_eq!(
        fs::metadata(&pool).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::read_dir(&pool).unwrap().count(),
        0,
        "no material was published"
    );
}

#[test]
fn incomplete_corpus_keeps_all_existing_wiki_and_material_bytes() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\"objects\":[]}");
    let args = [
        "wiki",
        "ingest",
        corpus.to_str().unwrap(),
        "--file",
        wiki_json.to_str().unwrap(),
        "--apply",
        "--update",
    ];
    let (code, envelope) = wiki(scratch.path(), &args);
    assert_eq!(code, 0, "{envelope}");
    let material_path = work.path().join("ingested.sources/corpus-000.json");
    let before_wiki = read(&wiki_json);
    let before_material = read(&material_path);
    fs::write(corpus.join("unreadable-text.md"), [0xff, 0xfe]).unwrap();
    let (code, failure) = wiki(scratch.path(), &args);
    assert_ne!(code, 0);
    assert_eq!(
        failure["error"]["code"],
        "knowledge.ingest_corpus_incomplete"
    );
    assert_eq!(failure["error"]["details"]["command_effect"], "none");
    assert_eq!(read(&wiki_json), before_wiki);
    assert_eq!(read(&material_path), before_material);
}

#[cfg(unix)]
#[test]
fn rejected_material_replacement_retains_old_shards_and_acknowledged_wiki() {
    use std::os::unix::fs::MetadataExt;
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\"objects\":[]}");
    let args = [
        "wiki",
        "ingest",
        corpus.to_str().unwrap(),
        "--file",
        wiki_json.to_str().unwrap(),
        "--apply",
        "--update",
    ];
    let (code, envelope) = wiki(scratch.path(), &args);
    assert_eq!(code, 0, "{envelope}");
    let pool = work.path().join("ingested.sources");
    let old = pool.join("corpus-000.json");
    let stale = pool.join("corpus-999.json");
    fs::copy(&old, &stale).unwrap();
    let alias = work.path().join("actual-retained-material-alias");
    fs::hard_link(&old, &alias).unwrap();
    let basis = read(&old);
    let metadata = fs::metadata(&old).unwrap();
    let before_wiki = read(&wiki_json);
    let source = corpus.join("symbolon/episteme/arguments/A24-Arbitration.md");
    let next = format!("{}\nA subsequent authored observation.\n", read(&source));
    fs::write(&source, next).unwrap();
    let (code, failure) = wiki(scratch.path(), &args);
    assert_ne!(code, 0);
    assert_eq!(
        failure["error"]["code"],
        "knowledge.wiki_publication_identity"
    );
    let details = &failure["error"]["details"];
    assert_eq!(details["command_effect"], "present");
    assert_eq!(details["outcome"], "partial");
    assert_eq!(details["automatic_retry"], "false");
    let completed: Value =
        serde_json::from_str(details["completed_effects"].as_str().unwrap()).unwrap();
    assert_eq!(completed.as_array().unwrap().len(), 1);
    assert_eq!(completed[0]["owner"], "AIKit/Wiki");
    assert_ne!(
        read(&wiki_json),
        before_wiki,
        "the earlier Wiki publication actually completed"
    );
    let failed: Value = serde_json::from_str(details["failed_effect"].as_str().unwrap()).unwrap();
    assert_eq!(failed["owner"], "AIKit/SourcePool");
    assert_eq!(failed["phase"], "read_basis");
    assert_eq!(failed["effect"], "none");
    let original: Value =
        serde_json::from_str(details["original_error"].as_str().unwrap()).unwrap();
    assert_eq!(original["code"], "knowledge.wiki_publication_identity");
    assert!(!original["details"]
        .as_object()
        .unwrap()
        .contains_key("published"));
    assert_eq!(read(&old), basis);
    assert_eq!(read(&alias), basis);
    assert_eq!(
        read(&stale),
        basis,
        "stale removal cannot precede the required replacement"
    );
    let after = fs::metadata(&old).unwrap();
    assert_eq!(
        (
            after.dev(),
            after.ino(),
            after.uid(),
            after.gid(),
            after.mode()
        ),
        (
            metadata.dev(),
            metadata.ino(),
            metadata.uid(),
            metadata.gid(),
            metadata.mode()
        )
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn source_pool_basis_refuses_actual_symlink_and_fifo_substitutions_without_losing_wiki_acknowledgement(
) {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::time::{Duration, Instant};

    for fifo in [false, true] {
        let (work, scratch) = fixture();
        let corpus = work.path().join("corpus");
        ingest_corpus_fixture(&corpus);
        let wiki_json = work.path().join("ingested.json");
        write(&wiki_json, "{\"objects\":[]}");
        let args = [
            "wiki",
            "ingest",
            corpus.to_str().unwrap(),
            "--file",
            wiki_json.to_str().unwrap(),
            "--apply",
            "--update",
        ];
        let (code, envelope) = wiki(scratch.path(), &args);
        assert_eq!(code, 0, "{envelope}");
        let pool = work.path().join("ingested.sources");
        let material = pool.join("corpus-000.json");
        let physical_material = fs::canonicalize(&pool).unwrap().join("corpus-000.json");
        let retained = pool.join("retained-original.json");
        let stale = pool.join("corpus-999.json");
        fs::copy(&material, &stale).unwrap();
        let basis = read(&material);
        let before = fs::metadata(&material).unwrap();
        fs::rename(&material, &retained).unwrap();
        let unselected = work.path().join("unselected-source.json");
        write(&unselected, "retained unselected source");
        if fifo {
            let setup = Command::new("/usr/bin/mkfifo")
                .arg(&material)
                .output()
                .unwrap();
            assert!(
                setup.status.success(),
                "actual FIFO setup failed: {setup:?}"
            );
        } else {
            std::os::unix::fs::symlink(&unselected, &material).unwrap();
        }
        let authored = corpus.join("symbolon/episteme/arguments/A24-Arbitration.md");
        fs::write(
            &authored,
            format!("{}\nA subsequent authored observation.\n", read(&authored)),
        )
        .unwrap();
        let before_wiki = read(&wiki_json);
        // The actual native process must return without opening a blocking FIFO.
        // A timeout retains its real output and fails; it never fabricates a refusal.
        let mut child = Command::new(assert_cmd::cargo::cargo_bin("aikit"))
            .args(args)
            .arg("--json")
            .current_dir(scratch.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("actual SourcePool basis refusal did not return within its test bound: {output:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            !output.status.success(),
            "actual nonordinary source must be refused"
        );
        let failure: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!("actual native error envelope missing: {error}; {output:?}")
        });
        assert_eq!(
            failure["error"]["code"],
            "knowledge.wiki_publication_identity"
        );
        let details = &failure["error"]["details"];
        assert_eq!(details["command_effect"], "present");
        assert_eq!(details["outcome"], "partial");
        assert_eq!(details["automatic_retry"], "false");
        let completed: Value =
            serde_json::from_str(details["completed_effects"].as_str().unwrap()).unwrap();
        assert_eq!(completed.as_array().unwrap().len(), 1);
        assert_eq!(completed[0]["owner"], "AIKit/Wiki");
        assert_eq!(
            completed[0]["source_path"],
            fs::canonicalize(&wiki_json).unwrap().display().to_string()
        );
        assert_ne!(read(&wiki_json), before_wiki);
        let failed: Value =
            serde_json::from_str(details["failed_effect"].as_str().unwrap()).unwrap();
        assert_eq!(failed["owner"], "AIKit/SourcePool");
        assert_eq!(failed["source_path"], material.display().to_string());
        assert_eq!(failed["phase"], "read_basis");
        assert_eq!(failed["effect"], "none");
        let original: Value =
            serde_json::from_str(details["original_error"].as_str().unwrap()).unwrap();
        assert_eq!(original["code"], "knowledge.wiki_publication_identity");
        assert_eq!(
            original["details"]["path"],
            physical_material.display().to_string()
        );
        assert!(!original["details"]
            .as_object()
            .unwrap()
            .contains_key("published"));
        assert_eq!(read(&retained), basis);
        assert_eq!(read(&stale), basis, "no prune after basis refusal");
        assert_eq!(read(&unselected), "retained unselected source");
        let after = fs::metadata(&retained).unwrap();
        assert_eq!(
            (
                after.dev(),
                after.ino(),
                after.uid(),
                after.gid(),
                after.mode()
            ),
            (
                before.dev(),
                before.ino(),
                before.uid(),
                before.gid(),
                before.mode()
            )
        );
        let substituted = fs::symlink_metadata(&material).unwrap();
        if fifo {
            assert!(substituted.file_type().is_fifo());
        } else {
            assert!(substituted.file_type().is_symlink());
        }
    }
}

#[test]
fn source_material_replacement_completes_before_stale_shards_are_removed() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\"objects\":[]}");
    let args = [
        "wiki",
        "ingest",
        corpus.to_str().unwrap(),
        "--file",
        wiki_json.to_str().unwrap(),
        "--apply",
        "--update",
    ];
    let (code, envelope) = wiki(scratch.path(), &args);
    assert_eq!(code, 0, "{envelope}");
    let pool = work.path().join("ingested.sources");
    let material = pool.join("corpus-000.json");
    let stale = pool.join("corpus-999.json");
    fs::copy(&material, &stale).unwrap();
    let unrelated = pool.join("retained-other-owner.txt");
    write(&unrelated, "other owner's material");
    let source = corpus.join("symbolon/episteme/arguments/A24-Arbitration.md");
    let next = format!("{}\nA subsequent authored observation.\n", read(&source));
    fs::write(&source, &next).unwrap();
    let (code, envelope) = wiki(scratch.path(), &args);
    assert_eq!(code, 0, "{envelope}");
    assert!(!stale.exists());
    let retained: Value = serde_json::from_str(&read(&material)).unwrap();
    let record = retained
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["binding"]["source"] == "central:source:corpus:A24")
        .unwrap();
    assert_eq!(record["body"], next);
    assert_eq!(read(&unrelated), "other owner's material");
}

#[cfg(unix)]
#[test]
fn actual_process_file_size_failure_during_staging_keeps_previous_material() {
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\"objects\":[]}");
    let args = [
        "wiki",
        "ingest",
        corpus.to_str().unwrap(),
        "--file",
        wiki_json.to_str().unwrap(),
        "--apply",
        "--update",
    ];
    let (code, envelope) = wiki(scratch.path(), &args);
    assert_eq!(code, 0, "{envelope}");
    let pool = work.path().join("ingested.sources");
    let material = pool.join("corpus-000.json");
    let previous_material = read(&material);
    let source = corpus.join("symbolon/episteme/arguments/A24-Arbitration.md");
    fs::write(
        &source,
        format!(
            "{}\n{}\n",
            read(&source),
            "Actual subsequent authored content. ".repeat(256)
        ),
    )
    .unwrap();
    let (code, envelope) = wiki(scratch.path(), &args);
    assert_eq!(code, 0, "{envelope}");
    let acknowledged_wiki = read(&wiki_json);
    // Retain a real earlier material cut beside the current acknowledged
    // Wiki, as the interrupted old refresh can leave it. No fake receipt.
    fs::write(&material, &previous_material).unwrap();
    let stale = pool.join("corpus-999.json");
    fs::copy(&material, &stale).unwrap();
    let binary = assert_cmd::cargo::cargo_bin("aikit");
    let output = Command::new("/bin/sh")
        .args([
            "-c",
            "ulimit -f 1 || exit 125; exec \"$@\"",
            "bounded-real-file-write",
        ])
        .arg(binary)
        .args(args)
        .arg("--json")
        .current_dir(scratch.path())
        .output()
        .unwrap();
    assert_ne!(
        output.status.code(),
        Some(125),
        "native process file-size limit must be available"
    );
    assert!(
        !output.status.success(),
        "a real bounded write must fail, not a simulated provider"
    );
    assert_eq!(
        read(&wiki_json),
        acknowledged_wiki,
        "the first leg was a byte-identical no-op"
    );
    assert_eq!(
        read(&material),
        previous_material,
        "a failed stage must not truncate the retained destination"
    );
    assert_eq!(
        read(&stale),
        previous_material,
        "no destructive pruning before required replacement acknowledgement"
    );
}

#[cfg(unix)]
#[test]
fn real_refresh_prunes_only_canonical_native_shards_and_preserves_foreign_names() {
    use std::os::unix::ffi::OsStringExt;
    let (work, scratch) = fixture();
    let corpus = work.path().join("corpus");
    ingest_corpus_fixture(&corpus);
    let wiki_json = work.path().join("ingested.json");
    write(&wiki_json, "{\"objects\":[]}");
    let args = [
        "wiki",
        "ingest",
        corpus.to_str().unwrap(),
        "--file",
        wiki_json.to_str().unwrap(),
        "--apply",
        "--update",
    ];
    let (code, envelope) = wiki(scratch.path(), &args);
    assert_eq!(code, 0, "{envelope}");
    let pool = work.path().join("ingested.sources");
    let canonical = pool.join("corpus-000.json");
    let stale = pool.join("corpus-999.json");
    let large_index_stale = pool.join("corpus-1000.json");
    fs::copy(&canonical, &stale).unwrap();
    fs::copy(&canonical, &large_index_stale).unwrap();
    let mut foreign = vec![
        pool.join("corpus-notes.json"),
        pool.join("corpus-0000.json"),
        pool.join("corpus-β.json"),
    ];
    for (index, path) in foreign.iter().enumerate() {
        fs::write(path, format!("authored foreign source {index}\n")).unwrap();
    }
    let non_utf8 = pool.join(std::ffi::OsString::from_vec(b"corpus-\xff.json".to_vec()));
    match fs::write(
        &non_utf8,
        "authored foreign source with non-UTF8 filename\n",
    ) {
        Ok(()) => foreign.push(non_utf8),
        Err(error) => {
            // The actual Mac CI filesystem rejects this filename with EILSEQ
            // (Darwin errno 92). Retain that precise capacity limit while the
            // Unicode and canonical-lookalike preservation cases still run.
            assert!(
                cfg!(target_os = "macos") && error.raw_os_error() == Some(92),
                "unexpected native non-UTF8 filename creation failure: {error}"
            );
            eprintln!("non-UTF8 filename preservation subcase unavailable: {error}; portable foreign-name cases remain active");
        }
    }
    let before: Vec<_> = foreign.iter().map(|path| fs::read(path).unwrap()).collect();
    let source = corpus.join("symbolon/episteme/arguments/A24-Arbitration.md");
    fs::write(
        &source,
        format!("{}\nActual next authored observation.\n", read(&source)),
    )
    .unwrap();
    let (code, envelope) = wiki(scratch.path(), &args);
    assert_eq!(code, 0, "{envelope}");
    assert!(
        !stale.exists(),
        "the canonical native stale shard is removed"
    );
    assert!(
        !large_index_stale.exists(),
        "canonical native indices above 999 are still owned"
    );
    for (path, bytes) in foreign.iter().zip(before) {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    let material: Value = serde_json::from_str(&read(&canonical)).unwrap();
    assert!(material
        .as_array()
        .unwrap()
        .iter()
        .any(
            |record| record["binding"]["source"] == "central:source:corpus:A24"
                && record["body"]
                    .as_str()
                    .unwrap()
                    .contains("Actual next authored observation.")
        ));
}
