//! Filesystem adapter for Central's public ProjectCentral contract.
//!
//! Project entry reads only the small ProjectCentral manifest, Central's optional
//! accepted source-relation ledger, and filesystem metadata. Human material and
//! SemanticWiki payloads remain unloaded until an explicit ContextSource or Wiki
//! read. `.no-agent-retrieval` prunes a subtree before any descendant is disclosed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use aikit_core::{
    parse_wiki_objects, AbsenceKind, AgentWikiMaintenancePlan, AikitError, ContextSourceOperation,
    ContextSourceProvider, ContextSourceProviderCapabilities, ContextSourceProviderStatus,
    ContextSourceReadRequest, ProjectCentralBinding, ProjectCentralProvenance,
    ProjectCentralSourceDescriptor, ProjectCentralSourceKind, ProjectCentralStanding,
    ProjectCentralTreatment, ProjectCentralTruthStanding, ProviderReadResult, ProviderRef,
    ResourceRef, ResourceSource, Result, SourceRef, SourceRevision, SourceState, StructuredAbsence,
    CENTRAL_GROUND_RELATIONS_SCHEMA, CENTRAL_PROJECT_SCHEMA, CENTRAL_ROOT_WIKI_SOURCE,
    CENTRAL_WIKI_PROFILE, NO_AGENT_RETRIEVAL_MARKER, PROJECTCENTRAL_BINDING_VERSION,
    PROJECTCENTRAL_FILESYSTEM_PROVIDER, PROJECTCENTRAL_GOVERNANCE_ROOT,
    PROJECTCENTRAL_GROUND_RELATIONS_SOURCE, PROJECTCENTRAL_HUMAN_ROOT, PROJECTCENTRAL_WIKI_SOURCE,
};
use serde::Deserialize;
use serde_json::Value;

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
pub struct ProjectCentralFilesystemBinding {
    pub semantic: ProjectCentralBinding,
    project_root: PathBuf,
    paths: BTreeMap<ResourceRef, PathBuf>,
    standing: BTreeMap<ResourceRef, ProjectCentralStanding>,
}

impl ProjectCentralFilesystemBinding {
    pub fn inspect(project_root: impl AsRef<Path>, central_root: Option<&Path>) -> Result<Self> {
        let project_root = project_root.as_ref().to_path_buf();
        let manifest_path = project_root.join("ProjectCentral/project.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .map_err(|error| io_error("projectcentral.manifest_read", &manifest_path, error))?;
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
        let human_allowed =
            human_path.exists() && !human_path.join(NO_AGENT_RETRIEVAL_MARKER).exists();
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
        let governance_allowed =
            governance_path.exists() && !governance_path.join(NO_AGENT_RETRIEVAL_MARKER).exists();
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
                agent_readable: exists,
                is_directory: false,
                revision: revision_for(&root_path),
            };
            let key = ResourceRef::parse(root_ref.as_str())?;
            if exists {
                paths.insert(key.clone(), root_path);
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
        let key = ResourceRef::parse(source.as_str())?;
        let path = self.paths.get(&key).ok_or_else(|| {
            AikitError::new(
                "projectcentral.source_unavailable",
                format!("ProjectCentral source {source} is not available"),
            )
        })?;
        let input = fs::read_to_string(path)
            .map_err(|error| io_error("projectcentral.wiki_read", path, error))?;
        parse_wiki_objects(&input)
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
    pub fn persist_agent_wiki(&self, plan: &AgentWikiMaintenancePlan) -> Result<()> {
        let key = ResourceRef::parse(self.semantic.canonical_wiki.as_str())?;
        let path = self.paths.get(&key).ok_or_else(|| {
            AikitError::new(
                "projectcentral.canonical_wiki_unavailable",
                "canonical ProjectCentral Agent Wiki is unavailable",
            )
        })?;
        let rendered = render_wiki_objects(&plan.next_objects)?;
        let temporary = path.with_extension("json.aikit-tmp");
        fs::write(&temporary, rendered)
            .map_err(|error| io_error("projectcentral.wiki_write", &temporary, error))?;
        fs::rename(&temporary, path)
            .map_err(|error| io_error("projectcentral.wiki_replace", path, error))?;
        Ok(())
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }
}

#[derive(Debug, Clone)]
pub struct ProjectCentralFileProvider {
    provider: ProviderRef,
    paths: BTreeMap<ResourceRef, PathBuf>,
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
        let Some(path) = self.paths.get(&request.resource) else {
            return ProviderReadResult::Absent(StructuredAbsence::new(
                AbsenceKind::Missing,
                "ProjectCentral source is not bound to a readable filesystem object",
            ));
        };
        if path.is_dir() {
            return ProviderReadResult::Absent(StructuredAbsence::new(
                AbsenceKind::Bound,
                "ProjectCentral directory exists but has no eager aggregate payload",
            ));
        }
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return ProviderReadResult::Absent(StructuredAbsence::new(
                    AbsenceKind::Missing,
                    format!("ProjectCentral source read failed: {error}"),
                ))
            }
        };
        let payload = match String::from_utf8(bytes) {
            Ok(payload) => payload,
            Err(_) => return ProviderReadResult::Absent(StructuredAbsence::new(
                AbsenceKind::Bound,
                "source exists and is readable, but this text provider cannot interpret its format",
            )),
        };
        let standing = self
            .standing
            .get(&request.resource)
            .copied()
            .unwrap_or(ProjectCentralStanding::Unresolved);
        let source = SourceRef::parse(request.resource.as_str())
            .expect("ContextSource ResourceRef originated from a SourceRef");
        ProviderReadResult::Retrieved {
            payload,
            revision: revision_for(path),
            provenance: vec![ResourceSource {
                source,
                authority: standing.source_authority(),
                revision: revision_for(path),
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
    paths: &mut BTreeMap<ResourceRef, PathBuf>,
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
    paths: &mut BTreeMap<ResourceRef, PathBuf>,
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

#[allow(clippy::too_many_arguments)]
fn push_source(
    sources: &mut Vec<ProjectCentralSourceDescriptor>,
    paths: &mut BTreeMap<ResourceRef, PathBuf>,
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
    if exists && agent_readable {
        paths.insert(key.clone(), absolute);
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

fn path_agent_readable(project_root: &Path, relative: &Path) -> bool {
    let absolute = project_root.join(relative);
    if is_symlink(&absolute) {
        return false;
    }
    let mut cursor = absolute.parent();
    while let Some(directory) = cursor {
        if !directory.starts_with(project_root) {
            return false;
        }
        if directory.join(NO_AGENT_RETRIEVAL_MARKER).is_file() {
            return false;
        }
        if directory == project_root {
            break;
        }
        cursor = directory.parent();
    }
    true
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

fn render_wiki_objects(objects: &[aikit_core::WikiObject]) -> Result<String> {
    let objects = objects
        .iter()
        .map(wiki_object_value)
        .collect::<Result<Vec<_>>>()?;
    serde_json::to_string_pretty(&serde_json::json!({
        "profile": CENTRAL_WIKI_PROFILE,
        "objects": objects
    }))
    .map_err(|error| {
        AikitError::new(
            "projectcentral.wiki_serialize",
            format!("could not serialize Agent Wiki: {error}"),
        )
    })
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
        let temp = TempDir::new().unwrap();
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
        let current = binding.load_project_wiki().unwrap();
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
        binding.persist_agent_wiki(&plan).unwrap();
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

    #[test]
    fn projectcentral_wiki_remains_native_knowledge_application_surface() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let wiki = SemanticWikiIndex::rebuild(binding.load_project_wiki().unwrap()).unwrap();
        let app = KnowledgeApplication::new(aikit_core::FamiliarityContext::default())
            .with_wiki(SemanticWikiProvider::new(&wiki));
        let result = app.search("Purpose", 10);
        assert_eq!(result.hits.len(), 1);
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
}
