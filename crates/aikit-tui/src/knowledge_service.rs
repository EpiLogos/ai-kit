//! Typed Knowledge Navigation application seam for V2 human surfaces.
//!
//! This module deliberately begins with the relations AIKit genuinely owns at
//! the application boundary. Today that includes Resource identity and canonical
//! contextual Action relations. Provider-native SemanticWiki, SourcePool and
//! CodeIndex/GitNexus edges are **not** reconstructed from generic resolver
//! adjacency; those providers can extend this same contract when their adapters
//! exist.

use aikit_core::knowledge::{
    KnowledgeRelationView, RelationDirection, RelationEdge, RelationNode, RelationOrigin,
    RelationQuery,
};
use aikit_core::resource::{ResourceIndex, SourceAuthority};
use aikit_core::{AikitError, Result};

use crate::palette_service::PaletteApplicationService;

/// Provider-neutral relation expansion used by list/tree/graph presentations.
///
/// A presentation chooses how to draw the returned nodes and edges. It must not
/// infer new relation kinds from position, indentation or provider payloads.
pub trait KnowledgeNavigationService {
    fn relation_view(&self, query: RelationQuery) -> Result<KnowledgeRelationView>;
}

impl KnowledgeNavigationService for PaletteApplicationService<'_> {
    fn relation_view(&self, query: RelationQuery) -> Result<KnowledgeRelationView> {
        query.validate()?;
        let index = self.backend().navigation_index();
        let focus = ResourceIndex::resource(&index, &query.focus).ok_or_else(|| {
            AikitError::new(
                "knowledge.focus_not_in_navigation_index",
                format!("{} is not in the V2 Resource navigation field", query.focus),
            )
            .with("focus", query.focus.to_string())
        })?;

        let mut view = KnowledgeRelationView::focus_only(
            query.clone(),
            RelationNode::new(
                focus.descriptor.id.clone(),
                focus.descriptor.kind,
                focus.descriptor.name.clone(),
            ),
        )?;

        // Contextual Action applicability is a relation AIKit can state exactly:
        // one canonical ActionRef is applicable to one selected subject Resource.
        // It is a generated application projection over canonical Resources, not
        // an authored Wiki/code/source edge.
        let actions = index
            .actions_for(&query.focus)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        for action in actions {
            let Some(action_record) = ResourceIndex::resource(&index, &action.action) else {
                view.warnings.push(format!(
                    "contextual Action {} is applicable but absent from the Resource index",
                    action.action
                ));
                continue;
            };
            if !view.push_node(RelationNode::new(
                action_record.descriptor.id.clone(),
                action_record.descriptor.kind,
                action_record.descriptor.name.clone(),
            )) {
                break;
            }
            if !view.push_edge(RelationEdge::new(
                query.focus.clone(),
                action.action.clone(),
                "contextual-action",
                RelationDirection::Outgoing,
                RelationOrigin::new(SourceAuthority::Generated).in_lens("aikit-resource-index"),
            ))? {
                break;
            }
        }

        if view.edges.is_empty() {
            view.warnings.push(
                "no typed local relation expansion is available for this Resource; provider-native relations were not inferred"
                    .to_string(),
            );
        }

        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::resource::ResourceRef;

    // Public integration tests exercise the service with the normal palette
    // fixtures. This unit test intentionally verifies only the provider-neutral
    // query contract so the module does not acquire a second fake resolver.
    #[test]
    fn relation_contract_types_are_provider_neutral() {
        let focus = ResourceRef::parse("project/aikit").unwrap();
        let query = RelationQuery::local(focus.clone());
        assert_eq!(query.focus, focus);
        assert_eq!(query.depth, aikit_core::DEFAULT_RELATION_DEPTH);
        assert!(query.max_nodes > 0);
        assert!(query.max_edges > 0);
    }
}
