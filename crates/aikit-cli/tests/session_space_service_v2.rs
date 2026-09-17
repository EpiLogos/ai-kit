use aikit_cli::app::Service;
use aikit_cli::SessionSpaceServiceOps;
use aikit_core::resource::ResourceRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{SessionSpaceFocus, SessionSpaceMutation};
use aikit_store::AikitHome;
use tempfile::TempDir;

fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    Service::open(home, temp.path(), |_| None).expect("open canonical application Service")
}

#[test]
fn canonical_service_stages_applies_reopens_and_explains_session_space() {
    let temp = TempDir::new().unwrap();
    let service = open_service(&temp);
    let space = SessionSpaceRef::parse("session-space/service-parity").unwrap();

    let create = service
        .session_space_stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: Some("service parity".into()),
            },
        )
        .unwrap();
    let created = service.session_space_apply(&create).unwrap();
    assert_eq!(created.after, service.session_space_show(&space).unwrap());

    let focus = service
        .session_space_stage(
            Some(&space),
            SessionSpaceMutation::Focus {
                focus: Some(SessionSpaceFocus {
                    target: ResourceRef::parse("surface/editor").unwrap(),
                    region: Some("primary".into()),
                    provenance: vec!["canonical Service".into()],
                }),
            },
        )
        .unwrap();
    let focused = service.session_space_apply(&focus).unwrap();
    assert_eq!(
        focused.after.focus.as_ref().unwrap().region.as_deref(),
        Some("primary")
    );

    drop(service);
    let reopened = open_service(&temp);
    assert_eq!(reopened.session_space_open(&space).unwrap(), focused.after);
    let evidence = reopened.session_space_explain(&space, None).unwrap();
    assert_eq!(evidence.latest_receipt.as_ref().unwrap().sequence, 1);
    assert_eq!(evidence.explanation.semantic_revision, 1);
}
