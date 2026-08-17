//! Canonical application-level Knowledge Navigation operation family.
//!
//! Providers retain their own graph, search, provenance and authority semantics.
//! This trait only gives every consumer (CLI, TUI and agent/Skill surfaces) one
//! vocabulary for invoking the already-canonical [`KnowledgeApplication`].

use serde::{Deserialize, Serialize};

use crate::knowledge::{
    KnowledgeContextPack, KnowledgeReading, KnowledgeRelationView, KnowledgeRoute,
};
use crate::knowledge_navigation::{
    KnowledgeAddress, KnowledgeApplication, KnowledgeExplanation, KnowledgeProviderStatus,
    KnowledgeSearchResult,
};
use crate::resource::{ProviderRef, SourceAuthority, SourceRef};
use crate::Result;

pub const KNOWLEDGE_OPERATIONS_VERSION: &str = "aikit.knowledge-operations/v1";

/// Source/provenance disclosure for one canonical Knowledge address.
///
/// This is a projection over provider-owned explanation evidence. It does not
/// create a new source relation, ContextSource, Wiki edge or Project declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeSources {
    pub address: KnowledgeAddress,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderRef>,
    pub authority: SourceAuthority,
    #[serde(default)]
    pub sources: Vec<SourceRef>,
    pub summary: String,
}

/// One provider-neutral operation vocabulary over the federated Knowledge field.
///
/// `frame` is the derived context-pack projection; it is deliberately not a new
/// canonical ContextSource. `route` records traversal and never mutates provider
/// graphs. `sources` exposes provenance already owned by the native provider.
pub trait KnowledgeOperations {
    fn search(&self, query: &str, limit: usize) -> KnowledgeSearchResult;
    fn read(&self, address: &KnowledgeAddress) -> Result<KnowledgeReading>;
    fn relations(
        &self,
        address: &KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView>;
    fn route(&self, query: Option<&str>, addresses: &[KnowledgeAddress]) -> Result<KnowledgeRoute>;
    fn frame(&self, query: Option<&str>, addresses: &[KnowledgeAddress]) -> KnowledgeContextPack;
    fn sources(&self, address: &KnowledgeAddress) -> Result<KnowledgeSources>;
    fn explain(&self, address: &KnowledgeAddress) -> Result<KnowledgeExplanation>;
    fn history<'b>(&self, routes: &'b [KnowledgeRoute]) -> Vec<&'b KnowledgeRoute>;
    fn status(&self) -> KnowledgeProviderStatus;
}

impl KnowledgeOperations for KnowledgeApplication<'_> {
    fn search(&self, query: &str, limit: usize) -> KnowledgeSearchResult {
        KnowledgeApplication::search(self, query, limit)
    }

    fn read(&self, address: &KnowledgeAddress) -> Result<KnowledgeReading> {
        KnowledgeApplication::read(self, address)
    }

    fn relations(
        &self,
        address: &KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<KnowledgeRelationView> {
        KnowledgeApplication::relations(self, address, depth, max_nodes, max_edges)
    }

    fn route(&self, query: Option<&str>, addresses: &[KnowledgeAddress]) -> Result<KnowledgeRoute> {
        KnowledgeApplication::route(self, query, addresses)
    }

    fn frame(&self, query: Option<&str>, addresses: &[KnowledgeAddress]) -> KnowledgeContextPack {
        KnowledgeApplication::context_pack(self, query, addresses)
    }

    fn sources(&self, address: &KnowledgeAddress) -> Result<KnowledgeSources> {
        let explanation = KnowledgeApplication::explain(self, address)?;
        Ok(KnowledgeSources {
            address: explanation.address,
            provider: explanation.provider,
            authority: explanation.authority,
            sources: explanation.sources,
            summary: explanation.summary,
        })
    }

    fn explain(&self, address: &KnowledgeAddress) -> Result<KnowledgeExplanation> {
        KnowledgeApplication::explain(self, address)
    }

    fn history<'b>(&self, routes: &'b [KnowledgeRoute]) -> Vec<&'b KnowledgeRoute> {
        KnowledgeApplication::history(self, routes)
    }

    fn status(&self) -> KnowledgeProviderStatus {
        KnowledgeApplication::status(self)
    }
}
