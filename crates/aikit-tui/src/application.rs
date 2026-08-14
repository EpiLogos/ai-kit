//! V2 TUI application-service and authoritative interaction-state foundation.
//!
//! The V1 palette and tree remain useful presentation implementations, but they
//! must not remain independent semantic owners. This module defines the state that
//! list, tree and future graph views share: one selected [`ResourceRef`], one
//! staged change set and mutation scope, one navigation/overlay history, and
//! refresh reconciliation by stable identity rather than row index.
//!
//! Application semantics stay behind [`TuiApplicationService`]. The reducer is
//! pure and can only request effects; it cannot resolve capabilities, eligibility,
//! provenance, composition or history itself.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::scope::ScopeKind;
use aikit_core::Result;
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
    pub staged: StagedChanges,
    /// Mutation scope is semantic intent, not view chrome. A staged change without
    /// its scope is incomplete and may not be previewed or applied.
    pub mutation_scope: Option<ScopeKind>,
    pub presentation: PresentationMode,
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
            staged: StagedChanges::default(),
            mutation_scope: None,
            presentation: PresentationMode::Quick,
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
    OpenSelection,
    Back,
    SetMutationScope(ScopeKind),
    SetPresentation(PresentationMode),
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
            UiEffect::PreviewComposition { scope, staged } => Ok(UiAction::CompositionPreviewed(
                service.preview_composition(scope, &staged)?,
            )),
            UiEffect::ApplyComposition { preview } => {
                Ok(UiAction::ApplyFinished(service.apply_composition(&preview)?))
            }
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
            effects.push(UiEffect::Search { query });
        }
        UiAction::SearchFinished(model) | UiAction::Refresh(model) => {
            reconcile_read_model(&mut state, model);
        }
        UiAction::Select(resource) => {
            if state.read_model.contains(&resource) {
                state.selected = Some(resource);
                state.selection_invalidation = None;
            } else {
                state.selection_invalidation = Some(SelectionInvalidation {
                    previous: resource,
                    reason: "selected resource is not present in the current read model".into(),
                });
            }
        }
        UiAction::SelectNext => select_relative(&mut state, 1),
        UiAction::SelectPrevious => select_relative(&mut state, -1),
        UiAction::OpenSelection => {
            state.navigation.push(NavigationPoint {
                selected: state.selected.clone(),
                relation_view: state.relation_view,
            });
        }
        UiAction::Back => {
            if state.overlay.take().is_none() {
                if let Some(point) = state.navigation.pop() {
                    state.selected = point.selected.filter(|id| state.read_model.contains(id));
                    state.relation_view = point.relation_view;
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
        UiAction::SetRelationView(view) => state.relation_view = view,
        UiAction::ShowOverlay(overlay) => state.overlay = Some(overlay),
        UiAction::Dismiss => {
            // Esc/dismiss is intentionally incapable of clearing query, staged
            // changes, selection, scope, or requesting application exit.
            state.overlay = None;
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
                    if state.mutation_scope == Some(preview.scope) && preview.staged == state.staged {
                        effects.push(UiEffect::ApplyComposition { preview });
                    } else {
                        state.overlay = None;
                        state.preview = None;
                        state.status = Some(UiStatus {
                            message: "composition changed after preview; preview again before apply"
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

fn select_relative(state: &mut TuiState, delta: isize) {
    if state.read_model.resources.is_empty() {
        state.selected = None;
        return;
    }
    let current = state
        .selected
        .as_ref()
        .and_then(|resource| state.read_model.position(resource));
    let next = match (current, delta.is_negative()) {
        (None, _) => 0,
        (Some(index), true) => index.saturating_sub(delta.unsigned_abs()),
        (Some(index), false) => (index + delta as usize)
            .min(state.read_model.resources.len().saturating_sub(1)),
    };
    state.selected = state.read_model.resource_at(next).cloned();
    state.selection_invalidation = None;
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
