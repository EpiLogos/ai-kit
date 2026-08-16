//! AIKit's terminal application surfaces.
//!
//! V2 has one semantic authority: [`application::TuiState`] reduced by
//! [`application::TuiRuntime`] against shared application services. Quick and
//! Workspace, plus List/Tree/Graph relation presentations, are views of that
//! state. Compatibility modules remain only while #59 migrates their remaining
//! external callers; the shipped V2 surface is [`application_surface`].

#![forbid(unsafe_code)]

pub mod app;
pub mod application;
pub mod application_service;
pub mod application_surface;
pub mod backend;
pub mod event;
pub mod form;
pub mod host;
pub mod knowledge_service;
pub mod layout;
pub mod navigation;
pub mod palette_service;
pub mod project_workspace;
pub mod project_workspace_render;
pub mod project_world_api;
pub mod project_world_service;
pub mod render;
pub mod scope;
pub mod search;
pub mod staging;
pub mod surface;
pub mod theme;
pub mod tree;
pub mod tree_driver;
pub mod v2_render;

pub mod driver;

use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::scope::ScopeKind;

pub use app::{reduce, Action, AppState, Effect, ManageAction, Mode, Reduction};
pub use application::{
    keyboard_select, mouse_select, reduce_tui, selected_contextual_action, unresolved_staged,
    ActionOutcome, ActivationIntent, ApplyReceipt, CompositionPreview, HistoryEntry,
    NavigationPoint, Overlay, PresentationMode, RelationReadModel, RelationView, ResourceListItem,
    ResourceListReadModel, SelectionInvalidation, StagedChanges, TuiApplicationService,
    TuiReduction, TuiRuntime, TuiState, UiAction, UiEffect, UiStatus, WorkspaceSection,
};
pub use application_service::ApplicationService;
pub use application_surface::{
    ApplicationSurfaceController, ApplicationSurfaceRequest, ApplicationSurfaceStep,
};
pub use backend::{
    ClientEffect, JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle,
};
pub use event::{EventSource, PaletteEvent, ScriptedEvents};
pub use form::{ArgForm, RunPreview};
pub use host::{Escalation, TerminalProfile, UiHost};
pub use knowledge_service::KnowledgeNavigationService;
pub use layout::{Glyphs, Layout, Width};
pub use navigation::{
    keyboard_invoke_action, keyboard_open_hit, keyboard_select_hit, keyboard_set_presentation,
    mouse_invoke_action, mouse_open_hit, mouse_select_hit, mouse_set_presentation, stage_action,
    AmbientContext, NavigationIntent,
};
pub use palette_service::PaletteApplicationService;
pub use project_workspace::{ComposeHorizon, ProjectWorkspaceSelection, ProjectWorkspaceState};
pub use project_world_api::ProjectWorldApplicationService;
pub use scope::ScopeSelector;
pub use search::{rank, Matcher, Row};
pub use staging::{stage, StagedDiff, StagedProblem, StagedSet};
pub use theme::Theme;

/// Compatibility request for the pre-V2 palette entry point.
///
/// The shipped `aikit ui` path uses [`ApplicationSurfaceRequest`]. This remains
/// temporarily for explicit compatibility consumers while #59 closes them.
pub struct PaletteRequest {
    pub host: UiHost,
    pub initial_query: Option<String>,
    pub activate_initial: bool,
    pub activation_target: Option<aikit_core::CapsuleId>,
    pub scope: Option<ScopeKind>,
}

impl PaletteRequest {
    pub fn new(host: UiHost) -> Self {
        Self {
            host,
            initial_query: None,
            activate_initial: false,
            activation_target: None,
            scope: None,
        }
    }

    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.initial_query = Some(query.into());
        self
    }

    #[must_use]
    pub fn activating_initial(mut self) -> Self {
        if let Some(query) = self.initial_query.as_deref() {
            if let Ok(target) = query.parse::<aikit_core::CapsuleId>() {
                self.initial_query = Some(target.path().to_string());
                self.activation_target = Some(target);
                self.activate_initial = true;
            }
        }
        self
    }

    #[must_use]
    pub fn with_scope(mut self, scope: ScopeKind) -> Self {
        self.scope = Some(scope);
        self
    }
}

/// What a terminal operation did before it closed.
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteOutcome {
    Closed,
    Tree,
    Run(RunIntent),
    Applied(GenerationId),
    Promoted(CapsuleId),
}

/// Compatibility entry point for the old standalone tree. New UI callers must
/// select [`RelationView::Tree`] on [`ApplicationSurfaceRequest`] instead.
pub fn run_tree(
    state: tree::TreeState,
    request: tree_driver::TreeRequest,
) -> aikit_core::Result<tree_driver::TreeOutcome> {
    tree_driver::run_on_terminal(state, request)
}

/// Compatibility entry point for the old palette. New UI callers use
/// [`application_surface::run_on_terminal`].
pub fn run(app: &mut dyn PaletteBackend, request: PaletteRequest) -> aikit_core::Result<PaletteOutcome> {
    driver::run_on_terminal(app, request)
}
