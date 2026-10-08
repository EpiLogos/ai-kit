//! Task admission for the existing encounter owner. Central owns the clearing;
//! Workcell confines the existing protocol child; this module owns neither.
use super::{error, native_admission, native_admission_before, read_binding};
use crate::encounter_service::{EncounterProtocol, EncounterProvider, EncounterService};
use aikit_adapters::central_placement::{
    AllocatedCentralTask, CentralTaskRequest, NativeCentralPlacement,
};
use aikit_adapters::runner::{CommandRunner, Output, SystemRunner};
use aikit_core::{AikitError, ResourceRef, Result, SourceRevision};
use aikit_store::{AikitHome, ContextLock, LockOptions};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[path = "encounter_task_material.rs"]
mod material;
#[path = "encounter_task_run.rs"]
mod prepared_run;
use material::{MaterialBinding, MaterialHost};

const WRITE_ACTION: &str = "action/aikit/encounter-task";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskRequest {
    central: CentralTaskRequest,
    provider: EncounterProvider,
    cwd: PathBuf,
    selected_directories: Vec<PathBuf>,
    /// Explicit caller-owned exclusions narrow the native grant without
    /// changing Central's authored policy or the legacy request shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    additional_protected_directories: Vec<PathBuf>,
    #[serde(default)]
    workcell_boundary_bin: PathBuf,
    #[serde(default)]
    prepared_run_scope: Option<prepared_run::Request>,
    authority_ref: ResourceRef,
    /// A hosted arrangement must prepare/observe this native owner; omission
    /// keeps the explicitly unhosted protected-process mode, not fake hosting.
    #[serde(default)]
    material_host: Option<MaterialHost>,
    /// Exact child-specific A application retained by this task owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repertoire: Option<TaskRepertoire>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskRepertoire {
    reading: aikit_core::repertoire::RepertoireReading,
    procedure: aikit_core::ProcedureId,
}

/// Pi's ambient project/global discovery is independent of AIKit context IDs.
/// Pin explicit skills to the verified immutable application, never `current`.
fn selected_pi_skill_argv(argv: &mut Vec<String>, immutable_skills: Option<&Path>) -> Result<()> {
    if argv
        .iter()
        .any(|arg| arg == "--skill" || arg.starts_with("--skill="))
    {
        return Err(error(
            "Task Pi skill arguments conflict with the exact selected repertoire",
        ));
    }
    if !argv.iter().any(|arg| arg == "--no-skills" || arg == "-ns") {
        argv.push("--no-skills".into());
    }
    if let Some(path) = immutable_skills {
        let metadata = fs::symlink_metadata(path).map_err(error)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(error(
                "Selected immutable Pi skill root must be an actual generation directory",
            ));
        }
        argv.push("--skill".into());
        argv.push(
            path.to_str()
                .ok_or_else(|| error("Pi skill path is not UTF-8"))?
                .into(),
        );
    }
    Ok(())
}
fn pin_task_pi_repertoire(
    home: &AikitHome,
    session: &ResourceRef,
    request: &TaskRequest,
    argv: &mut Vec<String>,
) -> Result<()> {
    let Some(selected) = &request.repertoire else {
        return Ok(());
    };
    let generation = selected
        .reading
        .generation
        .as_ref()
        .ok_or_else(|| error("Selected task repertoire has no actual generation"))?;
    let skills = home
        .context_dir(&task_context_id(session))
        .join("generations")
        .join(generation.as_str())
        .join("projections/pi/.pi/skills");
    let carries_skills = selected
        .reading
        .members
        .iter()
        .any(|member| member.projected && member.practice == "Skill");
    if carries_skills && !skills.is_dir() {
        return Err(error(
            "Selected Pi generation skills are missing; reconcile the native application",
        ));
    }
    selected_pi_skill_argv(argv, skills.is_dir().then_some(skills.as_path()))
}

fn task_context_id(session: &ResourceRef) -> aikit_core::ContextId {
    aikit_core::ContextId::parse(&format!(
        "ctx_{}",
        &blake3::hash(session.as_str().as_bytes()).to_hex()[..24]
    ))
    .expect("canonical derived ContextId")
}
fn task_repertoire_env(session: &ResourceRef, key: &str) -> Option<String> {
    match key {
        "AIKIT_SESSION_ID" => Some(task_session_id(session).to_string()),
        "AIKIT_CONTEXT_ID" => Some(task_context_id(session).to_string()),
        "AIKIT_TASK" | "AIKIT_PROJECT_ID" | "AIKIT_VIEW" | "AIKIT_CONTEXT_ROOT" => None,
        _ => std::env::var(key).ok(),
    }
}
fn validate_task_repertoire(
    home: &AikitHome,
    session: &ResourceRef,
    request: &TaskRequest,
) -> Result<()> {
    let Some(selected) = &request.repertoire else {
        return Ok(());
    };
    if selected.reading.context_id != task_context_id(session).as_str()
        || selected.reading.generation.is_none()
    {
        return Err(error(
            "Task repertoire must be an actual application in this exact child runtime context",
        ));
    }
    let service = crate::app::Service::open(home.clone(), &request.cwd, |key| {
        task_repertoire_env(session, key)
    })?;
    service.verify_repertoire_procedure(&selected.reading, &selected.procedure)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskRecord {
    schema: String,
    revision: SourceRevision,
    request: TaskRequest,
    agency_revision: SourceRevision,
    ready: bool,
    allocation: Option<AllocatedCentralTask>,
    requirements: Option<Value>,
    inspection: Option<Value>,
    cwd_anchor: Option<Value>,
    launcher: EncounterProvider,
    #[serde(default)]
    material: Option<MaterialBinding>,
    #[serde(default)]
    prepared_run: Option<prepared_run::Binding>,
    #[serde(default)]
    cleanup: Option<Value>,
}
/// Finite native-owner requests, not protocol/session lifetime. Partial effects
/// stay uncertain on timeout; the durable task key is never replaced for retry.
struct OwnerRunner;
impl OwnerRunner {
    fn run_before(&self, argv: &[String], deadline: Option<std::time::Instant>) -> Result<Output> {
        let timeout = match deadline {
            Some(deadline) => deadline
                .checked_duration_since(std::time::Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| {
                    error("Native task readback exhausted the existing startup deadline")
                })?
                .min(Duration::from_secs(15)),
            None => Duration::from_secs(15),
        };
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| error("Missing native operation"))?;
        let mut command = Command::new(program);
        command.args(args);
        SystemRunner::new()
            .with_timeout(timeout)
            .with_output_limit_bytes(4 * 1024 * 1024)
            .with_strict_utf8()
            .capture_command(&mut command)
            .map_err(|cause| {
                // Keep the public encounter domain while forwarding the actual
                // capture, effect and lifecycle basis, including its IO cause.
                let mut failure = error(cause.message())
                    .with_io_source_from(&cause)
                    .with("native_runner_code", cause.code());
                for (key, value) in cause.details() {
                    failure = failure.with(key.clone(), value.clone());
                }
                failure
                    .with("automatic_retry", "false")
                    .with("recovery", "Recover the same durable task request explicitly; do not replace or replay its execution intent")
            })
    }
}
impl CommandRunner for OwnerRunner {
    fn run(&self, argv: &[String]) -> Result<Output> {
        self.run_before(argv, None)
    }
}
#[derive(Clone, Copy)]
struct DeadlineOwnerRunner(Option<std::time::Instant>);
impl CommandRunner for DeadlineOwnerRunner {
    fn run(&self, argv: &[String]) -> Result<Output> {
        OwnerRunner.run_before(argv, self.0)
    }
}

fn path(home: &AikitHome, session: &ResourceRef) -> PathBuf {
    home.state().join("encounter-tasks").join(format!(
        "{}.json",
        blake3::hash(session.as_str().as_bytes()).to_hex()
    ))
}
fn read(home: &AikitHome, session: &ResourceRef) -> Result<Option<TaskRecord>> {
    let path = path(home, session);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
        Ok(meta) if !meta.is_file() || meta.len() > 4 * 1024 * 1024 => {
            return Err(error("Task binding must be a bounded native file"))
        }
        Ok(_) => {}
    }
    if path.canonicalize().map_err(error)? != path {
        return Err(error("Redirected task binding"));
    }
    serde_json::from_slice(&fs::read(path).map_err(error)?)
        .map(Some)
        .map_err(error)
}
fn historical_ready(
    home: &AikitHome,
    session: &ResourceRef,
    revision: &SourceRevision,
) -> Result<TaskRecord> {
    let history = path(home, session)
        .parent()
        .expect("task parent")
        .join("history");
    if history.canonicalize().map_err(error)? != history {
        return Err(error("Redirected task history"));
    }
    let mut found = None;
    for (index, entry) in fs::read_dir(history).map_err(error)?.enumerate() {
        if index >= 512 {
            return Err(error("Task history exceeds bounded recovery search"));
        }
        let entry = entry.map_err(error)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(error)?;
        if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
            return Err(error("Task history entry must be a bounded native file"));
        }
        let bytes = fs::read(entry.path()).map_err(error)?;
        if entry.file_name().to_string_lossy() != format!("{}.json", blake3::hash(&bytes).to_hex())
        {
            return Err(error("Task history digest mismatch"));
        }
        let record: TaskRecord = serde_json::from_slice(&bytes).map_err(error)?;
        // One successful configure journals pending and ready with the same
        // revision. Only the actual ready reading can be a restore target;
        // two ready readings for one revision remain an ambiguity refusal.
        if record.ready && &record.revision == revision && found.replace(record).is_some() {
            return Err(error("Ambiguous task history revision"));
        }
    }
    found.ok_or_else(|| error("Requested ready task revision is absent from native history"))
}
fn launcher_for(
    session: &ResourceRef,
    provider: &EncounterProvider,
    revision: &SourceRevision,
) -> Result<EncounterProvider> {
    let resolved_body = crate::encounter_profile_provider::resolve_provider(provider.clone())?;
    let mut launcher = provider.clone();
    launcher.protocol = resolved_body.protocol;
    launcher.id = format!(
        "task-{}",
        blake3::hash(session.as_str().as_bytes()).to_hex()
    );
    let mut argv = vec![std::env::current_exe()
        .map_err(error)?
        .display()
        .to_string()];
    if let Some(prefix) = crate::session_space_verb_prefix() {
        argv.push(prefix.to_owned());
    }
    argv.push("encounter-task-exec".into());
    argv.extend([
        "--agent-session".into(),
        session.to_string(),
        "--expected-revision".into(),
        revision.to_string(),
    ]);
    launcher.argv = argv;
    // The outer launcher is a single Workcell boundary command. The original
    // profile and its variants remain in request.provider for final exec;
    // carrying them on this wrapper would either contradict from_profile or
    // allow a fallback around the boundary.
    launcher.from_profile = None;
    launcher.argv_fallback.clear();
    Ok(launcher)
}
fn launcher_belongs_to(session: &ResourceRef, record: &TaskRecord) -> bool {
    let Ok(resolved_body) =
        crate::encounter_profile_provider::resolve_provider(record.request.provider.clone())
    else {
        return false;
    };
    let mut expected = record.request.provider.clone();
    expected.protocol = resolved_body.protocol;
    expected.id = format!(
        "task-{}",
        blake3::hash(session.as_str().as_bytes()).to_hex()
    );
    expected.argv = record.launcher.argv.clone();
    expected.from_profile = None;
    expected.argv_fallback.clear();
    let suffix = [
        "encounter-task-exec",
        "--agent-session",
        session.as_str(),
        "--expected-revision",
        record.revision.as_str(),
    ];
    serde_json::to_value(&record.launcher)
        .ok()
        .zip(serde_json::to_value(&expected).ok())
        .is_some_and(|(actual, expected)| actual == expected)
        && record.launcher.argv.len() > suffix.len()
        && PathBuf::from(&record.launcher.argv[0]).is_absolute()
        && record.launcher.argv[record.launcher.argv.len() - suffix.len()..]
            .iter()
            .map(String::as_str)
            .eq(suffix)
}
fn publish(home: &AikitHome, session: &ResourceRef, record: &TaskRecord) -> Result<()> {
    let target = path(home, session);
    let parent = target.parent().expect("task parent");
    fs::create_dir_all(parent).map_err(error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(error)?;
    }
    // Keep every replaced pending/ready reading. A new body/configuration must
    // not erase the historical material, NOW/source and return correlations.
    if target.exists() {
        let previous = fs::read(&target).map_err(error)?;
        let history = parent.join("history");
        fs::create_dir_all(&history).map_err(error)?;
        let entry = history.join(format!("{}.json", blake3::hash(&previous).to_hex()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&entry)
        {
            Ok(mut file) => {
                file.write_all(&previous).map_err(error)?;
                file.sync_all().map_err(error)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(&entry).map_err(error)? != previous {
                    return Err(error("Task history conflict"));
                }
            }
            Err(e) => return Err(error(e)),
        }
        fs::File::open(&history)
            .and_then(|f| f.sync_all())
            .map_err(error)?;
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    staged
        .write_all(&serde_json::to_vec_pretty(record).map_err(error)?)
        .map_err(error)?;
    staged.as_file().sync_all().map_err(error)?;
    staged.persist(&target).map_err(error)?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(error)
}
fn authority(
    home: &AikitHome,
    session: &ResourceRef,
    request: &TaskRequest,
) -> Result<SourceRevision> {
    authority_before(home, session, request, None)
}
fn authority_before(
    home: &AikitHome,
    session: &ResourceRef,
    request: &TaskRequest,
    deadline: Option<std::time::Instant>,
) -> Result<SourceRevision> {
    let binding = read_binding(home, session)?
        .ok_or_else(|| error("Task needs an actual selected Agency, not a profile"))?;
    let admitted = match deadline {
        Some(deadline) => native_admission_before(&binding, deadline)?,
        None => native_admission(&binding)?,
    };
    if !admitted.authorises(&ResourceRef::parse(WRITE_ACTION)?)
        || !admitted.receipt["determination"]["authority_refs"]
            .as_array()
            .is_some_and(|refs| refs.contains(&json!(request.authority_ref)))
        || !request
            .central
            .participant_refs
            .contains(&admitted.agent_ref)
    {
        return Err(error(
            "Current Agency does not authorise this task, authority or participant",
        ));
    }
    Ok(binding.revision)
}
/// The request is caller source, including historical requests whose boundary
/// path was normalized by an older owner. A prepared Run separately owns the
/// effective executable; selecting it must never rewrite that source or make
/// a changed pending request look equivalent to the retained one.
fn boundary_executable(record: &TaskRecord) -> Result<&Path> {
    match (&record.request.prepared_run_scope, &record.prepared_run) {
        (Some(_), Some(run)) => Ok(&run.boundary_executable),
        (None, None) => Ok(&record.request.workcell_boundary_bin),
        _ => Err(error(
            "The prepared run binding is missing; no material fallback",
        )),
    }
}
/// A native owner that exits non-zero must be refused as itself: its actual
/// bounded diagnostics travel with the structured refusal. An exited owner's
/// empty reply is never reparsed into an unrelated JSON parse error.
fn owner_refusal(operation: &str, output: &Output) -> AikitError {
    let diagnostics = [output.stderr.trim(), output.stdout.trim()]
        .into_iter()
        .find(|text| !text.is_empty())
        .map(|text| {
            let mut bounded: String = text.chars().take(1024).collect();
            if text.chars().count() > 1024 {
                bounded.push('…');
            }
            bounded
        })
        .unwrap_or_else(|| "no diagnostics were emitted".to_string());
    error(format!(
        "Native owner {operation} refused with exit status {}: {diagnostics}",
        output.status
    ))
}
fn inspect(boundary: &Path, requirements: &Value) -> Result<Value> {
    inspect_with_runner(boundary, requirements, &OwnerRunner)
}
fn inspect_with_runner(
    boundary: &Path,
    requirements: &Value,
    runner: &dyn CommandRunner,
) -> Result<Value> {
    if !boundary.is_absolute() {
        return Err(error("Explicit Workcell executable required"));
    }
    let file = tempfile::NamedTempFile::new().map_err(error)?;
    fs::write(file.path(), requirements.to_string()).map_err(error)?;
    let output = runner.run(&[
        boundary.display().to_string(),
        "inspect".into(),
        file.path().display().to_string(),
        requirements["policy_revision"]
            .as_str()
            .ok_or_else(|| error("Missing policy revision"))?
            .into(),
    ])?;
    if !output.ok() {
        return Err(owner_refusal("write-boundary inspect", &output));
    }
    let value: Value = serde_json::from_str(&output.stdout).map_err(error)?;
    if value["schema"] != "workcell.prepared-write-boundary/v1"
        || value["requirements"] != *requirements
        || value["state"] != "prepared-not-executed"
        || value["requirements_digest"]
            .as_str()
            .is_none_or(str::is_empty)
    {
        return Err(error(
            "Workcell cannot prepare the exact required protection; no weaker fallback",
        ));
    }
    Ok(value)
}
struct TaskCodexRuntime {
    npm_cache: PathBuf,
    sqlite_home: PathBuf,
    projection: Value,
}

/// The embedded Codex ACP connection uses npx, which writes package/runtime
/// cache before the protocol opens. Keep those writes in the actual allocated
/// Task T; neither ambient npm configuration nor another directory is a grant.
fn task_codex_runtime(
    record: &TaskRecord,
    body: &EncounterProvider,
    argv: &[String],
) -> Result<Option<TaskCodexRuntime>> {
    if body.from_profile.as_deref() != Some("codex")
        || body.protocol != EncounterProtocol::Acp
        || argv
            .first()
            .and_then(|program| std::path::Path::new(program).file_name())
            != Some(std::ffi::OsStr::new("npx"))
    {
        return Ok(None);
    }
    let now = record
        .allocation
        .as_ref()
        .ok_or_else(|| error("Codex runtime cache needs the actual native Task allocation"))?
        .now_directory()?;
    let requirements = record
        .requirements
        .as_ref()
        .ok_or_else(|| error("Codex runtime cache needs the actual Task write boundary"))?;
    let inspection = record
        .inspection
        .as_ref()
        .ok_or_else(|| error("Codex runtime cache needs native protection inspection"))?;
    if !now.is_absolute()
        || !requirements["writable_paths"]
            .as_array()
            .is_some_and(|paths| paths.contains(&json!(now)))
        || inspection["requirements"] != *requirements
        || inspection["capabilities"]["supported"] != true
        || !inspection["capabilities"]["coverage"]
            .as_array()
            .is_some_and(|coverage| {
                [
                    "file-content",
                    "file-creation",
                    "file-removal",
                    "rename-link",
                    "truncate",
                ]
                .iter()
                .all(|required| coverage.contains(&json!(required)))
            })
    {
        return Err(error(
            "Codex runtime cache requires the exact protected Task T write aperture",
        ));
    }
    let runtime = now.join("runtime");
    let cache = runtime.join("npm-cache");
    let sqlite_home = runtime.join("codex-sqlite");
    for directory in [&now, &runtime, &cache, &sqlite_home] {
        match fs::symlink_metadata(directory) {
            Ok(metadata)
                if metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && directory
                        .canonicalize()
                        .map_err(|failure| error(&failure).with_io_source(failure))?
                        == *directory => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && directory != &now => {}
            Err(failure) => return Err(error(&failure).with_io_source(failure)),
            _ => {
                return Err(error(
                    "Codex runtime cache ancestors must be real canonical directories",
                ))
            }
        }
    }
    // This is the same original native home route delivered by ModelEnvironment,
    // not provider.env, another credential home, or a Session identity. Native
    // Codex canonicalizes nonempty CODEX_HOME. Missing/default input refuses
    // here rather than granting creation in the ambient home.
    let supplied_home = std::env::var("CODEX_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
        .ok_or_else(|| error("Codex Task runtime needs the original native home input"))?;
    // Qualify only once, preserving the exact lexical route supplied to
    // the unchanged provider environment. The actual native owner checks
    // this route against the admitted canonical held origin before body.
    let requested_input_root = if supplied_home.is_absolute() {
        supplied_home
    } else {
        std::env::current_dir()
            .map_err(|failure| error(&failure).with_io_source(failure))?
            .join(supplied_home)
    };
    let input_root = fs::canonicalize(&requested_input_root)
        .map_err(|failure| error(&failure).with_io_source(failure))?;
    if !fs::symlink_metadata(&input_root)
        .map_err(|failure| error(&failure).with_io_source(failure))?
        .is_dir()
        || requested_input_root.to_str().is_none()
        || input_root.to_str().is_none()
        || now.to_str().is_none()
    {
        return Err(error(
            "Codex Task runtime needs an existing representable native input directory",
        ));
    }
    let projection = json!({"schema":"workcell.runtime-projection/v1",
        "requested_input_root":requested_input_root,"input_root":input_root,"runtime_root":now.join("native-codex-runtime"),
        "immutable_members":["auth.json",".credentials.json","config.toml","config.d","managed_config.toml","hooks.json"],
        "mutable_directories":["tmp","log","sessions","archived_sessions","shell_snapshots","thread-writer-locks"],
        "mutable_files":["installation_id","history.jsonl","models_cache.json","session_index.jsonl"],
        "boundary_digest":inspection["requirements_digest"]});
    // npm and native Codex create absent runtime directories only after
    // Workcell applies the object-bound Task aperture. HOME/CODEX_HOME remain
    // the original auth/config origin; SQLite placement is not a new Session.
    Ok(Some(TaskCodexRuntime {
        npm_cache: cache,
        sqlite_home,
        projection,
    }))
}
fn validate(home: &AikitHome, session: &ResourceRef, record: &TaskRecord) -> Result<()> {
    validate_before(home, session, record, None)
}
fn validate_before(
    home: &AikitHome,
    session: &ResourceRef,
    record: &TaskRecord,
    deadline: Option<std::time::Instant>,
) -> Result<()> {
    let runner = DeadlineOwnerRunner(deadline);
    validate_task_repertoire(home, session, &record.request)?;
    if record.schema != "aikit.encounter-task/v1" || !record.ready {
        return Err(error(
            "Task preparation is incomplete; explicitly recover the same request",
        ));
    }
    if !launcher_belongs_to(session, record) {
        return Err(error(
            "Prepared task launcher no longer corresponds to its exact source body",
        ));
    }
    crate::encounter_profile_provider::ensure_connection_facts_reachable(&record.request.provider)?;
    if authority_before(home, session, &record.request, deadline)? != record.agency_revision {
        return Err(error(
            "Task Agency changed; explicitly re-resolve before further work",
        ));
    }
    let task = record
        .allocation
        .as_ref()
        .ok_or_else(|| error("Missing native NOW"))?;
    let owner = NativeCentralPlacement::new(runner);
    owner.revalidate(task)?;
    let cwd_anchor = owner.working_directory_anchor(task, &record.request.cwd)?;
    if Some(&cwd_anchor) != record.cwd_anchor.as_ref() {
        return Err(error("Task working directory changed since preparation or retains a legacy write-destination anchor; explicitly prepare the same task request again"));
    }
    let requirements = owner.write_boundary_requirements_with_additional_protection(
        task,
        &record.request.authority_ref,
        &record.request.selected_directories,
        &record.request.additional_protected_directories,
    )?;
    if Some(&requirements) != record.requirements.as_ref() {
        return Err(error("Material requirements changed; no automatic renewal"));
    }
    let fresh = inspect_with_runner(boundary_executable(record)?, &requirements, &runner)?;
    // Includes every native path/type/inode and the exact material requirement digest.
    if record.inspection.as_ref().is_none_or(|old| {
        old["requirements_digest"] != fresh["requirements_digest"]
            || old["writable_objects"] != fresh["writable_objects"]
            || old["protected_objects"] != fresh["protected_objects"]
            || old["objects"] != fresh["objects"]
    }) {
        return Err(error(
            "Material path identity changed; prepare an explicit new binding",
        ));
    }
    match (&record.request.prepared_run_scope, &record.prepared_run) {
        (Some(request), Some(run)) => {
            if run.scope["prepared_write_boundary"] != fresh {
                return Err(error("Prepared run execution boundary changed"));
            }
            run.revalidate_with_runner(request, &runner)?;
        }
        (None, None) => {}
        _ => {
            return Err(error(
                "The prepared run binding is missing; no material fallback",
            ))
        }
    }
    match (&record.request.material_host, &record.material) {
        (Some(host), Some(binding)) if host == &binding.host => {
            binding.validate_with_runner(task, &runner)?;
        }
        (None, None) => {}
        _ => {
            return Err(error(
                "Required native material binding is missing or replaced; no unhosted fallback",
            ))
        }
    }
    Ok(())
}

pub(super) fn check(home: &AikitHome, session: &ResourceRef) -> Result<()> {
    if let Some(record) = read(home, session)? {
        validate(home, session, &record)?;
    }
    Ok(())
}
pub(super) fn check_before(
    home: &AikitHome,
    session: &ResourceRef,
    deadline: std::time::Instant,
) -> Result<()> {
    crate::encounter_service::ensure_native_startup_deadline(deadline)?;
    if let Some(record) = read(home, session)? {
        validate_before(home, session, &record, Some(deadline))?;
    }
    crate::encounter_service::ensure_native_startup_deadline(deadline)
}

/// Called at the existing prompt boundary for human and addressed turns alike.
/// A new task configuration cannot bless an older, unconfined resident process.
pub(super) fn prompt(service: &EncounterService, session: &ResourceRef) -> Result<String> {
    let Some(record) = read(&service.home, session)? else {
        if service
            .resident(session)?
            .argv
            .iter()
            .any(|arg| arg == "encounter-task-exec")
        {
            return Err(error(
                "Task binding removed from an already task-bound resident; no fallback",
            ));
        }
        return Ok(String::new());
    };
    validate(&service.home, session, &record)?;
    if let Some(material) = &record.material {
        material.check_encounter_owner()?;
    }
    let resident = service.resident(session)?;
    if resident.cwd != record.request.cwd
        || resident.argv != record.launcher.argv
        || resident.provider != record.launcher.id
    {
        return Err(error("This resident was not launched for the current task boundary; explicit new body/continuation required"));
    }
    let task = record.allocation.as_ref().expect("validated allocation");
    Ok(format!("\nTask: {}\nTask NOW: {}\nTask output directory: {}\nWorking directory: {}\nPolicy revision: {}\n", task.request.task_ref, task.allocation["now_ref"], task.now_directory()?.display(), record.request.cwd.display(), task.allocation["policy"]["revision"]))
}
/// Complete a previously journalled pending task under the caller's session
/// lock. Recovery uses the same native owners, but must retain the historical
/// Central identity and protection basis while obtaining a fresh finite lease.
fn prepare_published(
    home: &AikitHome,
    session: &ResourceRef,
    mut record: TaskRecord,
    restore: Option<&TaskRecord>,
) -> Result<TaskRecord> {
    let owner = NativeCentralPlacement::new(OwnerRunner);
    let task = owner.allocate(&record.request.central)?;
    // Retain allocation before subsequent effects so refusal and cleanup
    // cannot erase the exact native resource identity.
    record.allocation = Some(task.clone());
    publish(home, session, &record)?;
    let prepared: Result<Value> = (|| {
        if let Some(previous) = restore {
            let old = previous
                .allocation
                .as_ref()
                .ok_or_else(|| error("Recovery target lacks native Central allocation"))?;
            if task.allocation["policy"]["revision"] != old.allocation["policy"]["revision"]
                || task.allocation["now_ref"] != old.allocation["now_ref"]
                || task.allocation["source"]["ref"] != old.allocation["source"]["ref"]
                || task.allocation["record"]["task_ref"] != old.allocation["record"]["task_ref"]
            {
                return Err(error(
                "Native Central task or placement policy changed; recovery cannot widen the historical allocation",
            ));
            }
        }
        let binding = read_binding(home, session)?.ok_or_else(|| error("Agency disappeared"))?;
        if task.allocation["policy"]["scope_ref"] != json!(binding.world_ref) {
            return Err(error("Central task scope differs from the native Agency World; an explicit owner-backed relation is required"));
        }
        record.cwd_anchor = Some(owner.working_directory_anchor(&task, &record.request.cwd)?);
        let requirements = owner.write_boundary_requirements_with_additional_protection(
            &task,
            &record.request.authority_ref,
            &record.request.selected_directories,
            &record.request.additional_protected_directories,
        )?;
        if let Some(previous) = restore {
            let old = previous
                .requirements
                .as_ref()
                .ok_or_else(|| error("Recovery target lacks native material requirements"))?;
            if ["writable_paths", "protected_paths", "required_coverage"]
                .iter()
                .any(|key| requirements[*key] != old[*key])
            {
                return Err(error(
                "Native material boundary changed; recovery cannot widen the historical protection",
            ));
            }
        }
        let boundary = boundary_executable(&record)?.to_path_buf();
        record.inspection = Some(
            if let (Some(run), Some(request)) =
                (&mut record.prepared_run, &record.request.prepared_run_scope)
            {
                let scope=run.prepare(request,&record.request.cwd,&requirements,&json!({"agency_ref":binding.agency_ref,"source":binding.agency_source.path,"revision":binding.agency_source.revision,"digest":binding.agency_source.content_digest}))?;
                let current = inspect(&boundary, &requirements)?;
                if scope != current {
                    return Err(error(
                        "Prepared run boundary differs from native executable inspection",
                    ));
                }
                scope
            } else {
                inspect(&boundary, &requirements)?
            },
        );
        if let Some(host) = &record.request.material_host {
            record.material = Some(host.prepare(
                &task,
                json!({
                    "agent":binding.agent_ref, "agency":binding.agency_ref,
                    "world_binding":binding.world_binding_ref, "world":binding.world_ref,
                    "agent_session":session, "task":task.request.task_ref,
                    "now":task.allocation["now_ref"], "source":task.allocation["source"]["ref"],
                    "now_revision":task.allocation["revision"]["revision"],
                    "policy_revision":task.allocation["policy"]["revision"],
                    "authority":record.request.authority_ref
                }),
            )?);
        }
        Ok(requirements)
    })();
    let requirements = match prepared {
        Ok(requirements) => requirements,
        Err(reason) => {
            record.cleanup = Some(
                match if record.request.prepared_run_scope.is_some() {
                    owner.close_new_allocation(&task)
                } else {
                    Ok(None)
                } {
                    Ok(receipt) => json!({"state":"confirmed","receipt":receipt}),
                    Err(cleanup) => json!({"state":"unconfirmed","reason":cleanup.to_string()}),
                },
            );
            publish(home, session, &record)?;
            return Err(reason.with(
                "cleanup",
                record.cleanup.as_ref().expect("cleanup").to_string(),
            ));
        }
    };
    record.allocation = Some(task);
    record.requirements = Some(requirements);
    // Configuration is independent from the resident. Changing this does
    // not pretend to retrofit an existing process with Landlock.
    let finish: Result<()> = (|| {
        EncounterService::configure(home, record.launcher.clone())?;
        record.ready = true;
        validate(home, session, &record)
    })();
    if let Err(reason) = finish {
        record.ready = false;
        if record.request.prepared_run_scope.is_some() {
            record.cleanup = Some(
                match owner
                    .close_new_allocation(record.allocation.as_ref().expect("allocated task"))
                {
                    Ok(receipt) => json!({"state":"confirmed","receipt":receipt}),
                    Err(cleanup) => json!({"state":"unconfirmed","reason":cleanup.to_string()}),
                },
            );
        }
        publish(home, session, &record)?;
        return Err(reason.with(
            "cleanup",
            record.cleanup.as_ref().unwrap_or(&Value::Null).to_string(),
        ));
    }
    publish(home, session, &record)?;
    Ok(record)
}

/// Child runtime scope derives from the actual retained AgentSession, not an
/// inherited parent overlay or a fabricated Position tenure.
fn task_session_id(session: &ResourceRef) -> aikit_core::SessionId {
    aikit_core::SessionId::parse(&format!(
        "ses_{}",
        &blake3::hash(session.as_str().as_bytes()).to_hex()[..24]
    ))
    .expect("canonical derived SessionId")
}
fn isolate_task_identity(
    command: &mut Command,
    session: &ResourceRef,
    context: Option<&aikit_core::ContextId>,
) {
    for name in [
        "OI_POSITION_REF",
        "OI_OCCUPANT_GENERATION",
        "AIKIT_CONTEXT_ID",
        "AIKIT_SESSION_ID",
        "AIKIT_VIEW",
        "AIKIT_CONTEXT_ROOT",
        "AIKIT_TASK",
        "AIKIT_PROJECT_ID",
    ] {
        command.env_remove(name);
    }
    command.env("AIKIT_SESSION_ID", task_session_id(session).as_str());
    if let Some(context) = context {
        command.env("AIKIT_CONTEXT_ID", context.as_str());
    }
}
impl EncounterService {
    pub fn task_repertoire_session_id(session: &ResourceRef) -> aikit_core::SessionId {
        task_session_id(session)
    }
    pub fn task_repertoire_context_id(session: &ResourceRef) -> aikit_core::ContextId {
        task_context_id(session)
    }
    pub fn open_task_repertoire(
        home: &AikitHome,
        cwd: &Path,
        session: &ResourceRef,
    ) -> Result<crate::app::Service> {
        crate::app::Service::open(home.clone(), cwd, |key| task_repertoire_env(session, key))
    }
    /// Owner-only CAS. A pending record is durable before allocating NOW; any
    /// failed preparation remains blocking, not an unconfined fallback.
    pub fn configure_task(
        home: &AikitHome,
        session: &ResourceRef,
        input: Value,
        expected: Option<&SourceRevision>,
    ) -> Result<Value> {
        let request: TaskRequest = serde_json::from_value(input).map_err(error)?;
        validate_task_repertoire(home, session, &request)?;
        crate::encounter_profile_provider::ensure_connection_facts_reachable(&request.provider)?;
        // Resolve and validate the declared body before journalling a pending
        // task or allocating its NOW. The raw request remains the immutable
        // source; its exact embedded profile is resolved again at admission
        // and final protected exec.
        let resolved_body =
            crate::encounter_profile_provider::resolve_provider(request.provider.clone())?;
        crate::encounter_profile_provider::ensure_connection_facts_reachable(&resolved_body)?;
        if resolved_body.argv.first().is_none_or(|arg| arg.is_empty()) {
            return Err(error("Task body has no native protocol launcher"));
        }
        let _lock = ContextLock::acquire(
            home,
            &format!(
                "encounter-agency-{}",
                blake3::hash(session.as_str().as_bytes()).to_hex()
            ),
            LockOptions::default(),
        )?;
        let service = Self::new(home.clone())?;
        service.require_attached(session)?;
        if request.cwd.canonicalize().map_err(error)? != request.cwd || !request.cwd.is_dir() {
            return Err(error("Task cwd must be an existing canonical directory"));
        }
        let agency_revision = authority(home, session, &request)?;
        let prepared_run = if let Some(run) = &request.prepared_run_scope {
            if request.material_host.is_some() {
                return Err(error(
                    "An existing prepared run cannot also allocate another material host",
                ));
            }
            Some(prepared_run::Binding::resolve(run)?)
        } else {
            None
        };
        if let Some(host) = &request.material_host {
            host.preflight()?;
        }
        let current = read(home, session)?;
        if current.as_ref().map(|c| &c.revision) != expected {
            return Err(error(
                "Task revision conflict; read current task before changing it",
            ));
        }
        if current
            .as_ref()
            .is_some_and(|c| c.request.material_host.is_some() && request.material_host.is_none())
        {
            return Err(error(
                "A hosted task cannot silently drop its material requirement",
            ));
        }
        if current.as_ref().is_some_and(|c| {
            c.request.prepared_run_scope.is_some() && request.prepared_run_scope.is_none()
        }) {
            return Err(error(
                "A task cannot silently drop its existing prepared run",
            ));
        }
        if current
            .as_ref()
            .is_some_and(|c| c.request.central.task_ref != request.central.task_ref)
        {
            return Err(error(
                "A session cannot silently become another task or Candidate",
            ));
        }
        if current.as_ref().is_some_and(|c| {
            !c.ready && serde_json::to_value(&c.request).ok() != serde_json::to_value(&request).ok()
        }) {
            return Err(error(
                "Uncertain preparation must recover the same request, not replace it",
            ));
        }
        // Central's allocation identity is immutable for a task. Check the
        // actual allocated record before replacing an existing ready binding;
        // a refused amendment must leave that ready binding untouched. First
        // preparation still journals pending before any owner effect, because
        // a native timeout may leave an allocation uncertain.
        if let Some(current) = current.as_ref().filter(|c| c.ready) {
            let allocated = current
                .allocation
                .as_ref()
                .ok_or_else(|| error("Ready task lacks native Central allocation"))?;
            if request.central.central_root != current.request.central.central_root
                || request.central.project != current.request.central.project
                || allocated.allocation["record"]["task_ref"] != json!(request.central.task_ref)
                || allocated.allocation["record"]["purpose"] != request.central.purpose
                || allocated.allocation["record"]["participant_refs"]
                    != json!(request.central.participant_refs)
                || allocated.allocation["record"]["source_refs"]
                    != json!(request.central.source_refs)
            {
                return Err(error(
                    "Native Central allocation identity is immutable; retain the ready task and use its original request",
                ));
            }
        }
        let revision = SourceRevision::parse(format!("task-binding/{}", ulid::Ulid::generate()))?;
        let launcher = launcher_for(session, &request.provider, &revision)?;
        let record = TaskRecord {
            schema: "aikit.encounter-task/v1".into(),
            revision,
            request,
            agency_revision,
            ready: false,
            allocation: None,
            requirements: None,
            inspection: None,
            cwd_anchor: None,
            launcher,
            material: None,
            prepared_run,
            cleanup: None,
        };
        publish(home, session, &record)?;
        serde_json::to_value(prepare_published(home, session, record, None)?).map_err(error)
    }
    /// Explicitly restore one exact ready revision after a failed unhosted
    /// preparation. A hosted pending demand may have uncertain external
    /// effects, so it can only be retried with the same request/Workcell key.
    pub fn abort_task_preparation(
        home: &AikitHome,
        session: &ResourceRef,
        expected: &SourceRevision,
        restore: &SourceRevision,
    ) -> Result<Value> {
        let _lock = ContextLock::acquire(
            home,
            &format!(
                "encounter-agency-{}",
                blake3::hash(session.as_str().as_bytes()).to_hex()
            ),
            LockOptions::default(),
        )?;
        let service = Self::new(home.clone())?;
        service.require_attached(session)?;
        let current = read(home, session)?.ok_or_else(|| error("No task preparation to abort"))?;
        if &current.revision != expected {
            return Err(error(
                "Task revision conflict; read current task before recovery",
            ));
        }
        if current.ready {
            return Err(error("Only a pending task preparation can be aborted"));
        }
        if current.request.material_host.is_some() || current.material.is_some() {
            return Err(error(
                "Hosted preparation may have uncertain effects; recover the same request through its native Workcell demand",
            ));
        }
        if current.request.prepared_run_scope.is_some() || current.prepared_run.is_some() {
            return Err(error(
                "Prepared-run preparation may have uncertain effects; recover the same request through its native Workcell run",
            ));
        }
        let mut prior = historical_ready(home, session, restore)?;
        if prior.schema != "aikit.encounter-task/v1"
            || !prior.ready
            || prior.request.material_host.is_some()
            || prior.material.is_some()
            || prior.request.prepared_run_scope.is_some()
            || prior.prepared_run.is_some()
            || prior.request.central.task_ref != current.request.central.task_ref
            || prior.request.central.central_root != current.request.central.central_root
            || prior.request.central.project != current.request.central.project
            || !launcher_belongs_to(session, &prior)
        {
            return Err(error(
                "Recovery target must be an earlier unhosted ready revision for the same task",
            ));
        }
        if prior.request.cwd.canonicalize().map_err(error)? != prior.request.cwd
            || !prior.request.cwd.is_dir()
        {
            return Err(error("Task cwd must be an existing canonical directory"));
        }
        let agency_revision = authority(home, session, &prior.request)?;
        let historical = prior.clone();
        prior.revision = SourceRevision::parse(format!("task-binding/{}", ulid::Ulid::generate()))?;
        prior.launcher = launcher_for(session, &prior.request.provider, &prior.revision)?;
        prior.agency_revision = agency_revision;
        prior.ready = false;
        prior.allocation = None;
        prior.requirements = None;
        prior.inspection = None;
        prior.cwd_anchor = None;
        prior.material = None;
        prior.cleanup = None;
        publish(home, session, &prior)?;
        let prior = prepare_published(home, session, prior, Some(&historical))?;
        Ok(json!({
            "status":"restored",
            "aborted_revision": current.revision,
            "record": prior,
        }))
    }
    pub(crate) fn selected_model_provider(
        &self,
        session: &ResourceRef,
        provider: &EncounterProvider,
        cwd: &std::path::Path,
    ) -> Result<(EncounterProvider, bool)> {
        self.selected_model_provider_with_deadline(session, provider, cwd, None)
    }
    pub(crate) fn selected_model_provider_before(
        &self,
        session: &ResourceRef,
        provider: &EncounterProvider,
        cwd: &std::path::Path,
        deadline: std::time::Instant,
    ) -> Result<(EncounterProvider, bool)> {
        crate::encounter_service::ensure_native_startup_deadline(deadline)?;
        self.selected_model_provider_with_deadline(session, provider, cwd, Some(deadline))
    }
    fn selected_model_provider_with_deadline(
        &self,
        session: &ResourceRef,
        provider: &EncounterProvider,
        cwd: &std::path::Path,
        deadline: Option<std::time::Instant>,
    ) -> Result<(EncounterProvider, bool)> {
        let Some(record) = read(&self.home, session)? else {
            return Ok((provider.clone(), false));
        };
        validate_before(&self.home, session, &record, deadline)?;
        if let Some(material) = &record.material {
            material.check_encounter_owner()?;
        }
        if serde_json::to_value(provider).map_err(error)?
            != serde_json::to_value(&record.launcher).map_err(error)?
            || cwd != record.request.cwd
        {
            return Err(error("Task-bound session must use its prepared native launcher and exact working directory; another provider is not a permitted fallback"));
        }
        let body =
            crate::encounter_profile_provider::resolve_provider(record.request.provider.clone())?;
        if body.protocol != record.launcher.protocol {
            return Err(error(
                "Prepared task body protocol differs from its Workcell launcher",
            ));
        }
        Ok((body, true))
    }
    pub fn read_task(home: &AikitHome, session: &ResourceRef) -> Result<Value> {
        serde_json::to_value(read(home, session)?).map_err(error)
    }
    /// Native protocol launcher: replace this process under Workcell's actual
    /// stdio-preserving boundary. No wrapper stdout enters the ACP/Pi stream.
    pub fn exec_task(
        home: &AikitHome,
        session: &ResourceRef,
        expected: &SourceRevision,
    ) -> Result<()> {
        let record = read(home, session)?
            .ok_or_else(|| error("Task binding removed; refusing provider start"))?;
        if &record.revision != expected
            || fs::canonicalize(std::env::current_dir().map_err(error)?).map_err(error)?
                != record.request.cwd
        {
            return Err(error(
                "Task revision or actual process cwd differs from the prepared execution basis",
            ));
        }
        validate(home, session, &record)?;
        let file =
            tempfile::NamedTempFile::new_in(path(home, session).parent().expect("task parent"))
                .map_err(error)?;
        fs::write(
            file.path(),
            record
                .requirements
                .as_ref()
                .expect("validated requirements")
                .to_string(),
        )
        .map_err(error)?;
        let inspection = record.inspection.as_ref().expect("validated inspection");
        let requirements = record
            .requirements
            .as_ref()
            .expect("validated requirements");
        // Pi writes its agent configuration at startup even when --session-dir
        // points into task scratch. Keep that state in this task's actual
        // Central allocation, already present in the prepared Workcell grant.
        // Do not inherit an ambient PI_CODING_AGENT_DIR or widen the grant.
        let pi_config_dir = if record.launcher.protocol == EncounterProtocol::PiRpc {
            let now = record
                .allocation
                .as_ref()
                .expect("validated allocation")
                .now_directory()?;
            if now.canonicalize().map_err(error)? != now
                || !requirements["writable_paths"]
                    .as_array()
                    .is_some_and(|paths| paths.contains(&json!(now)))
            {
                return Err(error(
                    "Pi state needs the exact prepared task NOW write allocation",
                ));
            }
            let config_dir = now.join("pi-agent");
            match fs::symlink_metadata(&config_dir) {
                Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                    return Err(error("Pi state directory must be a native directory"));
                }
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(error(e)),
                _ => {}
            }
            Some(config_dir)
        } else {
            None
        };
        let resolved_body =
            crate::encounter_profile_provider::resolve_provider(record.request.provider.clone())?;
        if resolved_body.protocol != record.launcher.protocol {
            return Err(error(
                "Prepared task body protocol differs from its Workcell launcher",
            ));
        }
        let (mut model_argv, model_environment) =
            super::model::execution(home, session, &resolved_body)?;
        if resolved_body.model_policy.is_none() {
            let default = crate::model_defaults::for_session(home, session, &resolved_body)?;
            model_argv = crate::model_defaults::launch_argv(&resolved_body, default.as_ref())?;
        }
        if resolved_body.protocol == EncounterProtocol::PiRpc {
            pin_task_pi_repertoire(home, session, &record.request, &mut model_argv)?;
        }
        let codex_runtime = task_codex_runtime(&record, &resolved_body, &model_argv)?;
        // Only nonsecret routing/type facts enter this private immutable launch
        // source. It lives with the existing requirements owner, outside Task T.
        let mut projection_file = if let Some(runtime) = codex_runtime.as_ref() {
            let mut projection =
                tempfile::NamedTempFile::new_in(path(home, session).parent().expect("task parent"))
                    .map_err(|failure| error(&failure).with_io_source(failure))?;
            projection
                .write_all(runtime.projection.to_string().as_bytes())
                .map_err(|failure| error(&failure).with_io_source(failure))?;
            projection
                .as_file()
                .sync_all()
                .map_err(|failure| error(&failure).with_io_source(failure))?;
            Some(projection)
        } else {
            None
        };
        let mut command = Command::new(boundary_executable(&record)?);
        command
            .arg(if projection_file.is_some() {
                "exec-runtime"
            } else {
                "exec"
            })
            .arg(file.path())
            .arg(
                requirements["policy_revision"]
                    .as_str()
                    .expect("validated revision"),
            )
            .arg(
                inspection["requirements_digest"]
                    .as_str()
                    .expect("validated digest"),
            );
        if let (Some(projection), Some(runtime)) =
            (projection_file.as_ref(), codex_runtime.as_ref())
        {
            command.arg(projection.path()).arg(format!(
                "sha256:{:x}",
                Sha256::digest(runtime.projection.to_string().as_bytes())
            ));
        }
        command
            .arg("--")
            .args(&model_argv)
            .env_remove("CENTRAL_NATIVE_TOKEN")
            .env_remove("WORKCELL_CONTROL_TOKEN");
        if let Some(environment) = model_environment {
            environment.apply(&mut command);
        }
        // Credential/profile delivery may reconstruct its safe allowlist;
        // identity isolation therefore follows it, at the final child owner.
        validate_task_repertoire(home, session, &record.request)?;
        let selected_context = record
            .request
            .repertoire
            .as_ref()
            .map(|_| task_context_id(session));
        isolate_task_identity(&mut command, session, selected_context.as_ref());
        if let Some(runtime) = codex_runtime {
            command.env("npm_config_cache", runtime.npm_cache);
            // Task-owned native material placement follows the credential
            // scrub. Codex retains persisted/managed sqlite_home precedence;
            // this does not override its auth/config home or session storage.
            command.env("CODEX_SQLITE_HOME", runtime.sqlite_home);
        }
        if let Some(config_dir) = pi_config_dir {
            command.env("PI_CODING_AGENT_DIR", config_dir);
        }
        // Retain the immutable requirements path across exec. Its private owner
        // directory is outside every write aperture. History can inspect it.
        let (_file, _retained_path) = file.keep().map_err(error)?;
        let _retained_projection = projection_file
            .take()
            .map(|file| file.keep())
            .transpose()
            .map_err(|failure| {
                let cause = failure.error;
                error(&cause).with_io_source(cause)
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            Err(error(command.exec()))
        }
        #[cfg(not(unix))]
        {
            Err(error(
                "Native task protocol boundary unsupported on this platform",
            ))
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod owner_runner_native_tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn completed_native_nonzero_status_and_valid_replacement_character_are_data() {
        let output = OwnerRunner
            .run(&[
                "/bin/sh".into(),
                "-c".into(),
                r#"printf '\357\277\275'; printf diagnostic >&2; exit 7"#.into(),
            ])
            .unwrap();
        assert_eq!(output.status, 7);
        assert_eq!(output.stdout, "\u{fffd}");
        assert_eq!(output.stderr, "diagnostic");
    }

    #[test]
    fn invalid_native_receipt_preserves_actual_effect_exit_lifecycle_and_typed_cause() {
        let owned = tempfile::tempdir().unwrap();
        for (stream, script) in [
            ("stdout", r#"printf effect > "$1"; printf '\377'; exit 7"#),
            (
                "stderr",
                r#"printf effect > "$1"; printf '\377' >&2; exit 7"#,
            ),
        ] {
            let marker = owned.path().join(stream);
            let failure = OwnerRunner
                .run(&[
                    "/bin/sh".into(),
                    "-c".into(),
                    script.into(),
                    "owned-native-owner".into(),
                    marker.to_str().unwrap().into(),
                ])
                .unwrap_err();
            assert_eq!(fs::read(marker).unwrap(), b"effect");
            assert_eq!(failure.code(), "encounter.runtime");
            assert_eq!(
                failure.details()["native_runner_code"],
                "mux.command_utf8_invalid"
            );
            assert_eq!(failure.details()["stream"], stream);
            assert_eq!(failure.details()["execution_started"], "true");
            assert_eq!(failure.details()["known_exit_status"], "7");
            assert_eq!(failure.details()["direct_child_reaped"], "true");
            assert_eq!(failure.details()["effects"], "unknown");
            assert_eq!(failure.details()["automatic_retry"], "false");
            let actual = failure
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap();
            assert_eq!(actual.kind(), std::io::ErrorKind::InvalidData);
            let decoder = actual
                .get_ref()
                .unwrap()
                .downcast_ref::<std::str::Utf8Error>()
                .unwrap();
            assert_eq!(decoder.valid_up_to(), 0);
            assert_eq!(decoder.error_len(), Some(1));
            let cloned = failure.clone();
            let retained = cloned
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap();
            assert!(std::ptr::eq(actual, retained));
        }
    }

    #[test]
    fn missing_actual_native_program_retains_not_started_and_original_io_cause() {
        let owned = tempfile::tempdir().unwrap();
        let missing = owned.path().join("missing-native-owner");
        let failure = OwnerRunner
            .run(&[missing.to_str().unwrap().into()])
            .unwrap_err();
        assert_eq!(failure.code(), "encounter.runtime");
        assert_eq!(
            failure.details()["native_runner_code"],
            "mux.command_spawn_failed"
        );
        assert_eq!(failure.details()["execution_started"], "false");
        assert_eq!(failure.details()["automatic_retry"], "false");
        let actual = failure
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        assert_eq!(actual.kind(), std::io::ErrorKind::NotFound);
        assert!(actual.raw_os_error().is_some());
    }

    #[test]
    fn natural_inherited_native_output_retains_the_actual_complete_nonzero_result() {
        let output = OwnerRunner
            .run(&[
                "/bin/sh".into(),
                "-c".into(),
                r#"(sleep 0.05; printf late; printf final >&2) & exit 7"#.into(),
            ])
            .unwrap();
        assert_eq!(output.status, 7);
        assert_eq!(output.stdout, "late");
        assert_eq!(output.stderr, "final");
    }

    #[test]
    fn complete_inherited_invalid_output_keeps_its_actual_decode_cause() {
        let failure = OwnerRunner
            .run(&[
                "/bin/sh".into(),
                "-c".into(),
                r#"(sleep 0.05; printf '\377') & exit 0"#.into(),
            ])
            .unwrap_err();
        assert_eq!(failure.code(), "encounter.runtime");
        assert_eq!(
            failure.details()["native_runner_code"],
            "mux.command_utf8_invalid"
        );
        assert_eq!(failure.details()["known_exit_status"], "0");
        assert_eq!(failure.details()["direct_child_reaped"], "true");
        assert_eq!(failure.details()["group_signal"], "not-needed");
        assert_eq!(failure.details()["capture_cancelled"], "false");
        assert_eq!(failure.details()["stdout_eof"], "true");
        assert_eq!(failure.details()["stderr_eof"], "true");
        assert_eq!(failure.details()["effects"], "unknown");
        assert_eq!(failure.details()["automatic_retry"], "false");
        let actual = failure
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        assert_eq!(actual.kind(), std::io::ErrorKind::InvalidData);
        let decoder = actual
            .get_ref()
            .unwrap()
            .downcast_ref::<std::str::Utf8Error>()
            .unwrap();
        assert_eq!(decoder.valid_up_to(), 0);
        assert_eq!(decoder.error_len(), Some(1));
    }

    #[test]
    fn unfinished_inherited_capture_refuses_a_receipt_and_keeps_actual_cancellation_facts() {
        let owned = tempfile::tempdir().unwrap();
        let marker = owned.path().join("native-descendant-ready");
        let failure = OwnerRunner
            .run(&[
                "/bin/sh".into(),
                "-c".into(),
                r#"(printf ready > "$1"; sleep 30; printf unfinished) & while [ ! -s "$1" ]; do sleep 0.005; done; exit 23"#.into(),
                "owned-native-capture".into(),
                marker.to_str().unwrap().into(),
            ])
            .unwrap_err();
        assert_eq!(fs::read(marker).unwrap(), b"ready");
        assert_eq!(failure.code(), "encounter.runtime");
        match failure.details()["native_runner_code"].as_str() {
            "mux.command_capture_cancelled" => {
                assert_eq!(failure.details()["stdout_eof"], "true");
                assert_eq!(failure.details()["stderr_eof"], "true");
            }
            "mux.command_capture_incomplete" => {
                assert!(
                    failure.details()["stdout_eof"] == "false"
                        || failure.details()["stderr_eof"] == "false"
                );
            }
            other => panic!("actual held native capture had an unrelated failure: {other}"),
        }
        assert_eq!(failure.details()["known_exit_status"], "23");
        assert_eq!(failure.details()["execution_started"], "true");
        assert_eq!(failure.details()["direct_child_reaped"], "true");
        assert_eq!(failure.details()["group_signal"], "delivered");
        assert_eq!(failure.details()["capture_cancelled"], "true");
        assert_eq!(failure.details()["effects"], "unknown");
        assert_eq!(failure.details()["automatic_retry"], "false");
    }
}

#[cfg(test)]
mod profile_task_tests {
    use super::*;

    fn raw_task_request(directory: &std::path::Path) -> Value {
        json!({
            "central": {
                "ctrl_bin": directory.join("ctrl"), "central_root": directory,
                "project": null, "task_ref": "task:request-contract",
                "purpose": "Request serialization only, no native admission",
                "participant_refs": ["agent/request-contract"], "source_refs": []
            },
            "provider": {"id":"request-contract", "label":"Request contract",
                "protocol":"acp", "from_profile":"codex"},
            "cwd": directory, "selected_directories": [],
            "workcell_boundary_bin": directory.join("workcell-write-boundary"),
            "authority_ref": "authority:request-contract"
        })
    }

    #[test]
    fn legacy_task_request_omits_empty_additional_protection() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let raw = raw_task_request(&root);
        let legacy: TaskRequest = serde_json::from_value(raw.clone()).unwrap();
        let legacy = serde_json::to_value(legacy).unwrap();
        assert!(legacy.get("additional_protected_directories").is_none());
        let mut explicit_empty = raw;
        explicit_empty["additional_protected_directories"] = json!([]);
        let parsed: TaskRequest = serde_json::from_value(explicit_empty).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), legacy);
    }

    #[test]
    fn task_request_retains_exact_explicit_exclusions_as_source() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let mut raw = raw_task_request(&root);
        let requested = json!([second, first, second]);
        raw["additional_protected_directories"] = requested.clone();
        let parsed: TaskRequest = serde_json::from_value(raw).unwrap();
        let retained = serde_json::to_value(parsed).unwrap();
        assert_eq!(retained["additional_protected_directories"], requested);
        assert!(retained["selected_directories"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(retained["authority_ref"], "authority:request-contract");
        assert_eq!(retained["central"]["task_ref"], "task:request-contract");
    }

    #[test]
    fn codex_profile_request_keeps_raw_source_and_resolves_inside_the_boundary() {
        let raw: EncounterProvider = serde_json::from_value(json!({
            "id":"codex-native-body",
            "label":"Codex native body",
            "protocol":"acp",
            "from_profile":"codex"
        }))
        .unwrap();
        assert!(raw.argv.is_empty());
        let resolved = crate::encounter_profile_provider::resolve_provider(raw.clone()).unwrap();
        assert_eq!(resolved.from_profile.as_deref(), Some("codex"));
        assert_eq!(resolved.protocol, EncounterProtocol::Acp);
        assert_eq!(resolved.argv.first().map(String::as_str), Some("npx"));
        assert!(resolved
            .argv
            .iter()
            .any(|arg| arg == "@agentclientprotocol/codex-acp"));

        let session = ResourceRef::parse("agent-session/codex-native-task").unwrap();
        let revision = SourceRevision::parse("task-binding/codex-native-task").unwrap();
        let launcher = launcher_for(&session, &raw, &revision).unwrap();
        assert!(
            raw.argv.is_empty(),
            "saved request is still the raw profile source"
        );
        assert_eq!(launcher.from_profile, None);
        assert!(launcher.argv_fallback.is_empty());
        assert_eq!(launcher.protocol, resolved.protocol);
        assert!(launcher.argv.iter().any(|arg| arg == "encounter-task-exec"));
        assert_ne!(
            launcher.argv, resolved.argv,
            "Codex is not launched outside Workcell"
        );
    }
}

#[cfg(test)]
mod task_identity_tests {
    use super::*;
    #[test]
    fn actual_child_process_receives_distinct_native_runtime_scope() {
        let first = ResourceRef::parse("agent-session/first-child").unwrap();
        let second = ResourceRef::parse("agent-session/second-child").unwrap();
        let context = aikit_core::ContextId::parse("ctx_selected-child").unwrap();
        let mut command = Command::new("/bin/sh");
        command.args(["-c","test -z \"$OI_POSITION_REF\" && test -z \"$OI_OCCUPANT_GENERATION\" && test -z \"$AIKIT_VIEW\" && printf '%s\\n%s\\n%s' \"$AIKIT_SESSION_ID\" \"$AIKIT_CONTEXT_ID\" \"$CENTRAL_NATIVE_TOKEN\""]);
        for name in [
            "OI_POSITION_REF",
            "OI_OCCUPANT_GENERATION",
            "AIKIT_CONTEXT_ID",
            "AIKIT_SESSION_ID",
            "AIKIT_VIEW",
        ] {
            command.env(name, "parent-only");
        }
        command.env("CENTRAL_NATIVE_TOKEN", "explicit-owner-grant");
        isolate_task_identity(&mut command, &first, Some(&context));
        let output = command.output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!(
                "{}\n{}\nexplicit-owner-grant",
                task_session_id(&first),
                context
            )
        );
        assert_ne!(task_session_id(&first), task_session_id(&second));
        let mut unselected = Command::new("/bin/sh");
        unselected
            .args(["-c", "test -z \"$AIKIT_CONTEXT_ID\""])
            .env("AIKIT_CONTEXT_ID", "parent-only");
        isolate_task_identity(&mut unselected, &second, None);
        assert!(unselected.status().unwrap().success());
    }
}

#[cfg(test)]
mod task_pi_repertoire_tests {
    use super::*;
    #[test]
    fn pins_native_skill_arguments_to_immutable_directories_and_refuses_ambient_override() {
        let temporary = tempfile::tempdir().unwrap();
        let selected = temporary
            .path()
            .join("generations/gen_selected/projections/pi/.pi/skills");
        fs::create_dir_all(&selected).unwrap();
        fs::write(
            selected.join("SKILL.md"),
            "---\nname: selected\ndescription: Selected repertoire\n---\nExact material\n",
        )
        .unwrap();
        let mut argv = vec!["pi".into(), "--mode".into(), "rpc".into()];
        selected_pi_skill_argv(&mut argv, Some(&selected)).unwrap();
        assert_eq!(
            &argv[3..],
            &[
                "--no-skills".to_string(),
                "--skill".into(),
                selected.to_str().unwrap().into()
            ]
        );
        assert!(fs::read_to_string(selected.join("SKILL.md"))
            .unwrap()
            .contains("Exact material"));
        for conflicting in ["--skill", "--skill=/foreign/context"] {
            let mut argv = vec!["pi".into(), conflicting.into()];
            assert!(selected_pi_skill_argv(&mut argv, Some(&selected)).is_err());
            assert_eq!(argv.len(), 2);
        }
        let mut empty = vec!["pi".into()];
        selected_pi_skill_argv(&mut empty, None).unwrap();
        assert_eq!(empty, vec!["pi", "--no-skills"]);
        #[cfg(unix)]
        {
            let link = temporary.path().join("current");
            std::os::unix::fs::symlink(&selected, &link).unwrap();
            assert!(selected_pi_skill_argv(&mut vec!["pi".into()], Some(&link)).is_err());
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod startup_deadline_tests {
    use super::*;
    #[test]
    fn actual_native_readbacks_share_the_existing_operation_deadline() {
        let start = std::time::Instant::now();
        let runner = DeadlineOwnerRunner(Some(start + Duration::from_secs(5)));
        let first = runner
            .run(&["/bin/sh".into(), "-c".into(), "sleep 2".into()])
            .unwrap();
        assert!(first.ok());
        let second = runner.run(&["/bin/sh".into(), "-c".into(), "sleep 60".into()]);
        assert!(second.is_err());
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "a second owner readback renewed startup"
        );
        let expired = runner.run(&["/bin/sh".into(), "-c".into(), "exit 0".into()]);
        assert!(
            expired.is_err(),
            "even a fast owner is not invoked after the retained budget"
        );
    }
}
