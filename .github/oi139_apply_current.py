from pathlib import Path


def replace_once(path: str, old: str, new: str, label: str) -> None:
    file = Path(path)
    text = file.read_text()
    if text.count(old) != 1:
        raise SystemExit(f"{label} drifted: expected one insertion point, found {text.count(old)}")
    file.write_text(text.replace(old, new, 1))


replace_once(
    "crates/aikit-core/src/resource/model.rs",
    "    Surface,\n    Host,\n",
    "    Surface,\n    /// Durable semantic workspace identity; provider/native workspaces remain bindings/evidence.\n    SessionSpace,\n    Host,\n",
    "ResourceKind insertion",
)
replace_once(
    "crates/aikit-core/src/resource/model.rs",
    '            Self::Surface => "surface",\n            Self::Host => "host",\n',
    '            Self::Surface => "surface",\n            Self::SessionSpace => "session-space",\n            Self::Host => "host",\n',
    "ResourceKind as_str insertion",
)
replace_once(
    "crates/aikit-core/src/resource/operative.rs",
    """        ResourceKind::Surface => {\n            BTreeSet::from([AddressHorizon::H3, AddressHorizon::H4, AddressHorizon::H5])\n        }\n        ResourceKind::SkillSet\n""",
    """        ResourceKind::Surface => {\n            BTreeSet::from([AddressHorizon::H3, AddressHorizon::H4, AddressHorizon::H5])\n        }\n        ResourceKind::SessionSpace => {\n            BTreeSet::from([AddressHorizon::H4, AddressHorizon::H5])\n        }\n        ResourceKind::SkillSet\n""",
    "SessionSpace operative horizon classification",
)

replace_once(
    "crates/aikit-tui/src/backend.rs",
    """    fn application_home(&self) -> Option<&AikitHome> {\n        None\n    }\n\n    /// Historical package-search documents retained for the public package/CLI\n""",
    """    fn application_home(&self) -> Option<&AikitHome> {\n        None\n    }\n\n    /// SessionSpace state that may participate in ordinary Resource navigation.\n    ///\n    /// Navigation-only/fake backends deliberately have no canonical AIKit home,\n    /// so they contribute no SessionSpace resources. Explicit SessionSpace\n    /// operations retain their stronger contract and still fail when persistence\n    /// is unavailable. A backend with another legitimate source can override this\n    /// projection without fabricating `AikitHome`.\n    fn session_space_navigation(&self) -> Result<Vec<SessionSpaceAuthoredState>> {\n        if self.application_home().is_none() {\n            return Ok(Vec::new());\n        }\n        self.session_space_list()\n    }\n\n    /// Historical package-search documents retained for the public package/CLI\n""",
    "PaletteBackend SessionSpace navigation capability",
)

replace_once(
    "crates/aikit-tui/src/session_space_service.rs",
    "use aikit_core::project::ProjectRef;\nuse aikit_core::session_space::{SessionSpaceReadModel, SessionSpaceRef};\n",
    "use aikit_core::project::ProjectRef;\nuse aikit_core::resource::{ResourceDescriptor, ResourceKind, ResourceRecord, ResourceSearchIndex};\nuse aikit_core::session_space::{SessionSpaceReadModel, SessionSpaceRef};\n",
    "session_space_service imports",
)
replace_once(
    "crates/aikit-tui/src/session_space_service.rs",
    "use crate::application_service::ApplicationService;\n\n",
    """use crate::application_service::ApplicationService;\n\n/// Project authored SessionSpaces into AIKit's common Resource navigation field.\n///\n/// This creates no SessionSpace state and no provider-specific identity. The\n/// canonical `SessionSpaceRef` is reused as the ResourceRef so TUI, Agent and\n/// application consumers can co-refer through the same Resource/Action grammar.\npub fn install_session_space_navigation_resources(\n    index: &mut ResourceSearchIndex,\n    states: &[SessionSpaceAuthoredState],\n) {\n    for state in states {\n        let resource = state.id().as_resource_ref().clone();\n        let label = state\n            .label\n            .clone()\n            .unwrap_or_else(|| resource.as_str().to_string());\n        let summary = format!(\n            \"SessionSpace revision {} · {} Project context(s) · {} AgentSession intent(s) · {} Surface intent(s) · {} native reference(s)\",\n            state.revision,\n            state.project_contexts.len(),\n            state.agent_sessions.len(),\n            state.surfaces.len(),\n            state.native_references.len(),\n        );\n        let mut descriptor = ResourceDescriptor::new(\n            resource,\n            ResourceKind::SessionSpace,\n            label,\n            summary,\n        );\n        descriptor.annotations.insert(\n            \"session-space-revision\".into(),\n            state.revision.to_string(),\n        );\n        index.insert_resource(ResourceRecord::new(descriptor), Vec::new());\n    }\n}\n\n""",
    "session_space_service Resource projection",
)
service = Path("crates/aikit-tui/src/session_space_service.rs")
text = service.read_text()
test_marker = "mod resource_projection_tests"
if test_marker in text:
    raise SystemExit("SessionSpace resource projection tests already present")
service.write_text(
    text
    + """

#[cfg(test)]
mod resource_projection_tests {
    use super::*;
    use aikit_core::resource::{horizons_for_resource, AddressHorizon, ResourceIndex};
    use aikit_core::{install_explain_history_actions, EXPLAIN_ACTION_REF, HISTORY_ACTION_REF};

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
        assert_eq!(
            horizons_for_resource(resource),
            [AddressHorizon::H4, AddressHorizon::H5].into_iter().collect()
        );

        install_explain_history_actions(&mut index).unwrap();
        let actions = index.actions_for(id.as_resource_ref());
        assert!(actions.iter().any(|action| action.action.as_str() == EXPLAIN_ACTION_REF));
        assert!(actions.iter().any(|action| action.action.as_str() == HISTORY_ACTION_REF));
        assert!(actions.iter().all(|action| action.subject == *id.as_resource_ref()));

        let hits = index.search("Omarchy reference", 8);
        assert!(hits.iter().any(|hit| {
            hit.resource == *id.as_resource_ref() && hit.kind == ResourceKind::SessionSpace
        }));
    }
}
"""
)

replace_once(
    "crates/aikit-tui/src/application_service.rs",
    "use crate::staging::is_on;\n",
    "use crate::session_space_service::install_session_space_navigation_resources;\nuse crate::staging::is_on;\n",
    "ApplicationService Resource projection import",
)
replace_once(
    "crates/aikit-tui/src/application_service.rs",
    """    fn navigation_index(&self) -> Result<ResourceSearchIndex> {\n        let mut index = self.backend.navigation_index();\n        install_explain_history_actions(&mut index)?;\n""",
    """    fn navigation_index(&self) -> Result<ResourceSearchIndex> {\n        let mut index = self.backend.navigation_index();\n        let session_spaces = self.backend.session_space_navigation()?;\n        install_session_space_navigation_resources(&mut index, &session_spaces);\n        install_explain_history_actions(&mut index)?;\n""",
    "ApplicationService SessionSpace projection",
)

replace_once(
    "crates/aikit-cli/src/app/mod.rs",
    """    fn view(&self) -> &ResolvedView {\n        &self.view\n    }\n\n    fn scope_layers(&self) -> Option<&[ScopeLayer]> {\n""",
    """    fn view(&self) -> &ResolvedView {\n        &self.view\n    }\n\n    fn application_home(&self) -> Option<&AikitHome> {\n        Some(&self.home)\n    }\n\n    fn scope_layers(&self) -> Option<&[ScopeLayer]> {\n""",
    "production Service application_home exposure",
)
