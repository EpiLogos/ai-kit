//! The AIKit home directory.
//!
//! Every path AIKit writes to is derived from one root, and the root is derived
//! from `AIKIT_HOME` or `~/.aikit`. Nothing else in this crate is allowed to
//! consult the environment for a location: a second opinion about where the home
//! is would eventually disagree with the first, and the failure mode of that
//! disagreement is a generation written somewhere the reader never looks.
//!
//! The accessors are typed rather than string-joined at each call site so that
//! the layout in `ARCHITECTURE.md` §5 has exactly one encoding:
//!
//! ```text
//! <home>/
//!   config.toml
//!   scopes/global/profile.toml
//!   registries/<name>/capsules/... profiles/...
//!   profiles/<group>/<name>.toml
//!   inbox/{ready,quarantine,rejected}/
//!   state/{aikit.sqlite3,contexts/,sessions/,locks/,trust/,credentials/}
//!   cache/
//!   logs/events.jsonl
//! ```

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use aikit_core::error::err;
use aikit_core::{AikitError, ContextId, Result, SessionId};

/// A resolved AIKit home. Construction never creates anything; see
/// [`AikitHome::ensure_layout`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AikitHome {
    root: PathBuf,
}

impl AikitHome {
    /// Root the home at an explicit directory. This is what tests use, and what
    /// `--home` would use.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The home for this process: `AIKIT_HOME`, else `~/.aikit`.
    ///
    /// A home whose folder is gone is refused here ([`Self::ground_present`]).
    /// Every command that opens the home finds it through this door, so none
    /// starts in a World that has been removed, where the first folder it made
    /// would make the World again (O-I #1001). A World removed while a command
    /// is already running is not caught here: the window that removed it stops
    /// asking AIKit anything there.
    pub fn discover() -> Result<Self> {
        let aikit_home = std::env::var_os("AIKIT_HOME");
        let user_home = std::env::var_os("HOME");
        let home = Self::from_env_values(aikit_home.as_deref(), user_home.as_deref())?;
        home.ground_present()?;
        Ok(home)
    }

    /// The pure half of [`Self::discover`], so the precedence rule is testable
    /// without mutating the process environment — which is a global, and which
    /// two tests running in parallel would fight over.
    pub fn from_env_values(aikit_home: Option<&OsStr>, user_home: Option<&OsStr>) -> Result<Self> {
        if let Some(explicit) = aikit_home.filter(|v| !v.is_empty()) {
            return Ok(Self::at(PathBuf::from(explicit)));
        }
        match user_home.filter(|v| !v.is_empty()) {
            Some(home) => Ok(Self::at(PathBuf::from(home).join(".aikit"))),
            None => err(
                "home.not_found",
                "cannot locate an AIKit home: neither AIKIT_HOME nor HOME is set",
            ),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    // -- top level ---------------------------------------------------------

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn registries(&self) -> PathBuf {
        self.root.join("registries")
    }

    /// The personal KnowledgeDomain register, `<home>/domains`.
    ///
    /// Domains have been project-layer data only (`<project>/.aikit/domains`),
    /// which meant a convention could be declared inside one project and
    /// nowhere else — including not at the root register, where the root wiki
    /// and cross-project work live. This is the personal-scope half of the same
    /// declaration: same schema, same grammar, lower precedence than a
    /// project's own, so a project can still say something different about
    /// itself.
    pub fn domains(&self) -> PathBuf {
        self.root.join("domains")
    }

    /// The personal profile tree, `<home>/profiles`. Distinct from a registry's
    /// own `profiles/` directory.
    pub fn profiles(&self) -> PathBuf {
        self.root.join("profiles")
    }

    /// The active, lowest-precedence User Baseline Profile.
    ///
    /// This is deliberately outside `profiles/`: that tree contains reusable
    /// named profile definitions, while this file is a scope declaration that
    /// is resolved in every context.
    pub fn global_profile(&self) -> PathBuf {
        self.root.join("scopes/global/profile.toml")
    }

    pub fn inbox(&self) -> PathBuf {
        self.root.join("inbox")
    }

    pub fn inbox_ready(&self) -> PathBuf {
        self.inbox().join("ready")
    }

    pub fn inbox_quarantine(&self) -> PathBuf {
        self.inbox().join("quarantine")
    }

    pub fn inbox_rejected(&self) -> PathBuf {
        self.inbox().join("rejected")
    }

    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    // -- state -------------------------------------------------------------

    pub fn database(&self) -> PathBuf {
        self.state().join("aikit.sqlite3")
    }

    pub fn contexts(&self) -> PathBuf {
        self.state().join("contexts")
    }

    pub fn sessions(&self) -> PathBuf {
        self.state().join("sessions")
    }

    pub fn locks(&self) -> PathBuf {
        self.state().join("locks")
    }

    /// The Agency Gateway's well-known same-host carrier. Bootstrap, doctor,
    /// the terminal surface and the O:I desktop all address a default gateway
    /// here unless a carrier is named explicitly.
    pub fn gateway_socket(&self) -> PathBuf {
        self.state().join("gateway.sock")
    }

    /// The default semantic state file the gateway persists across restarts.
    pub fn gateway_state(&self) -> PathBuf {
        self.state().join("gateway.json")
    }

    /// `state/trust/` holds exported trust review material (signatures, notes).
    /// The authoritative trust records live in the database.
    pub fn trust_dir(&self) -> PathBuf {
        self.state().join("trust")
    }

    /// Provider-neutral credential binding metadata. Raw secrets never live here;
    /// native providers retain them in their own secure stores.
    pub fn credentials(&self) -> PathBuf {
        self.state().join("credentials")
    }

    /// Configuration-plane receipts and idempotency records (`state/config/`).
    /// The owner-side history an `oi.config-receipt/v1` `native_ref` points into.
    pub fn config_plane(&self) -> PathBuf {
        self.state().join("config")
    }

    pub fn event_log(&self) -> PathBuf {
        self.logs().join("events.jsonl")
    }

    // -- registries --------------------------------------------------------

    pub fn registry(&self, name: &str) -> PathBuf {
        self.registries().join(name)
    }

    pub fn registry_capsules(&self, name: &str) -> PathBuf {
        self.registry(name).join("capsules")
    }

    pub fn registry_profiles(&self, name: &str) -> PathBuf {
        self.registry(name).join("profiles")
    }

    // -- per-context and per-session ---------------------------------------

    pub fn context_dir(&self, context: &ContextId) -> PathBuf {
        self.contexts().join(context.as_str())
    }

    pub fn session_dir(&self, session: &SessionId) -> PathBuf {
        self.sessions().join(session.as_str())
    }

    pub fn session_overlay(&self, session: &SessionId) -> PathBuf {
        self.session_dir(session).join("overlay.toml")
    }

    /// Advisory lock files are named after whatever is being serialized — a
    /// context id, usually — and are deliberately *not* placed inside the thing
    /// they guard, so that a lock survives the directory being replaced.
    pub fn lock_file(&self, key: &str) -> PathBuf {
        self.locks().join(format!("{key}.lock"))
    }

    // -- creation ----------------------------------------------------------

    /// Refuse a home whose folder is gone. A home stands in the folder that holds it: the
    /// person's home folder for `~/.aikit`, a World's `home` for that World's. Once that folder
    /// has been removed (the World was deleted), nothing may bring the home back: creating it
    /// would create the removed folders again around it, and an owner started there would serve
    /// a World that no longer exists. A first run, where the home itself does not exist yet but
    /// the folder that holds it does, is not refused.
    pub fn ground_present(&self) -> Result<()> {
        if self.root.is_dir() {
            return Ok(());
        }
        let folder = match self.root.parent() {
            None => return Ok(()),
            Some(parent) if parent.as_os_str().is_empty() => Path::new("."),
            Some(parent) => parent,
        };
        if folder.is_dir() {
            return Ok(());
        }
        Err(AikitError::new(
            "home.ground_gone",
            format!(
                "the folder that holds the AIKit home {} is gone ({}): nothing is created or started there",
                self.root.display(),
                folder.display()
            ),
        )
        .with("path", self.root.display().to_string()))
    }

    /// Create the whole documented layout. Idempotent: existing directories and
    /// their contents are left exactly as they were. It makes the home and what
    /// lies in it, never the folder that holds it: where that folder is gone it
    /// refuses ([`Self::ground_present`]), and a folder whose parent went while
    /// the layout was being made is not made either.
    pub fn ensure_layout(&self) -> Result<()> {
        self.ground_present()?;
        for dir in [
            self.root.clone(),
            self.registries(),
            self.domains(),
            self.profiles(),
            self.inbox_ready(),
            self.inbox_quarantine(),
            self.inbox_rejected(),
            self.state(),
            self.contexts(),
            self.sessions(),
            self.locks(),
            self.trust_dir(),
            self.credentials(),
            self.cache(),
            self.logs(),
        ] {
            self.create_within(&dir)?;
        }
        Ok(())
    }

    /// Create (or confirm) one context's directory, including `generations/`.
    pub fn ensure_context_dir(&self, context: &ContextId) -> Result<PathBuf> {
        let dir = self.context_dir(context);
        self.create_within(&dir.join("generations"))?;
        Ok(dir)
    }

    pub fn ensure_session_dir(&self, session: &SessionId) -> Result<PathBuf> {
        let dir = self.session_dir(session);
        self.create_within(&dir)?;
        Ok(dir)
    }

    /// Make `dir`, a folder of this home, and the folders between it and the
    /// home, one at a time from the home down. Nothing above the home is made:
    /// a home whose folder went while it was being made (its World removed
    /// meanwhile) fails here, where `create_dir_all` would make the removed
    /// folders again around it.
    fn create_within(&self, dir: &Path) -> Result<()> {
        let below = dir.strip_prefix(&self.root).map_err(|_| {
            AikitError::new(
                "home.create_failed",
                format!(
                    "{} is not inside the AIKit home {}",
                    dir.display(),
                    self.root.display()
                ),
            )
            .with("path", dir.display().to_string())
        })?;
        let mut path = self.root.clone();
        make_dir(&path)?;
        for part in below.components() {
            path.push(part);
            make_dir(&path)?;
        }
        Ok(())
    }
}

/// Make one folder whose parent stands. A folder already there is left as it
/// is; a parent that is gone is the home's ground gone, and nothing is made.
fn make_dir(path: &Path) -> Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(AikitError::new(
            "home.ground_gone",
            format!(
                "{}: the folder that holds it is gone, and nothing is created there",
                path.display()
            ),
        )
        .with("path", path.display().to_string())),
        Err(e) => Err(io_error("home.create_failed", path, &e)),
    }
}

/// `create_dir_all` with an AIKit error that names the path.
///
/// A bare `io::Error` here reads "Permission denied" with no indication of what
/// was denied, which is exactly the report a user cannot act on.
pub(crate) fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| io_error("home.create_failed", path, &e))
}

pub(crate) fn io_error(code: &'static str, path: &Path, source: &std::io::Error) -> AikitError {
    AikitError::new(code, format!("{}: {source}", path.display()))
        .with("path", path.display().to_string())
}
