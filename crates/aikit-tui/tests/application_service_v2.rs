use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::scope::ScopeKind;
use aikit_core::Result;
use aikit_tui::{
    ActivationIntent, ApplyReceipt, CompositionPreview, HistoryEntry, RelationReadModel,
    ResourceListItem, ResourceListReadModel, StagedChanges, TuiApplicationService, TuiRuntime,
    TuiState, UiAction,
};
use serde_json::{json, Value};

fn resource() -> ResourceRef {
    ResourceRef::parse("factory:capability:alpha").unwrap()
}

#[derive(Default)]
struct FakeService {
    applies: usize,
}

impl TuiApplicationService for FakeService {
    fn search(&self, query: &str) -> Result<ResourceListReadModel> {
        Ok(ResourceListReadModel {
            revision: format!("search:{query}"),
            resources: vec![ResourceListItem {
                resource: resource(),
                kind: ResourceKind::Capability,
                label: "alpha".into(),
                summary: format!("result for {query}"),
            }],
        })
    }

    fn context_disclosure(&self, subject: &ResourceRef) -> Result<Value> {
        Ok(json!({"resource": subject.as_str(), "disclosed": true}))
    }

    fn preview_composition(
        &self,
        scope: ScopeKind,
        staged: &StagedChanges,
    ) -> Result<CompositionPreview> {
        Ok(CompositionPreview {
            revision: "preview-r1".into(),
            scope,
            staged: staged.clone(),
            summary: format!("preview {} staged change(s)", staged.len()),
        })
    }

    fn apply_composition(&mut self, preview: &CompositionPreview) -> Result<ApplyReceipt> {
        self.applies += 1;
        Ok(ApplyReceipt {
            revision: "applied-r1".into(),
            summary: format!("applied {}", preview.summary),
        })
    }

    fn explain(&self, subject: &ResourceRef) -> Result<Value> {
        Ok(json!({"resource": subject.as_str(), "explain": true}))
    }

    fn history(&self, subject: Option<&ResourceRef>) -> Result<Vec<HistoryEntry>> {
        Ok(vec![HistoryEntry {
            id: "history-1".into(),
            summary: subject
                .map(|value| format!("history for {value}"))
                .unwrap_or_else(|| "global history".into()),
        }])
    }

    fn relations(&self, subject: &ResourceRef) -> Result<RelationReadModel> {
        Ok(RelationReadModel {
            subject: subject.clone(),
            value: json!({"edges": []}),
        })
    }
}

#[test]
fn search_effect_is_executed_by_the_application_service_and_returns_as_state() {
    let mut runtime = TuiRuntime::new();
    let mut service = FakeService::default();

    let state = runtime
        .step(
            &mut service,
            TuiState::default(),
            UiAction::SetQuery("alpha".into()),
        )
        .unwrap();

    assert_eq!(state.query, "alpha");
    assert_eq!(state.read_model.revision, "search:alpha");
    assert_eq!(state.read_model.resources.len(), 1);
    assert_eq!(state.read_model.resources[0].resource, resource());
}

#[test]
fn preview_then_confirmation_then_apply_crosses_the_service_boundary_once() {
    let mut runtime = TuiRuntime::new();
    let mut service = FakeService::default();
    let mut state = TuiState::default();
    state.mutation_scope = Some(ScopeKind::Project);
    state.read_model = ResourceListReadModel {
        revision: "r1".into(),
        resources: vec![ResourceListItem {
            resource: resource(),
            kind: ResourceKind::Capability,
            label: "alpha".into(),
            summary: "alpha".into(),
        }],
    };

    state = runtime
        .step(
            &mut service,
            state,
            UiAction::Stage {
                resource: resource(),
                intent: ActivationIntent::Enable,
            },
        )
        .unwrap();
    assert_eq!(service.applies, 0);

    state = runtime
        .step(&mut service, state, UiAction::RequestApply)
        .unwrap();
    assert_eq!(state.preview.as_ref().unwrap().scope, ScopeKind::Project);
    assert!(!state.staged.is_empty());
    assert_eq!(service.applies, 0);

    state = runtime
        .step(&mut service, state, UiAction::RequestApply)
        .unwrap();
    assert_eq!(service.applies, 0);

    state = runtime
        .step(&mut service, state, UiAction::ConfirmApply)
        .unwrap();
    assert_eq!(service.applies, 1);
    assert!(state.staged.is_empty());
    assert!(state.preview.is_none());
    assert!(state
        .status
        .as_ref()
        .unwrap()
        .message
        .starts_with("applied preview"));
}

#[test]
fn read_models_for_context_explain_history_and_relations_remain_service_owned() {
    let service = FakeService::default();
    let subject = resource();

    assert_eq!(
        service.context_disclosure(&subject).unwrap()["resource"],
        subject.as_str()
    );
    assert_eq!(service.explain(&subject).unwrap()["explain"], true);
    assert_eq!(service.history(Some(&subject)).unwrap().len(), 1);
    assert_eq!(service.relations(&subject).unwrap().subject, subject);
}
