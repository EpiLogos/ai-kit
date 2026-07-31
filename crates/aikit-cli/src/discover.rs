//! Where am I, and what does that make the context?
//!
//! Two questions, kept apart on purpose:
//!
//! * **The filesystem question** — walking up from the cwd for `.aikit/` markers
//!   and building the root→cwd chain of project profiles. This is
//!   [`discover_project`], and it touches only the disk.
//! * **The environment question** — turning the `AIKIT_*` variables a session or
//!   task exports into a [`ContextDescriptor`]. This is [`descriptor_from`], and
//!   it touches only the passed-in lookup, never the real process environment, so
//!   it is testable and free of hidden global state.
//!
//! Isolation is the load-bearing default: an absent or unparseable
//! `AIKIT_ISOLATION` yields [`Isolation::Shared`], never a guessed worktree. A
//! task that shares its tree is written down as sharing it.

use std::path::{Path, PathBuf};

use aikit_core::context::{ContextDescriptor, Isolation};
use aikit_core::id::{ContextId, ProjectId, SessionId};
use aikit_core::platform::{MuxKind, Platform, TargetId};

/// The name of the per-project marker directory.
pub const MARKER: &str = ".aikit";
/// The committed project profile inside a marker directory.
pub const PROFILE_FILE: &str = "profile.toml";
/// The git-ignored private project profile inside a marker directory.
pub const PROFILE_LOCAL_FILE: &str = "profile.local.toml";

/// One `.aikit/` marker on the path from the repo root to the cwd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLayer {
    /// The directory that owns this `.aikit/`.
    pub dir: PathBuf,
    /// Distance from the repo root: 0 at the root, increasing towards the cwd,
    /// which is exactly the `depth` the resolver uses to break precedence ties.
    pub depth: u32,
}

impl ProjectLayer {
    /// The committed profile path, whether or not it exists.
    pub fn profile(&self) -> PathBuf {
        self.dir.join(MARKER).join(PROFILE_FILE)
    }

    /// The private profile path, whether or not it exists.
    pub fn profile_local(&self) -> PathBuf {
        self.dir.join(MARKER).join(PROFILE_LOCAL_FILE)
    }
}

/// A discovered project: its root and the ordered chain of profile markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredProject {
    /// The topmost ancestor carrying a `.aikit/` marker.
    pub root: PathBuf,
    /// Markers from the root down to the cwd, depth increasing.
    pub chain: Vec<ProjectLayer>,
}

/// Walk up from `start` collecting every ancestor that carries a `.aikit/`
/// marker, returning the topmost as the project root and the whole chain ordered
/// root→cwd.
///
/// Returning `None` means the user is outside any AIKit project; the caller then
/// resolves against the global scope only. This never reads the marker's
/// contents — parsing the profiles is the store's job, done later and only for
/// the layers that exist.
pub fn discover_project(start: &Path) -> Option<DiscoveredProject> {
    discover_project_excluding(start, None)
}

/// Discover project markers while excluding the operational AIKit home.
///
/// The default store is `~/.aikit`, which has the same basename as a project
/// marker. Treating that store as a marker makes every directory below `$HOME`
/// one enormous project. Production callers know the resolved store path and
/// pass it here; the simpler [`discover_project`] remains useful for pure
/// filesystem callers and tests.
pub fn discover_project_excluding(
    start: &Path,
    excluded_marker: Option<&Path>,
) -> Option<DiscoveredProject> {
    let excluded = excluded_marker.into_iter().collect::<Vec<_>>();
    discover_project_excluding_many(start, &excluded)
}

/// The multi-store form used when an explicit `AIKIT_HOME` coexists with the
/// default store under `$HOME`.
pub fn discover_project_excluding_many(
    start: &Path,
    excluded_markers: &[&Path],
) -> Option<DiscoveredProject> {
    let mut markers: Vec<PathBuf> = Vec::new();
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        let marker = dir.join(MARKER);
        if marker.is_dir()
            && !excluded_markers
                .iter()
                .any(|excluded| same_location(&marker, excluded))
        {
            markers.push(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    if markers.is_empty() {
        return None;
    }
    // `markers` is deepest-first; the root is the last one we pushed.
    markers.reverse();
    let root = markers[0].clone();
    let chain = markers
        .into_iter()
        .enumerate()
        .map(|(depth, dir)| ProjectLayer {
            dir,
            depth: depth as u32,
        })
        .collect();
    Some(DiscoveredProject { root, chain })
}

fn same_location(left: &Path, right: &Path) -> bool {
    left == right
        || matches!(
            (std::fs::canonicalize(left), std::fs::canonicalize(right)),
            (Ok(left), Ok(right)) if left == right
        )
}

/// Build a [`ContextDescriptor`] from a project root and an environment lookup.
///
/// The lookup is `Fn(&str) -> Option<String>` so callers pass either
/// `|k| std::env::var(k).ok()` in production or a fixture map in tests. Nothing
/// here reads the ambient process environment directly.
pub fn descriptor_from<F>(project_root: &Path, env: F) -> ContextDescriptor
where
    F: Fn(&str) -> Option<String>,
{
    let context_id = env("AIKIT_CONTEXT_ID")
        .and_then(|v| ContextId::parse(&v).ok())
        .unwrap_or_else(ContextId::generate);
    let session_id = env("AIKIT_SESSION_ID").and_then(|v| SessionId::parse(&v).ok());
    let project_id = env("AIKIT_PROJECT_ID").and_then(|v| ProjectId::parse(&v).ok());
    let task = env("AIKIT_TASK").filter(|s| !s.is_empty());
    let isolation = parse_isolation(env("AIKIT_ISOLATION").as_deref());
    let mux = env("AIKIT_MUX").and_then(|v| v.parse::<MuxKind>().ok());
    let host = env("AIKIT_HOST")
        .or_else(|| env("HOSTNAME"))
        .or_else(|| env("HOST"))
        .unwrap_or_else(|| "localhost".to_string());

    ContextDescriptor {
        context_id,
        session_id,
        project_id,
        project_root: Some(project_root.to_path_buf()),
        task,
        isolation,
        platform: Platform::current(),
        targets: vec![
            TargetId::shell(),
            TargetId::claude_code(),
            TargetId::codex(),
        ],
        mux,
        host,
    }
}

/// Parse `AIKIT_ISOLATION`, degrading honestly to [`Isolation::Shared`].
///
/// An unrecognised value is not an error: it means "AIKit does not know how to
/// isolate this, so treat it as shared", which is the safe reading — a shared
/// tree never pretends to be private, whereas a guessed worktree would.
fn parse_isolation(value: Option<&str>) -> Isolation {
    match value {
        Some("worktree") => Isolation::Worktree,
        Some("directory") => Isolation::Directory,
        _ => Isolation::Shared,
    }
}

/// The descriptor for "not in any project": a plain global context.
pub fn global_descriptor<F>(env: F) -> ContextDescriptor
where
    F: Fn(&str) -> Option<String>,
{
    let mut d = descriptor_from(Path::new("."), &env);
    d.project_root = None;
    d
}
