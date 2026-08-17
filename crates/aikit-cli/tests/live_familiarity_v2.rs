use aikit_cli::app::Service;
use aikit_core::resource::ResourceRef;
use aikit_core::{FamiliarityContext, FamiliarityObservation};
use aikit_store::AikitHome;
use aikit_tui::backend::PaletteBackend;
use tempfile::TempDir;

fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    Service::open(home, temp.path(), |_| None).expect("open production application service")
}

#[test]
fn production_service_persists_and_replays_actual_navigation_use() {
    let temp = TempDir::new().unwrap();
    let destination = ResourceRef::parse("wiki:node:auth").unwrap();
    let context = FamiliarityContext {
        project: None,
        actor: None,
        agency: None,
        focus: Some("knowledge-navigation".into()),
    };

    {
        let mut service = open_service(&temp);
        PaletteBackend::record_familiarity(
            &mut service,
            FamiliarityObservation::destination(
                "obs-live-1",
                destination.clone(),
                context.clone(),
                1_000,
            )
            .from_surface(ResourceRef::parse("surface/aikit/tui").unwrap()),
        )
        .unwrap();
    }

    let service = open_service(&temp);
    let learned = PaletteBackend::familiarity(&service)
        .unwrap()
        .expect("production service should replay learned accessibility");
    let assessment = learned.assess_destination(
        &destination,
        &context,
        1_000,
        aikit_core::DEFAULT_FAMILIARITY_HALF_LIFE_MS,
    );
    assert_eq!(assessment.observations, 1);
    assert_eq!(assessment.contextual_observations, 1);
}
