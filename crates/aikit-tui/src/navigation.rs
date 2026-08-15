//! Presentation-neutral navigation grammar for the V2 Quick/Workspace shell.
//!
//! This module owns no semantic state. It translates already-ranked Resource hits
//! and contextual Action relations into the small intent vocabulary that keyboard
//! and mouse presentations share. Resolver, provider and ranking semantics remain
//! in the application/core layer.

use aikit_core::resource::{ContextualActionDescriptor, ResourceSearchHit, ResourceRef};

use crate::application::PresentationMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationIntent {
    Select(ResourceRef),
    Open(ResourceRef),
    InvokeAction {
        action: ResourceRef,
        subject: ResourceRef,
    },
    StageAction {
        action: ResourceRef,
        subject: ResourceRef,
    },
    SetPresentation(PresentationMode),
}

/// Context chrome is a read model, not another controller. Missing dimensions are
/// omitted rather than guessed, and the same values feed Quick and Workspace.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AmbientContext {
    pub project: Option<String>,
    pub focus: Option<String>,
    pub profile: Option<String>,
    pub agency: Option<String>,
    pub host: Option<String>,
    pub target: Option<String>,
}

impl AmbientContext {
    /// Narrow surfaces carry truthful values without verbose labels so the current
    /// Project/Focus/Host remain legible. Wide surfaces add the explicit labels and
    /// only show Profile/Agency/Target when authoritative values are actually known.
    pub fn line(&self, width: u16) -> String {
        if width < 80 {
            return [
                self.project.as_deref(),
                self.focus.as_deref(),
                self.host.as_deref(),
            ]
            .into_iter()
            .flatten()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        }

        let mut parts = Vec::new();
        push(&mut parts, "Project", self.project.as_deref());
        push(&mut parts, "Focus", self.focus.as_deref());
        push(&mut parts, "Host", self.host.as_deref());
        push(&mut parts, "Profile", self.profile.as_deref());
        push(&mut parts, "Agency", self.agency.as_deref());
        push(&mut parts, "Target", self.target.as_deref());
        parts.join(" · ")
    }
}

fn push(parts: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        parts.push(format!("{label}: {value}"));
    }
}

/// Search selection is always the identity of one canonical ResourceRef.
pub fn keyboard_select_hit(hit: &ResourceSearchHit) -> NavigationIntent {
    NavigationIntent::Select(hit.resource.clone())
}

pub fn mouse_select_hit(hit: &ResourceSearchHit) -> NavigationIntent {
    keyboard_select_hit(hit)
}

/// Opening a search hit opens the canonical Resource. Contextual applicability is
/// intentionally resolved *after* subject selection through `actions_for(subject)`.
pub fn keyboard_open_hit(hit: &ResourceSearchHit) -> NavigationIntent {
    NavigationIntent::Open(hit.resource.clone())
}

pub fn mouse_open_hit(hit: &ResourceSearchHit) -> NavigationIntent {
    keyboard_open_hit(hit)
}

/// Invoke one canonical Action in the context of the selected subject Resource.
pub fn keyboard_invoke_action(action: &ContextualActionDescriptor) -> NavigationIntent {
    NavigationIntent::InvokeAction {
        action: action.action.clone(),
        subject: action.subject.clone(),
    }
}

pub fn mouse_invoke_action(action: &ContextualActionDescriptor) -> NavigationIntent {
    keyboard_invoke_action(action)
}

/// Space may stage only an Action relation whose canonical descriptor explicitly
/// declares it stageable. A non-stageable Action has no staging intent at all.
pub fn stage_action(action: &ContextualActionDescriptor) -> Option<NavigationIntent> {
    action.stageability.is_stageable().then(|| NavigationIntent::StageAction {
        action: action.action.clone(),
        subject: action.subject.clone(),
    })
}

pub fn keyboard_set_presentation(mode: PresentationMode) -> NavigationIntent {
    NavigationIntent::SetPresentation(mode)
}

pub fn mouse_set_presentation(mode: PresentationMode) -> NavigationIntent {
    keyboard_set_presentation(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::resource::{
        ActionStageability, ResourceDescriptor, ResourceKind, ResourceRecord, ResourceSearchIndex,
    };

    fn rref(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn resource_hit() -> ResourceSearchHit {
        let mut index = ResourceSearchIndex::default();
        index.insert_resource(
            ResourceRecord::new(ResourceDescriptor::new(
                rref("project/aikit"),
                ResourceKind::Project,
                "AIKit",
                "project",
            )),
            Vec::new(),
        );
        index.search("AIKit", 1).remove(0)
    }

    fn action(stageability: ActionStageability) -> ContextualActionDescriptor {
        ContextualActionDescriptor::new(
            rref("action/project/open"),
            rref("project/aikit"),
            "Open workspace",
            "open project",
            stageability,
        )
    }

    #[test]
    fn keyboard_and_mouse_selection_are_the_same_intent() {
        let hit = resource_hit();
        assert_eq!(keyboard_select_hit(&hit), mouse_select_hit(&hit));
    }

    #[test]
    fn keyboard_and_mouse_open_the_same_canonical_resource() {
        let hit = resource_hit();
        assert_eq!(keyboard_open_hit(&hit), mouse_open_hit(&hit));
        assert_eq!(
            keyboard_open_hit(&hit),
            NavigationIntent::Open(rref("project/aikit"))
        );
    }

    #[test]
    fn keyboard_and_mouse_invoke_the_same_contextual_action_relation() {
        let action = action(ActionStageability::NotStageable);
        assert_eq!(keyboard_invoke_action(&action), mouse_invoke_action(&action));
        assert_eq!(
            keyboard_invoke_action(&action),
            NavigationIntent::InvokeAction {
                action: rref("action/project/open"),
                subject: rref("project/aikit"),
            }
        );
    }

    #[test]
    fn space_only_stages_explicitly_stageable_actions() {
        assert!(stage_action(&action(ActionStageability::NotStageable)).is_none());
        assert_eq!(
            stage_action(&action(ActionStageability::Stageable)),
            Some(NavigationIntent::StageAction {
                action: rref("action/project/open"),
                subject: rref("project/aikit"),
            })
        );
    }

    #[test]
    fn presentation_expansion_has_keyboard_mouse_parity() {
        assert_eq!(
            keyboard_set_presentation(PresentationMode::Workspace),
            mouse_set_presentation(PresentationMode::Workspace)
        );
    }

    #[test]
    fn narrow_and_wide_context_chrome_keep_the_present_project_legible() {
        let context = AmbientContext {
            project: Some("ai-kit".into()),
            focus: Some("V2-E1".into()),
            profile: Some("code".into()),
            agency: Some("Mahāmāyā".into()),
            host: Some("worker-laptop".into()),
            target: Some("codex".into()),
        };

        let narrow = context.line(60);
        let wide = context.line(120);
        assert_eq!(narrow, "ai-kit · V2-E1 · worker-laptop");
        assert!(wide.contains("Project: ai-kit"));
        assert!(wide.contains("Focus: V2-E1"));
        assert!(wide.contains("Host: worker-laptop"));
        assert!(wide.contains("Profile: code"));
        assert!(wide.contains("Agency: Mahāmāyā"));
        assert!(wide.contains("Target: codex"));
    }

    #[test]
    fn absent_profile_and_agency_are_not_invented() {
        let context = AmbientContext {
            project: Some("ai-kit".into()),
            focus: Some("V2-E1".into()),
            host: Some("worker-laptop".into()),
            target: Some("codex".into()),
            ..AmbientContext::default()
        };
        let wide = context.line(120);
        assert!(!wide.contains("Profile:"));
        assert!(!wide.contains("Agency:"));
        assert!(wide.contains("Target: codex"));
    }
}
