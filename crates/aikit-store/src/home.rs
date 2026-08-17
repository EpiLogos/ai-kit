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
    pub fn discover() -> Result<Self> {
        let aikit_home = std::env::var_os("AIKIT_HOME");
        let user_home = std::env::var_os("HOME");
        Self::from_env_values(aikit_home.as_deref(), user_home.as_deref())
    }

    /// The pure half of [`Self::discover`], so the precedence rule is testable
    /// without mutating the process environment — which is a global, and which
    /// two tests running in parallel would fight over.
    pub fn from_env_values(
        aikit_home: Option<&OsStr>,
        user_home: Option<&OsStr>,
    ) -> Result<Self> {
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

    /// Create the whole documented layout. Idempotent: existing directories and
    /// their contents are left exactly as they were.
    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            self.root.clone(),
            self.registries(),
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
            create_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Create (or confirm) one context's directory, including `generations/`.
    pub fn ensure_context_dir(&self, context: &ContextId) -> Result<PathBuf> {
        let dir = self.context_dir(context);
        create_dir_all(&dir.join("generations"))?;
        Ok(dir)
    }

    pub fn ensure_session_dir(&self, session: &SessionId) -> Result<PathBuf> {
        let dir = self.session_dir(session);
        create_dir_all(&dir)?;
        Ok(dir)
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