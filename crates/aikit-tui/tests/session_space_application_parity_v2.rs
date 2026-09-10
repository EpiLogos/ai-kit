use aikit_core::resource::ResourceRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceFocus, SessionSpaceMutation,
};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use aikit_tui::SessionSpaceApplicationAdapter;

#[test]
fn tui_adapter_projects_the_same_preview_receipt_and_read_model() {
    let dir = tempfile::tempdir().unwrap();
    let home = AikitHome::at(dir.path());
    home.ensure_layout().unwrap();
    let store = SessionSpaceApplicationStore::new(home);
    let adapter = SessionSpaceApplicationAdapter::new(&store);

    let space = SessionSpaceRef::parse("session-space/parity").unwrap();
    let create = adapter
        .stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: Some("parity".into()),
            },
        )
        .unwrap();
    let create_receipt = adapter.apply(&create).unwrap();
    assert_eq!(create_receipt.after, store.load(&space).unwrap());

    let focus = SessionSpaceMutation::Focus {
        focus: Some(SessionSpaceFocus {
            target: ResourceRef::parse("surface/editor").unwrap(),
            region: Some("primary".into()),
            provenance: vec!["TUI application adapter".into()],
        }),
    };
    let direct_preview = store.stage(Some(&space), focus.clone()).unwrap();
    let tui_preview = adapter.stage(Some(&space), focus).unwrap();
    assert_eq!(tui_preview, direct_preview);

    let receipt = adapter.apply(&tui_preview).unwrap();
    let projected = adapter.show(&space).unwrap();
    assert_eq!(projected, serde_json::to_value(&receipt.after).unwrap());
    assert_eq!(adapter.history(&space).unwrap().as_array().unwrap().len(), 2);
}