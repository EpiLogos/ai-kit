//! W2/CASE 07 + CASE 08 — star prompt-commands at the engine seam.
//!
//! These drive the production dispatch (`Service::dispatch_hook`) against a
//! real composition, a real trust review and a real declared domain, so what
//! they assert is what a session would actually receive: the protocol arrives
//! only because the composition selected the capability *and* armed a pack,
//! and a recognised command short-circuits the domain branch.

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
    let root = home.join(format!(
        "registries/personal/capsules/hook/continuity/{name}"
    ));
    write(
        &root.join("manifest.toml"),
        &format!(
            r#"schema = 1
id = "hook/continuity/{name}"
kind = "hook"
name = "{name}"
description = "First-party continuity reaction under test."
[hook]
entry = "payload/{name}"
events = ["{event}"]
"#
        ),
    );
    let payload = root.join(format!("payload/{name}"));
    write(&payload, "#!/bin/sh\nexit 0\n");
    let mut permissions = std::fs::metadata(&payload).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&payload, permissions).unwrap();
}

/// A project that declares one domain triggering on "release", with the given
/// profile composing whatever the case under test needs.
fn service(home: &std::path::Path, project: &std::path::Path, profile: &str) -> Service {
    capsule(home, "star-commands", "UserPromptSubmit");
    capsule(home, "domain-activation", "UserPromptSubmit");
    write(
        &project.join(".aikit/domains/release.toml"),
        r#"
schema = "aikit.knowledge-domain/v1"
id = "domain/release"
title = "Release discipline"
revision = "r1"
source = "central:source:project:demo:.aikit/domains/release.toml"
triggers = ["release"]

[horizon_range]
min = 2
max = 4

[[guidance]]
rule = "A release names the gate that proved it"
provenance = "central:source:project:demo:ProjectCentral/user/release.md"
classification = "ordinary"
"#,
    );
    write(&project.join(".aikit/profile.toml"), profile);

    let context = ContextId::generate();
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_owned(), context.to_string());
    let mut service =
        Service::open(AikitHome::at(home), project, |key| env.get(key).cloned()).unwrap();
    for name in ["star-commands", "domain-activation"] {
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

fn prompt(text: &str) -> HookEvent {
    HookEvent::new(
        "claude",
        HookEventKind::UserPromptSubmit,
        json!({"prompt": text, "session_id": "session-star"}),
    )
}

fn injected(service: &Service, text: &str) -> Vec<String> {
    service.dispatch_hook(&prompt(text)).unwrap().injected
}

const BOTH_COMPOSED_ARMED: &str = r#"schema = 1
enable = ["hook/continuity/star-commands", "hook/continuity/domain-activation"]

[config."hook/continuity/star-commands"]
packs = ["continuity-closeout"]
actor = "claude"
factory_ledger_root = "/tmp/ledger"
factory_run_ref = "run:01ARZ3NDEKTSV4RRFFQ69G5FCB"
"#;

#[test]
fn an_armed_star_command_arrives_as_its_protocol() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let blocks = injected(
        &service(tmp.path(), &project, BOTH_COMPOSED_ARMED),
        "*end wrap the slice up",
    );
    let protocol = blocks
        .iter()
        .find(|block| block.starts_with("[continuity/star-commands] *end"))
        .expect("the *end protocol arrives");
    assert!(protocol.contains("projectcentral.now.return"));
    assert!(protocol.contains("factory development observe /tmp/ledger"));
    assert!(protocol.contains("aikit continuity closeout verify"));
    assert!(protocol.contains("you wrote: wrap the slice up"));
}

#[test]
fn a_recognised_command_short_circuits_the_domain_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let service = service(tmp.path(), &project, BOTH_COMPOSED_ARMED);

    // The same word arms the domain in both prompts; only the star prompt
    // suppresses it, so this is precedence, not a disabled domain.
    let with_star = injected(&service, "*end the release is done");
    assert!(with_star
        .iter()
        .any(|block| block.starts_with("[continuity/star-commands] *end")));
    assert!(
        !with_star
            .iter()
            .any(|block| block.contains("[continuity/domain-activation]")),
        "domain guidance must not ride along with an explicit protocol: {with_star:?}"
    );

    let without_star = injected(&service, "the release is done");
    assert!(
        without_star
            .iter()
            .any(|block| block.contains("[continuity/domain-activation]")),
        "the domain still activates on its own trigger: {without_star:?}"
    );
}

#[test]
fn the_default_composition_leaves_star_tokens_as_ordinary_prose() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let blocks = injected(
        &service(tmp.path(), &project, "schema = 1\n"),
        "*end wrap the slice up",
    );
    assert!(
        !blocks
            .iter()
            .any(|block| block.contains("continuity/star-commands")),
        "descope law: nothing beyond the floor without a composition: {blocks:?}"
    );
}

#[test]
fn composing_the_capability_without_arming_a_pack_recognises_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let profile = "schema = 1\nenable = [\"hook/continuity/star-commands\"]\n";
    let blocks = injected(
        &service(tmp.path(), &project, profile),
        "*end wrap the slice up",
    );
    assert!(
        !blocks
            .iter()
            .any(|block| block.contains("continuity/star-commands")),
        "arming a pack is a separate act from composing the capability: {blocks:?}"
    );
}

#[test]
fn an_unknown_pack_is_disclosed_as_a_warning_rather_than_silently_arming_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let profile = r#"schema = 1
enable = ["hook/continuity/star-commands"]

[config."hook/continuity/star-commands"]
packs = ["continuity-closeut"]
"#;
    let decision = service(tmp.path(), &project, profile)
        .dispatch_hook(&prompt("*end wrap up"))
        .unwrap();
    assert!(
        decision
            .warnings
            .iter()
            .any(|warning| warning.contains("unknown pack `continuity-closeut`")),
        "a typo that disarms a protocol must be visible: {:?}",
        decision.warnings
    );
    assert!(decision
        .injected
        .iter()
        .all(|block| !block.contains("continuity/star-commands")));
}

#[test]
fn stacked_commands_each_arrive_once_in_the_order_they_were_written() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let blocks = injected(
        &service(tmp.path(), &project, BOTH_COMPOSED_ARMED),
        "*fork the pressure brackets *end and close out",
    );
    let stars: Vec<&String> = blocks
        .iter()
        .filter(|block| block.starts_with("[continuity/star-commands]"))
        .collect();
    assert_eq!(stars.len(), 2, "{blocks:?}");
    assert!(stars[0].starts_with("[continuity/star-commands] *fork"));
    assert!(stars[1].starts_with("[continuity/star-commands] *end"));
}
