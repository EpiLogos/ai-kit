mod common;

use std::path::PathBuf;

use aikit_core::project::{ProjectBinding, ProjectBindingLocator, ProjectConstituentRef};
use aikit_core::resource::{
    Eligibility, MemoryResourceIndex, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef,
    ResourceSource, SourceRef, SourceRevision, SourceState,
};
use aikit_core::scope::ScopeKind;
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

#[test]
fn ambient_codex_activation_does_not_imply_aikit_selection_or_source_authority() {
    let mut resolution = resolution();
    let source = ResourceRef::parse("context-source:project:agents").unwrap();
    let receipt = ContextActivationReceipt::new(
        source.clone(),
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
    .unwrap();

    attach_context_activations(&mut resolution, [receipt]).unwrap();
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
    let receipt = ContextActivationReceipt::new(
        source.clone(),
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
    .unwrap();

    attach_context_activations(&mut resolution, [receipt]).unwrap();
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
    let receipt = ContextActivationReceipt::new(
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
    .unwrap();
    attach_context_activations(&mut resolution, [receipt]).unwrap();

    let encoded = serde_json::to_string(&resolution).unwrap();
    let decoded: aikit_core::ContextResolution = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, resolution);
}
