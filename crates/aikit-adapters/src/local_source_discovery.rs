//! Bounded discovery of native Project source that may carry local articulation.
//!
//! This adapter deliberately does not impose a repository layout. It samples
//! likely source/contract material from the existing Project, preserves any
//! owner-issued/adopted relation supplied by the caller, and delegates semantic
//! role classification to `aikit-core::project_reflection`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use aikit_core::{
    classify_local_source, AikitError, LocalSourceCandidate, LocalSourceClassification,
    LocalSourceRole, ProjectRef, Result, SourceAuthority, SourceRef,
};
use serde::{Deserialize, Serialize};

pub const LOCAL_SOURCE_DISCOVERY_VERSION: &str = "aikit.local-source-discovery/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSourceDiscoveryLimits {
    pub max_files_visited: usize,
    pub max_depth: usize,
    pub max_body_bytes: usize,
}

impl Default for LocalSourceDiscoveryLimits {
    fn default() -> Self {
        Self {
            max_files_visited: 512,
            max_depth: 10,
            max_body_bytes: 8 * 1024,
        }
    }
}

/// Owner/adoption evidence for a native source which should retain its existing
/// location rather than being copied into ProjectCentral for AIKit's convenience.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSourceRelation {
    pub relative_path: PathBuf,
    pub source: SourceRef,
    pub role: LocalSourceRole,
    pub authority: SourceAuthority,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredLocalSource {
    pub relative_path: PathBuf,
    pub candidate: LocalSourceCandidate,
    pub classification: LocalSourceClassification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSourceDiscovery {
    pub version: String,
    pub project: ProjectRef,
    pub files_visited: usize,
    pub truncated: bool,
    pub sources: Vec<DiscoveredLocalSource>,
}

/// Discover useful existing source without recursively ingesting a Project.
///
/// Filenames/path conventions affect *candidate discovery* only. `AGENTS.md`,
/// `CONTEXT.md`, README/ADR names, etc. do not establish authorship or semantic
/// role. Exact owner/adoption relations supplied in `relations` win because they
/// carry source identity, role and authority from the owning system.
pub fn discover_local_sources(
    project: ProjectRef,
    project_root: impl AsRef<Path>,
    relations: &[NativeSourceRelation],
    limits: LocalSourceDiscoveryLimits,
) -> Result<LocalSourceDiscovery> {
    if limits.max_files_visited == 0 || limits.max_body_bytes == 0 {
        return Err(AikitError::new(
            "local_source_discovery.invalid_limit",
            "native source discovery requires non-zero file and body budgets",
        ));
    }
    let project_root = project_root.as_ref();
    if !project_root.is_dir() {
        return Err(AikitError::new(
            "local_source_discovery.project_missing",
            format!("Project root {} is not a directory", project_root.display()),
        ));
    }

    let relation_by_path = relations
        .iter()
        .map(|relation| (normalise_relative(&relation.relative_path), relation))
        .collect::<BTreeMap<_, _>>();
    let mut state = WalkState {
        project: &project,
        root: project_root,
        relation_by_path,
        limits,
        files_visited: 0,
        truncated: false,
        sources: Vec::new(),
    };
    walk(project_root, 0, &mut state)?;

    // An accepted native relation is source evidence even if the path did not
    // match a discovery convention. Surface any readable relation not encountered
    // by the bounded walk without inventing content or moving it.
    for (relative, relation) in &state.relation_by_path {
        if state
            .sources
            .iter()
            .any(|source| normalise_relative(&source.relative_path) == *relative)
        {
            continue;
        }
        let path = project_root.join(relative);
        if path.is_file() {
            let candidate = candidate_for(
                &project,
                project_root,
                &path,
                Some(relation),
                limits.max_body_bytes,
            )?;
            let classification = classify_local_source(&candidate);
            state.sources.push(DiscoveredLocalSource {
                relative_path: relative.clone(),
                candidate,
                classification,
            });
        }
    }

    state
        .sources
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(LocalSourceDiscovery {
        version: LOCAL_SOURCE_DISCOVERY_VERSION.into(),
        project,
        files_visited: state.files_visited,
        truncated: state.truncated,
        sources: state.sources,
    })
}

struct WalkState<'a> {
    project: &'a ProjectRef,
    root: &'a Path,
    relation_by_path: BTreeMap<PathBuf, &'a NativeSourceRelation>,
    limits: LocalSourceDiscoveryLimits,
    files_visited: usize,
    truncated: bool,
    sources: Vec<DiscoveredLocalSource>,
}

fn walk(directory: &Path, depth: usize, state: &mut WalkState<'_>) -> Result<()> {
    if state.truncated || depth > state.limits.max_depth {
        return Ok(());
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|error| io_error("local_source_discovery.read_dir", directory, error))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| io_error("local_source_discovery.read_dir", directory, error))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if state.files_visited >= state.limits.max_files_visited {
            state.truncated = true;
            break;
        }
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("local_source_discovery.metadata", &path, error))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if skip_directory(&path, state.root) {
                continue;
            }
            walk(&path, depth + 1, state)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        state.files_visited += 1;
        let relative = path
            .strip_prefix(state.root)
            .expect("walked path remains beneath Project root");
        let key = normalise_relative(relative);
        let relation = state.relation_by_path.get(&key).copied();
        if relation.is_none() && !candidate_path(&path, relative) {
            continue;
        }
        let candidate = candidate_for(
            state.project,
            state.root,
            &path,
            relation,
            state.limits.max_body_bytes,
        )?;
        if relation.is_none() && !candidate_content(&candidate) && !strong_filename_hint(relative) {
            continue;
        }
        let classification = classify_local_source(&candidate);
        state.sources.push(DiscoveredLocalSource {
            relative_path: relative.to_path_buf(),
            candidate,
            classification,
        });
    }
    Ok(())
}

fn candidate_for(
    project: &ProjectRef,
    root: &Path,
    path: &Path,
    relation: Option<&NativeSourceRelation>,
    max_body_bytes: usize,
) -> Result<LocalSourceCandidate> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| AikitError::new("local_source_discovery.path_escape", "source escaped Project root"))?;
    let bytes = fs::read(path)
        .map_err(|error| io_error("local_source_discovery.read", path, error))?;
    let body_excerpt = String::from_utf8(bytes.into_iter().take(max_body_bytes).collect())
        .ok()
        .filter(|body| !body.trim().is_empty());
    let generated = generated_hint(relative, body_excerpt.as_deref());
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "discovery.version".into(),
        LOCAL_SOURCE_DISCOVERY_VERSION.into(),
    );
    metadata.insert("project".into(), project.to_string());
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        metadata.insert("extension".into(), extension.into());
    }
    metadata.insert(
        "owner-relation".into(),
        if relation.is_some() { "present" } else { "absent" }.into(),
    );

    let (source, authority, adopted_role) = if let Some(relation) = relation {
        (
            relation.source.clone(),
            relation.authority,
            Some(relation.role),
        )
    } else {
        (
            SourceRef::parse(&format!(
                "source:project-local:{}:{}",
                project.as_str(),
                relative.to_string_lossy()
            ))?,
            SourceAuthority::Observed,
            None,
        )
    };

    Ok(LocalSourceCandidate {
        source,
        path: relative.to_string_lossy().into_owned(),
        authority,
        declared_role: None,
        adopted_role,
        generated,
        body_excerpt,
        metadata,
    })
}

fn candidate_path(path: &Path, relative: &Path) -> bool {
    strong_filename_hint(relative)
        || relative
            .components()
            .any(|component| matches!(component.as_os_str().to_str(), Some("docs" | "doc" | "adr" | "adrs" | "architecture")))
        || path.extension().and_then(|value| value.to_str()).is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "c" | "h" | "hpp" | "java" | "kt" | "swift"
            )
        })
}

fn strong_filename_hint(relative: &Path) -> bool {
    let Some(name) = relative.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower == "agents.md"
        || lower == "claude.md"
        || lower == "context.md"
        || lower == "readme.md"
        || lower.starts_with("adr-")
        || lower.ends_with(".adr.md")
        || lower.contains("architecture")
        || lower.contains("interface")
        || lower.contains("contract")
        || lower.contains("manifest")
}

fn candidate_content(candidate: &LocalSourceCandidate) -> bool {
    let body = candidate
        .body_excerpt
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    [
        "local structural description",
        "agent governance",
        "how agents should",
        "project purpose",
        "why this project exists",
        "architecture contract",
        "module owns",
        "interface contract",
        "applies to",
        "describes",
        "method",
        "contextsource",
    ]
    .iter()
    .any(|needle| body.contains(needle))
}

fn generated_hint(relative: &Path, body: Option<&str>) -> bool {
    let path = relative.to_string_lossy().to_ascii_lowercase();
    let body = body.unwrap_or_default().to_ascii_lowercase();
    path.contains("/generated/")
        || path.starts_with("generated/")
        || path.contains("/dist/")
        || path.contains("/build/")
        || body.contains("generated file; do not edit")
        || body.contains("generated file - do not edit")
}

fn skip_directory(path: &Path, root: &Path) -> bool {
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or_default();
    if matches!(name, ".git" | "node_modules" | "target" | "dist" | "build") {
        return true;
    }
    // Respect the same recursive disclosure marker used by ProjectCentral. This
    // is a retrieval/discovery boundary, not a semantic source classification.
    path != root && path.join(aikit_core::NO_AGENT_RETRIEVAL_MARKER).exists()
}

fn normalise_relative(path: &Path) -> PathBuf {
    path.components().collect()
}

fn io_error(code: &str, path: &Path, error: std::io::Error) -> AikitError {
    AikitError::new(code, format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_only_is_discovery_not_authority() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("AGENTS.md"), "Use project conventions.\n").unwrap();
        let result = discover_local_sources(
            ProjectRef::parse("example/project").unwrap(),
            temp.path(),
            &[],
            LocalSourceDiscoveryLimits::default(),
        )
        .unwrap();
        let source = result
            .sources
            .iter()
            .find(|source| source.relative_path == PathBuf::from("AGENTS.md"))
            .unwrap();
        assert_eq!(source.classification.role, LocalSourceRole::Unresolved);
        assert!(!source.classification.candidates.is_empty());
        assert_eq!(source.candidate.authority, SourceAuthority::Observed);
    }

    #[test]
    fn owner_relation_retains_native_source_in_place() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("README.md"),
            "Project purpose: preserve the existing world.\n",
        )
        .unwrap();
        let relation = NativeSourceRelation {
            relative_path: "README.md".into(),
            source: SourceRef::parse("central:project-source:example/project:readme").unwrap(),
            role: LocalSourceRole::HumanProjectGround,
            authority: SourceAuthority::Authored,
        };
        let result = discover_local_sources(
            ProjectRef::parse("example/project").unwrap(),
            temp.path(),
            &[relation],
            LocalSourceDiscoveryLimits::default(),
        )
        .unwrap();
        assert_eq!(result.sources.len(), 1);
        let source = &result.sources[0];
        assert_eq!(source.relative_path, PathBuf::from("README.md"));
        assert_eq!(source.classification.role, LocalSourceRole::HumanProjectGround);
        assert_eq!(source.candidate.source.as_str(), "central:project-source:example/project:readme");
    }

    #[test]
    fn generated_projection_never_promotes_itself() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("docs/generated")).unwrap();
        fs::write(
            temp.path().join("docs/generated/architecture.md"),
            "Generated file; do not edit. Project purpose: copied text.\n",
        )
        .unwrap();
        let result = discover_local_sources(
            ProjectRef::parse("example/project").unwrap(),
            temp.path(),
            &[],
            LocalSourceDiscoveryLimits::default(),
        )
        .unwrap();
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].classification.role, LocalSourceRole::DerivedDocumentation);
    }

    #[test]
    fn discovery_budget_is_explicit() {
        let temp = tempfile::tempdir().unwrap();
        for index in 0..4 {
            fs::write(
                temp.path().join(format!("README-{index}.md")),
                "ordinary notes\n",
            )
            .unwrap();
        }
        let result = discover_local_sources(
            ProjectRef::parse("example/project").unwrap(),
            temp.path(),
            &[],
            LocalSourceDiscoveryLimits {
                max_files_visited: 2,
                ..LocalSourceDiscoveryLimits::default()
            },
        )
        .unwrap();
        assert_eq!(result.files_visited, 2);
        assert!(result.truncated);
    }
}
