//! Canonical Action identities for the context-wide Explain and History faculties.
//!
//! These helpers deliberately stop short of UI keybindings or application
//! dispatch. They establish the semantic invariant every Surface must consume:
//! one Explain Action and one History Action, contextualised to many subjects
//! without manufacturing per-Surface/per-kind Action identities.

use crate::resource::{
    ActionStageability, ContextualActionDescriptor, ResourceDescriptor, ResourceIndex,
    ResourceKind, ResourceRecord, ResourceRef, ResourceSearchIndex,
};
use crate::Result;

pub const EXPLAIN_ACTION_REF: &str = "action/aikit/explain";
pub const HISTORY_ACTION_REF: &str = "action/aikit/history";

pub fn explain_history_action_resources() -> Result<[ResourceRecord; 2]> {
    Ok([
        ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse(EXPLAIN_ACTION_REF)?,
            ResourceKind::Action,
            "Explain",
            "Explain why the selected Resource is present, unavailable, degraded, staged, projected or learned-easy from owner-held evidence.",
        )),
        ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse(HISTORY_ACTION_REF)?,
            ResourceKind::Action,
            "History",
            "Read evidence-bearing recent, familiar, changed and recoverable history for the selected Resource without creating a second history authority.",
        )),
    ])
}

pub fn explain_history_actions_for(
    subject: &ResourceRef,
) -> Result<[ContextualActionDescriptor; 2]> {
    Ok([
        ContextualActionDescriptor::new(
            ResourceRef::parse(EXPLAIN_ACTION_REF)?,
            subject.clone(),
            "Explain",
            "Explain provenance, availability, staged/projected state and learned accessibility",
            ActionStageability::NotStageable,
        )
        .with_keywords(["why", "provenance", "availability", "degraded", "source"]),
        ContextualActionDescriptor::new(
            ResourceRef::parse(HISTORY_ACTION_REF)?,
            subject.clone(),
            "History",
            "Inspect recent, familiar, changed and recoverable evidence for this Resource",
            ActionStageability::NotStageable,
        )
        .with_keywords([
            "recent",
            "familiar",
            "changed",
            "generation",
            "route",
            "receipt",
        ]),
    ])
}

/// Install the two common Actions into one already-resolved navigation field and
/// relate them to every subject that existed at the time of installation.
///
/// The subject snapshot is taken **before** the Action resources are inserted, so
/// this helper does not recursively manufacture Explain/History-on-Explain rows.
/// Calling it repeatedly is idempotent because the ResourceSearchIndex is keyed by
/// canonical ResourceRef and contextual Action relation.
pub fn install_explain_history_actions(index: &mut ResourceSearchIndex) -> Result<()> {
    let subjects = ResourceIndex::resources(index)
        .into_iter()
        .map(|record| record.descriptor.id.clone())
        .filter(|id| id.as_str() != EXPLAIN_ACTION_REF && id.as_str() != HISTORY_ACTION_REF)
        .collect::<Vec<_>>();

    for record in explain_history_action_resources()? {
        index.insert_resource(record, Vec::new());
    }
    for subject in subjects {
        for action in explain_history_actions_for(&subject)? {
            index.insert_action(action)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_keep_one_identity_across_different_subjects() {
        let project = ResourceRef::parse("project/app").unwrap();
        let component = ResourceRef::parse("component/editor").unwrap();
        let project_actions = explain_history_actions_for(&project).unwrap();
        let component_actions = explain_history_actions_for(&component).unwrap();

        assert_eq!(project_actions[0].action, component_actions[0].action);
        assert_eq!(project_actions[1].action, component_actions[1].action);
        assert_ne!(project_actions[0].subject, component_actions[0].subject);
        assert_eq!(project_actions[0].action.as_str(), EXPLAIN_ACTION_REF);
        assert_eq!(project_actions[1].action.as_str(), HISTORY_ACTION_REF);
        assert!(!project_actions[0].stageability.is_stageable());
        assert!(!project_actions[1].stageability.is_stageable());
    }

    #[test]
    fn action_records_are_action_resources_not_reading_surrogates() {
        let records = explain_history_action_resources().unwrap();
        assert!(records
            .iter()
            .all(|record| record.descriptor.kind == ResourceKind::Action));
        assert_eq!(records[0].descriptor.id.as_str(), EXPLAIN_ACTION_REF);
        assert_eq!(records[1].descriptor.id.as_str(), HISTORY_ACTION_REF);
    }

    #[test]
    fn install_relates_one_action_identity_to_every_existing_subject() {
        let project = ResourceRef::parse("project/app").unwrap();
        let component = ResourceRef::parse("component/editor").unwrap();
        let mut index = ResourceSearchIndex::default();
        for (id, kind, name) in [
            (project.clone(), ResourceKind::Project, "App"),
            (component.clone(), ResourceKind::Component, "Editor"),
        ] {
            index.insert_resource(
                ResourceRecord::new(ResourceDescriptor::new(id, kind, name, "fixture")),
                Vec::new(),
            );
        }

        install_explain_history_actions(&mut index).unwrap();
        install_explain_history_actions(&mut index).unwrap();

        assert_eq!(index.actions_for(&project).len(), 2);
        assert_eq!(index.actions_for(&component).len(), 2);
        assert!(index
            .actions_for(&project)
            .iter()
            .any(|action| action.action.as_str() == EXPLAIN_ACTION_REF));
        assert!(index
            .actions_for(&project)
            .iter()
            .any(|action| action.action.as_str() == HISTORY_ACTION_REF));
        assert_eq!(ResourceIndex::resources(&index).len(), 4);
        assert!(index
            .actions_for(&ResourceRef::parse(EXPLAIN_ACTION_REF).unwrap())
            .is_empty());
        assert!(index
            .actions_for(&ResourceRef::parse(HISTORY_ACTION_REF).unwrap())
            .is_empty());
    }
}
