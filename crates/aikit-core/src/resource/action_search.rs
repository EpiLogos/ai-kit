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

// ---------------------------------------------------------------------------
// Task-phrase matching: significant-word overlap over descriptor text.
//
// A task phrase ("verify this implementation") is not a handle: no resource is
// named by it, so exact/containment matching resolves to nothing. These
// helpers score the words the phrase actually carries against the searchable
// descriptor corpus — names, descriptions, ids, tags — so task language
// surfaces the practices and Actions that speak the same words. This is still
// ranking of already-searchable descriptors only: no new registry, no
// natural-language execution, search stays an inert reading.
// ---------------------------------------------------------------------------

/// English function words and generic interrogatives that name no resource.
/// They are dropped before scoring so a phrase is judged by the words that
/// carry its meaning.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "do", "does", "for", "from", "how", "i", "in", "is", "it", "its",
    "me", "my", "of", "on", "or", "our", "show", "that", "the", "these", "this", "those", "to",
    "was", "were", "what", "when", "where", "which", "who", "why", "with", "you", "your",
];

/// The significant words of a task-language query: lowercased, stripped of
/// punctuation, stop-words dropped. A query made only of stop-words keeps its
/// raw terms — the phrase is what the searcher meant, and an empty term list
/// would agree with every record.
pub fn significant_terms(query: &str) -> Vec<String> {
    fn clean(term: &str) -> Vec<String> {
        term.trim_matches(|c: char| !c.is_alphanumeric())
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(|word| word.to_lowercase())
            .collect()
    }
    let terms: Vec<String> = query
        .split_whitespace()
        .flat_map(clean)
        .filter(|term| !STOP_WORDS.contains(&term.as_str()))
        .collect();
    if terms.is_empty() {
        query.split_whitespace().flat_map(clean).collect()
    } else {
        terms
    }
}

/// Two words name the same thing when they are equal or share a stem: a
/// common prefix of at least four characters ("verify" and "verification",
/// "implement" and "implementation"). A shorter shared prefix is accident,
/// not agreement.
fn words_agree(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let common = left
        .chars()
        .zip(right.chars())
        .take_while(|(left_char, right_char)| left_char == right_char)
        .count();
    common >= 4
}

/// Word-overlap score of one descriptor against the significant words of a
/// task-language query. `primary` is the descriptor's own name and id; `text`
/// the rest of its searchable corpus text (description, tags, relations).
///
/// The score is the count of query terms some word of the descriptor agrees
/// with, plus a bonus for each term the primary text agrees with — a practice
/// named by the task's word is a stronger answer than one that merely mentions
/// it — and a small exact-word bonus. `None` when nothing overlaps. This is
/// the resolver's fallback lane: it always ranks below an explicit containment
/// match and never mints a second identity.
pub fn word_overlap_score(terms: &[String], primary: &str, text: &str) -> Option<i64> {
    if terms.is_empty() {
        return None;
    }
    fn agreeing_terms(terms: &[String], text: &str) -> (usize, bool) {
        let mut matched = 0_usize;
        let mut exact = false;
        for term in terms {
            let mut term_match = false;
            for word in text
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(str::to_lowercase)
            {
                if word == *term {
                    term_match = true;
                    exact = true;
                    break;
                }
                if words_agree(&word, term) {
                    term_match = true;
                }
            }
            matched += usize::from(term_match);
        }
        (matched, exact)
    }
    let (matched_text, exact_text) = agreeing_terms(terms, text);
    let (matched_primary, _) = agreeing_terms(terms, primary);
    let matched = matched_text.max(matched_primary);
    (matched > 0)
        .then_some(matched as i64 * 100 + matched_primary as i64 * 80 + i64::from(exact_text) * 40)
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

    #[test]
    fn significant_terms_drop_stop_words_and_punctuation() {
        assert_eq!(
            significant_terms("verify this implementation"),
            vec!["verify".to_owned(), "implementation".to_owned()]
        );
        assert_eq!(
            significant_terms("the how a this"),
            vec![
                "the".to_owned(),
                "how".to_owned(),
                "a".to_owned(),
                "this".to_owned()
            ]
        );
        assert_eq!(
            significant_terms("Verify, the implementation."),
            vec!["verify".to_owned(), "implementation".to_owned()]
        );
    }

    #[test]
    fn word_overlap_counts_agreeing_terms_and_ranks_by_count() {
        let terms = significant_terms("verify this implementation");
        let two = word_overlap_score(
            &terms,
            "verification-before-completion",
            "Verify the implementation before claiming completion",
        );
        let one = word_overlap_score(&terms, "closure", "verification runs and closure evidence");
        assert!(
            two.unwrap() > one.unwrap(),
            "more agreeing terms outrank fewer"
        );
        assert_eq!(
            word_overlap_score(&terms, "entry", "an unrelated catalogue entry"),
            None
        );
        assert_eq!(
            word_overlap_score(&[], "verification", "verification"),
            None
        );
    }

    #[test]
    fn word_overlap_agrees_by_stem_not_by_accident() {
        // Stems agree: verify/verification, implement/implementation.
        assert!(
            word_overlap_score(&significant_terms("verify"), "runs", "verification runs").is_some()
        );
        assert!(word_overlap_score(
            &significant_terms("implement"),
            "plan",
            "implementation plan"
        )
        .is_some());
        // A three-character shared prefix is not agreement.
        assert!(
            word_overlap_score(&significant_terms("act"), "catalogue", "action catalogue")
                .is_none()
        );
        assert!(
            word_overlap_score(&significant_terms("rat"), "manual", "operation manual").is_none()
        );
    }

    #[test]
    fn word_overlap_weights_a_name_agreement_above_a_mention() {
        let terms = significant_terms("verify this implementation");
        let named = word_overlap_score(
            &terms,
            "verification-before-completion",
            "never claim completion without proof",
        );
        let mentioned = word_overlap_score(
            &terms,
            "receiving-code-review",
            "before implementing suggestions from review",
        );
        assert!(
            named.unwrap() > mentioned.unwrap(),
            "a practice named by the task's word outranks one that merely mentions it"
        );
    }
}
