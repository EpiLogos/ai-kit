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

#[test]
fn the_pi_extension_carrier_declares_a_transport_not_a_chain_step() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry_root = repo_root.join("registry");

    let load = load_registry(&registry_root, RegistrySource::personal()).unwrap();
    let capsule = load
        .catalog
        .get(&CapsuleId::parse("hook/aikit/pi-extension-carrier").unwrap())
        .expect("the pi extension carrier ships with the registry");

    let hook = capsule
        .hook()
        .expect("the carrier declares a [hook] section");
    assert!(
        hook.events.is_empty(),
        "the carrier FEEDS the dispatcher for pi; a declared event would put it in \
         the chains it feeds, double-firing and recursing: {:?}",
        hook.events
    );
    assert_eq!(
        hook.phase,
        HookPhase::Observe,
        "the phase is a declaration of least authority: the carrier is never a chain \
         step, and observe can neither deny nor block an event if it ever were one"
    );
    assert_eq!(
        hook.failure,
        FailurePolicy::Open,
        "the carrier must never fail a session because AIKit is absent"
    );
    assert_eq!(
        hook.timeout.as_ref().map(|t| t.as_duration()),
        Some(std::time::Duration::from_secs(15)),
        "a hung dispatcher degrades to a no-op on a bounded timeout"
    );
    assert!(
        capsule.targets.is_empty(),
        "the carrier rides the pi hooks profile, not a target filter"
    );

    let payload = capsule
        .root
        .as_ref()
        .expect("the registry supplies the capsule root")
        .join(&hook.entry);
    let payload_text =
        std::fs::read_to_string(&payload).expect("the carrier payload ships beside its manifest");
    for seam in [
        "aikit hook dispatch",
        "session_start",
        "tool_call",
        "before_agent_start",
    ] {
        assert!(
            payload_text.contains(seam),
            "the carrier payload must carry the `{seam}` seam"
        );
    }
}

#[test]
fn every_capability_this_repo_declares_ships_in_the_committed_registry() {
    // The checkout's own `.aikit/profile.toml` is the product's statement of
    // its default skillsets (ADR 0002's `mattpocock/wayfinder-foundation`).
    // A declared id that exists in no registry is the install defect this
    // corpus exists to prevent: every `aikit` command inside the checkout
    // would fail with `resolution.unknown_capability` on a fresh home. So the
    // committed registry must carry every declared id, and
    // `scripts/verify-native-skills.py` pins the same corpus from the other
    // side — removing one while it stays declared fails here, in CI.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry_root = repo_root.join("registry");

    let profile_text = std::fs::read_to_string(repo_root.join(".aikit/profile.toml"))
        .expect("the repo declares its default skill surface in .aikit/profile.toml");
    let profile: toml::Value =
        toml::from_str(&profile_text).expect(".aikit/profile.toml is valid TOML");
    let enabled = profile
        .get("enable")
        .and_then(|value| value.as_array())
        .expect(".aikit/profile.toml declares an `enable` list");
    assert!(
        !enabled.is_empty(),
        "the repo declares at least one default skill"
    );

    let load = load_registry(&registry_root, RegistrySource::personal()).unwrap();
    for value in enabled {
        let declared = value.as_str().expect("enable entries are capsule ids");
        let id = CapsuleId::parse(declared).expect("declared id parses");
        assert!(
            load.catalog.get(&id).is_some(),
            "{id} is enabled by .aikit/profile.toml but does not ship in the \
             committed registry — a fresh install would fail with \
             resolution.unknown_capability"
        );
    }
}
