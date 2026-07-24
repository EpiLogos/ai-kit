//! Read-only discovery of the foreign skill roots already on the machine.
//!
//! `aikit init` discovers, it does not interrogate (SPEC-III §4.4, STANDARDS §5):
//! it indexes the skill trees that already exist — `~/.claude/skills`,
//! `~/.agents/skills`, `~/.hermes/skills`, a Codex tree, plugin caches — and shows
//! what it found, counting the problems a user cannot currently see: dead symlinks
//! and skills with no usable frontmatter. It writes nothing and asks nothing
//! before the first useful output.
//!
//! This is deliberately *read-only*. Turning a foreign root into something AIKit
//! owns is Adoption, which is a Procedure with an explicit confirmation (Spec II
//! §3) — discovery never crosses that line.

use std::path::{Path, PathBuf};

use aikit_adapters::clients::agent_skills;

/// A skill tree AIKit did not create but can see and index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignRoot {
    /// The `@`-prefixed short name a user sees, e.g. `@claude`.
    pub label: String,
    pub path: PathBuf,
    /// Directories that validate as a native Agent Skill.
    pub skills: usize,
    /// Symlinks whose target no longer resolves — the "someone's working setup
    /// has quietly rotted" problem the tree makes visible.
    pub dead_symlinks: usize,
    /// Directories that look like a skill (they have a `SKILL.md`) but whose
    /// frontmatter does not carry a usable `name`/`description`.
    pub missing_frontmatter: usize,
}

impl ForeignRoot {
    /// Total problems worth a user's attention.
    pub fn problems(&self) -> usize {
        self.dead_symlinks + self.missing_frontmatter
    }
}

/// The well-known foreign skill roots, resolved under a home directory.
///
/// Returned as `(label, path)` pairs whether or not they exist; [`discover`] skips
/// the ones that are absent, so a fresh machine simply reports fewer roots.
pub fn default_roots(home: &Path) -> Vec<(String, PathBuf)> {
    vec![
        ("@claude".to_string(), home.join(".claude/skills")),
        ("@agents".to_string(), home.join(".agents/skills")),
        ("@hermes".to_string(), home.join(".hermes/skills")),
        ("@codex".to_string(), home.join(".codex/skills")),
    ]
}

/// Discover every foreign root at `roots` that exists, read-only.
pub fn discover(roots: &[(String, PathBuf)]) -> Vec<ForeignRoot> {
    roots
        .iter()
        .filter_map(|(label, path)| scan_root(label, path))
        .collect()
}

/// Scan one root, or `None` if it does not exist on disk.
fn scan_root(label: &str, path: &Path) -> Option<ForeignRoot> {
    if !path.exists() {
        return None;
    }
    let mut root = ForeignRoot {
        label: label.to_string(),
        path: path.to_path_buf(),
        skills: 0,
        dead_symlinks: 0,
        missing_frontmatter: 0,
    };
    scan_dir(path, 0, &mut root);
    Some(root)
}

/// Walk a root at most two levels deep, so both the **flat** layout
/// (`<root>/<skill>/SKILL.md`) and the **two-level container** layout
/// (`<root>/<category>/<skill>/SKILL.md`, the Hermes shape) are indexed —
/// PRIOR-ART-ACTIONS #29: an indexer that walks one level deep misses whole
/// categories.
fn scan_dir(dir: &Path, depth: usize, root: &mut ForeignRoot) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();

        // A dead symlink: a symlink whose target does not resolve. Checked before
        // `is_dir` (which follows the link and would report false for a dead one).
        if path.is_symlink() && std::fs::metadata(&path).is_err() {
            root.dead_symlinks += 1;
            continue;
        }
        if !path.is_dir() {
            continue;
        }

        if path.join(agent_skills::SKILL_FILE).exists() {
            match agent_skills::validate(&path) {
                Ok(_) => root.skills += 1,
                Err(_) => root.missing_frontmatter += 1,
            }
        } else if depth == 0 {
            // A category container in the two-level layout: recurse exactly once.
            scan_dir(&path, depth + 1, root);
        }
    }
}
