from pathlib import Path
import subprocess


def replace(path, old, new, count=1):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"anchor missing in {path}: {old[:160]!r}")
    p.write_text(text.replace(old, new, count))

# Eliminate formatter-only churn across the whole core crate by taking the exact
# current main files, then reapply only Knowledge semantics owned by this PR.
subprocess.check_call(['git', 'checkout', 'origin/main', '--', 'crates/aikit-core/src'])
subprocess.check_call(['git', 'checkout', 'origin/main', '--', 'crates/aikit-store/src/lib.rs'])

# knowledge.rs: context-pack uncertainty is derived only from materialised evidence.
replace(
    'crates/aikit-core/src/knowledge.rs',
    '''    #[serde(default)]
    pub explanations: Vec<String>,
    #[serde(default)]
    pub budget: ContextPackBudget,''',
    '''    #[serde(default)]
    pub explanations: Vec<String>,
    /// Conflicts observed in the materialised retrieval result. This never authors
    /// a provider relation or resolves the contradiction on the provider's behalf.
    #[serde(default)]
    pub contradictions: Vec<String>,
    /// Questions left open by explicit provider/read absences.
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub budget: ContextPackBudget,''',
)
replace(
    'crates/aikit-core/src/knowledge.rs',
    '''            absences: Vec::new(),
            explanations: Vec::new(),
            budget: ContextPackBudget::default(),''',
    '''            absences: Vec::new(),
            explanations: Vec::new(),
            contradictions: Vec::new(),
            open_questions: Vec::new(),
            budget: ContextPackBudget::default(),''',
)
replace(
    'crates/aikit-core/src/knowledge.rs',
    '''    }
}

#[cfg(test)]
mod tests {''',
    '''    }

    /// Derive only uncertainty already evidenced by this pack. Provider-owned
    /// semantics are not normalised or silently reconciled here.
    pub fn derive_uncertainty(&mut self) {
        self.contradictions.clear();
        for (index, left) in self.readings.iter().enumerate() {
            for right in self.readings.iter().skip(index + 1) {
                if left.resource != right.resource {
                    continue;
                }
                let conflicts = left.revision != right.revision
                    || left.content != right.content
                    || left.authority != right.authority;
                if conflicts {
                    let finding = format!(
                        "conflicting materialised readings for {} (provider/revision/authority evidence differs)",
                        left.resource
                    );
                    if !self.contradictions.contains(&finding) {
                        self.contradictions.push(finding);
                    }
                }
            }
        }
        self.open_questions = self
            .absences
            .iter()
            .map(|absence| format!("unresolved provider/material question: {absence}"))
            .collect();
    }
}

#[cfg(test)]
mod tests {''',
)

# knowledge_navigation.rs: provider results retain native score; application layer
# can attach explicit learned evidence without rewriting provider relevance.
replace(
    'crates/aikit-core/src/knowledge_navigation.rs',
    'use crate::familiarity::FamiliarityContext;',
    'use crate::familiarity::{AccessibilityAssessment, FamiliarityContext};',
)
replace(
    'crates/aikit-core/src/knowledge_navigation.rs',
    '''pub struct KnowledgeSearchHit {
    pub address: KnowledgeAddress,
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub label: String,
    pub score: f64,
    #[serde(default)]
    pub snippet: String,
    pub provider: ProviderRef,
    pub authority: SourceAuthority,
}''',
    '''pub struct KnowledgeSearchHit {
    pub address: KnowledgeAddress,
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub label: String,
    /// Provider-native relevance score. Learned accessibility never overwrites it.
    pub score: f64,
    #[serde(default)]
    pub snippet: String,
    pub provider: ProviderRef,
    pub authority: SourceAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranking: Option<KnowledgeRankingEvidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeRankingEvidence {
    pub provider_score: f64,
    pub navigation_score: f64,
    pub destination: AccessibilityAssessment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<AccessibilityAssessment>,
}''',
)
p = Path('crates/aikit-core/src/knowledge_navigation.rs')
text = p.read_text()
text = text.replace('                    authority: SourceAuthority::Authored,\n                }', '                    authority: SourceAuthority::Authored,\n                    ranking: None,\n                }')
text = text.replace('                        authority: SourceAuthority::Observed,\n                    }', '                        authority: SourceAuthority::Observed,\n                        ranking: None,\n                    }')
text = text.replace('                            authority: SourceAuthority::Derived,\n                        }', '                            authority: SourceAuthority::Derived,\n                            ranking: None,\n                        }')
text = text.replace('                    authority: endpoint.authority,\n                })', '                    authority: endpoint.authority,\n                    ranking: None,\n                })')
p.write_text(text)
replace(
    'crates/aikit-core/src/knowledge_navigation.rs',
    '''        if let Ok(route) = self.route(query, addresses) {
            pack.routes.push(route);
        }
        pack
    }''',
    '''        if let Ok(route) = self.route(query, addresses) {
            pack.routes.push(route);
        }
        pack.derive_uncertainty();
        pack
    }''',
)

replace(
    'crates/aikit-core/src/lib.rs',
    '''    KnowledgeAddress, KnowledgeApplication, KnowledgeExplanation, KnowledgeProviderStatus,
    KnowledgeSearchHit, KnowledgeSearchResult, SourcePoolBinding, KNOWLEDGE_APPLICATION_VERSION,
};''',
    '''    KnowledgeAddress, KnowledgeApplication, KnowledgeExplanation, KnowledgeProviderStatus,
    KnowledgeRankingEvidence, KnowledgeSearchHit, KnowledgeSearchResult, SourcePoolBinding,
    KNOWLEDGE_APPLICATION_VERSION,
};''',
)

# Store module/export only; provider truth remains outside this state module.
replace(
    'crates/aikit-store/src/lib.rs',
    'pub mod index;\n',
    'pub mod index;\npub mod knowledge_application;\n',
)
replace(
    'crates/aikit-store/src/lib.rs',
    'pub use index::{CapsuleFilter, CapsuleRow, Facets, Index, ReindexReport};\n',
    '''pub use index::{CapsuleFilter, CapsuleRow, Facets, Index, ReindexReport};
pub use knowledge_application::{
    KnowledgeApplicationReceipt, KnowledgeApplicationStore, KnowledgeHistoryOperation,
    KNOWLEDGE_APPLICATION_STORE_VERSION,
};
''',
)
