//! Shared application backend contract beneath AIKit V2 surfaces.
//!
//! The historical trait name `PaletteBackend` is retained temporarily as an outer
//! source-compatibility name, but there is no Palette semantic controller left.
//! `ApplicationService` is the application authority and this trait only exposes
//! resolved/package/runtime data plus preview/apply operations.
//!
//! Canonical navigation is ResourceRef-native. The retained `documents()` method
//! is explicitly package/CLI compatibility: it is not consulted by
//! `navigation_index()` and therefore cannot define the V2 search field.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use aikit_core::arg::{ArgSpec, ArgValue, ArgValues};
use aikit_core::capsule::{Capsule, ExecMode, WorkingDir};
use aikit_core::context::ContextDescriptor;
use aikit_core::id::{CapsuleId, ContextId, GenerationId};
use aikit_core::platform::TargetId;
use aikit_core::projection::ActivationEffect;
use aikit_core::resolve::ResolvedView;
use aikit_core::resource::{
    ActionStageability, ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass,
    ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef, ResourceSearchIndex,
};
use aikit_core::scope::{ScopeKind, ScopeLayer};
use aikit_core::search::SearchDoc;
use aikit_core::{FamiliarityObservation, FamiliarityStore, Result};
use aikit_store::inbox::{Candidate, CandidateState, PromotionEdits, Similarity};

/// The mask a secret wears everywhere it is displayed.
pub const REDACTED: &str = "••••••";

/// One requested package-activation change at the compatibility/runtime boundary.
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

    pub fn describe(&self) -> String {
        self.effect.describe_for(&self.target)
    }
}

/// Resolver-owned hypothetical view plus adapter-owned activation effects.
#[derive(Debug, Clone, PartialEq)]
pub struct Projected {
    pub view: ResolvedView,
    pub effects: Vec<ClientEffect>,
}

/// Everything needed to run one package-backed capability once.
#[derive(Debug, Clone, PartialEq)]
pub struct RunIntent {
    pub capsule: CapsuleId,
    pub context: ContextId,
    pub specs: Vec<ArgSpec>,
    pub values: ArgValues,
    pub mode: ExecMode,
    pub cwd: WorkingDir,
    pub env: BTreeMap<String, String>,
    pub requires_confirmation: bool,
}

impl RunIntent {
    pub fn argv(&self) -> Result<Vec<String>> {
        aikit_core::arg::build_argv(&self.specs, &self.values)
    }

    pub fn redacted_argv(&self) -> Result<Vec<String>> {
        aikit_core::arg::build_argv(&self.specs, &self.redacted_values())
    }

    pub fn has_secrets(&self) -> bool {
        self.specs
            .iter()
            .any(|spec| spec.is_secret() && self.values.contains_key(&spec.name))
    }

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
    pub status: Option<i32>,
    pub lines: Vec<String>,
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

/// A package capture ready for explicit promotion.
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

/// Low-level resolved/package/runtime backend beneath `ApplicationService`.
///
/// Despite the retained compatibility name, this trait owns no application state,
/// selection, Resource identity, search ranking, staging or relation semantics.
pub trait PaletteBackend {
    fn context(&self) -> &ContextDescriptor;

    fn view(&self) -> &ResolvedView;

    fn scope_layers(&self) -> Option<&[ScopeLayer]> {
        None
    }

    /// Historical package-search documents retained for the public package/CLI
    /// compatibility surface only. Canonical V2 navigation does not call this.
    fn documents(&self) -> Vec<SearchDoc>;

    /// Canonical shallow ResourceRef-native application navigation field.
    ///
    /// Package catalogue entries are projected directly from the one resolved
    /// catalogue; no `SearchDoc` row is converted back into application identity.
    /// Slow/deep providers remain outside this low-latency baseline.
    fn navigation_index(&self) -> ResourceSearchIndex {
        let recent: BTreeSet<CapsuleId> = self
            .recent()
            .into_iter()
            .map(|intent| intent.capsule)
            .collect();
        let mut index = ResourceSearchIndex::default();
        let current = vec![NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)
            .with_detail("part of the resolved operating context")];
        let mut project_subject = None;
        let mut capability_subjects = Vec::new();

        if let Some(project_id) = self.context().project_id.as_ref() {
            if let Ok(resource) = ResourceRef::parse(&format!("project/{project_id}")) {
                let name = self
                    .context()
                    .project_root
                    .as_ref()
                    .and_then(|root| root.file_name())
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| project_id.to_string());
                let description = self
                    .context()
                    .project_root
                    .as_ref()
                    .map(|root| format!("current project · {}", root.display()))
                    .unwrap_or_else(|| "current project".into());
                index.insert_resource(
                    ResourceRecord::new(ResourceDescriptor::new(
                        resource.clone(),
                        ResourceKind::Project,
                        name,
                        description,
                    )),
                    current.clone(),
                );
                project_subject = Some(resource);
            }
        }

        if !self.context().host.trim().is_empty() {
            if let Ok(resource) = ResourceRef::parse(&format!("host/{}", self.context().host)) {
                index.insert_resource(
                    ResourceRecord::new(ResourceDescriptor::new(
                        resource,
                        ResourceKind::Host,
                        self.context().host.clone(),
                        format!("current host · {}", self.context().platform),
                    )),
                    current.clone(),
                );
            }
        }

        // Package-backed capabilities are projected directly from the resolved
        // catalogue. `SearchDoc` is intentionally absent from this path.
        for id in self.view().catalog_index.keys() {
            let Ok(resource) = ResourceRef::parse(&id.to_string()) else {
                continue;
            };
            let mut evidence = Vec::new();
            if self.view().is_active(id) {
                evidence.push(
                    NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)
                        .with_detail("active in the resolved context"),
                );
            }
            if recent.contains(id) {
                evidence.push(
                    NavigationEvidence::new(NavigationEvidenceClass::Recent)
                        .with_detail("present in recent run history"),
                );
            }
            let mut descriptor = ResourceDescriptor::new(
                resource.clone(),
                ResourceKind::Capability,
                id.leaf(),
                format!("package-backed {} capability", id.kind().as_str()),
            );
            descriptor
                .annotations
                .insert("capsule-kind".into(), id.kind().as_str().into());
            index.insert_resource(ResourceRecord::new(descriptor), evidence);
            capability_subjects.push(resource);
        }

        let open_project = ResourceRef::parse("action/project/open")
            .expect("static V2 Action ResourceRef must be valid");
        let explain_capability = ResourceRef::parse("action/capability/explain")
            .expect("static V2 Action ResourceRef must be valid");
        let toggle_capability = ResourceRef::parse("action/capability/toggle")
            .expect("static V2 Action ResourceRef must be valid");
        index.insert_resource(
            ResourceRecord::new(ResourceDescriptor::new(
                open_project.clone(),
                ResourceKind::Action,
                "Open project",
                "enter the selected Project workspace",
            )),
            Vec::new(),
        );
        index.insert_resource(
            ResourceRecord::new(ResourceDescriptor::new(
                explain_capability.clone(),
                ResourceKind::Action,
                "Explain capability",
                "show why this Capability has its current resolved state",
            )),
            Vec::new(),
        );
        index.insert_resource(
            ResourceRecord::new(ResourceDescriptor::new(
                toggle_capability.clone(),
                ResourceKind::Action,
                "Toggle capability",
                "stage an enable/disable change at the selected mutation scope",
            )),
            Vec::new(),
        );

        if let Some(subject) = project_subject {
            index
                .insert_action(
                    ContextualActionDescriptor::new(
                        open_project,
                        subject,
                        "Open workspace",
                        "enter this Project without changing composition",
                        ActionStageability::NotStageable,
                    )
                    .with_keywords(["open", "workspace", "enter"]),
                )
                .expect("indexed Project and Action must form a valid relation");
        }
        for subject in capability_subjects {
            index
                .insert_action(
                    ContextualActionDescriptor::new(
                        explain_capability.clone(),
                        subject.clone(),
                        "Explain",
                        "show resolution, eligibility and provenance for this Capability",
                        ActionStageability::NotStageable,
                    )
                    .with_keywords(["why", "explain", "provenance"]),
                )
                .expect("indexed Capability and Action must form a valid relation");
            index
                .insert_action(
                    ContextualActionDescriptor::new(
                        toggle_capability.clone(),
                        subject,
                        "Toggle activation",
                        "stage an explicit enable/disable change for this Capability",
                        ActionStageability::Stageable,
                    )
                    .with_keywords(["enable", "disable", "stage"]),
                )
                .expect("indexed Capability and Action must form a valid relation");
        }
        index
    }

    fn familiarity(&self) -> Result<Option<FamiliarityStore>> {
        Ok(None)
    }

    fn record_familiarity(&mut self, _observation: FamiliarityObservation) -> Result<()> {
        Ok(())
    }

    fn capsule(&self, id: &CapsuleId) -> Option<&Capsule>;

    fn preview(&self, scope: ScopeKind, toggles: &[Toggle]) -> Result<Projected>;

    fn apply(&mut self, scope: ScopeKind, toggles: &[Toggle]) -> Result<GenerationId>;

    fn start(&mut self, intent: &RunIntent) -> Result<JobOutput>;

    fn recent(&self) -> Vec<RunIntent>;

    fn promotion_drafts(&self) -> Vec<PromotionDraft>;

    fn promote(&mut self, draft: &PromotionDraft) -> Result<CapsuleId>;

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
