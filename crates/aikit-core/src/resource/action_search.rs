//! Search within the contextual Actions already resolved for one selected Resource.
//!
//! This is navigation search only. It ranks Action descriptors without changing
//! their stageability, trust, eligibility, subject, or canonical Action identity.

use super::ContextualActionDescriptor;

/// Return contextual Actions ordered by a small deterministic fzf-like score.
/// Empty query preserves the authored/action-provider order.
pub fn search_contextual_actions(
    actions: &[ContextualActionDescriptor],
    query: &str,
) -> Vec<ContextualActionDescriptor> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return actions.to_vec();
    }

    let mut ranked = actions
        .iter()
        .filter_map(|action| {
            let mut score = [
                action.label.as_str(),
                action.description.as_str(),
                action.action.as_str(),
            ]
            .iter()
            .filter_map(|candidate| fuzzy_score(&query, candidate))
            .max();
            for keyword in &action.keywords {
                score = score.max(fuzzy_score(&query, keyword));
            }
            score.map(|score| (score, action.clone()))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.action.cmp(&right.action))
    });
    ranked.into_iter().map(|(_, action)| action).collect()
}

fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    let candidate = candidate.to_lowercase();
    if let Some(position) = candidate.find(query) {
        let prefix = if position == 0 { 2_000 } else { 0 };
        return Some(10_000 + prefix - position as i64 * 5 - candidate.len() as i64);
    }

    let mut query_chars = query.chars();
    let mut wanted = query_chars.next()?;
    let mut score = 0_i64;
    let mut first = None;
    let mut last_match = None;
    let mut previous = None;
    for (index, current) in candidate.chars().enumerate() {
        if current == wanted {
            first.get_or_insert(index);
            score += 100;
            if last_match == Some(index.saturating_sub(1)) {
                score += 60;
            }
            if index == 0 || previous.is_some_and(|value: char| !value.is_alphanumeric()) {
                score += 40;
            }
            last_match = Some(index);
            match query_chars.next() {
                Some(next) => wanted = next,
                None => {
                    let start = first.unwrap_or_default();
                    return Some(score - start as i64 * 3 - index.saturating_sub(start) as i64);
                }
            }
        }
        previous = Some(current);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{ActionStageability, ResourceRef};

    fn action(id: &str, label: &str, keywords: &[&str]) -> ContextualActionDescriptor {
        ContextualActionDescriptor::new(
            ResourceRef::parse(id).unwrap(),
            ResourceRef::parse("project/aikit").unwrap(),
            label,
            "contextual operation",
            ActionStageability::NotStageable,
        )
        .with_keywords(keywords.iter().copied())
    }

    #[test]
    fn empty_query_preserves_provider_order() {
        let actions = vec![
            action("action/project/open", "Open workspace", &["enter"]),
            action("action/project/explain", "Explain project", &["why"]),
        ];
        assert_eq!(search_contextual_actions(&actions, ""), actions);
    }

    #[test]
    fn fuzzy_query_searches_label_description_id_and_keywords() {
        let actions = vec![
            action("action/project/open", "Open workspace", &["enter"]),
            action(
                "action/project/explain",
                "Explain project",
                &["why", "provenance"],
            ),
        ];
        let results = search_contextual_actions(&actions, "prov");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].action.as_str(), "action/project/explain");
    }
}
