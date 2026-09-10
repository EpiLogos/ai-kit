//! Native Central placement/NOW consumer. No policy, path or NOW is invented
//! here: the actual public owner operations supply and revalidate every basis.
use crate::runner::CommandRunner;
use aikit_core::{AikitError, ResourceRef, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentralTaskRequest {
    pub ctrl_bin: PathBuf,
    pub central_root: PathBuf,
    pub project: Option<String>,
    pub task_ref: ResourceRef,
    pub purpose: String,
    #[serde(default)]
    pub participant_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub source_refs: Vec<ResourceRef>,
}

/// Retains the complete native reading, including source/authority bases,
/// permitted destinations, NOW source revision, lifecycle and policy expiry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllocatedCentralTask {
    pub request: CentralTaskRequest,
    pub allocation: Value,
}

pub struct NativeCentralPlacement<R> {
    runner: R,
}
impl<R: CommandRunner> NativeCentralPlacement<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    fn call(&self, request: &CentralTaskRequest, action: &str, mut input: Value) -> Result<Value> {
        if !request.ctrl_bin.is_absolute()
            || !request.central_root.is_absolute()
            || request.purpose.trim().is_empty()
            || request.purpose.len() > 16384
        {
            return Err(failure("configuration", "Expected explicit native binary, World root and bounded task purpose"));
        }
        if let Some(project) = &request.project {
            input["project"] = json!(project);
        }
        let output = self.runner.run(&[
            request.ctrl_bin.to_string_lossy().into_owned(),
            "--json".into(),
            "--root".into(),
            request.central_root.to_string_lossy().into_owned(),
            "action".into(),
            "run".into(),
            action.into(),
            input.to_string(),
        ])?;
        if output.stdout.len() > 4 * 1024 * 1024 {
            return Err(failure("owner_response", "Native Central response exceeds the 4 MiB limit"));
        }
        let envelope: Value = serde_json::from_str(&output.stdout)
            .map_err(|_| failure("owner_response", "Native Central did not return an ActionResult"))?;
        if !output.ok() || envelope["ok"] != true || envelope["action"] != action {
            return Err(failure("owner_refused", &format!("{action} refused or returned an unrelated receipt; no fallback or automatic retry"))
                .with("native_receipt", envelope.to_string()));
        }
        envelope.get("data").filter(|v| v.is_object()).cloned()
            .ok_or_else(|| failure("owner_response", "Native ActionResult has no object data"))
    }

    pub fn allocate(&self, request: &CentralTaskRequest) -> Result<AllocatedCentralTask> {
        let policy = self.call(request, "central.work.policy", json!({}))?;
        check_policy(&policy)?;
        let allocation = self.call(request, "central.now.allocate", json!({
            "task_ref": request.task_ref, "purpose": request.purpose,
            "participant_refs": request.participant_refs, "source_refs": request.source_refs,
            "expected_policy_revision": policy["revision"],
        }))?;
        if allocation["schema"] != "central.now-allocation/v1"
            || allocation["record"]["task_ref"] != json!(request.task_ref)
            || allocation["record"]["purpose"] != request.purpose
            || allocation["record"]["participant_refs"] != json!(request.participant_refs)
            || allocation["record"]["source_refs"] != json!(request.source_refs)
            || allocation["record"]["lifecycle"] != "active"
            || allocation["policy"]["revision"] != policy["revision"]
            || allocation["policy"]["scope_ref"] != policy["scope_ref"]
            || allocation["record"]["scope_ref"] != policy["scope_ref"]
            || allocation["record"]["now_ref"] != allocation["now_ref"]
            || allocation["record"]["source_ref"] != allocation["source"]["ref"]
        {
            return Err(failure("allocation_mismatch", "Central allocation does not match the selected task and current World/policy"));
        }
        text(&allocation, "/now_ref")?;
        text(&allocation, "/source/ref")?;
        text(&allocation, "/revision/revision")?;
        let task = AllocatedCentralTask { request: request.clone(), allocation };
        let now = task.now_directory()?;
        if !now.is_absolute() || !now.is_dir() || now.canonicalize().map_err(io_error)? != now {
            return Err(failure("allocation_mismatch", "Central must allocate a real canonical task destination"));
        }
        self.revalidate(&task)?;
        Ok(task)
    }

    /// A continuation reads the allocated source; it cannot mint a replacement
    /// NOW, reactivate an archived task, refresh a stale policy or renew a lease.
    pub fn revalidate(&self, task: &AllocatedCentralTask) -> Result<Value> {
        check_policy(&task.allocation["policy"])?;
        let policy = self.call(&task.request, "central.work.policy", json!({}))?;
        check_policy(&policy)?;
        if policy["revision"] != task.allocation["policy"]["revision"] {
            return Err(failure("policy_changed", "Placement policy changed; explicitly re-resolve before another effect"));
        }
        let current = self.call(&task.request, "central.now.read", json!({"now_ref": task.allocation["now_ref"]}))?;
        if current["schema"] != "central.now-reading/v1"
            || current["record"]["now_ref"] != task.allocation["now_ref"]
            || current["record"]["task_ref"] != json!(task.request.task_ref)
            || current["record"]["lifecycle"] != "active"
            || current["source"]["ref"] != task.allocation["source"]["ref"]
            || current["revision"]["revision"] != task.allocation["revision"]["revision"]
        {
            return Err(failure("now_changed", "Task NOW was changed, withdrawn or archived; retain its identity and explicitly re-enter"));
        }
        Ok(current)
    }

    pub fn validate_write(&self, task: &AllocatedCentralTask, destination: &Path) -> Result<Value> {
        self.revalidate(task)?;
        let result = self.call(&task.request, "central.work.validate", json!({
            "now_ref": task.allocation["now_ref"],
            "expected_now_revision": task.allocation["revision"]["revision"],
            "expected_policy_revision": task.allocation["policy"]["revision"],
            "destination": destination,
        }))?;
        if result["schema"] != "central.work-placement-validation/v1"
            || result["allowed"] != true
            || result["now_ref"] != task.allocation["now_ref"]
            || result["now_revision"] != task.allocation["revision"]["revision"]
            || result["policy_revision"] != task.allocation["policy"]["revision"]
        {
            return Err(failure("validation_mismatch", "Native write decision is not for this exact NOW/policy basis"));
        }
        Ok(result)
    }

    /// Narrow explicitly selected directory grants, not every directory in a
    /// repository. Workcell must still prepare and enforce this exact request.
    /// Every native protected path and coverage requirement survives unchanged;
    /// unsupported holes/objects are Workcell refusals, never filtered here.
    pub fn write_boundary_requirements(
        &self,
        task: &AllocatedCentralTask,
        authority: &ResourceRef,
        selected_directories: &[PathBuf],
    ) -> Result<Value> {
        self.revalidate(task)?;
        if selected_directories.len() > 63 {
            return Err(failure("material_bounds", "At most 63 selected directories plus the native task NOW"));
        }
        let now = task.now_directory()?;
        let mut writable = vec![now.clone()];
        for path in selected_directories {
            if !path.is_absolute() || !path.is_dir() || path.canonicalize().map_err(io_error)? != *path {
                return Err(failure("material_bounds", "Writable directory must exist with its exact canonical identity"));
            }
            if path != &now {
                self.validate_write(task, path)?;
            }
            if !writable.contains(path) {
                writable.push(path.clone());
            }
        }
        let policy = &task.allocation["policy"];
        let policy_source = policy["sources"].as_array().and_then(|s| s.last())
            .ok_or_else(|| failure("owner_response", "Effective policy has no native source basis"))?;
        let policy_ref = text(policy_source, "/source/ref")?;
        let expiry = policy["expires_at_unix_seconds"].as_u64()
            .and_then(|s| s.checked_mul(1000))
            .ok_or_else(|| failure("material_bounds", "Policy expiry cannot be represented in milliseconds"))?;
        Ok(json!({
            "schema": "workcell.write-boundary/v1",
            "policy_ref": policy_ref, "policy_revision": policy["revision"],
            "authority_ref": authority, "writable_paths": writable,
            "protected_paths": policy["protected_paths"],
            "required_coverage": policy["required_coverage"],
            "expires_at_unix_ms": expiry,
        }))
    }
}
impl AllocatedCentralTask {
    pub fn now_directory(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(text(&self.allocation, "/writable_destination")?))
    }
    pub fn storage_declaration(&self) -> Result<Value> {
        Ok(json!({"schema": "workcell.directory-storage/v1", "directories": [{
            "logical_ref": text(&self.allocation, "/now_ref")?, "path": self.now_directory()?,
        }]}))
    }
    pub fn storage_requirement(&self) -> Result<Value> {
        Ok(json!({"logical_ref": text(&self.allocation, "/now_ref")?,
            "access": "writable", "sharing": "shared", "minimum_capacity": null,
            "unit": null, "persistence": "external", "retention": "preserve"}))
    }
}
fn check_policy(policy: &Value) -> Result<()> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io_error)?.as_secs();
    if policy["schema"] != "central.effective-placement-policy/v1"
        || policy["expires_at_unix_seconds"].as_u64().is_none_or(|t| t <= now)
        || policy["sources"].as_array().is_none_or(Vec::is_empty)
        || !policy["protected_paths"].is_array()
        || policy["required_coverage"].as_array().is_none_or(Vec::is_empty)
        || !matches!(policy["enforcement"].as_str(), Some("native-actions" | "harness-interception" | "material-filesystem"))
    {
        return Err(failure("policy_unavailable", "Current recognised placement source, bounded lease and explicit coverage are required"));
    }
    text(policy, "/revision")?;
    Ok(())
}
fn text<'a>(value: &'a Value, pointer: &str) -> Result<&'a str> {
    value.pointer(pointer).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
        .ok_or_else(|| failure("owner_response", &format!("Native receipt lacks {pointer}")))
}
fn failure(kind: &str, message: &str) -> AikitError {
    AikitError::new(format!("placement.central_{kind}"), message)
}
fn io_error(error: impl std::fmt::Display) -> AikitError {
    failure("io", &error.to_string())
}
