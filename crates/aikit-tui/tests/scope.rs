//! Where a toggle would be written, and what it costs to write there.
//!
//! The asymmetry tested here is the point of the whole scope model: a session
//! overlay is cheap and disposable, so toggling into it must cost one key. A
//! committed project profile is a shared, reviewable, version-controlled artefact
//! that changes what every colleague's palette resolves, so it must cost a
//! deliberate second act. If the confirmation were skippable — by a flag, by a
//! repeated key, by anything — the two would be the same scope wearing different
//! names.

use aikit_core::context::ContextDescriptor;
use aikit_core::id::{SessionId, ProjectId};
use aikit_core::scope::ScopeKind;
use aikit_tui::scope::ScopeSelector;

fn in_a_session() -> ContextDescriptor {
    let mut descriptor = ContextDescriptor::for_project("/work/payments");
    descriptor.session_id = Some(SessionId::generate());
    descriptor.project_id = Some(ProjectId::generate());
    descriptor
}

fn outside_a_project() -> ContextDescriptor {
    ContextDescriptor {
        project_root: None,
        session_id: None,
        task: None,
        host: String::new(),
        ..ContextDescriptor::for_project("/tmp")
    }
}

// ---------------------------------------------------------------------------
// Where changes go by default
// ---------------------------------------------------------------------------

#[test]
fn the_starting_scope_is_the_one_core_says_belongs_to_this_context() {
    let descriptor = in_a_session();
    let selector = ScopeSelector::for_context(&descriptor);
    assert_eq!(selector.current(), descriptor.default_mutation_scope());
    assert_eq!(selector.current(), ScopeKind::Session);
}

#[test]
fn a_task_context_defaults_to_its_own_overlay_and_not_the_sessions() {
    let mut descriptor = in_a_session();
    descriptor.task = Some("migration-review".into());
    assert_eq!(
        ScopeSelector::for_context(&descriptor).current(),
        ScopeKind::Task
    );
}

// ---------------------------------------------------------------------------
// Tab
// ---------------------------------------------------------------------------

#[test]
fn tab_cycles_only_the_scopes_that_exist_in_this_context() {
    let descriptor = in_a_session();
    let permitted = descriptor.permitted_scopes();
    let mut selector = ScopeSelector::for_context(&descriptor);

    let mut seen = vec![selector.current()];
    for _ in 0..permitted.len() {
        selector.cycle();
        seen.push(selector.current());
    }
    for scope in &seen {
        assert!(
            permitted.contains(scope),
            "{scope} is not a scope this context can write to"
        );
    }
    assert_eq!(
        seen.first(),
        seen.last(),
        "a full cycle must return to where it started"
    );
}

#[test]
fn tab_outside_a_project_has_exactly_one_place_to_go() {
    let descriptor = outside_a_project();
    let mut selector = ScopeSelector::for_context(&descriptor);
    assert_eq!(selector.permitted(), &[ScopeKind::Global]);
    selector.cycle();
    assert_eq!(selector.current(), ScopeKind::Global);
}

#[test]
fn tab_never_offers_the_one_shot_scope_because_it_is_not_a_place_to_write() {
    let descriptor = in_a_session();
    let selector = ScopeSelector::for_context(&descriptor);
    assert!(!selector.permitted().contains(&ScopeKind::OneShot));
    assert!(selector.permitted().iter().all(|s| s.is_mutation_target()));
}

#[test]
fn a_requested_scope_that_does_not_exist_here_is_refused_rather_than_ignored() {
    let descriptor = outside_a_project();
    let error = ScopeSelector::with_scope(&descriptor, ScopeKind::Project)
        .expect_err("there is no project here to write to");
    assert_eq!(error.code(), "scope.unavailable_in_context");
    assert_eq!(error.details().get("scope").map(String::as_str), Some("project"));
}

#[test]
fn a_requested_scope_that_does_exist_here_is_honoured() {
    let descriptor = in_a_session();
    let selector = ScopeSelector::with_scope(&descriptor, ScopeKind::Project).unwrap();
    assert_eq!(selector.current(), ScopeKind::Project);
}

// ---------------------------------------------------------------------------
// The confirmation
// ---------------------------------------------------------------------------

#[test]
fn writing_a_committed_project_profile_demands_a_confirmation() {
    let descriptor = in_a_session();
    let selector = ScopeSelector::with_scope(&descriptor, ScopeKind::Project).unwrap();
    assert!(selector.requires_confirmation());

    let confirmation = selector
        .confirmation(3)
        .expect("a project write must produce a confirmation");
    assert_eq!(confirmation.scope, ScopeKind::Project);
    assert!(
        confirmation.prompt.contains('3'),
        "the prompt must say how much is being written: {}",
        confirmation.prompt
    );
    assert!(
        confirmation.detail.contains("committed"),
        "the user must be told this is the shared, version-controlled file: {}",
        confirmation.detail
    );
}

#[test]
fn writing_a_session_overlay_or_a_private_project_file_does_not() {
    let descriptor = in_a_session();
    for scope in [ScopeKind::Session, ScopeKind::ProjectLocal] {
        let selector = ScopeSelector::with_scope(&descriptor, scope).unwrap();
        assert!(!selector.requires_confirmation(), "{scope} should be cheap");
        assert_eq!(selector.confirmation(3), None);
    }
}

#[test]
fn the_global_profile_is_as_shared_as_the_project_one_and_confirms_too() {
    let descriptor = in_a_session();
    let selector = ScopeSelector::with_scope(&descriptor, ScopeKind::Global).unwrap();
    assert!(selector.requires_confirmation());
    assert!(selector.confirmation(1).is_some());
}

#[test]
fn the_confirmation_matches_cores_own_answer_and_is_not_a_second_opinion() {
    let descriptor = in_a_session();
    for scope in descriptor.permitted_scopes() {
        let selector = ScopeSelector::with_scope(&descriptor, scope).unwrap();
        assert_eq!(
            selector.requires_confirmation(),
            scope.requires_confirmation_to_write(),
            "{scope} disagrees with core"
        );
    }
}

#[test]
fn a_confirmation_names_the_file_that_would_change() {
    let descriptor = in_a_session();
    let selector = ScopeSelector::with_scope(&descriptor, ScopeKind::Project).unwrap();
    let confirmation = selector.confirmation(2).unwrap();
    assert!(
        confirmation.detail.contains(".aikit/profile.toml"),
        "an error or prompt names the thing to edit: {}",
        confirmation.detail
    );
}
