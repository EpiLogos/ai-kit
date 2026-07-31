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

use sha2::Digest;

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

/// Foreign roots visible from a particular project. The npx `skills` CLI installs
/// project skills into `.agents/skills` beside `skills-lock.json`; only indexing
/// the global home root makes those project-local installations disappear from
/// AIKit's map.
pub fn roots_for(home: &Path, cwd: &Path) -> Vec<(String, PathBuf)> {
    let mut roots = default_roots(home);
    if let Some(project) = closest_ancestor_with(cwd, "skills-lock.json")
        .or_else(|| closest_ancestor_dir(cwd, ".agents/skills"))
    {
        let path = project.join(".agents/skills");
        let global = home.join(".agents/skills");
        if path != global && !roots.iter().any(|(_, existing)| existing == &path) {
            roots.push(("@project-agents".to_string(), path));
        }
    }
    roots
}

fn closest_ancestor_with(start: &Path, file: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|ancestor| ancestor.join(file).is_file())
        .map(Path::to_path_buf)
}

fn closest_ancestor_dir(start: &Path, dir: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|ancestor| ancestor.join(dir).is_dir())
        .map(Path::to_path_buf)
}

// ---------------------------------------------------------------------------
// npx `skills` provenance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpxScope {
    Global,
    Project,
}

impl NpxScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpxSkillEntry {
    pub name: String,
    pub source: Option<String>,
    pub source_type: Option<String>,
    pub source_url: Option<String>,
    pub reference: Option<String>,
    pub skill_path: Option<String>,
    pub expected_hash: Option<String>,
    pub actual_hash: Option<String>,
    pub hash_matches: Option<bool>,
    pub installed_path: Option<PathBuf>,
    pub installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpxLock {
    pub scope: NpxScope,
    pub path: PathBuf,
    pub version: Option<u64>,
    pub supported: bool,
    pub entries: Vec<NpxSkillEntry>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NpxSurvey {
    pub locks: Vec<NpxLock>,
}

impl NpxSurvey {
    pub fn entries(&self) -> usize {
        self.locks.iter().map(|lock| lock.entries.len()).sum()
    }
}

/// Read the two lock authorities used by the npx `skills` CLI. Unknown fields are
/// ignored in memory and the files are never rewritten. Unknown schema versions
/// are surfaced but deliberately not interpreted.
pub fn survey_npx_skills(home: &Path, cwd: &Path) -> NpxSurvey {
    let xdg_state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    survey_npx_skills_at(home, cwd, xdg_state.as_deref())
}

pub fn survey_npx_skills_at(home: &Path, cwd: &Path, xdg_state_home: Option<&Path>) -> NpxSurvey {
    let global_lock = xdg_state_home
        .map(|state| state.join("skills/.skill-lock.json"))
        .unwrap_or_else(|| home.join(".agents/.skill-lock.json"));
    let mut candidates = vec![(
        NpxScope::Global,
        global_lock,
        home.join(".agents/skills"),
        3,
    )];
    if let Some(project) = closest_ancestor_with(cwd, "skills-lock.json") {
        candidates.push((
            NpxScope::Project,
            project.join("skills-lock.json"),
            project.join(".agents/skills"),
            1,
        ));
    }

    let locks = candidates
        .into_iter()
        .filter(|(_, path, _, _)| path.is_file())
        .map(|(scope, path, root, supported_version)| {
            parse_npx_lock(scope, path, root, supported_version)
        })
        .collect();
    NpxSurvey { locks }
}

fn parse_npx_lock(
    scope: NpxScope,
    path: PathBuf,
    root: PathBuf,
    supported_version: u64,
) -> NpxLock {
    let parsed = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let Some(value) = parsed else {
        return NpxLock {
            scope,
            path,
            version: None,
            supported: false,
            entries: vec![],
            note: Some("the lockfile is unreadable or invalid JSON".to_string()),
        };
    };
    let version = value.get("version").and_then(|v| v.as_u64());
    if version != Some(supported_version) {
        return NpxLock {
            scope,
            path,
            version,
            supported: false,
            entries: vec![],
            note: Some(format!(
                "unsupported npx skills lock version {}; expected {supported_version}, so entries were not guessed",
                version
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "missing".to_string())
            )),
        };
    }

    let entries = value
        .get("skills")
        .and_then(|skills| skills.as_object())
        .into_iter()
        .flat_map(|skills| skills.iter())
        .map(|(name, entry)| {
            let installed_path = safe_child(&root, name);
            let installed = installed_path.as_ref().is_some_and(|path| path.is_dir());
            let expected_hash = string_field(entry, "skillFolderHash")
                .or_else(|| string_field(entry, "computedHash"));
            let actual_hash = installed_path
                .as_deref()
                .filter(|_| installed)
                .and_then(|path| {
                    expected_hash
                        .as_deref()
                        .and_then(|hash| skill_hash(path, hash))
                });
            let hash_matches = expected_hash
                .as_ref()
                .zip(actual_hash.as_ref())
                .map(|(expected, actual)| expected.eq_ignore_ascii_case(actual));
            NpxSkillEntry {
                name: name.clone(),
                source: string_field(entry, "source"),
                source_type: string_field(entry, "sourceType"),
                source_url: string_field(entry, "sourceUrl"),
                reference: string_field(entry, "ref"),
                skill_path: string_field(entry, "skillPath"),
                expected_hash,
                actual_hash,
                hash_matches,
                installed_path,
                installed,
            }
        })
        .collect();

    NpxLock {
        scope,
        path,
        version,
        supported: true,
        entries,
        note: None,
    }
}

/// Compute the same two hashes written by skills 1.5.x.
///
/// Project locks use a SHA-256 over sorted `(relative path, contents)` pairs.
/// GitHub-backed global locks use the Git tree object's SHA-1. The expected
/// digest length selects the lock's declared algorithm; an unknown shape is
/// deliberately not guessed.
fn skill_hash(root: &Path, expected: &str) -> Option<String> {
    match expected.len() {
        64 if expected.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            flat_folder_sha256(root).ok()
        }
        40 if expected.bytes().all(|byte| byte.is_ascii_hexdigit()) => git_tree_sha1(root).ok(),
        _ => None,
    }
}

fn collected_files(root: &Path) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    // skills 1.5.x ignores symlink Dirents. Following one would disagree with
    // the lock algorithm and could read content outside the skill root.
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(std::io::Error::other)?
            .to_string_lossy()
            .replace('\\', "/");
        if relative
            .split('/')
            .any(|part| matches!(part, ".git" | "node_modules"))
        {
            continue;
        }
        files.push((relative, std::fs::read(entry.path())?));
    }
    sort_like_skills_cli(&mut files)?;
    Ok(files)
}

/// `skills` 1.5.x calls JavaScript's default `String.localeCompare` before
/// hashing. That order is ICU- and locale-sensitive: byte ordering, Unicode
/// scalar ordering, and lowercase approximations all produce false drift for
/// valid filenames. A machine with an npx lock necessarily has a Node runtime;
/// use that runtime's exact comparator, passing filenames as JSON over stdin so
/// no path is ever interpreted as code or a shell argument. If Node has gone
/// missing, hash verification remains unknown rather than reporting false drift.
fn sort_like_skills_cli(files: &mut [(String, Vec<u8>)]) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind, Write};
    use std::process::{Command, Stdio};

    const SORT_SCRIPT: &str = r#"
const fs = require("fs");
const paths = JSON.parse(fs.readFileSync(0, "utf8"));
paths.sort((left, right) => left.localeCompare(right));
process.stdout.write(JSON.stringify(paths));
"#;

    let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
    let input = serde_json::to_vec(&paths).map_err(Error::other)?;
    let mut child = Command::new("node")
        .args(["-e", SORT_SCRIPT])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| Error::new(ErrorKind::BrokenPipe, "Node stdin was not available"))?
        .write_all(&input)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(Error::other(format!(
            "Node localeCompare failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let ordered: Vec<String> = serde_json::from_slice(&output.stdout).map_err(Error::other)?;
    if ordered.len() != files.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Node localeCompare returned the wrong number of paths",
        ));
    }
    let ranks: std::collections::HashMap<String, usize> = ordered
        .into_iter()
        .enumerate()
        .map(|(rank, path)| (path, rank))
        .collect();
    if ranks.len() != files.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "skill paths are not unique after Unicode conversion",
        ));
    }
    files.sort_by_key(|(path, _)| ranks.get(path).copied().unwrap_or(usize::MAX));
    Ok(())
}

fn flat_folder_sha256(root: &Path) -> std::io::Result<String> {
    let mut digest = sha2::Sha256::new();
    for (path, contents) in collected_files(root)? {
        digest.update(path.as_bytes());
        digest.update(contents);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn git_object_sha1(kind: &str, contents: &[u8]) -> [u8; 20] {
    let mut digest = sha1::Sha1::new();
    digest.update(format!("{kind} {}\0", contents.len()).as_bytes());
    digest.update(contents);
    digest.finalize().into()
}

fn git_tree_sha1(root: &Path) -> std::io::Result<String> {
    let root = std::fs::canonicalize(root)?;
    let digest = git_tree_digest(&root)?;
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn git_tree_digest(directory: &Path) -> std::io::Result<[u8; 20]> {
    let mut entries = Vec::<(Vec<u8>, Vec<u8>, [u8; 20])>::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().as_bytes().to_vec();
        if name == b".git" || name == b"node_modules" {
            continue;
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_dir() {
            let mut sort_key = name.clone();
            sort_key.push(b'/');
            entries.push((
                sort_key,
                tree_record(b"40000", &name),
                git_tree_digest(&path)?,
            ));
        } else if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&path)?;
            let bytes = target.to_string_lossy().as_bytes().to_vec();
            entries.push((
                name.clone(),
                tree_record(b"120000", &name),
                git_object_sha1("blob", &bytes),
            ));
        } else if metadata.file_type().is_file() {
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            let mode = if executable { b"100755" } else { b"100644" };
            entries.push((
                name.clone(),
                tree_record(mode, &name),
                git_object_sha1("blob", &std::fs::read(&path)?),
            ));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut contents = Vec::new();
    for (_, mut record, digest) in entries {
        contents.append(&mut record);
        contents.extend_from_slice(&digest);
    }
    Ok(git_object_sha1("tree", &contents))
}

fn tree_record(mode: &[u8], name: &[u8]) -> Vec<u8> {
    let mut record = Vec::with_capacity(mode.len() + name.len() + 2);
    record.extend_from_slice(mode);
    record.push(b' ');
    record.extend_from_slice(name);
    record.push(0);
    record
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn safe_child(root: &Path, name: &str) -> Option<PathBuf> {
    let mut components = Path::new(name).components();
    let one = components.next()?;
    if components.next().is_some() || !matches!(one, std::path::Component::Normal(_)) {
        return None;
    }
    Some(root.join(name))
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

        // A dead symlink is one whose target *does not exist*. Checked before
        // `is_dir` (which follows the link and reports false for a dead one), and
        // narrowed to `NotFound`: a target behind a permission-denied component or
        // on an unmounted filesystem resolves fine for whoever can read it, and
        // counting it as rotted would inflate the problem count and send a user
        // hunting for a link that is not broken.
        if path.is_symlink() {
            if let Err(error) = std::fs::metadata(&path) {
                if error.kind() == std::io::ErrorKind::NotFound {
                    root.dead_symlinks += 1;
                }
                continue;
            }
        }
        if !path.is_dir() {
            continue;
        }

        if path.join(agent_skills::SKILL_FILE).exists() {
            match agent_skills::validate(&path) {
                Ok(_) => root.skills += 1,
                Err(_) => root.missing_frontmatter += 1,
            }
        }

        // Recurse independently of whether this directory is itself a skill: a
        // directory can be both a skill and a container of skills, and hanging the
        // recursion off the `else` silently swallowed the whole subtree.
        if depth == 0 {
            scan_dir(&path, depth + 1, root);
        }
    }
}

// ---------------------------------------------------------------------------
// Foreign hooks: scripts on disk versus scripts actually wired
// ---------------------------------------------------------------------------

/// A hook script found on disk, and whether anything actually calls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignHook {
    pub name: String,
    pub path: PathBuf,
    /// `true` when a client's settings reference this path.
    pub wired: bool,
}

/// What a hook survey found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookSurvey {
    pub hooks: Vec<ForeignHook>,
}

impl HookSurvey {
    pub fn wired(&self) -> usize {
        self.hooks.iter().filter(|h| h.wired).count()
    }

    /// Scripts sitting on disk that nothing dispatches. These are the ones nobody
    /// can currently see: they look installed, they are executable, and they never
    /// run.
    pub fn orphaned(&self) -> Vec<&ForeignHook> {
        self.hooks.iter().filter(|h| !h.wired).collect()
    }
}

/// Survey a client's hook directory against the settings that would dispatch it.
///
/// Read-only, and deliberately generous about what counts as "wired": a settings
/// file that mentions the script's file name anywhere is taken as a reference. An
/// over-count here is a false negative (a script reported as wired when it is
/// not), which is much safer than telling somebody a live hook is dead.
pub fn survey_hooks(hooks_dir: &Path, settings: &[PathBuf]) -> HookSurvey {
    let mut referenced = String::new();
    for path in settings {
        if let Ok(text) = std::fs::read_to_string(path) {
            referenced.push_str(&text);
            referenced.push('\n');
        }
    }

    let Ok(entries) = std::fs::read_dir(hooks_dir) else {
        return HookSurvey::default();
    };

    let mut hooks = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        // Skip dotfiles; they are not hook scripts anybody registered.
        if name.starts_with('.') {
            continue;
        }
        // A directory can hold a hook (the `gitnexus/` shape), so it counts as an
        // entry and is wired if anything under it is referenced.
        let wired = referenced.contains(&name);
        hooks.push(ForeignHook { name, path, wired });
    }
    hooks.sort_by(|a, b| a.name.cmp(&b.name));
    HookSurvey { hooks }
}

/// The well-known hook directory and the settings files that could wire it.
pub fn default_hook_survey(home: &Path) -> HookSurvey {
    survey_hooks(
        &home.join(".claude/hooks"),
        &[
            home.join(".claude/settings.json"),
            home.join(".claude/settings.local.json"),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::sort_like_skills_cli;

    #[test]
    fn skills_cli_sort_preserves_nodes_case_and_unicode_collation() {
        let mut files = vec![
            ("SKILL.md".to_string(), Vec::new()),
            ("ä.txt".to_string(), Vec::new()),
            ("z.txt".to_string(), Vec::new()),
            ("A".to_string(), Vec::new()),
            ("a".to_string(), Vec::new()),
        ];
        sort_like_skills_cli(&mut files).unwrap();
        assert_eq!(
            files.into_iter().map(|(path, _)| path).collect::<Vec<_>>(),
            ["a", "A", "ä.txt", "SKILL.md", "z.txt"]
        );
    }
}
