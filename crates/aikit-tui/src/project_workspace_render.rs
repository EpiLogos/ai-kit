//! Read-only Project-world presentation for the V2 Workspace.
//!
//! This module formats [`ProjectWorldReadModel`] into human-facing Workspace
//! lines. It owns no resolver, selection, retrieval or mutation state: live
//! selection remains [`TuiState::selected`], ContextSource retrieval remains an
//! explicit provider operation, and durable composition remains the existing
//! staging -> preview -> confirm -> apply path.
//!
//! The public Workspace field is Search / Worlds / Compose / Work / Knowledge /
//! History / System — spec `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §3-§8.
//! Explain is deliberately
//! not a section here: spec §17 retires it as a top-level destination in
//! favour of an Inspector overlay reached through the `:` Explain contextual
//! Action (see `Overlay::Explain` in `crate::v2_render`, which now renders
//! `explain_lines`' content alongside the provider Explain evidence rather
//! than losing it).

use std::collections::BTreeSet;

use aikit_core::context_resolution::Availability;
use aikit_core::project::ProjectBindingLocator;
use aikit_core::credential_world::{CredentialStatusKnowledge, ProviderRosterKnowledge};
use aikit_core::resource::{Eligibility, ResourceKind, SourceAuthority};
use aikit_core::explain_history::{HistoryEvidence, HistoryReadModel, HistoryRecoverability};
use aikit_core::session_space_application::SessionSpaceAuthoredState;
use aikit_core::working_environment::WorkingEnvironmentHealth;
use aikit_core::{ContextSourceHit, ProjectWorldReadModel, ProjectWorldResource};

use crate::application::{TuiState, WorkspaceSection};
use crate::backend::FactoryWorkEntry;
use crate::compose_spine::compose_spine_lines;
use crate::live_field::LiveWorkingField;
use crate::layout::Glyphs;

/// Canonical product label for each Workspace slot.
///
/// Search is the universal query field and therefore does not need its own
/// `WorkspaceSection`; the six section slots complete the canonical field as
/// Worlds / Compose / Work / Knowledge / History / System.
pub fn workspace_section_label(section: WorkspaceSection) -> &'static str {
    match section {
        WorkspaceSection::Worlds => "Worlds",
        WorkspaceSection::Compose => "Compose",
        WorkspaceSection::Work => "Work",
        WorkspaceSection::Knowledge => "Knowledge",
        WorkspaceSection::History => "History",
        WorkspaceSection::System => "System",
    }
}

/// Everything the Workspace renders from that is a *reading* rather than
/// state.
///
/// Grouped into one borrow so the render path does not grow a parameter every
/// time a section learns to consume another owner's read model — the Compose
/// spine needs SessionSpaces, and the next step will need something else.
/// Selection and staging stay in [`TuiState`]; nothing here can retrieve,
/// resolve or mutate.
#[derive(Debug, Clone, Copy)]
pub struct WorkspaceReading<'a> {
    pub world: &'a ProjectWorldReadModel,
    pub session_spaces: &'a SessionSpaceRoster,
    pub history: &'a HistoryReading,
    pub factory_work_entry: &'a FactoryWorkEntry,
}

impl<'a> WorkspaceReading<'a> {
    pub fn new(
        world: &'a ProjectWorldReadModel,
        session_spaces: &'a SessionSpaceRoster,
        history: &'a HistoryReading,
    ) -> Self {
        static UNAVAILABLE: std::sync::OnceLock<FactoryWorkEntry> = std::sync::OnceLock::new();
        Self {
            world,
            session_spaces,
            history,
            factory_work_entry: UNAVAILABLE.get_or_init(|| FactoryWorkEntry::Unavailable {
                reason: "no Factory Commission binding supplied to this application".into(),
            }),
        }
    }

    #[must_use]
    pub fn with_factory_work_entry(mut self, entry: &'a FactoryWorkEntry) -> Self {
        self.factory_work_entry = entry;
        self
    }
}

/// What the application boundary could tell us when asked for a reading.
///
/// `Observed` with an empty value is a real, confirmed negative — there is
/// none — and must never be confused with a reading that could not be taken.
/// Every boundary call behind the Workspace is fallible, and folding an error
/// into an empty value makes "nothing exists" and "we could not ask" render
/// identically. That is the one distinction the §5.1 spine's three standings
/// exist to keep, so it is kept once, here, rather than reinvented per read
/// model. Deliberately the same shape as
/// [`aikit_core::credential_world::ProviderRosterKnowledge`], which keeps it
/// for the same reason on the owner's side of the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryReading<T> {
    Observed(T),
    Unreadable { reason: String },
}

impl<T> BoundaryReading<T> {
    /// `Some` only when the reading was actually taken (possibly empty).
    /// `None` means unreadable — a caller must not treat that as emptiness.
    pub fn observed(&self) -> Option<&T> {
        match self {
            Self::Observed(value) => Some(value),
            Self::Unreadable { .. } => None,
        }
    }

    /// The reason a reading could not be taken, if it could not.
    pub fn unreadable_reason(&self) -> Option<&str> {
        match self {
            Self::Observed(_) => None,
            Self::Unreadable { reason } => Some(reason),
        }
    }

    /// Take the reading, keeping the boundary's own error as the reason rather
    /// than discarding it for a default.
    pub fn from_result<E: std::fmt::Display>(result: std::result::Result<T, E>) -> Self {
        match result {
            Ok(value) => Self::Observed(value),
            Err(error) => Self::Unreadable {
                reason: error.to_string(),
            },
        }
    }
}

impl<T: Default> Default for BoundaryReading<T> {
    fn default() -> Self {
        Self::Observed(T::default())
    }
}

/// The authored SessionSpaces the boundary disclosed. Named because the spine
/// reads it by name; it is a `BoundaryReading` like every other.
pub type SessionSpaceRoster = BoundaryReading<Vec<SessionSpaceAuthoredState>>;

/// The history evidence the boundary disclosed.
pub type HistoryReading = BoundaryReading<HistoryReadModel>;

/// Section-specific Project-world lines. Empty means another canonical read model
/// (currently Knowledge relations) owns the presentation for this section.
pub fn project_world_lines(
    state: &TuiState,
    reading: WorkspaceReading<'_>,
    glyphs: Glyphs,
) -> Vec<String> {
    let world = reading.world;
    match state.workspace_section {
        WorkspaceSection::Worlds => {
            let mut lines = context_lines(world, glyphs);
            lines.extend(live_field_lines(state.live_field.as_ref(), glyphs));
            lines
        }
        WorkspaceSection::Compose => compose_lines(state, reading, glyphs),
        WorkspaceSection::Work => work_lines(state, reading, glyphs),
        WorkspaceSection::History => history_lines(reading, glyphs),
        WorkspaceSection::System => system_lines(world, glyphs),
        WorkspaceSection::Knowledge => Vec::new(),
    }
}

fn context_lines(world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let mut lines = vec![
        format!("Context {sep} resolved Project world"),
        String::new(),
        format!("Project  {}", world.project.project.as_str()),
        format!("Binding  {}", locator_label(&world.project.locator)),
    ];

    if let Some(root) = world.context.project_root.as_ref() {
        lines.push(format!("Root     {}", root.display()));
    }
    if let Some(focus) = world.context.task.as_ref() {
        lines.push(format!("Focus    {focus}"));
    }
    lines.push(format!("Host     {}", world.context.host));

    let profiles = world
        .resolution_basis
        .profiles
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    lines.push(format!(
        "Profiles {}",
        if profiles.is_empty() {
            "none disclosed".into()
        } else {
            profiles.join(", ")
        }
    ));

    if world.resolution_basis.scopes.is_empty() {
        lines.push("Scopes   not exposed by application boundary".into());
    } else {
        lines.push(format!(
            "Scopes   {}",
            world
                .resolution_basis
                .scopes
                .iter()
                .map(|scope| format!("{}:{}", scope.kind.as_str(), scope.origin))
                .collect::<Vec<_>>()
                .join(" -> ")
        ));
    }

    lines.push(String::new());
    lines.extend(git_lines(world, sep));

    lines.push(String::new());
    lines.push(format!(
        "Revision catalog {} {sep} resolution {}{}",
        world.effective_revision.catalog_revision,
        world.effective_revision.resolution_hash,
        world
            .effective_revision
            .generation
            .as_ref()
            .map(|generation| format!(" {sep} generation {generation}"))
            .unwrap_or_default(),
    ));
    for warning in &world.warnings {
        lines.push(format!("Boundary {warning}"));
    }
    lines
}

/// The Worlds pane's repository rows, read from
/// `ProjectWorldReadModel::versioned_world`.
///
/// `versioned_world` is `None` for reasons that must not collapse into one
/// picture: nobody attached a versioned-material provider to this reading at
/// all, or a provider looked and this Project genuinely is not under version
/// control. Either way, rendering nothing here — or worse, a blank "clean"
/// section that looks identical to a real clean repository — would tell the
/// person less than they had a right to know. `compose_preview::material` and
/// `compose_spine`'s `WorldsAndBounds` step already carry this exact
/// discipline and this exact absence sentence for the Compose surfaces; this
/// is the same fact read from the same field, so it keeps their words rather
/// than inventing a second vocabulary for one repository.
fn git_lines(world: &ProjectWorldReadModel, sep: &str) -> Vec<String> {
    let Some(versioned) = world.versioned_world.as_ref() else {
        return vec![format!(
            "{:<8} no versioned material provider attached to this reading",
            "Git"
        )];
    };
    let repository = &versioned.repository;

    // `detached` and `branch: None` are the same fact reported twice by the
    // provider (see `NativeGitProvider::inspect`); there is no third case
    // where a branch name exists but `detached` disagrees, so the fallback
    // below is defensive, not a live branch.
    let branch = repository
        .branch
        .as_deref()
        .unwrap_or(if repository.detached { "detached" } else { "unnamed" });

    let mut lines = vec![
        format!("{:<8} {branch}", "Branch"),
        format!("{:<8} {}", "Head", short_revision(repository.head.as_str())),
    ];

    lines.push(match repository.upstream.as_deref() {
        None => format!("{:<8} no upstream tracked", "Upstream"),
        Some(upstream) if repository.ahead == 0 && repository.behind == 0 => {
            format!("{:<8} {upstream} {sep} up to date", "Upstream")
        }
        Some(upstream) => format!(
            "{:<8} {upstream} {sep} {} ahead {sep} {} behind",
            "Upstream", repository.ahead, repository.behind
        ),
    });

    let working = &versioned.working;
    lines.push(if working.is_clean() {
        format!("{:<8} clean", "Working")
    } else {
        format!(
            "{:<8} {} staged {sep} {} unstaged {sep} {} untracked {sep} {} conflicted",
            "Working",
            working.staged.len(),
            working.unstaged.len(),
            working.untracked.len(),
            working.conflicted.len(),
        )
    });

    // `worktrees` names every worktree the provider observed, including this
    // one (`git worktree list` always does); only the *other*, linked
    // worktrees are new information for a reader already looking at this one.
    let linked = versioned
        .worktrees
        .iter()
        .filter(|worktree| worktree.path != repository.worktree_root)
        .map(|worktree| worktree.path.as_str())
        .collect::<Vec<_>>();
    if !linked.is_empty() {
        lines.push(format!(
            "{:<8} {} {sep} {}",
            "Worktrees",
            linked.len(),
            linked.join(", ")
        ));
    }

    lines
}

/// A revision short enough for a status line while remaining unambiguous in
/// any repository this codebase's own scale would produce. Twelve hex
/// characters of a SHA-1 is the same abbreviation length `git` itself favors
/// once a repository has grown past a trivial number of objects.
fn short_revision(revision: &str) -> &str {
    revision.get(..12).unwrap_or(revision)
}

/// The live working-environment block of the Worlds pane (§W6).
///
/// Three states, kept apart because they are three different facts about the
/// machine and collapsing them is how a dashboard starts lying:
///
/// * no reading at all — no provider was attached at this application
///   boundary, so nobody looked;
/// * a reading with no providers — a caller looked and this host is running
///   none;
/// * a reading with providers — each one's health and what it can actually do.
///
/// Provider-native ids are printed as `native <id>` beside their provider,
/// never in the identity column. They are how the provider finds the pane, not
/// what the pane *is*.
fn live_field_lines(field: Option<&LiveWorkingField>, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let mut lines = vec![String::new()];

    let Some(field) = field else {
        lines.push("Environment".into());
        lines.push("  no working-environment provider is attached here".into());
        return lines;
    };

    if field.is_empty() {
        lines.push("Environment".into());
        lines.push("  observed: no working environment is running on this host".into());
        return lines;
    }

    lines.push(format!("Environment {sep} {} observed", field.observed.len()));
    for provider in &field.observed {
        let health = match provider.health {
            WorkingEnvironmentHealth::Healthy => "healthy",
            WorkingEnvironmentHealth::Degraded => "degraded",
            WorkingEnvironmentHealth::Unavailable => "unavailable",
        };
        let mut can = Vec::new();
        if provider.capabilities.open {
            can.push("open");
        }
        if provider.capabilities.focus {
            can.push("focus");
        }
        if provider.capabilities.surface_attach_detach {
            can.push("attach");
        }
        let can = if can.is_empty() {
            "claims nothing".to_string()
        } else {
            can.join("/")
        };
        lines.push(format!(
            "  {} {sep} {health} {sep} {can} {sep} {} bound",
            provider.provider.as_str(),
            provider.bound_subjects,
        ));
    }

    if field.subjects.is_empty() {
        lines.push("  nothing here is bound to a provider yet".into());
        return lines;
    }

    lines.push(String::new());
    lines.push("Reachable".into());
    for subject in &field.subjects {
        lines.push(format!(
            "  {} {sep} {}",
            subject.subject.as_str(),
            subject.semantic_kind
        ));
        for reach in &subject.projections {
            let focus_mark = if reach.focused { " (focused)" } else { "" };
            let verbs = match (reach.can_open(), reach.can_focus()) {
                (true, true) => "open/focus".to_string(),
                (true, false) => "open".to_string(),
                (false, true) => "focus".to_string(),
                // Both withheld: say which condition, so the row explains its
                // own absence instead of looking like an oversight.
                (false, false) => reach
                    .open
                    .map(|withheld| withheld.describe("open"))
                    .unwrap_or_else(|| "unavailable".into()),
            };
            // A live pane shows the native id it is bound to; one that has
            // never been started says so instead of showing a blank column.
            let binding = match reach.native_id.as_deref() {
                Some(native_id) => format!("native {native_id}"),
                None => "not live yet".to_string(),
            };
            lines.push(format!(
                "    {} {sep} {verbs} {sep} {binding}{focus_mark}",
                reach.provider.as_str(),
            ));
        }
    }
    lines
}

/// Compose's human question (spec §5) is "what could I build, and how far
/// have I got building it". §5.1 answers the second half with an ordered
/// ten-step spine, so the spine — not a set of read-model horizon counts — is
/// what this pane leads with. `Capabilities 12 · 8 actions` says how much
/// resolved, which is a different question from where a person stands.
///
/// The four read-model horizon count rows this pane used to carry are retired
/// into the spine rather than kept beside it. They said less than the rows
/// that replaced them — `Information 9 visible sources` against the spine's
/// `9 eligible sources, 4 planned retrievals`; `Actor/Runtime 6 effective or
/// candidate` against `1 harness, 2 models, 2 available` — and keeping both
/// meant two structures competing for one pane.
/// [`crate::project_workspace::ComposeHorizon`] still owns the horizons as a
/// grouping of the read model; it was never the spine.
///
/// The selected Resource's own intent/effective detail is likewise not
/// repeated here. It is already reachable two ways that do not cost the spine
/// its rows — the wide shell's persistent Inspector column, and the `:` Explain
/// overlay, which both render it from `explain_lines` — and the step in hand
/// now discloses its own detail, which is what this pane is for.
fn compose_lines(state: &TuiState, reading: WorkspaceReading<'_>, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let world = reading.world;
    let mut lines = compose_spine_lines(state, reading, glyphs);

    if let Some(agent) = world.actor_runtime.agent.effective.as_ref() {
        lines.push(format!("Agent         {}", agent.resource));
    }
    if let Some(agency) = world.actor_runtime.agency.effective.as_ref() {
        lines.push(format!("Agency        {}", agency.resource));
    }
    for harness in &world.actor_runtime.harnesses {
        lines.push(format!("Harness       {}", harness.resource));
    }
    for model in &world.actor_runtime.models {
        lines.push(format!("Model         {}", model.resource));
    }
    for offer in &world.actor_runtime.execution_offers {
        lines.push(format!("Execution     {}", offer.resource));
    }

    if !state.staged.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "{} staged change{} {sep} preview -> explain -> confirm -> apply",
            state.staged.len(),
            if state.staged.len() == 1 { "" } else { "s" },
        ));
    }
    lines
}

/// Work's three headings are presentation over shared application state, not
/// new lifecycle categories: DIRECT never receives Factory ancestry by
/// proximity; FACTORY contains only admitted native owner readings; ATTENTION
/// contains only explicit owner requests or recognitions in those readings.
fn work_lines(state: &TuiState, reading: WorkspaceReading<'_>, glyphs: Glyphs) -> Vec<String> {
    let world = reading.world;
    let sep = glyphs.separator();
    let mut lines = vec![format!("Work {sep} direct and developmental activity"), String::new()];

    lines.push("DIRECT".into());
    let mut any_runtime = false;
    if let Some(agent) = world.actor_runtime.agent.effective.as_ref() {
        lines.push(format!("  Agent       {}", agent.resource));
        any_runtime = true;
    }
    if let Some(agency) = world.actor_runtime.agency.effective.as_ref() {
        lines.push(format!("  Agency      {}", agency.resource));
        any_runtime = true;
    }
    if let Some(host) = world.actor_runtime.host.effective.as_ref() {
        lines.push(format!("  Host        {}", host.resource));
        any_runtime = true;
    }
    for harness in &world.actor_runtime.harnesses {
        lines.push(format!("  Harness     {}", harness.resource));
        any_runtime = true;
    }
    for model in &world.actor_runtime.models {
        lines.push(format!("  Model       {}", model.resource));
        any_runtime = true;
    }
    for offer in &world.actor_runtime.execution_offers {
        lines.push(format!("  Execution   {}", offer.resource));
        any_runtime = true;
    }
    if let Some(session) = &world.context.session_id {
        lines.push(format!(
            "  Session     {session} {sep} current context; Factory ancestry not supplied"
        ));
        any_runtime = true;
    }
    match reading.session_spaces {
        BoundaryReading::Observed(spaces) => {
            for space in spaces {
                for session in space.agent_sessions.keys() {
                    lines.push(format!(
                        "  Attached    {session} {sep} SessionSpace {} {sep} liveness not observed",
                        space.id()
                    ));
                    any_runtime = true;
                }
            }
        }
        BoundaryReading::Unreadable { reason } => {
            lines.push(format!("  Sessions    unreadable {sep} {reason}"));
        }
    }
    if !any_runtime {
        lines.push("  none observed; external sessions have no inferred Factory ancestry".into());
    }

    lines.push(String::new());
    lines.push("FACTORY".into());
    if world.developmental_work.is_empty() {
        lines.push("  no Factory owner readings admitted".into());
    } else {
        for resource in &world.developmental_work {
            let revision = resource
                .annotations
                .get("factory.owner-revision")
                .map(|value| format!(" {sep} owner r{value}"))
                .unwrap_or_default();
            lines.push(format!(
                "  {:<12} {}{revision}",
                match resource.kind {
                    ResourceKind::Journey => "Journey",
                    ResourceKind::Run => "Run",
                    ResourceKind::WorkflowUnit => "WorkflowUnit",
                    _ => "Factory",
                },
                resource.resource,
            ));
            lines.push(format!("    {}", resource.description));
        }
        for commission in factory_commission_rows(&world.developmental_work, sep) {
            lines.push(format!("  {commission}"));
        }
    }
    match reading.factory_work_entry {
        FactoryWorkEntry::Ready => lines.push(format!(
            "  Start Factory Work  ready {sep} select Work in Navigator, then press :"
        )),
        FactoryWorkEntry::Unavailable { reason } => {
            lines.push(format!("  Start Factory Work  unavailable {sep} {reason}"));
        }
    }
    lines.push(String::new());
    lines.push("ATTENTION".into());
    let attention = factory_attention_rows(&world.developmental_work);
    if attention.is_empty() {
        lines.push("  none in supplied Factory owner readings".into());
    } else {
        lines.extend(attention.into_iter().map(|row| format!("  {row}")));
    }

    if let Some(crate::application::ActionOutcome::FactoryWorkStarted { summary, receipt }) =
        state.action_result.as_ref()
    {
        lines.push(String::new());
        lines.push(format!("  {summary}"));
        lines.push("  Owner receipt".into());
        lines.extend(receipt.lines().map(|line| format!("    {line}")));
    }

    if let Some(selected) = state.selected.as_ref() {
        if let Some(resource) = selected_world_resource(world, selected) {
            lines.push(String::new());
            lines.extend(resource_lines(resource, glyphs));
        }
    }
    lines
}

fn factory_commission_rows(resources: &[ProjectWorldResource], sep: &str) -> Vec<String> {
    let mut rows = BTreeSet::new();
    for resource in resources {
        for (key, encoded) in &resource.annotations {
            let Some(request_ref) = key.strip_prefix("factory.commission.") else {
                continue;
            };
            let Ok(reading) = serde_json::from_str::<serde_json::Value>(encoded) else {
                continue;
            };
            let standing = reading
                .pointer("/commission/request/rootAct/standing")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("standing unavailable");
            rows.insert(format!("Commission {request_ref} {sep} {standing}"));
        }
    }
    rows.into_iter().collect()
}

fn factory_attention_rows(resources: &[ProjectWorldResource]) -> Vec<String> {
    let mut rows = BTreeSet::new();
    for resource in resources {
        let Some(encoded) = resource.annotations.get("factory.owner-reading") else {
            continue;
        };
        let Ok(reading) = serde_json::from_str::<serde_json::Value>(encoded) else {
            continue;
        };
        for (field, label) in [
            ("humanRequests", "HumanRequest"),
            ("recognitions", "Recognition"),
        ] {
            if let Some(values) = reading.get(field).and_then(serde_json::Value::as_array) {
                for value in values {
                    rows.insert(format!("{label} {value}"));
                }
            }
        }
    }
    rows.into_iter().collect()
}

/// System's human question (spec §11) is "what does this installation depend
/// on". `crate::credential_surface::CredentialSetupView` is real, tested
/// product code, but `ProjectWorldReadModel`/`ContextResolution` carry no
/// credential field today, so a live System tab cannot honestly show real
/// credential rows without new provider plumbing (see the PR body's owner-gap
/// note). This is therefore honest-minimal, matching `work_lines`' idiom
/// exactly rather than fabricating a dashboard: it names System as a real,
/// Ctrl+K-navigable destination and says plainly what is not yet disclosed.
fn system_lines(world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let mut lines = vec![
        format!("System {sep} installation and provider disclosure"),
        String::new(),
    ];
    lines.extend(credential_lines(world, glyphs));
    lines.push("Adapters      not exposed by application boundary".into());
    lines.push("Workcell      not exposed by application boundary".into());
    lines.push(String::new());
    lines.push(format!(
        "Revision      catalog {} {sep} resolution {}",
        world.effective_revision.catalog_revision, world.effective_revision.resolution_hash,
    ));
    lines
}

/// The Credentials and Providers rows of §8 System, read from
/// `ProjectWorldReadModel::credential_world`.
///
/// The disclosure's whole point is that "none" and "we could not tell" are
/// different facts, so this renderer never collapses them into one row. An
/// `Unknown` roster says so and carries its reason; an `Observed` empty roster
/// is a confirmed negative and says *that*. Per-credential, only a `Resolved`
/// status with nothing selected is a real "no" — an `Unresolved` status is an
/// open question and is counted separately rather than being added to the
/// failures.
fn credential_lines(world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let disclosure = &world.credential_world;

    let providers = match &disclosure.providers {
        ProviderRosterKnowledge::Observed { providers } if providers.is_empty() => {
            "Providers     none on this machine (roster observed)".to_string()
        }
        ProviderRosterKnowledge::Observed { providers } => {
            let names = providers
                .iter()
                .map(|provider| provider.provider_ref.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("Providers     {} observed {sep} {names}", providers.len())
        }
        ProviderRosterKnowledge::Unknown { reason } => {
            format!("Providers     not observed {sep} {reason}")
        }
    };

    let credentials = if disclosure.credentials.is_empty() {
        match &disclosure.providers {
            // No requirements against a roster we could not read is not a
            // statement about credentials at all.
            ProviderRosterKnowledge::Unknown { .. } => {
                "Credentials   not attempted for this world".to_string()
            }
            ProviderRosterKnowledge::Observed { .. } => {
                "Credentials   none required by this world".to_string()
            }
        }
    } else {
        let total = disclosure.credentials.len();
        let selected = disclosure
            .credentials
            .values()
            .filter(|status| status.is_selected())
            .count();
        let unresolved = disclosure
            .credentials
            .values()
            .filter(|status| matches!(status, CredentialStatusKnowledge::Unresolved { .. }))
            .count();
        let mut row = format!("Credentials   {selected}/{total} resolved to a provider");
        if unresolved > 0 {
            row.push_str(&format!(" {sep} {unresolved} not attempted"));
        }
        row
    };

    vec![credentials, providers]
}

/// Selected-resource resolved intent/effective-state lines. No longer reached
/// as a Workspace tab (`WorkspaceSection::Projection` is retired, see this
/// module's own doc comment) — `crate::v2_render`'s `Overlay::Explain` branch
/// calls this directly, alongside the provider Explain evidence, when a
/// Project world is available.
pub fn explain_lines(state: &TuiState, world: &ProjectWorldReadModel, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let mut lines = vec![
        format!("Explain {sep} authored intent and effective state"),
        String::new(),
    ];
    let Some(selected) = state.selected.as_ref() else {
        lines.push("Select a Resource to inspect its resolved intent/effective state.".into());
        lines.push(format!("Resolution {}", world.effective_revision.resolution_hash));
        return lines;
    };

    lines.push(format!("Resource       {selected}"));
    if let Some(resource) = selected_world_resource(world, selected) {
        lines.extend(resource_lines(resource, glyphs));
    } else if let Some(source) = world
        .information_horizon
        .sources
        .iter()
        .find(|source| &source.resource == selected)
    {
        lines.extend(context_source_lines(source, glyphs));
    } else {
        lines.push("No Project-world resolution record for this shallow navigation Resource.".into());
        lines.push("Use the contextual Explain Action for provider-specific detail.".into());
    }

    lines.push(String::new());
    lines.push(format!("Catalog        {}", world.effective_revision.catalog_revision));
    lines.push(format!("Resolution     {}", world.effective_revision.resolution_hash));
    lines.push(format!(
        "Generation     {}",
        world
            .effective_revision
            .generation
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "not materialised".into()),
    ));
    for warning in &world.warnings {
        lines.push(format!("Boundary       {warning}"));
    }
    lines
}

/// §8 History's human question is "why does this world look like this, what
/// changed, and what can safely be restored".
///
/// The effective-revision numbers answer the first part. The rest is real
/// evidence the boundary already publishes through
/// `ExplainHistoryApplicationService::history_evidence` and which this section
/// never read — it was reachable only through the `:` History contextual
/// action, so the destination named History showed less than the action did.
fn history_lines(reading: WorkspaceReading<'_>, glyphs: Glyphs) -> Vec<String> {
    let world = reading.world;
    let sep = glyphs.separator();
    let mut lines = vec![
        format!("History {sep} effective world lineage"),
        String::new(),
        format!("Catalog revision  {}", world.effective_revision.catalog_revision),
        format!("Resolution hash   {}", world.effective_revision.resolution_hash),
        format!(
            "Generation        {}",
            world
                .effective_revision
                .generation
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "none in this read model".into()),
        ),
        format!("Active projection {} capabilities", world.projection.active_capabilities.len()),
    ];
    if world.warnings.is_empty() {
        lines.push("Boundary          no degraded context disclosures".into());
    } else {
        for warning in &world.warnings {
            lines.push(format!("Boundary          {warning}"));
        }
    }

    lines.push(String::new());
    lines.extend(history_evidence_lines(reading, glyphs));
    lines
}

/// The evidence half of §8, grouped by the kind of thing that happened.
///
/// Two properties travel with every entry because §8 turns on them. Its
/// `authorities` are the epistemic classes that truthfully apply — a
/// SessionSpace receipt is generated evidence of an authored change, so more
/// than one is normal and collapsing them to the first would misreport what
/// kind of fact this is. Its `recoverability` is what can actually be done
/// about it, which is the difference between history a person can act on and
/// history they can only read.
///
/// Familiarity appears here as evidence, never as trust or preference: it is
/// one `HistoryKind` among others, carrying its own authority like the rest.
fn history_evidence_lines(reading: WorkspaceReading<'_>, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let Some(history) = reading.history.observed() else {
        let reason = reading
            .history
            .unreadable_reason()
            .unwrap_or("no reason given");
        return vec![
            format!("Evidence          not read {sep} {reason}"),
            "                  This is an unread history, not an empty one.".into(),
        ];
    };

    if history.entries.is_empty() {
        return vec![
            "Evidence          none recorded for this world".into(),
            "                  Observed and empty, not unread.".into(),
        ];
    }

    let recoverable = history
        .entries
        .iter()
        .filter(|entry| {
            !matches!(
                entry.recoverability,
                HistoryRecoverability::NotRecoverable | HistoryRecoverability::InspectOnly
            )
        })
        .count();
    let mut lines = vec![format!(
        "Evidence          {} entr{} {sep} {recoverable} with a recovery path",
        history.entries.len(),
        if history.entries.len() == 1 { "y" } else { "ies" },
    )];

    // Grouped by kind so a person reads what *sort* of thing changed before
    // reading which. `entries` arrives in the owner's order; grouping presents
    // it without reordering the evidence within a kind.
    let mut kinds: Vec<&'static str> = Vec::new();
    for entry in &history.entries {
        let kind = history_kind_label(entry);
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    for kind in kinds {
        lines.push(String::new());
        lines.push(kind.to_string());
        for entry in history
            .entries
            .iter()
            .filter(|entry| history_kind_label(entry) == kind)
            .take(HISTORY_ROWS)
        {
            lines.push(format!("  {}", entry.summary));
            lines.push(format!(
                "    {} {sep} {}",
                authorities_label(entry),
                recoverability_label(entry.recoverability),
            ));
        }
        let total = history
            .entries
            .iter()
            .filter(|entry| history_kind_label(entry) == kind)
            .count();
        if total > HISTORY_ROWS {
            lines.push(format!("  and {} more", total - HISTORY_ROWS));
        }
    }
    lines
}

/// How many entries one kind lists before it stops enumerating.
const HISTORY_ROWS: usize = 4;

fn history_kind_label(entry: &HistoryEvidence) -> &'static str {
    use aikit_core::explain_history::HistoryKind;
    match entry.kind {
        HistoryKind::Recent => "Recent",
        HistoryKind::Familiarity => "Familiarity",
        HistoryKind::ResolvePath => "Resolve path",
        HistoryKind::KnowledgeRoute => "Knowledge route",
        HistoryKind::KnowledgeFrame => "Knowledge frame",
        HistoryKind::Generation => "Generation",
        HistoryKind::HarnessComposition => "Harness composition",
        HistoryKind::SessionSpace => "SessionSpace",
        HistoryKind::Procedure => "Procedure",
        HistoryKind::LiveActivation => "Live activation",
    }
}

/// Every authority that truthfully applies, not just the first.
fn authorities_label(entry: &HistoryEvidence) -> String {
    if entry.authorities.is_empty() {
        return "no authority declared".to_string();
    }
    entry
        .authorities
        .iter()
        .map(|authority| authority_label(*authority))
        .collect::<Vec<_>>()
        .join("+")
}

fn recoverability_label(recoverability: HistoryRecoverability) -> &'static str {
    match recoverability {
        HistoryRecoverability::InspectOnly => "inspect only",
        HistoryRecoverability::RestageThroughCurrentAuthority => {
            "restageable through current authority"
        }
        HistoryRecoverability::ReplayNavigation => "replayable as navigation",
        HistoryRecoverability::NotRecoverable => "not recoverable",
    }
}

fn selected_world_resource<'a>(
    world: &'a ProjectWorldReadModel,
    selected: &aikit_core::resource::ResourceRef,
) -> Option<&'a ProjectWorldResource> {
    world
        .capability_horizon
        .capabilities
        .iter()
        .chain(world.capability_horizon.actions.iter())
        .chain(world.information_horizon.resolved_sources.iter())
        .chain(world.actor_runtime.models.iter())
        .chain(world.actor_runtime.harnesses.iter())
        .chain(world.actor_runtime.execution_offers.iter())
        .chain(world.developmental_work.iter())
        .chain(world.actor_runtime.agent.effective.iter())
        .chain(world.actor_runtime.agency.effective.iter())
        .chain(world.actor_runtime.host.effective.iter())
        .find(|resource| &resource.resource == selected)
}

fn resource_lines(resource: &ProjectWorldResource, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    let preference = resource
        .intent
        .preference
        .as_ref()
        .map(|preference| format!("preferred rank {} via {}", preference.rank, preference.source))
        .unwrap_or_else(|| "no authored preference".into());
    let authorities = resource
        .intent
        .sources
        .iter()
        .filter_map(|source| source.authority)
        .map(authority_label)
        .collect::<Vec<_>>();
    vec![
        format!("{} {sep} {}", resource.name, resource.kind.as_str()),
        resource.resource.as_str().to_string(),
        format!(
            "Intent        {} {sep} {}{}",
            eligibility_label(&resource.intent.eligibility),
            preference,
            if authorities.is_empty() {
                String::new()
            } else {
                format!(" {sep} provenance {}", authorities.join(", "))
            },
        ),
        format!(
            "Effective     {} {sep} {} provider{}",
            availability_label(&resource.effective.availability),
            resource.effective.providers.len(),
            if resource.effective.providers.len() == 1 { "" } else { "s" },
        ),
    ]
}

fn context_source_lines(source: &ContextSourceHit, glyphs: Glyphs) -> Vec<String> {
    let sep = glyphs.separator();
    vec![
        format!("{} {sep} context-source", source.name),
        source.resource.as_str().to_string(),
        format!(
            "Disclosure    exists={} {sep} known={} {sep} askable={} {sep} retrieved={} {sep} focused={}",
            source.disclosure.exists,
            source.disclosure.known_to_exist,
            source.disclosure.askable,
            source.disclosure.retrieved,
            source.disclosure.focused,
        ),
        format!("Effective     {}", availability_label(&source.availability)),
        "Selection is descriptor-only; retrieval remains an explicit Action.".into(),
    ]
}

fn locator_label(locator: &ProjectBindingLocator) -> String {
    match locator {
        ProjectBindingLocator::LocalDirectory { path } => format!("local {}", path.display()),
        ProjectBindingLocator::Repository { repository } => format!("repository {repository}"),
        ProjectBindingLocator::Remote { locator } => format!("remote {locator}"),
    }
}

fn eligibility_label(eligibility: &Eligibility) -> &'static str {
    match eligibility {
        Eligibility::Eligible => "eligible",
        Eligibility::Undetermined => "eligibility unresolved",
        Eligibility::Ineligible { .. } => "ineligible",
    }
}

fn availability_label(availability: &Availability) -> &'static str {
    match availability {
        Availability::Available => "available",
        Availability::Unresolved { .. } => "availability unresolved",
        Availability::Unavailable { .. } => "unavailable",
    }
}

/// `pub(crate)`: `crate::inspector_render` reuses this exact vocabulary for
/// the Inspector column's Evidence facts rather than inventing a second
/// authority-label mapping.
pub(crate) fn authority_label(authority: SourceAuthority) -> &'static str {
    match authority {
        SourceAuthority::Authored => "authored",
        SourceAuthority::Observed => "observed",
        SourceAuthority::Derived => "derived",
        SourceAuthority::Learned => "learned",
        SourceAuthority::Generated => "generated",
    }
}

#[cfg(test)]
mod credential_disclosure_tests {
    use aikit_core::context::ContextDescriptor;
    use aikit_core::credential::{
        CredentialRef, SecretMaterialisationClass, SecretProviderDescriptor, SecretProviderRef,
        SecretProviderTier,
    };
    use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
    use aikit_core::credential_world::CredentialWorldDisclosure;

    use super::*;

    fn world_with(credential_world: CredentialWorldDisclosure) -> ProjectWorldReadModel {
        let context = ContextDescriptor::for_project("/work/aikit");
        ProjectWorldReadModel::empty(
            ProjectBinding::from_legacy_context(
                ProjectRef::parse("project:aikit").unwrap(),
                ProjectConstituentRef::parse("source:working-tree").unwrap(),
                &context,
            )
            .unwrap(),
            context,
        )
        .with_credential_world(credential_world)
    }

    fn provider(id: &str) -> SecretProviderDescriptor {
        SecretProviderDescriptor {
            provider_ref: SecretProviderRef::new(id).unwrap(),
            provider_kind: id.into(),
            tier: SecretProviderTier::OsSecureStore,
            available: true,
            headless_capable: true,
            assurance: "os-keychain".into(),
            degradation: None,
            supported_credentials: [CredentialRef::new("credential:openai").unwrap()]
                .into_iter()
                .collect(),
            supported_materialisation: [SecretMaterialisationClass::ProviderNativeLease]
                .into_iter()
                .collect(),
            binding_provenance: format!("binding:{id}"),
            revision_or_lease_class: None,
        }
    }

    /// The whole reason `credential_world.rs` exists: "there are none" and "we
    /// could not tell" are different facts. If System renders them the same
    /// way, the disclosure has been wasted at the last step.
    #[test]
    fn an_unread_roster_and_a_confirmed_empty_roster_do_not_render_alike() {
        let unknown = credential_lines(
            &world_with(CredentialWorldDisclosure::not_attempted("no roster gathered")),
            Glyphs::unicode(),
        );
        let observed_empty = credential_lines(
            &world_with(CredentialWorldDisclosure {
                version: "aikit.credential-world/v1".into(),
                providers: ProviderRosterKnowledge::Observed { providers: vec![] },
                credentials: Default::default(),
            }),
            Glyphs::unicode(),
        );

        assert_ne!(unknown, observed_empty);
        assert!(unknown.iter().any(|line| line.contains("not observed")));
        assert!(unknown.iter().any(|line| line.contains("not attempted for this world")));
        assert!(observed_empty
            .iter()
            .any(|line| line.contains("none on this machine (roster observed)")));
        assert!(observed_empty
            .iter()
            .any(|line| line.contains("none required by this world")));
    }

    #[test]
    fn an_observed_roster_names_the_providers_it_actually_saw() {
        let lines = credential_lines(
            &world_with(CredentialWorldDisclosure {
                version: "aikit.credential-world/v1".into(),
                providers: ProviderRosterKnowledge::Observed {
                    providers: vec![provider("keychain"), provider("varlock")],
                },
                credentials: Default::default(),
            }),
            Glyphs::unicode(),
        );

        assert!(lines
            .iter()
            .any(|line| line.contains("2 observed") && line.contains("keychain, varlock")));
    }

    /// A `not_attempted` disclosure must never reach the pane as a claim about
    /// credentials. This is the regression that would re-fabricate exactly the
    /// state the old placeholder row honestly refused to fabricate.
    #[test]
    fn a_not_attempted_disclosure_never_renders_as_a_negative() {
        let lines = credential_lines(
            &world_with(CredentialWorldDisclosure::default()),
            Glyphs::unicode(),
        );
        let rendered = lines.join("\n");

        assert!(!rendered.contains("none required"));
        assert!(!rendered.contains("none on this machine"));
        assert!(!rendered.contains("0/0"));
    }
}

#[cfg(test)]
mod history_evidence_tests {
    use aikit_core::context::ContextDescriptor;
    use aikit_core::explain_history::{HistoryKind, EXPLAIN_HISTORY_VERSION};
    use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
    use aikit_core::resource::ResourceRef;

    use super::*;

    fn world() -> ProjectWorldReadModel {
        let context = ContextDescriptor::for_project("/work/aikit");
        ProjectWorldReadModel::empty(
            ProjectBinding::from_legacy_context(
                ProjectRef::parse("project:aikit").unwrap(),
                ProjectConstituentRef::parse("source:working-tree").unwrap(),
                &context,
            )
            .unwrap(),
            context,
        )
    }

    fn entry(
        id: &str,
        kind: HistoryKind,
        authorities: Vec<SourceAuthority>,
        recoverability: HistoryRecoverability,
    ) -> HistoryEvidence {
        HistoryEvidence {
            schema: EXPLAIN_HISTORY_VERSION.into(),
            id: id.into(),
            kind,
            subject: ResourceRef::parse("capability:deploy").unwrap(),
            authorities,
            occurred_at_unix_ms: None,
            summary: format!("{id} happened"),
            canonical_refs: Vec::new(),
            provenance: Vec::new(),
            recoverability,
            details: Default::default(),
        }
    }

    fn lines(history: &HistoryReading) -> Vec<String> {
        let world = world();
        let spaces = SessionSpaceRoster::default();
        history_lines(
            WorkspaceReading::new(&world, &spaces, history),
            Glyphs::unicode(),
        )
    }

    /// An unread history is not an empty one. The section must say which it is
    /// looking at, or a person reads "nothing has happened" off a failed call.
    #[test]
    fn an_unread_history_does_not_render_as_an_empty_one() {
        let unread = lines(&HistoryReading::Unreadable {
            reason: "application home unavailable".into(),
        });
        let empty = lines(&HistoryReading::Observed(HistoryReadModel::new(Vec::new())));

        let unread = unread.join("\n");
        let empty = empty.join("\n");
        assert!(unread.contains("not read"));
        assert!(unread.contains("application home unavailable"));
        assert!(unread.contains("unread history, not an empty one"));
        assert!(empty.contains("none recorded"));
        assert!(empty.contains("Observed and empty, not unread"));
        assert_ne!(unread, empty);
    }

    /// More than one authority may truthfully apply to one entry — a
    /// SessionSpace receipt is generated evidence of an authored change — so
    /// collapsing to the first would misreport what kind of fact it is.
    #[test]
    fn every_authority_that_applies_is_shown_not_just_the_first() {
        let rendered = lines(&HistoryReading::Observed(HistoryReadModel::new(vec![entry(
            "receipt",
            HistoryKind::SessionSpace,
            vec![SourceAuthority::Generated, SourceAuthority::Authored],
            HistoryRecoverability::RestageThroughCurrentAuthority,
        )])))
        .join("\n");

        assert!(rendered.contains("generated+authored"), "got:\n{rendered}");
    }

    /// §8's point is what can safely be done, so recoverability travels with
    /// every entry and the count names only entries with a real path.
    #[test]
    fn only_entries_with_a_recovery_path_are_counted_as_recoverable() {
        let rendered = lines(&HistoryReading::Observed(HistoryReadModel::new(vec![
            entry(
                "restageable",
                HistoryKind::Generation,
                vec![SourceAuthority::Authored],
                HistoryRecoverability::RestageThroughCurrentAuthority,
            ),
            entry(
                "replayable",
                HistoryKind::KnowledgeRoute,
                vec![SourceAuthority::Observed],
                HistoryRecoverability::ReplayNavigation,
            ),
            entry(
                "inspect-only",
                HistoryKind::Familiarity,
                vec![SourceAuthority::Learned],
                HistoryRecoverability::InspectOnly,
            ),
            entry(
                "gone",
                HistoryKind::LiveActivation,
                vec![SourceAuthority::Observed],
                HistoryRecoverability::NotRecoverable,
            ),
        ])))
        .join("\n");

        assert!(rendered.contains("4 entries"), "got:\n{rendered}");
        assert!(rendered.contains("2 with a recovery path"), "got:\n{rendered}");
        assert!(rendered.contains("not recoverable"));
        assert!(rendered.contains("inspect only"));
    }

    /// Familiarity is evidence like any other kind, carrying its own authority.
    /// It must never render as trust or preference.
    #[test]
    fn familiarity_is_one_evidence_kind_and_not_a_preference() {
        let rendered = lines(&HistoryReading::Observed(HistoryReadModel::new(vec![entry(
            "familiar",
            HistoryKind::Familiarity,
            vec![SourceAuthority::Learned],
            HistoryRecoverability::InspectOnly,
        )])))
        .join("\n");

        assert!(rendered.contains("Familiarity"));
        assert!(rendered.contains("learned"));
        assert!(!rendered.to_lowercase().contains("trust"));
        assert!(!rendered.to_lowercase().contains("preferred"));
    }

    /// Entries group by kind so a person reads what sort of thing changed
    /// before reading which.
    #[test]
    fn entries_group_under_their_kind() {
        let rendered = lines(&HistoryReading::Observed(HistoryReadModel::new(vec![
            entry("g1", HistoryKind::Generation, vec![], HistoryRecoverability::InspectOnly),
            entry("r1", HistoryKind::Recent, vec![], HistoryRecoverability::InspectOnly),
            entry("g2", HistoryKind::Generation, vec![], HistoryRecoverability::InspectOnly),
        ])))
        .join("\n");

        let generation = rendered.find("Generation").unwrap();
        let recent = rendered.find("Recent").unwrap();
        assert!(generation < recent, "kinds keep first-seen order:\n{rendered}");
        assert!(rendered.find("g2 happened").unwrap() < recent, "g2 belongs under Generation");
        assert!(rendered.contains("no authority declared"));
    }

    /// The revision lineage the section already showed is not lost to the
    /// evidence block.
    #[test]
    fn the_effective_revision_lineage_survives_beside_the_evidence() {
        let rendered = lines(&HistoryReading::Observed(HistoryReadModel::new(vec![entry(
            "one",
            HistoryKind::Recent,
            vec![],
            HistoryRecoverability::InspectOnly,
        )])))
        .join("\n");

        assert!(rendered.contains("Catalog revision"));
        assert!(rendered.contains("Resolution hash"));
        assert!(rendered.contains("Evidence"));
    }
}

#[cfg(test)]
mod live_field_tests {
    use super::*;
    use aikit_core::working_environment::{
        NativeBindingKind, ProviderNativeBinding, WorkingEnvironmentCapabilities,
        WorkingEnvironmentObservation, WORKING_ENVIRONMENT_PROVIDER_VERSION,
    };
    use aikit_core::resource::ResourceRef;
    use crate::live_field::live_working_field;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn observation(
        provider: &str,
        native_id: &str,
        health: WorkingEnvironmentHealth,
        open: bool,
    ) -> WorkingEnvironmentObservation {
        WorkingEnvironmentObservation {
            schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
            provider: r(provider),
            provider_version: None,
            health,
            capabilities: WorkingEnvironmentCapabilities {
                discover: true,
                open,
                focus: true,
                terminal_surface: true,
                surface_attach_detach: true,
                ..WorkingEnvironmentCapabilities::default()
            },
            bindings: vec![ProviderNativeBinding {
                kind: NativeBindingKind::Surface,
                native_id: native_id.into(),
                canonical_ref: Some(r("surface/terminal/main/shell")),
                provenance: Vec::new(),
            }],
            focused_native_id: Some(native_id.into()),
            provenance: Vec::new(),
        }
    }

    #[test]
    fn no_provider_attached_and_no_provider_running_read_differently() {
        let nobody_looked = live_field_lines(None, Glyphs::unicode()).join("\n");
        assert!(nobody_looked.contains("no working-environment provider is attached here"));

        let looked = live_working_field(&[], &[]);
        let nothing_running = live_field_lines(Some(&looked), Glyphs::unicode()).join("\n");
        assert!(nothing_running.contains("no working environment is running on this host"));
        assert_ne!(nobody_looked, nothing_running);
    }

    #[test]
    fn two_projections_of_one_subject_are_drawn_under_that_one_subject() {
        let field = live_working_field(
            &[
                observation(
                    "provider/tmux/current",
                    "%12",
                    WorkingEnvironmentHealth::Healthy,
                    true,
                ),
                observation(
                    "provider/cmux/current",
                    "surface-3",
                    WorkingEnvironmentHealth::Healthy,
                    true,
                ),
            ],
            &[r("surface/terminal/main/shell")],
        );
        let rendered = live_field_lines(Some(&field), Glyphs::unicode()).join("\n");

        assert!(rendered.contains("Environment"));
        assert!(rendered.contains("Reachable"));
        // The canonical subject appears once; each provider row hangs under it
        // and carries its own native id, marked as native.
        assert_eq!(rendered.matches("surface/terminal/main/shell").count(), 1);
        assert!(rendered.contains("native %12"));
        assert!(rendered.contains("native surface-3"));
        assert!(rendered.contains("(focused)"));
    }

    #[test]
    fn a_withheld_capability_explains_itself_in_the_row() {
        let field = live_working_field(
            &[observation(
                "provider/tmux/current",
                "%12",
                WorkingEnvironmentHealth::Unavailable,
                true,
            )],
            &[r("surface/terminal/main/shell")],
        );
        let rendered = live_field_lines(Some(&field), Glyphs::unicode()).join("\n");
        assert!(rendered.contains("unavailable"));
        assert!(
            rendered.contains("provider is unavailable"),
            "the row must say why, not just go quiet: {rendered}"
        );
    }

    #[test]
    fn the_block_is_ascii_pure_under_ascii_glyphs() {
        let field = live_working_field(
            &[
                observation(
                    "provider/tmux/current",
                    "%12",
                    WorkingEnvironmentHealth::Healthy,
                    true,
                ),
                observation(
                    "provider/cmux/current",
                    "surface-3",
                    WorkingEnvironmentHealth::Degraded,
                    false,
                ),
            ],
            &[r("surface/terminal/main/shell")],
        );
        for lines in [
            live_field_lines(Some(&field), Glyphs::ascii()),
            live_field_lines(Some(&live_working_field(&[], &[])), Glyphs::ascii()),
            live_field_lines(None, Glyphs::ascii()),
        ] {
            let rendered = lines.join("\n");
            assert!(
                rendered.is_ascii(),
                "ASCII rendering leaked a non-ASCII character: {rendered:?}"
            );
        }
    }
}
