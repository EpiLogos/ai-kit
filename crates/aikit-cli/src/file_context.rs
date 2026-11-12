//! File context (W1/W3, CASE 05): the composed engine reaction that puts a
//! file's semantic context in front of the operation about to touch it.
//!
//! Descope law: this runs only when the active composition selected
//! `hook/continuity/file-context` — never ambient. The content is what the
//! project already declares or holds, never invented here:
//!
//! * **Wiki relations** — nodes from the project's own wiki
//!   (`ProjectCentral/agents/wiki/wiki.json`) whose `source_refs` cite the
//!   file, with their immediate links and backlinks (bounded).
//! * **Domain guidance** — declared KnowledgeDomains whose `path_patterns`
//!   address the file; the same ordinary/standing laws as prompt activation
//!   apply, and the explanation names domain, path and source.
//!
//! Dedup law, as behaviour: the key is the rendered content scoped per file,
//! so an unchanged file with unchanged relations re-injects nothing at all;
//! a changed file (new relations, new guidance) re-arms injection. Standing
//! rules stay dedup-exempt by their explicit classification, visibly.
//!
//! Fail-open: unreadable wiki state or ledger trouble downgrades to a
//! warning on the decision; the operation proceeds.

use std::path::Path;

use aikit_core::domain::{decide_injection, dedup_hash, render_guidance_lines, KnowledgeDomain};
use aikit_core::hooks::HookEvent;
use aikit_core::pressure::Block;
use aikit_core::skillset::glob_matches;
use aikit_core::{parse_wiki_objects, SemanticWikiIndex, WikiObject};
use aikit_store::index::Index;

/// How many wiki relations per direction survive the budget.
const NEIGHBOUR_LIMIT: usize = 5;
/// How many wiki nodes citing the file survive the budget.
const NODE_LIMIT: usize = 4;

/// The file path a pre-tool event carries, if any: the explicit path fields
/// of the tool input (`file_path`, `path`, `notebook_path`), never a guessed
/// one. Any tool that declares an explicit file path is in scope — the
/// reaction is about the file, not about a tool table.
pub fn file_path_of(event: &HookEvent) -> Option<String> {
    let input = event
        .payload
        .get("tool_input")
        .or_else(|| event.payload.get("input"))
        .unwrap_or(&event.payload);
    ["file_path", "path", "notebook_path"]
        .iter()
        .find_map(|key| input.get(*key))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

/// Load the project's own wiki through current native admission and physical
/// observation. Standalone use keeps its own declared root; no manifest or
/// hosted/native owner executable is required for this independent consumer.
pub fn load_project_wiki(project_root: &Path) -> (Vec<WikiObject>, Vec<String>) {
    load_project_wiki_in_world(project_root, None)
}

/// A supplied native World is an additional aperture only when its actual
/// canonical boundary contains the Project. A failed boundary reading is
/// unavailable, never evidence that the Project is an external standalone.
/// Captures are operation-local physical continuity, not new semantic IDs.
pub fn load_project_wiki_in_world(
    project_root: &Path,
    native_world_root: Option<&Path>,
) -> (Vec<WikiObject>, Vec<String>) {
    match read_project_wiki(project_root, native_world_root) {
        Ok(Some(objects)) => (objects, Vec::new()),
        Ok(None) => (Vec::new(), Vec::new()),
        Err(error) => (
            Vec::new(),
            vec![format!(
                "continuity/file-context project wiki withheld or unavailable ({}, {}): {error}",
                project_root.join(PROJECT_WIKI_MEMBER).display(),
                error.code(),
            )],
        ),
    }
}

const PROJECT_WIKI_MEMBER: &str = "ProjectCentral/agents/wiki/wiki.json";
const PROJECT_WIKI_READ_BUDGET: u64 = 16 * 1024 * 1024;

struct WikiRootObservation {
    requested: std::path::PathBuf,
    canonical: std::path::PathBuf,
    identity: (u64, u64),
}

fn wiki_io(path: &Path, error: std::io::Error) -> aikit_core::AikitError {
    aikit_core::AikitError::new(
        "file_context.wiki_unavailable",
        format!("{}: {error}", path.display()),
    )
    .with("path", path.display().to_string())
    .with("cause_kind", format!("{:?}", error.kind()))
    .with(
        "cause_raw_os_error",
        serde_json::json!(error.raw_os_error()).to_string(),
    )
    .with_io_source(error)
}

fn wiki_directory_identity(path: &Path) -> aikit_core::Result<(u64, u64)> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).map_err(|error| wiki_io(path, error))?;
        if !metadata.is_dir() {
            return Err(aikit_core::AikitError::new(
                "file_context.wiki_unavailable",
                "declared Wiki owner boundary is not a directory",
            )
            .with("path", path.display().to_string()));
        }
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    Err(aikit_core::AikitError::new(
        "knowledge.wiki_publication_metadata_unsupported",
        "native physical Wiki observation is unavailable on this platform",
    )
    .with("path", path.display().to_string()))
}

impl WikiRootObservation {
    fn capture(root: &Path) -> aikit_core::Result<Self> {
        // Retain the selected lexical route in invocation coordinates. Do not
        // canonicalize an alias away before the native ancestor policy sees it.
        let requested = if root.is_absolute() {
            root.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|error| wiki_io(root, error))?
                .join(root)
        };
        let canonical =
            std::fs::canonicalize(&requested).map_err(|error| wiki_io(&requested, error))?;
        let observation = Self {
            identity: wiki_directory_identity(&requested)?,
            requested,
            canonical,
        };
        observation.check()?;
        Ok(observation)
    }

    fn declared_world_member(
        &self,
        world: &Self,
    ) -> aikit_core::Result<Option<std::path::PathBuf>> {
        // Parent components before the World can be legitimate invocation
        // coordinates. Match an actual ancestor without collapsing symlinks
        // or replacing the remaining original route with its destination.
        let mut observed_member = None;
        for ancestor in self.requested.ancestors() {
            let canonical =
                std::fs::canonicalize(ancestor).map_err(|error| wiki_io(ancestor, error))?;
            if canonical == world.canonical {
                let member = self
                    .requested
                    .strip_prefix(ancestor)
                    .expect("an observed path ancestor is a lexical prefix");
                // Keep the outermost matching boundary. A descendant alias
                // back to the World must not erase earlier lexical ancestors.
                observed_member = Some(member.join(PROJECT_WIKI_MEMBER));
            }
        }
        Ok(observed_member)
    }

    fn check(&self) -> aikit_core::Result<()> {
        let canonical = std::fs::canonicalize(&self.requested)
            .map_err(|error| wiki_io(&self.requested, error))?;
        if canonical != self.canonical || wiki_directory_identity(&self.requested)? != self.identity
        {
            return Err(aikit_core::AikitError::new(
                "file_context.wiki_unavailable",
                "declared Wiki owner root changed during this observation",
            )
            .with("path", self.requested.display().to_string()));
        }
        Ok(())
    }
}

fn admit_project_wiki(
    project: &WikiRootObservation,
    world: Option<&WikiRootObservation>,
) -> aikit_core::Result<()> {
    use aikit_adapters::projectcentral::path_agent_readability;
    project.check()?;
    let member = Path::new(PROJECT_WIKI_MEMBER);
    let admitted = path_agent_readability(&project.requested, member)
        .map_err(|error| wiki_io(&project.requested.join(member), error))?;
    if !admitted {
        return Err(aikit_core::AikitError::new(
            "file_context.wiki_withheld",
            "current native Project read admission refused the selected Wiki",
        )
        .with("path", project.requested.join(member).display().to_string()));
    }
    if let Some(world) = world {
        world.check()?;
        if let Some(relative) = project.declared_world_member(world)? {
            let admitted = path_agent_readability(&world.requested, &relative)
                .map_err(|error| wiki_io(&world.requested.join(&relative), error))?;
            if !admitted {
                return Err(aikit_core::AikitError::new(
                    "file_context.wiki_withheld",
                    "current native World read admission refused the original selected Wiki route",
                )
                .with("path", world.requested.join(relative).display().to_string()));
            }
        }
        let relative = project
            .canonical
            .join(member)
            .strip_prefix(&world.canonical)
            .map_err(|_| {
                aikit_core::AikitError::new(
                    "file_context.wiki_unavailable",
                    "Project no longer belongs to the admitted native World aperture",
                )
            })?
            .to_path_buf();
        let admitted = path_agent_readability(&world.requested, &relative)
            .map_err(|error| wiki_io(&world.requested.join(&relative), error))?;
        if !admitted {
            return Err(aikit_core::AikitError::new(
                "file_context.wiki_withheld",
                "current native World read admission refused the selected Wiki",
            )
            .with("path", world.requested.join(relative).display().to_string()));
        }
        world.check()?;
    }
    project.check()
}

fn read_project_wiki(
    project_root: &Path,
    native_world_root: Option<&Path>,
) -> aikit_core::Result<Option<Vec<WikiObject>>> {
    let project = WikiRootObservation::capture(project_root)?;
    let world = match native_world_root {
        Some(root) => {
            let observed = WikiRootObservation::capture(root)?;
            project
                .canonical
                .starts_with(&observed.canonical)
                .then_some(observed)
        }
        None => None,
    };
    admit_project_wiki(&project, world.as_ref())?;
    let path = project.requested.join(PROJECT_WIKI_MEMBER);
    let canonical_source = match std::fs::canonicalize(&path) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            admit_project_wiki(&project, world.as_ref())?;
            return Ok(None);
        }
        Err(error) => return Err(wiki_io(&path, error)),
    };
    let member = canonical_source
        .strip_prefix(&project.canonical)
        .map_err(|_| {
            aikit_core::AikitError::new(
                "file_context.wiki_withheld",
                "selected Wiki is outside its admitted Project boundary",
            )
        })?;
    let observed = aikit_adapters::projectcentral::publication::material_bytes_affiliated(
        &project.requested,
        project.identity,
        member,
        PROJECT_WIKI_READ_BUDGET,
    );
    let bytes = match observed {
        Ok(bytes) => bytes,
        Err(error) => {
            // Never project final-source Missing from an unresolved owner
            // boundary. Keep the actual original physical cause independently.
            if let Err(current) = admit_project_wiki(&project, world.as_ref()) {
                return Err(error
                    .with("current_admission_code", current.code())
                    .with("current_admission_message", current.message()));
            }
            use std::error::Error;
            let missing = error
                .source()
                .and_then(|cause| cause.downcast_ref::<std::io::Error>())
                .is_some_and(|cause| cause.kind() == std::io::ErrorKind::NotFound);
            if missing && !error.details().contains_key("observation_stage") {
                return Ok(None);
            }
            return Err(error);
        }
    };
    admit_project_wiki(&project, world.as_ref())?;
    let current_mapping = std::fs::canonicalize(&path).map_err(|error| wiki_io(&path, error))?;
    if current_mapping != canonical_source {
        return Err(aikit_core::AikitError::new(
            "file_context.wiki_withheld",
            "selected Wiki member mapping changed during this observation",
        ));
    }
    let text = String::from_utf8(bytes).map_err(|error| {
        aikit_core::AikitError::new(
            "file_context.wiki_unavailable",
            format!("selected Wiki is not UTF-8: {error}"),
        )
    })?;
    parse_wiki_objects(&text).map(Some)
}

/// The project-relative form of the path, for pattern matching and keys, or
/// none for a path that is not in the project (round 55 item 8c; the review of
/// #616, F6). The path is read three ways, the first that places it under the
/// project standing:
///
/// - as the two are given, the path under the project's root;
/// - a relative route taken from the project's own folder, which is how a
///   harness names a file to its tool (the PreToolUse reaction in
///   `app/mod.rs` hands that route on as it came), with `.` and `..` read as
///   written, so a route that climbs out of the project is not in it;
/// - each as the disk resolves it, for a project or a file named through a
///   link or, where the volume folds case, in another case. A file about to
///   be made is resolved by the nearest folder above it that stands, the
///   names below that folder as written.
///
/// A reading places the path in the project only when what is left under the
/// root is plain names: a `..` left over after the project's own prefix
/// (`<project>/../elsewhere/x`), or a whole path left over under a root given
/// as `.` or as nothing, is not in the project by that reading. A path that
/// is in the project by none of them is matched by nothing: it is never
/// matched as it was given.
fn relative(project_root: &Path, path: &str) -> Option<String> {
    let given = Path::new(path);
    let named = if given.is_relative() {
        project_root.join(given)
    } else {
        given.to_path_buf()
    };
    let resolved = |named: &Path| {
        let mut below = Vec::new();
        let mut standing = named;
        loop {
            if let Ok(resolved) = std::fs::canonicalize(standing) {
                return Some(
                    below
                        .iter()
                        .rev()
                        .fold(resolved, |path, name| path.join(name)),
                );
            }
            // What does not stand yet is named as written, under what does.
            below.push(standing.file_name()?);
            standing = standing.parent()?;
        }
    };
    let under = |file: &Path, root: &Path| {
        file.strip_prefix(root)
            .ok()
            .filter(|inside| {
                inside
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
            })
            .map(Path::to_path_buf)
    };
    let inside = under(given, project_root)
        .or_else(|| under(&as_written(&named), &as_written(project_root)))
        .or_else(|| under(&resolved(&named)?, &resolved(project_root)?))?;
    Some(inside.to_string_lossy().into_owned())
}

/// A path with its `.` and `..` read as written: each `..` takes away the name
/// before it, and one with no name before it stays.
fn as_written(path: &Path) -> std::path::PathBuf {
    let mut plain = std::path::PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
                if matches!(
                    plain.components().next_back(),
                    Some(std::path::Component::Normal(_))
                ) =>
            {
                plain.pop();
            }
            other => plain.push(other.as_os_str()),
        }
    }
    plain
}

fn path_touches(reference: &str, relative_path: &str) -> bool {
    // Source refs carry scheme prefixes (`source:project:<path>`,
    // `central:source:...:<path>`, plain paths); the file matches when the
    // reference ends with its project-relative path.
    reference.trim_start_matches("./").ends_with(relative_path)
}

/// The wiki-relation lines for the file, or an empty vector when the wiki
/// holds nothing about it (unrelated wiki material never arrives).
fn wiki_lines(index: &SemanticWikiIndex, relative_path: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cited = Vec::new();
    for resource in index.discover() {
        let Some(node) = index.node(&resource) else {
            continue;
        };
        if node
            .source_refs
            .iter()
            .any(|source| path_touches(source.as_str(), relative_path))
        {
            cited.push(node);
        }
    }
    for node in cited.iter().take(NODE_LIMIT) {
        lines.push(format!(
            "- {} — {}",
            node.ref_id,
            node.title.as_deref().unwrap_or(&node.node_type)
        ));
        for neighbour in index
            .neighbours(&node.ref_id, NEIGHBOUR_LIMIT)
            .iter()
            .take(NEIGHBOUR_LIMIT)
        {
            lines.push(format!(
                "  links to: {} ({})",
                neighbour.resource, neighbour.relation
            ));
        }
        for neighbour in index.backlinks(&node.ref_id).iter().take(NEIGHBOUR_LIMIT) {
            lines.push(format!(
                "  cited by: {} ({})",
                neighbour.resource, neighbour.relation
            ));
        }
    }
    if cited.len() > NODE_LIMIT {
        lines.push(format!(
            "- … {} further nodes citing this file withheld by the disclosure budget",
            cited.len() - NODE_LIMIT
        ));
    }
    lines
}

/// Run the reaction for a file about to be touched. Returns the blocks to
/// inject plus warnings — wiki relations first, then file-addressed domain
/// guidance, everything deduped through the ledger.
pub fn run(
    index: &Index,
    scope: &str,
    project_root: &Path,
    path: &str,
    domains: &[KnowledgeDomain],
    wiki_objects: Vec<WikiObject>,
) -> (Vec<Block>, Vec<String>) {
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    // A file outside the project has no wiki relation in it and no domain of
    // its own to arm.
    let Some(relative_path) = relative(project_root, path) else {
        return (blocks, warnings);
    };

    // Wiki relations: one block, deduped as a whole per file.
    if !wiki_objects.is_empty() {
        match SemanticWikiIndex::rebuild(wiki_objects) {
            Ok(wiki) => {
                let lines = wiki_lines(&wiki, &relative_path);
                if !lines.is_empty() {
                    let hash = dedup_hash(
                        "file-context:wiki",
                        &[relative_path.clone(), lines.join("\n")],
                    );
                    if seen(index, scope, &hash, &mut warnings) {
                        // Unchanged file, unchanged relations: nothing arrives.
                    } else {
                        if let Err(error) = index.record_injection(scope, &hash) {
                            warnings.push(format!(
                                "continuity/file-context ledger unavailable: {error}"
                            ));
                        }
                        blocks.push(Block::ordinary(
                            format!(
                                "[continuity/file-context] wiki relations for {relative_path} (composed):"
                            ),
                            lines,
                        ));
                    }
                }
            }
            Err(error) => warnings.push(format!(
                "continuity/file-context wiki index unavailable: {error}"
            )),
        }
    }

    // File-addressed domain guidance: per-domain blocks under the same laws.
    // A pattern is matched against the project-relative path alone, so the
    // folders above the project never choose its guidance (round 55 item 8b).
    for domain in domains {
        let matched = domain
            .path_patterns
            .iter()
            .any(|pattern| glob_matches(pattern, &relative_path));
        if !matched {
            continue;
        }
        let (ordinary, standing) = render_guidance_lines(domain);
        let hash = dedup_hash(&format!("{}@{relative_path}", domain.id), &ordinary);
        // Same law as every other reaction; only the key and the header
        // differ here.
        let decision =
            decide_injection(ordinary, standing, seen(index, scope, &hash, &mut warnings));
        if decision.suppressed {
            continue;
        }
        if decision.record {
            if let Err(error) = index.record_injection(scope, &hash) {
                warnings.push(format!(
                    "continuity/file-context ledger unavailable: {error}"
                ));
            }
        }
        let (deduped, has_standing) = (decision.deduped, decision.has_standing);
        let header = format!(
            "[continuity/file-context] domain {} armed for {relative_path} — horizon: {}; \
             source: {} revision {};{}{}",
            domain.id,
            domain
                .horizon_range
                .as_ref()
                .map(|range| range.render())
                .unwrap_or_else(|| "unbounded".into()),
            domain.source,
            domain.revision,
            if deduped {
                " ordinary payload deduped (unchanged rendered content);"
            } else {
                ""
            },
            if has_standing {
                " standing rules reasserted (dedup-exempt by classification)"
            } else {
                ""
            }
        );
        blocks.push(Block {
            header,
            standing: decision.standing,
            ordinary: decision.ordinary,
        });
    }
    (blocks, warnings)
}

/// Ledger access with the fail-open law: trouble becomes a warning and the
/// injection proceeds.
fn seen(index: &Index, scope: &str, hash: &str, warnings: &mut Vec<String>) -> bool {
    match index.injection_seen(scope, hash) {
        Ok(seen) => seen,
        Err(error) => {
            warnings.push(format!(
                "continuity/file-context dedup check unavailable: {error}"
            ));
            false
        }
    }
}
