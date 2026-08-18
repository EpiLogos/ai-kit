use std::fs;

use aikit_cli::app::Service;
use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::{HistoryKind, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF};
use aikit_store::AikitHome;
use aikit_tui::application::TuiApplicationService;
use aikit_tui::application::{
    reduce_tui, ActionOutcome, ActivationIntent, ResourceListItem, TuiState, UiAction,
    WorkspaceSection,
};
use aikit_tui::application_service::ApplicationService;
use aikit_tui::ExplainHistoryApplicationService;
use tempfile::TempDir;

fn open_service(temp: &TempDir) -> Service {
    Service::open(
        AikitHome::at(temp.path().join("aikit-home")),
        temp.path(),
        |_| None,
    )
    .expect("open production application service")
}

fn write_knowledge(temp: &TempDir) {
    fs::write(
        temp.path().join("semantic-wiki.json"),
        r#"{"objects":[{"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:history-auth","revision":4,"provenance":[{"source_ref":"source:history-auth","source_revision":"r4"}],"type":"Concept","title":"History authentication","space_refs":[],"source_refs":["source:history-auth"]}]}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("source-history.json"),
        r#"{"binding":{"source":"source:history-auth","revision":"r4","title":"History auth source","tags":["history","auth"],"visibility":"public","owners":[],"media_type":"text/markdown","metadata":{}},"body":"History evidence keeps provider truth and learned use separate."}"#,
    )
    .unwrap();
}

#[test]
fn common_explain_history_uses_live_knowledge_receipts_actions_and_read_only_navigation() {
    let temp = TempDir::new().unwrap();
    write_knowledge(&temp);
    let mut service = open_service(&temp);

    let search = service.knowledge_search("history", 50).unwrap();
    let wiki = search
        .hits
        .iter()
        .find(|hit| hit.resource.as_str() == "wiki:node:history-auth")
        .unwrap()
        .address
        .clone();
    let source = search
        .hits
        .iter()
        .find(|hit| hit.resource.as_str() == "source:history-auth")
        .unwrap()
        .address
        .clone();
    service
        .knowledge_route(Some("history evidence"), &[wiki.clone(), source.clone()])
        .unwrap();
    service
        .knowledge_frame(Some("history evidence"), &[wiki, source])
        .unwrap();

    let source_ref = ResourceRef::parse("source:history-auth").unwrap();
    let mut application = ApplicationService::new(&mut service);

    let history = application.history_evidence(Some(&source_ref)).unwrap();
    assert!(history.entries.iter().any(|entry| {
        entry.kind == HistoryKind::KnowledgeRoute
            && entry.authorities.contains(&SourceAuthority::Generated)
            && entry.authorities.contains(&SourceAuthority::Observed)
    }));
    assert!(history.entries.iter().any(|entry| {
        entry.kind == HistoryKind::KnowledgeFrame
            && entry.authorities.contains(&SourceAuthority::Generated)
            && entry.canonical_refs.contains(&source_ref)
    }));
    assert!(history.entries.iter().any(|entry| {
        entry.kind == HistoryKind::KnowledgeRoute
            && entry.authorities == vec![SourceAuthority::Learned]
    }));

    let explain = application.explain_evidence(&source_ref).unwrap();
    let reading = explain
        .facts
        .iter()
        .find(|fact| fact.relation == "knowledge-reading")
        .expect("provider-bearing Knowledge reading is common Explain evidence");
    assert_eq!(reading.authority, Some(SourceAuthority::Observed));
    assert!(reading
        .provenance
        .iter()
        .any(|origin| origin.provider.is_some() && origin.lens.as_deref() == Some("source-pool")));
    let learned_route = explain
        .facts
        .iter()
        .find(|fact| fact.relation == "learned-route-accessibility")
        .expect("route use remains distinct learned accessibility evidence");
    assert_eq!(learned_route.authority, Some(SourceAuthority::Learned));
    assert!(learned_route
        .canonical_refs
        .iter()
        .any(|resource| resource.as_str().starts_with("knowledge-route:")));
    assert!(!explain
        .facts
        .iter()
        .any(|fact| fact.relation == "learned-accessibility"));

    let actions = TuiApplicationService::contextual_actions(&application, &source_ref).unwrap();
    assert!(actions
        .iter()
        .any(|action| action.action.as_str() == EXPLAIN_ACTION_REF));
    let history_action = actions
        .iter()
        .find(|action| action.action.as_str() == HISTORY_ACTION_REF)
        .unwrap()
        .clone();
    let outcome = TuiApplicationService::invoke_action(&mut application, &history_action).unwrap();
    assert!(matches!(outcome, ActionOutcome::History { .. }));

    let mut state = TuiState {
        selected: Some(source_ref.clone()),
        ..TuiState::default()
    };
    state.read_model.resources.push(ResourceListItem {
        resource: source_ref,
        kind: aikit_core::resource::ResourceKind::KnowledgeSource,
        label: "History auth source".into(),
        summary: "fixture".into(),
    });
    state.staged.stage(
        ResourceRef::parse("skill/test/keep-staged").unwrap(),
        ActivationIntent::Enable,
    );
    let staged_before = state.staged.len();
    let reduced = reduce_tui(state, UiAction::ActionFinished(outcome));
    assert_eq!(reduced.state.workspace_section, WorkspaceSection::History);
    assert_eq!(reduced.state.staged.len(), staged_before);
    assert!(reduced.state.overlay.is_none());
}
