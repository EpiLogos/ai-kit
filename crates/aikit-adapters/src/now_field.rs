//! NOW-field search: ripgrep over the temporal records the field law itself
//! places at known paths.
//!
//! The Central ground keeps one NOW field per register at stable locations:
//! root clearings under `Control/agents/now/clearings/`, day snapshots under
//! `Control/agents/now/day/`, flow documents under `Control/agents/now/flows/`,
//! the human day under `Control/user/day/<date>/day.md`, and each project's
//! own register under `Work/<Name>/ProjectCentral/now/`. That placement is the
//! authorisation. This provider searches only those record families; it never
//! infers eligibility from filesystem readability, never descends into `.git`,
//! and honours the owner's `.no-agent-retrieval` marker by pruning marked
//! subtrees *before* any bytes are read — not by filtering results afterwards.
//!
//! Hits and reads carry canonical Central identity: source refs in the
//! `central:source:control:root:<path>` form and revisions in Central's own
//! in-tree `central.content-fnv1a64/v1` grammar. Search is literal by default;
//! regex is a separate, deliberate method. Ripgrep does not follow symlinks;
//! direct reads also reject symlink and traversal components.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use aikit_core::knowledge_source_pool::*;
use aikit_core::resource::{ProviderRef, ResourceLocator, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result};
use serde_json::json;

use crate::ripgrep::{RipgrepMatch, RipgrepSearcher, SearchRequest};
use crate::runner::CommandRunner;

pub const NOW_FIELD_PROVIDER_REF: &str = "provider/source-pool/now-field";
pub const AGENT_RETRIEVAL_MARKER: &str = ".no-agent-retrieval";
/// Bound on a single live owner read through this provider.
pub const MAX_READ_BYTES: u64 = 1024 * 1024;
/// Bound on the descriptor roster carried at attachment. The roster carries
/// identity (ref/revision/tags), never bodies; reads go back to the file.
pub const MAX_ROSTER_FILES: usize = 2048;

/// The owner's withhold lever: a subtree carrying this marker is invisible to
/// the provider no matter where it sits inside an eligible record family.
///
/// The runner is pinned to the central root because ripgrep matches globs
/// against root-relative paths only when the search root itself is relative;
/// the provider therefore searches `.` and reads paths back relative to the
/// root.
pub fn default_runner(central_root: &Path) -> crate::runner::SystemRunner {
    crate::runner::SystemRunner::new()
        .with_env_removed("RIPGREP_CONFIG_PATH")
        .with_cwd(central_root)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeInclude {
    pub glob: String,
    pub family: &'static str,
}

/// The authorised search scope. Constructed from the field law's record
/// families; nothing outside `includes` is ever read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowFieldScope {
    pub central_root: PathBuf,
    pub includes: Vec<ScopeInclude>,
    pub excludes: Vec<String>,
    /// Subtrees pruned because a marker was observed inside them.
    pub pruned: Vec<String>,
}

impl NowFieldScope {
    /// The root register's NOW field: clearings, day snapshots, flows, the
    /// human day file, and every project's ProjectCentral now register.
    pub fn standard(central_root: impl Into<PathBuf>) -> Self {
        let central_root = central_root.into();
        let includes = vec![
            ScopeInclude {
                glob: "Control/agents/now/clearings/**/*.json".into(),
                family: "now",
            },
            ScopeInclude {
                glob: "Control/agents/now/clearings/**/*.md".into(),
                family: "now",
            },
            ScopeInclude {
                glob: "Control/agents/now/day/**".into(),
                family: "day",
            },
            ScopeInclude {
                glob: "Control/agents/now/flows/**".into(),
                family: "flow",
            },
            ScopeInclude {
                glob: "Control/user/day/*/day.md".into(),
                family: "day",
            },
            ScopeInclude {
                glob: "Work/*/ProjectCentral/now/**/*.json".into(),
                family: "projectcentral",
            },
            ScopeInclude {
                glob: "Work/*/ProjectCentral/now/**/*.md".into(),
                family: "projectcentral",
            },
        ];
        let mut scope = Self {
            central_root,
            includes,
            excludes: vec![
                "**/.git/**".into(),
                "**/.central/**".into(),
                "**/.aikit/**".into(),
                "**/.vite/**".into(),
                "**/node_modules/**".into(),
                "**/target/**".into(),
                "**/dist/**".into(),
            ],
            pruned: Vec::new(),
        };
        scope.prune_marked_subtrees();
        scope
    }

    /// Retain common Control records and only one discovered Work project's
    /// NOW files. `None` keeps Control alone, so an unknown Project cannot
    /// turn a wildcard scope into a search of every sibling register.
    pub fn for_project(&self, project_name: Option<&str>) -> Self {
        let mut scoped = self.clone();
        scoped.includes.retain_mut(|include| {
            if let Some(rest) = include.glob.strip_prefix("Work/*/").map(str::to_owned) {
                if let Some(name) = project_name {
                    include.glob = format!("Work/{}/{rest}", escape_glob_literal(name));
                    true
                } else {
                    false
                }
            } else {
                !include.glob.starts_with("Work/")
            }
        });
        let own_root = project_name.map(|name| format!("Work/{name}"));
        scoped.pruned.retain(|path| {
            !path.starts_with("Work/")
                || own_root
                    .as_ref()
                    .is_some_and(|root| path == root || path.starts_with(&format!("{root}/")))
        });
        scoped
    }

    /// Walk only the directories an include glob can actually reach, and stop
    /// at any directory carrying the owner's marker. This is the
    /// before-processing half of authorisation: a pruned path is never opened.
    fn prune_marked_subtrees(&mut self) {
        let mut pruned = BTreeSet::new();
        for include in &self.includes {
            for dir in marked_boundaries(&self.central_root, &include.glob) {
                if let Ok(relative) = dir.strip_prefix(&self.central_root) {
                    pruned.insert(relative.to_string_lossy().into_owned());
                }
            }
        }
        self.pruned = pruned.into_iter().collect();
    }

    fn is_authorised(&self, relative: &Path) -> bool {
        let relative_str = relative.to_string_lossy();
        if self
            .excludes
            .iter()
            .any(|glob| glob_match(glob, &relative_str))
        {
            return false;
        }
        if self
            .pruned
            .iter()
            .any(|dir| relative.starts_with(Path::new(dir)))
        {
            return false;
        }
        self.includes
            .iter()
            .any(|include| glob_match(&include.glob, &relative_str))
    }
}

fn escape_glob_literal(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '*' | '?' | '[' | ']' | '{' | '}' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GlobPart {
    Literal(String),
    AnyWithin,
    AnyDepth,
}

fn parse_glob(glob: &str) -> Vec<Vec<GlobPart>> {
    glob.split('/')
        .map(|part| {
            if part == "**" {
                return vec![GlobPart::AnyDepth];
            }
            let mut parts = Vec::new();
            let mut literal = String::new();
            let mut chars = part.chars();
            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    literal.push(chars.next().unwrap_or('\\'));
                } else if ch == '*' {
                    if !literal.is_empty() {
                        parts.push(GlobPart::Literal(std::mem::take(&mut literal)));
                    }
                    parts.push(GlobPart::AnyWithin);
                } else {
                    literal.push(ch);
                }
            }
            if !literal.is_empty() {
                parts.push(GlobPart::Literal(literal));
            }
            if parts.is_empty() {
                parts.push(GlobPart::Literal(String::new()));
            }
            parts
        })
        .collect()
}

fn match_parts(pattern: &[Vec<GlobPart>], path: &str) -> bool {
    let Some((segment, rest)) = path.split_once('/') else {
        return match_segment(&pattern[0], path) && pattern.len() == 1;
    };
    if pattern.len() == 1 {
        return false;
    }
    if match_segment(&pattern[0], segment) && match_parts(&pattern[1..], rest) {
        return true;
    }
    if pattern[0].len() == 1 && pattern[0][0] == GlobPart::AnyDepth {
        return match_parts(pattern, rest);
    }
    false
}

fn match_segment(parts: &[GlobPart], segment: &str) -> bool {
    match parts {
        [] => segment.is_empty(),
        [GlobPart::AnyDepth, rest @ ..] => {
            boundary_suffixes(segment).any(|suffix| match_segment(rest, suffix))
        }
        [GlobPart::Literal(literal), rest @ ..] => {
            segment.starts_with(literal.as_str()) && match_segment(rest, &segment[literal.len()..])
        }
        [GlobPart::AnyWithin, rest @ ..] => {
            boundary_suffixes(segment).any(|suffix| match_segment(rest, suffix))
        }
    }
}

/// Path components can carry any character the filesystem allows, so a
/// wildcard cut may only land on a character boundary.
fn boundary_suffixes(segment: &str) -> impl Iterator<Item = &str> {
    (0..=segment.len())
        .filter(|cut| segment.is_char_boundary(*cut))
        .map(|cut| &segment[cut..])
}

pub fn glob_match(glob: &str, path: &str) -> bool {
    let pattern = parse_glob(glob);
    // `a/**` must also match the directory itself so its contents stay
    // eligible; every other shape is a whole-path match.
    match_parts(&pattern, path)
        || (glob.ends_with("/**") && match_parts(&pattern[..pattern.len() - 1], path))
}

/// Enumerate every directory a glob's file part could resolve under, honouring
/// markers as the walk goes. Only these directories are ever visited.
fn candidate_directories(root: &Path, glob: &str) -> Vec<PathBuf> {
    let pattern = parse_glob(glob);
    let mut current = vec![root.to_path_buf()];
    for parts in &pattern[..pattern.len().saturating_sub(1)] {
        let mut next = Vec::new();
        for dir in &current {
            if parts.len() == 1 && parts[0] == GlobPart::AnyDepth {
                next.extend(
                    walk_marked(root, dir)
                        .into_iter()
                        .filter(|candidate| !marker_between(root, candidate)),
                );
            } else if let Some(literal) = single_literal(parts) {
                let child = dir.join(literal);
                if !marker_between(root, &child) && is_real_directory(&child) {
                    next.push(child);
                }
            } else if is_real_directory(dir) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let child = entry.path();
                        if is_real_directory(&child)
                            && !marker_between(root, &child)
                            && matches_parts(parts, file_name(&child))
                        {
                            next.push(child);
                        }
                    }
                }
            }
        }
        current = next;
    }
    current
}

/// Discover the first marker on each path an include could traverse. Unlike
/// `candidate_directories`, this keeps a marked directory long enough to
/// record its boundary, then refuses to descend or read any child beneath it.
/// Ripgrep uses these boundaries as excludes before searching; descriptor
/// enumeration independently uses `candidate_directories` and cannot open the
/// marked directories either.
fn marked_boundaries(root: &Path, glob: &str) -> BTreeSet<PathBuf> {
    let pattern = parse_glob(glob);
    let mut current = vec![root.to_path_buf()];
    let mut marked = BTreeSet::new();
    for parts in &pattern[..pattern.len().saturating_sub(1)] {
        let mut next = Vec::new();
        for dir in &current {
            let candidates = if parts.len() == 1 && parts[0] == GlobPart::AnyDepth {
                walk_marked(root, dir)
            } else if let Some(literal) = single_literal(parts) {
                vec![dir.join(literal)]
            } else {
                std::fs::read_dir(dir)
                    .ok()
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|child| matches_parts(parts, file_name(child)))
                    .collect()
            };
            for child in candidates {
                if !is_real_directory(&child) {
                    continue;
                }
                if std::fs::symlink_metadata(child.join(AGENT_RETRIEVAL_MARKER)).is_ok() {
                    marked.insert(child);
                } else {
                    next.push(child);
                }
            }
        }
        current = next;
    }
    marked
}

fn single_literal(parts: &[GlobPart]) -> Option<&str> {
    if parts.len() == 1 {
        if let GlobPart::Literal(literal) = &parts[0] {
            return Some(literal.as_str());
        }
    }
    None
}

fn matches_parts(parts: &[GlobPart], segment: &str) -> bool {
    match_segment(parts, segment)
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .map(|name| name.to_str().unwrap_or_default())
        .unwrap_or_default()
}

fn is_real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn is_real_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

/// Whether this directory or any ancestor below the root carries the marker.
fn marker_between(root: &Path, dir: &Path) -> bool {
    let mut cursor = Some(dir);
    while let Some(current) = cursor {
        if current == root {
            return false;
        }
        if std::fs::symlink_metadata(current.join(AGENT_RETRIEVAL_MARKER)).is_ok() {
            return true;
        }
        cursor = current.parent();
    }
    false
}

fn walk_marked(root: &Path, dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let may_descend = is_real_directory(&current) && !marker_between(root, &current);
        found.push(current.clone());
        if !may_descend {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&current) {
            for entry in entries.flatten() {
                let child = entry.path();
                // A marked child is still reported as a boundary leaf, but
                // neither its contents nor a symbolic-link target are walked.
                if is_real_directory(&child) {
                    pending.push(child);
                }
            }
        }
    }
    found
}

/// Central's in-tree content revision grammar, identical to the owner's
/// `central.content-fnv1a64/v1:<byte_len>:<digest>` identity.
pub fn content_revision(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("central.content-fnv1a64/v1:{}:{hash:016x}", bytes.len())
}

fn provider() -> ProviderRef {
    ProviderRef::parse(NOW_FIELD_PROVIDER_REF).expect("static ref")
}

pub struct NowFieldSourcePoolProvider<R> {
    searcher: RipgrepSearcher<R>,
    scope: NowFieldScope,
    version: Option<String>,
}

impl<R: CommandRunner> NowFieldSourcePoolProvider<R> {
    /// Attach to the root register's NOW field. The ripgrep probe happens
    /// here so an unavailable binary is an attachment disclosure, not a
    /// mid-search surprise.
    pub fn connect(
        runner: R,
        executable: impl Into<PathBuf>,
        scope: NowFieldScope,
    ) -> Result<Self> {
        let searcher = RipgrepSearcher::new(runner, executable);
        let version = searcher.probe().ok();
        Ok(Self {
            searcher,
            scope,
            version,
        })
    }

    pub fn scope(&self) -> &NowFieldScope {
        &self.scope
    }

    /// Every currently-authorised record, enumerated from the same scope the
    /// searches run over. Bodies stay on disk; the roster carries identity so
    /// the Knowledge read path can attach this provider to its sources.
    pub fn descriptors(&self) -> Vec<SourceMaterial> {
        self.authorised_files()
            .into_iter()
            .filter_map(|(relative, _family)| self.material(&relative).ok())
            .collect()
    }

    fn authorised_files(&self) -> Vec<(PathBuf, &'static str)> {
        let mut files = Vec::new();
        let mut seen = BTreeSet::new();
        'includes: for include in &self.scope.includes {
            for dir in candidate_directories(&self.scope.central_root, &include.glob) {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !is_real_file(&path) {
                        continue;
                    }
                    let Some(relative) = self.relative_to_root(&path) else {
                        continue;
                    };
                    if !self.scope.is_authorised(&relative) {
                        continue;
                    }
                    if seen.insert(relative.clone()) {
                        files.push((relative, include.family));
                        if files.len() >= MAX_ROSTER_FILES {
                            break 'includes;
                        }
                    }
                }
            }
        }
        files.sort();
        files
    }

    fn material(&self, relative: &Path) -> Result<SourceMaterial> {
        let mut absolute = self.scope.central_root.clone();
        for component in relative.components() {
            let std::path::Component::Normal(name) = component else {
                return Err(AikitError::new(
                    "now_field.source_unauthorised",
                    format!("{relative:?} is not a canonical relative NOW-field path"),
                ));
            };
            absolute.push(name);
            if std::fs::symlink_metadata(&absolute)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                return Err(AikitError::new(
                    "now_field.source_unauthorised",
                    format!("{relative:?} crosses a symbolic link"),
                ));
            }
        }
        if marker_between(&self.scope.central_root, &absolute) {
            return Err(AikitError::new(
                "now_field.source_unauthorised",
                format!("{relative:?} is withheld by an owner marker"),
            ));
        }
        let bytes = std::fs::read(&absolute).map_err(|e| {
            AikitError::new(
                "now_field.source_unreadable",
                format!("could not read {relative:?}: {e}"),
            )
        })?;
        if bytes.len() as u64 > MAX_READ_BYTES {
            return Err(AikitError::new(
                "now_field.source_too_large",
                format!("{relative:?} exceeds the NOW-field read budget"),
            ));
        }
        let media_type = match relative.extension().and_then(|e| e.to_str()) {
            Some("json") => "application/json",
            Some("md") => "text/markdown",
            Some("html") => "text/html",
            _ => "text/plain",
        };
        Ok(SourceMaterial {
            binding: SourceBinding {
                source: SourceRef::parse(format!(
                    "central:source:control:root:{}",
                    relative.to_string_lossy()
                ))?,
                revision: SourceRevision::parse(content_revision(&bytes))?,
                title: relative.to_string_lossy().into_owned(),
                tags: self.derive_tags(relative),
                // Owner-authorised ground, not an actor-independent grant:
                // eligibility came from the scope, the way the file-map owner
                // read does it.
                visibility: SourceVisibility::Personal,
                owners: vec![],
                media_type: media_type.into(),
                locator: Some(ResourceLocator::Path(absolute)),
                metadata: [
                    (
                        "now-field",
                        json!({"family_authorised": true, "marker": AGENT_RETRIEVAL_MARKER}),
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

    fn relative_to_root(&self, path: &Path) -> Option<PathBuf> {
        if let Ok(relative) = path.strip_prefix(&self.scope.central_root) {
            return Some(relative.to_path_buf());
        }
        // Searched with cwd at the root and root ".", ripgrep prints "./…".
        let raw = path.to_string_lossy();
        let stripped = raw.strip_prefix("./").unwrap_or(&raw);
        let candidate = Path::new(stripped);
        if candidate.is_relative() && !stripped.starts_with("..") {
            return Some(candidate.to_path_buf());
        }
        None
    }

    fn derive_tags(&self, relative: &Path) -> Vec<String> {
        let relative_str = relative.to_string_lossy();
        let mut tags = vec!["now-field".to_string()];
        if let Some(include) = self
            .scope
            .includes
            .iter()
            .find(|include| glob_match(&include.glob, &relative_str))
        {
            tags.push(include.family.to_string());
        }
        let mut components = relative.components();
        if components.next().and_then(|c| c.as_os_str().to_str()) == Some("Work") {
            if let Some(project) = components.next().and_then(|c| c.as_os_str().to_str()) {
                tags.push(project.to_lowercase());
            }
        }
        tags.sort();
        tags.dedup();
        tags
    }

    fn hit(&self, matched: &RipgrepMatch) -> Option<SourceHit> {
        let relative = self.relative_to_root(&matched.path)?;
        if !self.scope.is_authorised(&relative) {
            return None;
        }
        let snippet: String = matched.line.chars().take(240).collect();
        Some(SourceHit {
            source: SourceRef::parse(format!(
                "central:source:control:root:{}",
                relative.to_string_lossy()
            ))
            .ok()?,
            provider: provider(),
            score: None,
            title: relative.to_string_lossy().into_owned(),
            snippet: snippet.trim_end().to_string(),
            tags: self.derive_tags(&relative),
            provider_binding: Some(format!("line:{}", matched.line_number)),
            retrieval_mode: SourceSearchMode::Fulltext,
        })
    }

    fn run_search(
        &self,
        pattern: &str,
        regex: bool,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        if limit == 0 || pattern.is_empty() {
            return Ok(Vec::new());
        }
        // A marker may have arrived after this provider attached. Refresh the
        // owner boundary before ripgrep, which otherwise searches files before
        // `hit` can reject an unauthorised result.
        let mut live_scope = self.scope.clone();
        live_scope.prune_marked_subtrees();
        let mut excludes = live_scope.excludes.clone();
        excludes.extend(
            live_scope
                .pruned
                .iter()
                .map(|dir| format!("{}/**", escape_glob_literal(dir))),
        );
        let request = SearchRequest {
            pattern: pattern.to_string(),
            regex,
            // The NOW field answers for exact ground: case-sensitive, like
            // the records themselves.
            ignore_case: false,
            // The runner is pinned to the central root (see default_runner);
            // a relative search root is what makes root-relative globs match.
            roots: vec![PathBuf::from(".")],
            include_globs: live_scope
                .includes
                .iter()
                .map(|include| include.glob.clone())
                .collect(),
            exclude_globs: excludes,
            hidden: true,
            max_file_bytes: crate::ripgrep::DEFAULT_MAX_FILE_BYTES,
            limit,
            timeout: Some(std::time::Duration::from_secs(30)),
        };
        let outcome = self.searcher.search(&request)?;
        let required: BTreeSet<&str> = tags.iter().map(String::as_str).collect();
        // One row per document: a clearing that mentions the query on five
        // lines is one answer, not five. The first match's line binding is
        // kept on the folded hit.
        let mut seen_documents = BTreeSet::new();
        Ok(outcome
            .matches
            .iter()
            .filter_map(|matched| self.hit(matched))
            .filter(|hit| {
                required.is_empty()
                    || required.is_subset(&hit.tags.iter().map(String::as_str).collect())
            })
            .filter(|hit| seen_documents.insert(hit.source.to_string()))
            .take(limit)
            .collect())
    }

    /// The explicit-regex path. The KnowledgeApplication contract searches
    /// literal by default; a caller that wants caller-supplied syntax asks for
    /// it here, by name.
    pub fn search_regex(
        &self,
        pattern: &str,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        self.run_search(pattern, true, tags, limit)
    }
}

impl<R: CommandRunner> SourcePoolProvider for NowFieldSourcePoolProvider<R> {
    fn capabilities(&self) -> SourceProviderCapabilities {
        SourceProviderCapabilities {
            provider: provider(),
            version: self.version.clone(),
            fulltext: self.version.is_some(),
            fuzzy_interactive: false,
            semantic: false,
            hybrid: false,
            tags: true,
            structured_output: true,
            reasons: [
                (
                    "semantic",
                    "literal content search needs no embeddings; semantic retrieval stays with the semantic providers",
                ),
                (
                    "hybrid",
                    "hybrid retrieval stays with hybrid-capable providers",
                ),
                (
                    "fuzzy-interactive",
                    "the NOW field answers exact and literal questions; fuzzy ranking stays with indexed providers",
                ),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        }
    }

    /// The NOW field is live owner ground, not a disposable local index; there
    /// is nothing for AIKit to rebuild and no material to preload.
    fn rebuild(&mut self, _: &[SourceMaterial]) -> Result<()> {
        Err(AikitError::new(
            "now_field.owner_only",
            "the NOW field is live owner ground; AIKit searches it in place and cannot rebuild it",
        ))
    }

    fn read(&self, source: &SourceRef) -> Result<Option<SourceMaterial>> {
        let raw = source.as_str();
        let prefix = "central:source:control:root:";
        let Some(relative) = raw.strip_prefix(prefix) else {
            return Err(AikitError::new(
                "now_field.source_out_of_scope",
                format!("{raw} is not a root-register Central source ref"),
            ));
        };
        let relative = PathBuf::from(relative);
        if !self.scope.is_authorised(&relative) {
            return Err(AikitError::new(
                "now_field.source_unauthorised",
                format!("{relative:?} is outside the authorised NOW-field scope"),
            ));
        }
        let mut material = self.material(&relative)?;
        material.binding.source = source.clone();
        Ok(Some(material))
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
                format!("the NOW-field provider does not support {}", mode.as_str()),
            ));
        }
        self.run_search(query, false, tags, limit)
    }

    fn status(&self) -> SourceProviderStatus {
        let capabilities = self.capabilities();
        SourceProviderStatus {
            available: capabilities.fulltext || capabilities.semantic || capabilities.hybrid,
            version: capabilities.version.clone(),
            tested_version: Some(crate::ripgrep::RIPGREP_TESTED_VERSION.into()),
            version_drift: capabilities
                .version
                .as_deref()
                .is_some_and(|value| !value.contains(crate::ripgrep::RIPGREP_TESTED_VERSION)),
            capabilities,
            detail: format!(
                "live ripgrep content search over the root NOW field; {} include families; \
                 owner marker {AGENT_RETRIEVAL_MARKER:?} prunes {} subtree(s); authorisation by \
                 scope allowlist, not filesystem readability",
                self.scope.includes.len(),
                self.scope.pruned.len(),
            ),
            provider: provider(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{RecordingRunner, ScriptedRunner};

    fn scope() -> NowFieldScope {
        NowFieldScope {
            central_root: PathBuf::from("/ground"),
            includes: vec![
                ScopeInclude {
                    glob: "Control/agents/now/clearings/**/*.json".into(),
                    family: "now",
                },
                ScopeInclude {
                    glob: "Control/user/day/*/day.md".into(),
                    family: "day",
                },
                ScopeInclude {
                    glob: "Work/*/ProjectCentral/now/**/*.json".into(),
                    family: "projectcentral",
                },
            ],
            excludes: vec!["**/.git/**".into()],
            pruned: vec!["Work/Factory/ProjectCentral/now/sealed".into()],
        }
    }

    #[test]
    fn authorisation_follows_the_scope_not_filesystem_readability() {
        let scope = scope();
        assert!(scope.is_authorised(Path::new("Control/agents/now/clearings/abc/now.json")));
        assert!(scope.is_authorised(Path::new("Control/user/day/2026-09-17/day.md")));
        assert!(scope.is_authorised(Path::new(
            "Work/Factory/ProjectCentral/now/agents/handoff.json"
        )));
        assert!(!scope.is_authorised(Path::new("Control/user/private-notes.md")));
        assert!(!scope.is_authorised(Path::new("Work/Factory/.git/config")));
        assert!(!scope.is_authorised(Path::new(
            "Work/Factory/ProjectCentral/now/sealed/secret.json"
        )));
    }

    #[test]
    fn project_view_keeps_common_control_and_escapes_literal_work_name() {
        let scope = scope();
        let scoped = scope.for_project(Some("fee*box"));
        assert!(scoped.is_authorised(Path::new("Control/user/day/2026-09-17/day.md")));
        assert!(scoped.is_authorised(Path::new(
            "Work/fee*box/ProjectCentral/now/agents/handoff.json"
        )));
        assert!(!scoped.is_authorised(Path::new(
            "Work/feeeeeebox/ProjectCentral/now/agents/handoff.json"
        )));
        assert!(!scoped.is_authorised(Path::new(
            "Work/Factory/ProjectCentral/now/agents/handoff.json"
        )));
        let unknown = scope.for_project(None);
        assert!(unknown.is_authorised(Path::new("Control/user/day/2026-09-17/day.md")));
        assert!(!unknown.is_authorised(Path::new(
            "Work/Factory/ProjectCentral/now/agents/handoff.json"
        )));
    }

    #[test]
    fn glob_matching_survives_multi_byte_path_components() {
        assert!(glob_match(
            "Work/*/ProjectCentral/now/**/*.json",
            "Work/\u{1d70b}-logic/ProjectCentral/now/agents/handoff.json"
        ));
        assert!(glob_match(
            "Work/*/ProjectCentral/now/**/*.json",
            "Work/\u{03c0}/ProjectCentral/now/.archive/old.json"
        ));
    }

    #[test]
    fn glob_matching_is_component_exact() {
        assert!(glob_match(
            "Control/user/day/*/day.md",
            "Control/user/day/2026-09-17/day.md"
        ));
        assert!(!glob_match(
            "Control/user/day/*/day.md",
            "Control/user/day/2026-09-17/notes/day.md"
        ));
        assert!(glob_match(
            "Work/*/ProjectCentral/now/**/*.json",
            "Work/Factory/ProjectCentral/now/.archive/old.json"
        ));
        assert!(glob_match(
            "Control/agents/now/day/**",
            "Control/agents/now/day"
        ));
    }

    #[test]
    fn the_empty_file_revision_is_the_offset_basis() {
        assert_eq!(
            content_revision(b""),
            "central.content-fnv1a64/v1:0:cbf29ce484222325"
        );
    }

    fn scripted_match(path: &str, line_number: u64, text: &str) -> String {
        format!(
            r#"{{"type":"match","data":{{"path":{{"text":"{path}"}},"lines":{{"text":"{text}\n"}},"line_number":{line_number}}}}}"#
        )
    }

    #[test]
    fn search_returns_only_authorised_hits_with_canonical_identity() {
        let runner = ScriptedRunner::new().on(
            "--json",
            &format!(
                "{}\n{}\n{}\n{}\n",
                scripted_match(
                    "/ground/Control/agents/now/clearings/abc/now.json",
                    4,
                    "harness-profile experiment"
                ),
                scripted_match(
                    "/ground/Control/user/private-notes.md",
                    1,
                    "harness-profile experiment"
                ),
                scripted_match(
                    "/ground/Work/Factory/.git/config",
                    2,
                    "harness-profile experiment"
                ),
                scripted_match(
                    "/ground/Work/Factory/ProjectCentral/now/agents/handoff.json",
                    9,
                    "harness-profile experiment"
                ),
            ),
        );
        let provider = NowFieldSourcePoolProvider::connect(runner, "rg", scope()).unwrap();
        let hits = provider
            .search(
                "harness-profile experiment",
                SourceSearchMode::Fulltext,
                &[],
                10,
            )
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0].source.as_str(),
            "central:source:control:root:Control/agents/now/clearings/abc/now.json"
        );
        assert_eq!(hits[0].tags, vec!["now", "now-field"]);
        assert_eq!(hits[0].provider_binding.as_deref(), Some("line:4"));
        assert_eq!(
            hits[1].source.as_str(),
            "central:source:control:root:Work/Factory/ProjectCentral/now/agents/handoff.json"
        );
        assert!(hits[1].tags.contains(&"factory".to_string()));
        assert!(hits[1].tags.contains(&"projectcentral".to_string()));
        assert_eq!(hits[0].retrieval_mode, SourceSearchMode::Fulltext);
    }

    #[test]
    fn tag_terms_narrow_the_now_field_like_any_other_pool() {
        let runner = ScriptedRunner::new().on(
            "--json",
            &format!(
                "{}\n{}\n",
                scripted_match(
                    "/ground/Control/agents/now/clearings/abc/now.json",
                    4,
                    "opencode strap"
                ),
                scripted_match(
                    "/ground/Work/Factory/ProjectCentral/now/agents/handoff.json",
                    9,
                    "opencode strap"
                ),
            ),
        );
        let provider = NowFieldSourcePoolProvider::connect(runner, "rg", scope()).unwrap();
        let hits = provider
            .search(
                "opencode strap",
                SourceSearchMode::Fulltext,
                &["now".to_string()],
                10,
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].source.as_str(),
            "central:source:control:root:Control/agents/now/clearings/abc/now.json"
        );
    }

    #[test]
    fn semantic_mode_is_a_capability_refusal_not_a_silent_empty() {
        let provider =
            NowFieldSourcePoolProvider::connect(ScriptedRunner::new(), "rg", scope()).unwrap();
        let error = provider
            .search("anything", SourceSearchMode::Semantic, &[], 10)
            .unwrap_err();
        assert_eq!(error.code(), "knowledge.source_provider_capability");
    }

    #[test]
    fn rebuild_is_refused_because_the_field_is_live_ground() {
        let mut provider =
            NowFieldSourcePoolProvider::connect(ScriptedRunner::new(), "rg", scope()).unwrap();
        let error = provider.rebuild(&[]).unwrap_err();
        assert_eq!(error.code(), "now_field.owner_only");
    }

    #[test]
    fn regex_is_a_deliberate_separate_path() {
        let recorder = RecordingRunner::new(ScriptedRunner::new().on("--json", ""));
        let provider = NowFieldSourcePoolProvider::connect(&recorder, "rg", scope()).unwrap();
        provider.search_regex("now-\\d+", &[], 10).unwrap();
        let searches = |recorder: &RecordingRunner<ScriptedRunner>| {
            recorder
                .calls()
                .into_iter()
                .filter(|argv| argv.iter().any(|arg| arg == "-e"))
                .collect::<Vec<_>>()
        };
        assert!(searches(&recorder)
            .iter()
            .all(|argv| !argv.iter().any(|arg| arg == "--fixed-strings")));
        provider
            .search("plain", SourceSearchMode::Fulltext, &[], 10)
            .unwrap();
        let searches = searches(&recorder);
        let last = searches.last().expect("the literal search was recorded");
        assert!(last.iter().any(|arg| arg == "--fixed-strings"));
    }

    #[test]
    fn read_returns_owner_authorised_material_with_the_live_revision() {
        // A scripted filesystem is not possible through std::fs; the
        // real-filesystem read behaviour is covered by the integration test.
        // Here: an out-of-scope source is refused before any read is attempted.
        let provider =
            NowFieldSourcePoolProvider::connect(ScriptedRunner::new(), "rg", scope()).unwrap();
        let error = provider
            .read(
                &SourceRef::parse("central:source:control:root:Control/user/private-notes.md")
                    .unwrap(),
            )
            .unwrap_err();
        assert_eq!(error.code(), "now_field.source_unauthorised");
        let error = provider
            .read(&SourceRef::parse("source:somewhere-else").unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "now_field.source_out_of_scope");
    }

    #[test]
    fn unavailable_ripgrep_is_a_disclosed_absence_not_a_fake_available() {
        let runner = ScriptedRunner::new().failing("--version", 127, "command not found");
        let provider = NowFieldSourcePoolProvider::connect(runner, "rg", scope()).unwrap();
        assert!(!provider.status().available);
        assert!(!provider.capabilities().fulltext);
    }

    #[test]
    fn status_discloses_the_scope_and_marker_pruning() {
        let runner = ScriptedRunner::new().on("--version", "ripgrep 15.2.0");
        let provider = NowFieldSourcePoolProvider::connect(runner, "rg", scope()).unwrap();
        let status = provider.status();
        assert!(status.available);
        assert_eq!(status.version.as_deref(), Some("ripgrep 15.2.0"));
        assert!(status.detail.contains("3 include families"));
        assert!(status.detail.contains("prunes 1 subtree(s)"));
    }
}
