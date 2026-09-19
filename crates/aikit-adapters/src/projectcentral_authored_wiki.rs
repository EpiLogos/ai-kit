//! ProjectCentral → authored SemanticWiki relation binding.
//!
//! Central remains the source owner: `ProjectCentralFilesystemBinding` supplies
//! stable SourceRef, path, revision and epistemic standing. AIKit reads only
//! eligible retained Markdown files already disclosed by that binding and compiles
//! their explicit links/OKF Properties into the existing SemanticWiki relation
//! field. No source migration or second Wiki store is introduced.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use aikit_core::knowledge_living::KnowledgeDependency;
use aikit_core::knowledge_navigation::{PendingAuthoredTarget, ProjectAuthoredPending};
use aikit_core::knowledge_okf::{AuthoredRelationChannel, AuthoredRelationResolution};
use aikit_core::knowledge_wiki::{WikiEdge, WikiObject};
use aikit_core::knowledge_wiki_index::SemanticWikiIndex;
use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::{Result, NO_AGENT_RETRIEVAL_MARKER};
use serde::{Deserialize, Serialize};

use crate::authored_wiki_source::{
    authored_relation_dependencies, compile_authored_wiki_relations,
    parse_authored_wiki_source_with_authority, rebuild_semantic_wiki_with_authored_relations,
    AuthoredWikiRelationCompilation, AuthoredWikiSourceProjection,
};
use crate::ProjectCentralFilesystemBinding;

pub const PROJECTCENTRAL_AUTHORED_WIKI_VERSION: &str = "aikit.projectcentral-authored-wiki/v1";

/// Bound matching the per-file read cap already applied to canonical Wiki
/// sources (`central_wiki.rs`'s 4 MiB bound) and to CLI discovery scanning
/// (`app/knowledge.rs::MAX_DISCOVERY_FILE_BYTES`): an authored Markdown
/// source larger than this is disclosed as an absence and skipped rather
/// than read whole into memory on every command invocation.
pub const AUTHORED_WIKI_MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;

/// Materialized read model for one ProjectCentral world. Every field is
/// rebuildable from Central-owned source descriptors/files plus the canonical
/// project Wiki.
#[derive(Debug)]
pub struct ProjectCentralAuthoredWiki {
    pub version: String,
    pub wiki_objects: Vec<WikiObject>,
    pub source_projections: Vec<AuthoredWikiSourceProjection>,
    pub compilation: AuthoredWikiRelationCompilation,
    pub dependencies: Vec<KnowledgeDependency>,
    pub index: SemanticWikiIndex,
    /// Source interpretation/rebuild is deterministic and never invokes a model.
    pub automatic_agent_or_model_invocation: bool,
    /// An oversized, unreadable, or unparseable eligible source is disclosed
    /// here and skipped — it never aborts the compile for every other
    /// eligible source in this project.
    pub absences: Vec<String>,
}

/// Compile the current ProjectCentral world into the existing SemanticWiki index.
///
/// Only file-like, agent-readable Markdown sources already present in Central's
/// public binding participate. JSON Agent Wiki storage, directories, hidden
/// subtrees and unavailable sources retain their existing semantics.
pub fn projectcentral_authored_wiki(
    binding: &ProjectCentralFilesystemBinding,
) -> Result<ProjectCentralAuthoredWiki> {
    let wiki_objects = binding.load_project_wiki()?;
    let mut source_projections = Vec::new();
    let mut absences = Vec::new();

    // Project-relative path → the owner's stable source ref for disclosed
    // files. A link target that names a disclosed file resolves to that
    // file's own source ref rather than a derived address.
    let disclosed: BTreeMap<PathBuf, ResourceRef> = binding
        .semantic
        .sources
        .iter()
        .filter(|descriptor| {
            descriptor.exists && descriptor.agent_readable && !descriptor.is_directory
        })
        .filter_map(|descriptor| {
            ResourceRef::parse(descriptor.source.as_str())
                .ok()
                .map(|reference| (descriptor.relative_path.clone(), reference))
        })
        .collect();

    for descriptor in &binding.semantic.sources {
        if !descriptor.exists
            || !descriptor.agent_readable
            || descriptor.is_directory
            || !is_markdown_path(&descriptor.relative_path)
        {
            continue;
        }

        let path = binding.project_root().join(&descriptor.relative_path);
        let relative_display = descriptor.relative_path.display();
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                absences.push(format!(
                    "authored wiki source {relative_display} is unreadable: {error}"
                ));
                continue;
            }
        };
        if metadata.len() > AUTHORED_WIKI_MAX_SOURCE_BYTES {
            absences.push(format!(
                "authored wiki source {relative_display} exceeds the bounded read size ({AUTHORED_WIKI_MAX_SOURCE_BYTES} bytes); skipped"
            ));
            continue;
        }
        let markdown = match fs::read_to_string(&path) {
            Ok(markdown) => markdown,
            Err(error) => {
                absences.push(format!(
                    "authored wiki source {relative_display} is unreadable: {error}"
                ));
                continue;
            }
        };
        let subject_ref = ResourceRef::parse(descriptor.source.as_str())?;
        let authority = descriptor
            .standing
            .source_authority()
            .unwrap_or(SourceAuthority::Observed);
        match parse_authored_wiki_source_with_authority(
            subject_ref,
            descriptor.source.clone(),
            authority,
            descriptor.revision.clone(),
            vec![descriptor.relative_path.to_string_lossy().into_owned()],
            &markdown,
        ) {
            Ok(mut projection) => {
                resolve_filesystem_link_targets(
                    binding.project_root(),
                    &descriptor.relative_path,
                    binding.semantic.project_id.as_str(),
                    &disclosed,
                    &mut projection.relations,
                    &mut absences,
                );
                source_projections.push(projection);
            }
            Err(error) => absences.push(format!(
                "authored wiki source {relative_display} could not be parsed: {}",
                error.message()
            )),
        }
    }

    source_projections.sort_by(|left, right| left.source_ref.cmp(&right.source_ref));
    let compilation = compile_authored_wiki_relations(&source_projections, &wiki_objects, &[])?;
    let dependencies = authored_relation_dependencies(&source_projections);
    let index = rebuild_semantic_wiki_with_authored_relations(&wiki_objects, &compilation)?;

    Ok(ProjectCentralAuthoredWiki {
        version: PROJECTCENTRAL_AUTHORED_WIKI_VERSION.into(),
        wiki_objects,
        source_projections,
        compilation,
        dependencies,
        index,
        automatic_agent_or_model_invocation: false,
        absences,
    })
}

pub struct AuthoredWikiWorldReading {
    pub edges: Vec<WikiEdge>,
    /// Structural absences only (unreadable/oversized/unparseable sources,
    /// collisions, ambiguities, withheld targets). Pending authored relations
    /// are disclosed per project through `pending`, not as one string per
    /// occurrence.
    pub absences: Vec<String>,
    /// Per-project rollups of pending authored relations. Search/resolve/frame
    /// replies carry at most their own scope's rollup; `knowledge status`
    /// carries every project plus this per-target detail.
    pub pending: Vec<ProjectAuthoredPending>,
    /// Which project compiled each authored edge (`wiki:edge:authored:*` ref →
    /// Work-relative project display). Scoped knowledge queries use it to keep
    /// another project's authored edges out of their results.
    pub edge_projects: BTreeMap<String, String>,
}

/// Discover and compile every ProjectCentral authored-Markdown wiki
/// disclosed by the world: each `Work/<project>/ProjectCentral` register.
/// Mirrors `capability_matrix::compile_world_matrices` and
/// `central_entities::materialise_central_entities` — infallible, and every
/// absence (a project whose ProjectCentral binding cannot be inspected, an
/// unreadable/oversized/unparseable source, a pending `[[link]]`, a
/// colliding edge identity) is disclosed rather than aborting the world.
/// Only edges are returned: this never promotes a Markdown source to a
/// `WikiNode` (see `authored_wiki_source.rs`'s module doc comment).
pub fn compile_world_authored_wiki(central_root: &Path) -> AuthoredWikiWorldReading {
    let mut edges = Vec::new();
    let mut absences = Vec::new();
    let mut pending = Vec::new();
    let mut edge_projects = BTreeMap::new();
    let mut seen_edge_refs = BTreeSet::new();

    let mut projects: Vec<_> = match fs::read_dir(central_root.join("Work")) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect(),
        Err(_) => Vec::new(),
    };
    projects.sort();

    for project_root in projects {
        let home = project_root
            .strip_prefix(central_root)
            .unwrap_or(&project_root)
            .display()
            .to_string();
        let binding =
            match ProjectCentralFilesystemBinding::inspect(&project_root, Some(central_root)) {
                Ok(binding) => binding,
                Err(error) => {
                    absences.push(format!(
                        "ProjectCentral authored wiki unavailable at {home}: {}",
                        error.message()
                    ));
                    continue;
                }
            };
        let projected = match projectcentral_authored_wiki(&binding) {
            Ok(projected) => projected,
            Err(error) => {
                absences.push(format!(
                    "ProjectCentral authored wiki compile failed at {home}: {}",
                    error.message()
                ));
                continue;
            }
        };
        for absence in projected.absences {
            absences.push(format!("{home}: {absence}"));
        }
        // Identical pendings collapse: one rollup per project, one row per
        // distinct (subject, target, relation), occurrences counted.
        if let Some(rollup) = pending_rollup(
            home.clone(),
            Some(binding.semantic.project_id.clone()),
            &projected.compilation.pending,
        ) {
            pending.push(rollup);
        }
        for edge in projected.compilation.edges {
            let key = edge.ref_id.as_str().to_owned();
            if seen_edge_refs.insert(key.clone()) {
                edge_projects.insert(key, home.clone());
                edges.push(edge);
            } else {
                absences.push(format!(
                    "Authored wiki at {home} re-declares edge {key} from an earlier project; kept the first"
                ));
            }
        }
    }

    AuthoredWikiWorldReading {
        edges,
        absences,
        pending,
        edge_projects,
    }
}

/// Build one project's pending rollup from its compiled pendings. Identical
/// (subject, target, relation) pendings collapse; occurrences are counted.
pub fn pending_rollup(
    project: impl Into<String>,
    project_id: Option<String>,
    pending: &[crate::authored_wiki_source::PendingAuthoredRelation],
) -> Option<ProjectAuthoredPending> {
    if pending.is_empty() {
        return None;
    }
    let mut counts: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for pending_relation in pending {
        *counts
            .entry((
                pending_relation.subject_ref.as_str().to_owned(),
                pending_relation.evidence.raw_target.clone(),
                pending_relation.evidence.relation.clone(),
            ))
            .or_default() += 1;
    }
    let targets = counts
        .into_iter()
        .map(
            |((subject_ref, target, relation), occurrences)| PendingAuthoredTarget {
                subject_ref,
                target,
                relation,
                occurrences,
            },
        )
        .collect::<Vec<_>>();
    Some(ProjectAuthoredPending {
        unresolved_targets: targets
            .iter()
            .map(|target| target.target.clone())
            .collect::<BTreeSet<_>>()
            .len(),
        occurrences: pending.len(),
        project: project.into(),
        project_id,
        targets,
    })
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// Give unresolved body links real filesystem semantics: a target that spells
/// a relative path resolves against the note file's own directory first and
/// the project root second, matching on-disk files. Resolution is
/// deterministic and never guesses: a target with two on-disk candidates stays
/// pending with a name-your-target absence, a target under a
/// `.no-agent-retrieval` boundary stays pending and withheld, and a target
/// that escapes the project root is never resolved. A resolved target names
/// the file's own source ref when the binding discloses that file, and a
/// stable `central:project-file:` address otherwise.
fn resolve_filesystem_link_targets(
    project_root: &Path,
    note_relative: &Path,
    project_id: &str,
    disclosed: &BTreeMap<PathBuf, ResourceRef>,
    relations: &mut [aikit_core::knowledge_okf::AuthoredRelationEvidence],
    absences: &mut Vec<String>,
) {
    let note_dir = note_relative.parent().unwrap_or(Path::new(""));
    for relation in relations.iter_mut() {
        if relation.channel != AuthoredRelationChannel::Body {
            continue;
        }
        if !matches!(relation.resolution, AuthoredRelationResolution::Unresolved) {
            continue;
        }
        let Some(raw) = filesystem_link_relative(&relation.raw_target) else {
            continue;
        };
        // Relative to the note file's own directory first, the project root
        // second — the two spellings an authored relative link can mean.
        let mut candidates = Vec::new();
        if let Some(relative) = join_project_relative(note_dir, &raw) {
            candidates.push(relative);
        }
        if let Some(relative) = join_project_relative(Path::new(""), &raw) {
            if !candidates.contains(&relative) {
                candidates.push(relative);
            }
        }
        let on_disk: Vec<&PathBuf> = candidates
            .iter()
            .filter(|relative| project_root.join(relative).is_file())
            .collect();
        match on_disk.len() {
            0 => {}
            1 => {
                let relative = on_disk[0].as_path();
                if path_under_retrieval_boundary(project_root, relative) {
                    absences.push(format!(
                        "authored link target {} is withheld under a .no-agent-retrieval boundary; it stays unresolved",
                        relative.display()
                    ));
                    continue;
                }
                let target_ref = disclosed
                    .get(relative)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| project_file_ref(project_id, relative));
                relation.resolution = AuthoredRelationResolution::Resolved { target_ref };
            }
            _ => {
                let spelled = on_disk
                    .iter()
                    .map(|relative| relative.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" or ");
                absences.push(format!(
                    "authored link target is ambiguous between {spelled}; name the intended target to resolve it"
                ));
            }
        }
    }
}

/// Whether a raw link target spells a project-relative filesystem path at all.
/// Ref-like targets (`:`), absolute targets and bare note names are left to
/// the ordinary candidate matching.
fn filesystem_link_relative(raw_target: &str) -> Option<PathBuf> {
    let raw = raw_target.trim().replace('\\', "/");
    if raw.is_empty() || raw.contains(':') || raw.starts_with('/') {
        return None;
    }
    let last_segment = raw.rsplit('/').next().unwrap_or(&raw);
    let path_like = raw.contains('/') || last_segment.contains('.');
    path_like.then(|| PathBuf::from(raw))
}

/// Normalise `base` + `target` into a project-relative path, or `None` when
/// the target escapes the project root.
fn join_project_relative(base: &Path, target: &Path) -> Option<PathBuf> {
    let mut components: Vec<OsString> = Vec::new();
    for component in base.join(target).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => components.push(part.to_os_string()),
            Component::ParentDir => {
                // A `..` above the project root escapes it: never resolved.
                components.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(components.into_iter().collect())
}

/// Whether any directory from the target's parent up to the project root
/// carries the owner's retrieval marker. Mirrors the binding's
/// `path_agent_readable` walk.
fn path_under_retrieval_boundary(project_root: &Path, relative: &Path) -> bool {
    let absolute = project_root.join(relative);
    let mut cursor = absolute.parent();
    while let Some(directory) = cursor {
        if !directory.starts_with(project_root) {
            return true;
        }
        if directory.join(NO_AGENT_RETRIEVAL_MARKER).exists() {
            return true;
        }
        if directory == project_root {
            break;
        }
        cursor = directory.parent();
    }
    false
}

/// Stable address for an on-disk project file the binding does not disclose
/// (a repo README, a contract): FNV-1a over the project-relative path, in the
/// same spirit as the binding's own path-hashed source refs. It names a file
/// by location and carries no standing claim.
fn project_file_ref(project_id: &str, project_relative: &Path) -> ResourceRef {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in project_relative.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    ResourceRef::parse(format!("central:project-file:{project_id}:{hash:016x}"))
        .expect("project file refs are schema-valid by construction")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCentralAuthoredWikiStatus {
    pub version: String,
    pub sources: usize,
    pub resolved_relations: usize,
    pub pending_relations: usize,
    pub living_dependencies: usize,
    pub semantic_wiki_revision: String,
    pub automatic_agent_or_model_invocation: bool,
}

impl ProjectCentralAuthoredWiki {
    pub fn status(&self) -> ProjectCentralAuthoredWikiStatus {
        ProjectCentralAuthoredWikiStatus {
            version: self.version.clone(),
            sources: self.source_projections.len(),
            resolved_relations: self.compilation.edges.len(),
            pending_relations: self.compilation.pending.len(),
            living_dependencies: self.dependencies.len(),
            semantic_wiki_revision: self.index.revision().to_string(),
            automatic_agent_or_model_invocation: self.automatic_agent_or_model_invocation,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use aikit_core::knowledge_wiki::WikiEdgeOrigin;
    use aikit_core::resource::{ResourceRef, SourceAuthority};
    use tempfile::TempDir;

    use super::*;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn fixture() -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("Work/demo");
        write(
            &project.join("ProjectCentral/project.json"),
            r#"{
              "schema":"central.project/v1",
              "project_id":"epilogos/demo",
              "human_source":"ProjectCentral/user",
              "wiki":{
                "profile":"okf-wiki/v1",
                "source":"ProjectCentral/agents/wiki/wiki.json",
                "adopted_sources":[]
              }
            }"#,
        );
        write(
            &project.join("ProjectCentral/relations/source-relations.json"),
            r#"{
              "schema":"central.project.ground-relations/v1",
              "project_id":"epilogos/demo",
              "relations":[{
                "ref":"source:demo:alpha",
                "path":"ProjectCentral/user/alpha.md",
                "provenance":"human-authored",
                "standing":"authored-human-position",
                "roles":["working-note"],
                "treatment":"projectcentral-user",
                "recognition":"human-accepted source relation",
                "recorded_at_unix_seconds":1
              }]
            }"#,
        );
        write(
            &project.join("ProjectCentral/user/alpha.md"),
            "Alpha explicitly links [[Beta]] and [[Future Concept]].\n",
        );
        write(
            &project.join("ProjectCentral/agents/wiki/wiki.json"),
            r#"{
              "profile":"okf-wiki/v1",
              "objects":[{
                "profile":"okf-wiki/v1",
                "object":"node",
                "ref":"wiki:node:beta",
                "revision":1,
                "provenance":[],
                "type":"Concept",
                "title":"Beta",
                "space_refs":[],
                "source_refs":[]
              }]
            }"#,
        );
        (temp, project)
    }

    #[test]
    fn projectcentral_retained_markdown_rebuilds_into_existing_wiki_and_backlinks() {
        let (_temp, project) = fixture();
        let binding = ProjectCentralFilesystemBinding::inspect(&project, None).unwrap();
        let projected = projectcentral_authored_wiki(&binding).unwrap();

        assert_eq!(projected.source_projections.len(), 1);
        assert_eq!(
            projected.source_projections[0].source_authority,
            SourceAuthority::Authored
        );
        assert_eq!(projected.compilation.edges.len(), 1);
        assert_eq!(projected.compilation.pending.len(), 1);
        assert_eq!(projected.dependencies.len(), 1);
        assert!(!projected.automatic_agent_or_model_invocation);

        let beta = ResourceRef::parse("wiki:node:beta").unwrap();
        let backlinks = projected.index.backlinks(&beta);
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].resource.as_str(), "source:demo:alpha");
        assert_eq!(backlinks[0].origin, WikiEdgeOrigin::Authored);
        assert_eq!(
            backlinks[0].provenance[0].extensions["source_authority"],
            serde_json::json!("authored")
        );
        assert_eq!(
            projected.compilation.pending[0].evidence.raw_target,
            "Future Concept"
        );
    }

    #[test]
    fn world_compile_gathers_edges_across_projects_and_discloses_absences_fail_open() {
        let (temp, _project) = fixture();
        // A second Work project with no ProjectCentral at all: an absence,
        // never an abort of the world compile.
        fs::create_dir_all(temp.path().join("Work/bare")).unwrap();

        let reading = compile_world_authored_wiki(temp.path());

        assert_eq!(reading.edges.len(), 1, "{:?}", reading.absences);
        assert_eq!(reading.edges[0].origin, WikiEdgeOrigin::Authored);
        // Pending relations are rolled up per project, not repeated per
        // occurrence: the structured rollup names the target, and no absence
        // string repeats it.
        assert_eq!(reading.pending.len(), 1, "{:?}", reading.absences);
        assert_eq!(reading.pending[0].project, "Work/demo");
        assert_eq!(
            reading.pending[0].project_id.as_deref(),
            Some("epilogos/demo")
        );
        assert_eq!(reading.pending[0].unresolved_targets, 1);
        assert_eq!(reading.pending[0].occurrences, 1);
        assert_eq!(reading.pending[0].targets[0].target, "Future Concept");
        assert!(
            reading
                .absences
                .iter()
                .all(|absence| !absence.contains("Future Concept")),
            "{:?}",
            reading.absences
        );
        assert!(
            reading
                .absences
                .iter()
                .any(|absence| absence.contains("bare")),
            "{:?}",
            reading.absences
        );
        // Each compiled edge carries its project of origin for scoped queries.
        assert_eq!(reading.edge_projects.len(), 1);
        assert_eq!(
            reading.edge_projects.values().next().map(String::as_str),
            Some("Work/demo")
        );
    }

    #[test]
    fn identical_pendings_collapse_into_one_rollup_with_correct_counts() {
        let (temp, project) = fixture();
        write(
            &project.join("ProjectCentral/user/alpha.md"),
            "Alpha cites [[Future Concept]], [[Future Concept]] and [[Future Concept]] once more.\n",
        );

        let reading = compile_world_authored_wiki(temp.path());

        assert_eq!(reading.pending.len(), 1);
        assert_eq!(reading.pending[0].unresolved_targets, 1);
        assert_eq!(reading.pending[0].occurrences, 3);
        assert_eq!(reading.pending[0].targets.len(), 1);
        assert_eq!(reading.pending[0].targets[0].occurrences, 3);
        assert_eq!(
            reading.pending[0].rollup_line(),
            "Work/demo: 1 unresolved target across 3 occurrences pending (per-target detail: knowledge status)"
        );
    }

    #[test]
    fn relative_links_resolve_to_real_files_without_guessing() {
        let (temp, project) = fixture();
        // A repo file outside the ProjectCentral register (note-dir relative:
        // `../../README.md` from ProjectCentral/user/alpha.md).
        write(&project.join("README.md"), "# demo repo\n");
        // A disclosed ProjectCentral file (same directory, project-relative
        // spelling) resolved to its own source ref.
        write(&project.join("ProjectCentral/user/notes.csv"), "id\n1\n");
        write(
            &project.join("ProjectCentral/user/alpha.md"),
            "Repo: [README](../../README.md). Table: [CSV](notes.csv). Escaping: [out](../../../../etc/hosts). Missing: [nope](missing-file.md).\n",
        );

        let reading = compile_world_authored_wiki(temp.path());

        // The repo README resolves to a stable project-file address; the CSV
        // resolves to the disclosed file's own source ref; the escaping and
        // missing targets stay pending.
        assert_eq!(reading.pending.len(), 1, "{:?}", reading.absences);
        let targets = &reading.pending[0].targets;
        assert_eq!(targets.len(), 2, "{:?}", targets);
        assert!(targets
            .iter()
            .any(|target| target.target == "missing-file.md"));
        assert!(targets
            .iter()
            .any(|target| target.target.contains("etc/hosts")));
        let readme_edges = reading
            .edges
            .iter()
            .filter(|edge| {
                edge.to_ref
                    .as_str()
                    .starts_with("central:project-file:epilogos/demo:")
            })
            .count();
        assert_eq!(readme_edges, 1, "{:?}", reading.absences);
        let csv_edges = reading
            .edges
            .iter()
            .filter(|edge| {
                edge.to_ref
                    .as_str()
                    .starts_with("central:project-source:epilogos/demo:")
            })
            .count();
        assert_eq!(csv_edges, 1, "{:?}", reading.absences);
    }

    #[test]
    fn ambiguous_link_targets_stay_pending_with_a_name_your_target_absence() {
        let (temp, project) = fixture();
        // `shared.md` exists both beside the note and at the project root:
        // two on-disk candidates, never a guess.
        write(
            &project.join("ProjectCentral/user/shared.md"),
            "beside the note\n",
        );
        write(&project.join("shared.md"), "at the project root\n");
        write(
            &project.join("ProjectCentral/user/alpha.md"),
            "Ambiguous: [shared](shared.md).\n",
        );

        let reading = compile_world_authored_wiki(temp.path());

        assert_eq!(reading.pending.len(), 1);
        assert_eq!(reading.pending[0].targets[0].target, "shared.md");
        assert!(
            reading.absences.iter().any(|absence| {
                absence.contains("ambiguous") && absence.contains("name the intended target")
            }),
            "{:?}",
            reading.absences
        );
    }

    #[test]
    fn retrieval_marked_link_targets_stay_unresolved_and_withheld() {
        let (temp, project) = fixture();
        write(
            &project.join("ProjectCentral/user/private/secret.md"),
            "private\n",
        );
        write(
            &project.join("ProjectCentral/user/private/.no-agent-retrieval"),
            "",
        );
        write(
            &project.join("ProjectCentral/user/alpha.md"),
            "Private: [secret](private/secret.md). Public: [[Beta]].\n",
        );

        let reading = compile_world_authored_wiki(temp.path());

        // The marked target never resolves; the marker's existence is
        // disclosed, its contents are not.
        assert_eq!(reading.pending.len(), 1);
        assert!(
            reading.absences.iter().any(|absence| {
                absence.contains("withheld under a .no-agent-retrieval boundary")
            }),
            "{:?}",
            reading.absences
        );
        assert!(
            !reading
                .edges
                .iter()
                .any(|edge| edge.to_ref.as_str().contains("secret")),
            "{:?}",
            reading.edges
        );
    }

    #[test]
    fn world_compile_discloses_an_oversized_source_without_aborting_the_world() {
        let (temp, project) = fixture();
        write(
            &project.join("ProjectCentral/user/oversized.md"),
            &"x".repeat(AUTHORED_WIKI_MAX_SOURCE_BYTES as usize + 1),
        );

        let reading = compile_world_authored_wiki(temp.path());

        assert_eq!(reading.edges.len(), 1, "{:?}", reading.absences);
        assert!(
            reading
                .absences
                .iter()
                .any(|absence| absence.contains("oversized.md")
                    && absence.contains("bounded read size")),
            "{:?}",
            reading.absences
        );
    }

    #[test]
    fn source_revision_change_rebuilds_relation_revision_without_model_work() {
        let (_temp, project) = fixture();
        let first_binding = ProjectCentralFilesystemBinding::inspect(&project, None).unwrap();
        let first = projectcentral_authored_wiki(&first_binding).unwrap();
        let first_edge_ref = first.compilation.edges[0].ref_id.clone();
        let first_edge_revision = first.compilation.edges[0].revision;

        write(
            &project.join("ProjectCentral/user/alpha.md"),
            "Alpha explicitly links [[Beta]].\nA second sentence changes the source revision.\n",
        );
        let second_binding = ProjectCentralFilesystemBinding::inspect(&project, None).unwrap();
        let second = projectcentral_authored_wiki(&second_binding).unwrap();

        assert_eq!(first_edge_ref, second.compilation.edges[0].ref_id);
        assert_ne!(first_edge_revision, second.compilation.edges[0].revision);
        assert!(!second.automatic_agent_or_model_invocation);
        assert_eq!(second.status().resolved_relations, 1);
    }
}
