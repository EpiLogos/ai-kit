//! AIKit's V2 terminal application surface.
//!
//! There is one semantic authority: [`application::TuiState`] reduced by
//! [`application::TuiRuntime`] against [`application_service::ApplicationService`].
//! Quick/Workspace and List/Tree/Graph are presentations of that state, not
//! alternate controllers.

#![forbid(unsafe_code)]

pub mod application;
pub mod application_service;
pub mod application_surface;
pub mod backend;
pub mod credential_surface;
pub mod event;
pub mod explain_history_service;
pub mod host;
pub mod knowledge_service;
pub mod layout;
pub mod model_roster_surface;
pub mod navigation;
pub mod project_workspace;
pub mod project_workspace_render;
pub mod project_world_api;
pub mod project_world_service;
pub mod session_space_adapter;
pub mod session_space_service;
pub mod staging;
pub mod theme;
pub mod tree;
pub mod v2_render;
pub mod working_field;

use aikit_core::id::{CapsuleId, GenerationId};

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
pub use credential_surface::{
    credential_setup_widget, render_credential_setup_panel, CredentialSetupView,
};
pub use event::{EventSource, PaletteEvent, ScriptedEvents};
pub use explain_history_service::ExplainHistoryApplicationService;
pub use host::{Escalation, TerminalProfile, UiHost};
pub use knowledge_service::KnowledgeNavigationService;
pub use layout::{Glyphs, Layout, Width};
pub use model_roster_surface::model_roster_matrix;
pub use navigation::{
    keyboard_invoke_action, keyboard_open_hit, keyboard_select_hit, keyboard_set_presentation,
    mouse_invoke_action, mouse_open_hit, mouse_select_hit, mouse_set_presentation, stage_action,
    AmbientContext, NavigationIntent,
};
pub use project_workspace::{ComposeHorizon, ProjectWorkspaceSelection, ProjectWorkspaceState};
pub use project_world_api::ProjectWorldApplicationService;
pub use session_space_adapter::SessionSpaceApplicationAdapter;
pub use session_space_service::SessionSpaceApplicationProjection;
pub use theme::Theme;
pub use working_field::{
    select_working_field_subject, working_field_from_session_space, PermissionProjection,
    SurfaceProjection, TerminalContributionKind, TerminalWorkingField, WorkingFieldAvailability,
    WorkingFieldItem, TERMINAL_WORKING_FIELD_VERSION,
};

/// Terminal application outcome.
///
/// The extra variants remain only until the CLI's historical post-palette match is
/// migrated; the final V2 ApplicationSurface itself emits `Closed` only.
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteOutcome {
    Closed,
    Tree,
    Run(RunIntent),
    Applied(GenerationId),
    Promoted(CapsuleId),
}
