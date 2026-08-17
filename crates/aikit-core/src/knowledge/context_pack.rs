//! Deterministic composition of already-retrieved Knowledge readings.
//!
//! Provider search, retrieval, ranking and native semantics happen before this
//! module. Composition only answers: given explicitly selected ResourceRefs and
//! provenance-bearing readings/routes, what can be materialised into a bounded
//! context pack without silently inventing or dropping knowledge?

use std::collections::BTreeSet;

use crate::guidance::estimate_tokens;
use crate::resource::ResourceRef;

use super::{
    ContextPackBudget, KnowledgeContextPack, KnowledgeReading, KnowledgeRoute,
};
use crate::FamiliarityContext;

/// Compose a derived context pack from provider-selected inputs.
///
/// Ordering is stable and caller-owned: selected ResourceRefs, routes and readings
/// retain their input order. AIKit does not re-rank provider results here. When a
/// token limit is present, readings are admitted in that order until each next
/// reading would exceed the limit; omitted material is recorded explicitly.
pub fn compose_context_pack(
    context: FamiliarityContext,
    query: Option<String>,
    selected: Vec<ResourceRef>,
    routes: Vec<KnowledgeRoute>,
    readings: Vec<KnowledgeReading>,
    token_limit: Option<usize>,
) -> KnowledgeContextPack {
    let mut pack = KnowledgeContextPack::new(context);
    pack.query = query;

    let mut seen = BTreeSet::new();
    pack.selected = selected
        .into_iter()
        .filter(|resource| seen.insert(resource.clone()))
        .collect();
    let selected_set: BTreeSet<_> = pack.selected.iter().cloned().collect();

    // Routes are evidence of actual or proposed navigation, not additional
    // selection authority. Keep only routes that intersect this pack's selected
    // field (or every route when no explicit selection was supplied).
    pack.routes = routes
        .into_iter()
        .filter(|route| {
            selected_set.is_empty()
                || route
                    .steps
                    .iter()
                    .any(|step| selected_set.contains(&step.resource))
        })
        .collect();

    let mut materialised_tokens = 0usize;
    let mut represented = BTreeSet::new();

    for reading in readings {
        if !selected_set.is_empty() && !selected_set.contains(&reading.resource) {
            continue;
        }

        represented.insert(reading.resource.clone());
        let tokens = reading
            .content
            .as_deref()
            .map(estimate_tokens)
            .unwrap_or_default();

        if token_limit.is_some_and(|limit| materialised_tokens.saturating_add(tokens) > limit) {
            pack.budget.truncated = true;
            pack.absences.push(format!(
                "budget omitted reading {}{}{}",
                reading.resource,
                reading
                    .provider
                    .as_ref()
                    .map(|provider| format!(" from {provider}"))
                    .unwrap_or_default(),
                reading
                    .lens
                    .as_ref()
                    .map(|lens| format!(" through {lens}"))
                    .unwrap_or_default(),
            ));
            continue;
        }

        materialised_tokens = materialised_tokens.saturating_add(tokens);
        pack.explanations.push(format!(
            "{}: {}",
            reading.resource, reading.why_selected
        ));
        pack.readings.push(reading);
    }

    for resource in &pack.selected {
        if !represented.contains(resource) {
            pack.absences.push(format!(
                "selected resource {resource} had no retrieved Knowledge reading"
            ));
        }
    }

    pack.budget = ContextPackBudget {
        token_limit,
        materialised_tokens: Some(materialised_tokens),
        truncated: pack.budget.truncated,
    };
    pack
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{ProviderRef, SourceAuthority};
    use crate::{KnowledgeReading, KnowledgeRouteStep};

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn reading(resource: &str, content: &str, why: &str) -> KnowledgeReading {
        KnowledgeReading {
            resource: r(resource),
            provider: Some(ProviderRef::parse("provider/wiki").unwrap()),
            lens: Some("semantic-wiki".into()),
            revision: Some("wiki-r7".into()),
            freshness: Some("current".into()),
            authority: SourceAuthority::Authored,
            content: Some(content.into()),
            evidence: Vec::new(),
            why_selected: why.into(),
        }
    }

    #[test]
    fn composition_preserves_selection_order_provenance_and_explicit_absence() {
        let selected = vec![r("knowledge-node/auth"), r("knowledge-node/missing")];
        let pack = compose_context_pack(
            FamiliarityContext::default(),
            Some("session auth".into()),
            selected.clone(),
            Vec::new(),
            vec![reading(
                "knowledge-node/auth",
                "session identity remains stable",
                "matched the selected project concept",
            )],
            None,
        );

        assert_eq!(pack.selected, selected);
        assert_eq!(pack.readings.len(), 1);
        assert_eq!(pack.readings[0].authority, SourceAuthority::Authored);
        assert_eq!(pack.readings[0].revision.as_deref(), Some("wiki-r7"));
        assert!(pack
            .absences
            .iter()
            .any(|absence| absence.contains("knowledge-node/missing")));
        assert!(pack
            .explanations
            .iter()
            .any(|why| why.contains("matched the selected project concept")));
    }

    #[test]
    fn budget_truncation_is_explicit_and_does_not_reorder_readings() {
        let first = reading("knowledge-node/first", "small", "first");
        let second = reading(
            "knowledge-node/second",
            "this reading is deliberately much larger than the first reading",
            "second",
        );
        let first_tokens = estimate_tokens(first.content.as_deref().unwrap());
        let pack = compose_context_pack(
            FamiliarityContext::default(),
            None,
            vec![first.resource.clone(), second.resource.clone()],
            Vec::new(),
            vec![first.clone(), second.clone()],
            Some(first_tokens),
        );

        assert_eq!(pack.readings, vec![first]);
        assert!(pack.budget.truncated);
        assert_eq!(pack.budget.materialised_tokens, Some(first_tokens));
        assert!(pack
            .absences
            .iter()
            .any(|absence| absence.contains("budget omitted reading knowledge-node/second")));
    }

    #[test]
    fn routes_are_kept_only_when_they_intersect_selected_resources() {
        let mut relevant = KnowledgeRoute::new(
            r("knowledge-route/relevant"),
            FamiliarityContext::default(),
        );
        relevant.steps.push(KnowledgeRouteStep::new(
            r("knowledge-node/auth"),
            SourceAuthority::Authored,
        ));
        let mut unrelated = KnowledgeRoute::new(
            r("knowledge-route/unrelated"),
            FamiliarityContext::default(),
        );
        unrelated.steps.push(KnowledgeRouteStep::new(
            r("knowledge-node/other"),
            SourceAuthority::Derived,
        ));

        let pack = compose_context_pack(
            FamiliarityContext::default(),
            None,
            vec![r("knowledge-node/auth")],
            vec![relevant.clone(), unrelated],
            vec![reading("knowledge-node/auth", "auth", "selected")],
            None,
        );

        assert_eq!(pack.routes, vec![relevant]);
    }
}