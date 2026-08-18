//! Filesystem adapter for Central's public ProjectCentral contract.
//!
//! Project entry reads only the small ProjectCentral manifest plus filesystem
//! metadata. Human material and SemanticWiki payloads remain unloaded until an
//! explicit ContextSource or Wiki read. `.no-agent-retrieval` prunes a subtree
//! before any descendant is disclosed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use aikit_core::{
    parse_wiki_objects, AbsenceKind, AgentWikiMaintenancePlan, AikitError,
    ContextSourceOperation, ContextSourceProvider, ContextSourceProviderCapabilities,
    ContextSourceProviderStatus, ContextSourceReadRequest, ProjectCentralBinding,
    ProjectCentralSourceDescriptor, ProjectCentralSourceKind, ProjectCentralStanding, ProjectRef,
    ProviderReadResult, ProviderRef, ResourceRef, ResourceSource, Result, SemanticWikiIndex,
    SourceAuthority, SourceRef, SourceRevision, SourceState, StructuredAbsence,
    CENTRAL_PROJECT_SCHEMA, CENTRAL_ROOT_WIKI_SOURCE, CENTRAL_WIKI_PROFILE,
    NO_AGENT_RETRIEVAL_MARKER, PROJECTCENTRAL_BINDING_VERSION, PROJECTCENTRAL_FILESYSTEM_PROVIDER,
    PROJECTCENTRAL_GOVERNANCE_ROOT, PROJECTCENTRAL_HUMAN_ROOT, PROJECTCENTRAL_WIKI_SOURCE,
};
use serde::Deserialize;
use serde_json::{Map, Value};

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
        let manifest_text = fs::read_to_string(&manifest_path).map_err(|error| {
            io_error(
                "projectcentral.manifest_read",
                &manifest_path,
                error,
            )
        })?;
        let manifest: Manifest = serde_json::from_str(&manifest_text).map_err(|error| {
            AikitError::new(
                "projectcentral.manifest_invalid",
                format!("invalid ProjectCentral/project.json: {error}"),
            )
        })?;
        validate_manifest(&manifest)?;

        let project = ProjectRef::parse(&manifest.project_id)?;
        let manifest_source = source(&format!("source:central:{}:manifest", manifest.project_id))?;
        let human_root = source(&format!("source:central:{}:human-root", manifest.project_id))?;
        let governance_root = source(&format!(
            "source:central:{}:governance-root",
            manifest.project_id
        ))?;
        let canonical_wiki = source(&format!("source:central:{}:agent-wiki", manifest.project_id))?;
        let native_project_root = source(&format!("source:project:{}:root", manifest.project_id))?;

        let mut sources = Vec::new();
        let mut paths = BTreeMap::new();
        let mut standings = BTreeMap::new();

        push_source(
            &mut sources,
            &mut paths,
            &mut standings,
            &project_root,
            manifest_source.clone(),
            PathBuf::from("ProjectCentral/project.json"),
            ProjectCentralSourceKind::Manifest,
            ProjectCentralStanding::Observed,
            true,
            false,
        )?;

        let human_path = project_root.join(PROJECTCENTRAL_HUMAN_ROOT);
        let human_allowed = human_path.exists() && !human_path.join(NO_AGENT_RETRIEVAL_MARKER).exists();
        push_source(
            &mut sources,
            &mut paths,
            &mut standings,
            &project_root,
            human_root.clone(),
            PathBuf::from(PROJECTCENTRAL_HUMAN_ROOT),
            ProjectCentralSourceKind::HumanRoot,
            ProjectCentralStanding::HumanAuthored,
            human_allowed,
            true,
        )?;
        if human_allowed {
            scan_tree(
                &project_root,
                &human_path,
                &manifest.project_id,
                "human",
                ProjectCentralSourceKind::HumanMaterial,
                ProjectCentralStanding::HumanAuthored,
                &mut sources,
                &mut paths,
                &mut standings,
            )?;
        }

        let governance_path = project_root.join(PROJECTCENTRAL_GOVERNANCE_ROOT);
        let governance_allowed = governance_path.exists()
            && !governance_path.join(NO_AGENT_RETRIEVAL_MARKER).exists();
        push_source(
            &mut sources,
            &mut paths,
            &mut standings,
            &project_root,
            governance_root.clone(),
            PathBuf::from(PROJECTCENTRAL_GOVERNANCE_ROOT),
            ProjectCentralSourceKind::GovernanceRoot,
            ProjectCentralStanding::HumanGovernance,
            governance_allowed,
            true,
        )?;
        if governance_allowed {
            scan_tree(
                &project_root,
                &governance_path,
                &manifest.project_id,
                "governance",
                ProjectCentralSourceKind::GovernanceMaterial,
                ProjectCentralStanding::HumanGovernance,
                &mut sources,
                &mut paths,
                &mut standings,
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
                true,
                false,
            )?;
            adopted_wikis.push(source_ref);
        }

        let root_wiki = if let Some(central_root) = central_root {
            let root_ref = source("source:central:root:agent-wiki")?;
            let root_path = central_root.join(CENTRAL_ROOT_WIKI_SOURCE);
            let exists = root_path.is_file();
            let descriptor = ProjectCentralSourceDescriptor {
                source: root_ref.clone(),
                relative_path: PathBuf::from(CENTRAL_ROOT_WIKI_SOURCE),
                kind: ProjectCentralSourceKind::RootWiki,
                standing: ProjectCentralStanding::AgentMaintained,
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
            Err(_) => {
                return ProviderReadResult::Absent(StructuredAbsence::new(
                    AbsenceKind::Bound,
                    "source exists and is readable, but this text provider cannot interpret its format",
                ))
            }
        };
        let standing = self
            .standing
            .get(&request.resource)
            .copied()
            .unwrap_or(ProjectCentralStanding::Observed);
        let source = SourceRef::parse(request.resource.as_str())
            .expect("ContextSource ResourceRef originated from a SourceRef");
        ProviderReadResult::Retrieved {
            payload,
            revision: revision_for(path),
            provenance: vec![ResourceSource {
                source,
                authority: Some(standing.source_authority()),
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
            format!("expected {CENTRAL_PROJECT_SCHEMA}, found {}", manifest.schema),
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

fn validate_relative_source(raw: &str) -> Result<()> {
    let path = Path::new(raw);
    if path.is_absolute()
        || raw.trim().is_empty()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return Err(AikitError::new(
            "projectcentral.adopted_source_escape",
            "adopted Wiki source must remain project-relative",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn scan_tree(
    project_root: &Path,
    directory: &Path,
    project_id: &str,
    namespace: &str,
    kind: ProjectCentralSourceKind,
    standing: ProjectCentralStanding,
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
        let path = entry.path();
        if entry.file_name() == NO_AGENT_RETRIEVAL_MARKER {
            continue;
        }
        if path.is_dir() {
            scan_tree(
                project_root,
                &path,
                project_id,
                namespace,
                kind,
                standing,
                sources,
                paths,
                standings,
            )?;
            continue;
        }
        let relative = path.strip_prefix(project_root).map_err(|_| {
            AikitError::new(
                "projectcentral.source_escape",
                "ProjectCentral source escaped the Project root",
            )
        })?;
        let local = relative
            .strip_prefix(if namespace == "human" {
                PROJECTCENTRAL_HUMAN_ROOT
            } else {
                PROJECTCENTRAL_GOVERNANCE_ROOT
            })
            .unwrap_or(relative);
        let source_ref = source(&format!(
            "source:central:{project_id}:{namespace}:{}",
            local.to_string_lossy()
        ))?;
        push_source(
            sources,
            paths,
            standings,
            project_root,
            source_ref,
            relative.to_path_buf(),
            kind,
            standing,
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
    agent_readable: bool,
    is_directory: bool,
) -> Result<()> {
    let absolute = project_root.join(&relative_path);
    let exists = if is_directory {
        absolute.is_dir()
    } else {
        absolute.is_file()
    };
    let descriptor = ProjectCentralSourceDescriptor {
        source: source_ref.clone(),
        relative_path,
        kind,
        standing,
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

fn revision_for(path: &Path) -> Option<SourceRevision> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    SourceRevision::parse(&format!("fs:{modified}:{}", metadata.len())).ok()
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
        ContextSourceReadOutcome, HumanSourceRevisionProposal, HorizonRequest, KnowledgeAddress,
        KnowledgeApplication, ProjectCentralSourceKind, RetrievalTarget, SemanticRevision,
        SemanticWikiProvider, WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject, WikiProvenanceRef,
        WikiSpace,
    };
    use tempfile::TempDir;

    use super::*;

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
        write(
            &project.join("ProjectCentral/agents/governance/STYLE.md"),
            "Human governance",
        );
        let human_source = "source:central:epilogos/demo:human:research/deep/purpose.md";
        write(
            &project.join(PROJECTCENTRAL_WIKI_SOURCE),
            &wiki_json("Purpose", Some(human_source)),
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
        assert!(binding.semantic.sources.iter().any(|source| {
            source.kind == ProjectCentralSourceKind::HumanMaterial
                && source.relative_path.ends_with("research/deep/purpose.md")
        }));
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
        let before = fs::read_to_string(
            project.join("ProjectCentral/user/research/deep/purpose.md"),
        )
        .unwrap();
        let current = binding.load_project_wiki().unwrap();
        let source_ref = SourceRef::parse(
            "source:central:epilogos/demo:human:research/deep/purpose.md",
        )
        .unwrap();
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
    fn account_context_exposes_source_refs_without_generating_an_account() {
        let (_temp, central, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, Some(&central)).unwrap();
        let context = binding.semantic.account_context().unwrap();
        assert_eq!(context.preferred_human_sources.len(), 1);
        assert!(context
            .capabilities
            .iter()
            .any(|capability| capability.as_str() == "skill:product-understanding"));
        assert!(!project.join("ProjectCentral/user/ACCOUNT.md").exists());
    }
}
