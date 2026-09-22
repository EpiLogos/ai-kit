//! Worktree projection: is a repository's checkout at the canonical target
//! (`origin/main`), and what *safe* action reconciles it when it is not.
//!
//! This is the git-repository sense of "projection" — the material state of a
//! working checkout against the canonical branch it is meant to track — and is
//! deliberately distinct from skill-source projection (harness-visible skill
//! copies) and from Workcell material projection (execution bindings). It is the
//! reconciliation the owner otherwise runs by hand as `git fetch` +
//! `git reset --hard origin/main` + `git clean`, per repository, per machine.
//!
//! The model here is I/O-free: it classifies an already-observed relation
//! (`ahead`/`behind` counts plus working-tree cleanliness) into a divergence and
//! a safe decision. The native Git adapter owns the process/filesystem mechanics
//! (fetch, the ancestry counts, and the one fast-forward it is allowed to make).
//!
//! The one non-negotiable law this module encodes: a projection **never**
//! discards uncommitted or unmerged work to force the target. A worktree that is
//! dirty, ahead, or diverged is *surfaced* as a delta a human must resolve — it
//! is never fast-forwarded, reset, or cleaned. Only a clean checkout that is
//! strictly behind the target (its HEAD an ancestor of the target) is
//! fast-forwarded, and only then when the caller asked to apply.

use serde::{Deserialize, Serialize};

use crate::project::ProjectRef;

use super::VersionRevision;

pub const WORKTREE_PROJECTION_VERSION: &str = "aikit.worktree-projection/v1";

/// The canonical revision a checkout is projected onto: a remote and a ref on
/// it, e.g. `origin` / `main`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionTarget {
    pub remote: String,
    pub reference: String,
}

impl Default for ProjectionTarget {
    fn default() -> Self {
        Self {
            remote: "origin".to_string(),
            reference: "main".to_string(),
        }
    }
}

impl ProjectionTarget {
    /// Parse a target spelling. `origin/main` names remote `origin`, ref `main`;
    /// a bare `main` names the default remote (`origin`) and that ref. An empty
    /// spelling is the default target.
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        if raw.is_empty() {
            return Self::default();
        }
        match raw.split_once('/') {
            Some((remote, reference)) if !remote.is_empty() && !reference.is_empty() => Self {
                remote: remote.to_string(),
                reference: reference.to_string(),
            },
            _ => Self {
                remote: "origin".to_string(),
                reference: raw.to_string(),
            },
        }
    }

    /// The revision expression a checkout is compared against, e.g. `origin/main`.
    pub fn qualified(&self) -> String {
        format!("{}/{}", self.remote, self.reference)
    }
}

/// How a checkout's HEAD relates to the target revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "relation", rename_all = "kebab-case")]
pub enum Divergence {
    /// HEAD is exactly the target.
    UpToDate,
    /// HEAD is strictly behind the target: the target has commits HEAD lacks and
    /// HEAD has none the target lacks. A fast-forward is possible.
    Behind { by: u64 },
    /// HEAD has commits the target lacks, but is not behind it.
    Ahead { by: u64 },
    /// HEAD and the target have each diverged from their merge base.
    Diverged { ahead: u64, behind: u64 },
    /// The target revision could not be resolved (unknown remote/ref, or never
    /// fetched).
    TargetMissing,
    /// The checkout itself could not be observed (not a git repository, empty,
    /// or unreadable). Never produced by `classify`; carried on a `Failed` entry.
    Unknown,
}

impl Divergence {
    /// Classify from the target's presence and the `ahead`/`behind` counts of
    /// HEAD relative to the target (as `git rev-list --left-right --count
    /// HEAD...<target>` reports them: left = ahead, right = behind).
    pub fn classify(target_present: bool, ahead: u64, behind: u64) -> Self {
        if !target_present {
            return Self::TargetMissing;
        }
        match (ahead, behind) {
            (0, 0) => Self::UpToDate,
            (0, behind) => Self::Behind { by: behind },
            (ahead, 0) => Self::Ahead { by: ahead },
            (ahead, behind) => Self::Diverged { ahead, behind },
        }
    }
}

/// The safe reconciliation decision for one checkout — a pure function of its
/// divergence and cleanliness. The adapter turns `FastForward` into the one
/// mutation it is allowed to make; everything else is read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionDecision {
    /// HEAD already sits at the target. Nothing to do.
    AlreadyProjected,
    /// A clean checkout that is strictly behind the target: a fast-forward is
    /// safe and loses nothing.
    FastForward,
    /// Drift a human must resolve — dirty, ahead, diverged, or a missing target.
    /// The worktree is never mutated to force the target.
    Surface,
}

/// Decide the safe action for a checkout. The dirty-and-behind case is the one
/// that most tempts a `reset --hard`; it is refused here so live work is never
/// silently discarded.
pub fn decide(divergence: &Divergence, clean: bool) -> ProjectionDecision {
    match divergence {
        Divergence::UpToDate => ProjectionDecision::AlreadyProjected,
        Divergence::Behind { .. } if clean => ProjectionDecision::FastForward,
        // Behind but dirty, or any ahead/diverged/missing state: surface it.
        _ => ProjectionDecision::Surface,
    }
}

/// Plain-language reason a checkout is surfaced rather than projected. Only
/// called for states `decide` maps to `Surface`.
pub fn surface_reason(divergence: &Divergence, clean: bool, target: &str) -> String {
    match divergence {
        Divergence::TargetMissing => {
            format!("{target} could not be resolved; fetch the remote or check the ref name")
        }
        Divergence::Behind { by } => format!(
            "{by} commit(s) behind {target} but the working tree has uncommitted changes; \
             commit or stash them, then re-run to fast-forward"
        ),
        Divergence::Ahead { by } => format!(
            "{by} local commit(s) not on {target}; push or open a PR — projection will not discard them"
        ),
        Divergence::Diverged { ahead, behind } => format!(
            "diverged from {target} ({ahead} local, {behind} remote); reconcile by hand — \
             projection will not rewrite local history"
        ),
        Divergence::UpToDate if !clean => {
            format!("on {target} with uncommitted changes")
        }
        Divergence::UpToDate => format!("on {target}"),
        Divergence::Unknown => "the checkout could not be observed".to_string(),
    }
}

/// What the projection did (or would do) to one checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum ProjectionAction {
    /// HEAD already sits at the target; nothing was done.
    AlreadyProjected,
    /// A clean, strictly-behind checkout that a fast-forward would advance —
    /// reported in observe mode (no `--apply`).
    WouldFastForward { to: VersionRevision },
    /// A clean, strictly-behind checkout that was fast-forwarded to the target.
    FastForwarded {
        from: VersionRevision,
        to: VersionRevision,
    },
    /// Drift left untouched for a human to resolve, with the reason named. Dirty,
    /// ahead, diverged, and missing-target checkouts all land here.
    Surfaced { reason: String },
    /// This checkout could not be read or fast-forwarded; the reason is carried
    /// rather than aborting the whole-suite projection.
    Failed { reason: String },
}

impl ProjectionAction {
    /// Whether this action leaves work a human still has to attend to.
    pub fn needs_attention(&self) -> bool {
        matches!(self, Self::Surfaced { .. } | Self::Failed { .. })
    }

    /// Whether this checkout ends the projection sitting at the target.
    pub fn is_projected(&self) -> bool {
        matches!(self, Self::AlreadyProjected | Self::FastForwarded { .. })
    }

    fn label(&self) -> &'static str {
        match self {
            Self::AlreadyProjected => "projected",
            Self::WouldFastForward { .. } => "behind",
            Self::FastForwarded { .. } => "fast-forwarded",
            Self::Surfaced { .. } => "surfaced",
            Self::Failed { .. } => "failed",
        }
    }
}

/// The projection reading for one repository checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoProjection {
    /// The caller's stable key for this checkout (e.g. the dev-world project key).
    pub key: String,
    pub project: ProjectRef,
    /// The checkout root on this machine.
    pub locator: String,
    /// The target the checkout is projected onto, qualified (e.g. `origin/main`).
    pub target: String,
    pub head: VersionRevision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<VersionRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub detached: bool,
    pub clean: bool,
    pub divergence: Divergence,
    pub action: ProjectionAction,
}

impl RepoProjection {
    /// A one-line reading: `<key>: <label> — <detail>`.
    pub fn summary(&self) -> String {
        let detail = match &self.action {
            ProjectionAction::AlreadyProjected if self.clean => format!("at {}", self.target),
            ProjectionAction::AlreadyProjected => {
                format!("at {} with uncommitted changes", self.target)
            }
            ProjectionAction::WouldFastForward { to } => format!(
                "behind {}; fast-forward to {} with --apply",
                self.target,
                short(to)
            ),
            ProjectionAction::FastForwarded { from, to } => {
                format!("{} → {} ({})", short(from), short(to), self.target)
            }
            ProjectionAction::Surfaced { reason } => reason.clone(),
            ProjectionAction::Failed { reason } => reason.clone(),
        };
        format!("{}: {} — {}", self.key, self.action.label(), detail)
    }
}

fn short(revision: &VersionRevision) -> String {
    let raw = revision.as_str();
    raw.get(..12).unwrap_or(raw).to_string()
}

/// The whole-suite projection: one reading per checkout, plus whether `--apply`
/// was in effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuiteProjection {
    pub version: String,
    /// The target every checkout was projected onto (qualified).
    pub target: String,
    /// True when fast-forwards were actually performed; false in observe mode.
    pub applied: bool,
    pub entries: Vec<RepoProjection>,
}

impl SuiteProjection {
    pub fn new(target: String, applied: bool, entries: Vec<RepoProjection>) -> Self {
        Self {
            version: WORKTREE_PROJECTION_VERSION.to_string(),
            target,
            applied,
            entries,
        }
    }

    /// Checkouts that end the projection sitting at the target.
    pub fn projected_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.action.is_projected())
            .count()
    }

    /// Checkouts still needing a human: surfaced drift or a read/apply failure.
    pub fn attention(&self) -> Vec<&RepoProjection> {
        self.entries
            .iter()
            .filter(|entry| entry.action.needs_attention())
            .collect()
    }

    /// True when every checkout sits at the target.
    pub fn all_projected(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|entry| entry.action.is_projected())
    }

    /// The plain reading: a headline count and one line per checkout.
    pub fn summary_lines(&self) -> Vec<String> {
        let total = self.entries.len();
        let projected = self.projected_count();
        let mode = if self.applied { "apply" } else { "observe" };
        let mut lines = vec![format!(
            "{projected}/{total} checkouts projected onto {} ({mode})",
            self.target
        )];
        for entry in &self.entries {
            lines.push(format!("  {}", entry.summary()));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_parses_qualified_and_bare_and_empty() {
        assert_eq!(
            ProjectionTarget::parse("origin/main"),
            ProjectionTarget {
                remote: "origin".into(),
                reference: "main".into()
            }
        );
        assert_eq!(
            ProjectionTarget::parse("main"),
            ProjectionTarget {
                remote: "origin".into(),
                reference: "main".into()
            }
        );
        assert_eq!(
            ProjectionTarget::parse("upstream/release"),
            ProjectionTarget {
                remote: "upstream".into(),
                reference: "release".into()
            }
        );
        assert_eq!(ProjectionTarget::parse("   "), ProjectionTarget::default());
        assert_eq!(ProjectionTarget::default().qualified(), "origin/main");
    }

    #[test]
    fn divergence_classifies_every_relation() {
        assert_eq!(Divergence::classify(true, 0, 0), Divergence::UpToDate);
        assert_eq!(
            Divergence::classify(true, 0, 3),
            Divergence::Behind { by: 3 }
        );
        assert_eq!(
            Divergence::classify(true, 2, 0),
            Divergence::Ahead { by: 2 }
        );
        assert_eq!(
            Divergence::classify(true, 2, 3),
            Divergence::Diverged {
                ahead: 2,
                behind: 3
            }
        );
        assert_eq!(Divergence::classify(false, 0, 0), Divergence::TargetMissing);
    }

    #[test]
    fn only_a_clean_behind_checkout_is_fast_forwarded() {
        // The safe case.
        assert_eq!(
            decide(&Divergence::Behind { by: 4 }, true),
            ProjectionDecision::FastForward
        );
        // The dangerous case: behind but dirty must never be forced.
        assert_eq!(
            decide(&Divergence::Behind { by: 4 }, false),
            ProjectionDecision::Surface
        );
        // Local work is never discarded.
        assert_eq!(
            decide(&Divergence::Ahead { by: 1 }, true),
            ProjectionDecision::Surface
        );
        assert_eq!(
            decide(
                &Divergence::Diverged {
                    ahead: 1,
                    behind: 1
                },
                true
            ),
            ProjectionDecision::Surface
        );
        // Already there.
        assert_eq!(
            decide(&Divergence::UpToDate, true),
            ProjectionDecision::AlreadyProjected
        );
        assert_eq!(
            decide(&Divergence::UpToDate, false),
            ProjectionDecision::AlreadyProjected
        );
        // No target to project onto.
        assert_eq!(
            decide(&Divergence::TargetMissing, true),
            ProjectionDecision::Surface
        );
        // An unobservable checkout is surfaced, never mutated.
        assert_eq!(
            decide(&Divergence::Unknown, false),
            ProjectionDecision::Surface
        );
    }

    #[test]
    fn surface_reasons_name_the_safe_next_step() {
        let dirty_behind = surface_reason(&Divergence::Behind { by: 2 }, false, "origin/main");
        assert!(dirty_behind.contains("uncommitted"));
        assert!(dirty_behind.contains("fast-forward"));

        let ahead = surface_reason(&Divergence::Ahead { by: 1 }, true, "origin/main");
        assert!(ahead.contains("will not discard"));

        let diverged = surface_reason(
            &Divergence::Diverged {
                ahead: 1,
                behind: 2,
            },
            true,
            "origin/main",
        );
        assert!(diverged.contains("will not rewrite"));

        let missing = surface_reason(&Divergence::TargetMissing, true, "origin/main");
        assert!(missing.contains("fetch"));
    }

    fn entry(
        key: &str,
        action: ProjectionAction,
        divergence: Divergence,
        clean: bool,
    ) -> RepoProjection {
        RepoProjection {
            key: key.into(),
            project: ProjectRef::parse(&format!("project:{key}")).unwrap(),
            locator: format!("/work/{key}"),
            target: "origin/main".into(),
            head: VersionRevision::new("deadbeefcafe0000"),
            target_revision: Some(VersionRevision::new("feedface12340000")),
            branch: None,
            detached: true,
            clean,
            divergence,
            action,
        }
    }

    #[test]
    fn suite_aggregates_projected_and_attention() {
        let suite = SuiteProjection::new(
            "origin/main".into(),
            true,
            vec![
                entry(
                    "o-i",
                    ProjectionAction::AlreadyProjected,
                    Divergence::UpToDate,
                    true,
                ),
                entry(
                    "ql-mef",
                    ProjectionAction::FastForwarded {
                        from: VersionRevision::new("2bcdc78aaaaa"),
                        to: VersionRevision::new("a3b1169bbbbb"),
                    },
                    Divergence::Behind { by: 2 },
                    true,
                ),
                entry(
                    "central",
                    ProjectionAction::Surfaced {
                        reason: "3 local commit(s) not on origin/main".into(),
                    },
                    Divergence::Ahead { by: 3 },
                    true,
                ),
            ],
        );
        assert_eq!(suite.projected_count(), 2);
        assert_eq!(suite.attention().len(), 1);
        assert_eq!(suite.attention()[0].key, "central");
        assert!(!suite.all_projected());

        let headline = &suite.summary_lines()[0];
        assert!(headline.starts_with("2/3 checkouts projected onto origin/main"));
    }

    #[test]
    fn a_fully_projected_suite_reports_all_projected() {
        let suite = SuiteProjection::new(
            "origin/main".into(),
            false,
            vec![entry(
                "o-i",
                ProjectionAction::AlreadyProjected,
                Divergence::UpToDate,
                true,
            )],
        );
        assert!(suite.all_projected());
        assert_eq!(suite.attention().len(), 0);
    }

    #[test]
    fn repo_summary_reads_as_key_label_detail() {
        let ff = entry(
            "ql-mef",
            ProjectionAction::FastForwarded {
                from: VersionRevision::new("2bcdc78aaaaaaaaa"),
                to: VersionRevision::new("a3b1169bbbbbbbbb"),
            },
            Divergence::Behind { by: 2 },
            true,
        );
        let line = ff.summary();
        assert!(line.starts_with("ql-mef: fast-forwarded — "));
        assert!(line.contains("2bcdc78aaaaa → a3b1169bbbbb"));

        let would = entry(
            "ql-mef",
            ProjectionAction::WouldFastForward {
                to: VersionRevision::new("a3b1169bbbbbbbbb"),
            },
            Divergence::Behind { by: 2 },
            true,
        );
        assert!(would
            .summary()
            .contains("fast-forward to a3b1169bbbbb with --apply"));
    }
}
