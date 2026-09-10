//! Hosting AIKit's terminal application surface.
//!
//! Placement remains a CLI concern; semantic operation belongs to the shared V2
//! application service. Every interactive CLI entry point delegates to the same
//! reducer-native ApplicationSurface. No CLI path instantiates the retired
//! Palette/Tree semantic controllers.

use std::collections::BTreeSet;
use std::path::PathBuf;

use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::platform::MuxKind;
use aikit_core::resolve::ResolvedView;
use aikit_core::resource::{
    NavigationEvidence, NavigationEvidenceClass, ResourceDescriptor, ResourceKind, ResourceRecord,
    ResourceRef, ResourceSearchIndex,
};
use aikit_core::scope::{ScopeKind, ScopeLayer};
use aikit_core::search::SearchDoc;
use aikit_core::{FamiliarityObservation, FamiliarityStore, Result};

use aikit_store::{
    familiarity_observation_event, replay_familiarity, AikitHome, EventRecorder,
    FamiliarityReplay,
};

use aikit_tui::application::RelationView;
use aikit_tui::application_surface::ApplicationSurfaceRequest;
use aikit_tui::backend::{JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle};
use aikit_tui::host::{TerminalProfile, UiHost};
use aikit_tui::PaletteOutcome;

use crate::app::Service;

/// V2 surface decorator that gives the shared Service two application-boundary
/// responsibilities which must not leak into the deterministic capsule resolver:
/// rebuilding learned navigation evidence and advertising real compositional
/// Resources already selected by the loaded Project/scope stack.
///
/// It delegates every resolver/package/runtime operation to the existing Service;
/// this is not a second application service or semantic store.
struct V2SurfaceService<'a> {
    service: &'a mut Service,
}

impl<'a> V2SurfaceService<'a> {
    fn new(service: &'a mut Service) -> Self {
        Self { service }
    }

    fn composition_navigation_index(&self) -> ResourceSearchIndex {
        let mut index = <Service as PaletteBackend>::navigation_index(self.service);

        // Profiles are already part of the authoritative ordered scope stack.
        // Advertising them as V2 Resources makes that existing composition intent
        // selectable/searchable without turning ProfileId into a CapsuleId.
        let mut profiles = BTreeSet::new();
        if let Some(layers) = <Service as PaletteBackend>::scope_layers(self.service) {
            for layer in layers {
                profiles.extend(layer.patch.profiles.iter().cloned());
                profiles.extend(layer.patch.uses.iter().map(|used| used.profile.clone()));
            }
        }
        for profile in profiles {
            if let Ok(id) = ResourceRef::parse(profile.to_string()) {
                let record = ResourceRecord::new(ResourceDescriptor::new(
                    id,
                    ResourceKind::Profile,
                    profile.to_string(),
                    "Profile selected by the resolved scope composition",
                ));
                index.insert_resource(
                    record,
                    vec![NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)
                        .with_detail("profile selected by the active scope stack")],
                );
            }
        }

        // Project Skill Sets are authored project composition, not hidden package
        // activation. Keep their own Resource identity so human and agent views can
        // inspect the same selection without manufacturing Capability/Capsule ids.
        for name in self.service.project_skill_sets() {
            let Ok(id) = ResourceRef::parse(format!("skill-set/{name}")) else {
                continue;
            };
            let record = ResourceRecord::new(ResourceDescriptor::new(
                id,
                ResourceKind::SkillSet,
                name.clone(),
                "Skill Set selected by the current Project",
            ));
            index.insert_resource(
                record,
                vec![NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)
                    .with_detail("skill set selected by the current Project")],
            );
        }

        index
    }
}

impl PaletteBackend for V2SurfaceService<'_> {
    fn context(&self) -> &aikit_core::ContextDescriptor {
        <Service as PaletteBackend>::context(self.service)
    }

    fn view(&self) -> &ResolvedView {
        <Service as PaletteBackend>::view(self.service)
    }

    /// Forwarded so the interactive surface can see the same native owner
    /// identity the CLI's own commands already resolve through `Service`.
    /// Before this forward existed the trait default (`Ok(None)`) answered
    /// here instead, which is indistinguishable from "no ProjectCentral
    /// identity is bound" — a silent downgrade this decorator must not
    /// introduce.
    fn project_binding(&self) -> Result<Option<aikit_core::project::ProjectBinding>> {
        <Service as PaletteBackend>::project_binding(self.service)
    }

    /// Forwarded for the same reason as `project_binding`: `Service` already
    /// observes real Git material through `NativeGitProvider`, and this
    /// decorator's job is to carry every resolver/package/runtime answer
    /// through unchanged, not to re-decide which of them the interactive
    /// surface is allowed to see. Leaving this one unforwarded is exactly the
    /// wiring gap that made the Worlds pane's Git section permanently absent.
    fn versioned_world(
        &self,
    ) -> Result<Option<aikit_core::resource::VersionedProjectWorld>> {
        <Service as PaletteBackend>::versioned_world(self.service)
    }

    fn scope_layers(&self) -> Option<&[ScopeLayer]> {
        <Service as PaletteBackend>::scope_layers(self.service)
    }

    fn application_home(&self) -> Option<&AikitHome> {
        Some(self.service.home())
    }

    fn documents(&self) -> Vec<SearchDoc> {
        <Service as PaletteBackend>::documents(self.service)
    }

    fn context_resource_records(&self) -> Result<Vec<ResourceRecord>> {
        self.service.context_resource_records()
    }

    fn navigation_index(&self) -> ResourceSearchIndex {
        self.composition_navigation_index()
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
/// `opening_tree` is retained as a user-facing CLI option, but its meaning is
/// "open Knowledge with the Tree projection of the one relation read model". It
/// does not instantiate TreeState or a separate tree controller.
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

/// Compatibility helper for callers that historically asked to "open the
/// palette". The behavior is now exactly the final ApplicationSurface.
pub fn run(
    service: &mut Service,
    query: Option<String>,
    fullscreen: bool,
) -> Result<PaletteOutcome> {
    run_surface(service, query, fullscreen, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git is available in the test environment");
        assert!(status.success(), "git {args:?} failed");
    }

    /// A real repository with a real ProjectCentral identity, the same
    /// minimal fixture `versioned_world_wiring.rs` uses for `Service` itself
    /// -- `.aikit` is what makes the service recognise a Project at all, and
    /// `ProjectCentral/project.json` is what gives it a native owner identity
    /// `versioned_world`/`project_binding` refuse to observe without.
    fn project(root: &Path) {
        std::fs::create_dir_all(root.join(".aikit")).unwrap();
        std::fs::write(root.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
        std::fs::create_dir_all(root.join("ProjectCentral")).unwrap();
        std::fs::write(
            root.join("ProjectCentral/project.json"),
            r#"{"schema":"central.project/v1","project_id":"project:surface-probe","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#,
        )
        .unwrap();
        git(root, &["init", "--initial-branch=trunk"]);
        git(root, &["config", "user.email", "probe@example.invalid"]);
        git(root, &["config", "user.name", "probe"]);
        std::fs::write(root.join("README.md"), "probe\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "first"]);
    }

    fn service(home: &Path, root: &Path) -> Service {
        let mut env = BTreeMap::new();
        env.insert(
            "AIKIT_CONTEXT_ID".to_owned(),
            aikit_core::ContextId::generate().to_string(),
        );
        Service::open(AikitHome::at(home), root, |key| env.get(key).cloned()).unwrap()
    }

    /// The wiring proof this module exists to guard: every interactive entry
    /// point opens the terminal application through `V2SurfaceService`, not
    /// through `Service` directly, so a forward that only exists on
    /// `Service` never reaches the TUI. Before `project_binding` and
    /// `versioned_world` were added to this `impl PaletteBackend for
    /// V2SurfaceService`, calling them on the decorator silently ran the
    /// `PaletteBackend` trait default (`Ok(None)`) instead of `Service`'s
    /// real observation -- indistinguishable, at the type level, from "this
    /// Project has no Git material". This test drives the decorator itself
    /// against a real repository made by real `git`, the same discipline
    /// `versioned_world_wiring.rs` uses one layer down for `Service` alone.
    #[test]
    fn the_surface_decorator_forwards_real_git_material_not_the_trait_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("probe");
        std::fs::create_dir_all(&root).unwrap();
        project(&root);

        let mut svc = service(tmp.path(), &root);
        let backend = V2SurfaceService::new(&mut svc);

        let observed = backend
            .versioned_world()
            .expect("observation does not fail on a real worktree");
        let versioned = observed.expect(
            "the decorator must forward Service's real observation, not the \
             PaletteBackend trait default of None",
        );
        assert_eq!(versioned.repository.branch.as_deref(), Some("trunk"));
        assert!(versioned.working.is_clean(), "a fresh commit leaves a clean tree");

        let binding = backend
            .project_binding()
            .expect("binding observation does not fail")
            .expect(
                "the decorator must forward Service's real ProjectBinding, not the \
                 PaletteBackend trait default of None",
            );
        assert_eq!(binding.project.as_str(), "project:surface-probe");
    }
}
