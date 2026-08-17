use aikit_core::{
    explain_history_actions_for, ResourceRef, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,
};
use aikit_tui::{
    keyboard_invoke_action, mouse_invoke_action, stage_action, NavigationIntent,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

#[test]
fn explain_and_history_are_the_same_actions_through_keyboard_and_mouse() {
    let project = r("project/app");
    let [explain, history] = explain_history_actions_for(&project).unwrap();

    assert_eq!(explain.action.as_str(), EXPLAIN_ACTION_REF);
    assert_eq!(history.action.as_str(), HISTORY_ACTION_REF);
    assert_eq!(keyboard_invoke_action(&explain), mouse_invoke_action(&explain));
    assert_eq!(keyboard_invoke_action(&history), mouse_invoke_action(&history));
    assert_eq!(
        keyboard_invoke_action(&explain),
        NavigationIntent::InvokeAction {
            action: r(EXPLAIN_ACTION_REF),
            subject: project.clone(),
        }
    );
    assert_eq!(
        keyboard_invoke_action(&history),
        NavigationIntent::InvokeAction {
            action: r(HISTORY_ACTION_REF),
            subject: project,
        }
    );
    assert!(stage_action(&explain).is_none());
    assert!(stage_action(&history).is_none());
}

#[test]
fn surface_context_changes_the_subject_not_the_action_identity() {
    let project_actions = explain_history_actions_for(&r("project/app")).unwrap();
    let component_actions = explain_history_actions_for(&r("component/editor")).unwrap();
    let surface_actions = explain_history_actions_for(&r("surface/aikit/tui")).unwrap();

    assert_eq!(project_actions[0].action, component_actions[0].action);
    assert_eq!(component_actions[0].action, surface_actions[0].action);
    assert_eq!(project_actions[1].action, component_actions[1].action);
    assert_eq!(component_actions[1].action, surface_actions[1].action);

    assert_ne!(project_actions[0].subject, component_actions[0].subject);
    assert_ne!(component_actions[0].subject, surface_actions[0].subject);
}
