use aikit_cli::SessionSpaceCliAdapter;
use aikit_core::resource::ResourceRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{SessionSpaceFocus, SessionSpaceMutation};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};

#[test]
fn cli_adapter_uses_the_same_preview_receipt_and_read_model() {
    let dir = tempfile::tempdir().unwrap();
    let home = AikitHome::at(dir.path());
    home.ensure_layout().unwrap();
    let store = SessionSpaceApplicationStore::new(home);
    let cli = SessionSpaceCliAdapter::new(&store);

    let space = SessionSpaceRef::parse("session-space/cli-parity").unwrap();
    let create = cli
        .stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: Some("cli parity".into()),
            },
        )
        .unwrap();
    let created = cli.apply(&create).unwrap();
    assert_eq!(created.after, store.load(&space).unwrap());
    assert_eq!(cli.open(&space).unwrap(), cli.show(&space).unwrap());

    let focus = SessionSpaceMutation::Focus {
        focus: Some(SessionSpaceFocus {
            target: ResourceRef::parse("surface/editor").unwrap(),
            region: Some("primary".into()),
            provenance: vec!["CLI application adapter".into()],
        }),
    };
    let direct = store.stage(Some(&space), focus.clone()).unwrap();
    let projected = cli.stage(Some(&space), focus).unwrap();
    assert_eq!(projected, direct);

    let receipt = cli.apply(&projected).unwrap();
    assert_eq!(
        cli.show(&space).unwrap(),
        serde_json::to_value(&receipt.after).unwrap()
    );
    assert_eq!(cli.history(&space).unwrap().as_array().unwrap().len(), 2);
}
