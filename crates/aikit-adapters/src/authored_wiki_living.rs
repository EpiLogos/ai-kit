//! Source-authored relation → Living Knowledge deterministic impact.
//!
//! The relation compiler already knows the exact source/revision carrying every
//! explicit body/metadata relation. This adapter turns that basis into the current
//! `KnowledgeDependency` vocabulary and delegates impact/freshness to the existing
//! Living Knowledge engine. No background Agent/model work is introduced.

use aikit_core::knowledge_living::{
    deterministic_knowledge_impact, KnowledgeChangeHorizon, KnowledgeImpact,
};
use aikit_core::Result;

use crate::authored_wiki_source::{
    authored_relation_dependencies, AuthoredWikiSourceProjection,
};

pub const AUTHORED_WIKI_LIVING_VERSION: &str = "aikit.authored-wiki-living/v1";

pub fn authored_wiki_knowledge_impact(
    horizon: &KnowledgeChangeHorizon,
    sources: &[AuthoredWikiSourceProjection],
) -> Result<KnowledgeImpact> {
    deterministic_knowledge_impact(horizon, &authored_relation_dependencies(sources))
}

#[cfg(test)]
mod tests {
    use aikit_core::knowledge_living::{
        KnowledgeChangeKind, KnowledgeFreshness, KnowledgeObservedSource, KnowledgeSourceChange,
    };
    use aikit_core::resource::{ResourceRef, SourceAuthority, SourceRef, SourceRevision};

    use crate::authored_wiki_source::parse_authored_wiki_source_with_authority;

    use super::*;

    fn revision(raw: &str) -> SourceRevision {
        SourceRevision::parse(raw).unwrap()
    }

    #[test]
    fn authored_properties_revision_change_enters_existing_changed_affected_metabolism() {
        let source = parse_authored_wiki_source_with_authority(
            ResourceRef::parse("wiki:concept:flow").unwrap(),
            SourceRef::parse("source:flow").unwrap(),
            SourceAuthority::Authored,
            Some(revision("rev-1")),
            vec!["flow.md".into()],
            r#"---
type: Concept
resource: wiki:concept:flow
relations:
  develops: [Living Wiki]
---
Flow body.
"#,
        )
        .unwrap();
        let horizon = KnowledgeChangeHorizon {
            provider: "central".into(),
            cursor: 8,
            sources: vec![KnowledgeObservedSource {
                source: SourceRef::parse("source:flow").unwrap(),
                revision: Some(revision("rev-2")),
                available: true,
            }],
            changes: vec![KnowledgeSourceChange {
                cursor: 8,
                world_ref: "world:demo".into(),
                source: SourceRef::parse("source:flow").unwrap(),
                roles: vec!["flow".into()],
                provenance: "human-authored".into(),
                standing: "authored-human-position".into(),
                before_revision: Some(revision("rev-1")),
                after_revision: Some(revision("rev-2")),
                kind: KnowledgeChangeKind::Modified,
                agent_retrieval_allowed: true,
            }],
        };

        let impact = authored_wiki_knowledge_impact(&horizon, &[source]).unwrap();
        assert_eq!(impact.changed_sources.len(), 1);
        assert_eq!(impact.affected.len(), 1);
        assert_eq!(impact.affected[0].resource.as_str(), "wiki:concept:flow");
        assert_eq!(impact.affected[0].relation, "authored-source:develops");
        assert_eq!(impact.affected[0].freshness, KnowledgeFreshness::BasisChanged);
        assert!(!impact.automatic_agent_or_model_invocation);
    }

    #[test]
    fn unchanged_exact_relation_basis_remains_fresh_without_model_work() {
        let source = parse_authored_wiki_source_with_authority(
            ResourceRef::parse("source:note:alpha").unwrap(),
            SourceRef::parse("source:note:alpha").unwrap(),
            SourceAuthority::Observed,
            Some(revision("rev-4")),
            vec!["alpha.md".into()],
            "See [[Beta]].",
        )
        .unwrap();
        let horizon = KnowledgeChangeHorizon {
            provider: "source-house".into(),
            cursor: 9,
            sources: vec![KnowledgeObservedSource {
                source: SourceRef::parse("source:note:alpha").unwrap(),
                revision: Some(revision("rev-4")),
                available: true,
            }],
            changes: Vec::new(),
        };

        let impact = authored_wiki_knowledge_impact(&horizon, &[source]).unwrap();
        assert!(impact.changed_sources.is_empty());
        assert!(impact.affected.is_empty());
        assert!(!impact.automatic_agent_or_model_invocation);
    }
}
