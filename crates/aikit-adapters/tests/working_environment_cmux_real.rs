//! Provider-gated SessionSpace conformance against a real current cmux app.
//!
//! This test is intentionally not a mock fallback. It skips on ordinary hosts
//! and becomes mandatory when `AIKIT_REQUIRE_CMUX_REAL=1` is supplied by a macOS
//! provider runner with a reachable cmux control socket.

mod common;

use aikit_adapters::mux::cmux::Cmux;
use aikit_adapters::runner::SystemRunner;
use aikit_adapters::{
    MuxWorkingEnvironment, NativeBindingKind, WorkingEnvironmentHealth, WorkingEnvironmentProvider,
};
use aikit_core::resource::ResourceRef;
use common::plan_from;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn require_cmux() -> Option<Cmux<SystemRunner>> {
    let cmux = Cmux::system();
    let probe = cmux.probe().unwrap();
    if probe.reachable {
        return Some(cmux);
    }
    if std::env::var_os("AIKIT_REQUIRE_CMUX_REAL").is_some() {
        panic!(
            "AIKIT_REQUIRE_CMUX_REAL is set but current cmux is not reachable: {:?}",
            probe.note
        );
    }
    eprintln!(
        "SKIP real cmux SessionSpace provider proof: no reachable current cmux control socket"
    );
    None
}

#[test]
fn real_cmux_exposes_native_ids_only_through_explicit_session_space_bindings() {
    let Some(cmux) = require_cmux() else {
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let plan = plan_from(&format!(
        r#"
schema = 1
id = "aikit-cmux-provider-real"
name = "aikit-cmux-provider-real"
root = "{}"

[[views]]
id = "main"
[[views.panes]]
id = "agent"
focus = true
command = ["sh", "-c", "sleep 120"]
[[views.panes]]
id = "shell"
split_from = "agent"
direction = "right"
ratio = 0.4
command = ["sh", "-c", "sleep 120"]
"#,
        temp.path().display()
    ));
    let mut environment = MuxWorkingEnvironment::new(cmux, plan, r("provider/cmux/current"))
        .bind_surface(r("surface/cmux/agent"), "main/agent")
        .bind_surface(r("surface/cmux/shell"), "main/shell")
        .bind_project(r("project/alpha"), "workspace-provenance:alpha")
        .bind_project(r("project/beta"), "workspace-provenance:beta")
        .bind_agent_session(r("agent-session/cmux-a"), "surface-host:agent")
        .bind_agent_session(r("agent-session/cmux-b"), "surface-host:shell");

    let opened = environment.open().unwrap();
    assert_eq!(opened.health, WorkingEnvironmentHealth::Healthy);
    assert!(opened.provider_version.is_some());
    for canonical in [r("surface/cmux/agent"), r("surface/cmux/shell")] {
        let binding = opened
            .bindings
            .iter()
            .find(|binding| {
                binding.kind == NativeBindingKind::Surface
                    && binding.canonical_ref.as_ref() == Some(&canonical)
            })
            .expect("real cmux must return the explicitly-bound surface");
        assert!(!binding.native_id.is_empty());
        assert_ne!(binding.native_id, canonical.to_string());
    }
    assert_eq!(
        opened
            .bindings
            .iter()
            .filter(|binding| binding.kind == NativeBindingKind::Project)
            .count(),
        2
    );
    assert_eq!(
        opened
            .bindings
            .iter()
            .filter(|binding| binding.kind == NativeBindingKind::AgentSession)
            .count(),
        2
    );

    environment.focus_surface(&r("surface/cmux/shell")).unwrap();
    let observed = environment.observe().unwrap();
    assert!(observed
        .canonical_native_id(&r("surface/cmux/shell"))
        .is_some());

    environment
        .detach_surface(&r("surface/cmux/shell"))
        .unwrap();
    let after_detach = environment.observe().unwrap();
    assert!(after_detach
        .bindings
        .iter()
        .all(|binding| binding.canonical_ref.as_ref() != Some(&r("surface/cmux/shell"))));
}
