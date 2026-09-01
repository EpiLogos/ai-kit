//! cmux: a macOS-native terminal driven through a JSON control socket.
//!
//! cmux is **not** tmux, and this adapter refuses to impersonate one. The
//! mapping is stated once, here, and everything else follows from it:
//!
//! | AIKit | cmux |
//! |---|---|
//! | session with one view | one workspace |
//! | session with several views | a **window** grouping one workspace per view |
//! | pane | a split surface inside a workspace |
//! | status / progress / log / notification | native sidebar surfaces |
//!
//! cmux's grouping container is a window; "workspace group" is AIKit's name for
//! the same idea, and [`Grouping`] decides whether a session gets one.
//!
//! ## Nothing is assumed to exist
//!
//! Every capability is **probed** from `cmux capabilities` and cached for the
//! process. A command name that is not in the probe is treated as absent, and an
//! operation that depends on it degrades with a stated reason rather than being
//! attempted and failing. A response this build cannot parse degrades all the way
//! to "assume nothing" — an optimistic reading of an unknown protocol is how a
//! tool starts issuing commands that do not exist.
//!
//! One consequence is deliberate and worth naming: `true_popup` is **false**
//! unless the probe explicitly declares a `popup` feature. cmux ships a
//! tmux-compatibility `popup` command whose semantics are not documented as an
//! arbitrary-command overlay, so its mere presence in the command list is not
//! evidence. The palette is therefore inline by default.
//!
//! ## Why pane commands carry an `env` prefix
//!
//! cmux has no session environment for AIKit to set, so a pane cannot inherit
//! `AIKIT_SESSION_ID` the way a tmux pane does. Rather than invent a shell
//! profile or a synthetic `HOME`, the adapter prefixes each pane command with
//! `env KEY=VALUE ...`. A pane started with no command at all genuinely does not
//! get the variables, and the binding says so instead of implying otherwise.

use std::collections::{BTreeMap, BTreeSet};
#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
#[cfg(unix)]
use std::time::Duration;

use aikit_core::platform::MuxKind;
use aikit_core::profile::ConfigTable;
use aikit_core::session::{Direction, Placement, SessionPlan, ViewPlan};
use aikit_core::{AikitError, Result, SessionId};

use crate::runner::{CommandRunner, Output, SystemRunner};

use super::{
    MuxAdapter, MuxCapabilities, MuxLocation, MuxPresence, MuxTarget, Notification, PaletteRequest,
    ReconcileMode, SessionBinding, SessionIdentity, SpawnRequest, SpawnedTarget, StatusUpdate,
    UiHost,
};

// ---------------------------------------------------------------------------
// Grouping
// ---------------------------------------------------------------------------

/// `[backend.cmux] grouping = "auto" | "always" | "never"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Grouping {
    /// Group only when there is more than one view. A single-view session in a
    /// group of one is a switcher entry that never has anything to switch to.
    #[default]
    Auto,
    Always,
    Never,
}

impl Grouping {
    pub fn parse(raw: &str) -> Result<Self> {
        Ok(match raw {
            "auto" => Grouping::Auto,
            "always" => Grouping::Always,
            "never" => Grouping::Never,
            other => {
                return Err(AikitError::new(
                    "mux.invalid_grouping",
                    format!("`{other}` is not a grouping mode (auto, always, never)"),
                )
                .with("grouping", other))
            }
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Grouping::Auto => "auto",
            Grouping::Always => "always",
            Grouping::Never => "never",
        }
    }

    pub fn wants_group(self, views: usize) -> bool {
        match self {
            Grouping::Auto => views > 1,
            Grouping::Always => true,
            Grouping::Never => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Sidebar surfaces
// ---------------------------------------------------------------------------

/// The native surfaces cmux offers instead of a status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSurface {
    StatusPill,
    Progress,
    Log,
    Notification,
}

impl SidebarSurface {
    pub fn as_str(self) -> &'static str {
        match self {
            SidebarSurface::StatusPill => "status pill",
            SidebarSurface::Progress => "progress",
            SidebarSurface::Log => "log",
            SidebarSurface::Notification => "notification",
        }
    }

    /// The cmux command this surface needs.
    fn command(self) -> &'static str {
        match self {
            SidebarSurface::StatusPill | SidebarSurface::Progress => "workspace-action",
            SidebarSurface::Log => "markdown",
            SidebarSurface::Notification => "notify",
        }
    }
}

/// What became of a message sent to a sidebar surface.
///
/// `delivered: false` is a normal outcome, not an error: an older cmux simply
/// does not have every surface. What is not acceptable is the caller being unable
/// to tell, so a message that went nowhere always carries the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posted {
    pub surface: SidebarSurface,
    pub delivered: bool,
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// Capability probe
// ---------------------------------------------------------------------------

/// What `cmux capabilities` said, reduced to what AIKit needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CmuxProbe {
    pub version: Option<String>,
    /// Exact live control socket reported by the current provider.
    pub socket_path: Option<String>,
    /// Legacy/user-facing CLI commands reported by older capability responses.
    pub commands: BTreeSet<String>,
    /// Current cmux v2 socket methods. v0.64.22 keeps the legacy CLI command
    /// contract while advertising these method names from `capabilities`.
    pub methods: BTreeSet<String>,
    pub features: BTreeMap<String, bool>,
    /// Whether the socket answered at all.
    pub reachable: bool,
    /// Why the probe learned nothing, when it learned nothing.
    pub note: Option<String>,
}

impl CmuxProbe {
    pub fn has_command(&self, name: &str) -> bool {
        if self.commands.contains(name) {
            return true;
        }

        // cmux v0.64.22's `capabilities` response advertises the v2 socket
        // method registry instead of the still-supported compatibility CLI
        // command list. Keep AIKit on the documented CLI contract, but accept a
        // method as evidence that its corresponding CLI operation is present.
        let method = match name {
            "new-workspace" => "workspace.create",
            "new-window" => "window.create",
            "move-workspace-to-window" => "workspace.move_to_window",
            "new-split" => "surface.split",
            "new-pane" => "pane.create",
            "list-panes" => "pane.list",
            "list-pane-surfaces" => "pane.surfaces",
            "rename-tab" => "tab.action",
            "workspace-action" => "workspace.action",
            "notify" => "notification.create",
            "markdown" => "markdown.open",
            "respawn-pane" => "surface.respawn",
            "close-surface" => "surface.close",
            "close-workspace" => "workspace.close",
            "select-workspace" => "workspace.select",
            _ => return false,
        };
        self.methods.contains(method)
    }

    pub fn feature(&self, name: &str) -> bool {
        self.features.get(name).copied().unwrap_or(false)
    }

    /// The capability view derived from what the probe actually reported.
    pub fn capabilities(&self) -> MuxCapabilities {
        if !self.reachable {
            return MuxCapabilities::default();
        }
        MuxCapabilities {
            // Requires an explicit feature declaration. See the module docs.
            true_popup: self.feature("popup"),
            workspaces: self.has_command("new-workspace") || self.feature("workspaces"),
            workspace_groups: self.has_command("new-window")
                && self.has_command("move-workspace-to-window"),
            windows: self.has_command("new-window"),
            panes: self.has_command("new-split") || self.has_command("new-pane"),
            browser_surface: self.has_command("browser") && self.feature("browser_surface"),
            status_metadata: self.has_command("workspace-action"),
            notifications: self.has_command("notify"),
            remote_control: true,
        }
    }

    fn parse(stdout: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(stdout).ok()?;
        let object = value.as_object()?;

        let commands = object
            .get("commands")
            .and_then(|c| c.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let methods = object
            .get("methods")
            .and_then(|m| m.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let features = object
            .get("features")
            .and_then(|f| f.as_object())
            .map(|table| {
                table
                    .iter()
                    .filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b)))
                    .collect()
            })
            .unwrap_or_default();

        Some(Self {
            version: object
                .get("version")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            socket_path: object
                .get("socket_path")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            commands,
            methods,
            features,
            reachable: true,
            note: None,
        })
    }
}

// ---------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------

pub struct Cmux<R> {
    runner: R,
    binary: String,
    grouping: Grouping,
    identity: SessionIdentity,
    env: BTreeMap<String, String>,
    /// Probed once per process: a palette that reprobes on every keystroke is a
    /// palette that misses its latency budget.
    probe: Mutex<Option<CmuxProbe>>,
}

impl Cmux<SystemRunner> {
    pub fn system() -> Self {
        let mut adapter = Self::new(SystemRunner::new());
        for key in ["CMUX_WORKSPACE_ID", "CMUX_SURFACE_ID", "CMUX_TAB_ID"] {
            if let Ok(value) = std::env::var(key) {
                adapter.env.insert(key.to_string(), value);
            }
        }
        adapter
    }
}

impl<R: CommandRunner> Cmux<R> {
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            binary: "cmux".to_string(),
            grouping: Grouping::default(),
            identity: SessionIdentity::default(),
            env: BTreeMap::new(),
            probe: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn with_binary(mut self, binary: impl Into<String>) -> Self {
        self.binary = binary.into();
        self
    }

    #[must_use]
    pub fn with_grouping(mut self, grouping: Grouping) -> Self {
        self.grouping = grouping;
        self
    }

    #[must_use]
    pub fn with_identity(mut self, identity: SessionIdentity) -> Self {
        self.identity = identity;
        self
    }

    #[must_use]
    pub fn with_env_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn runner(&self) -> &R {
        &self.runner
    }

    pub fn argv(&self, args: &[&str]) -> Vec<String> {
        let mut out = vec![self.binary.clone()];
        out.extend(args.iter().map(|a| a.to_string()));
        out
    }

    /// cmux defaults several query and mutation commands to human-readable
    /// output. Anything AIKit parses must request the global JSON mode
    /// explicitly; accepting whichever format happened to be printed is not a
    /// protocol.
    fn argv_json(&self, args: &[&str]) -> Vec<String> {
        let mut out = vec![self.binary.clone(), "--json".to_string()];
        out.extend(args.iter().map(|a| a.to_string()));
        out
    }

    fn run(&self, args: &[&str]) -> Result<Output> {
        self.runner.run(&self.argv(args))
    }

    fn must(&self, args: &[&str]) -> Result<Output> {
        let argv = self.argv(args);
        self.runner.run(&argv)?.require(&argv, "mux.cmux_failed")
    }

    fn run_json(&self, args: &[&str]) -> Result<Output> {
        self.runner.run(&self.argv_json(args))
    }

    fn must_json(&self, args: &[&str]) -> Result<Output> {
        let argv = self.argv_json(args);
        self.runner.run(&argv)?.require(&argv, "mux.cmux_failed")
    }

    fn must_owned(&self, args: &[String]) -> Result<Output> {
        self.must(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }

    #[cfg(unix)]
    fn call_v2(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let probe = self.probe()?;
        let socket = std::env::var("CMUX_SOCKET_PATH")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or(probe.socket_path)
            .ok_or_else(|| {
                AikitError::new(
                    "mux.cmux_socket_missing",
                    format!("cmux advertises `{method}` but did not disclose a live socket path"),
                )
            })?;

        let mut stream = UnixStream::connect(&socket).map_err(|error| {
            AikitError::new(
                "mux.cmux_socket_connect_failed",
                format!("could not connect to cmux socket `{socket}`: {error}"),
            )
        })?;
        let timeout = Some(Duration::from_secs(10));
        stream.set_read_timeout(timeout).map_err(|error| {
            AikitError::new(
                "mux.cmux_socket_timeout_failed",
                format!("could not set cmux socket read timeout: {error}"),
            )
        })?;
        stream.set_write_timeout(timeout).map_err(|error| {
            AikitError::new(
                "mux.cmux_socket_timeout_failed",
                format!("could not set cmux socket write timeout: {error}"),
            )
        })?;

        let request = serde_json::json!({
            "id": 1,
            "method": method,
            "params": params,
        });
        writeln!(stream, "{request}").map_err(|error| {
            AikitError::new(
                "mux.cmux_socket_write_failed",
                format!("could not write cmux `{method}` request: {error}"),
            )
        })?;
        stream.flush().map_err(|error| {
            AikitError::new(
                "mux.cmux_socket_write_failed",
                format!("could not flush cmux `{method}` request: {error}"),
            )
        })?;

        let mut line = String::new();
        BufReader::new(stream)
            .read_line(&mut line)
            .map_err(|error| {
                AikitError::new(
                    "mux.cmux_socket_read_failed",
                    format!("could not read cmux `{method}` response: {error}"),
                )
            })?;
        let response: serde_json::Value = serde_json::from_str(line.trim()).map_err(|error| {
            AikitError::new(
                "mux.cmux_socket_decode_failed",
                format!("could not decode cmux `{method}` response: {error}"),
            )
        })?;
        if response.get("id").and_then(serde_json::Value::as_u64) != Some(1) {
            return Err(AikitError::new(
                "mux.cmux_socket_response_mismatch",
                format!("cmux `{method}` response did not match request id 1: {response}"),
            ));
        }
        if response.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(AikitError::new(
                "mux.cmux_socket_operation_failed",
                format!("cmux `{method}` failed: {response}"),
            ));
        }
        Ok(response
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    #[cfg(not(unix))]
    fn call_v2(&self, method: &str, _params: serde_json::Value) -> Result<serde_json::Value> {
        Err(AikitError::new(
            "mux.cmux_socket_unsupported",
            format!("cmux v2 socket operation `{method}` requires a Unix host"),
        ))
    }

    // -----------------------------------------------------------------------
    // Probing
    // -----------------------------------------------------------------------

    /// Ask cmux what it can do. Cached after the first call.
    pub fn probe(&self) -> Result<CmuxProbe> {
        let mut cached = self.probe.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(probe) = cached.as_ref() {
            return Ok(probe.clone());
        }

        let probe = match self.run(&["capabilities"]) {
            Err(e) if e.code() == "mux.command_spawn_failed" => CmuxProbe {
                reachable: false,
                note: Some("`cmux` is not on PATH".to_string()),
                ..CmuxProbe::default()
            },
            Err(e) => return Err(e),
            Ok(out) if !out.ok() => CmuxProbe {
                reachable: false,
                note: Some(first_line(&out.stderr, &out.stdout)),
                ..CmuxProbe::default()
            },
            Ok(out) => CmuxProbe::parse(out.line()).unwrap_or(CmuxProbe {
                reachable: false,
                note: Some(
                    "the `capabilities` response could not be parsed by this build, so no \
                     capability is assumed"
                        .to_string(),
                ),
                ..CmuxProbe::default()
            }),
        };

        *cached = Some(probe.clone());
        Ok(probe)
    }

    fn probe_or_empty(&self) -> CmuxProbe {
        // Capabilities are asked for on rendering paths that have nowhere to put
        // an error; a failed probe is indistinguishable from an absent feature at
        // that point, and both mean "claim nothing".
        self.probe().unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Naming
    // -----------------------------------------------------------------------

    /// The workspace title AIKit binds to.
    ///
    /// cmux ids are handles, not identity — a restored app hands the same
    /// workspace a new id — so the title is what `ensure_session` rebinds by.
    fn ownership_scope(&self) -> Result<String> {
        self.identity
            .session_id
            .as_ref()
            .map(ToString::to_string)
            .ok_or_else(|| {
                AikitError::new(
                    "mux.cmux_identity_missing",
                    "cmux ownership requires AIKit's durable session id",
                )
            })
    }

    pub fn workspace_title(&self, plan: &SessionPlan, view: &ViewPlan) -> Result<String> {
        // The view component is always present. `name` is presentation text and
        // may repeat, whereas the compiler requires unique view ids.
        let base = format!("{} · {}", plan.id, view.id);
        Ok(format!("{base} · {}", self.ownership_scope()?))
    }

    /// Prefix a pane command with the AIKit context, since cmux has no session
    /// environment to put it in.
    fn contextual_command(&self, command: &[String]) -> Option<String> {
        if command.is_empty() {
            return None;
        }
        let environment = self.identity.environment();
        if environment.is_empty() {
            return Some(shell_words::join(command));
        }
        let mut argv: Vec<String> = vec!["env".to_string()];
        argv.extend(environment.into_iter().map(|(k, v)| format!("{k}={v}")));
        argv.extend(command.iter().cloned());
        Some(shell_words::join(&argv))
    }

    fn grouping_for(&self, plan: &SessionPlan) -> Result<Grouping> {
        match plan
            .backend_extensions
            .get(MuxKind::Cmux.as_str())
            .and_then(|table: &ConfigTable| table.get("grouping"))
        {
            // A per-session declaration beats the adapter default: the person who
            // wrote the spec knows what that session is for.
            Some(value) => match value.as_str() {
                Some(raw) => Grouping::parse(raw),
                None => Err(AikitError::new(
                    "mux.invalid_grouping",
                    "[backend.cmux] grouping must be a string (auto, always, never)",
                )),
            },
            None => Ok(self.grouping),
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Every workspace cmux currently has, as `(title, workspace)`.
    pub fn workspaces(&self) -> Result<Vec<Workspace>> {
        let out = self.run_json(&["list-workspaces"])?;
        if !out.ok() {
            return Err(cmux_unreachable(&out));
        }
        let value: serde_json::Value = serde_json::from_str(out.line()).map_err(|e| {
            AikitError::new(
                "mux.cmux_protocol",
                format!("could not read the workspace listing: {e}"),
            )
        })?;
        let items = value
            .get("workspaces")
            .and_then(|w| w.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(items
            .iter()
            .filter_map(|item| {
                Some(Workspace {
                    id: object_handle(item)?,
                    title: item
                        .get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    window: related_handle(item, "window"),
                })
            })
            .collect())
    }

    /// All surfaces in one workspace, discovered from cmux itself. A workspace
    /// handle does not imply a root-surface handle, and fabricating one makes the
    /// very first split fail on a real installation.
    fn surfaces(&self, workspace: &str) -> Result<Vec<Surface>> {
        let panes = self.must_json(&["list-panes", "--workspace", workspace])?;
        let value: serde_json::Value = serde_json::from_str(panes.line()).map_err(|e| {
            AikitError::new(
                "mux.cmux_protocol",
                format!("could not read the pane listing for `{workspace}`: {e}"),
            )
        })?;
        let pane_handles: Vec<String> = value
            .get("panes")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .filter_map(object_handle)
            .collect();

        let mut found = BTreeMap::<String, Surface>::new();
        for pane in pane_handles {
            let out = self.must_json(&[
                "list-pane-surfaces",
                "--workspace",
                workspace,
                "--pane",
                &pane,
            ])?;
            let value: serde_json::Value = serde_json::from_str(out.line()).map_err(|e| {
                AikitError::new(
                    "mux.cmux_protocol",
                    format!("could not read surfaces in pane `{pane}`: {e}"),
                )
            })?;
            for item in value
                .get("surfaces")
                .and_then(|s| s.as_array())
                .into_iter()
                .flatten()
            {
                if let Some(id) = object_handle(item) {
                    found.insert(
                        id.clone(),
                        Surface {
                            id,
                            title: item
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                        },
                    );
                }
            }
        }
        Ok(found.into_values().collect())
    }

    /// Resolve the current cmux handles for an AIKit session from its durable
    /// surface markers. Stored cmux refs are bindings and may change after an app
    /// restart, so attach/down must call this instead of trusting the database's
    /// last-seen handle.
    pub fn session_targets(&self, session_id: &SessionId) -> Result<CmuxSessionTargets> {
        let marker = format!("AIKit · {session_id}/");
        let mut workspaces = Vec::new();
        let mut windows = BTreeSet::new();
        let mut window_workspace_counts = BTreeMap::<String, usize>::new();
        let mut owned_window_counts = BTreeMap::<String, usize>::new();
        let mut workspace_without_window = false;
        for workspace in self.workspaces()? {
            if let Some(window) = &workspace.window {
                *window_workspace_counts.entry(window.clone()).or_default() += 1;
            }
            let owned = self
                .surfaces(&workspace.id)?
                .iter()
                .any(|surface| surface.title.starts_with(&marker));
            if owned {
                if let Some(window) = &workspace.window {
                    windows.insert(window.clone());
                    *owned_window_counts.entry(window.clone()).or_default() += 1;
                } else {
                    workspace_without_window = true;
                }
                workspaces.push(workspace.id);
            }
        }
        workspaces.sort();
        let common_window = (!workspace_without_window && windows.len() == 1)
            .then(|| windows.into_iter().next())
            .flatten();
        let exclusive_window = common_window
            .as_ref()
            .filter(|window| {
                window_workspace_counts.get(*window) == owned_window_counts.get(*window)
            })
            .cloned();
        Ok(CmuxSessionTargets {
            workspaces,
            common_window,
            exclusive_window,
        })
    }

    // -----------------------------------------------------------------------
    // Sidebar
    // -----------------------------------------------------------------------

    /// Send something to one of cmux's native surfaces.
    pub fn post(&self, surface: SidebarSurface, workspace: &str, message: &str) -> Result<Posted> {
        let probe = self.probe_or_empty();
        if !probe.has_command(surface.command()) {
            return Ok(Posted {
                surface,
                delivered: false,
                note: Some(format!(
                    "this cmux has no `{}` command, so the {} surface is not available",
                    surface.command(),
                    surface.as_str()
                )),
            });
        }

        match surface {
            SidebarSurface::StatusPill => {
                self.must(&[
                    "workspace-action",
                    "--action",
                    "set-title",
                    "--workspace",
                    workspace,
                    "--title",
                    message,
                ])?;
            }
            SidebarSurface::Progress => {
                self.must(&[
                    "workspace-action",
                    "--action",
                    "set-progress",
                    "--workspace",
                    workspace,
                    "--title",
                    message,
                ])?;
            }
            SidebarSurface::Log => {
                // cmux's markdown viewer panel is the log surface: it reloads
                // live, which is what a running apply wants.
                self.must(&["markdown", "open", message])?;
            }
            SidebarSurface::Notification => {
                self.must(&["notify", "--title", "AIKit", "--body", message])?;
            }
        }

        Ok(Posted {
            surface,
            delivered: true,
            note: None,
        })
    }

    // -----------------------------------------------------------------------
    // Building
    // -----------------------------------------------------------------------

    fn create_workspace(&self, title: &str, plan: &SessionPlan, view: &ViewPlan) -> Result<String> {
        let mut args: Vec<String> =
            vec!["new-workspace".into(), "--name".into(), title.to_string()];
        let root = view.steps[0].cwd.as_deref().or(plan.root.as_deref());
        if let Some(cwd) = root {
            args.push("--cwd".into());
            args.push(cwd.display().to_string());
        }
        if let Some(command) = self.contextual_command(&view.steps[0].command) {
            args.push("--command".into());
            args.push(command);
        }
        let out = self.must_owned(&args)?;
        extract_id(out.line(), "workspace")
    }

    fn split_surface(&self, workspace: &str, origin: &str, direction: Direction) -> Result<String> {
        let out = self.must_json(&[
            "new-split",
            direction.as_str(),
            "--workspace",
            workspace,
            "--surface",
            origin,
        ])?;
        extract_id(out.line(), "surface")
    }

    fn ownership_title(&self, plan: &SessionPlan, view: &ViewPlan, pane: &str) -> Result<String> {
        Ok(format!(
            "AIKit · {}/{}/{}/{}",
            self.ownership_scope()?,
            plan.id,
            view.id,
            pane
        ))
    }

    fn ownership_prefix(&self, plan: &SessionPlan, view: &ViewPlan) -> Result<String> {
        Ok(format!(
            "AIKit · {}/{}/{}/",
            self.ownership_scope()?,
            plan.id,
            view.id
        ))
    }

    fn rename_owned_surface(&self, workspace: &str, surface: &str, title: &str) -> Result<()> {
        self.must_json(&[
            "rename-tab",
            "--workspace",
            workspace,
            "--surface",
            surface,
            "--title",
            title,
        ])?;
        Ok(())
    }

    fn run_in_surface(&self, workspace: &str, surface: &str, command: &str) -> Result<()> {
        self.must(&[
            "respawn-pane",
            "--workspace",
            workspace,
            "--surface",
            surface,
            "--command",
            command,
        ])?;
        Ok(())
    }
}

/// One cmux workspace, as far as AIKit needs to know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub title: String,
    pub window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmuxSessionTargets {
    pub workspaces: Vec<String>,
    pub common_window: Option<String>,
    /// Present only when every workspace in the window is owned by this
    /// SessionId. Teardown may close this window without taking unrelated work
    /// with it; otherwise callers close the owned workspaces individually.
    pub exclusive_window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Surface {
    id: String,
    title: String,
}

/// Pull an id out of a cmux response, tolerating both `{"x":{"id":..}}` and
/// `{"id":..}`.
fn extract_id(stdout: &str, key: &str) -> Result<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) {
        let candidate = value
            .get(key)
            .and_then(object_handle)
            .or_else(|| related_handle(&value, key))
            .or_else(|| object_handle(&value));
        if let Some(candidate) = candidate {
            return Ok(candidate);
        }
    }

    let prefix = format!("{key}:");
    if let Some(candidate) = stdout
        .split_whitespace()
        .map(|part| part.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != ':' && c != '-'))
        .find(|part| part.starts_with(&prefix))
    {
        return Ok(candidate.to_string());
    }

    Err(AikitError::new(
        "mux.cmux_protocol",
        format!("the `{key}` response carried no id"),
    )
    .with("response", stdout.chars().take(200).collect::<String>()))
}

fn object_handle(value: &serde_json::Value) -> Option<String> {
    value
        .get("ref")
        .or_else(|| value.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn related_handle(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(format!("{key}_ref"))
        .or_else(|| value.get(format!("{key}_id")))
        .or_else(|| value.get(key).filter(|v| v.is_string()))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn cmux_unreachable(out: &Output) -> AikitError {
    let detail = first_line(&out.stderr, &out.stdout);
    cmux_probe_error(&detail)
}

fn is_access_denied(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    detail.contains("failed to write to socket")
        || detail.contains("not authorized")
        || detail.contains("permission")
        || detail.contains("not permitted")
}

fn cmux_probe_error(detail: &str) -> AikitError {
    let access_denied = is_access_denied(detail);
    if access_denied {
        AikitError::new(
            "mux.cmux_access_denied",
            "cmux is running but rejected this process; run AIKit inside cmux or allow the intended automation access in cmux Settings",
        )
        .with("detail", detail.to_string())
    } else {
        AikitError::new(
            "mux.cmux_unreachable",
            "cmux did not answer its control socket",
        )
        .with("detail", detail.to_string())
    }
}

fn first_line(stderr: &str, stdout: &str) -> String {
    let source = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    source.lines().next().unwrap_or("").trim().to_string()
}

// ---------------------------------------------------------------------------
// MuxAdapter
// ---------------------------------------------------------------------------

impl<R: CommandRunner> MuxAdapter for Cmux<R> {
    fn kind(&self) -> MuxKind {
        MuxKind::Cmux
    }

    fn capabilities(&self) -> MuxCapabilities {
        self.probe_or_empty().capabilities()
    }

    fn detect(&self) -> Result<MuxPresence> {
        let version = match self.run(&["version"]) {
            Ok(out) if out.ok() => Some(out.line().trim().to_string()),
            Ok(_) => None,
            Err(e) if e.code() == "mux.command_spawn_failed" => {
                return Ok(MuxPresence::absent(MuxKind::Cmux, "`cmux` is not on PATH"))
            }
            Err(e) => return Err(e),
        };

        let probe = self.probe()?;
        let access_denied = probe.note.as_deref().is_some_and(is_access_denied);
        let detail = if access_denied {
            Some(
                "cmux is running but this process is outside its allowed automation scope; run AIKit inside cmux or adjust cmux Settings intentionally"
                    .to_string(),
            )
        } else {
            probe.note.clone()
        };
        Ok(MuxPresence {
            kind: MuxKind::Cmux,
            installed: version.is_some(),
            version,
            // A successful connect followed by a rejected write is evidence of a
            // live, access-controlled server—not evidence that the app is down.
            server_running: probe.reachable || access_denied,
            inside: self.env.contains_key("CMUX_WORKSPACE_ID"),
            detail,
        })
    }

    fn current_location(&self) -> Result<MuxLocation> {
        if !self.env.contains_key("CMUX_WORKSPACE_ID") {
            return Ok(MuxLocation::nowhere(MuxKind::Cmux));
        }
        let out = self.must_json(&["identify"])?;
        let value: serde_json::Value = serde_json::from_str(out.line()).map_err(|e| {
            AikitError::new(
                "mux.cmux_protocol",
                format!("could not read the `identify` response: {e}"),
            )
        })?;
        // 0.63 returns `caller`/`focused` objects with `*_ref` keys. Retain the
        // earlier nested-object shape as a backwards-compatible read path.
        let location = value
            .get("caller")
            .filter(|v| v.is_object())
            .or_else(|| value.get("focused").filter(|v| v.is_object()));
        let handle = |key: &str| {
            location
                .and_then(|v| related_handle(v, key))
                .or_else(|| value.get(key).and_then(object_handle))
        };
        Ok(MuxLocation {
            kind: MuxKind::Cmux,
            host: value
                .get("host")
                .and_then(|h| h.as_str())
                .unwrap_or_default()
                .to_string(),
            remote: false,
            project: self
                .identity
                .project_root
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string()),
            session: handle("workspace"),
            view: handle("window"),
            surface: handle("surface"),
        })
    }

    fn session_exists(&self, plan: &SessionPlan) -> Result<bool> {
        let existing = self.workspaces()?;
        for view in &plan.views {
            let title = self.workspace_title(plan, view)?;
            let Some(workspace) = existing.iter().find(|workspace| workspace.title == title) else {
                continue;
            };
            let prefix = self.ownership_prefix(plan, view)?;
            if self
                .surfaces(&workspace.id)?
                .iter()
                .any(|surface| surface.title.starts_with(&prefix))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn inspect_session(&self, plan: &SessionPlan) -> Result<SessionBinding> {
        let mut binding = SessionBinding {
            kind: Some(MuxKind::Cmux),
            warnings: plan.warnings.clone(),
            ..SessionBinding::default()
        };
        let existing = self.workspaces()?;
        let requested = self.grouping_for(plan)?;
        let grouped =
            requested.wants_group(plan.views.len()) && self.capabilities().workspace_groups;

        for view in &plan.views {
            let title = self.workspace_title(plan, view)?;
            let Some(workspace) = existing.iter().find(|workspace| workspace.title == title) else {
                binding.record(format!("workspace `{title}` is missing"));
                continue;
            };
            let discovered = self.surfaces(&workspace.id)?;
            let prefix = self.ownership_prefix(plan, view)?;
            let owned: BTreeMap<String, String> = discovered
                .iter()
                .filter_map(|surface| {
                    surface
                        .title
                        .strip_prefix(&prefix)
                        .filter(|pane| !pane.is_empty())
                        .map(|pane| (pane.to_string(), surface.id.clone()))
                })
                .collect();
            if owned.is_empty() {
                return Err(AikitError::new(
                    "mux.cmux_ownership_ambiguous",
                    format!(
                        "workspace `{title}` exists but carries no marker for this AIKit session"
                    ),
                )
                .with("workspace", &workspace.id));
            }

            binding.views.insert(view.id.clone(), workspace.id.clone());
            if binding.session.is_empty() {
                binding.session = if grouped {
                    workspace
                        .window
                        .clone()
                        .unwrap_or_else(|| workspace.id.clone())
                } else {
                    workspace.id.clone()
                };
            }
            let declared: BTreeSet<&str> =
                view.steps.iter().map(|step| step.pane.as_str()).collect();
            for step in &view.steps {
                if let Some(surface) = owned.get(&step.pane) {
                    binding
                        .surfaces
                        .insert(format!("{}/{}", view.id, step.pane), surface.clone());
                } else {
                    binding.record(format!("pane `{}/{}` is missing", view.id, step.pane));
                }
            }
            for pane in owned
                .keys()
                .filter(|pane| !declared.contains(pane.as_str()))
            {
                binding.record(format!(
                    "AIKit pane `{}/{pane}` is no longer declared",
                    view.id
                ));
            }
            for surface in discovered
                .iter()
                .filter(|surface| !surface.title.starts_with(&prefix))
            {
                binding.preserve(format!(
                    "unowned surface `{}` is outside the plan",
                    if surface.title.is_empty() {
                        &surface.id
                    } else {
                        &surface.title
                    }
                ));
            }
        }
        Ok(binding)
    }

    fn ensure_session(&self, plan: &SessionPlan, mode: ReconcileMode) -> Result<SessionBinding> {
        if plan.views.is_empty() {
            return Err(AikitError::new(
                "mux.empty_plan",
                format!("session plan `{}` declares no views", plan.id),
            ));
        }

        let probe = self.probe()?;
        if !probe.reachable {
            return Err(cmux_probe_error(
                probe.note.as_deref().unwrap_or("cmux did not answer"),
            ));
        }
        let capabilities = probe.capabilities();
        let mut binding = SessionBinding {
            kind: Some(MuxKind::Cmux),
            ..SessionBinding::default()
        };
        binding.warnings.extend(plan.warnings.clone());
        if !capabilities.panes
            || !probe.has_command("list-panes")
            || !probe.has_command("list-pane-surfaces")
            || !probe.has_command("rename-tab")
        {
            return Err(AikitError::new(
                "mux.cmux_topology_unsupported",
                "this cmux cannot expose and label split surfaces, so AIKit cannot reconcile it safely",
            )
            .with(
                "required",
                "new-split,list-panes,list-pane-surfaces,rename-tab",
            ));
        }

        let requested = self.grouping_for(plan)?;
        let mut grouped = requested.wants_group(plan.views.len());
        if grouped && !capabilities.workspace_groups {
            binding.warnings.push(format!(
                "grouping = \"{}\" was asked for, but this cmux cannot group workspaces into a \
                 window; the session's views were created as separate workspaces",
                requested.as_str()
            ));
            grouped = false;
        } else if !grouped && plan.views.len() > 1 {
            binding.warnings.push(format!(
                "grouping = \"{}\": this session's {} views are separate workspaces and are not \
                 switchable as one unit",
                requested.as_str(),
                plan.views.len()
            ));
        }

        let existing = self.workspaces()?;
        let by_title: BTreeMap<&str, &Workspace> =
            existing.iter().map(|w| (w.title.as_str(), w)).collect();

        // The titles this plan owns. Anything else in cmux belongs to somebody
        // else and is never touched, not even under `Exact`.
        let wanted: Vec<(String, &ViewPlan)> = plan
            .views
            .iter()
            .map(|view| Ok((self.workspace_title(plan, view)?, view)))
            .collect::<Result<_>>()?;

        // Validate every title collision before creating a group, workspace, or
        // pane. Otherwise a later foreign workspace could make us refuse the
        // plan only after earlier missing views had already been created.
        let mut known_surfaces = BTreeMap::<String, Vec<Surface>>::new();
        for (title, view) in &wanted {
            let Some(existing) = by_title.get(title.as_str()) else {
                continue;
            };
            let discovered = self.surfaces(&existing.id)?;
            let owned_prefix = self.ownership_prefix(plan, view)?;
            if !discovered
                .iter()
                .any(|surface| surface.title.starts_with(&owned_prefix))
            {
                return Err(AikitError::new(
                    "mux.cmux_ownership_ambiguous",
                    format!(
                        "workspace `{title}` has {} unowned surface{}; its title alone does not prove AIKit owns it",
                        discovered.len(),
                        if discovered.len() == 1 { "" } else { "s" }
                    ),
                )
                .with("workspace", &existing.id)
                .with(
                    "resolution",
                    "rename the workspace or create a fresh AIKit session",
                ));
            }
            known_surfaces.insert(existing.id.clone(), discovered);
        }

        let already_bound: Vec<&Workspace> = wanted
            .iter()
            .filter_map(|(title, _)| by_title.get(title.as_str()).copied())
            .collect();
        binding.created = already_bound.is_empty();

        // A group is only created when there is not one already: rebinding after a
        // cmux restart must not build a second copy of the session.
        let mut window = already_bound.iter().find_map(|w| w.window.clone());
        if grouped && window.is_none() {
            let out = self.must(&["new-window"])?;
            window = Some(extract_id(out.line(), "window")?);
            binding.record("created the workspace group".to_string());
        }

        for (title, view) in &wanted {
            let (workspace, workspace_created) = match by_title.get(title.as_str()) {
                Some(existing) => {
                    binding.preserve(format!(
                        "workspace `{title}` was already open as {}; rebound rather than \
                         recreated",
                        existing.id
                    ));
                    (existing.id.clone(), false)
                }
                None => {
                    let id = self.create_workspace(title, plan, view)?;
                    binding.record(format!("created workspace `{title}` for view {}", view.id));
                    if let Some(window) = &window {
                        self.must(&[
                            "move-workspace-to-window",
                            "--workspace",
                            &id,
                            "--window",
                            window,
                        ])?;
                    }

                    (id, true)
                }
            };

            let discovered = if workspace_created {
                self.surfaces(&workspace)?
            } else {
                known_surfaces.remove(&workspace).ok_or_else(|| {
                    AikitError::new(
                        "mux.cmux_protocol",
                        format!("preflight topology for workspace `{workspace}` was lost"),
                    )
                })?
            };
            if discovered.is_empty() {
                return Err(AikitError::new(
                    "mux.cmux_protocol",
                    format!(
                        "cmux reported no surface for workspace `{workspace}`; refusing to invent one"
                    ),
                ));
            }

            let owned_prefix = self.ownership_prefix(plan, view)?;
            let mut surfaces: BTreeMap<String, String> = discovered
                .iter()
                .filter_map(|surface| {
                    surface
                        .title
                        .strip_prefix(&owned_prefix)
                        .filter(|pane| !pane.is_empty())
                        .map(|pane| (pane.to_string(), surface.id.clone()))
                })
                .collect();

            // A newly created workspace has exactly one initial surface. An
            // existing same-named workspace is not evidence of ownership: it may
            // be the user's. Without an AIKit marker, even a single surface is
            // ambiguous and must remain untouched.
            if surfaces.is_empty() {
                if !workspace_created {
                    return Err(AikitError::new(
                        "mux.cmux_ownership_ambiguous",
                        format!(
                            "workspace `{title}` has {} unowned surface{}; its title alone does not prove AIKit owns it",
                            discovered.len()
                            ,
                            if discovered.len() == 1 { "" } else { "s" }
                        ),
                    )
                    .with("workspace", &workspace)
                    .with("resolution", "rename the workspace or create a fresh AIKit session"));
                }
                let root = &discovered[0];
                let root_pane = &view.steps[0].pane;
                self.rename_owned_surface(
                    &workspace,
                    &root.id,
                    &self.ownership_title(plan, view, root_pane)?,
                )?;
                surfaces.insert(root_pane.clone(), root.id.clone());
            }

            for step in &view.steps {
                if let Some(surface) = surfaces.get(&step.pane) {
                    binding
                        .surfaces
                        .insert(format!("{}/{}", view.id, step.pane), surface.clone());
                    continue;
                }
                let Some(split) = &step.split else {
                    return Err(AikitError::new(
                        "mux.cmux_missing_root",
                        format!(
                            "workspace `{title}` has no owned surface for root pane `{}`",
                            step.pane
                        ),
                    ));
                };
                let Some(origin) = surfaces.get(&split.from).cloned() else {
                    return Err(AikitError::new(
                        "mux.cmux_invalid_topology",
                        format!(
                            "pane `{}` splits from missing pane `{}`",
                            step.pane, split.from
                        ),
                    ));
                };
                let surface = self.split_surface(&workspace, &origin, split.direction)?;
                self.rename_owned_surface(
                    &workspace,
                    &surface,
                    &self.ownership_title(plan, view, &step.pane)?,
                )?;
                if let Some(command) = self.contextual_command(&step.command) {
                    self.run_in_surface(&workspace, &surface, &command)?;
                }
                binding.record(format!(
                    "created pane `{}` in workspace `{title}`",
                    step.pane
                ));
                surfaces.insert(step.pane.clone(), surface.clone());
                binding
                    .surfaces
                    .insert(format!("{}/{}", view.id, step.pane), surface);
            }

            let declared: BTreeSet<&str> =
                view.steps.iter().map(|step| step.pane.as_str()).collect();
            for surface in &discovered {
                let Some(pane) = surface.title.strip_prefix(&owned_prefix) else {
                    if !surface.title.is_empty() {
                        binding.preserve(format!(
                            "left unowned cmux surface `{}` unchanged",
                            surface.title
                        ));
                    }
                    continue;
                };
                if declared.contains(pane) {
                    continue;
                }
                if mode.may_close_panes() {
                    self.must_json(&[
                        "close-surface",
                        "--workspace",
                        &workspace,
                        "--surface",
                        &surface.id,
                    ])?;
                    binding.record(format!(
                        "closed AIKit-owned pane `{pane}`, which the plan no longer declares"
                    ));
                } else {
                    binding.preserve(format!(
                        "AIKit-owned pane `{pane}` is no longer declared but exact reconciliation was not requested"
                    ));
                }
            }
            binding.views.insert(view.id.clone(), workspace);
        }

        // cmux has no session environment, so a pane with no command of its own
        // starts a plain shell that never sees the AIKit variables. Saying so is
        // the whole difference between a limitation and a bug report.
        if plan
            .views
            .iter()
            .flat_map(|v| v.steps.iter())
            .any(|s| s.command.is_empty())
        {
            binding.warnings.push(
                "cmux has no session environment, so panes started with the default shell do not \
                 carry AIKIT_SESSION_ID; panes with a declared command do"
                    .to_string(),
            );
        }

        if mode.may_close_panes() {
            let keep: BTreeSet<&str> = wanted.iter().map(|(t, _)| t.as_str()).collect();
            let scope = self.ownership_scope()?;
            let title_suffix = format!(" · {scope}");
            let ownership_prefix = format!("AIKit · {scope}/{}/", plan.id);
            for workspace in &existing {
                if workspace.title.ends_with(&title_suffix)
                    && !keep.contains(workspace.title.as_str())
                {
                    let proven = self
                        .surfaces(&workspace.id)?
                        .iter()
                        .any(|surface| surface.title.starts_with(&ownership_prefix));
                    if !proven {
                        binding.preserve(format!(
                            "left same-named workspace `{}` unchanged because it has no AIKit ownership marker",
                            workspace.title
                        ));
                        continue;
                    }
                    self.must(&["close-workspace", "--workspace", &workspace.id])?;
                    binding.record(format!(
                        "closed workspace `{}`, which the plan no longer declares",
                        workspace.title
                    ));
                }
            }
        }

        binding.session = window
            .clone()
            .or_else(|| binding.views.values().next().cloned())
            .unwrap_or_else(|| plan.id.clone());

        if let Some(first) = plan.views.first().and_then(|v| binding.views.get(&v.id)) {
            self.must(&["select-workspace", "--workspace", first])?;
        }

        Ok(binding)
    }

    fn spawn(&self, request: SpawnRequest) -> Result<SpawnedTarget> {
        let capabilities = self.capabilities();
        let command = request.command_with_env();
        let target = request
            .target
            .clone()
            .unwrap_or_else(|| MuxTarget::surface(MuxKind::Cmux, ""));

        match request.placement {
            Placement::Current => {
                let surface = target.selector();
                if surface.is_empty() {
                    return Err(AikitError::new(
                        "mux.no_target",
                        "running in the current surface needs a surface, and none was given",
                    ));
                }
                if let Some(command) = self.contextual_command(&command) {
                    let workspace = target.session.clone().unwrap_or_default();
                    self.run_in_surface(&workspace, &surface, &command)?;
                }
                Ok(SpawnedTarget {
                    target: MuxTarget::surface(MuxKind::Cmux, surface),
                    created: false,
                    note: None,
                })
            }
            Placement::NewPane => {
                if !capabilities.panes {
                    return Err(AikitError::new(
                        "mux.unsupported",
                        "this cmux has no split command, so a new pane cannot be opened",
                    ));
                }
                let workspace = target.session.clone().unwrap_or_default();
                let origin = target.surface.clone().unwrap_or_default();
                let surface = self.split_surface(&workspace, &origin, request.direction)?;
                if let Some(command) = self.contextual_command(&command) {
                    self.run_in_surface(&workspace, &surface, &command)?;
                }
                Ok(SpawnedTarget {
                    target: MuxTarget::surface(MuxKind::Cmux, surface),
                    created: true,
                    note: request.ratio.map(|_| {
                        "cmux splits evenly; the requested ratio was not applied".to_string()
                    }),
                })
            }
            Placement::NewView | Placement::Background => {
                let mut args: Vec<String> = vec!["new-workspace".into()];
                if let Some(name) = &request.name {
                    args.push("--name".into());
                    args.push(name.clone());
                }
                if let Some(cwd) = &request.cwd {
                    args.push("--cwd".into());
                    args.push(cwd.display().to_string());
                }
                if let Some(command) = self.contextual_command(&command) {
                    args.push("--command".into());
                    args.push(command);
                }
                let id = extract_id(self.must_owned(&args)?.line(), "workspace")?;
                Ok(SpawnedTarget {
                    target: MuxTarget::session(MuxKind::Cmux, id),
                    created: true,
                    note: (request.placement == Placement::Background).then(|| {
                        "cmux has no detached workspace, so this one is visible in the switcher"
                            .to_string()
                    }),
                })
            }
        }
    }

    fn focus(&self, target: &MuxTarget) -> Result<()> {
        if let Some(surface) = &target.surface {
            // Current cmux reports stable Surface handles and the typed
            // `surface.focus` method in `capabilities`, but the signed v0.64.22
            // compatibility CLI does not expose a Surface-focus subcommand. Use
            // the provider's documented JSON-lines socket for that exact method;
            // older providers keep the legacy pane-focus path below.
            let probe = self.probe_or_empty();
            if probe.methods.contains("surface.focus") {
                self.call_v2(
                    "surface.focus",
                    serde_json::json!({ "surface_id": surface }),
                )?;
            } else {
                self.must(&["focus-pane", "--pane", surface])?;
            }
        } else if let Some(workspace) = &target.session {
            self.must(&["select-workspace", "--workspace", workspace])?;
        } else {
            return Err(AikitError::new(
                "mux.no_target",
                "focus was asked for without naming anything to focus",
            ));
        }
        Ok(())
    }

    fn close(&self, target: &MuxTarget) -> Result<()> {
        if let Some(surface) = &target.surface {
            // As with focus, current cmux exposes a stable Surface handle and
            // typed `surface.close` on the v2 socket. The v0.64.22 compatibility
            // CLI requires enclosing workspace/window scope for close-surface,
            // while the typed operation accepts the Surface id directly.
            let probe = self.probe_or_empty();
            if probe.methods.contains("surface.close") {
                self.call_v2(
                    "surface.close",
                    serde_json::json!({ "surface_id": surface }),
                )?;
            } else {
                self.must(&["close-surface", "--surface", surface])?;
            }
        } else if let Some(workspace) = &target.session {
            self.must(&["close-workspace", "--workspace", workspace])?;
        } else {
            return Err(AikitError::new(
                "mux.no_target",
                "close was asked for without naming anything to close",
            ));
        }
        Ok(())
    }

    fn open_palette(&self, request: PaletteRequest) -> Result<UiHost> {
        let capabilities = self.capabilities();
        if capabilities.true_popup {
            return Ok(UiHost::TruePopup {
                target: String::new(),
            });
        }
        if request.prefer_temporary_surface && capabilities.panes {
            let workspace = self
                .env
                .get("CMUX_WORKSPACE_ID")
                .cloned()
                .unwrap_or_default();
            let origin = self.env.get("CMUX_SURFACE_ID").cloned().unwrap_or_default();
            let surface = self.split_surface(&workspace, &origin, Direction::Right)?;
            return Ok(UiHost::TemporarySurface { id: surface });
        }
        // The honest default: no documented arbitrary-popup primitive is assumed,
        // so the palette is a modal in the terminal the user is already looking at.
        Ok(UiHost::InlineCurrentTerminal)
    }

    fn set_status(&self, status: StatusUpdate) -> Result<()> {
        let workspace = status.session.clone().unwrap_or_default();
        let mut values = status.values.clone();
        let colour = values.remove("color");

        let pill = values
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(" · ");

        let probe = self.probe_or_empty();
        if !probe.has_command("workspace-action") {
            return Ok(());
        }

        let mut args: Vec<String> = vec![
            "workspace-action".into(),
            "--action".into(),
            "set-title".into(),
            "--workspace".into(),
            workspace,
            "--title".into(),
            pill,
        ];
        if let Some(colour) = colour {
            args.push("--color".into());
            args.push(colour);
        }
        self.must_owned(&args)?;
        Ok(())
    }

    fn notify(&self, notification: Notification) -> Result<()> {
        let probe = self.probe_or_empty();
        if probe.has_command("notify") {
            let mut args: Vec<String> = vec![
                "notify".into(),
                "--title".into(),
                notification.title.clone(),
                "--body".into(),
                notification.body.clone(),
            ];
            if let Some(workspace) = notification.target.as_ref().and_then(|t| t.session.clone()) {
                args.push("--workspace".into());
                args.push(workspace);
            }
            self.must_owned(&args)?;
            return Ok(());
        }

        // No native notification. The status pill is a worse place for a message
        // than a toast, but it is a great deal better than nowhere.
        if probe.has_command("workspace-action") {
            let workspace = notification
                .target
                .as_ref()
                .and_then(|t| t.session.clone())
                .or_else(|| self.env.get("CMUX_WORKSPACE_ID").cloned())
                .unwrap_or_default();
            self.post(
                SidebarSurface::StatusPill,
                &workspace,
                &format!("{}: {}", notification.title, notification.body),
            )?;
            return Ok(());
        }

        Err(AikitError::new(
            "mux.unsupported",
            "this cmux has neither a notification nor a status surface, so there is nowhere to \
             put this message",
        )
        .with("title", notification.title))
    }
}
