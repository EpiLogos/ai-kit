//! One transient AIKit surface with palette and tree compatibility presentations.
//!
//! V2 makes [`TuiState`] the semantic owner. Palette and tree remain resident
//! compatibility presentations while they are migrated. Search, selection,
//! staging, mutation scope and composition effects are routed through the V2
//! reducer/runtime; the legacy controllers retain only presentation-specific and
//! capability-specific interaction until V2 Action descriptors replace them.

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

use crate::app::{Mode, Status};
use crate::application::{
    keyboard_select, mouse_select, reduce_tui, ActivationIntent, Overlay, PresentationMode,
    RelationView, ResourceListItem, ResourceListReadModel, TuiRuntime, TuiState, UiAction,
};
use crate::backend::{PaletteBackend, Toggle};
use crate::driver::{PaletteController, PaletteStep};
use crate::event::{CrosstermEvents, EventSource, PaletteEvent};
use crate::host::UiHost;
use crate::layout::Layout;
use crate::palette_service::PaletteApplicationService;
use crate::scope::ScopeSelector;
use crate::search::Row;
use crate::staging::is_on;
use crate::tree::{Node, NodeKind, TreeEffect, TreeState};
use crate::tree_driver::{TreeController, TreeRequest, TreeStep};
use crate::v2_render;
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
    runtime: TuiRuntime,
    palette: PaletteController,
    tree: TreeController,
}

impl SurfaceController {
    pub fn new<B: SurfaceBackend>(backend: &mut B, request: SurfaceRequest) -> Result<Self> {
        // The resident V1 controllers are created only as compatibility
        // presentations. Their initial effect pass is not accepted as V2 state;
        // the canonical read model is immediately resolved again through
        // TuiRuntime + TuiApplicationService below.
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
            runtime: TuiRuntime::new(),
            palette,
            tree,
        };
        match surface.mode {
            SurfaceMode::Palette => surface.capture_palette_semantics(),
            SurfaceMode::Tree => surface.capture_tree_semantics(),
        }
        let initial_query = surface.semantic.query.clone();
        surface.dispatch_semantic(backend, UiAction::SetQuery(initial_query))?;
        surface.ensure_semantic_selection();
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
            SurfaceMode::Palette
                if self.palette.state().mode == Mode::Search
                    && !self.palette.state().in_manage_lane() =>
            {
                v2_render::draw(frame, &self.semantic)
            }
            SurfaceMode::Palette => self.palette.draw(frame),
            SurfaceMode::Tree => self.tree.draw(frame),
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
                if let Some(step) = self.handle_v2_palette_event(backend, &event)? {
                    return Ok(step);
                }

                // Capability-specific details/forms/run handoff remain on the
                // compatibility controller. Search/stage/apply never reach this
                // path in the resting V2 surface.
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

    /// V2 owns ambiguous navigation before either compatibility presentation sees
    /// it. Esc is only back/dismiss at the resting surface; clear and exit are
    /// separate explicit commands.
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
            self.sync_semantic_status();
            return Ok(Some(SurfaceStep::Continue));
        }

        if control && key.code == KeyCode::Char('u') {
            self.clear_query_explicitly(backend)?;
            return Ok(Some(SurfaceStep::Continue));
        }

        if key.code == KeyCode::Esc {
            if self.mode == SurfaceMode::Palette && self.palette.state().mode != Mode::Search {
                return Ok(None);
            }
            self.apply_semantic(UiAction::Back);
            self.project_semantic_to_presentations(backend);
            return Ok(Some(SurfaceStep::Continue));
        }

        Ok(None)
    }

    /// Translate the resting Quick/Workspace compatibility palette into the one
    /// V2 action language. Keys not owned here are capability-specific legacy
    /// interactions and are deliberately handed on.
    fn handle_v2_palette_event<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        event: &PaletteEvent,
    ) -> Result<Option<SurfaceStep>> {
        if self.palette.state().mode != Mode::Search || self.palette.state().in_manage_lane() {
            return Ok(None);
        }
        let PaletteEvent::Key(key) = event else {
            return Ok(None);
        };
        if key.kind == KeyEventKind::Release {
            return Ok(Some(SurfaceStep::Continue));
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if ctrl && key.code == KeyCode::Char('s')
            || ctrl && key.code == KeyCode::Enter
        {
            self.advance_composition(backend)?;
            return Ok(Some(SurfaceStep::Continue));
        }

        if ctrl && key.code == KeyCode::Char('w') {
            let presentation = match self.semantic.presentation {
                PresentationMode::Quick => PresentationMode::Workspace,
                PresentationMode::Workspace => PresentationMode::Quick,
            };
            self.apply_semantic(UiAction::SetPresentation(presentation));
            self.sync_semantic_status();
            return Ok(Some(SurfaceStep::Continue));
        }

        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            self.cycle_scope(backend)?;
            return Ok(Some(SurfaceStep::Continue));
        }

        if key.code == KeyCode::Insert || ctrl && key.code == KeyCode::Char(' ') {
            self.toggle_selected_staged(backend);
            self.project_semantic_to_presentations(backend);
            return Ok(Some(SurfaceStep::Continue));
        }

        if key.code == KeyCode::Up || ctrl && key.code == KeyCode::Char('k') {
            self.apply_semantic(UiAction::SelectPrevious);
            self.project_semantic_to_palette(backend);
            return Ok(Some(SurfaceStep::Continue));
        }
        if key.code == KeyCode::Down || ctrl && key.code == KeyCode::Char('j') {
            self.apply_semantic(UiAction::SelectNext);
            self.project_semantic_to_palette(backend);
            return Ok(Some(SurfaceStep::Continue));
        }

        if key.code == KeyCode::Enter {
            if let Some(resource) = self.semantic.selected.clone() {
                if resource_capsule_id(&resource).is_none() {
                    self.apply_semantic(UiAction::OpenSelection);
                    let kind = self
                        .semantic
                        .read_model
                        .resources
                        .iter()
                        .find(|item| item.resource == resource)
                        .map(|item| item.kind.as_str())
                        .unwrap_or("resource");
                    self.semantic.status = Some(crate::application::UiStatus {
                        message: format!("opened {kind} {resource}"),
                    });
                    self.sync_semantic_status();
                    return Ok(Some(SurfaceStep::Continue));
                }
            }
            return Ok(None);
        }

        if key.code == KeyCode::Backspace {
            let mut query = self.semantic.query.clone();
            query.pop();
            self.update_query(backend, query)?;
            return Ok(Some(SurfaceStep::Continue));
        }

        if key.code == KeyCode::Char(' ') && self.semantic.query.is_empty() {
            self.toggle_selected_staged(backend);
            self.project_semantic_to_presentations(backend);
            return Ok(Some(SurfaceStep::Continue));
        }

        // Keep the proven management lane alive until V2 contextual Action
        // descriptors replace it; entering ':' therefore remains explicitly a
        // compatibility interaction rather than a second V2 search grammar.
        if key.code == KeyCode::Char(':') && self.semantic.query.is_empty() {
            return Ok(None);
        }
        if key.code == KeyCode::Char('?') && self.semantic.query.is_empty() {
            return Ok(None);
        }

        if let KeyCode::Char(c) = key.code {
            if !ctrl && !alt {
                let mut query = self.semantic.query.clone();
                query.push(c);
                self.update_query(backend, query)?;
                return Ok(Some(SurfaceStep::Continue));
            }
        }

        Ok(None)
    }

    fn update_query<B: SurfaceBackend>(&mut self, backend: &mut B, query: String) -> Result<()> {
        self.dispatch_semantic(backend, UiAction::SetQuery(query))?;
        self.ensure_semantic_selection();
        self.project_semantic_to_palette(backend);
        self.tree.state_mut().filter = self.semantic.query.clone();
        Ok(())
    }

    fn clear_query_explicitly<B: SurfaceBackend>(&mut self, backend: &mut B) -> Result<()> {
        self.update_query(backend, String::new())?;
        self.tree.state_mut().filter.clear();
        Ok(())
    }

    fn cycle_scope<B: SurfaceBackend>(&mut self, backend: &mut B) -> Result<()> {
        let permitted = backend.context().permitted_scopes();
        if permitted.is_empty() {
            return Ok(());
        }
        let current = self
            .semantic
            .mutation_scope
            .unwrap_or_else(|| backend.context().default_mutation_scope());
        let index = permitted.iter().position(|scope| *scope == current).unwrap_or(0);
        let next = permitted[(index + 1) % permitted.len()];
        self.apply_semantic(UiAction::SetMutationScope(next));
        self.palette.state_mut().scope = ScopeSelector::with_scope(backend.context(), next)?;
        self.sync_semantic_status();
        Ok(())
    }

    fn toggle_selected_staged<B: SurfaceBackend>(&mut self, backend: &B) {
        let Some(resource) = self.semantic.selected.clone() else {
            return;
        };
        if self.semantic.staged.get(&resource).is_some() {
            self.apply_semantic(UiAction::Unstage(resource));
            return;
        }
        let Some(capsule) = resource_capsule_id(&resource) else {
            self.semantic.status = Some(crate::application::UiStatus {
                message: "the selected resource has no activation mutation".into(),
            });
            self.sync_semantic_status();
            return;
        };
        let intent = if is_on(backend.view(), &capsule) {
            ActivationIntent::Disable
        } else {
            ActivationIntent::Enable
        };
        self.apply_semantic(UiAction::Stage { resource, intent });
    }

    fn advance_composition<B: SurfaceBackend>(&mut self, backend: &mut B) -> Result<()> {
        if self.semantic.staged.is_empty() {
            self.semantic.status = Some(crate::application::UiStatus {
                message: "nothing is staged".into(),
            });
            self.sync_semantic_status();
            return Ok(());
        }

        match self.semantic.overlay {
            Some(Overlay::ConfirmApply) => {
                self.dispatch_semantic(backend, UiAction::ConfirmApply)?;
                let message = self
                    .semantic
                    .status
                    .as_ref()
                    .map(|status| status.message.clone())
                    .unwrap_or_else(|| "composition applied".into());
                self.refresh_after_change(backend, message)?;
            }
            Some(Overlay::CompositionPreview) => {
                self.dispatch_semantic(backend, UiAction::RequestApply)?;
                if self.semantic.overlay == Some(Overlay::ConfirmApply) {
                    self.palette.state_mut().status = Some(Status::info(
                        "preview accepted; press Ctrl+S again to confirm apply",
                    ));
                }
                self.project_semantic_to_presentations(backend);
            }
            _ => {
                self.dispatch_semantic(backend, UiAction::RequestCompositionPreview)?;
                if let Some(preview) = self.semantic.preview.as_ref() {
                    self.palette.state_mut().status = Some(Status::info(preview.summary.clone()));
                }
                self.project_semantic_to_presentations(backend);
            }
        }
        Ok(())
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
                self.advance_composition(backend)?;
                Ok(SurfaceStep::Continue)
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

    /// Ingest compatibility state only after a capability-specific legacy gesture.
    /// Resting search/stage/apply never get here because V2 consumed them first.
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
                self.apply_semantic(UiAction::Unstage(resource));
            }
        }
        for (resource, intent) in observed {
            if self.semantic.staged.get(&resource) != Some(intent) {
                self.apply_semantic(UiAction::Stage { resource, intent });
            }
        }
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

    /// Project the V2 search/staging state into the resident palette without
    /// executing the V1 Search or Stage effects. The order comes from the V2
    /// application-service read model; legacy rows only supply rendering metadata.
    fn project_semantic_to_palette<B: SurfaceBackend>(&mut self, backend: &mut B) {
        let docs: BTreeMap<CapsuleId, _> = backend
            .documents()
            .into_iter()
            .map(|doc| (doc.id.clone(), doc))
            .collect();
        let rows: Vec<Row> = self
            .semantic
            .read_model
            .resources
            .iter()
            .filter_map(|item| {
                let id = resource_capsule_id(&item.resource)?;
                docs.get(&id).cloned().map(|doc| Row {
                    doc,
                    score: 0.0,
                    text_score: 0.0,
                })
            })
            .collect();
        let toggles: Vec<Toggle> = self
            .semantic
            .staged
            .resources()
            .filter_map(|resource| {
                let capsule = resource_capsule_id(resource)?;
                let enable = self.semantic.staged.get(resource)? == ActivationIntent::Enable;
                Some(Toggle::new(capsule, enable))
            })
            .collect();

        let state = self.palette.state_mut();
        state.query = self.semantic.query.clone();
        state.rows = rows;
        state.staged.replace(toggles);
        state.staged_outcome = None;
        if state.rows.is_empty() {
            state.cursor = 0;
        }

        if let Some(selected) = &self.semantic.selected {
            if let Some(index) = state.rows.iter().position(|row| {
                capsule_resource_ref(&row.doc.id).as_ref() == Some(selected)
            }) {
                state.cursor = index;
            }
        }
        self.sync_semantic_status();
    }

    fn refresh_after_change<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        message: String,
    ) -> Result<()> {
        let descriptor = backend.context().clone();
        let scope = self
            .semantic
            .mutation_scope
            .unwrap_or_else(|| descriptor.default_mutation_scope());
        let scope_selector = ScopeSelector::with_scope(&descriptor, scope)?;
        let view = backend.view().clone();
        let recent = backend.recent();
        let drafts = backend.promotion_drafts();
        {
            let state = self.palette.state_mut();
            state.descriptor = descriptor;
            state.scope = scope_selector;
            state.view = view;
            state.recent = recent;
            state.drafts = drafts;
            state.outcome = None;
            state.mode = Mode::Search;
            state.form = None;
            state.confirm = None;
            state.job = None;
            state.status = Some(Status::info(message));
        }

        self.replace_tree(backend.surface_tree()?, backend);
        let query = self.semantic.query.clone();
        self.dispatch_semantic(backend, UiAction::SetQuery(query))?;
        self.ensure_semantic_selection();
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

    /// Pure state-only actions remain cheap. Any action capable of producing an
    /// application effect must go through `dispatch_semantic` instead.
    fn apply_semantic(&mut self, action: UiAction) {
        let state = std::mem::take(&mut self.semantic);
        self.semantic = reduce_tui(state, action).state;
    }

    fn dispatch_semantic<B: SurfaceBackend>(
        &mut self,
        backend: &mut B,
        action: UiAction,
    ) -> Result<()> {
        let state = std::mem::take(&mut self.semantic);
        let fallback = state.clone();
        let mut service = PaletteApplicationService::new(backend);
        match self.runtime.step(&mut service, state, action) {
            Ok(next) => {
                self.semantic = next;
                Ok(())
            }
            Err(error) => {
                self.semantic = fallback;
                Err(error)
            }
        }
    }

    fn ensure_semantic_selection(&mut self) {
        if self.semantic.selected.is_none() {
            if let Some(resource) = self
                .semantic
                .read_model
                .resources
                .first()
                .map(|item| item.resource.clone())
            {
                self.apply_semantic(UiAction::Select(resource));
            }
        }
    }

    fn sync_semantic_status(&mut self) {
        if let Some(status) = self.semantic.status.as_ref() {
            self.palette.state_mut().status = Some(Status::info(status.message.clone()));
        }
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
                let resource = self.semantic.read_model.resource_at(index)?.clone();
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
        let selected_index = self
            .semantic
            .selected
            .as_ref()
            .and_then(|selected| self.semantic.read_model.position(selected))
            .unwrap_or(0);
        let first = selected_index.saturating_sub(height.saturating_sub(1));
        let index = first.saturating_add((row - list.y) as usize);
        (index < self.semantic.read_model.resources.len()).then_some(index)
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
