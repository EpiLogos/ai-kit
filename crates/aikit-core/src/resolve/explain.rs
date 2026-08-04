//! Explanation.
//!
//! `aikit explain <capsule>` answers the question a user actually has, which is
//! not "what is in my profile" but "why is this capability affecting this
//! session". Everything needed to answer it is already in the resolved view, so
//! explanation is a pure projection rather than a second resolution.

use serde::{Deserialize, Serialize};

use crate::capsule::{Kind, Maturity};
use crate::id::{CapsuleId, Revision};
use crate::platform::TargetId;
use crate::trust::TrustState;

use super::{AppliedSkillUsageOverlay, ResolvedView, UnavailableReason};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Explanation {
    pub id: CapsuleId,
    pub kind: Kind,
    pub name: String,
    pub revision: Option<Revision>,
    pub maturity: Maturity,
    pub trust: TrustState,
    /// True when the capability is in the effective view.
    pub active: bool,
    /// True when a layer says it should be, whether or not it could be.
    pub declared_enabled: bool,
    /// Human-readable selection reasons, most specific last.
    pub selected_by: Vec<String>,
    pub required_by: Vec<String>,
    pub dependencies: Vec<String>,
    pub unavailable: Option<UnavailableReason>,
    pub targets: Vec<TargetId>,
    /// What "active" means for this kind.
    pub activation_meaning: &'static str,
    pub exports: Vec<String>,
    /// Advisory "often used with…" pointers (L5). Never a dependency.
    pub related_skills: Vec<CapsuleId>,
    pub skill_usage_overlays: Vec<AppliedSkillUsageOverlay>,
}

pub(super) fn explain(view: &ResolvedView, id: &CapsuleId) -> Option<Explanation> {
    let entry = view.catalog_index.get(id)?;

    // Every layer operation that mentioned this capsule, in application order.
    let mut selected_by: Vec<String> = view
        .selection_log
        .iter()
        .filter(|op| &op.capsule == id)
        .map(|op| op.describe())
        .collect();

    if let Some(active) = view.active.get(id) {
        if let super::SelectionOrigin::Dependency { required_by } = &active.origin {
            selected_by.push(format!("required by {required_by}"));
        }
    }
    if view.policy.requires(id) {
        selected_by.push(format!("required by managed policy {}", view.policy.source));
    }

    let (dependencies, required_by, targets) = match view.active.get(id) {
        Some(active) => (
            active.dependencies.iter().map(|d| d.to_string()).collect(),
            active.required_by.iter().map(|d| d.to_string()).collect(),
            active.targets.clone(),
        ),
        None => (Vec::new(), Vec::new(), Vec::new()),
    };

    Some(Explanation {
        id: id.clone(),
        kind: entry.kind,
        name: entry.name.clone(),
        revision: entry.revision.clone(),
        maturity: entry.maturity,
        trust: entry.trust,
        active: view.is_active(id),
        declared_enabled: view.is_declared_enabled(id),
        selected_by,
        required_by,
        dependencies,
        unavailable: view.unavailable.get(id).cloned(),
        targets,
        activation_meaning: entry.kind.activation_meaning(),
        exports: entry.exports.clone(),
        related_skills: entry.related_skills.clone(),
        skill_usage_overlays: view
            .skill_usage_overlays
            .get(id)
            .cloned()
            .unwrap_or_default(),
    })
}

impl Explanation {
    /// The plain-text rendering used by `aikit explain` without `--json`.
    pub fn render(&self) -> String {
        let mut out = String::new();
        match &self.revision {
            Some(rev) => out.push_str(&format!("{}@{}\n", self.id, rev.short())),
            None => out.push_str(&format!("{}\n", self.id)),
        }
        out.push_str(&format!(
            "State: {}\n",
            if self.active {
                "active".to_string()
            } else if let Some(reason) = &self.unavailable {
                format!("unavailable — {}", reason.describe())
            } else if self.declared_enabled {
                "declared enabled".to_string()
            } else {
                "inactive".to_string()
            }
        ));
        out.push_str(&format!("Trust: {}\n", self.trust));
        out.push_str(&format!("Maturity: {}\n", self.maturity.as_str()));

        out.push_str("Selected by:\n");
        if self.selected_by.is_empty() {
            out.push_str("  nothing — no scope selects it\n");
        } else {
            for reason in &self.selected_by {
                out.push_str(&format!("  {reason}\n"));
            }
        }

        out.push_str("Required by:\n");
        if self.required_by.is_empty() {
            out.push_str("  none\n");
        } else {
            for r in &self.required_by {
                out.push_str(&format!("  {r}\n"));
            }
        }

        out.push_str("Dependencies:\n");
        if self.dependencies.is_empty() {
            out.push_str("  none\n");
        } else {
            for d in &self.dependencies {
                out.push_str(&format!("  {d}\n    selected transitively\n"));
            }
        }

        if !self.targets.is_empty() {
            out.push_str("Projection:\n");
            for target in &self.targets {
                out.push_str(&format!("  {target}\n"));
            }
        }

        if !self.related_skills.is_empty() {
            out.push_str("Often used with:\n");
            for related in &self.related_skills {
                out.push_str(&format!("  {related}\n"));
            }
        }

        if !self.skill_usage_overlays.is_empty() {
            out.push_str("Skill Usage Overlays:\n");
            for overlay in &self.skill_usage_overlays {
                out.push_str(&format!("  {} {}\n", overlay.scope, overlay.origin));
            }
        }

        out.push_str(&format!("Activation means: {}\n", self.activation_meaning));
        out
    }
}
