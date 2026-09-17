//! Observe this host's terminal working environments and route open/focus to
//! them.
//!
//! This is the caller `aikit-tui`'s `PaletteBackend::working_environments` seam
//! expects. The TUI cannot reach a provider — it does not depend on
//! `aikit-adapters` — so the observing happens here, where both crates are in
//! scope, and the result is handed over as plain data.
//!
//! Every installed working-environment technology — the muxes and, through
//! its own rich provider, herdr — is observed over *the same* `SessionPlan`
//! with *the same* canonical Surface bindings. That is what makes W6's
//! acceptance meaningful: `surface/terminal/main/shell` is one canonical
//! subject, and tmux, cmux and herdr are projections of it, rather than
//! subjects that happen to look alike. Which native pane each provider hands
//! back is that provider's own business and stays provenance.

use aikit_adapters::herdr::created_place_bindings;
use aikit_adapters::mux::{MuxAdapter, tmux::Tmux};
use aikit_adapters::place_technology::{PlaceTechnologyReading, PlaceTechnologyRegistry};
use aikit_adapters::{MuxWorkingEnvironment, WorkingEnvironmentProvider};
use aikit_core::Result;
use aikit_core::platform::PlaceTechnology;
use aikit_core::resource::ResourceRef;
use aikit_core::session::SessionPlan;
use aikit_core::working_environment::{
    WORKING_ENVIRONMENT_PROVIDER_VERSION, WorkingEnvironmentCapabilities, WorkingEnvironmentHealth,
    WorkingEnvironmentObservation,
};
use aikit_core::Result;
use aikit_tui::live_field::{WorkingEnvironmentOperation, WorkingEnvironmentOutcome};
use serde::Serialize;

/// Provider-owned terminal client attachment material. It is intentionally
/// narrower than `open`: attachment never creates or reconciles a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum WorkingEnvironmentTerminalAttachment {
    Attach {
        provider: ResourceRef,
        subject: ResourceRef,
        native_id: String,
        argv: Vec<String>,
    },
    NotExposed {
        provider: ResourceRef,
        subject: ResourceRef,
        reason: String,
    },
}

/// The canonical Surface Ref for one logical pane of a plan.
///
/// Deliberately derived from the plan's own view/pane ids and nothing else: the
/// same plan yields the same canonical Refs in every provider, on every host,
/// on every run. A native pane id never enters this.
pub fn surface_ref(view: &str, pane: &str) -> Result<ResourceRef> {
    ResourceRef::parse(format!("surface/terminal/{view}/{pane}"))
}

/// The canonical provider Ref for a place technology.
pub fn provider_ref(technology: PlaceTechnology) -> Result<ResourceRef> {
    ResourceRef::parse(format!("provider/{}/current", technology.as_str()))
}

/// Every (canonical Surface, logical plan key) pair in a plan, in plan order.
///
/// A session spec may name a view or pane anything at all, and not every name
/// forms a valid canonical Ref. Such a pane is left out of the field rather
/// than failing the whole reading: it is one pane the working environment
/// cannot address, not a reason to take the Worlds pane down. It remains in
/// the plan and the mux still creates it — this function is about what AIKit
/// can *name*, not what tmux can run.
pub fn plan_surfaces(plan: &SessionPlan) -> Vec<(ResourceRef, String)> {
    let mut bound = Vec::new();
    for view in &plan.views {
        for step in &view.steps {
            if let Ok(surface) = surface_ref(&view.id, &step.pane) {
                bound.push((surface, format!("{}/{}", view.id, step.pane)));
            }
        }
    }
    bound
}

/// Which terminal working environments this host actually has, with the reason
/// attached when one is present but unusable.
///
/// The readings come from the place-technology registry: each registered
/// technology that hosts a working field is probed for real. plain is
/// registered and resolvable but never a field row — it is the terminal this
/// process already lives in, not a switchable world.
pub fn detect() -> Result<Vec<PlaceTechnologyReading>> {
    PlaceTechnologyRegistry::builtin().detect_field()
}

fn environment<A: MuxAdapter>(
    adapter: A,
    plan: &SessionPlan,
    provider: ResourceRef,
    surfaces: &[(ResourceRef, String)],
) -> MuxWorkingEnvironment<A> {
    let mut environment = MuxWorkingEnvironment::new(adapter, plan.clone(), provider);
    for (surface, logical) in surfaces {
        environment = environment.bind_surface(surface.clone(), logical.clone());
    }
    environment
}

/// Observe every installed working-environment technology over one plan.
///
/// A technology that is not installed is left out entirely rather than reported
/// as unavailable: "not on this machine" is not a degraded provider, and listing
/// it would put a row in the operator's field that can never become useful. A
/// technology that *is* installed but whose observation fails is reported with
/// the failure as provenance, because that one is worth knowing about.
pub fn observe(plan: &SessionPlan) -> Result<Vec<WorkingEnvironmentObservation>> {
    let surfaces = plan_surfaces(plan);
    let registry = PlaceTechnologyRegistry::builtin();
    let mut observations = Vec::new();

    for reading in registry.detect_field()? {
        if !reading.installed {
            continue;
        }
        let provider = provider_ref(reading.technology.clone())?;
        // The registry, not a closed match, decides who projects this plan.
        // An installed technology with no adapter is skipped rather than
        // guessed at: a silent guess would put a row in the operator's field
        // that no provider stands behind.
        let Some(entry) = registry.resolve(&reading.technology) else {
            continue;
        };
        let mut observed_environment: Box<dyn WorkingEnvironmentProvider> = if let Some(adapter) =
            entry.mux_adapter()
        {
            Box::new(environment(adapter, plan, provider.clone(), &surfaces))
        } else {
            // A technology driven without the mux contract hands back its own
            // plan-scoped provider through the same registry entry.
            let Some(registered) = entry.working_environment(plan, &provider, &surfaces, None) else {
                continue;
            };
            registered
        };
        let observed = observed_environment.observe();
        match observed {
            Ok(mut observation) => {
                if let Some(version) = reading.version.clone() {
                    observation.provider_version = Some(version);
                }
                observation
                    .provenance
                    .push(format!("observed over session plan {}", plan.name));
                observations.push(observation);
            }
            // An installed technology that will not answer is a real, common
            // state — a cmux app that is not running, a tmux server that died.
            // It is an unavailable provider, not a failure of this reading, and
            // certainly not a reason to refuse the whole field: doing that
            // would take the TUI's whole Worlds pane down with one dead
            // socket. Report it as the provider vocabulary already can, with
            // the reason attached so the operator can act on it.
            Err(error) => observations.push(WorkingEnvironmentObservation {
                schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
                provider,
                provider_version: reading.version.clone(),
                health: WorkingEnvironmentHealth::Unavailable,
                capabilities: WorkingEnvironmentCapabilities::default(),
                bindings: Vec::new(),
                focused_native_id: None,
                provenance: vec![
                    format!("installed but not observable: {}", error.message()),
                    format!("observation attempted over session plan {}", plan.name),
                ],
            }),
        }
    }

    Ok(observations)
}

/// Ask one provider to open or focus one canonical Surface.
///
/// `Open` reconciles the whole session (create-or-attach) because that is what
/// a mux can actually do — it has no "create just this pane in a session that
/// does not exist" primitive. `Focus` targets the single bound Surface.
///
/// The provider's place technology is resolved through the registry. A
/// technology this build cannot drive is a first-class declared-unsupported
/// outcome (`NotExposed`) naming the technology and what would support it —
/// never a crash and never a fallback onto another technology.
pub fn act(
    plan: &SessionPlan,
    provider: &ResourceRef,
    subject: &ResourceRef,
    operation: WorkingEnvironmentOperation,
) -> Result<WorkingEnvironmentOutcome> {
    let surfaces = plan_surfaces(plan);
    let registry = PlaceTechnologyRegistry::builtin();
    // The addressed provider may be a technology-canonical ref
    // (`provider/herdr/current`) or an instance ref a commissioned place
    // carries (`provider/herdr/w6`): both name herdr, and both are answered
    // by herdr's own entry — never laundered into an unknown.
    let Some(reading) = registry
        .detect_field()?
        .into_iter()
        .find(|reading| {
            provider_ref(reading.technology.clone()).ok().as_ref() == Some(provider)
                || technology_from_provider(provider).as_ref() == Some(&reading.technology)
        })
    else {
        return Ok(WorkingEnvironmentOutcome::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: declared_unsupported_reason(&registry, provider),
        });
    };
    if !reading.installed {
        return Ok(WorkingEnvironmentOutcome::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: reading
                .detail
                .unwrap_or_else(|| format!("{} is not installed on this host", reading.technology)),
        });
    }
    let Some(entry) = registry.resolve(&reading.technology) else {
        return Ok(WorkingEnvironmentOutcome::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: format!("{provider} has no working-environment projection in this build"),
        });
    };
    if let Some(adapter) = entry.mux_adapter() {
        return act_in(
            environment(adapter, plan, provider.clone(), &surfaces),
            provider,
            subject,
            operation,
        );
    }
    // The technology is driven without the mux contract: the registry hands
    // back its own plan-scoped provider, addressed through the same public
    // outcome vocabulary.
    let Some(mut registered) = entry.working_environment(plan, provider, &surfaces, Some(subject)) else {
        return Ok(WorkingEnvironmentOutcome::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: format!("{provider} has no working-environment projection in this build"),
        });
    };
    act_via_registered(
        registered.as_mut(),
        plan,
        provider,
        subject,
        operation,
    )
}

/// Why a provider ref cannot be acted on, stated so the operator can act on it.
///
/// A well-formed technology name is never laundered into "unknown": the
/// registry says whether the technology is registered at all, hosts a field,
/// and has a driver, and the reason names what would change the answer.
fn declared_unsupported_reason(
    registry: &PlaceTechnologyRegistry,
    provider: &ResourceRef,
) -> String {
    match technology_from_provider(provider) {
        Some(technology) => match registry.resolve(&technology) {
            Some(entry) if !entry.hosts_working_field() => format!(
                "{provider}: `{technology}` is the terminal this command already runs in and \
                 hosts no switchable world to open or focus"
            ),
            Some(_) => format!(
                "{provider}: `{technology}` is registered but this build cannot drive it as a \
                 working environment"
            ),
            None => format!(
                "{provider}: the place technology `{technology}` is declared, but this build \
                 registers no working-environment provider for it; a `{technology}` adapter in \
                 the place-technology registry would support it"
            ),
        },
        None => format!("{provider} is not a working environment this build projects"),
    }
}

/// The place technology a provider ref names, when it names one.
///
/// Both ref shapes resolve: the technology-canonical `provider/herdr/current`
/// and the instance ref a commissioned place actually carries —
/// `provider/herdr/w6`, found live when a workcell-commissioned herdr room
/// stayed unprojectable because only `/current` refs were read. The
/// technology is the ref's first segment; the rest is the instance's own
/// business and never re-parsed here.
fn technology_from_provider(provider: &ResourceRef) -> Option<PlaceTechnology> {
    let raw = provider.as_str().strip_prefix("provider/")?;
    let (technology, _instance) = raw.split_once('/')?;
    technology.parse::<PlaceTechnology>().ok()
}

/// Resolve a terminal-client attachment command for one already-live Surface.
///
/// The caller never names tmux argv. This owner operation inspects the exact
/// persisted plan binding first and refuses to recreate absent work.
pub fn terminal_attachment(
    plan: &SessionPlan,
    provider: &ResourceRef,
    subject: &ResourceRef,
) -> Result<WorkingEnvironmentTerminalAttachment> {
    let surfaces = plan_surfaces(plan);
    let Some((_, logical)) = surfaces.iter().find(|(surface, _)| surface == subject) else {
        return Ok(WorkingEnvironmentTerminalAttachment::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: format!(
                "{subject} is not a canonical Surface in persisted plan {}",
                plan.id
            ),
        });
    };
    let tmux_provider = provider_ref(PlaceTechnology::tmux())?;
    let herdr_provider = provider_ref(PlaceTechnology::herdr())?;
    if provider == &herdr_provider {
        return Ok(WorkingEnvironmentTerminalAttachment::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: "the herdr provider publishes open and workspace-focus operations, not a \
                     terminal-client attach command; a tmux Surface is the attachable route"
                .into(),
        });
    }
    if provider != &tmux_provider {
        return Ok(WorkingEnvironmentTerminalAttachment::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: format!("{provider} does not publish a terminal-client attachment operation"),
        });
    }
    let tmux = Tmux::system();
    let presence = tmux.detect()?;
    if !presence.installed {
        return Ok(WorkingEnvironmentTerminalAttachment::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: presence
                .detail
                .unwrap_or_else(|| "tmux is not installed on this host".into()),
        });
    }
    match tmux.attach_surface_command(plan, logical) {
        Ok((native_id, argv)) => Ok(WorkingEnvironmentTerminalAttachment::Attach {
            provider: provider.clone(),
            subject: subject.clone(),
            native_id,
            argv,
        }),
        Err(error) if error.code() == "mux.tmux_surface_not_live" => {
            Ok(WorkingEnvironmentTerminalAttachment::NotExposed {
                provider: provider.clone(),
                subject: subject.clone(),
                reason: error.message().to_string(),
            })
        }
        Err(error) => Err(error),
    }
}

fn act_in<A: MuxAdapter>(
    mut environment: MuxWorkingEnvironment<A>,
    provider: &ResourceRef,
    subject: &ResourceRef,
    operation: WorkingEnvironmentOperation,
) -> Result<WorkingEnvironmentOutcome> {
    match operation {
        WorkingEnvironmentOperation::Open => {
            let observation = environment.open()?;
            let Some(native_id) = observation.canonical_native_id(subject) else {
                // The session opened, but this Surface is not in it. Saying so
                // beats reporting a success the operator cannot see.
                return Ok(WorkingEnvironmentOutcome::NotExposed {
                    provider: provider.clone(),
                    subject: subject.clone(),
                    reason: format!(
                        "{provider} opened its session but has no live pane bound to {subject}"
                    ),
                });
            };
            Ok(WorkingEnvironmentOutcome::Opened {
                provider: provider.clone(),
                subject: subject.clone(),
                native_id: native_id.to_string(),
                created: None,
            })
        }
        WorkingEnvironmentOperation::Focus => {
            environment.focus_surface(subject)?;
            let observation = environment.observe()?;
            let native_id = observation
                .canonical_native_id(subject)
                .unwrap_or("unreported")
                .to_string();
            Ok(WorkingEnvironmentOutcome::Focused {
                provider: provider.clone(),
                subject: subject.clone(),
                native_id,
            })
        }
    }
}

/// Act through a registry-resolved provider that is driven without the mux
/// contract (herdr is the current one).
///
/// The provider owns create-or-attach over the plan's recorded evidence; the
/// outcome reports its fresh native fact. When an open created
/// provider-native material, the created bindings travel with the outcome so
/// the caller can persist them; nothing here writes state on its own. A
/// provider refusal that names a withheld operation (a focus the provider
/// cannot address directly) comes back typed as `NotExposed`, not as an
/// error the operator cannot act on.
fn act_via_registered(
    environment: &mut dyn WorkingEnvironmentProvider,
    plan: &SessionPlan,
    provider: &ResourceRef,
    subject: &ResourceRef,
    operation: WorkingEnvironmentOperation,
) -> Result<WorkingEnvironmentOutcome> {
    match operation {
        WorkingEnvironmentOperation::Open => {
            let observation = environment.open()?;
            let Some(native_id) = observation.canonical_native_id(subject) else {
                return Ok(WorkingEnvironmentOutcome::NotExposed {
                    provider: provider.clone(),
                    subject: subject.clone(),
                    reason: format!(
                        "{provider} ensured its place but has no live pane bound to {subject}; \
                         pane ids enter the plan only through an explicit open that creates them"
                    ),
                });
            };
            Ok(WorkingEnvironmentOutcome::Opened {
                provider: provider.clone(),
                subject: subject.clone(),
                native_id: native_id.to_string(),
                created: created_place_bindings(plan, &observation),
            })
        }
        WorkingEnvironmentOperation::Focus => match environment.focus_surface(subject) {
            Ok(()) => {
                let observation = environment.observe()?;
                let native_id = observation
                    .canonical_native_id(subject)
                    .unwrap_or("unreported")
                    .to_string();
                Ok(WorkingEnvironmentOutcome::Focused {
                    provider: provider.clone(),
                    subject: subject.clone(),
                    native_id,
                })
            }
            Err(error) if error.code() == "herdr.surface_focus_unsupported" => {
                Ok(WorkingEnvironmentOutcome::NotExposed {
                    provider: provider.clone(),
                    subject: subject.clone(),
                    reason: error.message().to_string(),
                })
            }
            Err(error) => Err(error),
        },
    }
}
