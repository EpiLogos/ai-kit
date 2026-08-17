//! Package-backed Capability state and read-only CLI preview compatibility.
//!
//! Canonical V2 staging lives in [`crate::application::StagedChanges`] and follows
//! the one staging -> preview/explain -> confirm -> apply route. Nothing in this
//! module can apply or mutate application state.
//!
//! `StagedSet` / `StagedDiff` / [`stage`] are retained only for the published
//! `aikit diff` compatibility verb while CLI package commands still speak
//! `CapsuleId`. [`stage`] accepts `&dyn PaletteBackend` and can therefore only ask
//! the shared backend to resolve a hypothetical preview. It is not a second
//! staging owner and must not be used by the V2 terminal surface.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::error::AikitError;
use aikit_core::id::CapsuleId;
use aikit_core::resolve::ResolvedView;
use aikit_core::scope::ScopeKind;

use crate::backend::{ClientEffect, PaletteBackend, Projected, Toggle};

/// Does a proven package-backed Capability read as enabled to compatibility code?
pub fn is_on(view: &ResolvedView, id: &CapsuleId) -> bool {
    view.is_active(id) || view.is_declared_enabled(id)
}

/// Read-only package-toggle input used by the external `aikit diff` boundary.
///
/// The V2 application surface must use `application::StagedChanges` instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StagedSet {
    toggles: BTreeMap<CapsuleId, bool>,
}

impl StagedSet {
    pub fn set(&mut self, id: &CapsuleId, enable: bool) {
        self.toggles.insert(id.clone(), enable);
    }

    pub fn toggles(&self) -> Vec<Toggle> {
        self.toggles
            .iter()
            .map(|(capsule, enable)| Toggle::new(capsule.clone(), *enable))
            .collect()
    }
}

/// Read-only consequences returned to the compatibility CLI preview.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedDiff {
    pub requested: Vec<Toggle>,
    pub added_dependencies: Vec<CapsuleId>,
    pub dropped_dependencies: Vec<CapsuleId>,
    pub still_unavailable: Vec<(CapsuleId, String)>,
    pub client_effects: Vec<ClientEffect>,
    pub projected: ResolvedView,
}

impl StagedDiff {
    pub fn client_restarts(&self) -> usize {
        self.client_effects
            .iter()
            .filter(|effect| effect.effect.needs_user_action())
            .count()
    }

    pub fn footer(&self) -> String {
        let mut parts = vec![format!(
            "{} staged {}",
            self.requested.len(),
            plural(self.requested.len(), "change", "changes")
        )];
        if !self.added_dependencies.is_empty() {
            parts.push(format!(
                "+{} {}",
                self.added_dependencies.len(),
                plural(self.added_dependencies.len(), "dependency", "dependencies")
            ));
        }
        if !self.dropped_dependencies.is_empty() {
            parts.push(format!(
                "−{} {}",
                self.dropped_dependencies.len(),
                plural(
                    self.dropped_dependencies.len(),
                    "dependency",
                    "dependencies"
                )
            ));
        }
        let restarts = self.client_restarts();
        if restarts > 0 {
            parts.push(format!(
                "{restarts} client {}",
                plural(restarts, "restart", "restarts")
            ));
        }
        if !self.still_unavailable.is_empty() {
            parts.push(format!(
                "{} still unavailable",
                self.still_unavailable.len()
            ));
        }
        parts.join(" · ")
    }
}

fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 { one } else { many }
}

/// Resolver failure surfaced by the read-only compatibility preview.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedProblem {
    error: AikitError,
}

impl StagedProblem {
    fn from_error(error: AikitError) -> Self {
        Self { error }
    }

    pub fn code(&self) -> &'static str {
        self.error.code()
    }

    pub fn headline(&self) -> String {
        self.error.message().to_string()
    }
}

pub type StagedOutcome = std::result::Result<StagedDiff, StagedProblem>;

/// Resolve a hypothetical package-toggle set for the external `aikit diff`
/// boundary. This function has no mutable backend access and cannot apply.
#[allow(clippy::result_large_err)]
pub fn stage(backend: &dyn PaletteBackend, scope: ScopeKind, staged: &StagedSet) -> StagedOutcome {
    let requested = staged.toggles();
    let projected = backend
        .preview(scope, &requested)
        .map_err(StagedProblem::from_error)?;
    Ok(diff(backend.view(), &projected, requested))
}

fn diff(current: &ResolvedView, projected: &Projected, requested: Vec<Toggle>) -> StagedDiff {
    let asked: BTreeSet<&CapsuleId> = requested.iter().map(|toggle| &toggle.capsule).collect();

    let added_dependencies = projected
        .view
        .active
        .keys()
        .filter(|id| !current.is_active(id) && !asked.contains(id))
        .cloned()
        .collect();

    let dropped_dependencies = current
        .active
        .keys()
        .filter(|id| !projected.view.is_active(id) && !asked.contains(id))
        .cloned()
        .collect();

    let still_unavailable = projected
        .view
        .unavailable
        .iter()
        .filter(|(id, _)| projected.view.is_declared_enabled(id))
        .map(|(id, reason)| (id.clone(), reason.describe()))
        .collect();

    StagedDiff {
        requested,
        added_dependencies,
        dropped_dependencies,
        still_unavailable,
        client_effects: projected.effects.clone(),
        projected: projected.view.clone(),
    }
}
