//! Surfacing: how an external-facing capability's output reaches the user.
//!
//! This is the one genuinely new mechanic the composables spec introduces. An
//! internal-facing capability's deliverable flows back into the agent's context and
//! needs no help; an external-facing one has to reach a *screen*, and where that is
//! genuinely varies — a notebook cell, a browser tab, or a file on disk.
//!
//! The invariant this module owns is the honest one:
//!
//! > **A headless context reports the artifact's path; it never claims to have
//! > shown anything.**
//!
//! `STANDARDS.md §1` names silent degradation as a thing AIKit refuses to ship, and
//! this is exactly where the temptation lives: pretending a chart was displayed
//! when the process had no display is a lie the user only discovers by not
//! learning the thing the chart existed to teach.
//!
//! Deciding is pure and testable: the environment is passed in, never read here.

use serde::{Deserialize, Serialize};

use crate::capsule::{Facing, Surface};

/// What the surroundings can actually display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayContext {
    /// A reactive notebook session (marimo, Jupyter) is present.
    pub notebook: bool,
    /// A browser can be opened — there is a display and the process is interactive.
    pub browser: bool,
    /// Nothing can be shown: CI, a pipe, a detached run.
    pub headless: bool,
}

impl DisplayContext {
    /// Decide from the ambient facts a caller has gathered.
    ///
    /// `notebook_marker` is the presence of a notebook kernel (the CLI passes
    /// whether a marimo/Jupyter environment variable is set); `interactive` is
    /// whether a terminal is attached; `ci` is the usual CI signal. A CI run is
    /// headless even when a terminal is attached, because nobody is watching it.
    pub fn detect(notebook_marker: bool, interactive: bool, ci: bool) -> Self {
        let headless = ci || !interactive;
        Self {
            notebook: notebook_marker && !ci,
            browser: !headless,
            headless,
        }
    }

    /// The context of a plain interactive terminal.
    pub fn interactive_terminal() -> Self {
        Self {
            notebook: false,
            browser: true,
            headless: false,
        }
    }

    /// The context of a CI run or a pipe.
    pub fn headless() -> Self {
        Self {
            notebook: false,
            browser: false,
            headless: true,
        }
    }
}

/// How output will actually reach the user, and whether that is what was asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "surfacing", rename_all = "kebab-case")]
pub enum SurfacingPlan {
    /// Rendered inline in the notebook — the reactive, native home.
    Notebook,
    /// A browser tab is opened.
    Browser,
    /// A self-contained artifact is written and its path reported. This is the
    /// honest headless answer, and it is a *success*, not a failure.
    Artifact { reason: String },
    /// Nothing is shown because nothing is meant to be: the capability's
    /// deliverable feeds the agent, not the user.
    NotShown,
}

impl SurfacingPlan {
    pub fn as_str(&self) -> &'static str {
        match self {
            SurfacingPlan::Notebook => "notebook",
            SurfacingPlan::Browser => "browser",
            SurfacingPlan::Artifact { .. } => "artifact",
            SurfacingPlan::NotShown => "not-shown",
        }
    }

    /// Whether the user will actually see this without opening a file themselves.
    pub fn reaches_the_user_directly(&self) -> bool {
        matches!(self, SurfacingPlan::Notebook | SurfacingPlan::Browser)
    }

    /// The sentence a user is owed about where their output went.
    pub fn describe(&self) -> String {
        match self {
            SurfacingPlan::Notebook => "rendered in the notebook".to_string(),
            SurfacingPlan::Browser => "opened in a browser tab".to_string(),
            SurfacingPlan::Artifact { reason } => {
                format!("written as a self-contained artifact ({reason})")
            }
            SurfacingPlan::NotShown => {
                "not shown: this capability's output feeds the agent's work".to_string()
            }
        }
    }
}

/// Decide how an external-facing capability's output should reach the user.
///
/// The declared [`Surface`] is a *preference*, and the surroundings get the final
/// say — but a downgrade is always reported rather than performed silently. A
/// capability that asked for a browser and got an artifact says so, with the
/// reason, so nobody is left believing a window opened somewhere.
pub fn plan_surfacing(
    facing: Facing,
    declared: Option<Surface>,
    context: &DisplayContext,
) -> SurfacingPlan {
    if !facing.shows_the_user() {
        return SurfacingPlan::NotShown;
    }

    // An undeclared surface takes the best the surroundings offer, which is the
    // right default for a capability whose author had no opinion.
    let preference = declared.unwrap_or(if context.notebook {
        Surface::Notebook
    } else if context.browser {
        Surface::Browser
    } else {
        Surface::ArtifactPath
    });

    match preference {
        Surface::Notebook if context.notebook => SurfacingPlan::Notebook,
        Surface::Notebook if context.browser => SurfacingPlan::Browser,
        Surface::Notebook => SurfacingPlan::Artifact {
            reason: "a notebook was asked for, and there is no notebook session or display here"
                .to_string(),
        },
        Surface::Browser if context.browser => SurfacingPlan::Browser,
        Surface::Browser => SurfacingPlan::Artifact {
            reason: "a browser was asked for, and this context has no display".to_string(),
        },
        Surface::ArtifactPath => SurfacingPlan::Artifact {
            reason: "this capability writes an artifact by design".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_internal_facing_capability_is_never_surfaced() {
        assert_eq!(
            plan_surfacing(
                Facing::Internal,
                None,
                &DisplayContext::interactive_terminal()
            ),
            SurfacingPlan::NotShown
        );
        // Even one that mistakenly carries a surface preference.
        assert_eq!(
            plan_surfacing(
                Facing::Internal,
                Some(Surface::Browser),
                &DisplayContext::interactive_terminal()
            ),
            SurfacingPlan::NotShown
        );
    }

    #[test]
    fn a_notebook_session_gets_the_native_reactive_rendering() {
        let notebook = DisplayContext::detect(true, true, false);
        assert_eq!(
            plan_surfacing(Facing::External, Some(Surface::Notebook), &notebook),
            SurfacingPlan::Notebook
        );
    }

    #[test]
    fn a_headless_run_writes_an_artifact_and_says_why_rather_than_pretending() {
        let plan = plan_surfacing(
            Facing::External,
            Some(Surface::Browser),
            &DisplayContext::headless(),
        );
        match &plan {
            SurfacingPlan::Artifact { reason } => {
                assert!(
                    reason.contains("no display"),
                    "the reason is stated: {reason}"
                );
            }
            other => panic!("a headless context must not claim to show anything: {other:?}"),
        }
        assert!(!plan.reaches_the_user_directly());
    }

    #[test]
    fn a_notebook_preference_falls_back_to_a_browser_before_a_file() {
        // Asking for a notebook in a terminal should still *show* something rather
        // than dropping straight to a file the user has to go and open.
        let terminal = DisplayContext::interactive_terminal();
        assert_eq!(
            plan_surfacing(Facing::External, Some(Surface::Notebook), &terminal),
            SurfacingPlan::Browser
        );
    }

    #[test]
    fn an_artifact_by_design_stays_an_artifact_even_with_a_display() {
        // `improve-codebase-architecture` writes a self-contained HTML report; that
        // is the deliverable, not a degradation.
        let plan = plan_surfacing(
            Facing::External,
            Some(Surface::ArtifactPath),
            &DisplayContext::interactive_terminal(),
        );
        assert!(matches!(plan, SurfacingPlan::Artifact { .. }));
        assert!(plan.describe().contains("by design"));
    }

    #[test]
    fn ci_is_headless_even_with_a_terminal_attached() {
        let ci = DisplayContext::detect(true, true, true);
        assert!(ci.headless, "nobody is watching a CI run");
        assert!(
            !ci.notebook,
            "a notebook marker in CI is not a live session"
        );
    }

    #[test]
    fn an_undeclared_surface_takes_the_best_the_surroundings_offer() {
        assert_eq!(
            plan_surfacing(
                Facing::External,
                None,
                &DisplayContext::detect(true, true, false)
            ),
            SurfacingPlan::Notebook
        );
        assert_eq!(
            plan_surfacing(
                Facing::External,
                None,
                &DisplayContext::interactive_terminal()
            ),
            SurfacingPlan::Browser
        );
        assert!(matches!(
            plan_surfacing(Facing::External, None, &DisplayContext::headless()),
            SurfacingPlan::Artifact { .. }
        ));
    }

    #[test]
    fn a_both_facing_capability_surfaces_because_it_also_teaches() {
        assert_eq!(
            plan_surfacing(Facing::Both, None, &DisplayContext::interactive_terminal()),
            SurfacingPlan::Browser
        );
    }
}
