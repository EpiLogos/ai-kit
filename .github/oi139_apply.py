from pathlib import Path

model = Path("crates/aikit-core/src/resource/model.rs")
text = model.read_text()
old = "    Surface,\n    Host,\n"
new = "    Surface,\n    /// Durable semantic workspace identity; provider/native workspaces remain bindings/evidence.\n    SessionSpace,\n    Host,\n"
if text.count(old) != 1:
    raise SystemExit("ResourceKind insertion point drifted")
text = text.replace(old, new, 1)
old = '            Self::Surface => "surface",\n            Self::Host => "host",\n'
new = '            Self::Surface => "surface",\n            Self::SessionSpace => "session-space",\n            Self::Host => "host",\n'
if text.count(old) != 1:
    raise SystemExit("ResourceKind as_str insertion point drifted")
model.write_text(text.replace(old, new, 1))

service = Path("crates/aikit-tui/src/session_space_service.rs")
text = service.read_text()
old = "use aikit_core::project::ProjectRef;\nuse aikit_core::session_space::{SessionSpaceReadModel, SessionSpaceRef};\n"
new = """use aikit_core::project::ProjectRef;
use aikit_core::resource::{ResourceDescriptor, ResourceKind, ResourceRecord, ResourceSearchIndex};
use aikit_core::session_space::{SessionSpaceReadModel, SessionSpaceRef};
"""
if text.count(old) != 1:
    raise SystemExit("session_space_service import point drifted")
text = text.replace(old, new, 1)
marker = "use crate::application_service::ApplicationService;\n\n"
addition = """use crate::application_service::ApplicationService;

/// Project authored SessionSpaces into AIKit's common Resource navigation field.
///
/// This creates no SessionSpace state and no provider-specific identity. The
/// canonical `SessionSpaceRef` is reused as the ResourceRef so TUI, Agent and
/// application consumers can co-refer through the same Resource/Action grammar.
pub fn install_session_space_navigation_resources(
    index: &mut ResourceSearchIndex,
    states: &[SessionSpaceAuthoredState],
) {
    for state in states {
        let resource = state.id().as_resource_ref().clone();
        let label = state
            .label
            .clone()
            .unwrap_or_else(|| resource.as_str().to_string());
        let summary = format!(
            "SessionSpace revision {} · {} Project context(s) · {} AgentSession intent(s) · {} Surface intent(s) · {} native reference(s)",
            state.revision,
            state.project_contexts.len(),
            state.agent_sessions.len(),
            state.surfaces.len(),
            state.native_references.len(),
        );
        let mut descriptor = ResourceDescriptor::new(
            resource,
            ResourceKind::SessionSpace,
            label,
            summary,
        );
        descriptor.annotations.insert(
            "session-space-revision".into(),
            state.revision.to_string(),
        );
        index.insert_resource(ResourceRecord::new(descriptor), Vec::new());
    }
}

"""
if text.count(marker) != 1:
    raise SystemExit("session_space_service function insertion point drifted")
text = text.replace(marker, addition, 1)
tests = """

#[cfg(test)]
mod resource_projection_tests {
    use super::*;
    use aikit_core::resource::ResourceIndex;
    use aikit_core::{
        install_explain_history_actions, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF,
    };

    #[test]
    fn authored_session_space_enters_common_resource_action_field_without_identity_collapse() {
        let id = SessionSpaceRef::parse("session-space/omarchy-reference").unwrap();
        let mut state = SessionSpaceAuthoredState::new(id.clone());
        state.label = Some("Omarchy reference".into());
        state.revision = 7;

        let mut index = ResourceSearchIndex::default();
        install_session_space_navigation_resources(&mut index, &[state]);

        let resource = ResourceIndex::resource(&index, id.as_resource_ref()).unwrap();
        assert_eq!(resource.descriptor.id, *id.as_resource_ref());
        assert_eq!(resource.descriptor.kind, ResourceKind::SessionSpace);
        assert_eq!(resource.descriptor.name, "Omarchy reference");
        assert_eq!(
            resource.descriptor.annotations.get("session-space-revision"),
            Some(&"7".to_string())
        );

        install_explain_history_actions(&mut index).unwrap();
        let actions = index.actions_for(id.as_resource_ref());
        assert!(actions
            .iter()
            .any(|action| action.action.as_str() == EXPLAIN_ACTION_REF));
        assert!(actions
            .iter()
            .any(|action| action.action.as_str() == HISTORY_ACTION_REF));
        assert!(actions.iter().all(|action| action.subject == *id.as_resource_ref()));

        let hits = index.search("Omarchy reference", 8);
        assert!(hits.iter().any(|hit| {
            hit.resource == *id.as_resource_ref() && hit.kind == ResourceKind::SessionSpace
        }));
    }
}
"""
service.write_text(text + tests)

app = Path("crates/aikit-tui/src/application_service.rs")
text = app.read_text()
marker = "use crate::staging::is_on;\n"
replacement = "use crate::session_space_service::install_session_space_navigation_resources;\nuse crate::staging::is_on;\n"
if text.count(marker) != 1:
    raise SystemExit("application_service import insertion point drifted")
text = text.replace(marker, replacement, 1)
old = """    fn navigation_index(&self) -> Result<ResourceSearchIndex> {
        let mut index = self.backend.navigation_index();
        install_explain_history_actions(&mut index)?;
"""
new = """    fn navigation_index(&self) -> Result<ResourceSearchIndex> {
        let mut index = self.backend.navigation_index();
        let session_spaces = self.backend.session_space_list()?;
        install_session_space_navigation_resources(&mut index, &session_spaces);
        install_explain_history_actions(&mut index)?;
"""
if text.count(old) != 1:
    raise SystemExit("application_service navigation insertion point drifted")
app.write_text(text.replace(old, new, 1))
