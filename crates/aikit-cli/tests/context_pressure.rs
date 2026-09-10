//! W1/CASE 04 — context pressure at the engine seam.
//!
//! Driven through the production `Service::dispatch_hook`: what these assert is
//! what a session would actually receive as its window fills.

use aikit_cli::app::Service;
use aikit_core::catalog::Catalog;
use aikit_core::hooks::{HookEvent, HookEventKind};
use aikit_core::{CapsuleId, ContextId, TrustKey, TrustState};
use aikit_store::{AikitHome, TrustStore};
use serde_json::json;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;

fn write(path: &std::path::Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn capsule(home: &std::path::Path, name: &str, event: &str) {
    let root = home.join(format!("registries/personal/capsules/hook/continuity/{name}"));
    write(
        &root.join("manifest.toml"),
        &format!(
            "schema = 1\nid = \"hook/continuity/{name}\"\nkind = \"hook\"\nname = \"{name}\"\n\
             description = \"First-party continuity reaction under test.\"\n\
             [hook]\nentry = \"payload/{name}\"\nevents = [\"{event}\"]\n"
        ),
    );
    let payload = root.join(format!("payload/{name}"));
    write(&payload, "#!/bin/sh\nexit 0\n");
    let mut permissions = std::fs::metadata(&payload).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&payload, permissions).unwrap();
}

fn domain(project: &std::path::Path, id: &str, standing: bool) {
    let ordinary: String = (0..12)
        .map(|n| {
            format!(
                "\n[[guidance]]\nrule = \"ordinary rule {n} for {id}\"\n\
                 provenance = \"central:source:project:demo:ProjectCentral/user/{id}.md\"\n\
                 classification = \"ordinary\"\n"
            )
        })
        .collect();
    let standing = if standing {
        format!(
            "\n[[guidance]]\nrule = \"never ship a release without its gate\"\n\
             rationale = \"a rule that stops asserting itself under load is worse than no rule\"\n\
             provenance = \"central:source:project:demo:ProjectCentral/user/{id}.md\"\n\
             classification = \"standing\"\n"
        )
    } else {
        String::new()
    };
    write(
        &project.join(format!(".aikit/domains/{id}.toml")),
        &format!(
            r#"
schema = "aikit.knowledge-domain/v1"
id = "domain/{id}"
title = "{id}"
revision = "r1"
source = "central:source:project:demo:.aikit/domains/{id}.toml"
triggers = ["release"]

[horizon_range]
min = 2
max = 4
{ordinary}{standing}"#
        ),
    );
}

fn service(home: &std::path::Path, project: &std::path::Path, profile: &str) -> Service {
    capsule(home, "domain-activation", "UserPromptSubmit");
    capsule(home, "context-pressure", "UserPromptSubmit");
    write(&project.join(".aikit/profile.toml"), profile);
    let context = ContextId::generate();
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_owned(), context.to_string());
    let mut service =
        Service::open(AikitHome::at(home), project, |key| env.get(key).cloned()).unwrap();
    for name in ["domain-activation", "context-pressure"] {
        let id = CapsuleId::parse(&format!("hook/continuity/{name}")).unwrap();
        if let Some(capsule) = service.snapshot().get(&id) {
            let key = TrustKey::new(
                capsule.source.clone().unwrap(),
                id,
                capsule.revision.clone().unwrap(),
            );
            TrustStore::new(service.index())
                .record(&key, TrustState::Trusted, Some("test review"))
                .unwrap();
        }
    }
    service.refresh().unwrap();
    service
}

fn prompt(session: &str) -> HookEvent {
    HookEvent::new(
        "claude",
        HookEventKind::UserPromptSubmit,
        json!({"prompt": "the release is ready", "session_id": session}),
    )
}

/// Composed, with a deliberately small budget so a few recorded turns walk a
/// session through every bracket.
const COMPOSED: &str = r#"schema = 1
enable = ["hook/continuity/domain-activation", "hook/continuity/context-pressure"]

[config."hook/continuity/context-pressure"]
prompt_budget = 4
"#;

const UNCOMPOSED_PRESSURE: &str = r#"schema = 1
enable = ["hook/continuity/domain-activation"]
"#;

fn ordinary_lines(blocks: &[String]) -> usize {
    blocks
        .iter()
        .flat_map(|block| block.lines())
        .filter(|line| line.contains("ordinary rule"))
        .count()
}

/// Dispatch one first-ever turn in a fresh session scope, after recording
/// `spent` turns against it.
///
/// The scope has to be fresh for each reading: dedup and pressure both key on
/// the session, so re-prompting the same scope would suppress ordinary payload
/// as *unchanged* and a test could not tell that apart from payload the
/// bracket bounded. Seeding the count instead isolates pressure exactly.
fn turn_at(service: &Service, session: &str, spent: u32) -> Vec<String> {
    for _ in 0..spent {
        service.index().record_prompt(session).unwrap();
    }
    service.dispatch_hook(&prompt(session)).unwrap().injected
}

#[test]
fn ordinary_payload_becomes_increasingly_bounded_as_the_window_fills() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    for id in ["alpha", "beta", "gamma"] {
        domain(&project, id, false);
    }
    let service = service(tmp.path(), &project, COMPOSED);

    // budget 4: 0 spent is FRESH, 1 MODERATE, 2 DEPLETED, 3 CRITICAL.
    let fresh = ordinary_lines(&turn_at(&service, "fresh-scope", 0));
    let moderate = ordinary_lines(&turn_at(&service, "moderate-scope", 1));
    let depleted = ordinary_lines(&turn_at(&service, "depleted-scope", 2));
    let critical = ordinary_lines(&turn_at(&service, "critical-scope", 3));

    assert_eq!(fresh, 36, "three domains of twelve ordinary rules arrive whole");
    assert!(
        fresh > moderate && moderate > depleted && depleted > critical,
        "strictly increasing bound: {fresh} {moderate} {depleted} {critical}"
    );
    assert_eq!(critical, 0, "critical admits no ordinary payload");
}

#[test]
fn standing_guidance_is_still_reasserted_when_ordinary_payload_is_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    domain(&project, "alpha", true);
    let service = service(tmp.path(), &project, COMPOSED);

    let blocks = turn_at(&service, "standing-scope", 3);
    let text = blocks.join("\n");
    assert!(
        text.contains("never ship a release without its gate"),
        "standing guidance was bounded away at CRITICAL: {text}"
    );
    assert!(
        !text.contains("ordinary rule"),
        "ordinary payload survived CRITICAL: {text}"
    );
    assert!(
        text.contains("further line(s) withheld"),
        "the bound was applied silently: {text}"
    );
}

#[test]
fn an_uncomposed_pressure_capability_bounds_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    for id in ["alpha", "beta", "gamma"] {
        domain(&project, id, false);
    }
    let service = service(tmp.path(), &project, UNCOMPOSED_PRESSURE);

    // The same turn counts that walk a composed session to CRITICAL.
    let fresh = ordinary_lines(&turn_at(&service, "descope-fresh", 0));
    let critical = ordinary_lines(&turn_at(&service, "descope-critical", 3));
    assert_eq!(fresh, 36, "the domains did not activate at all");
    assert_eq!(
        fresh, critical,
        "descope law: an uncomposed capability changes nothing, including how \
         much of something else arrives"
    );
}

#[test]
fn a_harness_that_reports_consumption_is_believed_over_the_turn_count() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    domain(&project, "alpha", false);
    let service = service(tmp.path(), &project, COMPOSED);

    // Nothing spent: on turn count alone this session is FRESH. The harness
    // says the window is nearly full, and the reported figure wins.
    let event = HookEvent::new(
        "claude",
        HookEventKind::UserPromptSubmit,
        json!({
            "prompt": "the release is ready",
            "session_id": "reported-scope",
            "context_used_fraction": 0.95
        }),
    );
    let injected = service.dispatch_hook(&event).unwrap().injected;
    assert_eq!(
        ordinary_lines(&injected),
        0,
        "a reported 95% must bound like CRITICAL: {injected:?}"
    );
}

#[test]
fn the_turn_count_outlives_the_process_that_recorded_it() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    domain(&project, "alpha", false);
    let service = service(tmp.path(), &project, COMPOSED);
    for _ in 0..3 {
        service.dispatch_hook(&prompt("durable-scope")).unwrap();
    }
    // A hook dispatcher is a fresh short-lived process every turn; what makes
    // the fallback work at all is that the count is in the store, not in it.
    drop(service);
    let reopened = self::service(tmp.path(), &project, COMPOSED);
    assert_eq!(
        reopened.index().prompt_count("durable-scope").unwrap(),
        3,
        "the turn count did not survive a new service"
    );
}
