//! tmux, as a first-class multiplexer rather than a lowest common denominator.
//!
//! ## Non-destructive by default
//!
//! [`Tmux::ensure_session`] under [`ReconcileMode::CreateOrAttach`] creates what
//! is missing and touches nothing else. It does not re-run a startup command in a
//! pane that is already alive, and it does not close a pane the user split off by
//! hand. This is the difference between a session manager people leave running
//! and one they stop trusting after it eats a long-running process.
//!
//! Knowing which panes are AIKit's requires marking them, so every pane this
//! adapter creates gets an `@aikit_pane` pane option naming the plan step it came
//! from. A pane without that option was made by a person, and `CreateOrAttach`
//! will never remove it.
//!
//! ## Why the environment is set between creating the session and running commands
//!
//! tmux copies the *session* environment into each pane as it is created. A
//! variable set after a pane exists never reaches it. So the session is created
//! with no command at all, `set-environment` runs, and only then is the root
//! pane's command started with `respawn-pane`. Splits made afterwards inherit
//! normally and need no such dance.
//!
//! ## Why commands are joined into one shell word
//!
//! [`SessionPlan`] carries argv, because a shell string would need quoting rules
//! that differ per multiplexer and per shell. tmux takes a *shell command*, so
//! the adapter joins with [`shell_words::join`] at the boundary — one place,
//! testable — rather than relying on tmux's own argument joining, which has not
//! been identical across versions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use aikit_core::platform::MuxKind;
use aikit_core::session::{Direction, PaneStep, Placement, SessionPlan, ViewPlan};
use aikit_core::{AikitError, Result};

use crate::runner::{CommandRunner, Output, SystemRunner};

pub use super::SessionIdentity;
use super::{
    MuxAdapter, MuxCapabilities, MuxLocation, MuxPresence, MuxTarget, Notification, PaletteRequest,
    ReconcileMode, SessionBinding, SpawnRequest, SpawnedTarget, StatusUpdate, UiHost,
};

/// The pane option marking a pane as created by AIKit from a plan step.
pub const PANE_TAG: &str = "@aikit_pane";
/// The session option carrying the AIKit session id, so a server restart that
/// hands back different pane ids can still be recognised.
pub const SESSION_OPTION: &str = "@aikit_session";
pub const PROFILE_OPTION: &str = "@aikit_profile";

// ---------------------------------------------------------------------------
// The adapter
// ---------------------------------------------------------------------------

pub struct Tmux<R> {
    runner: R,
    binary: String,
    /// A private server socket. Tests always set one; production usually does not.
    socket: Option<String>,
    identity: SessionIdentity,
    /// The ambient environment, injected so detection is testable.
    env: BTreeMap<String, String>,
}

impl Tmux<SystemRunner> {
    /// The adapter as production uses it: the real binary, the user's default
    /// socket, the real process environment.
    pub fn system() -> Self {
        let mut adapter = Self::new(SystemRunner::new());
        for key in ["TMUX", "TMUX_PANE", "SSH_CONNECTION"] {
            if let Ok(value) = std::env::var(key) {
                adapter.env.insert(key.to_string(), value);
            }
        }
        adapter
    }
}

impl<R: CommandRunner> Tmux<R> {
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            binary: "tmux".to_string(),
            socket: None,
            identity: SessionIdentity::default(),
            env: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_binary(mut self, binary: impl Into<String>) -> Self {
        self.binary = binary.into();
        self
    }

    /// Use a private server socket. Every command then carries `-L <name>`, which
    /// is what keeps an integration test out of the user's real session.
    #[must_use]
    pub fn with_socket(mut self, name: impl Into<String>) -> Self {
        self.socket = Some(name.into());
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

    pub fn identity(&self) -> &SessionIdentity {
        &self.identity
    }

    pub fn socket(&self) -> Option<&str> {
        self.socket.as_deref()
    }

    /// `tmux [-L socket] <args...>`.
    pub fn argv(&self, args: &[&str]) -> Vec<String> {
        let mut out = vec![self.binary.clone()];
        if let Some(socket) = &self.socket {
            out.push("-L".into());
            out.push(socket.clone());
        }
        out.extend(args.iter().map(|a| a.to_string()));
        out
    }

    fn run(&self, args: &[&str]) -> Result<Output> {
        self.runner.run(&self.argv(args))
    }

    /// Run a command that is expected to succeed, naming it if it does not.
    fn must(&self, args: &[&str]) -> Result<Output> {
        let argv = self.argv(args);
        self.runner.run(&argv)?.require(&argv, "mux.tmux_failed")
    }

    fn must_owned(&self, args: &[String]) -> Result<Output> {
        self.must(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Does this session exist?
    ///
    /// `has-session` answers with its exit status, so a non-zero status here is
    /// the answer "no" rather than a failure.
    pub fn has_session(&self, name: &str) -> Result<bool> {
        Ok(self.run(&["has-session", "-t", name])?.ok())
    }

    /// Every pane in a window as `(pane id, plan tag)`.
    ///
    /// An empty tag means a person created that pane.
    pub fn panes_of(&self, window: &str) -> Result<Vec<(String, String)>> {
        let format = format!("#{{pane_id}} #{{{PANE_TAG}}}");
        let out = self.must(&["list-panes", "-t", window, "-F", &format])?;
        Ok(out
            .line()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let (id, tag) = split_field(line);
                (id.to_string(), tag.to_string())
            })
            .collect())
    }

    /// Window name → window id for a session.
    pub fn windows_of(&self, session: &str) -> Result<BTreeMap<String, String>> {
        let out = self.must(&[
            "list-windows",
            "-t",
            session,
            "-F",
            "#{window_id} #{window_name}",
        ])?;
        Ok(out
            .line()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let (id, name) = split_field(line);
                (name.to_string(), id.to_string())
            })
            .collect())
    }

    /// The opaque layout string tmux uses, for drift detection.
    ///
    /// AIKit deliberately does not interpret it. Comparing it against the string
    /// recorded when the session was built is enough to say "this no longer looks
    /// like the plan"; parsing tmux's checksum-prefixed geometry would amount to
    /// writing a second, worse layout engine.
    pub fn layout_of(&self, window: &str) -> Result<String> {
        Ok(self
            .must(&["display-message", "-p", "-t", window, "#{window_layout}"])?
            .line()
            .trim()
            .to_string())
    }

    /// Read back a session user option such as `@aikit_session`.
    pub fn session_option(&self, session: &str, option: &str) -> Result<Option<String>> {
        let out = self.run(&["show-options", "-t", session, "-v", option])?;
        if !out.ok() {
            return Ok(None);
        }
        let value = out.line().trim().to_string();
        Ok((!value.is_empty()).then_some(value))
    }

    /// Read back a pane user option such as `@aikit_pane`.
    pub fn pane_option(&self, pane: &str, option: &str) -> Result<Option<String>> {
        let out = self.run(&["show-options", "-p", "-t", pane, "-v", option])?;
        if !out.ok() {
            return Ok(None);
        }
        let value = out.line().trim().to_string();
        Ok((!value.is_empty()).then_some(value))
    }

    /// Kill the whole server. Integration tests call this from a drop guard.
    pub fn kill_server(&self) -> Result<()> {
        self.run(&["kill-server"]).map(|_| ())
    }

    // -----------------------------------------------------------------------
    // Building
    // -----------------------------------------------------------------------

    fn cwd_for<'a>(&self, step: &'a PaneStep, plan: &'a SessionPlan) -> Option<&'a Path> {
        step.cwd.as_deref().or(plan.root.as_deref())
    }

    fn apply_environment(&self, session: &str) -> Result<()> {
        for (key, value) in self.identity.environment() {
            self.must(&["set-environment", "-t", session, &key, &value])?;
        }
        for (key, value) in self.identity.user_options() {
            self.must(&["set-option", "-t", session, &key, &value])?;
        }
        Ok(())
    }

    fn tag_pane(&self, pane: &str, view: &str, step: &str) -> Result<()> {
        self.must(&[
            "set-option",
            "-p",
            "-t",
            pane,
            PANE_TAG,
            &format!("{view}/{step}"),
        ])?;
        Ok(())
    }

    /// Create a view as a new window and return its root pane id.
    fn create_window(&self, session: &str, view: &ViewPlan, plan: &SessionPlan) -> Result<String> {
        // The compiler rejects a view with no panes, so this holds for any
        // compiled plan; guard rather than index so a hand-built plan gives a
        // clear error instead of a panic in the adapter.
        let step = view.steps.first().ok_or_else(|| {
            AikitError::new(
                "session.empty_view",
                format!("view `{}` has no panes", view.id),
            )
        })?;
        let name = view.name.clone().unwrap_or_else(|| view.id.clone());
        let mut args: Vec<String> = vec![
            "new-window".into(),
            "-t".into(),
            format!("{session}:"),
            "-n".into(),
            name,
        ];
        if let Some(cwd) = self.cwd_for(step, plan) {
            args.push("-c".into());
            args.push(cwd.display().to_string());
        }
        args.extend(["-P".into(), "-F".into(), "#{pane_id}".into()]);
        if !step.command.is_empty() {
            args.push(shell_words::join(&step.command));
        }
        Ok(self.must_owned(&args)?.line().trim().to_string())
    }

    fn split(&self, from_pane: &str, step: &PaneStep, plan: &SessionPlan) -> Result<String> {
        let split = step.split.as_ref().ok_or_else(|| {
            AikitError::new(
                "mux.tmux_failed",
                format!("pane `{}` is a view root and cannot be split off", step.pane),
            )
        })?;
        let mut args: Vec<String> = vec!["split-window".into(), "-t".into(), from_pane.to_string()];
        args.extend(direction_flags(split.direction));
        if let Some(ratio) = split.ratio {
            args.push("-l".into());
            args.push(format!("{}%", percent(ratio)));
        }
        if let Some(cwd) = self.cwd_for(step, plan) {
            args.push("-c".into());
            args.push(cwd.display().to_string());
        }
        args.extend(["-P".into(), "-F".into(), "#{pane_id}".into()]);
        if !step.command.is_empty() {
            args.push(shell_words::join(&step.command));
        }
        Ok(self.must_owned(&args)?.line().trim().to_string())
    }

    /// Create every step of a view except the root, which already exists.
    fn build_view_body(
        &self,
        view: &ViewPlan,
        plan: &SessionPlan,
        panes: &mut BTreeMap<String, String>,
        binding: &mut SessionBinding,
    ) -> Result<()> {
        for step in view.steps.iter().skip(1) {
            let Some(split) = &step.split else { continue };
            let Some(parent) = panes.get(&split.from).cloned() else {
                binding.warnings.push(format!(
                    "pane `{}` splits from `{}`, which was not created; it was skipped",
                    step.pane, split.from
                ));
                continue;
            };
            let pane = self.split(&parent, step, plan)?;
            self.tag_pane(&pane, &view.id, &step.pane)?;
            panes.insert(step.pane.clone(), pane.clone());
            binding
                .surfaces
                .insert(format!("{}/{}", view.id, step.pane), pane);
            binding.record(format!("created pane {}/{}", view.id, step.pane));
        }
        Ok(())
    }

    /// Bring one view of an existing session closer to the plan.
    ///
    /// Under `CreateOrAttach` this only ever *adds*. Panes the plan already has
    /// are left running — re-issuing a startup command in a live pane is how a
    /// session manager kills a build someone was watching.
    fn reconcile_view(
        &self,
        session: &str,
        view: &ViewPlan,
        plan: &SessionPlan,
        mode: ReconcileMode,
        binding: &mut SessionBinding,
    ) -> Result<()> {
        let name = view.name.clone().unwrap_or_else(|| view.id.clone());
        let windows = self.windows_of(session)?;

        let Some(window) = windows.get(&name).cloned() else {
            let root = self.create_window(session, view, plan)?;
            self.tag_pane(&root, &view.id, &view.steps[0].pane)?;
            binding.views.insert(view.id.clone(), name);
            binding
                .surfaces
                .insert(format!("{}/{}", view.id, view.steps[0].pane), root.clone());
            binding.record(format!("created view {}", view.id));
            let mut panes = BTreeMap::new();
            panes.insert(view.steps[0].pane.clone(), root);
            return self.build_view_body(view, plan, &mut panes, binding);
        };

        binding.views.insert(view.id.clone(), name);

        let mut by_tag: BTreeMap<String, String> = BTreeMap::new();
        let mut untagged: Vec<String> = Vec::new();
        for (pane, tag) in self.panes_of(&window)? {
            if tag.is_empty() {
                untagged.push(pane);
            } else {
                by_tag.insert(tag, pane);
            }
        }

        let planned: BTreeSet<String> = view
            .steps
            .iter()
            .map(|s| format!("{}/{}", view.id, s.pane))
            .collect();

        let mut panes: BTreeMap<String, String> = BTreeMap::new();
        for step in &view.steps {
            let key = format!("{}/{}", view.id, step.pane);
            if let Some(pane) = by_tag.get(&key) {
                panes.insert(step.pane.clone(), pane.clone());
                binding.surfaces.insert(key.clone(), pane.clone());
                binding.preserve(format!("{key} is already running; left as it is"));
            }
        }

        if mode.may_close_panes() {
            for (tag, pane) in &by_tag {
                if !planned.contains(tag) {
                    self.must(&["kill-pane", "-t", pane])?;
                    binding.record(format!("closed pane {tag}, which the plan no longer declares"));
                }
            }
            for pane in &untagged {
                self.must(&["kill-pane", "-t", pane])?;
                binding.record(format!(
                    "closed pane {pane}, which AIKit did not create (exact reconcile)"
                ));
            }
        } else {
            for pane in &untagged {
                binding.preserve(format!("{pane} was created by hand; left alone"));
            }
        }

        for step in &view.steps {
            let key = format!("{}/{}", view.id, step.pane);
            if panes.contains_key(&step.pane) {
                continue;
            }
            let pane = match &step.split {
                Some(split) => {
                    let parent = match panes.get(&split.from).cloned() {
                        Some(parent) => parent,
                        None => {
                            // The parent is not there. Splitting whatever the view
                            // does have is the honest approximation, and the
                            // binding says so rather than failing the whole
                            // reconcile over one pane.
                            let fallback = panes
                                .values()
                                .next()
                                .cloned()
                                .unwrap_or_else(|| window.clone());
                            binding.warnings.push(format!(
                                "pane `{key}` splits from `{}`, which is not in the session; it \
                                 was split from `{fallback}` instead",
                                split.from
                            ));
                            fallback
                        }
                    };
                    self.split(&parent, step, plan)?
                }
                // The window exists, so its first pane *is* the view root, however
                // it came to be there.
                None => self
                    .panes_of(&window)?
                    .first()
                    .map(|(id, _)| id.clone())
                    .unwrap_or_else(|| window.clone()),
            };
            self.tag_pane(&pane, &view.id, &step.pane)?;
            panes.insert(step.pane.clone(), pane.clone());
            binding.surfaces.insert(key.clone(), pane);
            binding.record(format!("created pane {key}"));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Installing the binding
// ---------------------------------------------------------------------------

/// The opening marker of AIKit's managed block in a tmux config.
///
/// The markers are a promise with two directions: everything between them is
/// AIKit's to rewrite, and everything outside them is the user's and is never
/// touched. Uninstall is "delete what is between the markers", which a person can
/// also do by hand with an editor.
pub const BLOCK_START: &str = "# >>> aikit >>>";
pub const BLOCK_END: &str = "# <<< aikit <<<";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallAction {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    pub path: PathBuf,
    pub action: InstallAction,
    pub changed: bool,
}

/// Render the managed block for a chosen prefix key.
///
/// The key is a parameter with no default on purpose. tmux users bind their own
/// prefix table heavily, and a tool that picked its own key would silently
/// shadow somebody's binding — the kind of intrusion that is only noticed weeks
/// later, when the old binding is missed.
pub fn config_block(key: &str) -> String {
    let request = PaletteRequest::default();
    format!(
        "{BLOCK_START}\n\
         # Managed by AIKit. Everything between these markers is regenerated;\n\
         # edit the key with `aikit install tmux --key <key>` rather than by hand.\n\
         bind-key {key} display-popup -E -w {w}% -h {h}% -T {title} 'aikit palette'\n\
         set-option -g @aikit_installed 1\n\
         {BLOCK_END}",
        w = request.width_percent,
        h = request.height_percent,
        title = request.title,
    )
}

/// Write (or refresh) AIKit's block in a tmux config file.
///
/// Idempotent by construction: the block is located by its markers and replaced,
/// never appended to. Running this twice is a byte-for-byte no-op.
pub fn install(config_path: &Path, key: &str) -> Result<InstallOutcome> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AikitError::new(
            "mux.invalid_key",
            "a key binding needs a key; pass the one you want the palette on",
        ));
    }

    let existing = match std::fs::read_to_string(config_path) {
        Ok(contents) => Some(contents),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(AikitError::new(
                "mux.config_unreadable",
                format!("could not read {}: {e}", config_path.display()),
            )
            .with("path", config_path.display().to_string()))
        }
    };

    let block = config_block(key);
    let (next, action) = match &existing {
        None => (format!("{block}\n"), InstallAction::Created),
        Some(contents) => match splice(contents, &block) {
            Some(next) if &next == contents => (next, InstallAction::Unchanged),
            Some(next) => (next, InstallAction::Updated),
            None => {
                let separator = if contents.is_empty() || contents.ends_with('\n') {
                    ""
                } else {
                    "\n"
                };
                (
                    format!("{contents}{separator}{block}\n"),
                    InstallAction::Created,
                )
            }
        },
    };

    if action != InstallAction::Unchanged {
        if let Some(parent) = config_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AikitError::new(
                        "mux.config_unwritable",
                        format!("could not create {}: {e}", parent.display()),
                    )
                })?;
            }
        }
        std::fs::write(config_path, &next).map_err(|e| {
            AikitError::new(
                "mux.config_unwritable",
                format!("could not write {}: {e}", config_path.display()),
            )
            .with("path", config_path.display().to_string())
        })?;
    }

    Ok(InstallOutcome {
        path: config_path.to_path_buf(),
        action,
        changed: action != InstallAction::Unchanged,
    })
}

/// Replace an existing marked block, or `None` when there is none to replace.
fn splice(contents: &str, block: &str) -> Option<String> {
    let start = contents.find(BLOCK_START)?;
    let end = contents[start..].find(BLOCK_END)? + start + BLOCK_END.len();
    Some(format!("{}{block}{}", &contents[..start], &contents[end..]))
}

/// Split one `-F` output line into its leading id and the rest.
///
/// A single space is the separator, and the id always comes first, because tmux
/// *rewrites tab characters in `-F` output as underscores* — a two-field format
/// separated by a tab silently comes back as one field. Ids (`%3`, `@1`) never
/// contain a space; names and pane tags may, so they take the remainder.
fn split_field(line: &str) -> (&str, &str) {
    match line.split_once(' ') {
        Some((id, rest)) => (id.trim(), rest.trim()),
        None => (line.trim(), ""),
    }
}

/// tmux expresses a split as an axis plus a "before" flag; the portable format
/// uses compass directions, because "horizontal split" means opposite things in
/// different multiplexers.
fn direction_flags(direction: Direction) -> Vec<String> {
    let mut flags = vec![if direction.is_horizontal() { "-h" } else { "-v" }.to_string()];
    if matches!(direction, Direction::Left | Direction::Up) {
        flags.push("-b".to_string());
    }
    flags
}

/// A ratio in `(0, 1)` as a tmux percentage.
///
/// Clamped rather than rejected: the plan compiler has already refused ratios
/// outside the open interval, so anything arriving here is a rounding artefact at
/// the extremes rather than a user error worth failing a session build for.
fn percent(ratio: f64) -> u32 {
    ((ratio * 100.0).round() as i64).clamp(1, 99) as u32
}

// ---------------------------------------------------------------------------
// MuxAdapter
// ---------------------------------------------------------------------------

impl<R: CommandRunner> MuxAdapter for Tmux<R> {
    fn kind(&self) -> MuxKind {
        MuxKind::Tmux
    }

    fn capabilities(&self) -> MuxCapabilities {
        MuxCapabilities {
            true_popup: true,
            workspaces: true,
            // A tmux session groups windows, but there is no switchable group of
            // *workspaces* in cmux's sense. Claiming it would make the stack
            // adapter route grouping operations somewhere they cannot land.
            workspace_groups: false,
            windows: false,
            panes: true,
            browser_surface: false,
            status_metadata: true,
            notifications: false,
            remote_control: true,
        }
    }

    fn detect(&self) -> Result<MuxPresence> {
        let version = match self.run(&["-V"]) {
            Ok(out) if out.ok() => out.line().trim().to_string(),
            Ok(out) => {
                return Ok(MuxPresence::absent(
                    MuxKind::Tmux,
                    format!("`tmux -V` exited with status {}", out.status),
                ))
            }
            Err(e) if e.code() == "mux.command_spawn_failed" => {
                return Ok(MuxPresence::absent(MuxKind::Tmux, "`tmux` is not on PATH"))
            }
            Err(e) => return Err(e),
        };

        // `list-sessions` fails with a message when no server is up, so its exit
        // status is exactly the question "is a server running?".
        let server_running = self
            .run(&["list-sessions"])
            .map(|out| out.ok())
            .unwrap_or(false);

        Ok(MuxPresence {
            kind: MuxKind::Tmux,
            installed: true,
            version: Some(version.trim_start_matches("tmux ").to_string()),
            server_running,
            inside: self.env.contains_key("TMUX"),
            detail: None,
        })
    }

    fn current_location(&self) -> Result<MuxLocation> {
        if !self.env.contains_key("TMUX") {
            return Ok(MuxLocation::nowhere(MuxKind::Tmux));
        }
        let out = self.must(&[
            "display-message",
            "-p",
            "#{session_name}\t#{window_id}\t#{pane_id}\t#{host}",
        ])?;
        let fields: Vec<&str> = out.line().split('\t').collect();
        let field = |i: usize| fields.get(i).map(|s| s.trim()).filter(|s| !s.is_empty());
        Ok(MuxLocation {
            kind: MuxKind::Tmux,
            host: field(3).unwrap_or_default().to_string(),
            remote: false,
            project: self
                .identity
                .project_root
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string()),
            session: field(0).map(str::to_string),
            view: field(1).map(str::to_string),
            surface: field(2).map(str::to_string),
        })
    }

    fn ensure_session(&self, plan: &SessionPlan, mode: ReconcileMode) -> Result<SessionBinding> {
        let Some(first_view) = plan.views.first() else {
            return Err(AikitError::new(
                "mux.empty_plan",
                format!("session plan `{}` declares no views", plan.id),
            )
            .with("session", plan.id.clone()));
        };

        let name = plan.id.clone();
        let mut binding = SessionBinding::new(MuxKind::Tmux, &name);
        binding.warnings.extend(plan.warnings.clone());

        if self.has_session(&name)? {
            binding.created = false;
            // Refreshed even on attach: the context id changes when the overlay
            // changes, and a stale one would send new panes to the wrong view.
            self.apply_environment(&name)?;
            for view in &plan.views {
                self.reconcile_view(&name, view, plan, mode, &mut binding)?;
            }
        } else {
            binding.created = true;
            let root_step = first_view.steps.first().ok_or_else(|| {
                AikitError::new(
                    "session.empty_view",
                    format!("view `{}` has no panes", first_view.id),
                )
            })?;
            let view_name = first_view
                .name
                .clone()
                .unwrap_or_else(|| first_view.id.clone());

            let mut args: Vec<String> = vec![
                "new-session".into(),
                "-d".into(),
                "-s".into(),
                name.clone(),
                "-n".into(),
                view_name.clone(),
            ];
            if let Some(cwd) = self.cwd_for(root_step, plan) {
                args.push("-c".into());
                args.push(cwd.display().to_string());
            }
            args.extend(["-P".into(), "-F".into(), "#{pane_id}".into()]);
            let root = self.must_owned(&args)?.line().trim().to_string();

            // Before any command runs anywhere. See the module documentation.
            self.apply_environment(&name)?;

            self.tag_pane(&root, &first_view.id, &root_step.pane)?;
            if !root_step.command.is_empty() {
                self.must(&[
                    "respawn-pane",
                    "-k",
                    "-t",
                    &root,
                    &shell_words::join(&root_step.command),
                ])?;
            }
            binding.views.insert(first_view.id.clone(), view_name);
            binding
                .surfaces
                .insert(format!("{}/{}", first_view.id, root_step.pane), root.clone());
            binding.record(format!("created session {name}"));

            let mut panes = BTreeMap::new();
            panes.insert(root_step.pane.clone(), root);
            self.build_view_body(first_view, plan, &mut panes, &mut binding)?;

            for view in plan.views.iter().skip(1) {
                let window_root = self.create_window(&name, view, plan)?;
                self.tag_pane(&window_root, &view.id, &view.steps[0].pane)?;
                binding.views.insert(
                    view.id.clone(),
                    view.name.clone().unwrap_or_else(|| view.id.clone()),
                );
                binding.surfaces.insert(
                    format!("{}/{}", view.id, view.steps[0].pane),
                    window_root.clone(),
                );
                binding.record(format!("created view {}", view.id));
                let mut view_panes = BTreeMap::new();
                view_panes.insert(view.steps[0].pane.clone(), window_root);
                self.build_view_body(view, plan, &mut view_panes, &mut binding)?;
            }
        }

        // Focus last, so a pane created after the focused one cannot steal it.
        for view in &plan.views {
            let Some(focus) = &view.focus else { continue };
            if let Some(pane) = binding.surface_of(&view.id, focus) {
                self.must(&["select-pane", "-t", pane])?;
            }
        }
        if let Some(pane) = binding.surface_of(&first_view.id, &first_view.steps[0].pane) {
            self.must(&["select-window", "-t", pane])?;
        }

        Ok(binding)
    }

    fn spawn(&self, request: SpawnRequest) -> Result<SpawnedTarget> {
        let target = request
            .target
            .clone()
            .unwrap_or_else(|| MuxTarget::surface(MuxKind::Tmux, ""));
        let selector = target.selector();

        match request.placement {
            Placement::Current => {
                if selector.is_empty() {
                    return Err(AikitError::new(
                        "mux.no_target",
                        "running in the current pane needs a pane to run in, and none was given",
                    ));
                }
                if !request.command.is_empty() {
                    self.must(&[
                        "respawn-pane",
                        "-k",
                        "-t",
                        &selector,
                        &shell_words::join(&request.command),
                    ])?;
                }
                Ok(SpawnedTarget {
                    target: MuxTarget::surface(MuxKind::Tmux, selector),
                    created: false,
                    note: None,
                })
            }
            Placement::NewPane => {
                if selector.is_empty() {
                    return Err(AikitError::new(
                        "mux.no_target",
                        "splitting a new pane needs a pane to split, and none was given",
                    ));
                }
                let mut args: Vec<String> =
                    vec!["split-window".into(), "-t".into(), selector.clone()];
                args.extend(direction_flags(request.direction));
                if let Some(ratio) = request.ratio {
                    args.push("-l".into());
                    args.push(format!("{}%", percent(ratio)));
                }
                if let Some(cwd) = &request.cwd {
                    args.push("-c".into());
                    args.push(cwd.display().to_string());
                }
                args.extend(["-P".into(), "-F".into(), "#{pane_id}".into()]);
                if !request.command.is_empty() {
                    args.push(shell_words::join(&request.command));
                }
                Ok(SpawnedTarget {
                    target: MuxTarget::surface(
                        MuxKind::Tmux,
                        self.must_owned(&args)?.line().trim(),
                    ),
                    created: true,
                    note: None,
                })
            }
            Placement::NewView | Placement::Background => {
                let session = target
                    .session
                    .clone()
                    .or_else(|| target.surface.clone())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        AikitError::new(
                            "mux.no_target",
                            "creating a window needs a session to create it in",
                        )
                    })?;
                let mut args: Vec<String> = vec!["new-window".into()];
                if request.placement == Placement::Background {
                    // `-d` is the whole difference: a background job that steals
                    // the user's focus is not a background job.
                    args.push("-d".into());
                }
                args.push("-t".into());
                args.push(format!("{session}:"));
                if let Some(name) = &request.name {
                    args.push("-n".into());
                    args.push(name.clone());
                }
                if let Some(cwd) = &request.cwd {
                    args.push("-c".into());
                    args.push(cwd.display().to_string());
                }
                args.extend(["-P".into(), "-F".into(), "#{pane_id}".into()]);
                if !request.command.is_empty() {
                    args.push(shell_words::join(&request.command));
                }
                Ok(SpawnedTarget {
                    target: MuxTarget::surface(
                        MuxKind::Tmux,
                        self.must_owned(&args)?.line().trim(),
                    ),
                    created: true,
                    note: (request.placement == Placement::Background)
                        .then(|| "created detached so it does not take focus".to_string()),
                })
            }
        }
    }

    fn focus(&self, target: &MuxTarget) -> Result<()> {
        let selector = target.selector();
        if selector.is_empty() {
            return Err(AikitError::new(
                "mux.no_target",
                "focus was asked for without naming anything to focus",
            ));
        }
        self.must(&["select-pane", "-t", &selector])?;
        // A pane in a background window is not visible merely because it is
        // selected; its window has to come forward too.
        self.must(&["select-window", "-t", &selector])?;
        Ok(())
    }

    fn close(&self, target: &MuxTarget) -> Result<()> {
        if let Some(surface) = &target.surface {
            self.must(&["kill-pane", "-t", surface])?;
        } else if let Some(view) = &target.view {
            self.must(&["kill-window", "-t", view])?;
        } else if let Some(session) = &target.session {
            self.must(&["kill-session", "-t", session])?;
        } else {
            return Err(AikitError::new(
                "mux.no_target",
                "close was asked for without naming anything to close",
            ));
        }
        Ok(())
    }

    fn open_palette(&self, request: PaletteRequest) -> Result<UiHost> {
        if request.command.is_empty() {
            // Asking where the palette would go must not open one.
            return Ok(UiHost::TruePopup {
                target: String::new(),
            });
        }
        let mut args: Vec<String> = vec![
            "display-popup".into(),
            "-E".into(),
            "-w".into(),
            format!("{}%", request.width_percent),
            "-h".into(),
            format!("{}%", request.height_percent),
            "-T".into(),
            request.title.clone(),
        ];
        if let Some(cwd) = &request.cwd {
            args.push("-d".into());
            args.push(cwd.display().to_string());
        }
        args.extend(request.command.iter().cloned());
        self.must_owned(&args)?;
        Ok(UiHost::TruePopup {
            target: String::new(),
        })
    }

    fn set_status(&self, status: StatusUpdate) -> Result<()> {
        let session = status.session.clone().unwrap_or_default();
        for (key, value) in &status.values {
            let option = format!("@aikit_{key}");
            if session.is_empty() {
                self.must(&["set-option", &option, value])?;
            } else {
                self.must(&["set-option", "-t", &session, &option, value])?;
            }
        }
        Ok(())
    }

    fn notify(&self, notification: Notification) -> Result<()> {
        // tmux has no notification primitive. `display-message` is the honest
        // equivalent: visible, transient, and not pretending to be an OS toast.
        let message = format!("{}: {}", notification.title, notification.body);
        match notification.target.as_ref().map(MuxTarget::selector) {
            Some(selector) if !selector.is_empty() => {
                self.must(&["display-message", "-t", &selector, &message])?;
            }
            _ => {
                self.must(&["display-message", &message])?;
            }
        }
        Ok(())
    }
}
