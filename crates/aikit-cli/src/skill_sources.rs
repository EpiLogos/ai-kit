//! Managed Agent Skill sources.
//!
//! Mutable directories and Git checkouts are never projected directly. A sync
//! copies their complete skill trees into an immutable, content-addressed
//! candidate snapshot; promotion is the separate act that makes one snapshot
//! visible to the catalogue.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use aikit_adapters::clients::agent_skills;
use aikit_core::catalog::Catalog;
use aikit_core::{AikitError, RegistrySource, Result, TrustKey, TrustState};
use aikit_store::home::AikitHome;
use aikit_store::index::Index;
use aikit_store::registry::load_registry;
use aikit_store::trust::TrustStore;
use serde::{Deserialize, Serialize};

const SPEC_FILE: &str = "source.toml";
const STATE_FILE: &str = "state.toml";
const SNAPSHOT_FILE: &str = "snapshot.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSpec {
    pub schema: u32,
    pub id: String,
    #[serde(flatten)]
    pub kind: SourceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SourceKind {
    Directory {
        path: PathBuf,
    },
    Git {
        repository: String,
        revision: String,
        #[serde(default)]
        root: PathBuf,
    },
}

impl SourceKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Directory { .. } => "directory",
            Self::Git { .. } => "git",
        }
    }

    pub fn portable(&self) -> bool {
        matches!(self, Self::Git { .. })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceState {
    #[serde(default)]
    pub candidate_snapshot: Option<String>,
    #[serde(default)]
    pub active_snapshot: Option<String>,
    #[serde(default)]
    pub history: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSkill {
    pub id: String,
    pub name: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub schema: u32,
    pub source: String,
    pub digest: String,
    #[serde(default)]
    pub git_commit: Option<String>,
    pub skills: Vec<SnapshotSkill>,
}

#[derive(Debug, Clone)]
pub struct SourceStatus {
    pub spec: SourceSpec,
    pub state: SourceState,
    pub candidate: Option<SnapshotRecord>,
    pub active: Option<SnapshotRecord>,
}

#[derive(Serialize)]
struct CapsuleManifest<'a> {
    schema: u32,
    id: &'a str,
    kind: &'static str,
    name: &'a str,
    description: &'a str,
    skill: SkillSection,
}

#[derive(Serialize)]
struct SkillSection {
    root: &'static str,
}

pub fn add_directory(home: &AikitHome, id: &str, path: &Path) -> Result<SourceSpec> {
    validate_id(id)?;
    let path = fs::canonicalize(path).map_err(|error| {
        AikitError::new(
            "source.directory_unreadable",
            format!("could not resolve {}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })?;
    if !path.is_dir() {
        return Err(AikitError::new(
            "source.directory_unreadable",
            format!("{} is not a directory", path.display()),
        ));
    }
    write_new_spec(
        home,
        SourceSpec {
            schema: 1,
            id: id.to_string(),
            kind: SourceKind::Directory { path },
        },
    )
}

pub fn add_git(
    home: &AikitHome,
    id: &str,
    repository: &str,
    revision: &str,
    root: &Path,
) -> Result<SourceSpec> {
    validate_id(id)?;
    if repository.trim().is_empty()
        || repository.starts_with('-')
        || has_embedded_credentials(repository)
        || !is_exact_commit(revision)
        || !is_contained_relative(root)
    {
        return Err(AikitError::new(
            "source.invalid_git",
            "a Git source needs a repository, exact 40- or 64-hex commit, and contained relative skill root",
        ));
    }
    write_new_spec(
        home,
        SourceSpec {
            schema: 1,
            id: id.to_string(),
            kind: SourceKind::Git {
                repository: repository.to_string(),
                revision: revision.to_string(),
                root: root.to_path_buf(),
            },
        },
    )
}

pub fn set_revision(home: &AikitHome, id: &str, revision: &str) -> Result<SourceSpec> {
    if !is_exact_commit(revision) {
        return Err(AikitError::new(
            "source.invalid_git",
            "a Git source revision must be an exact 40- or 64-hex commit",
        ));
    }
    let mut spec = load_spec(home, id)?;
    match &mut spec.kind {
        SourceKind::Git {
            revision: current, ..
        } => *current = revision.to_string(),
        SourceKind::Directory { .. } => {
            return Err(AikitError::new(
                "source.not_git",
                format!("skill source `{id}` is a directory source"),
            ));
        }
    }
    let dir = source_dir(home, id);
    let mut state = load_state(home, id)?;
    state.candidate_snapshot = None;
    write_toml_atomic(&dir.join(STATE_FILE), &state)?;
    write_toml_atomic(&dir.join(SPEC_FILE), &spec)?;
    Ok(spec)
}

fn write_new_spec(home: &AikitHome, spec: SourceSpec) -> Result<SourceSpec> {
    let dir = source_dir(home, &spec.id);
    if dir.exists() {
        return Err(AikitError::new(
            "source.exists",
            format!("skill source `{}` already exists", spec.id),
        ));
    }
    fs::create_dir_all(dir.join("snapshots"))
        .map_err(|error| io("source.write_failed", &dir, error))?;
    write_toml_atomic(&dir.join(SPEC_FILE), &spec)?;
    write_toml_atomic(&dir.join(STATE_FILE), &SourceState::default())?;
    Ok(spec)
}

pub fn sync(home: &AikitHome, id: &str) -> Result<SnapshotRecord> {
    let spec = load_spec(home, id)?;
    let source_dir_path = source_dir(home, id);
    let staging = source_dir_path.join(format!(".staging-{}", ulid::Ulid::generate()));
    fs::create_dir_all(&staging).map_err(|error| io("source.snapshot_failed", &staging, error))?;

    let prepared = prepare_source(&spec, &staging);
    let (scan_root, git_commit) = match prepared {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    let built = build_snapshot(&spec, &scan_root, git_commit, &staging);
    let record = match built {
        Ok(record) => record,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    let checkout = staging.join("checkout");
    if checkout.exists() {
        fs::remove_dir_all(&checkout)
            .map_err(|error| io("source.snapshot_failed", &checkout, error))?;
    }

    let final_dir = source_dir_path.join("snapshots").join(&record.digest);
    if final_dir.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| io("source.snapshot_failed", &staging, error))?;
    } else {
        fs::rename(&staging, &final_dir)
            .map_err(|error| io("source.snapshot_failed", &final_dir, error))?;
    }

    let mut state = load_state(home, id)?;
    state.candidate_snapshot = Some(record.digest.clone());
    write_toml_atomic(&source_dir_path.join(STATE_FILE), &state)?;
    Ok(record)
}

fn prepare_source(spec: &SourceSpec, staging: &Path) -> Result<(PathBuf, Option<String>)> {
    match &spec.kind {
        SourceKind::Directory { path } => Ok((path.clone(), None)),
        SourceKind::Git {
            repository,
            revision,
            root,
        } => {
            if has_embedded_credentials(repository) {
                return Err(AikitError::new(
                    "source.credentials_not_allowed",
                    "Git source URLs must use credential helpers instead of embedded credentials",
                ));
            }
            let checkout = staging.join("checkout");
            run_git(&[
                "clone",
                "--quiet",
                "--no-checkout",
                repository,
                checkout.to_string_lossy().as_ref(),
            ])?;
            run_git(&[
                "-C",
                checkout.to_string_lossy().as_ref(),
                "checkout",
                "--quiet",
                "--detach",
                revision,
            ])?;
            let commit = git_output(&[
                "-C",
                checkout.to_string_lossy().as_ref(),
                "rev-parse",
                "HEAD",
            ])?;
            let scan = checkout.join(root);
            if !scan.is_dir() {
                return Err(AikitError::new(
                    "source.skill_root_missing",
                    format!(
                        "Git source `{}` has no skill root `{}`",
                        spec.id,
                        root.display()
                    ),
                ));
            }
            Ok((scan, Some(commit)))
        }
    }
}

fn build_snapshot(
    spec: &SourceSpec,
    scan_root: &Path,
    git_commit: Option<String>,
    staging: &Path,
) -> Result<SnapshotRecord> {
    let roots = discover_skills(scan_root)?;
    if roots.is_empty() {
        return Err(AikitError::new(
            "source.no_skills",
            format!("skill source `{}` contains no valid Agent Skills", spec.id),
        ));
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit-skill-source-snapshot-v2\n");
    if let Some(commit) = git_commit.as_deref() {
        hash_field(&mut hasher, "git-commit");
        hash_field(&mut hasher, commit);
    }
    let mut skills = Vec::new();
    let mut ids = BTreeSet::new();

    for root in roots {
        reject_symlinks(&root)?;
        let skill = agent_skills::validate(&root)?;
        let relative = root.strip_prefix(scan_root).unwrap_or(Path::new(""));
        let capsule_tail = if relative.as_os_str().is_empty() {
            skill.name.clone()
        } else {
            path_text(relative)
        };
        let id = format!("skill/{}/{capsule_tail}", spec.id);
        aikit_core::CapsuleId::parse(&id)?;
        if !ids.insert(id.clone()) {
            return Err(AikitError::new(
                "source.skill_collision",
                format!("more than one skill maps to `{id}`"),
            ));
        }
        let capsule_dir = staging.join("registry/capsules").join(&id);
        fs::create_dir_all(capsule_dir.join("payload"))
            .map_err(|error| io("source.snapshot_failed", &capsule_dir, error))?;
        let manifest = toml::to_string_pretty(&CapsuleManifest {
            schema: 1,
            id: &id,
            kind: "skill",
            name: &skill.name,
            description: &skill.description,
            skill: SkillSection { root: "payload" },
        })
        .map_err(|error| {
            AikitError::new(
                "source.snapshot_failed",
                format!("could not encode `{id}`: {error}"),
            )
        })?;
        fs::write(capsule_dir.join("manifest.toml"), manifest)
            .map_err(|error| io("source.snapshot_failed", &capsule_dir, error))?;

        for relative_file in &skill.files {
            let from = root.join(relative_file);
            let to = capsule_dir.join("payload").join(relative_file);
            let bytes =
                fs::read(&from).map_err(|error| io("source.snapshot_failed", &from, error))?;
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| io("source.snapshot_failed", parent, error))?;
            }
            fs::write(&to, &bytes).map_err(|error| io("source.snapshot_failed", &to, error))?;
            let mode = copy_permissions(&from, &to)?;
            hash_field(&mut hasher, &id);
            hash_field(&mut hasher, relative_file);
            hasher.update(&mode.to_le_bytes());
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        }
        skills.push(SnapshotSkill {
            id,
            name: skill.name,
            source_path: path_text(relative),
        });
    }
    skills.sort_by(|left, right| left.id.cmp(&right.id));
    let digest = hasher.finalize().to_hex().to_string();
    let record = SnapshotRecord {
        schema: 1,
        source: spec.id.clone(),
        digest,
        git_commit,
        skills,
    };
    write_toml_atomic(&staging.join(SNAPSHOT_FILE), &record)?;
    Ok(record)
}

pub fn promote(
    home: &AikitHome,
    id: &str,
    trust_all: bool,
    trust_skills: &[String],
) -> Result<(SnapshotRecord, usize)> {
    let mut state = load_state(home, id)?;
    let digest = state.candidate_snapshot.clone().ok_or_else(|| {
        AikitError::new(
            "source.no_candidate",
            format!("skill source `{id}` has no candidate snapshot; sync it first"),
        )
    })?;
    let record = load_snapshot(home, id, &digest)?;
    let registry = snapshot_dir(home, id, &digest).join("registry");
    let mut trusted = 0;
    let requested: BTreeSet<&str> = trust_skills.iter().map(String::as_str).collect();
    for capability in &requested {
        if !record.skills.iter().any(|skill| skill.id == *capability) {
            return Err(AikitError::new(
                "source.skill_not_in_candidate",
                format!("`{capability}` is not in source `{id}` candidate `{digest}`"),
            )
            .with("capability", capability.to_string()));
        }
    }
    if trust_all || !requested.is_empty() {
        let index = Index::open(&home.database())?;
        let store = TrustStore::new(&index);
        let source = RegistrySource::new(id.to_string());
        let loaded = load_registry(&registry, source.clone())?;
        for capsule in loaded.catalog.capsules() {
            if !trust_all && !requested.contains(capsule.id.to_string().as_str()) {
                continue;
            }
            let Some(revision) = &capsule.revision else {
                continue;
            };
            store.record(
                &TrustKey::new(source.clone(), capsule.id.clone(), revision.clone()),
                TrustState::Trusted,
                Some("explicit source promotion"),
            )?;
            trusted += 1;
        }
    }
    if state.active_snapshot.as_deref() != Some(&digest) {
        if let Some(previous) = state.active_snapshot.replace(digest.clone()) {
            state
                .history
                .retain(|item| item != &previous && item != &digest);
            state.history.push(previous);
        }
    }
    write_toml_atomic(&source_dir(home, id).join(STATE_FILE), &state)?;
    Ok((record, trusted))
}

pub fn rollback(home: &AikitHome, id: &str) -> Result<SnapshotRecord> {
    let mut state = load_state(home, id)?;
    let previous = state.history.pop().ok_or_else(|| {
        AikitError::new(
            "source.no_rollback",
            format!("skill source `{id}` has no promoted rollback point"),
        )
    })?;
    let record = load_snapshot(home, id, &previous)?;
    restore_previously_reviewed_trust(home, id, &previous)?;
    state.active_snapshot = Some(previous.clone());
    write_toml_atomic(&source_dir(home, id).join(STATE_FILE), &state)?;
    Ok(record)
}

fn restore_previously_reviewed_trust(home: &AikitHome, id: &str, digest: &str) -> Result<()> {
    let index = Index::open(&home.database())?;
    let store = TrustStore::new(&index);
    let source = RegistrySource::new(id.to_string());
    let registry = snapshot_dir(home, id, digest).join("registry");
    let loaded = load_registry(&registry, source.clone())?;
    for capsule in loaded.catalog.capsules() {
        let Some(revision) = &capsule.revision else {
            continue;
        };
        let key = TrustKey::new(source.clone(), capsule.id.clone(), revision.clone());
        if matches!(
            store.state_of(&key)?,
            TrustState::Reviewed | TrustState::Trusted | TrustState::Superseded
        ) {
            store.record(
                &key,
                TrustState::Trusted,
                Some("source rollback restored a previously reviewed revision"),
            )?;
        }
    }
    Ok(())
}

pub fn status(home: &AikitHome, id: &str) -> Result<SourceStatus> {
    let spec = load_spec(home, id)?;
    let state = load_state(home, id)?;
    let candidate = state
        .candidate_snapshot
        .as_deref()
        .map(|digest| load_snapshot(home, id, digest))
        .transpose()?;
    let active = state
        .active_snapshot
        .as_deref()
        .map(|digest| load_snapshot(home, id, digest))
        .transpose()?;
    Ok(SourceStatus {
        spec,
        state,
        candidate,
        active,
    })
}

pub fn active_registries(home: &AikitHome) -> Result<Vec<(String, PathBuf)>> {
    let root = home.root().join("sources");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io("source.read_failed", &root, error)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io("source.read_failed", &root, error))?;
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let state = load_state(home, &id)?;
        if let Some(active) = state.active_snapshot {
            out.push((
                id.clone(),
                snapshot_dir(home, &id, &active).join("registry"),
            ));
        }
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(out)
}

pub fn active_registry(home: &AikitHome, id: &str) -> Result<Option<PathBuf>> {
    let state = load_state(home, id)?;
    Ok(state
        .active_snapshot
        .map(|digest| snapshot_dir(home, id, &digest).join("registry")))
}

fn discover_skills(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
    {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "source.read_failed",
                format!("could not walk {}: {error}", root.display()),
            )
        })?;
        if entry.file_type().is_dir() && entry.path().join(agent_skills::SKILL_FILE).is_file() {
            out.push(entry.path().to_path_buf());
        }
    }
    Ok(out)
}

fn reject_symlinks(root: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "source.read_failed",
                format!("could not walk {}: {error}", root.display()),
            )
        })?;
        if entry.file_type().is_symlink() {
            return Err(AikitError::new(
                "source.symlink_not_supported",
                format!("snapshot input contains symlink {}", entry.path().display()),
            ));
        }
    }
    Ok(())
}

fn load_spec(home: &AikitHome, id: &str) -> Result<SourceSpec> {
    validate_id(id)?;
    read_toml(&source_dir(home, id).join(SPEC_FILE), "source.unknown")
}

fn load_state(home: &AikitHome, id: &str) -> Result<SourceState> {
    read_toml(
        &source_dir(home, id).join(STATE_FILE),
        "source.state_unreadable",
    )
}

fn load_snapshot(home: &AikitHome, id: &str, digest: &str) -> Result<SnapshotRecord> {
    read_toml(
        &snapshot_dir(home, id, digest).join(SNAPSHOT_FILE),
        "source.snapshot_unreadable",
    )
}

fn source_dir(home: &AikitHome, id: &str) -> PathBuf {
    home.root().join("sources").join(id)
}

fn snapshot_dir(home: &AikitHome, id: &str, digest: &str) -> PathBuf {
    source_dir(home, id).join("snapshots").join(digest)
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AikitError::new(
            "source.invalid_id",
            format!("`{id}` is not a safe skill source id"),
        ));
    }
    Ok(())
}

fn is_exact_commit(revision: &str) -> bool {
    matches!(revision.len(), 40 | 64) && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_contained_relative(path: &Path) -> bool {
    use std::path::Component;

    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::CurDir | Component::Normal(_)))
}

fn has_embedded_credentials(repository: &str) -> bool {
    let Some((scheme, rest)) = repository.split_once("://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    let Some((userinfo, _)) = authority.rsplit_once('@') else {
        return false;
    };
    !scheme.eq_ignore_ascii_case("ssh") || userinfo.contains(':')
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &Path, code: &'static str) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|error| io(code, path, error))?;
    toml::from_str(&text).map_err(|error| {
        AikitError::new(
            code,
            format!("{} is not valid source state: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })
}

fn write_toml_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = toml::to_string_pretty(value).map_err(|error| {
        AikitError::new(
            "source.write_failed",
            format!("could not serialize {}: {error}", path.display()),
        )
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| io("source.write_failed", parent, error))?;
    }
    let temporary = path.with_extension("toml.tmp");
    fs::write(&temporary, text).map_err(|error| io("source.write_failed", &temporary, error))?;
    fs::rename(&temporary, path).map_err(|error| io("source.write_failed", path, error))
}

fn run_git(args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .map_err(|error| {
            AikitError::new("source.git_failed", format!("could not run git: {error}"))
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(AikitError::new(
        "source.git_failed",
        format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    ))
}

fn git_output(args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .map_err(|error| {
            AikitError::new("source.git_failed", format!("could not run git: {error}"))
        })?;
    if !output.status.success() {
        return Err(AikitError::new(
            "source.git_failed",
            format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn path_text(path: &Path) -> String {
    path.components()
        .map(|part| part.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn hash_field(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

#[cfg(unix)]
fn copy_permissions(from: &Path, to: &Path) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(from)
        .map_err(|error| io("source.snapshot_failed", from, error))?
        .permissions()
        .mode()
        & 0o7777;
    fs::set_permissions(to, fs::Permissions::from_mode(mode))
        .map_err(|error| io("source.snapshot_failed", to, error))?;
    Ok(mode)
}

#[cfg(not(unix))]
fn copy_permissions(from: &Path, to: &Path) -> Result<u32> {
    let readonly = fs::metadata(from)
        .map_err(|error| io("source.snapshot_failed", from, error))?
        .permissions()
        .readonly();
    let mut permissions = fs::metadata(to)
        .map_err(|error| io("source.snapshot_failed", to, error))?
        .permissions();
    permissions.set_readonly(readonly);
    fs::set_permissions(to, permissions)
        .map_err(|error| io("source.snapshot_failed", to, error))?;
    Ok(u32::from(readonly))
}

fn io(code: &'static str, path: &Path, error: std::io::Error) -> AikitError {
    AikitError::new(code, format!("{}: {error}", path.display()))
        .with("path", path.display().to_string())
}
