//! The wiring proof: the CLI backend composes a real credential/provider
//! reading for a real world, and the reading the System pane renders carries
//! it instead of the `not_attempted` default #239 shipped with nothing behind.
//!
//! This drives the production `PaletteBackend` implementation against a real
//! home and the real first-party Model catalogue — not a fixture — because the
//! whole point of the slice is that the producer nobody wired is now wired: the
//! roster is actually observed over the OS secure store, and the world's
//! declared credentials are actually resolved against it.

use aikit_cli::app::Service;
use aikit_core::credential_world::CredentialStatusKnowledge;
use aikit_store::AikitHome;
use std::collections::BTreeMap;
use std::process::Command;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .expect("git is available in the test environment");
    assert!(status.success(), "git {args:?} failed");
}

/// A real Project the service recognises: `.aikit` is the discovery marker and
/// `ProjectCentral/project.json` binds a native owner identity.
fn project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join(".aikit")).unwrap();
    std::fs::write(root.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
    std::fs::create_dir_all(root.join("ProjectCentral")).unwrap();
    std::fs::write(
        root.join("ProjectCentral/project.json"),
        r#"{"schema":"central.project/v1","project_id":"project:probe","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#,
    )
    .unwrap();
    git(root, &["init", "--initial-branch=trunk"]);
    git(root, &["config", "user.email", "probe@example.invalid"]);
    git(root, &["config", "user.name", "probe"]);
    std::fs::write(root.join("README.md"), "probe\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "first"]);
}

fn service(home: &std::path::Path, root: &std::path::Path) -> Service {
    let mut env = BTreeMap::new();
    env.insert(
        "AIKIT_CONTEXT_ID".to_owned(),
        aikit_core::ContextId::generate().to_string(),
    );
    Service::open(AikitHome::at(home), root, |key| env.get(key).cloned()).unwrap()
}

#[test]
fn the_cli_backend_composes_a_real_credential_reading_for_a_real_world() {
    use aikit_tui::backend::PaletteBackend;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    std::fs::create_dir_all(&root).unwrap();
    project(&root);

    let service = service(tmp.path(), &root);
    let disclosure = service
        .credential_world()
        .expect("composing the credential world does not fail")
        .expect("a producer is attached — this is the wiring #239 lacked");

    // The roster was actually observed. This is the whole distinction W7 fixes:
    // not the `Unknown` the `not_attempted` default carries, but a real reading
    // of what the machine has.
    assert!(
        disclosure.providers.is_known(),
        "the provider roster is observed, not left unknown: {:?}",
        disclosure.providers
    );

    // The world's declared credentials come straight from the first-party
    // catalogue's hosted routes (anthropic, openai, deepseek all require one;
    // local ollama requires none). A fixture could not produce this — it is the
    // real catalogue joined against the real (empty) binding store.
    assert!(
        !disclosure.credentials.is_empty(),
        "the seed catalogue's hosted Models declare credential needs"
    );
    let keys: Vec<&str> = disclosure
        .credentials
        .keys()
        .map(|requirement_ref| requirement_ref.as_str())
        .collect();
    assert!(
        keys.iter().any(|key| key.contains("openai")),
        "openai is a hosted seed provider that needs a credential: {keys:?}"
    );

    // Nothing is bound in a fresh home, so every credential is a *resolved*
    // negative — a real "no", explained — never `Unresolved` ("we could not
    // tell"). Collapsing those two is exactly the defect the read model exists
    // to prevent.
    for status in disclosure.credentials.values() {
        assert!(
            matches!(status, CredentialStatusKnowledge::Resolved(_)),
            "an observed roster resolves every requirement to an explained answer, \
             not an unresolved open question: {status:?}"
        );
        assert!(
            !status.is_selected(),
            "nothing is bound in a fresh home, so nothing resolves to a provider"
        );
    }

    // Roster known and every status resolved: the reading is fully observed,
    // which is what lets the System pane render `0/N resolved` honestly rather
    // than `not attempted`.
    assert!(disclosure.fully_observed());
}

#[test]
fn a_world_with_no_credential_routes_reports_an_observed_empty_need() {
    // A home whose catalogue is only local routes has no credential needs. The
    // producer must still report an *observed* roster and an empty requirement
    // set — "this world needs none" — never the `not_attempted` default, and
    // never a fabricated requirement. We cannot easily strip the seed
    // catalogue's hosted entries here, so this test instead pins the invariant
    // the derivation guarantees: a local-only route contributes no requirement.
    use aikit_core::credential_world::credential_requirements_for_model_routes;
    use aikit_core::resource::{
        CredentialCondition, ModelRoute, ModelRouteKind, ModelRouteSet, ProviderRef,
        ResourceRef, RouteAvailability,
    };

    let mut set = ModelRouteSet::new(ResourceRef::parse("model:local").unwrap());
    set.routes.push(ModelRoute {
        model: ResourceRef::parse("model:local").unwrap(),
        provider: ProviderRef::parse("provider:ollama").unwrap(),
        kind: ModelRouteKind::LocalServing,
        provider_native_id: "llama3.2:latest".into(),
        endpoint: Some("http://127.0.0.1:11434".into()),
        availability: RouteAvailability::Observed {
            detection_ref: "detection:test".into(),
        },
        credential: CredentialCondition::NotRequired,
        provenance: Vec::new(),
    });

    assert!(credential_requirements_for_model_routes(&[set]).is_empty());
}

/// A credential bound in the store flips its reading from a resolved "no" to a
/// selected provider. The real-machine probe cannot show this — nothing is
/// bound there — so this proves the other half of the seam hermetically: a
/// binding record the world's routes will match, resolved against the same
/// store the roster is observed over.
///
/// Binding a `CredentialBindingState` records provenance metadata only; it never
/// writes a secret. The native store reports `Bound` when both the record exists
/// and its keychain entry is reachable. Where the OS secure store is not
/// reachable at all (some headless CI), the reading stays a resolved,
/// provider-*Unavailable* "no" — still observed, still not the `not_attempted`
/// default — so the assertion is gated on the roster actually being available.
#[test]
fn a_bound_credential_resolves_to_the_native_provider() {
    use aikit_adapters::NativeSecureStoreProvider;
    use aikit_core::credential::{
        CredentialRef, SecretMaterialisationClass, SecretProvider, SecretProviderTier,
    };
    use aikit_core::credential_world::ProviderRosterKnowledge;
    use aikit_store::credentials::CredentialBindingStore;
    use aikit_tui::backend::PaletteBackend;
    use std::collections::BTreeMap;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("probe");
    std::fs::create_dir_all(&root).unwrap();
    project(&root);
    let home = AikitHome::at(tmp.path());

    // Bind a credential for a provider the seed catalogue declares (openai).
    // The provider_ref must be this platform's native store ref, read from the
    // provider's own descriptor rather than hardcoded per-OS.
    let credential = CredentialRef::new("credential:openai").unwrap();
    let native = NativeSecureStoreProvider::new();
    let descriptor = native.descriptor(&credential);
    let binding = aikit_core::credential::CredentialBindingState {
        credential_ref: credential.clone(),
        provider_ref: descriptor.provider_ref.clone(),
        provider_tier: SecretProviderTier::OsSecureStore,
        materialisation: SecretMaterialisationClass::ProviderNativeLease,
        binding_provenance: "test-binding".into(),
        revision_or_lease_class: None,
        expires_at: None,
        revoked: false,
        metadata: BTreeMap::new(),
    };
    CredentialBindingStore::new(&home).save(&binding).unwrap();

    let mut env = BTreeMap::new();
    env.insert(
        "AIKIT_CONTEXT_ID".to_owned(),
        aikit_core::ContextId::generate().to_string(),
    );
    let service = Service::open(home, &root, |key| env.get(key).cloned()).unwrap();

    let disclosure = service.credential_world().unwrap().unwrap();
    let requirement =
        aikit_core::credential::SecretRequirementRef::new("secret-requirement:credential:openai")
            .unwrap();
    let status = disclosure
        .status(&requirement)
        .expect("openai is a declared credential need");

    let store_available = matches!(
        &disclosure.providers,
        ProviderRosterKnowledge::Observed { providers }
            if providers.iter().any(|p| p.available)
    );
    if store_available {
        assert!(
            status.is_selected(),
            "a bound credential resolves to the native provider once the store is reachable: {status:?}"
        );
    } else {
        // Even where the store is unreachable, the reading is observed and
        // resolved — never the `not_attempted` default that had no producer.
        assert!(matches!(
            status,
            aikit_core::credential_world::CredentialStatusKnowledge::Resolved(_)
        ));
    }
}
