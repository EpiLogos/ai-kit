//! Typed Explain/History reads over the same backend used by the V2 TUI.
//!
//! This is an application projection, not a TUI history controller. It performs
//! no writes and owns no evidence. Generation, familiarity and SessionSpace data
//! are read from their existing authorities and classified by `aikit-core`.

use std::collections::BTreeMap;

use aikit_core::resource::{ResourceIndex, ResourceRef, SourceAuthority};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::{
    explain_resource_evidence, EvidenceProvenance, ExplainEvidence, ExplainFact,
    FamiliarityContext, HistoryEvidence, HistoryKind, HistoryReadModel, HistoryRecoverability,
    Result, DEFAULT_FAMILIARITY_HALF_LIFE_MS, EXPLAIN_HISTORY_VERSION,
};
use aikit_store::{
    familiarity_history_evidence_model, generation_history_evidence,
    session_space_history_evidence, SessionSpaceApplicationStore,
};

use crate::application_service::ApplicationService;

pub trait ExplainHistoryApplicationService {
    /// Evidence-classified explanation of one canonical Resource. Existing rich
    /// domain Explain surfaces remain valid; this read answers where each fact
    /// came from without replaying their semantics.
    fn explain_evidence(&self, resource: &ResourceRef) -> Result<ExplainEvidence>;

    /// Cross-domain History read assembled from existing authorities. No row here
    /// is writable and no TUI-local database exists.
    fn history_evidence(&self, resource: Option<&ResourceRef>) -> Result<HistoryReadModel>;
}

impl ExplainHistoryApplicationService for ApplicationService<'_> {
    fn explain_evidence(&self, resource: &ResourceRef) -> Result<ExplainEvidence> {
        let backend = self.backend();
        let index = backend.navigation_index();
        let record = ResourceIndex::resource(&index, resource).ok_or_else(|| {
            aikit_core::AikitError::new(
                "application.resource_not_in_navigation_index",
                format!("{resource} is not in the V2 navigation index"),
            )
        })?;
        let mut evidence = explain_resource_evidence(&record.explanation());

        if let Some(store) = backend.familiarity()? {
            let assessment = store.assess_destination(
                resource,
                &familiarity_context(backend.context()),
                now_ms(),
                DEFAULT_FAMILIARITY_HALF_LIFE_MS,
            );
            if !assessment.is_empty() {
                evidence.push(ExplainFact {
                    relation: "learned-accessibility".into(),
                    authority: Some(SourceAuthority::Learned),
                    summary: format!(
                        "{} observed use{}; contextual frecency {:.4}",
                        assessment.observations,
                        if assessment.observations == 1 { "" } else { "s" },
                        assessment.contextual_frecency
                    ),
                    canonical_refs: vec![resource.clone()],
                    provenance: assessment
                        .evidence_ids
                        .iter()
                        .map(|id| EvidenceProvenance {
                            source: ResourceRef::parse(&format!("familiarity-observation/{id}"))
                                .ok(),
                            ..EvidenceProvenance::default()
                        })
                        .collect(),
                });
            }
        }

        if resource.as_str().starts_with("session-space/") {
            let space = SessionSpaceRef::parse(resource.as_str())?;
            if let Ok(session_evidence) = backend.session_space_explain(&space, None) {
                evidence.push(ExplainFact {
                    relation: "session-space-authored-state".into(),
                    authority: Some(SourceAuthority::Authored),
                    summary: format!(
                        "canonical SessionSpace semantic revision {}",
                        session_evidence.explanation.semantic_revision
                    ),
                    canonical_refs: vec![resource.clone()],
                    provenance: Vec::new(),
                });
                if let Some(receipt) = session_evidence.latest_receipt {
                    evidence.push(ExplainFact {
                        relation: "session-space-receipt".into(),
                        authority: Some(SourceAuthority::Generated),
                        summary: format!(
                            "receipt {} changed {} semantic dimension{}",
                            receipt.sequence,
                            receipt.changed.len(),
                            if receipt.changed.len() == 1 { "" } else { "s" }
                        ),
                        canonical_refs: vec![resource.clone()],
                        provenance: vec![EvidenceProvenance {
                            source: ResourceRef::parse(&format!(
                                "session-space-receipt/{}/{}",
                                receipt.space, receipt.sequence
                            ))
                            .ok(),
                            ..EvidenceProvenance::default()
                        }],
                    });
                }
            }
        }

        Ok(evidence)
    }

    fn history_evidence(&self, resource: Option<&ResourceRef>) -> Result<HistoryReadModel> {
        let backend = self.backend();
        let mut entries = Vec::new();

        // Existing recent run history is observed operational evidence. It is
        // distinct from immutable Generation/Procedure history and from learned
        // familiarity even when all three mention the same Resource.
        for (index, intent) in backend.recent().into_iter().enumerate() {
            let subject = ResourceRef::parse(&intent.capsule.to_string())?;
            let summary = intent
                .redacted_argv()
                .ok()
                .filter(|argv| !argv.is_empty())
                .map(|argv| format!("run · {} · {}", intent.capsule, argv.join(" ")))
                .unwrap_or_else(|| format!("run · {}", intent.capsule));
            entries.push(HistoryEvidence {
                schema: EXPLAIN_HISTORY_VERSION.into(),
                id: format!("recent-{index}"),
                kind: HistoryKind::Recent,
                subject: subject.clone(),
                authorities: vec![SourceAuthority::Observed],
                occurred_at_unix_ms: None,
                summary,
                canonical_refs: vec![subject],
                provenance: Vec::new(),
                recoverability: HistoryRecoverability::InspectOnly,
                details: BTreeMap::new(),
            });
        }

        if let Some(store) = backend.familiarity()? {
            entries.extend(familiarity_history_evidence_model(&store).entries);
        }

        if let Some(home) = backend.application_home() {
            entries.extend(generation_history_evidence(
                home,
                &backend.context().context_id,
            )?);

            // SessionSpace receipts are indexed by canonical relations at read
            // time, not copied into an aggregate history store. This means a
            // Project, Surface, Component, AgentSession, Host or provider can
            // navigate back to the SessionSpace receipt that mentioned it.
            let session_spaces = SessionSpaceApplicationStore::new(home.clone());
            for state in session_spaces.list()? {
                entries.extend(session_space_history_evidence(
                    &session_spaces,
                    state.id(),
                )?);
            }
        }

        if let Some(resource) = resource {
            entries.retain(|entry| entry.matches(resource));
        }
        entries.sort_by(|left, right| {
            right
                .occurred_at_unix_ms
                .cmp(&left.occurred_at_unix_ms)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(HistoryReadModel::new(entries))
    }
}

fn familiarity_context(context: &aikit_core::ContextDescriptor) -> FamiliarityContext {
    FamiliarityContext {
        project: context
            .project_id
            .as_ref()
            .and_then(|project| ResourceRef::parse(&format!("project/{project}")).ok()),
        actor: None,
        agency: None,
        focus: context.task.clone(),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}
