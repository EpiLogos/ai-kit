//! Shared read model for source-authored relation consumers.
//!
//! Desktop/TUI/Agent surfaces consume this structure rather than reparsing source
//! syntax. Resolved relations come from the existing `SemanticWikiProvider` and
//! therefore include incoming/backlink traversal; unresolved/ambiguous authored
//! addresses remain explicit pending source state beside that graph projection.

use aikit_core::knowledge::{KnowledgeRelationView, RelationNode, RelationQuery};
use aikit_core::knowledge_wiki_provider::SemanticWikiProvider;
use aikit_core::resource::{ResourceKind, ResourceRef};
use aikit_core::Result;
use serde::{Deserialize, Serialize};

use crate::authored_wiki_source::{AuthoredWikiRelationCompilation, PendingAuthoredRelation};

pub const AUTHORED_WIKI_READ_VERSION: &str = "aikit.authored-wiki-read/v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthoredWikiSubjectRelations {
    pub version: String,
    pub subject_ref: ResourceRef,
    pub resolved: KnowledgeRelationView,
    #[serde(default)]
    pub pending: Vec<PendingAuthoredRelation>,
    /// Read-model construction never invokes an Agent/model.
    pub automatic_agent_or_model_invocation: bool,
}

pub fn authored_wiki_subject_relations(
    index: &aikit_core::SemanticWikiIndex,
    compilation: &AuthoredWikiRelationCompilation,
    subject_ref: ResourceRef,
    subject_kind: ResourceKind,
    subject_label: impl Into<String>,
) -> Result<AuthoredWikiSubjectRelations> {
    let label = subject_label.into();
    let query = RelationQuery::local(subject_ref.clone());
    let provider = SemanticWikiProvider::new(index);
    let mut resolved = match provider.relations(query.clone()) {
        Ok(view) => view,
        Err(error) if index.neighbours(&subject_ref, 1).is_empty() => {
            // A source may currently have only unresolved authored tendrils. It
            // remains a valid relation focus even though no graph edge can yet be
            // materialised for that target.
            let _ = error;
            KnowledgeRelationView::focus_only(
                query,
                RelationNode::new(subject_ref.clone(), subject_kind, label.clone()),
            )?
        }
        Err(error) => return Err(error),
    };

    if let Some(focus) = resolved
        .nodes
        .iter_mut()
        .find(|node| node.resource == subject_ref)
    {
        focus.kind = subject_kind;
        focus.label = label;
    }

    let mut pending = compilation
        .pending
        .iter()
        .filter(|relation| relation.subject_ref == subject_ref)
        .cloned()
        .collect::<Vec<_>>();
    pending.sort_by(|left, right| {
        left.evidence
            .raw_target
            .cmp(&right.evidence.raw_target)
            .then(left.evidence.relation.cmp(&right.evidence.relation))
    });

    Ok(AuthoredWikiSubjectRelations {
        version: AUTHORED_WIKI_READ_VERSION.into(),
        subject_ref,
        resolved,
        pending,
        automatic_agent_or_model_invocation: false,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aikit_core::knowledge_wiki::{WikiNode, WikiObject, OKF_WIKI_PROFILE};
    use aikit_core::resource::{ResourceKind, ResourceRef, SourceRef};

    use crate::authored_wiki_source::{
        compile_authored_wiki_relations, parse_authored_wiki_source,
        rebuild_semantic_wiki_with_authored_relations,
    };

    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn target() -> WikiObject {
        WikiObject::Node(WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: r("wiki:node:beta"),
            revision: 1,
            provenance: Vec::new(),
            node_type: "Concept".into(),
            title: Some("Beta".into()),
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions: BTreeMap::new(),
        })
    }

    #[test]
    fn consumer_gets_resolved_backlink_capable_view_and_pending_links_together() {
        let wiki = vec![target()];
        let source = parse_authored_wiki_source(
            r("source:alpha"),
            SourceRef::parse("source:alpha").unwrap(),
            None,
            vec!["alpha.md".into()],
            "See [[Beta]] and [[Future Concept]].",
        )
        .unwrap();
        let compilation = compile_authored_wiki_relations(&[source], &wiki, &[]).unwrap();
        let index = rebuild_semantic_wiki_with_authored_relations(&wiki, &compilation).unwrap();

        let view = authored_wiki_subject_relations(
            &index,
            &compilation,
            r("source:alpha"),
            ResourceKind::KnowledgeSource,
            "alpha.md",
        )
        .unwrap();
        assert_eq!(view.resolved.edges.len(), 1);
        assert_eq!(view.resolved.edges[0].relation, "references");
        assert_eq!(view.pending.len(), 1);
        assert_eq!(view.pending[0].evidence.raw_target, "Future Concept");
        assert_eq!(view.resolved.nodes[0].kind, ResourceKind::KnowledgeSource);
        assert!(!view.automatic_agent_or_model_invocation);
    }

    #[test]
    fn unresolved_only_source_is_still_a_valid_relation_focus() {
        let source = parse_authored_wiki_source(
            r("source:open-thread"),
            SourceRef::parse("source:open-thread").unwrap(),
            None,
            vec!["open-thread.md".into()],
            "Thinking toward [[Not Yet Written]].",
        )
        .unwrap();
        let compilation = compile_authored_wiki_relations(&[source], &[], &[]).unwrap();
        let index = rebuild_semantic_wiki_with_authored_relations(&[], &compilation).unwrap();

        let view = authored_wiki_subject_relations(
            &index,
            &compilation,
            r("source:open-thread"),
            ResourceKind::KnowledgeSource,
            "Open thread",
        )
        .unwrap();
        assert!(view.resolved.edges.is_empty());
        assert_eq!(view.pending.len(), 1);
        assert_eq!(view.resolved.nodes[0].label, "Open thread");
    }
}
