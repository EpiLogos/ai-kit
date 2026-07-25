//! Adoption: moving authority from a foreign Agent Skills root into AIKit.
//!
//! Import and discovery are reads. Adoption is the moment AIKit becomes the
//! source of truth, so it is planned as one Procedure: every byte is copied into
//! the personal registry, then each foreign file is replaced by a projection
//! link to that owned payload. Undo restores the original bytes and links from
//! the Procedure journal.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use aikit_adapters::clients::agent_skills;
use aikit_core::procedure::{
    select_isolation, Inverse, Plan, PlanDigest, Procedure, ProcedureKind, UndoRecord, WorldEdit,
};
use aikit_core::{AikitError, CapsuleId, ProcedureId, Result};
use aikit_store::home::AikitHome;
use aikit_store::procedure::{JOURNAL_FILE, PLAN_FILE, PROCEDURE_FILE};

#[derive(Debug, Clone, Eq, PartialEq)]
struct SkillRoot {
    /// The directory entry inside the requested foreign authority root.
    projection: PathBuf,
    /// The resolved directory whose bytes are copied into AIKit ownership.
    content: PathBuf,
    /// The original link target, preserved exactly (including relativity) for undo.
    original_link: Option<PathBuf>,
}

/// The plan plus the facts the CLI reports before and after applying it.
pub struct Adoption {
    pub procedure: Procedure,
    pub review_digest: PlanDigest,
    pub source: PathBuf,
    pub namespace: String,
    pub capsules: Vec<CapsuleId>,
}

#[derive(Serialize)]
struct Manifest<'a> {
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

#[derive(Serialize)]
struct OwnershipRecord<'a> {
    schema: u32,
    ownership: &'static str,
    source: String,
    namespace: &'a str,
    procedure: String,
    adopted_at: String,
    capsules: Vec<String>,
}

/// Durable authority state consumed by the tree and doctor surfaces.
#[derive(Debug, Clone, Deserialize)]
pub struct AdoptionRecord {
    pub schema: u32,
    pub ownership: String,
    pub source: PathBuf,
    pub namespace: String,
    pub procedure: String,
    pub adopted_at: String,
    pub capsules: Vec<String>,
}

/// Read every adoption record. A malformed record is an authority-state error,
/// not something the UI may silently relabel as foreign.
pub fn records(home: &AikitHome) -> Result<Vec<AdoptionRecord>> {
    let root = home.state().join("adoptions");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(AikitError::new(
                "adopt.record_unreadable",
                format!("could not read {}: {error}", root.display()),
            ))
        }
    };
    let mut records = Vec::new();
    let mut sources = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "adopt.record_unreadable",
                format!("could not read an entry in {}: {error}", root.display()),
            )
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|error| {
            AikitError::new(
                "adopt.record_unreadable",
                format!("could not read {}: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        let record: AdoptionRecord = toml::from_str(&text).map_err(|error| {
            AikitError::new(
                "adopt.record_unreadable",
                format!("{} is not a valid adoption record: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        let procedure = ProcedureId::parse(&record.procedure);
        let capsule_ids: Result<Vec<CapsuleId>> = record
            .capsules
            .iter()
            .map(|capsule| CapsuleId::parse(capsule))
            .collect();
        let file_namespace = path.file_stem().and_then(|value| value.to_str());
        if record.schema != 1
            || record.ownership != "adopted"
            || procedure.is_err()
            || record.adopted_at.parse::<jiff::Timestamp>().is_err()
            || valid_slug(&record.namespace, "namespace").is_err()
            || file_namespace != Some(record.namespace.as_str())
            || capsule_ids.is_err()
            || record.capsules.is_empty()
        {
            return Err(AikitError::new(
                "adopt.record_unreadable",
                format!("{} contains invalid authority metadata", path.display()),
            )
            .with("path", path.display().to_string()));
        }
        let canonical = std::fs::canonicalize(&record.source).map_err(|error| {
            AikitError::new(
                "adopt.record_unreadable",
                format!(
                    "{} names an authority root that cannot be resolved: {error}",
                    path.display()
                ),
            )
        })?;
        if !record.source.is_absolute()
            || canonical != record.source
            || !sources.insert(record.source.clone())
        {
            return Err(AikitError::new(
                "adopt.record_unreadable",
                format!(
                    "{} has a non-canonical or duplicate authority source",
                    path.display()
                ),
            ));
        }
        let procedure = procedure.unwrap();
        let capsule_ids = capsule_ids.unwrap();
        let procedure_dir = home.state().join("procedures").join(procedure.to_string());
        let metadata_path = procedure_dir.join(PROCEDURE_FILE);
        let plan_path = procedure_dir.join(PLAN_FILE);
        let journal_path = procedure_dir.join(JOURNAL_FILE);
        let stored_procedure: Procedure =
            read_json_record(&metadata_path, "Procedure metadata", &path)?;
        let stored_plan: Plan = read_json_record(&plan_path, "Procedure plan", &path)?;
        let journal: UndoRecord = read_json_record(&journal_path, "Procedure journal", &path)?;
        let mut recorded_capsules = capsule_ids.clone();
        recorded_capsules.sort();
        let mut procedure_capsules = match &stored_procedure.kind {
            ProcedureKind::Adopt {
                source,
                namespace,
                capsules,
            } if source == &record.source && namespace == &record.namespace => capsules.clone(),
            _ => {
                return Err(invalid_record(
                    &path,
                    "names a Procedure for a different adoption authority root",
                ))
            }
        };
        procedure_capsules.sort();
        if stored_procedure.id != procedure
            || stored_procedure.plan != stored_plan
            || stored_procedure.digest != stored_plan.digest()
            || journal.procedure != procedure
            || journal.digest != stored_procedure.digest
            || procedure_capsules != recorded_capsules
        {
            return Err(invalid_record(
                &path,
                "does not match the exact durable adoption Procedure",
            ));
        }
        for capsule in capsule_ids {
            if capsule.kind().as_str() != "skill"
                || capsule.path().split('/').next() != Some(record.namespace.as_str())
                || !home
                    .registry("personal")
                    .join(capsule.registry_path())
                    .join("manifest.toml")
                    .is_file()
            {
                return Err(AikitError::new(
                    "adopt.record_unreadable",
                    format!(
                        "{} names a capsule outside its namespace or without owned payload",
                        path.display()
                    ),
                ));
            }
        }
        records.push(record);
    }
    records.sort_by(|left, right| left.namespace.cmp(&right.namespace));
    Ok(records)
}

fn read_json_record<T: serde::de::DeserializeOwned>(
    path: &Path,
    description: &str,
    record_path: &Path,
) -> Result<T> {
    let bytes = std::fs::read(path).map_err(|error| {
        invalid_record(
            record_path,
            &format!("names a Procedure whose {description} cannot be read: {error}"),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        invalid_record(
            record_path,
            &format!("names a Procedure whose {description} is invalid: {error}"),
        )
    })
}

fn invalid_record(path: &Path, reason: &str) -> AikitError {
    AikitError::new(
        "adopt.record_unreadable",
        format!("{} {reason}", path.display()),
    )
    .with("path", path.display().to_string())
}

/// Survey a foreign root and construct the complete reversible adoption.
///
/// This function writes nothing. In particular it refuses every collision
/// before producing a Procedure, so confirmation can never become a partial
/// import.
pub fn plan(
    home: &AikitHome,
    source: &Path,
    requested_namespace: Option<&str>,
) -> Result<Adoption> {
    if !source.is_dir() {
        return Err(AikitError::new(
            "adopt.root_not_found",
            format!(
                "`{}` is not a foreign skill root directory",
                source.display()
            ),
        )
        .with("root", source.display().to_string()));
    }
    if std::fs::symlink_metadata(source)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(AikitError::new(
            "adopt.symlink_not_supported",
            format!(
                "refusing to adopt symlinked root {}; use its real directory so the authority boundary is explicit",
                source.display()
            ),
        )
        .with("path", source.display().to_string()));
    }
    let source = std::fs::canonicalize(source).map_err(|error| {
        AikitError::new(
            "adopt.root_unreadable",
            format!("could not resolve {}: {error}", source.display()),
        )
        .with("root", source.display().to_string())
    })?;

    let namespace = match requested_namespace {
        Some(value) => valid_slug(value, "namespace")?,
        None => {
            let leaf = source
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("adopted");
            slug(leaf, "namespace")?
        }
    };

    let mut roots = find_skills(&source)?;
    if roots.is_empty() {
        return Err(AikitError::new(
            "adopt.no_skills",
            format!("`{}` contains no valid Agent Skills", source.display()),
        )
        .with("root", source.display().to_string()));
    }
    roots.sort_by(|left, right| left.projection.cmp(&right.projection));
    reject_overlaps(&roots)?;

    let procedure_id = ProcedureId::generate();
    let mut plan = Plan::new().with_note(format!(
        "adopt {} valid Agent Skill(s) from {} into the personal registry",
        roots.len(),
        source.display()
    ));
    let mut capsules = Vec::new();
    let mut seen = BTreeSet::new();
    let mut review_facts = vec![format!("source-root:{}", source.display())];
    let mut content_roots = BTreeSet::new();

    for skill_root in roots {
        content_roots.insert(skill_root.content.clone());
        review_facts.push(format!(
            "skill-root:{}\ncontent-root:{}\noriginal-link:{}",
            skill_root.projection.display(),
            skill_root.content.display(),
            skill_root
                .original_link
                .as_deref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "<directory>".to_string())
        ));
        reject_symlinks(&skill_root.content)?;
        let skill = agent_skills::validate(&skill_root.content)?;
        let relative = skill_root.projection.strip_prefix(&source).map_err(|_| {
            AikitError::new(
                "adopt.outside_root",
                format!(
                    "{} is outside {}",
                    skill_root.projection.display(),
                    source.display()
                ),
            )
        })?;
        let path = capsule_path(relative)?;
        let id = CapsuleId::parse(&format!("skill/{namespace}/{path}"))?;
        if !seen.insert(id.clone()) {
            return Err(AikitError::new(
                "adopt.id_collision",
                format!("more than one foreign skill maps to `{id}`"),
            )
            .with("capsule", id.to_string()));
        }

        let owned = home.registry("personal").join(id.registry_path());
        if owned.exists() {
            return Err(AikitError::new(
                "adopt.destination_exists",
                format!("refusing to overwrite the existing owned capsule `{id}`"),
            )
            .with("capsule", id.to_string())
            .with("path", owned.display().to_string()));
        }

        let id_text = id.to_string();
        let manifest = toml::to_string_pretty(&Manifest {
            schema: 1,
            id: &id_text,
            kind: "skill",
            name: &skill.name,
            description: &skill.description,
            skill: SkillSection { root: "payload" },
        })
        .map_err(|error| {
            AikitError::new(
                "adopt.manifest_failed",
                format!("could not encode the manifest for `{id}`: {error}"),
            )
        })?;
        plan = plan.with_edit(WorldEdit::WriteFile {
            path: owned.join("manifest.toml"),
            contents: manifest.into_bytes(),
            inverse: Inverse::Remove,
        });

        for relative_file in &skill.files {
            let from = skill_root.content.join(relative_file);
            let bytes = std::fs::read(&from).map_err(|error| {
                AikitError::new(
                    "adopt.read_failed",
                    format!("could not read {}: {error}", from.display()),
                )
                .with("path", from.display().to_string())
            })?;
            let to = owned.join("payload").join(relative_file);
            let mode = source_mode(&from)?;
            plan = plan.with_edit(WorldEdit::WriteFileMode {
                path: to.clone(),
                contents: bytes,
                mode,
                inverse: Inverse::Remove,
            });
            // A normal foreign directory is retained as a directory and each
            // original file becomes a projection. For a linked skill root, the
            // link itself is the foreign projection boundary: changing files at
            // its resolved target would mutate a different authority tree.
            if skill_root.original_link.is_none() {
                plan = plan.with_edit(WorldEdit::CreateLink {
                    path: skill_root.projection.join(relative_file),
                    target: to,
                    inverse: Inverse::Restore {
                        blob: aikit_core::procedure::BlobId::deferred(),
                    },
                });
            }
        }
        if let Some(original_target) = &skill_root.original_link {
            plan = plan.with_edit(WorldEdit::CreateLink {
                path: skill_root.projection.clone(),
                target: owned.join("payload"),
                inverse: Inverse::Recreate {
                    target: original_target.clone(),
                },
            });
        }
        capsules.push(id);
    }

    // Bind the review to the exact source entries before adding volatile audit
    // metadata (Procedure id and timestamp).
    plan = aikit_store::procedure::bind_current_preconditions(plan)?;
    plan = aikit_store::procedure::bind_read_precondition(plan, &source)?;
    for content_root in content_roots {
        plan = aikit_store::procedure::bind_read_precondition(plan, &content_root)?;
    }

    // The review identity covers every authority-moving edit and its source
    // bytes/mode. Audit metadata (fresh Procedure id and commit timestamp) is
    // appended afterwards and deliberately does not make an unchanged source
    // impossible to confirm across two short-lived CLI invocations.
    let review_digest = plan.review_digest(&review_facts);

    let record_path = home
        .state()
        .join("adoptions")
        .join(format!("{namespace}.toml"));
    if record_path.exists() {
        return Err(AikitError::new(
            "adopt.namespace_exists",
            format!("the namespace `{namespace}` already has an adoption record"),
        )
        .with("path", record_path.display().to_string()));
    }
    let record = toml::to_string_pretty(&OwnershipRecord {
        schema: 1,
        ownership: "adopted",
        source: source.display().to_string(),
        namespace: &namespace,
        procedure: procedure_id.to_string(),
        adopted_at: jiff::Timestamp::now().to_string(),
        capsules: capsules.iter().map(ToString::to_string).collect(),
    })
    .map_err(|error| {
        AikitError::new(
            "adopt.record_failed",
            format!("could not encode the ownership record: {error}"),
        )
    })?;
    plan = plan.with_edit(WorldEdit::WriteFile {
        path: record_path,
        contents: record.into_bytes(),
        inverse: Inverse::Remove,
    });
    plan = aikit_store::procedure::bind_current_preconditions(plan)?;

    let shadow = home.state().join("procedures").join(".shadow");
    let isolation = select_isolation(&plan, &shadow, aikit_store::procedure::git_repo_of);
    let procedure = Procedure::with_id(
        procedure_id,
        ProcedureKind::Adopt {
            source: source.clone(),
            namespace: namespace.clone(),
            capsules: capsules.clone(),
        },
        plan,
        isolation,
    )?;

    Ok(Adoption {
        procedure,
        review_digest,
        source,
        namespace,
        capsules,
    })
}

fn source_mode(path: &Path) -> Result<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o7777)
            .map_err(|error| {
                AikitError::new(
                    "adopt.read_failed",
                    format!("could not read metadata for {}: {error}", path.display()),
                )
                .with("path", path.display().to_string())
            })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(0o644)
    }
}

fn find_skills(root: &Path) -> Result<Vec<SkillRoot>> {
    let mut skills = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
    {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "adopt.survey_failed",
                format!("could not survey {}: {error}", root.display()),
            )
        })?;
        if entry.file_type().is_dir() && entry.path().join(agent_skills::SKILL_FILE).is_file() {
            agent_skills::validate(entry.path())?;
            skills.push(SkillRoot {
                projection: entry.path().to_path_buf(),
                content: entry.path().to_path_buf(),
                original_link: None,
            });
        } else if entry.file_type().is_symlink() {
            let Ok(content) = std::fs::canonicalize(entry.path()) else {
                continue;
            };
            if content.is_dir() && content.join(agent_skills::SKILL_FILE).is_file() {
                agent_skills::validate(&content)?;
                let original_link = std::fs::read_link(entry.path()).map_err(|error| {
                    AikitError::new(
                        "adopt.survey_failed",
                        format!("could not read link {}: {error}", entry.path().display()),
                    )
                })?;
                skills.push(SkillRoot {
                    projection: entry.path().to_path_buf(),
                    content,
                    original_link: Some(original_link),
                });
            }
        }
    }
    Ok(skills)
}

fn reject_symlinks(root: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter() {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "adopt.survey_failed",
                format!("could not survey {}: {error}", root.display()),
            )
        })?;
        if entry.file_type().is_symlink() {
            return Err(AikitError::new(
                "adopt.symlink_not_supported",
                format!(
                    "refusing to adopt {}; symlink {} may cross the requested authority boundary",
                    root.display(),
                    entry.path().display()
                ),
            )
            .with("skill", root.display().to_string())
            .with("path", entry.path().display().to_string()));
        }
    }
    Ok(())
}

fn reject_overlaps(roots: &[SkillRoot]) -> Result<()> {
    for (index, parent) in roots.iter().enumerate() {
        if let Some(child) = roots
            .iter()
            .skip(index + 1)
            .find(|path| path.projection.starts_with(&parent.projection))
        {
            return Err(AikitError::new(
                "adopt.overlapping_skills",
                format!(
                    "{} is both a skill and a container for {}; adopt them separately so each \
                     owned capsule has one source of truth",
                    parent.projection.display(),
                    child.projection.display()
                ),
            ));
        }
    }
    Ok(())
}

fn capsule_path(relative: &Path) -> Result<String> {
    let mut segments = Vec::new();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err(AikitError::new(
                "adopt.invalid_path",
                format!("`{}` is not a safe relative skill path", relative.display()),
            ));
        };
        segments.push(slug(&value.to_string_lossy(), "skill path")?);
    }
    if segments.is_empty() {
        return Err(AikitError::new(
            "adopt.invalid_path",
            "a skill may not be the foreign root itself",
        ));
    }
    Ok(segments.join("/"))
}

fn valid_slug(value: &str, field: &str) -> Result<String> {
    let candidate = format!("skill/{value}/probe");
    CapsuleId::parse(&candidate).map_err(|_| {
        AikitError::new(
            "adopt.invalid_namespace",
            format!(
                "`{value}` is not a valid {field}; use lowercase letters, digits, `_`, `-` or `.`"
            ),
        )
        .with(field, value.to_string())
    })?;
    Ok(value.to_string())
}

fn slug(value: &str, field: &str) -> Result<String> {
    let mut out = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !out.is_empty() {
                out.push('-');
            }
            out.push(character.to_ascii_lowercase());
            separator = false;
        } else if matches!(character, '-' | '_' | '.') {
            if separator && !out.is_empty() {
                out.push('-');
            }
            out.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    while out.ends_with(['-', '_', '.']) {
        out.pop();
    }
    if out.is_empty() {
        return Err(AikitError::new(
            "adopt.invalid_path",
            format!("`{value}` cannot become a valid {field}"),
        ));
    }
    valid_slug(&out, field)
}
