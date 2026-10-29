//! CASE 18's product surface under regression.
//!
//! The shape module carried the whole structural floor for a release with no
//! caller, and its only evidence was a unit test. Giving it a CLI verb is what
//! made the case closable — so the verb itself has to be held by tests, or the
//! case rests on a manual run somebody did once.

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

fn run(cwd: &Path, args: &[&str], body: Option<&str>) -> (i32, Value) {
    let bin = assert_cmd::cargo::cargo_bin("aikit");
    let mut command = Command::new(&bin);
    command
        .args(args)
        .arg("--json")
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if body.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().expect("aikit runs");
    if let Some(body) = body {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?} must emit a JSON envelope; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(-1), envelope)
}

const DIRECT: [&str; 6] = [
    "continuity",
    "criterion",
    "delineation",
    "arbitration",
    "con-text",
    "resolution",
];
const CONJUGATE: [&str; 6] = [
    "indeterminacy",
    "distinction",
    "difference",
    "crisis",
    "diaphaneity",
    "reconciliation",
];

/// The Arbitration cluster's own twelvefold, in the corpus's own order.
fn frame_body(frame_ref: &str, shape_ref: &str, include_direct: bool) -> String {
    let anchor = "wiki:node:t09:arbitration-whole";
    let mut members: Vec<String> = Vec::new();
    if include_direct {
        for (position, name) in DIRECT.iter().enumerate() {
            members.push(format!(
                r#"{{"ref":"wiki:node:t09:{name}","position":{position},"conjugate":false}}"#
            ));
        }
    }
    for (position, name) in CONJUGATE.iter().enumerate() {
        members.push(format!(
            r#"{{"ref":"wiki:node:t09:{name}","position":{position},"conjugate":true}}"#
        ));
    }
    let generated: Vec<String> = (0..6)
        .map(|i| format!(r#""{i}":"wiki:node:t09:generated/g{i}""#))
        .collect();
    format!(
        r#"{{"profile":"okf-wiki/v1","object":"frame","ref":"{frame_ref}","revision":1,
             "provenance":[],"title":"Arbitration","space_refs":[],"member_refs":[],
             "external_refs":[],
             "constellations":[{{
               "anchor_ref":"{anchor}",
               "members":[{members}],
               "returns":[{{"through_anchor_ref":"{anchor}","ground_ref":"{anchor}",
                            "ground_kind":"own"}}],
               "aikit.ql-shape/v1":{{"shape_ref":"{shape_ref}","grain":"twelvefold",
                                     "generated":{{{generated}}}}}
             }}]}}"#,
        members = members.join(","),
        generated = generated.join(",")
    )
}

fn wiki_file(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("shapes.json");
    write(&path, "{\n  \"objects\": []\n}\n");
    path
}

/// A well-formed twelvefold declares, then validates against the contract.
#[test]
fn a_declared_constellation_validates_against_the_pinned_contract() {
    let work = TempDir::new().unwrap();
    let file = wiki_file(work.path());
    let body = frame_body(
        "wiki:frame:arbitration",
        "ql:structural:2.0.0:field:A:2:D3",
        true,
    );

    let (code, envelope) = run(
        work.path(),
        &[
            "wiki-shape",
            "declare",
            "wiki:frame:arbitration",
            "--file",
            file.to_str().unwrap(),
        ],
        Some(&body),
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["constellations"], 1, "{envelope}");

    let (code, envelope) = run(
        work.path(),
        &["wiki-shape", "validate", "--file", file.to_str().unwrap()],
        None,
    );
    assert_eq!(
        code, 0,
        "a contract-conforming constellation validates: {envelope}"
    );
}

/// Conjugate-requires-direct comes from the contract, and the refusal must
/// happen at declare time — before a malformed shape is written, not after.
#[test]
fn a_conjugate_without_its_direct_is_refused_at_declare_time() {
    let work = TempDir::new().unwrap();
    let file = wiki_file(work.path());
    let body = frame_body(
        "wiki:frame:conjugate-only",
        "ql:structural:2.0.0:field:A:2:D3",
        false,
    );

    let (code, envelope) = run(
        work.path(),
        &[
            "wiki-shape",
            "declare",
            "wiki:frame:conjugate-only",
            "--file",
            file.to_str().unwrap(),
        ],
        Some(&body),
    );
    assert_ne!(
        code, 0,
        "a conjugate without its direct must be refused: {envelope}"
    );
    assert!(
        envelope["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .starts_with("knowledge."),
        "the refusal is a typed knowledge error: {envelope}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap().trim(),
        "{\n  \"objects\": []\n}".trim(),
        "a refused declaration leaves the file untouched"
    );
}

/// The openness law: the theory may evolve its shape refs without an engine
/// release. An unrecognised future ref is preserved, not rejected.
#[test]
fn an_unrecognised_future_shape_ref_is_preserved_not_refused() {
    let work = TempDir::new().unwrap();
    let file = wiki_file(work.path());
    let body = frame_body(
        "wiki:frame:evolved",
        "ql:structural:99.0.0:field:A:2:D3",
        true,
    );

    let (code, envelope) = run(
        work.path(),
        &[
            "wiki-shape",
            "declare",
            "wiki:frame:evolved",
            "--file",
            file.to_str().unwrap(),
        ],
        Some(&body),
    );
    assert_eq!(
        code, 0,
        "a future shape ref does not need an engine release: {envelope}"
    );

    let (code, envelope) = run(
        work.path(),
        &["wiki-shape", "validate", "--file", file.to_str().unwrap()],
        None,
    );
    assert_eq!(code, 0, "and it still validates: {envelope}");
}

/// The 6+6′ compression through the 0 // 1 trinity, over the corpus's own
/// terms: position 3 is `arbitration` // `crisis`.
#[test]
fn the_compression_yields_the_six_plus_six_prime_trinity() {
    let work = TempDir::new().unwrap();
    let file = wiki_file(work.path());
    let body = frame_body(
        "wiki:frame:arbitration",
        "ql:structural:2.0.0:field:A:2:D3",
        true,
    );
    let (code, _) = run(
        work.path(),
        &[
            "wiki-shape",
            "declare",
            "wiki:frame:arbitration",
            "--file",
            file.to_str().unwrap(),
        ],
        Some(&body),
    );
    assert_eq!(code, 0);

    let (code, envelope) = run(
        work.path(),
        &[
            "wiki-shape",
            "compress",
            "wiki:node:t09:arbitration-whole",
            "--file",
            file.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "{envelope}");
    let rendered = envelope.to_string();
    assert!(
        rendered.contains("arbitration") && rendered.contains("crisis"),
        "position 3's direct and conjugate both ride the compression: {envelope}"
    );
}

// ---------------------------------------------------------------------------
// O-I #742: the Canvas's Form page — a draft checked before it is written, a
// change of roles on a declared frame, and the Wiki's projection of one
// constellation for QL's Technē reading.
// ---------------------------------------------------------------------------

fn declare(
    work: &Path,
    file: &Path,
    frame_ref: &str,
    body: &str,
    expected: Option<u64>,
) -> (i32, Value) {
    let revision = expected.map(|value| value.to_string());
    let mut args = vec![
        "wiki-shape",
        "declare",
        frame_ref,
        "--file",
        file.to_str().unwrap(),
    ];
    if let Some(revision) = revision.as_deref() {
        args.extend(["--expected-revision", revision]);
    }
    run(work, &args, Some(body))
}

fn check_draft(work: &Path, file: &Path, body: &str, expected: Option<u64>) -> (i32, Value) {
    let revision = expected.map(|value| value.to_string());
    let mut args = vec![
        "wiki-shape",
        "validate",
        "--file",
        file.to_str().unwrap(),
        "--stdin",
    ];
    if let Some(revision) = revision.as_deref() {
        args.extend(["--expected-revision", revision]);
    }
    run(work, &args, Some(body))
}

/// The same twelvefold with the direct members of positions 1 and 2 swapped: a change of roles, nothing else.
fn swapped_body(frame_ref: &str) -> String {
    frame_body(frame_ref, "ql:structural:2.0.0:field:A:2:D3", true)
        .replace("t09:criterion\"", "t09:SWAP\"")
        .replace("t09:delineation\"", "t09:criterion\"")
        .replace("t09:SWAP\"", "t09:delineation\"")
}

fn position_of(envelope: &Value, member: &str) -> Value {
    envelope["data"]["frames"][0]["constellations"][0]["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["ref"] == member)
        .map(|row| row["position"].clone())
        .unwrap_or(Value::Null)
}

/// A draft frame is checked through the same floor `declare` enforces, and nothing is written.
#[test]
fn a_draft_frame_is_checked_without_a_write() {
    let work = TempDir::new().unwrap();
    let file = wiki_file(work.path());
    let before = fs::read(&file).unwrap();
    let body = frame_body("wiki:frame:draft", "ql:structural:2.0.0:field:A:2:D3", true);

    let (code, envelope) = check_draft(work.path(), &file, &body, None);
    assert_eq!(code, 0, "{envelope}");
    let data = &envelope["data"];
    assert_eq!(data["command"], "wiki-shape.validate");
    assert_eq!(data["valid"], true, "{envelope}");
    assert_eq!(data["draft"]["ref"], "wiki:frame:draft");
    assert_eq!(data["draft"]["would"], "create");
    assert_eq!(data["draft"]["revision"], 1);
    assert_eq!(data["frames"][0]["frame"], "wiki:frame:draft");
    assert_eq!(
        data["frames"][0]["constellations"][0]["grain"],
        "twelvefold"
    );
    assert_eq!(
        fs::read(&file).unwrap(),
        before,
        "a draft check writes nothing"
    );

    // A draft the floor refuses comes back as the owner's errors, still with no write.
    let refused = frame_body(
        "wiki:frame:draft",
        "ql:structural:2.0.0:field:A:2:D3",
        false,
    );
    let (code, envelope) = check_draft(work.path(), &file, &refused, None);
    assert_ne!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["valid"], false, "{envelope}");
    let errors = envelope["data"]["errors"].to_string();
    assert!(errors.contains("conjugate"), "{envelope}");
    assert_eq!(fs::read(&file).unwrap(), before);
}

/// A draft of a frame the file already holds is a change: it names the revision it changes, or it is refused as
/// `declare` would refuse it.
#[test]
fn a_draft_of_a_held_frame_names_the_revision_it_changes() {
    let work = TempDir::new().unwrap();
    let file = wiki_file(work.path());
    let body = frame_body("wiki:frame:held", "ql:structural:2.0.0:field:A:2:D3", true);
    assert_eq!(
        declare(work.path(), &file, "wiki:frame:held", &body, None).0,
        0
    );
    let before = fs::read(&file).unwrap();

    let (code, envelope) = check_draft(work.path(), &file, &swapped_body("wiki:frame:held"), None);
    assert_ne!(code, 0, "{envelope}");
    assert!(
        envelope["data"]["errors"]
            .to_string()
            .contains("already exists"),
        "{envelope}"
    );

    let (code, envelope) = check_draft(
        work.path(),
        &file,
        &swapped_body("wiki:frame:held"),
        Some(1),
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["draft"]["would"], "update");
    assert_eq!(envelope["data"]["draft"]["basis_revision"], 1);
    assert_eq!(envelope["data"]["draft"]["revision"], 2);
    assert_eq!(position_of(&envelope, "wiki:node:t09:delineation"), 1);

    let (code, envelope) = check_draft(
        work.path(),
        &file,
        &swapped_body("wiki:frame:held"),
        Some(7),
    );
    assert_ne!(code, 0, "{envelope}");
    assert!(
        envelope["data"]["errors"].to_string().contains("revision"),
        "{envelope}"
    );
    assert_eq!(fs::read(&file).unwrap(), before);
}

/// A change of roles on a declared frame is the same `declare`, on the revision it read: written, read back, and
/// refused when the frame moved since.
#[test]
fn declare_changes_the_roles_of_a_held_frame_on_its_revision() {
    let work = TempDir::new().unwrap();
    let file = wiki_file(work.path());
    let body = frame_body("wiki:frame:roles", "ql:structural:2.0.0:field:A:2:D3", true);
    assert_eq!(
        declare(work.path(), &file, "wiki:frame:roles", &body, None).0,
        0
    );

    // Without a revision, declare still only creates.
    let (code, envelope) = declare(
        work.path(),
        &file,
        "wiki:frame:roles",
        &swapped_body("wiki:frame:roles"),
        None,
    );
    assert_ne!(code, 0, "{envelope}");
    assert!(
        envelope.to_string().contains("already exists"),
        "{envelope}"
    );

    let (code, envelope) = declare(
        work.path(),
        &file,
        "wiki:frame:roles",
        &swapped_body("wiki:frame:roles"),
        Some(1),
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["outcome"]["changed"], true, "{envelope}");
    assert_eq!(envelope["data"]["revision"], 2, "{envelope}");

    let (code, envelope) = run(
        work.path(),
        &["wiki-shape", "validate", "--file", file.to_str().unwrap()],
        None,
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["frames"][0]["revision"], 2);
    assert_eq!(position_of(&envelope, "wiki:node:t09:delineation"), 1);
    assert_eq!(position_of(&envelope, "wiki:node:t09:criterion"), 2);

    // The revision it read is gone: refused, the file as it was.
    let before = fs::read(&file).unwrap();
    let (code, envelope) = declare(work.path(), &file, "wiki:frame:roles", &body, Some(1));
    assert_ne!(code, 0, "{envelope}");
    assert_eq!(
        envelope["error"]["code"], "knowledge.wiki_revision_conflict",
        "{envelope}"
    );
    assert_eq!(fs::read(&file).unwrap(), before);
    // The fold reads the changed roles.
    let (code, envelope) = run(
        work.path(),
        &[
            "wiki-shape",
            "compress",
            "wiki:node:t09:arbitration-whole",
            "--file",
            file.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(
        envelope["data"]["composites"][1]["direct_ref"], "wiki:node:t09:delineation",
        "{envelope}"
    );
}

/// A frame written through the construction owner (`wiki-construct apply`) changes through it: `declare` never
/// replaces its participations.
#[test]
fn declare_does_not_rewrite_a_construction_frame() {
    let work = TempDir::new().unwrap();
    let file = wiki_file(work.path());
    let constructed = frame_body("wiki:frame:built", "ql:structural:2.0.0:field:A:2:D3", true)
        .replacen(
            "\"external_refs\":[],",
            "\"external_refs\":[],\"aikit.constellation/v1\":{\"title\":\"x\"},",
            1,
        );
    assert_eq!(
        declare(work.path(), &file, "wiki:frame:built", &constructed, None).0,
        0,
        "seeded"
    );
    let (code, envelope) = declare(
        work.path(),
        &file,
        "wiki:frame:built",
        &swapped_body("wiki:frame:built"),
        Some(1),
    );
    assert_ne!(code, 0, "{envelope}");
    assert!(
        envelope.to_string().contains("wiki-construct"),
        "{envelope}"
    );
}

/// The Wiki's own projection of one constellation for QL's Technē reading (`ql techne reading`): its frame as the
/// target, at its revision and snapshot, with the QL shape binding the constellation keeps.
#[test]
fn target_projects_a_constellation_for_the_technē_reading() {
    let work = TempDir::new().unwrap();
    let file = wiki_file(work.path());
    let binding = r#""ql-mef/shape-binding/v1":{"subject_ref":"wiki:frame:read","shape_ref":"ql:shape:1.0.0:constellation:twelvefold","whole_ref":"wiki:node:t09:arbitration-whole","basis_refs":[],"members":[],"return_refs":[],"provenance":{"caller_ref":"c","source_ref":"s","standing_ref":"proposed"}},"#;
    let body = frame_body("wiki:frame:read", "ql:structural:2.0.0:field:A:2:D3", true).replacen(
        "\"aikit.ql-shape/v1\"",
        &format!("{binding}\"aikit.ql-shape/v1\""),
        1,
    );
    assert_eq!(
        declare(work.path(), &file, "wiki:frame:read", &body, None).0,
        0
    );

    let (code, envelope) = run(
        work.path(),
        &[
            "wiki-shape",
            "target",
            "wiki:node:t09:arbitration-whole",
            "--file",
            file.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(code, 0, "{envelope}");
    let target = &envelope["data"]["target"];
    assert_eq!(envelope["data"]["command"], "wiki-shape.target");
    assert_eq!(target["kind"], "frame");
    assert_eq!(target["target_ref"], "wiki:frame:read");
    assert_eq!(target["target_revision"], 1);
    assert!(
        target["target_snapshot_hash"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"),
        "{envelope}"
    );
    assert_eq!(
        target["extensions"]["ql-mef/shape-binding/v1"]["whole_ref"],
        "wiki:node:t09:arbitration-whole"
    );

    let (code, envelope) = run(
        work.path(),
        &[
            "wiki-shape",
            "target",
            "wiki:node:absent",
            "--file",
            file.to_str().unwrap(),
        ],
        None,
    );
    assert_ne!(code, 0, "{envelope}");
    assert_eq!(
        envelope["error"]["code"], "knowledge.wiki_shape_anchor_not_found",
        "{envelope}"
    );
}
