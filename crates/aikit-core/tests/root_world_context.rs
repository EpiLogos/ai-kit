//! Root World evidence uses the ordinary bootstrap/SessionSpace/Flow consumers.
use aikit_core::catalog::MemoryCatalog;
use aikit_core::context::ContextDescriptor;
use aikit_core::context_resolution::{ContextResolution, RequestedActors};
use aikit_core::flow::*;
use aikit_core::policy::ManagedPolicy;
use aikit_core::project::{
    ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
};
use aikit_core::resolve::{resolve, ResolveRequest};
use aikit_core::resource::{MemoryResourceIndex, ResourceRef, SourceRef, SourceRevision};
use aikit_core::session_space_application::ContextResolutionEvidence;
use aikit_core::trust::MemoryTrust;
use aikit_core::{
    application_context_resolution_with_binding, project_actor_bootstrap, ActorBootstrapRequest,
    Result,
};

fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}
fn root(revision: &str) -> ContextResolution {
    let mut context = ContextDescriptor::for_project("/not-a-material-grant");
    context.project_root = None;
    let view = resolve(
        &MemoryCatalog::default(),
        &MemoryTrust::default(),
        &ResolveRequest {
            context: context.clone(),
            layers: vec![],
            policy: ManagedPolicy::default(),
        },
    )
    .unwrap();
    let mut binding = ProjectBinding::new(
        ProjectRef::parse("central:root").unwrap(),
        ProjectConstituentRef::parse("binding:root").unwrap(),
        ProjectBindingLocator::NativeWorld {
            world: r("central:root"),
            binding: r("binding:root"),
            scope: r("scope:root"),
            source_revision: SourceRevision::parse(revision).unwrap(),
            content_digest: "blake3:controlled-basis".into(),
        },
    );
    binding.source = Some(SourceRef::parse("source:root").unwrap());
    application_context_resolution_with_binding(
        &context,
        &view,
        &[],
        &MemoryResourceIndex::default(),
        RequestedActors::default(),
        binding,
    )
    .unwrap()
}

#[test]
fn root_binding_survives_bootstrap_evidence_and_roundtrip_without_a_directory() {
    let context = root("rev/1");
    let bootstrap = project_actor_bootstrap(&context, ActorBootstrapRequest::default()).unwrap();
    assert_eq!(bootstrap.project, context.project_binding);
    let evidence = ContextResolutionEvidence::from_resolution(&context).unwrap();
    assert_eq!(evidence.project().as_str(), "central:root");
    assert_eq!(evidence.basis.project_binding, context.project_binding);
    assert!(context.deterministic.context.project_root.is_none());
    let recovered: ContextResolution =
        serde_json::from_slice(&serde_json::to_vec(&context).unwrap()).unwrap();
    assert_eq!(recovered, context);
    assert!(matches!(
        recovered.project_binding.locator,
        ProjectBindingLocator::NativeWorld { .. }
    ));
    let mut changed = context.clone();
    if let ProjectBindingLocator::NativeWorld {
        source_revision, ..
    } = &mut changed.project_binding.locator
    {
        *source_revision = SourceRevision::parse("rev/2").unwrap();
    }
    assert_ne!(
        ContextResolutionEvidence::from_resolution(&changed)
            .unwrap()
            .reference,
        evidence.reference
    );
}

struct PrivateFlow(FlowSourceDescriptor);
impl FlowProvider for PrivateFlow {
    fn provider_ref(&self) -> &ResourceRef {
        &self.0.provider
    }
    fn inspect(&self, _: &ResourceRef) -> Result<FlowSourceDescriptor> {
        Ok(self.0.clone())
    }
    fn read_exact(&self, _: &ResourceRef, _: &SourceRevision) -> Result<FlowReadOutcome> {
        panic!("identity must not grant payload readability")
    }
    fn write(&mut self, _: &FlowWriteRequest) -> Result<FlowWriteResult> {
        panic!("binding must not write")
    }
}

#[test]
fn root_flow_keeps_scope_and_privacy_checks_instead_of_skipping_binding() {
    let context = root("rev/1");
    let mut provider = PrivateFlow(FlowSourceDescriptor {
        flow_ref: r("flow:root"),
        source_ref: SourceRef::parse("source:flow").unwrap(),
        revision: SourceRevision::parse("rev/flow").unwrap(),
        provider: r("provider:central"),
        lifecycle: FlowLifecycle::Active,
        title: None,
        scope: Some(r("central:root")),
        container_hint: None,
        capabilities: FlowCapabilities::default(),
        provenance: vec![],
    });
    let bound = bind_flow_for_act(
        &provider,
        &context,
        &r("flow:root"),
        r("agent-session/root"),
        None,
        None,
    )
    .unwrap();
    assert_eq!(bound.binding.project.as_str(), "central:root");
    assert!(matches!(
        bound.disclosure,
        FlowStandingDisclosure::Undisclosed { .. }
    ));
    assert!(!bound.automatic_agent_or_model_invocation);
    provider.0.scope = Some(r("project:child"));
    assert_eq!(
        bind_flow_for_act(
            &provider,
            &context,
            &r("flow:root"),
            r("agent-session/root"),
            None,
            None
        )
        .unwrap_err()
        .code(),
        "flow.scope_mismatch"
    );
}
