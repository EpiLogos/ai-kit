//! Folder subjects: a project's ProjectCentral register compiles as the
//! directory BASIS of the materialised knowledge graph — folders become
//! `project-dir` WikiNode subjects under the project's `project-root` anchor,
//! wired by Compiled `parent`/`contains` edges. Files stay file-level: a
//! folder's `contains` edges name only the file-level subjects that already
//! exist in the same materialised set.
//!
//! This is a materialisation-time construct and nothing else: the compiler
//! reads Central's disclosed ProjectCentral binding and emits wiki objects
//! for the per-context index. It never writes Central's protected wiki
//! files. `.no-agent-retrieval` prunes a subtree before any folder node is
//! compiled for it, mirroring the binding's own scan discipline
//! (`ProjectCentralFilesystemBinding`), and symlinks are never followed.
//!
//! Identity is stable across rebuilds: a folder node's ref derives from the
//! project's anchor slug and the project-relative directory path, never from
//! content or revision. Provenance names the compiler and the project's
//! native root source — the same discipline as the compiled entities.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use aikit_core::knowledge_wiki::project_wiki_space_ref;
use aikit_core::{
    ResourceRef, SourceRef, WikiEdge, WikiEdgeOrigin, WikiNode, WikiObject, WikiProvenanceRef,
    NO_AGENT_RETRIEVAL_MARKER, OKF_WIKI_PROFILE,
};
use serde_json::json;

use crate::ProjectCentralFilesystemBinding;

pub const FOLDER_SUBJECT_PRODUCER_REF: &str = "aikit/folder-subject-compiler/v1";
pub const FOLDER_SUBJECT_EXTENSION: &str = "aikit.project-dir/v1";
/// The wiki node type every compiled folder subject carries.
pub const PROJECT_DIR_NODE_TYPE: &str = "project-dir";
/// The folder → project anchor (or folder → parent folder) relation.
pub const PARENT_RELATION: &str = "parent";
/// The folder → immediate file-level subject relation.
pub const CONTAINS_RELATION: &str = "contains";

/// A folder subject is compiled for every directory from the register root
/// (depth 0, `ProjectCentral` itself) down to this depth; deeper subtrees are
/// disclosed as skipped rather than compiled.
pub const MAX_FOLDER_SUBJECT_DEPTH: usize = 4;
/// Folder subjects compiled per project before the walk stops, one absence
/// disclosing the truncation — the `MAX_DISCOVERY_*` discipline.
pub const MAX_FOLDER_SUBJECTS: usize = 256;

pub struct FolderSubjectReading {
    pub objects: Vec<WikiObject>,
    pub absences: Vec<String>,
    /// Every compiled ref (folder nodes and their edges) → the Work-relative
    /// project it belongs to. Scoped knowledge queries use it to keep another
    /// project's folder basis out of their results — the same attribution
    /// discipline as the authored wiki's `edge_projects`.
    pub subject_projects: BTreeMap<String, String>,
}

/// Compile folder subjects for every Work project (or only `only_project`'s,
/// the shape-derived current project) under `central_root`. Infallible: a
/// project whose ProjectCentral binding cannot be inspected, whose anchor is
/// absent, or that hits a bound is disclosed in `absences` and skipped, never
/// aborting the world.
pub fn compile_world_folder_subjects(
    central_root: &Path,
    materialised_refs: &BTreeSet<String>,
    only_project: Option<&str>,
) -> FolderSubjectReading {
    let mut objects = Vec::new();
    let mut absences = Vec::new();
    let mut subject_projects = BTreeMap::new();

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
        let Some(project) = project_root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
        else {
            continue;
        };
        if let Some(only) = only_project {
            if project != only {
                continue;
            }
        }
        let home = format!("Work/{project}");
        let binding =
            match ProjectCentralFilesystemBinding::inspect(&project_root, Some(central_root)) {
                Ok(binding) => binding,
                Err(error) => {
                    absences.push(format!(
                        "ProjectCentral folder basis unavailable at {home}: {}",
                        error.message()
                    ));
                    continue;
                }
            };
        let compiled = project_folder_subjects(&binding, &project, &home, materialised_refs);
        absences.extend(compiled.absences);
        objects.extend(compiled.objects);
        subject_projects.extend(compiled.subject_projects);
    }

    FolderSubjectReading {
        objects,
        absences,
        subject_projects,
    }
}

/// Compile one project's folder subjects: nodes for each directory of its
/// ProjectCentral register under the project-root anchor, `parent` edges
/// wiring the tree to that anchor, and `contains` edges to the project's
/// immediate disclosed file-level subjects that exist in `materialised_refs`.
pub fn project_folder_subjects(
    binding: &ProjectCentralFilesystemBinding,
    project: &str,
    project_display: &str,
    materialised_refs: &BTreeSet<String>,
) -> FolderSubjectReading {
    let declined = |absences: Vec<String>| FolderSubjectReading {
        objects: Vec::new(),
        absences,
        subject_projects: BTreeMap::new(),
    };

    // The anchor is the project's own authored ground: it exists when the
    // project is initialised (`aikit wiki root-anchor`). Without it the
    // folder basis has nothing to hang from, and a dangling parent edge is
    // never compiled instead.
    let anchor_ref = project_root_anchor_ref(project);
    let wiki_objects = match binding.load_project_wiki() {
        Ok(objects) => objects,
        Err(error) => {
            return declined(vec![format!(
                "ProjectCentral folder basis unavailable at {project_display}: {}",
                error.message()
            )]);
        }
    };
    if !wiki_objects
        .iter()
        .any(|object| object.ref_id().as_str() == anchor_ref)
    {
        return declined(vec![format!(
            "project-root anchor {anchor_ref} is absent from {project_display}'s canonical wiki; no folder subjects compiled"
        )]);
    }

    let space_ref = match project_wiki_space_ref(&binding.semantic.project_id) {
        Ok(space) => space,
        Err(error) => {
            return declined(vec![format!(
                "ProjectCentral folder basis unavailable at {project_display}: {}",
                error.message()
            )]);
        }
    };

    let register = binding.project_root().join("ProjectCentral");
    let metadata = match fs::metadata(&register) {
        Ok(metadata) => metadata,
        Err(error) => {
            return declined(vec![format!(
                "ProjectCentral folder basis unavailable at {project_display}: {error}"
            )]);
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return declined(Vec::new());
    }

    let mut walk = Walk {
        slug: project_slug(project),
        native_root: binding.semantic.native_project_root.clone(),
        space_ref,
        nodes: Vec::new(),
        edges: Vec::new(),
        folder_refs: BTreeMap::new(),
        absences: Vec::new(),
        materialised_refs,
        skipped_depth: 0,
        bound_reached: false,
    };
    visit_directory(
        &mut walk,
        &register,
        Path::new("ProjectCentral"),
        0,
        Some(anchor_ref.as_str()),
    );
    if walk.bound_reached {
        walk.absences.push(format!(
            "folder subject bound of {MAX_FOLDER_SUBJECTS} reached for {project_display}; deeper structure is not compiled"
        ));
    }
    if walk.skipped_depth > 0 {
        walk.absences.push(format!(
            "{} folder subtree(s) beyond depth {MAX_FOLDER_SUBJECT_DEPTH} not compiled for {project_display}",
            walk.skipped_depth
        ));
    }

    // `contains` edges: one per immediate disclosed file-level subject that
    // exists in the same materialised set. Files stay file-level — the edge
    // names the file's own source ref, never a second subject.
    let mut contains = Vec::new();
    for descriptor in &binding.semantic.sources {
        if !descriptor.exists || !descriptor.agent_readable || descriptor.is_directory {
            continue;
        }
        let Some(parent) = descriptor.relative_path.parent() else {
            continue;
        };
        let Some(folder_ref) = walk.folder_refs.get(parent) else {
            continue;
        };
        if !materialised_refs.contains(descriptor.source.as_str()) {
            continue;
        }
        let edge_ref = contains_edge_ref(folder_ref, descriptor.source.as_str());
        if materialised_refs.contains(edge_ref.as_str()) {
            walk.absences.push(format!(
                "contains edge {edge_ref} already exists in the materialised wiki; not re-declared"
            ));
            continue;
        }
        contains.push(WikiObject::Edge(WikiEdge {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: resource_ref(&edge_ref),
            revision: 1,
            provenance: folder_provenance(&walk.native_root),
            from_ref: resource_ref(folder_ref),
            to_ref: resource_ref(descriptor.source.as_str()),
            relation: CONTAINS_RELATION.into(),
            origin: WikiEdgeOrigin::Compiled,
            origin_ref: Some(resource_ref(FOLDER_SUBJECT_PRODUCER_REF)),
            extensions: BTreeMap::new(),
        }));
    }

    let mut objects = Vec::with_capacity(walk.nodes.len() + walk.edges.len() + contains.len());
    objects.extend(walk.nodes.into_iter().map(WikiObject::Node));
    objects.extend(walk.edges.into_iter().map(WikiObject::Edge));
    objects.extend(contains);

    let subject_projects = objects
        .iter()
        .map(|object| {
            (
                object.ref_id().as_str().to_owned(),
                project_display.to_owned(),
            )
        })
        .collect();

    FolderSubjectReading {
        objects,
        absences: walk.absences,
        subject_projects,
    }
}

/// `wiki:node:project-root/<slug>` — the anchor ref `aikit wiki root-anchor`
/// writes for a project (the slug of the Work directory name).
pub fn project_root_anchor_ref(project: &str) -> String {
    format!("wiki:node:project-root/{}", project_slug(project))
}

/// `wiki:node:project-dir/<project-slug>/<project-relative path>` — stable,
/// path-derived, never content-derived.
fn folder_subject_ref(slug: &str, relative: &Path) -> String {
    let path = relative.to_string_lossy().replace('\\', "/");
    format!("wiki:node:project-dir/{slug}/{path}")
}

/// Deterministic edge ref: endpoints + relation, never content, never the
/// order of discovery (the compiled entities' discipline).
fn contains_edge_ref(from: &str, to: &str) -> String {
    format!("wiki:edge:{from}->{to}:{CONTAINS_RELATION}")
}

fn parent_edge_ref(from: &str, to: &str) -> String {
    format!("wiki:edge:{from}->{to}:{PARENT_RELATION}")
}

/// The anchor slug convention: lowercase, every non-alphanumeric character a
/// hyphen (wiki.rs's `root_anchor`).
fn project_slug(project: &str) -> String {
    project
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn resource_ref(value: &str) -> ResourceRef {
    ResourceRef::parse(value).expect("folder subject refs are valid resource refs")
}

fn folder_provenance(native_root: &SourceRef) -> Vec<WikiProvenanceRef> {
    vec![WikiProvenanceRef {
        source_ref: native_root.clone(),
        source_revision: None,
        producer_ref: Some(resource_ref(FOLDER_SUBJECT_PRODUCER_REF)),
        generation_ref: None,
        extensions: BTreeMap::new(),
    }]
}

struct Walk<'a> {
    slug: String,
    native_root: SourceRef,
    space_ref: ResourceRef,
    nodes: Vec<WikiNode>,
    edges: Vec<WikiEdge>,
    /// Project-relative directory path → its compiled node ref, for parent
    /// edges and `contains` wiring.
    folder_refs: BTreeMap<PathBuf, String>,
    absences: Vec<String>,
    materialised_refs: &'a BTreeSet<String>,
    skipped_depth: usize,
    bound_reached: bool,
}

/// Depth-first register walk. The retrieval marker prunes a whole subtree
/// before anything is compiled for it — the folder itself never becomes a
/// subject — and a directory's presence, not its contents, is all a folder
/// node ever discloses.
fn visit_directory(
    walk: &mut Walk,
    absolute: &Path,
    relative: &Path,
    depth: usize,
    parent_ref: Option<&str>,
) {
    if absolute.join(NO_AGENT_RETRIEVAL_MARKER).exists() {
        return;
    }
    if depth > MAX_FOLDER_SUBJECT_DEPTH {
        walk.skipped_depth += 1;
        return;
    }
    if walk.nodes.len() >= MAX_FOLDER_SUBJECTS {
        walk.bound_reached = true;
        return;
    }
    let reference = folder_subject_ref(&walk.slug, relative);
    if walk.materialised_refs.contains(reference.as_str()) {
        walk.absences.push(format!(
            "folder subject {reference} already exists in the materialised wiki; kept the standing object"
        ));
        return;
    }

    if let Some(parent) = parent_ref {
        let edge_ref = parent_edge_ref(&reference, parent);
        if !walk.materialised_refs.contains(edge_ref.as_str()) {
            walk.edges.push(WikiEdge {
                profile: OKF_WIKI_PROFILE.into(),
                ref_id: resource_ref(&edge_ref),
                revision: 1,
                provenance: Vec::new(),
                from_ref: resource_ref(&reference),
                to_ref: resource_ref(parent),
                relation: PARENT_RELATION.into(),
                origin: WikiEdgeOrigin::Compiled,
                origin_ref: Some(resource_ref(FOLDER_SUBJECT_PRODUCER_REF)),
                extensions: BTreeMap::new(),
            });
        }
    }
    walk.nodes.push(WikiNode {
        profile: OKF_WIKI_PROFILE.into(),
        ref_id: resource_ref(&reference),
        revision: 1,
        // Provenance is exact: the folder subject is compiled from the
        // project's native root directory basis by this compiler, and no
        // authored revision is claimed for a directory.
        provenance: folder_provenance(&walk.native_root),
        node_type: PROJECT_DIR_NODE_TYPE.into(),
        title: Some(relative.to_string_lossy().replace('\\', "/")),
        space_refs: vec![walk.space_ref.clone()],
        source_refs: vec![walk.native_root.clone()],
        local_space_ref: None,
        extensions: BTreeMap::from([(
            FOLDER_SUBJECT_EXTENSION.to_owned(),
            json!({
                "path": relative.to_string_lossy().replace('\\', "/"),
                "depth": depth,
            }),
        )]),
    });
    walk.folder_refs
        .insert(relative.to_path_buf(), reference.clone());

    let mut entries = match fs::read_dir(absolute) {
        Ok(entries) => entries
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap_or_default(),
        Err(_) => return,
    };
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        let child = entry.path();
        let child_relative = relative.join(entry.file_name());
        visit_directory(
            walk,
            &child,
            &child_relative,
            depth + 1,
            Some(reference.as_str()),
        );
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn manifest(project_id: &str) -> String {
        format!(
            r#"{{
              "schema":"central.project/v1",
              "project_id":"{project_id}",
              "human_source":"ProjectCentral/user",
              "wiki":{{
                "profile":"okf-wiki/v1",
                "source":"ProjectCentral/agents/wiki/wiki.json",
                "adopted_sources":[]
              }}
            }}"#
        )
    }

    fn empty_relations(project_id: &str) -> String {
        format!(
            r#"{{"schema":"central.project.ground-relations/v1","project_id":"{project_id}","relations":[]}}"#
        )
    }

    /// A minimal canonical wiki for a project: its space, its project-root
    /// anchor, and one content node. `anchor` is `None` for the
    /// anchor-absent fixture.
    fn wiki_json(project_id: &str, anchor: Option<&str>) -> String {
        let slug = project_slug(anchor.map_or("demo", |name| {
            name.strip_prefix("wiki:node:project-root/").unwrap_or(name)
        }));
        let space = format!("central:wiki:project:{project_id}");
        let anchor_obj = anchor
            .map(|anchor_ref| {
                format!(
                    r#",{{"profile":"okf-wiki/v1","object":"node","ref":"{anchor_ref}","revision":1,"provenance":[],"type":"project-root","title":"{slug}","space_refs":["{space}"],"source_refs":["ProjectCentral/project.json"]}}"#
                )
            })
            .unwrap_or_default();
        format!(
            r#"{{
              "profile":"okf-wiki/v1",
              "objects":[
                {{"profile":"okf-wiki/v1","object":"space","ref":"{space}","revision":1,"provenance":[],"title":"{slug}","parent_space_refs":[],"child_space_refs":[],"node_refs":["wiki:node:beta"],"anchor_ref":null}},
                {{"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:beta","revision":1,"provenance":[],"type":"Concept","title":"Beta","space_refs":[],"source_refs":[]}}
                {anchor_obj}
              ]
            }}"#
        )
    }

    /// One project's register: manifest, an accepted ground relation for
    /// `ProjectCentral/user/alpha.md` (`source:demo:alpha`), a second
    /// disclosed file with its own relation, and the canonical wiki.
    fn write_demo_project(project: &Path) {
        write(
            &project.join("ProjectCentral/project.json"),
            &manifest("epilogos/demo"),
        );
        write(
            &project.join("ProjectCentral/relations/source-relations.json"),
            r#"{
              "schema":"central.project.ground-relations/v1",
              "project_id":"epilogos/demo",
              "relations":[
                {
                  "ref":"source:demo:alpha",
                  "path":"ProjectCentral/user/alpha.md",
                  "provenance":"human-authored",
                  "standing":"authored-human-position",
                  "roles":["working-note"],
                  "treatment":"projectcentral-user",
                  "recognition":"human-accepted source relation",
                  "recorded_at_unix_seconds":1
                },
                {
                  "ref":"source:demo:notes",
                  "path":"ProjectCentral/user/notes/tables.md",
                  "provenance":"human-authored",
                  "standing":"authored-human-position",
                  "roles":["working-note"],
                  "treatment":"projectcentral-user",
                  "recognition":"human-accepted source relation",
                  "recorded_at_unix_seconds":1
                }
              ]
            }"#,
        );
        write(
            &project.join("ProjectCentral/user/alpha.md"),
            "Alpha explicitly links [[Beta]].\n",
        );
        write(
            &project.join("ProjectCentral/user/notes/tables.md"),
            "Tables link [[Beta]] too.\n",
        );
        write(
            &project.join("ProjectCentral/agents/wiki/wiki.json"),
            &wiki_json("epilogos/demo", Some("wiki:node:project-root/demo")),
        );
    }

    fn fixture() -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("Work/demo");
        write_demo_project(&project);
        (temp, project)
    }

    fn binding_of(project: &Path) -> ProjectCentralFilesystemBinding {
        ProjectCentralFilesystemBinding::inspect(project, None).unwrap()
    }

    /// The refs a materialised set would carry: the project's own wiki
    /// objects plus the disclosed file subjects this fixture admits.
    fn materialised_refs(
        binding: &ProjectCentralFilesystemBinding,
        files: &[&str],
    ) -> BTreeSet<String> {
        let mut refs: BTreeSet<String> = binding
            .load_project_wiki()
            .unwrap()
            .iter()
            .map(|object| object.ref_id().as_str().to_owned())
            .collect();
        refs.extend(files.iter().map(|file| file.to_string()));
        refs
    }

    fn compile_with(files: &[&str]) -> FolderSubjectReading {
        let (_temp, project) = fixture();
        let binding = binding_of(&project);
        let refs = materialised_refs(&binding, files);
        project_folder_subjects(&binding, "demo", "Work/demo", &refs)
    }

    fn node_refs(objects: &[WikiObject]) -> BTreeSet<String> {
        objects
            .iter()
            .filter_map(|object| match object {
                WikiObject::Node(node) => Some(node.ref_id.as_str().to_owned()),
                _ => None,
            })
            .collect()
    }

    fn edge_ends(objects: &[WikiObject], relation: &str) -> Vec<(String, String)> {
        objects
            .iter()
            .filter_map(|object| match object {
                WikiObject::Edge(edge) if edge.relation == relation => Some((
                    edge.from_ref.as_str().to_owned(),
                    edge.to_ref.as_str().to_owned(),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn folder_subjects_compile_under_the_project_anchor_and_contains_wires_disclosed_files() {
        let reading = compile_with(&["source:demo:alpha"]);

        assert!(reading.absences.is_empty(), "{:?}", reading.absences);
        let nodes = node_refs(&reading.objects);
        assert!(nodes.contains("wiki:node:project-dir/demo/ProjectCentral"));
        assert!(nodes.contains("wiki:node:project-dir/demo/ProjectCentral/user"));
        assert!(nodes.contains("wiki:node:project-dir/demo/ProjectCentral/agents"));
        assert!(nodes.contains("wiki:node:project-dir/demo/ProjectCentral/relations"));

        let parents = edge_ends(&reading.objects, PARENT_RELATION);
        assert!(parents.contains(&(
            "wiki:node:project-dir/demo/ProjectCentral".to_owned(),
            "wiki:node:project-root/demo".to_owned()
        )));
        assert!(parents.contains(&(
            "wiki:node:project-dir/demo/ProjectCentral/user".to_owned(),
            "wiki:node:project-dir/demo/ProjectCentral".to_owned()
        )));

        // Only the disclosed file subject admitted into the materialised set
        // is wired; `source:demo:notes` was not admitted and gets no edge.
        let contains = edge_ends(&reading.objects, CONTAINS_RELATION);
        assert!(contains.contains(&(
            "wiki:node:project-dir/demo/ProjectCentral/user".to_owned(),
            "source:demo:alpha".to_owned()
        )));
        assert!(!contains.iter().any(|(_, to)| to == "source:demo:notes"));

        // Node discipline: type, Compiled provenance naming the compiler,
        // the project space, the native root source, and the path extension.
        let user = reading
            .objects
            .iter()
            .find_map(|object| match object {
                WikiObject::Node(node)
                    if node.ref_id.as_str() == "wiki:node:project-dir/demo/ProjectCentral/user" =>
                {
                    Some(node)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(user.node_type, PROJECT_DIR_NODE_TYPE);
        assert_eq!(
            user.space_refs[0].as_str(),
            "central:wiki:project:epilogos/demo"
        );
        assert_eq!(
            user.source_refs[0].as_str(),
            "source:project:epilogos/demo:root"
        );
        assert_eq!(
            user.provenance[0].producer_ref.as_ref().unwrap().as_str(),
            FOLDER_SUBJECT_PRODUCER_REF
        );
        assert_eq!(
            user.extensions[FOLDER_SUBJECT_EXTENSION]["path"],
            json!("ProjectCentral/user")
        );
        assert_eq!(user.extensions[FOLDER_SUBJECT_EXTENSION]["depth"], json!(1));

        // Every compiled ref attributes itself to the project.
        assert!(reading
            .subject_projects
            .values()
            .all(|project| project == "Work/demo"));
        assert_eq!(reading.objects.len(), reading.subject_projects.len());
    }

    #[test]
    fn refs_are_stable_across_rematerialisation() {
        let first = compile_with(&["source:demo:alpha"]);
        let second = compile_with(&["source:demo:alpha"]);

        assert_eq!(node_refs(&first.objects), node_refs(&second.objects));
        assert_eq!(
            edge_ends(&first.objects, PARENT_RELATION),
            edge_ends(&second.objects, PARENT_RELATION)
        );
        assert_eq!(
            edge_ends(&first.objects, CONTAINS_RELATION),
            edge_ends(&second.objects, CONTAINS_RELATION)
        );
    }

    #[test]
    fn retrieval_marked_subtrees_get_no_folder_subjects() {
        let (_temp, project) = fixture();
        write(
            &project.join("ProjectCentral/user/private/secret.md"),
            "s\n",
        );
        write(
            &project.join("ProjectCentral/user/private/.no-agent-retrieval"),
            "",
        );
        let binding = binding_of(&project);
        let refs = materialised_refs(&binding, &["source:demo:alpha"]);
        let reading = project_folder_subjects(&binding, "demo", "Work/demo", &refs);

        assert!(reading.absences.is_empty(), "{:?}", reading.absences);
        assert!(
            reading
                .objects
                .iter()
                .all(|object| !object.ref_id().as_str().contains("private")),
            "{:?}",
            reading.subject_projects.keys()
        );
    }

    #[test]
    fn an_absent_project_root_anchor_compiles_no_folder_subjects() {
        let (temp, project) = fixture();
        write(
            &project.join("ProjectCentral/agents/wiki/wiki.json"),
            &wiki_json("epilogos/demo", None),
        );
        let binding = binding_of(&project);
        let refs = materialised_refs(&binding, &["source:demo:alpha"]);
        let reading = project_folder_subjects(&binding, "demo", "Work/demo", &refs);

        assert!(reading.objects.is_empty());
        assert!(reading.subject_projects.is_empty());
        assert!(
            reading
                .absences
                .iter()
                .any(|absence| absence.contains("wiki:node:project-root/demo")
                    && absence.contains("no folder subjects compiled")),
            "{:?}",
            reading.absences
        );
        // The temp root is only kept alive for the fixture's lifetime.
        drop(temp);
    }

    #[test]
    fn only_the_current_project_compiles_in_a_scoped_world() {
        let temp = TempDir::new().unwrap();
        write_demo_project(&temp.path().join("Work/demo"));
        let other = temp.path().join("Work/other");
        write(
            &other.join("ProjectCentral/project.json"),
            &manifest("epilogos/other"),
        );
        write(
            &other.join("ProjectCentral/relations/source-relations.json"),
            &empty_relations("epilogos/other"),
        );
        write(
            &other.join("ProjectCentral/agents/wiki/wiki.json"),
            &wiki_json("epilogos/other", Some("wiki:node:project-root/other")),
        );

        let refs = materialised_refs(&binding_of(&temp.path().join("Work/demo")), &[]);
        let scoped = compile_world_folder_subjects(temp.path(), &refs, Some("demo"));
        assert!(
            scoped
                .subject_projects
                .values()
                .all(|project| project == "Work/demo"),
            "{:?}",
            scoped.subject_projects
        );
        assert!(
            scoped
                .subject_projects
                .keys()
                .all(|reference| reference.contains("project-dir/demo/")),
            "{:?}",
            scoped.subject_projects.keys()
        );

        let world = compile_world_folder_subjects(temp.path(), &refs, None);
        let projects: BTreeSet<String> = world.subject_projects.values().cloned().collect();
        assert_eq!(
            projects,
            BTreeSet::from(["Work/demo".to_owned(), "Work/other".to_owned()])
        );
    }

    #[test]
    fn depth_and_count_bounds_stop_the_walk_and_are_disclosed() {
        let (_temp, project) = fixture();
        // A chain deeper than the bound: ProjectCentral/a/b/c/d/e — `e` sits
        // at depth 5 and is skipped with the rest of its subtree.
        write(&project.join("ProjectCentral/a/b/c/d/e/leaf.md"), "deep\n");
        let binding = binding_of(&project);
        let reading = project_folder_subjects(&binding, "demo", "Work/demo", &BTreeSet::new());
        assert!(!node_refs(&reading.objects)
            .contains("wiki:node:project-dir/demo/ProjectCentral/a/b/c/d/e"));
        assert!(
            reading
                .absences
                .iter()
                .any(|absence| absence.contains("beyond depth 4")),
            "{:?}",
            reading.absences
        );

        // More directories than the bound: the walk stops at the bound with
        // one disclosing absence, and no more than the bound is ever handed
        // to the index (duplicates are fatal there).
        for index in 0..(MAX_FOLDER_SUBJECTS + 8) {
            fs::create_dir_all(
                project
                    .join("ProjectCentral/many")
                    .join(format!("d{index:04}")),
            )
            .unwrap();
        }
        let binding = binding_of(&project);
        let reading = project_folder_subjects(&binding, "demo", "Work/demo", &BTreeSet::new());
        assert_eq!(node_refs(&reading.objects).len(), MAX_FOLDER_SUBJECTS);
        assert!(
            reading
                .absences
                .iter()
                .any(|absence| absence.contains(&format!("bound of {MAX_FOLDER_SUBJECTS}"))),
            "{:?}",
            reading.absences
        );
    }

    #[test]
    fn a_ref_collision_keeps_the_standing_object_and_discloses() {
        let (_temp, project) = fixture();
        let binding = binding_of(&project);
        let mut refs = materialised_refs(&binding, &["source:demo:alpha"]);
        refs.insert("wiki:node:project-dir/demo/ProjectCentral".to_owned());
        let reading = project_folder_subjects(&binding, "demo", "Work/demo", &refs);

        // The colliding root folder — and its subtree below it, which would
        // dangle — is not re-declared: the materialised wiki is never handed
        // a duplicate ref.
        assert!(!node_refs(&reading.objects).contains("wiki:node:project-dir/demo/ProjectCentral"));
        assert!(
            reading
                .absences
                .iter()
                .any(|absence| absence.contains("wiki:node:project-dir/demo/ProjectCentral")),
            "{:?}",
            reading.absences
        );
    }

    #[test]
    fn anchor_ref_matches_the_root_anchor_convention() {
        assert_eq!(
            project_root_anchor_ref("ai-kit"),
            "wiki:node:project-root/ai-kit"
        );
        assert_eq!(
            project_root_anchor_ref("Point-Cloud-Demo"),
            "wiki:node:project-root/point-cloud-demo"
        );
    }
}
