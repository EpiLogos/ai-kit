//! Guidance delivery through the hook dispatch chain, end to end.
//!
//! The defect this pins: guidance capsules resolved — enabled, trusted, active
//! — but nothing delivered them; the SessionStart injection their manifests
//! declare never reached a session. These tests drive the production
//! `Service::dispatch_hook` over a real store, a real registry and the scope
//! profiles, so what they assert is what a session would actually receive.

use std::collections::BTreeMap;

use aikit_cli::app::Service;
use aikit_core::catalog::Catalog;
use aikit_core::hooks::{HookEvent, HookEventKind, StepOutcome};
use aikit_core::id::CapsuleId;
use aikit_core::{ContextId, TrustKey, TrustState};
use aikit_store::{AikitHome, TrustStore};
use serde_json::json;

fn write(path: &std::path::Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A first-party-shaped guidance capsule: an explicit SessionStart inject and a
/// non-empty entry — the shape `scripts/verify-native-skills.py` already pins.
fn guidance_capsule(home: &std::path::Path, leaf: &str, order: i32, body: &str) {
    let root = home.join(format!("registries/personal/capsules/guidance/mode/{leaf}"));
    write(
        &root.join("manifest.toml"),
        &format!(
            "schema = 1\nid = \"guidance/mode/{leaf}\"\nkind = \"guidance\"\nname = \"{leaf}\"\n\
             description = \"Test guidance capsule.\"\n\
             [guidance]\nentry = \"payload/guidance.md\"\ninject = [\"SessionStart\"]\norder = {order}\n"
        ),
    );
    write(&root.join("payload/guidance.md"), body);
}

fn payload_path(home: &std::path::Path, leaf: &str) -> std::path::PathBuf {
    home.join(format!(
        "registries/personal/capsules/guidance/mode/{leaf}/payload/guidance.md"
    ))
}

/// Open a service with an optional global (lowest-precedence) profile and a
/// project profile, reviewing whatever guidance the combination enables.
fn service(
    home: &std::path::Path,
    project: &std::path::Path,
    global_profile: Option<&str>,
    project_profile: &str,
) -> Service {
    if let Some(global) = global_profile {
        write(&AikitHome::at(home).global_profile(), global);
    }
    write(&project.join(".aikit/profile.toml"), project_profile);
    let context = ContextId::generate();
    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_owned(), context.to_string());
    let mut service =
        Service::open(AikitHome::at(home), project, |key| env.get(key).cloned()).unwrap();

    // Review what the scopes enabled. Guidance is behaviour-changing content,
    // so an unreviewed revision is withheld at resolution; these tests prove
    // delivery, which requires a reviewed revision first.
    for leaf in ["orientation", "collaboration"] {
        let id = CapsuleId::parse(&format!("guidance/mode/{leaf}")).unwrap();
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

fn session_start() -> HookEvent {
    HookEvent::new(
        "claude",
        HookEventKind::SessionStart,
        json!({"cwd": "/tmp"}),
    )
}

const ORIENTATION_ONLY: &str = r#"schema = 1
enable = ["guidance/mode/orientation"]
"#;

const BOTH: &str = r#"schema = 1
enable = ["guidance/mode/orientation", "guidance/mode/collaboration"]
"#;

#[test]
fn a_session_start_delivers_the_declared_guidance_fragments() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    guidance_capsule(
        tmp.path(),
        "orientation",
        15,
        "Orient before acting: purpose, ground, return.",
    );
    guidance_capsule(
        tmp.path(),
        "collaboration",
        20,
        "Keep the reason for a distinction beside the distinction.",
    );
    let service = service(tmp.path(), &project, None, BOTH);

    let decision = service.dispatch_hook(&session_start()).unwrap();

    assert_eq!(
        decision.injected,
        vec![
            "Orient before acting: purpose, ground, return.",
            "Keep the reason for a distinction beside the distinction.",
        ],
        "both fragments ride the composed injection, in declared order"
    );
    // Delivery is on the record, so the resolved-but-undelivered state this
    // closes can never pass silently again.
    for leaf in ["orientation", "collaboration"] {
        let step = decision.step(&format!("guidance/mode/{leaf}")).unwrap();
        assert_eq!(step.outcome, StepOutcome::Injected);
        assert_eq!(step.phase, aikit_core::capsule::HookPhase::Inject);
    }
}

#[test]
fn an_event_the_capsule_does_not_declare_receives_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    guidance_capsule(
        tmp.path(),
        "orientation",
        15,
        "Orient before acting: purpose, ground, return.",
    );
    let service = service(tmp.path(), &project, None, ORIENTATION_ONLY);

    let prompt = HookEvent::new(
        "claude",
        HookEventKind::UserPromptSubmit,
        json!({"prompt": "hello", "cwd": "/tmp"}),
    );
    let decision = service.dispatch_hook(&prompt).unwrap();

    assert!(
        decision.injected.is_empty(),
        "the manifest declares SessionStart only: {:?}",
        decision.injected
    );
    assert!(decision.step("guidance/mode/orientation").is_none());
}

#[test]
fn a_later_scope_disabling_one_capsule_removes_only_its_fragment() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    guidance_capsule(
        tmp.path(),
        "orientation",
        15,
        "Orient before acting: purpose, ground, return.",
    );
    guidance_capsule(
        tmp.path(),
        "collaboration",
        20,
        "Keep the reason for a distinction beside the distinction.",
    );

    // The global scope enables both; the project — the later, higher-rank
    // layer — undoes one of them. Resolver rule 1 is the disable path.
    let service = service(
        tmp.path(),
        &project,
        Some(BOTH),
        r#"schema = 1
disable = ["guidance/mode/collaboration"]
"#,
    );

    let decision = service.dispatch_hook(&session_start()).unwrap();

    assert_eq!(
        decision.injected,
        vec!["Orient before acting: purpose, ground, return."],
        "the disabled capsule composes nothing, silently at its own gate"
    );
    assert!(
        decision.step("guidance/mode/collaboration").is_none(),
        "a disabled capsule joins no chain and leaves no step record"
    );
}

#[test]
fn an_untrusted_new_revision_drops_the_fragment() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    guidance_capsule(
        tmp.path(),
        "orientation",
        15,
        "Orient before acting: purpose, ground, return.",
    );
    guidance_capsule(
        tmp.path(),
        "collaboration",
        20,
        "Keep the reason for a distinction beside the distinction.",
    );
    let mut service = service(tmp.path(), &project, None, BOTH);

    let first = service.dispatch_hook(&session_start()).unwrap();
    assert_eq!(first.injected.len(), 2, "reviewed revisions deliver");

    // Edit the payload: the capsule's content revision changes, and guidance
    // changes agent behaviour, so the new revision is back to unreviewed.
    write(
        &payload_path(tmp.path(), "orientation"),
        "Revised body nobody has reviewed yet.",
    );
    service.refresh().unwrap();

    let second = service.dispatch_hook(&session_start()).unwrap();
    assert!(
        !second
            .injected
            .iter()
            .any(|fragment| fragment.contains("Revised body")),
        "an unreviewed revision is withheld from delivery"
    );
    assert_eq!(
        second.injected,
        vec!["Keep the reason for a distinction beside the distinction."],
        "only the still-trusted capsule delivers"
    );
    assert!(second.allowed, "withholding is a gate, not a denial");
}
