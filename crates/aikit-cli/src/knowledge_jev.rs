//! Jev relevance selection over large knowledge-search pulls.
//!
//! The search faculty already federates every provider it holds; what it did
//! not have was the commissioned selection step (JEV-REDIS-NOW-INTEGRATION §2:
//! "Jev helps interpret the matrices and select relevant material") applied to
//! its own oversized pulls. This module is that join — and it is deliberately
//! **not** a second decision system: the invocation goes through the elected
//! provider of the provider-neutral decide route (`decide::invoke_selected`),
//! so a configured pull is scored by whatever the operator elected — the local
//! Kev recipe by default — under that route's laws (local unauthenticated
//! serving, no invented tariff, model-identity drift refusal). Without the
//! `tool/search/jev-rerank` capability — or with no provider elected — the
//! faculty behaves exactly as before; a failed selection degrades to the
//! unranked order with a named absence.

use crate::decide::{invoke_selected, parse_provider_config, DecisionProviderMode};
use crate::jev_now::minted_invocation_ref;
use aikit_core::jev::{JevRequest, JevResponse, Question};
use aikit_core::knowledge_navigation::{KnowledgeSearchHit, KnowledgeSearchResult};
use aikit_core::{AikitError, Result};
use serde_json::json;
use std::path::PathBuf;

fn invalid(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("knowledge.jev_rerank_invalid", message.to_string())
}

/// Hard bound on candidates scored in one pull. One Score question per
/// candidate, and the protocol accepts at most 256 questions; staying well
/// under it keeps the request bounded and the elected model's trained-state
/// envelope respected.
const MAX_CANDIDATES_CAP: usize = 64;

/// The five-step relevance scale every candidate is scored against. Criteria
/// entries are values, so the scale travels in the request verbatim and the
/// answer's legend is validated against it by the protocol itself.
const SCALE: [&str; 5] = [
    "0 — unrelated to the query",
    "1 — glancing: shares a word or topic, does not answer it",
    "2 — supporting: useful context for the query",
    "3 — directly answers or advances the query",
    "4 — exact subject: the query is essentially this item",
];

/// Search-side configuration: what the join needs beyond the elected
/// provider. The provider itself is never named here — it is whatever the
/// operator elected in the decision-provider file.
#[derive(Debug, Clone)]
pub struct JevRerankConfig {
    pub provider_file: PathBuf,
    pub max_candidates: usize,
    pub curl: Option<PathBuf>,
    pub allow_env_import: bool,
}

impl JevRerankConfig {
    /// Read the configuration from a capability table. `Ok(None)` — capability
    /// not configured; the search path must not change behaviour at all.
    pub fn from_table(table: &toml::Table) -> Result<Option<Self>> {
        let Some(provider_file) = table
            .get("provider_file")
            .and_then(|value| value.as_str())
            .map(PathBuf::from)
        else {
            return Ok(None);
        };
        let max_candidates = table
            .get("max_candidates")
            .and_then(|value| value.as_integer())
            .map(|value| value as usize)
            .unwrap_or(24)
            .clamp(1, MAX_CANDIDATES_CAP);
        Ok(Some(Self {
            provider_file,
            max_candidates,
            curl: table
                .get("curl")
                .and_then(|value| value.as_str())
                .map(PathBuf::from),
            allow_env_import: table
                .get("allow_env_import")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
        }))
    }
}

#[derive(Debug, Clone)]
pub struct JevRerankOutcome {
    pub candidates: usize,
    pub reordered: usize,
    pub returned_model: String,
}

/// One typed Score question per candidate: "how relevant is this candidate to
/// the query", scored on the shared five-step scale. The model selector comes
/// from the elected provider, never from this module.
fn build_rerank_request(
    query: &str,
    model: String,
    candidates: &[KnowledgeSearchHit],
) -> JevRequest {
    let state = json!({
        "query": query,
        "candidates": candidates
            .iter()
            .enumerate()
            .map(|(index, hit)| json!({
                "index": index,
                "kind": format!("{:?}", hit.kind),
                "label": hit.label,
                "provider": hit.provider.as_str(),
            }))
            .collect::<Vec<_>>(),
    });
    let mut questions = std::collections::BTreeMap::new();
    for (index, hit) in candidates.iter().enumerate() {
        questions.insert(
            format!("c{index}"),
            Question::Score {
                instructions: json!(format!(
                    "Score how relevant this candidate is to the shared query. Candidate: {}",
                    hit.label
                )),
                criteria: SCALE.iter().map(|entry| json!(entry)).collect(),
            },
        );
    }
    JevRequest {
        model,
        state,
        questions,
    }
}

/// Per-candidate scores in candidate order. The decide receipt already ran
/// the protocol's `parse_for` validation against this request — every
/// requested question has exactly one answer, or there is no receipt — so a
/// complete pull is the only success; this lifts the scores out in order.
fn scores_from_response(response: &JevResponse, candidates: usize) -> Result<Vec<f64>> {
    let mut scores = Vec::with_capacity(candidates);
    for index in 0..candidates {
        let Some(aikit_core::jev::Answer::Score { score, .. }) =
            response.answers.get(&format!("c{index}"))
        else {
            return Err(invalid(format!(
                "answer c{index} is missing or not a Score answer"
            )));
        };
        scores.push(*score);
    }
    Ok(scores)
}

/// Stable re-order by descending Jev score; ties keep the merged order, and
/// candidates beyond the scored window keep their tail positions. Returns how
/// many positions actually moved (0 means the selection agreed with the
/// faculty's own ranking).
fn apply_rerank(hits: &mut [KnowledgeSearchHit], scores: &[f64]) -> usize {
    let count = hits.len().min(scores.len());
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|left, right| {
        scores[*right]
            .total_cmp(&scores[*left])
            .then_with(|| left.cmp(right))
    });
    let before: Vec<_> = hits
        .iter()
        .take(count)
        .map(|hit| hit.resource.to_string())
        .collect();
    let mut ranked: Vec<KnowledgeSearchHit> =
        order.iter().map(|&index| hits[index].clone()).collect();
    let mut moved = 0usize;
    for (position, hit) in ranked.iter().enumerate() {
        if before[position] != hit.resource.to_string() {
            moved += 1;
        }
    }
    let tail: Vec<KnowledgeSearchHit> = hits[count..].to_vec();
    ranked.extend(tail);
    hits.clone_from_slice(&ranked);
    moved
}

/// The full join through the elected decision provider: build the bounded
/// question set under the elected model identity, let the decide route
/// validate bounds and invoke (local Kev by default, hosted only when
/// elected), and re-order by the validated scores.
pub fn rerank(
    query: &str,
    hits: &mut [KnowledgeSearchHit],
    config: &JevRerankConfig,
) -> Result<JevRerankOutcome> {
    let candidates = hits.len().min(config.max_candidates);
    let config_bytes = std::fs::read(&config.provider_file)
        .map_err(|e| invalid(format!("decision provider file unreadable: {e}")))?;
    if config_bytes.len() > 256 * 1024 {
        return Err(invalid(
            "decision provider file exceeds the bounded read size",
        ));
    }
    let elected = parse_provider_config(&config_bytes)?;
    elected.validate()?;
    if elected.mode == DecisionProviderMode::None {
        return Err(AikitError::new(
            "decision.provider_disabled",
            "The elected decision provider is none; the rerank has nothing to invoke",
        ));
    }
    let model = match elected.mode {
        DecisionProviderMode::Hosted => elected
            .jev_limits
            .as_ref()
            .map(|limits| limits.tariff.model_version.clone())
            .ok_or_else(|| invalid("hosted election requires JevLimits"))?,
        _ => elected.limits()?.model.clone(),
    };
    let request = build_rerank_request(query, model, &hits[..candidates]);
    request.validate()?;
    if let Some(limits) = &elected.limits {
        limits.validate(&request)?;
    }
    if let Some(limits) = &elected.jev_limits {
        limits.validate(&request)?;
    }
    let invocation_ref = minted_invocation_ref(&request)?;
    let receipt = invoke_selected(
        &elected,
        &request,
        invocation_ref,
        config.curl.clone(),
        config.allow_env_import,
        &mut || Ok(()),
    )?;
    let answer = receipt.answer().ok_or_else(|| {
        AikitError::new(
            "knowledge.jev_rerank_no_answer",
            receipt
                .failure_message()
                .unwrap_or_else(|| "The elected decision provider produced no answer".into()),
        )
    })?;
    if !receipt.outcome_completed() {
        return Err(AikitError::new(
            "knowledge.jev_rerank_incomplete",
            "The elected decision provider did not complete the determination",
        ));
    }
    let scores = scores_from_response(answer, candidates)?;
    let moved = apply_rerank(hits, &scores);
    Ok(JevRerankOutcome {
        candidates,
        reordered: moved,
        returned_model: answer.model.clone(),
    })
}

/// The search-side join, called from the CLI after a completed pull. Reads its
/// own capability configuration; does nothing at all unless the capability is
/// configured, and only fires on an oversized pull (more candidates than the
/// surface will show). Every outcome is disclosed in the result's absences,
/// which the CLI envelope surfaces as warnings.
pub fn maybe_rerank(
    config_table: Option<&toml::Table>,
    result: &mut KnowledgeSearchResult,
    limit: usize,
) {
    let Some(table) = config_table else {
        return;
    };
    let config = match JevRerankConfig::from_table(table) {
        Ok(Some(config)) => config,
        Ok(None) => return,
        Err(error) => {
            result
                .absences
                .push(format!("Jev rerank misconfigured and skipped: {error:#}"));
            return;
        }
    };
    if result.hits.len() <= limit {
        return;
    }
    match rerank(&result.query.clone(), &mut result.hits, &config) {
        Ok(outcome) => result.absences.push(format!(
            "Jev rerank applied: {} candidates scored by {} ({} positions reordered)",
            outcome.candidates, outcome.returned_model, outcome.reordered
        )),
        Err(error) => result.absences.push(format!(
            "Jev rerank unavailable, unranked order stands: {error:#}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::knowledge_navigation::KnowledgeAddress;
    use aikit_core::{ProviderRef, ResourceKind, ResourceRef, SourceAuthority, SourceRef};

    fn hit(index: usize) -> KnowledgeSearchHit {
        KnowledgeSearchHit {
            address: KnowledgeAddress::Source(
                SourceRef::parse(format!("source:c{index}")).unwrap(),
            ),
            resource: ResourceRef::parse(format!("source:c{index}")).unwrap(),
            kind: ResourceKind::KnowledgeSource,
            label: format!("candidate {index}"),
            score: 0.5,
            snippet: String::new(),
            provider: ProviderRef::parse("provider/test").unwrap(),
            authority: SourceAuthority::Observed,
            ranking: None,
            corroborated_by: Vec::new(),
        }
    }

    #[test]
    fn apply_rerank_orders_by_score_and_counts_moved_positions() {
        let mut hits: Vec<_> = (0..4).map(hit).collect();
        // Candidate 2 scores highest; candidate 0 lowest.
        let moved = apply_rerank(&mut hits, &[1.0, 2.0, 4.0, 0.0]);
        assert_eq!(
            moved, 2,
            "exactly the two displaced positions count as moved: c2 takes the top, c0 drops to third, c1 and c3 keep their places"
        );
        assert_eq!(hits[0].label, "candidate 2");
        assert_eq!(hits[1].label, "candidate 1");
        assert_eq!(hits[2].label, "candidate 0");
        assert_eq!(hits[3].label, "candidate 3");
    }

    #[test]
    fn apply_rerank_is_stable_on_ties_and_never_loses_hits() {
        let mut hits: Vec<_> = (0..3).map(hit).collect();
        let moved = apply_rerank(&mut hits, &[2.0, 2.0, 2.0]);
        assert_eq!(moved, 0, "equal scores keep the merged order");
        assert_eq!(
            hits.iter()
                .filter(|hit| hit.label.starts_with("candidate"))
                .count(),
            3
        );
    }

    #[test]
    fn build_rerank_request_carries_one_scored_question_per_candidate() {
        let candidates: Vec<_> = (0..3).map(hit).collect();
        let request =
            build_rerank_request("eight determinations", "kev-test.1.0".into(), &candidates);
        request
            .validate()
            .expect("a bounded candidate set satisfies the Jev request contract");
        assert_eq!(request.questions.len(), 3);
        assert_eq!(request.model, "kev-test.1.0");
        assert!(request.state["candidates"].as_array().unwrap().len() == 3);
    }

    #[test]
    fn config_absent_means_off_and_provider_file_is_required() {
        assert!(JevRerankConfig::from_table(&toml::Table::new())
            .unwrap()
            .is_none());
        let without_file: toml::Table = toml::from_str("max_candidates = 8").unwrap();
        assert!(JevRerankConfig::from_table(&without_file)
            .unwrap()
            .is_none());
    }
}
