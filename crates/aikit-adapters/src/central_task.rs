//! Task placement is composed from the real Central Actions and Workcell's
//! material operation. This adapter never interprets authored policy documents,
//! adopts governance, supplies a missing authority, or drops a protected path.
use crate::agency_admission::AdmittedAgency;
use crate::placement_enforcement::{
    EnforcementRequirement, PlacementBasis, PlacementDecision, PlacementOwner, WriteAttempt,
};
use crate::runner::CommandRunner;
use aikit_core::{AikitError, ResourceRef, Result, SourceRevision};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentralTaskRequest {
    pub central_bin: PathBuf,
    pub central_root: PathBuf,
    pub project: Option<String>,
    pub expected_policy_revision: SourceRevision,
    pub task_ref: ResourceRef,
    pub purpose: String,
    pub participant_refs: Vec<ResourceRef>,
    pub source_refs: Vec<ResourceRef>,
    pub cwd: PathBuf,
    /// Explicitly selected engineering directories, not the entire World grant.
    /// Central's allocated T directory is added without changing its identity.
    pub writable_paths: Vec<PathBuf>,
    pub authority_ref: ResourceRef,
    /// Preserved through admission/reopen; arrival is not automatic inclusion.
    pub return_destination: ResourceRef,
    pub workcell_boundary_bin: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentralTaskAdmission {
    pub request: CentralTaskRequest,
    pub allocation: Value,
    pub cwd_decision: Value,
    pub engineering_decisions: Vec<Value>,
    pub requirements: Value,
    pub preparation: Value,
}

fn invalid(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("central.task_admission", message.to_string())
}
fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field].as_str().filter(|v| !v.is_empty()).ok_or_else(|| invalid(format!("Native result omitted {field}")))
}
fn canonical_directory(path: &Path) -> Result<()> {
    if !path.is_absolute() || !path.is_dir() || path.canonicalize().map_err(invalid)? != path {
        return Err(invalid("Task directories must be existing absolute canonical paths"));
    }
    Ok(())
}
fn action(runner: &dyn CommandRunner, request: &CentralTaskRequest, name: &str, mut input: Value) -> Result<Value> {
    input["project"] = json!(request.project);
    let argv = vec![request.central_bin.display().to_string(), "--json".into(), "--root".into(), request.central_root.display().to_string(), "action".into(), "run".into(), name.into(), input.to_string()];
    let output = runner.run(&argv)?;
    if output.stdout.len() > 4 * 1024 * 1024 { return Err(invalid("Central result exceeded the bounded native response size")); }
    let result: Value = serde_json::from_str(&output.stdout).map_err(|_| invalid("Central returned no valid native result"))?;
    if !output.ok() || result["ok"] != true {
        // Native diagnostics can quote source material. Retain the owner code,
        // not arbitrary stdout/stderr or a private authored policy body.
        return Err(invalid(format!("Central {name} refused ({})", result["error"]["code"].as_str().unwrap_or("native-operation-failed"))));
    }
    if result["action"] != name { return Err(invalid("Central result has the wrong Action identity")); }
    result.get("data").cloned().ok_or_else(|| invalid("Central succeeded without data"))
}
fn validate(runner: &dyn CommandRunner, request: &CentralTaskRequest, allocation: &Value, path: &Path, expected_anchor: Option<&Value>) -> Result<Value> {
    let mut input = json!({"now_ref":allocation["now_ref"],"expected_now_revision":allocation["revision"]["revision"],"expected_policy_revision":request.expected_policy_revision,"destination":path});
    if let Some(anchor) = expected_anchor { input["expected_destination_anchor"] = anchor.clone(); }
    let result = action(runner, request, "central.work.validate", input)?;
    if result["schema"] != "central.work-placement-validation/v1"
        || result["allowed"] != true || result["destination"] != json!(path)
        || result["now_ref"] != allocation["now_ref"]
        || result["now_revision"] != allocation["revision"]["revision"]
        || result["policy_revision"] != json!(request.expected_policy_revision)
    { return Err(invalid("Central did not permit the exact current task/path basis")); }
    Ok(result)
}
fn check_agency(request: &CentralTaskRequest, allocation: &Value, agency: &AdmittedAgency) -> Result<()> {
    if !request.participant_refs.contains(&agency.agent_ref)
        || allocation["policy"]["scope_ref"] != json!(agency.world_ref)
        || !agency.receipt["determination"]["authority_refs"].as_array().is_some_and(|refs| refs.contains(&json!(request.authority_ref)))
        || !agency.receipt["differentiated_binding"]["authority_refs"].as_array().is_some_and(|refs| refs.contains(&json!(request.authority_ref)))
    { return Err(invalid("Task participant, World or current native Agency authority does not match placement")); }
    Ok(())
}

pub fn admit_task(runner: &dyn CommandRunner, request: CentralTaskRequest, agency: &AdmittedAgency) -> Result<CentralTaskAdmission> {
    canonical_directory(&request.central_root)?;
    canonical_directory(&request.cwd)?;
    if request.purpose.trim().is_empty() || request.writable_paths.len() > 62 || request.participant_refs.is_empty() {
        return Err(invalid("Task needs a purpose, participants and a bounded explicit write selection"));
    }
    let policy = action(runner, &request, "central.work.policy", json!({}))?;
    if policy["schema"] != "central.effective-placement-policy/v1" || policy["revision"] != json!(request.expected_policy_revision) {
        return Err(invalid("Placement policy changed; preview the current owner basis before admission"));
    }
    // Check authority before the allocation effect, not after a rejected Agent
    // has created an unattributed clearing.
    check_agency(&request, &json!({"policy":policy}), agency)?;
    let allocation = action(runner, &request, "central.now.allocate", json!({"task_ref":request.task_ref,"purpose":request.purpose,"participant_refs":request.participant_refs,"source_refs":request.source_refs,"expected_policy_revision":request.expected_policy_revision}))?;
    if allocation["schema"] != "central.now-allocation/v1"
        || allocation["record"]["task_ref"] != json!(request.task_ref)
        || allocation["record"]["lifecycle"] != "active"
        || allocation["policy"]["revision"] != json!(request.expected_policy_revision)
    { return Err(invalid("Central allocated a different or inactive task basis")); }
    check_agency(&request, &allocation, agency)?;
    let now = PathBuf::from(string(&allocation, "writable_destination")?);
    canonical_directory(&now)?;
    // The T directory is already allocated by the owner. Its first potential
    // member is a write-free validation plan, not a created marker or a grant.
    let cwd_target = if request.cwd == now { now.join(".task-placement-plan") } else { request.cwd.clone() };
    let cwd_decision = validate(runner, &request, &allocation, &cwd_target, None)?;
    let mut paths = Vec::new();
    let mut engineering_decisions = Vec::new();
    for path in &request.writable_paths {
        canonical_directory(path)?;
        if path == &now { continue; }
        if paths.contains(path) { return Err(invalid("Duplicate engineering write selection")); }
        engineering_decisions.push(validate(runner, &request, &allocation, path, None)?);
        paths.push(path.clone());
    }
    paths.push(now);
    let policy = &allocation["policy"];
    let protected = policy["protected_paths"].as_array().ok_or_else(|| invalid("Central omitted protected paths"))?;
    let coverage = policy["required_coverage"].as_array().ok_or_else(|| invalid("Central omitted required coverage"))?;
    let expiry = policy["expires_at_unix_seconds"].as_u64().and_then(|s| s.checked_mul(1000)).ok_or_else(|| invalid("Invalid native policy lease"))?;
    let policy_ref = string(&policy["sources"][0]["source"], "ref")?;
    let requirements = json!({"schema":"workcell.write-boundary/v1","policy_ref":policy_ref,"policy_revision":request.expected_policy_revision,"authority_ref":request.authority_ref,"writable_paths":paths,"protected_paths":protected,"required_coverage":coverage,"expires_at_unix_ms":expiry});
    let preparation = prepare_boundary(runner, &request, &requirements)?;
    Ok(CentralTaskAdmission { request, allocation, cwd_decision, engineering_decisions, requirements, preparation })
}

fn prepare_boundary(runner: &dyn CommandRunner, request: &CentralTaskRequest, requirements: &Value) -> Result<Value> {
    let staged = tempfile::NamedTempFile::new().map_err(invalid)?;
    std::fs::write(staged.path(), serde_json::to_vec(requirements).map_err(invalid)?).map_err(invalid)?;
    let output = runner.run(&[request.workcell_boundary_bin.display().to_string(), "inspect".into(), staged.path().display().to_string(), request.expected_policy_revision.to_string()])?;
    if !output.ok() { return Err(invalid("Workcell refused the exact required write boundary; no advisory fallback was selected")); }
    let preparation: Value = serde_json::from_str(&output.stdout).map_err(invalid)?;
    if preparation["schema"] != "workcell.prepared-write-boundary/v1"
        || preparation["state"] != "prepared-not-executed"
        || preparation["requirements"] != *requirements
        || preparation["capabilities"]["supported"] != true
        || preparation["requirements_digest"].as_str().is_none()
    { return Err(invalid("Workcell did not prepare the exact native material requirements")); }
    Ok(preparation)
}

impl CentralTaskAdmission {
    /// Re-read the source owner at the acting boundary. No allocation, renewal,
    /// adoption, replay or silent migration is performed here.
    pub fn revalidate(&self, runner: &dyn CommandRunner, agency: &AdmittedAgency, cwd: &Path) -> Result<()> {
        if cwd != self.request.cwd { return Err(invalid("Wrong working copy/cwd; retain this task's exact selected directory")); }
        canonical_directory(cwd)?;
        let current = action(runner, &self.request, "central.now.read", json!({"now_ref":self.allocation["now_ref"]}))?;
        if current["revision"] != self.allocation["revision"]
            || current["record"] != self.allocation["record"]
            || current["writable_destination"] != self.allocation["writable_destination"]
            || current["policy"]["revision"] != json!(self.request.expected_policy_revision)
        { return Err(invalid("Task NOW, policy, lifecycle or material destination changed; explicitly re-resolve")); }
        check_agency(&self.request, &current, agency)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(invalid)?.as_millis();
        if now >= u128::from(self.requirements["expires_at_unix_ms"].as_u64().ok_or_else(|| invalid("Missing material lease"))?) {
            return Err(invalid("Prepared material lease expired; re-resolve and reopen without replaying work"));
        }
        let cwd_target = PathBuf::from(string(&self.cwd_decision, "destination")?);
        validate(runner, &self.request, &self.allocation, &cwd_target, self.cwd_decision.get("destination_anchor"))?;
        for decision in &self.engineering_decisions {
            validate(runner, &self.request, &self.allocation, Path::new(string(decision, "destination")?), decision.get("destination_anchor"))?;
        }
        // Include exact current filesystem object identity, not merely matching
        // externally supplied policy strings. Changed material must be reopened.
        let prepared = prepare_boundary(runner, &self.request, &self.requirements)?;
        if prepared != self.preparation { return Err(invalid("Material objects or supported protection changed; reopen on the current owner basis")); }
        Ok(())
    }
    pub fn protocol_argv(&self, requirements_path: &Path, provider_argv: &[String]) -> Result<Vec<String>> {
        if provider_argv.is_empty() { return Err(invalid("Missing native protocol body")); }
        let mut argv = vec![self.request.workcell_boundary_bin.display().to_string(), "protocol".into(), requirements_path.display().to_string(), self.request.expected_policy_revision.to_string(), "--".into()];
        argv.extend_from_slice(provider_argv);
        Ok(argv)
    }
}

/// Connect the existing native write guard to the same owner operations. A
/// prepared receipt alone is never represented as installed interception.
pub struct NativePlacementOwner<'a> {
    pub runner: &'a dyn CommandRunner,
    pub admission: &'a CentralTaskAdmission,
    pub agency: &'a AdmittedAgency,
}
impl PlacementOwner for NativePlacementOwner<'_> {
    fn resolve_and_allocate(&self, task: &ResourceRef) -> Result<PlacementBasis> {
        if task != &self.admission.request.task_ref { return Err(invalid("Wrong task identity")); }
        self.admission.revalidate(self.runner, self.agency, &self.admission.request.cwd)?;
        Ok(PlacementBasis {
            policy_ref: ResourceRef::parse(string(&self.admission.requirements, "policy_ref")?)?,
            policy_revision: self.admission.request.expected_policy_revision.clone(),
            allocation_ref: ResourceRef::parse(string(&self.admission.allocation, "now_ref")?)?,
            now: PathBuf::from(string(&self.admission.allocation, "writable_destination")?),
            requirement: match self.admission.allocation["policy"]["enforcement"].as_str() {
                Some("native-actions") => EnforcementRequirement::Advisory,
                Some("harness-interception") => EnforcementRequirement::NativeWriteEvents,
                Some("material-filesystem") => EnforcementRequirement::MaterialConfinement,
                _ => return Err(invalid("Unknown native enforcement requirement")),
            },
        })
    }
    fn validate_write(&self, basis: &PlacementBasis, attempt: &WriteAttempt) -> Result<PlacementDecision> {
        if basis.policy_revision != self.admission.request.expected_policy_revision || attempt.cwd != self.admission.request.cwd {
            return Err(invalid("Guard basis/cwd changed"));
        }
        validate(self.runner, &self.admission.request, &self.admission.allocation, &attempt.target, None)?;
        Ok(PlacementDecision::Allow { decision_ref: basis.allocation_ref.clone(), policy_revision: basis.policy_revision.clone(), canonical_target: attempt.target.clone() })
    }
}
