//! Conformance for the capability registry committed with AIKit.
//!
//! Unit fixtures prove the loader's mechanics, but they cannot prove that the
//! product's own shipped manifests still inhabit the vocabulary the loader
//! accepts. A committed invalid capsule must therefore fail CI rather than
//! becoming a runtime `RegistryProblem` on the user's machine.

use std::path::PathBuf;

use aikit_core::capsule::{FailurePolicy, HookPhase};
use aikit_core::catalog::Catalog;
use aikit_core::{CapsuleId, RegistrySource};
use aikit_store::registry::load_registry;

#[test]
fn committed_builtin_registry_has_no_manifest_problems() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry_root = repo_root.join("registry");

    let load = load_registry(&registry_root, RegistrySource::personal()).unwrap();

    assert!(
        !load.catalog.is_empty(),
        "the committed AIKit registry should contain capabilities"
    );
    assert!(
        load.problems.is_empty(),
        "the committed AIKit registry contains invalid manifests: {:#?}",
        load.problems
    );
}

#[test]
fn the_knowledge_route_hook_declares_an_inject_phase_content_step() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry_root = repo_root.join("registry");

    let load = load_registry(&registry_root, RegistrySource::personal()).unwrap();
    let capsule = load
        .catalog
        .get(&CapsuleId::parse("hook/aikit/knowledge-route").unwrap())
        .expect("the knowledge-route hook capsule ships with the registry");

    let hook = capsule
        .hook()
        .expect("the knowledge-route capsule declares a [hook] section");
    assert_eq!(
        hook.phase,
        HookPhase::Inject,
        "a content hook must sit in the inject phase; a gate cannot carry stdout"
    );
    assert!(
        hook.events.iter().any(|e| e == "SessionStart"),
        "steering belongs at session start: {:?}",
        hook.events
    );
    assert!(
        hook.events.iter().any(|e| e == "UserPromptSubmit"),
        "and on every prompt, so the steer survives long sessions: {:?}",
        hook.events
    );
    assert_eq!(
        hook.failure,
        FailurePolicy::Open,
        "a steering hint must never fail a session when it cannot run"
    );
}
