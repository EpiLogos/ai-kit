//! Development Field carrier and bounded read substrate.
//!
//! AIKit does not own the source facts carried here. Central owns source identity
//! and standing, QL owns shape/operator meaning, Factory owns developmental
//! meaning, and Workcell owns material lifecycle. This module gives those
//! owner-native facts one ResourceRef-addressable projection without creating a
//! second source store or inferring relations from paths, names, or co-location.

use serde::{Deserialize, Serialize};

use crate::{AikitError, Result};

use super::{
    OwnerRef, ResourceDescriptor, ResourceIndex, ResourceKind, ResourceRecord, ResourceRef,
    ResourceSource, SourceRef, SourceRevision, VersionRevision, VersionedProjectWorld,
};

pub const DEVELOPMENT_FIELD_BINDING_VERSION: &str = "aikit.development-field-binding/v1";
pub const DEVELOPMENT_FIELD_READING_VERSION: &str = "aikit.development-field-reading/v1";
pub const DEVELOPMENT_FIELD_BINDING_ANNOTATION: &str = "aikit.development-field-binding";
pub const DEFAULT_DEVELOPMENT_FIELD_READ_LIMIT: usize = 16;
pub const MAX_DEVELOPMENT_FIELD_READ_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DevelopmentFieldCarrierKind {
    SelfDescription,
    TierBinding,
    UserExperience,
    ExperienceMetadata,
    Evidence,
    Capability,
    Plan,
}

/// One owner-declared relation from a carrier to another stable ResourceRef.
///
/// `relation` deliberately remains owner vocabulary. AIKit transports the edge;
/// it does not convert an unfamiliar relation name into a semantic assertion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldRelation {
    pub relation: String,
    pub target: ResourceRef,
}

impl DevelopmentFieldRelation {
    pub fn new(relation: impl Into<String>, target: ResourceRef) -> Result<Self> {
        let relation = relation.into();
        if relation.trim().is_empty() {
            return Err(AikitError::new(
                "resource.development_field_relation_invalid",
                "Development Field relation name must not be empty",
            ));
        }
        Ok(Self { relation, target })
    }
}

/// One QL-owned shape address bound to an owner-native ResourceRef.
///
/// Presence in this list is structural addressability only. It never asserts a
/// semantic relation between the addressed members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlShapeMemberBinding {
    pub address_ref: String,
    pub member_ref: ResourceRef,
}

/// Portable view of a QL-owned ShapeBinding.
///
/// AIKit preserves these refs and provenance and validates only carrier
/// integrity. It does not reproduce QL's shape ontology, reject partial/developed
/// shapes, or generate meanings for addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlShapeBindingCarrier {
    pub contract_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_revision: Option<SourceRevision>,
    pub subject_ref: ResourceRef,
    pub shape_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whole_ref: Option<ResourceRef>,
    #[serde(default)]
    pub basis_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub member_bindings: Vec<QlShapeMemberBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_ref: Option<String>,
    #[serde(default)]
    pub return_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<ResourceSource>,
}

impl QlShapeBindingCarrier {
    fn validate(&self, subject: &ResourceRef) -> Result<()> {
        if &self.subject_ref != subject {
            return Err(AikitError::new(
                "resource.development_field_shape_subject_mismatch",
                format!(
                    "QL ShapeBinding subject {} does not match carrier {}",
                    self.subject_ref, subject
                ),
            ));
        }
        if self.contract_ref.trim().is_empty() || self.shape_ref.trim().is_empty() {
            return Err(AikitError::new(
                "resource.development_field_shape_invalid",
                "QL ShapeBinding must retain non-empty contract_ref and shape_ref",
            ));
        }
        if self
            .member_bindings
            .iter()
            .any(|binding| binding.address_ref.trim().is_empty())
        {
            return Err(AikitError::new(
                "resource.development_field_shape_address_invalid",
                "QL ShapeBinding member address_ref must not be empty",
            ));
        }
        Ok(())
    }
}

/// A reference into Workcell-owned material state. Workcell remains the owner of
/// lifecycle; the material identity never replaces Project/Session/Run identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkcellMaterialRef {
    pub material_ref: ResourceRef,
    pub workcell_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<SourceRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldBinding {
    pub version: String,
    pub carrier_kind: DevelopmentFieldCarrierKind,
    #[serde(default)]
    pub relations: Vec<DevelopmentFieldRelation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape_binding: Option<QlShapeBindingCarrier>,
    #[serde(default)]
    pub material_refs: Vec<WorkcellMaterialRef>,
}

impl DevelopmentFieldBinding {
    pub fn new(carrier_kind: DevelopmentFieldCarrierKind) -> Self {
        Self {
            version: DEVELOPMENT_FIELD_BINDING_VERSION.into(),
            carrier_kind,
            relations: Vec::new(),
            shape_binding: None,
            material_refs: Vec::new(),
        }
    }

    pub fn validate(&self, subject: &ResourceRef) -> Result<()> {
        if self.version != DEVELOPMENT_FIELD_BINDING_VERSION {
            return Err(AikitError::new(
                "resource.development_field_binding_version_unsupported",
                format!(
                    "unsupported Development Field binding version {}; expected {}",
                    self.version, DEVELOPMENT_FIELD_BINDING_VERSION
                ),
            ));
        }
        if let Some(binding) = &self.shape_binding {
            binding.validate(subject)?;
        }
        Ok(())
    }
}

/// Attach owner-supplied Development Field metadata to the existing derived
/// Resource record. The record's owner/source/provenance remain the authority;
/// this annotation is only the typed projection AIKit can rebuild from them.
pub fn attach_development_field_binding(
    record: &mut ResourceRecord,
    binding: &DevelopmentFieldBinding,
) -> Result<()> {
    binding.validate(&record.descriptor.id)?;
    let encoded = serde_json::to_string(binding).map_err(|error| {
        AikitError::new(
            "resource.development_field_binding_encode_failed",
            format!("could not encode Development Field binding: {error}"),
        )
    })?;
    record
        .descriptor
        .annotations
        .insert(DEVELOPMENT_FIELD_BINDING_ANNOTATION.into(), encoded);
    Ok(())
}

pub fn development_field_binding(record: &ResourceRecord) -> Result<Option<DevelopmentFieldBinding>> {
    let Some(raw) = record
        .descriptor
        .annotations
        .get(DEVELOPMENT_FIELD_BINDING_ANNOTATION)
    else {
        return Ok(None);
    };
    let binding: DevelopmentFieldBinding = serde_json::from_str(raw).map_err(|error| {
        AikitError::new(
            "resource.development_field_binding_invalid",
            format!(
                "Development Field binding on {} is invalid: {error}",
                record.descriptor.id
            ),
        )
    })?;
    binding.validate(&record.descriptor.id)?;
    Ok(Some(binding))
}

/// Stable ingestion seam for an owner adapter. The descriptor retains the
/// native Resource kind, owner and sources; the binding only adds Development
/// Field addressability to that same record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldCarrierProjection {
    pub descriptor: ResourceDescriptor,
    pub binding: DevelopmentFieldBinding,
}

impl DevelopmentFieldCarrierProjection {
    pub fn into_record(self) -> Result<ResourceRecord> {
        let mut record = ResourceRecord::new(self.descriptor);
        attach_development_field_binding(&mut record, &self.binding)?;
        Ok(record)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DevelopmentFieldAvailabilityState {
    Available,
    Unknown,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldAvailability {
    pub state: DevelopmentFieldAvailabilityState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl DevelopmentFieldAvailability {
    pub fn available() -> Self {
        Self {
            state: DevelopmentFieldAvailabilityState::Available,
            reason: None,
        }
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Self {
            state: DevelopmentFieldAvailabilityState::Unknown,
            reason: Some(reason.into()),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            state: DevelopmentFieldAvailabilityState::Unavailable,
            reason: Some(reason.into()),
        }
    }
}

/// The exact tracked diff from a caller-supplied base to the currently observed
/// worktree. Untracked paths are disclosed separately because Git diff does not
/// include their contents without changing their standing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldCurrentDiff {
    pub base_revision: VersionRevision,
    pub observed_head: VersionRevision,
    pub patch: String,
    pub truncated: bool,
    #[serde(default)]
    pub untracked_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldGitBasis {
    pub world: VersionedProjectWorld,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<VersionRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_diff_from_base: Option<DevelopmentFieldCurrentDiff>,
}

impl DevelopmentFieldGitBasis {
    pub fn new(
        world: VersionedProjectWorld,
        base_revision: Option<VersionRevision>,
        current_diff_from_base: Option<DevelopmentFieldCurrentDiff>,
    ) -> Result<Self> {
        match (&base_revision, &current_diff_from_base) {
            (Some(base), Some(diff)) => {
                if &diff.base_revision != base || diff.observed_head != world.repository.head {
                    return Err(AikitError::new(
                        "resource.development_field_git_basis_mismatch",
                        "Development Field diff must retain the requested base and observed HEAD",
                    ));
                }
            }
            (None, Some(_)) => {
                return Err(AikitError::new(
                    "resource.development_field_git_basis_mismatch",
                    "Development Field diff cannot be supplied without an exact base revision",
                ));
            }
            (Some(_), None) | (None, None) => {}
        }
        Ok(Self {
            world,
            base_revision,
            current_diff_from_base,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DevelopmentFieldExecutableModality {
    Installed,
    Source,
    Developer,
    Unknown,
}

/// Provenance for the AIKit process that produced a reading. A checkout revision
/// is only populated when it is evidence for this executable; an installed
/// binary must not borrow the surrounding checkout's HEAD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldExecutableBasis {
    pub executable: String,
    pub package_version: String,
    pub modality: DevelopmentFieldExecutableModality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<VersionRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldSubjectReading {
    pub subject: ResourceRef,
    pub availability: DevelopmentFieldAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_kind: Option<ResourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<OwnerRef>,
    #[serde(default)]
    pub sources: Vec<ResourceSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier_kind: Option<DevelopmentFieldCarrierKind>,
    #[serde(default)]
    pub declared_relations: Vec<DevelopmentFieldRelation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape_binding: Option<QlShapeBindingCarrier>,
    #[serde(default)]
    pub material_refs: Vec<WorkcellMaterialRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldSelfDescriptionAperture {
    pub availability: DevelopmentFieldAvailability,
    #[serde(default)]
    pub self_description_refs: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldReadRequest {
    #[serde(default)]
    pub subjects: Vec<ResourceRef>,
    #[serde(default = "default_read_limit")]
    pub limit: usize,
}

fn default_read_limit() -> usize {
    DEFAULT_DEVELOPMENT_FIELD_READ_LIMIT
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentFieldReading {
    pub version: String,
    pub executable_basis: DevelopmentFieldExecutableBasis,
    pub git_basis: DevelopmentFieldAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<DevelopmentFieldGitBasis>,
    pub central_self_description: DevelopmentFieldSelfDescriptionAperture,
    pub subjects: Vec<DevelopmentFieldSubjectReading>,
    pub truncated: bool,
}

/// Build one bounded, deterministic reading over the current Resource field.
///
/// Empty `subjects` means "Development Field carriers already present in this
/// index"; it never triggers source discovery. Explicit subjects may address any
/// native resource and preserve an `Unknown` carrier state when no Development
/// Field binding was supplied.
pub fn read_development_field(
    index: &dyn ResourceIndex,
    request: &DevelopmentFieldReadRequest,
    executable_basis: DevelopmentFieldExecutableBasis,
    git_basis: Result<Option<DevelopmentFieldGitBasis>>,
) -> DevelopmentFieldReading {
    let limit = request.limit.clamp(1, MAX_DEVELOPMENT_FIELD_READ_LIMIT);
    let mut refs = if request.subjects.is_empty() {
        index
            .resources()
            .into_iter()
            .filter(|record| {
                record
                    .descriptor
                    .annotations
                    .contains_key(DEVELOPMENT_FIELD_BINDING_ANNOTATION)
            })
            .map(|record| record.descriptor.id.clone())
            .collect::<Vec<_>>()
    } else {
        request.subjects.clone()
    };
    refs.sort();
    refs.dedup();
    let truncated = refs.len() > limit;
    refs.truncate(limit);

    let subjects = refs
        .into_iter()
        .map(|subject| subject_reading(index, subject))
        .collect();

    let mut self_description_refs = index
        .resources()
        .into_iter()
        .filter_map(|record| match development_field_binding(record) {
            Ok(Some(binding))
                if binding.carrier_kind == DevelopmentFieldCarrierKind::SelfDescription =>
            {
                Some(record.descriptor.id.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    self_description_refs.sort();
    self_description_refs.dedup();
    let central_self_description = if self_description_refs.is_empty() {
        DevelopmentFieldSelfDescriptionAperture {
            availability: DevelopmentFieldAvailability::unknown(
                "no Central public self-description carrier is present in the Resource field; AIKit does not guess ProjectCentral/self paths",
            ),
            self_description_refs,
        }
    } else {
        DevelopmentFieldSelfDescriptionAperture {
            availability: DevelopmentFieldAvailability::available(),
            self_description_refs,
        }
    };

    let (git_basis_state, git) = match git_basis {
        Ok(Some(git)) => (DevelopmentFieldAvailability::available(), Some(git)),
        Ok(None) => (
            DevelopmentFieldAvailability::unknown(
                "no VersionedWorld observation is available for this application context",
            ),
            None,
        ),
        Err(error) => (
            DevelopmentFieldAvailability::unavailable(error.message().to_string()),
            None,
        ),
    };

    DevelopmentFieldReading {
        version: DEVELOPMENT_FIELD_READING_VERSION.into(),
        executable_basis,
        git_basis: git_basis_state,
        git,
        central_self_description,
        subjects,
        truncated,
    }
}

fn subject_reading(index: &dyn ResourceIndex, subject: ResourceRef) -> DevelopmentFieldSubjectReading {
    let Some(record) = index.resource(&subject) else {
        return DevelopmentFieldSubjectReading {
            subject,
            availability: DevelopmentFieldAvailability::unknown(
                "ResourceRef is not present in the current Resource field",
            ),
            resource_kind: None,
            owner: None,
            sources: Vec::new(),
            carrier_kind: None,
            declared_relations: Vec::new(),
            shape_binding: None,
            material_refs: Vec::new(),
        };
    };

    match development_field_binding(record) {
        Ok(binding) => DevelopmentFieldSubjectReading {
            subject,
            availability: DevelopmentFieldAvailability::available(),
            resource_kind: Some(record.descriptor.kind),
            owner: record.descriptor.owner.clone(),
            sources: record.descriptor.sources.clone(),
            carrier_kind: binding.as_ref().map(|binding| binding.carrier_kind),
            declared_relations: binding
                .as_ref()
                .map(|binding| binding.relations.clone())
                .unwrap_or_default(),
            shape_binding: binding
                .as_ref()
                .and_then(|binding| binding.shape_binding.clone()),
            material_refs: binding
                .map(|binding| binding.material_refs)
                .unwrap_or_default(),
        },
        Err(error) => DevelopmentFieldSubjectReading {
            subject,
            availability: DevelopmentFieldAvailability::unavailable(error.message().to_string()),
            resource_kind: Some(record.descriptor.kind),
            owner: record.descriptor.owner.clone(),
            sources: record.descriptor.sources.clone(),
            carrier_kind: None,
            declared_relations: Vec::new(),
            shape_binding: None,
            material_refs: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectRef;
    use crate::resource::{
        GitRepositoryRelation, GitWorkingState, MemoryResourceIndex, SourceAuthority, SourceState,
        VersionedWorldProviderDescriptor, VersionedWorldProviderStatus, VERSIONED_WORLD_VERSION,
    };

    fn source() -> ResourceSource {
        ResourceSource {
            source: SourceRef::parse("central:source:self").unwrap(),
            authority: Some(SourceAuthority::Authored),
            revision: Some(SourceRevision::parse("rev-17").unwrap()),
            locator: None,
            state: SourceState::Available,
        }
    }

    fn carrier(id: &str, kind: DevelopmentFieldCarrierKind) -> ResourceRecord {
        let mut descriptor = ResourceDescriptor::new(
            ResourceRef::parse(id).unwrap(),
            ResourceKind::KnowledgeSource,
            id,
            "owner-native Development Field carrier",
        );
        descriptor.owner = Some(OwnerRef::parse("central:project:aikit").unwrap());
        descriptor.sources.push(source());
        DevelopmentFieldCarrierProjection {
            descriptor,
            binding: DevelopmentFieldBinding::new(kind),
        }
        .into_record()
        .unwrap()
    }

    fn executable() -> DevelopmentFieldExecutableBasis {
        DevelopmentFieldExecutableBasis {
            executable: "/work/aikit/target/debug/aikit".into(),
            package_version: "0.1.0".into(),
            modality: DevelopmentFieldExecutableModality::Developer,
            source_revision: Some(VersionRevision::new("abc123")),
        }
    }

    #[test]
    fn carrier_projection_retains_native_owner_source_and_revision() {
        let record = carrier("central:self:aikit", DevelopmentFieldCarrierKind::SelfDescription);
        let binding = development_field_binding(&record).unwrap().unwrap();
        assert_eq!(record.descriptor.owner.unwrap().as_str(), "central:project:aikit");
        assert_eq!(record.descriptor.sources, vec![source()]);
        assert_eq!(binding.carrier_kind, DevelopmentFieldCarrierKind::SelfDescription);
    }

    #[test]
    fn partial_ql_shape_binding_is_carried_without_semantic_inference() {
        let mut record = carrier("central:tier:2", DevelopmentFieldCarrierKind::TierBinding);
        let mut binding = DevelopmentFieldBinding::new(DevelopmentFieldCarrierKind::TierBinding);
        binding.shape_binding = Some(QlShapeBindingCarrier {
            contract_ref: "ql.shape@1.0.0".into(),
            contract_revision: Some(SourceRevision::parse("ql-main:44ed3cd").unwrap()),
            subject_ref: record.descriptor.id.clone(),
            shape_ref: "ql:shape:1.0.0:constellation:partial-conjugate-9".into(),
            whole_ref: None,
            basis_refs: Vec::new(),
            member_bindings: vec![QlShapeMemberBinding {
                address_ref: "ql-address:0/0-prime".into(),
                member_ref: ResourceRef::parse("central:tier:member:0").unwrap(),
            }],
            derivation_ref: None,
            operator_ref: None,
            return_refs: Vec::new(),
            provenance: vec![source()],
        });
        attach_development_field_binding(&mut record, &binding).unwrap();
        let restored = development_field_binding(&record).unwrap().unwrap();
        assert!(restored.shape_binding.is_some());
        assert!(restored.relations.is_empty(), "a shape address is not a semantic relation");
    }

    #[test]
    fn bounded_read_reports_explicit_relations_and_unknown_refs_without_guessing() {
        let mut index = MemoryResourceIndex::default();
        let mut self_record = carrier(
            "central:self:aikit",
            DevelopmentFieldCarrierKind::SelfDescription,
        );
        let mut binding = development_field_binding(&self_record).unwrap().unwrap();
        binding.relations.push(
            DevelopmentFieldRelation::new(
                "ux_ref",
                ResourceRef::parse("central:ux:aikit").unwrap(),
            )
            .unwrap(),
        );
        attach_development_field_binding(&mut self_record, &binding).unwrap();
        index.insert(self_record);

        let request = DevelopmentFieldReadRequest {
            subjects: vec![
                ResourceRef::parse("central:self:aikit").unwrap(),
                ResourceRef::parse("central:missing").unwrap(),
            ],
            limit: 8,
        };
        let reading = read_development_field(&index, &request, executable(), Ok(None));
        assert_eq!(reading.subjects.len(), 2);
        let self_read = reading
            .subjects
            .iter()
            .find(|read| read.subject.as_str() == "central:self:aikit")
            .unwrap();
        assert_eq!(self_read.declared_relations.len(), 1);
        let missing = reading
            .subjects
            .iter()
            .find(|read| read.subject.as_str() == "central:missing")
            .unwrap();
        assert_eq!(missing.availability.state, DevelopmentFieldAvailabilityState::Unknown);
        assert_eq!(
            reading.central_self_description.availability.state,
            DevelopmentFieldAvailabilityState::Available
        );
    }

    #[test]
    fn no_self_description_carrier_is_an_explicit_unknown_not_a_guessed_path() {
        let index = MemoryResourceIndex::default();
        let reading = read_development_field(
            &index,
            &DevelopmentFieldReadRequest {
                subjects: Vec::new(),
                limit: 8,
            },
            executable(),
            Ok(None),
        );
        assert_eq!(
            reading.central_self_description.availability.state,
            DevelopmentFieldAvailabilityState::Unknown
        );
        assert!(reading
            .central_self_description
            .availability
            .reason
            .as_deref()
            .unwrap()
            .contains("does not guess ProjectCentral/self paths"));
    }

    #[test]
    fn workcell_material_ref_does_not_replace_the_subject_identity() {
        let mut record = carrier("factory:plan:42", DevelopmentFieldCarrierKind::Plan);
        let mut binding = development_field_binding(&record).unwrap().unwrap();
        binding.material_refs.push(WorkcellMaterialRef {
            material_ref: ResourceRef::parse("instance:claude:abcd").unwrap(),
            workcell_ref: ResourceRef::parse("workcell:local").unwrap(),
            source_ref: None,
            source_revision: None,
        });
        attach_development_field_binding(&mut record, &binding).unwrap();
        assert_eq!(record.descriptor.id.as_str(), "factory:plan:42");
        assert_eq!(
            development_field_binding(&record).unwrap().unwrap().material_refs[0]
                .material_ref
                .as_str(),
            "instance:claude:abcd"
        );
    }

    #[test]
    fn git_basis_rejects_a_diff_for_a_different_head() {
        let world = VersionedProjectWorld {
            version: VERSIONED_WORLD_VERSION.into(),
            project: ProjectRef::parse("project:aikit").unwrap(),
            provider: VersionedWorldProviderDescriptor {
                provider: super::super::ProviderRef::parse("provider:native-git").unwrap(),
                status: VersionedWorldProviderStatus::Available,
                capabilities: Vec::new(),
                implementation_version: None,
            },
            repository: GitRepositoryRelation {
                repository_root: "/work/aikit".into(),
                worktree_root: "/work/aikit".into(),
                head: VersionRevision::new("head-1"),
                branch: Some("main".into()),
                detached: false,
                upstream: None,
                ahead: 0,
                behind: 0,
            },
            working: GitWorkingState::default(),
            worktrees: Vec::new(),
        };
        let base = VersionRevision::new("base-1");
        let diff = DevelopmentFieldCurrentDiff {
            base_revision: base.clone(),
            observed_head: VersionRevision::new("head-2"),
            patch: String::new(),
            truncated: false,
            untracked_paths: Vec::new(),
        };
        assert!(DevelopmentFieldGitBasis::new(world, Some(base), Some(diff)).is_err());
    }
}