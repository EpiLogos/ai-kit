//! `aikit knowledge impact`, `aikit knowledge reflection verify` and `aikit
//! knowledge reflection laws`: the library's own deterministic Living
//! Knowledge closure and reflection-law verdict, asked for readings and for a
//! supplied law, and the reflection laws a project declares. Nothing is
//! recorded, no Agent or model is invoked, and the horizon and the law are
//! owner seams the caller supplies or the project writes.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use aikit_cli::app::Service;
use aikit_core::project_reflection::ReflectionLaw;
use aikit_core::resource::{ResourceRef, SourceRef, SourceRevision};
use aikit_core::{
    KnowledgeChangeHorizon, KnowledgeChangeKind, KnowledgeFreshness, KnowledgeObservedSource,
    KnowledgeSourceChange,
};
use aikit_store::home::AikitHome;
use serde_json::{json, Value};
use tempfile::TempDir;

const NODE: &str = "wiki:node:staged/impact-subject";
const SOURCE: &str = "source:file:impact-note";
const PROJECT_ID: &str = "project/test-knowledge-impact";

fn write(path: &std::path::Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A private AIKIT_HOME and a project whose Semantic Wiki carries one node
/// with exact source-revision provenance (the flow-cognition fixture's shape).
fn project(temp: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let home = temp.path().join("aikit-home");
    let project = temp.path().join("project");
    write(&project.join(".aikit/profile.toml"), "schema = 1\n");
    write(&home.join("scopes/global/profile.toml"), "schema = 1\n");
    write(
        &project.join("ProjectCentral/project.json"),
        &format!(
            r#"{{"schema": "central.project/v1", "project_id": "{PROJECT_ID}",
              "human_source": "ProjectCentral/user",
              "wiki": {{"profile": "okf-wiki/v1", "source": "ProjectCentral/agents/wiki/wiki.json"}}}}"#
        ),
    );
    write(
        &project.join("ProjectCentral/agents/wiki/wiki.json"),
        r#"{"objects": []}"#,
    );
    write(
        &project.join("ProjectCentral/user/position.md"),
        "# Ground\n",
    );
    write(
        &project.join("semantic-wiki.json"),
        &format!(
            r#"{{"objects": [{{"profile": "okf-wiki/v1", "object": "node", "ref": "{NODE}",
              "revision": 1, "provenance": [{{"source_ref": "{SOURCE}", "source_revision": "r1"}}],
              "type": "concept", "title": "Impact subject", "space_refs": [],
              "source_refs": ["{SOURCE}"]}}]}}"#
        ),
    );
    write(
        &project.join("source-material.json"),
        &format!(
            r#"{{"binding": {{"source": "{SOURCE}", "revision": "r1", "title": "Impact note",
              "tags": [], "visibility": "public", "owners": [], "media_type": "text/markdown",
              "metadata": {{}}}}, "body": "The note the subject rests on."}}"#
        ),
    );
    (home, project)
}

fn horizon(observed: &str, changed: bool) -> KnowledgeChangeHorizon {
    KnowledgeChangeHorizon {
        provider: "owner-seam/test".into(),
        cursor: 4,
        sources: vec![KnowledgeObservedSource {
            source: SourceRef::parse(SOURCE).unwrap(),
            revision: Some(SourceRevision::parse(observed).unwrap()),
            available: true,
        }],
        changes: if changed {
            vec![KnowledgeSourceChange {
                cursor: 4,
                world_ref: "world/test".into(),
                source: SourceRef::parse(SOURCE).unwrap(),
                roles: vec![],
                provenance: "owner seam observation (test fixture)".into(),
                standing: "observed".into(),
                before_revision: Some(SourceRevision::parse("r1").unwrap()),
                after_revision: Some(SourceRevision::parse(observed).unwrap()),
                kind: KnowledgeChangeKind::Modified,
                agent_retrieval_allowed: true,
            }]
        } else {
            vec![]
        },
    }
}

#[test]
fn impact_names_the_readings_a_changed_source_moved_and_only_the_ones_asked() {
    let temp = TempDir::new().unwrap();
    let (home, project) = project(&temp);
    let service = Service::open(AikitHome::at(home), &project, |_| None).unwrap();
    let node = ResourceRef::parse(NODE).unwrap();

    let quiet = service
        .knowledge_impact(&horizon("r1", false), &[], 8, 512)
        .unwrap();
    assert_eq!(quiet.version, "aikit.knowledge-impact-reading/v1");
    assert!(
        quiet.impact.direct.affected.is_empty(),
        "an unmoved basis is fresh"
    );
    assert!(
        quiet.source_dependencies >= 1,
        "the Wiki's provenance is a manifest"
    );
    assert!(!quiet.automatic_agent_or_model_invocation);

    let moved = service
        .knowledge_impact(&horizon("r2", true), &[], 8, 512)
        .unwrap();
    let row = moved
        .impact
        .direct
        .affected
        .iter()
        .find(|row| row.resource == node)
        .expect("the subject rests on the moved source");
    assert_eq!(row.freshness, KnowledgeFreshness::BasisChanged);
    assert_eq!(moved.impact.direct.changed_sources.len(), 1);

    let asked = service
        .knowledge_impact(&horizon("r2", true), std::slice::from_ref(&node), 8, 512)
        .unwrap();
    assert_eq!(asked.readings, vec![node.clone()]);
    assert!(asked
        .impact
        .direct
        .affected
        .iter()
        .all(|row| row.resource == node));
    assert!(!asked.impact.direct.affected.is_empty());

    let other = ResourceRef::parse("wiki:node:staged/unrelated").unwrap();
    let elsewhere = service
        .knowledge_impact(&horizon("r2", true), &[other], 8, 512)
        .unwrap();
    assert!(elsewhere.impact.direct.affected.is_empty());
    assert!(elsewhere.impact.transitive.is_empty());
    assert_eq!(
        elsewhere.impact.direct.changed_sources.len(),
        1,
        "the changed sources stay as computed"
    );

    assert!(service
        .knowledge_impact(&horizon("r2", true), &[], 0, 512)
        .is_err());
}

#[test]
fn a_reflection_law_is_verified_against_the_project_maps_bindings() {
    let temp = TempDir::new().unwrap();
    let (home, project) = project(&temp);
    let service = Service::open(AikitHome::at(home), &project, |_| None).unwrap();
    let law: ReflectionLaw = serde_json::from_value(json!({
        "id": "law/impact-subject",
        "unique_implementation": true,
        "mappings": [{
            "coordinate": "subject",
            "semantic": NODE,
            "implementation": "code:symbol:impact::subject",
            "relation": "implemented-by"
        }],
        "constitutive_relations": []
    }))
    .unwrap();
    let verdict = service.reflection_verdict(&law).unwrap();
    assert_eq!(verdict.version, "aikit.reflection-verdict/v1");
    assert_eq!(verdict.law, "law/impact-subject");
    assert!(
        !verdict.verification.passed,
        "no binding carries the mapping"
    );
    assert!(verdict
        .verification
        .issues
        .iter()
        .any(|issue| serde_json::to_value(issue.kind).unwrap() == "missing"));
    assert!(!verdict.automatic_agent_or_model_invocation);
}

fn aikit(
    home: &std::path::Path,
    project: &std::path::Path,
    args: &[&str],
    stdin: &str,
) -> (bool, Value, String) {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .args(args)
        .arg("--json")
        .env("AIKIT_HOME", home)
        .env("HOME", home)
        .current_dir(project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let envelope = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
    (
        output.status.success(),
        envelope,
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn the_binary_reads_the_horizon_and_the_law_from_stdin() {
    let temp = TempDir::new().unwrap();
    let (home, project) = project(&temp);
    let horizon = serde_json::to_string(&horizon("r2", true)).unwrap();
    let (ok, envelope, stderr) = aikit(
        &home,
        &project,
        &["knowledge", "impact", "--horizon", "-", NODE],
        &horizon,
    );
    assert!(ok, "{envelope} {stderr}");
    let data = &envelope["data"];
    assert_eq!(data["version"], "aikit.knowledge-impact-reading/v1");
    assert_eq!(data["readings"], json!([NODE]));
    assert_eq!(data["impact"]["direct"]["affected"][0]["resource"], NODE);
    assert_eq!(
        data["impact"]["direct"]["affected"][0]["freshness"],
        "basis-changed"
    );

    let (ok, envelope, _) = aikit(
        &home,
        &project,
        &["knowledge", "reflection", "verify", "-"],
        &json!({"id": "law/x", "unique_implementation": false, "mappings": [],
            "constitutive_relations": []})
        .to_string(),
    );
    assert!(ok, "{envelope}");
    assert_eq!(envelope["data"]["version"], "aikit.reflection-verdict/v1");
    assert_eq!(envelope["data"]["verification"]["passed"], true);

    let (ok, envelope, _) = aikit(
        &home,
        &project,
        &["knowledge", "impact", "--horizon", "-"],
        "{not json",
    );
    assert!(!ok);
    assert_eq!(envelope["error"]["code"], "knowledge.horizon_unreadable");
}

/// A law the project writes in its own `.aikit/reflection-laws/` (O-I #786):
/// the meaning a source names, the code that holds it, and the source
/// revision the law was last read against the work at.
fn declared_law(id: &str) -> Value {
    json!({
        "id": id,
        "source": "central:source:project:test:ProjectCentral/docs/2-design/impact.md",
        "source_revision": "central.content-fnv1a64/v1:12:0000000000000001",
        "unique_implementation": true,
        "mappings": [{
            "coordinate": "subject",
            "semantic": NODE,
            "implementation": "code:symbol:impact::subject",
            "relation": "implemented-by"
        }],
        "constitutive_relations": []
    })
}

#[test]
fn a_project_lists_the_reflection_laws_it_declares_and_names_each_it_cannot_read() {
    let temp = TempDir::new().unwrap();
    let (home, project) = project(&temp);
    let service = Service::open(AikitHome::at(home.clone()), &project, |_| None).unwrap();
    let none = service.reflection_laws().unwrap();
    assert_eq!(none.version, "aikit.reflection-laws/v1");
    assert_eq!(none.declared_in, ".aikit/reflection-laws");
    assert!(
        none.laws.is_empty() && none.unreadable.is_empty(),
        "a project that declares none lists none"
    );

    let laws = project.join(".aikit/reflection-laws");
    write(
        &laws.join("b-impact.json"),
        &declared_law("law/impact-subject").to_string(),
    );
    write(
        &laws.join("a-second.json"),
        &declared_law("law/second").to_string(),
    );
    write(&laws.join("c-broken.json"), "{not json");
    write(
        &laws.join("d-again.json"),
        &declared_law("law/impact-subject").to_string(),
    );
    write(&laws.join("notes.md"), "Not a law.\n");
    let service = Service::open(AikitHome::at(home), &project, |_| None).unwrap();
    let listing = service.reflection_laws().unwrap();
    let paths = listing
        .laws
        .iter()
        .map(|row| row.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            ".aikit/reflection-laws/a-second.json",
            ".aikit/reflection-laws/b-impact.json"
        ],
        "by path; only .json files"
    );
    assert_eq!(listing.laws[1].law.id, "law/impact-subject");
    assert_eq!(
        listing.laws[1].law.source_revision.as_deref(),
        Some("central.content-fnv1a64/v1:12:0000000000000001"),
        "the revision the source was last read against the work at"
    );
    let unreadable = listing
        .unreadable
        .iter()
        .map(|row| (row.path.as_str(), row.error.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(unreadable.len(), 2, "{unreadable:?}");
    assert_eq!(unreadable[0].0, ".aikit/reflection-laws/c-broken.json");
    assert_eq!(unreadable[1].0, ".aikit/reflection-laws/d-again.json");
    assert!(
        unreadable[1].1.contains("b-impact.json"),
        "a second law of the same id names the first: {}",
        unreadable[1].1
    );
    assert!(!listing.automatic_agent_or_model_invocation);
    // Each law listed is one the verdict takes as it stands.
    let verdict = service.reflection_verdict(&listing.laws[1].law).unwrap();
    assert_eq!(verdict.law, "law/impact-subject");
    assert!(!verdict.verification.passed);
}

#[test]
fn the_binary_lists_the_declared_laws_and_verifies_one_as_listed() {
    let temp = TempDir::new().unwrap();
    let (home, project) = project(&temp);
    write(
        &project.join(".aikit/reflection-laws/impact.json"),
        &declared_law("law/impact-subject").to_string(),
    );
    let (ok, envelope, stderr) = aikit(&home, &project, &["knowledge", "reflection", "laws"], "");
    assert!(ok, "{envelope} {stderr}");
    let data = &envelope["data"];
    assert_eq!(data["version"], "aikit.reflection-laws/v1");
    assert_eq!(data["declared_in"], ".aikit/reflection-laws");
    assert_eq!(
        data["laws"][0]["path"],
        ".aikit/reflection-laws/impact.json"
    );
    assert_eq!(data["laws"][0]["law"]["id"], "law/impact-subject");
    assert_eq!(data["unreadable"], json!([]));
    let (ok, envelope, _) = aikit(
        &home,
        &project,
        &["knowledge", "reflection", "verify", "-"],
        &data["laws"][0]["law"].to_string(),
    );
    assert!(ok, "{envelope}");
    assert_eq!(envelope["data"]["law"], "law/impact-subject");
    assert_eq!(
        envelope["data"]["verification"]["issues"][0]["kind"],
        "missing"
    );
}
