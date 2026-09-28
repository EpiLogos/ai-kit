//! Jev relevance selection over large knowledge-search pulls.
//!
//! The search faculty already federates every provider it holds; what it did
//! not have was the commissioned selection step (JEV-REDIS-NOW-INTEGRATION §2:
//! "Jev helps interpret the matrices and select relevant material") applied to
//! its own oversized pulls. This module is that join: when a pull carries more
//! candidates than the surface will show and the `tool/search/jev-rerank`
//! capability is configured, one bounded, budgeted, cancellable Jev invocation
//! scores the candidates and the merged order re-ranks accordingly. Without
//! the configuration the faculty behaves exactly as before; nothing here is
//! ever required, and a failed selection degrades to the unranked order with
//! a named absence.

use crate::jev_now::minted_invocation_ref;
use aikit_adapters::jev::{CurlJevProvider, JevBoundary, JevCancellation, JevEndpoint};
use aikit_adapters::secret_resolver::SuiteSecretResolver;
use aikit_core::jev::{JevLimits, JevRequest, JevResponse, Question};
use aikit_core::knowledge_navigation::{KnowledgeSearchHit, KnowledgeSearchResult};
use aikit_core::secret_ref::{SecretRef, SecretResolver};
use aikit_core::{AikitError, Result};
use serde_json::json;
use std::path::PathBuf;

fn invalid(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("knowledge.jev_rerank_invalid", message.to_string())
}

/// Hard bound on candidates scored in one pull. One Score question per
/// candidate, and the protocol accepts at most 256 questions; staying well
/// under it keeps the request bounded and the spend predictable.
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

/// Parsed `tool/search/jev-rerank` capability configuration. Absent means the
/// join is off; present-but-invalid means the faculty says so and searches on.
#[derive(Debug, Clone)]
pub struct JevRerankConfig {
    pub credential_ref: String,
    pub model: String,
    pub controlled_endpoint: Option<std::net::SocketAddr>,
    pub max_candidates: usize,
    pub limits: JevLimits,
}

impl JevRerankConfig {
    /// Read the configuration from a capability table. `Ok(None)` — capability
    /// not configured; the search path must not change behaviour at all.
    pub fn from_table(table: &toml::Table) -> Result<Option<Self>> {
        let Some(credential_ref) = table
            .get("credential_ref")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        let model = table
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .ok_or_else(|| {
                invalid("tool/search/jev-rerank requires a Jev model selector (jev-…)")
            })?;
        let tariff = table.get("tariff").ok_or_else(|| {
            invalid("tool/search/jev-rerank requires a declared tariff (never invented prices)")
        })?;
        let max_candidates = table
            .get("max_candidates")
            .and_then(|value| value.as_integer())
            .map(|value| value as usize)
            .unwrap_or(24)
            .clamp(1, MAX_CANDIDATES_CAP);
        let controlled_endpoint = match table.get("controlled_endpoint") {
            None => None,
            Some(value) => Some(
                value
                    .as_str()
                    .ok_or_else(|| invalid("tool/search/jev-rerank controlled_endpoint must be host:port"))?
                    .parse::<std::net::SocketAddr>()
                    .map_err(|_| {
                        invalid("tool/search/jev-rerank controlled_endpoint is not a valid socket address")
                    })?,
            ),
        };
        let limits = JevLimits {
            timeout_ms: table
                .get("timeout_ms")
                .and_then(|value| value.as_integer())
                .map(|value| value as u64)
                .unwrap_or(60_000),
            max_attempts: table
                .get("max_attempts")
                .and_then(|value| value.as_integer())
                .map(|value| value as u32)
                .unwrap_or(2),
            max_total_reserved_microusd: table
                .get("max_total_reserved_microusd")
                .and_then(|value| value.as_integer())
                .map(|value| value as u64)
                .unwrap_or(20_000),
            tariff: serde_json::from_value(
                serde_json::to_value(tariff)
                    .map_err(|e| invalid(format!("tariff round-trip failed: {e}")))?,
            )
            .map_err(|_| {
                invalid("tool/search/jev-rerank tariff does not satisfy the Jev tariff contract")
            })?,
        };
        Ok(Some(Self {
            credential_ref,
            model,
            controlled_endpoint,
            max_candidates,
            limits,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct JevRerankOutcome {
    pub candidates: usize,
    pub reordered: usize,
    pub reserved_microusd: u64,
    pub returned_model: String,
}

/// One typed Score question per candidate: "how relevant is this candidate to
/// the query", scored on the shared five-step scale.
fn build_rerank_request(query: &str, candidates: &[KnowledgeSearchHit]) -> JevRequest {
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
        model: String::new(),
        state,
        questions,
    }
}

/// Per-candidate scores in candidate order. The provider path already ran
/// `JevResponse::parse_for` against this request — every requested question
/// has exactly one validated Score answer — so a complete pull is the only
/// success; this just lifts the scores out in candidate order.
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

/// The full join: build the bounded question set, resolve the credential
/// natively, invoke once through the real provider seam, and re-order by the
/// validated scores. Credential rotation mid-invocation refuses, exactly as
/// `jev_now` and `contemplation_intel` do.
pub fn rerank(
    query: &str,
    hits: &mut [KnowledgeSearchHit],
    config: &JevRerankConfig,
) -> Result<JevRerankOutcome> {
    let candidates = hits.len().min(config.max_candidates);
    let mut request = build_rerank_request(query, &hits[..candidates]);
    request.model = config.model.clone();
    request.validate()?;
    let limits = &config.limits;
    limits.validate(&request)?;

    let resolver = SuiteSecretResolver::default();
    let credential_ref = SecretRef::parse(&config.credential_ref)?;
    let secret = resolver.resolve(&credential_ref)?;
    let initial_material_digest = blake3::hash(secret.expose().as_bytes());
    let endpoint = config
        .controlled_endpoint
        .map(JevEndpoint::Controlled)
        .unwrap_or(JevEndpoint::Official);
    let provider = CurlJevProvider::new(PathBuf::from("curl"), endpoint);
    let cancellation = JevCancellation::default();
    let invocation_ref = minted_invocation_ref(&request)?;
    let mut guard = |_: JevBoundary| -> Result<()> {
        let current = resolver.resolve(&credential_ref)?;
        if blake3::hash(current.expose().as_bytes()) != initial_material_digest {
            return Err(AikitError::new(
                "knowledge.jev_rerank_credential_changed",
                "The native credential changed during the Jev rerank; re-run explicitly",
            ));
        }
        Ok(())
    };

    let invocation = provider.invoke(
        invocation_ref,
        &request,
        limits,
        &secret,
        &cancellation,
        &mut guard,
    )?;
    let reserved = invocation.total_reserved_microusd;
    let returned_model = invocation.answer.as_ref().map(|a| a.model.clone());
    let answer = invocation.answer.as_ref().ok_or_else(|| {
        AikitError::new(
            "knowledge.jev_rerank_no_answer",
            "The Jev rerank invocation completed without an answer; the unranked order stands",
        )
    })?;
    let scores = scores_from_response(answer, candidates)?;
    let moved = apply_rerank(hits, &scores);
    Ok(JevRerankOutcome {
        candidates,
        reordered: moved,
        reserved_microusd: reserved,
        returned_model: returned_model.unwrap_or_default(),
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
            "Jev rerank applied: {} candidates scored by {} ({} positions reordered, {} µ$ reserved)",
            outcome.candidates, outcome.returned_model, outcome.reordered, outcome.reserved_microusd
        )),
        Err(error) => result
            .absences
            .push(format!("Jev rerank unavailable, unranked order stands: {error:#}")),
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
    fn apply_rerank_orders_by_score_and_reports_moved_positions() {
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
        let mut request = build_rerank_request("eight determinations", &candidates);
        request.model = "jev-test.1.0".into();
        request
            .validate()
            .expect("a bounded candidate set satisfies the Jev request contract");
        assert_eq!(request.questions.len(), 3);
        assert!(request.state["candidates"].as_array().unwrap().len() == 3);
    }

    #[test]
    fn config_absent_means_off_and_credential_only_is_invalid() {
        assert!(JevRerankConfig::from_table(&toml::Table::new())
            .unwrap()
            .is_none());
        let with_credential: toml::Table = toml::from_str("credential_ref = \"suite:x\"").unwrap();
        assert!(JevRerankConfig::from_table(&with_credential).is_err());
    }
}
