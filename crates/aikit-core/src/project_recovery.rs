//! Project recovery/bootstrap read model.
//!
//! This composes already-owned ProjectCentral orientation, native source
//! classification, Project reflection and praxis resolution around the existing
//! ContextResolution. It does not perform filesystem I/O, classify source by
//! fiat, activate capabilities, or create Agency authority.

use serde::{Deserialize, Serialize};

use crate::context_resolution::{Availability, ContextResolution};
use crate::praxis::PraxisResolution;
use crate::project_map::ProjectLens;
use crate::project_reflection::{
    LocalSourceClassification, LocalSourceRole, ProjectReflectionReadModel,
};
use crate::projectcentral::{ProjectCentralGroundStatus, ProjectCentralOrientation};
use crate::resource::{ResourceRef, SourceRef};

pub const PROJECT_RECOVERY_VERSION: &str = "aikit.project-recovery/v1";

/// Stable source identity plus the separately derived role classification used by
/// Project recovery. Source identity never comes from the classification itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectLocalSourceBinding {
    pub source: SourceRef,
    pub classification: LocalSourceClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectRecoveryStageKind {
    AuthoredGround,
    NativeProjectSource,
    SemanticWiki,
    LocalStructuralDescription,
    ProjectReflection,
    CapabilityPraxis,
    ContextResolution,
    HarnessProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectRecoveryStageState {
    Available,
    Partial,
    OptionalAbsent,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecoveryStage {
    pub kind: ProjectRecoveryStageKind,
    pub state: ProjectRecoveryStageState,
    #[serde(default)]
    pub resources: Vec<ResourceRef>,
    #[serde(default)]
    pub sources: Vec<SourceRef>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecognitionPressure {
    #[serde(default)]
    pub source: Option<SourceRef>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecoveryReadModel {
    pub version: String,
    pub project: String,
    pub stages: Vec<ProjectRecoveryStage>,
    #[serde(default)]
    pub unresolved_capabilities: Vec<ResourceRef>,
    #[serde(default)]
    pub unavailable_capabilities: Vec<ResourceRef>,
    #[serde(default)]
    pub selected_methods: Vec<ResourceRef>,
    #[serde(default)]
    pub recognition_pressure: Vec<RecognitionPressure>,
    /// Construction of this receipt never implies that an Agent is authorised to act.
    pub act_authority_inferred: bool,
}

/// Compose the median Project-recovery state without making richer substrate a
/// validity requirement for ordinary Projects.
pub fn project_recovery(
    resolution: &ContextResolution,
    central: Option<&ProjectCentralOrientation>,
    local_sources: &[ProjectLocalSourceBinding],
    reflection: Option<&ProjectReflectionReadModel>,
    praxis: Option<&PraxisResolution>,
) -> ProjectRecoveryReadModel {
    let mut stages = Vec::new();
    let mut recognition_pressure = Vec::new();

    let reflected_canon = reflected_by_lens(reflection, ProjectLens::Canon);
    let ground_stage = match central {
        Some(orientation) => {
            let mut sources = vec![orientation.human_root.clone()];
            if let Some(relations) = &orientation.ground_relations {
                sources.push(relations.clone());
            }
            let state = match orientation.ground_status {
                ProjectCentralGroundStatus::Established => ProjectRecoveryStageState::Available,
                ProjectCentralGroundStatus::Partial => {
                    recognition_pressure.push(RecognitionPressure {
                        source: Some(orientation.human_root.clone()),
                        reason: "Project ground exists but currently recognised human source does not establish all candidate authored standing".into(),
                    });
                    ProjectRecoveryStageState::Partial
                }
                ProjectCentralGroundStatus::Empty => ProjectRecoveryStageState::OptionalAbsent,
            };
            ProjectRecoveryStage {
                kind: ProjectRecoveryStageKind::AuthoredGround,
                state,
                resources: reflected_canon.clone(),
                sources,
                note: "Central/native owner remains authority for human-authored Project ground; bootstrap only discloses its state".into(),
            }
        }
        None if !reflected_canon.is_empty() => ProjectRecoveryStage {
            kind: ProjectRecoveryStageKind::AuthoredGround,
            state: ProjectRecoveryStageState::Available,
            resources: reflected_canon.clone(),
            sources: Vec::new(),
            note: "authored/canonical ProjectMap source is available without requiring ProjectCentral".into(),
        },
        None => ProjectRecoveryStage {
            kind: ProjectRecoveryStageKind::AuthoredGround,
            state: ProjectRecoveryStageState::OptionalAbsent,
            resources: Vec::new(),
            sources: Vec::new(),
            note: "ordinary Projects remain valid without a Central-authored ground aperture".into(),
        },
    };
    stages.push(ground_stage);

    let mut native_sources = local_sources
        .iter()
        .map(|binding| binding.source.clone())
        .collect::<Vec<_>>();
    native_sources.sort();
    native_sources.dedup();

    let mut descriptions = Vec::new();
    let mut unresolved_local = 0usize;
    for binding in local_sources {
        match resolved_local_role(&binding.classification) {
            Some(LocalSourceRole::LocalStructuralDescription) => {
                descriptions.push(binding.source.clone())
            }
            Some(_) => {}
            None => unresolved_local += 1,
        }
    }
    descriptions.sort();
    descriptions.dedup();

    stages.push(ProjectRecoveryStage {
        kind: ProjectRecoveryStageKind::NativeProjectSource,
        state: if native_sources.is_empty() {
            ProjectRecoveryStageState::OptionalAbsent
        } else if unresolved_local > 0 {
            ProjectRecoveryStageState::Partial
        } else {
            ProjectRecoveryStageState::Available
        },
        resources: Vec::new(),
        sources: native_sources,
        note: if unresolved_local > 0 {
            format!(
                "{unresolved_local} discovered local source candidate(s) remain role-unresolved or hint-only; source identity is preserved without promoting filename/path hints to authority"
            )
        } else {
            "native Project source is retained in place and classified only as owner/provenance evidence supports".into()
        },
    });

    let reflected_wiki = reflected_by_lens(reflection, ProjectLens::SemanticWiki);
    let central_wiki = central
        .filter(|orientation| orientation.canonical_wiki_exists)
        .map(|orientation| orientation.canonical_wiki.clone());
    stages.push(ProjectRecoveryStage {
        kind: ProjectRecoveryStageKind::SemanticWiki,
        state: if central_wiki.is_some() || !reflected_wiki.is_empty() {
            ProjectRecoveryStageState::Available
        } else {
            ProjectRecoveryStageState::OptionalAbsent
        },
        resources: reflected_wiki,
        sources: central_wiki.into_iter().collect(),
        note: "SemanticWiki is maintained Project knowledge, not a replacement for human Ground or native source".into(),
    });

    let reflected_descriptions = reflected_by_lens(reflection, ProjectLens::SourcePool);
    stages.push(ProjectRecoveryStage {
        kind: ProjectRecoveryStageKind::LocalStructuralDescription,
        state: if descriptions.is_empty() && reflected_descriptions.is_empty() {
            ProjectRecoveryStageState::OptionalAbsent
        } else {
            ProjectRecoveryStageState::Available
        },
        resources: reflected_descriptions,
        sources: descriptions,
        note: "local description is scoped source about implementation; current code remains implementation truth".into(),
    });

    stages.push(match reflection {
        Some(view) if !view.meaning.is_empty() && !view.code.is_empty() => ProjectRecoveryStage {
            kind: ProjectRecoveryStageKind::ProjectReflection,
            state: ProjectRecoveryStageState::Available,
            resources: reflected_resources(view),
            sources: Vec::new(),
            note: "explicit ProjectMap bindings support bounded bidirectional meaning/description/code/evidence traversal".into(),
        },
        Some(view) => ProjectRecoveryStage {
            kind: ProjectRecoveryStageKind::ProjectReflection,
            state: ProjectRecoveryStageState::Partial,
            resources: reflected_resources(view),
            sources: Vec::new(),
            note: "ProjectMap reflection exists but does not yet expose both semantic and code faces".into(),
        },
        None => ProjectRecoveryStage {
            kind: ProjectRecoveryStageKind::ProjectReflection,
            state: ProjectRecoveryStageState::OptionalAbsent,
            resources: Vec::new(),
            sources: Vec::new(),
            note: "rich semantic↔code reflection is developmental capacity, not a minimum requirement".into(),
        },
    });

    let unresolved_capabilities = resolution
        .capabilities
        .iter()
        .filter(|value| matches!(value.availability, Availability::Unresolved { .. }))
        .map(|value| value.resource.descriptor.id.clone())
        .collect::<Vec<_>>();
    let unavailable_capabilities = resolution
        .capabilities
        .iter()
        .filter(|value| matches!(value.availability, Availability::Unavailable { .. }))
        .map(|value| value.resource.descriptor.id.clone())
        .collect::<Vec<_>>();
    let selected_methods = praxis
        .into_iter()
        .flat_map(|value| value.methods.iter())
        .map(|method| method.method.clone())
        .collect::<Vec<_>>();
    let mut praxis_resources = resolution
        .capabilities
        .iter()
        .map(|value| value.resource.descriptor.id.clone())
        .collect::<Vec<_>>();
    praxis_resources.extend(selected_methods.iter().cloned());
    praxis_resources.sort();
    praxis_resources.dedup();
    stages.push(ProjectRecoveryStage {
        kind: ProjectRecoveryStageKind::CapabilityPraxis,
        state: if resolution.capabilities.is_empty() && selected_methods.is_empty() {
            ProjectRecoveryStageState::OptionalAbsent
        } else if !unavailable_capabilities.is_empty() || !unresolved_capabilities.is_empty() {
            ProjectRecoveryStageState::Partial
        } else {
            ProjectRecoveryStageState::Available
        },
        resources: praxis_resources,
        sources: Vec::new(),
        note: "capability availability, Method selection and praxis fitness remain distinct; selected Method does not confer authority".into(),
    });

    stages.push(ProjectRecoveryStage {
        kind: ProjectRecoveryStageKind::ContextResolution,
        state: if resolution.warnings.is_empty() {
            ProjectRecoveryStageState::Available
        } else {
            ProjectRecoveryStageState::Partial
        },
        resources: resolution
            .context_sources
            .iter()
            .map(|value| value.resource.descriptor.id.clone())
            .collect(),
        sources: Vec::new(),
        note: "ContextResolution remains the operational resolution owner; recovery does not introduce a second precedence system".into(),
    });

    stages.push(ProjectRecoveryStage {
        kind: ProjectRecoveryStageKind::HarnessProjection,
        state: if resolution.projection.targets.is_empty() {
            ProjectRecoveryStageState::OptionalAbsent
        } else {
            ProjectRecoveryStageState::Available
        },
        resources: Vec::new(),
        sources: Vec::new(),
        note: "target-native projection is derived from resolved context; projection files never become source".into(),
    });

    ProjectRecoveryReadModel {
        version: PROJECT_RECOVERY_VERSION.into(),
        project: resolution.project_binding.project.to_string(),
        stages,
        unresolved_capabilities,
        unavailable_capabilities,
        selected_methods,
        recognition_pressure,
        act_authority_inferred: false,
    }
}

/// A classification only becomes operative bootstrap knowledge when it is more
/// than a conventional filename/path hint. This lets discovery be generous while
/// keeping role attribution conservative.
fn resolved_local_role(classification: &LocalSourceClassification) -> Option<LocalSourceRole> {
    match classification {
        LocalSourceClassification::Classified(evidence)
            if !evidence.evidence.iter().all(|item| {
                item == "conventional-filename-hint" || item == "path-hint"
            }) => Some(evidence.role),
        LocalSourceClassification::Classified(_)
        | LocalSourceClassification::Ambiguous(_)
        | LocalSourceClassification::Unresolved => None,
    }
}

fn reflected_by_lens(
    reflection: Option<&ProjectReflectionReadModel>,
    lens: ProjectLens,
) -> Vec<ResourceRef> {
    let Some(view) = reflection else {
        return Vec::new();
    };
    let mut resources = Vec::new();
    if let Some(subject) = &view.subject {
        if subject.lens == lens {
            resources.push(subject.resource.clone());
        }
    }
    for item in view
        .meaning
        .iter()
        .chain(&view.descriptions)
        .chain(&view.code)
        .chain(&view.verification)
        .chain(&view.other)
    {
        if item.endpoint.lens == lens {
            resources.push(item.endpoint.resource.clone());
        }
    }
    resources.sort();
    resources.dedup();
    resources
}

fn reflected_resources(view: &ProjectReflectionReadModel) -> Vec<ResourceRef> {
    let mut resources = view
        .meaning
        .iter()
        .chain(&view.descriptions)
        .chain(&view.code)
        .chain(&view.verification)
        .chain(&view.other)
        .map(|item| item.endpoint.resource.clone())
        .collect::<Vec<_>>();
    if let Some(subject) = &view.subject {
        resources.push(subject.resource.clone());
    }
    resources.sort();
    resources.dedup();
    resources
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::context_resolution::{compose_context_resolution, RequestedActors};
    use crate::project::{ProjectBinding, ProjectBindingLocator, ProjectConstituentRef};
    use crate::resolve::{resolve, ResolveRequest};
    use crate::resource::{MemoryResourceIndex, ResourceDescriptor, ResourceKind, ResourceRecord};
    use crate::{AlwaysTrusted, ContextDescriptor, ManagedPolicy, MemoryCatalog, ProjectRef};

    fn resolution() -> ContextResolution {
        let catalog = MemoryCatalog::default();
        let trust = AlwaysTrusted;
        let deterministic = resolve(
            &catalog,
            &trust,
            &ResolveRequest {
                context: ContextDescriptor::for_project("/tmp/project"),
                layers: vec![],
                policy: ManagedPolicy::default(),
            },
        )
        .unwrap();
        let mut resources = MemoryResourceIndex::default();
        resources.insert(ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse("skill/aikit/knowledge-navigation").unwrap(),
            ResourceKind::Capability,
            "knowledge-navigation",
            "navigate",
        )));
        compose_context_resolution(
            &deterministic,
            ProjectBinding::new(
                ProjectRef::parse("example/project").unwrap(),
                ProjectConstituentRef::parse("example/project").unwrap(),
                ProjectBindingLocator::LocalDirectory {
                    path: PathBuf::from("/tmp/project"),
                },
            ),
            &[],
            &resources,
            RequestedActors::default(),
        )
    }

    #[test]
    fn ordinary_project_needs_no_central_coordinates_or_methods() {
        let receipt = project_recovery(&resolution(), None, &[], None, None);
        assert!(!receipt.act_authority_inferred);
        assert!(receipt.stages.iter().any(|stage| {
            stage.kind == ProjectRecoveryStageKind::AuthoredGround
                && stage.state == ProjectRecoveryStageState::OptionalAbsent
        }));
        assert!(receipt.stages.iter().any(|stage| {
            stage.kind == ProjectRecoveryStageKind::ProjectReflection
                && stage.state == ProjectRecoveryStageState::OptionalAbsent
        }));
        assert!(receipt.stages.iter().any(|stage| {
            stage.kind == ProjectRecoveryStageKind::CapabilityPraxis
                && stage.state == ProjectRecoveryStageState::Available
        }));
    }

    #[test]
    fn filename_hint_remains_partial_not_project_truth() {
        let local = ProjectLocalSourceBinding {
            source: SourceRef::parse("source:project-local:example:AGENTS.md").unwrap(),
            classification: LocalSourceClassification::Classified(
                crate::project_reflection::LocalSourceRoleEvidence {
                    role: LocalSourceRole::AgentGovernance,
                    evidence: vec!["conventional-filename-hint".into()],
                    authoritative: false,
                },
            ),
        };
        let receipt = project_recovery(&resolution(), None, &[local], None, None);
        assert!(receipt.stages.iter().any(|stage| {
            stage.kind == ProjectRecoveryStageKind::NativeProjectSource
                && stage.state == ProjectRecoveryStageState::Partial
        }));
    }

    #[test]
    fn partial_central_ground_creates_recognition_pressure_not_fake_authorship() {
        let central = ProjectCentralOrientation {
            project: ProjectRef::parse("example/project").unwrap(),
            project_id: "example/project".into(),
            human_root: SourceRef::parse("source:central:example:human-root").unwrap(),
            human_material_count: 2,
            recognised_human_source_count: 0,
            ground_status: ProjectCentralGroundStatus::Partial,
            governance_present: true,
            canonical_wiki: SourceRef::parse("source:central:example:wiki").unwrap(),
            canonical_wiki_exists: true,
            adopted_wikis: vec![],
            root_wiki: None,
            ground_relations: Some(SourceRef::parse("source:central:example:relations").unwrap()),
            native_project_root: SourceRef::parse("source:project:example:root").unwrap(),
            optional_account_capabilities: vec![],
        };
        let receipt = project_recovery(&resolution(), Some(&central), &[], None, None);
        assert_eq!(receipt.recognition_pressure.len(), 1);
        assert!(receipt.stages.iter().any(|stage| {
            stage.kind == ProjectRecoveryStageKind::AuthoredGround
                && stage.state == ProjectRecoveryStageState::Partial
        }));
    }
}
