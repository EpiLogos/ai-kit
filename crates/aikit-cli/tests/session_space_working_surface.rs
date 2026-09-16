use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use aikit_cli::session_space_working_surface::{
    WorkingSurfaceNativeStanding, focus, observe, open, terminal_attachment,
};
use aikit_cli::working_environment_field::WorkingEnvironmentTerminalAttachment;
use aikit_core::resource::ResourceRef;
use aikit_core::session::SessionSpec;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation, SessionSpaceNativeReferenceBinding,
    SessionSpaceNativeReferenceKind, SessionSpaceSurfaceAttachmentIntent,
    SessionSpaceWorkingSurfaceBinding,
};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use aikit_tui::live_field::WorkingEnvironmentOutcome;
use assert_cmd::cargo::cargo_bin;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn tmux_installed() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

struct SocketGuard(String);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", self.0.as_str(), "kill-server"])
            .output();
    }
}

fn plan(name: &str) -> aikit_core::SessionPlan {
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
command = ["sh"]
"#
    ))
    .unwrap()
    .compile()
    .unwrap()
}

fn apply(
    store: &SessionSpaceApplicationStore,
    space: &SessionSpaceRef,
    intent: SessionSpaceMutation,
) {
    let preview = store.stage(Some(space), intent).unwrap();
    store.apply(&preview).unwrap();
}

fn attach_through_public_cli(
    home: &std::path::Path,
    socket: &str,
    space: &SessionSpaceRef,
    binding: &ResourceRef,
    marker: &str,
    native: &str,
) -> Vec<u8> {
    if !std::path::Path::new("/usr/bin/script").is_file() {
        eprintln!("SKIP terminal attach PTY proof: /usr/bin/script is unavailable");
        return Vec::new();
    }
    let binary = cargo_bin("aikit-session-space");
    let argv = vec![
        binary.to_str().unwrap().to_owned(),
        "-C".into(),
        home.to_str().unwrap().to_owned(),
        "working-surface".into(),
        "attach".into(),
        space.to_string(),
        binding.as_str().to_owned(),
    ];
    let mut script = Command::new("/usr/bin/script");
    // util-linux uses -c; BSD script accepts a command argv. Do not depend on
    // the recent util-linux positional-command extension absent on CI hosts.
    if cfg!(target_os = "linux") {
        script.args(["-q", "-e", "-c", &shell_words::join(&argv), "/dev/null"]);
    } else {
        script.args(["-q", "/dev/null"]).args(&argv);
    }
    let mut client = script
        .env("AIKIT_HOME", home)
        .env("AIKIT_TMUX_SOCKET", socket)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("public owner command attaches a terminal client through a PTY");
    let mut input = client.stdin.take().unwrap();
    input
        .write_all(format!("printf '{marker}\\n'\n").as_bytes())
        .unwrap();
    input.flush().unwrap();
    // Verify shell execution in the exact native pane, not merely input echoed
    // by the outer PTY. The owner-resolved pane remains the same throughout.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let capture = Command::new("tmux")
            .args(["-L", socket, "capture-pane", "-p", "-t", native])
            .output()
            .unwrap();
        if capture.status.success()
            && String::from_utf8_lossy(&capture.stdout)
                .lines()
                .any(|line| line.trim() == marker)
        {
            break;
        }
        if client.try_wait().unwrap().is_some() || Instant::now() >= deadline {
            let _ = client.kill();
            let output = client.wait_with_output().unwrap();
            panic!(
                "public PTY command did not execute in exact pane: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    // tmux detach prefix. The shell continues in the provider Surface.
    input.write_all(&[0x02, b'd']).unwrap();
    input.flush().unwrap();
    drop(input);
    let output = client
        .wait_with_output()
        .expect("attached public CLI client exits after tmux detach");
    assert!(
        output.status.success(),
        "public CLI attach client failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

/// The public owner path persists a canonical provider plan/surface binding,
/// restarts its state authority, then asks the existing provider seam to create
/// and focus the exact same canonical surface on a private real tmux server.
#[test]
fn persisted_working_surface_opens_and_focuses_real_tmux_after_store_restart() {
    if !tmux_installed() {
        eprintln!("SKIP persisted working Surface tmux proof: tmux is not installed");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("aikit-home"));
    home.ensure_layout().unwrap();
    let store = SessionSpaceApplicationStore::new(home.clone());
    let space = SessionSpaceRef::parse("session-space/persisted-tmux").unwrap();
    let agent = r("agent-session/persisted-tmux");
    let surface = r("surface/terminal/main/shell");
    let provider = r("provider/tmux/current");
    let binding = r("working-surface/persisted-tmux-shell");

    let create = store
        .stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: Some("persisted tmux".into()),
            },
        )
        .unwrap();
    store.apply(&create).unwrap();
    apply(
        &store,
        &space,
        SessionSpaceMutation::AttachAgentSession {
            attachment: SessionSpaceAgentAttachmentIntent {
                agent_session: agent.clone(),
                purpose: Some("real provider proof".into()),
                provenance: vec!["test".into()],
            },
        },
    );
    apply(
        &store,
        &space,
        SessionSpaceMutation::AttachSurface {
            attachment: SessionSpaceSurfaceAttachmentIntent {
                surface: surface.clone(),
                component: None,
                purpose: Some("exact terminal Surface".into()),
                provenance: vec!["test".into()],
            },
        },
    );
    apply(
        &store,
        &space,
        SessionSpaceMutation::BindNativeReference {
            binding: SessionSpaceNativeReferenceBinding {
                reference: provider.clone(),
                kind: SessionSpaceNativeReferenceKind::Provider,
                owner: None,
                provider: None,
                host: None,
                purpose: Some("tmux provider".into()),
                provenance: vec!["test".into()],
            },
        },
    );
    apply(
        &store,
        &space,
        SessionSpaceMutation::BindWorkingSurface {
            binding: Box::new(SessionSpaceWorkingSurfaceBinding {
                binding: binding.clone(),
                surface: surface.clone(),
                agent_session: agent.clone(),
                provider: provider.clone(),
                plan: plan("aikit-persisted-surface"),
                plan_key: "main/shell".into(),
                provenance: vec!["persisted test binding".into()],
            }),
        },
    );

    let socket = format!(
        "aikit-persisted-surface-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    let _guard = SocketGuard(socket.clone());
    std::env::set_var("AIKIT_TMUX_SOCKET", &socket);

    let before = observe(&store.load(&space).unwrap(), &binding).unwrap();
    assert!(before.reading.live_native_id.is_none());
    assert!(before.outcome.is_none());

    let opened = open(&store.load(&space).unwrap(), &binding).unwrap();
    let native = match opened.outcome.unwrap() {
        WorkingEnvironmentOutcome::Opened {
            provider: opened_provider,
            subject,
            native_id,
        } => {
            assert_eq!(opened_provider, provider);
            assert_eq!(subject, surface);
            native_id
        }
        other => panic!("expected real tmux open, got {other:?}"),
    };
    assert_eq!(
        opened.reading.live_native_id.as_deref(),
        Some(native.as_str())
    );

    let attachment = terminal_attachment(&store.load(&space).unwrap(), &binding).unwrap();
    match attachment {
        WorkingEnvironmentTerminalAttachment::Attach {
            provider: attached_provider,
            subject,
            native_id,
            argv,
        } => {
            assert_eq!(attached_provider, provider);
            assert_eq!(subject, surface);
            assert_eq!(native_id, native);
            assert_eq!(
                argv,
                vec![
                    "tmux",
                    "-L",
                    socket.as_str(),
                    "attach-session",
                    "-t",
                    "aikit-persisted-surface",
                    ";",
                    "select-pane",
                    "-t",
                    native.as_str(),
                ]
            );
        }
        other => panic!("expected exact terminal attachment, got {other:?}"),
    }

    let marker_one = "PERSISTED_WORKING_SURFACE_ONE";
    let output_one =
        attach_through_public_cli(home.root(), &socket, &space, &binding, marker_one, &native);
    if !output_one.is_empty() {
        assert!(
            String::from_utf8_lossy(&output_one).contains(marker_one),
            "first public PTY attachment did not expose the exact pane marker: {}",
            String::from_utf8_lossy(&output_one)
        );
    }
    let after_detach = observe(&store.load(&space).unwrap(), &binding).unwrap();
    assert_eq!(
        after_detach.reading.live_native_id.as_deref(),
        Some(native.as_str())
    );

    let marker_two = "PERSISTED_WORKING_SURFACE_TWO";
    let output_two =
        attach_through_public_cli(home.root(), &socket, &space, &binding, marker_two, &native);
    if !output_two.is_empty() {
        assert!(
            String::from_utf8_lossy(&output_two).contains(marker_two),
            "second public PTY attachment did not preserve the exact pane: {}",
            String::from_utf8_lossy(&output_two)
        );
    }

    // Reopen the durable authority before focus. The live native id is observed
    // again from tmux; it was never stored as SessionSpace identity.
    drop(store);
    let restarted = SessionSpaceApplicationStore::new(home);
    let focused = focus(&restarted.load(&space).unwrap(), &binding).unwrap();
    match focused.outcome.unwrap() {
        WorkingEnvironmentOutcome::Focused {
            provider: focused_provider,
            subject,
            native_id,
        } => {
            assert_eq!(focused_provider, provider);
            assert_eq!(subject, surface);
            assert_eq!(native_id, native);
        }
        other => panic!("expected real tmux focus, got {other:?}"),
    }
    assert_eq!(focused.reading.agent_session, agent);
    assert_eq!(
        focused.reading.live_native_id.as_deref(),
        Some(native.as_str())
    );

    // A provider session with the same plan name after destruction is a new
    // provider fact. The owner marks the explicit open as rebound rather than
    // letting a recycled tmux pane masquerade as the prior material.
    Command::new("tmux")
        .args(["-L", socket.as_str(), "kill-server"])
        .output()
        .unwrap();
    let rebound = open(&restarted.load(&space).unwrap(), &binding).unwrap();
    assert!(matches!(
        rebound.reading.native_standing,
        WorkingSurfaceNativeStanding::ReboundByExplicitOpen
    ));
    assert!(rebound.reading.live_native_id.is_some());
    std::env::remove_var("AIKIT_TMUX_SOCKET");
}

#[test]
fn working_surface_binding_refuses_unattached_or_mismatched_identities() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("aikit-home"));
    home.ensure_layout().unwrap();
    let store = SessionSpaceApplicationStore::new(home);
    let space = SessionSpaceRef::parse("session-space/binding-validation").unwrap();
    let create = store
        .stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: None,
            },
        )
        .unwrap();
    store.apply(&create).unwrap();

    let preview = store
        .stage(
            Some(&space),
            SessionSpaceMutation::BindWorkingSurface {
                binding: Box::new(SessionSpaceWorkingSurfaceBinding {
                    binding: r("working-surface/invalid"),
                    surface: r("surface/terminal/main/shell"),
                    agent_session: r("agent-session/missing"),
                    provider: r("provider/tmux/current"),
                    plan: plan("invalid-binding"),
                    plan_key: "main/shell".into(),
                    provenance: vec![],
                }),
            },
        )
        .unwrap_err();
    assert_eq!(preview.code(), "session_space.working_surface_unattached");
}

/// TM02-R, closed: a commissioned herdr room can now be *named* by a
/// working-surface plan. The open place-technology name stages, persists and
/// validates exactly like tmux does; what this build still cannot do is drive
/// herdr, and the provider boundary says so as a declared-unsupported outcome
/// naming the technology — not a parse failure, not a crash, not a fallback.
#[test]
fn a_plan_naming_herdr_stages_and_validates_then_declares_it_unsupported() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("aikit-home"));
    home.ensure_layout().unwrap();
    let store = SessionSpaceApplicationStore::new(home);
    let space = SessionSpaceRef::parse("session-space/herdr-room").unwrap();
    let agent = r("agent-session/herdr-room");
    let surface = r("surface/terminal/main/shell");
    let provider = r("provider/herdr/current");
    let binding = r("working-surface/herdr-room-shell");

    // The plan names herdr through the same portable spec path as any mux.
    let herdr_plan = SessionSpec::from_toml_str(
        r#"
schema = 1
id = "herdr-room-plan"
name = "herdr-room-plan"
backend = "herdr"

[[views]]
id = "main"
[[views.panes]]
id = "shell"
focus = true
command = ["sh"]
"#,
    )
    .unwrap()
    .compile()
    .unwrap();
    assert_eq!(
        herdr_plan.mux.as_ref().map(ToString::to_string),
        Some("herdr".to_string())
    );

    let create = store
        .stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: Some("herdr room".into()),
            },
        )
        .unwrap();
    store.apply(&create).unwrap();
    apply(
        &store,
        &space,
        SessionSpaceMutation::AttachAgentSession {
            attachment: SessionSpaceAgentAttachmentIntent {
                agent_session: agent.clone(),
                purpose: Some("commissioned herdr room".into()),
                provenance: vec!["test".into()],
            },
        },
    );
    apply(
        &store,
        &space,
        SessionSpaceMutation::AttachSurface {
            attachment: SessionSpaceSurfaceAttachmentIntent {
                surface: surface.clone(),
                component: None,
                purpose: Some("herdr room surface".into()),
                provenance: vec!["test".into()],
            },
        },
    );
    apply(
        &store,
        &space,
        SessionSpaceMutation::BindNativeReference {
            binding: SessionSpaceNativeReferenceBinding {
                reference: provider.clone(),
                kind: SessionSpaceNativeReferenceKind::Provider,
                owner: None,
                provider: None,
                host: None,
                purpose: Some("herdr provider".into()),
                provenance: vec!["test".into()],
            },
        },
    );
    // Validation passes the whole way to the provider boundary: the open name
    // is a first-class binding, not a parse error.
    let preview = store
        .stage(
            Some(&space),
            SessionSpaceMutation::BindWorkingSurface {
                binding: Box::new(SessionSpaceWorkingSurfaceBinding {
                    binding: binding.clone(),
                    surface: surface.clone(),
                    agent_session: agent.clone(),
                    provider: provider.clone(),
                    plan: herdr_plan,
                    plan_key: "main/shell".into(),
                    provenance: vec!["tm02-r herdr room".into()],
                }),
            },
        )
        .unwrap();
    store.apply(&preview).unwrap();

    // The persisted state round-trips the open name byte-identically.
    let state = store.load(&space).unwrap();
    let persisted = state.working_surfaces.get(&binding).unwrap();
    assert_eq!(
        persisted.plan.mux.as_ref().map(ToString::to_string),
        Some("herdr".to_string())
    );

    // Observing is honest: no provider observation (nothing drives herdr),
    // and the reading names the declared technology.
    let observed = observe(&state, &binding).unwrap();
    assert!(observed.reading.provider_observation.is_none());
    assert!(observed.outcome.is_none());
    assert!(
        observed
            .reading
            .provenance
            .iter()
            .any(|line| line.contains("herdr") && line.contains("place technology"))
    );

    // Opening reaches the provider boundary and comes back as the typed
    // declared-unsupported outcome, naming herdr and what would support it.
    let opened = open(&state, &binding).unwrap();
    match opened.outcome.expect("open returns a provider outcome") {
        WorkingEnvironmentOutcome::NotExposed {
            provider: not_exposed_provider,
            reason,
            ..
        } => {
            assert_eq!(not_exposed_provider, provider);
            assert!(
                reason.contains("herdr") && reason.contains("adapter"),
                "the reason must name the technology and what would support it: {reason}"
            );
        }
        other => panic!("herdr must be declared unsupported, not silently served: {other:?}"),
    }
}
