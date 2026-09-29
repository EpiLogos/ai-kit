//! Provider-native reconcile for session plans whose place technology is
//! driven through its provider's own interface rather than the mux contract.
//!
//! `Service::session_stack` can only drive the built-in multiplexers, so a
//! plan declaring a provider-native technology (herdr is the current one) used
//! to be refused outright. The split this module implements happens before the
//! stack: when the plan's declared technology resolves through the
//! place-technology registry to an entry with no mux adapter but a working
//! environment, reconcile is executed through the provider's own interface and
//! reported in the provider's own vocabulary:
//!
//! * the provider answered → the outcome *reflects* what it reported;
//! * the provider refused or could not answer → the outcome degrades to a
//!   named `protocol-opacity` state carrying the refusal verbatim — never a
//!   crash and never a mux-shaped answer fabricated around the gap;
//! * the reconcile grade has no provider-native inverse (exact-spec, kill) →
//!   a declared-unavailable state naming what would drive the place instead.
//!
//! A plan that is not provider-native returns `None` and the caller runs the
//! mux path byte-for-byte as before.

use aikit_adapters::place_technology::PlaceTechnologyRegistry;
use aikit_core::resource::ResourceRef;
use aikit_core::session::SessionPlan;
use aikit_core::working_environment::{NativeBindingKind, WorkingEnvironmentHealth};
use aikit_core::Result;
use serde::Serialize;

use crate::working_environment_field::{plan_surfaces, provider_ref};

/// What one provider-native reconcile did, in the provider's own vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderNativeReconcile {
    /// The open place-technology name the plan declared.
    pub technology: String,
    /// The provider ref the reconcile was addressed through.
    pub provider: ResourceRef,
    pub standing: ProviderReconcileStanding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ProviderReconcileStanding {
    /// The provider answered through its own interface; what it reported is
    /// carried as reported.
    Reflected {
        health: WorkingEnvironmentHealth,
        /// The provider-native place id (a workspace, a session) this plan's
        /// recorded evidence names, when the provider reports one.
        #[serde(skip_serializing_if = "Option::is_none")]
        native_place: Option<String>,
        /// How many provider-native bindings the provider reports.
        native_bindings: usize,
    },
    /// The provider would not or could not answer through its own interface.
    /// Its refusal travels verbatim; nothing mux-shaped is invented.
    ProtocolOpacity { reason: String },
    /// This reconcile grade has no provider-native inverse in this build.
    DeclaredUnavailable { reason: String },
}

impl ProviderNativeReconcile {
    /// The human-legible action lines the session reconcile reply carries,
    /// phrased in the provider's vocabulary — never mux verbs.
    pub fn actions(&self) -> Vec<String> {
        match &self.standing {
            ProviderReconcileStanding::Reflected {
                health,
                native_place,
                native_bindings,
            } => vec![format!(
                "{} ensured its place through its own provider interface (health {}, \
                 {} native binding(s){})",
                self.provider,
                health_word(*health),
                native_bindings,
                native_place
                    .as_ref()
                    .map(|id| format!(", place {id}"))
                    .unwrap_or_default(),
            )],
            ProviderReconcileStanding::ProtocolOpacity { .. }
            | ProviderReconcileStanding::DeclaredUnavailable { .. } => Vec::new(),
        }
    }

    /// Warnings the reply should surface beside the actions: a provider that
    /// answered but did not call itself healthy.
    pub fn warnings(&self) -> Vec<String> {
        match &self.standing {
            ProviderReconcileStanding::Reflected { health, .. }
                if !matches!(health, WorkingEnvironmentHealth::Healthy) =>
            {
                vec![format!(
                    "{} answered but reports its place {}",
                    self.provider,
                    health_word(*health)
                )]
            }
            _ => Vec::new(),
        }
    }
}

fn health_word(health: WorkingEnvironmentHealth) -> &'static str {
    match health {
        WorkingEnvironmentHealth::Healthy => "healthy",
        WorkingEnvironmentHealth::Degraded => "degraded",
        WorkingEnvironmentHealth::Unavailable => "unavailable",
    }
}

/// Reconcile one plan through its declared technology's own provider.
///
/// Returns `None` — and the caller must run the mux path exactly as before —
/// when the plan declares no technology, a built-in mux, or a name this build
/// registers nothing (or nothing drivable) for. `Some` carries the provider's
/// own answer in [`ProviderNativeReconcile`].
///
/// The routing consults only the registry's declared adapter seams, never a
/// probe: whether herdr is *reachable* is answered at reconcile time by the
/// provider itself, and its own failure is the honest answer.
pub fn reconcile_provider_native(
    registry: &PlaceTechnologyRegistry,
    plan: &SessionPlan,
    destructive: bool,
) -> Result<Option<ProviderNativeReconcile>> {
    let Some(declared) = &plan.mux else {
        return Ok(None);
    };
    let Some(entry) = registry.resolve(declared) else {
        return Ok(None);
    };
    if entry.mux_adapter().is_some() {
        return Ok(None);
    }
    let provider = provider_ref(declared.clone())?;
    let surfaces = plan_surfaces(plan);
    // Construction is I/O-free for every registered entry: this asks only
    // whether the build can drive the technology without the mux contract.
    let Some(mut environment) = entry.working_environment(plan, &provider, &surfaces, None) else {
        return Ok(None);
    };
    let technology = declared.to_string();
    if destructive {
        return Ok(Some(ProviderNativeReconcile {
            technology,
            provider,
            standing: ProviderReconcileStanding::DeclaredUnavailable {
                reason: format!(
                    "`{declared}` is driven through its own provider interface, which has no \
                     exact-spec or kill inverse in this build; reconcile without --destructive, \
                     or drive the place through the working-surface verbs"
                ),
            },
        }));
    }
    let observation = match environment.open() {
        Ok(observation) => observation,
        Err(error) => {
            let reason = format!(
                "{provider} would not answer its own open: {}",
                error.message()
            );
            return Ok(Some(ProviderNativeReconcile {
                technology,
                provider,
                standing: ProviderReconcileStanding::ProtocolOpacity { reason },
            }));
        }
    };
    Ok(Some(ProviderNativeReconcile {
        technology,
        provider: observation.provider.clone(),
        standing: ProviderReconcileStanding::Reflected {
            health: observation.health,
            native_place: observation
                .bindings
                .iter()
                .find(|binding| binding.kind == NativeBindingKind::Session)
                .map(|binding| binding.native_id.clone()),
            native_bindings: observation.bindings.len(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::place_technology::{
        MuxAdapterHandle, PlaceTechnologyAdapter, PlaceTechnologyReading,
    };
    use aikit_adapters::working_environment::{
        WorkingEnvironmentCapabilities, WorkingEnvironmentObservation, WorkingEnvironmentProvider,
        WORKING_ENVIRONMENT_PROVIDER_VERSION,
    };
    use aikit_core::platform::PlaceTechnology;
    use aikit_core::session::SessionSpec;
    use aikit_core::{AikitError, Result};

    const TECHNOLOGY: &str = "stubherdr";

    fn plan(backend: &str) -> SessionPlan {
        SessionSpec::from_toml_str(&format!(
            "schema = 1\nid = \"p\"\nname = \"p\"\nbackend = \"{backend}\"\n\n[[views]]\nid = \"main\"\n[[views.panes]]\nid = \"shell\"\ncommand = [\"sh\"]\n"
        ))
        .expect("spec parses")
        .compile()
        .expect("spec compiles")
    }

    fn observation(
        provider: ResourceRef,
        health: WorkingEnvironmentHealth,
    ) -> WorkingEnvironmentObservation {
        WorkingEnvironmentObservation {
            schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
            provider,
            provider_version: Some("stub 1".into()),
            health,
            capabilities: WorkingEnvironmentCapabilities::default(),
            bindings: vec![
                aikit_core::working_environment::ProviderNativeBinding {
                    kind: NativeBindingKind::Session,
                    native_id: "ws-1".into(),
                    canonical_ref: None,
                    provenance: vec!["stub workspace".into()],
                },
                aikit_core::working_environment::ProviderNativeBinding {
                    kind: NativeBindingKind::Surface,
                    native_id: "pane-1".into(),
                    canonical_ref: None,
                    provenance: vec!["stub pane".into()],
                },
            ],
            focused_native_id: None,
            provenance: Vec::new(),
        }
    }

    /// The test double for a provider-native technology: no mux adapter, a
    /// plan-scoped environment whose answers are canned. Nothing here spawns
    /// a process or touches a socket.
    struct FakeNativeTechnology {
        technology: PlaceTechnology,
        drivable: bool,
        open_error: Option<AikitError>,
        health: WorkingEnvironmentHealth,
    }

    impl FakeNativeTechnology {
        fn drivable() -> Self {
            Self {
                technology: PlaceTechnology::new(TECHNOLOGY),
                drivable: true,
                open_error: None,
                health: WorkingEnvironmentHealth::Healthy,
            }
        }
    }

    impl PlaceTechnologyAdapter for FakeNativeTechnology {
        fn technology(&self) -> PlaceTechnology {
            self.technology.clone()
        }

        fn detect(&self) -> Result<PlaceTechnologyReading> {
            panic!("reconcile must not probe presence; the provider's own answer is the truth")
        }

        fn mux_adapter(&self) -> Option<MuxAdapterHandle> {
            None
        }

        fn working_environment(
            &self,
            _plan: &SessionPlan,
            provider: &ResourceRef,
            _surfaces: &[(ResourceRef, String)],
            _subject: Option<&ResourceRef>,
        ) -> Option<Box<dyn WorkingEnvironmentProvider>> {
            if !self.drivable {
                return None;
            }
            Some(Box::new(FakeEnvironment {
                provider: provider.clone(),
                observation: observation(provider.clone(), self.health),
                open_error: self.open_error.clone(),
            }))
        }
    }

    struct FakeEnvironment {
        provider: ResourceRef,
        observation: WorkingEnvironmentObservation,
        open_error: Option<AikitError>,
    }

    impl WorkingEnvironmentProvider for FakeEnvironment {
        fn provider_ref(&self) -> &ResourceRef {
            &self.provider
        }

        fn capabilities(&self) -> WorkingEnvironmentCapabilities {
            WorkingEnvironmentCapabilities::default()
        }

        fn observe(&mut self) -> Result<WorkingEnvironmentObservation> {
            Ok(self.observation.clone())
        }

        fn open(&mut self) -> Result<WorkingEnvironmentObservation> {
            match &self.open_error {
                Some(error) => Err(error.clone()),
                None => Ok(self.observation.clone()),
            }
        }

        fn focus_surface(&mut self, _surface: &ResourceRef) -> Result<()> {
            Ok(())
        }

        fn detach_surface(&mut self, _surface: &ResourceRef) -> Result<()> {
            Ok(())
        }
    }

    fn registry(entry: FakeNativeTechnology) -> PlaceTechnologyRegistry {
        PlaceTechnologyRegistry::from_entries(vec![Box::new(entry)])
    }

    #[test]
    fn reflected_carries_the_provider_answer_for_a_provider_native_technology() {
        let outcome = reconcile_provider_native(
            &registry(FakeNativeTechnology::drivable()),
            &plan(TECHNOLOGY),
            false,
        )
        .expect("reconcile runs")
        .expect("a provider-native plan routes to the provider");
        assert_eq!(outcome.technology, TECHNOLOGY);
        assert_eq!(
            outcome.standing,
            ProviderReconcileStanding::Reflected {
                health: WorkingEnvironmentHealth::Healthy,
                native_place: Some("ws-1".into()),
                native_bindings: 2,
            }
        );
        let actions = outcome.actions();
        assert!(actions[0].contains("through its own provider interface"));
        assert!(actions[0].contains("place ws-1"));
        assert!(outcome.warnings().is_empty());
    }

    #[test]
    fn a_degraded_provider_answer_still_reflects_and_warns() {
        let entry = FakeNativeTechnology {
            health: WorkingEnvironmentHealth::Degraded,
            ..FakeNativeTechnology::drivable()
        };
        let outcome = reconcile_provider_native(&registry(entry), &plan(TECHNOLOGY), false)
            .expect("reconcile runs")
            .expect("a provider-native plan routes to the provider");
        assert_eq!(
            outcome.standing,
            ProviderReconcileStanding::Reflected {
                health: WorkingEnvironmentHealth::Degraded,
                native_place: Some("ws-1".into()),
                native_bindings: 2,
            }
        );
        assert!(!outcome.actions().is_empty());
        assert!(outcome.warnings()[0].contains("degraded"));
    }

    #[test]
    fn a_provider_refusal_degrades_to_named_protocol_opacity_without_a_mux_answer() {
        let entry = FakeNativeTechnology {
            open_error: Some(AikitError::new(
                "herdr.command_failed",
                "herdr refused: server is shutting down",
            )),
            ..FakeNativeTechnology::drivable()
        };
        let outcome = reconcile_provider_native(&registry(entry), &plan(TECHNOLOGY), false)
            .expect("reconcile runs")
            .expect("a provider-native plan routes to the provider");
        assert_eq!(
            outcome.standing,
            ProviderReconcileStanding::ProtocolOpacity {
                reason: format!(
                    "{provider} would not answer its own open: herdr refused: server is shutting down",
                    provider = outcome.provider,
                ),
            }
        );
        assert!(
            outcome.actions().is_empty(),
            "no mux-shaped actions are fabricated"
        );
        assert!(outcome.warnings().is_empty());
    }

    #[test]
    fn destructive_reconcile_is_declared_unavailable_without_calling_the_provider() {
        let outcome = reconcile_provider_native(
            &registry(FakeNativeTechnology::drivable()),
            &plan(TECHNOLOGY),
            true,
        )
        .expect("reconcile runs")
        .expect("a provider-native plan routes to the provider");
        let ProviderReconcileStanding::DeclaredUnavailable { reason } = outcome.standing else {
            panic!(
                "destructive reconcile must be declared unavailable, got {:?}",
                outcome.standing
            );
        };
        assert!(reason.contains(TECHNOLOGY));
        assert!(reason.contains("working-surface"));
    }

    #[test]
    fn an_undrivable_registered_entry_stays_on_the_mux_path() {
        let entry = FakeNativeTechnology {
            drivable: false,
            ..FakeNativeTechnology::drivable()
        };
        assert!(
            reconcile_provider_native(&registry(entry), &plan(TECHNOLOGY), false)
                .expect("reconcile runs")
                .is_none(),
            "no provider-native route means the mux path answers, unchanged"
        );
    }

    #[test]
    fn plans_outside_the_provider_native_route_stay_on_the_mux_path() {
        let composed = registry(FakeNativeTechnology::drivable());

        // No declared technology at all.
        let mut undeclared = plan(TECHNOLOGY);
        undeclared.mux = None;
        assert!(reconcile_provider_native(&composed, &undeclared, false)
            .expect("reconcile runs")
            .is_none());

        // A name the registry does not register: the mux path's own
        // declared-unsupported refusal stands, byte-for-byte as before.
        let unknown = plan("gibberish-tech");
        assert!(reconcile_provider_native(&composed, &unknown, false)
            .expect("reconcile runs")
            .is_none());

        // An empty registry registers nothing, so even a declared technology
        // stays on the mux path.
        let declared = plan(TECHNOLOGY);
        let empty = PlaceTechnologyRegistry::from_entries(vec![]);
        assert!(reconcile_provider_native(&empty, &declared, false)
            .expect("reconcile runs")
            .is_none());

        // A built-in mux is a mux-adapter entry, so the mux path answers
        // unchanged (this probe is construction only: no tmux is spawned).
        assert!(reconcile_provider_native(
            &PlaceTechnologyRegistry::builtin(),
            &plan("tmux"),
            false
        )
        .expect("reconcile runs")
        .is_none());
    }

    /// Herdr itself routes to the provider path through the real builtin
    /// registry. The destructive grade is used so the run performs no herdr
    /// I/O at all — routing is a registry question, answered by construction.
    #[test]
    fn builtin_registry_routes_herdr_to_the_provider_path_without_probing() {
        let outcome =
            reconcile_provider_native(&PlaceTechnologyRegistry::builtin(), &plan("herdr"), true)
                .expect("reconcile runs")
                .expect("herdr is a provider-native technology in this build");
        assert_eq!(outcome.technology, "herdr");
        assert_eq!(outcome.provider.as_str(), "provider/herdr/current");
        assert!(matches!(
            outcome.standing,
            ProviderReconcileStanding::DeclaredUnavailable { .. }
        ));
    }
}
