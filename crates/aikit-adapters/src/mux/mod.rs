//! Multiplexer adapters and the semantic contract they share.
//!
//! tmux and cmux are not variations on one geometry model, and this module does
//! not pretend they are. What they share is a set of *semantic* operations —
//! "make this session exist", "put this command somewhere the user can see it",
//! "show the palette", "say something on the status surface" — and each adapter
//! satisfies them with its own primitives. [`MuxCapabilities`] is how an adapter
//! declines the ones it has no honest way to provide.
//!
//! ## Conservative by construction
//!
//! [`MuxCapabilities::default`] is all-false and [`ReconcileMode::default`] is
//! the non-destructive one. Both defaults are chosen so that the failure mode of
//! a half-written adapter is "does less than it could" rather than "closed the
//! panes I was working in".

pub mod plain;
pub mod stack;
pub mod tmux;

pub mod cmux;

use std::collections::BTreeMap;
use std::path::PathBuf;

use aikit_core::context::Isolation;
use aikit_core::id::{ContextId, SessionId};
use aikit_core::platform::MuxKind;
use aikit_core::session::{Direction, Placement, SessionPlan};
use aikit_core::Result;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// What a session should tell its children about the AIKit context it belongs to.
///
/// Multiplexer ids are bindings, not identity. These values are AIKit's own, and
/// are written both into the session environment (so children inherit them) and
/// into user options (so they survive a detach and can be read back).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionIdentity {
    pub session_id: Option<SessionId>,
    pub context_id: Option<ContextId>,
    pub project_root: Option<PathBuf>,
    /// `AIKIT_VIEW`: the stable `current` symlink, never a generation directory,
    /// so the value stays correct across a generation swap.
    pub view_root: Option<PathBuf>,
    pub profile: Option<String>,
    pub isolation: Isolation,
}

impl SessionIdentity {
    /// The variables every pane in the session should inherit.
    ///
    /// A `Vec` rather than a map: the order is part of what the argv tests pin
    /// down, and an unordered container would make them depend on hashing.
    pub fn environment(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        if let Some(id) = &self.session_id {
            out.push(("AIKIT_SESSION_ID".into(), id.to_string()));
        }
        if let Some(id) = &self.context_id {
            out.push(("AIKIT_CONTEXT_ID".into(), id.to_string()));
        }
        if let Some(root) = &self.project_root {
            out.push(("AIKIT_PROJECT_ROOT".into(), root.display().to_string()));
        }
        if let Some(view) = &self.view_root {
            out.push(("AIKIT_VIEW".into(), view.display().to_string()));
        }
        out
    }

    /// The `@aikit_*` user options, for status rendering and recovery.
    pub fn user_options(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        if let Some(id) = &self.session_id {
            out.push((crate::mux::tmux::SESSION_OPTION.into(), id.to_string()));
        }
        if let Some(profile) = &self.profile {
            out.push((crate::mux::tmux::PROFILE_OPTION.into(), profile.clone()));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// What a multiplexer can actually do.
///
/// All-false by default: an adapter is trusted with a capability only when it
/// has said, in code, that it has one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MuxCapabilities {
    /// An overlay window that floats above the current surface and can run an
    /// arbitrary command. tmux's `display-popup`. Not assumed for anything else.
    pub true_popup: bool,
    /// Named surfaces that hold panes: tmux windows, cmux workspaces.
    pub workspaces: bool,
    /// A container that groups workspaces, so a multi-view session can be one
    /// switchable unit.
    pub workspace_groups: bool,
    /// Top-level OS windows the adapter can create and address.
    pub windows: bool,
    /// Splittable panes inside a workspace.
    pub panes: bool,
    /// A first-class browser surface, rather than "run a browser in a terminal".
    pub browser_surface: bool,
    /// Somewhere to attach structured key/value status that survives a detach.
    pub status_metadata: bool,
    /// Native notifications.
    pub notifications: bool,
    /// The multiplexer can be driven from outside the session it is showing.
    pub remote_control: bool,
}

impl MuxCapabilities {
    /// Where the palette should be shown when the caller has no preference.
    ///
    /// A multiplexer with no popup primitive gets an inline palette rather than a
    /// simulated one: a fake popup drawn over the user's scrollback is worse than
    /// an honest inline modal.
    pub fn default_palette_host(&self) -> UiHost {
        if self.true_popup {
            UiHost::TruePopup {
                target: String::new(),
            }
        } else {
            UiHost::InlineCurrentTerminal
        }
    }
}

// ---------------------------------------------------------------------------
// Reconciliation
// ---------------------------------------------------------------------------

/// How hard `ensure_session` should try to make reality match the plan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReconcileMode {
    /// **The default, and non-destructive.** Create what is missing, attach to
    /// what is there, and leave everything else — including panes the user made
    /// by hand and commands already running — exactly as it is.
    #[default]
    CreateOrAttach,
    /// Make the session match the plan, closing panes the plan does not declare.
    /// Only ever reached because someone asked for it.
    Exact,
}

impl ReconcileMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ReconcileMode::CreateOrAttach => "create-or-attach",
            ReconcileMode::Exact => "exact",
        }
    }

    /// The one question the destructive paths ask.
    pub fn may_close_panes(self) -> bool {
        matches!(self, ReconcileMode::Exact)
    }
}

// ---------------------------------------------------------------------------
// Presence and location
// ---------------------------------------------------------------------------

/// Whether a multiplexer is here, and whether we are inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxPresence {
    pub kind: MuxKind,
    pub installed: bool,
    pub version: Option<String>,
    /// A server/app is up, so an existing session could be attached to.
    pub server_running: bool,
    /// This process is running inside it.
    pub inside: bool,
    /// Why it is not usable, when it is not.
    pub detail: Option<String>,
}

impl MuxPresence {
    pub fn absent(kind: MuxKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            installed: false,
            version: None,
            server_running: false,
            inside: false,
            detail: Some(detail.into()),
        }
    }

    pub fn is_usable(&self) -> bool {
        self.installed
    }

    pub fn describe(&self) -> String {
        if !self.installed {
            return match &self.detail {
                Some(detail) => format!("{}: not available — {detail}", self.kind),
                None => format!("{}: not available", self.kind),
            };
        }
        let version = self.version.as_deref().unwrap_or("unknown version");
        let where_ = if self.inside {
            " (inside)"
        } else if self.server_running {
            " (running)"
        } else {
            ""
        };
        format!("{} {version}{where_}", self.kind)
    }
}

/// Where the caller currently is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxLocation {
    pub kind: MuxKind,
    /// Shown prominently whenever `remote` is true: a command typed in the wrong
    /// host is the most expensive mistake a session tool can invite.
    pub host: String,
    pub remote: bool,
    /// Human project label, for the palette title bar.
    pub project: Option<String>,
    pub session: Option<String>,
    pub view: Option<String>,
    pub surface: Option<String>,
}

impl MuxLocation {
    /// A location that knows nothing except which multiplexer it is not in.
    pub fn nowhere(kind: MuxKind) -> Self {
        Self {
            kind,
            host: String::new(),
            remote: false,
            project: None,
            session: None,
            view: None,
            surface: None,
        }
    }

    pub fn target(&self) -> MuxTarget {
        MuxTarget {
            kind: self.kind,
            session: self.session.clone(),
            view: self.view.clone(),
            surface: self.surface.clone(),
        }
    }

    /// `payments · staging-box · tmux`. The host segment appears only when the
    /// session is somewhere other than here.
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(project) = &self.project {
            parts.push(project.clone());
        }
        if self.remote && !self.host.is_empty() {
            parts.push(self.host.clone());
        }
        parts.push(self.kind.to_string());
        parts.join(" · ")
    }
}

/// A pane, surface, view or session, addressed the way its multiplexer expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxTarget {
    pub kind: MuxKind,
    pub session: Option<String>,
    pub view: Option<String>,
    pub surface: Option<String>,
}

impl MuxTarget {
    pub fn session(kind: MuxKind, session: impl Into<String>) -> Self {
        Self {
            kind,
            session: Some(session.into()),
            view: None,
            surface: None,
        }
    }

    pub fn view(kind: MuxKind, session: impl Into<String>, view: impl Into<String>) -> Self {
        Self {
            kind,
            session: Some(session.into()),
            view: Some(view.into()),
            surface: None,
        }
    }

    pub fn surface(kind: MuxKind, surface: impl Into<String>) -> Self {
        Self {
            kind,
            session: None,
            view: None,
            surface: Some(surface.into()),
        }
    }

    /// The most specific thing this target names.
    ///
    /// Both multiplexers accept an id for the innermost object without needing
    /// the enclosing ones, so narrowing here is always right and never ambiguous.
    pub fn selector(&self) -> String {
        self.surface
            .clone()
            .or_else(|| self.view.clone())
            .or_else(|| self.session.clone())
            .unwrap_or_default()
    }

    pub fn is_addressable(&self) -> bool {
        !self.selector().is_empty()
    }
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

/// The result of making a session exist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionBinding {
    pub kind: Option<MuxKind>,
    /// tmux session name, or the cmux workspace/group id.
    pub session: String,
    /// True only when *this* call created it. `attach = if-created` depends on
    /// the difference.
    pub created: bool,
    /// Plan view id → multiplexer view id.
    pub views: BTreeMap<String, String>,
    /// `"<view>/<pane>"` → multiplexer surface id.
    pub surfaces: BTreeMap<String, String>,
    /// What this call changed, in the words the palette prints.
    pub actions: Vec<String>,
    /// What this call deliberately left alone, and why.
    pub preserved: Vec<String>,
    pub warnings: Vec<String>,
}

impl SessionBinding {
    pub fn new(kind: MuxKind, session: impl Into<String>) -> Self {
        Self {
            kind: Some(kind),
            session: session.into(),
            ..Default::default()
        }
    }

    pub fn surface_of(&self, view: &str, pane: &str) -> Option<&str> {
        self.surfaces
            .get(&format!("{view}/{pane}"))
            .map(String::as_str)
    }

    pub fn record(&mut self, action: impl Into<String>) {
        self.actions.push(action.into());
    }

    pub fn preserve(&mut self, note: impl Into<String>) {
        self.preserved.push(note.into());
    }
}

// ---------------------------------------------------------------------------
// Spawning
// ---------------------------------------------------------------------------

/// "Put this command somewhere the user can see it."
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpawnRequest {
    pub placement: Placement,
    /// argv, not a shell string: quoting rules differ per multiplexer and shell.
    pub command: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub name: Option<String>,
    pub env: BTreeMap<String, String>,
    pub direction: Direction,
    /// Fraction of the split parent given to the new surface.
    pub ratio: Option<f64>,
    /// Where to split from. `None` means the caller's current location.
    pub target: Option<MuxTarget>,
}

impl SpawnRequest {
    pub fn new(placement: Placement, command: Vec<String>) -> Self {
        Self {
            placement,
            command,
            ..Default::default()
        }
    }

    #[must_use]
    pub fn in_dir(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[must_use]
    pub fn splitting(mut self, direction: Direction, ratio: Option<f64>) -> Self {
        self.direction = direction;
        self.ratio = ratio;
        self
    }

    #[must_use]
    pub fn from_target(mut self, target: MuxTarget) -> Self {
        self.target = Some(target);
        self
    }

    /// The exact argv to hand to a multiplexer command surface. Multiplexers do
    /// not inherit the palette process's per-capsule environment, so carry it
    /// explicitly through the portable `env KEY=VALUE ... command` form.
    pub fn command_with_env(&self) -> Vec<String> {
        if self.command.is_empty() || self.env.is_empty() {
            return self.command.clone();
        }
        let mut command = Vec::with_capacity(self.command.len() + self.env.len() + 1);
        command.push("env".to_string());
        command.extend(self.env.iter().map(|(key, value)| format!("{key}={value}")));
        command.extend(self.command.clone());
        command
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnedTarget {
    pub target: MuxTarget,
    pub created: bool,
    /// How the request was satisfied when it was not satisfied literally.
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// The palette
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteRequest {
    pub title: String,
    pub width_percent: u8,
    pub height_percent: u8,
    /// argv of the palette process. Empty means "the caller will run it itself
    /// and only wants to know where it should go".
    pub command: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Ask for a temporary surface rather than an inline modal, on multiplexers
    /// that have no popup but do have cheap surfaces.
    pub prefer_temporary_surface: bool,
}

impl Default for PaletteRequest {
    fn default() -> Self {
        Self {
            title: "AIKit".to_string(),
            width_percent: 82,
            height_percent: 70,
            command: Vec::new(),
            cwd: None,
            prefer_temporary_surface: false,
        }
    }
}

impl PaletteRequest {
    #[must_use]
    pub fn running(mut self, command: Vec<String>) -> Self {
        self.command = command;
        self
    }
}

/// Where the palette ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiHost {
    /// A real overlay. Only tmux claims this.
    TruePopup {
        target: String,
    },
    /// A modal drawn in the terminal the user is already looking at.
    InlineCurrentTerminal,
    /// A surface created for the palette and closed afterwards.
    TemporarySurface {
        id: String,
    },
    Unsupported {
        reason: String,
    },
}

impl UiHost {
    pub fn describe(&self) -> String {
        match self {
            UiHost::TruePopup { .. } => "popup".to_string(),
            UiHost::InlineCurrentTerminal => "inline".to_string(),
            UiHost::TemporarySurface { id } => format!("temporary surface {id}"),
            UiHost::Unsupported { reason } => format!("unsupported — {reason}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Status and notifications
// ---------------------------------------------------------------------------

/// Facts for the multiplexer's status surface. Not a rendered string: tmux wants
/// user options it can interpolate, cmux wants a structured pill.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusUpdate {
    pub session: Option<String>,
    pub values: BTreeMap<String, String>,
}

impl StatusUpdate {
    pub fn for_session(session: impl Into<String>) -> Self {
        Self {
            session: Some(session.into()),
            values: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NotificationLevel {
    #[default]
    Info,
    Warning,
    Error,
}

impl NotificationLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationLevel::Info => "info",
            NotificationLevel::Warning => "warning",
            NotificationLevel::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub level: NotificationLevel,
    pub target: Option<MuxTarget>,
}

impl Notification {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            level: NotificationLevel::default(),
            target: None,
        }
    }

    #[must_use]
    pub fn at_level(mut self, level: NotificationLevel) -> Self {
        self.level = level;
        self
    }
}

// ---------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------

/// One multiplexer's implementation of the shared semantics.
///
/// Object-safe: [`stack::MuxStack`] holds an inner and an outer adapter behind
/// `dyn`, because a hybrid stack is exactly two different multiplexers at once.
pub trait MuxAdapter {
    fn kind(&self) -> MuxKind;

    fn capabilities(&self) -> MuxCapabilities;

    fn detect(&self) -> Result<MuxPresence>;

    fn current_location(&self) -> Result<MuxLocation>;

    /// Make the plan's session exist. Non-destructive unless `mode` says
    /// otherwise.
    fn ensure_session(&self, plan: &SessionPlan, mode: ReconcileMode) -> Result<SessionBinding>;

    fn spawn(&self, request: SpawnRequest) -> Result<SpawnedTarget>;

    fn focus(&self, target: &MuxTarget) -> Result<()>;

    fn close(&self, target: &MuxTarget) -> Result<()>;

    fn open_palette(&self, request: PaletteRequest) -> Result<UiHost>;

    fn set_status(&self, status: StatusUpdate) -> Result<()>;

    fn notify(&self, notification: Notification) -> Result<()>;
}
