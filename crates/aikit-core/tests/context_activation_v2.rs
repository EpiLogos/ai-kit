mod common;

use std::path::PathBuf;

use serde::Serialize;

use aikit_core::project::{ProjectBinding, ProjectBindingLocator, ProjectConstituentRef};
use aikit_core::resource::{
    Eligibility, MemoryResourceIndex, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef,
    ResourceSource, SourceRef, SourceRevision, SourceState,
};
use aikit_core::scope::ScopeKind;
use aikit_core::session_space_application::{ContextResolutionBasis, ContextResolutionEvidence};
use aikit_core::{
    attach_context_activations, compose_context_resolution, explain_context_activation,
    ContextActivationEvidenceBasis, ContextActivationMode, ContextActivationReceipt,
    RequestedActors, TargetId,
};

use common::{layer_using, profile, script, Fixture};

fn deterministic_fixture() -> (aikit_core::ResolvedView, Vec<aikit_core::ScopeLayer>) {
    let layers = vec![layer_using(ScopeKind::Project, &["profile/code/base"])];
    let fixture = Fixture::new(vec![script("script/test/check")])
        .with_profiles(vec![profile(
            "profile/code/base",
            &["script/test/check"],
            &[],
        )])
        .with_layers(layers.clone());
    (fixture.resolve().expect("deterministic resolution"), layers)
}

fn project_binding() -> ProjectBinding {
    ProjectBinding::new(
        aikit_core::ProjectRef::parse("project:context-activation").unwrap(),
        ProjectConstituentRef::parse("constituent:source").unwrap(),
        ProjectBindingLocator::LocalDirectory {
            path: PathBuf::from("/work/context-activation"),
        },
    )
}

fn context_source(id: &str, path: &str, role: &str, provenance: &str) -> ResourceRecord {
    let mut descriptor = ResourceDescriptor::new(
        ResourceRef::parse(id).unwrap(),
        ResourceKind::ContextSource,
        path,
        format!("Project context source {path}"),
    );
    descriptor.annotations.insert("source-role".into(), role.into());
    descriptor
        .annotations
        .insert("provenance".into(), provenance.into());
    descriptor.sources.push(ResourceSource {
        source: SourceRef::parse(&format!("source:{id}")).unwrap(),
        authority: None,
        revision: Some(SourceRevision::parse("rev:1").unwrap()),
        locator: None,
        state: SourceState::Available,
    });
    let mut record = ResourceRecord::new(descriptor);
    record.eligibility = Eligibility::Eligible;
    record
}

fn resolution() -> aikit_core::ContextResolution {
    let (deterministic, layers) = deterministic_fixture();
    let mut resources = MemoryResourceIndex::default();
    resources.insert(context_source(
        "context-source:project:agents",
        "AGENTS.md",
        "possible-project-agent-governance",
        "unresolved",
    ));
    resources.insert(context_source(
        "context-source:project:claude",
        "CLAUDE.md",
        "project-agent-governance",
        "human-adopted",
    ));
    resources.insert(context_source(
        "context-source:project:intent",
        "intent/feature.md",
        "intent",
        "human-authored",
    ));
    compose_context_resolution(
        &deterministic,
        project_binding(),
        &layers,
        &resources,
        RequestedActors::default(),
    )
}

fn codex_agents_receipt() -> ContextActivationReceipt {
    ContextActivationReceipt::new(
        ResourceRef::parse("context-source:project:agents").unwrap(),
        TargetId::codex(),
        ContextActivationMode::HarnessNativeAutoLoaded,
        ContextActivationEvidenceBasis::AdapterSemantics,
        "codex-native-instruction-chain",
        "project root → current working directory",
        "codex-native-precedence",
        false,
        true,
        vec!["evidence:codex-agents-convention".into()],
    )
    .unwrap()
}

fn claude_governance_receipt() -> ContextActivationReceipt {
    ContextActivationReceipt::new(
        ResourceRef::parse("context-source:project:claude").unwrap(),
        TargetId::claude_code(),
        ContextActivationMode::HarnessNativeAutoLoaded,
        ContextActivationEvidenceBasis::AdapterSemantics,
        "claude-code-project-memory",
        "project/session native instruction scope",
        "claude-code-native-precedence",
        false,
        true,
        vec!["evidence:claude-code-memory-convention".into()],
    )
    .unwrap()
}

#[test]
fn ambient_codex_activation_does_not_imply_aikit_selection_or_source_authority() {
    let mut resolution = resolution();
    let source = ResourceRef::parse("context-source:project:agents").unwrap();

    attach_context_activations(&mut resolution, [codex_agents_receipt()]).unwrap();
    let explained = explain_context_activation(&resolution, &source).unwrap();

    assert_eq!(
        explained.source.resource.descriptor.annotations["source-role"],
        "possible-project-agent-governance"
    );
    assert_eq!(
        explained.source.resource.descriptor.annotations["provenance"],
        "unresolved"
    );
    assert_eq!(explained.activations.len(), 1);
    assert_eq!(
        explained.activations[0].mode,
        ContextActivationMode::HarnessNativeAutoLoaded
    );
    assert!(!explained.activations[0].ai_kit_selected);
    assert!(explained.activations[0].materially_active);
    assert_eq!(explained.activations[0].precedence_owner, "codex-native-precedence");
}

#[test]
fn adopted_claude_governance_keeps_source_standing_separate_from_native_loading() {
    let mut resolution = resolution();
    let source = ResourceRef::parse("context-source:project:claude").unwrap();

    attach_context_activations(&mut resolution, [claude_governance_receipt()]).unwrap();
    let explained = explain_context_activation(&resolution, &source).unwrap();
    assert_eq!(
        explained.source.resource.descriptor.annotations["provenance"],
        "human-adopted"
    );
    assert_eq!(
        explained.activations[0].evidence_basis,
        ContextActivationEvidenceBasis::AdapterSemantics
    );
    assert!(explained.activations[0].materially_active);
}

#[test]
fn aikit_selection_and_material_activation_are_independent_states() {
    let mut resolution = resolution();
    let source = ResourceRef::parse("context-source:project:intent").unwrap();
    let selected = ContextActivationReceipt::new(
        source.clone(),
        TargetId::new("guidance"),
        ContextActivationMode::AikitSelected,
        ContextActivationEvidenceBasis::Observed,
        "aikit-context-resolution",
        "current Focus",
        "aikit-context-resolution",
        true,
        false,
        vec!["resolution:selection".into()],
    )
    .unwrap();
    let retrieved = ContextActivationReceipt::new(
        source.clone(),
        TargetId::new("guidance"),
        ContextActivationMode::Retrieved,
        ContextActivationEvidenceBasis::Observed,
        "context-source-provider",
        "current Focus",
        "aikit-context-resolution",
        true,
        true,
        vec!["retrieval:feature-intent".into()],
    )
    .unwrap();

    attach_context_activations(&mut resolution, [retrieved, selected]).unwrap();
    let explained = explain_context_activation(&resolution, &source).unwrap();
    assert_eq!(explained.activations.len(), 2);
    assert_eq!(explained.activations[0].mode, ContextActivationMode::AikitSelected);
    assert!(!explained.activations[0].materially_active);
    assert_eq!(explained.activations[1].mode, ContextActivationMode::Retrieved);
    assert!(explained.activations[1].materially_active);
}

#[test]
fn material_activation_requires_evidence_and_unknown_sources_are_rejected() {
    let active_without_evidence = ContextActivationReceipt::new(
        ResourceRef::parse("context-source:project:agents").unwrap(),
        TargetId::codex(),
        ContextActivationMode::HarnessNativeAutoLoaded,
        ContextActivationEvidenceBasis::Observed,
        "codex",
        "project",
        "codex",
        false,
        true,
        vec![],
    );
    assert!(active_without_evidence.is_err());

    let mut resolution = resolution();
    let unknown = ContextActivationReceipt::new(
        ResourceRef::parse("context-source:project:missing").unwrap(),
        TargetId::codex(),
        ContextActivationMode::HarnessNativeAutoLoaded,
        ContextActivationEvidenceBasis::AdapterSemantics,
        "codex",
        "project",
        "codex",
        false,
        true,
        vec!["evidence:convention".into()],
    )
    .unwrap();
    assert!(attach_context_activations(&mut resolution, [unknown]).is_err());
}

#[test]
fn context_activation_receipts_survive_context_resolution_serialisation() {
    let mut resolution = resolution();
    attach_context_activations(&mut resolution, [codex_agents_receipt()]).unwrap();

    let encoded = serde_json::to_string(&resolution).unwrap();
    let decoded: aikit_core::ContextResolution = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, resolution);
}

#[test]
fn activation_truth_changes_the_persisted_context_resolution_ref_but_not_by_arrival_order() {
    let base = resolution();
    let base_evidence = ContextResolutionEvidence::from_resolution(&base).unwrap();

    let mut left = base.clone();
    attach_context_activations(
        &mut left,
        [codex_agents_receipt(), claude_governance_receipt()],
    )
    .unwrap();
    let left_evidence = ContextResolutionEvidence::from_resolution(&left).unwrap();

    let mut right = base;
    attach_context_activations(
        &mut right,
        [claude_governance_receipt(), codex_agents_receipt()],
    )
    .unwrap();
    let right_evidence = ContextResolutionEvidence::from_resolution(&right).unwrap();

    assert_ne!(base_evidence.reference, left_evidence.reference);
    assert_eq!(left_evidence.reference, right_evidence.reference);
    assert_eq!(left_evidence.basis.context_activations, right_evidence.basis.context_activations);
    assert_eq!(left_evidence.basis.context_activations.len(), 2);
    assert!(left_evidence
        .reference
        .to_string()
        .starts_with("context-resolution/"));
}

#[test]
fn empty_activation_basis_preserves_the_pre_activation_content_address() {
    let resolution = resolution();
    let evidence = ContextResolutionEvidence::from_resolution(&resolution).unwrap();
    assert!(evidence.basis.context_activations.is_empty());

    #[derive(Serialize)]
    struct LegacyContextResolutionBasis<'a> {
        project_binding: &'a ProjectBinding,
        resolver_hash: &'a str,
        catalog_revision: &'a str,
        scopes: &'a [aikit_core::ScopeResolution],
        context_sources: &'a [ResourceRef],
        #[serde(skip_serializing_if = "Option::is_none")]
        host: &'a Option<ResourceRef>,
    }

    let ContextResolutionBasis {
        project_binding,
        resolver_hash,
        catalog_revision,
        scopes,
        context_sources,
        host,
        context_activations: _,
    } = &evidence.basis;
    let legacy = LegacyContextResolutionBasis {
        project_binding,
        resolver_hash,
        catalog_revision,
        scopes,
        context_sources,
        host,
    };
    let bytes = serde_json::to_vec(&legacy).unwrap();
    let digest = blake3::hash(&bytes).to_hex().to_string();
    assert_eq!(
        evidence.reference.to_string(),
        format!("context-resolution/{}", &digest[..16])
    );
}
