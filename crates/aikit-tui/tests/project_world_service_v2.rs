mod common;

use common::*;

use aikit_tui::{PaletteApplicationService, ProjectWorldApplicationService};

#[test]
fn shared_application_service_discloses_project_local_world_without_tui_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![skill("skill/rust/review"), script("script/ops/deploy")],
    );

    let service = PaletteApplicationService::new(&mut backend);
    let world = service.project_world().unwrap();

    assert_eq!(world.context.project_root.as_deref(), Some(dir.path()));
    assert_eq!(world.project.root.as_deref(), Some(dir.path()));
    assert!(world
        .capability_horizon
        .capabilities
        .iter()
        .any(|resource| resource.resource.as_str() == "skill/rust/review"));
    assert!(world
        .warnings
        .iter()
        .any(|warning| warning.contains("empty scope layers")));
}
