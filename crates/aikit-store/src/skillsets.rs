//! Skill-sets on disk.
//!
//! A set is a **directory**, and this module is the thin layer that reads and
//! writes that shape. Sets live under `<home>/skillsets/`, deliberately **not**
//! under `capsules/`: they are not capsules, they have no kind, no revision and no
//! trust key, and resisting the urge to unify there is the point of the whole
//! concept (SPEC-III §7).
//!
//! ## The file format is "a directory, and optionally a note"
//!
//! Membership lives in `members` — one capsule id per line — because a set's whole
//! value proposition is that `mkdir` and a text editor are enough to make one. The
//! optional `set.toml` exists only for what a folder cannot say: a description, a
//! presentation order, and the globs that were expanded at authoring time.
//! No manifest, no problem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use aikit_core::id::CapsuleId;
use aikit_core::skillset::{SetMembership, SetProvenance, SkillSet};
use aikit_core::{AikitError, Result};

use crate::home::{create_dir_all, io_error, AikitHome};

/// One capsule id per line. The whole membership format.
const MEMBERS_FILE: &str = "members";
/// The optional note. Every field optional.
const SET_FILE: &str = "set.toml";

/// `set.toml` — present only when the folder cannot say something.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetFile {
    #[serde(default)]
    pub description: String,
    /// Members that live in another registry and are named rather than found.
    #[serde(default)]
    pub include: Vec<CapsuleId>,
    /// Presentation order, if it matters.
    #[serde(default)]
    pub order: Vec<String>,
    /// Globs expanded at authoring time, retained as provenance only. A new
    /// capsule matching one is *proposed*, never joined.
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// Where sets live under the home.
pub fn root(home: &AikitHome) -> PathBuf {
    home.root().join("skillsets")
}

/// The directory of one set.
pub fn dir(home: &AikitHome, name: &str) -> PathBuf {
    root(home).join(name)
}

/// Read every set under the home, nested sets included.
pub fn load_all(home: &AikitHome) -> Result<Vec<SkillSet>> {
    let base = root(home);
    let Ok(entries) = std::fs::read_dir(&base) else {
        // No sets yet is not an error: a fresh machine simply has none.
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            out.push(load_dir(&entry.path(), SetProvenance::Composed)?);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Read one set by name.
pub fn load(home: &AikitHome, name: &str) -> Result<SkillSet> {
    let path = dir(home, name);
    if !path.is_dir() {
        return Err(AikitError::new(
            "skillset.unknown",
            format!("there is no set `{name}`"),
        )
        .with("set", name.to_string()));
    }
    load_dir(&path, SetProvenance::Composed)
}

/// Read a set from a directory, recursing into nested sets.
pub fn load_dir(path: &Path, provenance: SetProvenance) -> Result<SkillSet> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut set = SkillSet::new(name, provenance.clone());

    // The optional note.
    let note_path = path.join(SET_FILE);
    if note_path.is_file() {
        let text = std::fs::read_to_string(&note_path)
            .map_err(|e| io_error("skillset.unreadable", &note_path, &e))?;
        let note: SetFile = toml::from_str(&text).map_err(|e| {
            AikitError::new(
                "skillset.malformed",
                format!("{} is not a readable set note: {e}", note_path.display()),
            )
            .with("path", note_path.display().to_string())
        })?;
        set.description = note.description;
        set.patterns = note.patterns;
        for id in note.include {
            set.members.insert(id, SetMembership::Explicit);
        }
    }

    // Membership: one id per line, blanks and `#` comments ignored so a human can
    // annotate the file they are expected to edit by hand.
    let members_path = path.join(MEMBERS_FILE);
    if members_path.is_file() {
        let text = std::fs::read_to_string(&members_path)
            .map_err(|e| io_error("skillset.unreadable", &members_path, &e))?;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let id = CapsuleId::parse(line).map_err(|e| {
                e.with("path", members_path.display().to_string())
            })?;
            set.members.insert(id, SetMembership::Explicit);
        }
    }

    // Nesting gives sub-sets, and that is the only composition a folder needs.
    if let Ok(entries) = std::fs::read_dir(path) {
        let mut children: Vec<SkillSet> = Vec::new();
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                children.push(load_dir(&entry.path(), provenance.clone())?);
            }
        }
        children.sort_by(|a, b| a.name.cmp(&b.name));
        set.children = children;
    }

    Ok(set)
}

/// Create a set. `mkdir` with a manifest only if there is something to say.
pub fn create(home: &AikitHome, name: &str, members: &[CapsuleId], patterns: &[String]) -> Result<SkillSet> {
    let path = dir(home, name);
    if path.exists() {
        return Err(AikitError::new(
            "skillset.exists",
            format!("a set named `{name}` already exists"),
        )
        .with("set", name.to_string()));
    }
    create_dir_all(&path)?;
    write_members(&path, members)?;

    if !patterns.is_empty() {
        let note = SetFile {
            patterns: patterns.to_vec(),
            ..Default::default()
        };
        let text = toml::to_string_pretty(&note).map_err(|e| {
            AikitError::new("skillset.unserializable", format!("could not write the set note: {e}"))
        })?;
        std::fs::write(path.join(SET_FILE), text)
            .map_err(|e| io_error("skillset.write_failed", &path.join(SET_FILE), &e))?;
    }
    load(home, name)
}

/// Add members to a set. Idempotent: a set is a set.
pub fn add(home: &AikitHome, name: &str, ids: &[CapsuleId]) -> Result<SkillSet> {
    let set = load(home, name)?;
    let mut members: Vec<CapsuleId> = set.members.keys().cloned().collect();
    for id in ids {
        if !members.contains(id) {
            members.push(id.clone());
        }
    }
    members.sort();
    write_members(&dir(home, name), &members)?;
    load(home, name)
}

/// Remove members from a set. Never deletes the capsule — a set is a view.
pub fn remove(home: &AikitHome, name: &str, ids: &[CapsuleId]) -> Result<SkillSet> {
    let set = load(home, name)?;
    let members: Vec<CapsuleId> = set
        .members
        .keys()
        .filter(|id| !ids.contains(id))
        .cloned()
        .collect();
    write_members(&dir(home, name), &members)?;
    load(home, name)
}

fn write_members(path: &Path, members: &[CapsuleId]) -> Result<()> {
    let mut text = String::from(
        "# One capsule id per line. A set is a folder; this is its membership.\n\
         # Blank lines and `#` comments are ignored.\n",
    );
    let mut sorted: Vec<&CapsuleId> = members.iter().collect();
    sorted.sort();
    for id in sorted {
        text.push_str(&id.to_string());
        text.push('\n');
    }
    let file = path.join(MEMBERS_FILE);
    std::fs::write(&file, text).map_err(|e| io_error("skillset.write_failed", &file, &e))
}

/// Index the observed sets under a foreign root: a real directory of skills that
/// already exists is already a set, and pointing a harness at it needs no import.
pub fn observe(root_path: &Path) -> Result<Vec<SkillSet>> {
    let Ok(entries) = std::fs::read_dir(root_path) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // A directory holding other skill directories is a set; one holding a
        // SKILL.md is a member, not a container.
        if path.join("SKILL.md").exists() {
            continue;
        }
        out.push(load_dir(
            &path,
            SetProvenance::Observed { path: path.clone() },
        )?);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Which sets a context points at, and what each would project.
pub fn projections(
    sets: &[SkillSet],
    view: &aikit_core::ResolvedView,
) -> BTreeMap<String, aikit_core::skillset::SetProjection> {
    sets.iter()
        .map(|set| (set.name.clone(), aikit_core::skillset::project(set, view)))
        .collect()
}
