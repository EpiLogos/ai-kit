//! Filesystem adapter for Central's public ProjectCentral contract.
//!
//! Project entry reads only the small ProjectCentral manifest, Central's optional
//! accepted source-relation ledger, and filesystem metadata. Human material and
//! SemanticWiki payloads remain unloaded until an explicit ContextSource or Wiki
//! read. `.no-agent-retrieval` prunes a subtree before any descendant is disclosed.
//! Eager text and Wiki reads admit at most 16 MiB per source. Larger material
//! needs a separately bounded reading through its owning native source/tool
//! route; this adapter reports the budget refusal without truncating material.
//! A retained binding also checks its admitted physical root on Linux/macOS.
//! Retargeted or replaced roots require a fresh native inspect; other platforms
//! disclose unevidenced physical affiliation as unavailable. Device/inode
//! observations are material affiliation, never World, Project or Source identity.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use aikit_core::{
    parse_wiki_objects, AbsenceKind, AgentWikiMaintenancePlan, AikitError, ContextSourceOperation,
    ContextSourceProvider, ContextSourceProviderCapabilities, ContextSourceProviderStatus,
    ContextSourceReadRequest, Eligibility, ProjectCentralBinding, ProjectCentralProvenance,
    ProjectCentralSourceDescriptor, ProjectCentralSourceKind, ProjectCentralStanding,
    ProjectCentralTreatment, ProjectCentralTruthStanding, ProviderReadResult, ProviderRef,
    ResourceDescriptor, ResourceKind, ResourceLocator, ResourceRecord, ResourceRef, ResourceSource,
    Result, SourceAuthority, SourceRef, SourceRevision, SourceState, StructuredAbsence,
    CENTRAL_GROUND_RELATIONS_SCHEMA, CENTRAL_PROJECT_SCHEMA, CENTRAL_ROOT_GOVERNANCE_ROOT,
    CENTRAL_ROOT_SOURCE_REF_PREFIX, CENTRAL_ROOT_WIKI_SOURCE, CENTRAL_WIKI_PROFILE,
    NO_AGENT_RETRIEVAL_MARKER, PROJECTCENTRAL_BINDING_VERSION, PROJECTCENTRAL_FILESYSTEM_PROVIDER,
    PROJECTCENTRAL_GOVERNANCE_ROOT, PROJECTCENTRAL_GROUND_RELATIONS_SOURCE,
    PROJECTCENTRAL_HUMAN_ROOT, PROJECTCENTRAL_WIKI_SOURCE,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::Digest;

#[path = "wiki_publication.rs"]
pub mod publication;

const EAGER_SOURCE_BUDGET: u64 = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Manifest {
    schema: String,
    project_id: String,
    human_source: String,
    wiki: WikiBinding,
}

#[derive(Debug, Deserialize)]
struct WikiBinding {
    profile: String,
    source: String,
    #[serde(default)]
    adopted_sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GroundRelationsFile {
    schema: String,
    project_id: String,
    #[serde(default)]
    relations: Vec<GroundRelation>,
}

#[derive(Debug, Clone, Deserialize)]
struct GroundRelation {
    #[serde(rename = "ref")]
    source_ref: String,
    path: String,
    provenance: ProjectCentralProvenance,
    #[serde(rename = "standing")]
    truth_standing: ProjectCentralTruthStanding,
    #[serde(default)]
    roles: Vec<String>,
    treatment: ProjectCentralTreatment,
    recognition: String,
}

#[derive(Debug, Clone)]
struct BoundSourcePath {
    path: PathBuf,
    owner_root: PathBuf,
    root_affiliation: Option<RootAffiliation>,
    is_directory: bool,
    canonical_member: PathBuf,
    enclosing_root: Option<(PathBuf, Option<RootAffiliation>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RootAffiliation {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    device: u64,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    inode: u64,
}

fn root_affiliation(root: &Path) -> std::io::Result<Option<RootAffiliation>> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(root)?;
        Ok(metadata.is_dir().then_some(RootAffiliation {
            device: metadata.dev(),
            inode: metadata.ino(),
        }))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = root;
        Ok(None)
    }
}

fn require_root_affiliation(root: &Path, expected: Option<RootAffiliation>) -> Result<()> {
    let current = root_affiliation(root).map_err(|error| {
        source_read_error("projectcentral.source_unavailable", root, error)
            .with("observation_stage", "owner_root")
    })?;
    if current != expected {
        return Err(AikitError::new(
            "projectcentral.source_binding_changed",
            "the native root no longer matches this binding's admitted physical directory; inspect a fresh native binding",
        )
        .with("owner_root", root.display().to_string())
        .with("expected_physical_root", format!("{expected:?}"))
        .with("observed_physical_root", format!("{current:?}")));
    }
    Ok(())
}

fn canonical_member(root: &Path, path: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root).map_err(|error| {
        source_read_error("projectcentral.source_unavailable", root, error)
            .with("observation_stage", "owner_root")
    })?;
    let member = fs::canonicalize(path).map_err(|error| {
        source_read_error("projectcentral.source_unavailable", path, error)
            .with("observation_stage", "source_mapping")
    })?;
    let relative = member.strip_prefix(&root).map_err(|_| {
        AikitError::new(
            "projectcentral.source_binding_changed",
            "the declared source no longer maps inside its admitted native root",
        ).with("path", path.display().to_string())
    })?;
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(AikitError::new(
            "projectcentral.source_escape",
            "the admitted source requires a nonempty normal native member",
        ).with("path", path.display().to_string()));
    }
    Ok(relative.to_path_buf())
}

fn enclosing_readable(
    enclosing: &Option<(PathBuf, Option<RootAffiliation>)>,
    path: &Path,
) -> Result<bool> {
    let Some((root, affiliation)) = enclosing else {
        return Ok(true);
    };
    require_root_affiliation(root, *affiliation)?;
    let member = canonical_member(root, path)?;
    if let Ok(declared_member) = path.strip_prefix(root) {
        if !path_agent_readability(root, declared_member).map_err(|error| {
            source_read_error("projectcentral.source_unavailable", path, error)
                .with("observation_stage", "read_admission")
        })? {
            return Ok(false);
        }
    }
    let readable = path_agent_readability(root, &member).map_err(|error| {
        source_read_error("projectcentral.source_unavailable", path, error)
            .with("observation_stage", "read_admission")
    })?;
    require_root_affiliation(root, *affiliation)?;
    Ok(readable)
}

impl BoundSourcePath {
    fn new(path: PathBuf, owner_root: PathBuf, is_directory: bool) -> Result<Self> {
        let root_affiliation = root_affiliation(&owner_root).map_err(|error| {
            source_read_error("projectcentral.source_unavailable", &owner_root, error)
                .with("observation_stage", "owner_root")
        })?;
        let canonical_member = canonical_member(&owner_root, &path)?;
        require_root_affiliation(&owner_root, root_affiliation)?;
        Ok(Self {
            path, owner_root, root_affiliation, is_directory, canonical_member,
            enclosing_root: None,
        })
    }

    fn require_owner(&self) -> Result<()> {
        let Some(expected) = self.root_affiliation else {
            return Err(AikitError::new(
                "projectcentral.source_unavailable",
                "native physical root affiliation cannot be evidenced on this platform",
            ).with("owner_root", self.owner_root.display().to_string())
                .with("observation_stage", "owner_root"));
        };
        require_root_affiliation(&self.owner_root, Some(expected))
    }

    fn physical_root_identity(&self) -> Result<(u64, u64)> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Some(expected) = self.root_affiliation {
            return Ok((expected.device, expected.inode));
        }
        Err(AikitError::new(
            "projectcentral.source_unavailable",
            "native physical root affiliation cannot be evidenced on this platform",
        ).with("owner_root", self.owner_root.display().to_string())
            .with("observation_stage", "owner_root"))
    }

    fn material_bytes(&self) -> Result<Vec<u8>> {
        publication::material_bytes_affiliated(
            &self.owner_root, self.physical_root_identity()?,
            &self.canonical_member, EAGER_SOURCE_BUDGET,
        ).map_err(|error| self.source_observation_error(error))
    }

    fn source_observation_error(&self, error: AikitError) -> AikitError {
        // Preserve the first observation's actual cause. A current failed owner
        // route is supplementary evidence, not evidence that the Source itself
        // was deleted, and must not manufacture a replacement IO error.
        let route = (|| -> Result<()> {
            self.require_owner()?;
            let root = fs::canonicalize(&self.owner_root).map_err(|cause| {
                source_read_error("projectcentral.source_unavailable", &self.owner_root, cause)
                    .with("observation_stage", "owner_root")
            })?;
            let parent = self.path.parent().ok_or_else(|| {
                AikitError::new("projectcentral.source_escape", "bound source has no native parent")
                    .with("observation_stage", "source_mapping")
            })?;
            let current = fs::canonicalize(parent).map_err(|cause| {
                source_read_error("projectcentral.source_unavailable", parent, cause)
                    .with("observation_stage", "owner_parent")
            })?;
            let expected = root.join(self.canonical_member.parent().unwrap_or(Path::new("")));
            if current != expected {
                return Err(AikitError::new(
                    "projectcentral.source_binding_changed",
                    "the original source parent no longer maps to this binding's admitted native parent",
                ).with("observation_stage", "source_mapping")
                    .with("expected_parent", expected.display().to_string())
                    .with("observed_parent", current.display().to_string()));
            }
            self.require_owner()
        })();
        match route {
            Ok(()) => error,
            Err(cause) => {
                let stage = cause.details().get("observation_stage")
                    .cloned().unwrap_or_else(|| "owner_root".into());
                let observation = serde_json::json!({
                    "code": cause.code(), "message": cause.message(), "details": cause.details(),
                }).to_string();
                error.with("observation_stage", stage)
                    .with("binding_route_cause", observation)
            }
        }
    }

    fn require_mapping_and_form(&self) -> Result<()> {
        self.require_owner()?;
        let current = fs::symlink_metadata(&self.path).map_err(|error| {
            self.source_observation_error(
                source_read_error("projectcentral.source_unavailable", &self.path, error)
                    .with("observation_stage", "source"),
            )
        })?;
        if current.file_type().is_symlink() || current.is_dir() != self.is_directory
            || (!current.is_file() && !current.is_dir())
        {
            return Err(AikitError::new(
                "projectcentral.source_binding_changed",
                "the source no longer matches this binding's inspected physical form; inspect a fresh native binding",
            )
            .with("path", self.path.display().to_string())
            .with("expected_is_directory", self.is_directory.to_string())
            .with("observed_is_directory", current.is_dir().to_string()));
        }
        let member = canonical_member(&self.owner_root, &self.path)?;
        if member != self.canonical_member {
            return Err(AikitError::new(
                "projectcentral.source_binding_changed",
                "the declared source no longer maps to this binding's admitted native member; inspect a fresh native binding",
            )
            .with("path", self.path.display().to_string())
            .with("expected_member", self.canonical_member.display().to_string())
            .with("observed_member", member.display().to_string()));
        }
        self.require_owner()
    }

    fn readable(&self) -> Result<bool> {
        self.require_owner()?;
        let relative = self.path.strip_prefix(&self.owner_root).map_err(|_| {
            AikitError::new(
                "projectcentral.source_escape",
                "bound ProjectCentral source escaped its native owner root",
            )
        })?;
        if !path_agent_readability(&self.owner_root, relative).map_err(|error| {
            source_read_error("projectcentral.source_unavailable", &self.path, error)
                .with("observation_stage", "read_admission")
        })? {
            return Ok(false);
        }
        // Retained mapping/form is material continuity, not a read grant or a
        // frozen source inode: native publication may legitimately replace it.
        self.require_mapping_and_form()?;
        if !enclosing_readable(&self.enclosing_root, &self.path)? {
            return Ok(false);
        }
        self.require_mapping_and_form()?;
        Ok(true)
    }

    fn require_readable(&self, source: &SourceRef) -> Result<()> {
        match self.readable() {
            Ok(true) => Ok(()),
            Ok(false) => Err(AikitError::new(
                "projectcentral.source_withheld",
                "ProjectCentral source is withheld from the current read",
            )
            .with("source", source.to_string())
            .with("path", self.path.display().to_string())),
            Err(error) => Err(error.with("source", source.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectCentralFilesystemBinding {
    pub semantic: ProjectCentralBinding,
    project_root: PathBuf,
    paths: BTreeMap<ResourceRef, BoundSourcePath>,
    standing: BTreeMap<ResourceRef, ProjectCentralStanding>,
}

impl ProjectCentralFilesystemBinding {
    /// The context binding of this validated native Central Project. Directory
    /// basenames and legacy AIKit specification ids never replace its identity.
    pub fn project_binding(&self) -> Result<aikit_core::project::ProjectBinding> {
        let mut binding = aikit_core::project::ProjectBinding::new(
            self.semantic.project.clone(),
            aikit_core::ProjectConstituentRef::parse(self.semantic.native_project_root.as_str())?,
            aikit_core::project::ProjectBindingLocator::LocalDirectory {
                path: self.project_root.clone(),
            },
        );
        binding.source = Some(self.semantic.manifest_source.clone());
        binding.provider = Some(aikit_core::ProviderRef::parse(
            PROJECTCENTRAL_FILESYSTEM_PROVIDER,
        )?);
        Ok(binding)
    }

    pub fn inspect(project_root: impl AsRef<Path>, central_root: Option<&Path>) -> Result<Self> {
        let project_root = project_root.as_ref().to_path_buf();
        let project_affiliation = root_affiliation(&project_root).map_err(|error| {
            source_read_error("projectcentral.source_unavailable", &project_root, error)
                .with("observation_stage", "owner_root")
        })?;
        let central_affiliation = central_root.map(|root| {
            root_affiliation(root).map_err(|error| {
                source_read_error("projectcentral.source_unavailable", root, error)
                    .with("observation_stage", "owner_root")
            })
        }).transpose()?;
        // A genuinely supplied enclosing World governs reads only when the
        // Project's canonical physical membership is actually established.
        // Failed canonical observation is unavailable, never external admission.
        let enclosing_root = if let (Some(root), Some(affiliation)) =
            (central_root, central_affiliation)
        {
            let physical_project = fs::canonicalize(&project_root).map_err(|error| {
                source_read_error("projectcentral.source_unavailable", &project_root, error)
                    .with("observation_stage", "owner_root")
            })?;
            let physical_central = fs::canonicalize(root).map_err(|error| {
                source_read_error("projectcentral.source_unavailable", root, error)
                    .with("observation_stage", "owner_root")
            })?;
            physical_project.starts_with(&physical_central)
                .then(|| (root.to_path_buf(), affiliation))
        } else {
            None
        };
        let manifest_path = project_root.join("ProjectCentral/project.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .map_err(|error| io_error("projectcentral.manifest_read", &manifest_path, error))?;
        #[cfg(test)]
        tests::after_manifest_read(&manifest_path);
        require_root_affiliation(&project_root, project_affiliation)?;
        if let (Some(root), Some(expected)) = (central_root, central_affiliation) {
            require_root_affiliation(root, expected)?;
        }
        let manifest: Manifest = serde_json::from_str(&manifest_text).map_err(|error| {
            AikitError::new(
                "projectcentral.manifest_invalid",
                format!("invalid ProjectCentral/project.json: {error}"),
            )
        })?;
        validate_manifest(&manifest)?;

        let ground_relations_file = read_ground_relations(&project_root, &manifest.project_id)?;
        let mut relations_by_path = BTreeMap::<PathBuf, GroundRelation>::new();
        if let Some(relations) = &ground_relations_file {
            for relation in &relations.relations {
                validate_relative_source(&relation.path)?;
                let path = PathBuf::from(&relation.path);
                if relations_by_path
                    .insert(path.clone(), relation.clone())
                    .is_some()
                {
                    return Err(AikitError::new(
                        "projectcentral.ground_relation_duplicate_path",
                        format!(
                            "{} contains more than one accepted relation for {}",
                            PROJECTCENTRAL_GROUND_RELATIONS_SOURCE,
                            path.display()
                        ),
                    ));
                }
            }
        }

        let project = aikit_core::ProjectRef::parse(&manifest.project_id)?;
        let manifest_source = source(&format!("source:central:{}:manifest", manifest.project_id))?;
        let human_root = source(&format!(
            "source:central:{}:human-root",
            manifest.project_id
        ))?;
        let governance_root = source(&format!(
            "source:central:{}:governance-root",
            manifest.project_id
        ))?;
        let canonical_wiki = source(&format!(
            "source:central:{}:agent-wiki",
            manifest.project_id
        ))?;
        let native_project_root = source(&format!("source:project:{}:root", manifest.project_id))?;

        let mut sources = Vec::new();
        let mut paths = BTreeMap::new();
        let mut standings = BTreeMap::new();
        let mut consumed_relations = BTreeSet::<PathBuf>::new();

        push_source(
            &mut sources,
            &mut paths,
            &mut standings,
            &project_root,
            manifest_source.clone(),
            PathBuf::from("ProjectCentral/project.json"),
            ProjectCentralSourceKind::Manifest,
            ProjectCentralStanding::Observed,
            ProjectCentralProvenance::Observed,
            ProjectCentralTruthStanding::Unspecified,
            Vec::new(),
            ProjectCentralTreatment::Unresolved,
            None,
            true,
            false,
        )?;

        let ground_relations = if ground_relations_file.is_some() {
            let relation_ref = source(&format!(
                "source:central:{}:ground-relations",
                manifest.project_id
            ))?;
            push_source(
                &mut sources,
                &mut paths,
                &mut standings,
                &project_root,
                relation_ref.clone(),
                PathBuf::from(PROJECTCENTRAL_GROUND_RELATIONS_SOURCE),
                ProjectCentralSourceKind::GroundRelations,
                ProjectCentralStanding::Observed,
                ProjectCentralProvenance::Observed,
                ProjectCentralTruthStanding::Unspecified,
                Vec::new(),
                ProjectCentralTreatment::Unresolved,
                None,
                true,
                false,
            )?;
            Some(relation_ref)
        } else {
            None
        };

        let human_path = project_root.join(PROJECTCENTRAL_HUMAN_ROOT);
        let human_allowed = human_path.exists()
            && path_agent_readable(&project_root, Path::new(PROJECTCENTRAL_HUMAN_ROOT))
            && enclosing_readable(&enclosing_root, &human_path)?;
        push_source(
            &mut sources,
            &mut paths,
            &mut standings,
            &project_root,
            human_root.clone(),
            PathBuf::from(PROJECTCENTRAL_HUMAN_ROOT),
            ProjectCentralSourceKind::HumanRoot,
            ProjectCentralStanding::Unresolved,
            ProjectCentralProvenance::Unresolved,
            ProjectCentralTruthStanding::Unspecified,
            Vec::new(),
            ProjectCentralTreatment::ProjectcentralUser,
            None,
            human_allowed,
            true,
        )?;
        if human_allowed {
            scan_human_tree(
                &project_root,
                &human_path,
                &manifest.project_id,
                &relations_by_path,
                &mut consumed_relations,
                &mut sources,
                &mut paths,
                &mut standings,
            )?;
        }

        let governance_path = project_root.join(PROJECTCENTRAL_GOVERNANCE_ROOT);
        let governance_allowed = governance_path.exists()
            && path_agent_readable(&project_root, Path::new(PROJECTCENTRAL_GOVERNANCE_ROOT))
            && enclosing_readable(&enclosing_root, &governance_path)?;
        push_source(
            &mut sources,
            &mut paths,
            &mut standings,
            &project_root,
            governance_root.clone(),
            PathBuf::from(PROJECTCENTRAL_GOVERNANCE_ROOT),
            ProjectCentralSourceKind::GovernanceRoot,
            ProjectCentralStanding::HumanGovernance,
            ProjectCentralProvenance::HumanAuthored,
            ProjectCentralTruthStanding::Unspecified,
            Vec::new(),
            ProjectCentralTreatment::Unresolved,
            None,
            governance_allowed,
            true,
        )?;
        if governance_allowed {
            scan_governance_tree(
                &project_root,
                &governance_path,
                &manifest.project_id,
                &mut sources,
                &mut paths,
                &mut standings,
            )?;
        }

        // Accepted relations may retain human-authored or evidential Project source
        // outside ProjectCentral/user. Preserve the Central-issued SourceRef and
        // provenance/standing without moving or copying the source.
        for (relative_path, relation) in &relations_by_path {
            if consumed_relations.contains(relative_path) {
                continue;
            }
            let kind = if relative_path.starts_with(PROJECTCENTRAL_HUMAN_ROOT) {
                ProjectCentralSourceKind::HumanMaterial
            } else {
                ProjectCentralSourceKind::RelatedProjectSource
            };
            let readable = path_agent_readable(&project_root, relative_path);
            push_source(
                &mut sources,
                &mut paths,
                &mut standings,
                &project_root,
                source(&relation.source_ref)?,
                relative_path.clone(),
                kind,
                relation.provenance.operational_standing(),
                relation.provenance,
                relation.truth_standing,
                relation.roles.clone(),
                relation.treatment,
                Some(relation.recognition.clone()),
                readable,
                false,
            )?;
        }

        push_source(
            &mut sources,
            &mut paths,
            &mut standings,
            &project_root,
            canonical_wiki.clone(),
            PathBuf::from(PROJECTCENTRAL_WIKI_SOURCE),
            ProjectCentralSourceKind::CanonicalWiki,
            ProjectCentralStanding::AgentMaintained,
            ProjectCentralProvenance::AgentMaintained,
            ProjectCentralTruthStanding::Unspecified,
            Vec::new(),
            ProjectCentralTreatment::GeneratedDerived,
            None,
            true,
            false,
        )?;

        let mut adopted_wikis = Vec::new();
        for (index, adopted) in manifest.wiki.adopted_sources.iter().enumerate() {
            validate_relative_source(adopted)?;
            let source_ref = source(&format!(
                "source:central:{}:adopted-wiki:{}",
                manifest.project_id, index
            ))?;
            push_source(
                &mut sources,
                &mut paths,
                &mut standings,
                &project_root,
                source_ref.clone(),
                PathBuf::from(adopted),
                ProjectCentralSourceKind::AdoptedWiki,
                ProjectCentralStanding::AgentMaintained,
                ProjectCentralProvenance::AgentMaintained,
                ProjectCentralTruthStanding::Unspecified,
                Vec::new(),
                ProjectCentralTreatment::GeneratedDerived,
                None,
                path_agent_readable(&project_root, Path::new(adopted)),
                false,
            )?;
            adopted_wikis.push(source_ref);
        }

        let root_wiki = if let Some(central_root) = central_root {
            let root_ref = source("source:central:root:agent-wiki")?;
            let root_path = central_root.join(CENTRAL_ROOT_WIKI_SOURCE);
            let exists = root_path.is_file() && !is_symlink(&root_path);
            let agent_readable = exists
                && path_agent_readable(central_root, Path::new(CENTRAL_ROOT_WIKI_SOURCE));
            let descriptor = ProjectCentralSourceDescriptor {
                source: root_ref.clone(),
                relative_path: PathBuf::from(CENTRAL_ROOT_WIKI_SOURCE),
                kind: ProjectCentralSourceKind::RootWiki,
                standing: ProjectCentralStanding::AgentMaintained,
                provenance: ProjectCentralProvenance::AgentMaintained,
                truth_standing: ProjectCentralTruthStanding::Unspecified,
                roles: Vec::new(),
                treatment: ProjectCentralTreatment::GeneratedDerived,
                recognition: None,
                exists,
                agent_readable,
                is_directory: false,
                revision: revision_for(&root_path),
            };
            let key = ResourceRef::parse(root_ref.as_str())?;
            if exists {
                paths.insert(
                    key.clone(),
                    BoundSourcePath::new(root_path, central_root.to_path_buf(), false)?,
                );
            }
            standings.insert(key, ProjectCentralStanding::AgentMaintained);
            sources.push(descriptor);
            Some(root_ref)
        } else {
            None
        };

        let native_key = ResourceRef::parse(native_project_root.as_str())?;
        standings.insert(native_key, ProjectCentralStanding::NativeProject);
        sources.push(ProjectCentralSourceDescriptor {
            source: native_project_root.clone(),
            relative_path: PathBuf::from("."),
            kind: ProjectCentralSourceKind::NativeProjectRoot,
            standing: ProjectCentralStanding::NativeProject,
            provenance: ProjectCentralProvenance::Observed,
            truth_standing: ProjectCentralTruthStanding::Unspecified,
            roles: Vec::new(),
            treatment: ProjectCentralTreatment::OrdinaryProjectSource,
            recognition: None,
            exists: project_root.is_dir(),
            agent_readable: true,
            is_directory: true,
            revision: revision_for(&project_root),
        });

        for bound in paths.values_mut() {
            if bound.owner_root == project_root {
                bound.enclosing_root = enclosing_root.clone();
            }
        }
        for descriptor in &mut sources {
            if descriptor.agent_readable {
                let key = ResourceRef::parse(descriptor.source.as_str())?;
                if let Some(bound) = paths.get(&key) {
                    descriptor.agent_readable = enclosing_readable(&bound.enclosing_root, &bound.path)?;
                }
            }
        }
        // Coherence checkpoints retain the actual root admitted before reading
        // native identity/relations. They do not exclude arbitrary external
        // namespace changes between checkpoints.
        require_root_affiliation(&project_root, project_affiliation)?;
        if let (Some(root), Some(expected)) = (central_root, central_affiliation) {
            require_root_affiliation(root, expected)?;
        }
        Ok(Self {
            semantic: ProjectCentralBinding {
                version: PROJECTCENTRAL_BINDING_VERSION.into(),
                project,
                project_id: manifest.project_id,
                manifest_source,
                human_root,
                governance_root,
                canonical_wiki,
                adopted_wikis,
                root_wiki,
                ground_relations,
                native_project_root,
                sources,
            },
            project_root,
            paths,
            standing: standings,
        })
    }

    pub fn file_provider(&self) -> Result<ProjectCentralFileProvider> {
        Ok(ProjectCentralFileProvider {
            provider: ProviderRef::parse(PROJECTCENTRAL_FILESYSTEM_PROVIDER)?,
            paths: self.paths.clone(),
            standing: self.standing.clone(),
        })
    }

    pub fn load_project_wiki(&self) -> Result<Vec<aikit_core::WikiObject>> {
        self.load_wiki(&self.semantic.canonical_wiki)
    }

    pub fn load_root_wiki(&self) -> Result<Option<Vec<aikit_core::WikiObject>>> {
        self.semantic
            .root_wiki
            .as_ref()
            .map(|source| self.load_wiki(source))
            .transpose()
    }

    pub fn load_adopted_wikis(&self) -> Result<Vec<(SourceRef, Vec<aikit_core::WikiObject>)>> {
        self.semantic
            .adopted_wikis
            .iter()
            .map(|source| Ok((source.clone(), self.load_wiki(source)?)))
            .collect()
    }

    pub fn load_wiki(&self, source: &SourceRef) -> Result<Vec<aikit_core::WikiObject>> {
        Ok(self.read_wiki_source(source)?.1)
    }

    /// Read the canonical Agent Wiki for a maintenance cycle, alongside the
    /// SHA-256 of the exact bytes read. The wiki is agent-maintained, so
    /// concurrent writers — two agents, or an agent and a human — are the
    /// normal case: this hash is the compare-and-swap base that must be
    /// passed back to `persist_agent_wiki`, captured at the same read that
    /// feeds `plan_agent_wiki_maintenance`'s `current_objects`, not from any
    /// later, separate read.
    pub fn load_project_wiki_for_maintenance(
        &self,
    ) -> Result<(Vec<aikit_core::WikiObject>, String)> {
        let (input, objects) = self.read_wiki_source(&self.semantic.canonical_wiki)?;
        Ok((objects, content_hash(input.as_bytes())))
    }

    fn read_wiki_source(
        &self,
        source: &SourceRef,
    ) -> Result<(String, Vec<aikit_core::WikiObject>)> {
        let key = ResourceRef::parse(source.as_str())?;
        let bound = self.paths.get(&key).ok_or_else(|| {
            AikitError::new(
                "projectcentral.source_unavailable",
                format!("ProjectCentral source {source} is not available"),
            )
        })?;
        bound.require_readable(source)?;
        let path = &bound.path;
        let bytes = bound.material_bytes()
            .map_err(|error| error.with("source", source.to_string()))?;
        #[cfg(test)]
        tests::after_source_read(path);
        bound.require_readable(source)?;
        let input = String::from_utf8(bytes).map_err(|error| {
            source_read_error("projectcentral.wiki_read", path,
                std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })?;
        let objects = parse_wiki_objects(&input)?;
        Ok((input, objects))
    }

    pub fn observed_source_revisions(&self) -> BTreeMap<SourceRef, aikit_core::SemanticRevision> {
        self.semantic
            .sources
            .iter()
            .filter_map(|source| {
                source.revision.as_ref().map(|revision| {
                    (
                        source.source.clone(),
                        aikit_core::SemanticRevision::Text(revision.to_string()),
                    )
                })
            })
            .collect()
    }

    /// Persist only the canonical Agent Wiki. Human source paths are not accepted
    /// by this operation, so a HumanSourceRevisionProposal can never become a
    /// filesystem mutation by accident.
    ///
    /// `base_hash` is the SHA-256 `load_project_wiki_for_maintenance` returned
    /// alongside the objects `plan` was built from. The wiki is agent-maintained,
    /// so concurrent writers are the normal case: this re-verifies that hash
    /// under the shared canonical publication lock through durable rename, and
    /// refuses with the same typed error rather than silently discarding a
    /// peer's write that landed first. A pre-publication refusal leaves the
    /// on-disk source exactly as the peer left it. Failed stages stay unpromoted;
    /// post-publication readback loss is returned as an uncertain effect.
    /// A rewrite that happens to land byte-identical
    /// content is never treated as a conflict.
    /// Returns the physical owner's actual changed acknowledgement: `false`
    /// leaves the exact bytes and modification time untouched.
    pub fn persist_agent_wiki(
        &self,
        plan: &AgentWikiMaintenancePlan,
        base_hash: &str,
    ) -> Result<bool> {
        let before_publication = |error: AikitError| error
            .with("source", self.semantic.canonical_wiki.to_string())
            .with("command_effect", "none");
        let key = ResourceRef::parse(self.semantic.canonical_wiki.as_str())
            .map_err(before_publication)?;
        let bound = self.paths.get(&key).ok_or_else(|| {
            AikitError::new(
                "projectcentral.canonical_wiki_unavailable",
                "canonical ProjectCentral Agent Wiki is unavailable",
            )
        }).map_err(before_publication)?;
        let path = &bound.path;
        // Physical affiliation/mapping is separate from read eligibility. This
        // semantic writer does not acquire a marker-derived mutation gate.
        bound.require_mapping_and_form().map_err(before_publication)?;
        let bytes = bound.material_bytes()
            .map_err(before_publication)?;
        bound.require_mapping_and_form().map_err(before_publication)?;
        let input = String::from_utf8(bytes).map_err(|error| {
            before_publication(source_read_error("projectcentral.wiki_read", path,
                std::io::Error::new(std::io::ErrorKind::InvalidData, error)))
        })?;
        let rendered = render_wiki_objects(&input, &plan.next_objects)
            .map_err(before_publication)?;
        bound.require_mapping_and_form().map_err(before_publication)?;
        let changed = publication::publish_wiki_affiliated(
            &bound.owner_root, bound.physical_root_identity().map_err(before_publication)?,
            &bound.canonical_member, &rendered, base_hash,
        ).map_err(|error| {
            let no_publication = error.details().get("changed").is_some_and(|value| value == "false")
                && error.details().get("published").is_some_and(|value| value == "false");
            error.with("source", self.semantic.canonical_wiki.to_string())
                .with("command_effect", if no_publication { "none" } else { "unknown" })
        })?;
        #[cfg(test)]
        tests::after_wiki_publication(path);
        if let Err(cause) = bound.require_mapping_and_form() {
            let original = serde_json::json!({
                "code": cause.code(), "message": cause.message(), "details": cause.details(),
            }).to_string();
            if changed {
                return Err(AikitError::new(
                    "knowledge.wiki_publication_uncertain",
                    "native Wiki publication completed, but its original binding route changed before acknowledgement",
                )
                .with_io_source_from(&cause)
                .with("source", self.semantic.canonical_wiki.to_string())
                .with("path", path.display().to_string())
                .with("owner_root", bound.owner_root.display().to_string())
                .with("canonical_member", bound.canonical_member.display().to_string())
                .with("admitted_physical_root", format!("{:?}", bound.root_affiliation))
                .with("published", "true")
                .with("published_hash", content_hash(rendered.as_bytes()))
                .with("base_hash", base_hash)
                .with("changed", "true")
                .with("command_effect", "unknown")
                .with("cause", original));
            }
            return Err(cause.with("source", self.semantic.canonical_wiki.to_string())
                .with("changed", "false")
                .with("published", "false")
                .with("observed_basis", content_hash(rendered.as_bytes()))
                .with("command_effect", "none"));
        }
        Ok(changed)
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }
}

#[derive(Debug, Clone)]
pub struct ProjectCentralFileProvider {
    provider: ProviderRef,
    paths: BTreeMap<ResourceRef, BoundSourcePath>,
    standing: BTreeMap<ResourceRef, ProjectCentralStanding>,
}

impl ContextSourceProvider for ProjectCentralFileProvider {
    fn provider(&self) -> &ProviderRef {
        &self.provider
    }

    fn status(&self) -> ContextSourceProviderStatus {
        ContextSourceProviderStatus::Available
    }

    fn capabilities(&self) -> ContextSourceProviderCapabilities {
        ContextSourceProviderCapabilities::with_operations([
            ContextSourceOperation::Discover,
            ContextSourceOperation::Read,
            ContextSourceOperation::Resolve,
            ContextSourceOperation::Explain,
        ])
    }

    fn read(&mut self, request: &ContextSourceReadRequest) -> ProviderReadResult {
        let Some(bound) = self.paths.get(&request.resource) else {
            return ProviderReadResult::Absent(StructuredAbsence::new(
                AbsenceKind::Unknown,
                "ProjectCentral source is not bound to a readable native filesystem object",
            ));
        };
        if let Some(absence) = source_read_admission(bound) {
            return ProviderReadResult::Absent(absence);
        }
        let path = &bound.path;
        if bound.is_directory {
            return ProviderReadResult::Absent(StructuredAbsence::new(
                AbsenceKind::Bound,
                "ProjectCentral directory exists but has no eager aggregate payload",
            ));
        }
        let bytes = match bound.material_bytes() {
            Ok(bytes) => bytes,
            Err(error) => {
                return ProviderReadResult::Absent(source_read_absence(path, &error));
            }
        };
        #[cfg(test)]
        tests::after_source_read(path);
        if let Some(absence) = source_read_admission(bound) {
            return ProviderReadResult::Absent(absence);
        }
        let payload = match String::from_utf8(bytes) {
            Ok(payload) => payload,
            Err(_) => {
                return ProviderReadResult::Absent(StructuredAbsence::new(
                    AbsenceKind::Bound,
                    "source exists and is readable, but this text provider cannot interpret its format",
                ));
            }
        };
        let standing = self
            .standing
            .get(&request.resource)
            .copied()
            .unwrap_or(ProjectCentralStanding::Unresolved);
        let source = SourceRef::parse(request.resource.as_str())
            .expect("ContextSource ResourceRef originated from a SourceRef");
        let revision = revision_for(path);
        ProviderReadResult::Retrieved {
            payload,
            revision: revision.clone(),
            provenance: vec![ResourceSource {
                source,
                authority: standing.source_authority(),
                revision,
                locator: Some(aikit_core::ResourceLocator::Path(path.clone())),
                state: SourceState::Available,
            }],
        }
    }
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.schema != CENTRAL_PROJECT_SCHEMA {
        return Err(AikitError::new(
            "projectcentral.unsupported_schema",
            format!(
                "expected {CENTRAL_PROJECT_SCHEMA}, found {}",
                manifest.schema
            ),
        ));
    }
    if manifest.human_source != PROJECTCENTRAL_HUMAN_ROOT {
        return Err(AikitError::new(
            "projectcentral.human_source_contract",
            "Central owns the canonical human source path ProjectCentral/user",
        ));
    }
    if manifest.wiki.profile != CENTRAL_WIKI_PROFILE
        || manifest.wiki.source != PROJECTCENTRAL_WIKI_SOURCE
    {
        return Err(AikitError::new(
            "projectcentral.wiki_contract",
            "Central owns the canonical ProjectCentral/agents/wiki/wiki.json okf-wiki/v1 binding",
        ));
    }
    for source in &manifest.wiki.adopted_sources {
        validate_relative_source(source)?;
    }
    Ok(())
}

fn read_ground_relations(
    project_root: &Path,
    project_id: &str,
) -> Result<Option<GroundRelationsFile>> {
    let path = project_root.join(PROJECTCENTRAL_GROUND_RELATIONS_SOURCE);
    if !path.is_file() {
        return Ok(None);
    }
    let input = fs::read_to_string(&path)
        .map_err(|error| io_error("projectcentral.ground_relations_read", &path, error))?;
    let relations: GroundRelationsFile = serde_json::from_str(&input).map_err(|error| {
        AikitError::new(
            "projectcentral.ground_relations_invalid",
            format!(
                "{} is not valid Central ground relations: {error}",
                path.display()
            ),
        )
    })?;
    if relations.schema != CENTRAL_GROUND_RELATIONS_SCHEMA {
        return Err(AikitError::new(
            "projectcentral.ground_relations_schema",
            format!(
                "expected {CENTRAL_GROUND_RELATIONS_SCHEMA}, found {}",
                relations.schema
            ),
        ));
    }
    if relations.project_id != project_id {
        return Err(AikitError::new(
            "projectcentral.ground_relations_project",
            "ground relation project_id does not match ProjectCentral/project.json",
        ));
    }
    Ok(Some(relations))
}

fn validate_relative_source(raw: &str) -> Result<()> {
    let path = Path::new(raw);
    if path.is_absolute()
        || raw.trim().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(AikitError::new(
            "projectcentral.source_escape",
            "ProjectCentral source must remain project-relative",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scan_human_tree(
    project_root: &Path,
    directory: &Path,
    project_id: &str,
    relations_by_path: &BTreeMap<PathBuf, GroundRelation>,
    consumed_relations: &mut BTreeSet<PathBuf>,
    sources: &mut Vec<ProjectCentralSourceDescriptor>,
    paths: &mut BTreeMap<ResourceRef, BoundSourcePath>,
    standings: &mut BTreeMap<ResourceRef, ProjectCentralStanding>,
) -> Result<()> {
    if directory.join(NO_AGENT_RETRIEVAL_MARKER).exists() {
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|error| io_error("projectcentral.directory_read", directory, error))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| io_error("projectcentral.directory_entry", directory, error))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("projectcentral.file_type", &entry.path(), error))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if entry.file_name() == NO_AGENT_RETRIEVAL_MARKER {
            continue;
        }
        if file_type.is_dir() {
            scan_human_tree(
                project_root,
                &path,
                project_id,
                relations_by_path,
                consumed_relations,
                sources,
                paths,
                standings,
            )?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let relative = relative_path(project_root, &path)?;
        if let Some(relation) = relations_by_path.get(&relative) {
            consumed_relations.insert(relative.clone());
            push_source(
                sources,
                paths,
                standings,
                project_root,
                source(&relation.source_ref)?,
                relative,
                ProjectCentralSourceKind::HumanMaterial,
                relation.provenance.operational_standing(),
                relation.provenance,
                relation.truth_standing,
                relation.roles.clone(),
                relation.treatment,
                Some(relation.recognition.clone()),
                true,
                false,
            )?;
        } else {
            let source_ref = source(&central_ground_source_ref(project_id, &relative))?;
            push_source(
                sources,
                paths,
                standings,
                project_root,
                source_ref,
                relative,
                ProjectCentralSourceKind::HumanMaterial,
                ProjectCentralStanding::Unresolved,
                ProjectCentralProvenance::Unresolved,
                ProjectCentralTruthStanding::Unspecified,
                Vec::new(),
                ProjectCentralTreatment::ProjectcentralUser,
                None,
                true,
                false,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scan_governance_tree(
    project_root: &Path,
    directory: &Path,
    project_id: &str,
    sources: &mut Vec<ProjectCentralSourceDescriptor>,
    paths: &mut BTreeMap<ResourceRef, BoundSourcePath>,
    standings: &mut BTreeMap<ResourceRef, ProjectCentralStanding>,
) -> Result<()> {
    if directory.join(NO_AGENT_RETRIEVAL_MARKER).exists() {
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|error| io_error("projectcentral.directory_read", directory, error))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| io_error("projectcentral.directory_entry", directory, error))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("projectcentral.file_type", &entry.path(), error))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if entry.file_name() == NO_AGENT_RETRIEVAL_MARKER {
            continue;
        }
        if file_type.is_dir() {
            scan_governance_tree(project_root, &path, project_id, sources, paths, standings)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let relative = relative_path(project_root, &path)?;
        let local = relative
            .strip_prefix(PROJECTCENTRAL_GOVERNANCE_ROOT)
            .unwrap_or(&relative);
        let source_ref = source(&format!(
            "source:central:{project_id}:governance:{}",
            local.to_string_lossy()
        ))?;
        push_source(
            sources,
            paths,
            standings,
            project_root,
            source_ref,
            relative,
            ProjectCentralSourceKind::GovernanceMaterial,
            ProjectCentralStanding::HumanGovernance,
            ProjectCentralProvenance::HumanAuthored,
            ProjectCentralTruthStanding::Unspecified,
            Vec::new(),
            ProjectCentralTreatment::Unresolved,
            None,
            true,
            false,
        )?;
    }
    Ok(())
}

/// Central's own root governance tree (`Control/agents/governance/**`) as
/// ContextSource ResourceRecords, named with the canonical
/// `central:source:control:root:<relative-path>` ref grammar — the same
/// grammar `now_field`, `central_entities` and `actor_composition` already
/// use for root Control material. This is the root-scope counterpart to
/// [`ProjectCentralBinding::context_sources`]'s `GovernanceMaterial` entries,
/// which only ever cover a Project's own `ProjectCentral/agents/governance`.
///
/// Bodies are never read here — only named, with a filesystem revision for
/// cache-busting, exactly like the Project scanner. `.no-agent-retrieval`
/// prunes a subtree before any descendant is disclosed, and a symlinked
/// governance file is skipped rather than followed, matching
/// `scan_governance_tree`'s own withholding rule. Absence of the tree (no
/// `Control/agents/governance` under this root) is a valid empty reading,
/// never an error.
pub fn root_governance_context_source_records(central_root: &Path) -> Result<Vec<ResourceRecord>> {
    let governance_path = central_root.join(CENTRAL_ROOT_GOVERNANCE_ROOT);
    let mut records = Vec::new();
    let readable = path_agent_readability(central_root, Path::new(CENTRAL_ROOT_GOVERNANCE_ROOT))
        .map_err(|error| source_read_error(
            "projectcentral.source_unavailable", &governance_path, error,
        ))?;
    if !readable || !governance_path.is_dir() {
        return Ok(records);
    }
    scan_root_governance_tree(central_root, &governance_path, &mut records)?;
    Ok(records)
}

fn scan_root_governance_tree(
    central_root: &Path,
    directory: &Path,
    records: &mut Vec<ResourceRecord>,
) -> Result<()> {
    let relative_directory = relative_path(central_root, directory)?;
    if !path_agent_readability(central_root, &relative_directory).map_err(|error| {
        source_read_error("projectcentral.source_unavailable", directory, error)
    })? {
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|error| source_read_error("projectcentral.directory_read", directory, error))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| source_read_error("projectcentral.directory_entry", directory, error))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| source_read_error("projectcentral.file_type", &entry.path(), error))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if entry.file_name() == NO_AGENT_RETRIEVAL_MARKER {
            continue;
        }
        if file_type.is_dir() {
            scan_root_governance_tree(central_root, &path, records)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let relative = relative_path(central_root, &path)?;
        if !path_agent_readability(central_root, &relative).map_err(|error| {
            source_read_error("projectcentral.source_unavailable", &path, error)
        })? {
            continue;
        }
        let source_ref = source(&format!(
            "{CENTRAL_ROOT_SOURCE_REF_PREFIX}{}",
            relative.display()
        ))?;
        let id = ResourceRef::parse(source_ref.as_str())?;
        let mut descriptor = ResourceDescriptor::new(
            id,
            ResourceKind::ContextSource,
            relative.display().to_string(),
            "Central root governance source known to exist; payload is retrieved only on explicit read",
        );
        descriptor
            .annotations
            .insert("central.standing".into(), "human-governance".into());
        descriptor
            .annotations
            .insert("central.provenance".into(), "human-authored".into());
        descriptor
            .annotations
            .insert("central.path".into(), relative.display().to_string());
        descriptor.sources.push(ResourceSource {
            source: source_ref,
            authority: Some(SourceAuthority::Authored),
            revision: revision_for(&path),
            locator: Some(ResourceLocator::Path(relative.clone())),
            state: SourceState::Available,
        });
        let mut record = ResourceRecord::new(descriptor);
        record.eligibility = Eligibility::Eligible;
        records.push(record);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_source(
    sources: &mut Vec<ProjectCentralSourceDescriptor>,
    paths: &mut BTreeMap<ResourceRef, BoundSourcePath>,
    standings: &mut BTreeMap<ResourceRef, ProjectCentralStanding>,
    project_root: &Path,
    source_ref: SourceRef,
    relative_path: PathBuf,
    kind: ProjectCentralSourceKind,
    standing: ProjectCentralStanding,
    provenance: ProjectCentralProvenance,
    truth_standing: ProjectCentralTruthStanding,
    roles: Vec<String>,
    treatment: ProjectCentralTreatment,
    recognition: Option<String>,
    agent_readable: bool,
    is_directory: bool,
) -> Result<()> {
    let absolute = project_root.join(&relative_path);
    let symlink = is_symlink(&absolute);
    let exists = !symlink
        && if is_directory {
            absolute.is_dir()
        } else {
            absolute.is_file()
        };
    // Retain the existing native path binding independently of this read's
    // current disclosure. In particular, this does not create a new publication
    // authority rule for the canonical Agent Wiki writer.
    let bound_path_exists = agent_readable && exists;
    let agent_readable =
        bound_path_exists && path_agent_readable(project_root, &relative_path);
    let descriptor = ProjectCentralSourceDescriptor {
        source: source_ref.clone(),
        relative_path,
        kind,
        standing,
        provenance,
        truth_standing,
        roles,
        treatment,
        recognition,
        exists,
        agent_readable: agent_readable && exists,
        is_directory,
        revision: revision_for(&absolute),
    };
    let key = ResourceRef::parse(source_ref.as_str())?;
    if bound_path_exists {
        paths.insert(
            key.clone(),
            BoundSourcePath::new(absolute, project_root.to_path_buf(), is_directory)?,
        );
    }
    standings.insert(key, standing);
    sources.push(descriptor);
    Ok(())
}

fn relative_path(project_root: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(project_root)
        .map(Path::to_path_buf)
        .map_err(|_| {
            AikitError::new(
                "projectcentral.source_escape",
                "ProjectCentral source escaped the Project root",
            )
        })
}

/// Whether a nonescaping project-relative member stays agent-readable under
/// the supplied native owner root. This is the same predicate used by native
/// ProjectCentral reads and the live Work-repos pool. A selected descendant
/// directory is not a replacement for its known native owner boundary.
///
/// The member's final symlink and non-regular/non-directory objects are refused;
/// directory markers include the directory itself. Admission IO errors return
/// `false` here, while native reads retain them through the fallible native
/// form. This check grants no provider egress and is not atomic against arbitrary
/// filesystem writers. The caller supplies the actual root, including accepted
/// aliases; this function does not infer or canonicalise a semantic owner.
pub fn path_agent_readable(project_root: &Path, relative: &Path) -> bool {
    path_agent_readability(project_root, relative).unwrap_or(false)
}

/// The fallible form of [`path_agent_readable`]. `false` means current
/// admission was refused (including markers, final aliases or escaped members),
/// not proof of a particular marker. IO failures retain their actual cause.
/// Neither `true` nor a missing final path establishes material existence or
/// grants egress; the source reader must perform its own bounded physical read.
pub fn path_agent_readability(project_root: &Path, relative: &Path) -> std::io::Result<bool> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Ok(false);
    }
    let member: PathBuf = relative
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect();
    if member.as_os_str().is_empty() {
        return Ok(false);
    }
    let absolute = project_root.join(member);
    let metadata = match fs::symlink_metadata(&absolute) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if metadata.as_ref().is_some_and(|metadata| {
        metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir())
    }) {
        return Ok(false);
    }
    let is_directory = metadata.as_ref().is_some_and(|metadata| metadata.is_dir());
    if !agent_readable_ancestors(project_root, &absolute, is_directory)? {
        return Ok(false);
    }
    if metadata.is_some() {
        // Resolve physical membership against the supplied native root, while
        // preserving accepted aliases of that root. This does not infer an
        // owner from a directory name or follow a final source symlink.
        let root = fs::canonicalize(project_root)?;
        let member = fs::canonicalize(&absolute)?;
        if !member.starts_with(&root) {
            return Ok(false);
        }
        return agent_readable_ancestors(&root, &member, is_directory);
    }
    Ok(true)
}

fn agent_readable_ancestors(
    root: &Path,
    member: &Path,
    is_directory: bool,
) -> std::io::Result<bool> {
    let mut cursor = if is_directory {
        Some(member)
    } else {
        member.parent()
    };
    while let Some(directory) = cursor {
        if !directory.starts_with(root) {
            return Ok(false);
        }
        match fs::metadata(directory.join(NO_AGENT_RETRIEVAL_MARKER)) {
            Ok(marker) if marker.is_file() => return Ok(false),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if directory == root {
            break;
        }
        cursor = directory.parent();
    }
    Ok(true)
}

fn source_read_admission(bound: &BoundSourcePath) -> Option<StructuredAbsence> {
    match bound.readable() {
        Ok(true) => None,
        Ok(false) => Some(StructuredAbsence::new(
            AbsenceKind::Bound,
            "ProjectCentral source is withheld from the current read",
        )),
        Err(error) => Some(source_read_absence(&bound.path, &error)),
    }
}

fn source_read_absence(path: &Path, error: &AikitError) -> StructuredAbsence {
    let cause = std::error::Error::source(error)
        .and_then(|cause| cause.downcast_ref::<std::io::Error>());
    let owner_route_unavailable = error.details().get("observation_stage").is_some_and(|stage| {
        matches!(stage.as_str(), "owner_root" | "owner_parent" | "source_mapping" | "read_admission")
    });
    let kind = if !owner_route_unavailable
        && cause.is_some_and(|cause| cause.kind() == std::io::ErrorKind::NotFound)
    {
        AbsenceKind::Missing
    } else {
        AbsenceKind::Unknown
    };
    let mut reason = format!(
        "ProjectCentral source {} is unavailable (native_code={}): {error}",
        path.display(), error.code(),
    );
    if let Some(cause) = cause {
        reason.push_str(&format!(
            " (cause_kind={:?}, cause_raw_os_error={:?})",
            cause.kind(), cause.raw_os_error(),
        ));
    }
    StructuredAbsence::new(kind, reason)
}

fn source_read_error(code: &'static str, path: &Path, error: std::io::Error) -> AikitError {
    let kind = format!("{:?}", error.kind());
    let raw_os_error = serde_json::json!(error.raw_os_error()).to_string();
    AikitError::new(code, format!("{}: {error}", path.display()))
        .with("cause_kind", kind)
        .with("cause_raw_os_error", raw_os_error)
        .with_io_source(error)
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn central_ground_source_ref(project_id: &str, relative_path: &Path) -> String {
    let path = relative_path.to_string_lossy();
    let mut hash = 0xcbf29ce484222325u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("central:project-source:{project_id}:{hash:016x}")
}

fn revision_for(path: &Path) -> Option<SourceRevision> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    SourceRevision::parse(format!("fs:{modified}:{}", metadata.len())).ok()
}

fn source(raw: &str) -> Result<SourceRef> {
    SourceRef::parse(raw)
}

fn io_error(code: &'static str, path: &Path, error: std::io::Error) -> AikitError {
    AikitError::new(code, format!("{}: {error}", path.display()))
}

/// SHA-256 of exactly these bytes, hex-encoded — the same compare-and-swap
/// primitive `crates/aikit-cli/src/wiki.rs` uses for its own write gate. A
/// rewrite that happens to land byte-identical content is never a conflict,
/// only a peer write that actually changed the file is.
fn content_hash(bytes: &[u8]) -> String {
    let mut digest = sha2::Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

/// Preserve document headers and extension-bearing objects. An unchanged
/// maintenance plan keeps its exact source bytes.
fn render_wiki_objects(input: &str, objects: &[aikit_core::WikiObject]) -> Result<String> {
    let current_document = aikit_core::WikiDocument::parse(input)?;
    current_document.validate()?;
    let mut document = serde_json::from_str::<Value>(input).map_err(|error| {
        AikitError::new("projectcentral.wiki_serialize", error.to_string())
    })?;
    let objects = objects
        .iter()
        .map(wiki_object_value)
        .collect::<Result<Vec<_>>>()?;
    document.as_object_mut().ok_or_else(|| AikitError::new(
        "projectcentral.wiki_serialize", "Agent Wiki document is not an object"))?
        .insert("objects".into(), Value::Array(objects));
    let rendered = serde_json::to_string_pretty(&document)
    .map_err(|error| {
        AikitError::new(
            "projectcentral.wiki_serialize",
            format!("could not serialize Agent Wiki: {error}"),
        )
    })?;
    let next_document = aikit_core::WikiDocument::parse(&rendered)?;
    next_document.validate()?;
    // Native validation establishes unique identities before either map can
    // collapse them. Object order alone does not require a new publication.
    let current = current_document.objects().iter()
        .map(|object| (object.ref_id(), object)).collect::<BTreeMap<_, _>>();
    let next = next_document.objects().iter()
        .map(|object| (object.ref_id(), object)).collect::<BTreeMap<_, _>>();
    if current == next {
        return Ok(input.to_string());
    }
    Ok(format!("{rendered}\n"))
}

fn wiki_object_value(object: &aikit_core::WikiObject) -> Result<Value> {
    let (kind, value) = match object {
        aikit_core::WikiObject::Space(value) => ("space", serde_json::to_value(value)),
        aikit_core::WikiObject::Node(value) => ("node", serde_json::to_value(value)),
        aikit_core::WikiObject::Edge(value) => ("edge", serde_json::to_value(value)),
        aikit_core::WikiObject::Frame(value) => ("frame", serde_json::to_value(value)),
        aikit_core::WikiObject::Reading(value) => ("reading", serde_json::to_value(value)),
    };
    let mut value = value.map_err(|error| {
        AikitError::new(
            "projectcentral.wiki_serialize",
            format!("could not serialize Agent Wiki object: {error}"),
        )
    })?;
    let map = value.as_object_mut().ok_or_else(|| {
        AikitError::new(
            "projectcentral.wiki_serialize",
            "Wiki object did not serialize as an object",
        )
    })?;
    map.insert("object".into(), Value::String(kind.into()));
    Ok(Value::Object(std::mem::take(map)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aikit_core::{
        plan_agent_wiki_maintenance, AgentWikiMaintenanceRequest, ContextSourceIndex,
        ContextSourceReadOutcome, HorizonRequest, HumanSourceRevisionProposal, KnowledgeAddress,
        KnowledgeApplication, ProjectCentralGroundStatus, ProjectCentralProvenance,
        ProjectCentralSourceKind, ProjectCentralStanding, ProjectCentralTreatment,
        ProjectCentralTruthStanding, RetrievalTarget, SemanticWikiIndex, SemanticWikiProvider,
        WikiNode, WikiObject, WikiProvenanceRef,
    };
    use tempfile::TempDir;

    use super::*;

    const PURPOSE_REF: &str = "central:project-source:epilogos/demo:0000000000000001";
    const VISION_REF: &str = "central:project-source:epilogos/demo:0000000000000002";

    type ReadObserver = Box<dyn FnOnce(&Path)>;
    std::thread_local! {
        static AFTER_SOURCE_READ: std::cell::RefCell<Option<ReadObserver>> =
            const { std::cell::RefCell::new(None) };
        static AFTER_MANIFEST_READ: std::cell::RefCell<Option<ReadObserver>> =
            const { std::cell::RefCell::new(None) };
        static AFTER_WIKI_PUBLICATION: std::cell::RefCell<Option<ReadObserver>> =
            const { std::cell::RefCell::new(None) };
    }

    struct ReadObservation;

    impl Drop for ReadObservation {
        fn drop(&mut self) {
            AFTER_SOURCE_READ.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }

    fn observe_source_read(observer: ReadObserver) -> ReadObservation {
        AFTER_SOURCE_READ.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(observer);
        });
        ReadObservation
    }

    pub(super) fn after_source_read(path: &Path) {
        let observer = AFTER_SOURCE_READ.with(|slot| slot.borrow_mut().take());
        if let Some(observer) = observer {
            observer(path);
        }
    }

    struct ManifestObservation;

    impl Drop for ManifestObservation {
        fn drop(&mut self) {
            AFTER_MANIFEST_READ.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }

    fn observe_manifest_read(observer: ReadObserver) -> ManifestObservation {
        AFTER_MANIFEST_READ.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(observer);
        });
        ManifestObservation
    }

    pub(super) fn after_manifest_read(path: &Path) {
        let observer = AFTER_MANIFEST_READ.with(|slot| slot.borrow_mut().take());
        if let Some(observer) = observer {
            observer(path);
        }
    }

    struct PublicationObservation;

    impl Drop for PublicationObservation {
        fn drop(&mut self) {
            AFTER_WIKI_PUBLICATION.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }

    fn observe_wiki_publication(observer: ReadObserver) -> PublicationObservation {
        AFTER_WIKI_PUBLICATION.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(observer);
        });
        PublicationObservation
    }

    pub(super) fn after_wiki_publication(path: &Path) {
        let observer = AFTER_WIKI_PUBLICATION.with(|slot| slot.borrow_mut().take());
        if let Some(observer) = observer {
            observer(path);
        }
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn wiki_json(title: &str, source_ref: Option<&str>) -> String {
        let provenance = source_ref
            .map(|source| format!(r#"[{{"source_ref":"{source}","source_revision":"r1"}}]"#))
            .unwrap_or_else(|| "[]".into());
        format!(
            r#"{{"profile":"okf-wiki/v1","objects":[
              {{"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:project","revision":1,"provenance":[],"title":"Project","parent_space_refs":[],"child_space_refs":[],"node_refs":["wiki:node:purpose"]}},
              {{"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:purpose","revision":1,"provenance":{provenance},"type":"ProjectKnowledge","title":"{title}","space_refs":["wiki:space:project"],"source_refs":{sources}}}
            ]}}"#,
            sources = source_ref
                .map(|source| format!(r#"["{source}"]"#))
                .unwrap_or_else(|| "[]".into())
        )
    }

    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        fixture_in(TempDir::new().unwrap())
    }

    fn native_read_fixture() -> (TempDir, PathBuf, PathBuf) {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../ProjectCentral/now/tmp");
        fs::create_dir_all(&scratch).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("projectcentral-current-read-")
            .tempdir_in(&scratch)
            .unwrap();
        fixture_in(temp)
    }

    fn fixture_in(temp: TempDir) -> (TempDir, PathBuf, PathBuf) {
        let central = temp.path().join("Central");
        let project = central.join("Work/demo");
        write(
            &project.join("ProjectCentral/project.json"),
            r#"{
              "schema":"central.project/v1",
              "project_id":"epilogos/demo",
              "human_source":"ProjectCentral/user",
              "wiki":{
                "profile":"okf-wiki/v1",
                "source":"ProjectCentral/agents/wiki/wiki.json",
                "adopted_sources":["legacy/wiki.json"]
              }
            }"#,
        );
        write(
            &project.join("ProjectCentral/user/research/deep/purpose.md"),
            "Human purpose",
        );
        write(&project.join("VISION.md"), "Retained native human vision");
        write(
            &project.join(PROJECTCENTRAL_GROUND_RELATIONS_SOURCE),
            &format!(
                r#"{{
                  "schema":"central.project.ground-relations/v1",
                  "project_id":"epilogos/demo",
                  "relations":[
                    {{"ref":"{PURPOSE_REF}","path":"ProjectCentral/user/research/deep/purpose.md","provenance":"human-authored","standing":"authored-human-position","roles":["purpose"],"treatment":"projectcentral-user","recognition":"human-accepted source relation","recorded_at_unix_seconds":1}},
                    {{"ref":"{VISION_REF}","path":"VISION.md","provenance":"human-adopted","standing":"design-commitment","roles":["vision"],"treatment":"retain-native-in-place","recognition":"human-accepted source relation","recorded_at_unix_seconds":2}}
                  ]
                }}"#
            ),
        );
        write(
            &project.join("ProjectCentral/agents/governance/STYLE.md"),
            "Human governance",
        );
        write(
            &project.join(PROJECTCENTRAL_WIKI_SOURCE),
            &wiki_json("Purpose", Some(PURPOSE_REF)),
        );
        write(
            &project.join("legacy/wiki.json"),
            &wiki_json("Adopted", None),
        );
        write(
            &central.join(CENTRAL_ROOT_WIKI_SOURCE),
            &wiki_json("Root", None),
        );
        (temp, central, project)
    }

    #[test]
    fn projectcentral_works_without_readme_and_recognises_arbitrarily_nested_human_source() {
        let (_temp, central, project) = fixture();
        assert!(!project.join("ProjectCentral/README.md").exists());
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let orientation = binding.semantic.orientation().unwrap();
        assert_eq!(orientation.human_material_count, 1);
        assert_eq!(orientation.recognised_human_source_count, 2);
        assert_eq!(
            orientation.ground_status,
            ProjectCentralGroundStatus::Established
        );
        assert!(binding.semantic.sources.iter().any(|source| {
            source.kind == ProjectCentralSourceKind::HumanMaterial
                && source.relative_path.ends_with("research/deep/purpose.md")
                && source.source.as_str() == PURPOSE_REF
        }));
    }

    #[test]
    fn unclassified_human_aperture_file_remains_unresolved_until_recognised() {
        let (_temp, central, project) = fixture();
        write(
            &project.join("ProjectCentral/user/generated-suggestion.md"),
            "not human-authored merely because it is here",
        );
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let unresolved = binding
            .semantic
            .sources
            .iter()
            .find(|source| source.relative_path.ends_with("generated-suggestion.md"))
            .unwrap();
        assert_eq!(unresolved.standing, ProjectCentralStanding::Unresolved);
        assert_eq!(unresolved.provenance, ProjectCentralProvenance::Unresolved);
        assert_eq!(
            unresolved.truth_standing,
            ProjectCentralTruthStanding::Unspecified
        );
        assert!(unresolved.recognition.is_none());
        let context = binding.semantic.account_context().unwrap();
        assert_eq!(context.preferred_human_sources.len(), 2);
        assert!(context
            .other_source_relations
            .iter()
            .any(|source| source.relative_path.ends_with("generated-suggestion.md")));
        let entry = binding
            .semantic
            .context_sources()
            .unwrap()
            .into_iter()
            .find(|entry| entry.resource.descriptor.id.as_str() == unresolved.source.as_str())
            .unwrap();
        assert!(entry.resource.descriptor.sources[0].authority.is_none());
    }

    #[test]
    fn recognised_native_human_source_stays_in_place_with_exact_central_standing() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let context = binding.semantic.account_context().unwrap();
        let vision = context
            .preferred_human_sources
            .iter()
            .find(|source| source.source.as_str() == VISION_REF)
            .unwrap();
        assert_eq!(vision.relative_path, PathBuf::from("VISION.md"));
        assert_eq!(vision.provenance, ProjectCentralProvenance::HumanAdopted);
        assert_eq!(
            vision.truth_standing,
            ProjectCentralTruthStanding::DesignCommitment
        );
        assert_eq!(
            vision.treatment,
            ProjectCentralTreatment::RetainNativeInPlace
        );
        assert_eq!(
            fs::read_to_string(project.join("VISION.md")).unwrap(),
            "Retained native human vision"
        );
    }

    fn read_request(source: &str) -> ContextSourceReadRequest {
        ContextSourceReadRequest {
            resource: ResourceRef::parse(source).unwrap(),
            provider: ProviderRef::parse(PROJECTCENTRAL_FILESYSTEM_PROVIDER).unwrap(),
            target: RetrievalTarget::LocalAgent,
        }
    }

    fn exact_provider_payload(
        provider: &mut ProjectCentralFileProvider,
        source: &str,
        expected: &str,
    ) {
        match provider.read(&read_request(source)) {
            ProviderReadResult::Retrieved {
                payload,
                revision,
                provenance,
            } => {
                assert_eq!(payload, expected);
                assert!(revision.is_some());
                assert_eq!(provenance.len(), 1);
                assert_eq!(provenance[0].source.as_str(), source);
            }
            other => panic!("expected actual native source {source}, received {other:?}"),
        }
    }

    fn provider_withheld(provider: &mut ProjectCentralFileProvider, source: &str) {
        match provider.read(&read_request(source)) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Bound);
                assert!(absence.reason.contains("withheld"));
            }
            other => panic!("withheld native source {source} was disclosed: {other:?}"),
        }
    }

    #[test]
    fn same_provider_rechecks_native_marker_and_restores_exact_source() {
        let (_temp, central, project) = native_read_fixture();
        let path = project.join("ProjectCentral/user/research/deep/purpose.md");
        let original = fs::read(&path).unwrap();
        let original_metadata = fs::metadata(&path).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");

        let marker = project.join("ProjectCentral/user/research/.no-agent-retrieval");
        fs::write(&marker, b"withheld by the fixture's source owner").unwrap();
        provider_withheld(&mut provider, PURPOSE_REF);
        exact_provider_payload(&mut provider, VISION_REF, "Retained native human vision");
        exact_provider_payload(
            &mut provider,
            binding.semantic.root_wiki.as_ref().unwrap().as_str(),
            &wiki_json("Root", None),
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            original_metadata.modified().unwrap(),
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = fs::metadata(&path).unwrap();
            assert_eq!(current.dev(), original_metadata.dev());
            assert_eq!(current.ino(), original_metadata.ino());
        }

        fs::remove_file(&marker).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn known_enclosing_world_withdrawal_preserves_external_and_inherited_boundaries() {
        let (_temp, central, project) = native_read_fixture();
        let (_external_temp, _external_central, external_project) = native_read_fixture();
        let path = project.join("ProjectCentral/user/research/deep/purpose.md");
        let original = fs::read(&path).unwrap();
        let original_metadata = fs::metadata(&path).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let external = ProjectCentralFilesystemBinding::inspect(&external_project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        let mut external_provider = external.file_provider().unwrap();
        let marker = central.join("Work/.no-agent-retrieval");
        fs::write(&marker, b"known enclosing native World withdraws its Work subtree").unwrap();
        provider_withheld(&mut provider, PURPOSE_REF);
        provider_withheld(&mut provider, binding.semantic.canonical_wiki.as_str());
        assert_eq!(binding.load_project_wiki().unwrap_err().code(), "projectcentral.source_withheld");
        exact_provider_payload(&mut external_provider, PURPOSE_REF, "Human purpose");
        exact_provider_payload(&mut provider,
            binding.semantic.root_wiki.as_ref().unwrap().as_str(), &wiki_json("Root", None));
        assert_eq!(fs::read(&path).unwrap(), original);
        let marked = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let descriptor = marked.semantic.sources.iter()
            .find(|source| source.source == binding.semantic.canonical_wiki).unwrap();
        assert!(descriptor.exists);
        assert!(!descriptor.agent_readable);
        assert_eq!(descriptor.standing, ProjectCentralStanding::AgentMaintained);
        fs::remove_file(&marker).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");

        let observed_marker = marker.clone();
        let _observation = observe_source_read(Box::new(move |_| {
            fs::write(&observed_marker, b"late actual known native withdrawal").unwrap();
        }));
        provider_withheld(&mut provider, PURPOSE_REF);
        AFTER_SOURCE_READ.with(|slot| assert!(slot.borrow().is_none()));
        fs::remove_file(marker).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(),
            original_metadata.modified().unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = fs::metadata(&path).unwrap();
            assert_eq!(current.dev(), original_metadata.dev());
            assert_eq!(current.ino(), original_metadata.ino());
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn enclosing_world_checks_original_alias_route_and_its_native_destination() {
        use std::os::unix::fs::{symlink, MetadataExt};
        let (_temp, central, project) = native_read_fixture();
        let physical_project = central.join("retained-project-room/demo");
        fs::create_dir_all(physical_project.parent().unwrap()).unwrap();
        fs::rename(&project, &physical_project).unwrap();
        symlink(&physical_project, &project).unwrap();
        let path = physical_project.join("ProjectCentral/user/research/deep/purpose.md");
        let original = fs::read(&path).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");
        let lexical_marker = central.join("Work/.no-agent-retrieval");
        fs::write(&lexical_marker, b"withdraw original native World route").unwrap();
        assert!(path_agent_readability(&project,
            Path::new("ProjectCentral/user/research/deep/purpose.md")).unwrap());
        assert!(path_agent_readability(&central,
            Path::new("retained-project-room/demo/ProjectCentral/user/research/deep/purpose.md")).unwrap());
        provider_withheld(&mut provider, PURPOSE_REF);
        assert_eq!(binding.load_project_wiki().unwrap_err().code(), "projectcentral.source_withheld");
        fs::remove_file(&lexical_marker).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");
        let physical_marker = physical_project.parent().unwrap().join(NO_AGENT_RETRIEVAL_MARKER);
        fs::write(&physical_marker, b"withdraw actual native World destination").unwrap();
        provider_withheld(&mut provider, PURPOSE_REF);
        fs::remove_file(physical_marker).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");
        let current = fs::metadata(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(current.dev(), metadata.dev());
        assert_eq!(current.ino(), metadata.ino());
        assert_eq!(current.modified().unwrap(), metadata.modified().unwrap());
        assert_eq!(ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap()
            .semantic.canonical_wiki, binding.semantic.canonical_wiki);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn removed_owner_alias_is_unknown_while_the_original_source_is_retained() {
        use std::os::unix::fs::{symlink, MetadataExt};
        let (temp, central, project) = native_read_fixture();
        let alias = temp.path().join("retained-native-project-alias");
        symlink(&project, &alias).unwrap();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let original = fs::read(&path).unwrap();
        let original_metadata = fs::metadata(&path).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&alias, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        fs::remove_file(&alias).unwrap();
        let actual = fs::metadata(&alias).unwrap_err();
        assert_eq!(actual.kind(), std::io::ErrorKind::NotFound);
        match provider.read(&read_request(binding.semantic.canonical_wiki.as_str())) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Unknown);
                assert!(absence.reason.contains("observation_stage=owner_root"));
                assert!(absence.reason.contains(&actual.to_string()));
                assert!(absence.reason.contains(&format!("cause_raw_os_error={}",
                    serde_json::json!(actual.raw_os_error()))));
            }
            other => panic!("missing owner alias was treated as a current source: {other:?}"),
        }
        let failure = binding.load_project_wiki().unwrap_err();
        assert_eq!(failure.details()["observation_stage"], "owner_root");
        let cause = std::error::Error::source(&failure).unwrap()
            .downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), actual.kind());
        assert_eq!(cause.raw_os_error(), actual.raw_os_error());
        assert_eq!(fs::read(&path).unwrap(), original);
        let current = fs::metadata(&path).unwrap();
        assert_eq!(current.dev(), original_metadata.dev());
        assert_eq!(current.ino(), original_metadata.ino());
        symlink(&project, &alias).unwrap();
        exact_provider_payload(&mut provider, binding.semantic.canonical_wiki.as_str(),
            &wiki_json("Purpose", Some(PURPOSE_REF)));
        let fresh = ProjectCentralFilesystemBinding::inspect(&alias, Some(&central)).unwrap();
        assert_eq!(fresh.semantic.canonical_wiki, binding.semantic.canonical_wiki);
    }

    #[test]
    fn retained_directory_source_cannot_deliver_replacement_file_contents() {
        let (_temp, central, project) = native_read_fixture();
        let human = project.join(PROJECTCENTRAL_HUMAN_ROOT);
        let retained = project.join("ProjectCentral/retained-human-root");
        let purpose = human.join("research/deep/purpose.md");
        let original = fs::read(&purpose).unwrap();
        let original_metadata = fs::metadata(&purpose).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let source = binding.semantic.human_root.clone();
        let descriptor = binding.semantic.sources.iter()
            .find(|descriptor| descriptor.source == source).unwrap().clone();
        assert!(descriptor.is_directory);
        let mut provider = binding.file_provider().unwrap();
        match provider.read(&read_request(source.as_str())) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Bound);
                assert!(absence.reason.contains("no eager aggregate payload"));
            }
            other => panic!("native directory unexpectedly returned a body: {other:?}"),
        }

        fs::rename(&human, &retained).unwrap();
        fs::write(&human, b"replacement ordinary file must not become directory source content").unwrap();
        match provider.read(&read_request(source.as_str())) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Unknown);
                assert!(absence.reason.contains("projectcentral.source_binding_changed"));
                assert!(absence.reason.contains("expected_is_directory=true"));
                assert!(absence.reason.contains("observed_is_directory=false"));
            }
            other => panic!("captured directory disclosed replacement-file contents: {other:?}"),
        }
        assert_eq!(
            ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap_err().code(),
            "projectcentral.directory_read",
        );
        exact_provider_payload(&mut provider, VISION_REF, "Retained native human vision");
        assert_eq!(fs::read(retained.join("research/deep/purpose.md")).unwrap(), original);
        assert_eq!(binding.semantic.human_root, source);
        assert_eq!(
            binding.semantic.sources.iter().find(|item| item.source == source).unwrap().standing,
            descriptor.standing,
        );

        fs::remove_file(&human).unwrap();
        fs::rename(&retained, &human).unwrap();
        let reopened = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        assert_eq!(reopened.semantic.human_root, source);
        let reopened_descriptor = reopened.semantic.sources.iter()
            .find(|item| item.source == source).unwrap();
        assert!(reopened_descriptor.exists && reopened_descriptor.is_directory);
        assert_eq!(reopened_descriptor.standing, descriptor.standing);
        exact_provider_payload(&mut reopened.file_provider().unwrap(), PURPOSE_REF, "Human purpose");
        assert_eq!(fs::read(&purpose).unwrap(), original);
        assert_eq!(fs::metadata(&purpose).unwrap().modified().unwrap(),
            original_metadata.modified().unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = fs::metadata(&purpose).unwrap();
            assert_eq!(current.dev(), original_metadata.dev());
            assert_eq!(current.ino(), original_metadata.ino());
        }
    }

    #[test]
    fn inherited_root_and_adopted_sources_keep_their_actual_read_boundaries() {
        let (_temp, central, project) = native_read_fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let root = binding.semantic.root_wiki.as_ref().unwrap().as_str();
        let adopted = binding.semantic.adopted_wikis[0].as_str();
        let mut provider = binding.file_provider().unwrap();
        exact_provider_payload(&mut provider, root, &wiki_json("Root", None));
        exact_provider_payload(&mut provider, adopted, &wiki_json("Adopted", None));

        let project_marker = project.join(NO_AGENT_RETRIEVAL_MARKER);
        fs::write(&project_marker, b"project excluded").unwrap();
        provider_withheld(&mut provider, adopted);
        assert_eq!(
            binding.load_adopted_wikis().unwrap_err().code(),
            "projectcentral.source_withheld",
        );
        exact_provider_payload(&mut provider, root, &wiki_json("Root", None));
        assert_eq!(binding.load_root_wiki().unwrap().unwrap().len(), 2);
        fs::remove_file(project_marker).unwrap();
        exact_provider_payload(&mut provider, adopted, &wiki_json("Adopted", None));
        assert_eq!(binding.load_adopted_wikis().unwrap().len(), 1);

        let root_marker = central.join("Control/agents/.no-agent-retrieval");
        fs::write(&root_marker, b"root source excluded").unwrap();
        provider_withheld(&mut provider, root);
        assert_eq!(binding.load_root_wiki().unwrap_err().code(), "projectcentral.source_withheld");
        exact_provider_payload(
            &mut provider,
            binding.semantic.canonical_wiki.as_str(),
            &wiki_json("Purpose", Some(PURPOSE_REF)),
        );
        assert_eq!(binding.load_project_wiki().unwrap().len(), 2);
        fs::remove_file(root_marker).unwrap();
        exact_provider_payload(&mut provider, root, &wiki_json("Root", None));
        assert_eq!(binding.load_root_wiki().unwrap().unwrap().len(), 2);
    }

    #[test]
    fn native_provider_retains_actual_unavailable_io_and_observed_missing_causes() {
        let (_temp, central, project) = native_read_fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        let path = project.join("ProjectCentral/user/research/deep/purpose.md");
        let room = path.parent().unwrap();
        let retained_room = room.with_file_name("retained-deep");
        fs::rename(room, &retained_room).unwrap();
        fs::write(
            room,
            b"this existing path is now an ordinary file, not a source directory",
        ).unwrap();
        let actual = fs::read(&path).unwrap_err();
        assert_ne!(actual.kind(), std::io::ErrorKind::NotFound);
        assert!(!path_agent_readable(
            &project,
            Path::new("ProjectCentral/user/research/deep/purpose.md"),
        ));
        match provider.read(&read_request(PURPOSE_REF)) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Unknown);
                assert!(absence.reason.contains(&actual.to_string()));
                assert!(absence.reason.contains(&format!("cause_kind={:?}", actual.kind())));
                assert!(absence.reason.contains(&format!(
                    "cause_raw_os_error={:?}", actual.raw_os_error(),
                )));
            }
            other => panic!("actual unavailable source was reported as a payload: {other:?}"),
        }
        fs::remove_file(room).unwrap();
        fs::rename(retained_room, room).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");

        let wiki = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let retained_wiki = wiki.with_file_name("retained-wiki.json");
        fs::rename(&wiki, &retained_wiki).unwrap();
        let actual_missing = fs::read(&wiki).unwrap_err();
        assert_eq!(actual_missing.kind(), std::io::ErrorKind::NotFound);
        match provider.read(&read_request(binding.semantic.canonical_wiki.as_str())) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Missing);
                assert!(absence.reason.contains(&actual_missing.to_string()));
                assert!(absence.reason.contains(&format!(
                    "cause_raw_os_error={:?}", actual_missing.raw_os_error(),
                )));
            }
            other => panic!("missing native Wiki was reported as a payload: {other:?}"),
        }
        let error = binding.load_project_wiki().unwrap_err();
        assert_eq!(error.code(), "projectcentral.source_unavailable");
        assert_eq!(
            error.details().get("cause_kind"),
            Some(&format!("{:?}", actual_missing.kind())),
        );
        assert_eq!(
            error.details().get("cause_raw_os_error"),
            Some(&serde_json::json!(actual_missing.raw_os_error()).to_string()),
        );
        let cause = std::error::Error::source(&error).unwrap()
            .downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), actual_missing.kind());
        assert_eq!(cause.raw_os_error(), actual_missing.raw_os_error());
        fs::rename(retained_wiki, wiki).unwrap();
        assert_eq!(binding.load_project_wiki().unwrap().len(), 2);
    }

    #[test]
    fn native_noop_wiki_read_and_publication_preserve_actual_source_identity() {
        let (_temp, central, project) = native_read_fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let original = fs::read(&path).unwrap();
        let before = fs::metadata(&path).unwrap();
        let (current_objects, basis) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects,
            upserts: vec![],
            observed_source_revisions: binding.observed_source_revisions(),
            human_source_proposals: vec![],
        }).unwrap();
        assert!(!binding.persist_agent_wiki(&plan, &basis).unwrap());
        let after = fs::metadata(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
        assert_eq!(after.permissions(), before.permissions());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(after.dev(), before.dev());
            assert_eq!(after.ino(), before.ino());
            assert_eq!(after.uid(), before.uid());
            assert_eq!(after.gid(), before.gid());
            assert_eq!(after.nlink(), before.nlink());
        }
    }

    #[test]
    fn native_eager_reads_refuse_actual_oversize_without_truncation_or_marker_claim() {
        let (_temp, central, project) = native_read_fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        let path = project.join("ProjectCentral/user/research/deep/purpose.md");
        let original = fs::read(&path).unwrap();
        fs::OpenOptions::new().write(true).open(&path).unwrap()
            .set_len(EAGER_SOURCE_BUDGET + 1).unwrap();
        match provider.read(&read_request(PURPOSE_REF)) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Unknown);
                assert!(absence.reason.contains("knowledge.wiki_publication_budget"));
                assert!(!absence.reason.contains("withheld"));
            }
            other => panic!("oversized native material returned as a payload: {other:?}"),
        }
        assert_eq!(fs::metadata(&path).unwrap().len(), EAGER_SOURCE_BUDGET + 1);
        fs::write(&path, original).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");

        let wiki = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let original_wiki = fs::read(&wiki).unwrap();
        let (current, base_hash) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = maintenance_plan(&binding, current, "wiki:node:oversize-refused");
        fs::OpenOptions::new().write(true).open(&wiki).unwrap()
            .set_len(EAGER_SOURCE_BUDGET + 1).unwrap();
        let error = binding.load_project_wiki().unwrap_err();
        assert_eq!(error.code(), "knowledge.wiki_publication_budget");
        let write_error = binding.persist_agent_wiki(&plan, &base_hash).unwrap_err();
        assert_eq!(write_error.code(), "knowledge.wiki_publication_budget");
        assert_eq!(write_error.details().get("command_effect"), Some(&"none".to_string()));
        assert_eq!(fs::metadata(&wiki).unwrap().len(), EAGER_SOURCE_BUDGET + 1);
        fs::write(wiki, original_wiki).unwrap();
        assert_eq!(binding.load_project_wiki().unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn captured_wiki_writer_refuses_real_final_symlink_and_fifo_before_effect() {
        use crate::runner::{CommandRunner, SystemRunner};
        use std::os::unix::fs::{symlink, FileTypeExt};
        use std::time::Duration;

        let (_temp, central, project) = native_read_fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let (current, base_hash) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = maintenance_plan(&binding, current, "wiki:node:replacement-refused");
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let retained = path.with_file_name("retained-original-wiki.json");
        let original = fs::read(&path).unwrap();
        let foreign = project.join("unselected-foreign-material");
        fs::write(&foreign, b"unselected material which is not a Wiki document").unwrap();
        fs::rename(&path, &retained).unwrap();
        symlink(&foreign, &path).unwrap();
        let error = binding.persist_agent_wiki(&plan, &base_hash).unwrap_err();
        assert_eq!(error.code(), "projectcentral.source_binding_changed");
        assert_eq!(error.details().get("command_effect"), Some(&"none".to_string()));
        assert_eq!(fs::read(&retained).unwrap(), original);
        assert_eq!(fs::read(&foreign).unwrap(), b"unselected material which is not a Wiki document");
        fs::remove_file(&path).unwrap();

        let argv = vec!["/usr/bin/mkfifo".to_string(), "-m".to_string(),
            "600".to_string(), path.to_string_lossy().into_owned()];
        let created = SystemRunner::new().with_timeout(Duration::from_secs(3))
            .run(&argv).unwrap();
        assert!(created.ok(), "{}", created.stderr);
        let (sent, received) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            sent.send(binding.persist_agent_wiki(&plan, &base_hash)).unwrap();
        });
        let error = received.recv_timeout(Duration::from_secs(3))
            .expect("captured native Wiki writer must not wait for a FIFO producer")
            .unwrap_err();
        worker.join().unwrap();
        assert_eq!(error.code(), "projectcentral.source_binding_changed");
        assert_eq!(error.details().get("command_effect"), Some(&"none".to_string()));
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
        assert_eq!(fs::read(&retained).unwrap(), original);
        fs::remove_file(&path).unwrap();
        fs::rename(retained, path).unwrap();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn retained_writer_refuses_equal_basis_foreign_root_and_preserves_current_authority_split() {
        use std::os::unix::fs::{symlink, MetadataExt};
        let (temp, central, project) = native_read_fixture();
        let (_other_temp, _other_central, other_project) = native_read_fixture();
        let alias = temp.path().join("writer-native-root-alias");
        symlink(&project, &alias).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&alias, Some(&central)).unwrap();
        let (current, basis) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = maintenance_plan(&binding, current, "wiki:node:affiliated-writer");
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let other_path = other_project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let original = fs::read(&path).unwrap();
        let other = fs::read(&other_path).unwrap();
        assert_eq!(original, other, "the content basis cannot distinguish native owner roots");
        let other_metadata = fs::metadata(&other_path).unwrap();
        fs::remove_file(&alias).unwrap();
        symlink(&other_project, &alias).unwrap();
        let refusal = binding.persist_agent_wiki(&plan, &basis).unwrap_err();
        assert_eq!(refusal.code(), "projectcentral.source_binding_changed");
        assert_eq!(refusal.details()["command_effect"], "none");
        assert_eq!(refusal.details()["source"], binding.semantic.canonical_wiki.as_str());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read(&other_path).unwrap(), other);
        fs::remove_file(&alias).unwrap();
        symlink(&project, &alias).unwrap();

        // This is the fixture's authorised semantic maintenance route. Current
        // read disclosure is separate and cannot become a new mutation gate.
        let marker = project.join(NO_AGENT_RETRIEVAL_MARKER);
        fs::write(&marker, b"retrieval is currently withheld").unwrap();
        assert_eq!(binding.load_project_wiki().unwrap_err().code(), "projectcentral.source_withheld");
        assert!(binding.persist_agent_wiki(&plan, &basis).unwrap());
        fs::remove_file(marker).unwrap();
        let index = SemanticWikiIndex::rebuild(binding.load_project_wiki().unwrap()).unwrap();
        assert!(index.node(&ResourceRef::parse("wiki:node:affiliated-writer").unwrap()).is_some());
        assert_eq!(fs::read(&other_path).unwrap(), other);
        let after = fs::metadata(&other_path).unwrap();
        assert_eq!(after.dev(), other_metadata.dev());
        assert_eq!(after.ino(), other_metadata.ino());
        assert_eq!(after.modified().unwrap(), other_metadata.modified().unwrap());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_writer_completion_keeps_publication_or_noop_when_original_alias_disappears() {
        use std::os::unix::fs::{symlink, MetadataExt};
        for changed in [false, true] {
            let (temp, central, project) = native_read_fixture();
            let (_other_temp, _other_central, other_project) = native_read_fixture();
            let alias = temp.path().join("actual-owner-completion-alias");
            symlink(&project, &alias).unwrap();
            let binding = ProjectCentralFilesystemBinding::inspect(&alias, Some(&central)).unwrap();
            let (current, basis) = binding.load_project_wiki_for_maintenance().unwrap();
            let plan = if changed {
                maintenance_plan(&binding, current, "wiki:node:completed-before-alias-loss")
            } else {
                plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
                    current_objects: current, upserts: vec![],
                    observed_source_revisions: binding.observed_source_revisions(),
                    human_source_proposals: vec![],
                }).unwrap()
            };
            let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
            let original = fs::read(&path).unwrap();
            let original_metadata = fs::metadata(&path).unwrap();
            let other_path = other_project.join(PROJECTCENTRAL_WIKI_SOURCE);
            let other = fs::read(&other_path).unwrap();
            let removed = alias.clone();
            let _observation = observe_wiki_publication(Box::new(move |path| {
                assert!(path.ends_with(PROJECTCENTRAL_WIKI_SOURCE));
                fs::remove_file(&removed).unwrap();
            }));
            let failure = binding.persist_agent_wiki(&plan, &basis).unwrap_err();
            AFTER_WIKI_PUBLICATION.with(|slot| assert!(slot.borrow().is_none()));
            let actual = fs::metadata(&alias).unwrap_err();
            let cause = std::error::Error::source(&failure).unwrap()
                .downcast_ref::<std::io::Error>().unwrap();
            assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
            assert_eq!(cause.raw_os_error(), actual.raw_os_error());
            assert_eq!(failure.details()["source"], binding.semantic.canonical_wiki.as_str());
            let after = fs::read(&path).unwrap();
            if changed {
                assert_eq!(failure.code(), "knowledge.wiki_publication_uncertain");
                assert_eq!(failure.details()["changed"], "true");
                assert_eq!(failure.details()["published"], "true");
                assert_eq!(failure.details()["command_effect"], "unknown");
                assert_eq!(failure.details()["base_hash"], basis);
                assert_eq!(failure.details()["published_hash"], content_hash(&after));
                assert_eq!(failure.details()["canonical_member"], PROJECTCENTRAL_WIKI_SOURCE);
                let original_cause: Value = serde_json::from_str(&failure.details()["cause"]).unwrap();
                assert_eq!(original_cause["details"]["observation_stage"], "owner_root");
                let index = SemanticWikiIndex::rebuild(parse_wiki_objects(
                    std::str::from_utf8(&after).unwrap()).unwrap()).unwrap();
                assert!(index.node(&ResourceRef::parse("wiki:node:completed-before-alias-loss").unwrap()).is_some());
            } else {
                assert_eq!(failure.details()["changed"], "false");
                assert_eq!(failure.details()["published"], "false");
                assert_eq!(failure.details()["command_effect"], "none");
                assert_eq!(failure.details()["observed_basis"], content_hash(&original));
                assert_eq!(after, original);
                let metadata = fs::metadata(&path).unwrap();
                assert_eq!(metadata.ino(), original_metadata.ino());
                assert_eq!(metadata.modified().unwrap(), original_metadata.modified().unwrap());
            }
            assert_eq!(fs::read(&other_path).unwrap(), other);
            symlink(&project, &alias).unwrap();
            let fresh = ProjectCentralFilesystemBinding::inspect(&alias, Some(&central)).unwrap();
            assert_eq!(fresh.semantic.canonical_wiki, binding.semantic.canonical_wiki);
        }
    }

    #[cfg(unix)]
    #[test]
    fn provider_and_wiki_delegate_ordinary_material_identity_to_physical_owner() {
        let (_temp, central, project) = native_read_fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        let path = project.join("VISION.md");
        let alias = project.join("unselected-vision-hardlink.md");
        fs::hard_link(&path, &alias).unwrap();
        assert!(path_agent_readable(&project, Path::new("VISION.md")));
        match provider.read(&read_request(VISION_REF)) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Unknown);
                assert!(absence.reason.contains("knowledge.wiki_publication_identity"));
                assert!(!absence.reason.contains("withheld"));
            }
            other => panic!("material identity refusal bypassed by provider: {other:?}"),
        }
        fs::remove_file(alias).unwrap();
        exact_provider_payload(&mut provider, VISION_REF, "Retained native human vision");

        let wiki = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let original = fs::read(&wiki).unwrap();
        let alias = project.join("unselected-wiki-hardlink.json");
        fs::hard_link(&wiki, &alias).unwrap();
        assert_eq!(binding.load_project_wiki().unwrap_err().code(),
            "knowledge.wiki_publication_identity");
        assert_eq!(fs::read(&wiki).unwrap(), original);
        fs::remove_file(alias).unwrap();
        assert_eq!(binding.load_project_wiki().unwrap().len(), 2);
    }

    #[test]
    fn actual_source_and_wiki_body_reads_recheck_withdrawal_after_io() {
        let (_temp, central, project) = native_read_fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        let path = project.join("ProjectCentral/user/research/deep/purpose.md");
        let original = fs::read(&path).unwrap();
        let marker = path.parent().unwrap().join(NO_AGENT_RETRIEVAL_MARKER);
        let observed_marker = marker.clone();
        let body_observation = observe_source_read(Box::new(move |read_path| {
            assert_eq!(fs::read(read_path).unwrap(), original);
            fs::write(observed_marker, b"withdrawn after the actual body read").unwrap();
        }));
        provider_withheld(&mut provider, PURPOSE_REF);
        AFTER_SOURCE_READ.with(|slot| assert!(slot.borrow().is_none()));
        drop(body_observation);
        exact_provider_payload(&mut provider, VISION_REF, "Retained native human vision");
        fs::remove_file(marker).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");

        let root = central.join(CENTRAL_ROOT_WIKI_SOURCE);
        let original_wiki = fs::read(&root).unwrap();
        let marker = root.parent().unwrap().join(NO_AGENT_RETRIEVAL_MARKER);
        let observed_marker = marker.clone();
        let wiki_observation = observe_source_read(Box::new(move |read_path| {
            assert_eq!(fs::read(read_path).unwrap(), original_wiki);
            fs::write(observed_marker, b"withdrawn after the actual Wiki read").unwrap();
        }));
        assert_eq!(binding.load_root_wiki().unwrap_err().code(), "projectcentral.source_withheld");
        AFTER_SOURCE_READ.with(|slot| assert!(slot.borrow().is_none()));
        drop(wiki_observation);
        exact_provider_payload(&mut provider, VISION_REF, "Retained native human vision");
        fs::remove_file(marker).unwrap();
        assert_eq!(binding.load_root_wiki().unwrap().unwrap().len(), 2);
    }

    #[test]
    fn public_native_read_predicate_rejects_escape_and_directory_self_marker() {
        let (_temp, central, project) = native_read_fixture();
        assert!(path_agent_readable(&project, Path::new("./VISION.md")));
        assert!(!path_agent_readable(&project, Path::new("../demo/VISION.md")));
        assert!(!path_agent_readable(&project, &central.join(CENTRAL_ROOT_WIKI_SOURCE)));
        assert!(!path_agent_readable(&project, Path::new("")));
        let directory = Path::new("ProjectCentral/user/research");
        assert!(path_agent_readable(&project, directory));
        let marker = project.join(directory).join(NO_AGENT_RETRIEVAL_MARKER);
        fs::write(&marker, b"withheld native directory").unwrap();
        assert!(!path_agent_readable(&project, directory));
        assert!(!path_agent_readable(&project, &directory.join("deep/purpose.md")));
        fs::remove_file(marker).unwrap();
        assert!(path_agent_readable(&project, directory));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn retained_in_root_alias_mapping_requires_fresh_inspection_after_retarget() {
        use std::os::unix::fs::{symlink, MetadataExt};
        let (_temp, central, project) = native_read_fixture();
        let human = project.join(PROJECTCENTRAL_HUMAN_ROOT);
        let requested = human.join("research");
        let admitted = human.join("admitted-room");
        let changed = human.join("changed-room");
        fs::rename(&requested, &admitted).unwrap();
        symlink(&admitted, &requested).unwrap();
        write(&changed.join("deep/purpose.md"), "Different actual in-root source room");
        let original_path = admitted.join("deep/purpose.md");
        let original = fs::read(&original_path).unwrap();
        let original_metadata = fs::metadata(&original_path).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");
        fs::remove_file(&requested).unwrap();
        let actual = fs::symlink_metadata(requested.join("deep/purpose.md")).unwrap_err();
        assert_eq!(actual.kind(), std::io::ErrorKind::NotFound);
        match provider.read(&read_request(PURPOSE_REF)) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Unknown);
                assert!(absence.reason.contains("observation_stage=owner_parent"));
                assert!(absence.reason.contains(&actual.to_string()));
            }
            other => panic!("lost original member alias was labelled as a missing retained source: {other:?}"),
        }
        assert_eq!(fs::read(&original_path).unwrap(), original);
        let retained = fs::metadata(&original_path).unwrap();
        assert_eq!(retained.dev(), original_metadata.dev());
        assert_eq!(retained.ino(), original_metadata.ino());
        assert_eq!(retained.modified().unwrap(), original_metadata.modified().unwrap());
        symlink(&changed, &requested).unwrap();
        assert!(path_agent_readability(&project,
            Path::new("ProjectCentral/user/research/deep/purpose.md")).unwrap());
        match provider.read(&read_request(PURPOSE_REF)) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Unknown);
                assert!(absence.reason.contains("projectcentral.source_binding_changed"));
                assert!(absence.reason.contains("expected_member="));
                assert!(absence.reason.contains("observed_member="));
            }
            other => panic!("retained native SourceRef disclosed a retargeted room: {other:?}"),
        }
        let fresh = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        exact_provider_payload(&mut fresh.file_provider().unwrap(), PURPOSE_REF,
            "Different actual in-root source room");
        assert_eq!(fresh.semantic.project, binding.semantic.project);
        assert_eq!(fresh.semantic.canonical_wiki, binding.semantic.canonical_wiki);
        let current = fs::metadata(&original_path).unwrap();
        assert_eq!(fs::read(&original_path).unwrap(), original);
        assert_eq!(current.dev(), original_metadata.dev());
        assert_eq!(current.ino(), original_metadata.ino());
        assert_eq!(current.modified().unwrap(), original_metadata.modified().unwrap());
        fs::remove_file(&requested).unwrap();
        symlink(&admitted, &requested).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn native_publication_can_replace_source_inode_without_reminting_binding() {
        use std::os::unix::fs::MetadataExt;
        let (_temp, central, project) = native_read_fixture();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let original = fs::read(&path).unwrap();
        let original_metadata = fs::metadata(&path).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let source = binding.semantic.canonical_wiki.clone();
        let mut provider = binding.file_provider().unwrap();
        let next = wiki_json("Actual native publication replaces material", Some(PURPOSE_REF));
        assert!(publication::publish_wiki(&path, &next, &content_hash(&original)).unwrap());
        let current = fs::metadata(&path).unwrap();
        assert_eq!(current.dev(), original_metadata.dev());
        assert_ne!(current.ino(), original_metadata.ino());
        exact_provider_payload(&mut provider, source.as_str(), &next);
        assert_eq!(binding.load_project_wiki().unwrap().len(), 2);
        assert_eq!(binding.semantic.canonical_wiki, source);
        assert_eq!(binding.semantic.sources.iter().find(|item| item.source == source)
            .unwrap().standing, ProjectCentralStanding::AgentMaintained);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn actual_inspection_refuses_one_way_project_or_inherited_root_retarget() {
        use std::os::unix::fs::{symlink, MetadataExt};
        for inherited in [false, true] {
            let (temp, central, project) = native_read_fixture();
            let (_other_temp, other_central, other_project) = native_read_fixture();
            let manifest = project.join("ProjectCentral/project.json");
            let original_manifest = fs::read(&manifest).unwrap();
            let original_metadata = fs::metadata(&manifest).unwrap();
            let original_wiki = fs::read(project.join(PROJECTCENTRAL_WIKI_SOURCE)).unwrap();
            let alias = temp.path().join("inspected-root-alias");
            let (original_root, changed_root) = if inherited {
                (&central, &other_central)
            } else {
                (&project, &other_project)
            };
            symlink(original_root, &alias).unwrap();
            let requested_project = if inherited { project.clone() } else { alias.clone() };
            let requested_central = if inherited { alias.clone() } else { central.clone() };
            let initial = ProjectCentralFilesystemBinding::inspect(
                &requested_project, Some(&requested_central),
            ).unwrap();
            let source = initial.semantic.canonical_wiki.clone();
            let changed_root = changed_root.to_path_buf();
            let observed_alias = alias.clone();
            let _observation = observe_manifest_read(Box::new(move |path| {
                assert!(path.ends_with("ProjectCentral/project.json"));
                fs::remove_file(&observed_alias).unwrap();
                symlink(&changed_root, &observed_alias).unwrap();
            }));
            let refusal = ProjectCentralFilesystemBinding::inspect(
                &requested_project, Some(&requested_central),
            ).unwrap_err();
            AFTER_MANIFEST_READ.with(|slot| assert!(slot.borrow().is_none()));
            assert_eq!(refusal.code(), "projectcentral.source_binding_changed");
            assert_eq!(refusal.details()["owner_root"], alias.display().to_string());
            assert_eq!(fs::read(&manifest).unwrap(), original_manifest);
            assert_eq!(fs::read(project.join(PROJECTCENTRAL_WIKI_SOURCE)).unwrap(), original_wiki);
            let current = fs::metadata(&manifest).unwrap();
            assert_eq!(current.dev(), original_metadata.dev());
            assert_eq!(current.ino(), original_metadata.ino());
            assert_eq!(current.modified().unwrap(), original_metadata.modified().unwrap());

            fs::remove_file(&alias).unwrap();
            symlink(original_root, &alias).unwrap();
            let reopened = ProjectCentralFilesystemBinding::inspect(
                &requested_project, Some(&requested_central),
            ).unwrap();
            assert_eq!(reopened.semantic.canonical_wiki, source);
            assert_eq!(reopened.semantic.project, initial.semantic.project);
            assert_eq!(reopened.load_project_wiki().unwrap().len(), 2);
        }
    }

    #[cfg(unix)]
    #[test]
    fn retained_native_alias_retarget_requires_fresh_owner_binding() {
        use std::os::unix::fs::symlink;

        let (temp, central, project) = native_read_fixture();
        let (_other_temp, other_central, other_project) = native_read_fixture();
        let manifest_path = other_project.join("ProjectCentral/project.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["project_id"] = serde_json::json!("epilogos/other");
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let relations_path = other_project.join(PROJECTCENTRAL_GROUND_RELATIONS_SOURCE);
        let mut relations: Value = serde_json::from_slice(&fs::read(&relations_path).unwrap()).unwrap();
        relations["project_id"] = serde_json::json!("epilogos/other");
        fs::write(relations_path, serde_json::to_vec(&relations).unwrap()).unwrap();
        write(&other_project.join(PROJECTCENTRAL_WIKI_SOURCE), &wiki_json("Other native root", None));

        let alias = temp.path().join("actual-native-root-alias");
        symlink(&central, &alias).unwrap();
        let alias_project = alias.join("Work/demo");
        let binding = ProjectCentralFilesystemBinding::inspect(&alias_project, Some(&alias)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");
        let original = fs::read(project.join(PROJECTCENTRAL_WIKI_SOURCE)).unwrap();
        let retarget = alias.clone();
        let destination = other_central.clone();
        let observation = observe_source_read(Box::new(move |_| {
            fs::remove_file(&retarget).unwrap();
            symlink(&destination, &retarget).unwrap();
        }));
        match provider.read(&read_request(PURPOSE_REF)) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Unknown);
                assert!(absence.reason.contains("projectcentral.source_binding_changed"));
            }
            other => panic!("retargeted native root returned under retained source: {other:?}"),
        }
        AFTER_SOURCE_READ.with(|slot| assert!(slot.borrow().is_none()));
        drop(observation);
        assert_eq!(binding.load_project_wiki().unwrap_err().code(), "projectcentral.source_binding_changed");
        assert_eq!(binding.load_root_wiki().unwrap_err().code(), "projectcentral.source_binding_changed");
        assert_eq!(fs::read(project.join(PROJECTCENTRAL_WIKI_SOURCE)).unwrap(), original);

        let fresh = ProjectCentralFilesystemBinding::inspect(&alias_project, Some(&alias)).unwrap();
        assert_eq!(fresh.semantic.project_id, "epilogos/other");
        assert_ne!(fresh.semantic.canonical_wiki, binding.semantic.canonical_wiki);
        let mut fresh_provider = fresh.file_provider().unwrap();
        exact_provider_payload(&mut fresh_provider, fresh.semantic.canonical_wiki.as_str(),
            &wiki_json("Other native root", None));
        fs::remove_file(&alias).unwrap();
        symlink(&central, &alias).unwrap();
        let restored = ProjectCentralFilesystemBinding::inspect(&alias_project, Some(&alias)).unwrap();
        assert_eq!(restored.semantic.canonical_wiki, binding.semantic.canonical_wiki);
        assert_eq!(restored.load_project_wiki().unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn replaced_native_project_root_requires_fresh_material_affiliation() {
        use std::os::unix::fs::MetadataExt;

        let (_temp, central, project) = native_read_fixture();
        let (_other_temp, _other_central, other_project) = native_read_fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        let source = project.join("ProjectCentral/user/research/deep/purpose.md");
        let original = fs::read(&source).unwrap();
        let original_metadata = fs::metadata(&source).unwrap();
        write(&other_project.join("ProjectCentral/user/research/deep/purpose.md"),
            "Replacement native material");
        let retained_project = project.with_file_name("retained-original-demo");
        fs::rename(&project, &retained_project).unwrap();
        fs::rename(&other_project, &project).unwrap();
        match provider.read(&read_request(PURPOSE_REF)) {
            ProviderReadResult::Absent(absence) => {
                assert_eq!(absence.kind, AbsenceKind::Unknown);
                assert!(absence.reason.contains("projectcentral.source_binding_changed"));
            }
            other => panic!("replaced root returned under retained material affiliation: {other:?}"),
        }
        let fresh = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        assert_eq!(fresh.semantic.project, binding.semantic.project);
        let mut fresh_provider = fresh.file_provider().unwrap();
        exact_provider_payload(&mut fresh_provider, PURPOSE_REF, "Replacement native material");
        let retained = retained_project.join("ProjectCentral/user/research/deep/purpose.md");
        assert_eq!(fs::read(&retained).unwrap(), original);
        let current = fs::metadata(&retained).unwrap();
        assert_eq!(current.dev(), original_metadata.dev());
        assert_eq!(current.ino(), original_metadata.ino());
        fs::rename(&project, other_project).unwrap();
        fs::rename(retained_project, &project).unwrap();
        let restored = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        assert_eq!(restored.semantic.project, binding.semantic.project);
        exact_provider_payload(&mut restored.file_provider().unwrap(), PURPOSE_REF, "Human purpose");
    }

    #[cfg(unix)]
    #[test]
    fn intermediate_native_alias_checks_membership_and_both_marker_routes() {
        use std::os::unix::fs::{symlink, MetadataExt};

        let (temp, central, project) = native_read_fixture();
        let path = project.join("ProjectCentral/user/research/deep/purpose.md");
        let original = fs::read(&path).unwrap();
        let original_metadata = fs::metadata(&path).unwrap();
        let room = path.parent().unwrap();
        let retained_room = project.join("retained-read-room");
        fs::rename(room, &retained_room).unwrap();
        symlink(&retained_room, room).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");

        let destination_marker = retained_room.join(NO_AGENT_RETRIEVAL_MARKER);
        fs::write(&destination_marker, b"the actual aliased source room is withheld").unwrap();
        assert!(!path_agent_readable(
            &project,
            Path::new("ProjectCentral/user/research/deep/purpose.md"),
        ));
        provider_withheld(&mut provider, PURPOSE_REF);
        exact_provider_payload(&mut provider, VISION_REF, "Retained native human vision");
        fs::remove_file(destination_marker).unwrap();
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");

        let lexical_marker = room.parent().unwrap().join(NO_AGENT_RETRIEVAL_MARKER);
        fs::write(&lexical_marker, b"the lexical source room is withheld").unwrap();
        provider_withheld(&mut provider, PURPOSE_REF);
        fs::remove_file(lexical_marker).unwrap();

        let outside = temp.path().join("outside-declared-native-root");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("purpose.md"), b"unselected external material").unwrap();
        fs::remove_file(room).unwrap();
        symlink(&outside, room).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"unselected external material");
        assert!(!path_agent_readable(
            &project,
            Path::new("ProjectCentral/user/research/deep/purpose.md"),
        ));
        provider_withheld(&mut provider, PURPOSE_REF);
        exact_provider_payload(&mut provider, VISION_REF, "Retained native human vision");

        let retained_source = retained_room.join("purpose.md");
        assert_eq!(fs::read(&retained_source).unwrap(), original);
        let current_metadata = fs::metadata(&retained_source).unwrap();
        assert_eq!(current_metadata.dev(), original_metadata.dev());
        assert_eq!(current_metadata.ino(), original_metadata.ino());
        assert_eq!(current_metadata.modified().unwrap(), original_metadata.modified().unwrap());
        fs::remove_file(room).unwrap();
        fs::rename(retained_room, room).unwrap();
        let restored = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        assert_eq!(restored.semantic.canonical_wiki, binding.semantic.canonical_wiki);
        exact_provider_payload(&mut restored.file_provider().unwrap(), PURPOSE_REF, "Human purpose");
    }

    #[cfg(unix)]
    #[test]
    fn explicit_native_root_alias_and_final_source_symlink_keep_distinct_admission() {
        use std::os::unix::fs::symlink;
        let (temp, central, project) = native_read_fixture();
        let alias = temp.path().join("accepted-native-root-alias");
        symlink(&central, &alias).unwrap();
        let alias_project = alias.join("Work/demo");
        let binding = ProjectCentralFilesystemBinding::inspect(&alias_project, Some(&alias))
            .unwrap();
        let mut provider = binding.file_provider().unwrap();
        exact_provider_payload(&mut provider, VISION_REF, "Retained native human vision");
        exact_provider_payload(
            &mut provider,
            binding.semantic.root_wiki.as_ref().unwrap().as_str(),
            &wiki_json("Root", None),
        );
        let path = project.join("VISION.md");
        let retained = project.join("retained-vision.md");
        fs::rename(&path, &retained).unwrap();
        symlink(project.join("ProjectCentral/user/research/deep/purpose.md"), &path).unwrap();
        provider_withheld(&mut provider, VISION_REF);
        assert_eq!(fs::read_to_string(&retained).unwrap(), "Retained native human vision");
        fs::remove_file(&path).unwrap();
        fs::rename(retained, path).unwrap();
        exact_provider_payload(&mut provider, VISION_REF, "Retained native human vision");
    }

    #[test]
    fn no_agent_retrieval_prunes_subtree_without_magic_private_name() {
        let (_temp, central, project) = fixture();
        write(
            &project.join("ProjectCentral/user/whatever-they-call-private/.no-agent-retrieval"),
            "",
        );
        write(
            &project.join("ProjectCentral/user/whatever-they-call-private/secret.md"),
            "secret",
        );
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        assert!(!binding
            .semantic
            .sources
            .iter()
            .any(|source| source.relative_path.ends_with("secret.md")));
    }

    #[test]
    fn canonical_project_wiki_and_root_wiki_load_from_central_owned_paths() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        assert_eq!(binding.load_project_wiki().unwrap().len(), 2);
        assert_eq!(binding.load_root_wiki().unwrap().unwrap().len(), 2);
    }

    #[test]
    fn adopted_wiki_participates_without_replacing_canonical_identity() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        assert_eq!(binding.load_adopted_wikis().unwrap().len(), 1);
        assert_eq!(
            binding.semantic.canonical_wiki.as_str(),
            "source:central:epilogos/demo:agent-wiki"
        );
        assert_ne!(
            binding.semantic.adopted_wikis[0],
            binding.semantic.canonical_wiki
        );
    }

    #[test]
    fn project_entry_does_not_eagerly_parse_human_payload_or_wiki() {
        let (_temp, central, project) = fixture();
        fs::write(project.join(PROJECTCENTRAL_WIKI_SOURCE), b"not json").unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let entries = binding.semantic.context_sources().unwrap();
        assert!(entries.iter().all(|entry| !entry.disclosure.retrieved));
        assert!(binding.load_project_wiki().is_err());
    }

    #[test]
    fn exact_source_retrieval_is_explicit_and_bounded() {
        let (_temp, central, project) = fixture();
        write(&project.join("ProjectCentral/user/other.md"), "Other");
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut index = ContextSourceIndex::default();
        for entry in binding.semantic.context_sources().unwrap() {
            index.insert(entry);
        }
        let hit = index
            .search(
                &HorizonRequest::agent(Some(binding.semantic.project.clone())),
                "purpose.md",
            )
            .into_iter()
            .next()
            .unwrap();
        let other = index
            .search(
                &HorizonRequest::agent(Some(binding.semantic.project.clone())),
                "other.md",
            )
            .into_iter()
            .next()
            .unwrap();
        let mut provider = binding.file_provider().unwrap();
        let outcome = index.retrieve(
            &ContextSourceReadRequest {
                resource: hit.resource.clone(),
                provider: ProviderRef::parse(PROJECTCENTRAL_FILESYSTEM_PROVIDER).unwrap(),
                target: RetrievalTarget::LocalAgent,
            },
            &mut provider,
        );
        assert!(matches!(outcome, ContextSourceReadOutcome::Retrieved(_)));
        assert!(index.explain(&hit.resource).unwrap().disclosure.retrieved);
        assert!(!index.explain(&other.resource).unwrap().disclosure.retrieved);
    }

    #[test]
    fn unknown_non_text_material_can_exist_without_claiming_semantic_understanding() {
        let (_temp, central, project) = fixture();
        let binary = project.join("ProjectCentral/user/visual.bin");
        fs::write(&binary, [0xff, 0xfe, 0xfd]).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut index = ContextSourceIndex::default();
        for entry in binding.semantic.context_sources().unwrap() {
            index.insert(entry);
        }
        let hit = index
            .search(
                &HorizonRequest::agent(Some(binding.semantic.project.clone())),
                "visual.bin",
            )
            .into_iter()
            .next()
            .unwrap();
        assert!(index.explain(&hit.resource).unwrap().disclosure.exists);
        let mut provider = binding.file_provider().unwrap();
        let outcome = index.retrieve(
            &ContextSourceReadRequest {
                resource: hit.resource,
                provider: ProviderRef::parse(PROJECTCENTRAL_FILESYSTEM_PROVIDER).unwrap(),
                target: RetrievalTarget::LocalAgent,
            },
            &mut provider,
        );
        assert!(matches!(outcome, ContextSourceReadOutcome::Absent(_)));
    }

    #[test]
    fn wiki_maintenance_persists_agent_knowledge_with_provenance_and_not_human_source() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let before =
            fs::read_to_string(project.join("ProjectCentral/user/research/deep/purpose.md"))
                .unwrap();
        let (current, base_hash) = binding.load_project_wiki_for_maintenance().unwrap();
        let source_ref = SourceRef::parse(PURPOSE_REF).unwrap();
        let update = WikiObject::Node(WikiNode {
            profile: CENTRAL_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse("wiki:node:purpose").unwrap(),
            revision: 2,
            provenance: vec![WikiProvenanceRef {
                source_ref: source_ref.clone(),
                source_revision: binding
                    .observed_source_revisions()
                    .get(&source_ref)
                    .cloned(),
                producer_ref: Some(ResourceRef::parse("agent:test").unwrap()),
                generation_ref: Some(ResourceRef::parse("run:test").unwrap()),
                extensions: BTreeMap::new(),
            }],
            node_type: "ProjectKnowledge".into(),
            title: Some("Purpose returned".into()),
            space_refs: vec![ResourceRef::parse("wiki:space:project").unwrap()],
            source_refs: vec![source_ref.clone()],
            local_space_ref: None,
            extensions: BTreeMap::new(),
        });
        let plan = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects: current,
            upserts: vec![update],
            observed_source_revisions: binding.observed_source_revisions(),
            human_source_proposals: vec![HumanSourceRevisionProposal {
                source: source_ref,
                reason: "returned reality creates decision pressure".into(),
                evidence: vec![SourceRef::parse("source:evidence:test").unwrap()],
            }],
        })
        .unwrap();
        assert_eq!(plan.human_source_proposals.len(), 1);
        binding.persist_agent_wiki(&plan, &base_hash).unwrap();
        let reloaded = binding.load_project_wiki().unwrap();
        let index = SemanticWikiIndex::rebuild(reloaded).unwrap();
        assert_eq!(
            index
                .node(&ResourceRef::parse("wiki:node:purpose").unwrap())
                .unwrap()
                .revision,
            2
        );
        assert_eq!(
            fs::read_to_string(project.join("ProjectCentral/user/research/deep/purpose.md"))
                .unwrap(),
            before
        );
    }

    // -----------------------------------------------------------------------
    // Concurrency: the write gate against a peer that lands between read and
    // rename, mirroring `crates/aikit-cli/src/wiki.rs`'s own
    // `concurrency_tests` module for its write path (EpiLogos/ai-kit#215).
    // `persist_agent_wiki` is the write leg of `aikit wiki maintenance`, and
    // the wiki it writes is explicitly agent-maintained, so concurrent
    // writers are the normal case. These are unit tests, not integration
    // tests, for the same reason #215's are: the race is a *sequence* — read
    // (capture base), a peer's independent read-mutate-persist, then this
    // writer's persist — and driving `load_project_wiki_for_maintenance` and
    // `persist_agent_wiki` directly gives that exact interleaving
    // deterministically, using the very functions a real maintenance caller
    // would compose.
    // -----------------------------------------------------------------------

    fn maintenance_node(ref_id: &str) -> WikiObject {
        WikiObject::Node(WikiNode {
            profile: CENTRAL_WIKI_PROFILE.into(),
            ref_id: ResourceRef::parse(ref_id).unwrap(),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: SourceRef::parse(PURPOSE_REF).unwrap(),
                source_revision: None,
                producer_ref: Some(ResourceRef::parse("agent:test").unwrap()),
                generation_ref: Some(ResourceRef::parse("run:test").unwrap()),
                extensions: BTreeMap::new(),
            }],
            node_type: "ProjectKnowledge".into(),
            title: Some(ref_id.into()),
            space_refs: vec![ResourceRef::parse("wiki:space:project").unwrap()],
            source_refs: vec![SourceRef::parse(PURPOSE_REF).unwrap()],
            local_space_ref: None,
            extensions: BTreeMap::new(),
        })
    }

    fn maintenance_plan(
        binding: &ProjectCentralFilesystemBinding,
        current: Vec<WikiObject>,
        upsert_ref: &str,
    ) -> AgentWikiMaintenancePlan {
        plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects: current,
            upserts: vec![maintenance_node(upsert_ref)],
            observed_source_revisions: binding.observed_source_revisions(),
            human_source_proposals: vec![],
        })
        .unwrap()
    }

    #[test]
    fn an_uncontended_agent_wiki_write_still_succeeds() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();

        let (current, base_hash) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = maintenance_plan(&binding, current, "wiki:node:uncontended");
        binding.persist_agent_wiki(&plan, &base_hash).unwrap();

        let reloaded = SemanticWikiIndex::rebuild(binding.load_project_wiki().unwrap()).unwrap();
        assert!(reloaded
            .node(&ResourceRef::parse("wiki:node:uncontended").unwrap())
            .is_some());
    }

    #[test]
    fn maintenance_preserves_header_extensions_and_exact_no_op_source() {
        let (_temp, central, project) = fixture();
        let path = project.join("ProjectCentral/agents/wiki/wiki.json");
        let mut document: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        document["retained_owner_field"] = serde_json::json!({"meaning":"kept", "revision":7});
        let input = format!("  {}\n\n", serde_json::to_string(&document).unwrap());
        fs::write(&path, &input).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let (current, basis) = binding.load_project_wiki_for_maintenance().unwrap();
        let unchanged = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects: current.clone(), upserts: vec![],
            observed_source_revisions: binding.observed_source_revisions(), human_source_proposals: vec![],
        }).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(!binding.persist_agent_wiki(&unchanged, &basis).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        let plan = maintenance_plan(&binding, current, "wiki:node:retained-header");
        assert!(binding.persist_agent_wiki(&plan, &basis).unwrap());
        let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after["retained_owner_field"], document["retained_owner_field"]);
        assert!(SemanticWikiIndex::rebuild(binding.load_project_wiki().unwrap()).unwrap()
            .node(&ResourceRef::parse("wiki:node:retained-header").unwrap()).is_some());
    }

    fn unchanged_wiki_plan(
        binding: &ProjectCentralFilesystemBinding,
        current: Vec<WikiObject>,
    ) -> AgentWikiMaintenancePlan {
        plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects: current,
            upserts: vec![],
            observed_source_revisions: binding.observed_source_revisions(),
            human_source_proposals: vec![],
        }).unwrap()
    }

    fn assert_native_validation_refusal_retains_wiki(
        binding: &ProjectCentralFilesystemBinding,
        plan: &AgentWikiMaintenancePlan,
        path: &Path,
        expected: &AikitError,
    ) {
        let bytes = fs::read(path).unwrap();
        let metadata = fs::metadata(path).unwrap();
        let lock = path.parent().unwrap().join(".wiki.json.publication.lock");
        assert!(!lock.exists());
        let error = binding.persist_agent_wiki(plan, &content_hash(&bytes)).unwrap_err();
        assert_eq!(error.code(), expected.code());
        assert_eq!(error.message(), expected.message());
        assert_eq!(error.details().get("command_effect").map(String::as_str), Some("none"));
        assert_eq!(error.details().get("source").map(String::as_str),
            Some(binding.semantic.canonical_wiki.as_str()));
        assert_eq!(fs::read(path).unwrap(), bytes);
        let after = fs::metadata(path).unwrap();
        assert_eq!(after.modified().unwrap(), metadata.modified().unwrap());
        assert_eq!(after.permissions(), metadata.permissions());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!((after.dev(), after.ino(), after.uid(), after.gid()),
                (metadata.dev(), metadata.ino(), metadata.uid(), metadata.gid()));
        }
        assert!(!lock.exists(), "invalid Wiki reached physical publication");
    }

    #[test]
    fn bound_wiki_writer_refuses_duplicate_current_before_unchanged_acknowledgement() {
        let (_temp, central, project) = native_read_fixture();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let (current, _) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = unchanged_wiki_plan(&binding, current);
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let duplicate = document["objects"][1].clone();
        document["objects"].as_array_mut().unwrap().push(duplicate);
        let malformed = serde_json::to_string(&document).unwrap();
        let expected = aikit_core::WikiDocument::parse(&malformed).unwrap().validate().unwrap_err();
        assert_eq!(expected.code(), "knowledge.wiki_document_invalid");
        fs::write(&path, malformed).unwrap();
        assert_native_validation_refusal_retains_wiki(&binding, &plan, &path, &expected);
    }

    #[test]
    fn bound_wiki_writer_refuses_duplicate_next_before_unchanged_acknowledgement() {
        let (_temp, central, project) = native_read_fixture();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let (current, _) = binding.load_project_wiki_for_maintenance().unwrap();
        let mut plan = unchanged_wiki_plan(&binding, current);
        let duplicate = plan.next_objects.iter().find(|object| matches!(object, WikiObject::Node(_)))
            .unwrap().clone();
        plan.next_objects.push(duplicate);
        let mut proposed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let duplicate = proposed["objects"][1].clone();
        proposed["objects"].as_array_mut().unwrap().push(duplicate);
        let proposed = serde_json::to_string(&proposed).unwrap();
        let expected = aikit_core::WikiDocument::parse(&proposed).unwrap().validate().unwrap_err();
        assert_eq!(expected.code(), "knowledge.wiki_document_invalid");
        assert_native_validation_refusal_retains_wiki(&binding, &plan, &path, &expected);
    }

    #[test]
    fn bound_wiki_writer_validates_topology_before_unchanged_or_changed_acknowledgement() {
        for malformed_current in [false, true] {
            let (_temp, central, project) = native_read_fixture();
            let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
            let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
            let (current, _) = binding.load_project_wiki_for_maintenance().unwrap();
            let mut plan = unchanged_wiki_plan(&binding, current);
            let mut proposed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            let mut child = proposed["objects"][0].clone();
            child["ref"] = Value::String("wiki:space:child".into());
            child["node_refs"] = serde_json::json!([]);
            // Both spaces exist, so this is a broken internal parent/child
            // relation rather than a legitimate external federation reference.
            proposed["objects"][0]["child_space_refs"] = serde_json::json!(["wiki:space:child"]);
            proposed["objects"].as_array_mut().unwrap().push(child);
            let malformed = serde_json::to_string(&proposed).unwrap();
            let native = aikit_core::WikiDocument::parse(&malformed).unwrap();
            assert!(native.report().errors.iter().any(|error|
                error.code == "knowledge.wiki_space_asymmetry"));
            let expected = native.validate().unwrap_err();
            plan.next_objects = native.objects().to_vec();
            if malformed_current {
                fs::write(&path, &malformed).unwrap();
            }
            assert_native_validation_refusal_retains_wiki(&binding, &plan, &path, &expected);
        }
    }

    #[test]
    fn bound_wiki_writer_validated_object_reorder_keeps_header_extensions_and_exact_bytes() {
        let (_temp, central, project) = native_read_fixture();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        document["retained_owner_field"] = serde_json::json!({"meaning":"kept", "revision":7});
        document["objects"][1]["producer_extension"] = serde_json::json!({"ordered":["first","second"]});
        document["objects"][1]["source_refs"] = serde_json::json!([PURPOSE_REF, VISION_REF]);
        let input = format!("  {}\n\n", serde_json::to_string(&document).unwrap());
        aikit_core::WikiDocument::parse(&input).unwrap().validate().unwrap();
        fs::write(&path, &input).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let (current, basis) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = unchanged_wiki_plan(&binding, current);
        let proposed_refs = plan.next_objects.iter().map(|object| object.ref_id().clone()).collect::<Vec<_>>();
        let current_refs = aikit_core::WikiDocument::parse(&input).unwrap().objects().iter()
            .map(|object| object.ref_id().clone()).collect::<Vec<_>>();
        assert_ne!(proposed_refs, current_refs);
        let before = fs::metadata(&path).unwrap();
        assert!(!binding.persist_agent_wiki(&plan, &basis).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
        let after = fs::metadata(&path).unwrap();
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
        assert_eq!(after.permissions(), before.permissions());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!((after.dev(), after.ino(), after.uid(), after.gid()),
                (before.dev(), before.ino(), before.uid(), before.gid()));
        }
    }

    #[test]
    fn bound_wiki_writer_validated_inner_array_change_publishes_and_retains_extensions() {
        let (_temp, central, project) = native_read_fixture();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        document["retained_owner_field"] = serde_json::json!({"meaning":"kept", "revision":7});
        document["objects"][1]["producer_extension"] = serde_json::json!({"ordered":["first","second"]});
        document["objects"][1]["source_refs"] = serde_json::json!([PURPOSE_REF, VISION_REF]);
        let input = serde_json::to_string(&document).unwrap();
        aikit_core::WikiDocument::parse(&input).unwrap().validate().unwrap();
        fs::write(&path, &input).unwrap();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let (current, basis) = binding.load_project_wiki_for_maintenance().unwrap();
        let mut plan = unchanged_wiki_plan(&binding, current);
        let node = plan.next_objects.iter_mut().find_map(|object| match object {
            WikiObject::Node(node) => Some(node),
            _ => None,
        }).unwrap();
        node.source_refs.reverse();
        assert!(binding.persist_agent_wiki(&plan, &basis).unwrap());
        let after = fs::read_to_string(&path).unwrap();
        assert_ne!(after, input);
        let native = aikit_core::WikiDocument::parse(&after).unwrap();
        native.validate().unwrap();
        let node = native.objects().iter().find_map(|object| match object {
            WikiObject::Node(node) => Some(node),
            _ => None,
        }).unwrap();
        assert_eq!(node.source_refs, vec![SourceRef::parse(VISION_REF).unwrap(),
            SourceRef::parse(PURPOSE_REF).unwrap()]);
        let after: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(after["retained_owner_field"], document["retained_owner_field"]);
        assert_eq!(node.extensions["producer_extension"], document["objects"][1]["producer_extension"]);
        assert!(path.parent().unwrap().join(".wiki.json.publication.lock").is_file());
    }

    /// The race this whole change exists for: writer A reads the canonical
    /// wiki and builds its maintenance plan from that snapshot; before A
    /// persists, writer B independently reads, plans and persists against the
    /// *same* file; A then attempts to commit its now-stale plan. Without the
    /// fix, A's `rename` simply wins and B's write vanishes with no trace.
    /// With it, A's `persist_agent_wiki` must refuse: B's content is the only
    /// thing on disk afterwards, byte for byte, and no temp file is left
    /// behind.
    #[test]
    fn a_peer_write_between_read_and_persist_is_refused_and_survives_byte_identical() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);

        // Writer A: read (captures the base) and build its plan in memory. No
        // write has happened yet — exactly where a maintenance caller stands
        // right before its own `persist_agent_wiki` call.
        let (current_a, base_hash_a) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan_a = maintenance_plan(&binding, current_a, "wiki:node:from-a");

        // Writer B: an independent, complete read-plan-persist that lands
        // first, through the exact same production path.
        let (current_b, base_hash_b) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan_b = maintenance_plan(&binding, current_b, "wiki:node:from-b");
        binding.persist_agent_wiki(&plan_b, &base_hash_b).unwrap();
        let after_b = fs::read(&path).unwrap();

        // Writer A now tries to commit its stale plan.
        let error = binding
            .persist_agent_wiki(&plan_a, &base_hash_a)
            .unwrap_err();
        assert_eq!(error.code(), "knowledge.wiki_concurrent_write");
        assert!(
            error.message().to_lowercase().contains("re-read"),
            "the refusal must say what to do next: {}",
            error.message()
        );

        // B's write is untouched: byte for byte, not just semantically.
        assert_eq!(
            fs::read(&path).unwrap(),
            after_b,
            "a refused write leaves the file exactly as the peer left it"
        );
        let surviving = SemanticWikiIndex::rebuild(
            parse_wiki_objects(&String::from_utf8(after_b).unwrap()).unwrap(),
        )
        .unwrap();
        assert!(surviving
            .node(&ResourceRef::parse("wiki:node:from-b").unwrap())
            .is_some());
        assert!(surviving
            .node(&ResourceRef::parse("wiki:node:from-a").unwrap())
            .is_none());
        assert!(
            surviving
                .node(&ResourceRef::parse("wiki:node:purpose").unwrap())
                .is_some(),
            "the document is still whole and valid, not half-written"
        );

        // The refused write's temp file does not linger next to the target.
        let temp = path.with_extension("json.aikit-tmp");
        assert!(
            !temp.exists(),
            "a refused write must not leave a temp file behind: {}",
            temp.display()
        );
    }

    /// A rewrite that lands byte-identical content is not a peer's change —
    /// only a peer write that actually altered the file trips the gate.
    #[test]
    fn a_rewrite_that_lands_the_same_bytes_is_not_a_conflict() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let path = project.join(PROJECTCENTRAL_WIKI_SOURCE);
        let original = fs::read(&path).unwrap();

        let (current, base_hash) = binding.load_project_wiki_for_maintenance().unwrap();
        let plan = plan_agent_wiki_maintenance(AgentWikiMaintenanceRequest {
            current_objects: current,
            upserts: vec![],
            observed_source_revisions: binding.observed_source_revisions(),
            human_source_proposals: vec![],
        })
        .unwrap();

        // "Someone" rewrites the file to the exact bytes it already held —
        // e.g. a filesystem sync or an editor save with no real change.
        fs::write(&path, &original).unwrap();

        // Re-committing over that base is not a conflict, because the bytes
        // on disk never actually changed.
        binding.persist_agent_wiki(&plan, &base_hash).unwrap();
    }

    #[test]
    fn projectcentral_wiki_remains_native_knowledge_application_surface() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let wiki = SemanticWikiIndex::rebuild(binding.load_project_wiki().unwrap()).unwrap();
        let app = KnowledgeApplication::new(aikit_core::FamiliarityContext::default())
            .with_wiki(SemanticWikiProvider::new(&wiki));
        let result = app.search("Purpose", 10);
        // Two distinct hits, not one: the curated node, and the authored
        // human source it cites (`purpose.md`), which is findable through
        // its citing node's label without ever becoming curated identity.
        // The curated node leads — findability never displaces the field.
        assert_eq!(result.hits.len(), 2);
        assert_eq!(
            result.hits[0].address,
            KnowledgeAddress::Wiki(ResourceRef::parse("wiki:node:purpose").unwrap())
        );
        assert_eq!(
            result.hits[1].address,
            KnowledgeAddress::Source(SourceRef::parse(PURPOSE_REF).unwrap())
        );
        assert_eq!(
            result.hits[1].kind,
            aikit_core::ResourceKind::KnowledgeSource
        );

        let address = KnowledgeAddress::Wiki(ResourceRef::parse("wiki:node:purpose").unwrap());
        assert!(app.read(&address).unwrap().content.is_some());
        assert!(!app.relations(&address, 1, 16, 16).unwrap().nodes.is_empty());
    }

    #[test]
    fn structured_account_handoff_preserves_exact_sources_and_standings() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let wiki = SemanticWikiIndex::rebuild(binding.load_project_wiki().unwrap()).unwrap();
        let app = KnowledgeApplication::new(aikit_core::FamiliarityContext::default())
            .with_wiki(SemanticWikiProvider::new(&wiki));
        let hit = app.search("Purpose", 10).hits.into_iter().next().unwrap();
        assert!(app.read(&hit.address).unwrap().content.is_some());

        let context = binding.semantic.account_context().unwrap();
        assert_eq!(context.preferred_human_sources.len(), 2);
        assert!(context.preferred_human_sources.iter().any(|source| {
            source.source.as_str() == PURPOSE_REF
                && source.provenance == ProjectCentralProvenance::HumanAuthored
                && source.truth_standing == ProjectCentralTruthStanding::AuthoredHumanPosition
        }));
        assert!(context.preferred_human_sources.iter().any(|source| {
            source.source.as_str() == VISION_REF
                && source.provenance == ProjectCentralProvenance::HumanAdopted
                && source.truth_standing == ProjectCentralTruthStanding::DesignCommitment
        }));
        assert!(context.ground_relations.is_some());
        assert!(context
            .capabilities
            .iter()
            .any(|capability| capability.as_str() == "skill:structured-account-authoring"));
        assert!(!project.join("ProjectCentral/user/ACCOUNT.md").exists());
    }

    // -----------------------------------------------------------------------
    // root_governance_context_source_records: Central's own root
    // `Control/agents/governance/**`, the counterpart to `scan_governance_tree`
    // above for a Project's `ProjectCentral/agents/governance`. This is the
    // read that was entirely missing before the O:I #65 native-owner repair —
    // root governance had no scanner at all, so it could never surface as a
    // ContextSource regardless of where a session stood.
    // -----------------------------------------------------------------------

    #[test]
    fn root_governance_tree_names_every_file_with_the_canonical_root_ref_grammar() {
        let temp = TempDir::new().unwrap();
        let central = temp.path().join("Central");
        write(
            &central.join("Control/agents/governance/authorship-and-return/responsibility.md"),
            "own the gaps",
        );
        write(
            &central.join("Control/agents/governance/attention/consult-authored-ground.md"),
            "consult authored ground",
        );
        let records = root_governance_context_source_records(&central).unwrap();
        let ids: Vec<String> = records
            .iter()
            .map(|record| record.descriptor.id.to_string())
            .collect();
        assert!(ids.contains(
            &"central:source:control:root:Control/agents/governance/authorship-and-return/responsibility.md".to_string()
        ));
        assert!(ids.contains(
            &"central:source:control:root:Control/agents/governance/attention/consult-authored-ground.md".to_string()
        ));
        assert_eq!(ids.len(), 2);
        for record in &records {
            assert_eq!(record.descriptor.kind, ResourceKind::ContextSource);
            assert_eq!(
                record
                    .descriptor
                    .annotations
                    .get("central.standing")
                    .map(String::as_str),
                Some("human-governance")
            );
            // Named, never read: no payload rides the descriptor or its
            // sources, only a locator and a filesystem revision.
            assert!(record.descriptor.sources[0].revision.is_some());
        }
    }

    #[test]
    fn root_governance_tree_respects_no_agent_retrieval() {
        let temp = TempDir::new().unwrap();
        let central = temp.path().join("Central");
        write(
            &central.join("Control/agents/governance/withheld/.no-agent-retrieval"),
            "",
        );
        write(
            &central.join("Control/agents/governance/withheld/secret.md"),
            "not for agents",
        );
        write(
            &central.join("Control/agents/governance/open/notice.md"),
            "fine to name",
        );
        let records = root_governance_context_source_records(&central).unwrap();
        let ids: Vec<String> = records
            .iter()
            .map(|record| record.descriptor.id.to_string())
            .collect();
        assert!(!ids.iter().any(|id| id.contains("secret")));
        assert!(ids.iter().any(|id| id.contains("open/notice.md")));
    }

    #[test]
    fn root_governance_discovery_rechecks_known_ancestor_and_branch_markers() {
        let (_temp, central, project) = native_read_fixture();
        let open = central.join("Control/agents/governance/open/notice.md");
        let private = central.join("Control/agents/governance/withheld/secret.md");
        write(&open, "allowed governance source");
        write(&private, "retained excluded source");
        let original = fs::read(&private).unwrap();
        let original_metadata = fs::metadata(&private).unwrap();
        let ids = |records: Vec<ResourceRecord>| {
            records.into_iter().map(|record| record.descriptor.id.to_string())
                .collect::<Vec<_>>()
        };
        let original_ids = ids(root_governance_context_source_records(&central).unwrap());
        assert_eq!(original_ids.len(), 2);
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let mut provider = binding.file_provider().unwrap();

        let ancestor_marker = central.join("Control/agents/.no-agent-retrieval");
        fs::write(&ancestor_marker, b"known native ancestor is withheld").unwrap();
        assert!(root_governance_context_source_records(&central).unwrap().is_empty());
        exact_provider_payload(&mut provider, PURPOSE_REF, "Human purpose");
        assert_eq!(binding.load_project_wiki().unwrap().len(), 2);
        fs::remove_file(ancestor_marker).unwrap();
        assert_eq!(ids(root_governance_context_source_records(&central).unwrap()), original_ids);

        let branch_marker = private.parent().unwrap().join(NO_AGENT_RETRIEVAL_MARKER);
        fs::write(&branch_marker, b"only this branch is withheld").unwrap();
        let visible = ids(root_governance_context_source_records(&central).unwrap());
        assert_eq!(visible.len(), 1);
        assert!(visible[0].ends_with("governance/open/notice.md"));
        assert_eq!(fs::read(&private).unwrap(), original);
        assert_eq!(
            fs::metadata(&private).unwrap().modified().unwrap(),
            original_metadata.modified().unwrap(),
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let current = fs::metadata(&private).unwrap();
            assert_eq!(current.dev(), original_metadata.dev());
            assert_eq!(current.ino(), original_metadata.ino());
        }
        fs::remove_file(branch_marker).unwrap();
        assert_eq!(ids(root_governance_context_source_records(&central).unwrap()), original_ids);
    }

    #[test]
    fn root_governance_discovery_retains_real_unavailable_directory_cause() {
        let (_temp, central, _project) = native_read_fixture();
        let agents = central.join("Control/agents");
        let retained = central.join("Control/retained-agents");
        fs::rename(&agents, &retained).unwrap();
        fs::write(&agents, b"the native ancestor is currently an ordinary file").unwrap();
        let actual = fs::symlink_metadata(central.join(CENTRAL_ROOT_GOVERNANCE_ROOT))
            .unwrap_err();
        assert_ne!(actual.kind(), std::io::ErrorKind::NotFound);
        let error = root_governance_context_source_records(&central).unwrap_err();
        assert_eq!(error.code(), "projectcentral.source_unavailable");
        assert!(error.message().contains(&actual.to_string()));
        assert_eq!(error.details().get("cause_kind"), Some(&format!("{:?}", actual.kind())));
        assert_eq!(
            error.details().get("cause_raw_os_error"),
            Some(&serde_json::json!(actual.raw_os_error()).to_string()),
        );
        let cause = std::error::Error::source(&error).unwrap()
            .downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), actual.kind());
        assert_eq!(cause.raw_os_error(), actual.raw_os_error());
        fs::remove_file(&agents).unwrap();
        fs::rename(retained, agents).unwrap();
        assert!(root_governance_context_source_records(&central).unwrap().is_empty());
    }

    #[test]
    fn root_governance_tree_is_a_valid_empty_reading_when_the_tree_is_absent() {
        let temp = TempDir::new().unwrap();
        let central = temp.path().join("Central");
        fs::create_dir_all(&central).unwrap();
        let records = root_governance_context_source_records(&central).unwrap();
        assert!(records.is_empty());
    }
}
