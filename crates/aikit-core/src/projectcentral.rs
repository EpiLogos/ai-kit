//! Pure AIKit binding semantics for Central/ProjectCentral.
//!
//! Central owns filesystem/source identity. This module consumes the public
//! ProjectCentral contract and gives Knowledge Navigation typed descriptors for
//! discovery, authority, smallest-sufficient project entry, Wiki maintenance and
//! source-return proposals. Filesystem I/O lives in `aikit-adapters`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::context_source::{
    AgentVisibility, ContextSourceEntry, ContextSourceOperationalState, ContextSourcePrivacy,
    ContextSourceScope, DisclosureState, ExternalEgress, Freshness,
};
use crate::knowledge_wiki::{SemanticRevision, WikiObject, WikiProvenanceRef};
use crate::knowledge_wiki_index::SemanticWikiIndex;
use crate::project::ProjectRef;
use crate::resource::{
    Eligibility, ProviderOffer, ProviderRef, ProviderState, ResourceDescriptor, ResourceKind,
    ResourceLocator, ResourceRecord, ResourceRef, ResourceSource, SourceAuthority, SourceRef,
    SourceRevision, SourceState,
};
use crate::{AikitError, Result};

pub const PROJECTCENTRAL_BINDING_VERSION: &str = "aikit.projectcentral-binding/v1";
pub const CENTRAL_PROJECT_SCHEMA: &str = "central.project/v1";
pub const CENTRAL_WIKI_PROFILE: &str = "okf-wiki/v1";
pub const CENTRAL_GROUND_RELATIONS_SCHEMA: &str = "central.project.ground-relations/v1";
pub const PROJECTCENTRAL_HUMAN_ROOT: &str = "ProjectCentral/user";
pub const PROJECTCENTRAL_GOVERNANCE_ROOT: &str = "ProjectCentral/agents/governance";
pub const PROJECTCENTRAL_WIKI_SOURCE: &str = "ProjectCentral/agents/wiki/wiki.json";
pub const PROJECTCENTRAL_GROUND_RELATIONS_SOURCE: &str =
    "ProjectCentral/relations/source-relations.json";
pub const CENTRAL_ROOT_WIKI_SOURCE: &str = "Control/agents/wiki/wiki.json";
pub const NO_AGENT_RETRIEVAL_MARKER: &str = ".no-agent-retrieval";
pub const PROJECTCENTRAL_FILESYSTEM_PROVIDER: &str = "provider/projectcentral/filesystem";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectCentralStanding {
    HumanAuthored,
    HumanGovernance,
    AgentMaintained,
    Observed,
    InferredDerived,
    Generated,
    Unresolved,
    NativeProject,
}

impl ProjectCentralStanding {
    pub fn source_authority(self) -> Option<SourceAuthority> {
        match self {
            Self::HumanAuthored | Self::HumanGovernance => Some(SourceAuthority::Authored),
            Self::AgentMaintained => Some(SourceAuthority::Learned),
            Self::Observed | Self::NativeProject => Some(SourceAuthority::Observed),
            Self::InferredDerived => Some(SourceAuthority::Derived),
            Self::Generated => Some(SourceAuthority::Generated),
            Self::Unresolved => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::HumanAuthored => "human-authored",
            Self::HumanGovernance => "human-governance",
            Self::AgentMaintained => "agent-maintained",
            Self::Observed => "observed",
            Self::InferredDerived => "inferred-derived",
            Self::Generated => "generated",
            Self::Unresolved => "unresolved",
            Self::NativeProject => "native-project",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectCentralProvenance {
    HumanAuthored,
    HumanEditedDraft,
    HumanAdopted,
    GeneratedSuggestion,
    GeneratedDerived,
    AgentMaintained,
    Observed,
    Inference,
    #[default]
    Unresolved,
}

impl ProjectCentralProvenance {
    pub fn is_recognised_human_source(self) -> bool {
        matches!(self, Self::HumanAuthored | Self::HumanAdopted)
    }

    pub fn operational_standing(self) -> ProjectCentralStanding {
        match self {
            Self::HumanAuthored | Self::HumanAdopted => ProjectCentralStanding::HumanAuthored,
            Self::AgentMaintained => ProjectCentralStanding::AgentMaintained,
            Self::Observed => ProjectCentralStanding::Observed,
            Self::Inference | Self::GeneratedDerived => ProjectCentralStanding::InferredDerived,
            Self::GeneratedSuggestion => ProjectCentralStanding::Generated,
            Self::HumanEditedDraft | Self::Unresolved => ProjectCentralStanding::Unresolved,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::HumanAuthored => "human-authored",
            Self::HumanEditedDraft => "human-edited-draft",
            Self::HumanAdopted => "human-adopted",
            Self::GeneratedSuggestion => "generated-suggestion",
            Self::GeneratedDerived => "generated-derived",
            Self::AgentMaintained => "agent-maintained",
            Self::Observed => "observed",
            Self::Inference => "inference",
            Self::Unresolved => "unresolved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectCentralTruthStanding {
    #[default]
    Unspecified,
    AuthoredHumanPosition,
    DesignCommitment,
    ArchitectureContract,
    ImplementationFact,
    ObservedEvidence,
    CurrentDevelopmentState,
    AgentInference,
}

impl ProjectCentralTruthStanding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::AuthoredHumanPosition => "authored-human-position",
            Self::DesignCommitment => "design-commitment",
            Self::ArchitectureContract => "architecture-contract",
            Self::ImplementationFact => "implementation-fact",
            Self::ObservedEvidence => "observed-evidence",
            Self::CurrentDevelopmentState => "current-development-state",
            Self::AgentInference => "agent-inference",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectCentralTreatment {
    ProjectcentralUser,
    RetainNativeInPlace,
    OrdinaryProjectSource,
    GeneratedDerived,
    #[default]
    Unresolved,
}

impl ProjectCentralTreatment {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProjectcentralUser => "projectcentral-user",
            Self::RetainNativeInPlace => "retain-native-in-place",
            Self::OrdinaryProjectSource => "ordinary-project-source",
            Self::GeneratedDerived => "generated-derived",
            Self::Unresolved => "unresolved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectCentralGroundStatus {
    Empty,
    Partial,
    Established,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectCentralSourceKind {
    Manifest,
    GroundRelations,
    HumanRoot,
    HumanMaterial,
    RelatedProjectSource,
    GovernanceRoot,
    GovernanceMaterial,
    CanonicalWiki,
    AdoptedWiki,
    RootWiki,
    NativeProjectRoot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCentralSourceDescriptor {
    pub source: SourceRef,
    pub relative_path: PathBuf,
    pub kind: ProjectCentralSourceKind,
    pub standing: ProjectCentralStanding,
    #[serde(default)]
    pub provenance: ProjectCentralProvenance,
    #[serde(default)]
    pub truth_standing: ProjectCentralTruthStanding,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub treatment: ProjectCentralTreatment,
    #[serde(default)]
    pub recognition: Option<String>,
    pub exists: bool,
    pub agent_readable: bool,
    pub is_directory: bool,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCentralBinding {
    pub version: String,
    pub project: ProjectRef,
    pub project_id: String,
    pub manifest_source: SourceRef,
    pub human_root: SourceRef,
    pub governance_root: SourceRef,
    pub canonical_wiki: SourceRef,
    #[serde(default)]
    pub adopted_wikis: Vec<SourceRef>,
    #[serde(default)]
    pub root_wiki: Option<SourceRef>,
    #[serde(default)]
    pub ground_relations: Option<SourceRef>,
    pub native_project_root: SourceRef,
    #[serde(default)]
    pub sources: Vec<ProjectCentralSourceDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCentralOrientation {
    pub project: ProjectRef,
    pub project_id: String,
    pub human_root: SourceRef,
    pub human_material_count: usize,
    pub recognised_human_source_count: usize,
    pub ground_status: ProjectCentralGroundStatus,
    pub governance_present: bool,
    pub canonical_wiki: SourceRef,
    pub canonical_wiki_exists: bool,
    #[serde(default)]
    pub adopted_wikis: Vec<SourceRef>,
    #[serde(default)]
    pub root_wiki: Option<SourceRef>,
    #[serde(default)]
    pub ground_relations: Option<SourceRef>,
    pub native_project_root: SourceRef,
    /// Skills are disclosed as available powers; project entry does not execute them.
    pub optional_account_capabilities: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCentralAccountSource {
    pub source: SourceRef,
    pub relative_path: PathBuf,
    pub provenance: ProjectCentralProvenance,
    pub truth_standing: ProjectCentralTruthStanding,
    #[serde(default)]
    pub roles: Vec<String>,
    pub treatment: ProjectCentralTreatment,
}

impl From<&ProjectCentralSourceDescriptor> for ProjectCentralAccountSource {
    fn from(value: &ProjectCentralSourceDescriptor) -> Self {
        Self {
            source: value.source.clone(),
            relative_path: value.relative_path.clone(),
            provenance: value.provenance,
            truth_standing: value.truth_standing,
            roles: value.roles.clone(),
            treatment: value.treatment,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCentralAccountContext {
    pub project: ProjectRef,
    /// Preferred human-owned aperture. This identity does not assert authorship of every child.
    pub preferred_authored_aperture: SourceRef,
    /// Only explicitly recognised human-authored/adopted sources, including retained native files.
    pub preferred_human_sources: Vec<ProjectCentralAccountSource>,
    /// Other Project-ground relations, including unresolved material and non-human standings.
    pub other_source_relations: Vec<ProjectCentralAccountSource>,
    pub agent_wiki: SourceRef,
    #[serde(default)]
    pub ground_relations: Option<SourceRef>,
    pub native_project_root: SourceRef,
    pub capabilities: Vec<ResourceRef>,
}

impl ProjectCentralBinding {
    pub fn orientation(&self) -> Result<ProjectCentralOrientation> {
        let account_sources = self
            .sources
            .iter()
            .filter(|source| {
                source.exists
                    && source.agent_readable
                    && matches!(
                        source.kind,
                        ProjectCentralSourceKind::HumanMaterial
                            | ProjectCentralSourceKind::RelatedProjectSource
                    )
            })
            .collect::<Vec<_>>();
        let human_material_count = account_sources
            .iter()
            .filter(|source| source.kind == ProjectCentralSourceKind::HumanMaterial)
            .count();
        let recognised_human_source_count = account_sources
            .iter()
            .filter(|source| source.provenance.is_recognised_human_source())
            .count();
        let ground_status = if recognised_human_source_count > 0 {
            ProjectCentralGroundStatus::Established
        } else if account_sources.is_empty() {
            ProjectCentralGroundStatus::Empty
        } else {
            ProjectCentralGroundStatus::Partial
        };
        let governance_present = self.sources.iter().any(|source| {
            matches!(
                source.kind,
                ProjectCentralSourceKind::GovernanceRoot
                    | ProjectCentralSourceKind::GovernanceMaterial
            ) && source.exists
        });
        let canonical_wiki_exists = self.sources.iter().any(|source| {
            source.kind == ProjectCentralSourceKind::CanonicalWiki && source.exists
        });
        Ok(ProjectCentralOrientation {
            project: self.project.clone(),
            project_id: self.project_id.clone(),
            human_root: self.human_root.clone(),
            human_material_count,
            recognised_human_source_count,
            ground_status,
            governance_present,
            canonical_wiki: self.canonical_wiki.clone(),
            canonical_wiki_exists,
            adopted_wikis: self.adopted_wikis.clone(),
            root_wiki: self.root_wiki.clone(),
            ground_relations: self.ground_relations.clone(),
            native_project_root: self.native_project_root.clone(),
            optional_account_capabilities: account_capabilities()?,
        })
    }

    pub fn account_context(&self) -> Result<ProjectCentralAccountContext> {
        let mut account_sources = self
            .sources
            .iter()
            .filter(|source| {
                source.exists
                    && source.agent_readable
                    && matches!(
                        source.kind,
                        ProjectCentralSourceKind::HumanMaterial
                            | ProjectCentralSourceKind::RelatedProjectSource
                    )
            })
            .collect::<Vec<_>>();
        account_sources.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

        let preferred_human_sources = account_sources
            .iter()
            .filter(|source| source.provenance.is_recognised_human_source())
            .map(|source| ProjectCentralAccountSource::from(*source))
            .collect();
        let other_source_relations = account_sources
            .iter()
            .filter(|source| !source.provenance.is_recognised_human_source())
            .map(|source| ProjectCentralAccountSource::from(*source))
            .collect();

        Ok(ProjectCentralAccountContext {
            project: self.project.clone(),
            preferred_authored_aperture: self.human_root.clone(),
            preferred_human_sources,
            other_source_relations,
            agent_wiki: self.canonical_wiki.clone(),
            ground_relations: self.ground_relations.clone(),
            native_project_root: self.native_project_root.clone(),
            capabilities: account_capabilities()?,
        })
    }

    /// Build descriptor-only ContextSources. No source payload and no Wiki body is
    /// loaded here: `retrieved` and `focused` remain false until explicit use.
    pub fn context_sources(&self) -> Result<Vec<ContextSourceEntry>> {
        let provider = ProviderRef::parse(PROJECTCENTRAL_FILESYSTEM_PROVIDER)?;
        self.sources
            .iter()
            .filter(|source| {
                source.exists
                    && source.agent_readable
                    && !source.is_directory
                    && matches!(
                        source.kind,
                        ProjectCentralSourceKind::HumanMaterial
                            | ProjectCentralSourceKind::RelatedProjectSource
                            | ProjectCentralSourceKind::GovernanceMaterial
                    )
            })
            .map(|source| {
                let id = ResourceRef::parse(source.source.as_str())?;
                let mut descriptor = ResourceDescriptor::new(
                    id,
                    ResourceKind::ContextSource,
                    source.relative_path.display().to_string(),
                    "ProjectCentral source known to exist; payload is retrieved only on explicit read",
                );
                descriptor.annotations.insert(
                    "central.standing".into(),
                    source.standing.as_str().into(),
                );
                descriptor.annotations.insert(
                    "central.provenance".into(),
                    source.provenance.as_str().into(),
                );
                descriptor.annotations.insert(
                    "central.truth-standing".into(),
                    source.truth_standing.as_str().into(),
                );
                descriptor.annotations.insert(
                    "central.treatment".into(),
                    source.treatment.as_str().into(),
                );
                descriptor.annotations.insert(
                    "central.path".into(),
                    source.relative_path.display().to_string(),
                );
                if !source.roles.is_empty() {
                    descriptor
                        .annotations
                        .insert("central.roles".into(), source.roles.join(","));
                }
                if let Some(recognition) = &source.recognition {
                    descriptor
                        .annotations
                        .insert("central.recognition".into(), recognition.clone());
                }
                descriptor.sources.push(ResourceSource {
                    source: source.source.clone(),
                    authority: source.standing.source_authority(),
                    revision: source.revision.clone(),
                    locator: Some(ResourceLocator::Path(source.relative_path.clone())),
                    state: SourceState::Available,
                });
                let mut record = ResourceRecord::new(descriptor);
                record.eligibility = Eligibility::Eligible;
                record.providers.push(ProviderOffer {
                    provider: provider.clone(),
                    locator: Some(ResourceLocator::Path(source.relative_path.clone())),
                    state: ProviderState::Available,
                });
                let mut entry = ContextSourceEntry::new(record)?;
                entry.relation = ContextSourceScope {
                    project: Some(self.project.clone()),
                    scope: None,
                };
                entry.freshness = Freshness::Current;
                entry.disclosure = DisclosureState {
                    exists: true,
                    known_to_exist: true,
                    askable: true,
                    retrieved: false,
                    focused: false,
                };
                entry.privacy = ContextSourcePrivacy {
                    agent_visibility: AgentVisibility::Payload,
                    external_egress: ExternalEgress::Denied,
                };
                entry.operational = ContextSourceOperationalState {
                    enabled: true,
                    projected: false,
                    loaded: false,
                    invoked: false,
                };
                Ok(entry)
            })
            .collect()
    }
}

fn account_capabilities() -> Result<Vec<ResourceRef>> {
    Ok(vec![
        ResourceRef::parse("skill:product-understanding")?,
        ResourceRef::parse("skill:structured-account-authoring")?,
        ResourceRef::parse("skill:projection-authoring")?,
        ResourceRef::parse("skill:html-account")?,
    ])
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanSourceRevisionProposal {
    pub source: SourceRef,
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentWikiMaintenanceRequest {
    pub current_objects: Vec<WikiObject>,
    pub upserts: Vec<WikiObject>,
    pub observed_source_revisions: BTreeMap<SourceRef, SemanticRevision>,
    pub human_source_proposals: Vec<HumanSourceRevisionProposal>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentWikiMaintenancePlan {
    pub current_index_revision: String,
    pub stale_resources: Vec<ResourceRef>,
    pub next_objects: Vec<WikiObject>,
    /// Decision pressure only. Applying this plan never mutates human source.
    pub human_source_proposals: Vec<HumanSourceRevisionProposal>,
}

pub fn plan_agent_wiki_maintenance(
    request: AgentWikiMaintenanceRequest,
) -> Result<AgentWikiMaintenancePlan> {
    let index = SemanticWikiIndex::rebuild(request.current_objects.clone())?;
    let mut stale = BTreeSet::new();
    for object in &request.current_objects {
        for provenance in provenance(object) {
            let Some(observed) = request
                .observed_source_revisions
                .get(&provenance.source_ref)
            else {
                continue;
            };
            if provenance.source_revision.as_ref() != Some(observed) {
                stale.insert(object.ref_id().clone());
            }
        }
    }

    let mut next = request
        .current_objects
        .into_iter()
        .map(|object| (object.ref_id().clone(), object))
        .collect::<BTreeMap<_, _>>();

    for object in request.upserts {
        object.validate()?;
        require_maintenance_provenance(&object)?;
        if let Some(existing) = next.get(object.ref_id()) {
            if object.revision() <= existing.revision() {
                return Err(AikitError::new(
                    "projectcentral.wiki_revision_not_advanced",
                    "Agent Wiki upsert must advance the canonical object revision",
                )
                .with("resource", object.ref_id().to_string()));
            }
        }
        next.insert(object.ref_id().clone(), object);
    }

    // Rebuild proves that the proposed whole remains a valid SemanticWiki before
    // any adapter is allowed to persist it.
    let next_objects = next.into_values().collect::<Vec<_>>();
    SemanticWikiIndex::rebuild(next_objects.clone())?;

    Ok(AgentWikiMaintenancePlan {
        current_index_revision: index.revision().to_string(),
        stale_resources: stale.into_iter().collect(),
        next_objects,
        human_source_proposals: request.human_source_proposals,
    })
}

fn provenance(object: &WikiObject) -> &[WikiProvenanceRef] {
    match object {
        WikiObject::Space(value) => &value.provenance,
        WikiObject::Node(value) => &value.provenance,
        WikiObject::Edge(value) => &value.provenance,
        WikiObject::Frame(value) => &value.provenance,
        WikiObject::Reading(value) => &value.provenance,
    }
}

fn require_maintenance_provenance(object: &WikiObject) -> Result<()> {
    if matches!(object, WikiObject::Node(_) | WikiObject::Edge(_) | WikiObject::Reading(_))
        && provenance(object).is_empty()
    {
        return Err(AikitError::new(
            "projectcentral.wiki_provenance_required",
            "Agent-maintained knowledge upserts require exact provenance",
        )
        .with("resource", object.ref_id().to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_wiki::{WikiNode, WikiProvenanceRef};

    fn source(path: &str) -> SourceRef {
        SourceRef::parse(path).unwrap()
    }

    fn binding() -> ProjectCentralBinding {
        ProjectCentralBinding {
            version: PROJECTCENTRAL_BINDING_VERSION.into(),
            project: ProjectRef::parse("epilogos/test").unwrap(),
            project_id: "epilogos/test".into(),
            manifest_source: source("source:central:test:manifest"),
            human_root: source("source:central:test:human-root"),
            governance_root: source("source:central:test:governance-root"),
            canonical_wiki: source("source:central:test:agent-wiki"),
            adopted_wikis: vec![],
            root_wiki: None,
            ground_relations: Some(source("source:central:test:ground-relations")),
            native_project_root: source("source:project:test:root"),
            sources: vec![ProjectCentralSourceDescriptor {
                source: source("central:project-source:epilogos/test:0000000000000001"),
                relative_path: PathBuf::from("ProjectCentral/user/notes/deep.md"),
                kind: ProjectCentralSourceKind::HumanMaterial,
                standing: ProjectCentralStanding::HumanAuthored,
                provenance: ProjectCentralProvenance::HumanAuthored,
                truth_standing: ProjectCentralTruthStanding::AuthoredHumanPosition,
                roles: vec!["purpose".into()],
                treatment: ProjectCentralTreatment::ProjectcentralUser,
                recognition: Some("human-accepted source relation".into()),
                exists: true,
                agent_readable: true,
                is_directory: false,
                revision: Some(SourceRevision::parse("r1").unwrap()),
            }],
        }
    }

    #[test]
    fn project_entry_discloses_without_retrieving_or_focusing() {
        let entries = binding().context_sources().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].disclosure.exists);
        assert!(entries[0].disclosure.known_to_exist);
        assert!(entries[0].disclosure.askable);
        assert!(!entries[0].disclosure.retrieved);
        assert!(!entries[0].disclosure.focused);
        assert!(!entries[0].operational.loaded);
    }

    #[test]
    fn account_context_preserves_human_source_standing_without_executing_account_craft() {
        let context = binding().account_context().unwrap();
        assert_eq!(context.preferred_human_sources.len(), 1);
        assert_eq!(
            context.preferred_human_sources[0].truth_standing,
            ProjectCentralTruthStanding::AuthoredHumanPosition
        );
        assert_eq!(context.agent_wiki.as_str(), "source:central:test:agent-wiki");
        assert!(context
            .capabilities
            .iter()
            .any(|item| item.as_str() == "skill:structured-account-authoring"));
    }

    #[test]
    fn unresolved_human_aperture_material_does_not_acquire_authored_authority() {
        let mut value = binding();
        value.sources[0].standing = ProjectCentralStanding::Unresolved;
        value.sources[0].provenance = ProjectCentralProvenance::Unresolved;
        value.sources[0].truth_standing = ProjectCentralTruthStanding::Unspecified;
        value.sources[0].recognition = None;
        let context = value.account_context().unwrap();
        assert!(context.preferred_human_sources.is_empty());
        assert_eq!(context.other_source_relations.len(), 1);
        let entries = value.context_sources().unwrap();
        assert!(entries[0].resource.descriptor.sources[0].authority.is_none());
    }

    #[test]
    fn maintenance_detects_stale_source_and_keeps_human_pressure_as_proposal() {
        let src = source("central:project-source:epilogos/test:0000000000000001");
        let node = WikiObject::Node(WikiNode {
            profile: CENTRAL_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse("wiki:node:vision").unwrap(),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: src.clone(),
                source_revision: Some(SemanticRevision::Text("old".into())),
                producer_ref: None,
                generation_ref: None,
                extensions: BTreeMap::new(),
            }],
            node_type: "ProjectPosition".into(),
            title: Some("Vision".into()),
            space_refs: vec![],
            source_refs: vec![src.clone()],
            local_space_ref: None,
            extensions: BTreeMap::new(),
        });
        let mut observed = BTreeMap::new();
        observed.insert(src.clone(), SemanticRevision::Text("new".into()));
        let plan = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects: vec![node],
            upserts: vec![],
            observed_source_revisions: observed,
            human_source_proposals: vec![HumanSourceRevisionProposal {
                source: src.clone(),
                reason: "implementation evidence challenges the standing wording".into(),
                evidence: vec![source("source:test:evidence")],
            }],
        })
        .unwrap();
        assert_eq!(plan.stale_resources[0].as_str(), "wiki:node:vision");
        assert_eq!(plan.human_source_proposals[0].source, src);
    }
}
