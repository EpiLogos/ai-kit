//! Native cross-product proof for the Work surface's Factory entry.

use std::{collections::BTreeMap, path::PathBuf, process::Command};

use aikit_cli::app::Service;
use aikit_core::resource::ResourceKind;
use aikit_store::AikitHome;
use aikit_tui::{
    application::ActionOutcome,
    application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest},
    event::PaletteEvent,
    host::UiHost,
    layout::Glyphs,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use serde_json::Value;

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn alt(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

fn rendered(surface: &ApplicationSurfaceController, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join(".aikit")).unwrap();
    std::fs::write(root.join(".aikit/profile.toml"), "schema = 1\n").unwrap();
    std::fs::create_dir_all(root.join("ProjectCentral")).unwrap();
    std::fs::write(
        root.join("ProjectCentral/project.json"),
        r#"{"schema":"central.project/v1","project_id":"project:aikit-factory-work-proof","human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}"#,
    )
    .unwrap();
}

#[test]
#[ignore = "run by the mandatory exact-Factory conformance CI job"]
fn work_surface_commissions_through_the_real_factory_owner_and_renders_its_readback() {
    let factory = PathBuf::from(
        std::env::var_os("AIKIT_FACTORY_REAL_BIN").expect("AIKIT_FACTORY_REAL_BIN is required"),
    );
    let request = PathBuf::from(
        std::env::var_os("AIKIT_FACTORY_COMMISSION_REQUEST")
            .expect("AIKIT_FACTORY_COMMISSION_REQUEST is required"),
    );
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let project_root = directory.path().join("project");
    let state = directory.path().join("factory-state.json");
    project(&project_root);

    let mut env = BTreeMap::new();
    env.insert(
        "AIKIT_CONTEXT_ID".to_string(),
        aikit_core::ContextId::generate().to_string(),
    );
    env.insert(
        "AIKIT_SESSION_ID".to_string(),
        "ses_FACTORYTUI000000000000".into(),
    );
    env.insert(
        "AIKIT_FACTORY_BIN".to_string(),
        factory.display().to_string(),
    );
    env.insert(
        "AIKIT_FACTORY_STATE".to_string(),
        state.display().to_string(),
    );
    env.insert(
        "AIKIT_FACTORY_REQUEST_FILE".to_string(),
        request.display().to_string(),
    );
    let mut service = Service::open(AikitHome::at(home), &project_root, |key| {
        env.get(key).cloned()
    })
    .unwrap();
    let mut surface = ApplicationSurfaceController::new(
        &mut service,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("work")
            .with_glyphs(Glyphs::unicode()),
    )
    .unwrap();

    let destination = aikit_core::resource::ResourceRef::parse("surface/workspace/work").unwrap();
    let index = surface
        .semantic()
        .read_model
        .position(&destination)
        .expect("Work destination is in the shared Navigator");
    for _ in 0..=index {
        surface.handle(&mut service, key(KeyCode::Down)).unwrap();
    }
    assert_eq!(surface.semantic().selected.as_ref(), Some(&destination));

    surface
        .handle(&mut service, key(KeyCode::Char(':')))
        .unwrap();
    for character in "start factory work".chars() {
        surface
            .handle(&mut service, key(KeyCode::Char(character)))
            .unwrap();
    }
    surface.handle(&mut service, key(KeyCode::Enter)).unwrap();

    let (summary, receipt) = match surface.semantic().action_result.as_ref() {
        Some(ActionOutcome::FactoryWorkStarted { summary, receipt }) => (summary, receipt),
        other => panic!("expected exact Factory work receipt, got {other:?}"),
    };
    assert!(summary.contains("execution remains commissioned-not-executed"));
    assert!(receipt.contains("\"contract\": \"factory.commission-receipt/v1\""));
    assert!(receipt.contains("\"standing\": \"commissioned-not-executed\""));
    assert!(state.is_file());

    let world = surface
        .project_world()
        .expect("Project Work reading remains available");
    assert!(world
        .developmental_work
        .iter()
        .any(|resource| resource.kind == ResourceKind::Journey));
    assert!(world
        .developmental_work
        .iter()
        .any(|resource| resource.kind == ResourceKind::Run));
    for output in [rendered(&surface, 220, 90), rendered(&surface, 82, 110)] {
        assert!(output.contains("DIRECT"));
        assert!(output.contains("FACTORY"));
        assert!(output.contains("ATTENTION"));
        assert!(output.contains("commissioned-not-executed"));
        assert!(output.contains("journey:"));
        assert!(output.contains("run:"));
    }
}

#[test]
#[ignore = "run by the mandatory exact-Factory conformance CI job"]
fn work_surface_renders_real_factory_owner_readings_wide_and_narrow() {
    let factory = PathBuf::from(
        std::env::var_os("AIKIT_FACTORY_REAL_BIN").expect("AIKIT_FACTORY_REAL_BIN is required"),
    );
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("factory-state.json");
    let output = Command::new(&factory)
        .args([
            "conformance",
            "developmental-state",
            state.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        manifest["contract"],
        "factory.developmental-conformance-manifest/v1"
    );
    let project_ref = manifest["projectRef"].as_str().unwrap().to_string();

    let project_root = directory.path().join("project");
    project(&project_root);
    let mut env = BTreeMap::new();
    env.insert(
        "AIKIT_CONTEXT_ID".to_string(),
        aikit_core::ContextId::generate().to_string(),
    );
    env.insert(
        "AIKIT_FACTORY_BIN".to_string(),
        factory.display().to_string(),
    );
    env.insert(
        "AIKIT_FACTORY_STATE".to_string(),
        state.display().to_string(),
    );
    env.insert("AIKIT_FACTORY_PROJECT_REF".to_string(), project_ref);
    let mut service = Service::open(
        AikitHome::at(directory.path().join("home")),
        &project_root,
        |key| env.get(key).cloned(),
    )
    .unwrap();
    let mut surface = ApplicationSurfaceController::new(
        &mut service,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_glyphs(Glyphs::unicode()),
    )
    .unwrap();
    surface.handle(&mut service, alt(KeyCode::Right)).unwrap();
    surface.handle(&mut service, alt(KeyCode::Right)).unwrap();

    let world = surface.project_world().unwrap();
    for kind in [
        ResourceKind::Journey,
        ResourceKind::Run,
        ResourceKind::WorkflowUnit,
    ] {
        assert!(world
            .developmental_work
            .iter()
            .any(|resource| resource.kind == kind));
    }
    for output in [rendered(&surface, 220, 72), rendered(&surface, 82, 110)] {
        assert!(output.contains("DIRECT"));
        assert!(output.contains("FACTORY"));
        assert!(output.contains("ATTENTION"));
        assert!(output.contains("journey:"));
        assert!(output.contains("run:"));
        assert!(output.contains("workflow-unit:"));
        assert!(output.contains("owner r"));
        assert!(output.contains("none in supplied Factory owner readings"));
    }
}
