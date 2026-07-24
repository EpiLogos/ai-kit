//! Staging: consequences before commitment.
//!
//! `Space` puts a toggle in a set. Nothing else happens. `Ctrl+Enter` applies the
//! whole set at once. Between the two, the palette asks the backend to *resolve*
//! the hypothetical view and shows what came back: which dependencies come along,
//! which go away, which clients notice immediately and which need a restart, and —
//! most importantly — whether the set resolves at all.
//!
//! ## Why staging provably cannot write
//!
//! [`stage`] takes `&dyn PaletteBackend`, not `&mut`. The only mutating methods on
//! that trait are `apply`, `start`, `promote` and `open_source`, and none of them
//! is reachable from here. This is not a convention the tests check after the
//! fact; it is a borrow the compiler checks before there are any tests.
//!
//! ## Why a failure is a first-class outcome
//!
//! Rule 4 of the resolver (`ARCHITECTURE.md` §4) makes an explicitly disabled
//! requirement a *failure*, not a silent re-enable. That failure is the most
//! useful thing the palette can show, and it must arrive while the user is still
//! choosing rather than after they have committed. [`StagedProblem`] therefore
//! reads the resolver's own structured details — `capability`, `required_by`,
//! `conflicts_with` — rather than parsing its prose, so the conflict dialog and
//! the dependency warning are built from the same facts `aikit explain` uses.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::error::AikitError;
use aikit_core::id::CapsuleId;
use aikit_core::resolve::ResolvedView;
use aikit_core::scope::ScopeKind;

use crate::backend::{ClientEffect, PaletteBackend, Projected, Toggle};

/// The set of changes a user has staged and not yet applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StagedSet {
    /// Sorted, so the applied order is the same every time and a lock file diff
    /// is reviewable.
    toggles: BTreeMap<CapsuleId, bool>,
}

impl StagedSet {
    /// `Space`. Stages the opposite of the state the row is *showing*, and a
    /// second press removes the staging rather than staging a second flip — which
    /// would be a no-op the user would then have to apply.
    ///
    /// `currently_on` is [`is_on`], not `is_declared_enabled`. A capability that
    /// is live only because something requires it reads as on, and pressing
    /// `Space` on it must mean "switch this off", not "declare the thing that is
    /// already happening".
    pub fn toggle(&mut self, id: &CapsuleId, currently_on: bool) {
        if self.toggles.remove(id).is_some() {
            return;
        }
        self.toggles.insert(id.clone(), !currently_on);
    }

    /// Stage an explicit state, used when a conflict dialog resolves a choice.
    pub fn set(&mut self, id: &CapsuleId, enable: bool) {
        self.toggles.insert(id.clone(), enable);
    }

    pub fn remove(&mut self, id: &CapsuleId) {
        self.toggles.remove(id);
    }

    pub fn clear(&mut self) {
        self.toggles.clear();
    }

    pub fn state_of(&self, id: &CapsuleId) -> Option<bool> {
        self.toggles.get(id).copied()
    }

    pub fn len(&self) -> usize {
        self.toggles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.toggles.is_empty()
    }

    pub fn toggles(&self) -> Vec<Toggle> {
        self.toggles
            .iter()
            .map(|(capsule, enable)| Toggle::new(capsule.clone(), *enable))
            .collect()
    }
}

/// What applying the staged set would do.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedDiff {
    pub requested: Vec<Toggle>,
    /// Capabilities that would become active without being asked for.
    pub added_dependencies: Vec<CapsuleId>,
    /// Capabilities that would stop being active without being asked for.
    pub dropped_dependencies: Vec<CapsuleId>,
    /// Declared-enabled capabilities the projected view still cannot activate,
    /// with the resolver's reason.
    pub still_unavailable: Vec<(CapsuleId, String)>,
    pub client_effects: Vec<ClientEffect>,
    /// The view that would result, kept so the preview pane can explain any row
    /// in the *staged* world rather than the current one.
    pub projected: ResolvedView,
}

impl StagedDiff {
    /// The number of clients that need the user to do something.
    pub fn client_restarts(&self) -> usize {
        self.client_effects
            .iter()
            .filter(|e| e.effect.needs_user_action())
            .count()
    }

    /// The footer line: `3 staged changes · +2 dependencies · 1 client restart`.
    ///
    /// Zero-valued clauses are omitted rather than printed as "0 dependencies",
    /// because a footer that always shows every counter trains people to stop
    /// reading it.
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
    if n == 1 {
        one
    } else {
        many
    }
}

/// A staged set the resolver refuses, classified enough to act on.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedProblem {
    pub error: AikitError,
    pub kind: ProblemKind,
}

/// The shapes of refusal the palette can offer a next step for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemKind {
    /// Two capabilities cannot both be active. The user picks one.
    Conflict { left: CapsuleId, right: CapsuleId },
    /// Switching `capability` off would break `dependent`, which is still on.
    BreaksDependent {
        capability: CapsuleId,
        dependent: CapsuleId,
    },
    /// Enabling something no registry has.
    UnknownCapability { capability: CapsuleId },
    /// Anything else the resolver refuses. Shown with its code, never swallowed.
    Unclassified,
}

impl StagedProblem {
    /// Read the resolver's structured details.
    ///
    /// Details rather than message text: `STANDARDS.md` §3 makes details
    /// structured precisely so a UI can render them, and message wording is
    /// explicitly not stable.
    pub fn from_error(error: AikitError) -> Self {
        let detail = |key: &str| error.details().get(key).and_then(|v| CapsuleId::parse(v).ok());
        let kind = match error.code() {
            "resolution.conflict" => match (detail("capability"), detail("conflicts_with")) {
                (Some(left), Some(right)) => ProblemKind::Conflict { left, right },
                _ => ProblemKind::Unclassified,
            },
            "resolution.required_capability_disabled" => {
                match (detail("capability"), detail("required_by")) {
                    (Some(capability), Some(dependent)) => ProblemKind::BreaksDependent {
                        capability,
                        dependent,
                    },
                    _ => ProblemKind::Unclassified,
                }
            }
            "resolution.unknown_capability" | "resolution.missing_dependency" => {
                match detail("capability") {
                    Some(capability) => ProblemKind::UnknownCapability { capability },
                    None => ProblemKind::Unclassified,
                }
            }
            _ => ProblemKind::Unclassified,
        };
        Self { error, kind }
    }

    pub fn code(&self) -> &'static str {
        self.error.code()
    }

    /// The sentence at the top of the dialog. The resolver's own wording, which
    /// already names the file and line where one is known.
    pub fn headline(&self) -> String {
        self.error.message().to_string()
    }

    /// The ways out, when there are any. Empty means the only move is `Esc`.
    pub fn choices(&self) -> Vec<String> {
        match &self.kind {
            ProblemKind::Conflict { left, right } => {
                vec![format!("keep {left}"), format!("keep {right}")]
            }
            ProblemKind::BreaksDependent {
                capability,
                dependent,
            } => vec![
                format!("keep {capability} enabled"),
                format!("switch off {dependent} as well"),
            ],
            ProblemKind::UnknownCapability { .. } | ProblemKind::Unclassified => vec![],
        }
    }
}

/// The result of staging: consequences, or the refusal that stopped them.
pub type StagedOutcome = std::result::Result<StagedDiff, StagedProblem>;

/// Does this row read as "on" to the user?
///
/// Deliberately the union of active and declared-enabled. Active-but-undeclared
/// is a dependency; declared-but-inactive is something held back by trust or
/// policy. Both look like a switch that is up, and `Space` on either of them
/// means "switch it down".
pub fn is_on(view: &ResolvedView, id: &CapsuleId) -> bool {
    view.is_active(id) || view.is_declared_enabled(id)
}

/// Ask the backend what this staged set would do. Writes nothing.
// `StagedProblem` is ~136 bytes, but this returns once per `Space` keypress on a
// cold path, never in a loop; boxing it would only obscure the ergonomic
// `StagedOutcome` alias for no measurable gain.
#[allow(clippy::result_large_err)]
pub fn stage(
    backend: &dyn PaletteBackend,
    scope: ScopeKind,
    staged: &StagedSet,
) -> StagedOutcome {
    let toggles = staged.toggles();
    let projected = backend
        .preview(scope, &toggles)
        .map_err(StagedProblem::from_error)?;
    Ok(diff(backend.view(), &projected, toggles))
}

/// Compare the current effective view with the projected one.
fn diff(current: &ResolvedView, projected: &Projected, requested: Vec<Toggle>) -> StagedDiff {
    let asked: BTreeSet<&CapsuleId> = requested.iter().map(|t| &t.capsule).collect();

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

    // Declared on, still held back. This is the case the three-state rendering
    // exists for: the toggle "worked" and the capability is still not live.
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
