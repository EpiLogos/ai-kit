use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::Result;
use aikit_tui::application::{
    ApplyReceipt, CompositionPreview, HistoryEntry, RelationReadModel, ResourceListItem,
    ResourceListReadModel, StagedChanges, TuiApplicationService, TuiRuntime, TuiState, UiAction,
};
use serde_json::{json, Value};

#[derive(Default)]
struct Service {
    observed: Vec<ResourceRef>,
}

impl TuiApplicationService for Service {
    fn search(&self, _query: &str) -> Result<ResourceListReadModel> {
        Ok(ResourceListReadModel::default())
    }

    fn context_disclosure(&self, resource: &ResourceRef) -> Result<Value> {
        Ok(json!({"resource": resource.as_str()}))
    }

    fn preview_composition(
        &self,
        _scope: aikit_core::scope::ScopeKind,
        _staged: &StagedChanges,
    ) -> Result<CompositionPreview> {
        panic!("composition is outside this navigation test")
    }

    fn apply_composition(&mut self, _preview: &CompositionPreview) -> Result<ApplyReceipt> {
        panic!("composition is outside this navigation test")
    }

    fn explain(&self, resource: &ResourceRef) -> Result<Value> {
        Ok(json!({"resource": resource.as_str()}))
    }

    fn history(&self, _resource: Option<&ResourceRef>) -> Result<Vec<HistoryEntry>> {
        Ok(Vec::new())
    }

    fn relations(&self, resource: &ResourceRef) -> Result<RelationReadModel> {
        Ok(RelationReadModel {
            subject: resource.clone(),
            value: json!({}),
        })
    }

    fn observe_resource_use(&mut self, resource: &ResourceRef) -> Result<()> {
        self.observed.push(resource.clone());
        Ok(())
    }
}

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn state_with(resource: ResourceRef) -> TuiState {
    TuiState {
        read_model: ResourceListReadModel {
            revision: "test".into(),
            resources: vec![ResourceListItem {
                resource,
                kind: ResourceKind::Project,
                label: "AIKit".into(),
                summary: "project".into(),
            }],
        },
        ..TuiState::default()
    }
}

#[test]
fn selection_alone_is_not_learned_but_opening_is() {
    let resource = r("project/aikit");
    let mut service = Service::default();
    let mut runtime = TuiRuntime::new();

    let state = runtime
        .step(
            &mut service,
            state_with(resource.clone()),
            UiAction::Select(resource.clone()),
        )
        .unwrap();
    assert!(
        service.observed.is_empty(),
        "cursor selection is not resource use"
    );

    let state = runtime
        .step(&mut service, state, UiAction::OpenSelection)
        .unwrap();
    assert_eq!(service.observed, vec![resource.clone()]);
    assert_eq!(state.navigation.len(), 1);
    assert_eq!(state.navigation[0].selected.as_ref(), Some(&resource));
}

#[test]
fn open_without_a_selected_resource_records_nothing() {
    let mut service = Service::default();
    let mut runtime = TuiRuntime::new();
    let state = runtime
        .step(&mut service, TuiState::default(), UiAction::OpenSelection)
        .unwrap();

    assert!(service.observed.is_empty());
    assert_eq!(state.navigation.len(), 1);
    assert!(state.navigation[0].selected.is_none());
}
