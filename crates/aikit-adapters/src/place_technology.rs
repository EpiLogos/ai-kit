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

use aikit_core::platform::{MuxKind, PlaceTechnology};
use aikit_core::resource::ResourceRef;
use aikit_core::session::SessionPlan;
use aikit_core::Result;

use crate::herdr::HerdrWorkingEnvironment;
use crate::mux::{cmux::Cmux, plain::Plain, tmux::Tmux, MuxAdapter, MuxPresence};
use crate::runner::{CommandRunner, SystemRunner};
use crate::working_environment::WorkingEnvironmentProvider;

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

    /// The working-environment provider this build drives the technology with
    /// over one plan, when it projects plans without going through the mux
    /// contract.
    ///
    /// `provider` is the exact provider ref the caller addresses — the
    /// binding's ref, not a technology-canonical one — so the returned
    /// environment is the one that ref names. `surfaces` is the
    /// caller-owned canonical Surface -> logical plan key list — canonical
    /// identity is minted by the caller, never by the adapter — and
    /// `subject`, when given, is the canonical Surface a following open
    /// would address. `None` is the same declared fact as
    /// [`Self::mux_adapter`]'s: the name may be known and detected, and
    /// consumers state so rather than guessing.
    fn working_environment(
        &self,
        plan: &SessionPlan,
        provider: &ResourceRef,
        surfaces: &[(ResourceRef, String)],
        subject: Option<&ResourceRef>,
    ) -> Option<Box<dyn WorkingEnvironmentProvider>> {
        let _ = (plan, provider, surfaces, subject);
        None
    }

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

/// herdr, the terminal workspace manager, consumed through its public CLI.
///
/// Herdr is deliberately not a `MuxKind`: its workspace/tab/pane/agent world
/// is richer than the mux contract, so it is driven through the
/// working-environment provider contract instead. `mux_adapter` is therefore
/// `None` — a declared fact, not a gap — while `working_environment` carries
/// the plan route: create-or-attach against the Herdr workspace the
/// plan names, under the recorded provider-native evidence as attach
/// identity.
#[derive(Debug, Clone, Copy, Default)]
pub struct HerdrTechnology;

impl PlaceTechnologyAdapter for HerdrTechnology {
    fn technology(&self) -> PlaceTechnology {
        PlaceTechnology::herdr()
    }

    fn detect(&self) -> Result<PlaceTechnologyReading> {
        // `herdr --version` is the real presence probe. A binary that cannot
        // be spawned is absent; a binary that spawns but will not answer is
        // reported absent with the failure named, never silently assumed.
        let version = match SystemRunner::new().run(&["herdr".into(), "--version".into()]) {
            Ok(output) if output.ok() => output.line().trim().to_string(),
            Ok(_) => {
                return Ok(PlaceTechnologyReading::absent(
                    PlaceTechnology::herdr(),
                    "`herdr --version` exited with a failure, so herdr's presence cannot be proved",
                ));
            }
            Err(error) if error.code() == "mux.command_spawn_failed" => {
                return Ok(PlaceTechnologyReading::absent(
                    PlaceTechnology::herdr(),
                    "herdr is not installed on this host",
                ));
            }
            Err(error) => return Err(error),
        };
        // An installed herdr whose server is not answering is still installed;
        // the status probe separates the two states instead of folding them.
        let server_running = system_reports_herdr_server_running();
        Ok(PlaceTechnologyReading {
            technology: PlaceTechnology::herdr(),
            installed: true,
            version: Some(version),
            server_running,
            inside: false,
            detail: None,
        })
    }

    fn mux_adapter(&self) -> Option<MuxAdapterHandle> {
        None
    }

    fn working_environment(
        &self,
        plan: &SessionPlan,
        provider: &ResourceRef,
        surfaces: &[(ResourceRef, String)],
        subject: Option<&ResourceRef>,
    ) -> Option<Box<dyn WorkingEnvironmentProvider>> {
        Some(Box::new(HerdrWorkingEnvironment::for_plan(
            SystemRunner::new(),
            plan,
            provider.clone(),
            surfaces,
            subject,
        )))
    }
}

/// Whether a local `herdr status server` reports the server running.
///
/// The probe reads exactly the `status: running` line; any other output,
/// exit or parse shape is reported as not running rather than interpreted.
fn system_reports_herdr_server_running() -> bool {
    matches!(
        SystemRunner::new().run(&["herdr".into(), "status".into(), "server".into()]),
        Ok(output) if output.ok() && output.line().trim() == "status: running"
    )
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
    /// surfaces, herdr over its rich public-CLI provider, and plain as the
    /// builtin no-mux technology. A build with more place technologies
    /// composes its own registry from the same trait — no core release
    /// required, which is the point of open names.
    pub fn builtin() -> Self {
        Self::from_entries(vec![
            Box::new(TmuxTechnology),
            Box::new(CmuxTechnology),
            Box::new(HerdrTechnology),
            Box::new(PlainTechnology),
        ])
    }

    /// Compose a registry from exactly these entries, in this order.
    ///
    /// The explicit composition seam for a build (or a test scope) that must
    /// not carry the built-ins: the same trait, the same [`Self::resolve`]
    /// answers, none of the built-in detection. A consumer whose validation
    /// and dispatch take a `&PlaceTechnologyRegistry` parameter is thereby
    /// testable without any live technology on the host.
    pub fn from_entries(entries: Vec<Box<dyn PlaceTechnologyAdapter>>) -> Self {
        Self { entries }
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

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::session::SessionSpec;

    fn plan(backend: &str) -> SessionPlan {
        SessionSpec::from_toml_str(&format!(
            "schema = 1\nid = \"p\"\nname = \"p\"\nbackend = \"{backend}\"\n\n[[views]]\nid = \"main\"\n[[views.panes]]\nid = \"shell\"\ncommand = [\"sh\"]\n"
        ))
        .expect("spec parses")
        .compile()
        .expect("spec compiles")
    }

    /// Herdr must be reachable as a provider-native entry — registered,
    /// without a mux adapter, with a plan-scoped working environment — through
    /// construction alone. Selecting or routing herdr never needs a probe, so
    /// a consumer can validate a binding against it without spawning herdr.
    #[test]
    fn herdr_is_a_registered_provider_native_entry_without_probing() {
        let registry = PlaceTechnologyRegistry::builtin();
        let entry = registry
            .resolve(&PlaceTechnology::herdr())
            .expect("herdr is registered in the builtin registry");
        assert!(
            entry.mux_adapter().is_none(),
            "herdr is deliberately not a mux adapter"
        );
        let provider = ResourceRef::parse("provider/herdr/current").expect("provider ref parses");
        assert!(
            entry
                .working_environment(&plan("herdr"), &provider, &[], None)
                .is_some(),
            "herdr hands back a plan-scoped working environment"
        );
    }

    /// The composition seam must be able to drop every built-in, so a test
    /// scope (or a custom build) answers from its own entries alone.
    #[test]
    fn from_entries_composes_a_registry_without_the_builtins() {
        struct UnknownTechnology;
        impl PlaceTechnologyAdapter for UnknownTechnology {
            fn technology(&self) -> PlaceTechnology {
                PlaceTechnology::new("stubplace")
            }
            fn detect(&self) -> Result<PlaceTechnologyReading> {
                Ok(PlaceTechnologyReading::absent(
                    PlaceTechnology::new("stubplace"),
                    "stub",
                ))
            }
            fn mux_adapter(&self) -> Option<MuxAdapterHandle> {
                None
            }
        }
        let registry = PlaceTechnologyRegistry::from_entries(vec![Box::new(UnknownTechnology)]);
        assert!(registry.resolve(&PlaceTechnology::herdr()).is_none());
        assert!(registry
            .resolve(&PlaceTechnology::new("stubplace"))
            .is_some());
    }
}
