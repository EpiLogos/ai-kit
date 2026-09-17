use aikit_core::id::CapsuleId;
use aikit_tui::tree::{ascii_fold, render_lines, Node, NodeKind, Root, TreeGlyphs, TreeState};

fn id(raw: &str) -> CapsuleId {
    CapsuleId::parse(raw).unwrap()
}

#[test]
fn compatibility_tree_is_a_read_only_expand_and_filter_projection() {
    let capability = Node::leaf(
        NodeKind::Capability {
            id: id("skill/rust/review"),
        },
        "active",
    );
    let root = Node::branch(
        NodeKind::Root(Root::Kinds),
        "1 catalogued",
        vec![capability],
    );
    let mut state = TreeState::new(vec![root]);

    assert_eq!(state.rows().len(), 1);
    state.expanded.insert("kinds".into());
    assert_eq!(state.rows().len(), 2);
    assert_eq!(state.rows()[1].path, "kinds/skill/rust/review");

    state.filter = "review".into();
    let rows = state.rows();
    assert_eq!(
        rows.len(),
        2,
        "filter preserves the path to a matching child"
    );
}

#[test]
fn compatibility_tree_rendering_has_no_staging_or_mutation_markers() {
    let capability = Node::leaf(
        NodeKind::Capability {
            id: id("skill/rust/review"),
        },
        "active",
    );
    let root = Node::branch(
        NodeKind::Root(Root::Kinds),
        "1 catalogued",
        vec![capability],
    );
    let mut state = TreeState::new(vec![root]);
    state.expanded.insert("kinds".into());

    let rendered = render_lines(&state, TreeGlyphs::ascii()).join("\n");
    assert!(rendered.contains("skill/rust/review"));
    assert!(!rendered.contains("[x]"));
    assert!(!rendered.contains("[ ]"));
}

#[test]
fn ascii_projection_preserves_information_without_unicode_dependency() {
    assert_eq!(ascii_fold("▾ paśyantī → ready ⚠"), "- pa?yant? -> ready !");
}
