//! The interactive tree is the same reducer rendered on a real ratatui terminal.
//!
//! These are host tests, not reducer unit tests: scripted terminal events enter
//! the production event loop and the resulting outcome/state is observed.

use aikit_core::CapsuleId;
use aikit_tui::event::{PaletteEvent, ScriptedEvents};
use aikit_tui::host::UiHost;
use aikit_tui::layout::Glyphs;
use aikit_tui::tree::{Node, NodeKind, Root, TreeState};
use aikit_tui::tree_driver::{event_loop, TreeController, TreeOutcome, TreeRequest, TreeStep};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn id(value: &str) -> CapsuleId {
    value.parse().unwrap()
}

fn state() -> TreeState {
    TreeState::new(vec![Node::branch(
        NodeKind::Root(Root::Kinds),
        "two capabilities",
        vec![
            Node::leaf(
                NodeKind::Capability {
                    id: id("skill/demo/one"),
                },
                "first",
            ),
            Node::leaf(
                NodeKind::Capability {
                    id: id("skill/demo/two"),
                },
                "second",
            ),
        ],
    )])
}

fn set_state() -> TreeState {
    TreeState::new(vec![Node::branch(
        NodeKind::Root(Root::Sets),
        "one set",
        vec![Node::branch(
            NodeKind::Set {
                name: "old".into(),
                observed: false,
            },
            "writable",
            vec![],
        )],
    )])
}

fn request() -> TreeRequest {
    TreeRequest::new(UiHost::Fullscreen)
}

#[test]
fn tree_controller_returns_to_the_palette_without_losing_navigation_state() {
    let mut controller = TreeController::new(state(), request());

    assert_eq!(
        controller
            .handle(PaletteEvent::Key(KeyEvent::new(
                KeyCode::Right,
                KeyModifiers::NONE,
            )))
            .unwrap(),
        TreeStep::Continue
    );
    controller
        .handle(PaletteEvent::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        )))
        .unwrap();
    let selected = controller.state().selected;

    assert_eq!(
        controller
            .handle(PaletteEvent::Key(KeyEvent::new(
                KeyCode::Char('t'),
                KeyModifiers::CONTROL,
            )))
            .unwrap(),
        TreeStep::Palette
    );
    assert_eq!(controller.state().selected, selected);
}

#[test]
fn tree_controller_dismisses_a_local_prompt_before_returning_to_the_palette() {
    let mut controller = TreeController::new(set_state(), request());

    controller
        .handle(PaletteEvent::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )))
        .unwrap();
    assert_eq!(
        controller
            .handle(PaletteEvent::Key(KeyEvent::new(
                KeyCode::Esc,
                KeyModifiers::NONE,
            )))
            .unwrap(),
        TreeStep::Continue,
        "the first Escape dismisses the create-set prompt"
    );
    assert_eq!(
        controller
            .handle(PaletteEvent::Key(KeyEvent::new(
                KeyCode::Esc,
                KeyModifiers::NONE,
            )))
            .unwrap(),
        TreeStep::Palette,
        "only a resting tree returns to the palette"
    );
}

#[test]
fn keyboard_can_expand_stage_and_apply_the_selected_capability() {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut events = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
    ]);

    let outcome = event_loop(&mut terminal, &mut events, state(), request()).unwrap();

    assert_eq!(
        outcome,
        TreeOutcome::Apply(vec![id("skill/demo/one")]),
        "Ctrl-Enter hands the exact staged set back to the host"
    );
}

#[test]
fn mouse_click_selects_the_same_row_and_mouse_wheel_navigates() {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut events = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        // Tree content begins inside the border at row 1. Row 2 is the first
        // capability after the root.
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 4,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
    ]);

    let outcome = event_loop(&mut terminal, &mut events, state(), request()).unwrap();

    assert_eq!(
        outcome,
        TreeOutcome::Apply(vec![id("skill/demo/two")]),
        "click selects the first leaf, wheel-down moves to the second, and Ctrl-S applies"
    );
}

#[test]
fn clicking_the_rendered_checkbox_stages_the_capability() {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut events = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        // Child row: inner x=1, indent=2, marker+space=2, so `[ ]` begins at x=5.
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
    ]);

    let outcome = event_loop(&mut terminal, &mut events, state(), request()).unwrap();

    assert_eq!(outcome, TreeOutcome::Apply(vec![id("skill/demo/one")]));
}

#[test]
fn a_durable_scope_needs_a_second_explicit_confirmation() {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut events = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    ]);
    let request = request().with_apply_confirmation(
        "Write 1 change to the global profile?",
        "The global profile affects every project.",
    );

    let outcome = event_loop(&mut terminal, &mut events, state(), request).unwrap();

    assert_eq!(outcome, TreeOutcome::Apply(vec![id("skill/demo/one")]));
}

#[test]
fn mouse_can_apply_and_confirm_a_durable_change() {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut events = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 17,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 17,
            modifiers: KeyModifiers::NONE,
        }),
    ]);
    let request = request().with_apply_confirmation(
        "Write 1 change to the global profile?",
        "The global profile affects every project.",
    );

    let outcome = event_loop(&mut terminal, &mut events, state(), request).unwrap();

    assert_eq!(outcome, TreeOutcome::Apply(vec![id("skill/demo/one")]));
}

#[test]
fn slash_filter_is_edited_inside_the_real_loop_and_escape_closes() {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut events = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    ]);

    let outcome = event_loop(&mut terminal, &mut events, state(), request()).unwrap();

    assert_eq!(outcome, TreeOutcome::Closed);
}

#[test]
fn ascii_mode_folds_the_entire_interactive_frame_not_only_tree_rows() {
    let mut state = state();
    state.roots[0].summary = "paśyantī · ⚠".into();
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut events = ScriptedEvents::keys([KeyCode::Esc]);

    event_loop(
        &mut terminal,
        &mut events,
        state,
        request().with_glyphs(Glyphs::ascii()),
    )
    .unwrap();

    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            assert!(
                buffer[(x, y)].symbol().is_ascii(),
                "non-ASCII interactive cell at ({x},{y}): {:?}",
                buffer[(x, y)].symbol()
            );
        }
    }
}

#[test]
fn set_create_rename_and_delete_have_real_prompted_keyboard_paths() {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut create = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    ]);
    assert_eq!(
        event_loop(&mut terminal, &mut create, set_state(), request()).unwrap(),
        TreeOutcome::Effect(aikit_tui::tree::TreeEffect::CreateSet { set: "new".into() })
    );

    let mut rename_state = set_state();
    rename_state.expanded.insert("sets".into());
    rename_state.selected = 1;
    let mut rename = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    ]);
    assert_eq!(
        event_loop(&mut terminal, &mut rename, rename_state.clone(), request()).unwrap(),
        TreeOutcome::Effect(aikit_tui::tree::TreeEffect::RenameSet {
            from: "old".into(),
            to: "new".into()
        })
    );

    let mut delete = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    ]);
    assert_eq!(
        event_loop(&mut terminal, &mut delete, rename_state, request()).unwrap(),
        TreeOutcome::Effect(aikit_tui::tree::TreeEffect::DeleteSet { set: "old".into() })
    );
}

#[test]
fn mouse_footer_controls_open_and_confirm_set_management() {
    let mut rename_state = set_state();
    rename_state.expanded.insert("sets".into());
    rename_state.selected = 1;
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    // Footer x=1. The rendered controls put [rename] at relative columns
    // 16..23, and the prompt puts [confirm] at relative columns 0..8.
    let mut events = ScriptedEvents::new([
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 17,
            row: 17,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE)),
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 17,
            modifiers: KeyModifiers::NONE,
        }),
    ]);

    assert_eq!(
        event_loop(&mut terminal, &mut events, rename_state, request()).unwrap(),
        TreeOutcome::Effect(aikit_tui::tree::TreeEffect::RenameSet {
            from: "old".into(),
            to: "new".into(),
        })
    );
}

#[test]
fn mouse_footer_can_remove_the_selected_capability_from_a_writable_set() {
    let mut state = TreeState::new(vec![Node::branch(
        NodeKind::Root(Root::Sets),
        "one set",
        vec![Node::branch(
            NodeKind::Set {
                name: "old".into(),
                observed: false,
            },
            "writable",
            vec![Node::leaf(
                NodeKind::Capability {
                    id: id("skill/demo/one"),
                },
                "first",
            )],
        )],
    )]);
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/old".into());
    state.selected = 2;
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    // Footer starts at x=1; [remove] occupies relative columns 34..41.
    let mut events = ScriptedEvents::new([PaletteEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 35,
        row: 17,
        modifiers: KeyModifiers::NONE,
    })]);

    assert_eq!(
        event_loop(&mut terminal, &mut events, state, request()).unwrap(),
        TreeOutcome::Effect(aikit_tui::tree::TreeEffect::RemoveFromSet {
            set: "old".into(),
            capsule: id("skill/demo/one"),
        })
    );
}

#[test]
fn mouse_footer_centers_the_selected_row_in_a_long_tree() {
    let children = (0..30)
        .map(|index| {
            Node::leaf(
                NodeKind::Entry {
                    label: format!("entry-{index:02}"),
                    detail: String::new(),
                },
                "",
            )
        })
        .collect();
    let mut state = TreeState::new(vec![Node::branch(
        NodeKind::Root(Root::Kinds),
        "many entries",
        children,
    )]);
    state.expanded.insert("kinds".into());
    state.selected = 20;
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    // [center] occupies relative columns 43..50.
    let mut events = ScriptedEvents::new([
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 44,
            row: 17,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    ]);

    assert_eq!(
        event_loop(&mut terminal, &mut events, state, request()).unwrap(),
        TreeOutcome::Closed
    );
    let centered_line = (0..80)
        .map(|x| terminal.backend().buffer()[(x, 9)].symbol())
        .collect::<String>();
    assert!(
        centered_line.contains("entry-19"),
        "selected row 20 (including the root) should be centered; row 9 was {centered_line:?}"
    );
}

#[test]
fn a_timed_mouse_double_click_activates_the_leaf() {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let click = || {
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 2,
            modifiers: KeyModifiers::NONE,
        })
    };
    let mut events = ScriptedEvents::new([
        PaletteEvent::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        click(),
        click(),
    ]);

    assert_eq!(
        event_loop(&mut terminal, &mut events, state(), request()).unwrap(),
        TreeOutcome::Effect(aikit_tui::tree::TreeEffect::Activate {
            capsule: id("skill/demo/one"),
        })
    );
}

#[test]
fn mouse_dragging_a_capability_onto_a_set_requests_the_same_put_effect() {
    let mut state = TreeState::new(vec![
        Node::branch(
            NodeKind::Root(Root::Sets),
            "one set",
            vec![Node::branch(
                NodeKind::Set {
                    name: "target".into(),
                    observed: false,
                },
                "writable",
                vec![],
            )],
        ),
        Node::branch(
            NodeKind::Root(Root::Kinds),
            "one capability",
            vec![Node::leaf(
                NodeKind::Capability {
                    id: id("skill/demo/one"),
                },
                "first",
            )],
        ),
    ]);
    state.expanded.insert("sets".into());
    state.expanded.insert("kinds".into());
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut events = ScriptedEvents::new([
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 4,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 8,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 8,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }),
    ]);

    assert_eq!(
        event_loop(&mut terminal, &mut events, state, request()).unwrap(),
        TreeOutcome::Effect(aikit_tui::tree::TreeEffect::AddToSet {
            set: "target".into(),
            capsule: id("skill/demo/one"),
        })
    );
}

#[test]
fn dragging_a_member_out_of_its_writable_set_requests_removal() {
    let mut state = TreeState::new(vec![
        Node::branch(
            NodeKind::Root(Root::Sets),
            "one set",
            vec![Node::branch(
                NodeKind::Set {
                    name: "source".into(),
                    observed: false,
                },
                "writable",
                vec![Node::leaf(
                    NodeKind::Capability {
                        id: id("skill/demo/one"),
                    },
                    "first",
                )],
            )],
        ),
        Node::branch(NodeKind::Root(Root::Kinds), "drop target", vec![]),
    ]);
    state.expanded.insert("sets".into());
    state.expanded.insert("sets/source".into());
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut events = ScriptedEvents::new([
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 14,
            row: 3,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 4,
            row: 4,
            modifiers: KeyModifiers::NONE,
        }),
        PaletteEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 4,
            row: 4,
            modifiers: KeyModifiers::NONE,
        }),
    ]);

    assert_eq!(
        event_loop(&mut terminal, &mut events, state, request()).unwrap(),
        TreeOutcome::Effect(aikit_tui::tree::TreeEffect::RemoveFromSet {
            set: "source".into(),
            capsule: id("skill/demo/one"),
        })
    );
}
