//! Final V2 terminal surface over one [`TuiState`] / [`TuiRuntime`] authority.
//!
//! Quick and Workspace are presentations of the same semantic state. List / Tree /
//! Graph are projections of one cached [`RelationReadModel`] returned by the same
//! application service. Renderers and mouse hit-testing dispatch semantic Actions;
//! they do not own resolver, selection, retrieval or mutation state.

use std::io;

use aikit_core::resource::ActionStageability;
use aikit_core::{AikitError, ProjectWorldReadModel, Result};
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEventKind, KeyModifiers, MouseButton,
    MouseEventKind,
};
use ratatui::backend::{Backend, CrosstermBackend};
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
use crate::host::UiHost;
use crate::layout::Layout;
use crate::navigation::AmbientContext;
use crate::project_workspace_render::workspace_section_label;
use crate::project_world_api::ProjectWorldApplicationService;
use crate::theme::Theme;
use crate::v2_render;
use crate::PaletteOutcome;

#[derive(Debug, Clone)]
pub struct ApplicationSurfaceRequest {
    pub host: UiHost,
    pub initial_query: Option<String>,
    pub initial_relation_view: RelationView,
    pub initial_workspace_section: WorkspaceSection,
}

impl ApplicationSurfaceRequest {
    pub fn new(host: UiHost) -> Self {
        Self {
            host,
            initial_query: None,
            initial_relation_view: RelationView::List,
            initial_workspace_section: WorkspaceSection::Projects,
        }
    }

    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.initial_query = Some(query.into());
        self
    }

    /// Open Knowledge with one projection of the canonical relation read model.
    #[must_use]
    pub fn opening_relations(mut self, view: RelationView) -> Self {
        self.initial_relation_view = view;
        self.initial_workspace_section = WorkspaceSection::Explore;
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
}

impl ApplicationSurfaceController {
    pub fn new<B: PaletteBackend>(
        backend: &mut B,
        request: ApplicationSurfaceRequest,
    ) -> Result<Self> {
        let ambient = ambient_context(backend.context());
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
                self.handle_mouse(backend, mouse.column, mouse.row)?;
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
            v2_render::draw_with_project_world(frame, &self.semantic, &self.ambient, world);
        } else {
            v2_render::draw_with_context(frame, &self.semantic, &self.ambient);
        }
        if self.semantic.presentation == PresentationMode::Workspace
            && self.semantic.workspace_section == WorkspaceSection::Explore
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

        if self.semantic.action_query.is_some() {
            return self.handle_action_key(backend, code, ctrl);
        }
        if ctrl && matches!(code, KeyCode::Char('c') | KeyCode::Char('q')) {
            return self.dispatch(backend, UiAction::Exit);
        }
        if code == KeyCode::Esc {
            return self.dispatch(backend, UiAction::Back);
        }
        if ctrl && code == KeyCode::Char('u') {
            return self.dispatch(backend, UiAction::SetQuery(String::new()));
        }
        if ctrl && code == KeyCode::Char('w') {
            return self.toggle_presentation(backend);
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
                UiAction::SetWorkspaceSection(WorkspaceSection::Explore),
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
    ) -> Result<()> {
        let (cols, rows) = self.semantic.area;
        let cols = cols.max(2);
        let rows = rows.max(2);

        // The title itself is the compact/expanded affordance. It changes only
        // PresentationMode on the same TuiState.
        if row == 0 {
            return self.toggle_presentation(backend);
        }

        let inner = ratatui::layout::Rect::new(
            1,
            1,
            cols.saturating_sub(2),
            rows.saturating_sub(2),
        );
        let panes = Layout::for_width(inner.width).split(inner);

        if self.semantic.presentation == PresentationMode::Workspace && row == panes.query.y {
            if let Some(section) = workspace_tab_hit(&self.semantic, panes.query.x, column) {
                return self.dispatch(backend, UiAction::SetWorkspaceSection(section));
            }
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
        let Some(subject) = self.semantic.selected.clone() else {
            self.relation = None;
            return Ok(());
        };
        let service = ApplicationService::new(backend);
        self.relation = service.relations(&subject).ok();
        Ok(())
    }

    fn draw_relations(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        if area.width < 3 || area.height < 3 {
            return;
        }
        let inner = ratatui::layout::Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        let panes = Layout::for_width(inner.width).split(inner);
        let theme = Theme::new();
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Relations · {:?} ", self.semantic.relation_view));
        let lines = relation_lines(self.relation.as_ref(), self.semantic.relation_view, &theme);
        frame.render_widget(
            Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
            panes.list,
        );
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
        cursor = cursor.saturating_add(3); // ` · `
        let label = workspace_section_label(section);
        let width = u16::try_from(label.chars().count()).unwrap_or(u16::MAX);
        if column >= cursor && column < cursor.saturating_add(width) {
            return Some(section);
        }
        cursor = cursor.saturating_add(width);
    }
    None
}

fn relation_lines<'a>(
    relation: Option<&'a RelationReadModel>,
    view: RelationView,
    theme: &Theme,
) -> Vec<Line<'a>> {
    let Some(relation) = relation else {
        return vec![Line::from(Span::styled("no relation state", theme.dim()))];
    };
    let mut related = relation
        .value
        .get("related")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    if related.is_empty() {
        related = relation
            .value
            .get("resolverRelated")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
    }

    let mut lines = vec![Line::from(Span::styled(
        relation.subject.to_string(),
        theme.heading(),
    ))];
    if related.is_empty() {
        lines.push(Line::from(Span::styled(
            "no typed resource relations",
            theme.dim(),
        )));
        return lines;
    }
    let related_len = related.len();
    for (index, target) in related.into_iter().enumerate() {
        let text = match view {
            RelationView::List => target.to_string(),
            RelationView::Tree => format!(
                "{}─ {}",
                if index + 1 == related_len { "└" } else { "├" },
                target
            ),
            RelationView::Graph => format!("{}  ──related──▶  {}", relation.subject, target),
        };
        lines.push(Line::from(Span::raw(text)));
    }
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
