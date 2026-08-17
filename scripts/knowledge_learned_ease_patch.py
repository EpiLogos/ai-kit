from pathlib import Path


def replace(path, old, new, count=1):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"anchor missing in {path}: {old[:160]!r}")
    p.write_text(text.replace(old, new, count))

# Remove the broad rustfmt churn from this established core file, then apply only
# the two semantic additions this branch actually owns.
import subprocess
base = subprocess.check_output([
    'git', 'show', 'origin/main:crates/aikit-core/src/knowledge_navigation.rs'
], text=True)
p = Path('crates/aikit-core/src/knowledge_navigation.rs')
p.write_text(base)
replace(
    str(p),
    'use crate::familiarity::FamiliarityContext;',
    'use crate::familiarity::{AccessibilityAssessment, FamiliarityContext};',
)
replace(
    str(p),
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
    /// Application-level learned-ease evidence and combined ordering score.
    /// This is absent in provider-native results and attached only by an eligible
    /// application consumer after provider/trust/privacy filtering.
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
# Every provider-owned hit starts with no learned ranking evidence.
text = p.read_text()
text = text.replace('                    authority: SourceAuthority::Authored,\n                }', '                    authority: SourceAuthority::Authored,\n                    ranking: None,\n                }')
text = text.replace('                        authority: SourceAuthority::Observed,\n                    }', '                        authority: SourceAuthority::Observed,\n                        ranking: None,\n                    }')
text = text.replace('                            authority: SourceAuthority::Derived,\n                        }', '                            authority: SourceAuthority::Derived,\n                            ranking: None,\n                        }')
text = text.replace('                    authority: endpoint.authority,\n                })', '                    authority: endpoint.authority,\n                    ranking: None,\n                })')
p.write_text(text)
replace(
    str(p),
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

# Re-export the explicit distinction between provider relevance and learned ease.
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

# Fix two paths exposed by the first full PR compile.
p = Path('crates/aikit-cli/src/app/mod.rs')
text = p.read_text().replace('&aikit_core::ResourceRef', '&aikit_core::resource::ResourceRef')
text = text.replace('Option<&aikit_core::ResourceRef>', 'Option<&aikit_core::resource::ResourceRef>')
p.write_text(text)

# Apply learned ease only after provider eligibility/materialisation. Provider score
# remains intact; the combined score is carried separately and exact/addressed hits
# remain ahead of familiarity-promoted fuzzy hits.
p = Path('crates/aikit-cli/src/app/knowledge.rs')
text = p.read_text()
text = text.replace(
    '    KnowledgeExplanation, KnowledgeProviderStatus, KnowledgeSearchResult, KnowledgeSources, Result,\n',
    '    KnowledgeExplanation, KnowledgeProviderStatus, KnowledgeRankingEvidence, KnowledgeSearchResult,\n    KnowledgeSources, Result, DEFAULT_FAMILIARITY_HALF_LIFE_MS,\n',
)
old = '''    pub fn knowledge_search(&self, query: &str, limit: usize) -> Result<KnowledgeSearchResult> {
        let result = self.with_knowledge(|runtime, application| {
            let mut result = application.search(query, limit);
            result.absences.extend(runtime.absences.clone());
            Ok(result)
        })?;
        let mut result = result;
        if let Err(error) = self.knowledge_store().remember_search_hits(&result.hits) {
            result.absences.push(format!(
                "Knowledge address cache unavailable; live search results remain valid: {}",
                error.message()
            ));
        }
        Ok(result)
    }'''
new = '''    pub fn knowledge_search(&self, query: &str, limit: usize) -> Result<KnowledgeSearchResult> {
        let candidate_limit = if limit == 0 { 0 } else { limit.max(256) };
        let mut result = self.with_knowledge(|runtime, application| {
            let mut result = application.search(query, candidate_limit);
            result.absences.extend(runtime.absences.clone());
            Ok(result)
        })?;
        self.apply_learned_accessibility(query, &mut result)?;
        result.hits.truncate(limit);
        if let Err(error) = self.knowledge_store().remember_search_hits(&result.hits) {
            result.absences.push(format!(
                "Knowledge address cache unavailable; live search results remain valid: {}",
                error.message()
            ));
        }
        Ok(result)
    }

    fn apply_learned_accessibility(
        &self,
        query: &str,
        result: &mut KnowledgeSearchResult,
    ) -> Result<()> {
        let Some(store) = PaletteBackend::familiarity(self)? else {
            return Ok(());
        };
        if store.is_empty() {
            return Ok(());
        }
        let context = self.knowledge_context();
        let now = now_ms();
        let history = self.knowledge_store().history(Some(&context), None)?;
        let mut influenced = false;
        for hit in &mut result.hits {
            let destination = store.assess_destination(
                &hit.resource,
                &context,
                now,
                DEFAULT_FAMILIARITY_HALF_LIFE_MS,
            );
            let route = history
                .iter()
                .filter_map(|receipt| receipt.route.as_ref())
                .filter(|route| route.destination() == Some(&hit.resource))
                .map(|route| {
                    store.assess_route(
                        &route.route,
                        &hit.resource,
                        &context,
                        now,
                        DEFAULT_FAMILIARITY_HALF_LIFE_MS,
                    )
                })
                .filter(|assessment| !assessment.is_empty())
                .max_by(|left, right| {
                    left.contextual_frecency
                        .total_cmp(&right.contextual_frecency)
                        .then_with(|| left.frecency.total_cmp(&right.frecency))
                });
            let learned = destination.contextual_frecency
                + route
                    .as_ref()
                    .map(|assessment| assessment.contextual_frecency)
                    .unwrap_or_default();
            // Bounded, monotonic application boost. It can re-order eligible fuzzy
            // candidates but can never change provider score or eligibility.
            let boost = (learned.ln_1p() * 0.08).min(0.35);
            influenced |= boost > 0.0;
            hit.ranking = Some(KnowledgeRankingEvidence {
                provider_score: hit.score,
                navigation_score: hit.score + boost,
                destination,
                route,
            });
        }
        if influenced {
            result.hits.sort_by(|left, right| {
                exact_knowledge_hit(left, query)
                    .cmp(&exact_knowledge_hit(right, query))
                    .reverse()
                    .then_with(|| {
                        let left_score = left
                            .ranking
                            .as_ref()
                            .map(|ranking| ranking.navigation_score)
                            .unwrap_or(left.score);
                        let right_score = right
                            .ranking
                            .as_ref()
                            .map(|ranking| ranking.navigation_score)
                            .unwrap_or(right.score);
                        right_score.total_cmp(&left_score)
                    })
                    .then_with(|| left.resource.cmp(&right.resource))
            });
        }
        Ok(())
    }'''
if old not in text:
    raise SystemExit('knowledge_search anchor missing')
text = text.replace(old, new, 1)
old = '''    pub fn knowledge_explain(&self, address: &KnowledgeAddress) -> Result<KnowledgeExplanation> {
        self.with_knowledge(|_, application| application.explain(address))
    }'''
new = '''    pub fn knowledge_explain(&self, address: &KnowledgeAddress) -> Result<KnowledgeExplanation> {
        let mut explanation = self.with_knowledge(|_, application| application.explain(address))?;
        // Explain keeps provider-native detail and learned ranking evidence separate.
        let resource = address.resource_ref();
        let ranking = self
            .knowledge_search(resource.as_str(), 256)?
            .hits
            .into_iter()
            .find(|hit| hit.resource == resource)
            .and_then(|hit| hit.ranking);
        if let Some(ranking) = ranking {
            explanation.detail = Some(serde_json::json!({
                "provider": explanation.detail,
                "ranking": ranking,
                "signalClasses": ["provider-relevance", "frecency", "context"]
            }));
        }
        Ok(explanation)
    }'''
if old not in text:
    raise SystemExit('knowledge_explain anchor missing')
text = text.replace(old, new, 1)
text += '''

fn exact_knowledge_hit(hit: &aikit_core::KnowledgeSearchHit, query: &str) -> bool {
    !query.is_empty()
        && (hit.resource.as_str().eq_ignore_ascii_case(query)
            || hit.label.eq_ignore_ascii_case(query))
}
'''
p.write_text(text)

# Strengthen the production test: the same route is actually reused, becomes easier
# to recover through search, and forget removes only learned ranking influence.
p = Path('crates/aikit-cli/tests/knowledge_application_v2.rs')
text = p.read_text()
text = text.replace(
    '''        let frame = service
            .knowledge_frame(Some("authentication evidence"), &[wiki.clone(), source])
            .unwrap();''',
    '''        let repeated = service
            .knowledge_route(
                Some("authentication evidence"),
                &[wiki.clone(), source.clone()],
            )
            .unwrap();
        assert_eq!(repeated.route, route.route, "identical traversal has stable route identity");

        let learned = service.knowledge_search("authentication", 50).unwrap();
        let learned_source = learned
            .hits
            .iter()
            .find(|hit| hit.resource.as_str() == "source:paper:authentication")
            .expect("learned SourcePool destination remains discoverable");
        let ranking = learned_source
            .ranking
            .as_ref()
            .expect("production search discloses learned ranking separately");
        assert_eq!(
            ranking.route.as_ref().map(|assessment| assessment.observations),
            Some(2)
        );
        assert!(ranking.navigation_score > ranking.provider_score);

        let frame = service
            .knowledge_frame(Some("authentication evidence"), &[wiki.clone(), source])
            .unwrap();''',
)
text = text.replace('assert_eq!(history.len(), 2, "route and frame receipts survive reopen");', 'assert_eq!(history.len(), 3, "two route uses and the frame survive reopen");')
text = text.replace('assert_eq!(assessment.observations, 1);', 'assert_eq!(assessment.observations, 2);')
text = text.replace(
    '''    let service = open_service(&temp);
    let learned = PaletteBackend::familiarity(&service)''',
    '''    let service = open_service(&temp);
    let search_after_forget = service.knowledge_search("authentication", 50).unwrap();
    let source_after_forget = search_after_forget
        .hits
        .iter()
        .find(|hit| hit.resource.as_str() == "source:paper:authentication")
        .unwrap();
    let ranking_after_forget = source_after_forget.ranking.as_ref().unwrap();
    assert!(ranking_after_forget.route.is_none());
    assert_eq!(
        ranking_after_forget.navigation_score,
        ranking_after_forget.provider_score,
        "forget removes learned ranking influence without changing provider relevance"
    );
    let learned = PaletteBackend::familiarity(&service)''',
)
text = text.replace(
    '''        service.knowledge_history(None).unwrap().len(),
        2,
        "forget does not erase Knowledge audit receipts"''',
    '''        service.knowledge_history(None).unwrap().len(),
        3,
        "forget does not erase Knowledge audit receipts"''',
)
p.write_text(text)
