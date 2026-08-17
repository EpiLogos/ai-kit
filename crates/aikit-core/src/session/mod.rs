//! Portable session topology.
//!
//! A session capsule describes a session space *semantically*: views, panes,
//! incremental splits, what runs where, and which capabilities apply. tmux and
//! cmux are then export targets, not the source of truth — a tmux-shaped file
//! would bake in tmux's geometry model and cmux would be a permanent second-class
//! citizen.
//!
//! ## The spec is declarative; the plan is a script
//!
//! [`SessionSpec`] is what a person writes. [`compile`] turns it into a
//! [`SessionPlan`], which is an ordered creation script: the first step of a view
//! creates the view's root pane, and every later step names a pane that an
//! earlier step already created. An adapter walks it top to bottom issuing one
//! command per step and never has to look ahead, resolve a name, or build a graph
//! of its own — which is exactly what makes the same capsule launchable in two
//! multiplexers with different geometry models.
//!
//! ## Isolation is opt-in
//!
//! [`TaskSpec::isolation`] defaults to [`Isolation::Shared`]. The source
//! specification made a git worktree the implied default for agent tasks; AIKit
//! does not, because a worktree costs a checkout, a branch, disk and a teardown
//! decision, and most tasks do not want one. The legacy `worktree = true`
//! spelling is still understood so older manifests keep meaning what they said.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::context::Isolation;
use crate::error::{err, AikitError, Result};
use crate::platform::MuxKind;
use crate::profile::{ConfigTable, PoolPatch};

pub const SUPPORTED_SCHEMA: u32 = 1;

// ---------------------------------------------------------------------------
// Enumerations
// ---------------------------------------------------------------------------

/// Where a new pane goes relative to the one it splits from.
///
/// Expressed as a compass direction rather than tmux's `-h`/`-v`, because
/// "horizontal split" means opposite things in different multiplexers and a
/// portable format cannot afford that ambiguity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    Left,
    #[default]
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Left => "left",
            Direction::Right => "right",
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }

    /// True for left/right. Adapters that only speak in axes need this.
    pub fn is_horizontal(self) -> bool {
        matches!(self, Direction::Left | Direction::Right)
    }
}

/// What to do when a pane's command exits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Restart {
    /// Leave the dead pane alone. The default: a command that exited may have
    /// printed something the user still needs to read.
    #[default]
    Never,
    IfExited,
    Always,
}

impl Restart {
    pub fn as_str(self) -> &'static str {
        match self {
            Restart::Never => "never",
            Restart::IfExited => "if-exited",
            Restart::Always => "always",
        }
    }
}

/// Whether `aikit session up` should attach after reconciling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Attach {
    #[default]
    Always,
    Never,
    /// Attach only when this invocation actually created the session, so that
    /// re-running `session up` from inside a pane does not nest a client.
    IfCreated,
}

impl Attach {
    pub fn as_str(self) -> &'static str {
        match self {
            Attach::Always => "always",
            Attach::Never => "never",
            Attach::IfCreated => "if-created",
        }
    }
}

/// How long the session space outlives its clients.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lifecycle {
    /// Survives detach. The default, because losing work to a dropped ssh
    /// connection is the failure a multiplexer exists to prevent.
    #[default]
    Persist,
    /// Torn down when the last client detaches.
    Ephemeral,
}

impl Lifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Lifecycle::Persist => "persist",
            Lifecycle::Ephemeral => "ephemeral",
        }
    }
}

/// Where a spawned agent task should appear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Placement {
    Current,
    #[default]
    NewPane,
    NewView,
    Background,
}

impl Placement {
    pub fn as_str(self) -> &'static str {
        match self {
            Placement::Current => "current",
            Placement::NewPane => "new-pane",
            Placement::NewView => "new-view",
            Placement::Background => "background",
        }
    }
}

// ---------------------------------------------------------------------------
// Backend selection
// ---------------------------------------------------------------------------

/// Which multiplexer to use, plus whatever that multiplexer alone understands.
///
/// TOML cannot carry both a scalar and a subtable under one key, so
/// `backend = "tmux"` and a `[backend]` table are two spellings of the same
/// thing. The table form is what a spec uses when it needs `[backend.tmux]` or
/// `[backend.cmux]` extensions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BackendSpec {
    /// `None` means `auto`: the adapter picks based on what is running.
    pub mux: Option<MuxKind>,
    /// Per-multiplexer opaque options, keyed by multiplexer name.
    pub extensions: BTreeMap<String, ConfigTable>,
    /// Extension tables addressed to a multiplexer this build does not know.
    /// Kept so [`compile`] can warn about them by name.
    pub unknown_extensions: Vec<String>,
}

// ---------------------------------------------------------------------------
// The spec
// ---------------------------------------------------------------------------

/// One pane, declared as an incremental split of a pane declared elsewhere in the
/// same view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneSpec {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// The pane this one splits off. Absent means "this is the view's root pane".
    #[serde(default)]
    pub split_from: Option<String>,
    #[serde(default)]
    pub direction: Option<Direction>,
    /// Fraction of the parent given to the new pane. Must be in `(0, 1)`.
    #[serde(default)]
    pub ratio: Option<f64>,
    #[serde(default)]
    pub focus: bool,
    #[serde(default)]
    pub restart: Restart,
    /// argv, not a shell string: a shell string would need quoting rules that
    /// differ per multiplexer and per shell.
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// A capability patch scoped to this pane alone.
    #[serde(default)]
    pub capabilities: PoolPatch,
}

/// A window (tmux) or workspace surface group (cmux).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewSpec {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub panes: Vec<PaneSpec>,
}

/// An agent task declared alongside the topology.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub agent: String,
    /// **Defaults to [`Isolation::Shared`].** See the module documentation.
    #[serde(default)]
    pub isolation: Isolation,
    /// The git ref an isolated task branches from. Meaningless when shared, and
    /// the adapter says so rather than pretending otherwise.
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default)]
    pub capabilities: PoolPatch,
}

impl TaskSpec {
    /// Does this task get a working tree its siblings cannot see?
    ///
    /// The single question client adapters ask before deciding whether a
    /// per-task native skill directory is even possible.
    pub fn is_isolated(&self) -> bool {
        self.isolation.is_isolated()
    }
}

/// A portable session capsule's `spec` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSpec {
    pub schema: u32,
    pub id: String,
    pub name: String,
    /// Left exactly as written. Expanding `~` needs `HOME`, which is I/O, and
    /// resolving it here would bake one machine's answer into a portable file.
    #[serde(default)]
    pub root: Option<PathBuf>,
    #[serde(default)]
    pub backend: BackendSpec,
    #[serde(default)]
    pub attach: Attach,
    #[serde(default)]
    pub lifecycle: Lifecycle,
    #[serde(default)]
    pub capabilities: PoolPatch,
    #[serde(default)]
    pub views: Vec<ViewSpec>,
    #[serde(default)]
    pub task: Option<TaskSpec>,
}

impl SessionSpec {
    pub fn from_toml_str(src: &str) -> Result<Self> {
        let raw: RawSpec = toml::from_str(src).map_err(|e| {
            AikitError::new(
                "session.parse_error",
                format!("could not parse session spec: {e}"),
            )
        })?;
        raw.into_spec()
    }

    pub fn compile(&self) -> Result<SessionPlan> {
        compile(self)
    }
}

// ---------------------------------------------------------------------------
// The wire form
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSpec {
    schema: u32,
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    root: Option<PathBuf>,
    #[serde(default)]
    backend: Option<RawBackend>,
    #[serde(default)]
    attach: Option<RawAttach>,
    #[serde(default)]
    lifecycle: Lifecycle,
    #[serde(default)]
    capabilities: PoolPatch,
    #[serde(default)]
    views: Vec<ViewSpec>,
    #[serde(default)]
    task: Option<RawTask>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawBackend {
    Named(String),
    Table(BTreeMap<String, toml::Value>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawAttach {
    Flag(bool),
    Named(String),
}

/// The `[task]` table before the two isolation spellings are reconciled.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTask {
    agent: String,
    #[serde(default)]
    isolation: Option<Isolation>,
    /// The legacy spelling, from when a worktree was the implied default.
    #[serde(default)]
    worktree: Option<bool>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    placement: Placement,
    #[serde(default)]
    capabilities: PoolPatch,
}

impl RawSpec {
    fn into_spec(self) -> Result<SessionSpec> {
        if self.schema != SUPPORTED_SCHEMA {
            return Err(AikitError::new(
                "session.unsupported_schema",
                format!(
                    "session schema {} is not supported (this build understands {})",
                    self.schema, SUPPORTED_SCHEMA
                ),
            )
            .with("id", self.id.clone()));
        }
        if self.id.trim().is_empty() {
            return err("session.invalid", "a session spec needs an id");
        }

        let attach = match self.attach {
            None => Attach::default(),
            Some(RawAttach::Flag(true)) => Attach::Always,
            Some(RawAttach::Flag(false)) => Attach::Never,
            Some(RawAttach::Named(name)) => match name.as_str() {
                "always" => Attach::Always,
                "never" => Attach::Never,
                "if-created" => Attach::IfCreated,
                other => {
                    return err(
                        "session.invalid",
                        format!("`{other}` is not an attach policy (always, never, if-created)"),
                    )
                }
            },
        };

        let backend = match self.backend {
            None => BackendSpec::default(),
            Some(RawBackend::Named(name)) => BackendSpec {
                mux: parse_backend_name(&name)?,
                ..Default::default()
            },
            Some(RawBackend::Table(table)) => {
                let mut backend = BackendSpec::default();
                for (key, value) in table {
                    match key.as_str() {
                        "kind" => {
                            let name = value.as_str().ok_or_else(|| {
                                AikitError::new(
                                    "session.invalid",
                                    "[backend].kind must be a multiplexer name",
                                )
                            })?;
                            backend.mux = parse_backend_name(name)?;
                        }
                        "tmux" | "cmux" | "plain" => {
                            let table = value.as_table().cloned().ok_or_else(|| {
                                AikitError::new(
                                    "session.invalid",
                                    format!("[backend.{key}] must be a table of options"),
                                )
                            })?;
                            backend.extensions.insert(key, table);
                        }
                        // Recorded rather than rejected: see `compile`.
                        other => backend.unknown_extensions.push(other.to_string()),
                    }
                }
                backend
            }
        };

        let task = match self.task {
            None => None,
            Some(raw) => Some(TaskSpec {
                isolation: reconcile_isolation(&raw)?,
                agent: raw.agent,
                base: raw.base,
                placement: raw.placement,
                capabilities: raw.capabilities,
            }),
        };

        let id = self.id;
        Ok(SessionSpec {
            schema: self.schema,
            name: self.name.unwrap_or_else(|| id.clone()),
            id,
            root: self.root,
            backend,
            attach,
            lifecycle: self.lifecycle,
            capabilities: self.capabilities,
            views: self.views,
            task,
        })
    }
}

fn parse_backend_name(name: &str) -> Result<Option<MuxKind>> {
    if name == "auto" {
        return Ok(None);
    }
    name.parse::<MuxKind>().map(Some).map_err(|_| {
        AikitError::new(
            "session.invalid",
            format!("`{name}` is not a backend (auto, tmux, cmux, plain)"),
        )
    })
}

/// Reconcile `isolation` with the legacy `worktree` flag.
///
/// Both spellings are accepted, but they may not disagree: silently preferring
/// one would mean a manifest that says two contradictory things about isolation
/// quietly gets one of them, and isolation decides what a client adapter is even
/// able to project.
fn reconcile_isolation(raw: &RawTask) -> Result<Isolation> {
    let from_flag = raw.worktree.map(|on| {
        if on {
            Isolation::Worktree
        } else {
            Isolation::Shared
        }
    });
    match (raw.isolation, from_flag) {
        (None, None) => Ok(Isolation::default()),
        (Some(explicit), None) => Ok(explicit),
        (None, Some(legacy)) => Ok(legacy),
        (Some(explicit), Some(legacy)) if explicit == legacy => Ok(explicit),
        (Some(explicit), Some(legacy)) => Err(AikitError::new(
            "task.isolation_conflict",
            format!(
                "the task declares isolation = \"{}\" and worktree = {}, which imply \
                 \"{}\"; remove one",
                explicit.as_str(),
                raw.worktree.unwrap_or(false),
                legacy.as_str()
            ),
        )
        .with("isolation", explicit.as_str())
        .with("worktree", raw.worktree.unwrap_or(false).to_string())),
    }
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

/// How a pane is created from an already-created one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Split {
    /// A pane id that an earlier step in the same view already created.
    pub from: String,
    pub direction: Direction,
    #[serde(default)]
    pub ratio: Option<f64>,
}

/// One creation command's worth of instruction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneStep {
    pub view: String,
    pub pane: String,
    #[serde(default)]
    pub name: Option<String>,
    /// `None` for the view's root pane, which is created with the view itself.
    #[serde(default)]
    pub split: Option<Split>,
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    pub restart: Restart,
    pub focus: bool,
    pub capabilities: PoolPatch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewPlan {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// In creation order: the root pane first, then splits of already-created panes.
    pub steps: Vec<PaneStep>,
    /// The pane to focus once the view is built.
    #[serde(default)]
    pub focus: Option<String>,
}

/// An ordered, executable session creation script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPlan {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub root: Option<PathBuf>,
    #[serde(default)]
    pub mux: Option<MuxKind>,
    pub attach: Attach,
    pub lifecycle: Lifecycle,
    pub capabilities: PoolPatch,
    pub views: Vec<ViewPlan>,
    /// Options for multiplexers this build understands, keyed by name.
    pub backend_extensions: BTreeMap<String, ConfigTable>,
    #[serde(default)]
    pub task: Option<TaskSpec>,
    pub warnings: Vec<String>,
}

impl SessionPlan {
    pub fn view(&self, id: &str) -> Option<&ViewPlan> {
        self.views.iter().find(|v| v.id == id)
    }

    /// Total panes across all views, for the "this will create N panes" preview.
    pub fn pane_count(&self) -> usize {
        self.views.iter().map(|v| v.steps.len()).sum()
    }
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

/// Validate a spec and turn it into an ordered creation script.
pub fn compile(spec: &SessionSpec) -> Result<SessionPlan> {
    let mut warnings: Vec<String> = Vec::new();

    for unknown in &spec.backend.unknown_extensions {
        warnings.push(format!(
            "[backend.{unknown}] is addressed to a multiplexer this build does not know; it was \
             ignored"
        ));
    }

    if spec.views.is_empty() {
        return Err(AikitError::new(
            "session.no_views",
            format!("session `{}` declares no views", spec.id),
        )
        .with("session", spec.id.clone()));
    }

    let mut seen_views: BTreeSet<&str> = BTreeSet::new();
    let mut views: Vec<ViewPlan> = Vec::with_capacity(spec.views.len());

    for view in &spec.views {
        if !seen_views.insert(view.id.as_str()) {
            return Err(AikitError::new(
                "session.duplicate_view",
                format!(
                    "session `{}` declares the view `{}` twice",
                    spec.id, view.id
                ),
            )
            .with("session", spec.id.clone())
            .with("view", view.id.clone()));
        }
        views.push(compile_view(spec, view)?);
    }

    Ok(SessionPlan {
        id: spec.id.clone(),
        name: spec.name.clone(),
        root: spec.root.clone(),
        mux: spec.backend.mux,
        attach: spec.attach,
        lifecycle: spec.lifecycle,
        capabilities: spec.capabilities.clone(),
        views,
        backend_extensions: spec.backend.extensions.clone(),
        task: spec.task.clone(),
        warnings,
    })
}

fn compile_view(spec: &SessionSpec, view: &ViewSpec) -> Result<ViewPlan> {
    if view.panes.is_empty() {
        return Err(AikitError::new(
            "session.empty_view",
            format!(
                "view `{}` has no panes; there would be nothing to create",
                view.id
            ),
        )
        .with("session", spec.id.clone())
        .with("view", view.id.clone()));
    }

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for pane in &view.panes {
        if !seen.insert(pane.id.as_str()) {
            return Err(AikitError::new(
                "session.duplicate_pane",
                format!("view `{}` declares the pane `{}` twice", view.id, pane.id),
            )
            .with("view", view.id.clone())
            .with("pane", pane.id.clone()));
        }
    }

    let focused: Vec<&str> = view
        .panes
        .iter()
        .filter(|p| p.focus)
        .map(|p| p.id.as_str())
        .collect();
    if focused.len() > 1 {
        return Err(AikitError::new(
            "session.multiple_focus",
            format!(
                "view `{}` focuses {} panes ({}); only one pane can hold the cursor",
                view.id,
                focused.len(),
                focused.join(", ")
            ),
        )
        .with("view", view.id.clone())
        .with("panes", focused.join(", ")));
    }

    for pane in &view.panes {
        if let Some(parent) = &pane.split_from {
            if !seen.contains(parent.as_str()) {
                return Err(AikitError::new(
                    "session.unknown_split_parent",
                    format!(
                        "pane `{}` in view `{}` splits from `{parent}`, which that view does not \
                         declare",
                        pane.id, view.id
                    ),
                )
                .with("view", view.id.clone())
                .with("pane", pane.id.clone())
                .with("split_from", parent.clone()));
            }
        }
        if let Some(ratio) = pane.ratio {
            if !(ratio > 0.0 && ratio < 1.0) {
                return Err(AikitError::new(
                    "session.invalid_ratio",
                    format!(
                        "pane `{}` in view `{}` asks for a split ratio of {ratio}; a ratio must be \
                         between 0 and 1 exclusive",
                        pane.id, view.id
                    ),
                )
                .with("view", view.id.clone())
                .with("pane", pane.id.clone())
                .with("ratio", ratio.to_string()));
            }
        }
    }

    let roots: Vec<&PaneSpec> = view
        .panes
        .iter()
        .filter(|p| p.split_from.is_none())
        .collect();
    if roots.len() > 1 {
        return Err(AikitError::new(
            "session.multiple_root_panes",
            format!(
                "view `{}` declares {} panes that split from nothing ({}); a view starts with \
                 exactly one pane and every other pane is a split of it",
                view.id,
                roots.len(),
                roots
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
        .with("view", view.id.clone()));
    }

    let ordered = order_panes(view)?;
    let steps: Vec<PaneStep> = ordered
        .into_iter()
        .map(|pane| PaneStep {
            view: view.id.clone(),
            pane: pane.id.clone(),
            name: pane.name.clone(),
            split: pane.split_from.as_ref().map(|from| Split {
                from: from.clone(),
                direction: pane.direction.unwrap_or_default(),
                ratio: pane.ratio,
            }),
            command: pane.command.clone(),
            cwd: pane.cwd.clone(),
            restart: pane.restart,
            focus: pane.focus,
            capabilities: pane.capabilities.clone(),
        })
        .collect();

    Ok(ViewPlan {
        id: view.id.clone(),
        name: view.name.clone(),
        focus: focused.first().map(|id| id.to_string()),
        steps,
    })
}

/// Order panes so every parent precedes its children, keeping declaration order
/// among panes that do not constrain each other.
///
/// A view with no root, or with a split cycle, both show up here as panes that
/// can never become ready. They are the same failure from the adapter's point of
/// view — there is no first command to issue — so they share an error code.
fn order_panes(view: &ViewSpec) -> Result<Vec<&PaneSpec>> {
    let mut ordered: Vec<&PaneSpec> = Vec::with_capacity(view.panes.len());
    let mut created: BTreeSet<&str> = BTreeSet::new();
    let mut pending: Vec<&PaneSpec> = view.panes.iter().collect();

    while !pending.is_empty() {
        let ready = pending.iter().position(|pane| match &pane.split_from {
            None => true,
            Some(parent) => created.contains(parent.as_str()),
        });
        let Some(index) = ready else {
            let stuck: Vec<&str> = pending.iter().map(|p| p.id.as_str()).collect();
            return Err(AikitError::new(
                "session.split_cycle",
                format!(
                    "the panes {} in view `{}` split from each other in a cycle, so no pane can be \
                     created first",
                    stuck.join(", "),
                    view.id
                ),
            )
            .with("view", view.id.clone())
            .with("panes", stuck.join(", ")));
        };
        let pane = pending.remove(index);
        created.insert(pane.id.as_str());
        ordered.push(pane);
    }

    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_direction_knows_its_axis_so_axis_only_adapters_can_translate() {
        assert!(Direction::Left.is_horizontal());
        assert!(Direction::Right.is_horizontal());
        assert!(!Direction::Up.is_horizontal());
        assert!(!Direction::Down.is_horizontal());
    }

    #[test]
    fn the_conservative_defaults_are_the_declared_ones() {
        assert_eq!(Restart::default(), Restart::Never);
        assert_eq!(Lifecycle::default(), Lifecycle::Persist);
        assert_eq!(Attach::default(), Attach::Always);
        assert_eq!(Placement::default(), Placement::NewPane);
        assert_eq!(Isolation::default(), Isolation::Shared);
    }

    #[test]
    fn a_task_reports_isolation_through_the_one_question_adapters_ask() {
        let shared = TaskSpec {
            agent: "claude".into(),
            isolation: Isolation::Shared,
            base: None,
            placement: Placement::default(),
            capabilities: PoolPatch::default(),
        };
        assert!(!shared.is_isolated());
        assert!(TaskSpec {
            isolation: Isolation::Directory,
            ..shared
        }
        .is_isolated());
    }

    #[test]
    fn a_plan_counts_the_panes_it_would_create() {
        let spec = SessionSpec::from_toml_str(
            r#"
schema = 1
id = "t"
name = "T"

[[views]]
id = "a"
[[views.panes]]
id = "p"
[[views.panes]]
id = "q"
split_from = "p"

[[views]]
id = "b"
[[views.panes]]
id = "r"
"#,
        )
        .unwrap();
        let plan = compile(&spec).unwrap();
        assert_eq!(plan.pane_count(), 3);
        assert!(plan.view("b").is_some());
        assert!(plan.view("nope").is_none());
    }
}
