//! W6 at the production seam: one plan, one set of canonical Surface Refs, and
//! every installed mux observed as a projection of the same subjects.

mod common;

use aikit_cli::working_environment_field::{
    act, observe, plan_surfaces, provider_ref, surface_ref,
};
use aikit_core::SessionPlan;
use aikit_core::platform::{MuxKind, PlaceTechnology};
use aikit_core::session::SessionSpec;
use aikit_core::working_environment::WorkingEnvironmentHealth;
use aikit_tui::live_field::{
    WorkingEnvironmentOperation, WorkingEnvironmentOutcome, live_working_field,
};

fn plan() -> SessionPlan {
    SessionSpec::from_toml_str(
        r#"
schema = 1
id = "aikit-w6-proof"
name = "aikit-w6-proof"

[[views]]
id = "main"
[[views.panes]]
id = "shell"
[[views.panes]]
id = "agent"
split_from = "shell"
direction = "down"
"#,
    )
    .unwrap()
    .compile()
    .unwrap()
}

#[test]
fn canonical_surface_refs_come_from_the_plan_and_nothing_else() {
    let bound = plan_surfaces(&plan());
    let refs: Vec<String> = bound
        .iter()
        .map(|(surface, _)| surface.to_string())
        .collect();

    assert_eq!(
        refs,
        vec!["surface/terminal/main/shell", "surface/terminal/main/agent"]
    );
    // The logical key stays the mux plan key; the canonical Ref is the
    // identity. They are deliberately different strings.
    assert_eq!(bound[0].1, "main/shell");
    assert_eq!(surface_ref("main", "shell").unwrap(), bound[0].0);

    // The same plan is what both providers are handed, so both project the
    // same subjects rather than each minting its own.
    assert_eq!(plan_surfaces(&plan()), bound);
}

#[test]
fn provider_refs_are_distinct_per_mux_and_stable() {
    let tmux = provider_ref(PlaceTechnology::from(MuxKind::Tmux)).unwrap();
    let cmux = provider_ref(PlaceTechnology::from(MuxKind::Cmux)).unwrap();
    assert_ne!(tmux, cmux);
    assert_eq!(tmux.to_string(), "provider/tmux/current");
    assert_eq!(cmux.to_string(), "provider/cmux/current");
}

/// An unregistered place technology is a first-class declared-unsupported
/// outcome: typed (`NotExposed`), naming the technology and what would support
/// it. Never a crash, never a silent fallback onto another technology.
/// `screen` names nothing this build registers; `herdr`, by contrast, is
/// registered and driven (see the herdr module below).
#[test]
fn an_unregistered_technology_is_declared_unsupported_not_crashed_or_fallback() {
    let screen = PlaceTechnology::new("screen");
    let provider = provider_ref(screen).unwrap();
    let subject = surface_ref("main", "shell").unwrap();

    let opened = act(
        &plan(),
        &provider,
        &subject,
        WorkingEnvironmentOperation::Open,
    )
    .unwrap();
    match opened {
        WorkingEnvironmentOutcome::NotExposed { reason, .. } => {
            assert!(
                reason.contains("screen") && reason.contains("adapter"),
                "reason must name the technology and what would support it: {reason}"
            );
        }
        other => panic!("screen must be declared unsupported, not silently served: {other:?}"),
    }

    // The same discipline for focus, so every operation routes through the
    // registry's declared answer.
    let focused = act(
        &plan(),
        &provider,
        &subject,
        WorkingEnvironmentOperation::Focus,
    )
    .unwrap();
    assert!(matches!(
        focused,
        WorkingEnvironmentOutcome::NotExposed { .. }
    ));
}

/// A provider ref may carry an instance id (`provider/herdr/w6`, the shape a
/// commissioned place actually carries), not only the technology-canonical
/// `/current`. The ref's first segment names the technology either way, so an
/// instance ref must reach the herdr driver — a typed herdr-side refusal —
/// and never fall through to "not a working environment this build
/// projects", which is the unparseable-ref answer. Found live when a
/// workcell-commissioned herdr room stayed unprojectable under its instance
/// ref.
#[test]
fn an_instance_provider_ref_names_its_technology_and_reaches_its_driver() {
    let addressed = aikit_core::resource::ResourceRef::parse("provider/herdr/w6").unwrap();
    let subject = surface_ref("main", "shell").unwrap();
    let focused = act(
        &plan(),
        &addressed,
        &subject,
        WorkingEnvironmentOperation::Focus,
    );
    match focused {
        // The herdr driver answering with its own typed refusal (e.g.
        // `herdr.surface_unbound` on a plan with no recorded pane) is the
        // routed outcome this test wants.
        Err(error) => assert!(
            error.code().starts_with("herdr."),
            "the herdr driver answers under its own error vocabulary: {error}"
        ),
        Ok(WorkingEnvironmentOutcome::NotExposed { reason, .. }) => assert!(
            !reason.contains("not a working environment this build projects"),
            "an instance ref names its technology: the refusal must be about the \
             technology's own state on this host (installed? server running?), never \
             about ref parsing: {reason}"
        ),
        Ok(other) => {
            // Any herdr-side outcome is the driver answering — the regression
            // this test pins is the unparseable-ref fallback, not the driver's
            // own refusals.
            let _ = other;
        }
    }
}

/// Whatever this host happens to have installed, the observation is honest
/// about it and never fails: no mux gives an empty reading, an installed mux
/// that will not answer gives an unavailable one carrying the reason, and any
/// mux that does answer is projected over the plan's own canonical Refs.
#[test]
fn observing_this_host_projects_only_plan_derived_subjects() {
    let plan = plan();
    let observations = observe(&plan).expect("observation must not fail on any host");
    let plan_refs: Vec<_> = plan_surfaces(&plan)
        .into_iter()
        .map(|(surface, _)| surface)
        .collect();

    let mut providers: Vec<String> = Vec::new();
    for observation in &observations {
        providers.push(observation.provider.to_string());
        if observation.health == WorkingEnvironmentHealth::Unavailable {
            // An unavailable provider must say why rather than going quiet.
            assert!(
                !observation.provenance.is_empty(),
                "{} is unavailable with no reason given",
                observation.provider
            );
        }
        for binding in &observation.bindings {
            if let Some(canonical) = binding.canonical_ref.as_ref() {
                assert!(
                    plan_refs.contains(canonical),
                    "{canonical} is not one of this plan's canonical Surfaces"
                );
            }
        }
    }
    providers.sort();
    providers.dedup();
    assert_eq!(
        providers.len(),
        observations.len(),
        "each installed mux is observed exactly once"
    );

    // The derived field never invents a subject the providers did not bind.
    let field = live_working_field(&observations, &plan_refs);
    for subject in &field.subjects {
        assert!(plan_refs.contains(&subject.subject));
    }
}

/// Not an assertion about this machine — a printed reading of it, so the
/// physical proving matrix has something to compare a real terminal against.
#[test]
fn print_this_hosts_reading() {
    let plan = plan();
    let observations = observe(&plan).unwrap();
    let plan_refs: Vec<_> = plan_surfaces(&plan)
        .into_iter()
        .map(|(surface, _)| surface)
        .collect();
    let field = live_working_field(&observations, &plan_refs);
    println!("--- observed providers ---");
    for provider in &field.observed {
        println!(
            "{} :: {:?} :: open={} focus={} bound={} :: {:?}",
            provider.provider,
            provider.health,
            provider.capabilities.open,
            provider.capabilities.focus,
            provider.bound_subjects,
            provider.provenance
        );
    }
    println!("--- reachable subjects ---");
    for subject in &field.subjects {
        println!("{} ({})", subject.subject, subject.semantic_kind);
        for reach in &subject.projections {
            println!(
                "    {} open={} focus={} native={:?}",
                reach.provider,
                reach.can_open(),
                reach.can_focus(),
                reach.native_id
            );
        }
    }
}

// --------------------------------------------------------------------------
// Herdr: a nameable and executable place technology
// --------------------------------------------------------------------------

/// Herdr is the first place technology that is registered and driven without
/// the mux contract. Its route shares every law the mux path enforces, only
/// the provider differs, so these tests pin the wiring: the canonical provider
/// ref, the registry resolution, the honest detect reading, and the field row
/// that exists exactly when the CLI is installed. The deep create-or-attach
/// proofs live in `aikit-adapters/tests/herdr_contract.rs`, driven by
/// recorded responses, because no live herdr daemon can run on every host.
mod herdr_place_technology {
    use super::*;
    use aikit_adapters::place_technology::PlaceTechnologyRegistry;
    use aikit_cli::working_environment_field::WorkingEnvironmentTerminalAttachment;

    fn herdr_installed() -> bool {
        std::process::Command::new("herdr")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn herdr_provider_ref_is_distinct_from_every_mux_provider() {
        let herdr = provider_ref(PlaceTechnology::herdr()).unwrap();
        assert_eq!(herdr.to_string(), "provider/herdr/current");
        assert_ne!(herdr, provider_ref(PlaceTechnology::tmux()).unwrap());
        assert_ne!(herdr, provider_ref(PlaceTechnology::cmux()).unwrap());
        assert_ne!(herdr, provider_ref(PlaceTechnology::plain()).unwrap());
    }

    #[test]
    fn the_registry_drives_herdr_without_a_mux_adapter() {
        let registry = PlaceTechnologyRegistry::builtin();
        let entry = registry
            .resolve(&PlaceTechnology::herdr())
            .expect("this build registers herdr");
        assert_eq!(entry.technology(), PlaceTechnology::herdr());
        assert!(
            entry.mux_adapter().is_none(),
            "herdr is not a mux; driving it through the mux contract would be a lie"
        );
        let subject = surface_ref("main", "shell").unwrap();
        let plan = plan();
        // The addressed provider is the binding's ref, not the technology-
        // canonical one: the returned environment must be the one that ref
        // names (found live — a hardcoded `provider/herdr/current` left a
        // binding addressed at `provider/herdr/w6` unprojectable).
        let addressed = aikit_core::resource::ResourceRef::parse("provider/herdr/w6").unwrap();
        let driven = entry.working_environment(
            &plan,
            &addressed,
            &plan_surfaces(&plan),
            Some(&subject),
        );
        assert!(
            driven.is_some(),
            "a registered herdr must hand back the plan-scoped provider"
        );
    }

    /// Not an assertion about this machine — the reading is honest either
    /// way: installed carries a real version, absent carries the reason.
    #[test]
    fn herdr_detect_reading_is_honest_on_every_host() {
        let registry = PlaceTechnologyRegistry::builtin();
        let reading = registry
            .resolve(&PlaceTechnology::herdr())
            .unwrap()
            .detect()
            .expect("detection must observe, never assume");
        assert_eq!(reading.technology, PlaceTechnology::herdr());
        if herdr_installed() {
            assert!(reading.installed, "an installed herdr is reported installed");
            assert!(
                reading.version.is_some(),
                "the version probe result is carried: {:?}",
                reading.version
            );
        } else {
            let detail = reading.detail.clone().unwrap_or_default();
            assert!(
                !reading.installed && detail.contains("not installed"),
                "an absent herdr is absent with the reason attached: {reading:?}"
            );
        }
    }

    #[test]
    fn herdr_answers_in_the_field_exactly_when_installed() {
        let observations = observe(&plan()).expect("observation must not fail on any host");
        let herdr_row = observations
            .iter()
            .find(|observation| observation.provider == provider_ref(PlaceTechnology::herdr()).unwrap());
        if herdr_installed() {
            let row = herdr_row.expect("an installed herdr must answer in the field");
            assert!(row.provider_version.is_some());
            if row.health == WorkingEnvironmentHealth::Unavailable {
                assert!(
                    !row.provenance.is_empty(),
                    "an unobservable herdr must say why"
                );
            }
        } else {
            assert!(
                herdr_row.is_none(),
                "herdr is absent here: listing it would put an unusable row in the field"
            );
        }
    }

    #[test]
    fn herdr_publishes_no_terminal_client_attachment() {
        // Refused before any provider probe, on every host: the provider
        // check is about what the adapter publishes, not what is installed.
        let attachment = aikit_cli::working_environment_field::terminal_attachment(
            &plan(),
            &provider_ref(PlaceTechnology::herdr()).unwrap(),
            &surface_ref("main", "shell").unwrap(),
        )
        .unwrap();
        match attachment {
            WorkingEnvironmentTerminalAttachment::NotExposed { reason, .. } => {
                assert!(
                    reason.contains("herdr") && reason.contains("attach"),
                    "the refusal names the provider fact: {reason}"
                );
            }
            other => panic!("herdr must not publish attachment: {other:?}"),
        }
    }
}

// --------------------------------------------------------------------------
// Real-provider round trip
// --------------------------------------------------------------------------

fn tmux_installed() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// A plan whose panes idle instead of exiting, on its own tmux socket, so the
/// test never touches the developer's real server.
fn live_plan(name: &str) -> SessionPlan {
    SessionSpec::from_toml_str(&format!(
        r#"
schema = 1
id = "{name}"
name = "{name}"

[[views]]
id = "main"
[[views.panes]]
id = "shell"
focus = true
command = ["sh", "-c", "sleep 300"]
"#
    ))
    .unwrap()
    .compile()
    .unwrap()
}

struct SocketGuard(String);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        common::end_tmux_server(&self.0);
    }
}

/// W6's acceptance against a real provider: a canonical subject that is not
/// live becomes live through `open`, gains a native binding, becomes
/// focusable, and focuses — with the canonical Ref unchanged throughout and
/// the native id never standing in for it.
#[test]
fn a_real_provider_opens_then_focuses_the_same_canonical_subject() {
    if !tmux_installed() {
        eprintln!("SKIP real tmux W6 round trip: tmux is not installed");
        return;
    }
    let socket = format!("aikit-w6-{}", std::process::id());
    let _guard = SocketGuard(socket.clone());
    // The adapter reads its socket from the environment, which is how the
    // production path is configured too.
    std::env::set_var("AIKIT_TMUX_SOCKET", &socket);

    let plan = live_plan("aikit-w6-roundtrip");
    let subject = surface_ref("main", "shell").unwrap();
    let tmux = provider_ref(PlaceTechnology::tmux()).unwrap();
    let plan_refs = vec![subject.clone()];

    // Before: projectable, openable, not live, not focusable.
    let before = live_working_field(&observe(&plan).unwrap(), &plan_refs);
    let reach = before
        .subject(&subject)
        .expect("plan subject is projectable");
    let tmux_reach = reach.projection(&tmux).expect("tmux projects it");
    assert!(
        tmux_reach.can_open(),
        "tmux must offer open before anything is live"
    );
    assert!(!tmux_reach.can_focus());
    assert!(tmux_reach.native_id.is_none());

    let opened = act(&plan, &tmux, &subject, WorkingEnvironmentOperation::Open).unwrap();
    let native = match &opened {
        WorkingEnvironmentOutcome::Opened {
            provider,
            subject: opened_subject,
            native_id,
            ..
        } => {
            assert_eq!(provider, &tmux);
            // The canonical subject is the one we asked for, unchanged.
            assert_eq!(opened_subject, &subject);
            assert!(!native_id.is_empty());
            native_id.clone()
        }
        other => panic!("tmux did not open the subject: {other:?}"),
    };

    // After: live, bound to a native pane, and now focusable.
    let after = live_working_field(&observe(&plan).unwrap(), &plan_refs);
    let reach = after.subject(&subject).unwrap();
    let tmux_reach = reach.projection(&tmux).unwrap();
    assert_eq!(tmux_reach.native_id.as_deref(), Some(native.as_str()));
    assert!(tmux_reach.can_focus(), "a live pane must be focusable");

    // The native id is provenance: it is not, and never becomes, a subject.
    assert!(after.subject(&subject).is_some());
    assert_eq!(after.subjects.len(), 1);
    assert_ne!(subject.to_string(), native);

    let focused = act(&plan, &tmux, &subject, WorkingEnvironmentOperation::Focus).unwrap();
    match focused {
        WorkingEnvironmentOutcome::Focused {
            subject: focused_subject,
            native_id,
            ..
        } => {
            assert_eq!(focused_subject, subject);
            assert_eq!(native_id, native);
        }
        other => panic!("tmux did not focus the subject: {other:?}"),
    }

    std::env::remove_var("AIKIT_TMUX_SOCKET");
}

/// A pane whose name cannot form a canonical Ref — a trailing space is enough,
/// since the Ref grammar rejects an untrimmed string — is left out of the field
/// instead of failing the whole reading. The mux still runs the pane; AIKit
/// simply cannot address it, and says so by omission rather than by collapsing
/// the Worlds pane over one stray character in a session spec.
#[test]
fn a_pane_that_cannot_be_named_is_skipped_not_fatal() {
    let plan = SessionSpec::from_toml_str(
        r#"
schema = 1
id = "aikit-w6-odd-names"
name = "aikit-w6-odd-names"

[[views]]
id = "main"
[[views.panes]]
id = "shell"
[[views.panes]]
id = "trailing space "
split_from = "shell"
direction = "down"
"#,
    )
    .unwrap()
    .compile()
    .unwrap();

    let bound = plan_surfaces(&plan);
    let refs: Vec<String> = bound
        .iter()
        .map(|(surface, _)| surface.to_string())
        .collect();
    assert!(refs.contains(&"surface/terminal/main/shell".to_string()));
    assert!(refs.len() < plan.pane_count());
    // And observing over that plan still works rather than raising.
    observe(&plan).expect("an unnameable pane must not fail the whole reading");
}
