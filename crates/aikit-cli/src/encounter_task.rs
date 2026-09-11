//! Task admission for the existing encounter owner. Central owns the clearing;
//! Workcell confines the existing protocol child; this module owns neither.
use super::{error, native_admission, read_binding};
use crate::encounter_service::{EncounterProvider, EncounterService};
use aikit_adapters::central_placement::{AllocatedCentralTask, CentralTaskRequest, NativeCentralPlacement};
use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::{ResourceRef, Result, SourceRevision};
use aikit_store::{AikitHome, ContextLock, LockOptions};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, io::{Read, Write}, path::PathBuf, process::{Command, Stdio}, time::{Duration, Instant}};

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
}
/// Finite native-owner requests, not protocol/session lifetime. Partial effects
/// stay uncertain on timeout; the durable task key is never replaced for retry.
struct OwnerRunner;
impl CommandRunner for OwnerRunner {
    fn run(&self, argv: &[String]) -> Result<Output> {
        let (program, args) = argv.split_first().ok_or_else(|| error("Missing native operation"))?;
        let out = tempfile::tempfile().map_err(error)?;
        let err = tempfile::tempfile().map_err(error)?;
        let mut child = Command::new(program).args(args).stdin(Stdio::null())
            .stdout(out.try_clone().map_err(error)?).stderr(err.try_clone().map_err(error)?)
            .spawn().map_err(error)?;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(error)? { break status; }
            if start.elapsed() > Duration::from_secs(15)
                || out.metadata().map_err(error)?.len() > 4 * 1024 * 1024
                || err.metadata().map_err(error)?.len() > 4 * 1024 * 1024 {
                let _ = child.kill(); let _ = child.wait();
                return Err(error("Native owner timeout/output limit; effects may be uncertain; recover the same task request explicitly"));
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        fn text(mut file: fs::File) -> Result<String> {
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::Start(0)).map_err(error)?;
            let mut bytes = Vec::new(); file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(error)?;
            if bytes.len() > 4 * 1024 * 1024 { return Err(error("Native output too large")); }
            String::from_utf8(bytes).map_err(error)
        }
        Ok(Output { status: status.code().unwrap_or(-1), stdout: text(out)?, stderr: text(err)? })
    }
}
fn path(home: &AikitHome, session: &ResourceRef) -> PathBuf {
    home.state().join("encounter-tasks").join(format!("{}.json", blake3::hash(session.as_str().as_bytes()).to_hex()))
}
fn read(home: &AikitHome, session: &ResourceRef) -> Result<Option<TaskRecord>> {
    let path = path(home, session);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
        Ok(meta) if !meta.is_file() || meta.len() > 4 * 1024 * 1024 => return Err(error("Task binding must be a bounded native file")),
        Ok(_) => {}
    }
    if path.canonicalize().map_err(error)? != path { return Err(error("Redirected task binding")); }
    serde_json::from_slice(&fs::read(path).map_err(error)?).map(Some).map_err(error)
}
fn publish(home: &AikitHome, session: &ResourceRef, record: &TaskRecord) -> Result<()> {
    let target = path(home, session);
    let parent = target.parent().expect("task parent");
    fs::create_dir_all(parent).map_err(error)?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(error)?;
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    staged.write_all(&serde_json::to_vec_pretty(record).map_err(error)?).map_err(error)?;
    staged.as_file().sync_all().map_err(error)?;
    staged.persist(target).map_err(error)?;
    fs::File::open(parent).and_then(|f| f.sync_all()).map_err(error)
}
fn authority(home: &AikitHome, session: &ResourceRef, request: &TaskRequest) -> Result<SourceRevision> {
    let binding = read_binding(home, session)?.ok_or_else(|| error("Task needs an actual selected Agency, not a profile"))?;
    let admitted = native_admission(&binding)?;
    if !admitted.authorises(&ResourceRef::parse(WRITE_ACTION)?)
        || !admitted.receipt["determination"]["authority_refs"].as_array().is_some_and(|refs| refs.contains(&json!(request.authority_ref)))
        || !request.central.participant_refs.contains(&admitted.agent_ref) {
        return Err(error("Current Agency does not authorise this task, authority or participant"));
    }
    Ok(binding.revision)
}
fn inspect(request: &TaskRequest, requirements: &Value) -> Result<Value> {
    if !request.workcell_boundary_bin.is_absolute() { return Err(error("Explicit Workcell executable required")); }
    let file = tempfile::NamedTempFile::new().map_err(error)?;
    fs::write(file.path(), requirements.to_string()).map_err(error)?;
    let output = OwnerRunner.run(&[
        request.workcell_boundary_bin.display().to_string(), "inspect".into(),
        file.path().display().to_string(), requirements["policy_revision"].as_str().ok_or_else(|| error("Missing policy revision"))?.into(),
    ])?;
    let value: Value = serde_json::from_str(&output.stdout).map_err(error)?;
    if !output.ok() || value["schema"] != "workcell.prepared-write-boundary/v1"
        || value["requirements"] != *requirements || value["state"] != "prepared-not-executed"
        || value["requirements_digest"].as_str().is_none_or(str::is_empty) {
        return Err(error("Workcell cannot prepare the exact required protection; no weaker fallback"));
    }
    Ok(value)
}
fn validate(home: &AikitHome, session: &ResourceRef, record: &TaskRecord) -> Result<()> {
    if record.schema != "aikit.encounter-task/v1" || !record.ready {
        return Err(error("Task preparation is incomplete; explicitly recover the same request"));
    }
    if authority(home, session, &record.request)? != record.agency_revision {
        return Err(error("Task Agency changed; explicitly re-resolve before further work"));
    }
    let task = record.allocation.as_ref().ok_or_else(|| error("Missing native NOW"))?;
    let owner = NativeCentralPlacement::new(OwnerRunner);
    owner.revalidate(task)?;
    let decision = owner.validate_write(task, &record.request.cwd)?;
    if decision.get("destination_anchor") != record.cwd_anchor.as_ref() {
        return Err(error("Task working directory changed since preparation"));
    }
    let requirements = owner.write_boundary_requirements(task, &record.request.authority_ref, &record.request.selected_directories)?;
    if Some(&requirements) != record.requirements.as_ref() { return Err(error("Material requirements changed; no automatic renewal")); }
    let fresh = inspect(&record.request, &requirements)?;
    // Includes every native path/type/inode and the exact material requirement digest.
    if record.inspection.as_ref().is_none_or(|old|
        old["requirements_digest"] != fresh["requirements_digest"]
        || old["writable_objects"] != fresh["writable_objects"]
        || old["protected_objects"] != fresh["protected_objects"]
        || old["objects"] != fresh["objects"]) {
        return Err(error("Material path identity changed; prepare an explicit new binding"));
    }
    Ok(())
}

pub(super) fn check(home: &AikitHome, session: &ResourceRef) -> Result<()> {
    if let Some(record) = read(home, session)? { validate(home, session, &record)?; }
    Ok(())
}
/// Called at the existing prompt boundary for human and addressed turns alike.
/// A new task configuration cannot bless an older, unconfined resident process.
pub(super) fn prompt(service: &EncounterService, session: &ResourceRef) -> Result<String> {
    let Some(record) = read(&service.home, session)? else {
        if service.resident(session)?.argv.iter().any(|arg| arg == "encounter-task-exec") {
            return Err(error("Task binding removed from an already task-bound resident; no fallback"));
        }
        return Ok(String::new());
    };
    validate(&service.home, session, &record)?;
    let resident = service.resident(session)?;
    if resident.cwd != record.request.cwd || resident.argv != record.launcher.argv
        || resident.provider != record.launcher.id {
        return Err(error("This resident was not launched for the current task boundary; explicit new body/continuation required"));
    }
    let task = record.allocation.as_ref().expect("validated allocation");
    Ok(format!("\nTask: {}\nTask NOW: {}\nTask output directory: {}\nWorking directory: {}\nPolicy revision: {}\n", task.request.task_ref, task.allocation["now_ref"], task.now_directory()?.display(), record.request.cwd.display(), task.allocation["policy"]["revision"]))
}
impl EncounterService {
    /// Owner-only CAS. A pending record is durable before allocating NOW; any
    /// failed preparation remains blocking, not an unconfined fallback.
    pub fn configure_task(home: &AikitHome, session: &ResourceRef, input: Value, expected: Option<&SourceRevision>) -> Result<Value> {
        let request: TaskRequest = serde_json::from_value(input).map_err(error)?;
        let _lock = ContextLock::acquire(home, &format!("encounter-agency-{}", blake3::hash(session.as_str().as_bytes()).to_hex()), LockOptions::default())?;
        let service = Self::new(home.clone())?;
        service.require_attached(session)?;
        if request.cwd.canonicalize().map_err(error)? != request.cwd || !request.cwd.is_dir() {
            return Err(error("Task cwd must be an existing canonical directory"));
        }
        let agency_revision = authority(home, session, &request)?;
        let current = read(home, session)?;
        if current.as_ref().map(|c| &c.revision) != expected { return Err(error("Task revision conflict; read current task before changing it")); }
        if current.as_ref().is_some_and(|c| c.request.central.task_ref != request.central.task_ref) {
            return Err(error("A session cannot silently become another task or Candidate"));
        }
        if current.as_ref().is_some_and(|c| !c.ready && serde_json::to_value(&c.request).ok() != serde_json::to_value(&request).ok()) {
            return Err(error("Uncertain preparation must recover the same request, not replace it"));
        }
        let revision = SourceRevision::parse(format!("task-binding/{}", ulid::Ulid::generate()))?;
        let mut launcher = request.provider.clone();
        launcher.id = format!("task-{}", blake3::hash(session.as_str().as_bytes()).to_hex());
        launcher.argv = vec![std::env::current_exe().map_err(error)?.display().to_string(),
            "encounter-task-exec".into(), "--agent-session".into(), session.to_string(),
            "--expected-revision".into(), revision.to_string()];
        let mut record = TaskRecord { schema: "aikit.encounter-task/v1".into(), revision,
            request, agency_revision, ready: false, allocation: None, requirements: None,
            inspection: None, cwd_anchor: None, launcher };
        publish(home, session, &record)?;
        let owner = NativeCentralPlacement::new(OwnerRunner);
        let task = owner.allocate(&record.request.central)?;
        let binding = read_binding(home, session)?.ok_or_else(|| error("Agency disappeared"))?;
        if task.allocation["policy"]["scope_ref"] != json!(binding.world_ref) {
            return Err(error("Central task scope differs from the native Agency World; an explicit owner-backed relation is required"));
        }
        record.cwd_anchor = Some(owner.validate_write(&task, &record.request.cwd)?["destination_anchor"].clone());
        let requirements = owner.write_boundary_requirements(&task, &record.request.authority_ref, &record.request.selected_directories)?;
        record.inspection = Some(inspect(&record.request, &requirements)?);
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
    pub(crate) fn check_task_launch(&self, session: &ResourceRef, provider: &EncounterProvider, cwd: &std::path::Path) -> Result<()> {
        let Some(record) = read(&self.home, session)? else { return Ok(()); };
        validate(&self.home, session, &record)?;
        if provider.id != record.launcher.id || provider.argv != record.launcher.argv
            || provider.protocol != record.launcher.protocol || provider.required_context != record.launcher.required_context
            || cwd != record.request.cwd {
            return Err(error("Task-bound session must use its prepared native launcher and exact working directory; another provider is not a permitted fallback"));
        }
        Ok(())
    }
    pub fn read_task(home: &AikitHome, session: &ResourceRef) -> Result<Value> {
        serde_json::to_value(read(home, session)?).map_err(error)
    }
    /// Native protocol launcher: replace this process under Workcell's actual
    /// stdio-preserving boundary. No wrapper stdout enters the ACP/Pi stream.
    pub fn exec_task(home: &AikitHome, session: &ResourceRef, expected: &SourceRevision) -> Result<()> {
        let record = read(home, session)?.ok_or_else(|| error("Task binding removed; refusing provider start"))?;
        if &record.revision != expected || fs::canonicalize(std::env::current_dir().map_err(error)?).map_err(error)? != record.request.cwd {
            return Err(error("Task revision or actual process cwd differs from the prepared execution basis"));
        }
        validate(home, session, &record)?;
        let file = tempfile::NamedTempFile::new_in(path(home, session).parent().expect("task parent")).map_err(error)?;
        fs::write(file.path(), record.requirements.as_ref().expect("validated requirements").to_string()).map_err(error)?;
        let inspection = record.inspection.as_ref().expect("validated inspection");
        let requirements = record.requirements.as_ref().expect("validated requirements");
        let mut command = Command::new(&record.request.workcell_boundary_bin);
        command.args(["exec", &file.path().display().to_string(), requirements["policy_revision"].as_str().expect("validated revision"), inspection["requirements_digest"].as_str().expect("validated digest"), "--"])
            .args(&record.request.provider.argv)
            .env_remove("CENTRAL_NATIVE_TOKEN");
        // Retain the immutable requirements path across exec. Its private owner
        // directory is outside every write aperture. History can inspect it.
        let (_file, _retained_path) = file.keep().map_err(error)?;
        #[cfg(unix)] {
            use std::os::unix::process::CommandExt;
            Err(error(command.exec()))
        }
        #[cfg(not(unix))] { Err(error("Native task protocol boundary unsupported on this platform")) }
    }
}
