//! V2 ContextSource disclosure and retrieval-over-injection seam.
//!
//! Canonical identity, ownership, source provenance, provider offers and hard
//! eligibility remain on `ResourceRecord`. This module adds only derived,
//! rebuildable disclosure/search state and provider-owned retrieval. Source
//! payloads are never stored in the index.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::context_resolution::{availability, Availability};
use crate::project::ProjectRef;
use crate::resource::{
    Eligibility, ProviderRef, ProviderState, ResourceKind, ResourceRecord, ResourceRef,
    ResourceSource, SourceRevision,
};
use crate::scope::ScopeKind;
use crate::{AikitError, Result};

pub const CONTEXT_SOURCE_INDEX_VERSION: &str = "aikit.context-source-index/v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AbsenceKind {
    Open,
    Latent,
    Unknown,
    Irrelevant,
    Bound,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredAbsence {
    pub kind: AbsenceKind,
    pub reason: String,
}

impl StructuredAbsence {
    pub fn new(kind: AbsenceKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
        }
    }
}

/// Epistemic state. No field here implies any other field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DisclosureState {
    pub exists: bool,
    pub known_to_exist: bool,
    pub askable: bool,
    pub retrieved: bool,
    pub focused: bool,
}

/// Operational state is deliberately separate from disclosure state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContextSourceOperationalState {
    pub enabled: bool,
    pub projected: bool,
    pub loaded: bool,
    pub invoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Freshness {
    Current,
    Stale {
        reason: String,
    },
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentVisibility {
    Payload,
    MetadataOnly,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalEgress {
    Allowed,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSourcePrivacy {
    pub agent_visibility: AgentVisibility,
    pub external_egress: ExternalEgress,
}

impl Default for ContextSourcePrivacy {
    fn default() -> Self {
        Self {
            agent_visibility: AgentVisibility::Payload,
            external_egress: ExternalEgress::Denied,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContextSourceScope {
    #[serde(default)]
    pub project: Option<ProjectRef>,
    #[serde(default)]
    pub scope: Option<ScopeKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextSourceOperation {
    Discover,
    Search,
    Read,
    Resolve,
    Explain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContextSourceProviderCapabilities {
    #[serde(default)]
    pub operations: BTreeSet<ContextSourceOperation>,
}

impl ContextSourceProviderCapabilities {
    pub fn supports(&self, operation: ContextSourceOperation) -> bool {
        self.operations.contains(&operation)
    }

    pub fn with_operations(operations: impl IntoIterator<Item = ContextSourceOperation>) -> Self {
        Self {
            operations: operations.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ContextSourceProviderStatus {
    Available,
    Degraded { reason: String },
    Unavailable { reason: String },
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSourceProviderDescriptor {
    pub status: ContextSourceProviderStatus,
    pub capabilities: ContextSourceProviderCapabilities,
}

/// Rebuildable index entry. It intentionally has no payload field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSourceEntry {
    pub resource: ResourceRecord,
    #[serde(default)]
    pub relation: ContextSourceScope,
    #[serde(default)]
    pub freshness: Freshness,
    #[serde(default)]
    pub disclosure: DisclosureState,
    #[serde(default)]
    pub privacy: ContextSourcePrivacy,
    #[serde(default)]
    pub operational: ContextSourceOperationalState,
    #[serde(default)]
    pub provider_descriptors: BTreeMap<ProviderRef, ContextSourceProviderDescriptor>,
    #[serde(default)]
    pub absence: Option<StructuredAbsence>,
}

impl ContextSourceEntry {
    pub fn new(resource: ResourceRecord) -> Result<Self> {
        if resource.descriptor.kind != ResourceKind::ContextSource {
            return Err(AikitError::new(
                "context_source.wrong_resource_kind",
                format!(
                    "{} is {}, expected context-source",
                    resource.descriptor.id,
                    resource.descriptor.kind.as_str()
                ),
            ));
        }
        Ok(Self {
            resource,
            relation: ContextSourceScope::default(),
            freshness: Freshness::Unknown,
            disclosure: DisclosureState::default(),
            privacy: ContextSourcePrivacy::default(),
            operational: ContextSourceOperationalState::default(),
            provider_descriptors: BTreeMap::new(),
            absence: None,
        })
    }

    pub fn id(&self) -> &ResourceRef {
        &self.resource.descriptor.id
    }

    pub fn availability(&self) -> Availability {
        availability(&self.resource)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SearchAudience {
    Human,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HorizonRequest {
    pub project: Option<ProjectRef>,
    pub audience: SearchAudience,
}

impl HorizonRequest {
    pub fn human(project: Option<ProjectRef>) -> Self {
        Self {
            project,
            audience: SearchAudience::Human,
        }
    }

    pub fn agent(project: Option<ProjectRef>) -> Self {
        Self {
            project,
            audience: SearchAudience::Agent,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSourceHit {
    pub resource: ResourceRef,
    pub name: String,
    pub relation: ContextSourceScope,
    pub freshness: Freshness,
    pub disclosure: DisclosureState,
    pub availability: Availability,
    pub eligibility: Eligibility,
    pub sources: Vec<ResourceSource>,
    pub providers: Vec<ProviderRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSourceExplanation {
    pub resource: ResourceRef,
    pub relation: ContextSourceScope,
    pub freshness: Freshness,
    pub disclosure: DisclosureState,
    pub operational: ContextSourceOperationalState,
    pub privacy: ContextSourcePrivacy,
    pub availability: Availability,
    pub eligibility: Eligibility,
    pub sources: Vec<ResourceSource>,
    pub providers: Vec<(ProviderRef, ProviderState)>,
    pub provider_descriptors: BTreeMap<ProviderRef, ContextSourceProviderDescriptor>,
    #[serde(default)]
    pub absence: Option<StructuredAbsence>,
}

/// Descriptor-only index. Search cannot cause provider reads or prompt loading.
#[derive(Debug, Clone, Default)]
pub struct ContextSourceIndex {
    entries: BTreeMap<ResourceRef, ContextSourceEntry>,
}

impl ContextSourceIndex {
    pub fn insert(&mut self, entry: ContextSourceEntry) -> Option<ContextSourceEntry> {
        self.entries.insert(entry.id().clone(), entry)
    }

    pub fn get(&self, resource: &ResourceRef) -> Option<&ContextSourceEntry> {
        self.entries.get(resource)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn horizon(&self, request: &HorizonRequest) -> Vec<ContextSourceHit> {
        self.entries
            .values()
            .filter(|entry| visible_in_horizon(entry, request))
            .map(hit)
            .collect()
    }

    pub fn search(&self, request: &HorizonRequest, query: &str) -> Vec<ContextSourceHit> {
        let needle = query.to_lowercase();
        let mut matches: Vec<(u8, ContextSourceHit)> = self
            .entries
            .values()
            .filter(|entry| visible_in_horizon(entry, request))
            .filter_map(|entry| {
                let id = entry.id().as_str().to_lowercase();
                let name = entry.resource.descriptor.name.to_lowercase();
                let description = entry.resource.descriptor.description.to_lowercase();
                let score = if needle.is_empty() {
                    4
                } else if id == needle || name == needle {
                    0
                } else if id.starts_with(&needle) || name.starts_with(&needle) {
                    1
                } else if id.contains(&needle) || name.contains(&needle) {
                    2
                } else if description.contains(&needle) {
                    3
                } else {
                    return None;
                };
                Some((score, hit(entry)))
            })
            .collect();
        matches.sort_by(|(left_score, left), (right_score, right)| {
            left_score
                .cmp(right_score)
                .then_with(|| left.resource.cmp(&right.resource))
        });
        matches.into_iter().map(|(_, hit)| hit).collect()
    }

    pub fn explain(&self, resource: &ResourceRef) -> Option<ContextSourceExplanation> {
        self.entries
            .get(resource)
            .map(|entry| ContextSourceExplanation {
                resource: entry.id().clone(),
                relation: entry.relation.clone(),
                freshness: entry.freshness.clone(),
                disclosure: entry.disclosure,
                operational: entry.operational,
                privacy: entry.privacy,
                availability: entry.availability(),
                eligibility: entry.resource.eligibility.clone(),
                sources: entry.resource.descriptor.sources.clone(),
                providers: entry
                    .resource
                    .providers
                    .iter()
                    .map(|offer| (offer.provider.clone(), offer.state.clone()))
                    .collect(),
                provider_descriptors: entry.provider_descriptors.clone(),
                absence: entry.absence.clone(),
            })
    }

    pub fn set_disclosure(
        &mut self,
        resource: &ResourceRef,
        disclosure: DisclosureState,
    ) -> Result<()> {
        self.entry_mut(resource)?.disclosure = disclosure;
        Ok(())
    }

    pub fn set_loaded(&mut self, resource: &ResourceRef, loaded: bool) -> Result<()> {
        self.entry_mut(resource)?.operational.loaded = loaded;
        Ok(())
    }

    pub fn set_focused(&mut self, resource: &ResourceRef, focused: bool) -> Result<()> {
        self.entry_mut(resource)?.disclosure.focused = focused;
        Ok(())
    }

    pub fn retrieve(
        &mut self,
        request: &ContextSourceReadRequest,
        provider: &mut dyn ContextSourceProvider,
    ) -> ContextSourceReadOutcome {
        let Some(entry) = self.entries.get_mut(&request.resource) else {
            return absent(AbsenceKind::Missing, "ContextSource is not indexed");
        };
        if provider.provider() != &request.provider {
            return absent(
                AbsenceKind::Missing,
                "provider binding does not match request",
            );
        }
        if !entry.resource.eligibility.is_eligible() {
            return absent(AbsenceKind::Bound, "ContextSource is not eligible");
        }
        if !entry.disclosure.askable {
            return absent(
                AbsenceKind::Latent,
                "ContextSource is not presently askable",
            );
        }
        if let Some(bound) = privacy_boundary(entry.privacy, request.target) {
            return ContextSourceReadOutcome::Absent(bound);
        }

        let Some(offer) = entry
            .resource
            .providers
            .iter()
            .find(|offer| offer.provider == request.provider)
        else {
            return absent(
                AbsenceKind::Missing,
                "ContextSource has no matching provider offer",
            );
        };
        match &offer.state {
            ProviderState::Unresolved => {
                return absent(AbsenceKind::Unknown, "provider availability is unresolved");
            }
            ProviderState::Unavailable { .. } => {
                return absent(
                    AbsenceKind::Missing,
                    "ContextSource provider is unavailable",
                );
            }
            ProviderState::Available => {}
        }

        let provider_status = provider.status();
        match &provider_status {
            ContextSourceProviderStatus::Unresolved => {
                return absent(
                    AbsenceKind::Unknown,
                    "provider capability state is unresolved",
                );
            }
            ContextSourceProviderStatus::Unavailable { .. } => {
                return absent(AbsenceKind::Missing, "provider capability is unavailable");
            }
            ContextSourceProviderStatus::Available
            | ContextSourceProviderStatus::Degraded { .. } => {}
        }
        let capabilities = provider.capabilities();
        if !capabilities.supports(ContextSourceOperation::Read) {
            return absent(
                AbsenceKind::Bound,
                "provider does not advertise read capability",
            );
        }

        entry.provider_descriptors.insert(
            request.provider.clone(),
            ContextSourceProviderDescriptor {
                status: provider_status,
                capabilities,
            },
        );
        entry.operational.invoked = true;

        match provider.read(request) {
            ProviderReadResult::Retrieved {
                payload,
                revision,
                provenance,
            } => {
                entry.disclosure.retrieved = true;
                let mut combined_provenance = entry.resource.descriptor.sources.clone();
                combined_provenance.extend(provenance);
                ContextSourceReadOutcome::Retrieved(ContextSourceRetrieval {
                    resource: entry.id().clone(),
                    provider: request.provider.clone(),
                    payload,
                    revision: revision.or_else(|| source_revision(&entry.resource)),
                    freshness: entry.freshness.clone(),
                    provenance: combined_provenance,
                    eligibility: entry.resource.eligibility.clone(),
                })
            }
            ProviderReadResult::Absent(absence) => ContextSourceReadOutcome::Absent(absence),
        }
    }

    fn entry_mut(&mut self, resource: &ResourceRef) -> Result<&mut ContextSourceEntry> {
        self.entries.get_mut(resource).ok_or_else(|| {
            AikitError::new(
                "context_source.missing",
                format!("ContextSource {resource} is not indexed"),
            )
        })
    }
}

fn visible_in_horizon(entry: &ContextSourceEntry, request: &HorizonRequest) -> bool {
    if !entry.disclosure.known_to_exist || !entry.resource.eligibility.is_eligible() {
        return false;
    }
    if let (Some(request_project), Some(source_project)) =
        (request.project.as_ref(), entry.relation.project.as_ref())
    {
        if request_project != source_project {
            return false;
        }
    }
    match request.audience {
        SearchAudience::Human => true,
        SearchAudience::Agent => !matches!(entry.privacy.agent_visibility, AgentVisibility::Hidden),
    }
}

fn hit(entry: &ContextSourceEntry) -> ContextSourceHit {
    ContextSourceHit {
        resource: entry.id().clone(),
        name: entry.resource.descriptor.name.clone(),
        relation: entry.relation.clone(),
        freshness: entry.freshness.clone(),
        disclosure: entry.disclosure,
        availability: entry.availability(),
        eligibility: entry.resource.eligibility.clone(),
        sources: entry.resource.descriptor.sources.clone(),
        providers: entry
            .resource
            .providers
            .iter()
            .map(|offer| offer.provider.clone())
            .collect(),
    }
}

fn source_revision(resource: &ResourceRecord) -> Option<SourceRevision> {
    resource
        .descriptor
        .sources
        .iter()
        .find_map(|source| source.revision.clone())
}

fn absent(kind: AbsenceKind, reason: &str) -> ContextSourceReadOutcome {
    ContextSourceReadOutcome::Absent(StructuredAbsence::new(kind, reason))
}

fn privacy_boundary(
    privacy: ContextSourcePrivacy,
    target: RetrievalTarget,
) -> Option<StructuredAbsence> {
    match target {
        RetrievalTarget::Human => None,
        RetrievalTarget::LocalAgent => {
            if matches!(privacy.agent_visibility, AgentVisibility::Payload) {
                None
            } else {
                Some(StructuredAbsence::new(
                    AbsenceKind::Bound,
                    "payload is not agent-visible",
                ))
            }
        }
        RetrievalTarget::ExternalProvider => {
            if !matches!(privacy.agent_visibility, AgentVisibility::Payload) {
                return Some(StructuredAbsence::new(
                    AbsenceKind::Bound,
                    "payload is not agent-visible",
                ));
            }
            if matches!(privacy.external_egress, ExternalEgress::Denied) {
                return Some(StructuredAbsence::new(
                    AbsenceKind::Bound,
                    "payload is not eligible for external-provider egress",
                ));
            }
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetrievalTarget {
    Human,
    LocalAgent,
    ExternalProvider,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSourceReadRequest {
    pub resource: ResourceRef,
    pub provider: ProviderRef,
    pub target: RetrievalTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSourceRetrieval {
    pub resource: ResourceRef,
    pub provider: ProviderRef,
    pub payload: String,
    #[serde(default)]
    pub revision: Option<SourceRevision>,
    pub freshness: Freshness,
    pub provenance: Vec<ResourceSource>,
    pub eligibility: Eligibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "kebab-case")]
pub enum ContextSourceReadOutcome {
    Retrieved(ContextSourceRetrieval),
    Absent(StructuredAbsence),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderReadResult {
    Retrieved {
        payload: String,
        revision: Option<SourceRevision>,
        provenance: Vec<ResourceSource>,
    },
    Absent(StructuredAbsence),
}

/// Provider-owned payload access. Specialised Wiki/SourcePool providers can extend
/// this seam without introducing a second Context system.
pub trait ContextSourceProvider {
    fn provider(&self) -> &ProviderRef;
    fn status(&self) -> ContextSourceProviderStatus;
    fn capabilities(&self) -> ContextSourceProviderCapabilities;
    fn read(&mut self, request: &ContextSourceReadRequest) -> ProviderReadResult;
}
