from pathlib import Path


def replace(path, old, new, count=1):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"anchor missing in {path}: {old[:120]!r}")
    text = text.replace(old, new, count)
    p.write_text(text)

# Store: export durable Knowledge application state.
replace(
    "crates/aikit-store/src/lib.rs",
    "pub mod index;\n",
    "pub mod index;\npub mod knowledge_application;\n",
)
replace(
    "crates/aikit-store/src/lib.rs",
    "pub use index::{CapsuleFilter, CapsuleRow, Facets, Index, ReindexReport};\n",
    "pub use index::{CapsuleFilter, CapsuleRow, Facets, Index, ReindexReport};\npub use knowledge_application::{\n    KnowledgeApplicationReceipt, KnowledgeApplicationStore, KnowledgeHistoryOperation,\n    KNOWLEDGE_APPLICATION_STORE_VERSION,\n};\n",
)

# Store: retain a rebuildable ResourceRef -> typed KnowledgeAddress cache from live search.
p = Path("crates/aikit-store/src/knowledge_application.rs")
text = p.read_text()
text = text.replace("use std::fs;", "use std::collections::BTreeMap;\nuse std::fs;")
text = text.replace(
    "use aikit_core::{AikitError, FamiliarityContext, Result};",
    "use aikit_core::{\n    AikitError, FamiliarityContext, KnowledgeAddress, KnowledgeSearchHit, Result,\n};",
)
text = text.replace(
    "    #[serde(default)]\n    receipts: Vec<KnowledgeApplicationReceipt>,\n}",
    "    #[serde(default)]\n    receipts: Vec<KnowledgeApplicationReceipt>,\n    /// Rebuildable provider-address bindings observed from live Knowledge search.\n    /// Canonical ResourceRef/provider truth remains outside this cache.\n    #[serde(default)]\n    addresses: BTreeMap<ResourceRef, KnowledgeAddress>,\n}",
)
text = text.replace(
    "            receipts: Vec::new(),\n        }",
    "            receipts: Vec::new(),\n            addresses: BTreeMap::new(),\n        }",
)
text = text.replace(
    "    pub fn append_route(&self, route: KnowledgeRoute) -> Result<KnowledgeApplicationReceipt> {",
    "    pub fn remember_search_hits(&self, hits: &[KnowledgeSearchHit]) -> Result<()> {\n        let mut state = self.load()?;\n        for hit in hits {\n            state\n                .addresses\n                .insert(hit.resource.clone(), hit.address.clone());\n        }\n        self.save(&state)\n    }\n\n    pub fn address(&self, resource: &ResourceRef) -> Result<Option<KnowledgeAddress>> {\n        Ok(self.load()?.addresses.get(resource).cloned())\n    }\n\n    pub fn append_route(&self, route: KnowledgeRoute) -> Result<KnowledgeApplicationReceipt> {",
)
p.write_text(text)

# Core context-pack acceptance: explicit uncertainty fields derived only from actual readings/absences.
p = Path("crates/aikit-core/src/knowledge.rs")
text = p.read_text()
text = text.replace(
    "    #[serde(default)]\n    pub explanations: Vec<String>,\n    #[serde(default)]\n    pub budget: ContextPackBudget,",
    "    #[serde(default)]\n    pub explanations: Vec<String>,\n    /// Conflicts observed in the materialised retrieval result. This never authors\n    /// a provider relation or resolves the contradiction on the provider's behalf.\n    #[serde(default)]\n    pub contradictions: Vec<String>,\n    /// Questions left open by explicit provider/read absences.\n    #[serde(default)]\n    pub open_questions: Vec<String>,\n    #[serde(default)]\n    pub budget: ContextPackBudget,",
)
text = text.replace(
    "            explanations: Vec::new(),\n            budget: ContextPackBudget::default(),",
    "            explanations: Vec::new(),\n            contradictions: Vec::new(),\n            open_questions: Vec::new(),\n            budget: ContextPackBudget::default(),",
)
text = text.replace(
    "    }\n}\n\n#[cfg(test)]\nmod tests {",
    "    }\n\n    /// Derive only uncertainty already evidenced by this pack. Provider-owned\n    /// semantics are not normalised or silently reconciled here.\n    pub fn derive_uncertainty(&mut self) {\n        self.contradictions.clear();\n        for (index, left) in self.readings.iter().enumerate() {\n            for right in self.readings.iter().skip(index + 1) {\n                if left.resource != right.resource {\n                    continue;\n                }\n                let conflicts = left.revision != right.revision\n                    || left.content != right.content\n                    || left.authority != right.authority;\n                if conflicts {\n                    let finding = format!(\n                        \"conflicting materialised readings for {} (provider/revision/authority evidence differs)\",\n                        left.resource\n                    );\n                    if !self.contradictions.contains(&finding) {\n                        self.contradictions.push(finding);\n                    }\n                }\n            }\n        }\n        self.open_questions = self\n            .absences\n            .iter()\n            .map(|absence| format!(\"unresolved provider/material question: {absence}\"))\n            .collect();\n    }\n}\n\n#[cfg(test)]\nmod tests {",
)
p.write_text(text)

# Production Service owns one cached materialised Knowledge runtime.
replace(
    "crates/aikit-cli/src/app/mod.rs",
    "use crate::run::{self, RunReport};\n",
    "use crate::run::{self, RunReport};\n\nmod knowledge;\n",
)
replace(
    "crates/aikit-cli/src/app/mod.rs",
    "    invocation_cwd: PathBuf,\n}",
    "    invocation_cwd: PathBuf,\n    knowledge_runtime: std::cell::RefCell<Option<knowledge::KnowledgeRuntime>>,\n}",
)
replace(
    "crates/aikit-cli/src/app/mod.rs",
    "            invocation_cwd: cwd.to_path_buf(),\n        })",
    "            invocation_cwd: cwd.to_path_buf(),\n            knowledge_runtime: std::cell::RefCell::new(None),\n        })",
)
replace(
    "crates/aikit-cli/src/app/mod.rs",
    "        self.view = resolve_or_explain(\n            &self.catalog,\n            &self.trust,\n            &self.descriptor,\n            &self.layers,\n            &self.policy,\n        )?;\n        Ok(())\n    }\n\n    /// Where this context's client projections are materialised.",
    "        self.view = resolve_or_explain(\n            &self.catalog,\n            &self.trust,\n            &self.descriptor,\n            &self.layers,\n            &self.policy,\n        )?;\n        self.invalidate_knowledge_runtime();\n        Ok(())\n    }\n\n    /// Where this context's client projections are materialised.",
)
# apply() has another resolve assignment; invalidate after it.
needle = "        self.view = resolve_or_explain(\n            &self.catalog,\n            &self.trust,\n            &self.descriptor,\n            &self.layers,\n            &self.policy,\n        )?;\n\n        // 3. Build and commit a generation."
replace(
    "crates/aikit-cli/src/app/mod.rs",
    needle,
    needle.replace("\n\n        // 3.", "\n        self.invalidate_knowledge_runtime();\n\n        // 3."),
)

# Avoid a new CLI dependency just for observation IDs.
replace(
    "crates/aikit-cli/src/app/knowledge.rs",
    'format!("knowledge-route-use/{}", ulid::Ulid::new())',
    'format!("knowledge-route-use/{}", aikit_core::EventId::generate())',
)
