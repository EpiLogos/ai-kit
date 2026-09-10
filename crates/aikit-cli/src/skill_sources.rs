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
    /// Central retains the authoritative location and standing. The root is
    /// a connection hint; CENTRAL_ROOT can rebind it after a World relocation.
    Central {
        central_root: PathBuf,
        source_ref: String,
    },
    Directory {
        path: PathBuf,
        /// The directory publishes the Control ground skill manifest contract
        /// (`central.skill/v1`): each member skill may carry a `skill.json`
        /// whose standing and provenance become capability metadata. Skills
        /// without one have unresolved standing and cannot project.
        #[serde(default)]
        control_ground: bool,
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
            Self::Central { .. } => "central",
            Self::Directory { .. } => "directory",
            Self::Git { .. } => "git",
        }
    }

    pub fn portable(&self) -> bool {
        matches!(self, Self::Git { .. } | Self::Central { .. })
    }

    /// Whether this source reads the Control ground skill manifest contract.
    pub fn control_ground(&self) -> bool {
        matches!(
            self,
            Self::Directory {
                control_ground: true,
                ..
            } | Self::Central { .. }
        )
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
    /// The Control ground standing this skill carried into the snapshot, when
    /// the source reads the contract and the skill published one.
    #[serde(default)]
    pub standing: Option<String>,
    #[serde(default)]
    pub retirement_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub schema: u32,
    pub source: String,
    pub digest: String,
    #[serde(default)]
    pub git_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_revision: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<MetadataSection<'a>>,
}

#[derive(Serialize)]
struct SkillSection {
    root: &'static str,
}

/// `[metadata]`, carrying the Control ground facts under the `control`
/// namespace exactly as the capsule manifest carries AIKit's own under `aikit`.
#[derive(Serialize)]
struct MetadataSection<'a> {
    control: ControlMetadataSection<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct ControlMetadataSection<'a> {
    standing: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retired_by: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retired_at_unix_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retirement_reason: Option<&'a str>,
}

pub fn add_directory(
    home: &AikitHome,
    id: &str,
    path: &Path,
    control_ground: bool,
) -> Result<SourceSpec> {
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
            kind: SourceKind::Directory {
                path,
                control_ground,
            },
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
        SourceKind::Directory { .. } | SourceKind::Central { .. } => {
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
    validate_owner_snapshot(&spec, &record)?;
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
        SourceKind::Directory { path, .. } => Ok((path.clone(), None)),
        SourceKind::Central { .. } => prepare_central_source(spec, staging),
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
    if roots.is_empty() && !spec.kind.control_ground() {
        return Err(AikitError::new(
            "source.no_skills",
            format!("skill source `{}` contains no valid Agent Skills", spec.id),
        ));
    }
    let mut hasher = blake3::Hasher::new();
    let owner_revision = if matches!(&spec.kind, SourceKind::Central { .. }) {
        let receipt: serde_json::Value = serde_json::from_slice(
            &fs::read(staging.join("central-receipt.json"))
                .map_err(|e| AikitError::new("source.central_receipt", e.to_string()))?,
        )
        .map_err(|e| AikitError::new("source.central_receipt", e.to_string()))?;
        let revision = receipt["tree_revision"]
            .as_str()
            .ok_or_else(|| AikitError::new("source.central_receipt", "missing owner revision"))?
            .to_owned();
        hash_field(&mut hasher, &revision);
        Some(revision)
    } else {
        None
    };
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
        // A Control-ground source reads the sibling contract beside each skill;
        // every other source never opens skill.json at all.
        let control = if spec.kind.control_ground() {
            let directory_name = root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            Some(
                crate::control_ground::read(&root, directory_name)?
                    .unwrap_or_else(crate::control_ground::ControlMetadata::unresolved),
            )
        } else {
            None
        };
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
            metadata: control.as_ref().map(|ground| MetadataSection {
                control: ControlMetadataSection {
                    standing: &ground.standing,
                    scope: ground.scope.as_deref(),
                    provenance: ground.provenance.as_deref(),
                    retired_by: ground.retired_by.as_deref(),
                    retired_at_unix_seconds: ground.retired_at_unix_seconds,
                    retirement_reason: ground.retirement_reason.as_deref(),
                },
            }),
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
        // The standing participates in the snapshot identity even though its
        // source bytes are already hashed: a standing change must always be a
        // new candidate, never a no-op that leaves the promoted snapshot
        // speaking for ground that has moved.
        if let Some(ground) = &control {
            hash_field(&mut hasher, &format!("control/{}", ground.standing));
            if let Some(reason) = &ground.retirement_reason {
                hash_field(&mut hasher, reason);
            }
        }
        skills.push(SnapshotSkill {
            id,
            name: skill.name,
            source_path: path_text(relative),
            standing: control.as_ref().map(|ground| ground.standing.clone()),
            retirement_reason: control
                .as_ref()
                .and_then(|ground| ground.retirement_reason.clone()),
        });
    }
    skills.sort_by(|left, right| left.id.cmp(&right.id));
    let digest = hasher.finalize().to_hex().to_string();
    let record = SnapshotRecord {
        schema: 1,
        source: spec.id.clone(),
        digest,
        git_commit,
        owner_revision,
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
    // Registering and promoting an existing local directory is the user's
    // acceptance of that source. Downloaded Git snapshots retain explicit trust.
    let local = matches!(
        load_spec(home, id)?.kind,
        SourceKind::Directory { .. } | SourceKind::Central { .. }
    );
    let trust_all = trust_all || (local && trust_skills.is_empty());
    let mut state = load_state(home, id)?;
    let digest = state.candidate_snapshot.clone().ok_or_else(|| {
        AikitError::new(
            "source.no_candidate",
            format!("skill source `{id}` has no candidate snapshot; sync it first"),
        )
    })?;
    let record = load_snapshot(home, id, &digest)?;
    validate_owner_snapshot(&load_spec(home, id)?, &record)?;
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
                Some(if local {
                    "user-promoted local directory"
                } else {
                    "explicit source promotion"
                }),
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
    validate_owner_snapshot(&load_spec(home, id)?, &record)?;
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
            validate_owner_snapshot(&load_spec(home, &id)?, &load_snapshot(home, &id, &active)?)?;
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
    if let Some(active) = &state.active_snapshot {
        validate_owner_snapshot(&load_spec(home, id)?, &load_snapshot(home, id, active)?)?;
    }
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

/// Explicit binding/rebinding of a source through Central, retaining snapshot
/// history. A prior active snapshot cannot project until its owner basis agrees.
pub fn bind_central(
    home: &AikitHome,
    id: &str,
    root: &Path,
    source_ref: &str,
) -> Result<SourceSpec> {
    validate_id(id)?;
    aikit_core::resource::SourceRef::parse(source_ref)?;
    let spec = SourceSpec {
        schema: 1,
        id: id.into(),
        kind: SourceKind::Central {
            central_root: root.into(),
            source_ref: source_ref.into(),
        },
    };
    let _ = central_bundle(&spec)?;
    let dir = source_dir(home, id);
    let mut state = if dir.join(SPEC_FILE).exists() {
        load_state(home, id)?
    } else {
        SourceState::default()
    };
    fs::create_dir_all(dir.join("snapshots")).map_err(|e| io("source.write_failed", &dir, e))?;
    write_toml_atomic(&dir.join(SPEC_FILE), &spec)?;
    state.candidate_snapshot = None;
    write_toml_atomic(&dir.join(STATE_FILE), &state)?;
    Ok(spec)
}
fn central_bundle(spec: &SourceSpec) -> Result<serde_json::Value> {
    let SourceKind::Central {
        central_root,
        source_ref,
    } = &spec.kind
    else {
        return Err(AikitError::new(
            "source.not_central",
            "source is not Central-backed",
        ));
    };
    let root = std::env::var_os("CENTRAL_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| central_root.clone());
    let value = aikit_adapters::central_file_map::call(
        &aikit_adapters::runner::SystemRunner::new(),
        &aikit_adapters::central_file_map::executable(),
        &root,
        "skill-source",
        &serde_json::json!({"scope":"root","federated":true,"source_ref":source_ref}),
    )?;
    if value["source_ref"] != source_ref.as_str() || !value["tree_revision"].is_string() {
        return Err(AikitError::new(
            "source.central_mismatch",
            "owner returned a different source or no revision",
        ));
    }
    Ok(value)
}
fn prepare_central_source(spec: &SourceSpec, staging: &Path) -> Result<(PathBuf, Option<String>)> {
    use base64::Engine;
    let bundle = central_bundle(spec)?;
    let root = staging.join("checkout");
    fs::create_dir_all(&root).map_err(|e| io("source.snapshot_failed", &root, e))?;
    let files = bundle["files"]
        .as_array()
        .ok_or_else(|| AikitError::new("source.central_invalid", "owner file list absent"))?;
    if files.len() > 4096 {
        return Err(AikitError::new(
            "source.central_invalid",
            "owner file bound exceeded",
        ));
    }
    let mut total = 0usize;
    for file in files {
        let relative = file["path"]
            .as_str()
            .ok_or_else(|| AikitError::new("source.central_invalid", "owner path absent"))?;
        let p = Path::new(relative);
        if p.as_os_str().is_empty()
            || !p
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        {
            return Err(AikitError::new(
                "source.central_invalid",
                "unsafe owner file path",
            ));
        }
        let data = file["content_base64"]
            .as_str()
            .ok_or_else(|| AikitError::new("source.central_invalid", "owner bytes absent"))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| AikitError::new("source.central_invalid", e.to_string()))?;
        total += bytes.len();
        if total > 32 * 1024 * 1024 {
            return Err(AikitError::new(
                "source.central_invalid",
                "owner payload bound exceeded",
            ));
        }
        let mut hash = 0xcbf29ce484222325u64;
        for b in &bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        if file["revision"] != format!("central.content-fnv1a64/v1:{}:{hash:016x}", bytes.len()) {
            return Err(AikitError::new(
                "source.central_invalid",
                "owner source bytes do not match revision",
            ));
        }
        let destination = root.join(p);
        fs::create_dir_all(destination.parent().unwrap())
            .map_err(|e| io("source.snapshot_failed", &destination, e))?;
        fs::write(&destination, &bytes)
            .map_err(|e| io("source.snapshot_failed", &destination, e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = file["mode"].as_u64().ok_or_else(|| {
                AikitError::new("source.central_invalid", "owner file mode absent")
            })? as u32
                & 0o777;
            fs::set_permissions(&destination, fs::Permissions::from_mode(mode))
                .map_err(|e| io("source.snapshot_failed", &destination, e))?;
        }
    }
    fs::write(staging.join("central-receipt.json"),serde_json::to_vec(&serde_json::json!({"source_ref":bundle["source_ref"],"world_ref":bundle["world_ref"],"tree_revision":bundle["tree_revision"],"skills":bundle["skills"]})).map_err(|e|AikitError::new("source.central_invalid",e.to_string()))?).map_err(|e|io("source.snapshot_failed",staging,e))?;
    Ok((root, None))
}
fn validate_owner_snapshot(spec: &SourceSpec, snapshot: &SnapshotRecord) -> Result<()> {
    if matches!(&spec.kind, SourceKind::Central { .. }) {
        let bundle = central_bundle(spec)?;
        if bundle["tree_revision"].as_str() != snapshot.owner_revision.as_deref() {
            return Err(AikitError::new("source.central_revision_changed","Central source or standing changed; sync and promote a new owner-backed snapshot before projection"));
        }
    }
    Ok(())
}

/// Revalidate every participating Central-backed source before a generation
/// publication. Loading an old registry earlier in the process is not a lease.
pub fn validate_central_generations(home: &AikitHome) -> Result<()> {
    let _ = active_registries(home)?;
    Ok(())
}
/// Record the actual generated target, retaining the distinction from a loaded
/// harness. Errors are returned as reconciliation warnings after local commit.
pub fn report_central_generation(home: &AikitHome, generation: &str, target: &Path) -> Vec<String> {
    let root = home.root().join("sources");
    let mut warnings = Vec::new();
    let entries = match fs::read_dir(&root) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return warnings,
        Err(e) => return vec![e.to_string()],
    };
    for entry in entries {
        let result = (|| -> Result<()> {
            let entry = entry.map_err(|e| io("source.read_failed", &root, e))?;
            if !entry.path().is_dir() {
                return Ok(());
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let spec = load_spec(home, &id)?;
            let SourceKind::Central {
                central_root,
                source_ref,
            } = &spec.kind
            else {
                return Ok(());
            };
            let state = load_state(home, &id)?;
            let Some(digest) = state.active_snapshot else {
                return Ok(());
            };
            let snapshot = load_snapshot(home, &id, &digest)?;
            validate_owner_snapshot(&spec, &snapshot)?;
            let root = std::env::var_os("CENTRAL_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| central_root.clone());
            let runner = aikit_adapters::runner::SystemRunner::new();
            let executable = aikit_adapters::central_file_map::executable();
            let reading = aikit_adapters::central_file_map::call(
                &runner,
                &executable,
                &root,
                "inspect",
                &serde_json::json!({"federated":true}),
            )?;
            let map = reading["maps"]
                .as_array()
                .and_then(|m| {
                    m.iter().find(|m| {
                        m["sources"].as_array().is_some_and(|sources| {
                            sources
                                .iter()
                                .any(|s| s["source_ref"] == source_ref.as_str())
                        })
                    })
                })
                .ok_or_else(|| {
                    AikitError::new(
                        "source.central_source_missing",
                        "generation source withheld by owner",
                    )
                })?;
            let source = map["sources"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["source_ref"] == source_ref.as_str())
                .unwrap();
            let mut input = serde_json::json!({"source_ref":source_ref,"scope":"root","owner":"aikit","target":target,"generation":generation,"expected_revision":map["relations_revision"],"expected_source_revision":source["revision"]});
            if let Some(project) = map["project"].as_str() {
                input["scope"] = serde_json::json!("project");
                input["project"] = serde_json::json!(project);
            }
            aikit_adapters::central_file_map::call(
                &runner,
                &executable,
                &root,
                "projection",
                &input,
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            warnings.push(format!(
                "Generation committed; Central projection reconciliation pending: {}",
                error.message()
            ));
        }
    }
    warnings
}
