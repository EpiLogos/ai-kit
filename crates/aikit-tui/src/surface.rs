//! One transient AIKit surface with Quick and Workspace presentations.
//!
//! During the V2 migration the existing PaletteController and TreeController are
//! retained as presentation adapters. Semantic presentation, selection and staged
//! mutation intent live in [`TuiState`]; the V1 controller state is projected from
//! that authority only where a legacy renderer/effect path still requires it.

use std::collections::{BTreeMap, BTreeSet};
use std::io;

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEventKind, KeyModifiers, MouseButton,
    MouseEventKind,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};

use aikit_core::error::AikitError;
use aikit_core::id::CapsuleId;
use aikit_core::resource::ResourceRef;
use aikit_core::Result;

use crate::app::{Action, Mode};
use crate::backend::{PaletteBackend, Toggle};
use crate::driver::{PaletteController, PaletteStep};
use crate::event::{CrosstermEvents, EventSource, PaletteEvent};
use crate::host::UiHost;
use crate::layout::Layout;
use crate::staging::is_on;
use crate::tree::{NodeKind, TreeEffect, TreeState};
use crate::tree_driver::{TreeController, TreeRequest, TreeStep};
use crate::tui_state::{reduce_tui, Presentation, TuiState, UiAction, UiEffect};
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
        let state = TuiState::new(request.initial_mode.into());
        let mut surface = Self {
            state,
            host: request.host,
            palette,
            tree,
        };
        surface.reduce_semantic(UiAction::SetQuery(surface.palette.state().query.clone()));
        surface.capture_palette_staging();
        match surface.state.presentation {
            Presentation::Quick => surface.capture_palette_selection(),
            Presentation::Workspace => surface.capture_tree_selection(),
        }
        surface.project_staging_to_tree();
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

        if explicit_exit(&event) {
            let effects = self.reduce_semantic(UiAction::RequestExit);
            if effects.contains(&UiEffect::Exit) {
                return Ok(SurfaceStep::Outcome(PaletteOutcome::Closed));
            }
        }

        match self.state.presentation {
            Presentation::Quick => {
                // Mouse navigation resolves the exact same stable selection
                // action as keyboard navigation. The legacy palette never sees a
                // mouse-specific semantic branch; its cursor is only a projection.
                if let Some(resource) = self.quick_mouse_selection(&event) {
                    self.set_selected(Some(resource));
                    self.align_palette_to_selected();
                    return Ok(SurfaceStep::Continue);
                }

                // Search-resting Esc/Ctrl-C is semantic Back. It is deliberately
                // not forwarded to the V1 reducer, whose historical behaviour
                // cleared query, then staged changes, then exited on successive
                // presses. In real submodes Esc still goes to the local V1
                // dismiss/back path while those modes are migrated.
                if semantic_back(&event) && self.palette.state().mode == Mode::Search {
                    self.reduce_semantic(UiAction::Back);
                    return Ok(SurfaceStep::Continue);
                }

                if semantic_back(&event) {
                    self.reduce_semantic(UiAction::Back);
                }
                let step = self.palette.handle(backend, event)?;
                self.capture_palette_state();
                self.project_staging_to_tree();
                self.handle_palette_step(backend, step)
            }
            Presentation::Workspace => {
                let step = self.tree.handle(event)?;
                self.capture_tree_selection();
                self.capture_tree_staging();
                self.project_staging_to_palette(backend);
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
                self.set_presentation(Presentation::Workspace);
                self.align_tree_to_selected();
                self.project_staging_to_tree();
                SurfaceStep::Continue
            }
            PaletteStep::Outcome(PaletteOutcome::Applied(generation)) => {
                self.reduce_semantic(UiAction::ApplyFinished);
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
            TreeStep::Continue => Ok(SurfaceStep::Continue),
            TreeStep::Palette => {
                self.set_presentation(Presentation::Quick);
                self.align_palette_to_selected();
                Ok(SurfaceStep::Continue)
            }
            TreeStep::Apply(_) => {
                self.set_presentation(Presentation::Quick);
                self.align_palette_to_selected();
                let step = self.palette.dispatch(backend, Action::CtrlEnter);
                self.capture_palette_state();
                self.project_staging_to_tree();
                self.handle_palette_step(backend, step)
            }
            TreeStep::Effect(TreeEffect::Activate { capsule }) => {
                self.set_selected(resource_ref_for_capsule(&capsule));
                self.set_presentation(Presentation::Quick);
                self.align_palette_to_selected();
                let step = self.palette.activate(backend, capsule);
                self.capture_palette_state();
                self.project_staging_to_tree();
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

    /// Import the Quick adapter's V1 staged projection into the canonical state.
    /// This is a migration edge, not controller-to-controller synchronisation.
    fn capture_palette_staging(&mut self) {
        let observed = self
            .palette
            .state()
            .staged
            .toggles()
            .into_iter()
            .filter_map(|toggle| {
                resource_ref_for_capsule(&toggle.capsule).map(|resource| (resource, toggle.enable))
            })
            .collect();
        self.reconcile_staging(observed);
    }

    /// Import the Workspace adapter's ID-only staged projection. Direction is
    /// inherited from canonical staged intent where it exists; a newly staged V1
    /// capability uses the same resolved-view inverse that the palette uses.
    fn capture_tree_staging(&mut self) {
        let observed = self
            .tree
            .state()
            .staged
            .iter()
            .filter_map(|capsule| {
                let resource = resource_ref_for_capsule(capsule)?;
                let enable = self
                    .state
                    .staged
                    .get(&resource)
                    .copied()
                    .unwrap_or_else(|| !is_on(&self.palette.state().view, capsule));
                Some((resource, enable))
            })
            .collect();
        self.reconcile_staging(observed);
    }

    fn reconcile_staging(&mut self, observed: BTreeMap<ResourceRef, bool>) {
        let removed: Vec<_> = self
            .state
            .staged
            .keys()
            .filter(|resource| !observed.contains_key(*resource))
            .cloned()
            .collect();
        for resource in removed {
            self.reduce_semantic(UiAction::Unstage(resource));
        }
        for (resource, enable) in observed {
            if self.state.staged.get(&resource).copied() != Some(enable) {
                self.reduce_semantic(UiAction::Stage { resource, enable });
            }
        }
    }

    fn project_staging_to_tree(&mut self) {
        self.tree.state_mut().staged = self
            .state
            .staged
            .keys()
            .filter_map(|resource| CapsuleId::parse(resource.as_str()).ok())
            .collect::<BTreeSet<_>>();
    }

    fn project_staging_to_palette<B: SurfaceBackend>(&mut self, backend: &mut B) {
        let desired: Vec<_> = self
            .state
            .staged
            .iter()
            .filter_map(|(resource, enable)| {
                CapsuleId::parse(resource.as_str())
                    .ok()
                    .map(|capsule| Toggle::new(capsule, *enable))
            })
            .collect();
        let current = self.palette.state().staged.toggles();
        if current != desired {
            self.palette.replace_staged(backend, desired);
        }
    }

    fn refresh_after_change<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        message: String,
    ) -> Result<()> {
        let scope = self.palette.state().scope.current();
        self.palette = PaletteController::new(
            backend,
            PaletteRequest::new(self.host)
                .with_query(self.state.query.clone())
                .with_scope(scope),
        )?;
        self.palette.state_mut().status = Some(crate::app::Status::info(message));
        self.project_staging_to_palette(backend);
        self.align_palette_to_selected();
        self.capture_palette_query();
        self.replace_tree(backend.surface_tree()?);
        self.project_staging_to_tree();
        Ok(())
    }

    fn replace_tree(&mut self, mut state: TreeState) {
        let old = self.tree.state();
        let old_selected_path = old.selected_row().map(|row| row.path);
        state.expanded = old.expanded.clone();
        state.filter = old.filter.clone();
        state.yanked = old.yanked.clone();
        state.staged = self
            .state
            .staged
            .keys()
            .filter_map(|resource| CapsuleId::parse(resource.as_str()).ok())
            .collect();
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

    fn reduce_semantic(&mut self, action: UiAction) -> Vec<UiEffect> {
        let reduction = reduce_tui(self.state.clone(), action);
        self.state = reduction.state;
        reduction.effects
    }

    fn capture_palette_state(&mut self) {
        self.capture_palette_query();
        self.capture_palette_selection();
        self.capture_palette_staging();
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
        let Some(index) = rows
            .iter()
            .position(|row| resource_ref_for_tree_kind(&row.node.kind).as_ref() == Some(selected))
        else {
            return false;
        };
        self.tree.state_mut().selected = index;
        true
    }

    fn quick_mouse_selection(&self, event: &PaletteEvent) -> Option<ResourceRef> {
        if self.palette.state().mode != Mode::Search || self.palette.state().in_manage_lane() {
            return None;
        }
        let PaletteEvent::Mouse(mouse) = event else {
            return None;
        };

        let index = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.quick_row_at(mouse.column, mouse.row)?,
            MouseEventKind::ScrollDown => self
                .palette
                .state()
                .cursor
                .saturating_add(1)
                .min(self.palette.state().rows.len().saturating_sub(1)),
            MouseEventKind::ScrollUp => self.palette.state().cursor.saturating_sub(1),
            _ => return None,
        };
        self.palette
            .state()
            .rows
            .get(index)
            .and_then(|row| resource_ref_for_capsule(&row.doc.id))
    }

    fn quick_row_at(&self, column: u16, row: u16) -> Option<usize> {
        let (cols, rows) = self.state.area;
        if cols < 2 || rows < 2 {
            return None;
        }
        let inner = Rect::new(1, 1, cols.saturating_sub(2), rows.saturating_sub(2));
        let panes = Layout::for_width(inner.width).split(inner);
        let list = panes.list;
        if column < list.x
            || column >= list.x.saturating_add(list.width)
            || row < list.y
            || row >= list.y.saturating_add(list.height)
        {
            return None;
        }

        let height = list.height as usize;
        let first = self
            .palette
            .state()
            .cursor
            .saturating_sub(height.saturating_sub(1));
        let index = first.saturating_add((row - list.y) as usize);
        (index < self.palette.state().rows.len()).then_some(index)
    }
}

fn resource_ref_for_capsule(capsule: &CapsuleId) -> Option<ResourceRef> {
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

fn semantic_back(event: &PaletteEvent) -> bool {
    matches!(
        event,
        PaletteEvent::Key(key)
            if key.kind != KeyEventKind::Release
                && (key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)))
    )
}

fn explicit_exit(event: &PaletteEvent) -> bool {
    matches!(
        event,
        PaletteEvent::Key(key)
            if key.kind != KeyEventKind::Release
                && key.code == KeyCode::Char('q')
                && key.modifiers.contains(KeyModifiers::CONTROL)
    )
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
