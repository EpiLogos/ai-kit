from pathlib import Path


def replace(path: str, old: str, new: str):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"alignment anchor not found in {path}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))

replace(
    "crates/aikit-cli/src/knowledge_runtime.rs",
    "    pub fn relations(&self, address: &KnowledgeAddress) -> Result<aikit_core::KnowledgeRelationView> {\n        KnowledgeOperations::relations(&self.application(), address)\n    }",
    "    pub fn relations(\n        &self,\n        address: &KnowledgeAddress,\n        depth: u8,\n        max_nodes: usize,\n        max_edges: usize,\n    ) -> Result<aikit_core::KnowledgeRelationView> {\n        KnowledgeOperations::relations(&self.application(), address, depth, max_nodes, max_edges)\n    }",
)

replace(
    "crates/aikit-tui/src/backend.rs",
    "    fn knowledge_relations(&self, _address: &KnowledgeAddress) -> Result<KnowledgeRelationView> {\n        Err(knowledge_unavailable())\n    }",
    "    fn knowledge_relations(\n        &self,\n        _address: &KnowledgeAddress,\n        _depth: u8,\n        _max_nodes: usize,\n        _max_edges: usize,\n    ) -> Result<KnowledgeRelationView> {\n        Err(knowledge_unavailable())\n    }",
)

replace(
    "crates/aikit-tui/src/application_service.rs",
    "    pub fn knowledge_relations(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeRelationView> { self.backend.knowledge_relations(address) }",
    "    pub fn knowledge_relations(\n        &self,\n        address: &aikit_core::KnowledgeAddress,\n        depth: u8,\n        max_nodes: usize,\n        max_edges: usize,\n    ) -> Result<aikit_core::KnowledgeRelationView> {\n        self.backend.knowledge_relations(address, depth, max_nodes, max_edges)\n    }",
)

replace(
    "crates/aikit-cli/src/app/mod.rs",
    "    fn knowledge_relations(&self, address: &aikit_core::KnowledgeAddress) -> Result<aikit_core::KnowledgeRelationView> {\n        crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?.relations(address)\n    }",
    "    fn knowledge_relations(\n        &self,\n        address: &aikit_core::KnowledgeAddress,\n        depth: u8,\n        max_nodes: usize,\n        max_edges: usize,\n    ) -> Result<aikit_core::KnowledgeRelationView> {\n        crate::knowledge_runtime::KnowledgeRuntime::from_service(self)?\n            .relations(address, depth, max_nodes, max_edges)\n    }",
)

replace(
    "crates/aikit-cli/src/cli.rs",
    "    Relations { address: String },",
    "    Relations {\n        address: String,\n        #[arg(long, default_value_t = 1)]\n        depth: u8,\n        #[arg(long, default_value_t = 128)]\n        max_nodes: usize,\n        #[arg(long, default_value_t = 256)]\n        max_edges: usize,\n    },",
)

replace(
    "crates/aikit-cli/src/main.rs",
    "        KnowledgeSub::Relations { address } => serde_json::to_value(runtime.relations(&parse_address(&address)?)?),",
    "        KnowledgeSub::Relations { address, depth, max_nodes, max_edges } => serde_json::to_value(\n            runtime.relations(&parse_address(&address)?, depth, max_nodes, max_edges)?\n        ),",
)
