mod common;

use common::*;

use aikit_core::resource::ResourceRef;
use aikit_core::ScopeKind;
use aikit_tui::{
    selected_contextual_action, ActivationIntent, ApplicationService, PresentationMode,
    TuiApplicationService, TuiRuntime, TuiState, UiAction,
};

#[test]
fn resource_action_compose_explain_and_relations_share_one_identity_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Fixture::new(
        dir.path(),
        vec![skill("skill/rust/review"), script("script/ops/deploy")],
    );
    let resource = ResourceRef::parse("skill/rust/review").unwrap();

    {
        let mut service = ApplicationService::new(&mut backend);
        let mut runtime = TuiRuntime::new();
        let mut state = TuiState::default();

        // Quick and Workspace are two presentations over the same ResourceRef-native
        // search model. No row/capsule identity is introduced by the surface.
        state = runtime
            .step(&mut service, state, UiAction::SetQuery("review".into()))
            .unwrap();
        assert!(state.read_model.contains(&resource));

        state = runtime
            .step(&mut service, state, UiAction::Select(resource.clone()))
            .unwrap();
        assert_eq!(state.selected.as_ref(), Some(&resource));
        assert!(state
            .contextual_actions
            .iter()
            .any(|action| action.action.as_str() == "action/capability/toggle"));

        state = runtime
            .step(
                &mut service,
                state,
                UiAction::SetPresentation(PresentationMode::Workspace),
            )
            .unwrap();
        assert_eq!(state.selected.as_ref(), Some(&resource));

        // The contextual Action produces staged semantic intent; it does not bypass
        // preview/apply or own a second mutation implementation.
        state = runtime
            .step(&mut service, state, UiAction::BeginActionSearch)
            .unwrap();
        state = runtime
            .step(
                &mut service,
                state,
                UiAction::SetActionQuery("toggle".into()),
            )
            .unwrap();
        let action = selected_contextual_action(&state).expect("toggle Action should be searchable");
        assert_eq!(action.action.as_str(), "action/capability/toggle");
        state = runtime
            .step(
                &mut service,
                state,
                UiAction::InvokeAction(action.action.clone()),
            )
            .unwrap();
        assert_eq!(state.staged.get(&resource), Some(ActivationIntent::Enable));
        assert!(state.preview.is_none());

        state = runtime
            .step(
                &mut service,
                state,
                UiAction::SetMutationScope(ScopeKind::Project),
            )
            .unwrap();
        state = runtime
            .step(&mut service, state, UiAction::RequestCompositionPreview)
            .unwrap();
        assert!(state.preview.is_some());
        assert_eq!(state.staged.get(&resource), Some(ActivationIntent::Enable));

        // RequestApply only opens confirmation. The reducer owns this boundary and
        // emits no ApplyComposition effect until ConfirmApply.
        state = runtime
            .step(&mut service, state, UiAction::RequestApply)
            .unwrap();
        assert_eq!(state.overlay, Some(aikit_tui::Overlay::ConfirmApply));
        assert_eq!(state.staged.get(&resource), Some(ActivationIntent::Enable));

        state = runtime
            .step(&mut service, state, UiAction::ConfirmApply)
            .unwrap();
        assert!(state.staged.is_empty());
        assert!(state
            .status
            .as_ref()
            .is_some_and(|status| status.message.contains("applied generation")));

        // Explain and relation navigation still resolve the same stable ResourceRef
        // after mutation rather than falling back to package identity.
        let explanation = service.explain(&resource).unwrap();
        assert_eq!(explanation["resource"], resource.as_str());
        assert!(explanation.get("resolutionHash").is_some());
        assert!(explanation["packageCapabilityState"].is_object());

        let relations = service.relations(&resource).unwrap();
        assert_eq!(relations.subject, resource);
        let actions = relations.value["contextualActions"]
            .as_array()
            .expect("relation model must expose contextual Actions");
        assert!(actions.iter().any(|action| {
            action.get("action").and_then(|value| value.as_str())
                == Some("action/capability/explain")
        }));
        assert!(actions.iter().any(|action| {
            action.get("action").and_then(|value| value.as_str())
                == Some("action/capability/toggle")
        }));
    }

    // The adapter held the one mutable backend borrow for the whole user flow;
    // once that lexical scope ends we can prove exactly one durable apply reached
    // the backend, and only after the confirmation path above.
    assert_eq!(backend.applied.len(), 1);
}
