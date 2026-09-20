use aikit_adapters::{
    NativeBindingKind, ProviderNativeBinding, WorkingEnvironmentCapabilities,
    WorkingEnvironmentHealth, WorkingEnvironmentObservation, WorkingEnvironmentProvider,
    WORKING_ENVIRONMENT_PROVIDER_VERSION,
};
use aikit_core::resource::ResourceRef;
use aikit_core::Result;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

struct ExternalFixtureProvider {
    provider: ResourceRef,
    focused: Option<ResourceRef>,
    detached: Vec<ResourceRef>,
}

impl ExternalFixtureProvider {
    fn new() -> Self {
        Self {
            provider: r("provider/external/reference-fixture"),
            focused: None,
            detached: Vec::new(),
        }
    }

    fn observation(&self) -> WorkingEnvironmentObservation {
        WorkingEnvironmentObservation {
            schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
            provider: self.provider.clone(),
            provider_version: Some("fixture/1".into()),
            health: WorkingEnvironmentHealth::Healthy,
            capabilities: self.capabilities(),
            bindings: vec![
                ProviderNativeBinding {
                    kind: NativeBindingKind::View,
                    native_id: "native-view-7".into(),
                    canonical_ref: None,
                    provenance: vec!["provider-only native observation".into()],
                },
                ProviderNativeBinding {
                    kind: NativeBindingKind::Surface,
                    native_id: "native-view-7".into(),
                    canonical_ref: Some(r("surface/reference/external")),
                    provenance: vec!["caller supplied explicit canonical Surface binding".into()],
                },
                ProviderNativeBinding {
                    kind: NativeBindingKind::AgentSession,
                    native_id: "provider-session-42".into(),
                    canonical_ref: Some(r("agent-session/reference-world")),
                    provenance: vec![
                        "caller supplied explicit canonical AgentSession binding".into()
                    ],
                },
            ],
            focused_native_id: Some("native-view-7".into()),
            provenance: vec![
                "external-style provider fixture using only public AIKit exports".into(),
            ],
        }
    }
}

impl WorkingEnvironmentProvider for ExternalFixtureProvider {
    fn provider_ref(&self) -> &ResourceRef {
        &self.provider
    }

    fn capabilities(&self) -> WorkingEnvironmentCapabilities {
        WorkingEnvironmentCapabilities {
            discover: true,
            open: true,
            focus: true,
            select: true,
            multi_project: true,
            editor_surface: true,
            terminal_surface: true,
            conversation_surface: false,
            diff_surface: true,
            preview_surface: true,
            test_surface: true,
            surface_attach_detach: true,
            agent_session_attach_detach: false,
            reconstruct: true,
        }
    }

    fn observe(&mut self) -> Result<WorkingEnvironmentObservation> {
        Ok(self.observation())
    }

    fn open(&mut self) -> Result<WorkingEnvironmentObservation> {
        Ok(self.observation())
    }

    fn focus_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        self.focused = Some(surface.clone());
        Ok(())
    }

    fn detach_surface(&mut self, surface: &ResourceRef) -> Result<()> {
        self.detached.push(surface.clone());
        Ok(())
    }
}

#[test]
fn external_provider_participates_through_public_v1_without_native_identity_promotion() {
    let mut provider = ExternalFixtureProvider::new();
    assert_eq!(
        provider.provider_ref(),
        &r("provider/external/reference-fixture")
    );

    let observation = provider.open().unwrap();
    assert_eq!(observation.schema, WORKING_ENVIRONMENT_PROVIDER_VERSION);
    assert_eq!(observation.health, WorkingEnvironmentHealth::Healthy);
    assert!(observation.capabilities.editor_surface);
    assert!(observation.capabilities.surface_attach_detach);
    assert!(!observation.capabilities.agent_session_attach_detach);

    let native_only = observation
        .bindings
        .iter()
        .find(|binding| binding.kind == NativeBindingKind::View)
        .unwrap();
    assert_eq!(native_only.native_id, "native-view-7");
    assert_eq!(
        native_only.canonical_ref, None,
        "a provider-native view id never manufactures canonical identity"
    );

    assert_eq!(
        observation.canonical_native_id(&r("surface/reference/external")),
        Some("native-view-7")
    );
    assert_eq!(
        observation.canonical_native_id(&r("agent-session/reference-world")),
        Some("provider-session-42")
    );
    assert_eq!(
        observation.canonical_native_id(&r("surface/reference/unbound")),
        None
    );

    provider
        .focus_surface(&r("surface/reference/external"))
        .unwrap();
    provider
        .detach_surface(&r("surface/reference/external"))
        .unwrap();
    assert_eq!(provider.focused, Some(r("surface/reference/external")));
    assert_eq!(provider.detached, vec![r("surface/reference/external")]);
}
