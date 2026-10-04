//! Live Work-file search/read around exact native Project identities.
//!
//! Native discovery observes the maintained ProjectCentral declaration through
//! the same bounded held-descriptor reader as selected material. An attachment
//! retains its complete declaration byte basis and physical root affiliation;
//! each query/read checks them again. Identical-byte atomic declaration
//! replacement is admitted; changed bytes or root mapping requires a fresh
//! attachment. Explicit manifest-free roots remain independently useful.
//!
//! Fresh addresses use `source:work-file:v1:<base64url-id>:<base64url-member>`.
//! Both components retain exact UTF-8 spelling. This adapter language is
//! separate from the native Project-root Source role. Legacy delimiter-based
//! file refs are retained history, never guessed live routing from survivors.
//!
//! Search still delegates to ripgrep. Its existing ranking/limits, ignore
//! behavior and marker-glob snapshot remain; selected material additionally
//! uses current native aperture/physical admission. The depth-eight marker
//! snapshot and broad rg traversal do not establish atomic current exclusion
//! of every file before rg processing, or total memory/horizon completeness.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use aikit_core::knowledge_source_pool::*;
use aikit_core::resource::{ProviderRef, ResourceLocator, SourceRef, SourceRevision};
use aikit_core::{AikitError, Result, NO_AGENT_RETRIEVAL_MARKER};
use serde_json::json;

use crate::now_field::content_revision;
use crate::ripgrep::{RipgrepSearcher, SearchRequest, RIPGREP_TESTED_VERSION};
use crate::runner::CommandRunner;

pub const WORK_REPOS_PROVIDER_REF: &str = "provider/source-pool/work-repos";

/// Mechanical address allowance; this does not constrain native Project meaning.
pub const WORK_FILE_ADDRESS_MAX_BYTES: usize = 16 * 1024;
const WORK_FILE_ADDRESS_PREFIX: &str = "source:work-file:v1:";

/// The exact adapter transport coordinates around an existing Project/member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkFileAddress {
    pub project_id: String,
    pub member: PathBuf,
}

fn address_failure(code: &'static str, message: &str) -> AikitError {
    AikitError::new(code, message)
}

fn safe_work_member(member: &Path) -> Result<&str> {
    let text = member.to_str().ok_or_else(|| address_failure(
        "work_repos.member_transport_unsupported", "The Work member is not representable as exact UTF-8"))?;
    if text.is_empty() || text.contains('\0') || !member.components().all(|part| matches!(part, Component::Normal(_))) {
        return Err(address_failure("work_repos.source_escape", "A Work file requires a nonempty safe relative native member"));
    }
    Ok(text)
}

pub fn work_file_source_ref(project_id: &str, member: &Path) -> Result<SourceRef> {
    use base64::Engine as _;
    let member = safe_work_member(member)?;
    let encoded_id = base64::encoded_len(project_id.len(), false);
    let encoded_member = base64::encoded_len(member.len(), false);
    let length = encoded_id.zip(encoded_member)
        .and_then(|(id, member)| WORK_FILE_ADDRESS_PREFIX.len().checked_add(id)?.checked_add(1)?.checked_add(member));
    if !length.is_some_and(|length| length <= WORK_FILE_ADDRESS_MAX_BYTES) {
        return Err(address_failure("work_repos.address_budget", "The exact Work-file address exceeds its 16KiB transport allowance"));
    }
    aikit_core::ProjectRef::parse(project_id).map_err(|error| {
        address_failure("work_repos.project_transport_unsupported", "The Project ID cannot be represented by the existing AIKit ProjectRef profile")
            .with("profile_error", error.code())
    })?;
    SourceRef::parse(format!("{WORK_FILE_ADDRESS_PREFIX}{}:{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(project_id.as_bytes()),
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(member.as_bytes())))
}

pub fn decode_work_file_source_ref(source: &SourceRef) -> Result<Option<WorkFileAddress>> {
    use base64::Engine as _;
    let raw = source.as_str();
    let Some(components) = raw.strip_prefix(WORK_FILE_ADDRESS_PREFIX) else { return Ok(None); };
    if raw.len() > WORK_FILE_ADDRESS_MAX_BYTES {
        return Err(address_failure("work_repos.address_budget", "The Work-file address exceeds its 16KiB transport allowance"));
    }
    let Some((id, member)) = components.split_once(':') else {
        return Err(address_failure("work_repos.source_address_invalid", "The Work-file address has no member component"));
    };
    let decode = |encoded: &str| -> Result<String> {
        if encoded.is_empty() || encoded.contains(':') {
            return Err(address_failure("work_repos.source_address_invalid", "The Work-file address requires exactly two nonempty components"));
        }
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
            address_failure("work_repos.source_address_invalid", "The Work-file component is not unpadded URL-safe base64")
        })?;
        if base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes) != encoded {
            return Err(address_failure("work_repos.source_address_invalid", "The Work-file component is not canonically encoded"));
        }
        String::from_utf8(bytes).map_err(|_| address_failure(
            "work_repos.source_address_invalid", "The Work-file component is not exact UTF-8"))
    };
    let project_id = decode(id)?;
    aikit_core::ProjectRef::parse(&project_id).map_err(|error| {
        address_failure("work_repos.project_transport_unsupported", "The literal Work Project ID is unsupported by the existing consumer profile")
            .with("profile_error", error.code())
    })?;
    let member = PathBuf::from(decode(member)?);
    safe_work_member(&member)?;
    Ok(Some(WorkFileAddress { project_id, member }))
}


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

/// Matching lines retained per file in the match pass. Scoring saturates at
/// sixteen lines of mass, so more lines from one file only spend the budget
/// other files need.
const PER_FILE_MATCH_CAP: u64 = 16;

/// Words that carry no retrieval evidence on their own. They match nearly
/// every file, so as alternation terms they only spend the match budget.
const STOP_WORDS: [&str; 48] = [
    "a", "about", "after", "all", "an", "and", "any", "are", "as", "at", "be", "before", "but",
    "by", "can", "do", "does", "for", "from", "has", "have", "how", "if", "in", "into", "is", "it",
    "its", "not", "of", "on", "or", "should", "so", "that", "the", "their", "then", "there",
    "this", "to", "was", "what", "when", "which", "why", "with", "without",
];

/// Most distinct terms one query sends to ripgrep.
const MAX_QUERY_TERMS: usize = 12;

/// The query's evidence-bearing terms: whitespace-split, case-folded for
/// de-duplication, with stop words and one- or two-character fragments
/// dropped. A query made only of such words keeps its original terms, so a
/// deliberate short query still answers.
pub fn query_terms(query: &str) -> Vec<String> {
    let raw: Vec<String> = query
        .split_whitespace()
        .map(|term| {
            term.trim_matches(|c: char| {
                !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '#' | ':'))
            })
            .trim_end_matches(['.', ':'])
            .to_owned()
        })
        .filter(|term| !term.is_empty())
        .collect();
    let mut seen = BTreeSet::new();
    let specific: Vec<String> = raw
        .iter()
        .filter(|term| {
            term.chars().count() >= 3 && !STOP_WORDS.contains(&term.to_lowercase().as_str())
        })
        .filter(|term| seen.insert(term.to_lowercase()))
        .take(MAX_QUERY_TERMS)
        .cloned()
        .collect();
    if specific.is_empty() {
        let mut seen = BTreeSet::new();
        return raw
            .into_iter()
            .filter(|term| seen.insert(term.to_lowercase()))
            .take(MAX_QUERY_TERMS)
            .collect();
    }
    specific
}

struct FileEvidence {
    terms: BTreeSet<usize>,
    lines: usize,
    snippet: Option<String>,
    first_line: u64,
    matched_lines: Vec<(u64, String)>,
}

/// Rarity weight per term across the files that matched: `ln(1 + N / df)`.
/// A term no file carried keeps the weight of the rarest observed term, so a
/// file is never credited for it and a query of unmatched terms scores zero.
fn term_weights(terms: usize, files: &BTreeMap<PathBuf, FileEvidence>) -> Vec<f64> {
    let total = files.len().max(1) as f64;
    let mut frequency = vec![0usize; terms];
    for evidence in files.values() {
        for index in &evidence.terms {
            if let Some(slot) = frequency.get_mut(*index) {
                *slot += 1;
            }
        }
    }
    let rarest = (1.0 + total).ln();
    frequency
        .into_iter()
        .map(|df| {
            if df == 0 {
                rarest
            } else {
                (1.0 + total / df as f64).ln()
            }
        })
        .collect()
}

/// First-class content types swept per repo (addendum A-7). Per-repo result
/// limits and the searcher's `--max-filesize` budget keep json-heavy and
/// noise repos bounded.
pub const WORK_REPOS_TYPES: [&str; 14] = [
    "md", "rs", "ts", "tsx", "js", "mjs", "c", "h", "py", "sh", "toml", "json", "yml", "html",
];

/// Directories never swept, whatever the gitignore says (mirrors the
/// discovery walk's ignore list, for repos without a gitignore of their own).
const IGNORED_DIR_NAMES: [&str; 5] = [".git", "target", "node_modules", ".next", "dist"];

/// One project discovered from the Work/ manifests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkRepoProject {
    /// The Work folder name (`Work/<name>`), used for display and scoping.
    pub name: String,
    /// The manifest's own `project_id` — the identity refs are minted under.
    pub project_id: String,
    /// Absolute project root.
    pub root: PathBuf,
}

/// One discovery outcome: a usable project, or a named absence for a folder
/// that carries a manifest discovery cannot honour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkProjectEntry {
    Project(WorkRepoProject),
    Absence { name: String, reason: String },
}

/// A retained physical reading relation. It carries no semantic identity or grant.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkRoot {
    requested: PathBuf,
    canonical: PathBuf,
    identity: (u64, u64),
}

fn work_io(path: &Path, error: std::io::Error) -> AikitError {
    AikitError::new("work_repos.source_unavailable", format!("could not observe {}: {error}", path.display()))
        .with("path", path.display().to_string())
        .with("cause_kind", format!("{:?}", error.kind()))
        .with("cause_raw_os_error", error.raw_os_error().map(|value| value.to_string()).unwrap_or_default())
        .with_io_source(error)
}

fn work_root_identity(path: &Path) -> Result<(u64, u64)> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::metadata(path).map_err(|error| work_io(path, error))?;
        if !metadata.is_dir() {
            return Err(AikitError::new("work_repos.source_binding_changed", "The declared Work root is not a directory"));
        }
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = path;
        Err(AikitError::new("work_repos.physical_unsupported", "Retained Work root observation is unavailable on this platform"))
    }
}

impl WorkRoot {
    fn inspect(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(AikitError::new("work_repos.root_transport_unsupported", "A Work attachment requires its actual absolute invocation root"));
        }
        let identity = work_root_identity(path)?;
        let canonical = std::fs::canonicalize(path).map_err(|error| work_io(path, error))?;
        let root = Self { requested: path.to_path_buf(), canonical, identity };
        root.check()?;
        Ok(root)
    }

    fn check(&self) -> Result<()> {
        let current = work_root_identity(&self.requested)?;
        let canonical = std::fs::canonicalize(&self.requested).map_err(|error| work_io(&self.requested, error))?;
        if current != self.identity || canonical != self.canonical {
            return Err(AikitError::new("work_repos.source_binding_changed", "The original Work root no longer names its admitted physical directory; attach afresh")
                .with("root", self.requested.display().to_string()));
        }
        Ok(())
    }

    fn member(&self, path: &Path) -> Result<PathBuf> {
        self.check()?;
        let current = std::fs::canonicalize(path).map_err(|error| work_io(path, error))?;
        let member = current.strip_prefix(&self.canonical).map_err(|_| AikitError::new(
            "work_repos.source_escape", "The source's current physical member escapes its admitted Work root"))?;
        safe_work_member(member)?;
        self.check()?;
        Ok(member.to_path_buf())
    }

    fn readable(&self, member: &Path) -> Result<()> {
        self.check()?;
        let readable = crate::projectcentral::path_agent_readability(&self.requested, member)
            .map_err(|error| work_io(&self.requested.join(member), error))?;
        self.check()?;
        if !readable {
            return Err(AikitError::new("work_repos.source_unauthorised", "The current native source aperture withholds this Work member"));
        }
        Ok(())
    }

    fn read(&self, declared: &Path, admitted_member: &Path, max_bytes: u64) -> Result<Vec<u8>> {
        self.readable(declared)?;
        if self.member(&self.requested.join(declared))? != admitted_member {
            return Err(AikitError::new("work_repos.source_binding_changed", "The declared member no longer maps to its admitted physical member"));
        }
        let bytes = crate::projectcentral::publication::material_bytes_affiliated(
            &self.requested, self.identity, admitted_member, max_bytes).map_err(|error| {
                let code = if error.code() == "knowledge.wiki_publication_budget" {
                    "work_repos.source_too_large"
                } else { "work_repos.source_unreadable" };
                let mut projected = AikitError::new(code, error.message())
                    .with("original_code", error.code())
                    .with("physical_error", json!({"code":error.code(), "message":error.message(), "details":error.details()}).to_string())
                    .with_io_source_from(&error);
                for (key, value) in error.details() { projected = projected.with(key, value); }
                projected
            })?;
        self.readable(declared)?;
        if self.member(&self.requested.join(declared))? != admitted_member {
            return Err(AikitError::new("work_repos.source_binding_changed", "The member mapping changed during its current source observation"));
        }
        Ok(bytes)
    }
}

const WORK_MANIFEST_MEMBER: &str = "ProjectCentral/project.json";
const WORK_MANIFEST_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Absence is only admitted through an observed ordinary parent (or an
/// actually absent direct ProjectCentral member). A dangling/foreign parent
/// cannot turn failed native observation into an explicit independent root.
fn native_manifest_absent(root: &WorkRoot) -> Result<bool> {
    root.check()?;
    let parent_path = root.requested.join("ProjectCentral");
    let parent = match std::fs::symlink_metadata(&parent_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => { root.check()?; return Ok(true); }
        Err(error) => return Err(work_io(&parent_path,error)),
        Ok(metadata) if metadata.is_dir() => WorkRoot::inspect(&parent_path)?,
        Ok(_) => return Err(AikitError::new("work_repos.native_declaration_unavailable",
            "An unavailable or nonordinary native declaration parent is not independent attachment absence")),
    };
    let manifest = root.requested.join(WORK_MANIFEST_MEMBER);
    let absent = match std::fs::symlink_metadata(&manifest) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => return Err(work_io(&manifest,error)),
        Ok(_) => false,
    };
    parent.check()?;
    root.check()?;
    Ok(absent)
}

/// Native discovery retains its actual declaration separately from a checkout.
/// Fields are private so a plain display/ID/root tuple cannot invent this witness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeWorkRepoProject {
    project: WorkRepoProject,
    declaration_root: WorkRoot,
    material_root: WorkRoot,
    manifest_member: PathBuf,
    manifest_basis: String,
    enclosing_world: Option<WorkRoot>,
}

impl NativeWorkRepoProject {
    pub fn project(&self) -> &WorkRepoProject { &self.project }

    pub fn inspect(project_root: &Path, display_name: &str, enclosing_world: Option<&Path>) -> Result<Self> {
        let declaration_root = WorkRoot::inspect(project_root)?;
        let enclosing_world = enclosing_world.map(WorkRoot::inspect).transpose()?;
        if let Some(world) = &enclosing_world {
            world.readable(project_root.strip_prefix(&world.requested).map_err(|_| {
                AikitError::new("work_repos.source_escape", "Native Work discovery is outside its supplied lexical World aperture")
            })?)?;
        }
        let declared = Path::new(WORK_MANIFEST_MEMBER);
        declaration_root.readable(declared)?;
        let manifest_member = declaration_root.member(&project_root.join(declared))?;
        let bytes = declaration_root.read(declared, &manifest_member, WORK_MANIFEST_MAX_BYTES)?;
        let project_id = crate::projectcentral::project_ref_from_manifest_bytes(&bytes)?.to_string();
        work_file_source_ref(&project_id, Path::new("address-profile"))?;
        let native = Self {
            project: WorkRepoProject { name: display_name.to_owned(), project_id, root: project_root.to_path_buf() },
            material_root: declaration_root.clone(), declaration_root, manifest_member,
            manifest_basis: crate::projectcentral::publication::content_hash(&bytes), enclosing_world,
        };
        native.check()?;
        Ok(native)
    }

    pub fn with_checkout(&self, checkout: &Path) -> Result<Self> {
        self.check()?;
        let mut bound = self.clone();
        bound.material_root = WorkRoot::inspect(checkout)?;
        bound.project.root = checkout.to_path_buf();
        bound.check()?;
        Ok(bound)
    }

    fn check(&self) -> Result<()> {
        self.declaration_root.check()?;
        self.material_root.check()?;
        if let Some(world) = &self.enclosing_world {
            let member = self.declaration_root.requested.strip_prefix(&world.requested).map_err(|_| {
                AikitError::new("work_repos.source_escape", "The declaration lost its original World-relative route")
            })?;
            world.readable(member)?;
            if let Ok(member) = self.material_root.requested.strip_prefix(&world.requested) {
                world.readable(member)?;
            }
        }
        let bytes = self.declaration_root.read(Path::new(WORK_MANIFEST_MEMBER), &self.manifest_member, WORK_MANIFEST_MAX_BYTES)?;
        let current = crate::projectcentral::project_ref_from_manifest_bytes(&bytes)?;
        if current.as_str() != self.project.project_id
            || crate::projectcentral::publication::content_hash(&bytes) != self.manifest_basis
        {
            return Err(AikitError::new("work_repos.native_declaration_changed", "The retained native Project declaration changed; attach through fresh discovery")
                .with("project_id", self.project.project_id.clone()));
        }
        self.declaration_root.check()?;
        self.material_root.check()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeWorkProjectEntry {
    Project(NativeWorkRepoProject),
    Absence { name: String, error: AikitError },
}

impl NativeWorkProjectEntry {
    fn name(&self) -> &str {
        match self { Self::Project(project) => &project.project.name, Self::Absence { name, .. } => name }
    }
}

/// Native discovery uses held ordinary manifest observation; it never mints a
/// display-name identity on missing, corrupt or unreadable declarations.
pub fn discover_native_work_projects(central_root: &Path) -> Result<Vec<NativeWorkProjectEntry>> {
    let work = central_root.join("Work");
    let entries = std::fs::read_dir(&work).map_err(|error| work_io(&work, error))?;
    let mut outcomes = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| work_io(&work, error))?;
        let name = entry.file_name().into_string().map_err(|_| AikitError::new(
            "work_repos.member_transport_unsupported", "A Work display name is not exact UTF-8"))?;
        let root = entry.path();
        let metadata = std::fs::metadata(&root).map_err(|error| work_io(&root, error))?;
        if !metadata.is_dir() { continue; }
        let manifest = root.join(WORK_MANIFEST_MEMBER);
        match std::fs::symlink_metadata(&manifest) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match WorkRoot::inspect(&root).and_then(|root| native_manifest_absent(&root)) {
                    Ok(true) => continue,
                    Ok(false) => outcomes.push(NativeWorkProjectEntry::Absence { name, error: AikitError::new(
                        "work_repos.native_declaration_changed", "The declaration appeared during discovery; discover afresh") }),
                    Err(error) => outcomes.push(NativeWorkProjectEntry::Absence { name,error }),
                }
            }
            Err(error) => outcomes.push(NativeWorkProjectEntry::Absence { name, error: work_io(&manifest, error) }),
            Ok(_) => match NativeWorkRepoProject::inspect(&root, &name, Some(central_root)) {
                Ok(project) => outcomes.push(NativeWorkProjectEntry::Project(project)),
                Err(error) => outcomes.push(NativeWorkProjectEntry::Absence { name, error }),
            },
        }
    }
    outcomes.sort_by(|a,b| a.name().cmp(b.name()));
    Ok(outcomes)
}

/// Compatibility projection of the same scanner. This cannot retain typed IO.
pub fn discover_work_projects(central_root: &Path) -> Vec<WorkProjectEntry> {
    match discover_native_work_projects(central_root) {
        Ok(entries) => entries.into_iter().map(|entry| match entry {
            NativeWorkProjectEntry::Project(project) => WorkProjectEntry::Project(project.project),
            NativeWorkProjectEntry::Absence { name, error } => WorkProjectEntry::Absence { name, reason: error.to_string() },
        }).collect(),
        Err(error) => vec![WorkProjectEntry::Absence { name: "(Work)".into(), reason: error.to_string() }],
    }
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

#[derive(Debug, Clone)]
enum WorkAttachmentBasis { Native(NativeWorkRepoProject), Explicit }

#[derive(Debug, Clone)]
struct WorkAttachment {
    project: WorkRepoProject,
    root: WorkRoot,
    basis: WorkAttachmentBasis,
}

impl WorkAttachment {
    fn native(project: NativeWorkRepoProject) -> Result<Self> {
        project.check()?;
        Ok(Self { project: project.project.clone(), root: project.material_root.clone(), basis: WorkAttachmentBasis::Native(project) })
    }

    fn explicit(project: WorkRepoProject) -> Result<Self> {
        work_file_source_ref(&project.project_id, Path::new("address-profile"))?;
        let root = WorkRoot::inspect(&project.root)?;
        let manifest = project.root.join(WORK_MANIFEST_MEMBER);
        match std::fs::symlink_metadata(&manifest) {
            Ok(_) => {
                let native = NativeWorkRepoProject::inspect(&project.root, &project.name, None)?;
                if native.project.project_id != project.project_id {
                    return Err(AikitError::new("work_repos.native_declaration_changed", "The supplied Project ID differs from its actual native declaration"));
                }
                Self::native(native)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !native_manifest_absent(&root)? {
                    return Err(AikitError::new("work_repos.native_declaration_changed", "A native declaration appeared during attachment; attach afresh"));
                }
                Ok(Self { project, root, basis: WorkAttachmentBasis::Explicit })
            }
            Err(error) => Err(work_io(&manifest, error)),
        }
    }

    fn check(&self) -> Result<()> {
        self.root.check()?;
        match &self.basis {
            WorkAttachmentBasis::Native(native) => native.check(),
            WorkAttachmentBasis::Explicit => {
                let manifest = self.project.root.join(WORK_MANIFEST_MEMBER);
                match std::fs::symlink_metadata(&manifest) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if native_manifest_absent(&self.root)? { Ok(()) } else {
                            Err(AikitError::new("work_repos.native_declaration_changed", "The native declaration appeared during current observation; attach afresh"))
                        }
                    },
                    Err(error) => Err(work_io(&manifest, error)),
                    Ok(_) => {
                        NativeWorkRepoProject::inspect(&self.project.root, &self.project.name, None)?;
                        Err(AikitError::new("work_repos.native_declaration_changed", "A native declaration appeared after independent attachment; attach afresh"))
                    }
                }
            }
        }
    }
}

fn unique_work_projects(projects: &[WorkRepoProject]) -> Result<()> {
    let mut seen = BTreeMap::new();
    for project in projects {
        work_file_source_ref(&project.project_id, Path::new("address-profile"))?;
        if let Some(previous) = seen.insert(project.project_id.as_str(), project) {
            if previous != project {
                return Err(AikitError::new("work_repos.project_binding_conflict", "One literal Project ID has conflicting Work root/display attachments")
                    .with("project_id", project.project_id.clone()));
            }
        }
    }
    Ok(())
}

pub struct WorkReposSourcePoolProvider<R> {
    searcher: RipgrepSearcher<R>,
    projects: Vec<WorkRepoProject>,
    attachments: Vec<WorkAttachment>,
    attachment_error: Option<AikitError>,
    marker_globs: BTreeMap<String, Vec<String>>,
    version: Option<String>,
}

impl<R: CommandRunner> WorkReposSourcePoolProvider<R> {
    /// Explicit roots remain useful without optional native providers.
    pub fn connect(runner: R, executable: impl Into<PathBuf>, projects: Vec<WorkRepoProject>) -> Self {
        let admitted = unique_work_projects(&projects).and_then(|()| {
            let mut attachments = Vec::new();
            for project in &projects {
                if attachments.iter().any(|attached: &WorkAttachment| attached.project == *project) { continue; }
                attachments.push(WorkAttachment::explicit(project.clone())?);
            }
            Ok(attachments)
        });
        match admitted {
            Ok(attachments) => Self::connected(runner, executable, attachments, None),
            Err(error) => {
                let mut failed = Self::connected(runner, executable, Vec::new(), Some(error));
                failed.projects = projects;
                failed
            }
        }
    }

    pub fn connect_native(runner: R, executable: impl Into<PathBuf>, projects: Vec<NativeWorkRepoProject>) -> Result<Self> {
        let declared: Vec<_> = projects.iter().map(|project| project.project.clone()).collect();
        unique_work_projects(&declared)?;
        let mut attachments = Vec::new();
        for project in projects {
            if attachments.iter().any(|attached: &WorkAttachment| attached.project == project.project) { continue; }
            attachments.push(WorkAttachment::native(project)?);
        }
        Ok(Self::connected(runner, executable, attachments, None))
    }

    fn connected(runner: R, executable: impl Into<PathBuf>, attachments: Vec<WorkAttachment>, attachment_error: Option<AikitError>) -> Self {
        let projects: Vec<_> = attachments.iter().map(|attached| attached.project.clone()).collect();
        let marker_globs = projects.iter().map(|project| (project.name.clone(), marker_exclude_globs(&project.root))).collect();
        let searcher = RipgrepSearcher::new(runner, executable);
        let version = if attachment_error.is_none() { searcher.probe().ok() } else { None };
        Self { searcher, projects, attachments, attachment_error, marker_globs, version }
    }

    pub fn projects(&self) -> &[WorkRepoProject] { &self.projects }

    fn attachment(&self, project_id: &str) -> Result<Option<&WorkAttachment>> {
        if let Some(error) = &self.attachment_error { return Err(error.clone()); }
        Ok(self.attachments.iter().find(|attached| attached.project.project_id == project_id))
    }

    fn require_project(&self, project: &WorkRepoProject) -> Result<&WorkAttachment> {
        let attachment = self.attachment(&project.project_id)?.ok_or_else(|| {
            AikitError::new("work_repos.source_out_of_scope", "The literal Project is not attached to this Work provider")
        })?;
        if attachment.project != *project {
            return Err(AikitError::new("work_repos.project_binding_conflict", "The Project differs from its retained Work attachment"));
        }
        attachment.check()?;
        Ok(attachment)
    }

    pub fn for_project(&self, project_id: &str) -> Result<impl SourcePoolProvider + '_> {
        let attachment = self.attachment(project_id)?.ok_or_else(|| {
            AikitError::new("work_repos.source_out_of_scope", "The scoped Project is not attached to this Work provider")
        })?;
        attachment.check()?;
        Ok(WorkProjectView { owner: self, project: &attachment.project })
    }

    fn source_ref(project: &WorkRepoProject, relative: &Path) -> Result<SourceRef> {
        work_file_source_ref(&project.project_id, relative)
    }

    fn resolve_ref(&self, source: &SourceRef) -> Result<(&WorkAttachment, PathBuf)> {
        let address = decode_work_file_source_ref(source)?.ok_or_else(|| {
            AikitError::new("work_repos.source_out_of_scope", "The Source is not a fresh Work-file address")
        })?;
        let attachment = self.attachment(&address.project_id)?.ok_or_else(|| {
            AikitError::new("work_repos.source_out_of_scope", "The literal Work Project is not attached to this provider")
        })?;
        attachment.check()?;
        Ok((attachment, address.member))
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

    fn observed_bytes(&self, source: &SourceRef) -> Result<(&WorkAttachment, PathBuf, Vec<u8>)> {
        let (attachment, relative) = self.resolve_ref(source)?;
        let absolute = attachment.project.root.join(&relative);
        attachment.root.readable(&relative)?;
        let member = attachment.root.member(&absolute).map_err(|error| {
            if error.code() != "work_repos.source_unavailable" { return error; }
            let mut projected = AikitError::new("work_repos.source_unreadable", error.message())
                .with("physical_error", json!({"code":error.code(), "message":error.message(), "details":error.details()}).to_string())
                .with_io_source_from(&error);
            for (key, value) in error.details() { projected = projected.with(key, value); }
            projected
        })?;
        let bytes = attachment.root.read(&relative, &member, WORK_REPOS_MAX_READ_BYTES)?;
        attachment.check()?;
        Ok((attachment, relative, bytes))
    }

    /// Exact admitted material bytes for local consumers with their own
    /// derived revision scheme. This delegates the SAME observation used by
    /// material(), with no second capture or copied-body fallback.
    pub fn read_bytes(&self, source: &SourceRef) -> Result<Option<Vec<u8>>> {
        let Some(address) = decode_work_file_source_ref(source)? else { return Ok(None); };
        if self.attachment(&address.project_id)?.is_none() { return Ok(None); }
        let (_, _, bytes) = self.observed_bytes(source)?;
        Ok(Some(bytes))
    }

    fn material(&self, source: &SourceRef) -> Result<SourceMaterial> {
        let (attachment, relative, bytes) = self.observed_bytes(source)?;
        let project = &attachment.project;
        let absolute = project.root.join(&relative);
        Ok(SourceMaterial {
            binding: SourceBinding {
                source: source.clone(), revision: SourceRevision::parse(content_revision(&bytes))?,
                title: safe_work_member(&relative)?.to_owned(), tags: Self::tags(project),
                visibility: SourceVisibility::Personal, owners: vec![],
                media_type: Self::media_type(&relative).into(), locator: Some(ResourceLocator::Path(absolute)),
                metadata: [
                    ("work-repos", json!({"project": project.name, "project_id": project.project_id})),
                    ("owner_read_required", json!(true)),
                ].into_iter().map(|(key,value)| (key.to_owned(),value)).collect(),
            },
            body: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }

    /// Search one repo: one ripgrep invocation over an alternation of the
    /// escaped query terms, capped per file, grouped per file and scored by
    /// how much *specific* evidence the file carries. Each term is weighted
    /// by its rarity across the files that matched (a word every file
    /// carries says little about which file answers), files matching every
    /// term outrank partial matches, and more matching lines is a small tie
    /// breaker.
    ///
    /// When the match pass still exceeds the retained budget, which files it
    /// saw would depend on traversal order; the pass is then rescored from an
    /// exact per-term count over the whole scope, so ranking never depends
    /// on which files ripgrep's threads happened to reach first.
    ///
    /// The score is mapped into `[0, 0.5)` — strictly below the shared
    /// default the pre-existing pools answer at — so Work coverage ranks
    /// after Control/user and Control/agents material instead of flooding
    /// the surfaced limit (addendum A-5). Within the pool the order is the
    /// raw score's.
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
            max_count_per_file: Some(PER_FILE_MATCH_CAP),
            timeout: Some(std::time::Duration::from_secs(30)),
        };
        self.require_project(project)?;
        let outcome = self.searcher.search(&request)?;
        self.require_project(project)?;
        // Group matches by file; count distinct terms and keep the first
        // matching line as the snippet. Term membership folds case, matching
        // the searcher's own case-folding.
        let lowered: Vec<String> = terms.iter().map(|term| term.to_lowercase()).collect();
        let mut files: BTreeMap<PathBuf, FileEvidence> = BTreeMap::new();
        for matched in &outcome.matches {
            let Some(relative) = matched
                .path
                .strip_prefix(&project.root)
                .ok()
                .map(Path::to_path_buf)
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
                .entry(relative)
                .or_insert(FileEvidence {
                    terms: BTreeSet::new(),
                    lines: 0,
                    snippet: None,
                    first_line: matched.line_number,
                    matched_lines: Vec::new(),
                });
            entry.terms.extend(term_hits);
            entry.lines += 1;
            entry.matched_lines.push((matched.line_number, matched.line.clone()));
            if entry.snippet.is_none() {
                let snippet: String = matched.line.chars().take(240).collect();
                entry.snippet = Some(snippet.trim_end().to_string());
            }
        }
        if outcome.truncated {
            // The retained sample is traversal-ordered; replace its term
            // evidence with an exact count per term over the whole scope,
            // keeping the sample's snippets where it has them.
            let mut exact: BTreeMap<PathBuf, FileEvidence> = BTreeMap::new();
            for (index, term) in terms.iter().enumerate() {
                let per_term = SearchRequest {
                    pattern: term.clone(),
                    regex: false,
                    max_count_per_file: None,
                    ..request.clone()
                };
                self.require_project(project)?;
                let counts = self.searcher.count(&per_term)?;
                self.require_project(project)?;
                for (path, count) in counts {
                    let Some(relative) = path
                        .strip_prefix(&project.root)
                        .ok()
                        .map(Path::to_path_buf)
                    else {
                        continue;
                    };
                    let sampled = files.get(&relative);
                    let entry = exact.entry(relative).or_insert(FileEvidence {
                        terms: BTreeSet::new(),
                        lines: 0,
                        snippet: sampled.and_then(|evidence| evidence.snippet.clone()),
                        first_line: sampled.map(|evidence| evidence.first_line).unwrap_or(1),
                        matched_lines: sampled.map(|evidence| evidence.matched_lines.clone()).unwrap_or_default(),
                    });
                    entry.terms.insert(index);
                    entry.lines += count as usize;
                }
            }
            files = exact;
        }
        let weights = term_weights(terms.len(), &files);
        let total_weight: f64 = weights.iter().sum();
        let mut scored: Vec<(f64, PathBuf, FileEvidence)> = files
            .into_iter()
            .map(|(relative, evidence)| {
                let carried: f64 = evidence.terms.iter().map(|index| weights[*index]).sum();
                let coverage = if total_weight > 0.0 {
                    carried / total_weight
                } else {
                    0.0
                };
                let complete = if evidence.terms.len() == terms.len() {
                    1.0
                } else {
                    0.0
                };
                let mass = (evidence.lines.min(16) as f64) * 0.01;
                // Raw scores reach 1 + 1 + 0.16 = 2.16; the shared surface
                // answers 0.5 for unscored pools, so this pool publishes
                // under that floor.
                const SHARED_FLOOR: f64 = 0.5;
                const RAW_CEILING: f64 = 2.2;
                (
                    SHARED_FLOOR * (coverage + complete + mass) / RAW_CEILING,
                    relative,
                    evidence,
                )
            })
            .collect();
        scored.sort_by(|left, right| {
            right
                .0
                .total_cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        let mut hits = Vec::new();
        for (score, relative, evidence) in scored.into_iter().take(limit) {
            let source = Self::source_ref(project, &relative)?;
            let before = self.material(&source)?;
            let absolute = project.root.join(&relative);
            let selected = SearchRequest {
                roots: vec![absolute.clone()],
                ..request.clone()
            };
            // The global query carries relevance, not a whole-file revision.
            // Bracket a finite real selected-file query with current reads and
            // require its native lines and prior evidence to agree exactly.
            self.require_project(project)?;
            let checked = self.searcher.search(&selected)?;
            self.require_project(project)?;
            let mut checked_terms = BTreeSet::new();
            let mut checked_lines = Vec::new();
            for matched in &checked.matches {
                let index = matched.line_number.checked_sub(1)
                    .and_then(|index| usize::try_from(index).ok());
                let actual = index.and_then(|index| before.body.split('\n').nth(index))
                    .map(|line| line.trim_end_matches('\r'));
                if matched.path != absolute || actual != Some(matched.line.as_str()) {
                    return Err(AikitError::new("work_repos.source_search_basis_conflict",
                        "Native selected-file match no longer agrees with the current Source")
                        .with("source", source.to_string()));
                }
                let lowered_line = matched.line.to_lowercase();
                checked_terms.extend(lowered.iter().enumerate()
                    .filter(|(_, term)| lowered_line.contains(term.as_str()))
                    .map(|(index, _)| index));
                checked_lines.push((matched.line_number, matched.line.clone()));
            }
            let mut checked_count = checked_lines.len();
            if outcome.truncated {
                checked_count = 0;
                checked_terms.clear();
                for (index, term) in terms.iter().enumerate() {
                    let per_term = SearchRequest {
                        pattern: term.clone(), regex: false, max_count_per_file: None,
                        ..selected.clone()
                    };
                    self.require_project(project)?;
                    let counts = self.searcher.count(&per_term)?;
                    self.require_project(project)?;
                    for (path, count) in counts {
                        if path != absolute {
                            return Err(AikitError::new("work_repos.source_search_basis_conflict",
                                "Native selected-file count returned another Source")
                                .with("source", source.to_string()));
                        }
                        if count > 0 {
                            checked_terms.insert(index);
                            checked_count += count as usize;
                        }
                    }
                }
            }
            let after = self.material(&source)?;
            let same_lines = if outcome.truncated {
                checked_lines.starts_with(&evidence.matched_lines)
            } else {
                checked_lines == evidence.matched_lines
            };
            if before != after || checked.truncated || checked_lines.is_empty()
                || !same_lines || checked_terms != evidence.terms || checked_count != evidence.lines
            {
                return Err(AikitError::new("work_repos.source_search_basis_conflict",
                    "Native query evidence and the selected current Source basis changed")
                    .with("source", source.to_string()));
            }
            hits.push(SourceHit {
                    source,
                    revision: Some(before.binding.revision),
                    provider: provider(),
                    score: Some(score),
                    title: safe_work_member(&relative)?.to_owned(),
                    snippet: evidence.snippet.clone().unwrap_or_default(),
                    tags: Self::tags(project),
                    provider_binding: evidence.snippet.as_ref()
                        .map(|_| format!("line:{}", evidence.first_line)),
                    retrieval_mode: SourceSearchMode::Fulltext,
                });
        }
        Ok(hits)
    }

    fn run_search(&self, query: &str, tags: &[String], limit: usize) -> Result<Vec<SourceHit>> {
        let terms = query_terms(query);
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

struct WorkProjectView<'a, R> {
    owner: &'a WorkReposSourcePoolProvider<R>,
    project: &'a WorkRepoProject,
}

impl<R: CommandRunner> SourcePoolProvider for WorkProjectView<'_, R> {
    fn capabilities(&self) -> SourceProviderCapabilities { self.owner.capabilities() }
    fn rebuild(&mut self, _: &[SourceMaterial]) -> Result<()> {
        Err(AikitError::new("work_repos.owner_only", "A borrowed Work view cannot rebuild owner source"))
    }
    fn read(&self, source: &SourceRef) -> Result<Option<SourceMaterial>> {
        let Some(address) = decode_work_file_source_ref(source)? else { return Ok(None); };
        if address.project_id != self.project.project_id { return Ok(None); }
        self.owner.read(source)
    }
    fn read_for(&self, source: &SourceRef, target: aikit_core::context_source::RetrievalTarget) -> Result<Option<SourcePoolReading>> {
        let Some(address) = decode_work_file_source_ref(source)? else { return Ok(None); };
        if address.project_id != self.project.project_id { return Ok(None); }
        self.owner.read_for(source, target)
    }
    fn search(&self, query: &str, mode: SourceSearchMode, tags: &[String], limit: usize) -> Result<Vec<SourceHit>> {
        if mode != SourceSearchMode::Fulltext {
            return Err(AikitError::new("knowledge.source_provider_capability", "Work source supports fulltext search"));
        }
        let terms = query_terms(query);
        self.owner.require_project(self.project)?;
        self.owner.search_project(self.project, &terms, tags, limit)
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
        let Some(address) = decode_work_file_source_ref(source)? else { return Ok(None); };
        if self.attachment(&address.project_id)?.is_none() { return Ok(None); }
        Ok(Some(self.material(source)?))
    }

    fn read_for(&self, source: &SourceRef, target: aikit_core::context_source::RetrievalTarget) -> Result<Option<SourcePoolReading>> {
        let Some(address) = decode_work_file_source_ref(source)? else { return Ok(None); };
        let Some(attachment) = self.attachment(&address.project_id)? else { return Ok(None); };
        attachment.check()?;
        let privacy = aikit_core::context_source::ContextSourcePrivacy::default();
        SourcePoolReading::check_target(privacy, target)?;
        Ok(Some(SourcePoolReading { material: self.material(source)?, privacy }))
    }

    fn search(
        &self,
        query: &str,
        mode: SourceSearchMode,
        tags: &[String],
        limit: usize,
    ) -> Result<Vec<SourceHit>> {
        if let Some(error) = &self.attachment_error { return Err(error.clone()); }
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
            detail: self.attachment_error.as_ref().map(|error| format!("Work attachment unavailable: {error}")).unwrap_or_else(|| format!(
                "live ripgrep content search over {} Work repo(s); gitignore respected \
                 (hidden=false); types {}; .no-agent-retrieval prunes {} marked subtree(s); \
                 per-repo limits bound a root query by limit × projects",
                self.projects.len(), WORK_REPOS_TYPES.join("/"), marked,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{RecordingRunner, SystemRunner};

    fn project(root: &Path) -> WorkRepoProject {
        WorkRepoProject {
            name: "demo".into(),
            project_id: "demo".into(),
            root: root.to_path_buf(),
        }
    }

    fn manifest(project_id: &str) -> String {
        json!({"schema":aikit_core::CENTRAL_PROJECT_SCHEMA,"project_id":project_id,
            "human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1",
            "source":"ProjectCentral/agents/wiki/wiki.json","adopted_sources":[]}}).to_string()
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn discovery_reads_complete_native_declarations_without_reinterpreting_ids() {
        let temp = native_scratch();
        let root = temp.path();
        for (name,id) in [("alpha","a:b"),("delta","project:delta"),("slash","native/id") ] {
            write(&root.join(format!("Work/{name}/ProjectCentral/project.json")), &manifest(id));
        }
        std::fs::create_dir_all(root.join("Work/plain")).unwrap();
        write(&root.join("Work/beta/ProjectCentral/project.json"), "not json");
        write(&root.join("Work/gamma/ProjectCentral/project.json"), r#"{"schema":"central.project/v0","project_id":"gamma"}"#);
        let entries = discover_native_work_projects(root).unwrap();
        let mut projects = Vec::new();
        let mut absences = Vec::new();
        for entry in entries {
            match entry {
                NativeWorkProjectEntry::Project(project) => projects.push(project.project().clone()),
                NativeWorkProjectEntry::Absence {name,error} => absences.push((name,error)),
            }
        }
        assert_eq!(projects.iter().map(|project| (&*project.name,&*project.project_id)).collect::<Vec<_>>(),
            vec![("alpha","a:b"),("delta","project:delta"),("slash","native/id")]);
        assert_eq!(absences.len(),2);
        assert_eq!(absences.iter().find(|(name,_)|name=="beta").unwrap().1.code(),"projectcentral.manifest_invalid");
        assert_eq!(absences.iter().find(|(name,_)|name=="gamma").unwrap().1.code(),"projectcentral.manifest_invalid");
    }

    #[test]
    fn discovery_is_stable_and_sorted_by_folder_name() {
        let temp = native_scratch();
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
        let names: Vec<String> = discover_work_projects(temp.path())
            .into_iter()
            .filter_map(|entry| match entry {
                WorkProjectEntry::Project(project) => Some(project.name),
                WorkProjectEntry::Absence { .. } => None,
            })
            .collect();
        assert_eq!(names, vec!["Alpha", "beta", "zeta"]);
    }

    fn native_scratch() -> tempfile::TempDir {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        let scratch = std::fs::canonicalize(scratch).unwrap();
        tempfile::Builder::new().prefix("work-native-query-").tempdir_in(scratch).unwrap()
    }

    fn native_matches(root: &Path, files: &[(&str, u64, &str)]) -> crate::runner::SystemRunner {
        for (path, line_number, text) in files {
            let mut body = "\n".repeat(usize::try_from(line_number - 1).unwrap());
            body.push_str(text);
            body.push('\n');
            write(&root.join(path), &body);
        }
        crate::runner::SystemRunner::new()
            .with_env_removed("RIPGREP_CONFIG_PATH")
            .with_timeout(std::time::Duration::from_secs(30))
    }

    #[test]
    fn hits_carry_project_register_refs_and_all_term_files_outrank_partials() {
        let temp = native_scratch();
        let root = temp.path();
        let provider = WorkReposSourcePoolProvider::connect(
            native_matches(
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
            crate::ripgrep::executable(),
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
            work_file_source_ref("demo", Path::new("src/routine.rs")).unwrap().as_str(),
            "the file carrying every term outranks partials: {hits:?}"
        );
        assert_eq!(hits[0].provider.as_str(), WORK_REPOS_PROVIDER_REF);
        assert!(hits[0].tags.contains(&"work-repos".to_string()));
        assert!(hits.iter().all(|hit| hit.title.contains('.')));
    }

    #[test]
    fn query_terms_drop_stop_words_and_fragments_but_keep_identifiers() {
        assert_eq!(
            query_terms("hook delivery of the prepared NOW context before the first model turn"),
            vec!["hook", "delivery", "prepared", "NOW", "context", "first", "model", "turn"]
        );
        assert_eq!(
            query_terms("why does prepare_now_context drop cap.aikit.continuity, #388?"),
            vec![
                "prepare_now_context",
                "drop",
                "cap.aikit.continuity",
                "#388"
            ]
        );
        // A query made only of weak words still answers on its own terms.
        assert_eq!(query_terms("to be or"), vec!["to", "be", "or"]);
        assert_eq!(query_terms("Hook hook HOOK"), vec!["Hook"]);
    }

    #[test]
    fn a_rare_term_outweighs_several_common_ones() {
        let temp = native_scratch();
        let root = temp.path();
        // `context` and `model` appear everywhere; `prepared` only in the
        // design note. Before rarity weighting, three common terms beat one
        // specific term.
        let mut files: Vec<(String, u64, String)> = Vec::new();
        for index in 0..8 {
            files.push((
                format!("src/noise{index}.rs"),
                1,
                "context model turn".into(),
            ));
        }
        files.push(("docs/JEV-REDIS-NOW.md".into(), 4, "prepared context".into()));
        let borrowed: Vec<(&str, u64, &str)> = files
            .iter()
            .map(|(path, line, text)| (path.as_str(), *line, text.as_str()))
            .collect();
        let provider = WorkReposSourcePoolProvider::connect(
            native_matches(root, &borrowed),
            crate::ripgrep::executable(),
            vec![project(root)],
        );
        let hits = provider
            .search(
                "prepared context model turn",
                SourceSearchMode::Fulltext,
                &[],
                10,
            )
            .unwrap();
        assert_eq!(
            hits[0].source.as_str(),
            work_file_source_ref("demo", Path::new("docs/JEV-REDIS-NOW.md")).unwrap().as_str(),
            "the only file carrying the rare term ranks first: {hits:?}"
        );
    }

    #[test]
    fn the_match_pass_caps_matches_per_file() {
        let temp = native_scratch();
        let recorder = RecordingRunner::new(SystemRunner::probe());
        let provider =
            WorkReposSourcePoolProvider::connect(&recorder, "rg", vec![project(temp.path())]);
        let _ = provider.search("prepared", SourceSearchMode::Fulltext, &[], 5);
        let calls = recorder.calls();
        let search = calls
            .iter()
            .find(|argv| argv.iter().any(|arg| arg == "--json"))
            .expect("a search ran");
        let cap = search
            .iter()
            .position(|arg| arg == "--max-count")
            .expect("a per-file cap rides the match pass");
        assert_eq!(search[cap + 1], PER_FILE_MATCH_CAP.to_string());
    }

    #[test]
    fn a_truncated_match_pass_is_rescored_from_exact_counts() {
        let temp = native_scratch();
        let root = temp.path();
        // The actual native match count exceeds the production retained limit.
        // The count fallback must retain its original rarity/ranking semantics.
        for index in 0..(PER_REPO_MATCH_BUDGET + 1) {
            write(&root.join(format!("src/noise{index}.rs")), "common\n");
        }
        write(&root.join("docs/answer.md"), "rareword\nrareword\nrareword\ncommon\n");
        let runner = RecordingRunner::new(crate::runner::SystemRunner::new()
            .with_env_removed("RIPGREP_CONFIG_PATH")
            .with_timeout(std::time::Duration::from_secs(30)));
        let provider = WorkReposSourcePoolProvider::connect(&runner, crate::ripgrep::executable(), vec![project(root)]);
        assert!(provider.status().available, "real ripgrep is required for this native query test");
        let hits = provider.search("rareword common", SourceSearchMode::Fulltext, &[], 5).unwrap();
        assert_eq!(hits[0].source.as_str(), work_file_source_ref("demo", Path::new("docs/answer.md")).unwrap().as_str(),
            "native exact counts retain the file carrying both terms: {hits:?}");
        let reading = provider.read(&hits[0].source).unwrap().unwrap();
        assert_eq!(hits[0].revision.as_ref(), Some(&reading.binding.revision));
        assert!(runner.calls().iter().any(|argv| argv.iter().any(|arg| arg == "--count")),
            "the actual retained limit must exercise native count fallback");
        for hit in hits {
            if hit.snippet.is_empty() {
                assert!(hit.provider_binding.is_none(), "no sampled line means no invented line binding");
            }
        }
    }

    #[test]
    fn published_scores_stay_under_the_shared_floor_so_control_pools_keep_priority() {
        let temp = native_scratch();
        let root = temp.path();
        let provider = WorkReposSourcePoolProvider::connect(
            native_matches(
                root,
                &[
                    (
                        "src/routine.rs",
                        3,
                        "automations drive the cron scheduled gate",
                    ),
                    ("docs/plan.md", 9, "automations cron"),
                ],
            ),
            crate::ripgrep::executable(),
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
        assert_eq!(hits.len(), 2, "{hits:?}");
        for hit in &hits {
            let score = hit.score.expect("work-repos hits carry a score");
            assert!(
                score < 0.5,
                "a score at or above the shared 0.5 floor would rank this pool \
                 ahead of the pre-existing pools: {hits:?}"
            );
            assert!(score > 0.0, "scores stay positive: {hits:?}");
        }
        let first = hits[0].score.unwrap();
        let second = hits[1].score.unwrap();
        assert!(
            first > second,
            "within-pool order still follows match quality: {hits:?}"
        );
    }

    #[test]
    fn the_argv_pins_gitignore_respect_and_the_type_list() {
        let temp = native_scratch();
        let recorder = RecordingRunner::new(SystemRunner::probe());
        let provider =
            WorkReposSourcePoolProvider::connect(&recorder, "rg", vec![project(temp.path())]);
        // The actual native query may find no file; its real argv still
        // preserves the established ignore/type discipline.
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
        let temp = native_scratch();
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
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        let temp = tempfile::Builder::new().prefix("work-owner-read-").tempdir_in(scratch).unwrap();
        let root = temp.path();
        write(&root.join("src/routine.rs"), "automations cron\n");
        write(&root.join("sealed/secret.md"), "hidden\n");
        write(&root.join("sealed/.no-agent-retrieval"), "");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("src/routine.rs"), root.join("linked.md")).unwrap();
        let provider = WorkReposSourcePoolProvider::connect(
            crate::runner::SystemRunner::probe(),
            crate::ripgrep::executable(),
            vec![project(root)],
        );
        let reading = provider
            .read(&work_file_source_ref("demo", Path::new("src/routine.rs" )).unwrap())
            .unwrap()
            .expect("a disclosed repo file reads back live");
        assert!(reading.body.contains("automations cron"));
        assert_eq!(
            reading.binding.revision.as_str(),
            content_revision(b"automations cron\n")
        );

        let error = provider
            .read(&work_file_source_ref("demo", Path::new("sealed/secret.md" )).unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "work_repos.source_unauthorised");

        #[cfg(unix)]
        {
            let error = provider
                .read(&work_file_source_ref("demo", Path::new("linked.md" )).unwrap())
                .unwrap_err();
            assert_eq!(error.code(), "work_repos.source_unauthorised");
        }

        assert!(provider.read(&work_file_source_ref("other", Path::new("src/routine.rs" )).unwrap()).unwrap().is_none());

        let error = provider
            .read(&SourceRef::parse("source:work-file:v1:ZGVtbw:Li4vZXNjYXBlLm1k").unwrap())
            .unwrap_err();
        assert_eq!(error.code(), "work_repos.source_escape");

        assert!(provider.read(&SourceRef::parse("central:source:control:root:x").unwrap()).unwrap().is_none());
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn actual_work_material_missing_and_fifo_are_failures_not_another_owner() {
        use crate::runner::{CommandRunner, SystemRunner};
        use std::os::unix::fs::FileTypeExt;
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        let temp = tempfile::Builder::new().prefix("work-material-physical-").tempdir_in(scratch).unwrap();
        let root = temp.path();
        let fifo = root.join("held-fifo.md");
        let argv = vec!["/usr/bin/mkfifo".to_string(), fifo.display().to_string()];
        SystemRunner::probe().run(&argv).unwrap().require(&argv, "work_repos.test_fifo_creation_failed").unwrap();
        let provider = WorkReposSourcePoolProvider::connect(
            SystemRunner::probe(), crate::ripgrep::executable(), vec![project(root)],
        );
        let refused = provider.read(&work_file_source_ref("demo", Path::new("held-fifo.md" )).unwrap()).unwrap_err();
        assert_eq!(refused.code(), "work_repos.source_unauthorised");
        let missing = provider.read(&work_file_source_ref("demo", Path::new("absent.md" )).unwrap()).unwrap_err();
        assert_eq!(missing.code(), "work_repos.source_unreadable");
        let cause = std::error::Error::source(&missing).unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(cause.raw_os_error(), Some(2));
        let retained: serde_json::Value = serde_json::from_str(&missing.details()["physical_error"]).unwrap();
        assert_eq!(retained["details"]["cause_kind"], "NotFound");
        assert_eq!(retained["details"]["cause_raw_os_error"], "2");
        assert!(std::fs::symlink_metadata(fifo).unwrap().file_type().is_fifo());
        assert!(!root.join("absent.md").exists());
    }

    #[test]
    fn rebuild_is_refused_because_the_repos_are_live_ground() {
        let temp = native_scratch();
        let mut provider = WorkReposSourcePoolProvider::connect(
            SystemRunner::probe(),
            crate::ripgrep::executable(),
            vec![project(temp.path())],
        );
        let error = provider.rebuild(&[]).unwrap_err();
        assert_eq!(error.code(), "work_repos.owner_only");
    }

    #[test]
    fn unavailable_ripgrep_is_a_disclosed_absence_not_a_fake_available() {
        let temp = native_scratch();
        let runner = SystemRunner::probe();
        let provider =
            WorkReposSourcePoolProvider::connect(runner, "work-test-deliberately-unavailable-rg", vec![project(temp.path())]);
        assert!(!provider.status().available);
    }

    #[test]
    fn status_discloses_the_projects_and_actual_native_version() {
        let temp = native_scratch();
        let runner = SystemRunner::probe();
        let provider =
            WorkReposSourcePoolProvider::connect(runner, "rg", vec![project(temp.path())]);
        let status = provider.status();
        assert!(status.available);
        assert!(status.version.as_deref().is_some_and(|value| value.starts_with("ripgrep ")));
        assert_eq!(status.version_drift, !status.version.as_deref().unwrap().contains(RIPGREP_TESTED_VERSION));
        assert_eq!(
            status.tested_version.as_deref(),
            Some(RIPGREP_TESTED_VERSION)
        );
        assert!(status.detail.contains("1 Work repo(s)"));
        assert!(status.detail.contains("hidden=false"));
    }

    #[test]
    fn semantic_mode_is_a_capability_refusal_not_a_silent_empty() {
        let temp = native_scratch();
        let provider = WorkReposSourcePoolProvider::connect(
            SystemRunner::probe(),
            crate::ripgrep::executable(),
            vec![project(temp.path())],
        );
        let error = provider
            .search("anything", SourceSearchMode::Semantic, &[], 10)
            .unwrap_err();
        assert_eq!(error.code(), "knowledge.source_provider_capability");
    }
    #[test]
    fn exact_work_codec_roundtrips_opaque_ids_and_preserves_literal_members() {
        for id in ["a:b", "a", "project:delta", "native/名", "a::b", "%41", "a%3Ab"] {
            for member in ["docs/a:b.md", "docs/literal\\name.md", "docs/space name.md", "README.md"] {
                let source = work_file_source_ref(id, Path::new(member)).unwrap();
                let decoded = decode_work_file_source_ref(&source).unwrap().unwrap();
                assert_eq!(decoded.project_id, id);
                assert_eq!(decoded.member.as_os_str(), Path::new(member).as_os_str());
            }
        }
        let left = work_file_source_ref("a:b", Path::new("c.md")).unwrap();
        let right = work_file_source_ref("a", Path::new("b:c.md")).unwrap();
        assert_ne!(left,right);
        assert!(decode_work_file_source_ref(&SourceRef::parse("source:project:a:b:root").unwrap()).unwrap().is_none());
    }

    #[test]
    fn actual_codec_boundaries_and_malformed_coordinates_refuse_without_shortening() {
        let member = Path::new("c.md");
        let member_len = base64::encoded_len(4,false).unwrap();
        let max_id = (1..WORK_FILE_ADDRESS_MAX_BYTES).rev().find(|n| {
            WORK_FILE_ADDRESS_PREFIX.len()+base64::encoded_len(*n,false).unwrap()+1+member_len <= WORK_FILE_ADDRESS_MAX_BYTES
        }).unwrap();
        let id = "q".repeat(max_id);
        let source = work_file_source_ref(&id,member).unwrap();
        assert_eq!(decode_work_file_source_ref(&source).unwrap().unwrap().project_id,id);
        assert_eq!(work_file_source_ref(&"q".repeat(max_id+1),member).unwrap_err().code(),"work_repos.address_budget");
        for suffix in ["", "YQ", ":Yg", "YQ:", "YQ:Yg:Zg", "YQ==:Yg", "YQ:/w", "YR:Yg", "YQ:Li4vYg"] {
            let source = SourceRef::parse(format!("{WORK_FILE_ADDRESS_PREFIX}{suffix}")).unwrap();
            assert!(decode_work_file_source_ref(&source).is_err(),"{source}");
        }
        let oversized = SourceRef::parse(format!("{WORK_FILE_ADDRESS_PREFIX}{}:Yg","q".repeat(WORK_FILE_ADDRESS_MAX_BYTES))).unwrap();
        assert_eq!(decode_work_file_source_ref(&oversized).unwrap_err().code(),"work_repos.address_budget");
    }

    fn independent(root: &Path, id: &str, name: &str) -> WorkRepoProject {
        WorkRepoProject { name:name.into(),project_id:id.into(),root:root.to_path_buf() }
    }

    fn read_only_native(native: NativeWorkRepoProject) -> WorkReposSourcePoolProvider<crate::runner::SystemRunner> {
        WorkReposSourcePoolProvider::connect_native(crate::runner::SystemRunner::probe(),
            "work-test-deliberately-unavailable-rg", vec![native]).unwrap()
    }

    #[test]
    fn real_two_project_collision_lone_colon_roundtrip_and_legacy_survivor_never_rehome() {
        let owned = native_scratch();
        let world = owned.path();
        let left = world.join("Work/left");
        let right = world.join("Work/right");
        write(&left.join(WORK_MANIFEST_MEMBER), &manifest("a:b"));
        write(&right.join(WORK_MANIFEST_MEMBER), &manifest("a"));
        write(&left.join("c.md"),"left bytes\n");
        write(&right.join("b:c.md"),"right bytes\n");
        let provider = WorkReposSourcePoolProvider::connect_native(crate::runner::SystemRunner::probe(),
            "work-test-deliberately-unavailable-rg", vec![
                NativeWorkRepoProject::inspect(&left,"left",Some(world)).unwrap(),
                NativeWorkRepoProject::inspect(&right,"right",Some(world)).unwrap(),
            ]).unwrap();
        let l = work_file_source_ref("a:b",Path::new("c.md")).unwrap();
        let r = work_file_source_ref("a",Path::new("b:c.md")).unwrap();
        assert_ne!(l,r);
        assert_eq!(provider.read(&l).unwrap().unwrap().body,"left bytes\n");
        assert_eq!(provider.read(&r).unwrap().unwrap().body,"right bytes\n");
        write(&left.join("c.md"),"identical body\n");
        write(&right.join("b:c.md"),"identical body\n");
        let left_read=provider.read(&l).unwrap().unwrap();
        let right_read=provider.read(&r).unwrap().unwrap();
        assert_eq!(left_read.body,right_read.body);
        assert_ne!(left_read.binding.source,right_read.binding.source,"equal bytes cannot collapse native identity");
        write(&left.join("c.md"),"left bytes\n");
        write(&right.join("b:c.md"),"right bytes\n");
        let legacy = SourceRef::parse("source:project:a:b:c.md").unwrap();
        assert!(provider.read(&legacy).unwrap().is_none());
        let lone = read_only_native(NativeWorkRepoProject::inspect(&right,"right",Some(world)).unwrap());
        assert!(lone.read(&legacy).unwrap().is_none(),"one survivor is not issuance history");
        assert_eq!(lone.read(&r).unwrap().unwrap().body,"right bytes\n");
        let restart = read_only_native(NativeWorkRepoProject::inspect(&left,"left",Some(world)).unwrap());
        assert_eq!(restart.read(&l).unwrap().unwrap().body,"left bytes\n");
    }

    #[test]
    fn real_atomic_same_byte_declaration_survives_but_extensions_and_id_change_require_fresh_attachment() {
        let owned = native_scratch();
        let root = owned.path();
        let original = manifest("native:a");
        write(&root.join(WORK_MANIFEST_MEMBER),&original);
        write(&root.join("README.md"),"retained bytes\n");
        let provider = read_only_native(NativeWorkRepoProject::inspect(root,"display",None).unwrap());
        let reference = work_file_source_ref("native:a",Path::new("README.md")).unwrap();
        let replacement = root.join("ProjectCentral/replacement.json");
        write(&replacement,&original);
        std::fs::rename(&replacement,root.join(WORK_MANIFEST_MEMBER)).unwrap();
        assert_eq!(provider.read(&reference).unwrap().unwrap().body,"retained bytes\n");
        let mut changed:serde_json::Value = serde_json::from_str(&original).unwrap();
        changed["foreign-extension"] = json!({"retained":true});
        write(&root.join(WORK_MANIFEST_MEMBER),&changed.to_string());
        assert_eq!(provider.read(&reference).unwrap_err().code(),"work_repos.native_declaration_changed");
        let fresh = read_only_native(NativeWorkRepoProject::inspect(root,"display",None).unwrap());
        assert_eq!(fresh.read(&reference).unwrap().unwrap().body,"retained bytes\n");
        write(&root.join(WORK_MANIFEST_MEMBER),&manifest("native:b"));
        assert_eq!(fresh.read(&reference).unwrap_err().code(),"work_repos.native_declaration_changed");
        let new = read_only_native(NativeWorkRepoProject::inspect(root,"display",None).unwrap());
        assert!(new.read(&reference).unwrap().is_none());
        assert_eq!(new.read(&work_file_source_ref("native:b",Path::new("README.md")).unwrap()).unwrap().unwrap().body,"retained bytes\n");
        assert_eq!(std::fs::read_to_string(root.join("README.md")).unwrap(),"retained bytes\n");
    }

    #[test]
    fn real_native_declaration_withdrawal_corruption_and_missing_cannot_enable_independent_fallback() {
        let owned = native_scratch();
        let root = owned.path();
        write(&root.join(WORK_MANIFEST_MEMBER),&manifest("native:a"));
        write(&root.join("README.md"),"retained bytes\n");
        let provider = read_only_native(NativeWorkRepoProject::inspect(root,"display",None).unwrap());
        let reference = work_file_source_ref("native:a",Path::new("README.md")).unwrap();
        write(&root.join("ProjectCentral/.no-agent-retrieval"),"");
        assert_eq!(provider.read_bytes(&reference).unwrap_err().code(),"work_repos.source_unauthorised");
        std::fs::remove_file(root.join("ProjectCentral/.no-agent-retrieval")).unwrap();
        write(&root.join(WORK_MANIFEST_MEMBER),"not json");
        assert_eq!(provider.read(&reference).unwrap_err().code(),"projectcentral.manifest_invalid");
        let explicit = WorkReposSourcePoolProvider::connect(crate::runner::SystemRunner::probe(),
            "work-test-deliberately-unavailable-rg", vec![independent(root,"native:a","display")]);
        assert_eq!(explicit.read(&reference).unwrap_err().code(),"projectcentral.manifest_invalid");
        assert!(!explicit.status().available);
        std::fs::remove_file(root.join(WORK_MANIFEST_MEMBER)).unwrap();
        let missing = provider.read(&reference).unwrap_err();
        let cause = std::error::Error::source(&missing).unwrap().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(cause.kind(),std::io::ErrorKind::NotFound);
        assert_eq!(std::fs::read_to_string(root.join("README.md")).unwrap(),"retained bytes\n");
    }

    #[test]
    #[cfg(unix)]
    fn real_equal_byte_root_alias_retarget_never_moves_the_old_project_read() {
        let owned = native_scratch();
        let original = owned.path().join("original");
        let foreign = owned.path().join("foreign");
        for root in [&original,&foreign] { write(&root.join(WORK_MANIFEST_MEMBER),&manifest("same:id")); }
        write(&original.join("README.md"),"original bytes\n");
        write(&foreign.join("README.md"),"foreign bytes\n");
        let alias = owned.path().join("alias");
        std::os::unix::fs::symlink(&original,&alias).unwrap();
        let provider = read_only_native(NativeWorkRepoProject::inspect(&alias,"display",None).unwrap());
        let reference = work_file_source_ref("same:id",Path::new("README.md")).unwrap();
        assert_eq!(provider.read(&reference).unwrap().unwrap().body,"original bytes\n");
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&foreign,&alias).unwrap();
        assert_eq!(provider.read(&reference).unwrap_err().code(),"work_repos.source_binding_changed");
        assert_eq!(std::fs::read_to_string(original.join("README.md")).unwrap(),"original bytes\n");
        assert_eq!(std::fs::read_to_string(foreign.join("README.md")).unwrap(),"foreign bytes\n");
    }

    #[test]
    fn real_scoped_view_reuses_the_native_attachment_and_exact_opaque_identity() {
        let owned = native_scratch();
        let root = owned.path();
        write(&root.join(WORK_MANIFEST_MEMBER),&manifest("a:b"));
        write(&root.join("README.md"),"same owner\n");
        let owner = read_only_native(NativeWorkRepoProject::inspect(root,"display",None).unwrap());
        let view = owner.for_project("a:b").unwrap();
        let reference = work_file_source_ref("a:b",Path::new("README.md")).unwrap();
        assert_eq!(view.read(&reference).unwrap().unwrap().body,"same owner\n");
        assert!(view.read(&work_file_source_ref("a",Path::new("b:README.md")).unwrap()).unwrap().is_none());
        write(&root.join(WORK_MANIFEST_MEMBER),&manifest("a:c"));
        assert_eq!(view.read(&reference).unwrap_err().code(),"work_repos.native_declaration_changed");
    }

    #[test]
    fn real_explicit_reduced_composition_reads_without_ctrl_or_ripgrep_and_refuses_later_native_rebinding() {
        let owned = native_scratch();
        let root = owned.path();
        write(&root.join("README.md"),"standalone bytes\n");
        let provider = WorkReposSourcePoolProvider::connect(crate::runner::SystemRunner::probe(),
            "work-test-deliberately-unavailable-rg", vec![independent(root,"standalone:id","display")]);
        let reference = work_file_source_ref("standalone:id",Path::new("README.md")).unwrap();
        assert!(!provider.status().available,"no fake rg availability");
        assert_eq!(provider.read_bytes(&reference).unwrap().unwrap(),b"standalone bytes\n");
        assert!(provider.read_for(&reference,aikit_core::context_source::RetrievalTarget::LocalAgent).unwrap().is_some());
        assert_eq!(provider.read_for(&reference,aikit_core::context_source::RetrievalTarget::ExternalProvider).unwrap_err().code(),"knowledge.source_target_withheld");
        write(&root.join(".no-agent-retrieval"),"");
        assert_eq!(provider.read_bytes(&reference).unwrap_err().code(),"work_repos.source_unauthorised");
        std::fs::remove_file(root.join(".no-agent-retrieval")).unwrap();
        write(&root.join(WORK_MANIFEST_MEMBER),&manifest("standalone:id"));
        assert_eq!(provider.read(&reference).unwrap_err().code(),"work_repos.native_declaration_changed");
    }

    #[test]
    fn real_exact_byte_read_preserves_non_utf8_for_local_blake3_and_enforces_one_mebibyte() {
        let owned = native_scratch();
        let root = owned.path();
        let bytes = vec![0xff,b'\n',0xfe];
        std::fs::write(root.join("README.md"),&bytes).unwrap();
        let provider = WorkReposSourcePoolProvider::connect(crate::runner::SystemRunner::probe(),
            "work-test-deliberately-unavailable-rg",vec![independent(root,"literal:id","display")]);
        let reference = work_file_source_ref("literal:id",Path::new("README.md")).unwrap();
        assert_eq!(provider.read_bytes(&reference).unwrap().unwrap(),bytes);
        let boundary = vec![b'x';usize::try_from(WORK_REPOS_MAX_READ_BYTES).unwrap()];
        std::fs::write(root.join("README.md"),&boundary).unwrap();
        assert_eq!(provider.read_bytes(&reference).unwrap().unwrap().len(),boundary.len());
        std::fs::write(root.join("README.md"),vec![b'x';boundary.len()+1]).unwrap();
        assert_eq!(provider.read_bytes(&reference).unwrap_err().code(),"work_repos.source_too_large");
    }


    #[test]
    fn actual_conflicting_same_id_attachments_refuse_without_first_writer_selection() {
        let owned=native_scratch();
        let left=owned.path().join("left");
        let right=owned.path().join("right");
        std::fs::create_dir_all(&left).unwrap(); std::fs::create_dir_all(&right).unwrap();
        write(&left.join("README.md"),"left\n");write(&right.join("README.md"),"right\n");
        let provider=WorkReposSourcePoolProvider::connect(SystemRunner::probe(),
            "work-test-deliberately-unavailable-rg",vec![independent(&left,"same:id","left"),independent(&right,"same:id","right")]);
        let source=work_file_source_ref("same:id",Path::new("README.md")).unwrap();
        assert_eq!(provider.read(&source).unwrap_err().code(),"work_repos.project_binding_conflict");
        assert!(provider.status().detail.contains("conflicting"));
        assert_eq!(std::fs::read_to_string(left.join("README.md")).unwrap(),"left\n");
        assert_eq!(std::fs::read_to_string(right.join("README.md")).unwrap(),"right\n");
    }

    #[test]
    #[cfg(unix)]
    fn actual_dangling_native_parent_is_unavailable_not_explicit_absence() {
        let owned=native_scratch();
        let root=owned.path();
        std::os::unix::fs::symlink(root.join("missing-native-parent"),root.join("ProjectCentral")).unwrap();
        write(&root.join("README.md"),"retained bytes\n");
        let provider=WorkReposSourcePoolProvider::connect(SystemRunner::probe(),
            "work-test-deliberately-unavailable-rg",vec![independent(root,"native:id","display")]);
        let source=work_file_source_ref("native:id",Path::new("README.md")).unwrap();
        assert_eq!(provider.read(&source).unwrap_err().code(),"work_repos.native_declaration_unavailable");
        assert!(std::fs::symlink_metadata(root.join("ProjectCentral")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(root.join("README.md")).unwrap(),"retained bytes\n");
    }

}
