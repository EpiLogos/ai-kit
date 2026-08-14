//! V2 TUI semantic state and reducer foundation.
//!
//! This is the migration target for the existing Palette/Tree controller split.
//! The state here owns application meaning: stable resource selection, staged
//! resource mutations, presentation choice and explicit navigation/commit intent.
//! Palette/list/tree/graph cursors are projections of this state, never identities.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::resource::ResourceRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presentation {
    #[default]
    Quick,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionInvalidation {
    pub resource: ResourceRef,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewState {
    #[default]
    None,
    Required,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    ApplyConfirmation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedMutation {
    pub resource: ResourceRef,
    pub enable: bool,
}

/// One authoritative semantic state for the human TUI.
///
/// `query`, `area` and `presentation` are presentation/application-navigation
/// state. `selected` and `staged` are stable semantic state. No resolver,
/// provider, eligibility or provenance rule is reproduced here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiState {
    pub presentation: Presentation,
    pub selected: Option<ResourceRef>,
    pub selection_invalidation: Option<SelectionInvalidation>,
    pub query: String,
    pub area: (u16, u16),
    pub staged: BTreeMap<ResourceRef, bool>,
    pub preview: PreviewState,
    pub overlay: Option<Overlay>,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            presentation: Presentation::Quick,
            selected: None,
            selection_invalidation: None,
            query: String::new(),
            area: (80, 24),
            staged: BTreeMap::new(),
            preview: PreviewState::None,
            overlay: None,
        }
    }
}

impl TuiState {
    pub fn new(presentation: Presentation) -> Self {
        Self {
            presentation,
            ..Self::default()
        }
    }

    pub fn staged_mutations(&self) -> Vec<StagedMutation> {
        self.staged
            .iter()
            .map(|(resource, enable)| StagedMutation {
                resource: resource.clone(),
                enable: *enable,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Select(Option<ResourceRef>),
    /// The application service has produced a fresh addressable resource field.
    ResourcesRefreshed(BTreeSet<ResourceRef>),
    Present(Presentation),
    Resize(u16, u16),
    SetQuery(String),
    ClearQuery,
    Stage { resource: ResourceRef, enable: bool },
    Unstage(ResourceRef),
    PreviewReady,
    RequestApply,
    ConfirmApply,
    ApplyFinished,
    DiscardStaged,
    /// Semantic back: dismiss the top local overlay only. It never clears query,
    /// discards staged work, applies, or exits.
    Back,
    /// Explicit surface-exit intent. Kept distinct from Back/Esc semantics.
    RequestExit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEffect {
    PreviewComposition(Vec<StagedMutation>),
    ApplyComposition(Vec<StagedMutation>),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiReduction {
    pub state: TuiState,
    pub effects: Vec<UiEffect>,
}

impl TuiReduction {
    fn plain(state: TuiState) -> Self {
        Self {
            state,
            effects: Vec::new(),
        }
    }

    fn with(state: TuiState, effect: UiEffect) -> Self {
        Self {
            state,
            effects: vec![effect],
        }
    }
}

/// Pure V2 semantic reducer.
pub fn reduce_tui(mut state: TuiState, action: UiAction) -> TuiReduction {
    match action {
        UiAction::Select(selected) => {
            state.selected = selected;
            state.selection_invalidation = None;
            TuiReduction::plain(state)
        }
        UiAction::ResourcesRefreshed(available) => {
            if let Some(selected) = state.selected.as_ref() {
                if !available.contains(selected) {
                    let resource = selected.clone();
                    state.selected = None;
                    state.selection_invalidation = Some(SelectionInvalidation {
                        resource,
                        reason: "selected resource is not present in the refreshed read model"
                            .to_string(),
                    });
                } else {
                    state.selection_invalidation = None;
                }
            }
            // Staging is durable user intent until an explicit apply/discard;
            // refresh never silently edits it.
            TuiReduction::plain(state)
        }
        UiAction::Present(presentation) => {
            state.presentation = presentation;
            TuiReduction::plain(state)
        }
        UiAction::Resize(cols, rows) => {
            state.area = (cols, rows);
            TuiReduction::plain(state)
        }
        UiAction::SetQuery(query) => {
            state.query = query;
            TuiReduction::plain(state)
        }
        UiAction::ClearQuery => {
            state.query.clear();
            TuiReduction::plain(state)
        }
        UiAction::Stage { resource, enable } => {
            state.staged.insert(resource, enable);
            state.overlay = None;
            state.preview = PreviewState::Required;
            let mutations = state.staged_mutations();
            TuiReduction::with(state, UiEffect::PreviewComposition(mutations))
        }
        UiAction::Unstage(resource) => {
            state.staged.remove(&resource);
            state.overlay = None;
            if state.staged.is_empty() {
                state.preview = PreviewState::None;
                TuiReduction::plain(state)
            } else {
                state.preview = PreviewState::Required;
                let mutations = state.staged_mutations();
                TuiReduction::with(state, UiEffect::PreviewComposition(mutations))
            }
        }
        UiAction::PreviewReady => {
            if !state.staged.is_empty() {
                state.preview = PreviewState::Ready;
            }
            TuiReduction::plain(state)
        }
        UiAction::RequestApply => {
            if state.staged.is_empty() {
                return TuiReduction::plain(state);
            }
            if state.preview != PreviewState::Ready {
                state.preview = PreviewState::Required;
                let mutations = state.staged_mutations();
                return TuiReduction::with(state, UiEffect::PreviewComposition(mutations));
            }
            state.overlay = Some(Overlay::ApplyConfirmation);
            TuiReduction::plain(state)
        }
        UiAction::ConfirmApply => {
            if state.overlay != Some(Overlay::ApplyConfirmation)
                || state.preview != PreviewState::Ready
                || state.staged.is_empty()
            {
                return TuiReduction::plain(state);
            }
            state.overlay = None;
            let mutations = state.staged_mutations();
            TuiReduction::with(state, UiEffect::ApplyComposition(mutations))
        }
        UiAction::ApplyFinished => {
            state.staged.clear();
            state.preview = PreviewState::None;
            state.overlay = None;
            TuiReduction::plain(state)
        }
        UiAction::DiscardStaged => {
            state.staged.clear();
            state.preview = PreviewState::None;
            state.overlay = None;
            TuiReduction::plain(state)
        }
        UiAction::Back => {
            state.overlay = None;
            TuiReduction::plain(state)
        }
        UiAction::RequestExit => TuiReduction::with(state, UiEffect::Exit),
    }
}
