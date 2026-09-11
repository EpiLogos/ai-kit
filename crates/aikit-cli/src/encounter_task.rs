//! A task attaches native placement to an existing canonical encounter. It is
//! owner configuration, never an authority field accepted from an incoming turn.
use super::{error, EncounterService, Resident};
use aikit_adapters::{central_work::{prepare_boundary, AllocatedTask, CentralPlacement, CentralTask}, runner::SystemRunner};
use aikit_core::{AikitError, ResourceRef, Result, SourceRevision};
use aikit_core::resource::DevelopmentFieldGitBasis;
use aikit_store::AikitHome;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io::Write, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncounterTaskBinding {
    pub revision: SourceRevision,
    pub active: bool,
    pub task: CentralTask,
    /// Explicit AIKit composition relation; neither owner's identifier is renamed.
    pub world_ref: ResourceRef,
    pub central_scope_ref: ResourceRef,
    pub cwd: PathBuf,
    pub provider: String,
    pub workcell_boundary_bin: PathBuf,
    pub workcell_bin: PathBuf,
    pub workcell_ref: ResourceRef,
    /// Intended receiving destination survives restart, independently of a view.
    pub return_ref: ResourceRef,
    #[serde(default)]
    pub working_copy: Option<DevelopmentFieldGitBasis>,
}

pub(super) struct PreparedEncounterTask {
    pub binding: EncounterTaskBinding,
    pub allocation: AllocatedTask,
    pub boundary: Value,
    pub storage: Value,
    // Retain the exact non-writable input until the resident has stopped.
    _requirements_file: tempfile::NamedTempFile,
    pub argv: Vec<String>,
}
impl PreparedEncounterTask {
    pub fn prompt(&self, text: &str) -> String {
        let facts=json!({"task_ref":self.binding.task.task_ref,"purpose":self.binding.task.purpose,
            "selected_source_refs":self.binding.task.source_refs,"now_ref":self.allocation.basis.allocation_ref,
            "now_destination":self.allocation.basis.now,"cwd":self.binding.cwd,
            "policy_ref":self.allocation.basis.policy_ref,"policy_revision":self.allocation.basis.policy_revision,
            "return_ref":self.binding.return_ref,"working_copy":self.binding.working_copy,"material_world":self.storage["receipt_world"]["world_ref"]});
        format!("<native-task-basis>\n{facts}\n</native-task-basis>\nThese native bounds do not grant permission to change human source or perform Recognition.\n\n{text}")
    }
    pub fn snapshot(&self) -> Value {
        json!({"binding":self.binding,"allocation":self.allocation,"boundary":self.boundary,"storage":self.storage,
            "enforcement":"native-workcell-exec-before-provider-start", "live_revocation":false})
    }
}
fn binding_path(home: &AikitHome, session: &ResourceRef) -> PathBuf {
    home.state().join("encounter-tasks").join(format!("{}.json",blake3::hash(session.as_str().as_bytes()).to_hex()))
}
fn read_binding(home: &AikitHome, session: &ResourceRef) -> Result<Option<EncounterTaskBinding>> {
    let path=binding_path(home,session);
    let metadata=match std::fs::symlink_metadata(&path) {
        Ok(metadata)=>metadata,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
        Err(e)=>return Err(error(e)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len()>1024*1024 {
        return Err(error("Task binding must be a bounded non-redirected owner file"));
    }
    serde_json::from_slice(&std::fs::read(path).map_err(error)?).map(Some).map_err(error)
}
fn stopped(message: &str) -> AikitError {
    AikitError::new("encounter.task_readmission_required",message)
}
impl EncounterService {
    /// Explicit readmission replaces only the selected resident. A normal open
    /// or turn never renews its material grant or changes its native body.
    pub(super) fn reconnect_native(
        &self,
        space: aikit_core::session_space::SessionSpaceRef,
        session: ResourceRef,
        provider: String,
        cwd: PathBuf,
    ) -> Result<Value> {
        use super::{EncounterProtocol, Lifecycle};
        use std::sync::{atomic::Ordering, Arc};

        // All public operations hold a shared lease. Exclusive ownership makes
        // process replacement atomic with prompts, consent and owner shutdown.
        let mut lifecycle = self.lifecycle.write().map_err(error)?;
        if self.shutdown_requested.load(Ordering::SeqCst)
            || !matches!(*lifecycle, Lifecycle::Running)
        {
            return Err(AikitError::new(
                "encounter.owner_stopped",
                "The encounter owner is shutting down or stopped",
            ));
        }
        let cwd = cwd.canonicalize().map_err(error)?;
        let mut residents = self.residents.lock().map_err(error)?;
        if let Some(held) = residents.get(&session) {
            if held.space != space || held.provider != provider || held.cwd != cwd {
                return Err(stopped("Readmission must retain the recorded space, provider and cwd"));
            }
            if held.protocol != EncounterProtocol::Acp {
                return Err(AikitError::new(
                    "encounter.reconnect_unsupported",
                    "This provider has no native load operation; the existing resident was not stopped",
                ));
            }
            if format!("{:?}", held.host.identity(&session)?.state) == "TurnInFlight" {
                return Err(AikitError::new(
                    "encounter.readmission_busy",
                    "Finish or explicitly cancel the active turn before replacing its material body",
                ));
            }
            self.store.append(&session, &json!({
                "kind":"resident-readmission-requested",
                "native_session_id":held.lane.binding().native_session_id,
                "connection_generation":held.generation,
                "reason":"explicit reconnect; no automatic grant renewal",
            }))?;
            let held = residents.remove(&session).expect("resident checked under exclusive lease");
            let resident = match Arc::try_unwrap(held) {
                Ok(resident) => resident,
                Err(held) => {
                    residents.insert(session.clone(), held);
                    return Err(stopped("Resident is still borrowed; no replacement process was started"));
                }
            };
            let native = resident.lane.binding().native_session_id.clone();
            let generation = resident.generation.clone();
            let status = match resident.host.shutdown() {
                Ok(status) => status,
                Err(failure) => {
                    // Never start a second body when stopping the first one is
                    // uncertain. Preserve the failed owner state for diagnosis.
                    let reason = format!("{session}: readmission cleanup failed: {failure}");
                    *lifecycle = Lifecycle::Failed(reason.clone());
                    return Err(AikitError::new("encounter.shutdown_failed", reason));
                }
            };
            self.permissions.lock().map_err(error)?.remove(&session);
            self.store.append(&session, &json!({
                "kind":"resident-readmission-stopped",
                "native_session_id":native,
                "connection_generation":generation,
                "process_status":status.map(|s|s.to_string()),
                "process_stopped":true,
                "replacement_started":false,
            }))?;
        }
        drop(residents);
        // This is the existing launch path, including current Agency, Central
        // policy/NOW, Workcell storage and protocol-preserving confinement.
        // A refusal leaves the old body stopped; there is no Direct fallback.
        self.open_native(space, session, provider, cwd, true)
    }

    pub fn configure_task(home:&AikitHome, session:&ResourceRef, binding:&EncounterTaskBinding,
        expected_revision:Option<&SourceRevision>) -> Result<Value> {
        let service=Self::new(home.clone())?;
        service.require_attached(session)?;
        let _lock=service.lock_agency(session)?;
        let current=read_binding(home,session)?;
        if current.as_ref().map(|b|&b.revision)!=expected_revision
            || current.as_ref().is_some_and(|b|b.revision==binding.revision) {
            return Err(stopped("Task configuration needs the current revision and a distinct new revision"));
        }
        if current.as_ref().is_some_and(|b|b.task.task_ref!=binding.task.task_ref || b.return_ref!=binding.return_ref) {
            return Err(stopped("Do not silently retask a canonical session or redirect its Return; use a separately attributed session"));
        }
        if !binding.cwd.is_absolute() || binding.cwd.canonicalize().map_err(error)? != binding.cwd
            || !binding.cwd.is_dir() || !binding.workcell_boundary_bin.is_absolute() || !binding.workcell_bin.is_absolute() {
            return Err(error("Task requires an exact canonical cwd and an explicit material executable"));
        }
        let admission=if binding.active {
            let (agency,_)=service.check_agency(session)?.ok_or_else(||stopped("A continuous task needs its selected native Agency, not a Profile"))?;
            if binding.world_ref != agency.world_ref || !binding.task.participant_refs.contains(&agency.agent_ref) {
                return Err(stopped("Task participants do not include the selected native Agent"));
            }
            verify_working_copy(binding, true)?;
            let owner=CentralPlacement::new(SystemRunner::new(),binding.task.clone())?;
            let allocation=owner.allocate()?;
            if allocation.allocation["record"]["scope_ref"]!=json!(binding.central_scope_ref) {
                return Err(stopped("Central returned another scope than the explicitly composed task World"));
            }
            let decision=owner.validate(&allocation,&binding.cwd,&binding.cwd)?;
            if decision["allowed"]!=true { return Err(stopped("Task cwd is not an authorised engineering/NOW destination")); }
            let boundary=prepare_boundary(&SystemRunner::new(),&binding.workcell_boundary_bin,&allocation)?;
            Some(json!({"allocation":allocation,"validation":decision,"boundary":boundary,"executed":false}))
        } else {None};
        let path=binding_path(home,session);
        let parent=path.parent().expect("task parent");
        std::fs::create_dir_all(parent).map_err(error)?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent,std::fs::Permissions::from_mode(0o700)).map_err(error)?;
        }
        let mut file=tempfile::NamedTempFile::new_in(parent).map_err(error)?;
        file.write_all(&serde_json::to_vec_pretty(binding).map_err(error)?).map_err(error)?;
        file.as_file().sync_all().map_err(error)?;
        file.persist(&path).map_err(error)?;
        std::fs::File::open(parent).and_then(|file|file.sync_all()).map_err(error)?;
        Ok(json!({"configured":true,"task":binding,"admission":admission,"executed":false}))
    }

    pub(super) fn prepare_task(&self,session:&ResourceRef,provider:&str,cwd:&Path,argv:&[String])
        -> Result<Option<PreparedEncounterTask>> {
        let current=read_binding(&self.home,session)?;
        let previous=self.store.last_native_binding(session)?;
        // A removed binding is not a transition back to ungoverned Direct work.
        if previous.as_ref().is_some_and(|p|!p["task"].is_null())
            && current.as_ref().is_none_or(|b|previous.as_ref().unwrap()["task"]["binding"]["task"]["task_ref"]!=json!(b.task.task_ref)
                || previous.as_ref().unwrap()["task"]["binding"]["return_ref"]!=json!(b.return_ref)) {
            return Err(stopped("Recorded task/Return binding was removed or changed; no Direct fallback is permitted"));
        }
        let Some(binding)=current else {return Ok(None)};
        if !binding.active || binding.cwd!=cwd || binding.provider!=provider {
            return Err(stopped("Task is withdrawn or the requested cwd/provider differs from its admitted execution basis"));
        }
        let (agency,_)=self.check_agency(session)?.ok_or_else(||stopped("Selected Agency admission is missing"))?;
        if binding.world_ref != agency.world_ref || !binding.task.participant_refs.contains(&agency.agent_ref) {
            return Err(stopped("Selected Agent changed outside the task participation basis"));
        }
        // Native resume may contain the task's retained edits; the base and
        // actual worktree identity must still be unchanged.
        verify_working_copy(&binding, previous.is_none())?;
        let owner=CentralPlacement::new(SystemRunner::new(),binding.task.clone())?;
        let allocation=owner.allocate()?;
        if allocation.allocation["record"]["scope_ref"]!=json!(binding.central_scope_ref) {
            return Err(stopped("Central returned another scope than the explicitly composed task World"));
        }
        if owner.validate(&allocation,cwd,cwd)?["allowed"]!=true {
            return Err(stopped("Current Central policy does not permit this execution cwd"));
        }
        let boundary=prepare_boundary(&SystemRunner::new(),&binding.workcell_boundary_bin,&allocation)?;
        let root=self.home.state().join("encounter-task-material");
        std::fs::create_dir_all(&root).map_err(error)?;
        let mut file=tempfile::NamedTempFile::new_in(&root).map_err(error)?;
        file.write_all(boundary["requirements"].to_string().as_bytes()).map_err(error)?;
        file.as_file().sync_all().map_err(error)?;
        let mut native_argv=vec![binding.workcell_boundary_bin.to_string_lossy().into_owned(),"exec".into(),
            file.path().to_string_lossy().into_owned(),allocation.basis.policy_revision.to_string(),
            boundary["requirements_digest"].as_str().ok_or_else(||error("Missing native boundary digest"))?.to_owned(),"--".into()];
        native_argv.extend_from_slice(argv);
        let storage=storage_attachment(&self.home,session,&binding,&allocation)?;
        let prepared=PreparedEncounterTask {binding,allocation,boundary,storage,_requirements_file:file,argv:native_argv};
        self.store.append(session,&json!({"kind":"task-prepared","task":prepared.snapshot(),"executed":false}))?;
        Ok(Some(prepared))
    }

    pub(super) fn check_resident_task(&self,session:&ResourceRef,resident:&Resident,phase:&str)->Result<()> {
        let result = self.validate_resident_task(session, resident, phase);
        if let Err(failure) = &result {
            self.store.append(session, &json!({
                "kind":"task-admission-refused", "phase":phase,
                "code":failure.code(), "reason":failure.to_string(),
                "task_ref":resident.task.as_ref().map(|task|&task.binding.task.task_ref),
                "connection_generation":resident.generation, "effect_admitted":false,
            }))?;
        }
        result
    }

    fn validate_resident_task(&self,session:&ResourceRef,resident:&Resident,phase:&str)->Result<()> {
        let current=read_binding(&self.home,session)?;
        match (&resident.task,current) {
            (None,None)=>Ok(()),
            (Some(prepared),Some(binding)) if prepared.binding==binding && binding.active=>{
                let now=SystemTime::now().duration_since(UNIX_EPOCH).map_err(error)?.as_millis();
                if prepared.boundary["requirements"]["expires_at_unix_ms"].as_u64().is_none_or(|t|u128::from(t)<=now) {
                    return Err(stopped("Resident material lease expired; explicitly re-resolve and reconnect rather than silently renew it"));
                }
                let working_copy=verify_working_copy(&binding, false)?;
                let owner=CentralPlacement::new(SystemRunner::new(),binding.task.clone())?;
                let current=owner.allocate()?;
                if current.basis!=prepared.allocation.basis || current.now_revision!=prepared.allocation.now_revision {
                    return Err(stopped("Policy/NOW changed after provider admission; do not continue on the old material grant"));
                }
                if owner.validate(&current,&resident.cwd,&resident.cwd)?["allowed"]!=true {
                    return Err(stopped("Current task no longer permits this working copy"));
                }
                let boundary=prepare_boundary(&SystemRunner::new(),&binding.workcell_boundary_bin,&current)?;
                if boundary["objects"]!=prepared.boundary["objects"] || boundary["protected_objects"]!=prepared.boundary["protected_objects"] {
                    return Err(stopped("Native material object identity changed; re-resolve the body"));
                }
                let storage=storage_attachment(&self.home,session,&binding,&current)?;
                if storage["receipt_world"]["world_ref"]!=prepared.storage["receipt_world"]["world_ref"] {
                    return Err(stopped("NOW storage material identity changed; this resident was not admitted to its replacement"));
                }
                self.store.append(session,&json!({"kind":"task-admission-checked","phase":phase,
                    "task_ref":binding.task.task_ref,"now_ref":current.basis.allocation_ref,
                    "policy_revision":current.basis.policy_revision,"return_ref":binding.return_ref,
                    "resident_boundary_digest":prepared.boundary["requirements_digest"],"working_copy":working_copy,"live_revocation":false}))?;
                Ok(())
            }
            _=>Err(stopped("Task admission was added, removed, withdrawn or changed; the resident cannot silently change its execution basis")),
        }
    }
}

/// Bind only existing NOW storage through Workcell's published native CLI.
/// The declaration is private owner configuration, not a second NOW or a claim
/// of confinement. Actual prepare + independent inspect/observe are mandatory.
fn storage_attachment(home:&AikitHome,session:&ResourceRef,binding:&EncounterTaskBinding,
    allocation:&AllocatedTask)->Result<Value> {
    use aikit_adapters::runner::CommandRunner;
    let key=blake3::hash(session.as_str().as_bytes()).to_hex().to_string();
    let state=home.state().join("encounter-workcell-storage").join(&key);
    std::fs::create_dir_all(&state).map_err(error)?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&state,std::fs::Permissions::from_mode(0o700)).map_err(error)?;
    }
    let declaration=json!({"schema":"workcell.directory-storage/v1","directories":[
        {"logical_ref":allocation.basis.allocation_ref,"path":allocation.basis.now}]});
    let path=state.join("storage.json");
    if path.exists() {
        if std::fs::symlink_metadata(&path).map_err(error)?.file_type().is_symlink()
            || serde_json::from_slice::<Value>(&std::fs::read(&path).map_err(error)?).map_err(error)?!=declaration {
            return Err(stopped("Existing native storage declaration differs; do not overwrite or relocate it"));
        }
    } else {
        let mut file=tempfile::NamedTempFile::new_in(&state).map_err(error)?;
        file.write_all(declaration.to_string().as_bytes()).map_err(error)?;
        file.as_file().sync_all().map_err(error)?;
        file.persist_noclobber(&path).map_err(error)?;
    }
    let demand_ref=format!("demand:aikit-now:{key}");
    let subjects=json!({"task":binding.task.task_ref,"agent_session":session,
        "now":allocation.basis.allocation_ref,"source":allocation.allocation["source"]["ref"]});
    let receipt=state.join("now-receipt.json");
    let argv=vec![binding.workcell_bin.to_string_lossy().into_owned(),"--json".into(),
        "--state-root".into(),state.to_string_lossy().into_owned(),"--workcell-ref".into(),binding.workcell_ref.to_string(),
        "--receipt".into(),receipt.to_string_lossy().into_owned()];
    let invoke=|operation:Vec<String>|->Result<Value> {
        let mut command=argv.clone();command.extend(operation);
        let output=SystemRunner::new().run(&command)?;
        if !output.ok() || output.stdout.len()>4*1024*1024 {
            return Err(stopped("Native Workcell storage operation failed; inspect its retained receipt, never substitute another directory"));
        }
        let result:Value=serde_json::from_str(&output.stdout).map_err(error)?;
        if result["ok"]!=true {return Err(stopped("Native Workcell refused the storage operation"));}
        Ok(result)
    };
    if !receipt.exists() {
        let tier=json!({"required":[],"preferred":[],"optional":[]});
        let mut demand=json!({"demand_ref":demand_ref,"subjects":subjects,"workspace":null,
            "project_runtime":null,"resources":[],"persistence":null,"isolation_trust":null,
            "retention":"preserve","extensions":{},"affordances":tier,"connectivity":tier,
            "exposure":tier,"outputs":tier,"storage":tier});
        demand["storage"]["required"]=json!([{"logical_ref":allocation.basis.allocation_ref,
            "access":"writable","sharing":"shared","minimum_capacity":null,"unit":null,
            "persistence":"external","retention":"preserve"}]);
        let mut file=tempfile::NamedTempFile::new_in(&state).map_err(error)?;
        file.write_all(demand.to_string().as_bytes()).map_err(error)?;
        file.as_file().sync_all().map_err(error)?;
        invoke(vec!["prepare".into(),"--demand-json".into(),file.path().to_string_lossy().into_owned()])?;
    }
    let reading=invoke(vec!["inspect".into()])?;
    let world=&reading["receipt_world"];
    let bindings=world["binding_graph"]["bindings"].as_array().ok_or_else(||stopped("Native storage readback has no binding graph"))?;
    if reading["contract"]!="workcell.material-reading/v1" || world["version"]!="workcell.material-world/v1"
        || world["workcell_ref"]!=json!(binding.workcell_ref) || world["demand_ref"]!=demand_ref
        || world["subjects"]!=subjects || world["state"]!="healthy" || bindings.len()!=1 {
        return Err(stopped("Workcell storage readback differs from the exact task/NOW/material demand"));
    }
    let storage=&bindings[0];
    if storage["port"]!="storage" || storage["presence"]!="present" || storage["health"]!="healthy"
        || storage["properties"]["logical_ref"]!=json!(allocation.basis.allocation_ref)
        || storage["properties"]["path"]!=json!(allocation.basis.now)
        || reading["observation"]["status"]!="supplied" || reading["observation"]["reading"]["ok"]!=true
        || reading["observation"]["reading"]["observations"].as_array().is_none_or(|rows|
            !rows.iter().any(|row|row["logical_ref"]==storage["logical_ref"] && row["state"]=="healthy")) {
        return Err(stopped("Workcell did not observe the exact allocated NOW as healthy existing storage"));
    }
    Ok(reading)
}

/// Reuse the native Git/VersionedWorld reading. A path or branch label alone
/// cannot identify the requested execution basis. Later task edits are returned
/// as current diff, not confused with the immutable selected base.
fn verify_working_copy(binding:&EncounterTaskBinding, initial:bool)->Result<Option<DevelopmentFieldGitBasis>> {
    let Some(expected)=&binding.working_copy else {return Ok(None)};
    let base=expected.base_revision.as_ref().ok_or_else(||stopped("A versioned task needs its explicit exact base revision"))?;
    for revision in [base, &expected.world.repository.head] {
        if !matches!(revision.as_str().len(),40|64) || !revision.as_str().bytes().all(|b|b.is_ascii_hexdigit()) {
            return Err(stopped("Use exact native Git revisions, not a moving branch or HEAD alias"));
        }
    }
    let current=aikit_adapters::NativeGitProvider::new()?.development_field_basis(
        &expected.world.project, binding.cwd.to_str().ok_or_else(||stopped("Git cwd must be UTF-8"))?,
        Some(base.clone()),4*1024*1024)?;
    if current.world.repository.repository_root!=expected.world.repository.repository_root
        || current.world.repository.worktree_root!=expected.world.repository.worktree_root
        || current.world.repository.head!=expected.world.repository.head
        || current.world.provider.provider!=expected.world.provider.provider {
        return Err(stopped("Native Git reports a different repository/worktree/HEAD than the selected task basis"));
    }
    if initial && (current.current_diff_from_base!=expected.current_diff_from_base
        || !current.world.working.untracked.is_empty()
        || current.current_diff_from_base.as_ref().is_none_or(|d|d.truncated)) {
        return Err(stopped("Initial source differs from its complete captured tracked basis; untracked content needs explicit capture before admission"));
    }
    Ok(Some(current))
}
