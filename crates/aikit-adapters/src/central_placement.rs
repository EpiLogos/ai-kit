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
    /// Explicit native ancestry; absent fields preserve legacy owner relations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_now_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workcell_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub work_refs: Vec<Value>,
}

/// Retains the complete native reading, including source/authority bases,
/// permitted destinations, NOW source revision, lifecycle and policy expiry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllocatedCentralTask {
    pub request: CentralTaskRequest,
    pub allocation: Value,
}

fn validate_declared_relations(request: &CentralTaskRequest, record: &Value) -> Result<()> {
    if request
        .parent_now_ref
        .as_ref()
        .is_some_and(|parent| record["parent_now_ref"] != json!(parent))
        || request
            .workcell_ref
            .as_ref()
            .is_some_and(|workcell| record["workcell_ref"] != json!(workcell))
        || (!request.work_refs.is_empty() && record["work_refs"] != json!(request.work_refs))
    {
        return Err(failure("allocation_mismatch", "Native NOW does not retain the explicitly selected parent, Workcell and work relations"));
    }
    Ok(())
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
            return Err(failure(
                "configuration",
                "Expected explicit native binary, World root and bounded task purpose",
            ));
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
            return Err(failure(
                "owner_response",
                "Native Central response exceeds the 4 MiB limit",
            ));
        }
        let envelope: Value = serde_json::from_str(&output.stdout).map_err(|_| {
            failure(
                "owner_response",
                "Native Central did not return an ActionResult",
            )
        })?;
        if !output.ok() || envelope["ok"] != true || envelope["action"] != action {
            return Err(failure("owner_refused", &format!("{action} refused or returned an unrelated receipt; no fallback or automatic retry"))
                .with("native_receipt", envelope.to_string()));
        }
        envelope
            .get("data")
            .filter(|v| v.is_object())
            .cloned()
            .ok_or_else(|| failure("owner_response", "Native ActionResult has no object data"))
    }

    /// Reuse immutable relationships only from the actual owner of this exact
    /// Task and scope. A legacy client request has no relationship declaration;
    /// it must not erase an already-allocated child NOW's native ancestry.
    /// This is a read, not a new allocation or an amendment of the request.
    fn existing_now_basis(
        &self,
        request: &CentralTaskRequest,
        policy: &Value,
    ) -> Result<Option<Value>> {
        let listing = self.call(request, "central.now.list", json!({}))?;
        if listing["schema"] != "central.now-listing/v1" {
            return Err(failure(
                "owner_response",
                "Central did not return a NOW listing",
            ));
        }
        let rows = listing["records"]
            .as_array()
            .ok_or_else(|| failure("owner_response", "Central NOW listing has no record array"))?;
        let mut matches = rows
            .iter()
            .filter(|row| row["task_ref"] == json!(request.task_ref));
        let Some(row) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() || row["scope_ref"] != policy["scope_ref"] {
            return Err(failure(
                "allocation_mismatch",
                "Task NOW is ambiguous or outside the selected World",
            ));
        }
        let now_ref = text(row, "/now_ref")?;
        let current = self.call(request, "central.now.read", json!({"now_ref": now_ref}))?;
        let record = &current["record"];
        if current["schema"] != "central.now-reading/v1"
            || record["now_ref"] != row["now_ref"]
            || record["scope_ref"] != policy["scope_ref"]
            || record["task_ref"] != json!(request.task_ref)
            || record["purpose"] != request.purpose
            || record["participant_refs"] != json!(request.participant_refs)
            || record["source_refs"] != json!(request.source_refs)
            || record["lifecycle"] != "active"
            || record["source_ref"] != current["source"]["ref"]
            || current["source"]["ref"] != row["source_ref"]
            || current["revision"]["revision"] != row["revision"]["revision"]
        {
            return Err(failure("allocation_mismatch", "Existing NOW does not retain this exact Task intent, scope, source and active basis"));
        }
        validate_declared_relations(request, record)?;
        // The native allocate operation derives child from parent_now_ref.
        // A Workcell root belongs to central.now.workcell-root and cannot be
        // replayed as an ordinary Task allocation by this consumer.
        let expected_horizon = if record["parent_now_ref"].is_null() {
            Value::Null
        } else {
            json!("child")
        };
        if record["horizon"] != expected_horizon {
            return Err(failure(
                "allocation_mismatch",
                "Existing NOW requires a different native horizon owner operation",
            ));
        }
        text(&current, "/source/ref")?;
        text(&current, "/revision/revision")?;
        if record.get("work_refs").is_some_and(|refs| !refs.is_array()) {
            return Err(failure(
                "owner_response",
                "Native NOW work_refs is not an array",
            ));
        }
        Ok(Some(current))
    }

    pub fn allocate(&self, request: &CentralTaskRequest) -> Result<AllocatedCentralTask> {
        // Native NOW work relations are repository/branch objects. Semantic
        // Run, workflow-unit and Return refs remain in source_refs.
        if request.work_refs.len() > 64
            || request.work_refs.iter().any(|work| {
                !work.is_object()
                    || ["repo", "branch"].iter().any(|key| {
                        work.get(*key)
                            .and_then(Value::as_str)
                            .is_none_or(|text| text.trim().is_empty())
                    })
            })
        {
            return Err(failure("configuration", "Native work_refs require bounded repository/branch objects; semantic refs belong in source_refs"));
        }
        let policy = self.call(request, "central.work.policy", json!({}))?;
        check_policy(&policy)?;
        let existing = self.existing_now_basis(request, &policy)?;
        let mut input = json!({
            "task_ref": request.task_ref, "purpose": request.purpose,
            "participant_refs": request.participant_refs, "source_refs": request.source_refs,
            "expected_policy_revision": policy["revision"],
        });
        if let Some(current) = &existing {
            // Retain this owner's immutable relationship facts. Never infer a
            // parent or material placement from participant names or a path.
            for key in ["work_refs", "parent_now_ref", "workcell_ref"] {
                if let Some(value) = current["record"].get(key) {
                    input[key] = value.clone();
                }
            }
        }
        if let Some(parent) = &request.parent_now_ref {
            input["parent_now_ref"] = json!(parent);
        }
        if let Some(workcell) = &request.workcell_ref {
            input["workcell_ref"] = json!(workcell);
        }
        if !request.work_refs.is_empty() {
            input["work_refs"] = json!(request.work_refs);
        }
        let allocation = self.call(request, "central.now.allocate", input)?;
        validate_declared_relations(request, &allocation["record"])?;
        if let Some(current) = &existing {
            if allocation["created"] != false
                || allocation["now_ref"] != current["record"]["now_ref"]
                || allocation["source"]["ref"] != current["source"]["ref"]
                || allocation["revision"]["revision"] != current["revision"]["revision"]
                || ["work_refs", "parent_now_ref", "workcell_ref", "horizon"]
                    .iter()
                    .any(|key| allocation["record"][*key] != current["record"][*key])
            {
                return Err(failure("allocation_mismatch", "Task NOW changed between read and native allocation replay; retain the pending Task"));
            }
        }
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
            return Err(failure(
                "allocation_mismatch",
                "Central allocation does not match the selected task and current World/policy",
            ));
        }
        text(&allocation, "/now_ref")?;
        text(&allocation, "/source/ref")?;
        text(&allocation, "/revision/revision")?;
        let task = AllocatedCentralTask {
            request: request.clone(),
            allocation,
        };
        let now = task.now_directory()?;
        if !now.is_absolute() || !now.is_dir() || now.canonicalize().map_err(io_error)? != now {
            return Err(failure(
                "allocation_mismatch",
                "Central must allocate a real canonical task destination",
            ));
        }
        self.revalidate(&task)?;
        Ok(task)
    }

    /// Close only a clearing this failed preparation actually created. The
    /// native owner retains its history/artifacts and checks exact revisions.
    pub fn close_new_allocation(&self, task: &AllocatedCentralTask) -> Result<Option<Value>> {
        if task.allocation["created"] != true {
            return Ok(None);
        }
        let receipt = self.call(&task.request, "central.now.lifecycle", json!({
            "now_ref":task.allocation["now_ref"], "expected_revision":task.allocation["revision"]["revision"],
            "expected_policy_revision":task.allocation["policy"]["revision"], "lifecycle":"closed",
        }))?;
        if receipt["schema"] != "central.now-lifecycle/v1"
            || receipt["record"]["now_ref"] != task.allocation["now_ref"]
            || receipt["record"]["lifecycle"] != "closed"
        {
            return Err(failure(
                "cleanup_mismatch",
                "Central did not confirm the newly allocated clearing closed",
            ));
        }
        Ok(Some(receipt))
    }

    /// A continuation reads the allocated source; it cannot mint a replacement
    /// NOW, reactivate an archived task, refresh a stale policy or renew a lease.
    pub fn revalidate(&self, task: &AllocatedCentralTask) -> Result<Value> {
        check_policy(&task.allocation["policy"])?;
        let policy = self.call(&task.request, "central.work.policy", json!({}))?;
        check_policy(&policy)?;
        if policy["revision"] != task.allocation["policy"]["revision"] {
            return Err(failure(
                "policy_changed",
                "Placement policy changed; explicitly re-resolve before another effect",
            ));
        }
        let current = self.call(
            &task.request,
            "central.now.read",
            json!({"now_ref": task.allocation["now_ref"]}),
        )?;
        if current["schema"] != "central.now-reading/v1"
            || current["record"]["now_ref"] != task.allocation["now_ref"]
            || current["record"]["task_ref"] != json!(task.request.task_ref)
            || current["record"]["lifecycle"] != "active"
            || current["source"]["ref"] != task.allocation["source"]["ref"]
            || current["revision"]["revision"] != task.allocation["revision"]["revision"]
        {
            return Err(failure(
                "now_changed",
                "Task NOW was changed, withdrawn or archived; retain its identity and explicitly re-enter",
            ));
        }
        Ok(current)
    }

    pub fn validate_write(&self, task: &AllocatedCentralTask, destination: &Path) -> Result<Value> {
        self.revalidate(task)?;
        let result = self.call(
            &task.request,
            "central.work.validate",
            json!({
                "now_ref": task.allocation["now_ref"],
                "expected_now_revision": task.allocation["revision"]["revision"],
                "expected_policy_revision": task.allocation["policy"]["revision"],
                "destination": destination,
            }),
        )?;
        if result["schema"] != "central.work-placement-validation/v1"
            || result["allowed"] != true
            || result["now_ref"] != task.allocation["now_ref"]
            || result["now_revision"] != task.allocation["revision"]["revision"]
            || result["policy_revision"] != task.allocation["policy"]["revision"]
        {
            return Err(failure(
                "validation_mismatch",
                "Native write decision is not for this exact NOW/policy basis",
            ));
        }
        Ok(result)
    }

    /// Anchor the directory from which the task body runs without presenting
    /// that directory itself as a write. A registered repository/worktree root
    /// can be a valid invocation location while Central correctly refuses an
    /// ambiguous write/remove approval for it because protected descendants
    /// (`.git`, `.central`, `ProjectCentral`) live below it.
    pub fn working_directory_anchor(
        &self,
        task: &AllocatedCentralTask,
        directory: &Path,
    ) -> Result<Value> {
        self.revalidate(task)?;
        if !directory.is_absolute()
            || !directory.is_dir()
            || directory.canonicalize().map_err(io_error)? != directory
        {
            return Err(failure(
                "material_bounds",
                "Task working directory must exist with its exact canonical identity",
            ));
        }
        let policy = &task.allocation["policy"];
        let grants = policy["writable_destinations"].as_array().ok_or_else(|| {
            failure(
                "owner_response",
                "Effective policy has no native writable destination list",
            )
        })?;
        let mut within_grant = false;
        for grant in grants {
            let path = Path::new(text(grant, "/path")?);
            if !path.is_absolute() {
                return Err(failure(
                    "owner_response",
                    "Native writable destination is not absolute",
                ));
            }
            within_grant |= directory.starts_with(path);
        }
        let protected_paths = policy["protected_paths"].as_array().ok_or_else(|| {
            failure(
                "owner_response",
                "Effective policy has no native protected path list",
            )
        })?;
        let mut inside_protected = false;
        for protected in protected_paths {
            let raw = protected
                .as_str()
                .filter(|path| !path.trim().is_empty())
                .ok_or_else(|| {
                    failure(
                        "owner_response",
                        "Native protected path must be non-empty text",
                    )
                })?;
            let path = Path::new(raw);
            if !path.is_absolute() {
                return Err(failure(
                    "owner_response",
                    "Native protected path is not absolute",
                ));
            }
            inside_protected |= directory.starts_with(path);
        }
        if !within_grant || inside_protected {
            return Err(failure(
                "material_bounds",
                "Task working directory must be inside a native writable destination and outside protected source/metadata ground",
            ));
        }
        directory_anchor(directory)
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
        self.write_boundary_requirements_with_additional_protection(
            task,
            authority,
            selected_directories,
            &[],
        )
    }

    /// Explicit exclusions narrow this caller's material aperture; they do
    /// not author Central policy. Native rows/order/coverage stay intact, and
    /// the empty list retains the legacy requirements exactly.
    pub fn write_boundary_requirements_with_additional_protection(
        &self,
        task: &AllocatedCentralTask,
        authority: &ResourceRef,
        selected_directories: &[PathBuf],
        additional_protected_directories: &[PathBuf],
    ) -> Result<Value> {
        self.revalidate(task)?;
        if selected_directories.len() > 63 {
            return Err(failure(
                "material_bounds",
                "At most 63 selected directories plus the native task NOW",
            ));
        }
        let now = task.now_directory()?;
        let mut writable = vec![now.clone()];
        for path in selected_directories {
            if !path.is_absolute()
                || !path.is_dir()
                || path.canonicalize().map_err(io_error)? != *path
            {
                return Err(failure(
                    "material_bounds",
                    "Writable directory must exist with its exact canonical identity",
                ));
            }
            if path != &now {
                self.validate_write(task, path)?;
            }
            if !writable.contains(path) {
                writable.push(path.clone());
            }
        }
        let policy = &task.allocation["policy"];
        let mut protected = policy["protected_paths"]
            .as_array()
            .ok_or_else(|| {
                failure(
                    "owner_response",
                    "Native policy has no protected-path array",
                )
            })?
            .clone();
        if additional_protected_directories.len() > 64 {
            return Err(failure(
                "material_bounds",
                "At most 64 explicit protected directories",
            ));
        }
        for directory in additional_protected_directories {
            let metadata = std::fs::symlink_metadata(directory).map_err(io_error)?;
            if !directory.is_absolute()
                || !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || directory.canonicalize().map_err(io_error)? != *directory
            {
                return Err(failure(
                    "material_bounds",
                    "Additional protected directory must exist with its exact canonical identity",
                ));
            }
            if writable
                .iter()
                .any(|root| directory.starts_with(root) || root.starts_with(directory))
            {
                return Err(failure(
                    "material_bounds",
                    "Additional protected directory must be disjoint from every writable root",
                ));
            }
            let value = json!(directory);
            if !protected.contains(&value) {
                protected.push(value);
            }
        }
        // The empty list preserves the legacy API, including native policy
        // rows. Any augmented request must fit the material owner's bound.
        if !additional_protected_directories.is_empty() && protected.len() > 64 {
            return Err(failure(
                "material_bounds",
                "At most 64 total native and additional protected paths",
            ));
        }
        let policy_source = policy["sources"]
            .as_array()
            .and_then(|s| s.last())
            .ok_or_else(|| {
                failure(
                    "owner_response",
                    "Effective policy has no native source basis",
                )
            })?;
        let policy_ref = text(policy_source, "/source/ref")?;
        let expiry = policy["expires_at_unix_seconds"]
            .as_u64()
            .and_then(|s| s.checked_mul(1000))
            .ok_or_else(|| {
                failure(
                    "material_bounds",
                    "Policy expiry cannot be represented in milliseconds",
                )
            })?;
        Ok(json!({
            "schema": "workcell.write-boundary/v1",
            "policy_ref": policy_ref, "policy_revision": policy["revision"],
            "authority_ref": authority, "writable_paths": writable,
            "protected_paths": protected,
            "required_coverage": policy["required_coverage"],
            "expires_at_unix_ms": expiry,
        }))
    }
}
impl AllocatedCentralTask {
    pub fn now_directory(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(text(
            &self.allocation,
            "/writable_destination",
        )?))
    }
    pub fn storage_declaration(&self) -> Result<Value> {
        Ok(
            json!({"schema": "workcell.directory-storage/v1", "directories": [{
                "logical_ref": text(&self.allocation, "/now_ref")?, "path": self.now_directory()?,
            }]}),
        )
    }
    pub fn storage_requirement(&self) -> Result<Value> {
        Ok(json!({"logical_ref": text(&self.allocation, "/now_ref")?,
            "access": "writable", "sharing": "shared", "minimum_capacity": null,
            "unit": null, "persistence": "external", "retention": "preserve"}))
    }
}
fn check_policy(policy: &Value) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io_error)?
        .as_secs();
    if policy["schema"] != "central.effective-placement-policy/v1"
        || policy["expires_at_unix_seconds"]
            .as_u64()
            .is_none_or(|t| t <= now)
        || policy["sources"].as_array().is_none_or(Vec::is_empty)
        || !policy["protected_paths"].is_array()
        || policy["required_coverage"]
            .as_array()
            .is_none_or(Vec::is_empty)
        || !matches!(
            policy["enforcement"].as_str(),
            Some("native-actions" | "harness-interception" | "material-filesystem")
        )
    {
        return Err(failure(
            "policy_unavailable",
            "Current recognised placement source, bounded lease and explicit coverage are required",
        ));
    }
    text(policy, "/revision")?;
    Ok(())
}
#[cfg(unix)]
fn directory_anchor(path: &Path) -> Result<Value> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(failure(
            "material_bounds",
            "Task working directory must remain a real directory",
        ));
    }
    Ok(json!({
        "schema":"aikit.task-working-directory-anchor/v1",
        "path":path,
        "device":metadata.dev(),
        "inode":metadata.ino(),
    }))
}
#[cfg(not(unix))]
fn directory_anchor(_path: &Path) -> Result<Value> {
    Err(failure(
        "material_bounds",
        "Native task working-directory identity is unsupported on this platform",
    ))
}
fn text<'a>(value: &'a Value, pointer: &str) -> Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| failure("owner_response", &format!("Native receipt lacks {pointer}")))
}
fn failure(kind: &str, message: &str) -> AikitError {
    let code = match kind {
        "configuration" => "placement.central_configuration",
        "owner_response" => "placement.central_owner_response",
        "owner_refused" => "placement.central_owner_refused",
        "allocation_mismatch" => "placement.central_allocation_mismatch",
        "policy_changed" => "placement.central_policy_changed",
        "now_changed" => "placement.central_now_changed",
        "validation_mismatch" => "placement.central_validation_mismatch",
        "material_bounds" => "placement.central_material_bounds",
        "policy_unavailable" => "placement.central_policy_unavailable",
        _ => "placement.central_io",
    };
    AikitError::new(code, message)
}
fn io_error(error: impl std::fmt::Display) -> AikitError {
    failure("io", &error.to_string())
}
