//! One transient AIKit surface with palette and tree compatibility presentations.
//!
//! V2 makes [`TuiState`] the semantic owner. Palette and tree remain resident
//! presentation/controllers while they are migrated, but they no longer copy
//! semantic staging directly between one another or treat a row index as identity.

use std::collections::BTreeMap;
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
use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::Result;

use crate::app::{Action, Mode, Status};
use crate::application::{
    keyboard_select, mouse_select, reduce_tui, ActivationIntent, PresentationMode, RelationView,
    ResourceListItem, ResourceListReadModel, TuiState, UiAction,
};
use crate::backend::{PaletteBackend, Toggle};
use crate::driver::{PaletteController, PaletteStep};
use crate::event::{CrosstermEvents, EventSource, PaletteEvent};
use crate::host::UiHost;
use crate::layout::Layout;
use crate::staging::is_on;
use crate::tree::{Node, NodeKind, TreeEffect, TreeState};
use crate::tree_driver::{TreeController, TreeRequest, TreeStep};
use crate::{PaletteOutcome, PaletteRequest};

/// The application operations the unified surface needs beyond the palette.
pub trait SurfaceBackend: PaletteBackend {
    fn surface_tree(&self) -> Result<TreeState>;
    fn apply_tree_effect(&mut self, effect: TreeEffect) -> Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceMode {
    Palette,
    Tree,
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
    mode: SurfaceMode,
    host: UiHost,
    semantic: TuiState,
    palette: PaletteController,
    tree: TreeController,
}

impl SurfaceController {
    pub fn new<B: SurfaceBackend>(backend: &mut B, request: SurfaceRequest) -> Result<Self> {
        let palette = PaletteController::new(backend, request.palette_request())?;
        let tree_state = backend.surface_tree()?;
        let semantic = TuiState {
            query: palette.state().query.clone(),
            read_model: compatibility_read_model(backend, &tree_state),
            mutation_scope: Some(palette.state().scope.current()),
            presentation: presentation_for_host(request.host),
            relation_view: relation_view_for_mode(request.initial_mode),
            ..TuiState::default()
        };
        let tree = TreeController::new(tree_state, TreeRequest::new(request.host));
        let mut surface = Self {
            mode: request.initial_mode,
            host: request.host,
            semantic,
            palette,
            tree,
        };
        match surface.mode {
            SurfaceMode::Palette => surface.capture_palette_semantics(),
            SurfaceMode::Tree => surface.capture_tree_semantics(),
        }
        surface.project_semantic_to_presentations(backend);
        Ok(surface)
    }

    pub fn mode(&self) -> SurfaceMode {
        self.mode
    }

    /// The one semantic state shared by every presentation.
    pub fn semantic(&self) -> &TuiState {
        &self.semantic
    }

    pub fn palette(&self) -> &PaletteController {
        &self.palette
    }

    pub fn tree(&self) -> &TreeController {
        &self.tree
    }

    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        self.apply_semantic(UiAction::Resize(area.width, area.height));
        match self.mode {
            SurfaceMode::Palette => self.palette.draw(frame),
            SurfaceMode::Tree => self.tree.draw(frame),
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
        if let Some(step) = self.handle_explicit_navigation(backend, &event)? {
            return Ok(step);
        }

        match self.mode {
            SurfaceMode::Palette => {
                if let Some(action) = self.quick_mouse_action(&event) {
                    self.apply_semantic(action);
                    self.project_semantic_to_palette(backend);
                    return Ok(SurfaceStep::Continue);
                }
                let step = self.palette.handle(backend, event)?;
                self.capture_palette_semantics();
                self.handle_palette_step(backend, step)
            }
            SurfaceMode::Tree => {
                let step = self.tree.handle(event)?;
                self.capture_tree_semantics();
                self.handle_tree_step(backend, step)
            }
        }
    }

    /// V2 owns the ambiguous navigation keys before either compatibility
    /// presentation sees them. Esc is therefore only back/dismiss at the resting
    /// surface, while clearing and exiting are separate, explicit commands.
    fn handle_explicit_navigation<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        event: &PaletteEvent,
    ) -> Result<Option<SurfaceStep>> {
        let PaletteEvent::Key(key) = event else {
            return Ok(None);
        };
        if key.kind == KeyEventKind::Release {
            return Ok(None);
        }

        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q')) {
            self.apply_semantic(UiAction::Exit);
            if self.semantic.exit_requested {
                return Ok(Some(SurfaceStep::Outcome(PaletteOutcome::Closed)));
            }
            if let Some(status) = self.semantic.status.as_ref() {
                self.palette.state_mut().status = Some(Status::info(status.message.clone()));
            }
            return Ok(Some(SurfaceStep::Continue));
        }

        if control && key.code == KeyCode::Char('u') {
            self.clear_query_explicitly(backend);
            return Ok(Some(SurfaceStep::Continue));
        }

        if key.code == KeyCode::Esc {
            // Dialog/form/preview Esc already means one-layer back in the proven V1
            // reducer. Only the resting Search state and Tree need interception to
            // prevent the old context-dependent clear/discard/close semantics.
            if self.mode == SurfaceMode::Palette && self.palette.state().mode != Mode::Search {
                return Ok(None);
            }
            self.apply_semantic(UiAction::Back);
            self.project_semantic_to_presentations(backend);
            return Ok(Some(SurfaceStep::Continue));
        }

        Ok(None)
    }

    fn clear_query_explicitly<B: SurfaceBackend>(&mut self, backend: &mut B) {
        self.apply_semantic(UiAction::SetQuery(String::new()));

        // The V1 palette has no explicit ClearQuery action. Drive its proven
        // Backspace action until the compatibility presentation matches the one
        // semantic query; each step remains inside its reducer/effect discipline.
        while !self.palette.state().query.is_empty() {
            let _ = self.palette.dispatch(backend, Action::Backspace);
        }
        self.tree.state_mut().filter.clear();
        self.capture_palette_semantics();
        self.project_semantic_to_tree();
    }

    fn handle_palette_step<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        step: PaletteStep,
    ) -> Result<SurfaceStep> {
        Ok(match step {
            PaletteStep::Continue => {
                self.project_semantic_to_tree();
                SurfaceStep::Continue
            }
            PaletteStep::Tree => {
                self.mode = SurfaceMode::Tree;
                self.apply_semantic(UiAction::SetRelationView(RelationView::Tree));
                self.project_semantic_to_tree();
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
                self.project_semantic_to_palette(backend);
                Ok(SurfaceStep::Continue)
            }
            TreeStep::Palette => {
                self.mode = SurfaceMode::Palette;
                self.apply_semantic(UiAction::SetRelationView(RelationView::List));
                self.project_semantic_to_palette(backend);
                Ok(SurfaceStep::Continue)
            }
            TreeStep::Apply(_) => {
                self.mode = SurfaceMode::Palette;
                self.apply_semantic(UiAction::SetRelationView(RelationView::List));
                self.project_semantic_to_palette(backend);
                let step = self.palette.dispatch(backend, Action::CtrlEnter);
                self.capture_palette_semantics();
                self.handle_palette_step(backend, step)
            }
            TreeStep::Effect(TreeEffect::Activate { capsule }) => {
                self.mode = SurfaceMode::Palette;
                if let Some(resource) = capsule_resource_ref(&capsule) {
                    self.apply_semantic(UiAction::Select(resource));
                }
                self.apply_semantic(UiAction::SetRelationView(RelationView::List));
                self.project_semantic_to_palette(backend);
                let step = self.palette.activate(backend, capsule);
                self.capture_palette_semantics();
                self.handle_palette_step(backend, step)
            }
            TreeStep::Effect(effect) => {
                if let Err(error) = backend.apply_tree_effect(effect) {
                    self.tree.report_error(&error);
                    return Ok(SurfaceStep::Continue);
                }
                match backend.surface_tree() {
                    Ok(tree) => self.replace_tree(tree, backend),
                    Err(error) => self.tree.report_error(&error),
                }
                Ok(SurfaceStep::Continue)
            }
        }
    }

    /// Ingest the compatibility palette after a semantic gesture. This is the
    /// migration boundary: the palette may still own a row cursor, but its selected
    /// capsule and staged toggles are immediately converted to the authoritative
    /// ResourceRef state.
    fn capture_palette_semantics(&mut self) {
        self.semantic.query = self.palette.state().query.clone();
        self.apply_semantic(UiAction::SetMutationScope(self.palette.state().scope.current()));
        if let Some(resource) = self
            .palette
            .state()
            .selected_row()
            .and_then(|row| capsule_resource_ref(&row.doc.id))
        {
            self.apply_semantic(keyboard_select(resource));
        }

        let observed: BTreeMap<ResourceRef, ActivationIntent> = self
            .palette
            .state()
            .staged
            .toggles()
            .into_iter()
            .filter_map(|toggle| {
                capsule_resource_ref(&toggle.capsule).map(|resource| {
                    let intent = if toggle.enable {
                        ActivationIntent::Enable
                    } else {
                        ActivationIntent::Disable
                    };
                    (resource, intent)
                })
            })
            .collect();
        self.reconcile_compatibility_staging(observed);
    }

    fn capture_tree_semantics(&mut self) {
        if let Some(resource) = self
            .tree
            .state()
            .selected_row()
            .and_then(|row| node_resource_ref(&row.node))
        {
            self.apply_semantic(UiAction::Select(resource));
        }

        let observed: BTreeMap<ResourceRef, ActivationIntent> = self
            .tree
            .state()
            .staged
            .iter()
            .filter_map(|capsule| {
                let resource = capsule_resource_ref(capsule)?;
                let intent = self.semantic.staged.get(&resource).unwrap_or_else(|| {
                    if is_on(&self.palette.state().view, capsule) {
                        ActivationIntent::Disable
                    } else {
                        ActivationIntent::Enable
                    }
                });
                Some((resource, intent))
            })
            .collect();
        self.reconcile_compatibility_staging(observed);
    }

    /// Replace only the capsule-shaped compatibility subset. V2 Resources staged
    /// by newer surfaces survive even when the V1 palette/tree cannot render them.
    fn reconcile_compatibility_staging(
        &mut self,
        observed: BTreeMap<ResourceRef, ActivationIntent>,
    ) {
        let compatibility_refs: Vec<_> = self
            .semantic
            .staged
            .resources()
            .filter(|resource| resource_capsule_id(resource).is_some())
            .cloned()
            .collect();
        for resource in compatibility_refs {
            if !observed.contains_key(&resource) {
                self.semantic.staged.unstage(&resource);
            }
        }
        for (resource, intent) in observed {
            self.semantic.staged.stage(resource, intent);
        }
        self.semantic.preview = None;
    }

    fn project_semantic_to_presentations<B: SurfaceBackend>(&mut self, backend: &mut B) {
        self.project_semantic_to_tree();
        self.project_semantic_to_palette(backend);
    }

    fn project_semantic_to_tree(&mut self) {
        self.tree.state_mut().staged = self
            .semantic
            .staged
            .resources()
            .filter_map(resource_capsule_id)
            .collect();
        if let Some(selected) = &self.semantic.selected {
            if let Some(index) = self.tree.state().rows().iter().position(|row| {
                node_resource_ref(&row.node).as_ref() == Some(selected)
            }) {
                self.tree.state_mut().selected = index;
            }
        }
    }

    fn project_semantic_to_palette<B: SurfaceBackend>(&mut self, backend: &mut B) {
        let toggles = self
            .semantic
            .staged
            .resources()
            .filter_map(|resource| {
                let capsule = resource_capsule_id(resource)?;
                let enable = self.semantic.staged.get(resource)? == ActivationIntent::Enable;
                Some(Toggle::new(capsule, enable))
            })
            .collect();
        self.palette.replace_staged(backend, toggles);

        if let Some(selected) = &self.semantic.selected {
            if let Some(index) = self.palette.state().rows.iter().position(|row| {
                capsule_resource_ref(&row.doc.id).as_ref() == Some(selected)
            }) {
                self.palette.state_mut().cursor = index;
            }
        }
    }

    fn refresh_after_change<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        message: String,
    ) -> Result<()> {
        self.palette.refresh(backend)?;
        self.palette.state_mut().status = Some(Status::info(message));
        self.replace_tree(backend.surface_tree()?, backend);
        self.project_semantic_to_presentations(backend);
        Ok(())
    }

    fn replace_tree<B: SurfaceBackend>(&mut self, mut state: TreeState, backend: &mut B) {
        let old = self.tree.state();
        let old_selected_path = old.selected_row().map(|row| row.path);
        state.expanded = old.expanded.clone();
        state.filter = old.filter.clone();
        state.yanked = old.yanked.clone();

        self.tree = TreeController::new(state, TreeRequest::new(self.host));
        self.apply_semantic(UiAction::Refresh(compatibility_read_model(
            backend,
            self.tree.state(),
        )));

        // Resource selection is projected by stable ResourceRef. If the selected
        // row is a presentation-only group/root, preserve its stable path instead
        // of its old numeric row index.
        if self.semantic.selected.is_none() {
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
        self.project_semantic_to_presentations(backend);
    }

    fn apply_semantic(&mut self, action: UiAction) {
        let state = std::mem::take(&mut self.semantic);
        self.semantic = reduce_tui(state, action).state;
    }

    fn quick_mouse_action(&self, event: &PaletteEvent) -> Option<UiAction> {
        if self.mode != SurfaceMode::Palette
            || self.palette.state().mode != Mode::Search
            || self.palette.state().in_manage_lane()
        {
            return None;
        }
        let PaletteEvent::Mouse(mouse) = event else {
            return None;
        };

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let index = self.quick_row_at(mouse.column, mouse.row)?;
                let resource = self
                    .palette
                    .state()
                    .rows
                    .get(index)
                    .and_then(|row| capsule_resource_ref(&row.doc.id))?;
                Some(mouse_select(resource))
            }
            MouseEventKind::ScrollDown => Some(UiAction::SelectNext),
            MouseEventKind::ScrollUp => Some(UiAction::SelectPrevious),
            _ => None,
        }
    }

    fn quick_row_at(&self, column: u16, row: u16) -> Option<usize> {
        let (cols, rows) = self.semantic.area;
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

fn presentation_for_host(host: UiHost) -> PresentationMode {
    match host {
        UiHost::Inline(_) => PresentationMode::Quick,
        UiHost::TmuxPopup | UiHost::Fullscreen => PresentationMode::Workspace,
    }
}

fn relation_view_for_mode(mode: SurfaceMode) -> RelationView {
    match mode {
        SurfaceMode::Palette => RelationView::List,
        SurfaceMode::Tree => RelationView::Tree,
    }
}

fn capsule_resource_ref(capsule: &CapsuleId) -> Option<ResourceRef> {
    ResourceRef::parse(&capsule.to_string()).ok()
}

fn resource_capsule_id(resource: &ResourceRef) -> Option<CapsuleId> {
    CapsuleId::parse(resource.as_str()).ok()
}

fn node_resource_ref(node: &Node) -> Option<ResourceRef> {
    match &node.kind {
        NodeKind::Capability { id } | NodeKind::HookStep { capsule: id, .. } => {
            capsule_resource_ref(id)
        }
        NodeKind::Root(_) | NodeKind::Set { .. } | NodeKind::Group { .. } | NodeKind::Entry { .. } => {
            None
        }
    }
}

fn compatibility_read_model(
    backend: &dyn PaletteBackend,
    tree: &TreeState,
) -> ResourceListReadModel {
    let mut resources = BTreeMap::<ResourceRef, ResourceListItem>::new();
    for doc in backend.documents() {
        if let Some(resource) = capsule_resource_ref(&doc.id) {
            resources.insert(
                resource.clone(),
                ResourceListItem {
                    resource,
                    kind: ResourceKind::Capability,
                    label: doc.name,
                    summary: doc.description,
                },
            );
        }
    }
    for root in &tree.roots {
        collect_compatibility_resources(root, &mut resources);
    }
    ResourceListReadModel {
        revision: "aikit.tui/v1-compat-resources".into(),
        resources: resources.into_values().collect(),
    }
}

fn collect_compatibility_resources(
    node: &Node,
    resources: &mut BTreeMap<ResourceRef, ResourceListItem>,
) {
    if let Some(resource) = node_resource_ref(node) {
        resources.entry(resource.clone()).or_insert_with(|| ResourceListItem {
            resource,
            kind: ResourceKind::Capability,
            label: match &node.kind {
                NodeKind::Capability { id } => id.to_string(),
                NodeKind::HookStep { capsule, .. } => capsule.to_string(),
                _ => String::new(),
            },
            summary: node.summary.clone(),
        });
    }
    for child in &node.children {
        collect_compatibility_resources(child, resources);
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
