//! The AIKit palette.
//!
//! It is a **palette, not a dashboard**. It opens, does one or a few things, and
//! disappears. There is no persistent control-centre chrome, no background
//! refresh loop, and no state that outlives the invocation — closing it returns
//! the user to the work with the terminal exactly as they left it.
//!
//! ## What this crate refuses to do
//!
//! **It contains no resolver semantics.** Not a dependency rule, not a scope
//! precedence rule, not a ranking formula. Every question of the form "what would
//! happen if…" is asked of `aikit-core` through [`PaletteBackend`]. If the palette
//! ever computed an answer that `aikit explain` would compute differently, the
//! two front-ends would disagree about the same system, and the one that lies is
//! whichever the user is looking at.
//!
//! It also does not shell out to `aikit --json`. The CLI and the palette share one
//! application service; [`PaletteBackend`] is the shape of that service as the
//! palette needs it.
//!
//! ## The architecture is unidirectional
//!
//! ```text
//! event → Action → reduce → AppState → render
//!            ▲                  │
//!            └──── Effect ◄─────┘
//! ```
//!
//! [`app::reduce`] is a pure function: state in, state and a list of effects out.
//! Effects are the only things that touch the backend, and everything they learn
//! comes back as another [`app::Action`]. That is why nearly every test in this
//! crate is a reducer test — the interesting behaviour is reachable without a
//! terminal, and the part that does need a terminal is [`render`], which is
//! snapshot-tested against a real `TestBackend`.
//!
//! V2 builds on that proven reducer discipline with [`application::TuiState`]: one
//! semantic ResourceRef selection/staging/navigation state shared by Quick,
//! Workspace, list, tree and future graph presentations. The older palette/tree
//! models remain compatibility presentations while that state is adopted.
//!
//! ## Where to look
//!
//! | Question | Module |
//! |---|---|
//! | What is the V2 semantic TUI state/application-service seam? | [`application`] |
//! | How does the existing shared backend feed the V2 service? | [`palette_service`] |
//! | How are already-resolved V2 Resources added to search? | [`navigation`] |
//! | What is the resolved Project world shown by Workspace? | [`project_world`] |
//! | What does a key do? | [`app`], [`event`] |
//! | Why is this row above that one? | [`search`] |
//! | What would this toggle actually cost? | [`staging`] |
//! | Where would a change be written, and what confirms it? | [`scope`] |
//! | How is an argument form built from a manifest? | [`form`] |
//! | What fits at this width? | [`layout`] |
//! | Where does the palette appear? | [`host`] |
//! | What does it look like? | [`render`], [`v2_render`], [`theme`] |
//! | What does the palette need from the application? | [`backend`] |

#![forbid(unsafe_code)]

pub mod app;
pub mod application;
pub mod backend;
pub mod event;
pub mod form;
pub mod host;
pub mod layout;
pub mod navigation;
pub mod palette_service;
pub mod project_world;
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
    keyboard_select, mouse_select, reduce_tui, unresolved_staged, ActionOutcome, ActivationIntent,
    ApplyReceipt, CompositionPreview, HistoryEntry, NavigationPoint, Overlay, PresentationMode,
    RelationReadModel, RelationView, ResourceListItem, ResourceListReadModel, SelectionInvalidation,
    StagedChanges, TuiApplicationService, TuiReduction, TuiRuntime, TuiState, UiAction, UiEffect,
    UiStatus, WorkspaceSection,
};
pub use backend::{
    ClientEffect, JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle,
};
pub use event::{EventSource, PaletteEvent, ScriptedEvents};
pub use form::{ArgForm, RunPreview};
pub use host::{Escalation, TerminalProfile, UiHost};
pub use layout::{Glyphs, Layout, Width};
pub use navigation::resolved_navigation_index;
pub use palette_service::PaletteApplicationService;
pub use project_world::{
    ActorRuntimeWorld, CapabilityHorizon, DeclaredCapability, GenerationDisclosure,
    InformationHorizon, ProjectWorldIdentity, ProjectWorldReadModel, ProjectWorldResource,
    ProjectWorldResourceSet, ProjectionWorld, UnavailableCapability,
};
pub use scope::ScopeSelector;
pub use search::{rank, Matcher, Row};
pub use staging::{stage, StagedDiff, StagedProblem, StagedSet};
pub use theme::Theme;

/// How the CLI asks for a palette.
pub struct PaletteRequest {
    pub host: UiHost,
    pub initial_query: Option<String>,
    /// Open the first exact result as soon as the initial search settles. The
    /// tree uses this to hand a leaf directly to its natural palette action
    /// (details, form, or run) without requiring a redundant second Enter.
    pub activate_initial: bool,
    /// Exact capsule the tree handed to the palette. The visible query uses its
    /// searchable path, while this typed identity prevents a fuzzy neighbour
    /// from being activated by mistake.
    pub activation_target: Option<aikit_core::CapsuleId>,
    /// Override the mutation scope the palette starts on. `None` means the
    /// context's own default (see [`aikit_core::ContextDescriptor::default_mutation_scope`]).
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

/// What the palette did before it closed.
///
/// `Run` hands back rather than executing because a foreground or `replace`
/// execution mode needs the terminal the palette is currently holding: the caller
/// tears the palette down and *then* runs the command, so the child inherits a
/// clean terminal instead of a raw-mode one.
#[derive(Debug, Clone, PartialEq)]
pub enum PaletteOutcome {
    Closed,
    Tree,
    Run(RunIntent),
    Applied(GenerationId),
    Promoted(CapsuleId),
}

/// Open the interactive organising tree and return the operation selected after
/// the terminal has been restored.
pub fn run_tree(
    state: tree::TreeState,
    request: tree_driver::TreeRequest,
) -> aikit_core::Result<tree_driver::TreeOutcome> {
    tree_driver::run_on_terminal(state, request)
}

/// Open the palette, run it to completion, and return what it did.
///
/// Terminal setup and teardown happen here so that the outcome is delivered to a
/// caller holding a restored terminal. The loop itself is
/// [`driver::event_loop`], which is generic over the ratatui backend and the
/// event source — that is what lets the end-to-end tests drive a real palette
/// against a `TestBackend` and a scripted key sequence.
pub fn run(app: &mut dyn PaletteBackend, request: PaletteRequest) -> aikit_core::Result<PaletteOutcome> {
    driver::run_on_terminal(app, request)
}
