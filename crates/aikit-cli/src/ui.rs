//! Hosting the palette.
//!
//! The palette's *placement* is a CLI concern — it depends on where the binary is
//! running (inside tmux? a tiny terminal? was `--fullscreen` asked for?) — while
//! everything the palette *does* is the shared [`crate::app::Service`]. This
//! module builds a [`TerminalProfile`] from the environment and lets
//! [`aikit_tui::UiHost::choose`] make the placement decision, then hands the
//! chosen host and the service to [`aikit_tui::run`].
//!
//! AIKit never embeds a terminal emulator: the tmux popup is a real
//! `display-popup`, and the inline and fullscreen hosts draw into the terminal
//! that is already there.

use std::path::PathBuf;

use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::platform::MuxKind;
use aikit_core::projection::ResolvedView;
use aikit_core::resource::ResourceSearchIndex;
use aikit_core::scope::{ScopeKind, ScopeLayer};
use aikit_core::search::SearchDoc;
use aikit_core::{FamiliarityObservation, FamiliarityStore, Result};

use aikit_store::{
    familiarity_observation_event, replay_familiarity, EventRecorder, FamiliarityReplay,
};

use aikit_tui::backend::{JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle};
use aikit_tui::host::{TerminalProfile, UiHost};
use aikit_tui::surface::{SurfaceBackend, SurfaceRequest};
use aikit_tui::tree::TreeEffect;
use aikit_tui::tree_driver::{TreeOutcome, TreeRequest};
use aikit_tui::{PaletteOutcome, PaletteRequest};

use crate::app::Service;

impl SurfaceBackend for Service {
    fn surface_tree(&self) -> Result<aikit_tui::tree::TreeState> {
        crate::tree_build::build(self)
    }

    fn apply_tree_effect(&mut self, effect: TreeEffect) -> Result<()> {
        let procedure = match effect {
            TreeEffect::CreateSet { set } => {
                aikit_store::skillsets::plan_create(self.home(), &set, &[], &[])?
            }
            TreeEffect::RenameSet { from, to } => {
                aikit_store::skillsets::plan_rename(self.home(), &from, &to)?
            }
            TreeEffect::DeleteSet { set } => {
                let (procedure, _recovery) =
                    aikit_store::skillsets::plan_delete(self.home(), &set)?;
                procedure
            }
            TreeEffect::AddToSet { set, capsule } => {
                aikit_store::skillsets::plan_add(self.home(), &set, &[capsule])?
            }
            TreeEffect::RemoveFromSet { set, capsule } => {
                aikit_store::skillsets::plan_remove(self.home(), &set, &[capsule])?
            }
            TreeEffect::Activate { .. } => {
                return Err(aikit_core::AikitError::new(
                    "tui.invalid_tree_effect",
                    "activation must be routed into the palette inside the unified surface",
                ))
            }
        };
        aikit_store::procedure::ProcedureRunner::new(self.home()).run(&procedure)?;
        self.refresh()
    }
}

/// V2 surface decorator that gives the shared Service one additional operational
/// responsibility: replaying and recording learned navigation evidence.
///
/// This stays at the host/application boundary rather than inside the semantic
/// resolver. Familiarity is rebuildable evidence, so the underlying Service and
/// every non-V2 consumer keep the same canonical resolution semantics.
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
            // Schema evolution invalidates only the learned influence. The user
            // still gets the canonical navigation field and can inspect/reset the
            // old event evidence independently.
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

impl SurfaceBackend for V2SurfaceService<'_> {
    fn surface_tree(&self) -> Result<aikit_tui::tree::TreeState> {
        <Service as SurfaceBackend>::surface_tree(self.service)
    }

    fn apply_tree_effect(&mut self, effect: TreeEffect) -> Result<()> {
        <Service as SurfaceBackend>::apply_tree_effect(self.service, effect)
    }
}

/// Build a terminal profile from an environment lookup and the `--fullscreen`
/// flag.
///
/// A very small terminal is *not* forced fullscreen here — that escalation is
/// [`UiHost::choose`]'s call, which knows the inline minimum. What this function
/// contributes is the raw facts: the size, whether a multiplexer is present, and
/// whether the user explicitly asked for fullscreen.
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

/// Best-effort terminal size: the real `COLUMNS`/`LINES` the shell exports, else a
/// conventional 80×24. The palette re-measures on its own backend at draw time;
/// this only seeds the host choice.
fn terminal_size<F>(env: &F) -> (u16, u16)
where
    F: Fn(&str) -> Option<String>,
{
    let parse = |key: &str, default: u16| {
        env(key)
            .and_then(|v| v.trim().parse::<u16>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(default)
    };
    (parse("COLUMNS", 80), parse("LINES", 24))
}

/// Open the one continuous palette/tree surface over a single live service.
pub fn run_surface(
    service: &mut Service,
    query: Option<String>,
    fullscreen: bool,
    opening_tree: bool,
) -> Result<PaletteOutcome> {
    let profile = terminal_profile(|key| std::env::var(key).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    let mut request = SurfaceRequest::new(host);
    if let Some(query) = query {
        request = request.with_query(query);
    }
    if opening_tree {
        request = request.opening_tree();
    }
    let mut backend = V2SurfaceService::new(service);
    aikit_tui::surface::run_on_terminal(&mut backend, request)
}

/// Open the palette over the service and run it to completion.
pub fn run(
    service: &mut Service,
    query: Option<String>,
    fullscreen: bool,
) -> Result<PaletteOutcome> {
    let profile = terminal_profile(|k| std::env::var(k).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    let mut request = PaletteRequest::new(host);
    if let Some(query) = query {
        request = request.with_query(query);
    }
    aikit_tui::run(service, request)
}

/// Open the exact palette row and immediately hand it to its natural action.
pub fn run_activation(
    service: &mut Service,
    query: String,
    fullscreen: bool,
) -> Result<PaletteOutcome> {
    let profile = terminal_profile(|k| std::env::var(k).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    aikit_tui::run(
        service,
        PaletteRequest::new(host)
            .with_query(query)
            .activating_initial(),
    )
}

/// Open the organising tree over the same live service as the palette.
pub fn run_tree(service: &Service, fullscreen: bool) -> Result<TreeOutcome> {
    let profile = terminal_profile(|k| std::env::var(k).ok(), fullscreen);
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
