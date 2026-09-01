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
use aikit_core::resolve::AppliedSkillUsageOverlay;
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
    pub fn project(&self, prefix: &Path, mode: MaterializationMode) -> Result<Vec<ProjectionItem>> {
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

    /// Project the immutable source plus ordered, additive orientation. Skills
    /// with no overlay keep the one-link fast path; an Effective Skill writes
    /// only its generated `SKILL.md` and links/copies every companion file.
    pub fn project_effective(
        &self,
        prefix: &Path,
        mode: MaterializationMode,
        overlays: &[AppliedSkillUsageOverlay],
    ) -> Result<Vec<ProjectionItem>> {
        if overlays.is_empty() {
            return self.project(prefix, mode);
        }

        let destination = prefix.join(&self.name);
        let source = std::fs::read_to_string(self.root.join(SKILL_FILE)).map_err(|error| {
            AikitError::new(
                "skill.unreadable",
                format!(
                    "could not read {}: {error}",
                    self.root.join(SKILL_FILE).display()
                ),
            )
        })?;
        let rendered = render_effective_skill(&source, overlays)?;
        let mut items = vec![ProjectionItem::write(
            destination.join(SKILL_FILE),
            rendered,
        )?];
        for relative in self.files.iter().filter(|path| path.as_str() != SKILL_FILE) {
            let from = self.root.join(relative);
            let to = destination.join(relative);
            items.push(match mode {
                MaterializationMode::Copy => ProjectionItem::copy(from, to)?,
                MaterializationMode::Auto | MaterializationMode::Link => {
                    ProjectionItem::link(from, to)?
                }
            });
        }
        items.sort_by(|left, right| left.destination().cmp(&right.destination()));
        Ok(items)
    }

    pub fn effective_markdown(&self, overlays: &[AppliedSkillUsageOverlay]) -> Result<String> {
        let source = std::fs::read_to_string(self.root.join(SKILL_FILE)).map_err(|error| {
            AikitError::new(
                "skill.unreadable",
                format!(
                    "could not read {}: {error}",
                    self.root.join(SKILL_FILE).display()
                ),
            )
        })?;
        if overlays.is_empty() {
            Ok(source)
        } else {
            render_effective_skill(&source, overlays)
        }
    }

    /// The one-line index entry a broker skill lists this under.
    pub fn summary(&self) -> String {
        format!("{}: {}", self.name, first_sentence(&self.description))
    }
}

fn render_effective_skill(source: &str, overlays: &[AppliedSkillUsageOverlay]) -> Result<String> {
    let (frontmatter, body) = split_frontmatter(source)?;
    let mut yaml: serde_yaml::Mapping = serde_yaml::from_str(frontmatter).map_err(|error| {
        AikitError::new(
            "skill.invalid",
            format!("could not parse {SKILL_FILE} frontmatter as YAML: {error}"),
        )
    })?;
    let description_key = serde_yaml::Value::String("description".into());
    let description = yaml
        .get(&description_key)
        .and_then(serde_yaml::Value::as_str)
        .ok_or_else(|| {
            AikitError::new(
                "skill.invalid",
                format!("{SKILL_FILE} description is not a YAML string"),
            )
        })?;
    let additions: Vec<&str> = overlays
        .iter()
        .filter_map(|overlay| overlay.description.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect();
    if !additions.is_empty() {
        yaml.insert(
            description_key,
            serde_yaml::Value::String(format!("{} {}", description.trim(), additions.join(" "))),
        );
    }
    let mut encoded = serde_yaml::to_string(&yaml).map_err(|error| {
        AikitError::new(
            "skill.invalid",
            format!("could not render effective {SKILL_FILE} frontmatter: {error}"),
        )
    })?;
    if let Some(without_marker) = encoded.strip_prefix("---\n") {
        encoded = without_marker.to_string();
    }

    let mut rendered = format!("---\n{encoded}---\n{body}");
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    rendered.push_str(
        "\n## AIKit Skill Usage Overlay\n\n\
         > The following is user-authoritative orienting augmentation for this context. \
         It is additive to the immutable upstream skill: when it gives more specific \
         contextual direction, the more specific guidance governs. It does not change \
         invocation policy, permissions, trust, or payload identity.\n",
    );
    for overlay in overlays {
        rendered.push_str(&format!(
            "\n### {} augmentation\n\n- Scope: {}\n- Source: {}\n",
            overlay.scope.as_str(),
            overlay.scope.as_str(),
            overlay.origin
        ));
        if let Some(profile) = &overlay.via_profile {
            rendered.push_str(&format!("- Via profile: {profile}\n"));
        }
        if let Some(revision) = &overlay.reviewed_against {
            rendered.push_str(&format!("- Reviewed against: {revision}\n"));
        }
        if let Some(description) = &overlay.description {
            rendered.push_str(&format!(
                "\n**Routing orientation:** {}\n",
                description.trim()
            ));
        }
        if let Some(guidance) = &overlay.guidance {
            rendered.push('\n');
            rendered.push_str(guidance.trim());
            rendered.push('\n');
        }
    }
    Ok(rendered)
}

fn split_frontmatter(source: &str) -> Result<(&str, &str)> {
    let mut offset = 0usize;
    let mut lines = source.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if first.trim_end_matches(['\r', '\n']).trim() != "---" {
        return Err(AikitError::new(
            "skill.invalid",
            format!("this {SKILL_FILE} has no frontmatter"),
        ));
    }
    offset += first.len();
    let content_start = offset;
    for line in lines {
        if line.trim_end_matches(['\r', '\n']).trim() == "---" {
            let frontmatter = &source[content_start..offset];
            let body = &source[offset + line.len()..];
            return Ok((frontmatter, body));
        }
        offset += line.len();
    }
    Err(AikitError::new(
        "skill.invalid",
        format!("this {SKILL_FILE}'s frontmatter is never closed"),
    ))
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

    let frontmatter =
        parse_frontmatter(&source).map_err(|e| e.with("skill", root.display().to_string()))?;

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
        rest.strip_prefix("\r\n")
            .or_else(|| rest.strip_prefix('\n'))
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
