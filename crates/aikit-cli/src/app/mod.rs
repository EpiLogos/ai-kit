//! The one application service the CLI and the palette both talk to.
//!
//! ARCHITECTURE.md §11 is explicit: "CLI and TUI share **one** application
//! service. The TUI never shells out to `aikit --json` internally." This module
//! is that service. [`Service`] loads a real catalogue off disk, assembles the
//! scope-layer stack for the current context, asks `aikit-core` to resolve it,
//! and then answers questions about the result — first through the CLI-facing
//! [`AikitApplication`] trait, and then, over the very same state, through
//! [`aikit_tui::PaletteBackend`].
//!
//! Nothing here re-implements a resolver rule. Every "what would happen if…" goes
//! back through [`aikit_core::resolve`], so `aikit explain`, `aikit status` and
//! the palette can never disagree about the same system. The projection
//! *questions* ("what would Codex get in a shared tree?") are likewise answered
//! by the real adapters in `aikit-adapters`, not by a copy of their logic here.

use std::path::{Path, PathBuf};

use aikit_core::capsule::{Capsule, Kind};
use aikit_core::catalog::Catalog;
use aikit_core::continuity::ContinuityTuning;
use aikit_core::context::ContextDescriptor;
use aikit_core::id::{CapsuleId, GenerationId, SessionId};
use aikit_core::platform::TargetId;
use aikit_core::policy::ManagedPolicy;
use aikit_core::profile::SkillUsageOverlayPatch;
use aikit_core::projection::{
    ActivationEffect, ProjectionItem, ProjectionPlan, ResolvedContext, TargetAdapter,
};
use aikit_core::resolve::{resolve_diagnostic, ResolveRequest as CoreResolveRequest, ResolvedView};
use aikit_core::scope::{LayerOrigin, ScopeKind, ScopeLayer};
use aikit_core::search::SearchDoc;
use aikit_core::trust::TrustOracle;
use aikit_core::{AikitError, Result};

use aikit_store::edit::{OverlayDocument, ProfileDocument};
use aikit_store::generation::{self, GenerationBuilder};
use aikit_store::home::AikitHome;
use aikit_store::index::Index;
use aikit_store::registry::{load_project_local, load_registry, RegistryProblem, Snapshot};
use aikit_store::trust::{TrustSnapshot, TrustStore};
use aikit_store::SessionSpaceApplicationStore;

use aikit_adapters::actor_composition::compose_live_actor_inputs;
use aikit_adapters::clients::agent_skills;
use aikit_adapters::clients::aider::AiderAdapter;
use aikit_adapters::clients::antigravity::AntigravityAdapter;
use aikit_adapters::clients::broker::BrokerAdapter;
use aikit_adapters::clients::claude::ClaudeAdapter;
use aikit_adapters::clients::codex::CodexAdapter;
use aikit_adapters::clients::cursor::CursorAdapter;
use aikit_adapters::clients::dsh::DshAdapter;
use aikit_adapters::clients::gemini::GeminiAdapter;
use aikit_adapters::clients::goose::GooseAdapter;
use aikit_adapters::clients::grokbot::GrokbotAdapter;
use aikit_adapters::clients::kimi::KimiAdapter;
use aikit_adapters::clients::ollama::OllamaAdapter;
use aikit_adapters::clients::openclaw::OpenclawAdapter;
use aikit_adapters::clients::opencode::OpencodeAdapter;
use aikit_adapters::clients::pi::PiAdapter;
use aikit_adapters::clients::qwen::QwenAdapter;
use aikit_adapters::clients::zcode::ZcodeAdapter;
use aikit_adapters::runner::SystemRunner;
use aikit_adapters::factory_developmental::{
    read_factory_developmental, start_factory_work, FactoryDevelopmentalBinding,
};

use aikit_core::working_environment::WorkingEnvironmentObservation;
use aikit_tui::live_field::{WorkingEnvironmentOperation, WorkingEnvironmentOutcome};
use aikit_tui::backend::{
    ClientEffect, FactoryWorkEntry, FactoryWorkStartReceipt, JobOutput, PaletteBackend, Projected,
    PromotionDraft, RunIntent, Toggle,
};
pub use aikit_tui::staging::StagedDiff;

use crate::discover::{self, DiscoveredProject};
use crate::run::{self, RunReport};
use crate::temporal::process_central_root;

mod development_field;
mod flow_cognition;
mod knowledge;

pub use development_field::DevelopmentFieldApplicationRequest;
pub use flow_cognition::{
    FlowChangedSinceReceipt, FlowContemplateBasis, FlowContemplateReceipt, FlowPreflightOutcome,
};

// ---------------------------------------------------------------------------
// Request / response types for the CLI-facing trait
// ---------------------------------------------------------------------------

/// A catalogue search.
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub limit: usize,
}

/// One row of a search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub id: CapsuleId,
    pub name: String,
    pub kind: Kind,
    pub active: bool,
    pub runnable: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub rows: Vec<SearchHit>,
    pub warnings: Vec<String>,
}

/// Re-resolve the view with a set of pending toggles layered on top.
#[derive(Debug, Clone, Default)]
pub struct ResolveRequest {
    pub scope: Option<ScopeKind>,
    pub toggles: Vec<Toggle>,
}

/// Preview the diff a set of toggles would produce, without writing.
#[derive(Debug, Clone)]
pub struct StageRequest {
    pub scope: ScopeKind,
    pub toggles: Vec<Toggle>,
}

/// Commit a set of toggles at a scope into a new generation.
///
/// There is no `strict` flag: the compare-and-swap against the base generation is
/// unconditional and always on. It lives in the store (`generation::commit`,
/// which returns `generation.stale_base` if `current` moved), so a flag here
/// could only ever have toggled a guarantee that is not the caller's to weaken.
#[derive(Debug, Clone)]
pub struct ApplyRequest {
    pub scope: ScopeKind,
    pub toggles: Vec<Toggle>,
    /// A cosmetic label to attach to the resulting generation. Excluded from the
    /// generation's content identity, so it never forces a new one.
    pub label: Option<String>,
}

/// The result of a successful apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedGeneration {
    pub id: GenerationId,
    pub replaced: Option<GenerationId>,
    pub warnings: Vec<String>,
    pub effects: Vec<ClientEffect>,
}

/// Run an exported command name or a capability id once.
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub name: String,
    pub args: Vec<String>,
    /// The specific export the invocation was made under, when known (the
    /// multicall path and the generated shims both know it).
    pub export: Option<String>,
    /// Skip the run-confirmation gate for an unreviewed script.
    pub confirmed: bool,
}

/// What a run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunHandle {
    pub capsule: CapsuleId,
    pub report: RunReport,
}

/// Bring up a session topology.
#[derive(Debug, Clone, Default)]
pub struct SessionRequest {
    pub spec: Option<String>,
}

/// What a running session differs from its spec in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDiffOutcome {
    pub session: String,
    pub mux: String,
    pub differences: Vec<String>,
    pub warnings: Vec<String>,
}

/// What a reconcile changed, and what it deliberately left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReconcileOutcome {
    pub session: String,
    pub mux: String,
    pub actions: Vec<String>,
    /// Panes left as they were, with the reason — a hand-split pane is somebody's
    /// work, not drift to be corrected.
    pub preserved: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResult {
    pub session: String,
    pub mux: String,
    pub created: bool,
    pub actions: Vec<String>,
    pub preserved: Vec<String>,
    pub summary: String,
    pub warnings: Vec<String>,
}

/// Promote a captured candidate into a capsule.
#[derive(Debug, Clone)]
pub struct PromoteRequest {
    pub candidate: String,
    pub id: Option<CapsuleId>,
}

pub use aikit_store::inbox::PromotedCapsule;

// ---------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------

/// The application service, as the CLI needs it.
///
/// Deliberately the same seven verbs the palette's flow needs, so the two
/// front-ends are demonstrably driving one engine rather than two that happen to
/// agree today.
pub trait AikitApplication {
    fn search(&self, r: SearchRequest) -> Result<SearchResults>;
    fn resolve(&self, r: ResolveRequest) -> Result<ResolvedView>;
    fn stage(&self, r: StageRequest) -> Result<StagedDiff>;
    fn apply(&mut self, r: ApplyRequest) -> Result<AppliedGeneration>;
    fn run(&mut self, r: RunRequest) -> Result<RunHandle>;
    fn session_up(&mut self, r: SessionRequest) -> Result<SessionResult>;
    fn promote(&mut self, r: PromoteRequest) -> Result<PromotedCapsule>;
}

// ---------------------------------------------------------------------------
// The service
// ---------------------------------------------------------------------------

/// A loaded, resolved view of one context, backed by the real store.
pub struct Service {
    home: AikitHome,
    index: Index,
    catalog: Snapshot,
    problems: Vec<RegistryProblem>,
    descriptor: ContextDescriptor,
    project: Option<DiscoveredProject>,
    layers: Vec<ScopeLayer>,
    trust: TrustSnapshot,
    policy: ManagedPolicy,
    view: ResolvedView,
    invocation_cwd: PathBuf,
    knowledge_runtime: std::cell::RefCell<Option<knowledge::KnowledgeRuntime>>,
    factory_executable: PathBuf,
    factory_state: Option<PathBuf>,
    factory_project_ref: Option<String>,
    factory_request_file: Option<PathBuf>,
    /// Owner observations returned by a Factory Commission in this running
    /// application. This is an ephemeral read cache, not an AIKit Factory
    /// store; restarting re-observes through the configured owner binding.
    factory_started_resources: Option<Vec<aikit_core::resource::ResourceRecord>>,
    /// Working-environment observation cache.
    ///
    /// Observing a mux runs real subprocesses. Contextual Actions are loaded on
    /// every selection change, so an un-cached observation here would put a
    /// `tmux list-sessions` behind every arrow key — the exact shape of the
    /// input-responsiveness defect. Observe once, and re-observe only after an
    /// operation this application performed changed the host.
    working_environments: std::cell::RefCell<Option<Vec<WorkingEnvironmentObservation>>>,
}

impl Service {
    /// Discover everything from the current working directory and process
    /// environment, and resolve the view.
    pub fn discover(cwd: &Path) -> Result<Self> {
        let home = AikitHome::discover()?;
        Self::open(home, cwd, |k| std::env::var(k).ok())
    }

    /// Resolve a palette outcome without discarding any of the reviewed intent.
    ///
    /// The capsule supplies the executable entry point. The intent supplies the
    /// selected mode, working-directory policy and environment, including
    /// interactive overrides such as Alt+Enter. Re-planning from only capsule
    /// id and argv would silently replace those choices with manifest defaults.
    pub fn plan_run_intent(&self, intent: &RunIntent) -> Result<run::ScriptCommand> {
        if intent.context != self.descriptor.context_id {
            return Err(AikitError::new(
                "run.stale_context",
                format!(
                    "the invocation was prepared for context {}, but the active context is {}",
                    intent.context, self.descriptor.context_id
                ),
            )
            .with("expected_context", intent.context.to_string())
            .with("actual_context", self.descriptor.context_id.to_string()));
        }
        let capsule = self.catalog.get(&intent.capsule).ok_or_else(|| {
            AikitError::new(
                "run.unknown_command",
                format!("{} is not loaded", intent.capsule),
            )
        })?;
        let args = intent.argv()?;
        let project_root = self.descriptor.project_root.as_deref();
        let mut command = run::plan_script(capsule, &args, project_root, &self.invocation_cwd)?;
        command.mode = intent.mode;
        command.cwd = match intent.cwd {
            aikit_core::capsule::WorkingDir::Project => {
                project_root.unwrap_or(&self.invocation_cwd).to_path_buf()
            }
            aikit_core::capsule::WorkingDir::Cwd => self.invocation_cwd.clone(),
            aikit_core::capsule::WorkingDir::Capsule => capsule.root.clone().ok_or_else(|| {
                AikitError::new(
                    "run.source_missing",
                    format!("{} has no payload on this machine", capsule.id),
                )
            })?,
        };
        command.env = intent.env.clone();
        Ok(command)
    }

    /// The on-disk declaration that supplied a loaded profile, following the
    /// same registry ordering as catalogue loading.
    pub fn profile_source(&self, id: &aikit_core::ProfileId) -> Option<PathBuf> {
        let relative = PathBuf::from("profiles")
            .join(id.path())
            .with_extension("toml");
        let mut found = None;
        if let Ok(entries) = std::fs::read_dir(self.home.registries()) {
            let mut names: Vec<String> = entries
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect();
            names.sort();
            for name in names {
                let candidate = self.home.registry(&name).join(&relative);
                if candidate.is_file() {
                    found = Some(candidate);
                }
            }
        }
        if let Some(root) = self.descriptor.project_root.as_ref() {
            let candidate = root.join(".aikit").join(relative);
            if candidate.is_file() {
                found = Some(candidate);
            }
        }
        found
    }

    /// The injectable form: an explicit home and environment lookup, so tests run
    /// against a real temp home without touching the process environment.
    pub fn open<F>(home: AikitHome, cwd: &Path, env: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        home.ensure_layout()?;
        let default_store = env("HOME").map(|path| PathBuf::from(path).join(".aikit"));
        let additional_stores: Vec<&Path> = default_store.as_deref().into_iter().collect();
        let project =
            discover::discover_project_with_home_excluding(&home, cwd, &additional_stores)?;
        let project_root = project.as_ref().map(|p| p.root.clone());

        let descriptor = match &project_root {
            Some(root) => discover::descriptor_from(root, &env),
            None => discover::global_descriptor(&env),
        };

        let load = load_catalog(&home, project_root.as_deref())?;
        let catalog = load.catalog;
        let problems = load.problems;

        let index = Index::open(&home.database())?;
        let trust = TrustStore::new(&index).snapshot()?;
        let policy = ManagedPolicy::default();

        let layers = assemble_layers(&home, &descriptor, project.as_ref())?;
        let view = resolve_or_explain(&catalog, &trust, &descriptor, &layers, &policy)?;
        let factory_executable = env("AIKIT_FACTORY_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("factory"));
        let factory_state = env("AIKIT_FACTORY_STATE").map(PathBuf::from);
        let factory_project_ref = env("AIKIT_FACTORY_PROJECT_REF");
        let factory_request_file = env("AIKIT_FACTORY_REQUEST_FILE").map(PathBuf::from);

        Ok(Self {
            home,
            index,
            catalog,
            problems,
            descriptor,
            project,
            layers,
            trust,
            policy,
            view,
            invocation_cwd: cwd.to_path_buf(),
            knowledge_runtime: std::cell::RefCell::new(None),
            factory_executable,
            factory_state,
            factory_project_ref,
            factory_request_file,
            factory_started_resources: None,
            working_environments: std::cell::RefCell::new(None),
        })
    }

    /// What a running session differs from its spec in.
    ///
    /// Read-only: it compares and reports. Changing a live session is
    /// `reconcile`, which is a separate verb precisely so a diff can be run
    /// without wondering whether it moved anything.
    pub fn session_diff(&self, requested: Option<&str>) -> Result<SessionDiffOutcome> {
        let plan = self.diff_or_reconcile_plan(requested)?;
        let name = plan.name.clone();
        let (stack, _) = self.session_stack(&plan)?;
        if !stack.session_exists(&plan)? {
            return Ok(SessionDiffOutcome {
                session: name.clone(),
                mux: stack.topology_kind().as_str().into(),
                differences: vec![format!("session `{name}` is not running")],
                warnings: stack.warnings(),
            });
        }
        let binding = stack.inspect_session(&plan)?;
        Ok(SessionDiffOutcome {
            session: name,
            mux: stack.topology_kind().as_str().into(),
            differences: binding.actions.clone(),
            warnings: binding.warnings,
        })
    }

    /// Bring a running session towards its spec.
    pub fn session_reconcile(
        &self,
        requested: Option<&str>,
        destructive: bool,
    ) -> Result<SessionReconcileOutcome> {
        use aikit_adapters::mux::ReconcileMode;
        let plan = self.diff_or_reconcile_plan(requested)?;
        let name = plan.name.clone();
        let (stack, _) = self.session_stack(&plan)?;
        // Non-destructive unless asked: the default may only ever ADD, so a
        // reconcile can never close the pane somebody is working in.
        let mode = if destructive {
            ReconcileMode::Exact
        } else {
            ReconcileMode::CreateOrAttach
        };
        let binding = stack.ensure_session(&plan, mode)?;
        Ok(SessionReconcileOutcome {
            session: name,
            mux: stack.topology_kind().as_str().into(),
            actions: binding.actions,
            preserved: binding.preserved,
            warnings: binding.warnings,
        })
    }

    /// The session name a command is talking about.
    fn session_name(&self, session: Option<&str>) -> Result<String> {
        match session {
            Some(name) => Ok(name.to_string()),
            None => self
                .descriptor
                .project_root
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .ok_or_else(|| {
                    AikitError::new(
                        "session.unnamed",
                        "no session was named and the working directory is not inside a project",
                    )
                }),
        }
    }

    /// The compiled topology for a session: a `session` capsule if one is active,
    /// else a single-pane plan named for the project.
    fn session_plan(&self, session: Option<&str>) -> Result<aikit_core::SessionPlan> {
        use aikit_core::session::SessionSpec;
        let name = self.session_name(session)?;

        // Prefer a real session capsule when the context has one active.
        for capability in self.view.active_of_kind(Kind::Session) {
            if let Some(capsule) = self.catalog.get(&capability.id) {
                if let (Some(section), Some(root)) = (capsule.session(), capsule.root.as_ref()) {
                    let path = root.join(&section.spec);
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        return SessionSpec::from_toml_str(&text)?.compile();
                    }
                }
            }
        }

        SessionSpec::from_toml_str(&format!(
            "schema = 1\nid = \"{name}\"\nname = \"{name}\"\n\n[[views]]\nid = \"main\"\n[[views.panes]]\nid = \"shell\"\n"
        ))?
        .compile()
    }

    fn requested_session_plan(&self, requested: Option<&str>) -> Result<aikit_core::SessionPlan> {
        use aikit_core::session::SessionSpec;

        let Some(requested) = requested else {
            return self.session_plan(None);
        };
        let candidate = PathBuf::from(requested);
        let candidate = if candidate.is_absolute() {
            candidate
        } else {
            self.invocation_cwd.join(candidate)
        };
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).map_err(|error| {
                AikitError::new(
                    "session.spec_unreadable",
                    format!("could not read {}: {error}", candidate.display()),
                )
                .with("path", candidate.display().to_string())
            })?;
            return SessionSpec::from_toml_str(&text)?.compile();
        }

        let id = CapsuleId::parse(requested).map_err(|_| {
            AikitError::new(
                "session.unknown_spec",
                format!("`{requested}` is neither a readable session spec nor a capsule id"),
            )
            .with("spec", requested.to_string())
        })?;
        let capsule = self.catalog.get(&id).ok_or_else(|| {
            AikitError::new(
                "session.unknown_spec",
                format!("session capsule `{id}` is not loaded"),
            )
            .with("spec", requested.to_string())
        })?;
        let section = capsule.session().ok_or_else(|| {
            AikitError::new(
                "session.wrong_kind",
                format!("`{id}` is not a session capsule"),
            )
        })?;
        let root = capsule.root.as_ref().ok_or_else(|| {
            AikitError::new(
                "session.spec_unreadable",
                format!("session capsule `{id}` has no payload root"),
            )
        })?;
        let path = root.join(&section.spec);
        let text = std::fs::read_to_string(&path).map_err(|error| {
            AikitError::new(
                "session.spec_unreadable",
                format!("could not read {}: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        SessionSpec::from_toml_str(&text)?.compile()
    }

    /// Diff and reconcile historically accepted a bare human session name while
    /// `up` accepted a spec path or session capsule. Preserve the useful named
    /// fallback, but resolve anything that is actually a path or capsule through
    /// the same compiler as `session up`.
    fn diff_or_reconcile_plan(&self, requested: Option<&str>) -> Result<aikit_core::SessionPlan> {
        let Some(requested) = requested else {
            return self.session_plan(None);
        };
        let path = PathBuf::from(requested);
        let candidate = if path.is_absolute() {
            path.clone()
        } else {
            self.invocation_cwd.join(&path)
        };
        let looks_like_path =
            candidate.is_file() || path.extension().is_some() || path.components().count() > 1;
        if looks_like_path || CapsuleId::parse(requested).is_ok() {
            self.requested_session_plan(Some(requested))
        } else {
            self.session_plan(Some(requested))
        }
    }

    fn session_stack(
        &self,
        plan: &aikit_core::SessionPlan,
    ) -> Result<(aikit_adapters::mux::stack::MuxStack, SessionId)> {
        use aikit_adapters::mux::{
            cmux::Cmux, plain::Plain, stack::MuxStack, tmux::Tmux, SessionIdentity,
        };
        use aikit_store::state::StateStore;

        let mux = match plan.mux.or(self.descriptor.mux) {
            Some(kind) => kind,
            None => crate::mux_install::choose_installed(None)?,
        };
        let state = StateStore::new(&self.index);
        let existing = state.sessions()?.into_iter().rev().find(|record| {
            record.name == plan.name
                && record.mux == mux
                && record.project_marker.as_ref() == self.descriptor.project_id.as_ref()
                && record.state != aikit_store::state::SessionState::Closed
        });
        let session_id = self
            .descriptor
            .session_id
            .clone()
            .or_else(|| existing.map(|record| record.session_id))
            .unwrap_or_else(SessionId::generate);
        let identity = SessionIdentity {
            session_id: Some(session_id.clone()),
            context_id: Some(self.descriptor.context_id.clone()),
            project_root: self.descriptor.project_root.clone(),
            view_root: Some(self.context_projection_root().join("current")),
            profile: None,
            isolation: self.descriptor.isolation,
        };
        let stack = MuxStack::detect(
            vec![
                Box::new(Cmux::system().with_identity(identity.clone())),
                Box::new(Tmux::system().with_identity(identity)),
                Box::new(Plain::new()),
            ],
            Some(mux),
        )?;
        Ok((stack, session_id))
    }

    /// The AIKit home, for commands that plan Procedures or reach the inbox.
    pub fn home(&self) -> &AikitHome {
        &self.home
    }

    /// Reload catalogue, trust, layers, and resolution without changing context.
    ///
    /// The unified surface keeps one service allocation alive while tree
    /// Procedures change files underneath it. Re-discovering through process
    /// environment would lose injected homes in tests; refreshing from the
    /// service's own roots keeps that identity stable.
    pub fn refresh(&mut self) -> Result<()> {
        let project_root = self.descriptor.project_root.as_deref();
        let load = load_catalog(&self.home, project_root)?;
        self.catalog = load.catalog;
        self.problems = load.problems;
        self.trust = TrustStore::new(&self.index).snapshot()?;
        self.layers = assemble_layers(&self.home, &self.descriptor, self.project.as_ref())?;
        self.view = resolve_or_explain(
            &self.catalog,
            &self.trust,
            &self.descriptor,
            &self.layers,
            &self.policy,
        )?;
        self.invalidate_knowledge_runtime();
        Ok(())
    }

    /// Where this context's client projections are materialised.
    pub fn context_projection_root(&self) -> PathBuf {
        self.home
            .context_dir(&self.descriptor.context_id)
            .join("current")
    }

    pub fn invocation_cwd(&self) -> &Path {
        &self.invocation_cwd
    }

    /// Reference a profile from a scope's declaration and apply.
    ///
    /// Writing the reference and re-resolving are one act: a declaration that is
    /// written but never resolved would leave `status` disagreeing with the file.
    pub fn use_profile(
        &mut self,
        profile: &aikit_core::id::ProfileId,
        scope: ScopeKind,
    ) -> Result<AppliedGeneration> {
        {
            let mut writer = self.scope_document(scope)?;
            writer.use_profile(profile);
            writer.save()?;
        }
        AikitApplication::apply(
            self,
            ApplyRequest {
                scope,
                toggles: vec![],
                label: None,
            },
        )
    }

    pub fn set_skill_usage_overlay(
        &mut self,
        id: &CapsuleId,
        scope: ScopeKind,
        overlay: &SkillUsageOverlayPatch,
    ) -> Result<AppliedGeneration> {
        {
            let mut writer = self.scope_document(scope)?;
            writer.set_skill_overlay(id, overlay);
            writer.save()?;
        }
        AikitApplication::apply(
            self,
            ApplyRequest {
                scope,
                toggles: vec![],
                label: None,
            },
        )
    }

    pub fn clear_skill_usage_overlay(
        &mut self,
        id: &CapsuleId,
        scope: ScopeKind,
    ) -> Result<AppliedGeneration> {
        {
            let mut writer = self.scope_document(scope)?;
            writer.clear_skill_overlay(id);
            writer.save()?;
        }
        AikitApplication::apply(
            self,
            ApplyRequest {
                scope,
                toggles: vec![],
                label: None,
            },
        )
    }

    /// The operational index, for commands that read or write the store's own
    /// records (the inbox channel, collate's conflict reports).
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// `[secrets]` projection items for every active capsule that declares
    /// them, read from the same `capsule_roots` the adapters project payloads
    /// from. A capsule with no root contributes nothing — a `[secrets]` table
    /// lives only in a manifest, and a capsule without a root has no manifest
    /// surface. A manifest that exists but cannot be read or parsed is a loud
    /// error: a declared secret that silently never exports is the exact
    /// failure this feature exists to prevent.
    pub fn secret_env_items(&self, context: &ResolvedContext) -> Result<Vec<ProjectionItem>> {
        let mut items = Vec::new();
        for id in self.view.active.keys() {
            let Some(root) = context.capsule_roots.get(id) else {
                continue;
            };
            let manifest = root.join(aikit_store::registry::MANIFEST_FILE);
            if !manifest.exists() {
                continue;
            }
            let text = std::fs::read_to_string(&manifest).map_err(|e| {
                AikitError::new(
                    "context.secret_capsule_unloadable",
                    format!("could not re-read the manifest of {id}: {e}"),
                )
            })?;
            let capsule = Capsule::from_toml_str(&text)?;
            for (name, secret_ref) in &capsule.secrets {
                items.push(ProjectionItem::secret_env(name.clone(), secret_ref.clone())?);
            }
        }
        items.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        Ok(items)
    }

    pub fn descriptor(&self) -> &ContextDescriptor {
        &self.descriptor
    }

    pub fn project_specification(&self) -> Option<&str> {
        self.project.as_ref()?.specification.as_deref()
    }

    pub fn project_skill_sets(&self) -> &[String] {
        self.project
            .as_ref()
            .map(|project| project.skill_sets.as_slice())
            .unwrap_or(&[])
    }

    /// Join the canonical Model catalogue against the route availability this
    /// detection run observed, and lay the result onto the resolution.
    ///
    /// Direction is the whole point: catalogue -> availability -> selection.
    /// The catalogue supplies identity (first-party seed, owner entries layered
    /// over it); Actuation supplies whether any route to that identity is
    /// actually there. Nothing here mints a Model from what happens to be
    /// installed, and nothing reads usage telemetry.
    fn join_model_routes(
        &self,
        resolution: &mut aikit_core::ContextResolution,
        detection: &aikit_adapters::actuation_harness_detection::DetectionOutcome,
    ) -> Vec<String> {
        let (catalogue, mut notes) = aikit_store::model_catalogue::resolved_catalogue(&self.home);

        // Availability comes from two independent kinds of evidence, and they
        // stay distinguishable: what Actuation detected on this machine, and
        // what a Provider Source published. Neither is allowed to stand in for
        // the other, and neither mints identity.
        let (mut observed, detection_notes) =
            aikit_adapters::actuation_model_routes::observed_provider_models(detection);
        notes.extend(detection_notes);

        let (documents, problems) =
            aikit_store::model_catalogue::load_provider_catalogs(&self.home);
        notes.extend(problems);
        for document in documents {
            let outcome =
                aikit_adapters::provider_catalog_source::ProviderCatalogOutcome::Observed {
                    observations: document.observations,
                    source: document.source,
                    observed_at: document.observed_at,
                };
            observed.extend(
                aikit_adapters::provider_catalog_source::observed_router_routes(&outcome),
            );
        }

        // Harness workability: a harness that is actually installed here and
        // declares which provider it dispatches to is evidence that the
        // provider is reachable from this machine. It is the only evidence a
        // hosted provider can have short of calling its API with a key.
        let capabilities = aikit_adapters::actuation_harness_detection
            ::intake_actuation_capabilities(&SystemRunner::new(), "actuation");
        let (reachable, reach_notes) =
            aikit_adapters::actuation_model_routes::harness_provider_reachability(
                detection,
                &capabilities,
            );
        notes.extend(reach_notes);
        if !reachable.is_empty() {
            notes.push(format!(
                "harness dispatch reaches: {}",
                reachable
                    .iter()
                    .map(|reach| format!("{} via {}", reach.provider, reach.through))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        // Credentials qualify usability and nothing else. Only the fact that a
        // binding is recorded is read; no secret is materialised or carried.
        let credentials = match aikit_store::credentials::CredentialBindingStore::new(&self.home)
            .list()
        {
            Ok(bindings) => aikit_adapters::actuation_model_routes::CredentialEvidence::from_binding_refs(
                bindings
                    .into_iter()
                    .filter(|binding| !binding.revoked)
                    .map(|binding| binding.credential_ref.as_str().to_string()),
            ),
            Err(error) => {
                notes.push(format!(
                    "credential bindings unreadable ({error}) — observed routes are reported \
                     without their credential state, never as usable"
                ));
                aikit_adapters::actuation_model_routes::CredentialEvidence::default()
            }
        };
        if !credentials.is_empty() {
            notes.push(format!(
                "credential bindings observed for: {}",
                credentials.providers().join(", ")
            ));
        }

        let join = aikit_adapters::actuation_model_routes::join_model_routes_with_reach(
            &catalogue,
            &observed,
            &reachable,
            &credentials,
        );
        notes.extend(join.notes.clone());
        // Catalogue-joined Models are the model candidates. An authored Model
        // already in the index keeps its record; the join adds the rest.
        let known: Vec<String> = resolution
            .model_candidates
            .iter()
            .map(|candidate| candidate.resource.descriptor.id.to_string())
            .collect();
        for model in join.models {
            if !known
                .iter()
                .any(|id| id == &model.resource.descriptor.id.to_string())
            {
                resolution.model_candidates.push(model);
            }
        }
        resolution.model_routes = join.route_sets;
        resolution.unmatched_model_offers = join.unmatched.clone();
        for offer in &join.unmatched {
            notes.push(format!(
                "unmatched provider offer: {} from {} — {}",
                offer.provider_native_id, offer.provider, offer.reason
            ));
        }
        notes
    }

    /// Actualise a selected Model through Actuation, keeping its routes plural
    /// right up to the boundary.
    ///
    /// The whole chain in one place, in its own direction: the composed
    /// resolution already carries the catalogue↔availability join, so this
    /// selects a Model out of it (never a provider), ranks its viable routes,
    /// asks Workcell for a material body only where one is actually needed,
    /// and hands the first workable route to Actuation. Actuation re-runs live
    /// detection and applies its own evidence gate; a refusal comes back as a
    /// refusal, never as a success with a missing field.
    pub fn realise_model(
        &self,
        composed: &serde_json::Value,
        model: &str,
        provider: Option<&str>,
    ) -> Result<serde_json::Value> {
        use aikit_adapters::model_realisation::{
            material_body_plan, realise, MaterialBodyOutcome, RealisationOutcome,
            RealisationRequest,
        };
        use aikit_core::resource::{
            canonical_model_ref, candidates_from_routes, rank_model_roster, select_model,
            ModelRankingPolicy, ModelRouteSet, ProviderRef,
        };

        let model_ref = canonical_model_ref(model)?;
        let route_sets: Vec<ModelRouteSet> =
            serde_json::from_value(composed.get("model_routes").cloned().unwrap_or_default())
                .map_err(|error| {
                    AikitError::new("compose.route_sets_unreadable", error.to_string())
                })?;
        let routes = route_sets
            .into_iter()
            .find(|set| set.model == model_ref)
            .ok_or_else(|| {
                AikitError::new(
                    "compose.model_not_catalogued",
                    format!(
                        "{model_ref} is not in the resolved catalogue — a Model is selected from                          the catalogue, never minted at selection time"
                    ),
                )
            })?;
        let pin = provider.map(ProviderRef::parse).transpose()?;

        let base = model_roster_candidate_for(&model_ref);
        let roster = rank_model_roster(
            model_roster_demand(),
            ModelRankingPolicy::Balanced,
            candidates_from_routes(&routes, &base),
        );
        let Some(selection) = select_model(&roster, &routes, pin.as_ref()) else {
            return Ok(serde_json::json!({
                "model": model_ref,
                "selected": false,
                "reason": format!(
                    "{model_ref} is catalogued but has no viable route{} — known-but-unavailable, \
                     which is not the same as unknown",
                    pin.as_ref().map(|p| format!(" through the pinned {p}")).unwrap_or_default()
                ),
                "routes": routes.routes,
            }));
        };

        // Selection keeps every viable route. Only actualisation picks one,
        // and the rest stay on the record so a later loss re-resolves.
        let runner = SystemRunner::new();
        let mut attempts = Vec::new();
        for route in &selection.viable_routes {
            let body = material_body_plan(&runner, "workcell", &model_ref, route);
            if let MaterialBodyOutcome::Unsatisfiable { omissions } = &body {
                attempts.push(serde_json::json!({
                    "route": route,
                    "material_body": "unsatisfiable",
                    "omissions": omissions,
                    "outcome": "skipped — Workcell cannot supply the body this route needs",
                }));
                continue;
            }
            let request = RealisationRequest {
                actuation_ref: format!("actuation:{}", self.descriptor.host),
                agency_ref: composed
                    .pointer("/composed_inputs/agency")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("agency:aikit-compose")
                    .to_string(),
                world_binding_ref: composed
                    .pointer("/plan/session_space")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("binding:aikit-compose")
                    .to_string(),
                agent_session_ref: composed
                    .pointer("/composed_inputs/agent_session")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                harness_ref: composed
                    .pointer("/plan/harness/resource")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                model: model_ref.clone(),
                route: route.clone(),
                evidence_refs: Vec::new(),
            };
            match realise(&runner, "actuation", &request) {
                RealisationOutcome::Instantiated {
                    receipt,
                    detection_ref,
                } => {
                    return Ok(serde_json::json!({
                        "model": model_ref,
                        "selected": true,
                        "selection": selection,
                        "realised_through": route,
                        "material_body": body_reading(&body),
                        "detection_ref": detection_ref,
                        "instantiation": receipt,
                        "attempts": attempts,
                        "standing": "Actuation actualised this relation under its own evidence \
                                     gate; the model's other viable routes remain on the selection \
                                     for re-resolution",
                    }));
                }
                RealisationOutcome::Refused { reason } => attempts.push(serde_json::json!({
                    "route": route, "outcome": "refused by Actuation", "reason": reason,
                })),
                RealisationOutcome::Unavailable { reason } => attempts.push(serde_json::json!({
                    "route": route, "outcome": "Actuation unavailable", "reason": reason,
                })),
            }
        }
        Ok(serde_json::json!({
            "model": model_ref,
            "selected": true,
            "selection": selection,
            "realised": false,
            "reason": "every viable route was tried and none actualised; see attempts",
            "attempts": attempts,
        }))
    }

    /// Read one Provider Source's published model list into the local
    /// catalogue. The listing is public; no credential is used, because the
    /// catalogue half of the question ("what exists") is deliberately
    /// separable from the credential half ("what can I use today").
    pub fn refresh_model_catalogue(&self, provider: &str) -> Result<serde_json::Value> {
        use aikit_adapters::provider_catalog_source::{
            fetch_openrouter_catalog, ProviderCatalogOutcome, OPENROUTER_PROVIDER,
        };
        if provider != "openrouter" {
            return Err(AikitError::new(
                "model_catalogue.unknown_provider_source",
                format!("no Provider Source is implemented for {provider:?} (have: openrouter)"),
            ));
        }
        let observed_at = jiff::Timestamp::now().to_string();
        let outcome = fetch_openrouter_catalog(&SystemRunner::new(), &observed_at);
        match outcome {
            ProviderCatalogOutcome::Observed {
                observations,
                source,
                observed_at,
            } => {
                let document = aikit_core::resource::ProviderCatalogDocument::new(
                    aikit_core::resource::ProviderRef::parse(OPENROUTER_PROVIDER)?,
                    source.clone(),
                    observed_at.clone(),
                    observations,
                );
                let path = aikit_store::model_catalogue::save_provider_catalog(&self.home, &document)?;
                let folded =
                    aikit_core::resource::catalogue_from_observations(&document.observations)?;
                Ok(serde_json::json!({
                    "provider": OPENROUTER_PROVIDER,
                    "source": source,
                    "observed_at": observed_at,
                    "listings_read": document.observations.len(),
                    "models_catalogued": folded.len(),
                    "cached_at": path.display().to_string(),
                    "standing": "provider-published observation, not authored ground; \
                                 an owner entry supersedes it by canonical ModelRef",
                }))
            }
            ProviderCatalogOutcome::Unavailable { reason } => Err(AikitError::new(
                "model_catalogue.provider_source_unavailable",
                reason,
            )),
        }
    }

    /// Read the resolved catalogue back: the first-party seed, whatever
    /// Provider Sources published, and the owner's own entries, layered by
    /// canonical ModelRef. This reports identity only — whether any of it is
    /// reachable is the join's answer, and it lives on `compose`.
    pub fn show_model_catalogue(&self, filter: Option<&str>) -> Result<serde_json::Value> {
        let (catalogue, notes) = aikit_store::model_catalogue::resolved_catalogue(&self.home);
        let needle = filter.map(str::to_lowercase);
        let entries: Vec<serde_json::Value> = catalogue
            .entries()
            .filter(|entry| match &needle {
                None => true,
                Some(needle) => {
                    entry.model.as_str().to_lowercase().contains(needle)
                        || entry.name.to_lowercase().contains(needle)
                }
            })
            .map(|entry| {
                serde_json::json!({
                    "model": entry.model,
                    "name": entry.name,
                    "source": entry.source,
                    "declared_routes": entry.routes.iter().map(|route| serde_json::json!({
                        "provider": route.provider,
                        "kind": route.kind.as_str(),
                        "provider_native_ids": route.provider_native_ids,
                        "credential_required": route.credential.requires_credential(),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        Ok(serde_json::json!({
            "catalogued": catalogue.len(),
            "shown": entries.len(),
            "notes": notes,
            "standing": "catalogue identity only — a catalogued Model is not thereby available; \
                         see `aikit compose --json` for route availability",
            "entries": entries,
        }))
    }

    fn projection_context_for(&self, source_view: &ResolvedView) -> Result<ResolvedContext> {
        let mut view = source_view.clone();
        let selected = self.project_skill_sets();
        if self.has_project_skill_routing() {
            let mut sets = Vec::new();
            for name in selected {
                sets.push(aikit_store::skillsets::load(&self.home, name)?);
            }
            let references: Vec<&aikit_core::SkillSet> = sets.iter().collect();
            let projection = aikit_core::skillset::project_union(&references, &view);
            let projected: std::collections::BTreeSet<CapsuleId> =
                projection.projected.into_iter().collect();
            view.active
                .retain(|id, capability| capability.kind != Kind::Skill || projected.contains(id));
            for withheld in projection.withheld {
                view.warnings.push(format!(
                    "{} was selected by a skill set but withheld: {}",
                    withheld.capsule,
                    withheld.reason.describe()
                ));
            }
        }

        let actor_bootstrap = if self.descriptor.project_root.is_some() {
            // Compose the live actor inputs from the Actuation instantiation
            // receipt and the Central-authored profile. Absent or ambiguous
            // projections resolve to defaults — never guessed; a fetch failure
            // is fail-soft (no projection), never a resolution failure.
            let composed = match self.descriptor.project_root.as_deref() {
                Some(root) => process_central_root(Some(root)).and_then(|central| {
                    let runner = SystemRunner::new();
                    compose_live_actor_inputs(&runner, &central, root)
                        .ok()
                        .flatten()
                }),
                None => None,
            };

            let resources = aikit_tui::project_world_service::resource_index_with_records(
                self, composed.as_ref().map(|c|c.source_resources.clone()).unwrap_or_default(),
            )?;
            let mut resolution = aikit_tui::project_world_service::context_resolution_from_resources(
                self, composed.as_ref().map(|c|c.requested_actors.clone()).unwrap_or_default(), &resources,
            )?;
            // Detection intake, same law as compose_plan: Actuation owns
            // what operative bodies exist; detected harnesses join the
            // candidates as ephemeral resources; a failed run is disclosed
            // unavailability riding on the resolution, never absence.
            let detection = aikit_adapters::actuation_harness_detection
                ::intake_actuation_detection(&SystemRunner::new(), "actuation");
            resolution.harness_detection = Some(
                aikit_adapters::actuation_harness_detection::detection_summary(&detection),
            );
            if let aikit_adapters::actuation_harness_detection::DetectionOutcome::Record(record) =
                &detection
            {
                let known: Vec<String> = resolution
                    .harness_candidates
                    .iter()
                    .map(|candidate| candidate.resource.descriptor.id.to_string())
                    .collect();
                for entry in record.harnesses.iter().filter(|entry| {
                    matches!(
                        entry.state,
                        aikit_adapters::actuation_harness_detection::DetectionState::Detected
                    )
                }) {
                    if known.iter().any(|id| id == &entry.harness_ref) {
                        continue;
                    }
                    if let Ok(resource) = aikit_adapters::actuation_harness_detection
                        ::detected_harness_resource(
                            &entry.slug,
                            &entry.harness_ref,
                            &record.detection_ref,
                        )
                    {
                        resolution.harness_candidates.push(resource);
                    }
                }
            }
            // The Model catalogue joins the same detection evidence: catalogued
            // identity plus observed route availability, never identity minted
            // from what is installed.
            let _ = self.join_model_routes(&mut resolution, &detection);
            // The World (SessionSpace) identity is discoverable from the
            // Project. When exactly one authored SessionSpace names this
            // Project, disclose it as the canonical World identity; ambiguity
            // is never silently resolved, and a SessionSpace is never inferred
            // from provider presence.
            let session_space = SessionSpaceApplicationStore::new(self.home.clone())
                .discover(Some(&resolution.project_binding.project))
                .ok()
                .filter(|states| states.len() == 1)
                .and_then(|mut states| states.pop())
                .map(|state| state.id().clone());
            let request = aikit_core::ActorBootstrapRequest {
                selected_harness: composed.as_ref().and_then(|c| c.selected_harness.clone()),
                selected_model: composed.as_ref().and_then(|c| c.selected_model.clone()),
                agent_session: composed.as_ref().and_then(|c| c.agent_session.clone()),
                session_space,
                ..aikit_core::ActorBootstrapRequest::default()
            };
            Some(aikit_core::project_actor_bootstrap(&resolution, request)?)
        } else {
            None
        };

        Ok(ResolvedContext {
            view,
            capsule_roots: self.catalog.capsule_roots(),
            actor_bootstrap,
        })
    }

    /// The resolved projection context: the view plus the capsule roots the
    /// adapters project payloads from. The CLI's commands (`context env`,
    /// apply) and the adapters share this one source so a capsule's
    /// projection and its secret declarations can never disagree about where
    /// it lives.
    pub fn projection_context(&self) -> Result<ResolvedContext> {
        self.projection_context_for(&self.view)
    }

    /// Compose the live actor launch plan for the current project context:
    /// Central-authored profile + Actuation instantiation receipt → requested
    /// actors → actor bootstrap. This is the CLI form of the composition the
    /// palette performs during project-world resolution; harness and model are
    /// disclosed only when a surface actually selected them, never guessed.
    pub fn compose_plan(&self) -> Result<serde_json::Value> {
        let project_root = self.descriptor.project_root.as_deref().ok_or_else(|| {
            AikitError::new(
                "compose.no_project",
                "no project context here — run inside a project directory",
            )
        })?;
        let central_root = process_central_root(Some(project_root));
        // Explicit composition must report a broken source as a failure, not
        // present a successful plan silently stripped of its authored basis.
        // A missing optional Central root/profile remains an honest absence.
        let composed = central_root
            .as_ref()
            .map(|central| compose_live_actor_inputs(&SystemRunner::new(), central, project_root))
            .transpose()?
            .flatten();

        let resources = aikit_tui::project_world_service::resource_index_with_records(
            self, composed.as_ref().map(|c|c.source_resources.clone()).unwrap_or_default(),
        )?;
        let mut resolution = aikit_tui::project_world_service::context_resolution_from_resources(
            self, composed.as_ref().map(|c|c.requested_actors.clone()).unwrap_or_default(), &resources,
        )?;
        // Harness detection is owned by Actuation and consumed here — one
        // live `actuation harness detect` run discloses which operative
        // bodies exist on this machine. Detected harnesses join the
        // candidate set as ephemeral resources (never persisted to any
        // index); a failed run is disclosed unavailability, never an empty
        // set read as absence.
        let detection = aikit_adapters::actuation_harness_detection
            ::intake_actuation_detection(&SystemRunner::new(), "actuation");
        let mut detection_notes: Vec<String> = Vec::new();
        resolution.harness_detection = Some(
            aikit_adapters::actuation_harness_detection::detection_summary(&detection),
        );
        match &detection {
            aikit_adapters::actuation_harness_detection::DetectionOutcome::Record(record) => {
                let known: Vec<String> = resolution
                    .harness_candidates
                    .iter()
                    .map(|candidate| candidate.resource.descriptor.id.to_string())
                    .collect();
                let mut added: Vec<String> = Vec::new();
                for entry in record.harnesses.iter().filter(|entry| {
                    matches!(
                        entry.state,
                        aikit_adapters::actuation_harness_detection::DetectionState::Detected
                    )
                }) {
                    if known.iter().any(|id| id == &entry.harness_ref) {
                        continue;
                    }
                    match aikit_adapters::actuation_harness_detection
                        ::detected_harness_resource(
                            &entry.slug,
                            &entry.harness_ref,
                            &record.detection_ref,
                        ) {
                        Ok(resource) => {
                            resolution.harness_candidates.push(resource);
                            added.push(entry.slug.clone());
                        }
                        Err(error) => detection_notes.push(format!(
                            "harness detection intake failed for {}: {}",
                            entry.harness_ref, error
                        )),
                    }
                }
                if !added.is_empty() {
                    detection_notes.push(format!(
                        "harness detection via actuation ({} catalog r{}, observed {}): \
                         detected harnesses joined the candidates as live observations: {}",
                        record.detector.implementation,
                        record.catalog_revision,
                        record.observed_at,
                        added.join(", ")
                    ));
                }
            }
            aikit_adapters::actuation_harness_detection::DetectionOutcome::Unavailable {
                reason,
            } => {
                detection_notes.push(format!(
                    "harness detection unavailable: {reason} — no harness candidates \
                     disclosed from detection; install or expose `actuation` on PATH to repair"
                ));
            }
        }
        detection_notes.extend(self.join_model_routes(&mut resolution, &detection));
        // Runtime self-identification: which harness environment this very
        // invocation runs inside. Actuation owns the marker knowledge; the
        // resolved self is an observation, never an authored selection —
        // binding stays with the instantiation receipt. The matching
        // candidate carries a `self` annotation so surfaces can show it
        // without treating it as chosen.
        let self_outcome = aikit_adapters::actuation_harness_detection
            ::intake_actuation_self(&SystemRunner::new(), "actuation");
        match &self_outcome {
            aikit_adapters::actuation_harness_detection::SelfOutcome::Resolved(record) => {
                if let Some(matched) = &record.resolved {
                    for candidate in resolution.harness_candidates.iter_mut() {
                        if candidate.resource.descriptor.id.to_string() == matched.harness_ref {
                            candidate
                                .resource
                                .descriptor
                                .annotations
                                .insert("self".to_string(), "true".to_string());
                        }
                    }
                    detection_notes.push(format!(
                        "this invocation runs inside {} (env markers: {}; \
                         self-identified via Actuation {})",
                        matched.harness_ref,
                        matched.markers.join(", "),
                        record.detection_ref
                    ));
                }
            }
            aikit_adapters::actuation_harness_detection::SelfOutcome::Ambiguous { matched } => {
                detection_notes.push(format!(
                    "harness self-identification is ambiguous ({}); nested harnesses are real \
                     and the innermost is never guessed",
                    matched.join(", ")
                ));
            }
            aikit_adapters::actuation_harness_detection::SelfOutcome::NoMatch => {}
            aikit_adapters::actuation_harness_detection::SelfOutcome::Unavailable { reason } => {
                detection_notes.push(format!(
                    "harness self-identification unavailable: {reason} — \
                     install or expose `actuation` on PATH to repair"
                ));
            }
        }
        // The World (SessionSpace) identity is discoverable from the Project.
        // Exactly one authored SessionSpace names it as canonical; ambiguity is
        // never silently resolved, and one is never inferred from provider
        // presence.
        let session_space = SessionSpaceApplicationStore::new(self.home.clone())
            .discover(Some(&resolution.project_binding.project))
            .ok()
            .filter(|states| states.len() == 1)
            .and_then(|mut states| states.pop())
            .map(|state| state.id().clone());
        let request = aikit_core::ActorBootstrapRequest {
            selected_harness: composed.as_ref().and_then(|c| c.selected_harness.clone()),
            selected_model: composed.as_ref().and_then(|c| c.selected_model.clone()),
            agent_session: composed.as_ref().and_then(|c| c.agent_session.clone()),
            session_space,
            ..aikit_core::ActorBootstrapRequest::default()
        };
        let plan = aikit_core::project_actor_bootstrap(&resolution, request)?;

        // The AIKit-home agent seed is the standing identity of the O:I agent
        // (id, name, world tie — nothing else). It is disclosed alongside the
        // Central composition; it never selects a harness or model, because
        // binding happens at instantiation, when the live harness registers
        // itself.
        let seed_discovery =
            aikit_adapters::home_agent_profile::discover_home_agent_profiles(self.home.root());
        let home_seed = seed_discovery.exactly_one().cloned();

        // Instructive notes for the absence an owner can actually repair. Each
        // note names the surface that closes it; none of them fake a selection.
        let mut composition_notes: Vec<String> = Vec::new();
        if central_root.is_none() {
            composition_notes.push(
                "no Central root resolved for this project — profile composition skipped; \
                 set the Central root for this project or run under ~/Central/Work"
                    .to_owned(),
            );
        } else if plan.agent.is_none() {
            match &home_seed {
                Some(seed) => composition_notes.push(format!(
                    "no Central AgentProfile resolves for this project — the AIKit-home seed \
                     {} ({}) carries the standing agent identity; harness/model bind at \
                     instantiation, not in the profile",
                    seed.id,
                    seed.name.as_deref().unwrap_or("unnamed")
                )),
                None => composition_notes.push(
                    "no AgentProfile resolves for this project and no AIKit-home agent seed \
                     exists — author one with: ctrl action run agent-profile.save \
                     {\"scope\":\"project\", ...} (owner-authored; detection keeps running \
                     without it)"
                        .to_owned(),
                ),
            }
        }
        for problem in &seed_discovery.problems {
            composition_notes.push(format!("home agent seed unreadable: {problem}"));
        }
        if seed_discovery.profiles.len() > 1 {
            let ids = seed_discovery
                .profiles
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            composition_notes.push(format!(
                "multiple home agent seeds found ({ids}) — specify which one this project \
                 composes; none was guessed"
            ));
        }
        if plan.harness.is_none() {
            composition_notes.push(format!(
                "no harness selected by an authored source — detected candidates: [{}]; \
                 selection happens via Central profile / Actuation instantiation receipt, not here",
                plan.harness_candidates.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(", ")
            ));
        }
        if plan.model.is_none() {
            // Available and known-but-unavailable are different facts and are
            // reported as different lists. A Model with no proven route must
            // never appear as if it could be used.
            let mut available: Vec<String> = Vec::new();
            let mut unavailable: Vec<String> = Vec::new();
            for set in &resolution.model_routes {
                let line = format!(
                    "{} ({})",
                    set.model,
                    set.viable()
                        .into_iter()
                        .map(|route| format!(
                            "{} via {}",
                            route.provider_native_id, route.provider
                        ))
                        .collect::<Vec<_>>()
                        .join(" | ")
                );
                if set.is_available() {
                    available.push(line);
                } else {
                    unavailable.push(set.model.to_string());
                }
            }
            composition_notes.push(format!(
                "no model selected by an authored source — catalogued models with a proven \
                 route: [{}]; catalogued but no route proven here: [{}]; selection is not \
                 provider selection, so a selected model keeps every viable route for \
                 Actuation to resolve",
                available.join(", "),
                unavailable.join(", ")
            ));
        }

        composition_notes.extend(detection_notes);

        Ok(serde_json::json!({
            "project_root": project_root.display().to_string(),
            "central_root": central_root.as_ref().map(|p| p.display().to_string()),
            "composition_error": null,
            "composition_notes": composition_notes,
            "home_agent_seed": home_seed.as_ref().map(|seed| serde_json::json!({
                "id": seed.id,
                "name": seed.name,
                "description": seed.description,
            })),
            "composed_inputs": composed.as_ref().map(|c| serde_json::json!({
                "authored_basis": c.authored,
                "source_resources": c.source_resources,
                "authored_basis_standing": "requested-source-not-effective-selection",
                "agent": c.requested_actors.agent,
                "agency": c.requested_actors.agency,
                "host": c.requested_actors.host,
                "selected_harness": c.selected_harness,
                "selected_model": c.selected_model,
                "agent_session": c.agent_session,
            })),
            "plan": plan,
            // The catalogue-to-availability join, in the direction it runs.
            // Routes stay plural: selecting a Model is not selecting a
            // provider, and Actuation resolves an actual route from this set.
            "model_routes": resolution.model_routes,
            "unmatched_model_offers": resolution.unmatched_model_offers,
        }))
    }

    fn has_project_skill_routing(&self) -> bool {
        self.project_specification().is_some()
    }

    /// Publish the generation-backed Codex projection at Codex's native project
    /// discovery path. The link targets the stable `current` pointer, so future
    /// generation swaps are hot without rewriting the project tree.
    fn prepare_codex_project_link(&self, context_dir: &Path) -> Result<()> {
        if !self.has_project_skill_routing() {
            return Ok(());
        }
        let Some(project_root) = self.descriptor.project_root.as_ref() else {
            return Ok(());
        };
        let target = context_dir.join("current/projections/codex/.agents/skills");
        let parent = project_root.join(".agents");
        let link = parent.join("skills");
        std::fs::create_dir_all(&parent).map_err(|error| {
            AikitError::new(
                "projection.codex_link_failed",
                format!("could not create {}: {error}", parent.display()),
            )
        })?;

        if let Ok(metadata) = std::fs::symlink_metadata(&link) {
            if !metadata.file_type().is_symlink() {
                return Err(AikitError::new(
                    "projection.codex_tree_owned",
                    format!(
                        "refusing to replace user-owned Codex skill tree {}",
                        link.display()
                    ),
                )
                .with("path", link.display().to_string()));
            }
            let existing = std::fs::read_link(&link).map_err(|error| {
                AikitError::new(
                    "projection.codex_link_failed",
                    format!("could not inspect {}: {error}", link.display()),
                )
            })?;
            if existing == target {
                return Ok(());
            }
            if !existing.starts_with(self.home.contexts()) {
                return Err(AikitError::new(
                    "projection.codex_tree_owned",
                    format!(
                        "refusing to replace non-AIKit Codex skill link {}",
                        link.display()
                    ),
                )
                .with("path", link.display().to_string()));
            }
        }

        let temporary = parent.join("skills.aikit-tmp");
        if std::fs::symlink_metadata(&temporary).is_ok() {
            std::fs::remove_file(&temporary).map_err(|error| {
                AikitError::new(
                    "projection.codex_link_failed",
                    format!("could not clear {}: {error}", temporary.display()),
                )
            })?;
        }
        create_directory_link(&target, &temporary)?;
        std::fs::rename(&temporary, &link).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            AikitError::new(
                "projection.codex_link_failed",
                format!("could not publish {}: {error}", link.display()),
            )
        })
    }

    pub fn resolved(&self) -> &ResolvedView {
        &self.view
    }

    /// The loaded catalogue, for callers that need the raw manifests (the hook
    /// chain builder, capability previews).
    pub fn snapshot(&self) -> &Snapshot {
        &self.catalog
    }

    /// The exact instructions a brokered harness should read for an active
    /// Skill, including the same scoped augmentation native adapters project.
    pub fn effective_skill_markdown(&self, id: &CapsuleId) -> Result<String> {
        let capability = self.view.active.get(id).ok_or_else(|| {
            AikitError::new(
                "capabilities.not_active",
                format!("{id} is not active in this context"),
            )
            .with("capability", id.to_string())
        })?;
        if capability.kind != Kind::Skill {
            return Err(AikitError::new(
                "capabilities.not_a_skill",
                format!("{id} is {}, not a skill", capability.kind.as_str()),
            ));
        }
        let root = self.catalog.capsule_roots().remove(id).ok_or_else(|| {
            AikitError::new(
                "run.source_missing",
                format!("{id} has no payload on this machine"),
            )
        })?;
        let payload_root = capability
            .config
            .get("root")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("payload");
        let skill = agent_skills::validate(&root.join(payload_root))?;
        let overlays = self
            .view
            .skill_usage_overlays
            .get(id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        skill.effective_markdown(overlays)
    }

    /// Dispatch a client hook event through the immutable chain for this context,
    /// consuming a bypass token if one applies. This is the CLI's
    /// `hook dispatch`, wired to the resolved view and the real store.
    pub fn dispatch_hook(
        &self,
        event: &aikit_core::hooks::HookEvent,
    ) -> Result<aikit_core::hooks::HookDecision> {
        use aikit_core::hooks::{build_chains, HookChain};
        let chains = build_chains(&self.view, &self.catalog)?;
        let chain = match chains.get(event.kind.as_str()) {
            Some(chain) => chain.clone(),
            None => HookChain::plan(
                event.kind.clone(),
                Vec::new(),
                &std::collections::BTreeMap::new(),
            )?,
        };
        let roots = self.catalog.capsule_roots();
        let mut decision = crate::hook::dispatch(
            &self.index,
            &self.descriptor.context_id,
            &chain,
            event,
            &roots,
        )?;

        // W1 reaction engine. The floor (Central's temporal reground) already
        // ran inside dispatch and is law, never gated. Everything beyond it is
        // operative only because the active composition selected it: the
        // tuning is resolved from the view at event time, never from global
        // config.
        let tuning = ContinuityTuning::resolve(&self.view);

        // The engine's own blocks are collected classified rather than pushed
        // straight at the decision, because the last stage — context pressure
        // — bounds ordinary payload and must never bound standing guidance.
        // The floor's temporal reground is already in `decision.injected` and
        // is deliberately not in this list: it is law, and law is not bounded
        // by a capability.
        let mut blocks: Vec<aikit_core::pressure::Block> = Vec::new();

        if event.kind == aikit_core::hooks::HookEventKind::UserPromptSubmit
            && tuning.allows(aikit_core::continuity::TURN_LEDGER)
        {
            blocks.push(aikit_core::pressure::Block::ordinary(
                format!(
                    "[continuity/turn-ledger] composed by this context's composition; event {}                  dispatched for {}",
                    event.kind, event.client,
                ),
                Vec::new(),
            ));
        }
        if event.kind == aikit_core::hooks::HookEventKind::SessionStart
            && tuning.allows(aikit_core::continuity::ENTITY_DISCLOSURE)
        {
            match crate::continuity_disclosure::entity_disclosure(event) {
                Ok(Some(block)) => blocks.push(aikit_core::pressure::Block::ordinary(block, Vec::new())),
                Ok(None) => {}
                Err(error) => decision.warnings.push(format!(
                    "continuity/entity-disclosure unavailable: {error}"
                )),
            }
        }
        if event.kind == aikit_core::hooks::HookEventKind::SessionStart
            && tuning.allows(aikit_core::continuity::ORIENTATION_PACKET)
        {
            // The aperture tunings ride the composition's config for this
            // capsule, resolved from the view at event time — never ambient.
            // Closed by default: a missing config keeps the default bounds
            // and keeps the @1 human horizon shut.
            let capsule_id = aikit_core::id::CapsuleId::parse(
                "hook/continuity/orientation-packet",
            )
            .map_err(|error| {
                AikitError::new(
                    "capabilities.invalid_id",
                    format!("engine reaction id is malformed: {error}"),
                )
            })?;
            let tuned = match self.view.active.get(&capsule_id) {
                Some(active) => {
                    crate::orientation_packet::OrientationConfig::from_config(Some(&active.config))
                }
                None => crate::orientation_packet::OrientationConfig::default(),
            };
            match crate::orientation_packet::orientation_packet(event, &tuned) {
                Ok(Some(block)) => blocks.push(aikit_core::pressure::Block::ordinary(block, Vec::new())),
                Ok(None) => {}
                Err(error) => decision.warnings.push(error),
            }
        }
        if event.kind == aikit_core::hooks::HookEventKind::SessionStart
            && self.descriptor.project_root.is_none()
            && tuning.allows(aikit_core::continuity::PROJECT_RECENCY)
        {
            let capsule_id = aikit_core::id::CapsuleId::parse(
                "hook/continuity/project-recency",
            )?;
            let config = self.view.active.get(&capsule_id)
                .map(|active| crate::project_recency::ProjectRecencyConfig::from_config(Some(&active.config)))
                .unwrap_or_default();
            match crate::projects::load_all(&self.home).and_then(|specs| {
                crate::project_recency::classify_all(
                    &self.index, &specs, aikit_store::Timestamp::now(), config)
            }) {
                Ok(rows) => blocks.push(aikit_core::pressure::Block::ordinary(
                    crate::project_recency::render_session_start(&rows, config.max_projects),
                    Vec::new(),
                )),
                Err(error) => decision.warnings.push(format!(
                    "continuity/project-recency unavailable: {error}")),
            }
        }
        // Star prompt-commands come first and, when one matches, the domain
        // branch does not run: an explicit protocol the user asked for is not
        // improved by ambient guidance piled on top of it.
        let mut star_matched = false;
        if event.kind == aikit_core::hooks::HookEventKind::UserPromptSubmit
            && tuning.allows(aikit_core::continuity::STAR_COMMANDS)
        {
            let capsule_id =
                aikit_core::id::CapsuleId::parse("hook/continuity/star-commands").map_err(
                    |error| {
                        AikitError::new(
                            "capabilities.invalid_id",
                            format!("engine reaction id is malformed: {error}"),
                        )
                    },
                )?;
            let config = self
                .view
                .active
                .get(&capsule_id)
                .map(|active| crate::star_commands::StarConfig::from_config(Some(&active.config)))
                .unwrap_or_default();
            let central_root = crate::temporal::process_central_root(event.cwd.as_deref());
            let routing = crate::star_commands::routing_context(
                &config,
                central_root.as_deref(),
                event.cwd.as_deref(),
            );
            let prompt = crate::domain_activation::prompt_of(event);
            let reaction = crate::star_commands::run(prompt.as_deref(), &config, &routing);
            star_matched = reaction.matched_any();
            // Standing, not ordinary: the user asked for this protocol by
            // name. Bounding away the thing that was explicitly requested
            // would be pressure deciding what the user meant.
            blocks.extend(
                reaction
                    .blocks
                    .into_iter()
                    .map(|text| aikit_core::pressure::Block::standing(text, Vec::new())),
            );
            decision.warnings.extend(reaction.warnings);
        }
        if event.kind == aikit_core::hooks::HookEventKind::UserPromptSubmit
            && !star_matched
            && tuning.allows(aikit_core::continuity::DOMAIN_ACTIVATION)
        {
            // Domains are declared data in the project layer; they load only
            // under this composition, never ambient.
            if let Some(project_root)=self.descriptor.project_root.as_deref() {
                let (domains, mut load_warnings)=
                    crate::domain_activation::load_domains(project_root);
                decision.warnings.append(&mut load_warnings);
                let prompt=crate::domain_activation::prompt_of(event);
                let scope=crate::domain_activation::dedup_scope(event, Some(project_root));
                let Some(scope)=scope else {
                    return Ok(self.under_pressure(decision, &blocks, event));
                };
                let (domain_blocks, mut reaction_warnings)=crate::domain_activation::run(
                    &self.index, &scope, &domains, prompt.as_deref());
                blocks.extend(domain_blocks);
                decision.warnings.append(&mut reaction_warnings);
            }
        }
        if event.kind == aikit_core::hooks::HookEventKind::PreToolUse
            && tuning.allows(aikit_core::continuity::FILE_CONTEXT)
        {
            // File context is project-layer knowledge too: the project's wiki
            // relations and path-addressed domains, loaded only under this
            // composition, in front of the operation — never after it.
            if let Some(project_root)=self.descriptor.project_root.as_deref() {
                if let Some(path)=crate::file_context::file_path_of(event) {
                    let (domains, mut load_warnings)=
                        crate::domain_activation::load_domains(project_root);
                    decision.warnings.append(&mut load_warnings);
                    let (objects, mut wiki_warnings)=
                        crate::file_context::load_project_wiki(project_root);
                    decision.warnings.append(&mut wiki_warnings);
                    let scope=crate::domain_activation::dedup_scope(event, Some(project_root));
                    let Some(scope)=scope else {
                        return Ok(self.under_pressure(decision, &blocks, event));
                    };
                    let (file_blocks, mut reaction_warnings)=crate::file_context::run(
                        &self.index, &scope, project_root, &path, &domains, objects);
                    blocks.extend(file_blocks);
                    decision.warnings.append(&mut reaction_warnings);
                }
            }
        }
        if event.kind == aikit_core::hooks::HookEventKind::PostToolUse
            && tuning.allows(aikit_core::continuity::ACTIVITY_EVIDENCE)
        {
            if let Some(project_root)=self.descriptor.project_root.as_deref() {
                if let Err(error)=crate::activity_evidence::record(
                    &self.index, &self.descriptor.context_id, project_root, event)
                {
                    decision.warnings.push(format!(
                        "continuity/activity-evidence unavailable: {error}"));
                }
            }
        }

        Ok(self.under_pressure(decision, &blocks, event))
    }

    /// The last stage of the reaction engine: render the turn's blocks, under
    /// the composition's pressure brackets when it composed any.
    ///
    /// Uncomposed, this renders every block whole — the descope law again: a
    /// capability nobody selected changes nothing, including how much of
    /// something else arrives. Composed, ordinary payload is bounded by the
    /// bracket the reading falls in, standing guidance is exempt, and both the
    /// per-block and the per-turn withholding are stated.
    fn under_pressure(
        &self,
        mut decision: aikit_core::hooks::HookDecision,
        blocks: &[aikit_core::pressure::Block],
        event: &aikit_core::hooks::HookEvent,
    ) -> aikit_core::hooks::HookDecision {
        let tuning = ContinuityTuning::resolve(&self.view);
        let reading = if tuning.allows(aikit_core::continuity::CONTEXT_PRESSURE) {
            let config = aikit_core::id::CapsuleId::parse("hook/continuity/context-pressure")
                .ok()
                .and_then(|id| self.view.active.get(&id))
                .map(|active| active.config.clone());
            let (brackets, mut warnings) =
                aikit_core::pressure::PressureBrackets::from_config(config.as_ref());
            decision.warnings.append(&mut warnings);
            let scope = crate::domain_activation::dedup_scope(
                event,
                self.descriptor.project_root.as_deref(),
            );
            match scope {
                Some(scope) => {
                    let (reading, mut warnings) =
                        crate::pressure::read(&self.index, &scope, event, &brackets);
                    decision.warnings.append(&mut warnings);
                    // The turn is counted after it is read, so a session's
                    // first prompt is read as a fresh window rather than as
                    // one turn already spent.
                    if event.kind == aikit_core::hooks::HookEventKind::UserPromptSubmit {
                        decision
                            .warnings
                            .extend(crate::pressure::record_turn(&self.index, &scope));
                    }
                    Some(reading)
                }
                None => None,
            }
        } else {
            None
        };
        let bounded = crate::pressure::apply(blocks, reading.as_ref());
        decision.injected.extend(bounded.blocks);
        if let Some(notice) = bounded.notice {
            decision.injected.push(notice);
        }
        decision
    }

    /// The continuity composition in force for this context, resolved from
    /// the active view (the descope law): the floor is temporal reground,
    /// and everything beyond it is operative only because the composition
    /// selected it. Never global mutable config; read fresh on every ask.
    pub fn continuity_tuning(&self) -> ContinuityTuning {
        ContinuityTuning::resolve(&self.view)
    }

    /// Issue a bypass token for this context.
    pub fn issue_bypass(
        &self,
        scope: &str,
        reason: Option<&str>,
        capability: Option<&str>,
    ) -> Result<String> {
        crate::hook::issue_bypass(
            &self.index,
            &self.descriptor.context_id,
            scope,
            reason,
            capability,
        )
    }

    /// The open (unspent, unexpired) bypass tokens for this context, for `status`
    /// and `bypasses` to make visible.
    pub fn open_bypasses(&self) -> Result<Vec<aikit_store::index::BypassRecord>> {
        self.index.open_bypasses(&self.descriptor.context_id)
    }

    /// The inbox channel items (Spec II §2) — the messages the system and agents
    /// have addressed to the user. Pending-only by default; `all` includes
    /// resolved items kept for audit. This is what makes the inbox broker-readable:
    /// `aikit inbox list --json` runs through here.
    pub fn inbox_items(&self, all: bool) -> Result<Vec<aikit_store::InboxItem>> {
        let channel = aikit_store::InboxChannel::new(&self.index);
        if all {
            channel.items()
        } else {
            channel.pending(aikit_store::Timestamp::now())
        }
    }

    /// The cosmetic properties (a `label`, notes) recorded on this context's
    /// current generation, if any. Read-only: it never creates the context
    /// directory, so `status` on a context that has never applied stays a no-op.
    pub fn current_generation_properties(&self) -> std::collections::BTreeMap<String, String> {
        let context_dir = self.home.context_dir(&self.descriptor.context_id);
        let Ok(Some(id)) = generation::current(&context_dir) else {
            return std::collections::BTreeMap::new();
        };
        let dir = context_dir.join(generation::GENERATIONS).join(id.as_str());
        generation::read_lock(&dir)
            .map(|v| v.properties)
            .unwrap_or_default()
    }

    /// Roll the context's `current` generation back to `previous`.
    pub fn rollback(&self) -> Result<generation::RollbackOutcome> {
        let context_dir = self.context_dir()?;
        generation::rollback(&context_dir)
    }

    /// Garbage-collect old generations, keeping the most recent `keep`.
    pub fn prune(&self, keep: usize) -> Result<Vec<GenerationId>> {
        let context_dir = self.context_dir()?;
        generation::gc(&context_dir, keep)
    }

    /// Registry-load problems (a bad manifest that did not blind its neighbours),
    /// surfaced as warnings rather than swallowed.
    pub fn load_warnings(&self) -> Vec<String> {
        self.problems
            .iter()
            .map(|p| format!("{}: {}", p.path.display(), p.error.message()))
            .collect()
    }

    /// The context directory under the home, created if needed.
    fn context_dir(&self) -> Result<PathBuf> {
        self.home.ensure_context_dir(&self.descriptor.context_id)
    }

    /// Resolve with `toggles` folded in as a top-priority one-shot override.
    fn resolve_with(&self, toggles: &[Toggle]) -> Result<ResolvedView> {
        let mut layers = self.layers.clone();
        if !toggles.is_empty() {
            layers.push(override_layer(toggles));
        }
        resolve_or_explain(
            &self.catalog,
            &self.trust,
            &self.descriptor,
            &layers,
            &self.policy,
        )
    }

    /// Find a capsule by an exported command name (preferred) or by id.
    fn find_runnable(&self, name: &str, view: &ResolvedView) -> Result<CapsuleId> {
        if let Some(id) = view.exported_commands().get(name) {
            return Ok(id.clone());
        }
        if let Ok(id) = CapsuleId::parse(name) {
            if self.catalog.get(&id).is_some() {
                return Ok(id);
            }
        }
        Err(AikitError::new(
            "run.unknown_command",
            format!("no capability exports `{name}`"),
        )
        .with("name", name.to_string()))
    }

    /// The honest per-client effects of applying `view`, computed by the real
    /// adapters. Any adapter that cannot plan for this context is skipped rather
    /// than guessed at, so a preview never fabricates a projection outcome.
    fn client_effects(&self, view: &ResolvedView) -> Vec<ClientEffect> {
        let Ok(rc) = self.projection_context_for(view) else {
            return Vec::new();
        };
        let ctx_dir = self.home.context_dir(&self.descriptor.context_id);
        let tree = self
            .descriptor
            .project_root
            .clone()
            .unwrap_or_else(|| self.invocation_cwd.clone());

        let mut effects = Vec::new();
        for target in &self.descriptor.targets {
            let effect = match target.as_str() {
                TargetId::SHELL => Some(ActivationEffect::immediate("shell bin/")),
                TargetId::CLAUDE_CODE => {
                    plan_effect(&ClaudeAdapter::new(ctx_dir.join("projections/claude")), &rc)
                }
                TargetId::CODEX => plan_effect(&CodexAdapter::new(tree.clone()), &rc),
                TargetId::ZCODE => plan_effect(&ZcodeAdapter::new(), &rc),
                TargetId::DEEPSEEK_HARNESS => {
                    plan_effect(&DshAdapter::new(ctx_dir.join("projections/dsh")), &rc)
                }
                // Harness-admission sweep round 1: each admitted adapter answers
                // for its own target id with its evidence-backed plan. Binding
                // one of these targets in a context descriptor opts the context
                // into that harness's honest effect; unbound harnesses stay inert.
                TargetId::AIDER => {
                    plan_effect(&AiderAdapter::new(ctx_dir.join("projections/aider")), &rc)
                }
                TargetId::CURSOR_CLI => {
                    plan_effect(&CursorAdapter::new(ctx_dir.join("projections/cursor")), &rc)
                }
                TargetId::GEMINI_CLI => {
                    plan_effect(&GeminiAdapter::new(ctx_dir.join("projections/gemini")), &rc)
                }
                TargetId::GOOSE => {
                    plan_effect(&GooseAdapter::new(ctx_dir.join("projections/goose")), &rc)
                }
                TargetId::OPENCODE => {
                    plan_effect(&OpencodeAdapter::new(ctx_dir.join("projections/opencode")), &rc)
                }
                TargetId::QWEN_CODE => {
                    plan_effect(&QwenAdapter::new(ctx_dir.join("projections/qwen")), &rc)
                }
                // Harness-admission sweep round 3: six catalog-r4 harnesses
                // admitted through Actuation detection records; ids align to
                // catalog slugs (gemini-antigravity, grok-bot, kimi, ollama,
                // openclaw, pi). Binding one opts the context into that
                // harness's honest effect; unbound harnesses stay inert.
                TargetId::ANTIGRAVITY => plan_effect(
                    &AntigravityAdapter::new(ctx_dir.join("projections/antigravity")),
                    &rc,
                ),
                TargetId::GROK_BOT => {
                    plan_effect(&GrokbotAdapter::new(ctx_dir.join("projections/grokbot")), &rc)
                }
                TargetId::KIMI => {
                    plan_effect(&KimiAdapter::new(ctx_dir.join("projections/kimi")), &rc)
                }
                TargetId::OLLAMA => {
                    plan_effect(&OllamaAdapter::new(ctx_dir.join("projections/ollama")), &rc)
                }
                TargetId::OPENCLAW => {
                    plan_effect(&OpenclawAdapter::new(ctx_dir.join("projections/openclaw")), &rc)
                }
                TargetId::PI => plan_effect(&PiAdapter::new(ctx_dir.join("projections/pi")), &rc),
                _ => plan_effect(&BrokerAdapter::new(), &rc),
            };
            if let Some(effect) = effect {
                effects.push(ClientEffect::new(target.clone(), effect));
            }
        }
        effects
    }

    /// The shell projection: one `bin/` shim per exported command. This is the
    /// projection that makes the contextual PATH — and therefore `run` and the
    /// multicall shims — real.
    fn shell_plan(view: &ResolvedView, secret_items: Vec<ProjectionItem>) -> Result<ProjectionPlan> {
        let mut plan =
            ProjectionPlan::new(TargetId::shell(), ActivationEffect::immediate("shell bin/"));
        for (name, capsule) in view.exported_commands() {
            plan = plan.with_item(ProjectionItem::shim(name.clone(), capsule.clone(), name)?);
        }
        // The context's environment rides in the shell projection: `bin/` and the
        // exported variables are the same surface as far as a shell is concerned.
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        for item in crate::env::project(view, &home)? {
            plan = plan.with_item(item);
        }
        // Secret env vars ride the same surface, declared by ref: values
        // resolve at materialisation time, never at plan time.
        for item in secret_items {
            plan = plan.with_item(item);
        }
        Ok(plan)
    }

    fn scope_document(&self, scope: ScopeKind) -> Result<ScopeWriter> {
        match scope {
            ScopeKind::Global => Ok(ScopeWriter::Profile(ProfileDocument::open(
                &self.home.global_profile(),
            )?)),
            ScopeKind::Session => {
                let session = self.descriptor.session_id.clone().ok_or_else(|| {
                    AikitError::new(
                        "scope.no_session",
                        "the session scope needs an AIKit session, and this context has none",
                    )
                })?;
                let dir = self.home.ensure_session_dir(&session)?;
                let doc = OverlayDocument::open(&dir.join("overlay.toml"), &session)?;
                Ok(ScopeWriter::Overlay(doc))
            }
            ScopeKind::Project | ScopeKind::ProjectLocal => {
                let root = self
                    .project
                    .as_ref()
                    .map(|p| p.root.clone())
                    .ok_or_else(|| {
                        AikitError::new(
                            "scope.no_project",
                            "a project scope needs a project, and the cwd is not inside one",
                        )
                    })?;
                let file = if scope == ScopeKind::Project {
                    root.join(discover::MARKER).join(discover::PROFILE_FILE)
                } else {
                    root.join(discover::MARKER)
                        .join(discover::PROFILE_LOCAL_FILE)
                };
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        AikitError::new(
                            "scope.write_failed",
                            format!("could not create {}: {e}", parent.display()),
                        )
                    })?;
                }
                Ok(ScopeWriter::Profile(ProfileDocument::open(&file)?))
            }
            other => Err(AikitError::new(
                "scope.unwritable",
                format!(
                    "writing to the {} scope is not supported here",
                    other.as_str()
                ),
            )
            .with("scope", other.as_str())),
        }
    }
}

/// A scope's on-disk declaration, opened for editing.
enum ScopeWriter {
    Overlay(OverlayDocument),
    Profile(ProfileDocument),
}

impl ScopeWriter {
    fn apply_toggles(&mut self, toggles: &[Toggle]) {
        for toggle in toggles {
            match self {
                ScopeWriter::Overlay(doc) => {
                    if toggle.enable {
                        doc.enable(&toggle.capsule);
                    } else {
                        doc.disable(&toggle.capsule);
                    }
                }
                ScopeWriter::Profile(doc) => {
                    if toggle.enable {
                        doc.enable(&toggle.capsule);
                    } else {
                        doc.disable(&toggle.capsule);
                    }
                }
            }
        }
    }

    fn use_profile(&mut self, profile: &aikit_core::id::ProfileId) {
        match self {
            ScopeWriter::Overlay(doc) => doc.use_profile(profile),
            ScopeWriter::Profile(doc) => doc.use_profile(profile),
        }
    }

    fn set_skill_overlay(&mut self, id: &CapsuleId, overlay: &SkillUsageOverlayPatch) {
        match self {
            ScopeWriter::Overlay(doc) => doc.set_skill_overlay(id, overlay),
            ScopeWriter::Profile(doc) => doc.set_skill_overlay(id, overlay),
        }
    }

    fn clear_skill_overlay(&mut self, id: &CapsuleId) {
        match self {
            ScopeWriter::Overlay(doc) => doc.clear_skill_overlay(id),
            ScopeWriter::Profile(doc) => doc.clear_skill_overlay(id),
        }
    }

    fn save(&self) -> Result<()> {
        match self {
            ScopeWriter::Overlay(doc) => doc.save(),
            ScopeWriter::Profile(doc) => doc.save(),
        }
    }
}

// ---------------------------------------------------------------------------
// AikitApplication
// ---------------------------------------------------------------------------

impl AikitApplication for Service {
    fn search(&self, r: SearchRequest) -> Result<SearchResults> {
        let resolved = aikit_tui::application_service::ApplicationService::resolve_search_from(
            self,
            &r.query,
        )?;
        // This compatibility API returns packages only. Keep the canonical
        // relative order and apply its limit after narrowing the typed field.
        let rows = resolved
            .resources
            .resources
            .into_iter()
            .filter_map(|row| {
                let id = CapsuleId::parse(row.resource.as_str()).ok()?;
                let entry = self.view.catalog_index.get(&id)?;
                Some(SearchHit {
                    name: entry.name.clone(),
                    kind: entry.kind,
                    active: self.view.is_active(&id),
                    runnable: self.view.can_run(&id),
                    id,
                })
            })
            .take(r.limit)
            .collect();
        Ok(SearchResults {
            rows,
            warnings: self.load_warnings(),
        })
    }

    fn resolve(&self, r: ResolveRequest) -> Result<ResolvedView> {
        self.resolve_with(&r.toggles)
    }

    fn stage(&self, r: StageRequest) -> Result<StagedDiff> {
        let mut set = aikit_tui::staging::StagedSet::default();
        for toggle in &r.toggles {
            set.set(&toggle.capsule, toggle.enable);
        }
        aikit_tui::staging::stage(self, r.scope, &set)
            .map_err(|problem| AikitError::new(problem.code(), problem.headline()))
    }

    fn apply(&mut self, r: ApplyRequest) -> Result<AppliedGeneration> {
        // 1. Persist the declaration to the scope's file, so the change survives
        //    the process and a later resolve reads it back.
        if !r.toggles.is_empty() {
            let mut writer = self.scope_document(r.scope)?;
            writer.apply_toggles(&r.toggles);
            writer.save()?;
        }

        // 2. Re-read the layers now that the file has changed, and re-resolve.
        self.layers = assemble_layers(&self.home, &self.descriptor, self.project.as_ref())?;
        self.view = resolve_or_explain(
            &self.catalog,
            &self.trust,
            &self.descriptor,
            &self.layers,
            &self.policy,
        )?;
        self.invalidate_knowledge_runtime();

        // 3. Build and commit a generation. A failed build never replaces the
        //    live one — that guarantee lives in the store; here we honour the
        //    compare-and-swap against the base we resolved from.
        let context_dir = self.context_dir()?;
        let base = generation::current(&context_dir)?;
        let projection_context = self.projection_context()?;
        let tree = self
            .descriptor
            .project_root
            .clone()
            .unwrap_or_else(|| self.invocation_cwd.clone());
        let plans = vec![
            Self::shell_plan(&self.view, self.secret_env_items(&projection_context)?)?,
            ClaudeAdapter::new(context_dir.join("projections/claude")).plan(&projection_context)?,
            CodexAdapter::new(tree).plan(&projection_context)?,
            DshAdapter::new(context_dir.join("projections/dsh")).plan(&projection_context)?,
        ];

        // A cosmetic label rides on the view as a `[properties]` entry. It is
        // excluded from the generation's identity, so labelling an unchanged view
        // updates the label in place rather than minting a new generation.
        let mut view = self.view.clone();
        if let Some(label) = r.label.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            view.properties
                .insert("label".to_string(), label.to_string());
        }
        let staged = GenerationBuilder::new()
            .with_secret_resolver(std::sync::Arc::new(
                aikit_adapters::secret_resolver::SuiteSecretResolver::default(),
            ))
            .build(&context_dir, &view, &plans)?;
        self.prepare_codex_project_link(&context_dir)?;
        let committed = staged.commit(base.as_ref())?;
        let effects = self.client_effects(&self.view);

        Ok(AppliedGeneration {
            id: committed.id,
            replaced: committed.replaced,
            warnings: self.view.warnings.clone(),
            effects,
        })
    }

    fn run(&mut self, r: RunRequest) -> Result<RunHandle> {
        let id = self.find_runnable(&r.name, &self.view)?;
        let capsule =
            self.catalog.get(&id).cloned().ok_or_else(|| {
                AikitError::new("run.unknown_command", format!("{id} is not loaded"))
            })?;

        // Honour trust: an unreviewed executable must be confirmed before it
        // runs. This must be computed from the capsule's own trust, NOT from
        // whether it is active — a script is runnable while inactive, and the
        // inactive, unreviewed, run-ad-hoc case is exactly the one the
        // confirmation exists to guard. Reading it off `view.active` would skip
        // the gate for every capsule that is not currently enabled.
        if capsule.kind.is_executable() && !r.confirmed {
            let trust = self.trust.state_for(
                capsule.source.as_ref(),
                &capsule.id,
                capsule.revision.as_ref(),
            );
            if !trust.may_run_unattended() {
                return Err(AikitError::new(
                    "trust.required",
                    format!("{id} has not been reviewed; re-run with confirmation"),
                )
                .with("capability", id.to_string())
                .with("trust", trust.as_str()));
            }
        }

        let project_root = self.descriptor.project_root.as_deref();
        let plan = run::plan_script(&capsule, &r.args, project_root, &self.invocation_cwd)?;
        let report = run::execute(&plan)?;
        Ok(RunHandle {
            capsule: id,
            report,
        })
    }

    fn session_up(&mut self, r: SessionRequest) -> Result<SessionResult> {
        use aikit_adapters::mux::ReconcileMode;
        use aikit_store::events::Timestamp;
        use aikit_store::state::{SessionRecord, SessionState, StateStore};

        let plan = self.requested_session_plan(r.spec.as_deref())?;
        let (stack, session_id) = self.session_stack(&plan)?;
        let binding = stack.ensure_session(&plan, ReconcileMode::CreateOrAttach)?;
        let mux = stack.topology_kind();
        let now = Timestamp::now();
        let state = StateStore::new(&self.index);
        let created_at = state
            .session(&session_id)?
            .map(|record| record.created_at)
            .unwrap_or(now);
        state.put_session(&SessionRecord {
            session_id,
            name: plan.name.clone(),
            project_root: self.descriptor.project_root.clone(),
            project_marker: self.descriptor.project_id.clone(),
            mux,
            mux_session: Some(binding.session.clone()),
            state: SessionState::Live,
            created_at,
            last_seen: now,
        })?;
        let summary = if binding.created {
            format!(
                "created {} session `{}` with {} view(s)",
                mux,
                plan.name,
                binding.views.len()
            )
        } else {
            format!("reconciled {} session `{}`", mux, plan.name)
        };
        Ok(SessionResult {
            session: plan.name,
            mux: mux.as_str().to_string(),
            created: binding.created,
            actions: binding.actions,
            preserved: binding.preserved,
            summary,
            warnings: binding.warnings,
        })
    }

    fn promote(&mut self, r: PromoteRequest) -> Result<PromotedCapsule> {
        use aikit_store::inbox::{Inbox, PromotionEdits};
        let inbox = Inbox::new(&self.home, &self.index);
        let candidate = inbox.candidate(&r.candidate)?.ok_or_else(|| {
            AikitError::new(
                "inbox.unknown_candidate",
                format!("no candidate `{}` is waiting", r.candidate),
            )
        })?;
        let id = match r.id {
            Some(id) => id,
            None => CapsuleId::parse(&format!("script/captured/{}", candidate.id))?,
        };
        let edits = PromotionEdits::new(id, candidate.title.clone());
        let registry_root = self.home.registry("personal");
        inbox.promote(&r.candidate, &edits, &registry_root)
    }
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).map_err(|error| {
        AikitError::new(
            "projection.codex_link_failed",
            format!(
                "could not link {} to {}: {error}",
                link.display(),
                target.display()
            ),
        )
    })
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(target, link).map_err(|error| {
        AikitError::new(
            "projection.codex_link_failed",
            format!(
                "could not link {} to {}: {error}",
                link.display(),
                target.display()
            ),
        )
    })
}

// ---------------------------------------------------------------------------
// PaletteBackend — the same state, shaped for the palette
// ---------------------------------------------------------------------------

impl PaletteBackend for Service {
    /// The host's terminal working environments, observed over this Service's
    /// own session plan.
    ///
    /// `Some` because this boundary *did* look — an empty vector is the
    /// truthful "no mux is installed here", not "nobody checked". A plan that
    /// cannot be compiled is the one case that answers `None`: without a plan
    /// there is nothing to observe over, and claiming an empty host would be a
    /// second lie on top of the first.
    fn working_environments(&self) -> Result<Option<Vec<WorkingEnvironmentObservation>>> {
        if let Some(cached) = self.working_environments.borrow().as_ref() {
            return Ok(Some(cached.clone()));
        }
        let Ok(plan) = self.session_plan(None) else {
            return Ok(None);
        };
        let observed = crate::working_environment_field::observe(&plan)?;
        *self.working_environments.borrow_mut() = Some(observed.clone());
        Ok(Some(observed))
    }

    fn working_environment_subjects(&self) -> Result<Vec<aikit_core::resource::ResourceRef>> {
        let Ok(plan) = self.session_plan(None) else {
            return Ok(Vec::new());
        };
        Ok(crate::working_environment_field::plan_surfaces(&plan)
            .into_iter()
            .map(|(surface, _)| surface)
            .collect())
    }

    fn act_in_working_environment(
        &mut self,
        provider: &aikit_core::resource::ResourceRef,
        subject: &aikit_core::resource::ResourceRef,
        operation: WorkingEnvironmentOperation,
    ) -> Result<WorkingEnvironmentOutcome> {
        let plan = self.session_plan(None)?;
        let outcome = crate::working_environment_field::act(&plan, provider, subject, operation)?;
        // The host may have changed under us; the next reading must come from
        // the machine rather than from what it looked like before we acted.
        *self.working_environments.borrow_mut() = None;
        Ok(outcome)
    }

    fn context_resource_records(&self) -> Result<Vec<aikit_core::resource::ResourceRecord>> {
        let Some(project) = self.descriptor.project_root.as_deref() else { return Ok(Vec::new()) };
        let mut records = if let Some(central) = process_central_root(Some(project)) {
            compose_live_actor_inputs(&SystemRunner::new(), &central, project)?
                .map(|inputs| inputs.source_resources)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if let Some(started) = &self.factory_started_resources {
            records.extend(started.clone());
            return Ok(records);
        }
        match (&self.factory_state, &self.factory_project_ref) {
            (None, None) => {}
            (Some(state), Some(project_ref)) if state.is_file() => {
                let binding = FactoryDevelopmentalBinding::new(
                    self.factory_executable.clone(),
                    state.clone(),
                    project_ref.clone(),
                )?;
                records.extend(
                    read_factory_developmental(&SystemRunner::new(), &binding)?.resources,
                );
            }
            // A configured start-work request may legitimately point at a new
            // state path. Until the owner accepts the Commission, this is a
            // confirmed zero-Factory reading rather than a broken read.
            (Some(state), _) if self.factory_request_file.is_some() && !state.exists() => {}
            _ => {
                return Err(AikitError::new(
                    "factory.developmental_incomplete_binding",
                    "Factory navigation requires an existing AIKIT_FACTORY_STATE plus AIKIT_FACTORY_PROJECT_REF, or a complete AIKIT_FACTORY_STATE + AIKIT_FACTORY_REQUEST_FILE start-work binding; no Factory identity is inferred from the current Session or harness",
                ))
            }
        }
        Ok(records)
    }

    fn factory_work_entry(&self) -> FactoryWorkEntry {
        match (&self.factory_state, &self.factory_request_file) {
            (Some(_), Some(_)) => FactoryWorkEntry::Ready,
            (None, None) => FactoryWorkEntry::Unavailable {
                reason: "set AIKIT_FACTORY_STATE and AIKIT_FACTORY_REQUEST_FILE to expose the native Factory Commission action".into(),
            },
            _ => FactoryWorkEntry::Unavailable {
                reason: "Factory start-work binding is incomplete; both AIKIT_FACTORY_STATE and AIKIT_FACTORY_REQUEST_FILE are required".into(),
            },
        }
    }

    fn start_factory_work(&mut self) -> Result<FactoryWorkStartReceipt> {
        let state = self.factory_state.clone().ok_or_else(|| {
            AikitError::new(
                "factory.start_work_unavailable",
                "AIKIT_FACTORY_STATE is required for Start Factory Work",
            )
        })?;
        let request_file = self.factory_request_file.clone().ok_or_else(|| {
            AikitError::new(
                "factory.start_work_unavailable",
                "AIKIT_FACTORY_REQUEST_FILE is required for Start Factory Work",
            )
        })?;
        let started = start_factory_work(
            &SystemRunner::new(),
            self.factory_executable.clone(),
            state,
            request_file,
        )?;
        let receipt = serde_json::to_string_pretty(&started.receipt).map_err(|error| {
            AikitError::new(
                "factory.start_work_receipt_unserializable",
                format!("could not present Factory Commission receipt: {error}"),
            )
        })?;
        let commission = started
            .receipt
            .get("commission")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                AikitError::new(
                    "factory.start_work_receipt_invalid",
                    "validated Factory receipt omitted its Commission",
                )
            })?;
        let journey_ref = commission
            .get("journeyRef")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Factory Journey");
        let run_ref = commission
            .get("runRef")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Factory Run");
        self.factory_started_resources = Some(started.observation.resources);
        Ok(FactoryWorkStartReceipt {
            summary: format!(
                "Factory commissioned {journey_ref} / {run_ref}; execution remains commissioned-not-executed"
            ),
            receipt,
        })
    }

    fn project_binding(&self) -> Result<Option<aikit_core::project::ProjectBinding>> {
        let Some(root) = self.descriptor.project_root.as_ref() else { return Ok(None) };
        match std::fs::symlink_metadata(root.join("ProjectCentral/project.json")) {
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(AikitError::new("projectcentral.manifest_read", error.to_string())),
        }
        let binding = aikit_adapters::ProjectCentralFilesystemBinding::inspect(root, None)?;
        Ok(Some(binding.project_binding()?))
    }

    /// Observe the Project's versioned material through the native provider.
    ///
    /// This is the layer that *can* look: `aikit-core` is I/O-free and
    /// `aikit-tui` does not depend on `aikit-adapters`, so the observation has
    /// to be made here and handed over. Before this existed nobody made it,
    /// `ProjectWorldReadModel::versioned_world` was permanently `None`, and the
    /// Compose preview told every user "no versioned material provider
    /// attached to this reading" — an honest sentence about a socket nothing
    /// was plugged into.
    ///
    /// Three absences stay distinguishable rather than collapsing into one:
    ///
    /// * no Project bound, or no ProjectCentral identity — nothing to observe
    ///   *for*, so `None` with no warning;
    /// * git unavailable on this machine — the provider says so through its own
    ///   descriptor status, and that is an error the reading discloses;
    /// * a Project that is genuinely not under version control — `None`,
    ///   because "looked, and it is not a worktree" is a real answer and not a
    ///   failure. The provider reports that as `versioned_world.git_failed`
    ///   from its first probe, which is the same code a broken repository
    ///   would produce; the split below prefers the benign reading for a
    ///   *failed query* and reserves disclosure for a provider that could not
    ///   run at all (`versioned_world.git_spawn_failed`). Sniffing git's
    ///   stderr text to separate them would be worse than the coarse split.
    fn versioned_world(&self) -> Result<Option<aikit_core::resource::VersionedProjectWorld>> {
        let Some(root) = self.descriptor.project_root.as_deref() else { return Ok(None) };
        let Some(binding) = self.project_binding()? else { return Ok(None) };
        use aikit_core::resource::VersionedWorldProvider;
        let provider = aikit_adapters::native_git::NativeGitProvider::new()?;
        match provider.inspect(&binding.project, &root.to_string_lossy()) {
            Ok(observed) => Ok(Some(observed)),
            Err(error) if error.code() == "versioned_world.git_failed" => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Compose the credential/provider reading for this world.
    ///
    /// This is the layer that *can* look, and it is the producer #239 shipped
    /// the surface for without ever wiring: `aikit-core` is I/O-free and
    /// `aikit-tui` does not depend on the crates that reach the OS secure store,
    /// so the disclosure has to be composed here and handed over.
    ///
    /// Requirements come straight from the world's resolved Model routes — the
    /// credentials those routes declare a need for. A route's credential
    /// condition is fixed by the catalogue's declared need joined against
    /// recorded bindings; it does not depend on whether the route was observed
    /// on this machine, so this reuses the exact catalogue↔binding join the
    /// compose path uses, with no observed or reachable evidence, rather than
    /// spawning live detection just to read what a credential is *for*.
    ///
    /// The roster is observed against the same OS secure store the CLI's own
    /// `credential setup` uses. The three absences the disclosure exists to
    /// keep apart stay apart: an unreadable binding store is a `Unknown` roster
    /// carrying its reason; a readable store with no matching binding is an
    /// `Observed` roster whose credential resolves to no provider (a real "no");
    /// and a world whose routes declare no credential need yields an observed,
    /// empty requirement set (`none required`), never a fabricated one.
    fn credential_world(
        &self,
    ) -> Result<Option<aikit_core::credential_world::CredentialWorldDisclosure>> {
        use aikit_core::credential_world::{
            credential_requirements_for_model_routes, disclose_credential_world,
            ProviderRosterKnowledge,
        };

        let (catalogue, _notes) = aikit_store::model_catalogue::resolved_catalogue(&self.home);
        let bindings = aikit_store::credentials::CredentialBindingStore::new(&self.home).list();
        let (credentials, roster_unreadable) = match &bindings {
            Ok(list) => (
                aikit_adapters::actuation_model_routes::CredentialEvidence::from_binding_refs(
                    list.iter()
                        .filter(|binding| !binding.revoked)
                        .map(|binding| binding.credential_ref.as_str().to_string()),
                ),
                None,
            ),
            // The binding store could not be read at all: the roster is
            // genuinely unknown, not empty. Requirements are still derived (they
            // come from the catalogue, not the store) so the world can say what
            // it needs while honestly reporting that nothing was resolved.
            Err(error) => (
                aikit_adapters::actuation_model_routes::CredentialEvidence::default(),
                Some(format!("credential binding store is unreadable: {error}")),
            ),
        };

        let join = aikit_adapters::actuation_model_routes::join_model_routes_with_reach(
            &catalogue,
            &[],
            &[],
            &credentials,
        );
        let requirements = credential_requirements_for_model_routes(&join.route_sets);

        let roster = match roster_unreadable {
            Some(reason) => ProviderRosterKnowledge::Unknown { reason },
            None => observe_credential_roster(&self.home, &requirements)?,
        };

        // Headless and no env import: a world reading observes bound state, it
        // never prompts and never imports a secret from the ambient environment
        // just because a matching variable happens to exist.
        Ok(Some(disclose_credential_world(
            roster,
            &requirements,
            true,
            false,
        )))
    }

    fn context(&self) -> &ContextDescriptor {
        &self.descriptor
    }

    fn view(&self) -> &ResolvedView {
        &self.view
    }

    fn application_home(&self) -> Option<&AikitHome> {
        Some(&self.home)
    }

    fn scope_layers(&self) -> Option<&[ScopeLayer]> {
        Some(&self.layers)
    }

    fn familiarity(&self) -> Result<Option<aikit_core::FamiliarityStore>> {
        match aikit_store::replay_familiarity(&self.index)? {
            aikit_store::FamiliarityReplay::Loaded { store, .. } => Ok(Some(store)),
            aikit_store::FamiliarityReplay::Invalidated { .. } => Ok(None),
        }
    }

    fn record_familiarity(
        &mut self,
        observation: aikit_core::FamiliarityObservation,
    ) -> Result<()> {
        aikit_store::append_familiarity_observation(&self.index, observation)
    }

    fn knowledge_resolve(
        &self,
        expression: &aikit_core::resource::ResolveExpression,
        limit: usize,
    ) -> Result<Option<aikit_core::KnowledgeSearchResult>> {
        Service::knowledge_resolve(self, expression, limit).map(Some)
    }

    fn knowledge_address(
        &self,
        resource: &aikit_core::resource::ResourceRef,
    ) -> Result<Option<aikit_core::KnowledgeAddress>> {
        Service::knowledge_address(self, resource)
    }

    fn knowledge_read(
        &self,
        address: &aikit_core::KnowledgeAddress,
    ) -> Result<Option<aikit_core::KnowledgeReading>> {
        Service::knowledge_read(self, address).map(Some)
    }

    fn knowledge_relations(
        &self,
        address: &aikit_core::KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<Option<aikit_core::KnowledgeRelationView>> {
        Service::knowledge_relations(self, address, depth, max_nodes, max_edges).map(Some)
    }

    fn knowledge_route(
        &mut self,
        query: Option<&str>,
        addresses: &[aikit_core::KnowledgeAddress],
    ) -> Result<Option<aikit_core::KnowledgeRoute>> {
        Service::knowledge_route(self, query, addresses).map(Some)
    }

    fn knowledge_frame(
        &mut self,
        query: Option<&str>,
        addresses: &[aikit_core::KnowledgeAddress],
    ) -> Result<Option<aikit_core::KnowledgeContextPack>> {
        Service::knowledge_frame(self, query, addresses).map(Some)
    }

    fn knowledge_sources(
        &self,
        address: &aikit_core::KnowledgeAddress,
    ) -> Result<Option<aikit_core::KnowledgeSources>> {
        Service::knowledge_sources(self, address).map(Some)
    }

    fn knowledge_explain(
        &self,
        address: &aikit_core::KnowledgeAddress,
    ) -> Result<Option<aikit_core::KnowledgeExplanation>> {
        Service::knowledge_explain(self, address).map(Some)
    }

    fn knowledge_history(
        &self,
        resource: Option<&aikit_core::resource::ResourceRef>,
    ) -> Result<Vec<aikit_store::KnowledgeApplicationReceipt>> {
        Service::knowledge_history(self, resource)
    }

    fn knowledge_status(&self) -> Result<Option<aikit_core::KnowledgeProviderStatus>> {
        Service::knowledge_status(self).map(Some)
    }

    fn knowledge_forget(&mut self, scope: aikit_core::ForgetScope) -> Result<bool> {
        Service::knowledge_forget(self, scope)?;
        Ok(true)
    }

    fn documents(&self) -> Vec<SearchDoc> {
        self.view
            .catalog_index
            .keys()
            .filter_map(|id| {
                let usage = self.index.usage(id).unwrap_or_default();
                SearchDoc::from_view(&self.view, id, usage)
            })
            .collect()
    }

    fn capsule(&self, id: &CapsuleId) -> Option<&Capsule> {
        self.catalog.get(id)
    }

    fn preview(&self, scope: ScopeKind, toggles: &[Toggle]) -> Result<Projected> {
        let _ = scope; // the override is folded at top precedence; see resolve_with
        let view = self.resolve_with(toggles)?;
        let effects = self.client_effects(&view);
        Ok(Projected { view, effects })
    }

    fn apply(&mut self, scope: ScopeKind, toggles: &[Toggle]) -> Result<GenerationId> {
        let applied = AikitApplication::apply(
            self,
            ApplyRequest {
                scope,
                toggles: toggles.to_vec(),
                label: None,
            },
        )?;
        Ok(applied.id)
    }

    fn start(&mut self, intent: &RunIntent) -> Result<JobOutput> {
        let capsule = self.catalog.get(&intent.capsule).cloned().ok_or_else(|| {
            AikitError::new(
                "run.unknown_command",
                format!("{} is not loaded", intent.capsule),
            )
        })?;
        let args = intent.argv().unwrap_or_default();
        let project_root = self.descriptor.project_root.as_deref();
        let mut plan = run::plan_script(&capsule, &args, project_root, &self.invocation_cwd)?;
        // The palette only starts capture/background modes; force capture so the
        // output comes back for the result panel rather than seizing the terminal
        // the palette is holding.
        plan.mode = aikit_core::capsule::ExecMode::Capture;
        let report = run::execute(&plan)?;
        Ok(JobOutput {
            capsule: Some(intent.capsule.clone()),
            status: Some(report.status),
            lines: report.output,
            truncated: false,
        })
    }

    fn recent(&self) -> Vec<RunIntent> {
        // Recent invocations live in the event log; surfacing them as replayable
        // intents is left for the integration phase, so this is honestly empty
        // rather than fabricated.
        Vec::new()
    }

    fn promotion_drafts(&self) -> Vec<PromotionDraft> {
        use aikit_store::inbox::Inbox;
        let inbox = Inbox::new(&self.home, &self.index);
        let Ok(candidates) = inbox.candidates() else {
            return Vec::new();
        };
        candidates
            .into_iter()
            .filter_map(|candidate| {
                let id = CapsuleId::parse(&format!("script/captured/{}", candidate.id)).ok()?;
                let edits = aikit_store::inbox::PromotionEdits::new(id, candidate.title.clone());
                Some(PromotionDraft::new(candidate, edits))
            })
            .collect()
    }

    fn promote(&mut self, draft: &PromotionDraft) -> Result<CapsuleId> {
        let promoted = AikitApplication::promote(
            self,
            PromoteRequest {
                candidate: draft.candidate.id.clone(),
                id: Some(draft.edits.id.clone()),
            },
        )?;
        Ok(promoted.id)
    }
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Observe the world-level secret-provider roster over the OS secure store,
/// reusing the same provider the CLI's `credential setup` uses.
///
/// This is the I/O half of the credential world. The native store's descriptor
/// is per-credential — it advertises support for a credential only when a
/// binding for that exact credential is recorded — so the roster is observed by
/// probing each required credential's binding and merging the results into one
/// descriptor whose `supported_credentials` is the set actually bound. That one
/// descriptor is what `resolve_credential` then checks each requirement against:
/// a requirement finds it eligible exactly when its credential is bound.
///
/// A world whose routes declare no credential need probes nothing and reports an
/// observed, empty roster — a confirmed "this world needs none", never the
/// "nobody looked" the disclosure's own `not_attempted` default carries.
fn observe_credential_roster(
    home: &AikitHome,
    requirements: &[aikit_core::credential::SecretRequirement],
) -> Result<aikit_core::credential_world::ProviderRosterKnowledge> {
    use aikit_adapters::NativeSecureStoreProvider;
    use aikit_core::credential::{SecretProvider, SecretProviderDescriptor};
    use aikit_core::credential_world::ProviderRosterKnowledge;
    use std::collections::BTreeSet;

    let store = aikit_store::credentials::CredentialBindingStore::new(home);
    let mut supported = BTreeSet::new();
    let mut descriptor: Option<SecretProviderDescriptor> = None;

    for requirement in requirements {
        let binding = store.load(&requirement.credential_ref)?;
        let native = NativeSecureStoreProvider::with_binding(binding.as_ref());
        let observed = native.descriptor(&requirement.credential_ref);
        supported.extend(observed.supported_credentials.iter().cloned());
        descriptor.get_or_insert(observed);
    }

    let providers = match descriptor {
        Some(mut native) => {
            native.supported_credentials = supported;
            vec![native]
        }
        None => Vec::new(),
    };

    Ok(ProviderRosterKnowledge::Observed { providers })
}

/// Load every registry under the home plus the project-local `.aikit/` registry,
/// project-local last so it shadows the personal registries — which is exactly
/// the precedence a user expects from a repo-local capability.
pub fn load_catalog(
    home: &AikitHome,
    project_root: Option<&Path>,
) -> Result<aikit_store::registry::RegistryLoad> {
    use aikit_core::id::RegistrySource;
    let mut load = aikit_store::registry::RegistryLoad::default();

    if let Ok(entries) = std::fs::read_dir(home.registries()) {
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        for name in names {
            let root = home.registry(&name);
            let one = load_registry(&root, RegistrySource::new(name))?;
            load.merge(one);
        }
    }

    for (name, root) in crate::skill_sources::active_registries(home)? {
        let one = load_registry(&root, RegistrySource::new(name))?;
        load.merge(one);
    }

    if let Some(root) = project_root {
        let local = load_project_local(root)?;
        load.merge(local);
    }

    Ok(load)
}

/// Turn the discovered project profile chain and the session overlay into the
/// resolver's scope-layer stack. Missing files are simply absent layers, never
/// errors: a project with no `profile.toml` yet resolves against the global scope.
fn assemble_layers(
    home: &AikitHome,
    descriptor: &ContextDescriptor,
    project: Option<&DiscoveredProject>,
) -> Result<Vec<ScopeLayer>> {
    let mut layers = Vec::new();

    let global = home.global_profile();
    if global.exists() {
        let patch = ProfileDocument::open(&global)?.patch()?;
        if !patch.is_empty() {
            layers.push(ScopeLayer::new(
                ScopeKind::Global,
                LayerOrigin::new(global.display().to_string()),
                patch,
            ));
        }
    }

    if let Some(project) = project {
        for layer in &project.chain {
            let committed = layer.profile();
            if committed.exists() {
                let patch = ProfileDocument::open(&committed)?.patch()?;
                if !patch.is_empty() {
                    let mut scope = ScopeLayer::new(
                        ScopeKind::Project,
                        LayerOrigin::new(committed.display().to_string()),
                        patch,
                    );
                    scope.depth = layer.depth as u16;
                    layers.push(scope);
                }
            }
            let local = layer.profile_local();
            if local.exists() {
                let patch = ProfileDocument::open(&local)?.patch()?;
                if !patch.is_empty() {
                    let mut scope = ScopeLayer::new(
                        ScopeKind::ProjectLocal,
                        LayerOrigin::new(local.display().to_string()),
                        patch,
                    );
                    scope.depth = layer.depth as u16;
                    layers.push(scope);
                }
            }
        }
    }

    if let Some(session) = &descriptor.session_id {
        let overlay = home.session_overlay(session);
        if overlay.exists() {
            let patch = OverlayDocument::open(&overlay, session)?.patch()?;
            if !patch.is_empty() {
                layers.push(ScopeLayer::new(
                    ScopeKind::Session,
                    LayerOrigin::new(overlay.display().to_string()),
                    patch,
                ));
            }
        }
    }

    Ok(layers)
}

/// A one-shot override layer carrying a set of pending toggles at top precedence.
fn override_layer(toggles: &[Toggle]) -> ScopeLayer {
    let mut patch = aikit_core::profile::PoolPatch::default();
    for toggle in toggles {
        patch.set(&toggle.capsule, toggle.enable);
    }
    ScopeLayer::new(ScopeKind::OneShot, LayerOrigin::new("pending"), patch)
}

/// Resolve, preferring a produced view but turning a fatal problem into the very
/// error the JSON envelope and exit codes are built to carry.
fn resolve_or_explain(
    catalog: &Snapshot,
    trust: &TrustSnapshot,
    descriptor: &ContextDescriptor,
    layers: &[ScopeLayer],
    policy: &ManagedPolicy,
) -> Result<ResolvedView> {
    let request = CoreResolveRequest {
        context: descriptor.clone(),
        layers: layers.to_vec(),
        policy: policy.clone(),
    };
    let diagnosis = resolve_diagnostic(catalog, trust, &request);
    if let Some(fatal) = diagnosis.problems.iter().find(|p| p.fatal) {
        return Err(fatal.error.clone());
    }
    diagnosis
        .view
        .ok_or_else(|| AikitError::new("resolution.failed", "resolution produced no view"))
}

fn plan_effect(adapter: &dyn TargetAdapter, rc: &ResolvedContext) -> Option<ActivationEffect> {
    adapter
        .plan(rc)
        .ok()
        .map(|plan| adapter.activation_effect(None, &plan))
}

/// A neutral roster demand for compose-time selection. The roster exists to
/// order `(Model, route)` pairs; compose does not invent task fitness it has
/// not observed, so the demand carries only what it can honestly state.
fn model_roster_demand() -> aikit_core::resource::ModelRosterDemand {
    aikit_core::resource::ModelRosterDemand {
        project: None,
        profile: None,
        agency: None,
        use_type: "compose".into(),
        required_capabilities: Default::default(),
        required_modalities: Default::default(),
        required_tools: Default::default(),
        required_contracts: Default::default(),
        context_characteristics: Default::default(),
        independence_from: Default::default(),
        estimated_input_tokens: None,
        estimated_output_tokens: None,
        cost_ceiling_usd: None,
    }
}

/// The model-level facts a compose-time candidate carries. Route-level facts
/// are filled in per route by `candidates_from_routes`; nothing here asserts
/// fitness, price or authorisation that has not been observed.
fn model_roster_candidate_for(
    model: &aikit_core::resource::ResourceRef,
) -> aikit_core::resource::ModelRosterCandidate {
    aikit_core::resource::ModelRosterCandidate {
        model: model.clone(),
        variant: model.to_string(),
        provider: aikit_core::resource::ProviderRef::parse("provider:unresolved")
            .expect("static provider ref"),
        provider_revision: None,
        available: false,
        authorised: true,
        provider_usable: false,
        policy_allowed: true,
        contract_compatible: true,
        harness_compatible: true,
        harness_composition: None,
        native_capabilities: Default::default(),
        harness_capabilities: Default::default(),
        profile_skills: Default::default(),
        modalities: Default::default(),
        tool_support: Default::default(),
        contracts: Default::default(),
        task_fitness: Default::default(),
        role_fitness: Default::default(),
        profile_fit: None,
        authored_preference: None,
        frecency: None,
        latency_ms: None,
        reliability: None,
        context_window_tokens: None,
        price: None,
        exact_spend: Vec::new(),
        observed_fitness: Vec::new(),
        access: Default::default(),
        provenance: Vec::new(),
    }
}

fn body_reading(outcome: &aikit_adapters::model_realisation::MaterialBodyOutcome) -> serde_json::Value {
    use aikit_adapters::model_realisation::MaterialBodyOutcome;
    match outcome {
        MaterialBodyOutcome::NotRequired { reason } => {
            serde_json::json!({ "state": "not-required", "reason": reason })
        }
        MaterialBodyOutcome::Satisfiable { plan_ref } => {
            serde_json::json!({ "state": "satisfiable", "plan_ref": plan_ref })
        }
        MaterialBodyOutcome::Unsatisfiable { omissions } => {
            serde_json::json!({ "state": "unsatisfiable", "omissions": omissions })
        }
        MaterialBodyOutcome::Unavailable { reason } => {
            serde_json::json!({ "state": "unavailable", "reason": reason })
        }
    }
}
