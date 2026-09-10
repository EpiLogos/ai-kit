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
