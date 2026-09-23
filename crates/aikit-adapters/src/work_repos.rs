//! Live Work-repos search: ripgrep over every declared project's actual files
//! at query time, so `aikit knowledge search` reaches repo code and docs the
//! same way it reaches Control prose.
//!
//! The roster is read from the manifests every Work folder already carries —
//! `Work/<name>/ProjectCentral/project.json` (`central.project/v1`, field
//! `project_id`) — the same mechanical test ctrl's file map uses. A missing,
//! unparseable, or invalid manifest is one named absence per project, never a
//! silent skip; a double-prefixed `project_id` is rejected with a named
//! absence until the register itself is fixed.
//!
//! Search runs at query time over the existing ripgrep searcher. No index is
//! maintained, so nothing can go stale: hits carry `observed` authority by
//! construction. Gitignore is respected (`hidden=false` — the searcher's
//! default), the owner's `.no-agent-retrieval` marker prunes marked subtrees
//! before any bytes are read, and `.DS_Store` is excluded outright. The file
//! set is bounded by an explicit type list; a root query runs per-repo with
//! per-repo limits, so cost is bounded by `limit × projects`, never by repo
//! size.
//!
//! Identity is the project register's own namespace: hits carry
//! `source:project:<project_id>:<relative>` refs (the convention the
//! folder-subject compiler uses for a project's native root), and reads go
//! back to disk through the ProjectCentral binding's own agent-readability
//! rule, so withholding is enforced in one place.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use aikit_core::knowledge_source_pool::*;
use aikit_core::resource::{ProviderRef, ResourceLocator, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result, CENTRAL_PROJECT_SCHEMA, NO_AGENT_RETRIEVAL_MARKER};
use serde_json::json;

use crate::now_field::content_revision;
use crate::ripgrep::{RipgrepSearcher, SearchRequest, RIPGREP_TESTED_VERSION};
use crate::runner::CommandRunner;

pub const WORK_REPOS_PROVIDER_REF: &str = "provider/source-pool/work-repos";

/// Bound on one project read through this provider, mirroring the NOW-field
/// read budget.
pub const WORK_REPOS_MAX_READ_BYTES: u64 = 1024 * 1024;

/// Depth cap for the marker walk that prunes `.no-agent-retrieval` subtrees
/// per project (the NOW-field walk's discipline).
const MARKER_WALK_DEPTH: usize = 8;

/// Matches retained per repo before grouping; the per-repo *file* limit is the
/// caller's `limit`, so total materialised hits stay bounded by
/// `limit × projects`.
const PER_REPO_MATCH_BUDGET: usize = 4000;

/// First-class content types swept per repo (addendum A-7). Per-repo result
/// limits and the searcher's `--max-filesize` budget keep json-heavy and
/// noise repos bounded.
pub const WORK_REPOS_TYPES: [&str; 14] = [
    "md", "rs", "ts", "tsx", "js", "mjs", "c", "h", "py", "sh", "toml", "json", "yml", "html",
];

/// Directories never swept, whatever the gitignore says (mirrors the
/// discovery walk's ignore list, for repos without a gitignore of their own).
const IGNORED_DIR_NAMES: [&str; 5] = [".git", "target", "node_modules", ".next", "dist"];

/// One declared project on the runtime roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkRepoProject {
    /// The Work folder name (`Work/<name>`), used for display and scoping.
    pub name: String,
    /// The manifest's own `project_id` — the identity refs are minted under.
    pub project_id: String,
    /// Absolute project root.
    pub root: PathBuf,
}

/// One roster outcome: a usable project, or a named absence for a folder that
/// carries a manifest the roster cannot honour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterEntry {
    Project(WorkRepoProject),
    Absence { name: String, reason: String },
}

/// Assemble the project roster from declarations, not environment: every
/// `Work/*` folder carrying `ProjectCentral/project.json` is a candidate (the
/// same mechanical test ctrl's file map uses), and each manifest is parsed and
/// validated on its own terms. Infallible: a folder that cannot be honoured
/// yields one [`RosterEntry::Absence`] naming it.
pub fn discover_work_roster(central_root: &Path) -> Vec<RosterEntry> {
    let mut entries = Vec::new();
    let Ok(projects) = std::fs::read_dir(central_root.join("Work")) else {
        return entries;
    };
    let mut candidates: Vec<(String, PathBuf)> = projects
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            path.join("ProjectCentral/project.json")
                .is_file()
                .then_some((name, path))
        })
        .collect();
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, root) in candidates {
        entries.push(read_roster_entry(&name, &root));
    }
    entries
}

fn read_roster_entry(name: &str, root: &Path) -> RosterEntry {
    let manifest_path = root.join("ProjectCentral/project.json");
    let invalid = |reason: String| RosterEntry::Absence {
        name: name.to_owned(),
        reason,
    };
    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            return invalid(format!(
                "register unreadable: could not read {}: {error}",
                manifest_path.display()
            ));
        }
    };
    #[derive(serde::Deserialize)]
    struct Manifest {
        schema: String,
        project_id: String,
    }
    let manifest: Manifest = match serde_json::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => {
            return invalid(format!(
                "register unreadable: {} is not a valid manifest: {error}",
                manifest_path.display()
            ));
        }
    };
    if manifest.schema != CENTRAL_PROJECT_SCHEMA {
        return invalid(format!(
            "register invalid: expected {CENTRAL_PROJECT_SCHEMA}, found {}",
            manifest.schema
        ));
    }
    let project_id = manifest.project_id.trim().to_owned();
    if project_id.is_empty() {
        return invalid("register invalid: project_id is empty".to_owned());
    }
    // A double-prefixed id (`project:quaternal-logic`) would mint double-
    // prefixed refs (`source:project:project:…`). The roster rejects it with
    // a named absence; the register itself is the place the fix belongs.
    if project_id.starts_with("project:") || project_id.contains("::") {
        return invalid(format!(
            "register invalid: project_id {project_id:?} is already namespaced; expected the bare project id"
        ));
    }
    RosterEntry::Project(WorkRepoProject {
        name: name.to_owned(),
        project_id,
        root: root.to_path_buf(),
    })
}

fn provider() -> ProviderRef {
    ProviderRef::parse(WORK_REPOS_PROVIDER_REF).expect("static ref")
}

/// Glob class for a pattern letter that is special inside a ripgrep glob.
fn is_glob_metachar(ch: char) -> bool {
    matches!(ch, '*' | '?' | '[' | ']' | '{' | '}')
}

fn glob_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if is_glob_metachar(ch) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// An escape of one regex alternation arm: every term is a literal, whatever
/// characters it carries.
fn regex_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            escaped.push(ch);
        } else {
            escaped.push('\\');
            escaped.push(ch);
        }
    }
    escaped
}

/// The `.no-agent-retrieval` subtrees of one project, as ripgrep exclude globs
/// (`**/<relative>/**`). Marked directories are found by walking the project
/// before any search runs — the before-processing half of authorisation: a
/// pruned path is never opened.
///
/// `ProjectCentral/**` is excluded outright: the register (manifest, human
/// ground, governance, wiki, now records) is aperture owned by the
/// ProjectCentral binding, the Central file map and the NOW-field pool — the
/// live pool covers everything else in the repo (addendum A-4's join order).
fn marker_exclude_globs(project_root: &Path) -> Vec<String> {
    let mut globs = vec![
        format!("**/{NO_AGENT_RETRIEVAL_MARKER}"),
        "**/.DS_Store".into(),
        "**/ProjectCentral/**".into(),
    ];
    for name in IGNORED_DIR_NAMES {
        globs.push(format!("**/{name}/**"));
    }
    let mut marked = BTreeSet::new();
    let mut stack = vec![(project_root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MARKER_WALK_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let child = entry.path();
            if child
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| IGNORED_DIR_NAMES.contains(&value))
            {
                continue;
            }
            if child.join(NO_AGENT_RETRIEVAL_MARKER).is_file() {
                if let Ok(relative) = child.strip_prefix(project_root) {
                    marked.insert(relative.to_string_lossy().replace('\\', "/"));
                }
                continue;
            }
            stack.push((child, depth + 1));
        }
    }
    for relative in marked {
        // Glob metacharacters in a real directory name are escaped so the
        // exclusion names the directory, not a pattern.
        globs.push(format!("**/{}/**", glob_escape(&relative)));
    }
    globs
}

pub struct WorkReposSourcePoolProvider<R> {
    searcher: RipgrepSearcher<R>,
    projects: Vec<WorkRepoProject>,
    marker_globs: BTreeMap<String, Vec<String>>,
    version: Option<String>,
}

impl<R: CommandRunner> WorkReposSourcePoolProvider<R> {
    /// Attach to the declared Work roster. The ripgrep probe happens here so
    /// an unavailable binary is an attachment disclosure, not a mid-search
    /// surprise.
    pub fn connect(
        runner: R,
        executable: impl Into<PathBuf>,
        projects: Vec<WorkRepoProject>,
    ) -> Self {
        let marker_globs = projects
            .iter()
            .map(|project| (project.name.clone(), marker_exclude_globs(&project.root)))
            .collect();
        let searcher = RipgrepSearcher::new(runner, executable);
        let version = searcher.probe().ok();
        Self {
            searcher,
            projects,
            marker_globs,
            version,
        }
    }

    pub fn projects(&self) -> &[WorkRepoProject] {
        &self.projects
    }

    fn project_for(&self, project_id: &str) -> Option<&WorkRepoProject> {
        self.projects.iter().find(|p| p.project_id == project_id)
    }

    fn source_ref(project: &WorkRepoProject, relative: &Path) -> Result<SourceRef> {
        SourceRef::parse(format!(
            "source:project:{}:{}",
            project.project_id,
            relative.to_string_lossy().replace('\\', "/")
        ))
    }

    /// Parse `source:project:<project_id>:<relative>` back into its project
    /// and a safe project-relative path. Anything that escapes the project
    /// root, or names a project off the roster, is refused.
    fn resolve_ref(&self, source: &SourceRef) -> Result<(&WorkRepoProject, PathBuf)> {
        let raw = source.as_str();
        let prefix = "source:project:";
        let Some(rest) = raw.strip_prefix(prefix) else {
            return Err(AikitError::new(
                "work_repos.source_out_of_scope",
                format!("{raw} is not a project source ref"),
            ));
        };
        let Some((project_id, relative)) = rest.split_once(':') else {
            return Err(AikitError::new(
                "work_repos.source_out_of_scope",
                format!("{raw} carries no project-relative path"),
            ));
        };
        let project = self.project_for(project_id).ok_or_else(|| {
            AikitError::new(
                "work_repos.source_out_of_scope",
                format!("{project_id:?} is not on this horizon's Work roster"),
            )
        })?;
        let relative = PathBuf::from(relative);
        let safe = !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)));
        if !safe {
            return Err(AikitError::new(
                "work_repos.source_escape",
                format!("{relative:?} is not a safe project-relative path"),
            ));
        }
        Ok((project, relative))
    }

    fn media_type(relative: &Path) -> &'static str {
        match relative.extension().and_then(|value| value.to_str()) {
            Some("json") => "application/json",
            Some("html") => "text/html",
            Some("md") | Some("markdown") => "text/markdown",
            _ => "text/plain",
        }
    }

    fn tags(project: &WorkRepoProject) -> Vec<String> {
        vec!["work-repos".into(), project.name.to_lowercase()]
    }

    fn material(&self, source: &SourceRef) -> Result<SourceMaterial> {
        let (project, relative) = self.resolve_ref(source)?;
        let absolute = project.root.join(&relative);
        if !crate::projectcentral::path_agent_readable(&project.root, &relative) {
            return Err(AikitError::new(
                "work_repos.source_unauthorised",
                format!(
                    "{relative:?} is withheld from agent retrieval inside Work/{}",
                    project.name
                ),
            ));
        }
        let bytes = std::fs::read(&absolute).map_err(|error| {
            AikitError::new(
                "work_repos.source_unreadable",
                format!("could not read {relative:?}: {error}"),
            )
        })?;
        if bytes.len() as u64 > WORK_REPOS_MAX_READ_BYTES {
            return Err(AikitError::new(
                "work_repos.source_too_large",
                format!("{relative:?} exceeds the Work-repos read budget"),
            ));
        }
        Ok(SourceMaterial {
            binding: SourceBinding {
                source: source.clone(),
                revision: SourceRevision::parse(content_revision(&bytes))?,
                title: relative.to_string_lossy().replace('\\', "/"),
                tags: Self::tags(project),
                // Owner-authorised ground searched in place, the way the
                // file-map owner read does it — never an actor-independent
                // grant.
                visibility: SourceVisibility::Personal,
                owners: vec![],
                media_type: Self::media_type(&relative).into(),
                locator: Some(ResourceLocator::Path(absolute)),
                metadata: [
                    (
                        "work-repos",
                        json!({"project": project.name, "project_id": project.project_id}),
                    ),
                    ("owner_read_required", json!(false)),
                ]
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
            },
            body: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }

    /// Search one repo: one ripgrep invocation over an alternation of the
    /// escaped query terms, grouped per file and scored by how many distinct
    /// terms the file carries. Files matching every term outrank partial
    /// matches; within a class, more matching lines is more evidence.
    fn search_project(
        &self,
        project: &WorkRepoProject,
        terms: &[String],
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        if terms.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let pattern = terms
            .iter()
            .map(|term| regex_escape(term))
            .collect::<Vec<_>>()
            .join("|");
        let exclude_globs = self
            .marker_globs
            .get(&project.name)
            .cloned()
            .unwrap_or_default();
        for tag_term in tags {
            // A tag term that names a project narrows the sweep to it; a tag
            // this pool never minted answers nothing.
            if tag_term != "work-repos" && tag_term.to_lowercase() != project.name.to_lowercase() {
                return Ok(Vec::new());
            }
        }
        let request = SearchRequest {
            pattern,
            // An alternation of escaped literals: per-term OR, still literal
            // in substance.
            regex: true,
            // Code spells the same word in many cases (`mcp`, `Mcp`, `MCP`);
            // the query folds case.
            ignore_case: true,
            roots: vec![project.root.clone()],
            include_globs: vec![format!("*.{{{}}}", WORK_REPOS_TYPES.join(","))],
            exclude_globs,
            // Gitignore respected: no --hidden, no --no-ignore.
            hidden: false,
            max_file_bytes: crate::ripgrep::DEFAULT_MAX_FILE_BYTES,
            limit: PER_REPO_MATCH_BUDGET,
            timeout: Some(std::time::Duration::from_secs(30)),
        };
        let outcome = self.searcher.search(&request)?;
        // Group matches by file; count distinct terms and keep the first
        // matching line as the snippet. Term membership folds case, matching
        // the searcher's own case-folding.
        let lowered: Vec<String> = terms.iter().map(|term| term.to_lowercase()).collect();
        struct FileEvidence {
            terms: BTreeSet<usize>,
            lines: usize,
            snippet: Option<String>,
            first_line: u64,
        }
        let mut files: BTreeMap<PathBuf, FileEvidence> = BTreeMap::new();
        for matched in &outcome.matches {
            let Some(relative) = matched
                .path
                .strip_prefix(&project.root)
                .ok()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
            else {
                continue;
            };
            let line_lowered = matched.line.to_lowercase();
            let term_hits: Vec<usize> = lowered
                .iter()
                .enumerate()
                .filter(|(_, term)| line_lowered.contains(term.as_str()))
                .map(|(index, _)| index)
                .collect();
            let entry = files
                .entry(PathBuf::from(relative))
                .or_insert(FileEvidence {
                    terms: BTreeSet::new(),
                    lines: 0,
                    snippet: None,
                    first_line: matched.line_number,
                });
            entry.terms.extend(term_hits);
            entry.lines += 1;
            if entry.snippet.is_none() {
                let snippet: String = matched.line.chars().take(240).collect();
                entry.snippet = Some(snippet.trim_end().to_string());
            }
        }
        let mut scored: Vec<(f64, PathBuf, FileEvidence)> = files
            .into_iter()
            .map(|(relative, evidence)| {
                let coverage = evidence.terms.len() as f64 / terms.len() as f64;
                let complete = if evidence.terms.len() == terms.len() {
                    1.0
                } else {
                    0.0
                };
                let mass = (evidence.lines.min(16) as f64) * 0.01;
                (coverage + complete + mass, relative, evidence)
            })
            .collect();
        scored.sort_by(|left, right| {
            right
                .0
                .total_cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        Ok(scored
            .into_iter()
            .take(limit)
            .filter_map(|(score, relative, evidence)| {
                let source = Self::source_ref(project, &relative).ok()?;
                Some(SourceHit {
                    source,
                    provider: provider(),
                    score: Some(score),
                    title: relative.to_string_lossy().replace('\\', "/"),
                    snippet: evidence.snippet.clone().unwrap_or_default(),
                    tags: Self::tags(project),
                    provider_binding: Some(format!("line:{}", evidence.first_line)),
                    retrieval_mode: SourceSearchMode::Fulltext,
                })
            })
            .collect())
    }

    fn run_search(&self, query: &str, tags: &[String], limit: usize) -> Result<Vec<SourceHit>> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(str::to_owned)
            .filter(|term| !term.is_empty())
            .collect();
        if terms.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut merged: Vec<SourceHit> = Vec::new();
        for project in &self.projects {
            // Every repo answers on its own terms; the merge is bounded by
            // limit × projects and the final ranking decides what surfaces.
            let hits = self.search_project(project, &terms, tags, limit)?;
            merged.extend(hits);
        }
        merged.sort_by(|left, right| {
            right
                .score
                .unwrap_or(0.0)
                .total_cmp(&left.score.unwrap_or(0.0))
                .then_with(|| left.source.cmp(&right.source))
        });
        merged.truncate(limit);
        Ok(merged)
    }
}

impl<R: CommandRunner> SourcePoolProvider for WorkReposSourcePoolProvider<R> {
    fn capabilities(&self) -> SourceProviderCapabilities {
        SourceProviderCapabilities {
            provider: provider(),
            version: self.version.clone(),
            fulltext: self.version.is_some() && !self.projects.is_empty(),
            fuzzy_interactive: false,
            semantic: false,
            hybrid: false,
            tags: true,
            structured_output: true,
            reasons: [
                (
                    "semantic",
                    "live literal content search needs no embeddings; semantic retrieval stays with the semantic providers",
                ),
                (
                    "hybrid",
                    "hybrid retrieval stays with hybrid-capable providers",
                ),
                (
                    "fuzzy-interactive",
                    "repo files answer exact and literal questions; fuzzy ranking stays with indexed providers",
                ),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        }
    }

    /// The repos are live owner ground, not a disposable local index; there is
    /// nothing for AIKit to rebuild and no material to preload.
    fn rebuild(&mut self, _: &[SourceMaterial]) -> Result<()> {
        Err(AikitError::new(
            "work_repos.owner_only",
            "Work repositories are live owner ground; AIKit searches them in place and cannot rebuild them",
        ))
    }

    fn read(&self, source: &SourceRef) -> Result<Option<SourceMaterial>> {
        // resolve_ref refuses refs outside this pool's namespace before any
        // byte is read, so the live-read fallback can ask this pool safely.
        Ok(Some(self.material(source)?))
    }

    fn search(
        &self,
        query: &str,
        mode: SourceSearchMode,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        if mode != SourceSearchMode::Fulltext {
            return Err(AikitError::new(
                "knowledge.source_provider_capability",
                format!("the Work-repos provider does not support {}", mode.as_str()),
            ));
        }
        self.run_search(query, tags, limit)
    }

    fn status(&self) -> SourceProviderStatus {
        let capabilities = self.capabilities();
        let version = capabilities.version.clone();
        // Each per-project glob list is the three fixed exclusions (marker
        // file, .DS_Store, ProjectCentral aperture), the five ignored
        // directory names, then one glob per marked subtree — anything beyond
        // eight names a marked subtree.
        const FIXED_GLOBS: usize = 8;
        let marked: usize = self
            .marker_globs
            .values()
            .map(|globs| globs.len().saturating_sub(FIXED_GLOBS))
            .sum();
        SourceProviderStatus {
            provider: provider(),
            available: capabilities.fulltext,
            version: version.clone(),
            tested_version: Some(RIPGREP_TESTED_VERSION.into()),
            version_drift: version
                .as_deref()
                .is_some_and(|value| !value.contains(RIPGREP_TESTED_VERSION)),
            capabilities,
            detail: format!(
                "live ripgrep content search over {} Work repo(s); gitignore respected \
                 (hidden=false); types {}; .no-agent-retrieval prunes {} marked subtree(s); \
                 per-repo limits bound a root query by limit × projects",
                self.projects.len(),
                WORK_REPOS_TYPES.join("/"),
                marked,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{RecordingRunner, ScriptedRunner};

    fn project(root: &Path) -> WorkRepoProject {
        WorkRepoProject {
            name: "demo".into(),
            project_id: "demo".into(),
            root: root.to_path_buf(),
        }
    }

    fn manifest(project_id: &str) -> String {
        format!(
            r#"{{"schema":"central.project/v1","project_id":"{project_id}","human_source":"ProjectCentral/user","wiki":{{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json","adopted_sources":[]}}}}"#
        )
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn roster_reads_declared_manifests_and_names_every_failure() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(
            &root.join("Work/alpha/ProjectCentral/project.json"),
            &manifest("alpha"),
        );
        // Noise folder without a manifest is not a candidate at all.
        std::fs::create_dir_all(root.join("Work/plain")).unwrap();
        // Unparseable manifest.
        write(
            &root.join("Work/beta/ProjectCentral/project.json"),
            "not json",
        );
        // Wrong schema.
        write(
            &root.join("Work/gamma/ProjectCentral/project.json"),
            r#"{"schema":"central.project/v0","project_id":"gamma"}"#,
        );
        // Double-prefixed id.
        write(
            &root.join("Work/delta/ProjectCentral/project.json"),
            &manifest("project:delta"),
        );

        let entries = discover_work_roster(root);
        let mut projects = Vec::new();
        let mut absences = Vec::new();
        for entry in entries {
            match entry {
                RosterEntry::Project(project) => projects.push(project),
                RosterEntry::Absence { name, reason } => absences.push((name, reason)),
            }
        }
        assert_eq!(projects.len(), 1, "{absences:?}");
        assert_eq!(projects[0].name, "alpha");
        assert_eq!(projects[0].project_id, "alpha");
        assert_eq!(absences.len(), 3, "{absences:?}");
        let (beta_reason, gamma_reason, delta_reason) = (
            absences
                .iter()
                .find(|(name, _)| name == "beta")
                .expect("beta named")
                .1
                .clone(),
            absences
                .iter()
                .find(|(name, _)| name == "gamma")
                .expect("gamma named")
                .1
                .clone(),
            absences
                .iter()
                .find(|(name, _)| name == "delta")
                .expect("delta named")
                .1
                .clone(),
        );
        assert!(beta_reason.contains("register unreadable"), "{beta_reason}");
        assert!(
            gamma_reason.contains("central.project/v1"),
            "{gamma_reason}"
        );
        assert!(
            delta_reason.contains("already namespaced"),
            "{delta_reason}"
        );
    }

    #[test]
    fn roster_is_stable_and_sorted_by_folder_name() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["zeta", "Alpha", "beta"] {
            write(
                &temp
                    .path()
                    .join("Work")
                    .join(name)
                    .join("ProjectCentral/project.json"),
                &manifest(name),
            );
        }
        let names: Vec<String> = discover_work_roster(temp.path())
            .into_iter()
            .filter_map(|entry| match entry {
                RosterEntry::Project(project) => Some(project.name),
                RosterEntry::Absence { .. } => None,
            })
            .collect();
        assert_eq!(names, vec!["Alpha", "beta", "zeta"]);
    }

    fn scripted_matches(root: &Path, files: &[(&str, u64, &str)]) -> ScriptedRunner {
        let mut lines = String::new();
        for (path, line_number, text) in files {
            let absolute = root.join(path);
            lines.push_str(&format!(
                r#"{{"type":"match","data":{{"path":{{"text":"{}"}},"lines":{{"text":"{}\n"}},"line_number":{line_number}}}}}"#,
                absolute.display(),
                text
            ));
            lines.push('\n');
        }
        ScriptedRunner::new()
            .on("--version", "ripgrep 15.2.0")
            .on("--json", &lines)
    }

    #[test]
    fn hits_carry_project_register_refs_and_all_term_files_outrank_partials() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let provider = WorkReposSourcePoolProvider::connect(
            scripted_matches(
                root,
                &[
                    (
                        "src/routine.rs",
                        3,
                        "automations drive the cron scheduled gate",
                    ),
                    ("docs/plan.md", 9, "automations cron"),
                    ("docs/other.md", 2, "cron only"),
                ],
            ),
            "rg",
            vec![project(root)],
        );
        let hits = provider
            .search(
                "automations cron scheduled",
                SourceSearchMode::Fulltext,
                &[],
                10,
            )
            .unwrap();
        assert_eq!(hits.len(), 3, "{hits:?}");
        assert_eq!(
            hits[0].source.as_str(),
            "source:project:demo:src/routine.rs",
            "the file carrying every term outranks partials: {hits:?}"
        );
        assert_eq!(hits[0].provider.as_str(), WORK_REPOS_PROVIDER_REF);
        assert!(hits[0].tags.contains(&"work-repos".to_string()));
        assert!(hits.iter().all(|hit| hit.title.contains('.')));
    }

    #[test]
    fn the_argv_pins_gitignore_respect_and_the_type_list() {
        let temp = tempfile::tempdir().unwrap();
        let recorder = RecordingRunner::new(ScriptedRunner::new());
        let provider =
            WorkReposSourcePoolProvider::connect(&recorder, "rg", vec![project(temp.path())]);
        // The scripted inner runner answers nothing, so the search itself
        // errors — the recorded argv is what this test reads.
        let _ = provider.search("x", SourceSearchMode::Fulltext, &[], 5);
        let calls = recorder.calls();
        let search = calls
            .iter()
            .find(|argv| argv.iter().any(|arg| arg == "--json"))
            .expect("a search ran");
        assert!(
            search.iter().any(|arg| arg.starts_with("*.{md,")),
            "the type list rides one include glob: {search:?}"
        );
        assert!(
            !search.iter().any(|arg| arg == "--hidden"),
            "hidden=false keeps gitignore respect: {search:?}"
        );
        assert!(
            !search.iter().any(|arg| arg == "--no-ignore"),
            "hidden=false keeps gitignore respect: {search:?}"
        );
        assert!(search.iter().any(|arg| arg == "--max-filesize"));
    }

    #[test]
    fn marker_carrying_subtrees_are_excluded_before_any_search() {
        let temp = tempfile::tempdir().unwrap();
        let project_root = temp.path();
        write(
            &project_root.join("ProjectCentral/user/private/.no-agent-retrieval"),
            "",
        );
        let globs = marker_exclude_globs(project_root);
        assert!(
            globs.iter().any(|glob| glob.ends_with("private/**")),
            "the marked subtree is named as an exclude glob: {globs:?}"
        );
        assert!(globs.contains(&"**/.DS_Store".to_string()));
        assert!(
            globs.contains(&"**/ProjectCentral/**".to_string()),
            "the register aperture belongs to the binding lens, not the live pool"
        );
        assert!(globs.iter().any(|glob| glob.ends_with("target/**")));
    }

    #[test]
    fn read_enforces_agent_readability_and_project_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(&root.join("src/routine.rs"), "automations cron\n");
        write(&root.join("sealed/secret.md"), "hidden\n");
        write(&root.join("sealed/.no-agent-retrieval"), "");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("src/routine.rs"), root.join("linked.md")).unwrap();
        let provider = WorkReposSourcePoolProvider::connect(
            ScriptedRunner::new().on("--version", "ripgrep 15.2.0"),
            "rg",
            vec![project(root)],
        );
        let reading = provider
            .read(&SourceRef::parse("source:project:demo:src/routine.rs").unwrap())
            .unwrap()
            .expect("a disclosed repo file reads back live");
        assert!(reading.body.contains("automations cron"));
        assert_eq!(
            reading.binding.revision.as_str(),
            content_revision(b"automations cron\n")
        );

        let error = provider
            .read(&SourceRef::parse("source:project:demo:sealed/secret.md").unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "work_repos.source_unauthorised");

        #[cfg(unix)]
        {
            let error = provider
                .read(&SourceRef::parse("source:project:demo:linked.md").unwrap())
                .unwrap_err();
            assert_eq!(error.code(), "work_repos.source_unauthorised");
        }

        let error = provider
            .read(&SourceRef::parse("source:project:other:src/routine.rs").unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "work_repos.source_out_of_scope");

        let error = provider
            .read(&SourceRef::parse("source:project:demo:../escape.md").unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "work_repos.source_escape");

        let error = provider
            .read(&SourceRef::parse("central:source:control:root:x").unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "work_repos.source_out_of_scope");
    }

    #[test]
    fn rebuild_is_refused_because_the_repos_are_live_ground() {
        let mut provider = WorkReposSourcePoolProvider::connect(
            ScriptedRunner::new(),
            "rg",
            vec![project(Path::new("/ground"))],
        );
        let error = provider.rebuild(&[]).unwrap_err();
        assert_eq!(error.code(), "work_repos.owner_only");
    }

    #[test]
    fn unavailable_ripgrep_is_a_disclosed_absence_not_a_fake_available() {
        let runner = ScriptedRunner::new().failing("--version", 127, "command not found");
        let provider =
            WorkReposSourcePoolProvider::connect(runner, "rg", vec![project(Path::new("/ground"))]);
        assert!(!provider.status().available);
    }

    #[test]
    fn status_discloses_the_roster_and_version_drift() {
        let runner = ScriptedRunner::new().on("--version", "ripgrep 14.1.0");
        let provider =
            WorkReposSourcePoolProvider::connect(runner, "rg", vec![project(Path::new("/ground"))]);
        let status = provider.status();
        assert!(status.available);
        assert_eq!(status.version.as_deref(), Some("ripgrep 14.1.0"));
        assert!(
            status.version_drift,
            "the tested version is disclosed as drifted"
        );
        assert_eq!(
            status.tested_version.as_deref(),
            Some(RIPGREP_TESTED_VERSION)
        );
        assert!(status.detail.contains("1 Work repo(s)"));
        assert!(status.detail.contains("hidden=false"));
    }

    #[test]
    fn semantic_mode_is_a_capability_refusal_not_a_silent_empty() {
        let provider = WorkReposSourcePoolProvider::connect(
            ScriptedRunner::new(),
            "rg",
            vec![project(Path::new("/ground"))],
        );
        let error = provider
            .search("anything", SourceSearchMode::Semantic, &[], 10)
            .unwrap_err();
        assert_eq!(error.code(), "knowledge.source_provider_capability");
    }
}
