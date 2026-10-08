//! Effective repertoire disclosure shared by CLI, TUI and delegation consumers.
//! These facts project the existing resolver and accepted capsule sources; they
//! do not grant authority, choose a harness or create another capability store.
use crate::id::{CapsuleId, GenerationId, ProfileId};
use crate::procedure::{Procedure, ProcedureDiff};
use crate::projection::{ActivationEffect, ProjectionItem, ProjectionPlan};
use crate::scope::ScopeKind;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const REPERTOIRE_SCHEMA: &str = "aikit.resolved-repertoire/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoireMember {
    pub id: CapsuleId,
    pub revision: Option<String>,
    pub practice: String,
    pub source_root: Option<PathBuf>,
    pub projected: bool,
    pub withheld_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoireTarget {
    pub target: String,
    pub digest: String,
    pub items: usize,
    pub activation: ActivationEffect,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoireReading {
    pub schema: String,
    pub context_id: String,
    /// Current generation at resolution/preview time. Target digests describe
    /// the selected plan; only an application readback establishes publication.
    pub generation: Option<GenerationId>,
    pub resolution_hash: String,
    pub profiles: Vec<ProfileId>,
    pub skill_sets: Vec<String>,
    pub members: Vec<RepertoireMember>,
    pub targets: Vec<RepertoireTarget>,
    /// These read declarations currently stored at this exact context. A
    /// hypothetical preview selection must be applied before these export it.
    pub package_commands: Vec<String>,
}

impl RepertoireReading {
    /// Compact human/TUI rendering of the same structured reading agents read.
    pub fn render(&self) -> String {
        let projected = self
            .members
            .iter()
            .filter(|member| member.projected)
            .count();
        let mut lines = vec![format!(
            "repertoire {}: {projected} effective, {} withheld; {} set(s)",
            self.context_id,
            self.members.len() - projected,
            self.skill_sets.len()
        )];
        lines.push(format!(
            "current generation: {}",
            self.generation
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "none".into())
        ));
        if !self.profiles.is_empty() {
            lines.push(format!(
                "profiles: {}",
                self.profiles
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !self.skill_sets.is_empty() {
            lines.push(format!("sets: {}", self.skill_sets.join(", ")));
        }
        for target in &self.targets {
            lines.push(format!(
                "{}: {} ({} managed items)",
                target.target,
                target.activation.describe(),
                target.items
            ));
        }
        for member in self.members.iter().filter(|member| !member.projected) {
            lines.push(format!(
                "withheld {}: {}",
                member.id,
                member.withheld_reason.as_deref().unwrap_or("not selected")
            ));
        }
        lines.push("inspect detail with --json; package plan here: aikit set package plan . --target pi (or --target claude); JSON package_commands retains the exact context and reads its current declarations".into());
        lines.join("\n")
    }
}

/// Executed generation work, observed outside content and operation identity.
/// Writes count successful file-content operations (including staging); links
/// are separate. Tree scans count begun native traversals, entries successful
/// yields, and payload hashes completed folds, not inferred file counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationObservation {
    pub target_content_writes: u64,
    pub target_link_writes: u64,
    pub destination_checks: u64,
    pub destination_tree_scans: u64,
    pub destination_entries_inspected: u64,
    pub tree_hash_operations: u64,
    pub payload_hash_operations: u64,
    pub canonical_file_reads: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoireRequest {
    pub scope: ScopeKind,
    #[serde(default)]
    pub profile: Option<ProfileId>,
    /// Additive at this scope. Existing/project/session members remain unioned.
    #[serde(default)]
    pub skill_sets: Vec<String>,
}

/// Safe, freshly derived native plan metadata. Instruction and environment
/// bodies never enter this report; copies/links identify their actual source,
/// and writes identify their existing native content digest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepertoireItemKind {
    Link,
    Copy,
    Write,
    Shim,
    Environment,
    SecretEnvironment,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RepertoirePlanItem {
    pub kind: RepertoireItemKind,
    pub destination: Option<PathBuf>,
    pub name: Option<String>,
    pub source: Option<String>,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoirePlanSummary {
    pub target: String,
    pub digest: String,
    pub items: Vec<RepertoirePlanItem>,
    /// Compares selected native plans, not retained destination health. Apply
    /// and reconcile still inspect the actual managed generation for drift.
    pub differs_from_current_selection: bool,
    pub activation: ActivationEffect,
}

impl RepertoirePlanSummary {
    pub fn from_plan(plan: &ProjectionPlan, current: Option<&ProjectionPlan>) -> Self {
        let mut items: Vec<_> = plan
            .items
            .iter()
            .map(|item| {
                let (kind, name, source, content_hash) = match item {
                    ProjectionItem::Link { from, .. } => (
                        RepertoireItemKind::Link,
                        None,
                        Some(from.display().to_string()),
                        None,
                    ),
                    ProjectionItem::Copy { from, .. } => (
                        RepertoireItemKind::Copy,
                        None,
                        Some(from.display().to_string()),
                        None,
                    ),
                    ProjectionItem::Write { contents, .. } => (
                        RepertoireItemKind::Write,
                        None,
                        None,
                        Some(blake3::hash(contents.as_bytes()).to_hex().to_string()),
                    ),
                    ProjectionItem::Shim {
                        name,
                        capsule,
                        export,
                    } => (
                        RepertoireItemKind::Shim,
                        Some(name.clone()),
                        Some(format!("{capsule}#{export}")),
                        None,
                    ),
                    ProjectionItem::Env { name, .. } => (
                        RepertoireItemKind::Environment,
                        Some(name.clone()),
                        None,
                        None,
                    ),
                    ProjectionItem::SecretEnv { name, secret_ref } => (
                        RepertoireItemKind::SecretEnvironment,
                        Some(name.clone()),
                        Some(secret_ref.to_string()),
                        None,
                    ),
                };
                RepertoirePlanItem {
                    kind,
                    destination: item.destination().map(PathBuf::from),
                    name,
                    source,
                    content_hash,
                }
            })
            .collect();
        items.sort();
        Self {
            target: plan.target.to_string(),
            digest: plan.digest(),
            items,
            differs_from_current_selection: current
                .is_none_or(|current| current.digest() != plan.digest()),
            activation: plan.effect.clone(),
        }
    }

    /// Advisory pickup and current-selection observations can change after a
    /// legitimate interrupted apply. The reviewed native material cannot.
    pub fn same_material(&self, other: &Self) -> bool {
        self.target == other.target && self.digest == other.digest && self.items == other.items
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoirePreview {
    pub request: RepertoireRequest,
    pub reading: RepertoireReading,
    pub procedure: Procedure,
    pub diff: ProcedureDiff,
    /// Full safe metadata for the hypothetical native plans reviewed here.
    #[serde(default)]
    pub target_plans: Vec<RepertoirePlanSummary>,
    /// Package commands resolve the context's currently stored declarations.
    #[serde(default)]
    pub package_command_basis: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoireApplication {
    pub reading: RepertoireReading,
    pub procedure: crate::ProcedureId,
    pub applied_edits: usize,
    pub reused_generation: bool,
    pub recovered: bool,
    pub undo: String,
    /// Actual execution observation, absent for an unmeasured retained Return.
    #[serde(default)]
    pub observation: Option<RepertoireApplicationObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepertoireApplicationObservation {
    pub generation: GenerationObservation,
    pub elapsed_apply_ms: u64,
}

impl RepertoireApplicationObservation {
    pub fn render(&self) -> String {
        let work = &self.generation;
        format!("observed apply: {} ms; {} content write(s), {} link write(s); {} destination check(s), {} tree scan(s), {} entries; {} tree hash(es), {} payload fold(s), {} canonical read(s)", self.elapsed_apply_ms, work.target_content_writes, work.target_link_writes, work.destination_checks, work.destination_tree_scans, work.destination_entries_inspected, work.tree_hash_operations, work.payload_hash_operations, work.canonical_file_reads)
    }
}

impl RepertoireApplication {
    pub fn render(&self) -> String {
        format!(
            "{}\n{} managed edit(s); generation {}; procedure {}; undo: {}\n{}",
            self.reading.render(),
            self.applied_edits,
            self.reading
                .generation
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unavailable".into()),
            self.procedure,
            self.undo,
            self.observation
                .as_ref()
                .map(RepertoireApplicationObservation::render)
                .unwrap_or_else(
                    || "operation measurements unavailable for this retained Return".into()
                )
        )
    }
}
impl RepertoirePreview {
    pub fn render(&self) -> String {
        let mut lines = vec![
            self.reading.render(),
            format!(
                "scope: {}; {} reversible managed edit(s); procedure {}",
                self.request.scope.as_str(),
                self.diff.edits.len(),
                self.procedure.id
            ),
        ];
        for edit in &self.diff.edits {
            lines.push(match &edit.path {
                Some(path) => format!("{}: {}", path.display(), edit.description),
                None => edit.description.clone(),
            });
        }
        for plan in &self.target_plans {
            lines.push(format!(
                "{} proposed plan: {} selected plan; {}",
                plan.target,
                if plan.differs_from_current_selection {
                    "different from current"
                } else {
                    "same as current"
                },
                plan.activation.describe()
            ));
            for item in plan.items.iter().take(8) {
                let destination = item
                    .destination
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .or_else(|| item.name.clone())
                    .unwrap_or_else(|| "managed item".into());
                let kind = match item.kind {
                    RepertoireItemKind::Link => "link",
                    RepertoireItemKind::Copy => "copy",
                    RepertoireItemKind::Write => "write",
                    RepertoireItemKind::Shim => "shim",
                    RepertoireItemKind::Environment => "environment",
                    RepertoireItemKind::SecretEnvironment => "secret reference",
                };
                let basis = item
                    .source
                    .as_ref()
                    .map(|source| format!(" from {source}"))
                    .or_else(|| {
                        item.content_hash
                            .as_ref()
                            .map(|hash| format!(" content {hash}"))
                    })
                    .unwrap_or_default();
                lines.push(format!("  {kind} {destination}{basis}"));
            }
            if plan.items.len() > 8 {
                lines.push(format!(
                    "  {} more managed item(s) in --json",
                    plan.items.len() - 8
                ));
            }
        }
        lines.push("Package commands read current scope declarations; apply this preview before exporting its proposed selection.".into());
        lines.push("Inspect all proposed target metadata with --json; apply checks the exact retained native plans.".into());
        lines.join("\n")
    }
}
