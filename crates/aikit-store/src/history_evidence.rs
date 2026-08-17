//! Read-only History projection over existing persistence authorities.
//!
//! There is intentionally no history database here. Generations remain immutable
//! generation directories, familiarity remains replayed event evidence, and
//! SessionSpace receipts remain part of the canonical SessionSpace document. This
//! module only projects those owners into the common `aikit-core` History grammar.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use aikit_core::resource::{ResourceRef, SourceAuthority};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::{
    familiarity_history_evidence, AikitError, ContextId, FamiliarityStore, HistoryEvidence,
    HistoryKind, HistoryReadModel, HistoryRecoverability, Result, EXPLAIN_HISTORY_VERSION,
};

use crate::generation::{is_generation, read_lock, read_metadata, GENERATIONS};
use crate::{AikitHome, SessionSpaceApplicationStore, SessionSpaceReceipt};

/// Read immutable generation evidence for one canonical Context. A previous
/// Generation is inspectable historical ground; this function does not invent a
/// generic arbitrary-generation restore operation.
pub fn generation_history_evidence(
    home: &AikitHome,
    context: &ContextId,
) -> Result<Vec<HistoryEvidence>> {
    let root = home.context_dir(context).join(GENERATIONS);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(&root).map_err(|error| {
        AikitError::new(
            "history.generation_list_failed",
            format!("could not list {}: {error}", root.display()),
        )
    })? {
        let entry = entry.map_err(|error| {
            AikitError::new(
                "history.generation_list_failed",
                format!("could not read {}: {error}", root.display()),
            )
        })?;
        let path = entry.path();
        if !path.is_dir() || !is_generation(&path) {
            continue;
        }
        let metadata = read_metadata(&path)?;
        let resolved = read_lock(&path)?;
        let subject = ResourceRef::parse(&format!("generation/{}", metadata.generation_id))?;
        let mut canonical_refs = BTreeSet::new();
        canonical_refs.insert(subject.clone());
        if let Some(project) = resolved.context.project_id.as_ref() {
            canonical_refs.insert(ResourceRef::parse(&format!("project/{project}"))?);
        }

        let mut details = BTreeMap::new();
        details.insert("context".into(), metadata.context_id.clone());
        details.insert("resolutionHash".into(), metadata.resolution_hash.clone());
        details.insert("catalogRevision".into(), metadata.catalog_revision.clone());
        details.insert("isolation".into(), format!("{:?}", metadata.isolation));
        details.insert(
            "materialization".into(),
            format!("{:?}", metadata.materialization),
        );
        if let Some(project) = resolved.context.project_id.as_ref() {
            details.insert("project".into(), project.to_string());
        }
        if let Some(base) = &metadata.base_generation {
            details.insert("baseGeneration".into(), base.to_string());
        }
        if !metadata.targets.is_empty() {
            details.insert(
                "targetEffects".into(),
                metadata
                    .targets
                    .iter()
                    .map(|target| format!("{}:{}", target.target, target.effect))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        if !metadata.notes.is_empty() {
            details.insert("notes".into(), metadata.notes.join(" | "));
        }

        let nanos = metadata.created_at.as_nanos().max(0) as u128;
        entries.push(HistoryEvidence {
            schema: EXPLAIN_HISTORY_VERSION.into(),
            id: metadata.generation_id.to_string(),
            kind: HistoryKind::Generation,
            subject: subject.clone(),
            authorities: vec![SourceAuthority::Generated],
            occurred_at_unix_ms: Some(nanos / 1_000_000),
            summary: format!(
                "generation {} · resolution {} · catalog {}",
                metadata.generation_id, metadata.resolution_hash, metadata.catalog_revision
            ),
            canonical_refs: canonical_refs.into_iter().collect(),
            provenance: Vec::new(),
            recoverability: HistoryRecoverability::InspectOnly,
            details,
        });
    }
    entries.sort_by(|left, right| {
        right
            .occurred_at_unix_ms
            .cmp(&left.occurred_at_unix_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(entries)
}

/// Project learned accessibility events without changing their epistemic class.
pub fn familiarity_history_evidence_model(store: &FamiliarityStore) -> HistoryReadModel {
    let mut observations = store.snapshot().observations;
    observations.sort_by(|left, right| {
        right
            .observed_at_ms
            .cmp(&left.observed_at_ms)
            .then_with(|| right.observation_id.cmp(&left.observation_id))
    });
    HistoryReadModel::new(
        observations
            .iter()
            .map(familiarity_history_evidence)
            .collect(),
    )
}

/// Project canonical SessionSpace receipts. The receipt is generated evidence of
/// authored semantic state, so both classes remain visible. Restoration is not a
/// direct History write: it must go through `stage_restore` and the normal current
/// basis/apply authority.
pub fn session_space_history_evidence(
    store: &SessionSpaceApplicationStore,
    space: &SessionSpaceRef,
) -> Result<Vec<HistoryEvidence>> {
    store
        .history(space)?
        .iter()
        .map(session_space_receipt_evidence)
        .collect()
}

pub fn session_space_receipt_evidence(receipt: &SessionSpaceReceipt) -> Result<HistoryEvidence> {
    let subject = ResourceRef::parse(receipt.space.as_str())?;
    let mut canonical_refs = BTreeSet::new();
    canonical_refs.insert(subject.clone());
    collect_session_space_refs(&receipt.after, &mut canonical_refs)?;

    let mut details = BTreeMap::new();
    details.insert("sequence".into(), receipt.sequence.to_string());
    details.insert(
        "resultingRevision".into(),
        receipt.resulting_basis.revision.to_string(),
    );
    details.insert(
        "resultingStateHash".into(),
        receipt.resulting_basis.state_hash.clone(),
    );
    details.insert("operation".into(), format!("{:?}", receipt.operation));
    if !receipt.changed.is_empty() {
        details.insert(
            "changed".into(),
            receipt
                .changed
                .iter()
                .map(|change| format!("{change:?}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }

    Ok(HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: format!("session-space:{}:{}", receipt.space, receipt.sequence),
        kind: HistoryKind::SessionSpace,
        subject,
        authorities: vec![SourceAuthority::Authored, SourceAuthority::Generated],
        occurred_at_unix_ms: Some(receipt.applied_at_unix_ms),
        summary: format!(
            "SessionSpace {} receipt {} · {} semantic change{}",
            receipt.space,
            receipt.sequence,
            receipt.changed.len(),
            if receipt.changed.len() == 1 { "" } else { "s" }
        ),
        canonical_refs: canonical_refs.into_iter().collect(),
        provenance: Vec::new(),
        recoverability: HistoryRecoverability::RestageThroughCurrentAuthority,
        details,
    })
}

fn collect_session_space_refs(
    state: &aikit_core::session_space_application::SessionSpaceAuthoredState,
    refs: &mut BTreeSet<ResourceRef>,
) -> Result<()> {
    for (project, context) in &state.project_contexts {
        refs.insert(ResourceRef::parse(&project.to_string())?);
        refs.insert(context.reference.as_resource_ref().clone());
        refs.extend(context.basis.context_sources.iter().cloned());
        if let Some(host) = &context.basis.host {
            refs.insert(host.clone());
        }
    }
    refs.extend(state.agent_sessions.keys().cloned());
    for attachment in state.surfaces.values() {
        refs.insert(attachment.surface.clone());
        if let Some(component) = &attachment.component {
            refs.insert(component.clone());
        }
    }
    for binding in state.native_references.values() {
        refs.insert(binding.reference.clone());
        refs.extend(binding.owner.iter().cloned());
        refs.extend(binding.provider.iter().cloned());
        refs.extend(binding.host.iter().cloned());
    }
    if let Some(focus) = &state.focus {
        refs.insert(focus.target.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::session_space_application::{SessionSpaceFocus, SessionSpaceMutation};

    #[test]
    fn session_space_history_keeps_authored_and_generated_truth_separate_from_restore() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path());
        home.ensure_layout().unwrap();
        let store = SessionSpaceApplicationStore::new(home);
        let space = SessionSpaceRef::parse("session-space/history").unwrap();
        let create = store
            .stage(
                None,
                SessionSpaceMutation::Create {
                    id: space.clone(),
                    label: Some("History".into()),
                },
            )
            .unwrap();
        store.apply(&create).unwrap();
        let focus = store
            .stage(
                Some(&space),
                SessionSpaceMutation::Focus {
                    focus: Some(SessionSpaceFocus {
                        target: ResourceRef::parse("surface/editor").unwrap(),
                        region: Some("main".into()),
                        provenance: vec!["test".into()],
                    }),
                },
            )
            .unwrap();
        store.apply(&focus).unwrap();

        let history = session_space_history_evidence(&store, &space).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(
            history[1].authorities,
            vec![SourceAuthority::Authored, SourceAuthority::Generated]
        );
        assert_eq!(
            history[1].recoverability,
            HistoryRecoverability::RestageThroughCurrentAuthority
        );
        assert!(history[1]
            .canonical_refs
            .contains(&ResourceRef::parse("surface/editor").unwrap()));

        let before = store.load(&space).unwrap();
        let restore_preview = store.stage_restore(&space, 0).unwrap();
        assert_eq!(store.load(&space).unwrap(), before);
        assert!(matches!(
            restore_preview.intent,
            SessionSpaceMutation::Restore { .. }
        ));
    }

    #[test]
    fn familiarity_projection_never_changes_learned_authority() {
        let mut store = FamiliarityStore::new();
        store
            .record(aikit_core::FamiliarityObservation::destination(
                "obs-1",
                ResourceRef::parse("project/app").unwrap(),
                aikit_core::FamiliarityContext::default(),
                10,
            ))
            .unwrap();
        let history = familiarity_history_evidence_model(&store);
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].authorities, vec![SourceAuthority::Learned]);
    }
}
