use aikit_core::resource::ResourceRef;
use aikit_core::session_space::{
    SessionSpaceDefinition, SessionSpaceLifecycle, SessionSpaceRef, SessionSpaceRuntime,
};
use aikit_core::session_space_contribution::{
    SessionSpaceContributionDefinition, SessionSpaceContributionRef,
    SessionSpaceContributionRegistry, SESSION_SPACE_CONTRIBUTION_REGISTRY_VERSION,
};

#[test]
fn native_registration_readback_and_remove_preserve_session_space_identity() {
    let space = SessionSpaceRef::parse("session-space/factory-build").unwrap();
    let runtime = SessionSpaceRuntime::open(SessionSpaceDefinition::new(space.clone())).unwrap();
    let contribution_ref =
        SessionSpaceContributionRef::parse("session-space-contribution/factory-build").unwrap();
    let definition =
        SessionSpaceContributionDefinition::new(contribution_ref.clone(), space.clone())
            .with_provider(ResourceRef::parse("provider/factory-build").unwrap())
            .with_surface(ResourceRef::parse("surface/factory-build").unwrap())
            .with_provenance("package lifecycle conformance");

    let mut registry = SessionSpaceContributionRegistry::default();
    let registration = registry.register(definition.clone()).unwrap();

    assert_ne!(
        registration.native_registration_ref.to_string(),
        contribution_ref.to_string()
    );
    assert_ne!(contribution_ref.to_string(), space.to_string());
    assert_eq!(
        registry.read(&contribution_ref).unwrap().contribution,
        definition
    );
    registry
        .verify_session_space_read_model(&contribution_ref, &runtime.read_model())
        .unwrap();

    let observed = registry.read_model();
    assert_eq!(
        observed.version,
        SESSION_SPACE_CONTRIBUTION_REGISTRY_VERSION
    );
    assert_eq!(observed.registrations.len(), 1);

    let removal = registry.remove(&contribution_ref).unwrap();
    assert_eq!(removal.session_space, space);
    assert!(registry.read(&contribution_ref).is_none());

    // Removing the package/native-registration relation does not close, delete or
    // otherwise mutate the externally owned SessionSpace runtime.
    let after = runtime.read_model();
    assert_eq!(after.id, space);
    assert_eq!(after.lifecycle, SessionSpaceLifecycle::Open);
}

#[test]
fn duplicate_registration_and_wrong_read_model_are_rejected() {
    let expected_space = SessionSpaceRef::parse("session-space/expected").unwrap();
    let other_space = SessionSpaceRef::parse("session-space/other").unwrap();
    let contribution_ref =
        SessionSpaceContributionRef::parse("session-space-contribution/editor").unwrap();
    let definition =
        SessionSpaceContributionDefinition::new(contribution_ref.clone(), expected_space);

    let mut registry = SessionSpaceContributionRegistry::default();
    registry.register(definition.clone()).unwrap();
    let duplicate = registry.register(definition).unwrap_err();
    assert_eq!(
        duplicate.code(),
        "session_space_contribution.already_registered"
    );

    let other_runtime =
        SessionSpaceRuntime::open(SessionSpaceDefinition::new(other_space)).unwrap();
    let mismatch = registry
        .verify_session_space_read_model(&contribution_ref, &other_runtime.read_model())
        .unwrap_err();
    assert_eq!(
        mismatch.code(),
        "session_space_contribution.session_space_mismatch"
    );
}
