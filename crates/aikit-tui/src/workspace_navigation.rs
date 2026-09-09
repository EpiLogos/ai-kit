//! Workspace destinations ("places") as real hits in the shared navigation
//! field, not a second search/route ontology.
//!
//! `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §3.3 is explicit: "A navigation destination must not
//! become a second resource/search ontology. The host advertises a navigable
//! Surface/route; Search resolves it through the existing addressable
//! field." This module is that advertisement. It inserts one
//! `ResourceKind::Surface` record per top-level [`WorkspaceSection`] into the
//! same [`ResourceSearchIndex`] every other navigable thing already lives in
//! (mirroring `crate::session_space_service::install_session_space_navigation_resources`'s
//! shape exactly), plus one canonical Action relating each of them — the same
//! one-canonical-Action/many-subjects pattern `aikit_core::install_explain_history_actions`
//! and `backend::native_application_action_record`'s `action/project/open`
//! already establish. No new fuzzy matcher, no new selection state: Ctrl+K's
//! query still resolves through `ApplicationService::resolve_search`
//! (`aikit_core::resource::search`'s one `ResourceSearchIndex`), and acting on
//! a destination hit still rides the existing single-immediate-contextual-
//! action pipeline (`ApplicationSurfaceController::open_selected_action`).

use aikit_core::resource::{
    ActionStageability, ContextualActionDescriptor, OwnerRef, ResourceDescriptor, ResourceKind,
    ResourceRecord, ResourceRef, ResourceSearchIndex, ResourceSource, SourceAuthority, SourceRef,
    SourceState,
};
use aikit_core::Result;

use crate::application::WorkspaceSection;

/// The one canonical Action every Workspace-destination Surface is related
/// through, mirroring `action/project/open`'s shape: one Action identity,
/// many subjects, no per-destination Action manufactured.
pub const WORKSPACE_DESTINATION_ACTION_REF: &str = "action/workspace/open-destination";
pub const START_FACTORY_WORK_ACTION_REF: &str = "action/factory/start-work";

/// One row of the single source-of-truth destination table. `slug` forms the
/// Surface ResourceRef (`surface/workspace/<slug>`); `question` is the
/// spec-authored human question this destination answers, used as the
/// Resource's search description — real product copy, not a placeholder.
struct Destination {
    section: WorkspaceSection,
    slug: &'static str,
    question: &'static str,
}

/// The single source of truth both `install_workspace_destination_navigation_resources`
/// and `workspace_section_for_destination` are built from — the mapping is
/// defined exactly once.
const DESTINATIONS: [Destination; 6] = [
    Destination {
        section: WorkspaceSection::Worlds,
        slug: "worlds",
        question: "What world am I in, and what governs it — Project, binding, root, host, profiles, scopes?",
    },
    Destination {
        section: WorkspaceSection::Compose,
        slug: "compose",
        question: "What could I build — which Capabilities, information and Actor/Runtime are available to compose?",
    },
    Destination {
        section: WorkspaceSection::Work,
        slug: "work",
        question: "What is actually running — which Direct Sessions and effective Actor/Runtime are live right now?",
    },
    Destination {
        section: WorkspaceSection::Knowledge,
        slug: "knowledge",
        question: "What is known — how do these sources, skills, worlds, code and material relations connect?",
    },
    Destination {
        section: WorkspaceSection::History,
        slug: "history",
        question: "What happened, and what can be recovered — effective world lineage and evidence?",
    },
    Destination {
        section: WorkspaceSection::System,
        slug: "system",
        question: "What does this installation depend on — credentials, providers, adapters, Workcell?",
    },
];

fn destination_ref(destination: &Destination) -> ResourceRef {
    ResourceRef::parse(format!("surface/workspace/{}", destination.slug))
        .expect("static Workspace destination ResourceRef must be valid")
}

fn destination_action_record() -> Result<ResourceRecord> {
    let id = ResourceRef::parse(WORKSPACE_DESTINATION_ACTION_REF)?;
    let mut descriptor = ResourceDescriptor::new(
        id,
        ResourceKind::Action,
        "Open destination",
        "navigate to a top-level Workspace destination",
    );
    descriptor.owner = Some(OwnerRef::parse("aikit/application-service")?);
    descriptor.sources.push(ResourceSource {
        source: SourceRef::parse("source/aikit/application-service")?,
        authority: Some(SourceAuthority::Authored),
        revision: None,
        locator: None,
        state: SourceState::Available,
    });
    descriptor
        .annotations
        .insert("action.expected-return-forms".into(), "navigated".into());
    Ok(ResourceRecord::new(descriptor))
}

/// Insert one `ResourceKind::Surface` record for every top-level Workspace
/// destination, plus the shared canonical Action and one contextual-Action
/// relation per destination. Idempotent (`ResourceSearchIndex` is keyed by
/// `ResourceRef`); safe to call once alongside the other navigation-field
/// installers in `ApplicationService::navigation_index_from`.
pub fn install_workspace_destination_navigation_resources(
    index: &mut ResourceSearchIndex,
) -> Result<()> {
    index.insert_resource(destination_action_record()?, Vec::new());
    let action = ResourceRef::parse(WORKSPACE_DESTINATION_ACTION_REF)?;

    for destination in &DESTINATIONS {
        let subject = destination_ref(destination);
        let descriptor = ResourceDescriptor::new(
            subject.clone(),
            ResourceKind::Surface,
            destination.section.as_str(),
            destination.question,
        );
        index.insert_resource(ResourceRecord::new(descriptor), Vec::new());
        index.insert_action(
            ContextualActionDescriptor::new(
                action.clone(),
                subject,
                "Open",
                "navigate to this Workspace destination",
                ActionStageability::NotStageable,
            )
            .with_keywords([destination.slug]),
        )?;
    }
    Ok(())
}

/// Add the native Factory Commission entry to the Work destination when the
/// application backend has a complete binding. The Action remains Factory-
/// owned and immediate: the reviewed request file is the exact owner input,
/// and the returned value is the exact Factory receipt.
pub fn install_start_factory_work_action(index: &mut ResourceSearchIndex) -> Result<()> {
    let action = ResourceRef::parse(START_FACTORY_WORK_ACTION_REF)?;
    let mut descriptor = ResourceDescriptor::new(
        action.clone(),
        ResourceKind::Action,
        "Start Factory Work",
        "submit the configured Commission request through Factory's native owner operation",
    );
    descriptor.owner = Some(OwnerRef::parse("factory")?);
    descriptor.sources.push(ResourceSource {
        source: SourceRef::parse("factory.developmental-local-provider/v1")?,
        authority: Some(SourceAuthority::Observed),
        revision: None,
        locator: None,
        state: SourceState::Available,
    });
    descriptor.annotations.insert(
        "action.expected-return-forms".into(),
        "factory.commission-receipt/v1".into(),
    );
    index.insert_resource(ResourceRecord::new(descriptor), Vec::new());
    index.insert_action(
        ContextualActionDescriptor::new(
            action,
            ResourceRef::parse("surface/workspace/work")?,
            "Start Factory Work",
            "commission the configured developmental difference; this does not execute it",
            ActionStageability::NotStageable,
        )
        .with_keywords(["factory", "commission", "start work"]),
    )
}

/// The inverse of the same table: which [`WorkspaceSection`] a destination
/// Ref names, or `None` for any other Resource (an ordinary Capability,
/// SessionSpace, etc.) — never a false positive on an unrelated kind.
pub fn workspace_section_for_destination(resource: &ResourceRef) -> Option<WorkspaceSection> {
    DESTINATIONS
        .iter()
        .find(|destination| destination_ref(destination) == *resource)
        .map(|destination| destination.section)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::resource::{ResourceIndex, ResourceSearchIndex};

    #[test]
    fn installs_exactly_six_surface_destinations_each_with_one_contextual_action() {
        let mut index = ResourceSearchIndex::default();
        install_workspace_destination_navigation_resources(&mut index).unwrap();

        let mut surfaces = 0;
        for destination in &DESTINATIONS {
            let subject = destination_ref(destination);
            let record = ResourceIndex::resource(&index, &subject)
                .expect("every installed destination must be present in the index");
            assert_eq!(record.descriptor.kind, ResourceKind::Surface);
            let actions = index.actions_for(&subject);
            assert_eq!(
                actions.len(),
                1,
                "each Surface subject must carry exactly one contextual action"
            );
            assert_eq!(actions[0].action.as_str(), WORKSPACE_DESTINATION_ACTION_REF);
            surfaces += 1;
        }
        assert_eq!(surfaces, 6);
    }

    #[test]
    fn destination_lookup_round_trips_every_installed_destination() {
        let mut index = ResourceSearchIndex::default();
        install_workspace_destination_navigation_resources(&mut index).unwrap();

        for destination in &DESTINATIONS {
            let subject = destination_ref(destination);
            assert_eq!(
                workspace_section_for_destination(&subject),
                Some(destination.section)
            );
        }
    }

    #[test]
    fn destination_lookup_does_not_false_positive_on_an_unrelated_resource() {
        let unrelated = ResourceRef::parse("capability/skill/rust/review").unwrap();
        assert_eq!(workspace_section_for_destination(&unrelated), None);

        let action = ResourceRef::parse(WORKSPACE_DESTINATION_ACTION_REF).unwrap();
        assert_eq!(workspace_section_for_destination(&action), None);
    }
}
