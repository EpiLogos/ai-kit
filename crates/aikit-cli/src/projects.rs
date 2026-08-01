//! Reusable project specifications and deterministic directory matching.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectDefaults {
    #[serde(default)]
    pub default_skill_sets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSpec {
    pub schema: u32,
    pub id: String,
    #[serde(default)]
    pub directories: Vec<PathBuf>,
    #[serde(default)]
    pub repositories: Vec<String>,
    #[serde(default = "yes")]
    pub inherit_default_skill_sets: bool,
    #[serde(default)]
    pub skill_sets: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectMatch {
    pub spec: ProjectSpec,
    pub root: PathBuf,
    pub matched_by: &'static str,
}

pub fn bind(
    home: &AikitHome,
    id: &str,
    directories: &[PathBuf],
    repositories: &[String],
    skill_sets: &[String],
    inherit_default_skill_sets: bool,
) -> Result<ProjectSpec> {
    validate_id(id)?;
    let path = project_file(home, id);
    let previous = if path.is_file() {
        Some(read_spec(&path)?)
    } else {
        None
    };
    if directories.is_empty() && repositories.is_empty() {
        return Err(AikitError::new(
            "project.no_matcher",
            "a Project Specification needs at least one directory or repository matcher",
        ));
    }
    let mut canonical_directories = Vec::new();
    for directory in directories {
        let canonical = fs::canonicalize(directory).map_err(|error| {
            AikitError::new(
                "project.directory_unreadable",
                format!("could not resolve {}: {error}", directory.display()),
            )
            .with("path", directory.display().to_string())
        })?;
        if !canonical.is_dir() {
            return Err(AikitError::new(
                "project.directory_unreadable",
                format!("{} is not a directory", canonical.display()),
            ));
        }
        canonical_directories.push(canonical);
    }
    canonical_directories.sort();
    canonical_directories.dedup();

    let mut repository_ids: Vec<String> = repositories
        .iter()
        .map(|value| normalize_repository_identity(value))
        .collect::<Result<_>>()?;
    repository_ids.sort();
    repository_ids.dedup();
    let sets = stable_unique(skill_sets.iter().cloned());

    let spec = ProjectSpec {
        schema: 1,
        id: id.to_string(),
        directories: canonical_directories,
        repositories: repository_ids,
        inherit_default_skill_sets,
        skill_sets: sets,
    };
    if let Some(previous) = previous {
        for removed in previous
            .directories
            .iter()
            .filter(|directory| !spec.directories.contains(directory))
        {
            remove_aikit_owned_codex_link(home, removed)?;
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| io(&path, error))?;
    }
    write_atomic(&path, &spec)?;
    Ok(spec)
}

pub fn resolve(home: &AikitHome, cwd: &Path) -> Result<Option<ProjectMatch>> {
    let cwd = fs::canonicalize(cwd).map_err(|error| {
        AikitError::new(
            "project.directory_unreadable",
            format!("could not resolve {}: {error}", cwd.display()),
        )
    })?;
    let mut candidates: Vec<(usize, ProjectSpec, PathBuf, &'static str)> = Vec::new();
    let repository = repository_at(&cwd);
    for spec in load_all(home)? {
        for directory in &spec.directories {
            if cwd.starts_with(directory) && same_repository_boundary(directory, &cwd) {
                candidates.push((
                    directory.components().count(),
                    spec.clone(),
                    directory.clone(),
                    "directory",
                ));
            }
        }
        if let Some((identity, root)) = &repository {
            if spec.repositories.contains(identity) {
                candidates.push((0, spec.clone(), root.clone(), "repository"));
            }
        }
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.id.cmp(&right.1.id))
            .then_with(|| left.2.cmp(&right.2))
    });
    let mut by_specificity = std::collections::BTreeMap::<usize, BTreeSet<String>>::new();
    for (specificity, spec, _, _) in &candidates {
        by_specificity
            .entry(*specificity)
            .or_default()
            .insert(spec.id.clone());
    }
    if let Some((specificity, ids)) = by_specificity.iter().find(|(_, ids)| ids.len() > 1) {
        return Err(AikitError::new(
            "project.ambiguous_binding",
            format!(
                "{} matches multiple Project Specifications at specificity {}: {}",
                cwd.display(),
                specificity,
                ids.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        ));
    }

    let mut combined_sets = Vec::new();
    for (_, matched, _, _) in &candidates {
        combined_sets.extend(matched.skill_sets.iter().cloned());
    }
    let (_, mut spec, root, matched_by) = candidates
        .last()
        .cloned()
        .expect("non-empty candidates checked above");
    if spec.inherit_default_skill_sets {
        let mut defaults = load_defaults(home)?.default_skill_sets;
        defaults.extend(combined_sets);
        combined_sets = defaults;
    }
    spec.skill_sets = stable_unique(combined_sets);
    Ok(Some(ProjectMatch {
        spec,
        root,
        matched_by,
    }))
}

pub fn set_defaults(home: &AikitHome, skill_sets: &[String]) -> Result<ProjectDefaults> {
    use std::str::FromStr;
    use toml_edit::{Array, DocumentMut, Item, Value};

    let sets = stable_unique(skill_sets.iter().cloned());
    let defaults = ProjectDefaults {
        default_skill_sets: sets.clone(),
    };
    let path = home.config_file();
    let mut document = match fs::read_to_string(&path) {
        Ok(text) => DocumentMut::from_str(&text).map_err(|error| {
            AikitError::new(
                "project.config_unreadable",
                format!("{} is not valid TOML: {error}", path.display()),
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
        Err(error) => return Err(io(&path, error)),
    };
    let mut array = Array::new();
    for set in sets {
        array.push(set);
    }
    document["default_skill_sets"] = Item::Value(Value::Array(array));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
    }
    let temporary = path.with_extension("toml.tmp");
    fs::write(&temporary, document.to_string()).map_err(|error| io(&temporary, error))?;
    fs::rename(&temporary, &path).map_err(|error| io(&path, error))?;
    Ok(defaults)
}

pub fn load_defaults(home: &AikitHome) -> Result<ProjectDefaults> {
    let path = home.config_file();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProjectDefaults::default())
        }
        Err(error) => return Err(io(&path, error)),
    };
    toml::from_str(&text).map_err(|error| {
        AikitError::new(
            "project.config_unreadable",
            format!("{} is not valid project defaults: {error}", path.display()),
        )
    })
}

pub fn load_all(home: &AikitHome) -> Result<Vec<ProjectSpec>> {
    let root = home.root().join("projects");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(&root, error)),
    };
    let mut specs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io(&root, error))?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        specs.push(read_spec(&entry.path())?);
    }
    specs.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(specs)
}

pub fn normalize_repository_identity(value: &str) -> Result<String> {
    let mut raw = value.trim().to_string();
    if raw.is_empty() {
        return Err(AikitError::new(
            "project.invalid_repository",
            "repository identity is empty",
        ));
    }
    if let Some((_, rest)) = raw.split_once("://") {
        raw = rest.to_string();
    } else if let Some((user_host, path)) = raw.split_once(':') {
        if user_host.contains('@') {
            raw = format!(
                "{}/{}",
                user_host.split('@').next_back().unwrap_or(user_host),
                path
            );
        }
    }
    if let Some((_, rest)) = raw.split_once('@') {
        raw = rest.to_string();
    }
    raw = raw
        .split(['?', '#'])
        .next()
        .unwrap_or(&raw)
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase();
    let parts: Vec<&str> = raw.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() < 3 {
        return Err(AikitError::new(
            "project.invalid_repository",
            format!("`{value}` cannot be normalized to host/owner/repository"),
        ));
    }
    Ok(parts.join("/"))
}

fn same_repository_boundary(root: &Path, cwd: &Path) -> bool {
    let root_git = git_root(root);
    let cwd_git = git_root(cwd);
    match (root_git, cwd_git) {
        (Some(left), Some(right)) => left == right,
        (None, Some(right)) => !right.starts_with(root) || right == root,
        _ => true,
    }
}

fn git_root(path: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            path.to_string_lossy().as_ref(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
}

fn repository_at(path: &Path) -> Option<(String, PathBuf)> {
    let root = git_root(path)?;
    let output = std::process::Command::new("git")
        .args([
            "-C",
            root.to_string_lossy().as_ref(),
            "remote",
            "get-url",
            "origin",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let remote = String::from_utf8_lossy(&output.stdout);
    let identity = normalize_repository_identity(remote.trim()).ok()?;
    Some((identity, root))
}

fn project_file(home: &AikitHome, id: &str) -> PathBuf {
    home.root().join("projects").join(format!("{id}.toml"))
}

fn read_spec(path: &Path) -> Result<ProjectSpec> {
    let text = fs::read_to_string(path).map_err(|error| io(path, error))?;
    toml::from_str(&text).map_err(|error| {
        AikitError::new(
            "project.spec_unreadable",
            format!(
                "{} is not a valid Project Specification: {error}",
                path.display()
            ),
        )
    })
}

fn remove_aikit_owned_codex_link(home: &AikitHome, project_root: &Path) -> Result<()> {
    let parent = project_root.join(".agents");
    let link = parent.join("skills");
    let metadata = match fs::symlink_metadata(&link) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io(&link, error)),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(());
    }
    let target = fs::read_link(&link).map_err(|error| io(&link, error))?;
    if !target.starts_with(home.contexts()) {
        return Ok(());
    }
    fs::remove_file(&link).map_err(|error| io(&link, error))?;
    let parent_is_empty = fs::read_dir(&parent)
        .map_err(|error| io(&parent, error))?
        .next()
        .is_none();
    if parent_is_empty {
        fs::remove_dir(&parent).map_err(|error| io(&parent, error))?;
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AikitError::new(
            "project.invalid_id",
            format!("`{id}` is not a safe Project Specification id"),
        ));
    }
    Ok(())
}

fn write_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = toml::to_string_pretty(value).map_err(|error| {
        AikitError::new(
            "project.write_failed",
            format!("could not serialize {}: {error}", path.display()),
        )
    })?;
    let temporary = path.with_extension("toml.tmp");
    fs::write(&temporary, text).map_err(|error| io(&temporary, error))?;
    fs::rename(&temporary, path).map_err(|error| io(path, error))
}

fn yes() -> bool {
    true
}

fn stable_unique(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn io(path: &Path, error: std::io::Error) -> AikitError {
    AikitError::new("project.io_failed", format!("{}: {error}", path.display()))
        .with("path", path.display().to_string())
}
