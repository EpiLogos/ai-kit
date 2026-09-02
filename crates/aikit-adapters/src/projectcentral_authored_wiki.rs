//! ProjectCentral → authored SemanticWiki relation binding.
//!
//! Central remains the source owner: `ProjectCentralFilesystemBinding` supplies
//! stable SourceRef, path, revision and epistemic standing. AIKit reads only
//! eligible retained Markdown files already disclosed by that binding and compiles
//! their explicit links/OKF Properties into the existing SemanticWiki relation
//! field. No source migration or second Wiki store is introduced.

use std::fs;
use std::path::Path;

use aikit_core::knowledge_living::KnowledgeDependency;
use aikit_core::knowledge_wiki::WikiObject;
use aikit_core::knowledge_wiki_index::SemanticWikiIndex;
use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};

use crate::authored_wiki_source::{
    authored_relation_dependencies, compile_authored_wiki_relations,
    parse_authored_wiki_source_with_authority, rebuild_semantic_wiki_with_authored_relations,
    AuthoredWikiRelationCompilation, AuthoredWikiSourceProjection,
};
use crate::ProjectCentralFilesystemBinding;

pub const PROJECTCENTRAL_AUTHORED_WIKI_VERSION: &str = "aikit.projectcentral-authored-wiki/v1";

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

    for descriptor in &binding.semantic.sources {
        if !descriptor.exists
            || !descriptor.agent_readable
            || descriptor.is_directory
            || !is_markdown_path(&descriptor.relative_path)
        {
            continue;
        }

        let path = binding.project_root().join(&descriptor.relative_path);
        let markdown = fs::read_to_string(&path).map_err(|error| {
            AikitError::new(
                "projectcentral.authored_wiki_source_read",
                format!("{}: {error}", path.display()),
            )
        })?;
        let subject_ref = ResourceRef::parse(descriptor.source.as_str())?;
        let authority = descriptor
            .standing
            .source_authority()
            .unwrap_or(SourceAuthority::Observed);
        source_projections.push(parse_authored_wiki_source_with_authority(
            subject_ref,
            descriptor.source.clone(),
            authority,
            descriptor.revision.clone(),
            vec![descriptor.relative_path.to_string_lossy().into_owned()],
            &markdown,
        )?);
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
    })
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
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
