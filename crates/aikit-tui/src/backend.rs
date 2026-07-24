//! What the palette needs from the application.
//!
//! This trait is the whole of the palette's contact with the rest of AIKit. It is
//! deliberately small and deliberately *dumb*: every method either hands over
//! data the store already has or asks `aikit-core` a question. There is no method
//! here whose implementation could reasonably contain a capability rule, because
//! a rule reachable from the TUI is a rule that can disagree with `aikit explain`.
//!
//! Note what is **not** here. There is no `explain` method: an explanation is a
//! pure projection of a resolved view, so the palette calls
//! [`aikit_core::resolve::ResolvedView::explain`] on the view it already holds.
//! There is no `stage` method either — staging is [`preview`](PaletteBackend::preview)
//! plus a diff computed in [`crate::staging`], which is why staging provably
//! cannot write anything: the only write method on this trait is
//! [`apply`](PaletteBackend::apply).
//!
//! ## Running is handed back, not performed here
//!
//! [`PaletteBackend::start`] exists only for execution modes that keep the
//! palette on screen — `capture` and `background`. A foreground or `replace` mode
//! needs the terminal the palette is holding, so those come back to the caller as
//! [`crate::PaletteOutcome::Run`] and are executed after teardown. Doing otherwise
//! would hand a child process a raw-mode terminal and an alternate screen.

use std::collections::BTreeMap;
use std::path::PathBuf;

use aikit_core::arg::{ArgSpec, ArgValue, ArgValues};
use aikit_core::capsule::{Capsule, ExecMode, WorkingDir};
use aikit_core::context::ContextDescriptor;
use aikit_core::id::{CapsuleId, ContextId, GenerationId};
use aikit_core::platform::TargetId;
use aikit_core::projection::ActivationEffect;
use aikit_core::resolve::ResolvedView;
use aikit_core::scope::ScopeKind;
use aikit_core::search::SearchDoc;
use aikit_core::Result;
use aikit_store::inbox::{Candidate, CandidateState, PromotionEdits, Similarity};

/// The mask a secret wears everywhere it is displayed.
pub const REDACTED: &str = "••••••";

/// One requested change to the declared state of a capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toggle {
    pub capsule: CapsuleId,
    pub enable: bool,
}

impl Toggle {
    pub fn new(capsule: CapsuleId, enable: bool) -> Self {
        Self { capsule, enable }
    }
}

/// What applying a view would mean for one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientEffect {
    pub target: TargetId,
    pub effect: ActivationEffect,
}

impl ClientEffect {
    pub fn new(target: TargetId, effect: ActivationEffect) -> Self {
        Self { target, effect }
    }

    /// `Claude: live`, `Codex: brokered — …`.
    pub fn describe(&self) -> String {
        self.effect.describe_for(&self.target)
    }
}

/// The answer to "what would happen if I applied this".
///
/// The view is the resolver's; the effects are the client adapters'. Neither is
/// computed in this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct Projected {
    pub view: ResolvedView,
    pub effects: Vec<ClientEffect>,
}

/// Everything needed to run one capability once.
///
/// Carries the argument *specification* alongside the values so that redaction,
/// argv construction and the "can this be repeated" question all have one source
/// of truth — [`aikit_core::arg`] — rather than a copy of it in the palette.
#[derive(Debug, Clone, PartialEq)]
pub struct RunIntent {
    pub capsule: CapsuleId,
    pub context: ContextId,
    pub specs: Vec<ArgSpec>,
    pub values: ArgValues,
    pub mode: ExecMode,
    pub cwd: WorkingDir,
    pub env: BTreeMap<String, String>,
    /// Set when the capsule's revision has not been reviewed. An unreviewed
    /// script may be exposed but must be confirmed before it runs.
    pub requires_confirmation: bool,
}

impl RunIntent {
    /// The real argv. Fails exactly where [`aikit_core::arg::build_argv`] fails.
    pub fn argv(&self) -> Result<Vec<String>> {
        aikit_core::arg::build_argv(&self.specs, &self.values)
    }

    /// The argv as it may be shown, logged or recorded.
    ///
    /// Secrets are replaced *before* argv is built rather than masked afterwards,
    /// so there is no window in which the real value exists in a display string.
    pub fn redacted_argv(&self) -> Result<Vec<String>> {
        aikit_core::arg::build_argv(&self.specs, &self.redacted_values())
    }

    pub fn has_secrets(&self) -> bool {
        self.specs
            .iter()
            .any(|spec| spec.is_secret() && self.values.contains_key(&spec.name))
    }

    /// The same invocation with every secret dropped.
    ///
    /// This is what the recent list keeps. Repeating it will fail the required
    /// check for any secret argument, which is the intended outcome: a secret is
    /// re-entered, never replayed out of a history buffer.
    pub fn without_secrets(&self) -> Self {
        let secret_names: Vec<&str> = self
            .specs
            .iter()
            .filter(|s| s.is_secret())
            .map(|s| s.name.as_str())
            .collect();
        let mut out = self.clone();
        out.values
            .retain(|name, _| !secret_names.iter().any(|s| *s == name));
        out
    }

    fn redacted_values(&self) -> ArgValues {
        self.values
            .iter()
            .map(|(name, value)| {
                let secret = self
                    .specs
                    .iter()
                    .any(|spec| &spec.name == name && spec.is_secret());
                if secret {
                    (name.clone(), ArgValue::String(REDACTED.to_string()))
                } else {
                    (name.clone(), value.clone())
                }
            })
            .collect()
    }
}

/// What a captured run produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobOutput {
    pub capsule: Option<CapsuleId>,
    /// `None` while the job is still running.
    pub status: Option<i32>,
    pub lines: Vec<String>,
    /// Set when the palette is not showing everything the job printed.
    pub truncated: bool,
}

impl JobOutput {
    pub fn finished(&self) -> bool {
        self.status.is_some()
    }

    pub fn succeeded(&self) -> bool {
        self.status == Some(0)
    }
}

/// A capture ready to become a capsule, plus what promotion would produce.
///
/// The body is held separately from the candidate because a **quarantined**
/// candidate's body must never reach a preview. That is enforced at construction:
/// there is no way to build a draft that both is withheld and carries its text.
#[derive(Debug, Clone, PartialEq)]
pub struct PromotionDraft {
    pub candidate: Candidate,
    pub edits: PromotionEdits,
    pub similar: Vec<Similarity>,
    body: Vec<String>,
}

impl PromotionDraft {
    pub fn new(candidate: Candidate, edits: PromotionEdits) -> Self {
        Self {
            candidate,
            edits,
            similar: Vec::new(),
            body: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_similar(mut self, similar: Vec<Similarity>) -> Self {
        self.similar = similar;
        self
    }

    /// Attach the candidate's stored (already redacted) text.
    ///
    /// Silently refused for a withheld candidate — not as a courtesy, but because
    /// the alternative is a code path where quarantined text is in memory next to
    /// a renderer that might one day forget to check.
    #[must_use]
    pub fn with_body(mut self, lines: Vec<String>) -> Self {
        if self.withheld_reason().is_none() {
            self.body = lines;
        }
        self
    }

    /// Why this candidate cannot be promoted or previewed, if it cannot.
    pub fn withheld_reason(&self) -> Option<String> {
        if self.candidate.state == CandidateState::Quarantined || !self.candidate.findings.is_empty()
        {
            let what = self
                .candidate
                .findings
                .iter()
                .map(|f| f.rule.clone())
                .collect::<Vec<_>>()
                .join(", ");
            return Some(if what.is_empty() {
                "quarantined by the capture scanner".to_string()
            } else {
                format!("quarantined by the capture scanner: {what}")
            });
        }
        None
    }

    /// The body, which is empty for anything withheld.
    pub fn body(&self) -> &[String] {
        &self.body
    }
}

/// The application service, as the palette needs it.
pub trait PaletteBackend {
    /// The context the palette was opened in.
    fn context(&self) -> &ContextDescriptor;

    /// The effective view for that context, right now.
    fn view(&self) -> &ResolvedView;

    /// The rows to search over, with the usage facts from the event log.
    fn documents(&self) -> Vec<SearchDoc>;

    /// The full manifest behind a row, for the preview and the argument form.
    fn capsule(&self, id: &CapsuleId) -> Option<&Capsule>;

    /// Resolve the view these toggles *would* produce, and what each client would
    /// see. Writes nothing.
    fn preview(&self, scope: ScopeKind, toggles: &[Toggle]) -> Result<Projected>;

    /// Commit the whole set at once, returning the new generation.
    fn apply(&mut self, scope: ScopeKind, toggles: &[Toggle]) -> Result<GenerationId>;

    /// Run something whose output the palette will display.
    fn start(&mut self, intent: &RunIntent) -> Result<JobOutput>;

    /// Recently run invocations, most recent first, with secrets already dropped.
    fn recent(&self) -> Vec<RunIntent>;

    /// Captures waiting in the inbox.
    fn promotion_drafts(&self) -> Vec<PromotionDraft>;

    /// Turn a draft into a capsule.
    fn promote(&mut self, draft: &PromotionDraft) -> Result<CapsuleId>;

    /// Reveal where a capability's source lives, opening it if the application is
    /// configured to. Returns the path so the palette can say what it did.
    fn open_source(&mut self, id: &CapsuleId) -> Result<PathBuf> {
        match self.capsule(id).and_then(|c| c.root.clone()) {
            Some(root) => Ok(root),
            None => Err(aikit_core::AikitError::new(
                "capsule.no_source",
                format!("{id} has no source directory on this machine"),
            )
            .with("capability", id.to_string())),
        }
    }
}
