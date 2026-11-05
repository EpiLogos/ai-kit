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

/// Remove a Project Specification entirely, together with the AIKit-owned
/// link it placed inside each bound directory. The bound projects' own files
/// are untouched.
pub fn unbind(home: &AikitHome, id: &str) -> Result<()> {
    validate_id(id)?;
    let path = project_file(home, id);
    if !path.is_file() {
        return Err(AikitError::new(
            "project.unknown",
            format!("no Project Specification named `{id}` is registered"),
        )
        .with("project", id));
    }
    let spec = read_spec(&path)?;
    for directory in &spec.directories {
        remove_aikit_owned_codex_link(home, directory)?;
    }
    fs::remove_file(&path).map_err(|error| io(&path, error))
}

pub fn resolve(home: &AikitHome, cwd: &Path) -> Result<Option<ProjectMatch>> {
    let cwd = fs::canonicalize(cwd).map_err(|error| {
        AikitError::new(
            "project.directory_unreadable",
            format!("could not resolve {}: {error}", cwd.display()),
        )
    })?;
    let mut candidates: Vec<(usize, ProjectSpec, PathBuf, &'static str)> = Vec::new();
    let specs = load_all(home)?;
    // Only a specification that names repositories asks which one `cwd` is.
    let repository = specs
        .iter()
        .any(|spec| !spec.repositories.is_empty())
        .then(|| repository_at(&cwd))
        .flatten();
    for spec in specs {
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

/// Whether the World's own ground sits at `repository` beneath a bound world
/// root, rather than being one of the World's member projects.
///
/// A bound root must not swallow the member projects beneath it — each of
/// those carries its own project ground and its own specification, and is
/// matched on its own terms. The World's own ground is the exception: the
/// root register is its own repository holding the World record, the Agent
/// profiles and the Agent sets, and it carries no project ground of its own
/// because it is the World's source rather than a project inside the World.
/// Without this, binding the world root binds nothing that the World actually
/// contains.
///
/// Only a direct child of a world root qualifies: member projects live a
/// level further down, under the directory that indexes them. The world root
/// is recognised the same way `adopt.rs:547` recognises it — a directory
/// holding `Control/` beside `.central/`.
fn world_ground(root: &Path, repository: &Path) -> bool {
    repository.parent() == Some(root)
        && root.join(".central").is_dir()
        && !repository
            .join("ProjectCentral")
            .join("project.json")
            .is_file()
}

fn same_repository_boundary(root: &Path, cwd: &Path) -> bool {
    let Some(cwd_git) = git_root(cwd) else {
        // `cwd` stands in no repository, so no repository boundary excludes it.
        return true;
    };
    if git_root(root).as_deref() == Some(cwd_git.as_path()) {
        return true;
    }
    // A different repository: inside the boundary when `cwd` is not beneath
    // `root` at all, or when it is the World's own ground.
    !cwd_git.starts_with(root) || world_ground(root, &cwd_git)
}

/// The environment that moves git's answer about where a repository is or
/// what it is configured with. Where any is set, git itself is asked.
const GIT_ENVIRONMENT: [&str; 11] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_CONFIG",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
];

fn git_environment_set() -> bool {
    GIT_ENVIRONMENT
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

/// The working tree `path` stands in, as `git rev-parse --show-toplevel`
/// answers for an ordinary checkout or a linked worktree: the nearest
/// directory, `path` included, holding a `.git` directory with a `HEAD` or a
/// `.git` file naming its gitdir. Read from the filesystem, since a process
/// per question cost every Service two git spawns; git is asked only where
/// its environment says otherwise.
fn git_root(path: &Path) -> Option<PathBuf> {
    if git_environment_set() {
        return git_toplevel(path);
    }
    let start = fs::canonicalize(path).ok()?;
    start
        .ancestors()
        .find(|dir| git_dir_of(dir).is_some())
        .map(Path::to_path_buf)
}

/// The gitdir a working tree's `.git` names: the directory itself, or the
/// one a `.git` file points at.
fn git_dir_of(dir: &Path) -> Option<PathBuf> {
    let marker = dir.join(".git");
    if marker.join("HEAD").is_file() {
        return Some(marker);
    }
    let text = fs::read_to_string(&marker).ok()?;
    let named = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    let gitdir = Path::new(named);
    Some(if gitdir.is_absolute() {
        gitdir.to_path_buf()
    } else {
        dir.join(gitdir)
    })
}

fn git_toplevel(path: &Path) -> Option<PathBuf> {
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
    let remote = match configured_origin(&root) {
        Some(known) => known?,
        None => git_origin(&root)?,
    };
    let identity = normalize_repository_identity(remote.trim()).ok()?;
    Some((identity, root))
}

fn git_origin(root: &Path) -> Option<String> {
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
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The git configuration outside the repository that `git remote get-url`
/// also reads: the user's and the system's, wherever this machine's git keeps
/// them.
fn shared_git_configs() -> Vec<PathBuf> {
    let mut configs: Vec<PathBuf> = [
        "/etc/gitconfig",
        "/opt/homebrew/etc/gitconfig",
        "/usr/local/etc/gitconfig",
        "/Library/Developer/CommandLineTools/usr/share/git-core/gitconfig",
        "/Applications/Xcode.app/Contents/Developer/usr/share/git-core/gitconfig",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = &home {
        configs.push(home.join(".gitconfig"));
    }
    match std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        Some(xdg) => configs.push(PathBuf::from(xdg).join("git").join("config")),
        None => configs.extend(home.map(|home| home.join(".config").join("git").join("config"))),
    }
    configs
}

/// What `git remote get-url origin` answers in `root`, read from the
/// repository's own configuration: `Some(None)` when it names no origin, and
/// `None` when the configuration does what this reading does not follow (an
/// include, a URL rewrite, a per-worktree config, an origin named outside the
/// repository, a quoted or continued value, git's own environment), so git
/// itself is asked.
fn configured_origin(root: &Path) -> Option<Option<String>> {
    if git_environment_set() {
        return None;
    }
    let gitdir = git_dir_of(root)?;
    let common = match fs::read_to_string(gitdir.join("commondir")) {
        Ok(text) => {
            let named = Path::new(text.trim());
            if named.is_absolute() {
                named.to_path_buf()
            } else {
                gitdir.join(named)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => gitdir,
        Err(_) => return None,
    };
    let local = fs::read_to_string(common.join("config")).ok()?;
    let followed = |text: &str| {
        let lowered = text.to_ascii_lowercase();
        !lowered.contains("include") && !lowered.contains("insteadof")
    };
    if !followed(&local) || local.to_ascii_lowercase().contains("worktreeconfig") {
        return None;
    }
    for shared in shared_git_configs() {
        match fs::read_to_string(&shared) {
            Ok(text) if !followed(&text) || text.to_ascii_lowercase().contains("[remote") => {
                return None
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    let mut in_origin = false;
    for line in local.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            let header = line.trim_end_matches(|c: char| c != ']');
            in_origin =
                header == "[remote \"origin\"]" || header.eq_ignore_ascii_case("[remote.origin]");
            continue;
        }
        if !in_origin {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("url") {
            continue;
        }
        let value = value.trim();
        if value.contains(['"', '\\']) || value.contains(['#', ';']) {
            return None;
        }
        return Some(Some(value.to_owned()));
    }
    Some(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn the_repository_read_from_its_files_is_the_one_git_answers() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
            || git_environment_set()
        {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let main = temp.path().join("main");
        fs::create_dir_all(main.join("src/deep")).unwrap();
        git(&main, &["init", "-q"]);
        git(
            &main,
            &[
                "-c",
                "user.email=a@b",
                "-c",
                "user.name=a",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "first",
            ],
        );
        // No origin yet: the files say so, as git does.
        assert_eq!(configured_origin(&main), Some(None));
        git(
            &main,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/epilogos/ai-kit.git",
            ],
        );
        let linked = temp.path().join("linked");
        git(&main, &["worktree", "add", "-q", linked.to_str().unwrap()]);
        for (at, top) in [(main.join("src/deep"), &main), (linked.clone(), &linked)] {
            let answered = PathBuf::from(git(&at, &["rev-parse", "--show-toplevel"]));
            assert_eq!(git_root(&at), Some(answered.clone()), "{at:?}");
            assert_eq!(git_root(&at).unwrap(), fs::canonicalize(top).unwrap());
            assert_eq!(
                configured_origin(&answered),
                Some(Some(git(&answered, &["remote", "get-url", "origin"])))
            );
            assert_eq!(
                repository_at(&at).map(|(identity, _)| identity),
                Some("github.com/epilogos/ai-kit".to_owned())
            );
        }
        assert_eq!(git_root(temp.path()), None);
        // A URL rewrite is git's to apply: the reading leaves it to git.
        git(
            &main,
            &[
                "config",
                "url.https://example.com/.insteadOf",
                "https://github.com/",
            ],
        );
        assert_eq!(configured_origin(&main), None);
        assert_eq!(
            repository_at(&main).map(|(identity, _)| identity),
            Some("example.com/epilogos/ai-kit".to_owned())
        );
    }
}
