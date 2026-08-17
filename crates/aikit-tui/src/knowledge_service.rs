//! TUI-facing projection of the canonical V2 Knowledge application service.
//!
//! This module contains no provider semantics and no parallel relation store. The
//! same [`aikit_core::KnowledgeApplication`] used by CLI/agent surfaces supplies
//! search, read, relations, routes, context packs, sources, explain and history;
//! the TUI is only a consumer of those typed read models.

use aikit_core::{
    KnowledgeAddress, KnowledgeApplication, KnowledgeContextPack, KnowledgeExplanation,
    KnowledgeOperations, KnowledgeProviderStatus, KnowledgeReading, KnowledgeRelationView,
    KnowledgeRoute, KnowledgeSearchResult, KnowledgeSources, Result,
};

pub trait KnowledgeNavigationService {
    fn knowledge_search(&self, query: &str, limit: usize) -> KnowledgeSearchResult;
    fn knowledge_read(&self, address: &KnowledgeAddress) -> Result<KnowledgeReading>;
    fn knowledge_relations(
        &self,
        address: &KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView>;
    fn knowledge_route(
        &self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> Result<KnowledgeRoute>;
    fn knowledge_context_pack(
        &self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> KnowledgeContextPack;
    fn knowledge_sources(&self, address: &KnowledgeAddress) -> Result<KnowledgeSources>;
    fn knowledge_explain(&self, address: &KnowledgeAddress) -> Result<KnowledgeExplanation>;
    fn knowledge_history<'b>(&self, routes: &'b [KnowledgeRoute]) -> Vec<&'b KnowledgeRoute>;
    fn knowledge_status(&self) -> KnowledgeProviderStatus;
}

impl KnowledgeNavigationService for KnowledgeApplication<'_> {
    fn knowledge_search(&self, query: &str, limit: usize) -> KnowledgeSearchResult {
        KnowledgeOperations::search(self, query, limit)
    }

    fn knowledge_read(&self, address: &KnowledgeAddress) -> Result<KnowledgeReading> {
        KnowledgeOperations::read(self, address)
    }

    fn knowledge_relations(
        &self,
        address: &KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        KnowledgeOperations::relations(self, address, depth, max_nodes, max_edges)
    }

    fn knowledge_route(
        &self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> Result<KnowledgeRoute> {
        KnowledgeOperations::route(self, query, addresses)
    }

    fn knowledge_context_pack(
        &self,
        query: Option<&str>,
        addresses: &[KnowledgeAddress],
    ) -> KnowledgeContextPack {
        KnowledgeOperations::frame(self, query, addresses)
    }

    fn knowledge_sources(&self, address: &KnowledgeAddress) -> Result<KnowledgeSources> {
        KnowledgeOperations::sources(self, address)
    }

    fn knowledge_explain(&self, address: &KnowledgeAddress) -> Result<KnowledgeExplanation> {
        KnowledgeOperations::explain(self, address)
    }

    fn knowledge_history<'b>(&self, routes: &'b [KnowledgeRoute]) -> Vec<&'b KnowledgeRoute> {
        KnowledgeOperations::history(self, routes)
    }

    fn knowledge_status(&self) -> KnowledgeProviderStatus {
        KnowledgeOperations::status(self)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aikit_core::resource::{ResourceRef, SourceRef, SourceRevision};
    use aikit_core::{
        FamiliarityContext, NativeSourcePoolProvider, SemanticWikiIndex, SemanticWikiProvider,
        SourceBinding, SourceMaterial, SourcePoolProvider, SourceVisibility,
    };

    use super::*;

    #[test]
    fn tui_knowledge_contract_is_the_core_application_contract() {
        let objects = aikit_core::parse_wiki_objects(
            r#"{"objects":[
              {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:root","revision":1,
               "provenance":[],"title":"Root","parent_space_refs":[],"child_space_refs":[],
               "node_refs":["wiki:node:auth"]},
              {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:auth","revision":1,
               "provenance":[{"source_ref":"source:spec"}],"type":"Concept","title":"Authentication",
               "space_refs":["wiki:space:root"],"source_refs":["source:spec"]}
            ]}"#,
        )
        .unwrap();
        let index = SemanticWikiIndex::rebuild(objects).unwrap();
        let material = vec![SourceMaterial {
            binding: SourceBinding {
                source: SourceRef::parse("source:spec").unwrap(),
                revision: SourceRevision::parse("sha256:spec").unwrap(),
                title: "Auth spec".into(),
                tags: vec!["auth".into()],
                visibility: SourceVisibility::Team,
                owners: Vec::new(),
                media_type: "text/markdown".into(),
                locator: None,
                metadata: BTreeMap::new(),
            },
            body: "Authentication rotates session tokens.".into(),
        }];
        let mut sources = NativeSourcePoolProvider::new();
        sources.rebuild(&material).unwrap();
        let app = KnowledgeApplication::new(FamiliarityContext {
            project: Some(ResourceRef::parse("project:demo").unwrap()),
            actor: None,
            agency: None,
            focus: None,
        })
        .with_wiki(SemanticWikiProvider::new(&index))
        .with_source_pool(&sources, &material);

        let service: &dyn KnowledgeNavigationService = &app;
        let hits = service.knowledge_search("Authentication", 10);
        assert!(hits
            .hits
            .iter()
            .any(|hit| hit.resource.as_str() == "wiki:node:auth"));
        let address = KnowledgeAddress::Wiki(ResourceRef::parse("wiki:node:auth").unwrap());
        assert_eq!(
            service.knowledge_read(&address).unwrap().resource.as_str(),
            "wiki:node:auth"
        );
        assert!(service
            .knowledge_relations(&address, 1, 16, 16)
            .unwrap()
            .nodes
            .iter()
            .any(|node| node.resource.as_str() == "source:spec"));
        assert!(service
            .knowledge_sources(&address)
            .unwrap()
            .sources
            .iter()
            .any(|source| source.as_str() == "source:spec"));
        let route = service
            .knowledge_route(None, std::slice::from_ref(&address))
            .unwrap();
        assert_eq!(route.steps.len(), 1);
        assert_eq!(
            service
                .knowledge_history(std::slice::from_ref(&route))
                .len(),
            1
        );
        assert_eq!(
            service
                .knowledge_context_pack(None, std::slice::from_ref(&address))
                .selected,
            vec![ResourceRef::parse("wiki:node:auth").unwrap()]
        );
    }
}
