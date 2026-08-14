//! What the TUI needs from the application.
//!
//! This trait is the whole of the palette's contact with the rest of AIKit. It is
//! deliberately small and deliberately *dumb*: every method either hands over
//! data the store/core already has or asks `aikit-core` a question. There is no
//! method here whose implementation could reasonably contain a resolver rule,
//! because a rule reachable from the TUI is a rule that can disagree with CLI
//! Explain/Search over the same application service.
//!
//! The V2 resource methods are additive migration seams. Their defaults faithfully
//! adapt the V1 capability application service; a V2 backend can override them to
//! return the wider `ContextResolution`/`ContextSource` field without forcing the
//! TUI back into Capsule-only identity.

use std::collections::BTreeMap;
use std::path::PathBuf;

use aikit_core::arg::{ArgSpec, ArgValue, ArgValues};
use aikit_core::capsule::{Capsule, ExecMode, WorkingDir};
use aikit_core::context::ContextDescriptor;
use aikit_core::context_resolution::ContextResolution;
use aikit_core::context_source::ContextSourceExplanation;
use aikit_core::id::{CapsuleId, ContextId, GenerationId};
use aikit_core::platform::TargetId;
use aikit_core::projection::ActivationEffect;
use aikit_core::resolve::{Explanation, ResolvedView};
use aikit_core::resource::{ResourceKind, ResourceRef};
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

/// V2 mutation identity. The compatibility implementation accepts capability
/// Resources; future Action/other mutable resource types can be added by the
/// application service without changing `TuiState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceMutation {
    pub resource: ResourceRef,
    pub enable: bool,
}

impl ResourceMutation {
    pub fn new(resource: ResourceRef, enable: bool) -> Self {
        Self { resource, enable }
    }
}

/// Cheap, resource-oriented row identity for V2 application read models.
/// Operational/disclosure semantics remain on ContextResolution/ContextSource
/// explanation rather than being re-derived into this presentation summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSummary {
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub name: String,
    pub description: String,
}

/// The explanation variants the TUI can display without owning explanation
/// semantics. Both are produced by core contracts.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceExplanation {
    Capability(Explanation),
    ContextSource(ContextSourceExplanation),
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
    pub fn redacted_argv(&self) -> Result<Vec<String>> {
        aikit_core::arg::build_argv(&self.specs, &self.redacted_values())
    }

    pub fn has_secrets(&self) -> bool {
        self.specs
            .iter()
            .any(|spec| spec.is_secret() && self.values.contains_key(&spec.name))
    }

    /// The same invocation with every secret dropped.
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
    /// Silently refused for a withheld candidate.
    #[must_use]
    pub fn with_body(mut self, lines: Vec<String>) -> Self {
        if self.withheld_reason().is_none() {
            self.body = lines;
        }
        self
    }

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

    pub fn body(&self) -> &[String] {
        &self.body
    }
}

/// The application service, as the TUI needs it.
pub trait PaletteBackend {
    /// The context the palette was opened in.
    fn context(&self) -> &ContextDescriptor;

    /// The effective V1 capability view for that context, right now.
    fn view(&self) -> &ResolvedView;

    /// The rows to search over, with usage facts from the event log.
    fn documents(&self) -> Vec<SearchDoc>;

    /// V2 resource-oriented search/read-model identities.
    ///
    /// The default adapts every V1 capability row without inventing any wider
    /// resource semantics. V2 backends may append Action/ContextSource/etc rows
    /// from their authoritative resource index.
    fn resource_summaries(&self) -> Vec<ResourceSummary> {
        self.documents()
            .into_iter()
            .filter_map(|doc| {
                ResourceRef::parse(&doc.id.to_string())
                    .ok()
                    .map(|resource| ResourceSummary {
                        resource,
                        kind: ResourceKind::Capability,
                        name: doc.name,
                        description: doc.description,
                    })
            })
            .collect()
    }

    /// The complete V2 ContextResolution when the backend has migrated to it.
    /// Returning `None` is an explicit V1 compatibility state, not a second
    /// resolution performed in the TUI.
    fn context_resolution(&self) -> Option<ContextResolution> {
        None
    }

    /// ContextSource disclosure supplied by the #26 application seam.
    fn context_source_disclosure(&self) -> Vec<ContextSourceExplanation> {
        Vec::new()
    }

    /// Explain one stable ResourceRef through core-owned explanation contracts.
    fn explain_resource(&self, resource: &ResourceRef) -> Option<ResourceExplanation> {
        let capsule = CapsuleId::parse(resource.as_str()).ok()?;
        self.view()
            .explain(&capsule)
            .map(ResourceExplanation::Capability)
    }

    /// The full manifest behind a V1 capability row, for preview/run forms.
    fn capsule(&self, id: &CapsuleId) -> Option<&Capsule>;

    /// Resolve the view these V1 toggles *would* produce, and what each client
    /// would see. Writes nothing.
    fn preview(&self, scope: ScopeKind, toggles: &[Toggle]) -> Result<Projected>;

    /// Resource-oriented composition preview. The default accepts only Resources
    /// that are valid legacy capability ids and delegates to the same application
    /// service; it never performs resolution in the TUI.
    fn preview_resources(
        &self,
        scope: ScopeKind,
        mutations: &[ResourceMutation],
    ) -> Result<Projected> {
        let toggles = legacy_toggles(mutations)?;
        self.preview(scope, &toggles)
    }

    /// Commit the whole V1 set at once, returning the new generation.
    fn apply(&mut self, scope: ScopeKind, toggles: &[Toggle]) -> Result<GenerationId>;

    /// Resource-oriented commit using the same application write path.
    fn apply_resources(
        &mut self,
        scope: ScopeKind,
        mutations: &[ResourceMutation],
    ) -> Result<GenerationId> {
        let toggles = legacy_toggles(mutations)?;
        self.apply(scope, &toggles)
    }

    /// Run something whose output the palette will display.
    fn start(&mut self, intent: &RunIntent) -> Result<JobOutput>;

    /// Recently run invocations, most recent first, with secrets already dropped.
    fn recent(&self) -> Vec<RunIntent>;

    /// Captures waiting in the inbox.
    fn promotion_drafts(&self) -> Vec<PromotionDraft>;

    /// Turn a draft into a capsule.
    fn promote(&mut self, draft: &PromotionDraft) -> Result<CapsuleId>;

    /// Reveal where a capability's source lives, opening it if configured to.
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

fn legacy_toggles(mutations: &[ResourceMutation]) -> Result<Vec<Toggle>> {
    mutations
        .iter()
        .map(|mutation| {
            CapsuleId::parse(mutation.resource.as_str())
                .map(|capsule| Toggle::new(capsule, mutation.enable))
                .map_err(|_| {
                    aikit_core::AikitError::new(
                        "tui.resource_mutation_unsupported",
                        format!(
                            "resource {} is not mutable through the legacy capability adapter",
                            mutation.resource
                        ),
                    )
                    .with("resource", mutation.resource.to_string())
                })
        })
        .collect()
}
