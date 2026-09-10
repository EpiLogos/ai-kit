//! AIKit placement enforcement projections. Central owns policy/allocation and
//! decides a write; Workcell owns material confinement. These are internal ports,
//! NOT proposed Central/Workcell wire schemas or invented CLI operation names.
use crate::actuation_harness_capability::{HarnessCapability, ACTUATION_HARNESS_CAPABILITY_SCHEMA};
use aikit_core::{AikitError, ResourceRef, Result, SourceRevision};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EnforcementRequirement {
    Advisory,
    NativeWriteEvents,
    MaterialConfinement,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WriteExposure {
    NativeFileOperation,
    OpaqueProcess,
}
/// Narrow internal reading produced by a Central adapter, once its native
/// operation is published. A source reference alone is not an adopted policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementBasis {
    pub policy_ref: ResourceRef,
    pub policy_revision: SourceRevision,
    pub allocation_ref: ResourceRef,
    pub now: PathBuf,
    pub requirement: EnforcementRequirement,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteAttempt {
    pub cwd: PathBuf,
    pub target: PathBuf,
    pub exposure: WriteExposure,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum PlacementDecision {
    Allow {
        decision_ref: ResourceRef,
        policy_revision: SourceRevision,
        canonical_target: PathBuf,
    },
    Deny {
        decision_ref: ResourceRef,
        policy_revision: SourceRevision,
        reason: String,
    },
}
/// Implementors must read current Central policy, allocate through Central and
/// validate through Central at the effect boundary. There is deliberately no
/// speculative `ctrl work.guard` invocation in this repository.
pub trait PlacementOwner {
    fn resolve_and_allocate(&self, task: &ResourceRef) -> Result<PlacementBasis>;
    fn validate_write(
        &self,
        basis: &PlacementBasis,
        attempt: &WriteAttempt,
    ) -> Result<PlacementDecision>;
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnforcementCoverage {
    pub harness: String,
    pub descriptor_revision: Option<u32>,
    pub native_file_events_blockable: bool,
    /// Supplied only by a real material owner adapter after verification. A hook
    /// descriptor, settings file or skill text cannot produce this receipt.
    pub material_receipt: Option<ResourceRef>,
    pub limitations: Vec<String>,
}
impl EnforcementCoverage {
    pub fn satisfies(&self, requirement: EnforcementRequirement, exposure: WriteExposure) -> bool {
        match requirement {
            EnforcementRequirement::Advisory => true,
            EnforcementRequirement::NativeWriteEvents => {
                self.material_receipt.is_some()
                    || (self.native_file_events_blockable
                        && exposure == WriteExposure::NativeFileOperation)
            }
            EnforcementRequirement::MaterialConfinement => self.material_receipt.is_some(),
        }
    }
}
/// Read the actual owner descriptor, including its exact blockable event and
/// implemented native transport. A descriptor for another slug cannot lend its
/// blocking semantics to Codex or a provider that only has a post-tool event.
pub fn coverage(
    capability: &HarnessCapability,
    expected_slug: &str,
) -> Result<EnforcementCoverage> {
    if capability.schema != ACTUATION_HARNESS_CAPABILITY_SCHEMA
        || capability.document != "capability"
        || capability.harness_slug != expected_slug
    {
        return Err(AikitError::new(
            "placement.capability_mismatch",
            "Capability receipt is not for the requested native harness",
        ));
    }
    let native_file_events_blockable = expected_slug == "claude-code"
        && capability.blocking_semantics.kind == "deny-and-block"
        && capability.native_events.iter().any(|e| {
            e.event == "pre-tool-use"
                && e.native_name == "PreToolUse"
                && e.transport == "settings-json-hooks-map"
                && e.can_block
        });
    Ok(EnforcementCoverage {
        harness: expected_slug.into(),
        descriptor_revision: capability.provenance.catalog_revision,
        native_file_events_blockable,
        material_receipt: None,
        limitations: vec![
            "No installed-hook attestation is implied by a descriptor".into(),
            "Opaque shell/subprocess/external writes require a verified material boundary".into(),
        ],
    })
}
/// Canonicalise existing ancestors, not shell syntax. No lexical prefix check
/// may bless `..`, a redirected cwd, or a symlink escaping an approved root.
/// The native owner still revalidates policy/path at commit time; this function
/// does not claim to prevent a later filesystem race on an unconfined host.
pub fn canonical_write_target(cwd: &Path, target: &Path) -> Result<PathBuf> {
    let cwd = cwd.canonicalize().map_err(io_error)?;
    if !cwd.is_dir() {
        return Err(io_error("cwd is not a directory"));
    }
    let path = if target.is_absolute() {
        target.to_owned()
    } else {
        cwd.join(target)
    };
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(io_error(
            "Parent traversal needs a freshly resolved native write plan",
        ));
    }
    let mut existing = path.as_path();
    let mut tail = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tail.push(
                    existing
                        .file_name()
                        .ok_or_else(|| io_error("No canonical parent"))?
                        .to_os_string(),
                );
                existing = existing
                    .parent()
                    .ok_or_else(|| io_error("No canonical parent"))?;
            }
            Err(e) => return Err(io_error(e)),
        }
    }
    let mut result = existing.canonicalize().map_err(io_error)?;
    for component in tail.into_iter().rev() {
        result.push(component);
    }
    Ok(result)
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardOutcome {
    pub allowed: bool,
    pub basis: PlacementBasis,
    pub coverage: EnforcementCoverage,
    pub owner_decision: Option<PlacementDecision>,
    pub reason: String,
}
pub fn guard(
    owner: &dyn PlacementOwner,
    task: &ResourceRef,
    requested: &WriteAttempt,
    coverage: &EnforcementCoverage,
) -> Result<GuardOutcome> {
    let basis = owner.resolve_and_allocate(task)?;
    if !basis.now.is_absolute()
        || basis.now.canonicalize().map_err(io_error)? != basis.now
        || !basis.now.is_dir()
    {
        return Err(AikitError::new(
            "placement.invalid_allocation",
            "Central must return a real, canonical NOW destination before a task writes",
        ));
    }
    if !coverage.satisfies(basis.requirement, requested.exposure) {
        return Ok(GuardOutcome{allowed:false,basis,coverage:coverage.clone(),owner_decision:None,reason:"Required enforcement is not operative for this body/effect; obtain a supported native or Workcell boundary before dispatch".into()});
    }
    let attempt = WriteAttempt {
        cwd: requested.cwd.canonicalize().map_err(io_error)?,
        target: canonical_write_target(&requested.cwd, &requested.target)?,
        exposure: requested.exposure,
    };
    let decision = owner.validate_write(&basis, &attempt)?;
    let (allowed, revision, reason) = match &decision {
        PlacementDecision::Allow {
            policy_revision,
            canonical_target,
            ..
        } => (
            canonical_target == &attempt.target,
            policy_revision,
            "Native write decision correlated with the exact canonical target".to_owned(),
        ),
        PlacementDecision::Deny {
            policy_revision,
            reason,
            ..
        } => (false, policy_revision, reason.clone()),
    };
    if revision != &basis.policy_revision {
        return Err(AikitError::new("placement.policy_changed","Native policy changed between allocation and validation; resolve the current basis and retry"));
    }
    Ok(GuardOutcome {
        allowed,
        basis,
        coverage: coverage.clone(),
        owner_decision: Some(decision),
        reason,
    })
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeGuardResponse {
    pub exit_code: i32,
    pub stdout: Value,
    pub stderr: String,
}
/// Only the implemented, genuinely blockable Claude PreToolUse boundary gets
/// deny exit 2. Unsupported transports return a requirement failure to their
/// caller; printing a denial on Codex is not misreported as a blocked tool.
pub fn claude_pre_tool_response(outcome: &GuardOutcome) -> Result<NativeGuardResponse> {
    if outcome.coverage.harness != "claude-code" || !outcome.coverage.native_file_events_blockable {
        return Err(AikitError::new(
            "placement.native_blocking_unsupported",
            "This body has no implemented blockable PreToolUse adapter",
        ));
    }
    let reason = format!(
        "{}; NOW destination: {}; policy {}@{}",
        outcome.reason,
        outcome.basis.now.display(),
        outcome.basis.policy_ref,
        outcome.basis.policy_revision
    );
    Ok(NativeGuardResponse {
        exit_code: if outcome.allowed { 0 } else { 2 },
        stdout: if outcome.allowed {
            json!({})
        } else {
            json!({"decision":"block","reason":reason})
        },
        stderr: if outcome.allowed {
            String::new()
        } else {
            reason
        },
    })
}
/// Reversible JSON projection: only an exactly owned native command is changed.
/// Foreign hooks and settings remain byte-equivalent as JSON values. A CAS over
/// the containing file belongs to the existing projection application layer.
pub fn project_claude_hook(settings: &Value, command: &str, install: bool) -> Result<Value> {
    if !settings.is_object() || command.trim().is_empty() || command.contains('\n') {
        return Err(io_error(
            "Expected object settings and one explicit owned command",
        ));
    }
    let mut result = settings.clone();
    if result.get("hooks").is_none() {
        if !install {
            return Ok(result);
        }
        result["hooks"] = json!({});
    }
    let hooks = result["hooks"]
        .as_object_mut()
        .ok_or_else(|| io_error("Foreign hooks value is not an object; refusing to replace it"))?;
    if !hooks.contains_key("PreToolUse") {
        if !install {
            return Ok(result);
        }
        hooks.insert("PreToolUse".into(), json!([]));
    }
    let entries = hooks
        .get_mut("PreToolUse")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            io_error("Foreign PreToolUse value is not an array; refusing to replace it")
        })?;
    // Remove an empty entry only when removing our command made it empty.
    // Foreign empty/malformed entries are not ours to erase or repair.
    entries.retain_mut(|entry| {
        if let Some(items) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
            let before = items.len();
            items.retain(|h| !(h["type"] == "command" && h["command"].as_str() == Some(command)));
            !(before > items.len() && items.is_empty())
        } else {
            true
        }
    });
    if install {
        entries.push(json!({"hooks":[{"type":"command","command":command}]}));
    }
    Ok(result)
}
fn io_error(e: impl std::fmt::Display) -> AikitError {
    AikitError::new("placement.path_or_projection", e.to_string())
}
