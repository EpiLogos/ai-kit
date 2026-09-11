//! W6 at the production seam: one plan, one set of canonical Surface Refs, and
//! every installed mux observed as a projection of the same subjects.

mod common;

use aikit_cli::working_environment_field::{
    act, observe, plan_surfaces, provider_ref, surface_ref,
};
use aikit_core::platform::MuxKind;
use aikit_core::session::SessionSpec;
use aikit_core::working_environment::WorkingEnvironmentHealth;
use aikit_core::SessionPlan;
use aikit_tui::live_field::{
    live_working_field, WorkingEnvironmentOperation, WorkingEnvironmentOutcome,
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
    let tmux = provider_ref(MuxKind::Tmux).unwrap();
    let cmux = provider_ref(MuxKind::Cmux).unwrap();
    assert_ne!(tmux, cmux);
    assert_eq!(tmux.to_string(), "provider/tmux/current");
    assert_eq!(cmux.to_string(), "provider/cmux/current");
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
    let tmux = provider_ref(MuxKind::Tmux).unwrap();
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
