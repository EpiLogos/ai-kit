//! Published Central placement/NOW operations -> native Workcell requirements.
//! This adapter carries owner facts; it neither recognises policy nor implements
//! a second allocation service. Preparation is not an executed/constrained body.
use crate::placement_enforcement::{
    canonical_write_target, EnforcementRequirement, PlacementBasis, PlacementDecision,
    PlacementOwner, WriteAttempt,
};
use crate::runner::CommandRunner;
use aikit_core::{AikitError, ResourceRef, Result, SourceRevision};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Write, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentralTask {
    pub ctrl_bin: PathBuf,
    pub root: PathBuf,
    pub project: Option<String>,
    pub task_ref: ResourceRef,
    pub purpose: String,
    #[serde(default)]
    pub participant_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub source_refs: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllocatedTask {
    /// Full native owner reading, including source standing, scope and path anchors.
    pub allocation: Value,
    pub basis: PlacementBasis,
    pub now_revision: String,
}

pub struct CentralPlacement<R> {
    runner: R,
    pub task: CentralTask,
}
impl<R: CommandRunner> CentralPlacement<R> {
    pub fn new(runner: R, task: CentralTask) -> Result<Self> {
        if !task.ctrl_bin.is_absolute() || !task.root.is_absolute()
            || task.purpose.trim().is_empty() || task.purpose.len() > 8192
        {
            return Err(failure("invalid_request", "Explicit owner executable, root and bounded purpose are required"));
        }
        if task.root.canonicalize().map_err(io_error)? != task.root || !task.root.is_dir() {
            return Err(failure("invalid_root", "Central root must be an existing canonical directory"));
        }
        Ok(Self { runner, task })
    }

    fn call(&self, action: &str, mut input: Value) -> Result<Value> {
        if let Some(project) = &self.task.project {
            input["project"] = json!(project);
        }
        let argv = vec![
            path_text(&self.task.ctrl_bin)?, "--json".into(), "--root".into(),
            path_text(&self.task.root)?, "action".into(), "run".into(), action.into(),
            serde_json::to_string(&input).map_err(io_error)?,
        ];
        let output = self.runner.run(&argv)?;
        if output.stdout.len() > 4 * 1024 * 1024 {
            return Err(failure("oversized_receipt", "Central response exceeds 4 MiB"));
        }
        let envelope: Value = serde_json::from_str(&output.stdout)
            .map_err(|_| failure("invalid_receipt", "Central did not return a native Action envelope"))?;
        if envelope["action"] != action {
            return Err(failure("invalid_receipt", "Central Action attribution does not match the requested operation"));
        }
        if output.ok() && envelope["ok"] == true {
            return Ok(envelope["data"].clone());
        }
        // Only this documented negative carries a write decision. A failed
        // producer, stale source or malformed envelope is never an allow/deny fact.
        if action == "central.work.validate" && envelope["ok"] == false
            && envelope["error"]["code"] == "placement_rejected"
            && envelope["error"]["details"]["allowed"] == false
        {
            return Ok(envelope["error"]["details"].clone());
        }
        Err(failure("owner_refused", &format!("Central refused {action}; code {}", envelope["error"]["code"])))
    }

    pub fn allocate(&self) -> Result<AllocatedTask> {
        let policy = self.call("central.work.policy", json!({}))?;
        validate_policy(&policy)?;
        let allocation = self.call("central.now.allocate", json!({
            "task_ref": self.task.task_ref, "purpose": self.task.purpose,
            "participant_refs": self.task.participant_refs, "source_refs": self.task.source_refs,
            "expected_policy_revision": required(&policy, "revision")?,
        }))?;
        if allocation["schema"] != "central.now-allocation/v1"
            || allocation["record"]["schema"] != "central.now-clearing/v1"
            || allocation["record"]["task_ref"] != json!(self.task.task_ref)
            || allocation["record"]["purpose"] != self.task.purpose
            || allocation["record"]["participant_refs"] != json!(self.task.participant_refs)
            || allocation["record"]["source_refs"] != json!(self.task.source_refs)
            || allocation["record"]["lifecycle"] != "active"
            || allocation["record"]["now_ref"] != allocation["now_ref"]
            || allocation["record"]["source_ref"] != allocation["source"]["ref"]
            || allocation["source"]["agent_retrieval_allowed"] != true
            || allocation["policy"]["revision"] != policy["revision"]
            || allocation["record"]["scope_ref"] != policy["scope_ref"]
        {
            return Err(failure("allocation_mismatch", "Native NOW does not match the admitted task, source or policy"));
        }
        validate_policy(&allocation["policy"])?;
        let now = PathBuf::from(required(&allocation, "writable_destination")?);
        if !now.is_absolute() || now.canonicalize().map_err(io_error)? != now || !now.is_dir() {
            return Err(failure("allocation_unavailable", "Native NOW destination is not an existing canonical directory"));
        }
        let source = allocation["policy"]["sources"].as_array().and_then(|a| a.first())
            .ok_or_else(|| failure("authority_missing", "Effective policy has no native source basis"))?;
        let basis = PlacementBasis {
            policy_ref: ResourceRef::parse(required(&source["source"], "ref")?)?,
            policy_revision: SourceRevision::parse(required(&policy, "revision")?)?,
            allocation_ref: ResourceRef::parse(required(&allocation, "now_ref")?)?,
            now,
            requirement: requirement(required(&policy, "enforcement")?)?,
        };
        let now_revision = required(&allocation["revision"], "revision")?.to_owned();
        Ok(AllocatedTask { allocation, basis, now_revision })
    }

    pub fn validate(&self, allocated: &AllocatedTask, cwd: &Path, target: &Path) -> Result<Value> {
        let target = canonical_write_target(cwd, target)?;
        let result = self.call("central.work.validate", json!({
            "now_ref": allocated.basis.allocation_ref,
            "expected_now_revision": allocated.now_revision,
            "expected_policy_revision": allocated.basis.policy_revision,
            "destination": target,
        }))?;
        if result["schema"] != "central.work-placement-validation/v1"
            || result["now_ref"] != json!(allocated.basis.allocation_ref)
            || result["now_revision"] != allocated.now_revision
            || result["policy_revision"] != json!(allocated.basis.policy_revision)
            || result["destination"] != json!(target)
            || result["valid_now_destination"] != json!(allocated.basis.now)
            || !result["allowed"].is_boolean()
        {
            return Err(failure("decision_mismatch", "Native write decision is not for this exact task, revision and destination"));
        }
        Ok(result)
    }
}
impl<R: CommandRunner> PlacementOwner for CentralPlacement<R> {
    fn resolve_and_allocate(&self, task: &ResourceRef) -> Result<PlacementBasis> {
        if task != &self.task.task_ref {
            return Err(failure("task_mismatch", "Guard was called for another task"));
        }
        Ok(self.allocate()?.basis)
    }
    fn validate_write(&self, basis: &PlacementBasis, attempt: &WriteAttempt) -> Result<PlacementDecision> {
        let current = self.allocate()?;
        if &current.basis != basis {
            return Err(failure("policy_changed", "Re-resolve task placement before effects"));
        }
        let result = self.validate(&current, &attempt.cwd, &attempt.target)?;
        // The owner's NOW ref correlates this decision. Do not mint a receipt id.
        if result["allowed"] == true {
            Ok(PlacementDecision::Allow {
                decision_ref: basis.allocation_ref.clone(), policy_revision: basis.policy_revision.clone(),
                canonical_target: PathBuf::from(required(&result, "destination")?),
            })
        } else {
            Ok(PlacementDecision::Deny {
                decision_ref: basis.allocation_ref.clone(), policy_revision: basis.policy_revision.clone(),
                reason: required(&result, "reason")?.to_owned(),
            })
        }
    }
}

/// Exact conversion to the published Workcell owner contract. Required paths
/// and coverage are never filtered to fit a provider. Workcell may refuse it.
pub fn boundary_requirements(allocated: &AllocatedTask) -> Result<Value> {
    let policy = &allocated.allocation["policy"];
    validate_policy(policy)?;
    let authority = policy["sources"].as_array().into_iter().flatten()
        .flat_map(|s| s["authority_refs"].as_array().into_iter().flatten())
        .next().ok_or_else(|| failure("authority_missing", "Central disclosed no authority source for this policy"))?;
    let writable = policy["writable_destinations"].as_array()
        .ok_or_else(|| failure("invalid_receipt", "Missing native writable destinations"))?
        .iter().map(|entry| required(entry, "path").map(str::to_owned)).collect::<Result<Vec<_>>>()?;
    let protected = string_array(policy, "protected_paths")?;
    if !writable.iter().any(|p| Path::new(p) == allocated.basis.now) {
        return Err(failure("allocation_mismatch", "Central's material requirements omit its own NOW destination"));
    }
    let expiry = policy["expires_at_unix_seconds"].as_u64()
        .and_then(|n| n.checked_mul(1000))
        .ok_or_else(|| failure("invalid_expiry", "Native policy expiry cannot be represented by Workcell"))?;
    Ok(json!({
        "schema":"workcell.write-boundary/v1", "policy_ref":allocated.basis.policy_ref,
        "policy_revision":allocated.basis.policy_revision,
        "authority_ref":required(authority,"source_ref")?,
        "writable_paths":writable,"protected_paths":protected,
        "required_coverage":string_array(policy,"required_coverage")?,
        "expires_at_unix_ms":expiry,
    }))
}

pub fn prepare_boundary<R: CommandRunner>(runner: &R, binary: &Path, allocated: &AllocatedTask) -> Result<Value> {
    if !binary.is_absolute() {
        return Err(failure("invalid_provider", "Use an explicit native Workcell executable"));
    }
    let requirements = boundary_requirements(allocated)?;
    let mut input = tempfile::NamedTempFile::new().map_err(io_error)?;
    input.write_all(serde_json::to_string(&requirements).map_err(io_error)?.as_bytes()).map_err(io_error)?;
    input.as_file().sync_all().map_err(io_error)?;
    let argv = vec![path_text(binary)?, "inspect".into(), path_text(input.path())?, allocated.basis.policy_revision.to_string()];
    let output = runner.run(&argv)?;
    if !output.ok() || output.stdout.len() > 4 * 1024 * 1024 {
        return Err(failure("material_refused", "Workcell could not prepare the exact required boundary; no unconfined fallback is permitted"));
    }
    let receipt: Value = serde_json::from_str(&output.stdout).map_err(io_error)?;
    let digest = format!("sha256:{:x}", Sha256::digest(requirements.to_string().as_bytes()));
    if receipt["schema"] != "workcell.prepared-write-boundary/v1"
        || receipt["state"] != "prepared-not-executed" || receipt["requirements"] != requirements
        || receipt["requirements_digest"] != digest || receipt["capabilities"]["supported"] != true
    {
        return Err(failure("material_mismatch", "Workcell receipt did not retain the exact requirements and supported coverage"));
    }
    Ok(receipt)
}

fn validate_policy(policy: &Value) -> Result<()> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io_error)?.as_secs();
    if policy["schema"] != "central.effective-placement-policy/v1"
        || policy["expires_at_unix_seconds"].as_u64().is_none_or(|expiry| expiry <= now)
        || policy["sources"].as_array().is_none_or(|a| a.is_empty())
    {
        return Err(failure("invalid_policy", "Native policy is missing, expired or has no source basis"));
    }
    for source in policy["sources"].as_array().expect("checked array") {
        if source["source"]["agent_retrieval_allowed"] != true {
            return Err(failure("private_policy", "Central withheld the policy source; no body may borrow its authority"));
        }
        required(source, "revision")?;
        required(&source["source"], "ref")?;
    }
    requirement(required(policy, "enforcement")?)?;
    Ok(())
}
fn requirement(value: &str) -> Result<EnforcementRequirement> {
    match value {
        "native-actions" | "harness-interception" => Ok(EnforcementRequirement::NativeWriteEvents),
        "material-filesystem" => Ok(EnforcementRequirement::MaterialConfinement),
        _ => Err(failure("unsupported_enforcement", "Central required an unknown enforcement kind")),
    }
}
fn string_array(value: &Value, key: &str) -> Result<Vec<String>> {
    value[key].as_array().ok_or_else(|| failure("invalid_receipt", key))?
        .iter().map(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
            .ok_or_else(|| failure("invalid_receipt", key))).collect()
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key].as_str().filter(|s| !s.is_empty())
        .ok_or_else(|| failure("invalid_receipt", &format!("Native receipt is missing {key}")))
}
fn path_text(path: &Path) -> Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| failure("invalid_path", "Native protocol requires a UTF-8 path"))
}
fn failure(code: &str, message: &str) -> AikitError {
    AikitError::new(format!("placement.{code}"), message)
}
fn io_error(error: impl std::fmt::Display) -> AikitError {
    failure("native_io", &error.to_string())
}
