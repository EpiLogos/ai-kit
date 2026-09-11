//! W6 acceptance: the same canonical subject reached through two terminal
//! environment projections, with capabilities reported as the providers report
//! them and provider-native ids kept out of identity.

use aikit_core::resource::ResourceRef;
use aikit_core::working_environment::{
    NativeBindingKind, ProviderNativeBinding, WorkingEnvironmentCapabilities,
    WorkingEnvironmentHealth, WorkingEnvironmentObservation, WORKING_ENVIRONMENT_PROVIDER_VERSION,
};
use aikit_tui::application::{reduce_tui, TuiState, UiAction, UiEffect};
use aikit_tui::live_field::{
    action_ref, live_working_field, parse_action_ref, reach_for, working_environment_actions,
    ReachWithheld, WorkingEnvironmentOperation,
};

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn capabilities(open: bool, focus: bool) -> WorkingEnvironmentCapabilities {
    WorkingEnvironmentCapabilities {
        discover: true,
        open,
        focus,
        select: true,
        multi_project: true,
        terminal_surface: true,
        surface_attach_detach: true,
        ..WorkingEnvironmentCapabilities::default()
    }
}

/// One provider's observation, bound to `subject` under its own native id.
fn observation(
    provider: &str,
    native_id: &str,
    health: WorkingEnvironmentHealth,
    capabilities: WorkingEnvironmentCapabilities,
    subject: Option<&str>,
) -> WorkingEnvironmentObservation {
    WorkingEnvironmentObservation {
        schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
        provider: r(provider),
        provider_version: Some("test".into()),
        health,
        capabilities,
        bindings: vec![ProviderNativeBinding {
            kind: NativeBindingKind::Surface,
            native_id: native_id.into(),
            canonical_ref: subject.map(r),
            provenance: vec![format!("{provider} reported {native_id}")],
        }],
        focused_native_id: None,
        provenance: vec![format!("{provider} observed")],
    }
}

const SUBJECT: &str = "surface/terminal/main/shell";

fn both_muxes() -> Vec<WorkingEnvironmentObservation> {
    vec![
        observation(
            "provider/tmux/current",
            "%12",
            WorkingEnvironmentHealth::Healthy,
            capabilities(true, true),
            Some(SUBJECT),
        ),
        observation(
            "provider/cmux/current",
            "surface-3",
            WorkingEnvironmentHealth::Healthy,
            capabilities(true, true),
            Some(SUBJECT),
        ),
    ]
}

#[test]
fn one_canonical_subject_is_reachable_through_two_terminal_projections() {
    let field = live_working_field(&both_muxes(), &[r(SUBJECT)]);

    // One subject, not two: the canonical Ref is the identity, and the two
    // native ids are two ways of finding the same thing.
    assert_eq!(field.subjects.len(), 1);
    let reach = field.subject(&r(SUBJECT)).expect("subject in field");
    assert_eq!(reach.projections.len(), 2);
    assert!(reach.multiply_projected());
    assert_eq!(reach.openable().count(), 2);
    assert_eq!(reach.focusable().count(), 2);

    let natives: Vec<&str> = reach
        .projections
        .iter()
        .filter_map(|projection| projection.native_id.as_deref())
        .collect();
    assert!(natives.contains(&"%12") && natives.contains(&"surface-3"));

    // Both providers offer both operations, and each Action names its provider.
    let actions = working_environment_actions(&field, &r(SUBJECT)).unwrap();
    let refs: Vec<String> = actions
        .iter()
        .map(|action| action.action.to_string())
        .collect();
    assert_eq!(refs.len(), 4);
    for expected in [
        "action/working-environment/open/tmux",
        "action/working-environment/focus/tmux",
        "action/working-environment/open/cmux",
        "action/working-environment/focus/cmux",
    ] {
        assert!(refs.iter().any(|got| got == expected), "missing {expected}");
    }
    // Every Action stays on the one canonical subject.
    assert!(actions.iter().all(|action| action.subject == r(SUBJECT)));
}

#[test]
fn a_provider_native_id_never_becomes_a_canonical_subject() {
    let unbound = vec![observation(
        "provider/tmux/current",
        "%99",
        WorkingEnvironmentHealth::Healthy,
        capabilities(true, true),
        None,
    )];
    let field = live_working_field(&unbound, &[]);

    // The provider is observed and says so; nothing is reachable through it,
    // because nothing canonical was bound to that pane.
    assert_eq!(field.observed.len(), 1);
    assert_eq!(field.observed[0].bound_subjects, 0);
    assert!(field.subjects.is_empty());
    assert!(working_environment_actions(&field, &r(SUBJECT))
        .unwrap()
        .is_empty());
}

#[test]
fn capabilities_are_reported_as_the_provider_reports_them() {
    let mixed = vec![
        // Claims focus but not open.
        observation(
            "provider/tmux/current",
            "%12",
            WorkingEnvironmentHealth::Healthy,
            capabilities(false, true),
            Some(SUBJECT),
        ),
        // Healthy claims, but the provider itself is down.
        observation(
            "provider/cmux/current",
            "surface-3",
            WorkingEnvironmentHealth::Unavailable,
            capabilities(true, true),
            Some(SUBJECT),
        ),
    ];
    let field = live_working_field(&mixed, &[r(SUBJECT)]);
    let reach = field.subject(&r(SUBJECT)).unwrap();

    let tmux = reach.projection(&r("provider/tmux/current")).unwrap();
    assert_eq!(tmux.open, Some(ReachWithheld::CapabilityNotClaimed));
    assert!(tmux.can_focus());

    let cmux = reach.projection(&r("provider/cmux/current")).unwrap();
    assert_eq!(cmux.open, Some(ReachWithheld::ProviderUnavailable));
    assert_eq!(cmux.focus, Some(ReachWithheld::ProviderUnavailable));

    // Exactly one operation survives, so the subject is not multiply
    // projected — the predicate does not count claims, it counts what works.
    assert!(!reach.multiply_projected());
    let refs: Vec<String> = working_environment_actions(&field, &r(SUBJECT))
        .unwrap()
        .iter()
        .map(|action| action.action.to_string())
        .collect();
    assert_eq!(refs, vec!["action/working-environment/focus/tmux"]);
}

#[test]
fn a_withheld_operation_is_refused_before_it_reaches_a_provider() {
    let field = live_working_field(
        &[observation(
            "provider/tmux/current",
            "%12",
            WorkingEnvironmentHealth::Healthy,
            capabilities(false, true),
            Some(SUBJECT),
        )],
        &[r(SUBJECT)],
    );
    let state = TuiState {
        live_field: Some(field),
        ..TuiState::default()
    };

    let reduction = reduce_tui(
        state,
        UiAction::ActInWorkingEnvironment {
            provider: r("provider/tmux/current"),
            subject: r(SUBJECT),
            operation: WorkingEnvironmentOperation::Open,
        },
    );

    // No effect was emitted, so no provider was asked to do what it cannot,
    // and the operator is told which condition failed.
    assert!(reduction.effects.is_empty());
    let status = reduction.state.status.expect("a refusal says why").message;
    assert!(
        status.contains("does not claim the open capability"),
        "unhelpful refusal: {status}"
    );
}

#[test]
fn a_permitted_operation_becomes_exactly_one_effect() {
    let state = TuiState {
        live_field: Some(live_working_field(&both_muxes(), &[r(SUBJECT)])),
        ..TuiState::default()
    };

    let reduction = reduce_tui(
        state,
        UiAction::ActInWorkingEnvironment {
            provider: r("provider/cmux/current"),
            subject: r(SUBJECT),
            operation: WorkingEnvironmentOperation::Focus,
        },
    );

    assert_eq!(
        reduction.effects,
        vec![UiEffect::ActInWorkingEnvironment {
            provider: r("provider/cmux/current"),
            subject: r(SUBJECT),
            operation: WorkingEnvironmentOperation::Focus,
        }]
    );
}

#[test]
fn an_action_ref_resolves_only_back_to_a_provider_that_was_observed() {
    let field = live_working_field(&both_muxes(), &[r(SUBJECT)]);

    for (provider, operation) in [
        ("provider/tmux/current", WorkingEnvironmentOperation::Open),
        ("provider/cmux/current", WorkingEnvironmentOperation::Focus),
    ] {
        let action = action_ref(&r(provider), operation).unwrap();
        let (resolved, resolved_operation) =
            parse_action_ref(&action, &field).expect("round trip resolves");
        assert_eq!(resolved, r(provider));
        assert_eq!(resolved_operation, operation);
    }

    // A well-formed Action naming a provider nobody observed resolves to
    // nothing rather than to an invented provider Ref.
    let stale = ResourceRef::parse("action/working-environment/open/herdr").unwrap();
    assert!(parse_action_ref(&stale, &field).is_none());

    // And an Action from another family is not claimed by this parser.
    let other = ResourceRef::parse("action/explain/resource").unwrap();
    assert!(parse_action_ref(&other, &field).is_none());
}

#[test]
fn reach_for_names_the_condition_rather_than_failing_blankly() {
    let field = live_working_field(&both_muxes(), &[r(SUBJECT)]);

    // Unbound subject.
    let error = reach_for(
        &field,
        &r("provider/tmux/current"),
        &r("surface/terminal/main/absent"),
        WorkingEnvironmentOperation::Open,
    )
    .unwrap_err();
    assert_eq!(error.code(), "tui.live_field.subject_absent");

    // Bound subject, provider that has no binding for it.
    let error = reach_for(
        &field,
        &r("provider/herdr/current"),
        &r(SUBJECT),
        WorkingEnvironmentOperation::Open,
    )
    .unwrap_err();
    assert_eq!(error.code(), "tui.live_field.provider_not_bound");
}

#[test]
fn nobody_looked_and_nothing_is_running_stay_different_facts() {
    // A backend that cannot observe answers None; the reducer carries that
    // through rather than flattening it to an empty field.
    let carried = reduce_tui(
        TuiState::default(),
        UiAction::LiveWorkingFieldObserved(None),
    );
    assert!(carried.state.live_field.is_none());

    // A caller that looked and found no mux answers an empty field, which is
    // a confirmed negative and reads as one.
    let looked = reduce_tui(
        TuiState::default(),
        UiAction::LiveWorkingFieldObserved(Some(live_working_field(&[], &[]))),
    );
    let field = looked.state.live_field.expect("an observed empty field");
    assert!(field.is_empty());
    assert!(field.subjects.is_empty());
}

/// The state every host is in before anything has been started: the plan
/// defines panes, no provider has any of them live. Open must still be offered
/// — it is the operation that makes them live — and focus must not be.
#[test]
fn a_subject_that_is_not_live_yet_can_still_be_opened_but_not_focused() {
    let nothing_live = vec![
        observation(
            "provider/tmux/current",
            "%unused",
            WorkingEnvironmentHealth::Healthy,
            capabilities(true, true),
            None,
        ),
        observation(
            "provider/cmux/current",
            "unused",
            WorkingEnvironmentHealth::Healthy,
            capabilities(true, true),
            None,
        ),
    ];
    let field = live_working_field(&nothing_live, &[r(SUBJECT)]);

    let reach = field
        .subject(&r(SUBJECT))
        .expect("the plan's subject is in the field before it is live");
    assert_eq!(reach.projections.len(), 2);
    assert!(reach.projections.iter().all(|p| p.native_id.is_none()));

    // Openable through both; focusable through neither, for a reason that is
    // about liveness rather than capability.
    assert_eq!(reach.openable().count(), 2);
    assert_eq!(reach.focusable().count(), 0);
    assert!(reach.multiply_projected());
    for projection in &reach.projections {
        assert_eq!(projection.focus, Some(ReachWithheld::NotBound));
    }

    let refs: Vec<String> = working_environment_actions(&field, &r(SUBJECT))
        .unwrap()
        .iter()
        .map(|action| action.action.to_string())
        .collect();
    assert_eq!(
        refs,
        vec![
            "action/working-environment/open/cmux",
            "action/working-environment/open/tmux",
        ]
    );
}

/// A plan subject and a live binding for the same canonical Ref are one row,
/// not two, and the live half supplies the native id.
#[test]
fn a_plan_subject_and_its_live_binding_are_the_same_row() {
    let field = live_working_field(&both_muxes(), &[r(SUBJECT)]);
    assert_eq!(field.subjects.len(), 1);
    let reach = field.subject(&r(SUBJECT)).unwrap();
    assert_eq!(reach.projections.len(), 2);
    assert!(reach.projections.iter().all(|p| p.native_id.is_some()));
}
