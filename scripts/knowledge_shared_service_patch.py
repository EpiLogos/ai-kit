from pathlib import Path


def replace(path, old, new, count=1):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f'anchor missing in {path}: {old[:140]!r}')
    p.write_text(text.replace(old, new, count))

# Core frames carry explicit uncertainty in every consumer, not only the CLI wrapper.
replace(
    'crates/aikit-core/src/knowledge_navigation.rs',
    '        if let Ok(route) = self.route(query, addresses) {\n            pack.routes.push(route);\n        }\n        pack\n    }',
    '        if let Ok(route) = self.route(query, addresses) {\n            pack.routes.push(route);\n        }\n        pack.derive_uncertainty();\n        pack\n    }',
)

# Rebuildable address cache failure must not turn a successful provider search into an outage.
replace(
    'crates/aikit-cli/src/app/knowledge.rs',
    '        self.knowledge_store().remember_search_hits(&result.hits)?;\n        Ok(result)',
    '        let mut result = result;\n        if let Err(error) = self.knowledge_store().remember_search_hits(&result.hits) {\n            result.absences.push(format!(\n                "Knowledge address cache unavailable; live search results remain valid: {}",\n                error.message()\n            ));\n        }\n        Ok(result)',
)

# The low-level backend exposes the one Knowledge application faculty to final TUI consumers.
replace(
    'crates/aikit-tui/src/backend.rs',
    'use aikit_core::{FamiliarityObservation, FamiliarityStore, Result};',
    'use aikit_core::{\n    FamiliarityObservation, FamiliarityStore, ForgetScope, KnowledgeAddress,\n    KnowledgeContextPack, KnowledgeExplanation, KnowledgeProviderStatus, KnowledgeReading,\n    KnowledgeRelationView, KnowledgeRoute, KnowledgeSearchResult, KnowledgeSources, Result,\n};',
)
replace(
    'crates/aikit-tui/src/backend.rs',
    '    SessionSpaceExplainEvidence, SessionSpaceHistoryComparison, SessionSpaceReceipt,\n};',
    '    KnowledgeApplicationReceipt, SessionSpaceExplainEvidence, SessionSpaceHistoryComparison,\n    SessionSpaceReceipt,\n};',
)
replace(
    'crates/aikit-tui/src/backend.rs',
    '    fn record_familiarity(&mut self, _observation: FamiliarityObservation) -> Result<()> {\n        Ok(())\n    }\n\n    // SessionSpace application operations deliberately live on the shared backend',
    '''    fn record_familiarity(&mut self, _observation: FamiliarityObservation) -> Result<()> {
        Ok(())
    }

    // Knowledge operations deliberately live on the same shared application seam
    // as CLI/TUI. Defaults preserve deterministic fake backends; production owns
    // materialisation and returns Some(..) for the supported operation family.
    fn knowledge_search(&self, _query: &str, _limit: usize) -> Result<Option<KnowledgeSearchResult>> {
        Ok(None)
    }

    fn knowledge_address(&self, _resource: &ResourceRef) -> Result<Option<KnowledgeAddress>> {
        Ok(None)
    }

    fn knowledge_read(&self, _address: &KnowledgeAddress) -> Result<Option<KnowledgeReading>> {
        Ok(None)
    }

    fn knowledge_relations(
        &self,
        _address: &KnowledgeAddress,
        _depth: u8,
        _max_nodes: usize,
        _max_edges: usize,
    ) -> Result<Option<KnowledgeRelationView>> {
        Ok(None)
    }

    fn knowledge_route(
        &mut self,
        _query: Option<&str>,
        _addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeRoute>> {
        Ok(None)
    }

    fn knowledge_frame(
        &mut self,
        _query: Option<&str>,
        _addresses: &[KnowledgeAddress],
    ) -> Result<Option<KnowledgeContextPack>> {
        Ok(None)
    }

    fn knowledge_sources(&self, _address: &KnowledgeAddress) -> Result<Option<KnowledgeSources>> {
        Ok(None)
    }

    fn knowledge_explain(
        &self,
        _address: &KnowledgeAddress,
    ) -> Result<Option<KnowledgeExplanation>> {
        Ok(None)
    }

    fn knowledge_history(
        &self,
        _resource: Option<&ResourceRef>,
    ) -> Result<Vec<KnowledgeApplicationReceipt>> {
        Ok(Vec::new())
    }

    fn knowledge_status(&self) -> Result<Option<KnowledgeProviderStatus>> {
        Ok(None)
    }

    fn knowledge_forget(&mut self, _scope: ForgetScope) -> Result<bool> {
        Ok(false)
    }

    // SessionSpace application operations deliberately live on the shared backend''',
)

# Production Service is the actual implementation behind the shared backend.
replace(
    'crates/aikit-cli/src/app/mod.rs',
    '    fn record_familiarity(\n        &mut self,\n        observation: aikit_core::FamiliarityObservation,\n    ) -> Result<()> {\n        aikit_store::append_familiarity_observation(&self.index, observation)\n    }\n\n    fn documents(&self) -> Vec<SearchDoc> {',
    '''    fn record_familiarity(
        &mut self,
        observation: aikit_core::FamiliarityObservation,
    ) -> Result<()> {
        aikit_store::append_familiarity_observation(&self.index, observation)
    }

    fn knowledge_search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Option<aikit_core::KnowledgeSearchResult>> {
        Service::knowledge_search(self, query, limit).map(Some)
    }

    fn knowledge_address(
        &self,
        resource: &aikit_core::ResourceRef,
    ) -> Result<Option<aikit_core::KnowledgeAddress>> {
        Service::knowledge_address(self, resource)
    }

    fn knowledge_read(
        &self,
        address: &aikit_core::KnowledgeAddress,
    ) -> Result<Option<aikit_core::KnowledgeReading>> {
        Service::knowledge_read(self, address).map(Some)
    }

    fn knowledge_relations(
        &self,
        address: &aikit_core::KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<Option<aikit_core::KnowledgeRelationView>> {
        Service::knowledge_relations(self, address, depth, max_nodes, max_edges).map(Some)
    }

    fn knowledge_route(
        &mut self,
        query: Option<&str>,
        addresses: &[aikit_core::KnowledgeAddress],
    ) -> Result<Option<aikit_core::KnowledgeRoute>> {
        Service::knowledge_route(self, query, addresses).map(Some)
    }

    fn knowledge_frame(
        &mut self,
        query: Option<&str>,
        addresses: &[aikit_core::KnowledgeAddress],
    ) -> Result<Option<aikit_core::KnowledgeContextPack>> {
        Service::knowledge_frame(self, query, addresses).map(Some)
    }

    fn knowledge_sources(
        &self,
        address: &aikit_core::KnowledgeAddress,
    ) -> Result<Option<aikit_core::KnowledgeSources>> {
        Service::knowledge_sources(self, address).map(Some)
    }

    fn knowledge_explain(
        &self,
        address: &aikit_core::KnowledgeAddress,
    ) -> Result<Option<aikit_core::KnowledgeExplanation>> {
        Service::knowledge_explain(self, address).map(Some)
    }

    fn knowledge_history(
        &self,
        resource: Option<&aikit_core::ResourceRef>,
    ) -> Result<Vec<aikit_store::KnowledgeApplicationReceipt>> {
        Service::knowledge_history(self, resource)
    }

    fn knowledge_status(&self) -> Result<Option<aikit_core::KnowledgeProviderStatus>> {
        Service::knowledge_status(self).map(Some)
    }

    fn knowledge_forget(&mut self, scope: aikit_core::ForgetScope) -> Result<bool> {
        Service::knowledge_forget(self, scope)?;
        Ok(true)
    }

    fn documents(&self) -> Vec<SearchDoc> {''',
)
