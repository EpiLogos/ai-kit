//! Hosting AIKit's terminal surfaces.
//!
//! Placement remains a CLI concern; semantic operation belongs to shared
//! application services. The shipped `aikit ui` path now hosts the reducer-native
//! V2 application surface directly. The standalone legacy palette/tree entry
//! points remain only for explicit compatibility commands while #59 removes or
//! replaces their final callers.

use std::path::PathBuf;

use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::platform::MuxKind;
use aikit_core::resolve::ResolvedView;
use aikit_core::resource::ResourceSearchIndex;
use aikit_core::scope::{ScopeKind, ScopeLayer};
use aikit_core::search::SearchDoc;
use aikit_core::{FamiliarityObservation, FamiliarityStore, Result};

use aikit_store::{
    familiarity_observation_event, replay_familiarity, EventRecorder, FamiliarityReplay,
};

use aikit_tui::application::RelationView;
use aikit_tui::application_surface::ApplicationSurfaceRequest;
use aikit_tui::backend::{JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle};
use aikit_tui::host::{TerminalProfile, UiHost};
use aikit_tui::tree_driver::{TreeOutcome, TreeRequest};
use aikit_tui::{PaletteOutcome, PaletteRequest};

use crate::app::Service;

/// V2 surface decorator that gives the shared Service one additional operational
/// responsibility: replaying and recording learned navigation evidence.
///
/// Familiarity is rebuildable evidence and therefore belongs at the
/// host/application boundary rather than inside canonical resolution.
struct V2SurfaceService<'a> {
    service: &'a mut Service,
}

impl<'a> V2SurfaceService<'a> {
    fn new(service: &'a mut Service) -> Self {
        Self { service }
    }
}

impl PaletteBackend for V2SurfaceService<'_> {
    fn context(&self) -> &aikit_core::ContextDescriptor {
        <Service as PaletteBackend>::context(self.service)
    }

    fn view(&self) -> &ResolvedView {
        <Service as PaletteBackend>::view(self.service)
    }

    fn scope_layers(&self) -> Option<&[ScopeLayer]> {
        <Service as PaletteBackend>::scope_layers(self.service)
    }

    fn documents(&self) -> Vec<SearchDoc> {
        <Service as PaletteBackend>::documents(self.service)
    }

    fn navigation_index(&self) -> ResourceSearchIndex {
        <Service as PaletteBackend>::navigation_index(self.service)
    }

    fn familiarity(&self) -> Result<Option<FamiliarityStore>> {
        match replay_familiarity(self.service.index())? {
            FamiliarityReplay::Loaded { store, .. } => Ok(Some(store)),
            FamiliarityReplay::Invalidated { .. } => Ok(None),
        }
    }

    fn record_familiarity(&mut self, observation: FamiliarityObservation) -> Result<()> {
        let event = familiarity_observation_event(observation)?;
        EventRecorder::new(self.service.index(), self.service.home().event_log()).record(&event)
    }

    fn capsule(&self, id: &CapsuleId) -> Option<&aikit_core::Capsule> {
        <Service as PaletteBackend>::capsule(self.service, id)
    }

    fn preview(&self, scope: ScopeKind, toggles: &[Toggle]) -> Result<Projected> {
        <Service as PaletteBackend>::preview(self.service, scope, toggles)
    }

    fn apply(&mut self, scope: ScopeKind, toggles: &[Toggle]) -> Result<GenerationId> {
        <Service as PaletteBackend>::apply(self.service, scope, toggles)
    }

    fn start(&mut self, intent: &RunIntent) -> Result<JobOutput> {
        <Service as PaletteBackend>::start(self.service, intent)
    }

    fn recent(&self) -> Vec<RunIntent> {
        <Service as PaletteBackend>::recent(self.service)
    }

    fn promotion_drafts(&self) -> Vec<PromotionDraft> {
        <Service as PaletteBackend>::promotion_drafts(self.service)
    }

    fn promote(&mut self, draft: &PromotionDraft) -> Result<CapsuleId> {
        <Service as PaletteBackend>::promote(self.service, draft)
    }

    fn open_source(&mut self, id: &CapsuleId) -> Result<PathBuf> {
        <Service as PaletteBackend>::open_source(self.service, id)
    }
}

/// Build a terminal profile from an environment lookup and the `--fullscreen`
/// flag.
pub fn terminal_profile<F>(env: F, fullscreen: bool) -> TerminalProfile
where
    F: Fn(&str) -> Option<String>,
{
    let (cols, rows) = terminal_size(&env);
    let mut profile = TerminalProfile::new(cols, rows);

    if env("TMUX").is_some() {
        profile = profile.in_mux(MuxKind::Tmux);
    } else if env("CMUX").is_some() || env("CMUX_SURFACE").is_some() {
        profile = profile.in_mux(MuxKind::Cmux);
    }

    if fullscreen {
        profile = profile.requested(UiHost::Fullscreen);
    }
    profile
}

fn terminal_size<F>(env: &F) -> (u16, u16)
where
    F: Fn(&str) -> Option<String>,
{
    let parse = |key: &str, default: u16| {
        env(key)
            .and_then(|value| value.trim().parse::<u16>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(default)
    };
    (parse("COLUMNS", 80), parse("LINES", 24))
}

/// Open the final V2 application surface over one live service.
///
/// `opening_tree` is retained as an external CLI flag for compatibility, but its
/// meaning is now "open Explore with the Tree projection of the one relation read
/// model". It does not instantiate `TreeState` or `TreeController`.
pub fn run_surface(
    service: &mut Service,
    query: Option<String>,
    fullscreen: bool,
    opening_tree: bool,
) -> Result<PaletteOutcome> {
    let profile = terminal_profile(|key| std::env::var(key).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    let mut request = ApplicationSurfaceRequest::new(host);
    if let Some(query) = query {
        request = request.with_query(query);
    }
    if opening_tree {
        request = request.opening_relations(RelationView::Tree);
    }
    let mut backend = V2SurfaceService::new(service);
    aikit_tui::application_surface::run_on_terminal(&mut backend, request)
}

/// Explicit compatibility palette entry point.
pub fn run(
    service: &mut Service,
    query: Option<String>,
    fullscreen: bool,
) -> Result<PaletteOutcome> {
    let profile = terminal_profile(|key| std::env::var(key).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    let mut request = PaletteRequest::new(host);
    if let Some(query) = query {
        request = request.with_query(query);
    }
    aikit_tui::run(service, request)
}

/// Open the exact compatibility palette row and immediately hand it to its
/// natural action. This remains until the external activation path is translated
/// to ResourceRef-native Actions.
pub fn run_activation(
    service: &mut Service,
    query: String,
    fullscreen: bool,
) -> Result<PaletteOutcome> {
    let profile = terminal_profile(|key| std::env::var(key).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    aikit_tui::run(
        service,
        PaletteRequest::new(host)
            .with_query(query)
            .activating_initial(),
    )
}

/// Explicit compatibility standalone tree entry point. `aikit ui --tree` no
/// longer uses this path.
pub fn run_tree(service: &Service, fullscreen: bool) -> Result<TreeOutcome> {
    let profile = terminal_profile(|key| std::env::var(key).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    let state = crate::tree_build::build(service)?;
    let scope = service.descriptor().default_mutation_scope();
    let mut request = TreeRequest::new(host);
    if scope.requires_confirmation_to_write() {
        request = request.with_apply_confirmation(
            format!("Write staged changes to the {scope} profile?"),
            match scope {
                aikit_core::scope::ScopeKind::Global => {
                    "The User Baseline Profile applies at lowest precedence in every AIKit context."
                }
                aikit_core::scope::ScopeKind::Project => {
                    "<repo>/.aikit/profile.toml is committed and affects every collaborator."
                }
                _ => "This change is durable.",
            },
        );
    }
    aikit_tui::run_tree(state, request)
}
