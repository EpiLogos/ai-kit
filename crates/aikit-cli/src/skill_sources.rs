//! Managed Agent Skill sources.
//!
//! Mutable directories and Git checkouts are never projected directly. A sync
//! copies their complete skill trees into an immutable, content-addressed
//! candidate snapshot; promotion is the separate act that makes one snapshot
//! visible to the catalogue.

use std::collections::{BTreeMap, BTreeSet};
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
/// The verified archive a capsule source was registered from, kept inside the
/// source so a sync never depends on the file the visitor was handed.
const CAPSULE_ARCHIVE_FILE: &str = "capsule.json";

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
        /// whose standing and provenance become capability metadata. A skill
        /// without one stands active by being in the spool.
        #[serde(default)]
        control_ground: bool,
    },
    Git {
        repository: String,
        revision: String,
        #[serde(default)]
        root: PathBuf,
    },
    /// One practice capsule adopted from another World's exported archive
    /// (`aikit.practice-capsule/v1`). The capsule keeps its original id and
    /// revision: its files are carried verbatim, so the synced revision is the
    /// exported one. The upstream record is the provenance slot.
    Capsule {
        capsule_id: String,
        revision: String,
        upstream: CapsuleUpstream,
    },
}

/// Where an adopted capsule came from. `practice_id`, `revision` and
/// `exported_from` are read from the verified archive; `world_ref` and
/// `entry_ref` are the visitor's own statement of the publication it read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapsuleUpstream {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_ref: Option<String>,
    pub practice_id: String,
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_from: Option<crate::practice_capsule::ExportedFrom>,
    /// sha256 of the archive bytes as registered.
    pub archive_sha256: String,
}

impl CapsuleUpstream {
    /// The upstream record as `source show` and `add-capsule` print it.
    /// `exported_from` is the archive's own claim about where it was
    /// exported: nothing here can check it against the exporting World, so
    /// the reading carries `verified: false` beside it. The practice id and
    /// revision, by contrast, are proven from the archive's bytes.
    pub fn reading(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(claim) = value
            .get_mut("exported_from")
            .and_then(serde_json::Value::as_object_mut)
        {
            claim.insert("verified".into(), serde_json::Value::Bool(false));
            claim.insert(
                "basis".into(),
                serde_json::Value::String(
                    "the archive's own claim; not verified against the exporting World".into(),
                ),
            );
        }
        value
    }
}

impl SourceKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Central { .. } => "central",
            Self::Directory { .. } => "directory",
            Self::Git { .. } => "git",
            Self::Capsule { .. } => "capsule",
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
    /// Proposed revisions of single members (O-I manual §19.7, Refining a
    /// skill), each a snapshot of its own beside the active one. Absent from
    /// the file when none waits, so a state without proposals reads as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<ProposedRevision>,
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

/// A candidate skill the snapshot refused, with the machine code and message of
/// the refusal. Only Git and plain Directory sources tolerate rejections — one
/// broken tree must not take every unrelated skill down with it. Control-ground
/// and Central sources refuse the whole sync instead and never write a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedSkill {
    /// The refused skill's directory, relative to the scan root (`.` for the
    /// scan root itself).
    pub path: String,
    pub code: String,
    pub message: String,
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
    /// Candidate skills refused by validation. Absent from the record — on
    /// disk and in the digest — when empty, so snapshots without rejections
    /// keep the identity they have always had.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected: Vec<RejectedSkill>,
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

/// The visitor's statement of where an archive came from. `practice_id` and
/// `revision`, when stated, must match what the archive proves.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CapsuleProvenance {
    #[serde(default)]
    pub world_ref: Option<String>,
    #[serde(default)]
    pub entry_ref: Option<String>,
    #[serde(default)]
    pub practice_id: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
}

/// What `add_capsule` registered, and whether it was already there.
#[derive(Debug, Clone)]
pub struct AddedCapsule {
    pub spec: SourceSpec,
    pub already_registered: bool,
}

/// Register a machine-local source holding one practice capsule from a
/// verified archive. Nothing becomes active: the capsule reaches the catalogue
/// only through the ordinary sync → promote path. Re-adding the same archive
/// with the same provenance is a no-op; a different revision takes its own
/// source id and stands beside the first.
pub fn add_capsule(
    home: &AikitHome,
    archive: &Path,
    id: Option<&str>,
    provenance: &CapsuleProvenance,
) -> Result<AddedCapsule> {
    use sha2::Digest;
    let (bytes, verified) = crate::practice_capsule::read_archive(archive)?;
    for (name, claimed, held) in [
        (
            "practice_id",
            provenance.practice_id.as_deref(),
            verified.id.to_string(),
        ),
        (
            "revision",
            provenance.revision.as_deref(),
            verified.revision.clone(),
        ),
    ] {
        if let Some(claimed) = claimed {
            if claimed != held {
                return Err(AikitError::new(
                    "source.provenance_mismatch",
                    format!(
                        "the provenance names {name} `{claimed}` but the archive proves `{held}`"
                    ),
                ));
            }
        }
    }
    let world_ref = provenance.world_ref.as_deref();
    let entry_ref = provenance.entry_ref.as_deref();
    let id = match id {
        Some(id) => id.to_string(),
        None => default_capsule_source_id(verified.id.path(), &verified.revision),
    };
    validate_id(&id)?;
    let upstream = CapsuleUpstream {
        world_ref: world_ref.map(str::to_string),
        entry_ref: entry_ref.map(str::to_string),
        practice_id: verified.id.to_string(),
        revision: verified.revision.clone(),
        exported_from: verified.exported_from.clone(),
        archive_sha256: format!("sha256:{:x}", sha2::Sha256::digest(&bytes)),
    };
    let spec = SourceSpec {
        schema: 1,
        id: id.clone(),
        kind: SourceKind::Capsule {
            capsule_id: verified.id.to_string(),
            revision: verified.revision.clone(),
            upstream,
        },
    };
    let dir = source_dir(home, &id);
    if dir.join(SPEC_FILE).exists() {
        let existing = load_spec(home, &id)?;
        let same = match (&existing.kind, &spec.kind) {
            (
                SourceKind::Capsule {
                    capsule_id,
                    revision,
                    upstream,
                },
                SourceKind::Capsule {
                    capsule_id: new_id,
                    revision: new_revision,
                    upstream: new_upstream,
                },
            ) => capsule_id == new_id && revision == new_revision && upstream == new_upstream,
            _ => false,
        };
        if same {
            return Ok(AddedCapsule {
                spec: existing,
                already_registered: true,
            });
        }
        return Err(AikitError::new(
            "source.exists",
            format!(
                "skill source `{id}` already exists and holds something else; \
                 an adopted original is never overwritten — pass another --id"
            ),
        )
        .with("source", id));
    }
    let spec = write_new_spec(home, spec)?;
    write_bytes_atomic(&dir.join(CAPSULE_ARCHIVE_FILE), &bytes)?;
    Ok(AddedCapsule {
        spec,
        already_registered: false,
    })
}

/// `<path-slug>-<revision12>`: one source per adopted revision.
fn default_capsule_source_id(path: &str, revision: &str) -> String {
    let slug: String = path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let short: String = revision.chars().take(12).collect();
    format!("capsule-{slug}-{short}")
}

fn build_capsule_snapshot(
    home: &AikitHome,
    spec: &SourceSpec,
    staging: &Path,
) -> Result<SnapshotRecord> {
    let SourceKind::Capsule {
        capsule_id,
        revision,
        ..
    } = &spec.kind
    else {
        unreachable!("only capsule sources build capsule snapshots");
    };
    let archive = source_dir(home, &spec.id).join(CAPSULE_ARCHIVE_FILE);
    let (_, verified) = crate::practice_capsule::read_archive(&archive)?;
    if verified.id.to_string() != *capsule_id || verified.revision != *revision {
        return Err(AikitError::new(
            "source.capsule_changed",
            format!(
                "the archive held by source `{}` no longer proves `{capsule_id}` at {revision}",
                spec.id
            ),
        ));
    }
    let capsule_dir = staging.join("registry/capsules").join(capsule_id);
    fs::create_dir_all(&capsule_dir)
        .map_err(|error| io("source.snapshot_failed", &capsule_dir, error))?;
    crate::practice_capsule::write_files(&capsule_dir, &verified.files)?;
    // Prove the materialised capsule with the registry's own reader.
    let manifest = fs::read(capsule_dir.join(aikit_store::registry::MANIFEST_FILE))
        .map_err(|error| io("source.snapshot_failed", &capsule_dir, error))?;
    let materialised = aikit_store::registry::compute_revision(&capsule_dir, &manifest)?;
    if materialised.as_str() != revision {
        return Err(AikitError::new(
            "source.capsule_revision_mismatch",
            format!("`{capsule_id}` materialised at {materialised}, not the adopted {revision}"),
        ));
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit-capsule-source-snapshot-v1\n");
    hash_field(&mut hasher, capsule_id);
    hash_field(&mut hasher, revision);
    let record = SnapshotRecord {
        schema: 1,
        source: spec.id.clone(),
        digest: hasher.finalize().to_hex().to_string(),
        git_commit: None,
        owner_revision: None,
        skills: vec![SnapshotSkill {
            id: capsule_id.clone(),
            name: verified.name,
            source_path: ".".to_string(),
            standing: None,
            retirement_reason: None,
        }],
        rejected: Vec::new(),
    };
    write_toml_atomic(&staging.join(SNAPSHOT_FILE), &record)?;
    Ok(record)
}

/// Refuse a promotion that would make two active sources speak for one
/// practice id when an adopted capsule is on either side.
///
/// The rule: promoting source `id` is refused when any id in its candidate
/// snapshot is already catalogued by another source's active snapshot and
/// either the promoted source or that other source is a capsule source. Two
/// non-capsule sources (directory, git, central) that provide one id are left
/// as they were before capsules existed: their precedence is the catalogue's
/// existing layering. A capsule, though, is a proven revision adopted from
/// another World; a silent shadow in either direction would hide which
/// revision is operative.
fn refuse_active_identity_collision(
    home: &AikitHome,
    id: &str,
    promoting_capsule: bool,
    candidate: &SnapshotRecord,
) -> Result<()> {
    let root = home.root().join("sources");
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(());
    };
    let mut others: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|other| other != id)
        .collect();
    others.sort();
    for other in others {
        let Ok(state) = load_state(home, &other) else {
            continue;
        };
        let Some(active) = state.active_snapshot else {
            continue;
        };
        let other_capsule = matches!(
            load_spec(home, &other).map(|spec| spec.kind),
            Ok(SourceKind::Capsule { .. })
        );
        if !promoting_capsule && !other_capsule {
            continue;
        }
        let Ok(record) = load_snapshot(home, &other, &active) else {
            continue;
        };
        if let Some(shared) = candidate
            .skills
            .iter()
            .find(|skill| record.skills.iter().any(|theirs| theirs.id == skill.id))
        {
            let capability = shared.id.clone();
            return Err(AikitError::new(
                "source.capsule_identity_active",
                format!(
                    "`{capability}` is already active through source `{other}`{}; roll back or \
                     remove that source before promoting `{id}`, so one revision speaks for the id",
                    if other_capsule {
                        " (an adopted capsule)"
                    } else {
                        ""
                    }
                ),
            )
            .with("capability", capability)
            .with("active_source", other));
        }
    }
    Ok(())
}

/// The active snapshot digest of a managed source, when `id` names one.
pub fn active_snapshot_of(home: &AikitHome, id: &str) -> Option<String> {
    if validate_id(id).is_err() {
        return None;
    }
    load_state(home, id).ok()?.active_snapshot
}

/// Every retained snapshot of a managed source that holds `capsule_id`, as
/// `(digest, capsule directory)`: active first, then the candidate, any
/// proposed revision waiting beside them, and the rollback history, newest
/// first. Empty when `id` is not a managed source.
pub fn retained_capsule_dirs(
    home: &AikitHome,
    id: &str,
    capsule_id: &str,
) -> Vec<(String, PathBuf)> {
    if validate_id(id).is_err() {
        return Vec::new();
    }
    let Ok(state) = load_state(home, id) else {
        return Vec::new();
    };
    let mut digests: Vec<String> = Vec::new();
    for digest in state
        .active_snapshot
        .into_iter()
        .chain(state.candidate_snapshot)
        .chain(
            state
                .proposals
                .into_iter()
                .map(|proposal| proposal.snapshot),
        )
        .chain(state.history.into_iter().rev())
    {
        if !digests.contains(&digest) {
            digests.push(digest);
        }
    }
    digests
        .into_iter()
        .filter_map(|digest| {
            let dir = snapshot_dir(home, id, &digest)
                .join("registry/capsules")
                .join(capsule_id);
            dir.join(aikit_store::registry::MANIFEST_FILE)
                .is_file()
                .then_some((digest, dir))
        })
        .collect()
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
        SourceKind::Directory { .. } | SourceKind::Central { .. } | SourceKind::Capsule { .. } => {
            return Err(AikitError::new(
                "source.not_git",
                format!("skill source `{id}` is a {} source", spec.kind.label()),
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

    let built = if matches!(spec.kind, SourceKind::Capsule { .. }) {
        build_capsule_snapshot(home, &spec, &staging)
    } else {
        let prepared = prepare_source(&spec, &staging);
        let (scan_root, git_commit) = match prepared {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(error);
            }
        };
        build_snapshot(&spec, &scan_root, git_commit, &staging)
    };
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
        SourceKind::Capsule { .. } => unreachable!("capsule sources build from their archive"),
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
    let mut rejected = Vec::new();
    let mut ids = BTreeSet::new();

    for root in roots {
        reject_symlinks(&root)?;
        let relative = root.strip_prefix(scan_root).unwrap_or(Path::new(""));
        let relative_text = path_text(relative);
        let relative_text = if relative_text.is_empty() {
            ".".to_string()
        } else {
            relative_text
        };
        // Every source kind records an invalid skill as a rejection and keeps
        // the valid remainder — sources hold skills, not promises of
        // perfection: one broken tree beside sixty-four good ones must not
        // silence the whole source. A source whose candidates ALL fail
        // validation still refuses below, naming the rejections; a cleanly
        // empty Control-ground source stays a real, successful state.
        let control_ground_source = spec.kind.control_ground();
        let skill = match agent_skills::validate(&root) {
            Ok(skill) => skill,
            Err(error) => {
                record_rejection(&relative_text, scan_root, &mut rejected, &mut hasher, error)?;
                continue;
            }
        };
        // A Control-ground source reads the sibling contract beside each skill;
        // every other source never opens skill.json at all.
        let control = if control_ground_source {
            let directory_name = root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            Some(
                crate::control_ground::read(&root, directory_name)?
                    .unwrap_or_else(crate::control_ground::ControlMetadata::spooled),
            )
        } else {
            None
        };
        let capsule_tail = if relative.as_os_str().is_empty() {
            skill.name.clone()
        } else {
            path_text(relative)
        };
        let id = format!("skill/{}/{capsule_tail}", spec.id);
        if let Err(error) = aikit_core::CapsuleId::parse(&id) {
            record_rejection(&relative_text, scan_root, &mut rejected, &mut hasher, error)?;
            continue;
        }
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
    // A clean empty ground is a real state for a Control-ground source (an
    // authored collection can be emptied on purpose) — but candidates that
    // all failed validation are a defect in any source kind: refuse, naming
    // every rejection.
    if skills.is_empty() && (!rejected.is_empty() || !spec.kind.control_ground()) {
        return Err(no_valid_skills(spec, &rejected));
    }
    let digest = hasher.finalize().to_hex().to_string();
    let record = SnapshotRecord {
        schema: 1,
        source: spec.id.clone(),
        digest,
        git_commit,
        owner_revision,
        skills,
        rejected,
    };
    write_toml_atomic(&staging.join(SNAPSHOT_FILE), &record)?;
    Ok(record)
}

/// The refusal for a tolerated source whose candidates all failed: every
/// rejection is named, and the way out is scoping the source to the tree that
/// actually holds the skills.
fn no_valid_skills(spec: &SourceSpec, rejected: &[RejectedSkill]) -> AikitError {
    let message = if rejected.is_empty() {
        format!(
            "skill source `{}` contains no Agent Skills; a snapshot needs directories holding \
             a {} — point the source at the tree that contains them \
             (`--root <dir>` for a Git source)",
            spec.id,
            agent_skills::SKILL_FILE,
        )
    } else {
        let mut rendered = format!(
            "skill source `{}` contains no valid Agent Skills; all {} candidates were rejected:",
            spec.id,
            rejected.len(),
        );
        for rejection in rejected {
            rendered.push_str(&format!(
                "\n  - {}: [{}] {}",
                rejection.path, rejection.code, rejection.message
            ));
        }
        rendered.push_str(
            "\nFix the named skills, or scope the source to the valid tree \
             (`--root <dir>` for a Git source).",
        );
        rendered
    };
    AikitError::new("source.no_skills", message)
}

/// Record the refusal as part of the snapshot — the rejected path, code and
/// message participate in the digest so a rejection that appears or
/// disappears is a new candidate, never a stale record under an old name.
/// Every source kind rejects per skill; the sync reply and `source show`
/// carry the rejections, and a source with no valid skills left refuses.
fn record_rejection(
    path: &str,
    scan_root: &Path,
    rejected: &mut Vec<RejectedSkill>,
    hasher: &mut blake3::Hasher,
    error: AikitError,
) -> Result<()> {
    // Validator diagnostics can embed a checkout path. Git sync stages at a
    // fresh ULID every time: keep diagnostics and snapshot identity source-relative.
    let message = error
        .message()
        .replace(scan_root.to_string_lossy().as_ref(), ".");
    hash_field(hasher, "rejected");
    hash_field(hasher, path);
    hash_field(hasher, error.code());
    hash_field(hasher, &message);
    rejected.push(RejectedSkill {
        path: path.to_string(),
        code: error.code().to_string(),
        message,
    });
    Ok(())
}

pub fn promote(
    home: &AikitHome,
    id: &str,
    trust_all: bool,
    trust_skills: &[String],
) -> Result<(SnapshotRecord, usize)> {
    // Registering and promoting an existing local directory is the user's
    // acceptance of that source. Downloaded Git snapshots retain explicit trust.
    // An adopted capsule follows the directory path: the visitor registered
    // the verified archive and promotes it deliberately — publication alone
    // never reached the catalogue.
    let spec = load_spec(home, id)?;
    let local = matches!(
        spec.kind,
        SourceKind::Directory { .. } | SourceKind::Central { .. } | SourceKind::Capsule { .. }
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
    refuse_active_identity_collision(
        home,
        id,
        matches!(spec.kind, SourceKind::Capsule { .. }),
        &record,
    )?;
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
                Some(if matches!(spec.kind, SourceKind::Capsule { .. }) {
                    "user-promoted adopted capsule"
                } else if local {
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

// ---------------------------------------------------------------------------
// Refining a skill (O-I manual §19.7): one proposed revision of one member
// ---------------------------------------------------------------------------
//
// A skill that the work keeps pulling against gets one drafted revision, and
// the revision waits beside the one in use until the person answers it. It is
// kept where the member's revisions are already kept: its source's snapshots.
// A proposal is a snapshot of its own — the active snapshot with that one
// member's files replaced by the draft — named in the source's state beside
// the candidate, the active snapshot and the rollback history, so `praxis read
// --revision` reads it as any retained revision. Nothing in use changes until
// Accept, which makes the proposal's snapshot active with the one before kept
// to Roll back to (History takes it back); Discard drops it. A Repeat over
// Skillset carries it into one take's World only (§19.1), and the take is kept
// with the proposal as its Test.

/// A close that shows the reason a revision is proposed, in the words of the
/// reading that found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefinementReason {
    pub task_ref: String,
    pub reason: String,
}

/// A take that ran under a proposed revision (its trace), the takes of its
/// Repeat that ran beside it, the take's own NOW where Central kept one, and
/// what each of those takes answered to the task's Success conditions, by
/// its trace (O-I manual §19.7, Result).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefinementTest {
    pub trace: String,
    #[serde(default)]
    pub beside: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ref: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub answers: BTreeMap<String, Vec<RefinementAnswer>>,
}

/// One take's answer to one of the task's Success conditions, as Actuation
/// read it (research execution.rs): met, not met or open, with its evidence
/// in words, and why it reads as it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefinementAnswer {
    pub condition: String,
    pub answer: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// One proposed revision of one member, as its source keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedRevision {
    pub member: String,
    /// The snapshot that holds the revision.
    pub snapshot: String,
    /// The active snapshot the revision was drafted on.
    pub based_on: String,
    #[serde(default)]
    pub why: Vec<RefinementReason>,
    pub proposed_at_unix_seconds: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<RefinementTest>,
    /// Where the revision waits once Accept handed it to Central, for a
    /// member whose source Central keeps; absent before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handed: Option<HandedToCentral>,
}

/// A proposed revision of a skill whose source Central keeps, as Central
/// keeps it beside the skill in use (Central's `control.skills.revision.propose`):
/// its ref there, the digest the person reviews, the skill by its scope and
/// name, and the act that writes it — the person's Accept in Central.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandedToCentral {
    pub revision_ref: String,
    pub proposed_digest: String,
    pub scope: String,
    pub name: String,
    pub accept_action: String,
}

/// A member's revision in one snapshot: the snapshot and the capsule revision
/// AIKit's catalogue gives the member there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevisionAt {
    pub snapshot: String,
    pub revision: String,
}

/// A proposed revision as it is read: beside the revision in use, with why it
/// was proposed and the takes that ran under it.
#[derive(Debug, Clone, Serialize)]
pub struct Refinement {
    pub member: String,
    pub source: String,
    /// Whose operation keeps the member's next revision: `aikit` for a source
    /// AIKit owns, `central` for a skill whose source Central keeps.
    pub owner: &'static str,
    pub why: Vec<RefinementReason>,
    pub proposed: RevisionAt,
    pub in_use: RevisionAt,
    /// The revision in use is no longer the one the proposal was drafted on.
    pub stale: bool,
    pub proposed_at_unix_seconds: u64,
    pub tests: Vec<RefinementTest>,
    /// Where the revision waits in Central, once Accept handed it there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handed: Option<HandedToCentral>,
}

/// What Accept did: the member's next revision, and the snapshot Roll back
/// returns to.
#[derive(Debug, Clone, Serialize)]
pub struct AcceptedRevision {
    pub member: String,
    pub source: String,
    pub revision: String,
    pub active_snapshot: String,
    pub rollback_to: String,
}

/// What Accept did with a proposal whose source Central keeps: handed it to
/// Central, which keeps it beside the skill in use and writes it only on the
/// person's Accept there. AIKit's revision in use does not move.
#[derive(Debug, Clone, Serialize)]
pub struct HandedRevision {
    pub member: String,
    pub source: String,
    pub handed: HandedToCentral,
}

/// What Accept did: kept the proposal as the member's next revision here, or
/// handed it to Central, which keeps the member's source.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Accepted {
    Kept(AcceptedRevision),
    Handed(HandedRevision),
}

/// Central's act that keeps a proposed revision of one of its skills, the
/// reading it answers, and the person's act there that writes it.
const CENTRAL_REVISION_PROPOSE: &str = "control.skills.revision.propose";
const CENTRAL_REVISION_ACCEPT: &str = "control.skills.revision.accept";
const CENTRAL_REVISION_READING: &str = "central.skill-revision-reading/v1";
/// The Control scope AIKit hands a revision in: the person's own skills,
/// which Central addresses by name alone. A machine's or a project's skill
/// needs the machine or project, which a snapshot does not carry.
const CENTRAL_HANDED_SCOPE: &str = "control-user";
/// The body goes to `ctrl` as an argument: bounded well inside the
/// platform's argument limit.
const MAX_HANDED_BODY: usize = 256 * 1024;

/// What Discard dropped.
#[derive(Debug, Clone, Serialize)]
pub struct DiscardedRevision {
    pub discarded: String,
    pub source: String,
    pub snapshot: String,
    /// The revision in use, which stays.
    pub in_use: Option<String>,
}

fn refinement_error(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}

/// The source a member's id names (`skill/<source>/<tail>`) and its state.
fn member_source(home: &AikitHome, member: &str) -> Result<(String, SourceSpec, SourceState)> {
    aikit_core::CapsuleId::parse(member)?;
    let mut parts = member.split('/');
    let (Some("skill"), Some(source), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(refinement_error(
            "refinement.not_a_member",
            format!("`{member}` names no skill of a source AIKit keeps (a member is `skill/<source>/<name>`)"),
        ));
    };
    if validate_id(source).is_err() || !source_dir(home, source).join(SPEC_FILE).is_file() {
        return Err(refinement_error(
            "refinement.not_a_member",
            format!("AIKit keeps no skill source `{source}`, so `{member}` has no revisions here"),
        ));
    }
    Ok((
        source.to_owned(),
        load_spec(home, source)?,
        load_state(home, source)?,
    ))
}

/// The capsule revision AIKit's catalogue gives `member` in one snapshot.
fn member_revision(home: &AikitHome, source: &str, snapshot: &str, member: &str) -> Result<String> {
    let registry = snapshot_dir(home, source, snapshot).join("registry");
    let loaded = load_registry(&registry, RegistrySource::new(source.to_string()))?;
    loaded
        .catalog
        .capsules()
        .into_iter()
        .find(|capsule| capsule.id.to_string() == member)
        .and_then(|capsule| {
            capsule
                .revision
                .as_ref()
                .map(|revision| revision.to_string())
        })
        .ok_or_else(|| {
            refinement_error(
                "refinement.not_a_member",
                format!("snapshot `{snapshot}` of `{source}` holds no `{member}`"),
            )
        })
}

/// The files of one skill directory, sorted, with their modes: what a
/// revision's identity and its read-back are computed over.
pub(crate) fn tree_files(root: &Path) -> Result<Vec<(String, u32, Vec<u8>)>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u32, Vec<u8>)>) -> Result<()> {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|error| io("refinement.read_failed", dir, error))?
            .collect::<std::result::Result<_, _>>()
            .map_err(|error| io("refinement.read_failed", dir, error))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| io("refinement.read_failed", &path, error))?;
            if kind.is_symlink() {
                return Err(refinement_error(
                    "refinement.symlink",
                    format!(
                        "`{}` is a link; a revision carries files only",
                        path.display()
                    ),
                ));
            }
            if kind.is_dir() {
                walk(root, &path, out)?;
                continue;
            }
            let bytes =
                fs::read(&path).map_err(|error| io("refinement.read_failed", &path, error))?;
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                entry
                    .metadata()
                    .map_err(|error| io("refinement.read_failed", &path, error))?
                    .permissions()
                    .mode()
                    & 0o7777
            };
            #[cfg(not(unix))]
            let mode = 0;
            out.push((
                path_text(path.strip_prefix(root).unwrap_or(&path)),
                mode,
                bytes,
            ));
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

/// Copy a snapshot tree (directories and files, modes kept).
pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).map_err(|error| io("refinement.write_failed", to, error))?;
    for entry in fs::read_dir(from).map_err(|error| io("refinement.read_failed", from, error))? {
        let entry = entry.map_err(|error| io("refinement.read_failed", from, error))?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry
            .file_type()
            .map_err(|error| io("refinement.read_failed", &source, error))?;
        if kind.is_dir() {
            copy_tree(&source, &target)?;
        } else if kind.is_file() {
            fs::copy(&source, &target)
                .map_err(|error| io("refinement.write_failed", &target, error))?;
            copy_permissions(&source, &target)?;
        }
    }
    Ok(())
}

fn refinement_of(
    home: &AikitHome,
    source: &str,
    spec: &SourceSpec,
    state: &SourceState,
    proposal: &ProposedRevision,
) -> Result<Refinement> {
    let active = state.active_snapshot.clone().unwrap_or_default();
    let in_use = if active.is_empty() {
        RevisionAt {
            snapshot: active.clone(),
            revision: String::new(),
        }
    } else {
        RevisionAt {
            revision: member_revision(home, source, &active, &proposal.member).unwrap_or_default(),
            snapshot: active.clone(),
        }
    };
    Ok(Refinement {
        member: proposal.member.clone(),
        source: source.to_owned(),
        owner: if matches!(spec.kind, SourceKind::Central { .. }) {
            "central"
        } else {
            "aikit"
        },
        why: proposal.why.clone(),
        proposed: RevisionAt {
            revision: member_revision(home, source, &proposal.snapshot, &proposal.member)?,
            snapshot: proposal.snapshot.clone(),
        },
        stale: active != proposal.based_on,
        in_use,
        proposed_at_unix_seconds: proposal.proposed_at_unix_seconds,
        tests: proposal.tests.clone(),
        handed: proposal.handed.clone(),
    })
}

/// Propose one revision of one member: `revised` is a directory holding the
/// revised skill, under the member's own name. One proposal per member waits
/// at a time; it changes something, and it is drafted on the revision in use.
pub fn propose_revision(
    home: &AikitHome,
    member: &str,
    revised: &Path,
    why: Vec<RefinementReason>,
) -> Result<Refinement> {
    let (source, spec, mut state) = member_source(home, member)?;
    let active = state.active_snapshot.clone().ok_or_else(|| {
        refinement_error(
            "refinement.no_active",
            format!("skill source `{source}` has no active snapshot, so `{member}` has no revision in use to revise"),
        )
    })?;
    let record = load_snapshot(home, &source, &active)?;
    let Some(in_use) = record.skills.iter().find(|skill| skill.id == member) else {
        return Err(refinement_error(
            "refinement.not_a_member",
            format!(
                "`{member}` is not a skill of source `{source}` as it stands (snapshot `{active}`)"
            ),
        ));
    };
    if state
        .proposals
        .iter()
        .any(|proposal| proposal.member == member)
    {
        return Err(refinement_error(
            "refinement.already_proposed",
            format!("a revision of `{member}` is already proposed: one revision of one member waits at a time; accept or discard it first"),
        ));
    }
    if why.is_empty()
        || why
            .iter()
            .any(|reason| reason.task_ref.trim().is_empty() || reason.reason.trim().is_empty())
    {
        return Err(refinement_error(
            "refinement.why",
            "a proposed revision names the closes that show its reason: each a task and its reason (§19.7: nothing is refined because it is old or little used)",
        ));
    }
    let draft = agent_skills::validate(revised)?;
    if draft.name != in_use.name {
        return Err(refinement_error(
            "refinement.renamed",
            format!(
                "the draft is the skill `{}`, not `{}`: a revision keeps the member's name",
                draft.name, in_use.name
            ),
        ));
    }
    reject_symlinks(revised)?;
    let member_dir = |snapshot: &Path| snapshot.join("registry/capsules").join(member);
    let active_dir = snapshot_dir(home, &source, &active);
    let mut revised_files = Vec::new();
    for relative in &draft.files {
        let from = revised.join(relative);
        let bytes = fs::read(&from).map_err(|error| io("refinement.read_failed", &from, error))?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(&from)
                .map_err(|error| io("refinement.read_failed", &from, error))?
                .permissions()
                .mode()
                & 0o7777
        };
        #[cfg(not(unix))]
        let mode = 0;
        revised_files.push((relative.clone(), mode, bytes));
    }
    revised_files.sort_by(|left, right| left.0.cmp(&right.0));
    if tree_files(&member_dir(&active_dir).join("payload"))? == revised_files {
        return Err(refinement_error(
            "refinement.unchanged",
            format!("the draft of `{member}` is the revision in use: a proposal changes something"),
        ));
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit-skill-proposed-revision-v1\n");
    hash_field(&mut hasher, &active);
    hash_field(&mut hasher, member);
    for (relative, mode, bytes) in &revised_files {
        hash_field(&mut hasher, relative);
        hasher.update(&mode.to_le_bytes());
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    let digest = hasher.finalize().to_hex().to_string();
    let dir = source_dir(home, &source);
    let staging = dir.join(format!(".staging-{}", ulid::Ulid::generate()));
    let built = (|| -> Result<()> {
        copy_tree(&active_dir, &staging)?;
        let capsule = member_dir(&staging);
        let payload = capsule.join("payload");
        fs::remove_dir_all(&payload)
            .map_err(|error| io("refinement.write_failed", &payload, error))?;
        for (relative, _, bytes) in &revised_files {
            let to = payload.join(relative);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| io("refinement.write_failed", parent, error))?;
            }
            fs::write(&to, bytes).map_err(|error| io("refinement.write_failed", &to, error))?;
            copy_permissions(&revised.join(relative), &to)?;
        }
        // The manifest keeps everything the member carried (its Control
        // ground facts among them); only what the draft says it is changes.
        let manifest_path = capsule.join("manifest.toml");
        let text = fs::read_to_string(&manifest_path)
            .map_err(|error| io("refinement.read_failed", &manifest_path, error))?;
        let mut manifest: toml::Table = toml::from_str(&text).map_err(|error| {
            refinement_error(
                "refinement.manifest",
                format!("{}: {error}", manifest_path.display()),
            )
        })?;
        manifest.insert(
            "description".into(),
            toml::Value::String(draft.description.clone()),
        );
        fs::write(
            &manifest_path,
            toml::to_string_pretty(&manifest)
                .map_err(|error| refinement_error("refinement.manifest", error.to_string()))?,
        )
        .map_err(|error| io("refinement.write_failed", &manifest_path, error))?;
        let mut snapshot = record.clone();
        snapshot.digest = digest.clone();
        write_toml_atomic(&staging.join(SNAPSHOT_FILE), &snapshot)
    })();
    if let Err(error) = built {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let final_dir = dir.join("snapshots").join(&digest);
    if final_dir.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| io("refinement.write_failed", &staging, error))?;
    } else {
        fs::rename(&staging, &final_dir)
            .map_err(|error| io("refinement.write_failed", &final_dir, error))?;
    }
    let proposal = ProposedRevision {
        member: member.to_owned(),
        snapshot: digest,
        based_on: active,
        why,
        proposed_at_unix_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default(),
        tests: Vec::new(),
        handed: None,
    };
    state.proposals.push(proposal.clone());
    write_toml_atomic(&dir.join(STATE_FILE), &state)?;
    refinement_of(home, &source, &spec, &state, &proposal)
}

/// Every proposed revision AIKit keeps, each beside the revision in use.
pub fn refinements(home: &AikitHome) -> Result<Vec<Refinement>> {
    let mut out = Vec::new();
    for source in registered_ids(home)? {
        let state = load_state(home, &source)?;
        if state.proposals.is_empty() {
            continue;
        }
        let spec = load_spec(home, &source)?;
        for proposal in &state.proposals {
            out.push(refinement_of(home, &source, &spec, &state, proposal)?);
        }
    }
    Ok(out)
}

/// The proposal standing for `member`, as it is read.
pub fn refinement(home: &AikitHome, member: &str) -> Result<Refinement> {
    let (source, spec, state) = member_source(home, member)?;
    let proposal = state
        .proposals
        .iter()
        .find(|proposal| proposal.member == member)
        .ok_or_else(|| no_proposal(member))?;
    refinement_of(home, &source, &spec, &state, proposal)
}

fn no_proposal(member: &str) -> AikitError {
    refinement_error(
        "refinement.none",
        format!("no proposed revision of `{member}` stands"),
    )
}

/// The proposed revision's own skill directory (its snapshot's payload).
pub fn proposed_payload(home: &AikitHome, refinement: &Refinement) -> PathBuf {
    snapshot_dir(home, &refinement.source, &refinement.proposed.snapshot)
        .join("registry/capsules")
        .join(&refinement.member)
        .join("payload")
}

/// Keep a take that ran under a proposed revision with the proposal (its Test).
pub fn record_test(home: &AikitHome, member: &str, test: RefinementTest) -> Result<()> {
    let (source, _, mut state) = member_source(home, member)?;
    let proposal = state
        .proposals
        .iter_mut()
        .find(|proposal| proposal.member == member)
        .ok_or_else(|| no_proposal(member))?;
    proposal.tests.retain(|kept| kept.trace != test.trace);
    proposal.tests.push(test);
    write_toml_atomic(&source_dir(home, &source).join(STATE_FILE), &state)
}

/// Keep what a take answered to the task's Success conditions with every
/// proposal's Test that names it — the take that ran under the proposed
/// revision, or one of its Repeat that ran beside it (O-I manual §19.7,
/// Result: "What each take answered to the task's Success conditions").
/// Answers how many Tests keep it; none is no error, since a take of a
/// Repeat no proposal tested is kept by none.
pub fn record_answers(
    home: &AikitHome,
    trace: &str,
    answers: &[RefinementAnswer],
) -> Result<usize> {
    let mut kept = 0;
    for source in registered_ids(home)? {
        let mut state = load_state(home, &source)?;
        let mut named = false;
        for test in state
            .proposals
            .iter_mut()
            .flat_map(|proposal| proposal.tests.iter_mut())
        {
            if test.trace == trace || test.beside.iter().any(|beside| beside == trace) {
                test.answers.insert(trace.to_owned(), answers.to_vec());
                named = true;
                kept += 1;
            }
        }
        if named {
            write_toml_atomic(&source_dir(home, &source).join(STATE_FILE), &state)?;
        }
    }
    Ok(kept)
}

/// Accept: keep the proposal as the member's next revision. The snapshot it
/// is in becomes active, the one before stays to Roll back to, and the
/// person's answer is the review its revision needed.
///
/// A member whose source Central keeps is Central's to revise: Accept hands
/// the proposed SKILL.md to Central, which keeps it beside the skill in use
/// and writes it only on the person's Accept there.
pub fn accept_revision(home: &AikitHome, member: &str) -> Result<Accepted> {
    accept_revision_with(
        home,
        member,
        &aikit_adapters::central_file_map::executable(),
    )
}

/// Accept, asking Central through the `ctrl` at `ctrl` for a member whose
/// source Central keeps.
pub(crate) fn accept_revision_with(
    home: &AikitHome,
    member: &str,
    ctrl: &Path,
) -> Result<Accepted> {
    let (source, spec, mut state) = member_source(home, member)?;
    let index = state
        .proposals
        .iter()
        .position(|proposal| proposal.member == member)
        .ok_or_else(|| no_proposal(member))?;
    if let SourceKind::Central { central_root, .. } = &spec.kind {
        return hand_to_central(home, member, &source, central_root, &mut state, index, ctrl)
            .map(Accepted::Handed);
    }
    let proposal = state.proposals[index].clone();
    let active = state.active_snapshot.clone().unwrap_or_default();
    if active != proposal.based_on {
        return Err(refinement_error(
            "refinement.stale",
            format!("the revision of `{member}` was drafted on snapshot `{}`, and source `{source}` now stands at `{active}`: discard it, and propose again on the revision in use", proposal.based_on),
        ));
    }
    // The person's Accept is the review the revised member needed; the other
    // members are the bytes already trusted in the revision in use.
    let registry_source = RegistrySource::new(source.clone());
    let loaded = load_registry(
        &snapshot_dir(home, &source, &proposal.snapshot).join("registry"),
        registry_source.clone(),
    )?;
    let capsule = loaded
        .catalog
        .capsules()
        .into_iter()
        .find(|capsule| capsule.id.to_string() == member)
        .ok_or_else(|| {
            refinement_error(
                "refinement.not_a_member",
                format!("the proposed snapshot holds no `{member}`"),
            )
        })?;
    let Some(capsule_revision) = capsule.revision.clone() else {
        return Err(refinement_error(
            "refinement.not_a_member",
            format!("the proposed `{member}` has no revision"),
        ));
    };
    let revision = capsule_revision.to_string();
    let index_db = Index::open(&home.database())?;
    TrustStore::new(&index_db).record(
        &TrustKey::new(registry_source, capsule.id.clone(), capsule_revision),
        TrustState::Trusted,
        Some("user-accepted refinement"),
    )?;
    state
        .history
        .retain(|item| item != &active && item != &proposal.snapshot);
    state.history.push(active.clone());
    state.active_snapshot = Some(proposal.snapshot.clone());
    state.proposals.remove(index);
    write_toml_atomic(&source_dir(home, &source).join(STATE_FILE), &state)?;
    Ok(Accepted::Kept(AcceptedRevision {
        member: member.to_owned(),
        source,
        revision,
        active_snapshot: proposal.snapshot,
        rollback_to: active,
    }))
}

/// Hand a proposed revision of a member whose source Central keeps to
/// Central (`control.skills.revision.propose`): its SKILL.md as the body,
/// the closes that show its reason, AIKit as who proposed it. Central keeps
/// it beside the skill in use; the person's Accept there writes it. What
/// Central cannot keep is refused in words before it is asked, and Central's
/// own refusal comes back in its words; a refused hand keeps the proposal.
fn hand_to_central(
    home: &AikitHome,
    member: &str,
    source: &str,
    central_root: &Path,
    state: &mut SourceState,
    index: usize,
    ctrl: &Path,
) -> Result<HandedRevision> {
    use aikit_adapters::runner::CommandRunner;
    let proposal = state.proposals[index].clone();
    if let Some(handed) = &proposal.handed {
        return Err(refinement_error(
            "refinement.handed",
            format!("the revision of `{member}` waits in Central already ({}): it is the person's to accept there ({}), and Central writes it then; sync `{source}` after", handed.revision_ref, handed.accept_action),
        ));
    }
    let active = state.active_snapshot.clone().unwrap_or_default();
    if active != proposal.based_on {
        return Err(refinement_error(
            "refinement.stale",
            format!("the revision of `{member}` was drafted on snapshot `{}`, and source `{source}` now stands at `{active}`: discard it, and propose again on the revision in use", proposal.based_on),
        ));
    }
    let capsule = |snapshot: &str| {
        snapshot_dir(home, source, snapshot)
            .join("registry/capsules")
            .join(member)
    };
    let (proposed, in_use) = (capsule(&proposal.snapshot), capsule(&active));
    // Central keeps a revision of the skill's SKILL.md; a proposal that
    // changes any other file is not one it can keep.
    let others = |dir: &Path| -> Result<Vec<(String, u32, Vec<u8>)>> {
        Ok(tree_files(&dir.join("payload"))?
            .into_iter()
            .filter(|(relative, _, _)| relative != "SKILL.md")
            .collect())
    };
    if others(&proposed)? != others(&in_use)? {
        return Err(refinement_error(
            "refinement.central_body_only",
            format!("Central keeps a proposed revision of `{member}`'s SKILL.md only, and this one changes its other files too: discard it, and propose again with only SKILL.md revised"),
        ));
    }
    let body_path = proposed.join("payload/SKILL.md");
    let body = fs::read_to_string(&body_path)
        .map_err(|error| io("refinement.read_failed", &body_path, error))?;
    if body.len() > MAX_HANDED_BODY {
        return Err(refinement_error(
            "refinement.central_body_too_large",
            format!("the proposed SKILL.md of `{member}` is {} bytes, more than the {MAX_HANDED_BODY} AIKit hands to Central in one call", body.len()),
        ));
    }
    let manifest_path = proposed.join("manifest.toml");
    let manifest: toml::Table = toml::from_str(
        &fs::read_to_string(&manifest_path)
            .map_err(|error| io("refinement.read_failed", &manifest_path, error))?,
    )
    .map_err(|error| {
        refinement_error(
            "refinement.manifest",
            format!("{}: {error}", manifest_path.display()),
        )
    })?;
    let name = manifest
        .get("name")
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let scope = manifest
        .get("metadata")
        .and_then(|metadata| metadata.get("control"))
        .and_then(|control| control.get("scope"))
        .and_then(toml::Value::as_str);
    if scope != Some(CENTRAL_HANDED_SCOPE) || name.is_empty() {
        let kept = scope.map_or_else(
            || "no Control scope AIKit read".to_owned(),
            |scope| format!("scope `{scope}`"),
        );
        return Err(refinement_error(
            "refinement.central_scope",
            format!("Central keeps `{member}` in {kept}, and AIKit hands Central a revision only of the person's own Control skills (`{CENTRAL_HANDED_SCOPE}`): a machine's or a project's skill is named by a machine or project its snapshot does not carry. Discard it here, or revise the skill in Central's ground"),
        ));
    }
    let reason = proposal
        .why
        .iter()
        .map(|why| format!("{}: {}", why.task_ref, why.reason))
        .collect::<Vec<_>>()
        .join("; ");
    let root = std::env::var_os("CENTRAL_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| central_root.to_path_buf());
    let input = serde_json::json!({"scope":CENTRAL_HANDED_SCOPE,"name":name,"body":body,"reason":reason,
        "proposed_by":format!("aikit:{member}")});
    let argv = vec![
        ctrl.to_string_lossy().into_owned(),
        "--json".into(),
        "--root".into(),
        root.to_string_lossy().into_owned(),
        "action".into(),
        "run".into(),
        CENTRAL_REVISION_PROPOSE.into(),
        input.to_string(),
    ];
    let output = aikit_adapters::runner::SystemRunner::probe()
        .run(&argv)
        .map_err(|error| {
            refinement_error(
                "refinement.central_unavailable",
                format!("Central could not be asked to keep the revision of `{member}` ({CENTRAL_REVISION_PROPOSE}): {}", error.message()),
            )
        })?;
    let envelope: serde_json::Value =
        serde_json::from_str(&output.stdout).unwrap_or(serde_json::Value::Null);
    if envelope["ok"] != true {
        let words = envelope["error"]["message"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| output.stderr.trim().to_owned());
        return Err(refinement_error(
            "refinement.central_refused",
            format!("Central did not keep the revision of `{member}` ({CENTRAL_REVISION_PROPOSE}): {words}"),
        )
        .with(
            "native_error_code",
            envelope["error"]["code"].as_str().unwrap_or_default(),
        ));
    }
    let data = &envelope["data"];
    let (Some(revision_ref), Some(proposed_digest)) = (
        data["proposal"]["revision_ref"].as_str(),
        data["proposal"]["proposed_digest"].as_str(),
    ) else {
        return Err(refinement_error(
            "refinement.central_invalid",
            format!("Central answered {CENTRAL_REVISION_PROPOSE} for `{member}` with no revision it keeps"),
        ));
    };
    if data["schema"] != CENTRAL_REVISION_READING {
        return Err(refinement_error(
            "refinement.central_invalid",
            format!("Central answered {CENTRAL_REVISION_PROPOSE} for `{member}` with something other than its revision reading"),
        ));
    }
    let handed = HandedToCentral {
        revision_ref: revision_ref.to_owned(),
        proposed_digest: proposed_digest.to_owned(),
        scope: CENTRAL_HANDED_SCOPE.to_owned(),
        name,
        accept_action: data["accept"]["action"]
            .as_str()
            .unwrap_or(CENTRAL_REVISION_ACCEPT)
            .to_owned(),
    };
    state.proposals[index].handed = Some(handed.clone());
    write_toml_atomic(&source_dir(home, source).join(STATE_FILE), state)?;
    Ok(HandedRevision {
        member: member.to_owned(),
        source: source.to_owned(),
        handed,
    })
}

/// Discard: drop the proposal; the revision in use stays. Its snapshot goes
/// with it unless the source names it otherwise.
pub fn discard_revision(home: &AikitHome, member: &str) -> Result<DiscardedRevision> {
    let (source, _, mut state) = member_source(home, member)?;
    let index = state
        .proposals
        .iter()
        .position(|proposal| proposal.member == member)
        .ok_or_else(|| no_proposal(member))?;
    let proposal = state.proposals.remove(index);
    write_toml_atomic(&source_dir(home, &source).join(STATE_FILE), &state)?;
    let named = state
        .active_snapshot
        .iter()
        .chain(state.candidate_snapshot.iter())
        .chain(state.history.iter())
        .chain(state.proposals.iter().map(|other| &other.snapshot))
        .any(|digest| digest == &proposal.snapshot);
    if !named {
        let dir = snapshot_dir(home, &source, &proposal.snapshot);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|error| io("refinement.write_failed", &dir, error))?;
        }
    }
    Ok(DiscardedRevision {
        discarded: member.to_owned(),
        source,
        snapshot: proposal.snapshot,
        in_use: state.active_snapshot,
    })
}

/// What removing a source took with it.
#[derive(Debug, Clone, Serialize)]
pub struct RemovedSource {
    pub id: String,
    pub forced: bool,
    pub removed_snapshots: usize,
    /// The source was already gone. Removal is idempotent: a retry after a
    /// successful remove is a named no-op, not a raw missing-directory error.
    pub already_absent: bool,
}

/// Remove a registered source entirely — its spec, its state and every
/// immutable snapshot it owns. A promoted snapshot feeds the shared catalog,
/// so removing an active source without `force` refuses; with `force` the
/// reply is the receipt of a deliberate loss: catalogued capsules vanish,
/// enabled declarations resolve unavailable, and the next apply rebuilds the
/// projections without the source. Recorded trust decisions are review
/// evidence about capsule revisions; they outlive the source and are inert
/// without it.
pub fn remove(home: &AikitHome, id: &str, force: bool) -> Result<RemovedSource> {
    validate_id(id)?;
    let dir = source_dir(home, id);
    if !dir.is_dir() {
        return Ok(RemovedSource {
            id: id.to_string(),
            forced: force,
            removed_snapshots: 0,
            already_absent: true,
        });
    }
    let spec = load_spec(home, id)?;
    let state = load_state(home, id)?;
    if state.active_snapshot.is_some() && !force {
        return Err(AikitError::new(
            "source.still_active",
            format!(
                "source `{id}` still has an active snapshot feeding the catalog; \
                 pass --force to remove it anyway and accept the loss of its offers"
            ),
        )
        .with("source", id));
    }
    let removed_snapshots = fs::read_dir(dir.join("snapshots"))
        .map(|entries| entries.filter_map(std::result::Result::ok).count())
        .unwrap_or(0);
    fs::remove_dir_all(&dir).map_err(|error| io("source.remove_failed", &dir, error))?;
    Ok(RemovedSource {
        id: spec.id,
        forced: force,
        removed_snapshots,
        already_absent: false,
    })
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

/// Every registered source id, in id order: one directory per registration
/// under the home's `sources/`. An absent `sources/` registers none.
pub fn registered_ids(home: &AikitHome) -> Result<Vec<String>> {
    let root = home.root().join("sources");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io("source.read_failed", &root, error)),
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io("source.read_failed", &root, error))?;
        if entry.path().is_dir() {
            ids.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    ids.sort();
    Ok(ids)
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

/// Read a source's spec by id, for read models (such as projection drift
/// detection) that need a snapshot's canonical location without going through
/// the `source status` surface.
pub fn source_spec(home: &AikitHome, id: &str) -> Result<SourceSpec> {
    load_spec(home, id)
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

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes).map_err(|error| io("source.write_failed", &temporary, error))?;
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
        // Reading the owner's skill tree is a probe: bounded, so a hanging
        // `ctrl` is a source error within the budget, never a stalled read.
        &aikit_adapters::runner::SystemRunner::probe(),
        &aikit_adapters::central_file_map::executable(),
        &root,
        "skill-tree",
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
    let mut seen = BTreeSet::new();
    for file in files {
        let relative = file["path"]
            .as_str()
            .ok_or_else(|| AikitError::new("source.central_invalid", "owner path absent"))?;
        let p = Path::new(relative);
        if !seen.insert(relative.to_owned())
            || p.as_os_str().is_empty()
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
    let active = match aikit_store::generation::read_lock(target) {
        Ok(view) => view
            .active
            .keys()
            .map(ToString::to_string)
            .collect::<std::collections::BTreeSet<_>>(),
        Err(error) => {
            return vec![format!(
                "Generation committed; source-selection receipt unavailable: {}",
                error.message()
            )]
        }
    };
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
            let selected: Vec<_> = snapshot
                .skills
                .iter()
                .filter(|skill| active.contains(&skill.id))
                .map(|skill| skill.id.clone())
                .collect();
            if selected.is_empty() {
                return Ok(());
            }
            validate_owner_snapshot(&spec, &snapshot)?;
            let root = std::env::var_os("CENTRAL_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| central_root.clone());
            let runner = aikit_adapters::runner::SystemRunner::new();
            let executable = aikit_adapters::central_file_map::executable();
            let bundle = central_bundle(&spec)?;
            aikit_adapters::central_file_map::call(
                &runner,
                &executable,
                &root,
                "projection-record",
                &serde_json::json!({"source_ref":source_ref,"project":bundle["project"],
                    "owner":format!("aikit/source/{id}"),"path":target,
                    "generation":generation,"tree_revision":snapshot.owner_revision,
                    "projection_kind":"reported-generation","selected_capsules":selected,"allow_external":true}),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(dir: &Path, name: &str, marker: &str) {
        fs::create_dir_all(dir.join("references")).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: A member under refinement.\n---\n\n{marker}\n"
            ),
        )
        .unwrap();
        fs::write(dir.join("references/rules.md"), format!("{marker}\n")).unwrap();
    }

    /// A Directory source standing in for Central's skill tree (FAKE): its
    /// registration names Central's ground, and the proposal's snapshot
    /// carries the skill's Control scope, as a Central sync's does.
    fn as_central(home: &AikitHome, temp: &Path, scope: &str) {
        let mut spec = load_spec(home, "practice").unwrap();
        spec.kind = SourceKind::Central {
            central_root: temp.join("central"),
            source_ref: "skills".into(),
        };
        write_toml_atomic(&source_dir(home, "practice").join(SPEC_FILE), &spec).unwrap();
        let state = load_state(home, "practice").unwrap();
        let manifest_path = snapshot_dir(home, "practice", &state.proposals[0].snapshot)
            .join("registry/capsules/skill/practice/alpha/manifest.toml");
        let mut manifest: toml::Table =
            toml::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
        let mut control = toml::Table::new();
        control.insert("standing".into(), "active".into());
        control.insert("scope".into(), scope.into());
        let mut metadata = toml::Table::new();
        metadata.insert("control".into(), control.into());
        manifest.insert("metadata".into(), metadata.into());
        fs::write(&manifest_path, toml::to_string_pretty(&manifest).unwrap()).unwrap();
    }

    /// A `ctrl` that keeps each call's arguments and answers `reply` (FAKE:
    /// Central's envelope for `control.skills.revision.propose`).
    #[cfg(unix)]
    fn fake_ctrl(dir: &Path, reply: &serde_json::Value) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("reply.json"), reply.to_string()).unwrap();
        let ctrl = dir.join("ctrl");
        fs::write(
            &ctrl,
            "#!/bin/sh\ndir=$(dirname \"$0\")\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> \"$dir/calls.txt\"; done\nprintf '%s\\n' '---' >> \"$dir/calls.txt\"\ncat \"$dir/reply.json\"\n",
        )
        .unwrap();
        fs::set_permissions(&ctrl, fs::Permissions::from_mode(0o755)).unwrap();
        ctrl
    }

    fn calls(dir: &Path) -> Vec<Vec<String>> {
        fs::read_to_string(dir.join("calls.txt"))
            .unwrap_or_default()
            .split("---\n")
            .filter(|call| !call.is_empty())
            .map(|call| call.lines().map(str::to_owned).collect())
            .collect()
    }

    fn proposed_alpha(home: &AikitHome, temp: &Path, only_skill_md: bool) {
        let draft = temp.join("drafts/alpha");
        let _ = fs::remove_dir_all(&draft);
        skill(
            &draft,
            "alpha",
            if only_skill_md {
                "alpha-one"
            } else {
                "alpha-two"
            },
        );
        fs::write(
            draft.join("SKILL.md"),
            "---\nname: alpha\ndescription: A member under refinement.\n---\n\nalpha-two\n",
        )
        .unwrap();
        let why = vec![RefinementReason {
            task_ref: "task:plan".into(),
            reason: "Not met".into(),
        }];
        propose_revision(home, "skill/practice/alpha", &draft, why).unwrap();
    }

    // REPLACED (O-I 795 round 13): a_central_sources_proposal_is_refused_accept_in_words_and_can_be_discarded, "its Accept says in
    // words that Central has no operation to keep it": Central keeps one now (ctrl control_skills.rs control.skills.revision.*).
    /// A skill whose source Central keeps is Central's to revise: Accept
    /// hands the proposed SKILL.md to Central, which keeps it beside the skill
    /// in use (`control.skills.revision.propose`) and writes it only on the
    /// person's Accept there. AIKit's revision in use does not move, the
    /// proposal stays with what it was handed as, and Discard still drops it.
    #[cfg(unix)]
    #[test]
    fn a_central_sources_proposal_is_handed_to_central_which_keeps_it_for_the_persons_accept() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("aikit"));
        home.ensure_layout().unwrap();
        let source = temp.path().join("practice");
        skill(&source.join("alpha"), "alpha", "alpha-one");
        add_directory(&home, "practice", &source, false).unwrap();
        sync(&home, "practice").unwrap();
        promote(&home, "practice", false, &[]).unwrap();
        proposed_alpha(&home, temp.path(), true);
        as_central(&home, temp.path(), "control-user");
        let in_use = load_state(&home, "practice").unwrap().active_snapshot;
        assert_eq!(refinements(&home).unwrap()[0].owner, "central");
        let bin = temp.path().join("bin");
        let ctrl = fake_ctrl(
            &bin,
            &serde_json::json!({"ok":true,"data":{"schema":"central.skill-revision-reading/v1","name":"alpha","scope":"control-user",
                "path":"Control/user/skills/alpha","proposal":{"revision_ref":"central:skill-revision:k1","proposed_digest":"sha256:d1"},
                "accept":{"available":true,"action":"control.skills.revision.accept"},"skill_bytes_mutated":false}}),
        );
        let Accepted::Handed(handed) =
            accept_revision_with(&home, "skill/practice/alpha", &ctrl).unwrap()
        else {
            panic!("a Central source's proposal is handed to Central, not kept here");
        };
        assert_eq!(handed.handed.revision_ref, "central:skill-revision:k1");
        assert_eq!(handed.handed.proposed_digest, "sha256:d1");
        assert_eq!(handed.handed.scope, "control-user");
        assert_eq!(handed.handed.name, "alpha");
        assert_eq!(
            handed.handed.accept_action,
            "control.skills.revision.accept"
        );
        // One call (its arguments after the executable): Central's propose,
        // at Central's root, with the proposed SKILL.md as its body and the
        // closes that show its reason.
        let made = calls(&bin);
        assert_eq!(made.len(), 1, "{made:?}");
        let call = &made[0];
        let root = temp.path().join("central");
        assert_eq!(
            &call[..6],
            [
                "--json",
                "--root",
                root.to_str().unwrap(),
                "action",
                "run",
                "control.skills.revision.propose"
            ]
        );
        let input: serde_json::Value = serde_json::from_str(&call[6]).unwrap();
        assert_eq!(input["scope"], "control-user");
        assert_eq!(input["name"], "alpha");
        assert_eq!(
            input["body"],
            "---\nname: alpha\ndescription: A member under refinement.\n---\n\nalpha-two\n"
        );
        assert_eq!(input["reason"], "task:plan: Not met");
        assert_eq!(input["proposed_by"], "aikit:skill/practice/alpha");
        // AIKit's revision in use stays; the proposal names where it waits.
        let state = load_state(&home, "practice").unwrap();
        assert_eq!(state.active_snapshot, in_use);
        assert_eq!(state.proposals.len(), 1);
        let listed = refinements(&home).unwrap();
        assert_eq!(
            listed[0].handed.as_ref().map(|h| h.revision_ref.as_str()),
            Some("central:skill-revision:k1")
        );
        // Accept again: it waits in Central for the person's Accept there,
        // and Central is not asked twice.
        let again = accept_revision_with(&home, "skill/practice/alpha", &ctrl).unwrap_err();
        assert_eq!(again.code(), "refinement.handed");
        assert!(
            again.message().contains("control.skills.revision.accept")
                && again.message().contains("central:skill-revision:k1"),
            "{}",
            again.message()
        );
        assert_eq!(calls(&bin).len(), 1);
        let discarded = discard_revision(&home, "skill/practice/alpha").unwrap();
        assert_eq!(discarded.discarded, "skill/practice/alpha");
        assert!(refinements(&home).unwrap().is_empty());
    }

    /// What Central cannot keep is refused in words before or after it is
    /// asked: a proposal that changes more than SKILL.md, a skill outside the
    /// person's own Control skills, and Central's own refusal. Each refused
    /// Accept keeps the proposal, handed to no one.
    #[cfg(unix)]
    #[test]
    fn a_central_proposal_central_cannot_keep_is_refused_in_words_and_kept() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("aikit"));
        home.ensure_layout().unwrap();
        let source = temp.path().join("practice");
        skill(&source.join("alpha"), "alpha", "alpha-one");
        add_directory(&home, "practice", &source, false).unwrap();
        sync(&home, "practice").unwrap();
        promote(&home, "practice", false, &[]).unwrap();
        let bin = temp.path().join("bin");
        let ctrl = fake_ctrl(
            &bin,
            &serde_json::json!({"ok":false,"error":{"code":"skill_revision_stale",
                "message":"a proposed revision of alpha waits already (central:skill-revision:k0): accept or discard it first"}}),
        );
        // More than SKILL.md: Central keeps a revision of SKILL.md only.
        proposed_alpha(&home, temp.path(), false);
        as_central(&home, temp.path(), "control-user");
        let refused = accept_revision_with(&home, "skill/practice/alpha", &ctrl).unwrap_err();
        assert_eq!(refused.code(), "refinement.central_body_only");
        assert!(
            refused.message().contains("SKILL.md"),
            "{}",
            refused.message()
        );
        assert!(calls(&bin).is_empty(), "Central is not asked");
        discard_revision(&home, "skill/practice/alpha").unwrap();
        // A machine's skill: AIKit's snapshot does not name the machine.
        proposed_alpha(&home, temp.path(), true);
        as_central(&home, temp.path(), "control-machine");
        let refused = accept_revision_with(&home, "skill/practice/alpha", &ctrl).unwrap_err();
        assert_eq!(refused.code(), "refinement.central_scope");
        assert!(
            refused.message().contains("control-machine"),
            "{}",
            refused.message()
        );
        assert!(calls(&bin).is_empty(), "Central is not asked");
        // Central's own refusal, in its words.
        as_central(&home, temp.path(), "control-user");
        let refused = accept_revision_with(&home, "skill/practice/alpha", &ctrl).unwrap_err();
        assert_eq!(refused.code(), "refinement.central_refused");
        assert!(
            refused.message().contains("waits already")
                && refused
                    .message()
                    .contains("control.skills.revision.propose"),
            "{}",
            refused.message()
        );
        assert_eq!(calls(&bin).len(), 1);
        let state = load_state(&home, "practice").unwrap();
        assert_eq!(state.proposals.len(), 1);
        assert!(state.proposals[0].handed.is_none());
    }
}
