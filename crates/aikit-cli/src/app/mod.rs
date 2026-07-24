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
use aikit_core::context::ContextDescriptor;
use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::platform::TargetId;
use aikit_core::policy::ManagedPolicy;
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

use aikit_adapters::clients::broker::BrokerAdapter;
use aikit_adapters::clients::claude::ClaudeAdapter;
use aikit_adapters::clients::codex::CodexAdapter;

use aikit_tui::backend::{
    ClientEffect, JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle,
};
pub use aikit_tui::staging::StagedDiff;

use crate::discover::{self, DiscoveredProject};
use crate::run::{self, RunReport};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionResult {
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
}

impl Service {
    /// Discover everything from the current working directory and process
    /// environment, and resolve the view.
    pub fn discover(cwd: &Path) -> Result<Self> {
        let home = AikitHome::discover()?;
        Self::open(home, cwd, |k| std::env::var(k).ok())
    }

    /// The injectable form: an explicit home and environment lookup, so tests run
    /// against a real temp home without touching the process environment.
    pub fn open<F>(home: AikitHome, cwd: &Path, env: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        home.ensure_layout()?;
        let project = discover::discover_project(cwd);
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
        })
    }

    pub fn descriptor(&self) -> &ContextDescriptor {
        &self.descriptor
    }

    pub fn resolved(&self) -> &ResolvedView {
        &self.view
    }

    /// The loaded catalogue, for callers that need the raw manifests (the hook
    /// chain builder, capability previews).
    pub fn snapshot(&self) -> &Snapshot {
        &self.catalog
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
            None => {
                HookChain::plan(event.kind.clone(), Vec::new(), &std::collections::BTreeMap::new())?
            }
        };
        let roots = self.catalog.capsule_roots();
        crate::hook::dispatch(&self.index, &self.descriptor.context_id, &chain, event, &roots)
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
        Err(
            AikitError::new("run.unknown_command", format!("no capability exports `{name}`"))
                .with("name", name.to_string()),
        )
    }

    /// The honest per-client effects of applying `view`, computed by the real
    /// adapters. Any adapter that cannot plan for this context is skipped rather
    /// than guessed at, so a preview never fabricates a projection outcome.
    fn client_effects(&self, view: &ResolvedView) -> Vec<ClientEffect> {
        let rc = ResolvedContext {
            view: view.clone(),
            capsule_roots: self.catalog.capsule_roots(),
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
    fn shell_plan(view: &ResolvedView) -> Result<ProjectionPlan> {
        let mut plan =
            ProjectionPlan::new(TargetId::shell(), ActivationEffect::immediate("shell bin/"));
        for (name, capsule) in view.exported_commands() {
            plan = plan.with_item(ProjectionItem::shim(name.clone(), capsule.clone(), name)?);
        }
        Ok(plan)
    }

    fn scope_document(&self, scope: ScopeKind) -> Result<ScopeWriter> {
        match scope {
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
                    root.join(discover::MARKER).join(discover::PROFILE_LOCAL_FILE)
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
                format!("writing to the {} scope is not supported here", other.as_str()),
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
        let query = r.query.to_lowercase();
        let mut hits: Vec<(i32, SearchHit)> = Vec::new();
        for (id, entry) in &self.view.catalog_index {
            let haystack = format!(
                "{} {} {} {}",
                id,
                entry.name,
                entry.description,
                entry.tags.join(" ")
            )
            .to_lowercase();
            let score = subsequence_score(&query, &haystack);
            if query.is_empty() || score > 0 {
                hits.push((
                    score,
                    SearchHit {
                        id: id.clone(),
                        name: entry.name.clone(),
                        kind: entry.kind,
                        active: self.view.is_active(id),
                        runnable: self.view.can_run(id),
                    },
                ));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        hits.truncate(r.limit);
        Ok(SearchResults {
            rows: hits.into_iter().map(|(_, h)| h).collect(),
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

        // 3. Build and commit a generation. A failed build never replaces the
        //    live one — that guarantee lives in the store; here we honour the
        //    compare-and-swap against the base we resolved from.
        let context_dir = self.context_dir()?;
        let base = generation::current(&context_dir)?;
        let plans = vec![Self::shell_plan(&self.view)?];

        // A cosmetic label rides on the view as a `[properties]` entry. It is
        // excluded from the generation's identity, so labelling an unchanged view
        // updates the label in place rather than minting a new generation.
        let mut view = self.view.clone();
        if let Some(label) = r.label.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            view.properties.insert("label".to_string(), label.to_string());
        }
        let staged = GenerationBuilder::new().build(&context_dir, &view, &plans)?;
        let committed = staged.commit(base.as_ref())?;

        Ok(AppliedGeneration {
            id: committed.id,
            replaced: committed.replaced,
            warnings: self.view.warnings.clone(),
        })
    }

    fn run(&mut self, r: RunRequest) -> Result<RunHandle> {
        let id = self.find_runnable(&r.name, &self.view)?;
        let capsule = self
            .catalog
            .get(&id)
            .cloned()
            .ok_or_else(|| AikitError::new("run.unknown_command", format!("{id} is not loaded")))?;

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
        Ok(RunHandle { capsule: id, report })
    }

    fn session_up(&mut self, _r: SessionRequest) -> Result<SessionResult> {
        // Session topology orchestration is delegated to the mux adapters in
        // `aikit-adapters`; wiring the portable session spec through them is left
        // for the integration phase. What is honest to report now is the detected
        // stack and that no topology was changed.
        let mux = self
            .descriptor
            .mux
            .map(|m| m.as_str().to_string())
            .unwrap_or_else(|| "plain".to_string());
        Ok(SessionResult {
            summary: format!("no session change; multiplexer is {mux}"),
            warnings: vec!["session topology orchestration is not wired in this build".to_string()],
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

// ---------------------------------------------------------------------------
// PaletteBackend — the same state, shaped for the palette
// ---------------------------------------------------------------------------

impl PaletteBackend for Service {
    fn context(&self) -> &ContextDescriptor {
        &self.descriptor
    }

    fn view(&self) -> &ResolvedView {
        &self.view
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
            AikitError::new("run.unknown_command", format!("{} is not loaded", intent.capsule))
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

/// A tiny fuzzy scorer for the CLI's `search`: rewards contiguous, early matches
/// of the query as a subsequence of the haystack. The palette uses the richer
/// `nucleo` matcher; the CLI only needs a deterministic, dependency-free order.
fn subsequence_score(query: &str, haystack: &str) -> i32 {
    if query.is_empty() {
        return 1;
    }
    let mut score = 0;
    let mut last: Option<usize> = None;
    let mut q = query.chars().peekable();
    for (i, c) in haystack.chars().enumerate() {
        if let Some(&needle) = q.peek() {
            if needle == c {
                score += match last {
                    Some(prev) if prev + 1 == i => 3, // contiguous
                    _ => 1,
                };
                last = Some(i);
                q.next();
            }
        }
    }
    if q.peek().is_some() {
        0 // not all query chars matched
    } else {
        score
    }
}

fn plan_effect(adapter: &dyn TargetAdapter, rc: &ResolvedContext) -> Option<ActivationEffect> {
    adapter
        .plan(rc)
        .ok()
        .map(|plan| adapter.activation_effect(None, &plan))
}
