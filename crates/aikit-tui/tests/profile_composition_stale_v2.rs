mod common;

use common::*;

use aikit_core::scope::ScopeKind;
use aikit_tui::{ActivationIntent, ApplicationService, StagedChanges, TuiApplicationService};

#[test]
fn production_application_preview_reports_shared_changed_ground() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![skill("skill/rust/review"), skill("skill/rust/other")],
    );
    let mut staged = StagedChanges::default();
    staged.stage(
        aikit_core::resource::ResourceRef::parse("skill/rust/review").unwrap(),
        ActivationIntent::Enable,
    );

    let preview = {
        let service = ApplicationService::new(&mut backend);
        service
            .preview_composition(ScopeKind::Project, &staged)
            .unwrap()
    };

    assert!(preview.summary.contains("changed ground: +1 -0 capability"));
    assert!(preview.revision.contains("=>"));
    assert!(backend.applied.is_empty(), "preview must never write");
}

#[test]
fn production_application_rejects_accepted_preview_after_material_resolution_drift() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![skill("skill/rust/review"), skill("skill/rust/other")],
    );
    let mut staged = StagedChanges::default();
    staged.stage(
        aikit_core::resource::ResourceRef::parse("skill/rust/review").unwrap(),
        ActivationIntent::Enable,
    );

    let preview = {
        let service = ApplicationService::new(&mut backend);
        service
            .preview_composition(ScopeKind::Project, &staged)
            .unwrap()
    };

    // Another actor changes the same application basis after the user/agent has
    // accepted the preview. The original preview must not be applied blindly.
    backend = backend.enable(ScopeKind::Project, &["skill/rust/other"]);

    let error = {
        let mut service = ApplicationService::new(&mut backend);
        service.apply_composition(&preview).unwrap_err()
    };

    assert_eq!(error.code(), "composition.preview_stale");
    assert!(backend.applied.is_empty());
}
