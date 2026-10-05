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
//! Native reads and hits retain the attached Central owner's actual Source
//! identity, including registered Project refs. Explicit independent Control
//! records retain the escaped `central:source:control:root:<path>` grammar.
//! Metadata admission never substitutes for the selected payload revision. Search is literal by default;
//! regex is a separate, deliberate method. Ripgrep does not follow symlinks;
//! direct reads also reject symlink and traversal components.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aikit_core::knowledge_source_pool::*;
use aikit_core::resource::{ProviderRef, ResourceLocator, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result};
use aikit_core::context_source::{ContextSourcePrivacy, RetrievalTarget};
use serde_json::json;

use crate::ripgrep::{RipgrepMatch, RipgrepSearcher, SearchRequest};
use crate::runner::CommandRunner;
use crate::central_file_map::CentralFileMapProvider;
use crate::runner::SystemRunner;

pub const NOW_FIELD_PROVIDER_REF: &str = "provider/source-pool/now-field";
pub const AGENT_RETRIEVAL_MARKER: &str = ".no-agent-retrieval";
/// Bound on a single live owner read through this provider.
pub const MAX_READ_BYTES: u64 = 1024 * 1024;
/// Bound on selected NOW descriptors, query candidates and retained matches.
/// Native attachment preloads no bodies or roster; current query metadata is
/// admitted separately from the complete native World's capture/parse cost.
pub const MAX_ROSTER_FILES: usize = 2048;
const MAX_PENDING_DIRECTORIES: usize = 2048;
const MAX_SEEN_STATES: usize = 4096;
const MAX_OBSERVED_ENTRIES: usize = 65536;
const MAX_RETAINED_ROUTE_BYTES: usize = 4 * 1024 * 1024;
const MAX_SINGLE_PATH_BYTES: usize = 16384;
const MAX_NATIVE_REF_BYTES: usize = 4096;

// Operation-local allocation admission, separate from the native response's
// capture/parse capacity. Counts are charged before retained clones/inserts;
// they do not measure total process memory or claim a complete World roster.
#[derive(Default)]
struct QueryCapacity {
    retained_route_bytes: usize,
    seen_states: usize,
    observed_entries: usize,
    selected_native: usize,
}

impl QueryCapacity {
    fn failure(limit: &str) -> AikitError {
        AikitError::new("now_field.source_roster_budget", "NOW query capacity exhausted before completing its selected roster")
            .with("capacity", limit).with("remaining_roster", "unknown")
    }

    fn path_size(path: &Path, member: Option<&std::ffi::OsStr>) -> Result<usize> {
        let mut bytes = path.as_os_str().as_encoded_bytes().len();
        if let Some(member) = member {
            bytes = bytes.checked_add(1).and_then(|bytes|
                bytes.checked_add(member.as_encoded_bytes().len()))
                .ok_or_else(|| Self::failure("path_bytes"))?;
        }
        if bytes > MAX_SINGLE_PATH_BYTES { return Err(Self::failure("single_path_bytes")); }
        Ok(bytes)
    }

    fn retain_bytes(&mut self, bytes: usize) -> Result<()> {
        let next = self.retained_route_bytes.checked_add(bytes)
            .filter(|next| *next <= MAX_RETAINED_ROUTE_BYTES)
            .ok_or_else(|| Self::failure("retained_route_bytes"))?;
        self.retained_route_bytes = next;
        Ok(())
    }

    fn retain_path(&mut self, path: &Path) -> Result<()> {
        self.retain_bytes(Self::path_size(path, None)?)
    }

    fn queue(&mut self, pending: &mut Vec<(PathBuf, usize)>, directory: &Path,
        member: Option<&std::ffi::OsStr>, part: usize) -> Result<()> {
        if pending.len() >= MAX_PENDING_DIRECTORIES { return Err(Self::failure("pending_directories")); }
        self.retain_bytes(Self::path_size(directory, member)?)?;
        let path = member.map_or_else(|| directory.to_path_buf(), |member| directory.join(member));
        pending.push((path, part));
        Ok(())
    }

    fn observe_entry(&mut self) -> Result<()> {
        if self.observed_entries >= MAX_OBSERVED_ENTRIES { return Err(Self::failure("observed_entries")); }
        self.observed_entries += 1;
        Ok(())
    }

    fn retain_native(&mut self, relative: &Path, source: &str) -> Result<()> {
        if self.selected_native >= MAX_ROSTER_FILES { return Err(Self::failure("selected_native_sources")); }
        if source.len() > MAX_NATIVE_REF_BYTES { return Err(Self::failure("selected_source_ref_bytes")); }
        let bytes = Self::path_size(relative, None)?.checked_add(source.len())
            .ok_or_else(|| Self::failure("retained_route_bytes"))?;
        self.retain_bytes(bytes)?;
        self.selected_native += 1;
        Ok(())
    }
}

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
    ///
    /// "Common" is narrower than the whole root register. A root clearing is
    /// one commission's working field, not a common record: its scratch and
    /// evidence trees accumulate verbatim copies of any Project's material,
    /// and a day-rollover rehearsal verifiably carried Work/Factory's
    /// `.factory/development-state.json` into an O-I-scoped reply. Raw
    /// harness session captures parked under a flow directory are records of
    /// nobody — the flow record itself is the Markdown document beside them.
    /// Project scope therefore searches the root register's record surfaces
    /// only — dated day readings, the flow records at the flows root, the
    /// human day file — plus the project's own NOW register. Nested flow
    /// event directories and day `.sources` snapshot subtrees are working
    /// material the same narrowing excludes (a nested event document on the
    /// live ground carried verifier-only canaries and sibling repository
    /// names; snapshot subtrees verifiably carry sibling NOW records). The
    /// root scope keeps the broad aperture; root-scope and explicit
    /// cross-Project reads are unaffected.
    pub fn for_project(&self, project_name: Option<&str>) -> Self {
        let mut scoped = self.clone();
        let mut narrowed = Vec::new();
        for mut include in scoped.includes.drain(..) {
            if let Some(rest) = include.glob.strip_prefix("Work/*/").map(str::to_owned) {
                if let Some(name) = project_name {
                    include.glob = format!("Work/{}/{rest}", escape_glob_literal(name));
                    narrowed.push(include);
                }
            } else if include.glob.starts_with("Control/agents/now/clearings/") {
                // A clearing is one commission's bounded working field.
                // Nothing under it is a common record; drop the family whole
                // rather than judging its contents file by file.
            } else if include.glob.starts_with("Control/agents/now/flows/") {
                // Flow records are Markdown by naming law and live at the
                // flows root (`<slug>-<date>.md`). An event directory under
                // flows/ is a commission's working container, not a record
                // surface: a nested document there carries whatever the
                // commission parked beside its machinery, so only the
                // root's own records answer a Project scope. Top-level only
                // also keeps the glob shape unambiguous for the read
                // authorisation, which shares these globs with search.
                narrowed.push(ScopeInclude {
                    glob: "Control/agents/now/flows/*.md".into(),
                    family: include.family,
                });
            } else if include.glob.starts_with("Control/agents/now/day/") {
                // A day reading is the dated record. Its `.sources`
                // snapshot subtrees keep byte-exact copies of whatever a
                // day closed over — sibling-project material included,
                // verified on the live ground — so a Project scope reads
                // the readings only.
                narrowed.push(ScopeInclude {
                    glob: "Control/agents/now/day/*.md".into(),
                    family: include.family,
                });
            } else if !include.glob.starts_with("Work/") {
                narrowed.push(include);
            }
        }
        scoped.includes = narrowed;
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
        // A trailing `**` consumes every remaining segment: `a/**` must
        // authorise the directory's descendants, not only the directory
        // itself. Ripgrep's globs — which the search pass runs under —
        // already match these files; without this branch the authorisation
        // pass silently dropped every nested day snapshot and flow file.
        return pattern[0].len() == 1 && pattern[0][0] == GlobPart::AnyDepth;
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
    native_owner: Option<Arc<CentralFileMapProvider<SystemRunner>>>,
    executable: PathBuf,
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
        let executable = executable.into();
        for (coordinate, path) in [("executable", executable.as_path()), ("root", scope.central_root.as_path())] {
            if path.to_str().is_none() {
                return Err(AikitError::new("now_field.coordinate_invalid", "NOW coordinate is not representable by the query text contract")
                    .with("coordinate", coordinate));
            }
        }
        let searcher = RipgrepSearcher::new(runner, executable.clone());
        let version = searcher.probe().ok();
        Ok(Self {
            searcher,
            scope,
            version,
            native_owner: None,
            executable,
        })
    }

    /// Attach the existing native owner instance. A known failed owner must
    /// not be replaced by the independent constructor; its caller retains the
    /// failure. Sharing this Arc preserves the original World/Project route.
    pub fn with_native_owner(mut self, owner: Arc<CentralFileMapProvider<SystemRunner>>) -> Self {
        self.native_owner = Some(owner);
        self
    }

    pub fn scope(&self) -> &NowFieldScope {
        &self.scope
    }

    /// Every currently-authorised record, enumerated from the same scope the
    /// searches run over. Bodies stay on disk; the roster carries identity so
    /// the Knowledge read path can attach this provider to its sources.
    pub fn descriptors(&self) -> Vec<SourceMaterial> {
        // Native discovery/read stay live. Preloading this roster would read
        // every unselected body and duplicate the owner's material authority.
        if self.native_owner.is_some() { return Vec::new(); }
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

    fn current_scope(&self) -> NowFieldScope {
        let mut scope = self.scope.clone();
        // `pruned` is an attachment observation. The fallible native member
        // predicate below observes current markers; an old snapshot cannot
        // permanently deny a record after the owner restores retrieval.
        scope.pruned.clear();
        scope
    }

    fn remaining(deadline: std::time::Instant) -> Result<std::time::Duration> {
        deadline.checked_duration_since(std::time::Instant::now())
            .filter(|remaining| !remaining.is_zero()).ok_or_else(||
                AikitError::new("now_field.query_budget", "NOW query exhausted its operation budget"))
    }

    fn root_basis(&self) -> Result<(PathBuf, std::fs::Metadata)> {
        let canonical = std::fs::canonicalize(&self.scope.central_root)
            .map_err(|error| Self::io_failure("could not observe the declared NOW root", error))?;
        let metadata = std::fs::metadata(&self.scope.central_root)
            .map_err(|error| Self::io_failure("could not observe the declared NOW root", error))?;
        if !metadata.is_dir() {
            return Err(AikitError::new("now_field.source_unauthorised", "The declared NOW root is not a directory"));
        }
        Ok((canonical, metadata))
    }

    fn io_failure(message: &str, error: std::io::Error) -> AikitError {
        let kind = format!("{:?}", error.kind());
        let raw_os_error = error.raw_os_error().map(|value| value.to_string()).unwrap_or_default();
        AikitError::new("now_field.source_unreadable", format!("{message}: {error}"))
            .with("cause_kind", kind).with("cause_raw_os_error", raw_os_error).with_io_source(error)
    }

    fn check_root(&self, basis: &(PathBuf, std::fs::Metadata)) -> Result<()> {
        let current = self.root_basis()?;
        #[cfg(unix)]
        let same_identity = {
            use std::os::unix::fs::MetadataExt;
            basis.1.dev() == current.1.dev() && basis.1.ino() == current.1.ino()
        };
        #[cfg(not(unix))]
        let same_identity = basis.1.is_dir() == current.1.is_dir();
        if current.0 != basis.0 || !same_identity {
            return Err(AikitError::new("now_field.source_unauthorised", "The declared NOW root lost its physical affiliation"));
        }
        Ok(())
    }

    fn member_from_owner_path(&self, path: &Path, basis: &(PathBuf, std::fs::Metadata)) -> Result<Option<PathBuf>> {
        Ok(self.borrowed_owner_member(path, basis, &self.current_scope())?.map(Path::to_path_buf))
    }

    fn borrowed_owner_member<'a>(&self, path: &'a Path, basis: &(PathBuf, std::fs::Metadata), scope: &NowFieldScope) -> Result<Option<&'a Path>> {
        self.check_root(basis)?;
        let relative = path.strip_prefix(&self.scope.central_root)
            .or_else(|_| path.strip_prefix(&basis.0)).ok();
        let Some(relative) = relative else { return Ok(None); };
        if relative.components().any(|part| !matches!(part, std::path::Component::Normal(_)))
            || !scope.is_authorised(relative)
        { return Ok(None); }
        Ok(Some(relative))
    }

    fn check_member(&self, relative: &Path) -> Result<()> {
        if relative.to_str().is_none() {
            return Err(AikitError::new("now_field.coordinate_invalid", "NOW member is not representable by the SourceRef and query text contracts")
                .with("coordinate", "member"));
        }
        if relative.components().any(|part| !matches!(part, std::path::Component::Normal(_)))
            || !self.current_scope().is_authorised(relative)
        {
            return Err(AikitError::new("now_field.source_unauthorised", "Selected member is outside the declared NOW scope"));
        }
        if !crate::projectcentral::path_agent_readability(&self.scope.central_root, relative)
            .map_err(|error| Self::io_failure("could not observe current NOW admission", error))?
        {
            return Err(AikitError::new("now_field.source_unauthorised", "Selected NOW member is withheld"));
        }
        match std::fs::symlink_metadata(self.scope.central_root.join(relative)) {
            Ok(metadata) if metadata.len() > MAX_READ_BYTES => {
                return Err(AikitError::new("now_field.source_too_large", "Selected NOW member exceeds the read budget"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Self::io_failure("could not observe the selected NOW member", error)),
        }
        Ok(())
    }

    fn read_member_for(&self, relative: &Path, target: RetrievalTarget) -> Result<SourcePoolReading> {
        if self.native_owner.is_none() && !relative.starts_with("Control") {
            return Err(AikitError::new("now_field.native_binding_unavailable", "Project NOW records require their actual native owner binding"));
        }
        self.check_member(relative)?;
        let basis = self.root_basis()?;
        if let Some(owner) = &self.native_owner {
            let (source, path) = owner.locate_source(&self.scope.central_root.join(relative))?;
            if self.member_from_owner_path(&path, &basis)?.as_deref() != Some(relative) {
                return Err(AikitError::new("now_field.source_unauthorised", "Owner located another NOW member"));
            }
            let reading = owner.read_for(&source, target)?.ok_or_else(||
                AikitError::new("now_field.native_binding_unavailable", "Native owner declined its selected Source"))?;
            self.validate_owner_reading(&reading, &source, relative, &basis)?;
            return Ok(reading);
        }
        let privacy = ContextSourcePrivacy::default();
        SourcePoolReading::check_target(privacy, target)?;
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::fs::MetadataExt;
            crate::projectcentral::publication::material_bytes_affiliated(
                &self.scope.central_root, (basis.1.dev(), basis.1.ino()), relative, MAX_READ_BYTES,
            )?
        };
        #[cfg(not(unix))]
        let bytes = crate::projectcentral::publication::material_bytes(&self.scope.central_root.join(relative), MAX_READ_BYTES)?;
        self.check_root(&basis)?;
        self.check_member(relative)?;
        let media_type = match relative.extension().and_then(|e| e.to_str()) {
            Some("json") => "application/json", Some("md") => "text/markdown", Some("html") => "text/html", _ => "text/plain",
        };
        Ok(SourcePoolReading { privacy, material: SourceMaterial {
            binding: SourceBinding {
                source: SourceRef::parse(format!("central:source:control:root:{}", Self::encode_control_member(relative)))?,
                revision: SourceRevision::parse(content_revision(&bytes))?,
                title: relative.to_string_lossy().into_owned(), tags: self.derive_tags(relative),
                visibility: SourceVisibility::Personal, owners: vec![], media_type: media_type.into(),
                locator: Some(ResourceLocator::Path(self.scope.central_root.join(relative))),
                metadata: [("now-field".to_string(), json!({"family_authorised":true,"marker":AGENT_RETRIEVAL_MARKER})),
                    ("owner_read_required".to_string(), json!(false))].into_iter().collect(),
            }, body: String::from_utf8_lossy(&bytes).into_owned(),
        }})
    }

    fn encode_control_member(relative: &Path) -> String {
        relative.to_string_lossy().replace('%', "%25").replace(':', "%3A").replace(' ', "%20")
    }

    fn decode_control_member(raw: &str) -> Option<PathBuf> {
        let relative = PathBuf::from(raw.replace("%20", " ").replace("%3A", ":").replace("%25", "%"));
        (Self::encode_control_member(&relative) == raw).then_some(relative)
    }

    fn validate_owner_reading(&self, reading: &SourcePoolReading, source: &SourceRef, relative: &Path, basis: &(PathBuf, std::fs::Metadata)) -> Result<()> {
        let Some(ResourceLocator::Path(path)) = reading.material.binding.locator.as_ref() else {
            return Err(AikitError::new("now_field.native_binding_invalid", "Native owner returned no physical member locator"));
        };
        if &reading.material.binding.source != source
            || self.member_from_owner_path(path, basis)?.as_deref() != Some(relative)
        {
            return Err(AikitError::new("now_field.native_binding_invalid", "Native owner returned another Source or NOW member"));
        }
        self.check_member(relative)?;
        if reading.material.body.len() as u64 > MAX_READ_BYTES {
            return Err(AikitError::new("now_field.source_too_large", "Native NOW payload exceeds the read budget"));
        }
        self.check_root(basis)
    }

    fn material(&self, relative: &Path) -> Result<SourceMaterial> {
        Ok(self.read_member_for(relative, RetrievalTarget::LocalAgent)?.material)
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

    fn qualified_hit(&self, matched: &RipgrepMatch, request: &SearchRequest,
        native_source: Option<&SourceRef>, deadline: std::time::Instant,
    ) -> Result<Option<SourceHit>> {
        let Some(relative) = self.relative_to_root(&matched.path) else { return Ok(None); };
        if !self.current_scope().is_authorised(&relative) { return Ok(None); }
        let basis = self.root_basis()?;
        let before = if let Some(source) = native_source {
            self.native_owner.as_ref().expect("native query has an attached owner")
                .read_for_with_timeout(source, RetrievalTarget::LocalAgent, Self::remaining(deadline)?)?
                .ok_or_else(|| AikitError::new("now_field.native_binding_unavailable", "Native owner declined the admitted Source"))?
        } else { self.read_member_for(&relative, RetrievalTarget::LocalAgent)? };
        self.validate_owner_reading(&before, &before.material.binding.source, &relative, &basis)?;
        let absolute = self.scope.central_root.join(&relative);
        let selected = SearchRequest { roots:vec![absolute.clone()], include_globs:vec![], exclude_globs:vec![],
            timeout:Some(Self::remaining(deadline)?), ..request.clone() };
        self.check_query_argv(&selected)?;
        let checked = self.searcher.search(&selected)?;
        let checked_match = checked.matches.first();
        let actual_line = matched.line_number.checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| before.material.body.split('\n').nth(index))
            .map(|line| line.trim_end_matches('\r'));
        if checked.truncated || checked_match.is_none_or(|current|
            self.relative_to_root(&current.path).as_deref() != Some(relative.as_path())
                || current.line_number != matched.line_number || current.line != matched.line)
            || actual_line != Some(matched.line.as_str())
        {
            return Err(AikitError::new("now_field.source_search_basis_conflict", "Native query no longer agrees with the selected current Source"));
        }
        let after = if let Some(source) = native_source {
            self.native_owner.as_ref().expect("native query has an attached owner")
                .read_for_with_timeout(source, RetrievalTarget::LocalAgent, Self::remaining(deadline)?)?
                .ok_or_else(|| AikitError::new("now_field.native_binding_unavailable", "Native owner declined the admitted Source"))?
        } else { self.read_member_for(&relative, RetrievalTarget::LocalAgent)? };
        self.validate_owner_reading(&after, &before.material.binding.source, &relative, &basis)?;
        if before != after {
            return Err(AikitError::new("now_field.source_search_basis_conflict", "Selected current Source changed during its native query"));
        }
        Self::remaining(deadline)?;
        Ok(Some(SourceHit {
            source: before.material.binding.source, revision:Some(before.material.binding.revision),
            provider:provider(), score:None, title:relative.to_string_lossy().into_owned(),
            snippet:matched.line.chars().take(240).collect::<String>().trim_end().to_string(),
            tags:self.derive_tags(&relative), provider_binding:Some(format!("line:{}", matched.line_number)),
            retrieval_mode:SourceSearchMode::Fulltext,
        }))
    }

    fn check_query_argv(&self, request: &SearchRequest) -> Result<()> {
        let bytes = request.argv(&self.executable).iter().map(|arg| arg.len() + 1).sum::<usize>();
        if bytes > 64 * 1024 {
            return Err(AikitError::new("now_field.query_argv_budget", "NOW query exceeds the explicit argument capacity"));
        }
        Ok(())
    }

    fn query_candidates(&self, scope: &NowFieldScope, deadline: std::time::Instant, capacity: &mut QueryCapacity) -> Result<Vec<PathBuf>> {
        let mut files = BTreeSet::new();
        for include in &scope.includes {
            let segments: Vec<_> = include.glob.split('/').collect();
            // A final ** includes nested files too; it is a traversal segment,
            // not merely a leaf filename pattern.
            let parents = if segments.last() == Some(&"**") { &segments[..] }
                else { &segments[..segments.len().saturating_sub(1)] };
            let mut pending = Vec::new();
            let mut seen = std::collections::BTreeMap::<PathBuf, BTreeSet<usize>>::new();
            capacity.queue(&mut pending, &scope.central_root, None, 0)?;
            while let Some((directory, part)) = pending.pop() {
                Self::remaining(deadline)?;
                if seen.get(&directory).is_some_and(|parts| parts.contains(&part)) { continue; }
                if capacity.seen_states >= MAX_SEEN_STATES { return Err(QueryCapacity::failure("seen_states")); }
                if !seen.contains_key(&directory) { capacity.retain_path(&directory)?; }
                capacity.seen_states += 1;
                seen.entry(directory.clone()).or_default().insert(part);
                let relative = directory.strip_prefix(&scope.central_root).expect("declared member");
                if !relative.as_os_str().is_empty() {
                    if !crate::projectcentral::path_agent_readability(&scope.central_root, relative)
                        .map_err(|error| Self::io_failure("could not observe NOW directory admission", error))?
                    { continue; }
                } else {
                    match std::fs::symlink_metadata(directory.join(AGENT_RETRIEVAL_MARKER)) {
                        Ok(_) => continue,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(Self::io_failure("could not observe NOW root marker", error)),
                    }
                }
                if part < parents.len() && parents[part] != "**"
                    && !parents[part].contains(['*', '?', '['])
                {
                    capacity.queue(&mut pending, &directory, Some(std::ffi::OsStr::new(parents[part])), part + 1)?;
                    continue;
                }
                let entries = match std::fs::read_dir(&directory) {
                    Ok(entries) => entries,
                    // An absent declared family has no records. Other IO is
                    // uncertainty, not an empty successful roster.
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(Self::io_failure("could not list the declared NOW family", error)),
                };
                if part < parents.len() && parents[part] == "**" {
                    capacity.queue(&mut pending, &directory, None, part + 1)?;
                }
                for entry in entries {
                    Self::remaining(deadline)?;
                    capacity.observe_entry()?;
                    let entry = entry.map_err(|error| Self::io_failure("could not observe a NOW directory entry", error))?;
                    let metadata = entry.file_type().map_err(|error| Self::io_failure("could not observe NOW member type", error))?;
                    let name = entry.file_name();
                    if metadata.is_dir() && part < parents.len() {
                        if parents[part] == "**" {
                            capacity.queue(&mut pending, &directory, Some(&name), part)?;
                        } else if glob_match(parents[part], &name.to_string_lossy()) {
                            capacity.queue(&mut pending, &directory, Some(&name), part + 1)?;
                        }
                    } else if metadata.is_file() && part == parents.len() {
                        QueryCapacity::path_size(&directory, Some(&name))?;
                        let path = directory.join(&name);
                        let relative = path.strip_prefix(&scope.central_root).expect("declared member");
                        if scope.is_authorised(relative) {
                            if !crate::projectcentral::path_agent_readability(&scope.central_root, relative)
                                .map_err(|error| Self::io_failure("could not observe NOW member admission", error))?
                            { continue; }
                            if !files.contains(relative) {
                                if files.len() >= MAX_ROSTER_FILES { return Err(QueryCapacity::failure("candidate_files")); }
                                capacity.retain_path(relative)?;
                                files.insert(relative.to_path_buf());
                            }
                        }
                    }
                }
            }
        }
        Ok(files.into_iter().collect())
    }

    fn run_search(&self, pattern: &str, regex: bool, tags: &[String], limit: usize) -> Result<Vec<SourceHit>> {
        if limit == 0 || pattern.is_empty() { return Ok(Vec::new()); }
        let duration = self.native_owner.as_ref().and_then(|owner| owner.configured_timeout())
            .unwrap_or(std::time::Duration::from_secs(30)).min(std::time::Duration::from_secs(30));
        let deadline = std::time::Instant::now().checked_add(duration).ok_or_else(||
            AikitError::new("now_field.query_budget", "NOW query budget could not be represented"))?;
        let live_scope = self.current_scope();
        let mut excludes = live_scope.excludes.clone();
        excludes.extend(live_scope.pruned.iter().map(|dir| format!("{}/**", escape_glob_literal(dir))));
        let request = SearchRequest {
            pattern:pattern.into(), regex, ignore_case:false, roots:vec![PathBuf::from(".")],
            include_globs:live_scope.includes.iter().filter(|include|
                self.native_owner.is_some() || include.glob.starts_with("Control/"))
                .map(|include| include.glob.clone()).collect(),
            exclude_globs:excludes, hidden:true, max_file_bytes:MAX_READ_BYTES,
            limit:MAX_ROSTER_FILES, max_count_per_file:None, timeout:Some(Self::remaining(deadline)?),
        };
        let required: BTreeSet<&str> = tags.iter().map(String::as_str).collect();
        let mut matches = Vec::new();
        let mut native_sources = std::collections::BTreeMap::new();
        let mut capacity = QueryCapacity::default();
        let basis = self.root_basis()?;
        if let Some(owner) = &self.native_owner {
            // One body-free owner roster; its metadata revisions never become
            // Source payload revisions. Native registration remains explicit.
            owner.visit_source_roster(Self::remaining(deadline)?, |source, path| {
                Self::remaining(deadline)?;
                if let Some(relative) = self.borrowed_owner_member(path, &basis, &live_scope)? {
                    if native_sources.contains_key(relative) {
                        return Err(AikitError::new("now_field.native_binding_invalid", "Owner roster has ambiguous NOW source identity"));
                    }
                    capacity.retain_native(relative, source)?;
                    native_sources.insert(relative.to_path_buf(), SourceRef::parse(source)?);
                }
                Ok(())
            })?;
        }
        let mut query_scope = live_scope.clone();
        if self.native_owner.is_none() {
            // Explicit independent Control records keep their own declared
            // relation. An optional owner does not promote Project records.
            query_scope.includes.retain(|include| include.glob.starts_with("Control/"));
        }
        let candidates = self.query_candidates(&query_scope, deadline, &mut capacity)?;
        let mut paths = Vec::new();
        for relative in candidates {
            Self::remaining(deadline)?;
            self.check_root(&basis)?;
            self.check_member(&relative)?;
            if let Some(owner) = &self.native_owner {
                if !native_sources.contains_key(&relative) {
                    // Retain the actual native missing/denied response. A new
                    // admission after the roster makes this operation stale;
                    // it does not authorise a copied or invented fallback.
                    owner.locate_source_with_timeout(&self.scope.central_root.join(&relative), Some(Self::remaining(deadline)?))?;
                    return Err(AikitError::new("now_field.source_roster_changed", "Native NOW admission changed after the metadata roster"));
                }
            }
            capacity.retain_bytes(QueryCapacity::path_size(&self.scope.central_root, Some(relative.as_os_str()))?)?;
            paths.push(self.scope.central_root.join(relative));
        }
        for chunk in paths.chunks(64) {
            self.check_root(&basis)?;
            for path in chunk {
                let relative = path.strip_prefix(&self.scope.central_root).expect("admitted member");
                self.check_member(relative)?;
            }
            let chunk_request = SearchRequest { roots:chunk.to_vec(), include_globs:vec![], exclude_globs:vec![],
                timeout:Some(Self::remaining(deadline)?), ..request.clone() };
            self.check_query_argv(&chunk_request)?;
            let outcome = self.searcher.search(&chunk_request)?;
            if outcome.truncated || matches.len() + outcome.matches.len() > MAX_ROSTER_FILES {
                return Err(AikitError::new("now_field.query_truncated", "NOW query exceeded retained match capacity"));
            }
            matches.extend(outcome.matches);
        }
        self.check_root(&basis)?;
        let mut hits = Vec::new();
        let mut seen = BTreeSet::new();
        for matched in &matches {
            let Some(relative) = self.relative_to_root(&matched.path) else { continue; };
            let derived = self.derive_tags(&relative);
            if !required.is_subset(&derived.iter().map(String::as_str).collect()) || seen.contains(&relative) { continue; }
            capacity.retain_path(&relative)?;
            seen.insert(relative.clone());
            let native = native_sources.get(&relative);
            if self.native_owner.is_some() && native.is_none() {
                return Err(AikitError::new("now_field.native_binding_invalid", "Query returned a member outside the admitted native roster"));
            }
            if let Some(hit) = self.qualified_hit(matched, &request, native, deadline)? {
                hits.push(hit);
                if hits.len() == limit { break; }
            }
        }
        Self::remaining(deadline)?;
        Ok(hits)
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
        Ok(self.read_for(source, RetrievalTarget::LocalAgent)?.map(|reading| reading.material))
    }

    fn read_for(&self, source: &SourceRef, target: RetrievalTarget) -> Result<Option<SourcePoolReading>> {
        if !source.as_str().starts_with("central:source:") { return Ok(None); }
        if let Some(owner) = &self.native_owner {
            let basis = self.root_basis()?;
            // Metadata only: decline a different family before any payload or
            // target refusal; participating native owner failures stay errors.
            let path = owner.source_path(source)?;
            let Some(relative) = self.member_from_owner_path(&path, &basis)? else { return Ok(None); };
            self.check_member(&relative)?;
            let reading = owner.read_for(source, target)?.ok_or_else(||
                AikitError::new("now_field.native_binding_unavailable", "Native owner declined its selected Source"))?;
            self.validate_owner_reading(&reading, source, &relative, &basis)?;
            return Ok(Some(reading));
        }
        let Some(raw) = source.as_str().strip_prefix("central:source:control:root:") else { return Ok(None); };
        let relative = Self::decode_control_member(raw).ok_or_else(||
            AikitError::new("now_field.source_unauthorised", "Source ref is not a canonical Control member"))?;
        // Independent operation is an explicit Control aperture, never a
        // substitute identity for a Project participating in native Central.
        if !relative.starts_with("Control") { return Ok(None); }
        Ok(Some(self.read_member_for(&relative, target)?))
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
                "live ripgrep content search over the NOW field; {} include families; \
                 owner marker {AGENT_RETRIEVAL_MARKER:?} prunes {} subtree(s); {}. \
                 Descriptor roster is bounded and omits unavailable members; it is not a complete World roster",
                self.scope.includes.len(),
                self.scope.pruned.len(),
                if self.native_owner.is_some() { "current Source identity and payload delegated to the attached native owner" }
                else { "independent declared Control records only; Project NOW records withheld without native binding" },
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
    fn project_scope_treats_clearings_and_raw_flow_captures_as_out_of_scope() {
        let mut scope = scope();
        scope.includes.push(ScopeInclude {
            glob: "Control/agents/now/flows/**".into(),
            family: "flow",
        });
        scope.includes.push(ScopeInclude {
            glob: "Control/agents/now/day/**".into(),
            family: "day",
        });
        let scoped = scope.for_project(Some("O-I"));
        // The clearing family is dropped whole: a commission's scratch and
        // evidence trees are nobody's common record.
        assert!(!scoped
            .includes
            .iter()
            .any(|include| include.glob.starts_with("Control/agents/now/clearings/")));
        // Flow records stay searchable at the flows root; anything parked
        // inside an event directory — a Markdown note included — does not,
        // because event directories are commissions' working containers.
        assert!(scoped.is_authorised(Path::new(
            "Control/agents/now/flows/incident-2026-09-25-1210.md"
        )));
        assert!(!scoped.is_authorised(Path::new(
            "Control/agents/now/flows/event-dir/record-2026-09-25-1215.md"
        )));
        assert!(!scoped.is_authorised(Path::new(
            "Control/agents/now/flows/event-dir/sessions/stream.jsonl"
        )));
        // Day readings answer; their `.sources` snapshot subtrees do not.
        assert!(scoped.is_authorised(Path::new("Control/agents/now/day/2026-09-25.md")));
        assert!(!scoped.is_authorised(Path::new(
            "Control/agents/now/day/2026-09-24.sources/agents/handoff.json"
        )));
        // The root scope keeps the broad aperture.
        assert!(scope.is_authorised(Path::new(
            "Control/agents/now/flows/event-dir/sessions/stream.jsonl"
        )));
        assert!(scope.is_authorised(Path::new(
            "Control/agents/now/clearings/abc/T/evidence/state.json"
        )));
        // An unknown project keeps the same common-record discipline.
        let unknown = scope.for_project(None);
        assert!(unknown.is_authorised(Path::new(
            "Control/agents/now/flows/incident-2026-09-25-1210.md"
        )));
        assert!(!unknown.is_authorised(Path::new("Control/agents/now/clearings/abc/now.json")));
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
        // A trailing `**` authorises the directory's descendants — rg's
        // search semantics and this authorisation pass must agree, or the
        // search finds files the read path refuses.
        assert!(glob_match(
            "Control/agents/now/day/**",
            "Control/agents/now/day/2026-09-24.md"
        ));
        assert!(glob_match(
            "Control/agents/now/day/**",
            "Control/agents/now/day/2026-09-24.sources/handoff.json"
        ));
        assert!(glob_match(
            "Control/agents/now/flows/**",
            "Control/agents/now/flows/event/sessions/stream.jsonl"
        ));
        assert!(!glob_match(
            "Control/user/day/*/day.md",
            "Control/user/day/2026-09-17/extra/notes/day.md"
        ));
    }

    #[test]
    fn the_empty_file_revision_is_the_offset_basis() {
        assert_eq!(
            content_revision(b""),
            "central.content-fnv1a64/v1:0:cbf29ce484222325"
        );
    }

    fn native_scope() -> (tempfile::TempDir, NowFieldScope) {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        let scratch = std::fs::canonicalize(scratch).unwrap();
        let temp = tempfile::Builder::new().prefix("now-native-query-").tempdir_in(scratch).unwrap();
        let mut declared = scope();
        declared.central_root = temp.path().to_path_buf();
        (temp, declared)
    }

    fn write_record(root: &Path, relative: &str, line_number: usize, text: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("{}{}\n", "\n".repeat(line_number - 1), text)).unwrap();
    }

    #[test]
    fn search_returns_only_authorised_hits_with_canonical_identity() {
        let (temp, declared) = native_scope();
        let clearing = "Control/agents/now/clearings/abc/now.json";
        let day = "Control/user/day/2026-09-17/day.md";
        write_record(temp.path(), clearing, 4, "harness-profile experiment");
        write_record(temp.path(), day, 9, "harness-profile experiment");
        write_record(temp.path(), "Control/user/private-notes.md", 1, "harness-profile experiment");
        write_record(temp.path(), "Work/Factory/.git/config", 2, "harness-profile experiment");
        write_record(temp.path(), "Work/Factory/ProjectCentral/now/agents/handoff.json", 9, "harness-profile experiment");
        let provider = NowFieldSourcePoolProvider::connect(default_runner(temp.path()), crate::ripgrep::executable(), declared).unwrap();
        let hits = provider.search("harness-profile experiment", SourceSearchMode::Fulltext, &[], 10).unwrap();
        assert_eq!(hits.len(), 2, "{hits:?}");
        let clearing_hit = hits.iter().find(|hit| hit.source.as_str() == format!("central:source:control:root:{clearing}")).unwrap();
        assert_eq!(clearing_hit.tags, vec!["now", "now-field"]);
        assert_eq!(clearing_hit.provider_binding.as_deref(), Some("line:4"));
        assert_eq!(clearing_hit.revision.as_ref(), Some(&provider.read(&clearing_hit.source).unwrap().unwrap().binding.revision));
        let day_hit = hits.iter().find(|hit| hit.source.as_str() == format!("central:source:control:root:{day}")).unwrap();
        assert!(day_hit.tags.contains(&"day".to_string()));
        assert_eq!(day_hit.provider_binding.as_deref(), Some("line:9"));
        assert_eq!(clearing_hit.retrieval_mode, SourceSearchMode::Fulltext);
        assert!(hits.iter().all(|hit| !hit.source.as_str().contains("Work/")), "unbound Project files cannot acquire Root identity");
        assert!(provider.status().detail.contains("Project NOW records withheld without native binding"));
    }

    #[test]
    fn tag_terms_narrow_the_now_field_like_any_other_pool() {
        let (temp, declared) = native_scope();
        write_record(temp.path(), "Control/agents/now/clearings/abc/now.json", 4, "opencode strap");
        write_record(temp.path(), "Control/user/day/2026-09-17/day.md", 9, "opencode strap");
        let provider = NowFieldSourcePoolProvider::connect(default_runner(temp.path()), crate::ripgrep::executable(), declared).unwrap();
        let hits = provider.search("opencode strap", SourceSearchMode::Fulltext, &["now".to_string()], 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source.as_str(), "central:source:control:root:Control/agents/now/clearings/abc/now.json");
    }

    #[test]
    fn independent_control_records_keep_canonical_escaping_current_basis_and_target_privacy() {
        let (temp, declared) = native_scope();
        let relative = "Control/agents/now/clearings/abc/name :%.json";
        write_record(temp.path(), relative, 1, "Current independently declared Control record");
        let provider = NowFieldSourcePoolProvider::connect(default_runner(temp.path()), crate::ripgrep::executable(), declared).unwrap();
        let source = SourceRef::parse("central:source:control:root:Control/agents/now/clearings/abc/name%20%3A%25.json").unwrap();
        let reading = provider.read_for(&source, RetrievalTarget::LocalAgent).unwrap().unwrap();
        assert_eq!(reading.material.body, "Current independently declared Control record\n");
        assert_eq!(reading.material.binding.source, source);
        assert_eq!(reading.material.binding.revision.as_str(), content_revision(reading.material.body.as_bytes()));
        assert_eq!(provider.read_for(&source, RetrievalTarget::ExternalProvider).unwrap_err().code(), "knowledge.source_target_withheld");
        std::fs::write(temp.path().join("Control/agents/now/.no-agent-retrieval"), "current withdrawal").unwrap();
        assert_eq!(provider.read(&source).unwrap_err().code(), "now_field.source_unauthorised");
        assert_eq!(std::fs::read_to_string(temp.path().join(relative)).unwrap(), reading.material.body);
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
        let (temp, declared) = native_scope();
        write_record(temp.path(), "Control/user/day/2026-09-17/day.md", 1, "now-123 plain");
        let recorder = RecordingRunner::new(default_runner(temp.path()));
        let provider = NowFieldSourcePoolProvider::connect(&recorder, crate::ripgrep::executable(), declared).unwrap();
        assert_eq!(provider.search_regex("now-\\d+", &[], 10).unwrap().len(), 1);
        let searches = recorder.calls().into_iter()
            .filter(|argv| argv.iter().any(|arg| arg == "-e")).collect::<Vec<_>>();
        assert!(!searches.is_empty(), "the real regex query was executed");
        assert!(searches.iter().all(|argv| !argv.iter().any(|arg| arg == "--fixed-strings")));
        assert_eq!(provider.search("plain", SourceSearchMode::Fulltext, &[], 10).unwrap().len(), 1);
        let searches = recorder.calls().into_iter()
            .filter(|argv| argv.iter().any(|arg| arg == "-e")).collect::<Vec<_>>();
        assert!(searches.last().unwrap().iter().any(|arg| arg == "--fixed-strings"));
    }

    #[test]
    fn independent_search_never_passes_a_currently_withheld_body_to_rg_and_can_restore_it() {
        let (temp, mut declared) = native_scope();
        declared.includes.push(ScopeInclude { glob:"Control/agents/now/flows/**".into(), family:"flow" });
        let private = "Control/agents/now/clearings/private/now.json";
        let public = "Control/agents/now/flows/nested/current.md";
        write_record(temp.path(), private, 1, "bounded-current-marker needle");
        write_record(temp.path(), public, 1, "bounded-current-marker needle");
        let recorder = RecordingRunner::new(default_runner(temp.path()));
        let provider = NowFieldSourcePoolProvider::connect(&recorder, crate::ripgrep::executable(), declared).unwrap();
        // Withdraw after construction; the old prune snapshot is not authority.
        let marker = temp.path().join("Control/agents/now/clearings/private/.no-agent-retrieval");
        std::fs::write(&marker, "current native floor withdrawal").unwrap();
        let hits = provider.search("bounded-current-marker needle", SourceSearchMode::Fulltext, &[], 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source.as_str(), format!("central:source:control:root:{public}"));
        let searches = recorder.calls().into_iter()
            .filter(|argv| argv.iter().any(|arg| arg == "--json")).collect::<Vec<_>>();
        assert!(!searches.is_empty());
        assert!(searches.iter().all(|argv| !argv.iter().any(|arg| arg == temp.path().join(private).to_str().unwrap())),
            "actual argv must never select the withheld body, even before hit filtering");
        std::fs::remove_file(marker).unwrap();
        let restored = provider.search("bounded-current-marker needle", SourceSearchMode::Fulltext, &[], 10).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(std::fs::read_to_string(temp.path().join(private)).unwrap(), "bounded-current-marker needle\n");
    }

    #[test]
    #[cfg(unix)]
    fn text_coordinates_refuse_before_lossy_query_probe_or_body_selection() {
        use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
        let (temp, declared) = native_scope();
        let invalid = temp.path().join(std::ffi::OsString::from_vec(b"query-\xff".to_vec()));
        let replacement = PathBuf::from(invalid.to_string_lossy().into_owned());
        std::fs::write(&replacement, "#!/bin/sh\nprintf invoked > \"$0.invoked\"\n").unwrap();
        std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o700)).unwrap();
        let error = NowFieldSourcePoolProvider::connect(default_runner(temp.path()), &invalid, declared.clone()).err().unwrap();
        assert_eq!(error.code(), "now_field.coordinate_invalid");
        assert_eq!(error.details()["coordinate"], "executable");
        let invoked = PathBuf::from(format!("{}.invoked", replacement.to_str().unwrap()));
        assert!(!invoked.exists());
        let mut invalid_world = declared.clone();
        invalid_world.central_root = temp.path().join(std::ffi::OsString::from_vec(b"world-\xff".to_vec()));
        std::fs::create_dir(PathBuf::from(invalid_world.central_root.to_string_lossy().into_owned())).unwrap();
        let error = NowFieldSourcePoolProvider::connect(default_runner(temp.path()), &replacement, invalid_world).err().unwrap();
        assert_eq!(error.details()["coordinate"], "root");
        assert!(!invoked.exists());
        let provider = NowFieldSourcePoolProvider::connect(default_runner(temp.path()), crate::ripgrep::executable(), declared).unwrap();
        let member = PathBuf::from(std::ffi::OsString::from_vec(b"Control/agents/now/clearings/member-\xff.json".to_vec()));
        assert_eq!(provider.check_member(&member).unwrap_err().details()["coordinate"], "member");
    }

    #[test]
    fn read_returns_owner_authorised_material_with_the_live_revision() {
        // No native output supplies this decision: the declared scope refuses
        // this member and declines another namespace before any body read.
        let provider = NowFieldSourcePoolProvider::connect(
            crate::runner::SystemRunner::probe(), crate::ripgrep::executable(), scope(),
        ).unwrap();
        let error = provider
            .read(
                &SourceRef::parse("central:source:control:root:Control/user/private-notes.md")
                    .unwrap(),
            )
            .unwrap_err();
        assert_eq!(error.code(), "now_field.source_unauthorised");
        assert!(provider.read(&SourceRef::parse("source:somewhere-else").unwrap()).unwrap().is_none());
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

    #[test]
    #[ignore = "explicit real filesystem capacity qualification; requires actual ripgrep"]
    fn actual_wide_frontier_refuses_before_retaining_an_unbounded_directory_queue() {
        let (temp, mut declared) = native_scope();
        declared.includes = vec![ScopeInclude { glob:"Control/agents/now/clearings/**/*.json".into(), family:"now" }];
        let parent = temp.path().join("Control/agents/now/clearings");
        std::fs::create_dir_all(&parent).unwrap();
        for index in 0..MAX_PENDING_DIRECTORIES + 1 {
            std::fs::create_dir(parent.join(format!("actual-{index:04}"))).unwrap();
        }
        let provider = NowFieldSourcePoolProvider::connect(default_runner(temp.path()), crate::ripgrep::executable(), declared).unwrap();
        assert!(provider.capabilities().fulltext, "the actual query executable is required");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let error = provider.query_candidates(provider.scope(), deadline, &mut QueryCapacity::default()).unwrap_err();
        assert_eq!(error.code(), "now_field.source_roster_budget");
        assert_eq!(error.details()["capacity"], "pending_directories");
        assert_eq!(error.details()["remaining_roster"], "unknown");
        assert_eq!(std::fs::read_dir(&parent).unwrap().count(), MAX_PENDING_DIRECTORIES + 1);
        for index in 2..MAX_PENDING_DIRECTORIES + 1 {
            std::fs::remove_dir(parent.join(format!("actual-{index:04}"))).unwrap();
        }
        write_record(temp.path(), "Control/agents/now/clearings/actual-0001/current.json", 1, "Actual bounded roster");
        write_record(temp.path(), "Control/agents/now/clearings/actual-0000/current.json", 1, "Actual bounded roster");
        let current_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let current = provider.query_candidates(provider.scope(), current_deadline, &mut QueryCapacity::default()).unwrap();
        assert_eq!(current, vec![PathBuf::from("Control/agents/now/clearings/actual-0000/current.json"),
            PathBuf::from("Control/agents/now/clearings/actual-0001/current.json")]);
        assert_eq!(provider.search("Actual bounded roster", SourceSearchMode::Fulltext, &[], 8).unwrap().len(), 2);
    }

    #[test]
    #[ignore = "explicit real filesystem entry capacity qualification; requires actual ripgrep"]
    fn actual_nonmatching_entries_cannot_be_acknowledged_as_a_complete_empty_roster() {
        let (temp, mut declared) = native_scope();
        declared.includes = vec![ScopeInclude { glob:"Control/agents/now/clearings/*.json".into(), family:"now" }];
        let parent = temp.path().join("Control/agents/now/clearings");
        std::fs::create_dir_all(&parent).unwrap();
        for index in 0..MAX_OBSERVED_ENTRIES + 1 {
            std::fs::write(parent.join(format!("unselected-{index:05}.txt")), b"retained actual unselected material").unwrap();
        }
        let provider = NowFieldSourcePoolProvider::connect(default_runner(temp.path()), crate::ripgrep::executable(), declared).unwrap();
        assert!(provider.capabilities().fulltext, "the actual query executable is required");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let error = provider.query_candidates(provider.scope(), deadline, &mut QueryCapacity::default()).unwrap_err();
        assert_eq!(error.code(), "now_field.source_roster_budget");
        assert_eq!(error.details()["capacity"], "observed_entries");
        assert_eq!(error.details()["remaining_roster"], "unknown");
        assert_eq!(std::fs::read_dir(&parent).unwrap().count(), MAX_OBSERVED_ENTRIES + 1);
        assert_eq!(std::fs::read(parent.join("unselected-00000.txt")).unwrap(), b"retained actual unselected material");
    }

    #[test]
    #[ignore = "explicit real filesystem retained-route capacity qualification; requires actual ripgrep"]
    fn actual_long_legal_routes_are_charged_before_retained_clones() {
        let (temp, mut declared) = native_scope();
        let actual_limit = |name: &str| {
            let mut command = std::process::Command::new("/usr/bin/getconf");
            command.arg(name).arg(temp.path());
            let output = SystemRunner::new().with_timeout(std::time::Duration::from_secs(2))
                .with_strict_utf8().capture_command(&mut command).unwrap();
            assert!(output.ok(), "actual {name} observation failed: {}", output.stderr);
            output.stdout.trim().parse::<usize>().expect("this native fixture requires a finite observed path/component limit")
        };
        let path_max = actual_limit("PATH_MAX");
        let name_max = actual_limit("NAME_MAX");
        assert!(name_max >= 100, "actual component capacity is too small for the qualified geometry");
        let target = path_max.checked_sub(96).unwrap().min(768);
        let mut relative = PathBuf::from("Control/agents/now/clearings");
        loop {
            let length = temp.path().join(&relative).as_os_str().as_encoded_bytes().len();
            if length + 1 >= target { break; }
            relative.push("p".repeat((target - length - 1).min(100)));
        }
        let parent = temp.path().join(&relative);
        let branch_count = 1700;
        let first_directory = parent.join("actual-0000");
        let first_file = first_directory.join("current.json");
        assert!(first_file.as_os_str().as_encoded_bytes().len() + 1 <= path_max,
            "the actual owned fixture path must fit PATH_MAX before creation");
        let branch_bytes = 3 * first_directory.as_os_str().as_encoded_bytes().len()
            + first_file.strip_prefix(temp.path()).unwrap().as_os_str().as_encoded_bytes().len();
        assert!(branch_count * branch_bytes > MAX_RETAINED_ROUTE_BYTES,
            "actual legal route geometry must exceed the unchanged production byte budget");
        assert!(branch_count + 1 < MAX_PENDING_DIRECTORIES && branch_count < MAX_ROSTER_FILES);
        assert!(2 * branch_count + relative.components().count() + 2 < MAX_SEEN_STATES);
        assert!(4 * branch_count < MAX_OBSERVED_ENTRIES);
        std::fs::create_dir_all(&parent).unwrap();
        for index in 0..branch_count {
            let directory = parent.join(format!("actual-{index:04}"));
            std::fs::create_dir(&directory).unwrap();
            std::fs::write(directory.join("current.json"), b"retained actual record").unwrap();
        }
        declared.includes = vec![ScopeInclude { glob:format!("{}/**/*.json", relative.to_str().unwrap()), family:"now" }];
        let provider = NowFieldSourcePoolProvider::connect(default_runner(temp.path()), crate::ripgrep::executable(), declared).unwrap();
        assert!(provider.capabilities().fulltext, "the actual query executable is required");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let error = provider.query_candidates(provider.scope(), deadline, &mut QueryCapacity::default()).unwrap_err();
        assert_eq!(error.code(), "now_field.source_roster_budget");
        assert_eq!(error.details()["capacity"], "retained_route_bytes");
        assert_eq!(error.details()["remaining_roster"], "unknown");
        assert_eq!(std::fs::read(parent.join("actual-0000/current.json")).unwrap(), b"retained actual record");
        assert_eq!(std::fs::read_dir(parent).unwrap().count(), branch_count);
    }
}
