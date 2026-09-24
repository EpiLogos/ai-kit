//! Task admission for the existing encounter owner. Central owns the clearing;
//! Workcell confines the existing protocol child; this module owns neither.
use super::{error, native_admission, read_binding};
use crate::encounter_service::{EncounterProtocol, EncounterProvider, EncounterService};
use aikit_adapters::central_placement::{
    AllocatedCentralTask, CentralTaskRequest, NativeCentralPlacement,
};
use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::{ResourceRef, Result, SourceRevision};
use aikit_store::{AikitHome, ContextLock, LockOptions};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[path = "encounter_task_material.rs"]
mod material;
use material::{MaterialBinding, MaterialHost};

const WRITE_ACTION: &str = "action/aikit/encounter-task";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskRequest {
    central: CentralTaskRequest,
    provider: EncounterProvider,
    cwd: PathBuf,
    selected_directories: Vec<PathBuf>,
    workcell_boundary_bin: PathBuf,
    authority_ref: ResourceRef,
    /// A hosted arrangement must prepare/observe this native owner; omission
    /// keeps the explicitly unhosted protected-process mode, not fake hosting.
    #[serde(default)]
    material_host: Option<MaterialHost>,
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
}
/// Finite native-owner requests, not protocol/session lifetime. Partial effects
/// stay uncertain on timeout; the durable task key is never replaced for retry.
struct OwnerRunner;
impl CommandRunner for OwnerRunner {
    fn run(&self, argv: &[String]) -> Result<Output> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| error("Missing native operation"))?;
        let out = tempfile::tempfile().map_err(error)?;
        let err = tempfile::tempfile().map_err(error)?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(out.try_clone().map_err(error)?)
            .stderr(err.try_clone().map_err(error)?)
            .spawn()
            .map_err(error)?;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(error)? {
                break status;
            }
            if start.elapsed() > Duration::from_secs(15)
                || out.metadata().map_err(error)?.len() > 4 * 1024 * 1024
                || err.metadata().map_err(error)?.len() > 4 * 1024 * 1024
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error("Native owner timeout/output limit; effects may be uncertain; recover the same task request explicitly"));
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        fn text(mut file: fs::File) -> Result<String> {
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::Start(0)).map_err(error)?;
            let mut bytes = Vec::new();
            file.take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(error)?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(error("Native output too large"));
            }
            String::from_utf8(bytes).map_err(error)
        }
        Ok(Output {
            status: status.code().unwrap_or(-1),
            stdout: text(out)?,
            stderr: text(err)?,
        })
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
        if record.ready && &record.revision == revision {
            if found.replace(record).is_some() {
                return Err(error("Ambiguous task history revision"));
            }
        }
    }
    found.ok_or_else(|| error("Requested ready task revision is absent from native history"))
}
fn launcher_for(
    session: &ResourceRef,
    provider: &EncounterProvider,
    revision: &SourceRevision,
) -> Result<EncounterProvider> {
    let mut launcher = provider.clone();
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
    Ok(launcher)
}
fn launcher_belongs_to(session: &ResourceRef, record: &TaskRecord) -> bool {
    let mut expected = record.request.provider.clone();
    expected.id = format!(
        "task-{}",
        blake3::hash(session.as_str().as_bytes()).to_hex()
    );
    expected.argv = record.launcher.argv.clone();
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
        && record.launcher.argv.len() >= suffix.len() + 1
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
    let binding = read_binding(home, session)?
        .ok_or_else(|| error("Task needs an actual selected Agency, not a profile"))?;
    let admitted = native_admission(&binding)?;
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
fn inspect(request: &TaskRequest, requirements: &Value) -> Result<Value> {
    if !request.workcell_boundary_bin.is_absolute() {
        return Err(error("Explicit Workcell executable required"));
    }
    let file = tempfile::NamedTempFile::new().map_err(error)?;
    fs::write(file.path(), requirements.to_string()).map_err(error)?;
    let output = OwnerRunner.run(&[
        request.workcell_boundary_bin.display().to_string(),
        "inspect".into(),
        file.path().display().to_string(),
        requirements["policy_revision"]
            .as_str()
            .ok_or_else(|| error("Missing policy revision"))?
            .into(),
    ])?;
    let value: Value = serde_json::from_str(&output.stdout).map_err(error)?;
    if !output.ok()
        || value["schema"] != "workcell.prepared-write-boundary/v1"
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
fn validate(home: &AikitHome, session: &ResourceRef, record: &TaskRecord) -> Result<()> {
    if record.schema != "aikit.encounter-task/v1" || !record.ready {
        return Err(error(
            "Task preparation is incomplete; explicitly recover the same request",
        ));
    }
    if authority(home, session, &record.request)? != record.agency_revision {
        return Err(error(
            "Task Agency changed; explicitly re-resolve before further work",
        ));
    }
    let task = record
        .allocation
        .as_ref()
        .ok_or_else(|| error("Missing native NOW"))?;
    let owner = NativeCentralPlacement::new(OwnerRunner);
    owner.revalidate(task)?;
    let decision = owner.validate_write(task, &record.request.cwd)?;
    if decision.get("destination_anchor") != record.cwd_anchor.as_ref() {
        return Err(error("Task working directory changed since preparation"));
    }
    let requirements = owner.write_boundary_requirements(
        task,
        &record.request.authority_ref,
        &record.request.selected_directories,
    )?;
    if Some(&requirements) != record.requirements.as_ref() {
        return Err(error("Material requirements changed; no automatic renewal"));
    }
    let fresh = inspect(&record.request, &requirements)?;
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
    match (&record.request.material_host, &record.material) {
        (Some(host), Some(binding)) if host == &binding.host => {
            binding.validate(task)?;
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
impl EncounterService {
    /// Owner-only CAS. A pending record is durable before allocating NOW; any
    /// failed preparation remains blocking, not an unconfined fallback.
    pub fn configure_task(
        home: &AikitHome,
        session: &ResourceRef,
        input: Value,
        expected: Option<&SourceRevision>,
    ) -> Result<Value> {
        let request: TaskRequest = serde_json::from_value(input).map_err(error)?;
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
        let mut record = TaskRecord {
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
        };
        publish(home, session, &record)?;
        let owner = NativeCentralPlacement::new(OwnerRunner);
        let task = owner.allocate(&record.request.central)?;
        let binding = read_binding(home, session)?.ok_or_else(|| error("Agency disappeared"))?;
        if task.allocation["policy"]["scope_ref"] != json!(binding.world_ref) {
            return Err(error("Central task scope differs from the native Agency World; an explicit owner-backed relation is required"));
        }
        record.cwd_anchor =
            Some(owner.validate_write(&task, &record.request.cwd)?["destination_anchor"].clone());
        let requirements = owner.write_boundary_requirements(
            &task,
            &record.request.authority_ref,
            &record.request.selected_directories,
        )?;
        record.inspection = Some(inspect(&record.request, &requirements)?);
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
        record.allocation = Some(task);
        record.requirements = Some(requirements);
        // Configuration is independent from the resident. Changing this does
        // not pretend to retrofit an existing process with Landlock.
        Self::configure(home, record.launcher.clone())?;
        record.ready = true;
        validate(home, session, &record)?;
        publish(home, session, &record)?;
        serde_json::to_value(record).map_err(error)
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
        let mut prior = historical_ready(home, session, restore)?;
        if prior.schema != "aikit.encounter-task/v1"
            || !prior.ready
            || prior.request.material_host.is_some()
            || prior.material.is_some()
            || prior.request.central.task_ref != current.request.central.task_ref
            || prior.request.central.central_root != current.request.central.central_root
            || prior.request.central.project != current.request.central.project
            || !launcher_belongs_to(session, &prior)
        {
            return Err(error(
                "Recovery target must be an earlier unhosted ready revision for the same task",
            ));
        }
        prior.revision = SourceRevision::parse(format!("task-binding/{}", ulid::Ulid::generate()))?;
        prior.launcher = launcher_for(session, &prior.request.provider, &prior.revision)?;
        validate(home, session, &prior)?;
        Self::configure(home, prior.launcher.clone())?;
        publish(home, session, &prior)?;
        Ok(json!({
            "status":"restored",
            "aborted_revision": current.revision,
            "record": prior,
        }))
    }
    pub(crate) fn check_task_launch(
        &self,
        session: &ResourceRef,
        provider: &EncounterProvider,
        cwd: &std::path::Path,
    ) -> Result<()> {
        let Some(record) = read(&self.home, session)? else {
            return Ok(());
        };
        validate(&self.home, session, &record)?;
        if let Some(material) = &record.material {
            material.check_encounter_owner()?;
        }
        if provider.id != record.launcher.id
            || provider.argv != record.launcher.argv
            || provider.protocol != record.launcher.protocol
            || provider.required_context != record.launcher.required_context
            || provider.model_policy != record.launcher.model_policy
            || cwd != record.request.cwd
        {
            return Err(error("Task-bound session must use its prepared native launcher and exact working directory; another provider is not a permitted fallback"));
        }
        Ok(())
    }
    pub(crate) fn is_task_bound(&self, session: &ResourceRef) -> Result<bool> {
        Ok(read(&self.home, session)?.is_some())
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
        let (model_argv, model_environment) =
            super::model::execution(home, session, &record.request.provider)?;
        let mut command = Command::new(&record.request.workcell_boundary_bin);
        command
            .args([
                "exec",
                &file.path().display().to_string(),
                requirements["policy_revision"]
                    .as_str()
                    .expect("validated revision"),
                inspection["requirements_digest"]
                    .as_str()
                    .expect("validated digest"),
                "--",
            ])
            .args(&model_argv)
            .env_remove("CENTRAL_NATIVE_TOKEN")
            .env_remove("WORKCELL_CONTROL_TOKEN");
        if let Some(environment) = model_environment {
            environment.apply(&mut command);
        }
        if let Some(config_dir) = pi_config_dir {
            command.env("PI_CODING_AGENT_DIR", config_dir);
        }
        // Retain the immutable requirements path across exec. Its private owner
        // directory is outside every write aperture. History can inspect it.
        let (_file, _retained_path) = file.keep().map_err(error)?;
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
