//! One transient AIKit surface with Quick and Workspace presentations.
//!
//! During the V2 migration the existing PaletteController and TreeController are
//! retained as presentation adapters, but semantic presentation/selection already
//! lives in [`TuiState`]. The remaining staged-state adapter is intentionally
//! visible here until the next migration slice removes the duplicated V1 graph.

use std::collections::BTreeSet;
use std::io;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::{Terminal, TerminalOptions, Viewport};

use aikit_core::error::AikitError;
use aikit_core::id::CapsuleId;
use aikit_core::resource::ResourceRef;
use aikit_core::Result;

use crate::app::Action;
use crate::backend::{PaletteBackend, Toggle};
use crate::driver::{PaletteController, PaletteStep};
use crate::event::{CrosstermEvents, EventSource, PaletteEvent};
use crate::host::UiHost;
use crate::staging::is_on;
use crate::tree::{NodeKind, TreeEffect, TreeState};
use crate::tree_driver::{TreeController, TreeRequest, TreeStep};
use crate::tui_state::{reduce_tui, Presentation, TuiState, UiAction};
use crate::{PaletteOutcome, PaletteRequest};

/// The application operations the unified surface needs beyond the palette.
pub trait SurfaceBackend: PaletteBackend {
    fn surface_tree(&self) -> Result<TreeState>;
    fn apply_tree_effect(&mut self, effect: TreeEffect) -> Result<()>;
}

/// V1-compatible names for the two current presentations.
///
/// The semantic state uses `Quick`/`Workspace`: Palette/Tree remain only at the
/// compatibility boundary while callers migrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceMode {
    Palette,
    Tree,
}

impl From<SurfaceMode> for Presentation {
    fn from(value: SurfaceMode) -> Self {
        match value {
            SurfaceMode::Palette => Presentation::Quick,
            SurfaceMode::Tree => Presentation::Workspace,
        }
    }
}

impl From<Presentation> for SurfaceMode {
    fn from(value: Presentation) -> Self {
        match value {
            Presentation::Quick => SurfaceMode::Palette,
            Presentation::Workspace => SurfaceMode::Tree,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceStep {
    Continue,
    Outcome(PaletteOutcome),
}

#[derive(Debug, Clone)]
pub struct SurfaceRequest {
    pub host: UiHost,
    pub initial_query: Option<String>,
    pub initial_mode: SurfaceMode,
}

impl SurfaceRequest {
    pub fn new(host: UiHost) -> Self {
        Self {
            host,
            initial_query: None,
            initial_mode: SurfaceMode::Palette,
        }
    }

    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.initial_query = Some(query.into());
        self
    }

    #[must_use]
    pub fn opening_tree(mut self) -> Self {
        self.initial_mode = SurfaceMode::Tree;
        self
    }

    fn palette_request(&self) -> PaletteRequest {
        let mut request = PaletteRequest::new(self.host);
        if let Some(query) = self.initial_query.clone() {
            request = request.with_query(query);
        }
        request
    }
}

pub struct SurfaceController {
    state: TuiState,
    host: UiHost,
    palette: PaletteController,
    tree: TreeController,
}

impl SurfaceController {
    pub fn new<B: SurfaceBackend>(backend: &mut B, request: SurfaceRequest) -> Result<Self> {
        let palette = PaletteController::new(backend, request.palette_request())?;
        let tree = TreeController::new(backend.surface_tree()?, TreeRequest::new(request.host));
        let mut state = TuiState::new(request.initial_mode.into());
        state.query = palette.state().query.clone();
        let mut surface = Self {
            state,
            host: request.host,
            palette,
            tree,
        };
        match surface.state.presentation {
            Presentation::Quick => surface.capture_palette_selection(),
            Presentation::Workspace => surface.capture_tree_selection(),
        }
        surface.sync_palette_to_tree();
        Ok(surface)
    }

    pub fn mode(&self) -> SurfaceMode {
        self.state.presentation.into()
    }

    pub fn tui_state(&self) -> &TuiState {
        &self.state
    }

    pub fn palette(&self) -> &PaletteController {
        &self.palette
    }

    pub fn tree(&self) -> &TreeController {
        &self.tree
    }

    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        self.reduce_semantic(UiAction::Resize(area.width, area.height));
        match self.state.presentation {
            Presentation::Quick => self.palette.draw(frame),
            Presentation::Workspace => self.tree.draw(frame),
        }
    }

    /// Draw one production frame through a caller-owned terminal.
    ///
    /// Keeping this seam on the controller makes the release performance gate
    /// measure the same draw path as the event loop rather than a test-only
    /// rendering approximation.
    pub fn draw_terminal<T>(&mut self, terminal: &mut Terminal<T>) -> Result<()>
    where
        T: Backend,
        T::Error: std::fmt::Display,
    {
        terminal
            .draw(|frame| self.draw(frame))
            .map(|_| ())
            .map_err(|error| draw_error("could not draw the surface", error))
    }

    pub fn handle<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        event: PaletteEvent,
    ) -> Result<SurfaceStep> {
        if let PaletteEvent::Resize(cols, rows) = &event {
            self.reduce_semantic(UiAction::Resize(*cols, *rows));
        }
        match self.state.presentation {
            Presentation::Quick => {
                let step = self.palette.handle(backend, event)?;
                self.capture_palette_state();
                self.handle_palette_step(backend, step)
            }
            Presentation::Workspace => {
                let step = self.tree.handle(event)?;
                self.capture_tree_selection();
                self.handle_tree_step(backend, step)
            }
        }
    }

    fn handle_palette_step<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        step: PaletteStep,
    ) -> Result<SurfaceStep> {
        Ok(match step {
            PaletteStep::Continue => SurfaceStep::Continue,
            PaletteStep::Tree => {
                self.sync_palette_to_tree();
                self.set_presentation(Presentation::Workspace);
                self.align_tree_to_selected();
                SurfaceStep::Continue
            }
            PaletteStep::Outcome(PaletteOutcome::Applied(generation)) => {
                self.refresh_after_change(backend, format!("applied generation {generation}"))?;
                SurfaceStep::Continue
            }
            PaletteStep::Outcome(PaletteOutcome::Promoted(capsule)) => {
                self.refresh_after_change(backend, format!("promoted {capsule}"))?;
                SurfaceStep::Continue
            }
            PaletteStep::Outcome(outcome) => SurfaceStep::Outcome(outcome),
        })
    }

    fn handle_tree_step<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        step: TreeStep,
    ) -> Result<SurfaceStep> {
        match step {
            TreeStep::Continue => {
                self.sync_tree_to_palette(backend);
                Ok(SurfaceStep::Continue)
            }
            TreeStep::Palette => {
                self.sync_tree_to_palette(backend);
                self.set_presentation(Presentation::Quick);
                self.align_palette_to_selected();
                Ok(SurfaceStep::Continue)
            }
            TreeStep::Apply(_) => {
                self.sync_tree_to_palette(backend);
                self.set_presentation(Presentation::Quick);
                self.align_palette_to_selected();
                let step = self.palette.dispatch(backend, Action::CtrlEnter);
                self.capture_palette_state();
                self.handle_palette_step(backend, step)
            }
            TreeStep::Effect(TreeEffect::Activate { capsule }) => {
                self.sync_tree_to_palette(backend);
                self.set_selected(resource_ref_for_capsule(&capsule));
                self.set_presentation(Presentation::Quick);
                self.align_palette_to_selected();
                let step = self.palette.activate(backend, capsule);
                self.capture_palette_query();
                self.handle_palette_step(backend, step)
            }
            TreeStep::Effect(effect) => {
                if let Err(error) = backend.apply_tree_effect(effect) {
                    self.tree.report_error(&error);
                    return Ok(SurfaceStep::Continue);
                }
                match backend.surface_tree() {
                    Ok(tree) => self.replace_tree(tree),
                    Err(error) => self.tree.report_error(&error),
                }
                Ok(SurfaceStep::Continue)
            }
        }
    }

    /// Temporary V1 staging adapter. Selection/presentation no longer depend on
    /// this synchronisation; staged graph ownership moves into `TuiState` next.
    fn sync_palette_to_tree(&mut self) {
        self.tree.state_mut().staged = self
            .palette
            .state()
            .staged
            .toggles()
            .into_iter()
            .map(|toggle| toggle.capsule)
            .collect();
    }

    /// Temporary V1 staging adapter. Kept explicit so #40 cannot accidentally be
    /// declared closed while duplicate staged state still exists.
    fn sync_tree_to_palette<B: SurfaceBackend>(&mut self, backend: &mut B) {
        let ids = self.tree.state().staged.clone();
        let current: BTreeSet<_> = self
            .palette
            .state()
            .staged
            .toggles()
            .into_iter()
            .map(|toggle| toggle.capsule)
            .collect();
        if ids == current {
            return;
        }
        let toggles = ids
            .into_iter()
            .map(|capsule| {
                let enable = self
                    .palette
                    .state()
                    .staged
                    .state_of(&capsule)
                    .unwrap_or_else(|| !is_on(&self.palette.state().view, &capsule));
                Toggle::new(capsule, enable)
            })
            .collect();
        self.palette.replace_staged(backend, toggles);
    }

    fn refresh_after_change<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        message: String,
    ) -> Result<()> {
        let query = self.palette.state().query.clone();
        let scope = self.palette.state().scope.current();
        self.palette = PaletteController::new(
            backend,
            PaletteRequest::new(self.host)
                .with_query(query)
                .with_scope(scope),
        )?;
        self.palette.state_mut().status = Some(crate::app::Status::info(message));
        self.align_palette_to_selected();
        self.capture_palette_query();
        self.replace_tree(backend.surface_tree()?);
        self.sync_palette_to_tree();
        Ok(())
    }

    fn replace_tree(&mut self, mut state: TreeState) {
        let old = self.tree.state();
        let old_selected_path = old.selected_row().map(|row| row.path);
        state.expanded = old.expanded.clone();
        state.filter = old.filter.clone();
        state.yanked = old.yanked.clone();
        state.staged = old.staged.clone();
        self.tree = TreeController::new(state, TreeRequest::new(self.host));
        if self.align_tree_to_selected() {
            return;
        }
        if let Some(path) = old_selected_path {
            if let Some(index) = self
                .tree
                .state()
                .rows()
                .iter()
                .position(|row| row.path == path)
            {
                self.tree.state_mut().selected = index;
            }
        }
    }

    fn set_presentation(&mut self, presentation: Presentation) {
        self.reduce_semantic(UiAction::Present(presentation));
    }

    fn set_selected(&mut self, selected: Option<ResourceRef>) {
        self.reduce_semantic(UiAction::Select(selected));
    }

    fn reduce_semantic(&mut self, action: UiAction) {
        self.state = reduce_tui(self.state.clone(), action).state;
    }

    fn capture_palette_state(&mut self) {
        self.capture_palette_query();
        self.capture_palette_selection();
    }

    fn capture_palette_query(&mut self) {
        let query = self.palette.state().query.clone();
        self.reduce_semantic(UiAction::SetQuery(query));
    }

    fn capture_palette_selection(&mut self) {
        let selected = self
            .palette
            .state()
            .selected_row()
            .and_then(|row| resource_ref_for_capsule(&row.doc.id));
        self.set_selected(selected);
    }

    fn capture_tree_selection(&mut self) {
        let selected = self
            .tree
            .state()
            .selected_row()
            .and_then(|row| resource_ref_for_tree_kind(&row.node.kind));
        self.set_selected(selected);
    }

    fn align_palette_to_selected(&mut self) -> bool {
        let Some(selected) = self.state.selected.as_ref() else {
            return false;
        };
        let Ok(capsule) = CapsuleId::parse(selected.as_str()) else {
            return false;
        };
        let Some(index) = self
            .palette
            .state()
            .rows
            .iter()
            .position(|row| row.doc.id == capsule)
        else {
            return false;
        };
        self.palette.state_mut().cursor = index;
        true
    }

    fn align_tree_to_selected(&mut self) -> bool {
        let Some(selected) = self.state.selected.as_ref() else {
            return false;
        };
        let rows = self.tree.state().rows();
        let Some(index) = rows.iter().position(|row| {
            resource_ref_for_tree_kind(&row.node.kind).as_ref() == Some(selected)
        }) else {
            return false;
        };
        self.tree.state_mut().selected = index;
        true
    }
}

fn resource_ref_for_capsule(capsule: &CapsuleId) -> Option<ResourceRef> {
    // V1 capabilities already have stable content identity. During migration the
    // same opaque string is lifted into the V2 resource-address space; the V2
    // application service will eventually hand ResourceRef to the TUI directly.
    ResourceRef::parse(&capsule.to_string()).ok()
}

fn resource_ref_for_tree_kind(kind: &NodeKind) -> Option<ResourceRef> {
    match kind {
        NodeKind::Capability { id } => resource_ref_for_capsule(id),
        NodeKind::HookStep { capsule, .. } => resource_ref_for_capsule(capsule),
        NodeKind::Root(_)
        | NodeKind::Set { .. }
        | NodeKind::Group { .. }
        | NodeKind::Entry { .. } => None,
    }
}

pub fn event_loop<B, T, E>(
    terminal: &mut Terminal<T>,
    events: &mut E,
    backend: &mut B,
    request: SurfaceRequest,
) -> Result<PaletteOutcome>
where
    B: SurfaceBackend,
    T: Backend,
    T::Error: std::fmt::Display,
    E: EventSource + ?Sized,
{
    let mut controller = SurfaceController::new(backend, request)?;
    loop {
        controller.draw_terminal(terminal)?;
        let Some(event) = events.next()? else {
            return Ok(PaletteOutcome::Closed);
        };
        match controller.handle(backend, event)? {
            SurfaceStep::Continue => {}
            SurfaceStep::Outcome(outcome) => return Ok(outcome),
        }
    }
}

pub fn run_on_terminal<B: SurfaceBackend>(
    backend: &mut B,
    request: SurfaceRequest,
) -> Result<PaletteOutcome> {
    let host = request.host;
    let fullscreen = host == UiHost::Fullscreen;
    let _session = TerminalSession::enter(fullscreen)?;
    run_inner(backend, request, host)
}

/// Owns every terminal mode as soon as it is acquired.
///
/// Setup can fail between raw mode, the alternate screen, and mouse capture.
/// Keeping acquisition flags in a guard makes those partial paths use the same
/// reversal as ordinary exits and render errors.
struct TerminalSession {
    raw: bool,
    alternate: bool,
    mouse: bool,
}

impl TerminalSession {
    fn enter(fullscreen: bool) -> Result<Self> {
        let mut session = Self {
            raw: false,
            alternate: false,
            mouse: false,
        };
        crossterm::terminal::enable_raw_mode()
            .map_err(|error| terminal_setup_error("could not enter raw mode", error))?;
        session.raw = true;

        if fullscreen {
            crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen).map_err(
                |error| terminal_setup_error("could not enter the alternate screen", error),
            )?;
            session.alternate = true;
        }

        crossterm::execute!(io::stdout(), EnableMouseCapture)
            .map_err(|error| terminal_setup_error("could not enable mouse capture", error))?;
        session.mouse = true;
        Ok(session)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.mouse {
            let _ = crossterm::execute!(io::stdout(), DisableMouseCapture);
        }
        if self.alternate {
            let _ = crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        }
        if self.raw {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

fn terminal_setup_error(what: &str, error: io::Error) -> AikitError {
    AikitError::new("tui.terminal_setup_failed", format!("{what}: {error}"))
}

fn run_inner<B: SurfaceBackend>(
    backend: &mut B,
    request: SurfaceRequest,
    host: UiHost,
) -> Result<PaletteOutcome> {
    let terminal_backend = CrosstermBackend::new(io::stdout());
    let options = TerminalOptions {
        viewport: match host {
            UiHost::Inline(rows) => Viewport::Inline(rows),
            UiHost::TmuxPopup | UiHost::Fullscreen => Viewport::Fullscreen,
        },
    };
    let mut terminal = Terminal::with_options(terminal_backend, options)
        .map_err(|error| AikitError::new("tui.terminal_setup_failed", format!("{error}")))?;
    let mut events = CrosstermEvents::default();
    let outcome = event_loop(&mut terminal, &mut events, backend, request);
    let _ = terminal.clear();
    let _ = terminal.show_cursor();
    outcome
}

fn draw_error(what: &str, error: impl std::fmt::Display) -> AikitError {
    AikitError::new("tui.render_failed", format!("{what}: {error}"))
}
