//! Final V2 terminal surface over one [`TuiState`] / [`TuiRuntime`] authority.
//!
//! Quick and Workspace are presentations of the same semantic state. List / Tree /
//! Graph are projections of one cached [`RelationReadModel`] returned by the same
//! application service. Renderers and mouse hit-testing dispatch semantic Actions;
//! they do not own resolver, selection, retrieval or mutation state.

use std::collections::BTreeMap;
use std::io;

use aikit_core::resource::ActionStageability;
use aikit_core::{
    AikitError, KnowledgeRelationView, ProjectWorldReadModel, ResourceRef, Result,
};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEventKind, KeyModifiers, MouseButton,
    MouseEventKind,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::application::{
    selected_contextual_action, visible_contextual_actions, Overlay, PresentationMode,
    RelationReadModel, RelationView, TuiApplicationService, TuiRuntime, TuiState, UiAction,
    WorkspaceSection,
};
use crate::application_service::ApplicationService;
use crate::backend::PaletteBackend;
use crate::event::{CrosstermEvents, EventSource, PaletteEvent};
use crate::graph_layout::{self, GraphLayout, GraphLayoutRequest, GraphViewport, RelationBand};
use crate::graph_presentation;
use crate::host::UiHost;
use crate::layout::{Glyphs, Layout, Width};
use crate::navigation::AmbientContext;
use crate::project_workspace_render::workspace_section_label;
use crate::project_world_api::ProjectWorldApplicationService;
use crate::theme::Theme;
use crate::v2_render;
use crate::PaletteOutcome;

/// What [`ApplicationSurfaceController::graph_layout`] was computed from. The
/// spatial/grouped Graph rendering must not recompute merely because the
/// cursor moved between already-laid-out nodes — only a genuine change to
/// the relation data, the graph-local filter, or the viewport recomputes.
/// Depth is deliberately not a separate field: a depth change always
/// produces a different `KnowledgeRelationView` (a different `query.depth`
/// at minimum), so comparing `view` already captures it.
#[derive(Debug, Clone, PartialEq)]
struct GraphLayoutCacheKey {
    view: KnowledgeRelationView,
    filter: String,
    viewport: (u16, u16),
}

#[derive(Debug, Clone)]
pub struct ApplicationSurfaceRequest {
    pub host: UiHost,
    pub initial_query: Option<String>,
    pub initial_relation_view: RelationView,
    pub initial_workspace_section: WorkspaceSection,
    /// Explicit host glyph-capability override, governing every mark this
    /// surface draws — the resting shell's chrome and the Graph's connectors
    /// alike. `None` (every real run) resolves the capability once at
    /// construction from [`crate::layout::Glyphs::from_env`] — see
    /// [`ApplicationSurfaceController::shell_glyphs`]. `Some` is the
    /// injection point a test uses to pin ASCII or Unicode deterministically
    /// instead of depending on the process locale, without mutating
    /// process-global environment variables (which `nextest`'s parallel test
    /// execution makes racy).
    glyphs: Option<Glyphs>,
}

impl ApplicationSurfaceRequest {
    pub fn new(host: UiHost) -> Self {
        Self {
            host,
            initial_query: None,
            initial_relation_view: RelationView::List,
            initial_workspace_section: WorkspaceSection::Worlds,
            glyphs: None,
        }
    }

    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.initial_query = Some(query.into());
        self
    }

    /// Pin the glyph capability this surface renders with — shell chrome and
    /// Graph connectors together — overriding host locale detection. For
    /// tests only; real callers leave this unset so `Glyphs::from_env()`
    /// governs. One knob rather than two: a frame drawn with an ASCII footer
    /// and Unicode graph connectors is exactly the mixed rendering
    /// `layout.rs`'s module header rules out.
    #[must_use]
    pub fn with_glyphs(mut self, glyphs: Glyphs) -> Self {
        self.glyphs = Some(glyphs);
        self
    }

    /// Open Knowledge with one projection of the canonical relation read model.
    #[must_use]
    pub fn opening_relations(mut self, view: RelationView) -> Self {
        self.initial_relation_view = view;
        self.initial_workspace_section = WorkspaceSection::Knowledge;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApplicationSurfaceStep {
    Continue,
    Outcome(PaletteOutcome),
}

pub struct ApplicationSurfaceController {
    semantic: TuiState,
    runtime: TuiRuntime,
    relation: Option<RelationReadModel>,
    project_world: Option<ProjectWorldReadModel>,
    ambient: AmbientContext,
    graph_layout: Option<(GraphLayoutCacheKey, GraphLayout)>,
    /// Host glyph capability for the resting shell — the footer's keycap
    /// hints, field separators, cursors and elision marks — resolved exactly
    /// once, here at construction alongside `ambient`, rather than sniffed
    /// live inside the render path. A rendered frame is then a pure function
    /// of `semantic`/`graph_layout` and this already-resolved capability,
    /// never of the live process environment. Real callers get
    /// [`crate::layout::Glyphs::from_env`]'s answer (`ApplicationSurfaceRequest`
    /// leaves `glyphs` unset); [`ApplicationSurfaceRequest::with_glyphs`] is
    /// the injection point a test uses to pin ASCII or Unicode
    /// deterministically instead.
    shell_glyphs: Glyphs,
    /// The Graph presentation's connector set, derived from `shell_glyphs`
    /// by [`graph_glyphs_for`] at that same single construction-time
    /// reading. Two glyph types because the Graph's connectors answer a
    /// question the shell's marks do not; one capability behind both,
    /// because a frame must not come out half ASCII.
    graph_glyphs: graph_layout::GraphGlyphs,
    /// Whether the Graph-local filter text lane (`/`) is currently open.
    /// Controller-only input-routing state, not `TuiState`: it decides which
    /// method the next keystroke reaches, exactly like `graph_layout`'s
    /// cache is controller-only computed state, never a semantic fact
    /// `reduce_tui` needs to reason about.
    graph_filter_editing: bool,
    /// Incremented only on the branch of [`Self::sync_graph_layout`] that
    /// actually recomputes `graph_layout` (never on a cache hit). Output
    /// stability alone cannot prove the "recompute only on genuine input
    /// change" contract documented on `GraphLayoutCacheKey`: identical
    /// `(view, filter, viewport)` necessarily produces an identical
    /// `GraphLayout` whether it was recomputed or served from cache, so a
    /// test asserting on rendered/laid-out output cannot distinguish the
    /// two. This counter is the direct, deterministic witness integration
    /// tests assert on instead of wall-clock timing.
    graph_layout_recomputes: u64,
}

impl ApplicationSurfaceController {
    pub fn new<B: PaletteBackend>(
        backend: &mut B,
        request: ApplicationSurfaceRequest,
    ) -> Result<Self> {
        let ambient = ambient_context(backend.context());
        // One reading of host capability, at the one boundary that is
        // allowed to look: everything drawn below is a function of it.
        let shell_glyphs = request.glyphs.unwrap_or_else(Glyphs::from_env);
        let graph_glyphs = graph_glyphs_for(shell_glyphs);
        let mut semantic = TuiState {
            presentation: if matches!(request.host, UiHost::Inline(_)) {
                PresentationMode::Quick
            } else {
                PresentationMode::Workspace
            },
            workspace_section: request.initial_workspace_section,
            relation_view: request.initial_relation_view,
            mutation_scope: Some(backend.context().default_mutation_scope()),
            ..TuiState::default()
        };
        let mut runtime = TuiRuntime::new();
        let project_world;
        {
            let mut service = ApplicationService::new(backend);
            semantic = runtime.step(
                &mut service,
                semantic,
                UiAction::SetQuery(request.initial_query.unwrap_or_default()),
            )?;
            project_world = service.project_world().ok();
        }
        let mut controller = Self {
            semantic,
            runtime,
            relation: None,
            project_world,
            ambient,
            graph_layout: None,
            shell_glyphs,
            graph_glyphs,
            graph_filter_editing: false,
            graph_layout_recomputes: 0,
        };
        controller.refresh_relation(backend)?;
        Ok(controller)
    }

    pub fn semantic(&self) -> &TuiState {
        &self.semantic
    }

    pub fn relation(&self) -> Option<&RelationReadModel> {
        self.relation.as_ref()
    }

    /// Test/debug witness for [`Self::sync_graph_layout`]'s caching
    /// contract: the number of times `graph_layout` has actually been
    /// recomputed (a cache hit never increments it). See the field's own
    /// doc comment for why this exists instead of an output-stability
    /// assertion.
    pub fn graph_layout_recompute_count(&self) -> u64 {
        self.graph_layout_recomputes
    }

    pub fn project_world(&self) -> Option<&ProjectWorldReadModel> {
        self.project_world.as_ref()
    }

    pub fn handle<B: PaletteBackend>(
        &mut self,
        backend: &mut B,
        event: PaletteEvent,
    ) -> Result<ApplicationSurfaceStep> {
        match event {
            PaletteEvent::Resize(cols, rows) => {
                self.dispatch(backend, UiAction::Resize(cols, rows))?;
            }
            PaletteEvent::Mouse(mouse)
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) =>
            {
                self.handle_mouse(backend, mouse.column, mouse.row, mouse.modifiers)?;
            }
            PaletteEvent::Mouse(_) | PaletteEvent::Idle => {}
            PaletteEvent::Key(key)
                if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
            {
                self.handle_key(backend, key.code, key.modifiers)?;
            }
            PaletteEvent::Key(_) => {}
        }

        if self.semantic.exit_requested {
            return Ok(ApplicationSurfaceStep::Outcome(PaletteOutcome::Closed));
        }
        Ok(ApplicationSurfaceStep::Continue)
    }

    pub fn draw(&self, frame: &mut ratatui::Frame) {
        if let Some(world) = &self.project_world {
            v2_render::draw_with_project_world(
                frame,
                &self.semantic,
                &self.ambient,
                world,
                self.shell_glyphs,
            );
        } else {
            v2_render::draw_with_context(
                frame,
                &self.semantic,
                &self.ambient,
                self.shell_glyphs,
            );
        }
        if self.semantic.presentation == PresentationMode::Workspace
            && self.semantic.workspace_section == WorkspaceSection::Knowledge
        {
            self.draw_relations(frame);
        }
    }

    pub fn draw_terminal<T: Backend>(&self, terminal: &mut Terminal<T>) -> Result<()>
    where
        T::Error: std::fmt::Display,
    {
        terminal
            .draw(|frame| self.draw(frame))
            .map(|_| ())
            .map_err(|error| AikitError::new("tui.draw_failed", format!("{error}")))
    }

    fn handle_key<B: PaletteBackend>(
        &mut self,
        backend: &mut B,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let alt = modifiers.contains(KeyModifiers::ALT);

        if self.graph_filter_editing {
            return self.handle_graph_filter_key(backend, code);
        }
        if self.semantic.action_query.is_some() {
            return self.handle_action_key(backend, code, ctrl);
        }
        if ctrl && matches!(code, KeyCode::Char('c') | KeyCode::Char('q')) {
            return self.dispatch(backend, UiAction::Exit);
        }
        if code == KeyCode::Esc {
            // Esc inside an active Graph projection first unwinds the Graph's
            // own recenter history (spec §8.3: "Esc / Back  previous graph
            // focus"); only once that history is empty does Esc fall through
            // to ordinary navigation Back, so Esc is never swallowed.
            if self.graph_projection_active() && !self.semantic.graph.history.is_empty() {
                return self.dispatch(backend, UiAction::GraphBack);
            }
            return self.dispatch(backend, UiAction::Back);
        }
        if ctrl && code == KeyCode::Char('u') {
            return self.dispatch(backend, UiAction::SetQuery(String::new()));
        }
        if ctrl && code == KeyCode::Char('w') {
            return self.toggle_presentation(backend);
        }
        // Ctrl+K is the Universal Navigator: an explicit "go to Quick" alias,
        // consistent with Ctrl+T already being an explicit view-rotation alias
        // rather than a toggle. Quick is already the Navigator presentation
        // (search over the one shared ResourceSearchIndex, "things" and
        // "places" alike — see `crate::workspace_navigation`); this binding
        // adds no new state, it only jumps to it from wherever the operator
        // currently is, preserving query/selection/staged/mutation_scope.
        if ctrl && matches!(code, KeyCode::Char('k') | KeyCode::Char('K')) {
            return self.dispatch(backend, UiAction::SetPresentation(PresentationMode::Quick));
        }
        if alt && code == KeyCode::Left {
            return self.dispatch(backend, UiAction::PreviousWorkspaceSection);
        }
        if alt && code == KeyCode::Right {
            return self.dispatch(backend, UiAction::NextWorkspaceSection);
        }
        if ctrl && code == KeyCode::Char('t') {
            let view = match self.semantic.relation_view {
                RelationView::List => RelationView::Tree,
                RelationView::Tree => RelationView::Graph,
                RelationView::Graph => RelationView::List,
            };
            self.dispatch(backend, UiAction::SetRelationView(view))?;
            self.dispatch(backend, UiAction::SetPresentation(PresentationMode::Workspace))?;
            return self.dispatch(
                backend,
                UiAction::SetWorkspaceSection(WorkspaceSection::Knowledge),
            );
        }
        if ctrl && code == KeyCode::Char('s') {
            let action = match self.semantic.overlay {
                Some(Overlay::ConfirmApply) => UiAction::ConfirmApply,
                Some(Overlay::CompositionPreview) => UiAction::RequestApply,
                _ => UiAction::RequestCompositionPreview,
            };
            return self.dispatch(backend, action);
        }
        if code == KeyCode::Insert || (ctrl && code == KeyCode::Char(' ')) {
            return self.stage_selected(backend);
        }
        // Graph claims its own key vocabulary (arrows/hjkl, Enter, +/-, /)
        // for as long as it is the projection actually on screen; see
        // `handle_graph_key`'s doc comment for the precedence this chooses
        // and why.
        if self.graph_projection_active() {
            return self.handle_graph_key(backend, code);
        }
        match code {
            KeyCode::Up => self.dispatch(backend, UiAction::SelectPrevious),
            KeyCode::Down => self.dispatch(backend, UiAction::SelectNext),
            KeyCode::Char(':') => self.dispatch(backend, UiAction::BeginActionSearch),
            KeyCode::Enter => self.open_selected_action(backend),
            KeyCode::Backspace if !self.semantic.query.is_empty() => {
                let mut query = self.semantic.query.clone();
                query.pop();
                self.dispatch(backend, UiAction::SetQuery(query))
            }
            KeyCode::Char(character) if !ctrl && !alt => {
                let mut query = self.semantic.query.clone();
                query.push(character);
                self.dispatch(backend, UiAction::SetQuery(query))
            }
            _ => Ok(()),
        }
    }

    /// Key handling while the Graph projection is the thing actually on
    /// screen (`graph_projection_active`).
    ///
    /// Precedence: this is reached only after `handle_key` has already
    /// claimed Esc/Back (Graph-aware), the whole-application Ctrl/Alt
    /// combos (Exit, clear query, toggle presentation, workspace-section
    /// paging, Ctrl+T view rotation, Ctrl+S preview/apply, stage/unstage) —
    /// every one of those keeps working identically whether or not Graph is
    /// open. `:` for contextual Actions is ordinarily handled by the
    /// generic match arm further down in `handle_key`, but this method
    /// claims every key while Graph is active and that generic arm is never
    /// reached — so `:` is *re-declared* here (see the `Char(':')` arm
    /// below) rather than the whole-application binding being routed
    /// through.
    ///
    /// Everything this method does not explicitly bind is deliberately
    /// inert. Graph is a spatial browse mode, not a text-entry surface: the
    /// ordinary "append this character to the resource search query"
    /// typing that every other section supports is suspended for as long as
    /// Graph is the active projection (switching away with Ctrl+T restores
    /// it immediately), because a stray keystroke silently mutating a query
    /// the viewer cannot even see — the list pane is showing the graph, not
    /// search results — would be worse than that keystroke doing nothing.
    fn handle_graph_key<B: PaletteBackend>(&mut self, backend: &mut B, code: KeyCode) -> Result<()> {
        let direction = match code {
            KeyCode::Up | KeyCode::Char('k') => Some((0, -1)),
            KeyCode::Down | KeyCode::Char('j') => Some((0, 1)),
            KeyCode::Left | KeyCode::Char('h') => Some((-1, 0)),
            KeyCode::Right | KeyCode::Char('l') => Some((1, 0)),
            _ => None,
        };
        if let Some(direction) = direction {
            if let Some((_, layout)) = self.graph_layout.as_ref() {
                if let Some(next) = graph_presentation::move_selection(
                    layout,
                    self.semantic.selected.as_ref(),
                    direction,
                ) {
                    return self.dispatch(backend, UiAction::GraphSelectNode(next));
                }
            }
            return Ok(());
        }
        match code {
            KeyCode::Enter => {
                if let Some(resource) = self.semantic.selected.clone() {
                    return self.dispatch(backend, UiAction::GraphRecenter(resource));
                }
                Ok(())
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.dispatch(backend, UiAction::GraphIncreaseDepth)
            }
            KeyCode::Char('-') => self.dispatch(backend, UiAction::GraphDecreaseDepth),
            KeyCode::Char('/') => {
                self.graph_filter_editing = true;
                Ok(())
            }
            KeyCode::Char(':') => self.dispatch(backend, UiAction::BeginActionSearch),
            _ => Ok(()),
        }
    }

    /// Text entry for the graph-local filter (`/`), opened by
    /// `handle_graph_key`. Mirrors `handle_action_key`'s shape: every
    /// keystroke commits immediately (live filtering, not filter-then-
    /// confirm), and Enter/Esc both simply close the lane — Esc does not
    /// need Graph's own Back semantics here because there is nothing to
    /// navigate away from, only a text field to stop editing.
    fn handle_graph_filter_key<B: PaletteBackend>(
        &mut self,
        backend: &mut B,
        code: KeyCode,
    ) -> Result<()> {
        match code {
            KeyCode::Esc | KeyCode::Enter => {
                self.graph_filter_editing = false;
                Ok(())
            }
            KeyCode::Backspace => {
                let mut filter = self.semantic.graph.filter.clone();
                filter.pop();
                self.dispatch(backend, UiAction::GraphSetFilter(filter))
            }
            KeyCode::Char(character) => {
                let mut filter = self.semantic.graph.filter.clone();
                filter.push(character);
                self.dispatch(backend, UiAction::GraphSetFilter(filter))
            }
            _ => Ok(()),
        }
    }

    fn handle_action_key<B: PaletteBackend>(
        &mut self,
        backend: &mut B,
        code: KeyCode,
        ctrl: bool,
    ) -> Result<()> {
        match code {
            KeyCode::Esc => self.dispatch(backend, UiAction::Back),
            KeyCode::Up => self.dispatch(backend, UiAction::SelectPreviousAction),
            KeyCode::Down => self.dispatch(backend, UiAction::SelectNextAction),
            KeyCode::Enter => self.invoke_selected_action(backend, false),
            KeyCode::Char(' ') if !ctrl => self.invoke_selected_action(backend, true),
            KeyCode::Backspace => {
                let mut query = self.semantic.action_query.clone().unwrap_or_default();
                query.pop();
                self.dispatch(backend, UiAction::SetActionQuery(query))
            }
            KeyCode::Char(character) if !ctrl => {
                let mut query = self.semantic.action_query.clone().unwrap_or_default();
                query.push(character);
                self.dispatch(backend, UiAction::SetActionQuery(query))
            }
            _ => Ok(()),
        }
    }

    fn handle_mouse<B: PaletteBackend>(
        &mut self,
        backend: &mut B,
        column: u16,
        row: u16,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        let (cols, rows) = self.semantic.area;
        let cols = cols.max(2);
        let rows = rows.max(2);

        // The title itself is the compact/expanded affordance. It changes only
        // PresentationMode on the same TuiState.
        if row == 0 {
            return self.toggle_presentation(backend);
        }

        let inner = Rect::new(1, 1, cols.saturating_sub(2), rows.saturating_sub(2));
        let panes = Layout::for_width(inner.width).split(inner);

        if self.semantic.presentation == PresentationMode::Workspace && row == panes.query.y {
            if let Some(section) = workspace_tab_hit(&self.semantic, panes.query.x, column) {
                return self.dispatch(backend, UiAction::SetWorkspaceSection(section));
            }
        }

        // While the Graph projection is actually on screen, `panes.list`
        // shows the relations overlay (see `draw_relations`), not the
        // resource list — a click there must hit-test the graph canvas
        // (border-adjusted, matching `graph_content_rect`), never fall
        // through to interpreting the row as a list index.
        if self.graph_projection_active() {
            let content = self.graph_content_rect();
            let viewport = self.graph_viewport();
            if column >= content.x
                && column < content.x.saturating_add(viewport.width)
                && row >= content.y
                && row < content.y.saturating_add(viewport.height)
            {
                if let Some((_, layout)) = self.graph_layout.as_ref() {
                    let local_x = i32::from(column) - i32::from(content.x);
                    let local_y = i32::from(row) - i32::from(content.y);
                    if let Some(resource) =
                        graph_presentation::node_at(layout, local_x, local_y).cloned()
                    {
                        // A modifier-click recentres, matching Enter's semantic
                        // action; a plain click only highlights, matching a
                        // plain arrow/hjkl move — keyboard and mouse always
                        // resolve to the same two UiActions. Shift is chosen
                        // over Ctrl because Ctrl is already this surface's
                        // reserved modifier for whole-application shortcuts
                        // (Ctrl+T/S/U/W, Ctrl+arrow); Shift+click is free.
                        let action = if modifiers.contains(KeyModifiers::SHIFT) {
                            UiAction::GraphRecenter(resource)
                        } else {
                            UiAction::GraphSelectNode(resource)
                        };
                        return self.dispatch(backend, action);
                    }
                }
            }
            return Ok(());
        }

        if let Some(preview) = panes.preview {
            if column >= preview.x
                && column < preview.x.saturating_add(preview.width)
                && row >= preview.y.saturating_add(8)
                && row < preview.y.saturating_add(preview.height)
            {
                let action_index = usize::from(row.saturating_sub(preview.y).saturating_sub(8));
                let actions = visible_contextual_actions(&self.semantic);
                if let Some(action) = actions.get(action_index) {
                    return self.dispatch(backend, UiAction::InvokeAction(action.action.clone()));
                }
            }
        }

        if column >= panes.list.x
            && column < panes.list.x.saturating_add(panes.list.width)
            && row >= panes.list.y
            && row < panes.list.y.saturating_add(panes.list.height)
        {
            let index = usize::from(row.saturating_sub(panes.list.y));
            if let Some(item) = self.semantic.read_model.resources.get(index) {
                return self.dispatch(backend, UiAction::Select(item.resource.clone()));
            }
        }
        Ok(())
    }

    fn toggle_presentation<B: PaletteBackend>(&mut self, backend: &mut B) -> Result<()> {
        let presentation = match self.semantic.presentation {
            PresentationMode::Quick => PresentationMode::Workspace,
            PresentationMode::Workspace => PresentationMode::Quick,
        };
        self.dispatch(backend, UiAction::SetPresentation(presentation))
    }

    fn stage_selected<B: PaletteBackend>(&mut self, backend: &mut B) -> Result<()> {
        let Some(resource) = self.semantic.selected.clone() else {
            return Ok(());
        };
        if self.semantic.staged.get(&resource).is_some() {
            return self.dispatch(backend, UiAction::Unstage(resource));
        }
        let stageable = self
            .semantic
            .contextual_actions
            .iter()
            .filter(|action| action.stageability == ActionStageability::Stageable)
            .collect::<Vec<_>>();
        match stageable.as_slice() {
            [action] => self.dispatch(backend, UiAction::InvokeAction(action.action.clone())),
            [] => {
                self.semantic.status = Some(crate::application::UiStatus {
                    message: "the selected resource exposes no stageable action".into(),
                });
                Ok(())
            }
            _ => {
                self.semantic.status = Some(crate::application::UiStatus {
                    message: "multiple stageable actions are available; press : and choose one".into(),
                });
                Ok(())
            }
        }
    }

    fn invoke_selected_action<B: PaletteBackend>(
        &mut self,
        backend: &mut B,
        require_stageable: bool,
    ) -> Result<()> {
        let Some(action) = selected_contextual_action(&self.semantic) else {
            return Ok(());
        };
        if require_stageable && action.stageability != ActionStageability::Stageable {
            self.semantic.status = Some(crate::application::UiStatus {
                message: format!(
                    "{} is immediate, not stageable; press Enter to invoke it",
                    action.label
                ),
            });
            return Ok(());
        }
        self.dispatch(backend, UiAction::InvokeAction(action.action))
    }

    fn open_selected_action<B: PaletteBackend>(&mut self, backend: &mut B) -> Result<()> {
        let immediate = self
            .semantic
            .contextual_actions
            .iter()
            .filter(|action| action.stageability == ActionStageability::NotStageable)
            .collect::<Vec<_>>();
        match immediate.as_slice() {
            [action] => self.dispatch(backend, UiAction::InvokeAction(action.action.clone())),
            [] => Ok(()),
            _ => self.dispatch(backend, UiAction::BeginActionSearch),
        }
    }

    fn dispatch<B: PaletteBackend>(&mut self, backend: &mut B, action: UiAction) -> Result<()> {
        {
            let mut service = ApplicationService::new(backend);
            self.semantic = self.runtime.step(&mut service, self.semantic.clone(), action)?;
            self.project_world = service.project_world().ok();
        }
        self.refresh_relation(backend)
    }

    fn refresh_relation<B: PaletteBackend>(&mut self, backend: &mut B) -> Result<()> {
        let Some(subject) = self.relation_subject() else {
            self.relation = None;
            self.graph_layout = None;
            return Ok(());
        };
        let service = ApplicationService::new(backend);
        self.relation = service
            .relations_at_depth(&subject, self.semantic.graph.depth)
            .ok();
        self.sync_graph_layout();
        Ok(())
    }

    /// The subject the currently-active relation neighbourhood is fetched
    /// for. Graph pins the fetch to its own explicit `graph.focus` (falling
    /// back to the canonical selection when nothing has been recentred yet,
    /// which is the ordinary state); List/Tree always follow the canonical
    /// selection directly, exactly as before Graph existed. This is what
    /// lets moving the highlighted node *within* an open Graph (arrow/hjkl,
    /// which changes `selected` but not `graph.focus`) leave the fetched
    /// neighbourhood — and therefore the cached layout — untouched.
    fn relation_subject(&self) -> Option<ResourceRef> {
        if self.semantic.relation_view == RelationView::Graph {
            self.semantic
                .graph
                .focus
                .clone()
                .or_else(|| self.semantic.selected.clone())
        } else {
            self.semantic.selected.clone()
        }
    }

    /// The Rect the Graph canvas actually draws into: `panes.list` with its
    /// border consumed, computed the same way `draw_relations` and
    /// `handle_mouse` derive `panes.list` from `semantic.area`, so hit-testing
    /// and rendering can never drift into different coordinate spaces.
    fn graph_content_rect(&self) -> Rect {
        let (cols, rows) = self.semantic.area;
        let cols = cols.max(2);
        let rows = rows.max(2);
        let inner = Rect::new(1, 1, cols.saturating_sub(2), rows.saturating_sub(2));
        let panes = Layout::for_width(inner.width).split(inner);
        let list = panes.list;
        Rect::new(
            list.x.saturating_add(1),
            list.y.saturating_add(1),
            list.width.saturating_sub(2),
            list.height.saturating_sub(2),
        )
    }

    /// The spatial canvas's own viewport: `graph_content_rect` with its
    /// bottom rows reserved for the legend and Inspector text
    /// (`graph_presentation::spatial_lines` appends both after the canvas,
    /// and a `Paragraph` never scrolls — giving the canvas the *entire*
    /// content height would starve the legend/Inspector of any visible
    /// rows). The split is a documented simplification, not a computed
    /// optimum: the canvas keeps three fifths of the available rows (or
    /// everything, below `MIN_CANVAS_HEIGHT`, where splitting further would
    /// leave neither part usable).
    fn graph_viewport(&self) -> GraphViewport {
        const MIN_CANVAS_HEIGHT: u16 = 5;
        let content = self.graph_content_rect();
        let height = if content.height <= MIN_CANVAS_HEIGHT {
            content.height
        } else {
            (content.height * 3 / 5).max(MIN_CANVAS_HEIGHT)
        };
        GraphViewport::new(content.width, height)
    }

    /// Whether the Graph relation view is the thing actually on screen right
    /// now (Workspace presentation, Explore section, Graph relation view) —
    /// the precondition for every Graph-specific key/mouse binding.
    fn graph_projection_active(&self) -> bool {
        self.semantic.presentation == PresentationMode::Workspace
            && self.semantic.workspace_section == WorkspaceSection::Knowledge
            && self.semantic.relation_view == RelationView::Graph
    }

    /// Whether Graph has enough geometry to render the spatial canvas rather
    /// than the narrow band-grouped fallback. Reuses the crate's own
    /// `Width::Narrow` breakpoint (< 60 columns, see `layout.rs`) instead of
    /// inventing a parallel horizontal threshold — but a spatial canvas also
    /// needs *vertical* room that column width alone does not capture (a
    /// wide-but-shallow pane cannot show more than one or two Incoming/
    /// Outgoing rows), so a documented height floor is added on top.
    fn graph_is_spatial(&self) -> bool {
        const MIN_SPATIAL_HEIGHT: u16 = 6;
        let (cols, _) = self.semantic.area;
        Layout::for_width(cols.saturating_sub(2).max(1)).width != Width::Narrow
            && self.graph_content_rect().height >= MIN_SPATIAL_HEIGHT
    }

    /// Recompute the cached `GraphLayout` only when the relation data, the
    /// graph-local filter, or the viewport actually changed since the last
    /// call — never merely because the highlighted node moved. Called at the
    /// end of every `refresh_relation`, i.e. after every dispatched Action,
    /// so `draw`/`handle_key`/`handle_mouse` can all treat `graph_layout` as
    /// an up-to-date, already-computed read rather than recomputing it
    /// themselves.
    fn sync_graph_layout(&mut self) {
        let Some(relation) = self.relation.as_ref() else {
            self.graph_layout = None;
            return;
        };
        let viewport = self.graph_viewport();
        let key = GraphLayoutCacheKey {
            view: relation.view.clone(),
            filter: self.semantic.graph.filter.clone(),
            viewport: (viewport.width, viewport.height),
        };
        if self.graph_layout.as_ref().map(|(existing, _)| existing) == Some(&key) {
            return;
        }
        let filtered = graph_presentation::filtered_relation_view(&relation.view, &key.filter);
        let request = GraphLayoutRequest::for_viewport(viewport);
        let layout = graph_layout::layout(&filtered, &request);
        self.graph_layout_recomputes += 1;
        self.graph_layout = Some((key, layout));
    }

    fn draw_relations(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        if area.width < 3 || area.height < 3 {
            return;
        }
        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        let panes = Layout::for_width(inner.width).split(inner);
        let theme = Theme::new();
        let sep = self.shell_glyphs.separator();
        let title = if self.semantic.relation_view == RelationView::Graph {
            format!(
                " Relations {sep} Graph {sep} depth {} {}",
                self.semantic.graph.depth,
                if self.semantic.graph.filter.is_empty() {
                    String::new()
                } else {
                    format!("{sep} filter \"{}\" ", self.semantic.graph.filter)
                }
            )
        } else {
            format!(" Relations {sep} {:?} ", self.semantic.relation_view)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(self.shell_glyphs.border_set())
            .title(title);

        let lines = if self.semantic.relation_view == RelationView::Graph {
            let glyphs = self.graph_glyphs;
            match self.graph_layout.as_ref() {
                Some((_, layout)) if self.graph_is_spatial() => graph_presentation::spatial_lines(
                    layout,
                    self.semantic.selected.as_ref(),
                    &glyphs,
                    &theme,
                    self.graph_viewport(),
                ),
                Some((_, layout)) => graph_presentation::grouped_lines(layout, &glyphs, &theme),
                None => vec![Line::from(Span::styled("no relation state", theme.dim()))],
            }
        } else {
            relation_lines(
                self.relation.as_ref(),
                self.semantic.relation_view,
                &theme,
                self.shell_glyphs,
            )
        };
        frame.render_widget(
            Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
            panes.list,
        );
    }
}

/// The Graph connector set that goes with one already-resolved host glyph
/// capability. Derived rather than sniffed separately, so a single
/// `AIKIT_ASCII`/locale reading governs every glyph set this crate renders
/// and the two can never disagree. Called exactly once, at
/// [`ApplicationSurfaceController::new`] — never at draw time. See
/// [`ApplicationSurfaceController::shell_glyphs`] and
/// [`ApplicationSurfaceRequest::with_glyphs`].
fn graph_glyphs_for(glyphs: Glyphs) -> graph_layout::GraphGlyphs {
    if glyphs.is_ascii() {
        graph_layout::GraphGlyphs::ascii()
    } else {
        graph_layout::GraphGlyphs::unicode()
    }
}

fn workspace_tab_hit(state: &TuiState, query_x: u16, column: u16) -> Option<WorkspaceSection> {
    let displayed_query = if let Some(action_query) = state.action_query.as_ref() {
        if action_query.is_empty() {
            "search actions for selection"
        } else {
            action_query
        }
    } else if state.query.is_empty() {
        "Search resources and actions"
    } else {
        &state.query
    };
    let mut cursor = query_x
        .saturating_add(2)
        .saturating_add(u16::try_from(displayed_query.chars().count()).unwrap_or(u16::MAX))
        .saturating_add(3)
        .saturating_add(6); // `Search`
    for section in WorkspaceSection::ALL {
        cursor = cursor.saturating_add(3); // the separator plus its two spaces
        let label = workspace_section_label(section);
        let width = u16::try_from(label.chars().count()).unwrap_or(u16::MAX);
        if column >= cursor && column < cursor.saturating_add(width) {
            return Some(section);
        }
        cursor = cursor.saturating_add(width);
    }
    None
}

/// Renders the typed neighbourhood in `relation.view` — the canonical
/// [`aikit_core::KnowledgeRelationView`] `ApplicationService::relations`
/// returns — as List/Tree rows. Every row carries the provider's own
/// relation name, direction and origin authority; nothing here re-derives a
/// relation from a string. Graph is rendered separately by
/// `graph_presentation` (see `ApplicationSurfaceController::draw_relations`);
/// this function is never called for `RelationView::Graph`.
fn relation_lines<'a>(
    relation: Option<&'a RelationReadModel>,
    view: RelationView,
    theme: &Theme,
    glyphs: Glyphs,
) -> Vec<Line<'a>> {
    let Some(relation) = relation else {
        return vec![Line::from(Span::styled("no relation state", theme.dim()))];
    };
    match view {
        RelationView::Tree => tree_relation_lines(relation, theme, glyphs),
        RelationView::List | RelationView::Graph => list_relation_lines(relation, theme, glyphs),
    }
}

fn list_relation_lines<'a>(
    relation: &'a RelationReadModel,
    theme: &Theme,
    glyphs: Glyphs,
) -> Vec<Line<'a>> {
    let mut lines = vec![Line::from(Span::styled(
        relation.subject.to_string(),
        theme.heading(),
    ))];
    let edges = &relation.view.edges;
    if edges.is_empty() {
        lines.push(Line::from(Span::styled(
            "no typed resource relations",
            theme.dim(),
        )));
        return lines;
    }
    for edge in edges {
        let other = if edge.from == relation.subject {
            &edge.to
        } else {
            &edge.from
        };
        let arrow = glyphs.relation_arrow(edge.direction);
        let detail = format!(
            "{} {arrow} {other}  ({:?})",
            edge.relation, edge.origin.authority
        );
        lines.push(Line::from(Span::raw(detail)));
    }
    lines
}

/// Tree shows only genuine hierarchy/containment — reusing `graph_layout`'s
/// own Context/Contained band classification (its documented containment
/// vocabulary: `member`, `child-space`, `member-of`, `part-of`, etc — see
/// `graph_layout`'s module doc) rather than forcing every typed relation
/// into a fake nesting the way the old compatibility tree renderer did. A
/// neighbourhood with no containment relation says so plainly instead of
/// drawing branch glyphs over relations that were never hierarchical.
///
/// This reclassifies the already-fetched view through `graph_layout::layout`
/// with a viewport large enough that nothing is ever viewport-truncated —
/// Tree only wants the band classification, not a screen position — so
/// `truncated` here can only ever reflect the provider's own budget, never
/// this rendering's geometry. It is a small, uncached computation bounded by
/// the same relation budgets `graph_layout`'s own module doc discusses; it is
/// not the Graph projection's cached layout (see
/// `ApplicationSurfaceController::graph_layout`), which is the one this
/// crate's "recompute only when inputs change" contract actually governs.
fn tree_relation_lines<'a>(
    relation: &'a RelationReadModel,
    theme: &Theme,
    glyphs: Glyphs,
) -> Vec<Line<'a>> {
    let mut lines = vec![Line::from(Span::styled(
        relation.subject.to_string(),
        theme.heading(),
    ))];
    let request = GraphLayoutRequest::for_viewport(GraphViewport::new(u16::MAX, u16::MAX));
    let laid = graph_layout::layout(&relation.view, &request);
    let context: Vec<_> = laid
        .edges
        .iter()
        .filter(|edge| edge.band == RelationBand::Context)
        .collect();
    let contained: Vec<_> = laid
        .edges
        .iter()
        .filter(|edge| edge.band == RelationBand::Contained)
        .collect();
    if context.is_empty() && contained.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(
                "no genuine containment relation in this neighbourhood {} Tree only shows hierarchy; List shows the full typed neighbourhood",
                glyphs.dash()
            ),
            theme.dim(),
        )));
        return lines;
    }
    let labels: BTreeMap<&ResourceRef, &str> = laid
        .nodes
        .iter()
        .map(|node| (&node.resource, node.label.as_str()))
        .collect();
    let render_group = |title: &str, members: &[&graph_layout::LaidOutEdge], lines: &mut Vec<Line<'a>>| {
        if members.is_empty() {
            return;
        }
        lines.push(Line::from(Span::styled(title.to_string(), theme.dim())));
        let count = members.len();
        for (index, edge) in members.iter().enumerate() {
            let other = if edge.from == relation.subject {
                &edge.to
            } else {
                &edge.from
            };
            let label = labels.get(other).copied().unwrap_or_else(|| other.as_str());
            lines.push(Line::from(Span::raw(format!(
                "{}{} {label} ({})",
                if index + 1 == count {
                    glyphs.branch_last()
                } else {
                    glyphs.branch_tee()
                },
                glyphs.branch_stem(),
                edge.relation
            ))));
        }
    };
    render_group("Context (contains this subject)", &context, &mut lines);
    render_group("Contained (members of this subject)", &contained, &mut lines);
    lines
}

fn ambient_context(descriptor: &aikit_core::ContextDescriptor) -> AmbientContext {
    AmbientContext {
        project: descriptor
            .project_root
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned()),
        focus: descriptor.task.clone(),
        profile: None,
        agency: None,
        host: (!descriptor.host.is_empty()).then(|| descriptor.host.clone()),
        target: descriptor
            .targets
            .first()
            .map(|target| target.as_str().to_string()),
    }
}

pub fn event_loop<B, T, E>(
    terminal: &mut Terminal<T>,
    events: &mut E,
    backend: &mut B,
    request: ApplicationSurfaceRequest,
) -> Result<PaletteOutcome>
where
    B: PaletteBackend,
    T: Backend,
    T::Error: std::fmt::Display,
    E: EventSource + ?Sized,
{
    let mut controller = ApplicationSurfaceController::new(backend, request)?;
    let size = terminal
        .size()
        .map_err(|error| AikitError::new("tui.terminal_size_failed", format!("{error}")))?;
    controller.dispatch(backend, UiAction::Resize(size.width, size.height))?;
    loop {
        controller.draw_terminal(terminal)?;
        let Some(event) = events.next()? else {
            return Ok(PaletteOutcome::Closed);
        };
        match controller.handle(backend, event)? {
            ApplicationSurfaceStep::Continue => {}
            ApplicationSurfaceStep::Outcome(outcome) => return Ok(outcome),
        }
    }
}

pub fn run_on_terminal<B: PaletteBackend>(
    backend: &mut B,
    request: ApplicationSurfaceRequest,
) -> Result<PaletteOutcome> {
    let host = request.host;
    let fullscreen = host == UiHost::Fullscreen;
    let _session = TerminalSession::enter(fullscreen)?;
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

fn terminal_setup_error(message: &str, error: impl std::fmt::Display) -> AikitError {
    AikitError::new("tui.terminal_setup_failed", format!("{message}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_view_rotation_is_total() {
        let next = |view| match view {
            RelationView::List => RelationView::Tree,
            RelationView::Tree => RelationView::Graph,
            RelationView::Graph => RelationView::List,
        };
        assert_eq!(next(RelationView::List), RelationView::Tree);
        assert_eq!(next(RelationView::Tree), RelationView::Graph);
        assert_eq!(next(RelationView::Graph), RelationView::List);
    }
}
