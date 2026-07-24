//! The native Agent Skills form: validation, and projection that preserves it.
//!
//! A skill is a directory containing a required `SKILL.md` whose frontmatter
//! carries `name` and `description`, plus optional `scripts/`, `references/` and
//! `assets/`. That layout is *progressive disclosure*: the model reads the small
//! `SKILL.md` and follows the subdirectories only when the task calls for it.
//!
//! Everything here follows from taking that seriously:
//!
//! * `name` and `description` are required and validated, because a skill with no
//!   description is one a model can never decide to use.
//! * the `name` must survive becoming a directory, since it does become one.
//! * projection reproduces the tree rather than flattening it. A flattened skill
//!   still "works" in the sense that files exist, which is exactly what makes the
//!   bug expensive to find.
//!
//! ## Why the frontmatter parser is hand-written
//!
//! A YAML dependency would buy anchors, block scalars and type coercion that this
//! format does not use, and would turn a malformed field into a parse error with
//! no idea which skill it came from. What is needed is `key: value` lines, quoted
//! values, and an error that names the file and the problem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use aikit_core::projection::{MaterializationMode, ProjectionItem};
use aikit_core::{AikitError, Result};

/// The subdirectories the Agent Skills form defines, in the order they are
/// reported. Anything else in a skill directory is carried across untouched but
/// is not part of the disclosure structure.
pub const DISCLOSURE_DIRS: [&str; 3] = ["assets", "references", "scripts"];

pub const SKILL_FILE: &str = "SKILL.md";

/// A validated native Agent Skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSkill {
    pub root: PathBuf,
    /// Also the export name: the directory this skill becomes in a projection.
    pub name: String,
    pub description: String,
    /// Every file in the skill, relative to `root`, sorted.
    pub files: Vec<String>,
    /// Which of [`DISCLOSURE_DIRS`] are present.
    pub disclosure: Vec<String>,
}

impl AgentSkill {
    /// The projection items that reproduce this skill under `prefix`.
    ///
    /// `Link` is one item for the whole directory, which is both cheaper and
    /// automatically correct — a symlinked skill cannot drift from its capsule.
    /// `Copy` enumerates the files, preserving every subdirectory.
    pub fn project(
        &self,
        prefix: &Path,
        mode: MaterializationMode,
    ) -> Result<Vec<ProjectionItem>> {
        let destination = prefix.join(&self.name);
        match mode {
            MaterializationMode::Copy => self
                .files
                .iter()
                .map(|relative| {
                    ProjectionItem::copy(self.root.join(relative), destination.join(relative))
                })
                .collect(),
            // `Auto` on a target without symlinks is resolved to `Copy` by the
            // caller through `MaterializationMode::resolve_for`; reaching here
            // with `Auto` means links are wanted.
            MaterializationMode::Auto | MaterializationMode::Link => {
                Ok(vec![ProjectionItem::link(&self.root, destination)?])
            }
        }
    }

    /// The one-line index entry a broker skill lists this under.
    pub fn summary(&self) -> String {
        format!("{}: {}", self.name, first_sentence(&self.description))
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate a skill directory, or say precisely what is wrong with it.
pub fn validate(root: &Path) -> Result<AgentSkill> {
    let skill_file = root.join(SKILL_FILE);
    let source = std::fs::read_to_string(&skill_file).map_err(|e| {
        let detail = if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "`{}` has no {SKILL_FILE}; an Agent Skill is a directory with a {SKILL_FILE} in it",
                root.display()
            )
        } else {
            format!("could not read {}: {e}", skill_file.display())
        };
        AikitError::new("skill.invalid", detail).with("skill", root.display().to_string())
    })?;

    let frontmatter = parse_frontmatter(&source).map_err(|e| e.with("skill", root.display().to_string()))?;

    let invalid = |detail: String| {
        AikitError::new("skill.invalid", detail)
            .with("skill", root.display().to_string())
            .with("file", skill_file.display().to_string())
    };

    let name = frontmatter
        .get("name")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            invalid(format!(
                "`{}/{SKILL_FILE}` has no `name` in its frontmatter; the name is what the skill \
                 is called and what directory it becomes",
                root.display()
            ))
        })?
        .to_string();

    if !is_usable_directory_name(&name) {
        return Err(invalid(format!(
            "`{name}` is not a usable skill `name`: it becomes a directory in every projection, \
             so it may not be empty, `.`, `..`, or contain a path separator"
        )));
    }

    let description = frontmatter
        .get("description")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            invalid(format!(
                "`{}/{SKILL_FILE}` has no usable `description`; without one a model has nothing \
                 to decide by and the skill can never be chosen",
                root.display()
            ))
        })?
        .to_string();

    let files = collect_files(root);
    let disclosure = DISCLOSURE_DIRS
        .iter()
        .filter(|dir| root.join(dir).is_dir())
        .map(|dir| dir.to_string())
        .collect();

    Ok(AgentSkill {
        root: root.to_path_buf(),
        name,
        description,
        files,
        disclosure,
    })
}

/// Parse the leading `---` block of a `SKILL.md`.
pub fn parse_frontmatter(source: &str) -> Result<BTreeMap<String, String>> {
    let body = source.strip_prefix("---").and_then(|rest| {
        // Accept `---\n` and `---\r\n`, but not `----`.
        rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n'))
    });
    let Some(body) = body else {
        return Err(AikitError::new(
            "skill.invalid",
            format!(
                "this {SKILL_FILE} has no frontmatter; an Agent Skill starts with a `---` block \
                 declaring at least `name` and `description`"
            ),
        ));
    };

    let mut fields = BTreeMap::new();
    let mut closed = false;
    for line in body.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim() == "---" {
            closed = true;
            break;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            // A continuation or a list item. Ignored rather than rejected: the
            // format allows keys AIKit has no opinion about.
            continue;
        };
        fields.insert(key.trim().to_string(), unquote(value.trim()));
    }

    if !closed {
        return Err(AikitError::new(
            "skill.invalid",
            format!(
                "this {SKILL_FILE}'s frontmatter is never closed with a `---` line, so where the \
                 metadata ends and the instructions begin is undecidable"
            ),
        ));
    }
    Ok(fields)
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

/// The name becomes a directory in every projection, so it has to be one.
fn is_usable_directory_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && !name.starts_with('.')
}

/// Every file under `root`, relative and sorted, so a projection is deterministic.
fn collect_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(base, &path, out);
        } else if let Ok(relative) = path.strip_prefix(base) {
            // Forward slashes regardless of platform: these strings become
            // projection destinations and are compared in tests and lock files.
            out.push(
                relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
}

/// The first sentence of a description, for bounded index entries.
pub fn first_sentence(description: &str) -> String {
    let flattened = description.split_whitespace().collect::<Vec<_>>().join(" ");
    match flattened.find(". ") {
        Some(end) => flattened[..=end].trim().to_string(),
        None => flattened,
    }
}
