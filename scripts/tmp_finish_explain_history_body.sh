#!/usr/bin/env bash
set -euo pipefail
branch="${GITHUB_HEAD_REF:-${GITHUB_REF_NAME:-agent/explain-history-production-patch}}"
git config user.name github-actions[bot]
git config user.email 41898282+github-actions[bot]@users.noreply.github.com

python3 - <<'PY'
from pathlib import Path

# ------------------------------------------------------------------
# Core evidence grammar: durable frame receipts are not route events.
# ------------------------------------------------------------------
p = Path('crates/aikit-core/src/explain_history.rs')
text = p.read_text()
old = '''    Familiarity,
    KnowledgeRoute,
    Generation,
'''
new = '''    Familiarity,
    KnowledgeRoute,
    KnowledgeFrame,
    Generation,
'''
if old not in text:
    raise SystemExit('HistoryKind insertion point missing')
text = text.replace(old, new, 1)
p.write_text(text)

# ------------------------------------------------------------------
# Store projection: existing KnowledgeApplication receipts join the
# common timeline without becoming a second persistence authority.
# ------------------------------------------------------------------
p = Path('crates/aikit-store/src/history_evidence.rs')
text = p.read_text()
text = text.replace(
'''    familiarity_history_evidence, AikitError, ContextId, FamiliarityStore, HistoryEvidence,
    HistoryKind, HistoryReadModel, HistoryRecoverability, Result, EXPLAIN_HISTORY_VERSION,
''',
'''    familiarity_history_evidence, AikitError, ContextId, EvidenceProvenance, FamiliarityStore,
    HistoryEvidence, HistoryKind, HistoryReadModel, HistoryRecoverability, Result,
    EXPLAIN_HISTORY_VERSION,
''', 1)
text = text.replace(
'''use crate::{AikitHome, SessionSpaceApplicationStore, SessionSpaceReceipt};
''',
'''use crate::{
    AikitHome, KnowledgeApplicationReceipt, KnowledgeHistoryOperation,
    SessionSpaceApplicationStore, SessionSpaceReceipt,
};
''', 1)
marker = '''/// Project canonical SessionSpace receipts. The receipt is generated evidence of
'''
addition = r'''/// Project one durable AIKit-owned Knowledge operation receipt into the common
/// timeline. Provider/source semantics remain in their providers; this is only the
/// audit evidence that AIKit actually traversed a route or materialised a frame.
pub fn knowledge_application_receipt_evidence(
    receipt: &KnowledgeApplicationReceipt,
) -> Result<HistoryEvidence> {
    let mut canonical_refs = BTreeSet::new();
    let mut authorities = vec![SourceAuthority::Generated];
    let mut provenance = Vec::new();
    let mut details = BTreeMap::new();
    details.insert("sequence".into(), receipt.sequence.to_string());

    let (kind, subject, summary, recoverability) = match receipt.operation {
        KnowledgeHistoryOperation::Route => {
            let route = receipt.route.as_ref().ok_or_else(|| {
                AikitError::new(
                    "history.knowledge_route_receipt_invalid",
                    format!("{} contains no KnowledgeRoute", receipt.receipt_id),
                )
            })?;
            canonical_refs.insert(route.route.clone());
            if let Some(query) = &route.query {
                details.insert("query".into(), query.clone());
            }
            details.insert("steps".into(), route.steps.len().to_string());
            for step in &route.steps {
                canonical_refs.insert(step.resource.clone());
                if !authorities.contains(&step.authority) {
                    authorities.push(step.authority);
                }
                provenance.push(EvidenceProvenance {
                    provider: step
                        .provider
                        .as_ref()
                        .and_then(|provider| ResourceRef::parse(&provider.to_string()).ok()),
                    source: Some(step.resource.clone()),
                    lens: step.lens.clone(),
                    revision: step.revision.clone(),
                    native_id: None,
                });
            }
            (
                HistoryKind::KnowledgeRoute,
                route.route.clone(),
                format!(
                    "Knowledge route {} · {} step{}",
                    route.route,
                    route.steps.len(),
                    if route.steps.len() == 1 { "" } else { "s" }
                ),
                HistoryRecoverability::ReplayNavigation,
            )
        }
        KnowledgeHistoryOperation::Frame => {
            let frame = receipt.frame.as_ref().ok_or_else(|| {
                AikitError::new(
                    "history.knowledge_frame_receipt_invalid",
                    format!("{} contains no Knowledge context frame", receipt.receipt_id),
                )
            })?;
            let subject = ResourceRef::parse(&receipt.receipt_id)?;
            canonical_refs.insert(subject.clone());
            canonical_refs.extend(frame.selected.iter().cloned());
            details.insert("readings".into(), frame.readings.len().to_string());
            details.insert("routes".into(), frame.routes.len().to_string());
            details.insert("absences".into(), frame.absences.len().to_string());
            details.insert(
                "contradictions".into(),
                frame.contradictions.len().to_string(),
            );
            details.insert("openQuestions".into(), frame.open_questions.len().to_string());
            for reading in &frame.readings {
                canonical_refs.insert(reading.resource.clone());
                if !authorities.contains(&reading.authority) {
                    authorities.push(reading.authority);
                }
                provenance.push(EvidenceProvenance {
                    provider: reading
                        .provider
                        .as_ref()
                        .and_then(|provider| ResourceRef::parse(&provider.to_string()).ok()),
                    source: Some(reading.resource.clone()),
                    lens: reading.lens.clone(),
                    revision: reading.revision.clone(),
                    native_id: None,
                });
            }
            for route in &frame.routes {
                canonical_refs.insert(route.route.clone());
                for step in &route.steps {
                    canonical_refs.insert(step.resource.clone());
                    if !authorities.contains(&step.authority) {
                        authorities.push(step.authority);
                    }
                }
            }
            (
                HistoryKind::KnowledgeFrame,
                subject,
                format!(
                    "Knowledge frame · {} reading{} · {} route{} · {} absence{}",
                    frame.readings.len(),
                    if frame.readings.len() == 1 { "" } else { "s" },
                    frame.routes.len(),
                    if frame.routes.len() == 1 { "" } else { "s" },
                    frame.absences.len(),
                    if frame.absences.len() == 1 { "" } else { "s" },
                ),
                HistoryRecoverability::InspectOnly,
            )
        }
    };

    canonical_refs.insert(subject.clone());
    Ok(HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: receipt.receipt_id.clone(),
        kind,
        subject,
        authorities,
        occurred_at_unix_ms: Some(u128::from(receipt.recorded_at_ms)),
        summary,
        canonical_refs: canonical_refs.into_iter().collect(),
        provenance,
        recoverability,
        details,
    })
}

'''
if marker not in text:
    raise SystemExit('Knowledge history projection insertion point missing')
text = text.replace(marker, addition + marker, 1)
p.write_text(text)

p = Path('crates/aikit-store/src/lib.rs')
text = p.read_text()
old = '''pub use history_evidence::{
    familiarity_history_evidence_model, generation_history_evidence,
    session_space_history_evidence, session_space_receipt_evidence,
};
'''
new = '''pub use history_evidence::{
    familiarity_history_evidence_model, generation_history_evidence,
    knowledge_application_receipt_evidence, session_space_history_evidence,
    session_space_receipt_evidence,
};
'''
if old not in text:
    raise SystemExit('history_evidence export block missing')
text = text.replace(old, new, 1)
p.write_text(text)

# ------------------------------------------------------------------
# Common Explain/History service: Knowledge is a first-class evidence
# source and durable Knowledge receipts join the common timeline.
# ------------------------------------------------------------------
p = Path('crates/aikit-tui/src/explain_history_service.rs')
text = p.read_text()
text = text.replace(
'''    compare_generation_worlds, familiarity_history_evidence_model, generation_history_evidence,
    procedure_history_evidence, session_space_history_evidence, GenerationWorldComparison,
    SessionSpaceApplicationStore,
''',
'''    compare_generation_worlds, familiarity_history_evidence_model, generation_history_evidence,
    knowledge_application_receipt_evidence, procedure_history_evidence,
    session_space_history_evidence, GenerationWorldComparison, SessionSpaceApplicationStore,
''', 1)
old = '''        let record = ResourceIndex::resource(&index, resource).ok_or_else(|| {
            aikit_core::AikitError::new(
                "application.resource_not_in_navigation_index",
                format!("{resource} is not in the V2 navigation index"),
            )
        })?;
        let mut evidence = explain_resource_evidence(&record.explanation());

        if let Some(store) = backend.familiarity()? {
'''
new = '''        let mut evidence = ResourceIndex::resource(&index, resource)
            .map(|record| explain_resource_evidence(&record.explanation()))
            .unwrap_or_else(|| ExplainEvidence {
                schema: EXPLAIN_HISTORY_VERSION.into(),
                subject: resource.clone(),
                facts: Vec::new(),
            });

        if let Some(address) = backend.knowledge_address(resource)? {
            if let Some(reading) = backend.knowledge_read(&address)? {
                let mut canonical_refs = vec![reading.resource.clone()];
                canonical_refs.extend(reading.evidence.iter().filter_map(|source| {
                    ResourceRef::parse(source.as_str()).ok()
                }));
                evidence.push(ExplainFact {
                    relation: "knowledge-reading".into(),
                    authority: Some(reading.authority),
                    summary: reading.why_selected.clone(),
                    canonical_refs,
                    provenance: vec![EvidenceProvenance {
                        provider: reading
                            .provider
                            .as_ref()
                            .and_then(|provider| ResourceRef::parse(&provider.to_string()).ok()),
                        source: Some(reading.resource.clone()),
                        lens: reading.lens.clone(),
                        revision: reading.revision.clone(),
                        native_id: None,
                    }],
                });
            }
            if let Some(explanation) = backend.knowledge_explain(&address)? {
                evidence.push(ExplainFact {
                    relation: "knowledge-provider-explain".into(),
                    authority: Some(explanation.authority),
                    summary: explanation.summary,
                    canonical_refs: explanation
                        .sources
                        .iter()
                        .filter_map(|source| ResourceRef::parse(source.as_str()).ok())
                        .collect(),
                    provenance: vec![EvidenceProvenance {
                        provider: explanation
                            .provider
                            .as_ref()
                            .and_then(|provider| ResourceRef::parse(&provider.to_string()).ok()),
                        ..EvidenceProvenance::default()
                    }],
                });
            }
        }

        if let Some(store) = backend.familiarity()? {
'''
if old not in text:
    raise SystemExit('Explain evidence resource block missing')
text = text.replace(old, new, 1)
old = '''        if resource.as_str().starts_with("session-space/") {
'''
new = '''        if evidence.facts.is_empty() {
            return Err(aikit_core::AikitError::new(
                "application.resource_not_in_navigation_index",
                format!("{resource} has no Resource or Knowledge evidence in the V2 application field"),
            ));
        }

        if resource.as_str().starts_with("session-space/") {
'''
if old not in text:
    raise SystemExit('Explain empty-evidence insertion point missing')
text = text.replace(old, new, 1)
old = '''        if let Some(store) = backend.familiarity()? {
            entries.extend(familiarity_history_evidence_model(&store).entries);
        }

        if let Some(home) = backend.application_home() {
'''
new = '''        if let Some(store) = backend.familiarity()? {
            entries.extend(familiarity_history_evidence_model(&store).entries);
        }

        for receipt in backend.knowledge_history(resource)? {
            entries.push(knowledge_application_receipt_evidence(&receipt)?);
        }

        if let Some(home) = backend.application_home() {
'''
if old not in text:
    raise SystemExit('History Knowledge insertion point missing')
text = text.replace(old, new, 1)
p.write_text(text)

# ------------------------------------------------------------------
# Final TUI: install canonical Actions into the live field; dynamic
# Knowledge resources receive them too; invocation uses common evidence.
# ------------------------------------------------------------------
p = Path('crates/aikit-tui/src/application_service.rs')
text = p.read_text()
text = text.replace(
'''    AikitError, FamiliarityContext, FamiliarityObservation, FamiliarityUse, ForgetScope,
    KnowledgeAddress, KnowledgeContextPack, KnowledgeProviderStatus, KnowledgeReading,
    KnowledgeRoute, KnowledgeSources, Result, DEFAULT_FAMILIARITY_HALF_LIFE_MS,
''',
'''    explain_history_actions_for, install_explain_history_actions, AikitError,
    FamiliarityContext, FamiliarityObservation, FamiliarityUse, ForgetScope, KnowledgeAddress,
    KnowledgeContextPack, KnowledgeProviderStatus, KnowledgeReading, KnowledgeRoute,
    KnowledgeSources, Result, DEFAULT_FAMILIARITY_HALF_LIFE_MS, EXPLAIN_ACTION_REF,
    HISTORY_ACTION_REF,
''', 1)
old = '''    fn navigation_index(&self) -> Result<ResourceSearchIndex> {
        let mut index = self.backend.navigation_index();
        if let Some(familiarity) = self.backend.familiarity()? {
'''
new = '''    fn navigation_index(&self) -> Result<ResourceSearchIndex> {
        let mut index = self.backend.navigation_index();
        install_explain_history_actions(&mut index)?;
        if let Some(familiarity) = self.backend.familiarity()? {
'''
if old not in text:
    raise SystemExit('navigation_index block missing')
text = text.replace(old, new, 1)
old = '''    fn contextual_actions(
        &self,
        resource: &ResourceRef,
    ) -> Result<Vec<ContextualActionDescriptor>> {
        let index = self.navigation_index()?;
        Ok(index.actions_for(resource).into_iter().cloned().collect())
    }
'''
new = '''    fn contextual_actions(
        &self,
        resource: &ResourceRef,
    ) -> Result<Vec<ContextualActionDescriptor>> {
        let index = self.navigation_index()?;
        let mut actions = index.actions_for(resource).into_iter().cloned().collect::<Vec<_>>();
        if self.backend.knowledge_address(resource)?.is_some() {
            for action in explain_history_actions_for(resource)? {
                if !actions.iter().any(|existing| existing.action == action.action) {
                    actions.push(action);
                }
            }
        }
        Ok(actions)
    }
'''
if old not in text:
    raise SystemExit('contextual_actions block missing')
text = text.replace(old, new, 1)
old = '''        let outcome = match action.action.as_str() {
            "action/project/open" => ActionOutcome::Opened {
'''
new = '''        let outcome = match action.action.as_str() {
            EXPLAIN_ACTION_REF => {
                let evidence = crate::explain_history_service::ExplainHistoryApplicationService::explain_evidence(
                    self,
                    &action.subject,
                )?;
                ActionOutcome::Explained {
                    subject: action.subject.clone(),
                    summary: to_string_pretty(&evidence).map_err(json_error)?,
                }
            }
            HISTORY_ACTION_REF => {
                let history = crate::explain_history_service::ExplainHistoryApplicationService::history_evidence(
                    self,
                    Some(&action.subject),
                )?;
                ActionOutcome::History {
                    subject: action.subject.clone(),
                    summary: format!(
                        "history · {} evidence entr{} for {}",
                        history.entries.len(),
                        if history.entries.len() == 1 { "y" } else { "ies" },
                        action.subject
                    ),
                }
            }
            "action/project/open" => ActionOutcome::Opened {
'''
if old not in text:
    raise SystemExit('invoke_action match block missing')
text = text.replace(old, new, 1)
p.write_text(text)

# ------------------------------------------------------------------
# Reducer: History is a typed read-only navigation outcome, never staging.
# ------------------------------------------------------------------
p = Path('crates/aikit-tui/src/application.rs')
text = p.read_text()
old = '''    Explained {
        subject: ResourceRef,
        summary: String,
    },
    Staged {
'''
new = '''    Explained {
        subject: ResourceRef,
        summary: String,
    },
    History {
        subject: ResourceRef,
        summary: String,
    },
    Staged {
'''
if old not in text:
    raise SystemExit('ActionOutcome History insertion missing')
text = text.replace(old, new, 1)
old = '''            Self::Opened { summary, .. }
            | Self::Explained { summary, .. }
            | Self::Staged { summary, .. }
'''
new = '''            Self::Opened { summary, .. }
            | Self::Explained { summary, .. }
            | Self::History { summary, .. }
            | Self::Staged { summary, .. }
'''
if old not in text:
    raise SystemExit('ActionOutcome summary block missing')
text = text.replace(old, new, 1)
old = '''                ActionOutcome::Explained { .. } => {
                    state.overlay = Some(Overlay::Explain);
                }
                ActionOutcome::Staged {
'''
new = '''                ActionOutcome::Explained { .. } => {
                    state.overlay = Some(Overlay::Explain);
                }
                ActionOutcome::History { .. } => {
                    state.navigation.push(NavigationPoint {
                        selected: state.selected.clone(),
                        relation_view: state.relation_view,
                        workspace_section: state.workspace_section,
                    });
                    state.overlay = None;
                    state.workspace_section = WorkspaceSection::History;
                }
                ActionOutcome::Staged {
'''
if old not in text:
    raise SystemExit('ActionFinished History block missing')
text = text.replace(old, new, 1)
p.write_text(text)

# ------------------------------------------------------------------
# Canonical CLI: History is first-class; Explain retains package behavior
# and falls back to common Resource/Knowledge evidence.
# ------------------------------------------------------------------
p = Path('crates/aikit-cli/src/cli.rs')
text = p.read_text()
old = '''    /// Explain why a capability is or is not active.
    Explain(ExplainArgs),
    /// Show what applying the current declarations would change.
'''
new = '''    /// Explain why a capability or V2 Resource has its current effective evidence.
    Explain(ExplainArgs),
    /// Read cross-domain evidence-bearing History, optionally scoped to one Resource.
    History(HistoryArgs),
    /// Show what applying the current declarations would change.
'''
if old not in text:
    raise SystemExit('Command History insertion missing')
text = text.replace(old, new, 1)
old = '''#[derive(Debug, Args)]
pub struct ExplainArgs {
    /// The capability id, e.g. `skill/rust/code-review`.
    #[arg(value_name = "CAPABILITY")]
    pub capability: String,
}

#[derive(Debug, Args)]
pub struct DiffArgs {}
'''
new = '''#[derive(Debug, Args)]
pub struct ExplainArgs {
    /// Capability id or canonical V2 ResourceRef.
    #[arg(value_name = "RESOURCE")]
    pub capability: String,
}

#[derive(Debug, Args)]
pub struct HistoryArgs {
    /// Optional canonical ResourceRef to filter the common timeline.
    #[arg(value_name = "RESOURCE")]
    pub resource: Option<String>,
}

#[derive(Debug, Args)]
pub struct DiffArgs {}
'''
if old not in text:
    raise SystemExit('HistoryArgs insertion missing')
text = text.replace(old, new, 1)
p.write_text(text)

p = Path('crates/aikit-cli/src/main.rs')
text = p.read_text()
text = text.replace(
'''use aikit_cli::{hook, multicall, run, ui};
''',
'''use aikit_cli::{hook, multicall, run, ui};
use aikit_tui::{application_service::ApplicationService, ExplainHistoryApplicationService};
''', 1)
old = '''        Some(Command::Explain(a)) => cmd_explain(cwd, a),
        Some(Command::Run(a)) => cmd_run(cwd, a),
'''
new = '''        Some(Command::Explain(a)) => cmd_explain(cwd, a),
        Some(Command::History(a)) => cmd_history(cwd, a),
        Some(Command::Run(a)) => cmd_run(cwd, a),
'''
if old not in text:
    raise SystemExit('dispatch History insertion missing')
text = text.replace(old, new, 1)
old = r'''fn cmd_explain(cwd: &std::path::Path, a: ExplainArgs) -> Result<Reply> {
    let service = Service::discover(cwd)?;
    let id = CapsuleId::parse(&a.capability)?;
    let explanation = service.resolved().explain(&id).ok_or_else(|| {
        AikitError::new(
            "resolution.unknown_capability",
            format!("{id} is not in the catalogue for this context"),
        )
        .with("capability", id.to_string())
    })?;
    let data = jval!({
        "id": explanation.id.to_string(),
        "revision": explanation.revision.as_ref().map(|revision| revision.as_str()),
        "active": explanation.active,
        "declared_enabled": explanation.declared_enabled,
        "selected_by": explanation.selected_by,
        "required_by": explanation.required_by,
        "dependencies": explanation.dependencies,
        "exports": explanation.exports,
        "skill_usage_overlays": explanation.skill_usage_overlays,
        "unavailable": explanation.unavailable.as_ref().map(|r| r.describe()),
        "render": explanation.render(),
    });
    Ok(reply(&service, data, vec![]))
}
'''
new = r'''fn cmd_explain(cwd: &std::path::Path, a: ExplainArgs) -> Result<Reply> {
    use aikit_core::resource::ResourceRef;

    let mut service = Service::discover(cwd)?;
    let capsule_candidate = CapsuleId::parse(&a.capability).ok();
    if let Some(id) = capsule_candidate.as_ref() {
        if let Some(explanation) = service.resolved().explain(id) {
            let data = jval!({
                "id": explanation.id.to_string(),
                "revision": explanation.revision.as_ref().map(|revision| revision.as_str()),
                "active": explanation.active,
                "declared_enabled": explanation.declared_enabled,
                "selected_by": explanation.selected_by,
                "required_by": explanation.required_by,
                "dependencies": explanation.dependencies,
                "exports": explanation.exports,
                "skill_usage_overlays": explanation.skill_usage_overlays,
                "unavailable": explanation.unavailable.as_ref().map(|r| r.describe()),
                "render": explanation.render(),
            });
            return Ok(reply(&service, data, vec![]));
        }
    }

    let resource = ResourceRef::parse(&a.capability)?;
    let evidence = {
        let application = ApplicationService::new(&mut service);
        application.explain_evidence(&resource)
    };
    match evidence {
        Ok(evidence) => Ok(reply(&service, jval!(evidence), vec![])),
        Err(error) if capsule_candidate.is_some() => {
            let id = capsule_candidate.expect("checked above");
            Err(AikitError::new(
                "resolution.unknown_capability",
                format!("{id} is not in the catalogue for this context"),
            )
            .with("capability", id.to_string())
            .with("evidence_error", error.code()))
        }
        Err(error) => Err(error),
    }
}

fn cmd_history(cwd: &std::path::Path, a: HistoryArgs) -> Result<Reply> {
    use aikit_core::resource::ResourceRef;

    let mut service = Service::discover(cwd)?;
    let resource = a.resource.as_deref().map(ResourceRef::parse).transpose()?;
    let history = {
        let application = ApplicationService::new(&mut service);
        application.history_evidence(resource.as_ref())?
    };
    Ok(reply(&service, jval!(history), vec![]))
}
'''
if old not in text:
    raise SystemExit('cmd_explain block missing')
text = text.replace(old, new, 1)
p.write_text(text)

# Real binary no-stub sweep now includes the canonical History command.
p = Path('crates/aikit-cli/tests/every_command.rs')
text = p.read_text()
old = '''    &["knowledge", "history"],
    &["explain", "script/demo/greet"],
    &["diff"],
'''
new = '''    &["knowledge", "history"],
    &["explain", "script/demo/greet"],
    &["history"],
    &["diff"],
'''
if old not in text:
    raise SystemExit('every_command History insertion missing')
text = text.replace(old, new, 1)
p.write_text(text)
PY

cat > crates/aikit-cli/tests/explain_history_production_v2.rs <<'RS'
use std::fs;

use aikit_cli::app::Service;
use aikit_core::resource::ResourceRef;
use aikit_core::{
    HistoryKind, SourceAuthority, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,
};
use aikit_store::AikitHome;
use aikit_tui::application::{
    reduce_tui, ActionOutcome, ActivationIntent, ResourceListItem, TuiState, UiAction,
    WorkspaceSection,
};
use aikit_tui::application::TuiApplicationService;
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
    assert!(explain
        .facts
        .iter()
        .any(|fact| fact.relation == "learned-accessibility" && fact.authority == Some(SourceAuthority::Learned)));

    let actions = TuiApplicationService::contextual_actions(&application, &source_ref).unwrap();
    assert!(actions.iter().any(|action| action.action.as_str() == EXPLAIN_ACTION_REF));
    let history_action = actions
        .iter()
        .find(|action| action.action.as_str() == HISTORY_ACTION_REF)
        .unwrap()
        .clone();
    let outcome = TuiApplicationService::invoke_action(&mut application, &history_action).unwrap();
    assert!(matches!(outcome, ActionOutcome::History { .. }));

    let mut state = TuiState::default();
    state.selected = Some(source_ref.clone());
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
RS

rustfmt \
  crates/aikit-core/src/explain_history.rs \
  crates/aikit-store/src/history_evidence.rs \
  crates/aikit-store/src/lib.rs \
  crates/aikit-tui/src/explain_history_service.rs \
  crates/aikit-tui/src/application_service.rs \
  crates/aikit-tui/src/application.rs \
  crates/aikit-cli/src/cli.rs \
  crates/aikit-cli/src/main.rs \
  crates/aikit-cli/tests/every_command.rs \
  crates/aikit-cli/tests/explain_history_production_v2.rs

cargo test -p aikit-store history_evidence
cargo test -p aikit-tui --test explain_history_action_parity_v2
cargo test -p aikit-cli --test explain_history_production_v2
cargo test -p aikit-cli --test every_command
cargo test -p aikit-cli --test cli_parse
cargo clippy --workspace --all-targets -- -D warnings

rm -f .github/workflows/tmp-finish-explain-history.yml scripts/tmp_finish_explain_history.sh
git add -A
git commit -m "chore: remove temporary Explain History closure machinery"
git push origin HEAD:"$branch"
