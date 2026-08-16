mod common;

use common::*;

use aikit_core::project::ProjectBindingLocator;
use aikit_tui::{ApplicationService, ProjectWorldApplicationService};

#[test]
fn shared_application_service_discloses_project_local_world_without_tui_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![skill("skill/rust/review"), script("script/ops/deploy")],
    );
    let expected_root = backend
        .descriptor
        .project_root
        .clone()
        .expect("fixture has a project root");

    let service = ApplicationService::new(&mut backend);
    let world = service.project_world().unwrap();

    assert_eq!(world.context.project_root.as_ref(), Some(&expected_root));
    assert!(matches!(
        &world.project.locator,
        ProjectBindingLocator::LocalDirectory { path } if path == &expected_root
    ));
    assert!(world
        .capability_horizon
        .capabilities
        .iter()
        .any(|resource| resource.resource.as_str() == "skill/rust/review"));
    assert!(world.resolution_basis.scopes.is_empty());
    assert!(world
        .warnings
        .iter()
        .any(|warning| warning.contains("scope-layer stack")));
}
