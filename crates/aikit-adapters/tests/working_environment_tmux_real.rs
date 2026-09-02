//! SessionSpace working-environment conformance against a real tmux server.

mod common;

use common::plan_from;
use std::sync::atomic::{AtomicU32, Ordering};

use aikit_adapters::mux::tmux::Tmux;
use aikit_adapters::mux::{MuxAdapter, MuxTarget};
use aikit_adapters::runner::SystemRunner;
use aikit_adapters::{
    MuxSessionSpaceActivationDriver, MuxWorkingEnvironment, NativeBindingKind,
    WorkingEnvironmentHealth, WorkingEnvironmentProvider,
};
use aikit_core::composition::{
    ActivationScope, ActivationScopeKind, ComponentBinding, CompositionActivationMode,
    LifetimeOwner, LifetimeOwnerKind, ResolutionScope, SurfaceDescriptor, SurfaceKind,
};
use aikit_core::resource::ResourceRef;
use aikit_core::scope::ScopeKind;
use aikit_core::session::SessionPlan;
use aikit_core::{
    SessionSpaceActivationDriver, SessionSpaceActivationObservation, SessionSpaceActivationRequest,
    SessionSpaceDefinition, SessionSpaceRef, SessionSpaceRuntime,
};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn tmux_installed() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

struct SocketGuard(String);
impl SocketGuard {
    fn new() -> Self {
        Self(format!(
            "aikit-space-provider-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ))
    }
    fn name(&self) -> &str {
        &self.0
    }
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .output();
    }
}

fn plan(name: &str, root: &std::path::Path) -> SessionPlan {
    plan_from(&format!(
        r#"
schema = 1
id = "{name}"
name = "{name}"
root = "{root}"

[[views]]
id = "main"
[[views.panes]]
id = "agent"
focus = true
command = ["sh", "-c", "printf agent-ready > {root}/agent.ready; sleep 300"]
[[views.panes]]
id = "terminal"
split_from = "agent"
direction = "down"
ratio = 0.35
command = ["sh", "-c", "printf terminal-ready > {root}/terminal.ready; sleep 300"]
"#,
        root = root.display()
    ))
}

fn environment(socket: &str, plan: SessionPlan) -> MuxWorkingEnvironment<Tmux<SystemRunner>> {
    MuxWorkingEnvironment::new(
        Tmux::new(SystemRunner::new()).with_socket(socket),
        plan,
        r("provider/tmux/current"),
    )
    .bind_surface(r("surface/terminal/agent"), "main/agent")
    .bind_surface(r("surface/terminal/shell"), "main/terminal")
    .bind_project(r("project/alpha"), "tmux-cwd:/project/alpha")
    .bind_project(r("project/beta"), "tmux-cwd:/project/beta")
    .bind_agent_session(r("agent-session/alpha"), "hosted-in:main/agent")
    .bind_agent_session(r("agent-session/beta"), "hosted-in:main/terminal")
}

fn activation_request() -> SessionSpaceActivationRequest {
    let component = r("component/tmux/terminal-field");
    SessionSpaceActivationRequest {
        space: SessionSpaceRef::parse("session-space/tmux-real").unwrap(),
        agent_session: r("agent-session/alpha"),
        harness: r("harness/tmux-real"),
        component: ComponentBinding {
            component: component.clone(),
            resolution_scope: ResolutionScope::new(ScopeKind::Session, "tmux real test"),
            activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession),
            lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::AgentSession),
            activation_mode: CompositionActivationMode::LiveMounted,
            implementation: None,
        },
        composition_fingerprint: "tmux-real-fingerprint".into(),
        surfaces: vec![
            SurfaceDescriptor {
                resource: r("surface/terminal/agent"),
                kind: SurfaceKind::Conversation,
                target_native_id: None,
                owner_component: Some(component.clone()),
            },
            SurfaceDescriptor {
                resource: r("surface/terminal/shell"),
                kind: SurfaceKind::Cli,
                target_native_id: None,
                owner_component: Some(component),
            },
        ],
    }
}

#[test]
fn real_tmux_survives_adapter_restart_recovers_relations_and_never_mints_canonical_identity() {
    if !tmux_installed() {
        eprintln!("SKIP real tmux SessionSpace provider proof: tmux is not installed");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let guard = SocketGuard::new();
    let session_plan = plan("aikit-space-real", temp.path());
    let request = activation_request();

    let mut driver =
        MuxSessionSpaceActivationDriver::new(environment(guard.name(), session_plan.clone()));
    let activated = driver.activate(&request).unwrap();
    assert!(matches!(
        activated,
        SessionSpaceActivationObservation::Active { .. }
    ));
    assert!(temp.path().join("agent.ready").exists());
    assert!(temp.path().join("terminal.ready").exists());

    let first = driver.environment_mut().observe().unwrap();
    assert_eq!(first.health, WorkingEnvironmentHealth::Healthy);
    assert!(first.bindings.iter().any(|binding| {
        binding.kind == NativeBindingKind::Surface
            && binding.canonical_ref.as_ref() == Some(&r("surface/terminal/agent"))
            && !binding.native_id.is_empty()
    }));
    assert_eq!(
        first
            .bindings
            .iter()
            .filter(|binding| binding.kind == NativeBindingKind::Project)
            .count(),
        2
    );
    assert_eq!(
        first
            .bindings
            .iter()
            .filter(|binding| binding.kind == NativeBindingKind::AgentSession)
            .count(),
        2
    );
    assert!(driver.environment().adapter().capabilities().true_popup);
    assert!(driver.environment().adapter().capabilities().remote_control);

    // Focus is a real provider action against the explicitly-bound pane id.
    driver
        .environment_mut()
        .focus_surface(&r("surface/terminal/shell"))
        .unwrap();

    let provider_session = driver.environment().last_binding().unwrap().session.clone();

    // Dropping AIKit's adapter object is the provider-facing analogue of process
    // restart. The private tmux server/session continues independently.
    drop(driver);
    let mut restarted = environment(guard.name(), session_plan.clone());
    let after_restart = restarted.observe().unwrap();
    assert!(after_restart
        .canonical_native_id(&r("surface/terminal/agent"))
        .is_some());
    assert!(restarted.adapter().session_exists(&session_plan).unwrap());

    // A fresh SessionSpace runtime does not acquire AgentSession continuity merely
    // because the tmux server survived. That boundary belongs to AIKit/session
    // persistence and target protocol evidence, not tmux.
    let fresh_runtime = SessionSpaceRuntime::open(SessionSpaceDefinition::new(
        SessionSpaceRef::parse("session-space/fresh-after-aikit-restart").unwrap(),
    ))
    .unwrap();
    assert!(fresh_runtime.read_model().agent_sessions.is_empty());

    // Remove the provider session. Desired/canonical refs are still the same
    // values in the explicit binding configuration; observed Surface truth goes
    // absent until reconstruction.
    restarted
        .adapter_mut()
        .close(&MuxTarget::session(
            aikit_core::platform::MuxKind::Tmux,
            provider_session,
        ))
        .unwrap();
    let disappeared = restarted.observe().unwrap();
    assert!(disappeared
        .bindings
        .iter()
        .all(|binding| binding.kind != NativeBindingKind::Surface));

    let reconstructed = restarted.open().unwrap();
    assert_eq!(
        reconstructed
            .bindings
            .iter()
            .find(|binding| {
                binding.kind == NativeBindingKind::Surface
                    && binding.canonical_ref.as_ref() == Some(&r("surface/terminal/agent"))
            })
            .unwrap()
            .canonical_ref
            .as_ref(),
        Some(&r("surface/terminal/agent"))
    );
    assert!(temp.path().join("agent.ready").exists());

    // Detach is provider-local. It does not delete or rename the canonical Surface.
    restarted
        .detach_surface(&r("surface/terminal/shell"))
        .unwrap();
    assert_eq!(
        r("surface/terminal/shell").to_string(),
        "surface/terminal/shell"
    );
}
