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
use aikit_core::project::ProjectRef;
use aikit_core::projection::ActivationEffect;
use aikit_core::resolve::ResolvedView;
use aikit_core::resource::{
    ActionStageability, ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass,
    OwnerRef, ResolveExpression, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceRef,
    ResourceSearchIndex, ResourceSource, SourceAuthority, SourceRef, SourceState,
};
use aikit_core::scope::{ScopeKind, ScopeLayer};
use aikit_core::search::SearchDoc;
use aikit_core::session_space::{SessionSpaceReadModel, SessionSpaceRef};
use aikit_core::session_space_application::{
    AgentSessionContinuityEvidence, SessionSpaceAuthoredState, SessionSpaceMutation,
    SessionSpaceNativeObservation, SessionSpacePreview, SessionSpaceReconstructionReport,
};
use aikit_core::{
    FamiliarityObservation, FamiliarityStore, ForgetScope, KnowledgeAddress, KnowledgeContextPack,
    KnowledgeExplanation, KnowledgeProviderStatus, KnowledgeReading, KnowledgeRelationView,
    KnowledgeRoute, KnowledgeSearchResult, KnowledgeSources, Result,
};
use aikit_store::inbox::{Candidate, CandidateState, PromotionEdits, Similarity};
use aikit_store::{
    explain_session_space_with_receipts, AikitHome, KnowledgeApplicationReceipt,
    SessionSpaceApplicationStore, SessionSpaceExplainEvidence, SessionSpaceHistoryComparison,
    SessionSpaceReceipt,
};

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
        if self.candidate.state == CandidateState::Quarantined
            || !self.candidate.findings.is_empty()
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

fn native_application_action_record(
    id: ResourceRef,
    name: &str,
    description: &str,
    expected_return_forms: &str,
) -> ResourceRecord {
    let mut descriptor = ResourceDescriptor::new(id, ResourceKind::Action, name, description);
    descriptor.owner = Some(
        OwnerRef::parse("aikit/application-service")
            .expect("static native Action owner reference must be valid"),
    );
    descriptor.sources.push(ResourceSource {
        source: SourceRef::parse("source/aikit/application-service")
            .expect("static native Action source reference must be valid"),
        authority: Some(SourceAuthority::Authored),
        revision: None,
        locator: None,
        state: SourceState::Available,
    });
    descriptor.annotations.insert(
        "action.expected-return-forms".into(),
        expected_return_forms.into(),
    );
    ResourceRecord::new(descriptor)
}

fn session_space_store(home: Option<&AikitHome>) -> Result<SessionSpaceApplicationStore> {
    let home = home.cloned().ok_or_else(|| {
        aikit_core::AikitError::new(
            "session_space.application_home_unavailable",
            "this application backend has no canonical AIKit home for SessionSpace persistence",
        )
    })?;
    Ok(SessionSpaceApplicationStore::new(home))
}

/// Whether the shared application backend can cross the already-accepted
/// Factory Commission boundary. This is configuration/readiness evidence only;
/// `Ready` does not imply that any Commission or execution exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryWorkEntry {
    Ready,
    Unavailable { reason: String },
}

impl FactoryWorkEntry {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Exact owner receipt returned after the Factory start-work operation.
/// `receipt` is pretty-printed but otherwise unmodified Factory JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryWorkStartReceipt {
    pub summary: String,
    pub receipt: String,
}

/// Low-level resolved/package/runtime backend beneath `ApplicationService`.
///
/// Despite the retained compatibility name, this trait owns no application state,
/// selection, Resource identity, search ranking, staging or relation semantics.
pub trait PaletteBackend {
    fn context(&self) -> &ContextDescriptor;

    fn view(&self) -> &ResolvedView;

    /// Optional native owner identity. An invalid present binding is an error,
    /// never permission to derive a replacement identity from presentation.
    fn project_binding(&self) -> Result<Option<aikit_core::project::ProjectBinding>> { Ok(None) }

    /// Optional already-observed versioned material World for this Project.
    ///
    /// The observation belongs to the backend, not to this crate and not to
    /// the core: `aikit-core` is I/O-free by construction, and `aikit-tui`
    /// cannot reach a provider because it does not depend on `aikit-adapters`.
    /// So the shape is the same as [`PaletteBackend::project_binding`] — the
    /// caller who *can* observe hands the observation over, and a backend that
    /// cannot observe answers `None` rather than pretending.
    ///
    /// `None` is a real answer with two distinct meanings the reading keeps
    /// apart: no provider was attached at all, versus a provider that looked
    /// and found the Project is not under version control.
    fn versioned_world(
        &self,
    ) -> Result<Option<aikit_core::resource::VersionedProjectWorld>> {
        Ok(None)
    }

    /// Optional already-observed working-environment providers for this host.
    ///
    /// Same seam, same reason as [`PaletteBackend::versioned_world`]: tmux,
    /// cmux, Herdr and Hyprland providers do I/O and live in `aikit-adapters`,
    /// which this crate does not depend on. So the caller that *can* observe
    /// hands the observations over, and a backend that cannot observe answers
    /// `None`.
    ///
    /// The two meanings of an absent answer stay apart. `None` is "no provider
    /// was attached at all — nobody looked". `Some(vec![])` is "a caller looked
    /// and found no working environment here". A surface renders those
    /// differently because they are different facts about the machine.
    fn working_environments(
        &self,
    ) -> Result<Option<Vec<aikit_core::working_environment::WorkingEnvironmentObservation>>> {
        Ok(None)
    }

    /// Ask one observed provider to open or focus one canonical subject.
    ///
    /// The default answers `NotExposed` rather than erroring: a backend with no
    /// provider attached has not failed at anything, and the operator needs to
    /// be told the boundary is empty, not shown a failure they cannot act on.
    fn act_in_working_environment(
        &mut self,
        provider: &ResourceRef,
        subject: &ResourceRef,
        operation: crate::live_field::WorkingEnvironmentOperation,
    ) -> Result<crate::live_field::WorkingEnvironmentOutcome> {
        Ok(crate::live_field::WorkingEnvironmentOutcome::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: format!(
                "no working-environment provider is attached at this application boundary, so {} is not available here",
                operation.as_str()
            ),
        })
    }

    /// Optional already-composed credential/provider reading for this world.
    ///
    /// Same seam, same reason as [`PaletteBackend::versioned_world`]: resolving
    /// what secret providers reach this machine, and which of the world's
    /// declared credentials are bound, is I/O over the OS secure store and the
    /// binding record. `aikit-core` is I/O-free and `aikit-tui` does not depend
    /// on the crates that do it, so the caller that *can* observe composes the
    /// disclosure through `disclose_credential_world` and hands it over.
    ///
    /// The two meanings of an absent answer stay apart, on two levels. `None`
    /// here is "no producer is attached at all — nobody looked", and the
    /// reading keeps its honest `not_attempted` default. A `Some(disclosure)`
    /// then carries the finer distinctions the disclosure itself exists to
    /// keep: a roster genuinely observed empty versus one that could not be
    /// enumerated, and a credential resolved to no provider versus one never
    /// resolved at all.
    fn credential_world(
        &self,
    ) -> Result<Option<aikit_core::credential_world::CredentialWorldDisclosure>> {
        Ok(None)
    }

    /// Optional already-run installation-health reading for this world.
    ///
    /// Same seam, same reason as [`PaletteBackend::credential_world`]: running
    /// the health checks is I/O — it probes the OS secure store, reads harness
    /// config, asks the gateway socket, lists registries — and lives in the CLI
    /// crate `aikit-tui` cannot depend on. The caller that *can* run them does,
    /// and hands the composed findings over.
    ///
    /// `None` is "the checks were not run — nobody looked", and the reading
    /// keeps its honest `not_attempted` default. A `Some(disclosure)` whose
    /// `findings` are empty is the different, confirmed fact that the checks ran
    /// and found nothing wrong.
    fn doctor_world(&self) -> Result<Option<aikit_core::doctor_world::DoctorDisclosure>> {
        Ok(None)
    }

    /// Optional already-observed Workcell (body materialisation) reading.
    ///
    /// Same seam, same reason as the others: observing Workcell runs its
    /// external binary (`workcell instances list`), which lives behind the CLI
    /// crate `aikit-tui` cannot depend on. The caller that *can* observe does,
    /// and hands the composed disclosure over.
    ///
    /// `None` is "nobody looked", and the reading keeps its `not_attempted`
    /// default. A `Some(disclosure)` then keeps the finer split: the `workcell`
    /// binary that could not be read (`Unavailable`) versus a registry observed
    /// to hold nothing (`Observed` empty).
    fn workcell_world(&self) -> Result<Option<aikit_core::workcell_world::WorkcellDisclosure>> {
        Ok(None)
    }

    /// The canonical subjects this world can project into a working
    /// environment — the panes the current session plan defines, whether or
    /// not any of them is live yet.
    ///
    /// Separate from [`PaletteBackend::working_environments`] because it
    /// answers a different question. That one asks the machine what exists;
    /// this one asks the plan what could. Open needs the second: a subject
    /// that has never been started is exactly the one worth starting.
    fn working_environment_subjects(&self) -> Result<Vec<ResourceRef>> {
        Ok(Vec::new())
    }

    fn scope_layers(&self) -> Option<&[ScopeLayer]> {
        None
    }

    /// The store root already owned by the production application backend.
    /// Test/fake backends may omit it; SessionSpace operations then fail explicitly
    /// rather than falling back to process-global discovery.
    fn application_home(&self) -> Option<&AikitHome> {
        None
    }

    /// SessionSpace state that may participate in ordinary Resource navigation.
    ///
    /// Navigation-only/fake backends deliberately have no canonical AIKit home,
    /// so they contribute no SessionSpace resources. Explicit SessionSpace
    /// operations retain their stronger contract and still fail when persistence
    /// is unavailable. A backend with another legitimate source can override this
    /// projection without fabricating `AikitHome`.
    fn session_space_navigation(&self) -> Result<Vec<SessionSpaceAuthoredState>> {
        if self.application_home().is_none() {
            return Ok(Vec::new());
        }
        self.session_space_list()
    }

    /// Historical package-search documents retained for the public package/CLI
    /// compatibility surface only. Canonical V2 navigation does not call this.
    fn documents(&self) -> Vec<SearchDoc>;

    /// Canonical shallow ResourceRef-native application navigation field.
    ///
    /// Package catalogue entries are projected directly from the one resolved
    /// catalogue; no `SearchDoc` row is converted back into application identity.
    /// Slow/deep providers remain outside this low-latency baseline.
    /// Additional source-owned resource observations for canonical Context.
    /// This read may fail; it never manufactures provider offers or admission.
    fn context_resource_records(&self) -> Result<Vec<ResourceRecord>> {
        Ok(Vec::new())
    }

    /// Readiness of the native Factory Commission entry point. Backends that
    /// do not own such a binding remain explicitly unavailable.
    fn factory_work_entry(&self) -> FactoryWorkEntry {
        FactoryWorkEntry::Unavailable {
            reason: "no Factory Commission binding supplied to this application".into(),
        }
    }

    /// Invoke the configured native Factory start-work operation. The default
    /// refuses rather than manufacturing a local Commission.
    fn start_factory_work(&mut self) -> Result<FactoryWorkStartReceipt> {
        Err(aikit_core::AikitError::new(
            "factory.start_work_unavailable",
            "this application backend has no native Factory Commission binding",
        ))
    }

    fn navigation_index(&self) -> ResourceSearchIndex {
        let recent: BTreeSet<CapsuleId> = self
            .recent()
            .into_iter()
            .map(|intent| intent.capsule)
            .collect();
        let mut index = ResourceSearchIndex::default();
        let current_context = vec![
            NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)
                .with_detail("part of the resolved operating context"),
        ];
        let current_project = vec![
            NavigationEvidence::new(NavigationEvidenceClass::CurrentProject)
                .with_detail("the Project currently being inhabited"),
            NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)
                .with_detail("part of the resolved operating context"),
        ];
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
                    current_project.clone(),
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
                    current_context.clone(),
                );
            }
        }

        // Package-backed capabilities are projected directly from the resolved
        // catalogue. Search handles remain annotations on the same ResourceRef;
        // they do not create a second search/package identity.
        for (id, entry) in &self.view().catalog_index {
            let Ok(resource) = ResourceRef::parse(&id.to_string()) else {
                continue;
            };
            let mut evidence = Vec::new();
            if self
                .view()
                .declared
                .get(id)
                .is_some_and(|declared| matches!(declared.scope, ScopeKind::Project | ScopeKind::ProjectLocal))
            {
                evidence.push(
                    NavigationEvidence::new(NavigationEvidenceClass::CurrentProject)
                        .with_detail("declared by the current Project scope"),
                );
            }
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
            let description = if entry.description.trim().is_empty() {
                format!("package-backed {} capability", id.kind().as_str())
            } else {
                entry.description.clone()
            };
            let mut descriptor = ResourceDescriptor::new(
                resource.clone(),
                ResourceKind::Capability,
                entry.name.clone(),
                description,
            );
            // The entry's presence in the resolved catalogue is itself the
            // source observation: the package backing this capability is
            // loaded and resolved in the current view. Eligibility and
            // preference stay independent axes on the record.
            descriptor.sources.push(ResourceSource {
                source: SourceRef::parse("source/aikit/resolved-catalogue")
                    .expect("static catalogue source reference must be valid"),
                authority: Some(SourceAuthority::Authored),
                revision: None,
                locator: None,
                state: SourceState::Available,
            });
            descriptor
                .annotations
                .insert("capsule-kind".into(), id.kind().as_str().into());
            if !entry.exports.is_empty() {
                descriptor
                    .annotations
                    .insert("aikit.search-exports".into(), entry.exports.join(","));
            }
            if !entry.tags.is_empty() {
                descriptor
                    .annotations
                    .insert("aikit.search-tags".into(), entry.tags.join(","));
            }
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
            native_application_action_record(
                open_project.clone(),
                "Open project",
                "enter the selected Project workspace",
                "opened",
            ),
            Vec::new(),
        );
        index.insert_resource(
            native_application_action_record(
                explain_capability.clone(),
                "Explain capability",
                "show why this Capability has its current resolved state",
                "explanation",
            ),
            Vec::new(),
        );
        index.insert_resource(
            native_application_action_record(
                toggle_capability.clone(),
                "Toggle capability",
                "stage an enable/disable change at the selected mutation scope",
                "staged-change",
            ),
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

    // Knowledge operations deliberately live on the same shared application seam
    // as CLI/TUI. Defaults preserve deterministic fake backends; production owns
    // materialisation and returns Some(..) for the supported operation family.
    fn knowledge_resolve(
        &self,
        _expression: &ResolveExpression,
        _limit: usize,
    ) -> Result<Option<KnowledgeSearchResult>> {
        Ok(None)
    }

    fn knowledge_address(&self, _resource: &ResourceRef) -> Result<Option<KnowledgeAddress>> {
        Ok(None)
    }

    fn knowledge_read(&self, _address: &KnowledgeAddress) -> Result<Option<KnowledgeReading>> {
        Ok(None)
    }

    fn knowledge_relations(
        &self,
        _address: &KnowledgeAddress,
        _depth: u8,
        _max_nodes: usize,
        _max_edges: usize,
    ) -> Result<Option<KnowledgeRelationView>> {
        Ok(None)
    }

    fn knowledge_route(
        &mut self,
        _query: Option<&str>,
        _addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeRoute>> {
        Ok(None)
    }

    fn knowledge_frame(
        &mut self,
        _query: Option<&str>,
        _addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeContextPack>> {
        Ok(None)
    }

    fn knowledge_sources(&self, _address: &KnowledgeAddress) -> Result<Option<KnowledgeSources>> {
        Ok(None)
    }

    fn knowledge_explain(
        &self,
        _address: &KnowledgeAddress,
    ) -> Result<Option<KnowledgeExplanation>> {
        Ok(None)
    }

    fn knowledge_history(
        &self,
        _resource: Option<&ResourceRef>,
    ) -> Result<Vec<KnowledgeApplicationReceipt>> {
        Ok(Vec::new())
    }

    fn knowledge_status(&self) -> Result<Option<KnowledgeProviderStatus>> {
        Ok(None)
    }

    fn knowledge_forget(&mut self, _scope: ForgetScope) -> Result<bool> {
        Ok(false)
    }

    // SessionSpace application operations deliberately live on the shared backend
    // seam. They all resolve to the same canonical store and never rerun Project,
    // ContextResolution or provider semantics.
    fn session_space_list(&self) -> Result<Vec<SessionSpaceAuthoredState>> {
        session_space_store(self.application_home())?.list()
    }

    fn session_space_show(&self, space: &SessionSpaceRef) -> Result<SessionSpaceAuthoredState> {
        session_space_store(self.application_home())?.load(space)
    }

    fn session_space_open(&self, space: &SessionSpaceRef) -> Result<SessionSpaceAuthoredState> {
        self.session_space_show(space)
    }

    fn session_space_discover(
        &self,
        project: Option<&ProjectRef>,
    ) -> Result<Vec<SessionSpaceAuthoredState>> {
        session_space_store(self.application_home())?.discover(project)
    }

    fn session_space_stage(
        &self,
        space: Option<&SessionSpaceRef>,
        intent: SessionSpaceMutation,
    ) -> Result<SessionSpacePreview> {
        session_space_store(self.application_home())?.stage(space, intent)
    }

    fn session_space_apply(
        &mut self,
        preview: &SessionSpacePreview,
    ) -> Result<SessionSpaceReceipt> {
        session_space_store(self.application_home())?.apply(preview)
    }

    fn session_space_history(&self, space: &SessionSpaceRef) -> Result<Vec<SessionSpaceReceipt>> {
        session_space_store(self.application_home())?.history(space)
    }

    fn session_space_compare_history(
        &self,
        space: &SessionSpaceRef,
        from_sequence: u64,
        to_sequence: u64,
    ) -> Result<SessionSpaceHistoryComparison> {
        session_space_store(self.application_home())?.compare_history(
            space,
            from_sequence,
            to_sequence,
        )
    }

    fn session_space_stage_restore(
        &self,
        space: &SessionSpaceRef,
        sequence: u64,
    ) -> Result<SessionSpacePreview> {
        session_space_store(self.application_home())?.stage_restore(space, sequence)
    }

    fn session_space_reconstruct(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        session_space_store(self.application_home())?.reconstruct(
            space,
            runtime,
            native_observations,
            continuity,
        )
    }

    fn session_space_reconcile(
        &self,
        space: &SessionSpaceRef,
        runtime: Option<&SessionSpaceReadModel>,
        native_observations: &[SessionSpaceNativeObservation],
        continuity: &[AgentSessionContinuityEvidence],
    ) -> Result<SessionSpaceReconstructionReport> {
        self.session_space_reconstruct(space, runtime, native_observations, continuity)
    }

    fn session_space_explain(
        &self,
        space: &SessionSpaceRef,
        reconstruction: Option<SessionSpaceReconstructionReport>,
    ) -> Result<SessionSpaceExplainEvidence> {
        let store = session_space_store(self.application_home())?;
        explain_session_space_with_receipts(&store, space, reconstruction)
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
