//! V2 TUI application-service and authoritative interaction-state foundation.
//!
//! The V1 palette and tree remain useful presentation implementations, but they
//! must not remain independent semantic owners. This module defines the state that
//! list, tree and future graph views share: one selected [`ResourceRef`], one
//! staged change set and mutation scope, one navigation/overlay history, one
//! contextual Action lane, and refresh reconciliation by stable identity rather
//! than row index.
//!
//! Application semantics stay behind [`TuiApplicationService`]. The reducer is
//! pure and can only request effects; it cannot resolve capabilities, eligibility,
//! provenance, composition, contextual Actions or history itself.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use aikit_core::resource::{
    search_contextual_actions, ContextualActionDescriptor, ResourceKind, ResourceRef,
};
use aikit_core::scope::ScopeKind;
use aikit_core::{
    ForgetScope, KnowledgeAddress, KnowledgeContextPack, KnowledgeProviderStatus, KnowledgeReading,
    KnowledgeRoute, KnowledgeSources, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresentationMode {
    Quick,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceSection {
    Projects,
    Compose,
    Explore,
    Projection,
    History,
}

impl WorkspaceSection {
    pub const ALL: [Self; 5] = [
        Self::Projects,
        Self::Compose,
        Self::Explore,
        Self::Projection,
        Self::History,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Compose => "Compose",
            Self::Explore => "Explore",
            Self::Projection => "Projection",
            Self::History => "History",
        }
    }

    fn relative(self, delta: isize) -> Self {
        let current = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        let last = Self::ALL.len().saturating_sub(1);
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            (current + delta as usize).min(last)
        };
        Self::ALL[next]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationView {
    List,
    Tree,
    Graph,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceListItem {
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub label: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceListReadModel {
    /// Opaque application-service revision. The TUI compares it but does not
    /// interpret or mint it.
    pub revision: String,
    pub resources: Vec<ResourceListItem>,
}

impl ResourceListReadModel {
    pub fn contains(&self, resource: &ResourceRef) -> bool {
        self.resources
            .iter()
            .any(|candidate| &candidate.resource == resource)
    }

    pub fn position(&self, resource: &ResourceRef) -> Option<usize> {
        self.resources
            .iter()
            .position(|candidate| &candidate.resource == resource)
    }

    pub fn resource_at(&self, index: usize) -> Option<&ResourceRef> {
        self.resources.get(index).map(|item| &item.resource)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationIntent {
    Enable,
    Disable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StagedChanges {
    changes: BTreeMap<ResourceRef, ActivationIntent>,
}

impl StagedChanges {
    pub fn stage(&mut self, resource: ResourceRef, intent: ActivationIntent) {
        self.changes.insert(resource, intent);
    }

    pub fn unstage(&mut self, resource: &ResourceRef) -> Option<ActivationIntent> {
        self.changes.remove(resource)
    }

    pub fn get(&self, resource: &ResourceRef) -> Option<ActivationIntent> {
        self.changes.get(resource).copied()
    }

    pub fn resources(&self) -> impl Iterator<Item = &ResourceRef> {
        self.changes.keys()
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionPreview {
    pub revision: String,
    pub scope: ScopeKind,
    pub staged: StagedChanges,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyReceipt {
    pub revision: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationReadModel {
    pub subject: ResourceRef,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ActionOutcome {
    Opened {
        subject: ResourceRef,
        summary: String,
    },
    Explained {
        subject: ResourceRef,
        summary: String,
    },
    History {
        subject: ResourceRef,
        summary: String,
    },
    Staged {
        resource: ResourceRef,
        intent: ActivationIntent,
        summary: String,
    },
    Status {
        summary: String,
    },
}

impl ActionOutcome {
    pub fn summary(&self) -> &str {
        match self {
            Self::Opened { summary, .. }
            | Self::Explained { summary, .. }
            | Self::History { summary, .. }
            | Self::Staged { summary, .. }
            | Self::Status { summary } => summary,
        }
    }
}

/// Shared application service for the human TUI. The CLI may consume the same
/// underlying application services directly; the TUI must never shell the CLI.
///
/// Rich providers (Wiki, SourcePool, CodeIndex, history/familiarity) can return
/// their own read models behind these methods without teaching renderers their
/// resolver or provider rules.
pub trait TuiApplicationService {
    fn search(&self, query: &str) -> Result<ResourceListReadModel>;
    fn context_disclosure(&self, resource: &ResourceRef) -> Result<Value>;
    fn preview_composition(
        &self,
        scope: ScopeKind,
        staged: &StagedChanges,
    ) -> Result<CompositionPreview>;
    fn apply_composition(&mut self, preview: &CompositionPreview) -> Result<ApplyReceipt>;
    fn explain(&self, resource: &ResourceRef) -> Result<Value>;
    fn history(&self, resource: Option<&ResourceRef>) -> Result<Vec<HistoryEntry>>;
    fn relations(&self, resource: &ResourceRef) -> Result<RelationReadModel>;

    /// Rich Knowledge operations are part of the application faculty, not renderer
    /// semantics. Minimal services may return None; the production service exposes
    /// the same materialised runtime used by CLI and final TUI search/read/explain.
    fn knowledge_read(&self, _address: &KnowledgeAddress) -> Result<Option<KnowledgeReading>> {
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

    fn knowledge_status(&self) -> Result<Option<KnowledgeProviderStatus>> {
        Ok(None)
    }

    fn knowledge_forget(&mut self, _scope: ForgetScope) -> Result<bool> {
        Ok(false)
    }

    /// Record that the actor actually traversed/opened this Resource. Cursor
    /// movement and mere search visibility are deliberately not observations.
    /// Minimal/test services remain source-compatible and deterministic via the
    /// no-op default; durable backends may append typed familiarity evidence.
    fn observe_resource_use(&mut self, _resource: &ResourceRef) -> Result<()> {
        Ok(())
    }

    /// Canonical Actions currently applicable to this Resource in this context.
    /// The default keeps existing service implementations source-compatible while
    /// richer backends opt into the V2 Action field.
    fn contextual_actions(
        &self,
        _resource: &ResourceRef,
    ) -> Result<Vec<ContextualActionDescriptor>> {
        Ok(Vec::new())
    }

    /// Invoke one already-resolved contextual Action. Providers remain responsible
    /// for operation semantics; the reducer only consumes the typed outcome.
    fn invoke_action(&mut self, action: &ContextualActionDescriptor) -> Result<ActionOutcome> {
        Ok(ActionOutcome::Status {
            summary: format!("action {} has no application implementation", action.action),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Overlay {
    Help,
    Explain,
    CompositionPreview,
    ConfirmApply,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationPoint {
    pub selected: Option<ResourceRef>,
    pub relation_view: RelationView,
    pub workspace_section: WorkspaceSection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionInvalidation {
    pub previous: ResourceRef,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiStatus {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuiState {
    pub query: String,
    pub read_model: ResourceListReadModel,
    /// The one canonical semantic selection for all presentations.
    pub selected: Option<ResourceRef>,
    /// Contextual Actions are relations for the selected Resource; they are not
    /// alternate search-row identities.
    #[serde(default)]
    pub contextual_actions_for: Option<ResourceRef>,
    #[serde(default)]
    pub contextual_actions: Vec<ContextualActionDescriptor>,
    /// `Some` means the text-action lane is open. Its query is intentionally
    /// distinct from the global Resource search query.
    #[serde(default)]
    pub action_query: Option<String>,
    #[serde(default)]
    pub action_cursor: usize,
    #[serde(default)]
    pub action_result: Option<ActionOutcome>,
    pub staged: StagedChanges,
    /// Mutation scope is semantic intent, not view chrome. A staged change without
    /// its scope is incomplete and may not be previewed or applied.
    pub mutation_scope: Option<ScopeKind>,
    pub presentation: PresentationMode,
    pub workspace_section: WorkspaceSection,
    pub relation_view: RelationView,
    pub overlay: Option<Overlay>,
    pub navigation: Vec<NavigationPoint>,
    pub selection_invalidation: Option<SelectionInvalidation>,
    pub preview: Option<CompositionPreview>,
    pub status: Option<UiStatus>,
    pub area: (u16, u16),
    pub exit_requested: bool,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            query: String::new(),
            read_model: ResourceListReadModel::default(),
            selected: None,
            contextual_actions_for: None,
            contextual_actions: Vec::new(),
            action_query: None,
            action_cursor: 0,
            action_result: None,
            staged: StagedChanges::default(),
            mutation_scope: None,
            presentation: PresentationMode::Quick,
            workspace_section: WorkspaceSection::Projects,
            relation_view: RelationView::List,
            overlay: None,
            navigation: Vec::new(),
            selection_invalidation: None,
            preview: None,
            status: None,
            area: (80, 24),
            exit_requested: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    SetQuery(String),
    SearchFinished(ResourceListReadModel),
    Refresh(ResourceListReadModel),
    Select(ResourceRef),
    SelectNext,
    SelectPrevious,
    ContextualActionsLoaded {
        subject: ResourceRef,
        actions: Vec<ContextualActionDescriptor>,
    },
    BeginActionSearch,
    SetActionQuery(String),
    SelectNextAction,
    SelectPreviousAction,
    InvokeAction(ResourceRef),
    ActionFinished(ActionOutcome),
    OpenSelection,
    ResourceUseObserved(ResourceRef),
    Back,
    SetMutationScope(ScopeKind),
    SetPresentation(PresentationMode),
    SetWorkspaceSection(WorkspaceSection),
    NextWorkspaceSection,
    PreviousWorkspaceSection,
    SetRelationView(RelationView),
    ShowOverlay(Overlay),
    Dismiss,
    Stage {
        resource: ResourceRef,
        intent: ActivationIntent,
    },
    Unstage(ResourceRef),
    DiscardStaged,
    RequestCompositionPreview,
    CompositionPreviewed(CompositionPreview),
    RequestApply,
    ConfirmApply,
    ApplyFinished(ApplyReceipt),
    Resize(u16, u16),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEffect {
    Search {
        query: String,
    },
    ObserveResourceUse {
        resource: ResourceRef,
    },
    LoadContextualActions {
        subject: ResourceRef,
    },
    InvokeContextualAction {
        action: ContextualActionDescriptor,
    },
    PreviewComposition {
        scope: ScopeKind,
        staged: StagedChanges,
    },
    ApplyComposition {
        preview: CompositionPreview,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiReduction {
    pub state: TuiState,
    pub effects: Vec<UiEffect>,
}

/// Executes reducer effects exclusively through [`TuiApplicationService`]. This is
/// the V2 equivalent of the proven V1 effect runtime: the TUI asks for application
/// semantics and feeds the resulting read model/preview/receipt back as UiActions.
#[derive(Debug, Default)]
pub struct TuiRuntime;

impl TuiRuntime {
    pub fn new() -> Self {
        Self
    }

    pub fn execute(
        &mut self,
        service: &mut dyn TuiApplicationService,
        effect: UiEffect,
    ) -> Result<UiAction> {
        match effect {
            UiEffect::Search { query } => Ok(UiAction::SearchFinished(service.search(&query)?)),
            UiEffect::ObserveResourceUse { resource } => {
                service.observe_resource_use(&resource)?;
                Ok(UiAction::ResourceUseObserved(resource))
            }
            UiEffect::LoadContextualActions { subject } => Ok(UiAction::ContextualActionsLoaded {
                actions: service.contextual_actions(&subject)?,
                subject,
            }),
            UiEffect::InvokeContextualAction { action } => {
                Ok(UiAction::ActionFinished(service.invoke_action(&action)?))
            }
            UiEffect::PreviewComposition { scope, staged } => Ok(UiAction::CompositionPreviewed(
                service.preview_composition(scope, &staged)?,
            )),
            UiEffect::ApplyComposition { preview } => Ok(UiAction::ApplyFinished(
                service.apply_composition(&preview)?,
            )),
        }
    }

    pub fn settle(
        &mut self,
        service: &mut dyn TuiApplicationService,
        mut state: TuiState,
        effects: Vec<UiEffect>,
    ) -> Result<TuiState> {
        let mut queue: VecDeque<UiEffect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            let action = self.execute(service, effect)?;
            let reduction = reduce_tui(state, action);
            state = reduction.state;
            queue.extend(reduction.effects);
        }
        Ok(state)
    }

    pub fn step(
        &mut self,
        service: &mut dyn TuiApplicationService,
        state: TuiState,
        action: UiAction,
    ) -> Result<TuiState> {
        let reduction = reduce_tui(state, action);
        self.settle(service, reduction.state, reduction.effects)
    }
}

/// Pure semantic reducer. Presentation adapters may turn keys, clicks, list rows,
/// tree nodes or graph nodes into these same actions; no input path receives a
/// second mutation implementation.
pub fn reduce_tui(mut state: TuiState, action: UiAction) -> TuiReduction {
    let mut effects = Vec::new();

    match action {
        UiAction::SetQuery(query) => {
            state.query = query.clone();
            state.action_query = None;
            state.action_cursor = 0;
            effects.push(UiEffect::Search { query });
        }
        UiAction::SearchFinished(model) | UiAction::Refresh(model) => {
            reconcile_read_model(&mut state, model);
            if let Some(subject) = state.selected.clone() {
                clear_contextual_actions(&mut state);
                effects.push(UiEffect::LoadContextualActions { subject });
            }
        }
        UiAction::Select(resource) => {
            if state.read_model.contains(&resource) {
                state.selected = Some(resource.clone());
                state.selection_invalidation = None;
                state.action_query = None;
                state.action_cursor = 0;
                clear_contextual_actions(&mut state);
                effects.push(UiEffect::LoadContextualActions { subject: resource });
            } else {
                state.selection_invalidation = Some(SelectionInvalidation {
                    previous: resource,
                    reason: "selected resource is not present in the current read model".into(),
                });
            }
        }
        UiAction::SelectNext => {
            if let Some(subject) = select_relative(&mut state, 1) {
                state.action_query = None;
                state.action_cursor = 0;
                clear_contextual_actions(&mut state);
                effects.push(UiEffect::LoadContextualActions { subject });
            }
        }
        UiAction::SelectPrevious => {
            if let Some(subject) = select_relative(&mut state, -1) {
                state.action_query = None;
                state.action_cursor = 0;
                clear_contextual_actions(&mut state);
                effects.push(UiEffect::LoadContextualActions { subject });
            }
        }
        UiAction::ContextualActionsLoaded { subject, actions } => {
            if state.selected.as_ref() == Some(&subject) {
                state.contextual_actions_for = Some(subject);
                state.contextual_actions = actions;
                state.action_cursor = state
                    .action_cursor
                    .min(visible_contextual_actions(&state).len().saturating_sub(1));
            }
        }
        UiAction::BeginActionSearch => {
            if state.selected.is_some() && !state.contextual_actions.is_empty() {
                state.action_query = Some(String::new());
                state.action_cursor = 0;
                state.action_result = None;
            } else {
                state.status = Some(UiStatus {
                    message: "the selected resource exposes no contextual actions".into(),
                });
            }
        }
        UiAction::SetActionQuery(query) => {
            if state.action_query.is_some() {
                state.action_query = Some(query);
                state.action_cursor = 0;
            }
        }
        UiAction::SelectNextAction => move_action_cursor(&mut state, 1),
        UiAction::SelectPreviousAction => move_action_cursor(&mut state, -1),
        UiAction::InvokeAction(action_ref) => {
            let available = visible_contextual_actions(&state);
            if let Some(action) = available
                .into_iter()
                .find(|candidate| candidate.action == action_ref)
            {
                effects.push(UiEffect::InvokeContextualAction { action });
            } else {
                state.status = Some(UiStatus {
                    message: format!(
                        "action {action_ref} is not available for the selected resource"
                    ),
                });
            }
        }
        UiAction::ActionFinished(outcome) => {
            state.action_query = None;
            state.action_cursor = 0;
            state.status = Some(UiStatus {
                message: outcome.summary().to_string(),
            });
            match &outcome {
                ActionOutcome::Opened { .. } => {
                    state.navigation.push(NavigationPoint {
                        selected: state.selected.clone(),
                        relation_view: state.relation_view,
                        workspace_section: state.workspace_section,
                    });
                }
                ActionOutcome::Explained { .. } => {
                    state.overlay = Some(Overlay::Explain);
                }
                ActionOutcome::History { .. } => {
                    state.navigation.push(NavigationPoint {
                        selected: state.selected.clone(),
                        relation_view: state.relation_view,
                        workspace_section: state.workspace_section,
                    });
                    state.overlay = None;
                    state.workspace_section = WorkspaceSection::History;
                }
                ActionOutcome::Staged {
                    resource, intent, ..
                } => {
                    state.staged.stage(resource.clone(), *intent);
                    state.preview = None;
                }
                ActionOutcome::Status { .. } => {}
            }
            state.action_result = Some(outcome);
        }
        UiAction::OpenSelection => {
            state.navigation.push(NavigationPoint {
                selected: state.selected.clone(),
                relation_view: state.relation_view,
                workspace_section: state.workspace_section,
            });
            if let Some(resource) = state.selected.clone() {
                effects.push(UiEffect::ObserveResourceUse { resource });
            }
        }
        UiAction::ResourceUseObserved(_resource) => {
            // Observation is evidence, not a new navigation or status transition.
            // The stable ResourceRef is carried back only so effect execution can
            // be tested without inventing a second acknowledgement identity.
        }
        UiAction::Back => {
            if state.action_query.take().is_some() {
                state.action_cursor = 0;
            } else if state.overlay.take().is_none() {
                if let Some(point) = state.navigation.pop() {
                    state.selected = point.selected.filter(|id| state.read_model.contains(id));
                    state.relation_view = point.relation_view;
                    state.workspace_section = point.workspace_section;
                    clear_contextual_actions(&mut state);
                    if let Some(subject) = state.selected.clone() {
                        effects.push(UiEffect::LoadContextualActions { subject });
                    }
                }
            }
        }
        UiAction::SetMutationScope(scope) => {
            if state.mutation_scope != Some(scope) {
                state.mutation_scope = Some(scope);
                state.preview = None;
            }
        }
        UiAction::SetPresentation(presentation) => state.presentation = presentation,
        UiAction::SetWorkspaceSection(section) => state.workspace_section = section,
        UiAction::NextWorkspaceSection => {
            state.workspace_section = state.workspace_section.relative(1)
        }
        UiAction::PreviousWorkspaceSection => {
            state.workspace_section = state.workspace_section.relative(-1)
        }
        UiAction::SetRelationView(view) => state.relation_view = view,
        UiAction::ShowOverlay(overlay) => state.overlay = Some(overlay),
        UiAction::Dismiss => {
            // Esc/dismiss is intentionally incapable of clearing query, staged
            // changes, selection, scope, or requesting application exit.
            if state.action_query.take().is_some() {
                state.action_cursor = 0;
            } else {
                state.overlay = None;
            }
        }
        UiAction::Stage { resource, intent } => {
            state.staged.stage(resource, intent);
            state.preview = None;
        }
        UiAction::Unstage(resource) => {
            state.staged.unstage(&resource);
            state.preview = None;
        }
        UiAction::DiscardStaged => {
            state.staged = StagedChanges::default();
            state.preview = None;
            state.overlay = None;
            state.status = Some(UiStatus {
                message: "staged changes discarded explicitly".into(),
            });
        }
        UiAction::RequestCompositionPreview => request_preview(&mut state, &mut effects),
        UiAction::CompositionPreviewed(preview) => {
            if state.mutation_scope == Some(preview.scope) && preview.staged == state.staged {
                state.preview = Some(preview);
                state.overlay = Some(Overlay::CompositionPreview);
            } else {
                state.preview = None;
                state.status = Some(UiStatus {
                    message: "composition preview became stale before it was displayed".into(),
                });
            }
        }
        UiAction::RequestApply => {
            let preview_is_current = state.preview.as_ref().is_some_and(|preview| {
                state.mutation_scope == Some(preview.scope) && preview.staged == state.staged
            });
            if preview_is_current && !state.staged.is_empty() {
                state.overlay = Some(Overlay::ConfirmApply);
            } else if !state.staged.is_empty() {
                state.preview = None;
                request_preview(&mut state, &mut effects);
            }
        }
        UiAction::ConfirmApply => {
            if state.overlay == Some(Overlay::ConfirmApply) {
                if let Some(preview) = state.preview.clone() {
                    if state.mutation_scope == Some(preview.scope) && preview.staged == state.staged
                    {
                        effects.push(UiEffect::ApplyComposition { preview });
                    } else {
                        state.overlay = None;
                        state.preview = None;
                        state.status = Some(UiStatus {
                            message:
                                "composition changed after preview; preview again before apply"
                                    .into(),
                        });
                    }
                }
            }
        }
        UiAction::ApplyFinished(receipt) => {
            state.staged = StagedChanges::default();
            state.preview = None;
            state.overlay = None;
            state.status = Some(UiStatus {
                message: receipt.summary,
            });
        }
        UiAction::Resize(cols, rows) => state.area = (cols, rows),
        UiAction::Exit => {
            if state.staged.is_empty() {
                state.exit_requested = true;
            } else {
                state.exit_requested = false;
                state.status = Some(UiStatus {
                    message: format!(
                        "{} staged change{} remain; apply or discard them explicitly before exit",
                        state.staged.len(),
                        if state.staged.len() == 1 { "" } else { "s" }
                    ),
                });
            }
        }
    }

    TuiReduction { state, effects }
}

fn request_preview(state: &mut TuiState, effects: &mut Vec<UiEffect>) {
    if state.staged.is_empty() {
        return;
    }
    if let Some(scope) = state.mutation_scope {
        effects.push(UiEffect::PreviewComposition {
            scope,
            staged: state.staged.clone(),
        });
    } else {
        state.status = Some(UiStatus {
            message: "mutation scope is unresolved; choose a scope before preview/apply".into(),
        });
    }
}

fn reconcile_read_model(state: &mut TuiState, model: ResourceListReadModel) {
    let previous = state.selected.clone();
    state.read_model = model;
    state.selection_invalidation = None;

    if let Some(selected) = previous {
        if state.read_model.contains(&selected) {
            state.selected = Some(selected);
        } else {
            state.selected = None;
            state.action_query = None;
            state.action_cursor = 0;
            clear_contextual_actions(state);
            state.selection_invalidation = Some(SelectionInvalidation {
                previous: selected,
                reason: "selected resource disappeared during refresh".into(),
            });
        }
    }
    // Staging is deliberately not reconciled away. A disappeared staged Ref is a
    // fact the application preview must explain, not permission for the TUI to
    // silently discard user intent.
}

fn clear_contextual_actions(state: &mut TuiState) {
    state.contextual_actions_for = None;
    state.contextual_actions.clear();
}

fn select_relative(state: &mut TuiState, delta: isize) -> Option<ResourceRef> {
    if state.read_model.resources.is_empty() {
        state.selected = None;
        clear_contextual_actions(state);
        return None;
    }
    let current = state
        .selected
        .as_ref()
        .and_then(|resource| state.read_model.position(resource));
    let next = match (current, delta.is_negative()) {
        (None, _) => 0,
        (Some(index), true) => index.saturating_sub(delta.unsigned_abs()),
        (Some(index), false) => {
            (index + delta as usize).min(state.read_model.resources.len().saturating_sub(1))
        }
    };
    state.selected = state.read_model.resource_at(next).cloned();
    state.selection_invalidation = None;
    state.selected.clone()
}

fn move_action_cursor(state: &mut TuiState, delta: isize) {
    if state.action_query.is_none() {
        return;
    }
    let len = visible_contextual_actions(state).len();
    if len == 0 {
        state.action_cursor = 0;
        return;
    }
    state.action_cursor = if delta.is_negative() {
        state.action_cursor.saturating_sub(delta.unsigned_abs())
    } else {
        (state.action_cursor + delta as usize).min(len.saturating_sub(1))
    };
}

/// Contextual Action results visible for the current text-action query. This calls
/// the core navigation matcher, so Quick and Workspace do not grow a second fuzzy
/// grammar.
pub fn visible_contextual_actions(state: &TuiState) -> Vec<ContextualActionDescriptor> {
    let query = state.action_query.as_deref().unwrap_or("");
    search_contextual_actions(&state.contextual_actions, query)
}

pub fn selected_contextual_action(state: &TuiState) -> Option<ContextualActionDescriptor> {
    visible_contextual_actions(state)
        .get(state.action_cursor)
        .cloned()
}

/// Presentation adapters resolve both keyboard navigation and mouse hit-testing to
/// the same stable resource selection action. Keeping these constructors trivial
/// is intentional: parity is structural rather than two reducers behaving alike
/// by convention.
pub fn keyboard_select(resource: ResourceRef) -> UiAction {
    UiAction::Select(resource)
}

pub fn mouse_select(resource: ResourceRef) -> UiAction {
    UiAction::Select(resource)
}

/// Returns staged refs that are currently absent from the latest read model. The
/// TUI exposes this set for explanation; it never deletes those changes itself.
pub fn unresolved_staged(state: &TuiState) -> BTreeSet<ResourceRef> {
    state
        .staged
        .resources()
        .filter(|resource| !state.read_model.contains(resource))
        .cloned()
        .collect()
}
