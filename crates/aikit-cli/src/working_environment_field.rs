//! Observe this host's terminal working environments and route open/focus to
//! them.
//!
//! This is the caller `aikit-tui`'s `PaletteBackend::working_environments` seam
//! expects. The TUI cannot reach a provider — it does not depend on
//! `aikit-adapters` — so the observing happens here, where both crates are in
//! scope, and the result is handed over as plain data.
//!
//! Every installed mux is observed over *the same* `SessionPlan` with *the
//! same* canonical Surface bindings. That is what makes W6's acceptance
//! meaningful: `surface/terminal/main/shell` is one canonical subject, and tmux
//! and cmux are two projections of it, rather than two subjects that happen to
//! look alike. Which native pane each provider hands back is that provider's
//! own business and stays provenance.

use aikit_adapters::herdr::HerdrWorkingEnvironment;
use aikit_adapters::mux::{cmux::Cmux, tmux::Tmux, MuxAdapter, MuxPresence};
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_adapters::{MuxWorkingEnvironment, WorkingEnvironmentProvider};
use aikit_core::platform::MuxKind;
use aikit_core::resource::ResourceRef;
use aikit_core::session::SessionPlan;
use aikit_core::working_environment::{
    WorkingEnvironmentCapabilities, WorkingEnvironmentHealth, WorkingEnvironmentObservation,
    WORKING_ENVIRONMENT_PROVIDER_VERSION,
};
use aikit_core::{AikitError, Result};
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

/// The canonical provider Ref for a mux kind.
pub fn provider_ref(kind: MuxKind) -> Result<ResourceRef> {
    ResourceRef::parse(format!("provider/{}/current", kind.as_str()))
}

/// The canonical provider Ref for the Herdr rich working environment.
///
/// Herdr is deliberately not a `MuxKind`: it is the first rich Omarchy
/// reference (workspaces, agents, projects), so it keeps its own adapter and
/// provider Ref while answering over the same canonical Surfaces.
pub fn herdr_provider_ref() -> Result<ResourceRef> {
    ResourceRef::parse("provider/herdr/current")
}

/// The Herdr CLI version, when installed on this host.
fn herdr_version() -> Result<Option<String>> {
    match SystemRunner::new().run(&["herdr".into(), "--version".into()]) {
        Ok(output) if output.ok() => Ok(Some(output.line().trim().to_string())),
        Ok(_) => Ok(None),
        Err(error) if error.code() == "mux.command_spawn_failed" => Ok(None),
        Err(error) => Err(error),
    }
}

/// The Herdr workspace id recorded in a plan's `backend_extensions.herdr`
/// table, if the plan carries persisted provider-native evidence.
pub fn herdr_recorded_workspace(plan: &SessionPlan) -> Option<String> {
    plan.backend_extensions
        .get("herdr")?
        .get("workspace-id")
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
}

/// The recorded Herdr pane bindings for this plan's canonical Surfaces, in
/// plan order.
///
/// Only surfaces whose logical plan key has an explicitly recorded pane id are
/// returned. A pane id is never guessed from a name: provider-native ids are
/// minted by Herdr and enter the plan only through an explicit open whose
/// created evidence the caller persisted.
pub fn herdr_recorded_surfaces(plan: &SessionPlan) -> Vec<(ResourceRef, String)> {
    let Some(herdr) = plan.backend_extensions.get("herdr") else {
        return Vec::new();
    };
    let Some(surfaces) = herdr.get("surfaces").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    plan_surfaces(plan)
        .into_iter()
        .filter_map(|(surface, logical)| {
            surfaces
                .get(&logical)
                .and_then(toml::Value::as_str)
                .map(|pane| (surface, pane.to_owned()))
        })
        .collect()
}

/// A Herdr environment over one plan, bound to exactly the recorded evidence.
fn herdr_environment(plan: &SessionPlan) -> Result<HerdrWorkingEnvironment<SystemRunner>> {
    let mut environment = HerdrWorkingEnvironment::new(SystemRunner::new(), herdr_provider_ref()?);
    if let Some(workspace) = herdr_recorded_workspace(plan) {
        environment = environment.with_workspace(workspace);
    }
    for (surface, pane) in herdr_recorded_surfaces(plan) {
        environment = environment.bind_surface(surface, pane);
    }
    Ok(environment)
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
pub fn detect() -> Result<Vec<MuxPresence>> {
    Ok(vec![Tmux::system().detect()?, Cmux::system().detect()?])
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

/// Observe every installed mux over one plan.
///
/// A mux that is not installed is left out entirely rather than reported as
/// unavailable: "not on this machine" is not a degraded provider, and listing
/// it would put a row in the operator's field that can never become useful. A
/// mux that *is* installed but whose observation fails is reported with the
/// failure as provenance, because that one is worth knowing about.
pub fn observe(plan: &SessionPlan) -> Result<Vec<WorkingEnvironmentObservation>> {
    let surfaces = plan_surfaces(plan);
    let mut observations = Vec::new();

    for presence in detect()? {
        if !presence.installed {
            continue;
        }
        let provider = provider_ref(presence.kind)?;
        let observed = match presence.kind {
            MuxKind::Tmux => {
                environment(Tmux::system(), plan, provider.clone(), &surfaces).observe()
            }
            MuxKind::Cmux => {
                environment(Cmux::system(), plan, provider.clone(), &surfaces).observe()
            }
            // Any mux kind this build does not yet project is skipped rather
            // than guessed at. A silent guess here would put a row in the
            // operator's field that no provider stands behind.
            _ => continue,
        };
        match observed {
            Ok(mut observation) => {
                if let Some(version) = presence.version.clone() {
                    observation.provider_version = Some(version);
                }
                observation
                    .provenance
                    .push(format!("observed over session plan {}", plan.name));
                observations.push(observation);
            }
            // An installed mux that will not answer is a real, common state —
            // a cmux app that is not running, a tmux server that died. It is
            // an unavailable provider, not a failure of this reading, and
            // certainly not a reason to refuse the whole field: doing that
            // would take the TUI's whole Worlds pane down with one dead
            // socket. Report it as the provider vocabulary already can, with
            // the reason attached so the operator can act on it.
            Err(error) => observations.push(WorkingEnvironmentObservation {
                schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
                provider,
                provider_version: presence.version.clone(),
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

    // Herdr answers over the same plan and the same canonical Surfaces as the
    // muxes, through its own rich adapter, and is left out entirely when its
    // CLI is absent — the same law the mux field applies. An installed Herdr
    // whose server is not running is an unavailable provider, not a failure
    // of this reading.
    if let Some(version) = herdr_version()? {
        let provider = herdr_provider_ref()?;
        match herdr_environment(plan)?.observe() {
            Ok(mut observation) => {
                observation.provider_version = Some(version);
                observation
                    .provenance
                    .push(format!("observed over session plan {}", plan.name));
                observations.push(observation);
            }
            Err(error) => observations.push(WorkingEnvironmentObservation {
                schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
                provider,
                provider_version: Some(version),
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
pub fn act(
    plan: &SessionPlan,
    provider: &ResourceRef,
    subject: &ResourceRef,
    operation: WorkingEnvironmentOperation,
) -> Result<WorkingEnvironmentOutcome> {
    if provider == &herdr_provider_ref()? {
        return act_in_herdr(plan, provider, subject, operation);
    }
    let surfaces = plan_surfaces(plan);
    let Some(presence) = detect()?
        .into_iter()
        .find(|presence| provider_ref(presence.kind).ok().as_ref() == Some(provider))
    else {
        return Ok(WorkingEnvironmentOutcome::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: format!("{provider} is not a working environment this build projects"),
        });
    };
    if !presence.installed {
        return Ok(WorkingEnvironmentOutcome::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: presence
                .detail
                .unwrap_or_else(|| format!("{provider} is not installed on this host")),
        });
    }

    match presence.kind {
        MuxKind::Tmux => act_in(
            environment(Tmux::system(), plan, provider.clone(), &surfaces),
            provider,
            subject,
            operation,
        ),
        MuxKind::Cmux => act_in(
            environment(Cmux::system(), plan, provider.clone(), &surfaces),
            provider,
            subject,
            operation,
        ),
        _ => Ok(WorkingEnvironmentOutcome::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: format!("{provider} has no working-environment projection in this build"),
        }),
    }
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
    let tmux_provider = provider_ref(MuxKind::Tmux)?;
    if provider == &herdr_provider_ref()? {
        return Ok(WorkingEnvironmentTerminalAttachment::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: "the Herdr adapter publishes focus and agent-session operations, not a \
                     terminal-client attach command; open and focus are its supported routes"
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

/// Open or focus one canonical Surface in the Herdr rich environment.
///
/// Herdr workspaces and panes are provider-minted, so the first explicit open
/// for a plan creates the workspace and binds the opened Surface to its root
/// pane; the created evidence reaches the caller through the returned
/// observation for persistence into the plan's `backend_extensions.herdr`.
/// A plan that already records a workspace is ensured, never silently
/// recreated: if that workspace is gone, the provider's refusal is the
/// operation's result.
fn act_in_herdr(
    plan: &SessionPlan,
    provider: &ResourceRef,
    subject: &ResourceRef,
    operation: WorkingEnvironmentOperation,
) -> Result<WorkingEnvironmentOutcome> {
    if herdr_version()?.is_none() {
        return Ok(WorkingEnvironmentOutcome::NotExposed {
            provider: provider.clone(),
            subject: subject.clone(),
            reason: "herdr is not installed on this host".into(),
        });
    }
    let mut environment = herdr_environment(plan)?;
    match operation {
        WorkingEnvironmentOperation::Open => {
            let mut created = None;
            if herdr_recorded_workspace(plan).is_none() {
                let cwd = plan.root.as_ref().ok_or_else(|| {
                    AikitError::new(
                        "herdr.workspace_absent",
                        format!(
                            "plan {} has no root cwd, so its first Herdr open cannot create a workspace",
                            plan.id
                        ),
                    )
                })?;
                environment =
                    environment.with_create(cwd.display().to_string(), Some(plan.name.clone()));
                environment.create_workspace_and_bind_root(subject.clone())?;
            } else {
                environment.open()?;
            }
            let observation = environment.observe()?;
            let Some(native_id) = observation.canonical_native_id(subject) else {
                return Ok(WorkingEnvironmentOutcome::NotExposed {
                    provider: provider.clone(),
                    subject: subject.clone(),
                    reason: format!(
                        "{provider} ensured its Herdr workspace but Surface {subject} has no \
                         recorded pane binding in plan {}",
                        plan.id
                    ),
                });
            };
            if herdr_recorded_workspace(plan).is_none() {
                // This open created provider-native material. Its exact
                // bindings travel with the outcome so the caller can persist
                // them; nothing here writes state on its own.
                created = Some(observation.bindings.clone());
            }
            Ok(WorkingEnvironmentOutcome::Opened {
                provider: provider.clone(),
                subject: subject.clone(),
                native_id: native_id.to_string(),
                created,
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
