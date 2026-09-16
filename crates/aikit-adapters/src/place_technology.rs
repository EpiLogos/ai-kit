//! The place-technology registry: which place technologies this build
//! detects, and which of them it can drive as multiplexers.
//!
//! A session plan names the technology that owns its place with an open,
//! validated name ([`PlaceTechnology`]). This module is where a name becomes
//! a capability. The registry holds one entry per technology; an entry can
//! *detect* the technology on this host (a real probe, never an assumption)
//! and may hand back the [`MuxAdapter`] this build drives it with.
//!
//! An unregistered name stays first-class: [`PlaceTechnologyRegistry::resolve`]
//! returns `None` and the consumer states a declared-unsupported outcome
//! naming the technology and what would support it. Nothing here normalises an
//! unknown name into some other technology, and nothing crashes on it.

use aikit_core::Result;
use aikit_core::platform::{MuxKind, PlaceTechnology};

use crate::mux::{MuxAdapter, MuxPresence, cmux::Cmux, plain::Plain, tmux::Tmux};

/// The mux adapter behind a registry entry, as an object-safe handle.
pub type MuxAdapterHandle = Box<dyn MuxAdapter>;

/// What one place technology reports about its presence on this host.
///
/// The same facts [`MuxPresence`] carries for the built-in multiplexers,
/// named by the open technology name instead of a closed kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceTechnologyReading {
    pub technology: PlaceTechnology,
    pub installed: bool,
    pub version: Option<String>,
    /// A server/app is up, so an existing session could be attached to.
    pub server_running: bool,
    /// This process is running inside it.
    pub inside: bool,
    /// Why it is not usable, when it is not.
    pub detail: Option<String>,
}

impl PlaceTechnologyReading {
    pub fn absent(technology: PlaceTechnology, detail: impl Into<String>) -> Self {
        Self {
            technology,
            installed: false,
            version: None,
            server_running: false,
            inside: false,
            detail: Some(detail.into()),
        }
    }
}

impl From<MuxPresence> for PlaceTechnologyReading {
    fn from(presence: MuxPresence) -> Self {
        Self {
            technology: PlaceTechnology::from(presence.kind),
            installed: presence.installed,
            version: presence.version,
            server_running: presence.server_running,
            inside: presence.inside,
            detail: presence.detail,
        }
    }
}

/// One place technology's participation in the registry.
pub trait PlaceTechnologyAdapter {
    /// The open name plans and provider refs use for this technology.
    fn technology(&self) -> PlaceTechnology;

    /// Probe presence and version on this host. Detection must actually
    /// observe — a version probe, a socket check, whatever the technology's
    /// own detect path already does — never assume from the name alone.
    fn detect(&self) -> Result<PlaceTechnologyReading>;

    /// The multiplexer adapter this build drives the technology with, when it
    /// has one. `None` is a declared fact: the name is known and detected,
    /// the driving is not, and consumers say so rather than guessing.
    fn mux_adapter(&self) -> Option<MuxAdapterHandle>;

    /// Whether this technology owns a switchable world that the
    /// working-environment field projects as a provider row. plain declines:
    /// it is the terminal the process already lives in — registered and
    /// resolvable everywhere a plan can name it, but never a field row.
    fn hosts_working_field(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Built-in entries
// ---------------------------------------------------------------------------

/// tmux, over the existing adapter surface.
#[derive(Debug, Clone, Copy, Default)]
pub struct TmuxTechnology;

impl PlaceTechnologyAdapter for TmuxTechnology {
    fn technology(&self) -> PlaceTechnology {
        PlaceTechnology::tmux()
    }

    fn detect(&self) -> Result<PlaceTechnologyReading> {
        // The same `tmux -V` and server probe the mux path has always run.
        Ok(Tmux::system().detect()?.into())
    }

    fn mux_adapter(&self) -> Option<MuxAdapterHandle> {
        Some(Box::new(Tmux::system()))
    }
}

/// cmux, over the existing adapter surface.
#[derive(Debug, Clone, Copy, Default)]
pub struct CmuxTechnology;

impl PlaceTechnologyAdapter for CmuxTechnology {
    fn technology(&self) -> PlaceTechnology {
        PlaceTechnology::cmux()
    }

    fn detect(&self) -> Result<PlaceTechnologyReading> {
        // The same `cmux version` and app probe the mux path has always run.
        Ok(Cmux::system().detect()?.into())
    }

    fn mux_adapter(&self) -> Option<MuxAdapterHandle> {
        Some(Box::new(Cmux::system()))
    }
}

/// The builtin no-mux technology: the terminal this process was invoked in.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainTechnology;

impl PlaceTechnologyAdapter for PlainTechnology {
    fn technology(&self) -> PlaceTechnology {
        PlaceTechnology::plain()
    }

    fn detect(&self) -> Result<PlaceTechnologyReading> {
        Ok(Plain::new().detect()?.into())
    }

    fn mux_adapter(&self) -> Option<MuxAdapterHandle> {
        Some(Box::new(Plain::new()))
    }

    fn hosts_working_field(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

/// The place technologies this build knows, and what it can do for each.
pub struct PlaceTechnologyRegistry {
    entries: Vec<Box<dyn PlaceTechnologyAdapter>>,
}

impl PlaceTechnologyRegistry {
    /// The built-in registry: tmux and cmux over their existing mux adapter
    /// surfaces, and plain as the builtin no-mux technology. A build with
    /// more place technologies composes its own registry from the same trait
    /// — no core release required, which is the point of open names.
    pub fn builtin() -> Self {
        Self {
            entries: vec![
                Box::new(TmuxTechnology),
                Box::new(CmuxTechnology),
                Box::new(PlainTechnology),
            ],
        }
    }

    /// Compose a registry with an additional entry appended (after the
    /// built-ins, so built-in detection order is stable).
    pub fn with_entry(mut self, entry: Box<dyn PlaceTechnologyAdapter>) -> Self {
        self.entries.push(entry);
        self
    }

    /// The registered entries, in registry order.
    pub fn entries(&self) -> &[Box<dyn PlaceTechnologyAdapter>] {
        &self.entries
    }

    /// Resolve a registry entry by its open name.
    ///
    /// `None` means the name is well-formed but this build registers nothing
    /// for it. That is a declared-unsupported outcome for the caller to state
    /// — naming the technology and what would support it — never an error to
    /// launder and never a reason to fall back to another technology.
    pub fn resolve(&self, technology: &PlaceTechnology) -> Option<&dyn PlaceTechnologyAdapter> {
        self.entries
            .iter()
            .map(|entry| entry.as_ref())
            .find(|entry| &entry.technology() == technology)
    }

    /// The built-in multiplexer a name resolves to, when it is one. Closed-set
    /// consumers ask this instead of refusing the name upstream.
    pub fn known_mux(&self, technology: &PlaceTechnology) -> Option<MuxKind> {
        technology.known()
    }

    /// Probe every registered technology.
    pub fn detect_all(&self) -> Result<Vec<PlaceTechnologyReading>> {
        self.entries.iter().map(|entry| entry.detect()).collect()
    }

    /// Probe the technologies whose world the working-environment field
    /// projects. The field's scope is unchanged by the registry: the mux
    /// technologies that own a switchable world, in registry order. plain is
    /// resolvable everywhere a plan can name it, but never a field row.
    pub fn detect_field(&self) -> Result<Vec<PlaceTechnologyReading>> {
        self.entries
            .iter()
            .filter(|entry| entry.hosts_working_field())
            .map(|entry| entry.detect())
            .collect()
    }
}

impl std::fmt::Debug for PlaceTechnologyRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<String> = self
            .entries
            .iter()
            .map(|entry| entry.technology().to_string())
            .collect();
        f.debug_struct("PlaceTechnologyRegistry")
            .field("entries", &names)
            .finish()
    }
}
