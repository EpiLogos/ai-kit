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

use aikit_adapters::mux::{cmux::Cmux, tmux::Tmux, MuxAdapter, MuxPresence};
use aikit_adapters::{MuxWorkingEnvironment, WorkingEnvironmentProvider};
use aikit_core::platform::MuxKind;
use aikit_core::resource::ResourceRef;
use aikit_core::session::SessionPlan;
use aikit_core::working_environment::{
    WorkingEnvironmentCapabilities, WorkingEnvironmentHealth, WorkingEnvironmentObservation,
    WORKING_ENVIRONMENT_PROVIDER_VERSION,
};
use aikit_core::Result;
use aikit_tui::live_field::{WorkingEnvironmentOperation, WorkingEnvironmentOutcome};

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
