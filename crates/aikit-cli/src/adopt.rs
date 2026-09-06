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

/// Stage personal, project or machine skills on authored Control ground through a reviewed Procedure.
/// Originals remain intact until the separately reviewed projection cutover.
/// Existing staged skills are accepted only when their full payload is identical;
/// their authored standing and provenance are never overwritten.
pub fn plan_control(
    home: &AikitHome,
    source: &Path,
    ground: &Path,
    requested_namespace: Option<&str>,
) -> Result<Adoption> {
    let refusal = |message: String| AikitError::new("adopt.control_ground", message);
    if std::fs::symlink_metadata(source)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(refusal(
            "use the real source directory, not a symlinked root".into(),
        ));
    }
    let source = std::fs::canonicalize(source).map_err(|e| refusal(e.to_string()))?;
    let parent = ground
        .parent()
        .ok_or_else(|| refusal("missing skill scope".into()))?;
    let parent = std::fs::canonicalize(parent).map_err(|e| refusal(e.to_string()))?;
    if ground.file_name().and_then(|s| s.to_str()) != Some("skills") {
        return Err(refusal(
            "the published skill scope must end in skills".into(),
        ));
    }
    let ground = parent.join("skills");
    if std::fs::symlink_metadata(&ground)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(refusal(
            "use the authored skill scope, not a projection link".into(),
        ));
    }
    let owner = parent
        .parent()
        .ok_or_else(|| refusal("missing authored scope owner".into()))?;
    let leaf = |p: &Path| p.file_name().and_then(|s| s.to_str()).map(str::to_owned);
    let (scope, machine, default_namespace) = if leaf(&parent).as_deref() == Some("user")
        && leaf(owner).as_deref() == Some("Control")
        && owner.parent().is_some_and(|p| p.join(".central").is_dir())
    {
        ("control-user", None, "personal-ground")
    } else if leaf(&parent).as_deref() == Some("user")
        && leaf(owner).as_deref() == Some("ProjectCentral")
        && owner.join("project.json").is_file()
    {
        let project_root = owner
            .parent()
            .ok_or_else(|| refusal("missing project root".into()))?;
        aikit_adapters::projectcentral::ProjectCentralFilesystemBinding::inspect(
            project_root,
            None,
        )?;
        ("projectcentral-user", None, "project-ground")
    } else if leaf(owner).as_deref() == Some("machines")
        && owner.parent().is_some_and(|p| {
            leaf(p).as_deref() == Some("Control")
                && p.parent().is_some_and(|r| r.join(".central").is_dir())
        })
    {
        ("control-machine", leaf(&parent), "machine-ground")
    } else {
        return Err(refusal("expected Central Control/user/skills, Control/machines/ROLE/skills, or a valid ProjectCentral/user/skills scope".into()));
    };
    if !source.is_dir() || source.starts_with(&ground) || ground.starts_with(&source) {
        return Err(refusal(
            "source and authored ground must be disjoint directories".into(),
        ));
    }
    let namespace = valid_slug(
        requested_namespace.unwrap_or(default_namespace),
        "namespace",
    )?;
    let mut roots = find_skills(&source)?;
    roots.sort_by(|a, b| a.projection.cmp(&b.projection));
    reject_overlaps(&roots)?;
    if roots.is_empty() {
        return Err(refusal("source contains no valid skills".into()));
    }
    let mut plan = Plan::new().with_note(format!(
        "stage skills on Control ground {}; preserve originals until projection cutover",
        ground.display()
    ));
    let mut capsules = Vec::new();
    let mut names = BTreeSet::new();
    for root in roots {
        if root.original_link.is_some() {
            return Err(refusal(format!(
                "external source {} must be registered as a source, not copied into Control",
                root.projection.display()
            )));
        }
        reject_symlinks(&root.content)?;
        let skill = agent_skills::validate(&root.content)?;
        let name = root
            .projection
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| refusal("skill name must be UTF-8".into()))?;
        valid_slug(name, "skill")?;
        if !names.insert(name.to_string()) {
            return Err(refusal(format!("duplicate skill name {name}")));
        }
        let target = ground.join(name);
        if std::fs::symlink_metadata(&target)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(refusal(format!(
                "refusing symlinked Control destination {}",
                target.display()
            )));
        }
        if skill.files.iter().any(|p| p.as_str() == "skill.json") {
            return Err(refusal(format!(
                "{name} already carries a Control manifest; register its authored source directly"
            )));
        }
        let existing = target.exists();
        if existing {
            reject_symlinks(&target)?;
            let contract = crate::control_ground::read(&target, name)?
                .ok_or_else(|| refusal(format!("{} has no Control manifest", target.display())))?;
            if contract.scope.as_deref() != Some(scope) {
                return Err(refusal(format!(
                    "{} declares a different authored scope",
                    target.display()
                )));
            }
            let staged = agent_skills::validate(&target)?;
            let expected: BTreeSet<_> = skill
                .files
                .iter()
                .filter(|p| p.as_str() != "skill.json")
                .collect();
            let actual: BTreeSet<_> = staged
                .files
                .iter()
                .filter(|p| p.as_str() != "skill.json")
                .collect();
            if actual != expected {
                return Err(refusal(format!(
                    "staged payload file set differs for {name}"
                )));
            }
        }
        for relative in &skill.files {
            if relative == Path::new("skill.json") {
                continue;
            }
            let from = root.content.join(relative);
            let to = target.join(relative);
            let bytes = std::fs::read(&from).map_err(|e| refusal(e.to_string()))?;
            let mode = source_mode(&from)?;
            if existing {
                if std::fs::read(&to).map_err(|e| refusal(e.to_string()))? != bytes
                    || source_mode(&to)? != mode
                {
                    return Err(refusal(format!(
                        "staged bytes or mode differ at {}",
                        to.display()
                    )));
                }
            } else {
                plan = plan.with_edit(WorldEdit::WriteFileMode {
                    path: to,
                    contents: bytes,
                    mode,
                    inverse: Inverse::Remove,
                });
            }
        }
        if !existing {
            let mut manifest = serde_json::json!({
                "schema": "central.skill/v1", "name": name,
                "scope": scope,
                "standing": "active", "provenance": "adopted",
                "adopted_from": root.content,
            });
            if let Some(machine) = &machine {
                manifest["machine"] = machine.clone().into();
            }
            plan = plan.with_edit(WorldEdit::WriteFile {
                path: target.join("skill.json"),
                contents: serde_json::to_vec_pretty(&manifest)
                    .map_err(|e| refusal(e.to_string()))?,
                inverse: Inverse::Remove,
            });
        }
        capsules.push(CapsuleId::parse(&format!("skill/{namespace}/{name}"))?);
    }
    plan = aikit_store::procedure::bind_current_preconditions(plan)?;
    plan = aikit_store::procedure::bind_read_precondition(plan, &source)?;
    plan = aikit_store::procedure::bind_read_precondition(
        plan,
        if ground.exists() { &ground } else { &parent },
    )?;
    let review_digest = plan.review_digest(&[format!("Control ground: {}", ground.display())]);
    let shadow = home.state().join("procedures/.shadow");
    let isolation = select_isolation(&plan, &shadow, aikit_store::procedure::git_repo_of);
    let procedure = Procedure::with_id(
        ProcedureId::generate(),
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

/// Finish a staged Control adoption without creating another authored master.
/// The old tree is retained solely as the Procedure's reversible undo material.
pub fn plan_control_cutover(
    home: &AikitHome,
    source: &Path,
    ground: &Path,
    projection: &Path,
    namespace: Option<&str>,
) -> Result<Adoption> {
    let refusal = |message: String| AikitError::new("adopt.cutover_refused", message);
    let staged = plan_control(home, source, ground, namespace)?;
    if !staged.procedure.plan.edits.is_empty() {
        return Err(refusal(
            "adopt and accept all Control payloads before projection cutover".into(),
        ));
    }
    let source = &staged.source;
    let (projection, current, generation) = native_projection(home, projection)?;
    if !projection.is_dir() || projection.starts_with(source) || source.starts_with(&projection) {
        return Err(refusal(
            "projection and former authored root must be disjoint".into(),
        ));
    }
    let roots = find_skills(source)?;
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(|e| refusal(e.to_string()))?;
        if !entry.file_type().is_dir()
            && !roots.iter().any(|r| entry.path().starts_with(&r.content))
        {
            return Err(refusal(format!(
                "unaccounted source entry {}",
                entry.path().display()
            )));
        }
    }
    for root in roots {
        let name = root.content.file_name().unwrap();
        let authored = ground.join(name);
        let contract = crate::control_ground::read(&authored, &name.to_string_lossy())?
            .ok_or_else(|| refusal("Control standing must be authored before cutover".into()))?;
        let projected = projection.join(name);
        if contract.is_retired() {
            if projected.exists() || projected.is_symlink() {
                return Err(refusal(format!(
                    "retired skill {} is present in the generation",
                    name.to_string_lossy()
                )));
            }
            continue;
        }
        let original = agent_skills::validate(&root.content)?;
        let generated = agent_skills::validate(&projected)?;
        let payload = |files: &[String]| {
            files
                .iter()
                .filter(|p| p.as_str() != "skill.json")
                .cloned()
                .collect::<BTreeSet<_>>()
        };
        if payload(&original.files) != payload(&generated.files) {
            return Err(refusal(format!(
                "generated payload set differs for {}",
                name.to_string_lossy()
            )));
        }
        for relative in original.files {
            if std::fs::read(root.content.join(&relative)).map_err(|e| refusal(e.to_string()))?
                != std::fs::read(projected.join(&relative)).map_err(|e| refusal(e.to_string()))?
                || source_mode(&root.content.join(&relative))?
                    != source_mode(&projected.join(&relative))?
            {
                return Err(refusal(format!(
                    "generated bytes or mode differ at {}",
                    projected.join(relative).display()
                )));
            }
        }
    }
    let backup = home
        .state()
        .join("adoption-undo")
        .join(staged.review_digest.as_str());
    if backup.exists() || backup.is_symlink() {
        return Err(refusal(
            "this cutover's undo archive already exists; inspect its Procedure".into(),
        ));
    }
    let mut plan = Plan::new()
        .with_note(format!(
            "cut over {} to AIKit projection {}; Control remains authored ground; undo archive {}",
            source.display(),
            projection.display(),
            backup.display()
        ))
        .with_edit(WorldEdit::MovePath {
            from: source.clone(),
            to: backup,
        })
        .with_edit(WorldEdit::CreateLink {
            path: source.clone(),
            target: projection.clone(),
            inverse: Inverse::Remove,
        });
    plan = aikit_store::procedure::bind_current_preconditions(plan)?;
    for dependency in [ground, current.as_path(), generation.as_path()] {
        plan = aikit_store::procedure::bind_read_precondition(plan, dependency)?;
    }
    let review_digest = plan.review_digest(&[format!(
        "accepted Control adoption {}",
        staged.review_digest
    )]);
    let isolation = select_isolation(
        &plan,
        &home.state().join("procedures/.shadow"),
        aikit_store::procedure::git_repo_of,
    );
    let procedure = Procedure::new(
        ProcedureKind::Adopt {
            source: source.clone(),
            namespace: staged.namespace.clone(),
            capsules: staged.capsules.clone(),
        },
        plan,
        isolation,
    )?;
    Ok(Adoption {
        procedure,
        review_digest,
        source: source.clone(),
        namespace: staged.namespace,
        capsules: staged.capsules,
    })
}

fn native_projection(home: &AikitHome, projection: &Path) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let refusal = |message: String| AikitError::new("adopt.cutover_refused", message);
    let contexts = std::fs::canonicalize(home.contexts()).map_err(|e| refusal(e.to_string()))?;
    // Canonicalise the context owner, retaining the stable current component.
    // Accept only native skill projections, never an arbitrary directory link.
    let parts: Vec<_> = projection.components().collect();
    if parts.iter().any(|p| matches!(p, Component::ParentDir)) {
        return Err(refusal(
            "projection must name a native current-generation skill tree".into(),
        ));
    }
    let current_index = parts
        .iter()
        .rposition(|p| p.as_os_str() == "current")
        .ok_or_else(|| refusal("projection must follow an AIKit current generation".into()))?;
    let context_path: PathBuf = parts[..current_index].iter().collect();
    let context = std::fs::canonicalize(context_path).map_err(|e| refusal(e.to_string()))?;
    if context.parent() != Some(contexts.as_path()) {
        return Err(refusal(
            "projection belongs to a different AIKit home".into(),
        ));
    }
    let suffix: PathBuf = parts[current_index + 1..].iter().collect();
    if ![
        Path::new("projections/codex/.agents/skills"),
        Path::new("projections/claude/.claude/skills"),
    ]
    .contains(&suffix.as_path())
    {
        return Err(refusal(
            "projection must name a native Claude or Codex skill tree".into(),
        ));
    }
    let current = context.join("current");
    let generation = std::fs::canonicalize(&current).map_err(|e| refusal(e.to_string()))?;
    let generations =
        std::fs::canonicalize(context.join("generations")).map_err(|e| refusal(e.to_string()))?;
    if generation.parent() != Some(generations.as_path())
        || !aikit_store::generation::is_generation(&generation)
    {
        return Err(refusal(
            "current does not identify a committed AIKit generation".into(),
        ));
    }
    let projection = current.join(suffix);
    Ok((projection, current, generation))
}

/// Reconcile a mixed harness directory with a native generation. Authored
/// payloads must already survive in the generation; unrelated harness material
/// stays in place. Broken historical links are preserved in the undo archive
/// and replaced only when their named skill is actually generated.
pub fn plan_projection_cutover(
    home: &AikitHome,
    source: &Path,
    projection: &Path,
    namespace: Option<&str>,
) -> Result<Adoption> {
    let refusal = |message: String| AikitError::new("adopt.cutover_refused", message);
    let source = std::fs::canonicalize(source).map_err(|e| refusal(e.to_string()))?;
    let (projection, current, generation) = native_projection(home, projection)?;
    if source.starts_with(&projection) || projection.starts_with(&source) {
        return Err(refusal("source and projection must be disjoint".into()));
    }
    let namespace = namespace.unwrap_or("projection").to_owned();
    valid_slug(&namespace, "namespace")?;
    let mut generated = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(&projection).map_err(|e| refusal(e.to_string()))? {
        let path = entry.map_err(|e| refusal(e.to_string()))?.path();
        if path.join("SKILL.md").is_file() {
            agent_skills::validate(&path)?;
            generated.insert(path.file_name().unwrap().to_os_string(), path);
        }
    }
    if generated.is_empty() {
        return Err(refusal("generation contains no skill payloads".into()));
    }
    // Unselected entries remain untouched, including harness-owned skills.
    // Only entries actually supplied by this generation participate in cutover.
    let mut plan = Plan::new();
    let mut capsules = Vec::new();
    // Stable archive identity is content-bound below through preconditions;
    // include the source path to prevent two harness roots sharing an archive.
    let archive_key = blake3::hash(source.to_string_lossy().as_bytes())
        .to_hex()
        .to_string();
    let archive = home.state().join("adoption-undo").join(archive_key);
    for (name, target) in generated {
        let path = source.join(&name);
        if path.is_symlink() && std::fs::read_link(&path).ok().as_ref() == Some(&target) {
            continue;
        }
        if path.exists() && !path.join("SKILL.md").is_file() {
            return Err(refusal(format!(
                "{} is harness material, not a skill",
                path.display()
            )));
        }
        if path.join("SKILL.md").is_file() {
            let original = agent_skills::validate(&path)?;
            let replacement = agent_skills::validate(&target)?;
            let payload = |files: Vec<String>| {
                files
                    .into_iter()
                    .filter(|p| p != "skill.json")
                    .collect::<BTreeSet<_>>()
            };
            let files = payload(original.files);
            if files != payload(replacement.files) {
                return Err(refusal(format!(
                    "payload file set differs for {}",
                    path.display()
                )));
            }
            for relative in files {
                let old = path.join(&relative);
                let new = target.join(&relative);
                if std::fs::read(&old).map_err(|e| refusal(e.to_string()))?
                    != std::fs::read(&new).map_err(|e| refusal(e.to_string()))?
                    || source_mode(&old)? != source_mode(&new)?
                {
                    return Err(refusal(format!(
                        "payload bytes or mode differ for {}",
                        old.display()
                    )));
                }
            }
            if path.is_symlink() {
                plan = aikit_store::procedure::bind_read_precondition(
                    plan,
                    &std::fs::canonicalize(&path).map_err(|e| refusal(e.to_string()))?,
                )?;
            }
        }
        if path.exists() || path.is_symlink() {
            let backup = archive.join(&name);
            if backup.exists() || backup.is_symlink() {
                return Err(refusal(format!(
                    "undo archive already exists: {}",
                    backup.display()
                )));
            }
            plan = plan.with_edit(WorldEdit::MovePath {
                from: path.clone(),
                to: backup,
            });
        }
        plan = plan.with_edit(WorldEdit::CreateLink {
            path,
            target,
            inverse: Inverse::Remove,
        });
        capsules.push(CapsuleId::parse(&format!(
            "skill/{namespace}/{}",
            name.to_string_lossy()
        ))?);
    }
    plan = plan.with_note("Publish native generation skill entries; preserve harness-owned material and reversible originals.");
    plan = aikit_store::procedure::bind_current_preconditions(plan)?;
    for dependency in [&source, &current, &generation] {
        plan = aikit_store::procedure::bind_read_precondition(plan, dependency)?;
    }
    let review_digest = plan.review_digest(&[format!(
        "native generation projection: {}",
        projection.display()
    )]);
    let isolation = select_isolation(
        &plan,
        &home.state().join("procedures/.shadow"),
        aikit_store::procedure::git_repo_of,
    );
    let procedure = Procedure::new(
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
