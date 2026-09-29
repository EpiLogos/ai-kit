//! The typed Jev question set over `aikit.contemplation-field/v1` (owner item
//! 3), the test-selection Return prepared from the field/decision (owner item
//! 8), and the Redis publication of the three artifacts into the participant
//! prepared view (owner item 5).
//!
//! Nothing here invents a second acceptance database or a second matrix/
//! spine/code registry. `now_contemplate` reuses the general Jev invocation
//! path (`CurlJevProvider`, `JevLimits`, `SuiteSecretResolver`) exactly as
//! `jev_now::jev_invoke` and `jev_now::select_candidates` already do.
//! `now_test_selection` is a pure read/compose function over the field and an
//! optional decision — no new store. `now_publish_intelligence` republishes
//! through the existing `RedisNowStore::publish`/`append_change` CAS path.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aikit_adapters::central_file_map;
use aikit_adapters::jev::{CurlJevProvider, JevBoundary, JevCancellation, JevEndpoint};
use aikit_adapters::runner::SystemRunner;
use aikit_adapters::secret_resolver::SuiteSecretResolver;
use aikit_core::context_source::{AgentVisibility, ExternalEgress};
use aikit_core::jev::{Answer, JevLimits, JevRequest, JevResponse, Question, TokenUsage};
use aikit_core::secret_ref::{SecretRef, SecretResolver};
use aikit_core::{ResourceRef, Result};
use aikit_store::now_context::{
    NowContextBasis, NowContextChange, NowContextItem, PreparedNowContext, RedisNowConfig,
    RedisNowStore, NOW_PREPARED_SCHEMA,
};
use serde_json::{json, Value};

use crate::cli::{NowContemplateArgs, NowPublishIntelligenceArgs, NowTestSelectionArgs};
use crate::jev_now::{bounded_text, central_action, fail, file_digest, read_json, resolve_secret};

pub const QUESTION_SET_SCHEMA: &str = "aikit.contemplation-questions/v1";
pub const DECISION_SCHEMA: &str = "aikit.contemplation-decision/v1";
pub const TEST_SELECTION_SCHEMA: &str = "aikit.test-selection/v1";
const MAX_FIELD_BYTES: usize = 8 * 1024 * 1024;
const MAX_DOC_BYTES: usize = 4 * 1024 * 1024;
const MAX_EXPERIENCE_BYTES: usize = 8 * 1024 * 1024;
/// Keeps the whole request comfortably under the 256-question / 1 MiB Jev
/// bound (jev.rs) even with retrospective questions and a large field.
const MAX_TOTAL_CANDIDATE_QUESTIONS: usize = 220;
const MAX_PRACTICES_PER_ITEM_NOUL: usize = 30;
const MAX_CATEGORY_CANDIDATES: usize = 60;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------
// 1. Typed question set
// ---------------------------------------------------------------------

/// One question's provenance: which category it belongs to, whether it was a
/// per-item Noul or a Choice-over-set, and the subject it asked about. Kept
/// so the decision can be built back up from the raw `JevResponse` without
/// re-deriving candidate identity from the field a second time.
#[derive(Clone, Debug)]
struct QuestionMeta {
    category: &'static str,
    subject: Value,
}

struct BuiltQuestions {
    questions: BTreeMap<String, Question>,
    meta: BTreeMap<String, QuestionMeta>,
    /// Explicit relations that are mandatory regardless of any Jev score,
    /// keyed by category.
    mandatory: BTreeMap<&'static str, Vec<Value>>,
    /// Per-category disclosure of how many candidates existed vs. how many
    /// were actually asked about (the shared budget can truncate).
    category_counts: BTreeMap<&'static str, (usize, usize)>,
}

fn push_noul(
    built: &mut BuiltQuestions,
    budget: &mut usize,
    category: &'static str,
    id: String,
    instructions: Value,
    subject: Value,
) -> bool {
    let entry = built.category_counts.entry(category).or_insert((0, 0));
    entry.0 += 1;
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    entry.1 += 1;
    built.questions.insert(
        id.clone(),
        Question::Noul {
            instructions,
            criteria: None,
        },
    );
    built.meta.insert(id, QuestionMeta { category, subject });
    true
}

fn practice_id(practice: &Value) -> Option<&str> {
    practice["id"].as_str().filter(|id| !id.is_empty())
}

fn build_practices_category(built: &mut BuiltQuestions, budget: &mut usize, field: &Value) {
    let practices = field["spine"]["practices"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if practices.len() <= MAX_PRACTICES_PER_ITEM_NOUL {
        for practice in &practices {
            let Some(id) = practice_id(practice) else {
                continue;
            };
            push_noul(
                built,
                budget,
                "practice-applies",
                format!("practice/{id}"),
                json!({
                    "category": "practice-applies",
                    "question": "Does this Practice/METHOD apply to the current concern?",
                    "practice_id": id,
                    "binding_status": practice["binding"]["status"],
                    "state_ref": "state.ux_spine.practices (by id), its stories and coverage cells",
                }),
                json!({"practice_id": id}),
            );
        }
        return;
    }
    let mut criteria = BTreeMap::new();
    for practice in practices.iter().take(255) {
        if let Some(id) = practice_id(practice) {
            criteria.insert(
                id.to_owned(),
                json!({"purpose": practice["purpose"], "binding_status": practice["binding"]["status"]}),
            );
        }
    }
    let count = criteria.len();
    built
        .category_counts
        .insert("practice-applies", (practices.len(), count));
    if *budget == 0 || count == 0 {
        return;
    }
    *budget = budget.saturating_sub(1);
    built.questions.insert(
        "practices".into(),
        Question::Choice {
            instructions: json!({
                "category": "practice-applies",
                "question": "Which Practices/METHODS apply to the current concern? Several may apply; choose the single best-fitting option only if one dominates, otherwise choose the closest and let the narrower field carry the rest.",
            }),
            criteria,
        },
    );
    built.meta.insert(
        "practices".into(),
        QuestionMeta {
            category: "practice-applies",
            subject: json!({"mode": "choice-over-set"}),
        },
    );
}

fn build_capabilities_category(
    built: &mut BuiltQuestions,
    budget: &mut usize,
    field: &Value,
) -> BTreeSet<String> {
    let joins = field["joins"].as_array().cloned().unwrap_or_default();
    let mut explicit: BTreeSet<String> = BTreeSet::new();
    for join in &joins {
        if join["relation"] == "changed-path-implements-capability" && join["basis"] == "explicit" {
            if let Some(to) = join["to"].as_str() {
                explicit.insert(to.to_owned());
            }
        }
    }
    built.mandatory.insert(
        "capability-implicated",
        explicit
            .iter()
            .map(|id| json!({"capability_id": id, "basis": "explicit", "reason": "changed path explicitly implements this capability's code_refs"}))
            .collect(),
    );
    let capabilities = field["matrix"]["capabilities"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for capability in &capabilities {
        let Some(id) = capability["id"].as_str().filter(|id| !id.is_empty()) else {
            continue;
        };
        if explicit.contains(id) {
            continue;
        }
        push_noul(
            built,
            budget,
            "capability-implicated",
            format!("capability/{id}"),
            json!({
                "category": "capability-implicated",
                "question": "Is this capability materially implicated by the current concern?",
                "capability_id": id,
                "state_ref": "state.capability_matrix.capabilities (by id), with its relations_all_views",
            }),
            json!({"capability_id": id}),
        );
    }
    explicit
}

fn build_code_impact_category(built: &mut BuiltQuestions, budget: &mut usize, field: &Value) {
    let joins = field["joins"].as_array().cloned().unwrap_or_default();
    let derived: Vec<&Value> = joins
        .iter()
        .filter(|join| {
            join["relation"] == "gitnexus-impacted-path-implicates-capability"
                && join["basis"]
                    .as_str()
                    .is_some_and(|b| b.starts_with("derived("))
        })
        .collect();
    for (index, join) in derived.iter().enumerate() {
        push_noul(
            built,
            budget,
            "code-impact-candidate",
            format!("code-impact/{index:03}"),
            json!({
                "category": "code-impact-candidate",
                "question": "Does this GitNexus-derived structural relation materially affect the named capability (not merely a coincidental path match)?",
                "from": join["from"],
                "to": join["to"],
                "basis": join["basis"],
            }),
            (*join).clone(),
        );
    }
}

/// Candidate story/coverage relations from the raw `oi.experience.coverage-
/// reading/v1` document the field's `experience_reading.source` names (the
/// field itself only retains the digest, per B1). Absent a source path, this
/// category is empty and disclosed as such — never fabricated.
fn read_experience_candidates(field: &Value) -> Result<(Value, Vec<Value>)> {
    let Some(path) = field["experience_reading"]["source"].as_str() else {
        return Ok((
            json!({"available": false, "reason": "no --experience-reading was supplied to the field assembly"}),
            Vec::new(),
        ));
    };
    let path = PathBuf::from(path);
    let (bytes, digest) = file_digest(&path, "experience coverage reading", MAX_EXPERIENCE_BYTES)?;
    let expected_digest = field["experience_reading"]["source_revision"].as_str();
    if expected_digest.is_some_and(|expected| expected != digest) {
        return Err(fail(
            "contemplation_intel.experience_reading_stale",
            "the experience coverage reading changed since the field was assembled",
        ));
    }
    let doc: Value = serde_json::from_slice(&bytes).map_err(|e| {
        fail(
            "contemplation_intel.experience_reading_invalid",
            e.to_string(),
        )
    })?;
    let candidates = doc["capability_candidates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let inventory_by_capability: BTreeMap<String, &Value> = doc["capability_inventory"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row["capability_id"].as_str().map(|id| (id.to_owned(), row)))
        .collect();
    let mut enriched = Vec::new();
    for candidate in &candidates {
        let Some(capability_id) = candidate["capability_id"].as_str() else {
            continue;
        };
        // The candidate's own `repository` field and the matrix inventory's
        // `repository` field are drawn from different owner conventions in
        // the current O:I reading (e.g. `EpiLogos/ai-kit` vs `ai-kit`); only
        // the inventory's own repository+source_digest identify a row that
        // `experience_map.py apply_bindings` will actually accept. This
        // mismatch is disclosed in the Return rather than silently patched
        // over by trusting the candidate's own field.
        let inventory_row = inventory_by_capability.get(capability_id);
        let (repository, source_digest) = match inventory_row {
            Some(row) => (
                row["repository"].as_str().unwrap_or_default().to_owned(),
                row["source_digest"].as_str().unwrap_or_default().to_owned(),
            ),
            None => (String::new(), String::new()),
        };
        enriched.push(json!({
            "capability_id": capability_id,
            "story_ids": candidate["story_ids"],
            "reason": candidate["reason"],
            "binding_status": candidate["binding_status"],
            "candidate_repository": candidate["repository"],
            "inventory_repository": repository,
            "source_digest": source_digest,
            "inventory_matched": inventory_row.is_some(),
        }));
    }
    Ok((
        json!({
            "available": true,
            "source": path.display().to_string(),
            "source_revision": digest,
            "candidate_count": enriched.len(),
        }),
        enriched,
    ))
}

fn build_story_category(
    built: &mut BuiltQuestions,
    budget: &mut usize,
    field: &Value,
) -> Result<Value> {
    let (disclosure, candidates) = read_experience_candidates(field)?;
    for (index, candidate) in candidates.iter().enumerate() {
        push_noul(
            built,
            budget,
            "story-candidate",
            format!("story-candidate/{index:03}"),
            json!({
                "category": "story-candidate",
                "question": "Is this candidate UX story/branch condition affected by the current concern? A yes proposes a `direct` coverage binding for review; it is never applied automatically.",
                "capability_id": candidate["capability_id"],
                "story_ids": candidate["story_ids"],
                "reason": candidate["reason"],
            }),
            candidate.clone(),
        );
    }
    Ok(disclosure)
}

/// Candidate test relations from GitNexus code-lens readings: paths that look
/// like test files, mentioned in a context/impact reading, and not already an
/// explicit `test_refs` entry for any capability.
fn build_test_candidates_category(built: &mut BuiltQuestions, budget: &mut usize, field: &Value) {
    let explicit_tests: BTreeSet<String> = field["matrix"]["capabilities"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|capability| {
            capability["test_refs"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect();
    let mut seen = BTreeSet::new();
    let readings = field["code_lens"]["readings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for reading in &readings {
        for holder in [&reading["context"], &reading["impact_upstream"]] {
            for path in extract_test_like_paths(holder) {
                if explicit_tests.contains(&path) || !seen.insert(path.clone()) {
                    continue;
                }
                let index = seen.len() - 1;
                push_noul(
                    built,
                    budget,
                    "test-candidate",
                    format!("test-candidate/{index:03}"),
                    json!({
                        "category": "test-candidate",
                        "question": "Is this test file relevant to the current concern beyond the explicit test_refs already tied to an implicated capability?",
                        "path": path,
                        "observed_via": reading["reference"],
                    }),
                    json!({"path": path}),
                );
            }
        }
    }
}

fn extract_test_like_paths(value: &Value) -> Vec<String> {
    fn looks_like_test(path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        (lower.contains("test") || lower.contains("spec"))
            && Path::new(path)
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "mjs"))
    }
    fn walk(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(s) if looks_like_test(s) => out.push(s.clone()),
            Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
            Value::Object(map) => map.values().for_each(|item| walk(item, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, &mut out);
    out
}

fn build_evidence_sufficiency_category(
    built: &mut BuiltQuestions,
    budget: &mut usize,
    explicit_capabilities: &BTreeSet<String>,
) {
    let mut criteria = BTreeMap::new();
    criteria.insert(
        "absent".to_owned(),
        json!({"meaning": "No evidence exists for this capability's current claim."}),
    );
    criteria.insert(
        "stale".to_owned(),
        json!({"meaning": "Evidence exists but predates the current change or source basis."}),
    );
    criteria.insert(
        "contradictory".to_owned(),
        json!({"meaning": "Evidence conflicts with the current claim or with other retained evidence."}),
    );
    criteria.insert(
        "sufficient-at-grade".to_owned(),
        json!({"meaning": "Evidence is sufficient at its disclosed automation grade (D/C only; P/M/H require human/peer/maintainer review)."}),
    );
    for id in explicit_capabilities.iter().take(MAX_CATEGORY_CANDIDATES) {
        if *budget == 0 {
            let entry = built
                .category_counts
                .entry("evidence-sufficiency")
                .or_insert((0, 0));
            entry.0 += 1;
            continue;
        }
        *budget -= 1;
        let qid = format!("evidence-sufficiency/{id}");
        built.questions.insert(
            qid.clone(),
            Question::Choice {
                instructions: json!({
                    "category": "evidence-sufficiency",
                    "question": "How sufficient is the retained evidence for this implicated capability's current claim?",
                    "capability_id": id,
                }),
                criteria: criteria.clone(),
            },
        );
        built.meta.insert(
            qid,
            QuestionMeta {
                category: "evidence-sufficiency",
                subject: json!({"capability_id": id}),
            },
        );
        let entry = built
            .category_counts
            .entry("evidence-sufficiency")
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 += 1;
    }
}

fn build_neighbours_category(built: &mut BuiltQuestions, budget: &mut usize, field: &Value) {
    let neighbours = field["redis"]["neighbours"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for (index, neighbour) in neighbours.iter().enumerate() {
        push_noul(
            built,
            budget,
            "neighbouring-now-work",
            format!("neighbour/{index:03}"),
            json!({
                "category": "neighbouring-now-work",
                "question": "Does this neighbouring NOW participant's live work affect the current concern?",
                "neighbour": neighbour,
            }),
            neighbour.clone(),
        );
    }
}

const RETROSPECTIVE_WARRANTS: [(&str, &str); 5] = [
    ("regression", "a regression test"),
    ("wiki-reading", "a Wiki reading/return"),
    ("matrix-update", "a capability matrix update"),
    ("skill-revision", "a Skill/METHOD revision"),
    (
        "continuing-question",
        "a continuing open question for the next session",
    ),
];

fn build_retrospective_category(built: &mut BuiltQuestions, budget: &mut usize, pass: &str) {
    if pass != "retrospective" {
        return;
    }
    for (key, phrase) in RETROSPECTIVE_WARRANTS {
        push_noul(
            built,
            budget,
            "retrospective-warrant",
            format!("retro/{key}"),
            json!({
                "category": "retrospective-warrant",
                "question": format!("Does this Return warrant {phrase}?"),
                "warrant": key,
            }),
            json!({"warrant": key}),
        );
    }
}

/// The spine as Jev state: every story, practice and coverage cell, with the
/// duplication removed — practices reference their coverage cells by
/// capability_ref instead of embedding copies, bindings drop local absolute
/// paths, and a scope sentence shared by every cell is stated once.
fn compact_spine(spine: &Value) -> Value {
    let practices: Vec<Value> = spine["practices"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|p| {
                    json!({
                        "id": p["id"],
                        "purpose": p["purpose"],
                        "stories_served": p["stories_served"],
                        "support": p["support"],
                        "implementation_owner": p["implementation_owner"],
                        "binding": {
                            "status": p["binding"]["status"],
                            "kind": p["binding"]["kind"],
                            "skill_ref": p["binding"]["skill_ref"],
                            "classification": p["binding"]["classification"],
                            "gap_owner": p["binding"]["gap_owner"],
                        },
                        "coverage_cell_refs": p["capability_coverage_cells"]
                            .as_array()
                            .map(|cells| cells.iter().map(|c| c["capability_ref"].clone()).collect::<Vec<_>>())
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let cells = spine["coverage_cells"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let shared_scope = cells
        .first()
        .map(|c| c["scope"].clone())
        .filter(|scope| cells.iter().all(|c| &c["scope"] == scope));
    let coverage_cells: Vec<Value> = cells
        .iter()
        .map(|c| {
            let mut cell = c.clone();
            if shared_scope.is_some() {
                if let Some(obj) = cell.as_object_mut() {
                    obj.remove("scope");
                }
            }
            cell
        })
        .collect();
    json!({
        "source": spine["source"],
        "stories": spine["stories"],
        "practices": practices,
        "coverage_cells": coverage_cells,
        "coverage_scope_for_every_cell": shared_scope,
    })
}

/// Conservative input-token estimate for a Jev request (observed ~2.65 bytes
/// per token on this field; 2.5 overestimates tokens, so the guard trips early
/// rather than letting the provider refuse with an opaque HTTP 400).
fn estimated_input_tokens(request: &JevRequest) -> u64 {
    let bytes = serde_json::to_vec(request)
        .map(|b| b.len())
        .unwrap_or(usize::MAX) as u64;
    bytes.saturating_mul(10) / 25
}

/// Split one contemplation request into batches that each carry the whole
/// state and as many questions as fit under `ceiling` estimated input tokens.
fn split_into_batches(request: &JevRequest, ceiling: u64) -> Result<Vec<JevRequest>> {
    let empty = JevRequest {
        model: request.model.clone(),
        state: request.state.clone(),
        questions: BTreeMap::new(),
    };
    if estimated_input_tokens(&empty) >= ceiling {
        return Err(fail(
            "contemplation_intel.request_over_input_ceiling",
            format!(
                "the field state alone is ~{} input tokens, over the declared ceiling of {ceiling}; \
                 no question batch can carry it",
                estimated_input_tokens(&empty)
            ),
        ));
    }
    let mut batches = Vec::new();
    let mut current = empty.clone();
    for (id, question) in &request.questions {
        current.questions.insert(id.clone(), question.clone());
        if estimated_input_tokens(&current) > ceiling && current.questions.len() > 1 {
            current.questions.remove(id);
            batches.push(std::mem::replace(&mut current, empty.clone()));
            current.questions.insert(id.clone(), question.clone());
        }
        if estimated_input_tokens(&current) > ceiling {
            return Err(fail(
                "contemplation_intel.request_over_input_ceiling",
                format!("question {id} and the full state cannot fit the declared input ceiling"),
            ));
        }
    }
    if !current.questions.is_empty() {
        batches.push(current);
    }
    Ok(batches)
}

/// A split request still has one commissioned invocation budget. Reservations
/// include failed/unknown attempts; a new batch cannot reset money, time or tries.
fn remaining_batch_limits(
    limits: &JevLimits,
    reserved: u64,
    attempts: u32,
    elapsed_ms: u64,
) -> Result<JevLimits> {
    let mut remaining = limits.clone();
    remaining.max_total_reserved_microusd = limits
        .max_total_reserved_microusd
        .checked_sub(reserved)
        .ok_or_else(|| {
            fail(
                "jev.budget_exhausted",
                "Batch reservations exceeded the invocation budget",
            )
        })?;
    remaining.max_attempts = limits
        .max_attempts
        .checked_sub(attempts)
        .filter(|n| *n > 0)
        .ok_or_else(|| {
            fail(
                "jev.budget_exhausted",
                "No attempts remain for the next batch",
            )
        })?;
    remaining.timeout_ms = limits
        .timeout_ms
        .checked_sub(elapsed_ms)
        .filter(|n| *n > 0)
        .ok_or_else(|| {
            fail(
                "jev.timeout",
                "No invocation time remains for the next batch",
            )
        })?;
    if remaining.max_total_reserved_microusd < limits.tariff.reservation()? {
        return Err(fail(
            "jev.budget_exhausted",
            "Cannot reserve the next batch within the original invocation budget",
        ));
    }
    Ok(remaining)
}

fn build_question_set(
    field: &Value,
    pass: &str,
    model: String,
) -> Result<(JevRequest, BuiltQuestions)> {
    let mut built = BuiltQuestions {
        questions: BTreeMap::new(),
        meta: BTreeMap::new(),
        mandatory: BTreeMap::new(),
        category_counts: BTreeMap::new(),
    };
    let mut budget = MAX_TOTAL_CANDIDATE_QUESTIONS;
    build_practices_category(&mut built, &mut budget, field);
    let explicit_capabilities = build_capabilities_category(&mut built, &mut budget, field);
    build_code_impact_category(&mut built, &mut budget, field);
    let experience_disclosure = build_story_category(&mut built, &mut budget, field)?;
    build_test_candidates_category(&mut built, &mut budget, field);
    build_evidence_sufficiency_category(&mut built, &mut budget, &explicit_capabilities);
    build_neighbours_category(&mut built, &mut budget, field);
    build_retrospective_category(&mut built, &mut budget, pass);
    built
        .mandatory
        .insert("experience-reading-disclosure", vec![experience_disclosure]);

    if built.questions.is_empty() {
        return Err(fail(
            "contemplation_intel.no_questions",
            "the assembled field produced no questions for this pass; nothing to contemplate",
        ));
    }
    // Jev judges relations only as well as the state carries them: the whole
    // capability matrix (every row with every field, every view's relation
    // records) and the spine's practice -> story -> coverage traversal are
    // stated in full, not summarised into per-question slices.
    let state = json!({
        "pass": pass,
        "telos": field["telos"],
        "changed_subject": field["changed_subject"],
        "explicit_joins": field["joins"],
        "capability_matrix": {
            "matrix_id": field["matrix"]["matrix_id"],
            "source": field["matrix"]["source_csv"],
            "capabilities": field["matrix"]["capabilities"],
            "relations_all_views": field["matrix"]["all_view_relations"],
        },
        "ux_spine": compact_spine(&field["spine"]),
        "tests_evidence": field["tests_evidence"],
        "now": field["now"],
    });
    let request = JevRequest {
        model,
        state,
        questions: built.questions.clone(),
    };
    request.validate()?;
    Ok((request, built))
}

// ---------------------------------------------------------------------
// `aikit now-context contemplate`
// ---------------------------------------------------------------------

fn minted_invocation_ref(request: &JevRequest) -> Result<ResourceRef> {
    let identity = format!("{}:{}:{}", request.digest()?, now_ms(), std::process::id());
    ResourceRef::parse(format!(
        "invocation/jev-contemplate/{}",
        blake3::hash(identity.as_bytes()).to_hex()
    ))
}

pub fn now_contemplate(args: NowContemplateArgs) -> Result<Value> {
    if !matches!(args.pass.as_str(), "prospective" | "retrospective") {
        return Err(fail(
            "contemplation_intel.invalid_pass",
            "pass must be prospective or retrospective",
        ));
    }
    if !(0.0..=1.0).contains(&args.relevance_threshold) || !args.relevance_threshold.is_finite() {
        return Err(fail(
            "contemplation_intel.invalid_threshold",
            "relevance threshold must be between zero and one",
        ));
    }
    let (field_bytes, initial_field_digest) =
        file_digest(&args.field, "contemplation field", MAX_FIELD_BYTES)?;
    let field: Value = serde_json::from_slice(&field_bytes)
        .map_err(|e| fail("contemplation_intel.field_invalid", e.to_string()))?;
    if field["schema"] != crate::contemplation_field::FIELD_SCHEMA {
        return Err(fail(
            "contemplation_intel.field_invalid",
            "the supplied document is not an aikit.contemplation-field/v1",
        ));
    }

    let limits: JevLimits = read_json(&args.limits_file, "Jev limits", 256 * 1024)?;
    let (request, built) =
        build_question_set(&field, &args.pass, limits.tariff.model_version.clone())?;
    limits.validate(&request)?;
    // Every batch carries the whole state; questions are split so each call
    // fits the declared input ceiling (the provider answers an over-ceiling
    // request with an opaque HTTP 400).
    let batches = split_into_batches(&request, limits.tariff.max_input_tokens_per_attempt)?;

    let credential_ref = SecretRef::parse(&args.credential_ref)?;
    let resolver = if args.allow_env_import {
        SuiteSecretResolver::with_env_import()
    } else {
        SuiteSecretResolver::default()
    };
    let secret = resolver.resolve(&credential_ref)?;
    let initial_material_digest = blake3::hash(secret.expose().as_bytes());
    let endpoint = args
        .controlled_endpoint
        .map(JevEndpoint::Controlled)
        .unwrap_or(JevEndpoint::Official);
    let provider = CurlJevProvider::new(
        args.curl.clone().unwrap_or_else(|| PathBuf::from("curl")),
        endpoint,
    );
    let cancellation = JevCancellation::default();
    let invocation_ref = args
        .invocation_ref
        .as_deref()
        .map(ResourceRef::parse)
        .transpose()?
        .unwrap_or(minted_invocation_ref(&request)?);
    let field_path = args.field.clone();
    let mut guard = |_: JevBoundary| -> Result<()> {
        // Re-resolve at every provider boundary, exactly like `jev_now`: a
        // rotated/revoked credential ends this invocation, and a changed
        // field document refuses rather than contemplating a stale basis.
        let current = resolver.resolve(&credential_ref)?;
        if blake3::hash(current.expose().as_bytes()) != initial_material_digest {
            return Err(fail(
                "jev.credential_changed",
                "The native credential changed during the invocation; recompose explicitly",
            ));
        }
        let (_, current_digest) = file_digest(&field_path, "contemplation field", MAX_FIELD_BYTES)?;
        if current_digest != initial_field_digest {
            return Err(fail(
                "contemplation_intel.field_changed",
                "The contemplation field document changed during the Jev invocation; refuse publication exactly as jev_now does",
            ));
        }
        Ok(())
    };
    let mut merged_answers = BTreeMap::new();
    let mut usage = TokenUsage {
        input_tokens: 0,
        output_tokens: 0,
    };
    let mut returned_model: Option<String> = None;
    let mut batch_refs = Vec::new();
    let mut batch_receipts = Vec::new();
    let mut total_reserved = 0u64;
    let mut total_attempts = 0u32;
    let batch_clock = std::time::Instant::now();
    let mut invocation = None;
    for (index, batch) in batches.iter().enumerate() {
        let batch_ref = if batches.len() == 1 {
            invocation_ref.clone()
        } else {
            ResourceRef::parse(format!("{invocation_ref}-batch{}", index + 1))?
        };
        let batch_limits = remaining_batch_limits(
            &limits,
            total_reserved,
            total_attempts,
            batch_clock.elapsed().as_millis().min(u64::MAX as u128) as u64,
        )?;
        let attempt = provider.invoke(
            batch_ref.clone(),
            batch,
            &batch_limits,
            &secret,
            &cancellation,
            &mut guard,
        )?;
        total_reserved = total_reserved
            .checked_add(attempt.total_reserved_microusd)
            .ok_or_else(|| {
                fail(
                    "jev.budget_exhausted",
                    "Batch reservation arithmetic overflow",
                )
            })?;
        total_attempts = total_attempts
            .checked_add(attempt.attempts.len() as u32)
            .ok_or_else(|| fail("jev.budget_exhausted", "Batch attempt arithmetic overflow"))?;
        batch_receipts.push(attempt.clone());
        // A field that changed mid-call is the cause to report, not the
        // attempt failure it produced.
        let (_, batch_field_digest) =
            file_digest(&args.field, "contemplation field", MAX_FIELD_BYTES)?;
        if batch_field_digest != initial_field_digest {
            return Err(fail(
                "contemplation_intel.field_changed",
                "The contemplation field document changed during the Jev invocation; refuse publication exactly as jev_now does",
            ));
        }
        let answer = attempt.answer.clone().ok_or_else(|| {
            fail(
                "contemplation_intel.jev_failed",
                format!(
                    "batch {} of {}: {}",
                    index + 1,
                    batches.len(),
                    attempt
                        .failure
                        .as_ref()
                        .map(|f| f.message.as_str())
                        .unwrap_or("Jev produced no successful determination")
                ),
            )
        })?;
        if returned_model.as_ref().is_some_and(|m| m != &answer.model) {
            return Err(fail(
                "contemplation_intel.jev_model_changed",
                "batches of one contemplation were answered by different models",
            ));
        }
        returned_model = Some(answer.model.clone());
        usage.input_tokens += answer.usage.input_tokens;
        usage.output_tokens += answer.usage.output_tokens;
        merged_answers.extend(answer.answers);
        batch_refs.push(batch_ref);
        invocation = Some(attempt);
    }
    let mut invocation = invocation.expect("at least one batch");
    invocation.answer = Some(JevResponse {
        model: returned_model.unwrap_or_default(),
        answers: merged_answers,
        usage,
    });

    // Final revalidation immediately before the decision is returned, so a
    // late edit between the last attempt and this point cannot slip through.
    let (_, final_field_digest) = file_digest(&args.field, "contemplation field", MAX_FIELD_BYTES)?;
    if final_field_digest != initial_field_digest {
        return Err(fail(
            "contemplation_intel.field_changed",
            "The contemplation field document changed after the Jev invocation completed",
        ));
    }

    let answer = invocation.answer.clone().ok_or_else(|| {
        fail(
            "contemplation_intel.jev_failed",
            invocation
                .failure
                .as_ref()
                .map(|f| f.message.as_str())
                .unwrap_or("Jev produced no successful determination"),
        )
    })?;

    let standing = match invocation.standing {
        aikit_adapters::jev::JevStanding::ProviderProtocol => "provider-protocol",
        aikit_adapters::jev::JevStanding::ControlledProtocol => "controlled-protocol",
    };
    let tariff_cost_microusd = limits.tariff.cost_microusd(&answer.usage).ok();

    let mut answered = Vec::new();
    let mut selected_by_category: BTreeMap<&'static str, Vec<Value>> = BTreeMap::new();
    for (id, meta) in &built.meta {
        let Some(raw_answer) = answer.answers.get(id) else {
            continue;
        };
        let (selected, confidence, encoded) = describe_answer(raw_answer, args.relevance_threshold);
        answered.push(json!({
            "id": id,
            "category": meta.category,
            "subject": meta.subject,
            "answer": encoded,
            "confidence": confidence,
            "selected": selected,
        }));
        if selected {
            selected_by_category
                .entry(meta.category)
                .or_default()
                .push(meta.subject.clone());
        }
    }

    // Retrospective candidate coverage-binding proposals, in the exact shape
    // O-I's `scripts/experience_map.py apply_bindings` validates. Emitted for
    // review only; nothing here calls into O-I or mutates its reading.
    let mut proposed_coverage_bindings = Vec::new();
    for subject in selected_by_category
        .get("story-candidate")
        .into_iter()
        .flatten()
    {
        if subject["inventory_matched"] != true {
            continue;
        }
        proposed_coverage_bindings.push(json!({
            "repository": subject["inventory_repository"],
            "capability_id": subject["capability_id"],
            "source_digest": subject["source_digest"],
            "disposition": "direct",
            "story_ids": subject["story_ids"],
            "reason": subject["reason"],
        }));
    }

    let category_budget: Value = built
        .category_counts
        .iter()
        .map(|(k, (candidates, asked))| {
            (
                k.to_string(),
                json!({"candidates": candidates, "asked": asked}),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into();

    Ok(json!({
        "schema": DECISION_SCHEMA,
        "question_set_schema": QUESTION_SET_SCHEMA,
        "question_set_version": 1,
        "pass": args.pass,
        "field_source": args.field.display().to_string(),
        "field_basis_digest": initial_field_digest,
        "invocation_ref": invocation_ref,
        "jev_invocation_ref": invocation_ref,
        "jev_batches": batch_refs,
        "jev_batch_receipts": batch_receipts,
        "total_reserved_microusd": total_reserved,
        "total_attempts": total_attempts,
        "requested_model": request.model,
        "returned_model": answer.model,
        "usage": answer.usage,
        "tariff_cost_microusd": tariff_cost_microusd,
        "standing": standing,
        "relevance_threshold": args.relevance_threshold,
        "category_budget": category_budget,
        "mandatory": built.mandatory,
        "answers": answered,
        "selected": selected_by_category,
        "proposed_coverage_bindings": proposed_coverage_bindings,
        "proposed_coverage_bindings_standing": "proposal only, never applied here; review and apply through O-I's own experience_map.py apply_bindings",
    }))
}

fn describe_answer(answer: &Answer, threshold: f64) -> (bool, f64, Value) {
    match answer {
        // Strictly above: a Noul exactly at the threshold is a coin flip, not
        // a determination.
        Answer::Noul { noul } => (
            *noul > threshold,
            *noul,
            json!({"type": "noul", "noul": noul}),
        ),
        Answer::Choice {
            choice,
            probabilities,
            confidence,
        } => (
            true,
            *confidence,
            json!({"type": "choice", "choice": choice, "probabilities": probabilities, "confidence": confidence}),
        ),
        Answer::Score {
            score,
            legend,
            probabilities,
            confidence,
        } => (
            true,
            *confidence,
            json!({"type": "score", "score": score, "legend": legend, "probabilities": probabilities, "confidence": confidence}),
        ),
    }
}

// ---------------------------------------------------------------------
// 2. Test-selection Return
// ---------------------------------------------------------------------

const EVIDENCE_GRADE_LAW: &str =
    "O:I grade law (O-I scripts/experience_map.py, #201): D = deterministic (autonomous/CI); \
     C = cross-product conformance, and only when the receipt itself carries live/native or \
     real-kernel-bridge standing; P = real provider/harness; M = physical/material; H = human \
     UX/Recognition. Automation earns at most D, or C with such a receipt; P, M and H are never \
     earned by automation. A declared test existing at head is not evidence: nothing is earned \
     until the test is executed and its receipt names the revision it ran against.";

/// How a declared test_ref is run, or that it is not an executable test.
fn test_invocation(test_ref: &str) -> Value {
    let parts: Vec<&str> = test_ref.split('/').collect();
    let (kind, command) = match parts.as_slice() {
        ["crates", krate, "tests", file] if file.ends_with(".rs") => (
            "cargo-integration-test",
            Some(format!(
                "cargo test -p {krate} --test {}",
                file.trim_end_matches(".rs")
            )),
        ),
        ["crates", krate, "src", ..] if test_ref.ends_with(".rs") => (
            "cargo-unit-tests",
            Some(format!("cargo test -p {krate} --lib")),
        ),
        _ if test_ref.ends_with(".py") => ("python-script", Some(format!("python3 {test_ref}"))),
        _ => ("evidence-document", None),
    };
    json!({"kind": kind, "command": command})
}

fn capability_row<'a>(field: &'a Value, id: &str) -> Option<&'a Value> {
    field["matrix"]["capabilities"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["id"].as_str() == Some(id))
}

fn tests_for_capability(field: &Value, id: &str) -> Vec<Value> {
    field["tests_evidence"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["capability_id"].as_str() == Some(id))
        .cloned()
        .collect()
}

pub fn now_test_selection(args: NowTestSelectionArgs) -> Result<Value> {
    let (field_bytes, field_digest) =
        file_digest(&args.field, "contemplation field", MAX_FIELD_BYTES)?;
    let field: Value = serde_json::from_slice(&field_bytes)
        .map_err(|e| fail("contemplation_intel.field_invalid", e.to_string()))?;
    if field["schema"] != crate::contemplation_field::FIELD_SCHEMA {
        return Err(fail(
            "contemplation_intel.field_invalid",
            "the supplied document is not an aikit.contemplation-field/v1",
        ));
    }
    let decision: Option<Value> = match &args.decision {
        Some(path) => Some(read_json(path, "contemplation decision", 4 * 1024 * 1024)?),
        None => None,
    };
    if let Some(decision) = &decision {
        if decision["field_basis_digest"]
            .as_str()
            .is_some_and(|d| d != field_digest)
        {
            return Err(fail(
                "contemplation_intel.decision_stale",
                "the supplied decision was contemplated over a different field basis digest",
            ));
        }
    }

    // Affected capabilities, each labelled by how it entered the selection.
    let joins = field["joins"].as_array().cloned().unwrap_or_default();
    let mut explicit_ids = BTreeSet::new();
    let mut derived_ids = BTreeSet::new();
    for join in &joins {
        let Some(to) = join["to"].as_str() else {
            continue;
        };
        match join["basis"].as_str() {
            Some("explicit") if join["relation"] == "changed-path-implements-capability" => {
                explicit_ids.insert(to.to_owned());
            }
            Some(basis) if basis.starts_with("derived(") => {
                derived_ids.insert(to.to_owned());
            }
            _ => {}
        }
    }
    let mut jev_candidate_ids: BTreeSet<String> = BTreeSet::new();
    if let Some(decision) = &decision {
        for subject in decision["selected"]["capability-implicated"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(id) = subject["capability_id"].as_str() {
                if !explicit_ids.contains(id) && !derived_ids.contains(id) {
                    jev_candidate_ids.insert(id.to_owned());
                }
            }
        }
    }

    let mut affected_capabilities = Vec::new();
    let mut missing_evidence_grades = Vec::new();
    let mut negative_controls = Vec::new();
    let mut required_tests = Vec::new();
    let all_ids: BTreeSet<&String> = explicit_ids
        .iter()
        .chain(derived_ids.iter())
        .chain(jev_candidate_ids.iter())
        .collect();
    for id in all_ids {
        let label = if explicit_ids.contains(id) {
            "explicit"
        } else if derived_ids.contains(id) {
            "derived"
        } else {
            "jev-candidate"
        };
        let row = capability_row(&field, id);
        let known_tests = tests_for_capability(&field, id);
        let declared_test_refs = row
            .and_then(|r| r["test_refs"].as_array())
            .cloned()
            .unwrap_or_default();
        let any_exists = known_tests.iter().any(|t| t["exists_at_head"] == true);
        missing_evidence_grades.push(json!({
            "capability_id": id,
            "earned_grade": Value::Null,
            "earned_basis": "no execution receipt is part of the field; declared tests existing at head earn nothing",
            "automation_ceiling": "D (C only with a receipt carrying live/native standing)",
            // The field checks existence only for explicit/derived capabilities;
            // for a candidate it was never assessed, which is not "missing".
            "declared_tests_exist_at_head": if known_tests.is_empty() && !declared_test_refs.is_empty() { Value::Null } else { json!(any_exists) },
            "declared_test_refs": declared_test_refs,
            "known_tests_at_head": known_tests,
        }));
        if known_tests.is_empty() {
            for test_ref in declared_test_refs.iter().filter_map(Value::as_str) {
                required_tests.push(json!({
                    "capability_id": id,
                    "test_ref": test_ref,
                    "invocation": test_invocation(test_ref),
                    "exists_at_head": Value::Null,
                    "reason": format!("{label} capability: declared test, existence at head not assessed by the field"),
                }));
            }
        }
        for test in &known_tests {
            required_tests.push(json!({
                "capability_id": id,
                "test_ref": test["test_ref"],
                "invocation": test_invocation(test["test_ref"].as_str().unwrap_or_default()),
                "exists_at_head": test["exists_at_head"],
                "reason": if test["exists_at_head"] == true { "re-run: the capability is affected by this change" } else { "create: the capability declares this test but it does not exist at head" },
            }));
        }
        negative_controls.push(json!({
            "capability_id": id,
            "controls": [
                {"kind": "disconnected-producer", "test_ref": declared_test_refs.first(), "run": "with the capability's declared producer/dependency removed or mocked absent, to prove the test fails without it"},
                {"kind": "stale", "test_ref": declared_test_refs.first(), "run": "against the pre-change revision of the touched code_refs, to prove the test correctly fails on the regression this change fixes"},
                {"kind": "wrong-subject", "test_ref": declared_test_refs.first(), "run": "against an unrelated capability's fixture/subject, to prove it does not spuriously pass"},
            ],
            "disclosure": if declared_test_refs.is_empty() { json!("no known test_ref; a control cannot be named until one exists") } else { Value::Null },
        }));
        affected_capabilities.push(json!({
            "capability_id": id,
            "label": label,
            "need": row.map(|r| r["need"].clone()).unwrap_or(Value::Null),
            "operation": row.map(|r| r["operation"].clone()).unwrap_or(Value::Null),
            "standing": row.map(|r| r["standing"].clone()).unwrap_or(Value::Null),
        }));
    }

    let practices = field["spine"]["practices"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // Affected practices are the ones the determination actually selected
    // (Jev candidates above threshold); without a decision none is claimed.
    // The whole spine is never "affected" by default.
    let selected_practice_ids: BTreeSet<String> = decision
        .as_ref()
        .and_then(|d| d["selected"]["practice-applies"].as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i["practice_id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let affected: Vec<&Value> = practices
        .iter()
        .filter(|p| {
            p["id"]
                .as_str()
                .is_some_and(|id| selected_practice_ids.contains(id))
        })
        .collect();
    let required_skills: Vec<Value> = affected
        .iter()
        .map(|p| {
            json!({
                "practice_id": p["id"],
                "binding_status": p["binding"]["status"],
                "skill_ref": p["binding"]["skill_ref"],
                "resolved_path": p["binding"]["resolved_path"],
            })
        })
        .collect();
    // A legitimately-unavailable practice is a named boundary (its owner
    // capability does not exist yet), not a developmental gap a commission can
    // close; only affected practices whose binding is actually missing count.
    let legitimately_unavailable = |p: &Value| {
        p["binding"]["classification"] == "legitimately-unavailable"
            || p["classification"] == "legitimately-unavailable"
    };
    let unbound_practices = affected
        .iter()
        .filter(|p| p["binding"]["status"] != "bound" && !legitimately_unavailable(p))
        .count();
    let unavailable_boundaries: Vec<Value> = affected
        .iter()
        .filter(|p| legitimately_unavailable(p))
        .map(|p| json!({"practice_id": p["id"], "gap_owner": p["binding"]["gap_owner"].clone(), "standing": "legitimately-unavailable: named boundary, not a commissionable gap"}))
        .collect();

    // Disposition: a disclosed, deterministic heuristic — never a silent guess.
    let (disposition, disposition_rule) = if unbound_practices > 0 {
        (
            "Factory",
            "at least one implicated Practice/METHOD has no bound Skill (bound-missing or unbound); \
             a Factory commission carries the developmental gap explicitly rather than an ad hoc worker \
             guessing at an absent binding",
        )
    } else if affected_capabilities.len() > 5 {
        (
            "Factory",
            "more than five capabilities are affected; the breadth crosses the bound this selection \
             treats as beyond a single Direct/Routine pass",
        )
    } else if affected_capabilities.is_empty() {
        (
            "Direct",
            "no capability is implicated by the current changed subject; proceed directly",
        )
    } else {
        (
            "Direct",
            "a bounded, already-practice-bound set of implicated capabilities; proceed directly unless \
             a Routine already covers this exact recurring concern (name it explicitly if so)",
        )
    };

    let gitnexus_structural: Vec<Value> = field["code_lens"]["readings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["kind"] == "detect_changes" || r["kind"] == "symbol")
        .cloned()
        .collect();

    let participant_split = json!({
        "implementer": field["redis"]["participant_ref"].as_str().unwrap_or("participant/intelligence-arm/implementer"),
        "verifier": "participant/intelligence-arm/verifier",
        "related": field["redis"]["neighbours"].as_array().cloned().unwrap_or_default(),
    });

    let source_refs = json!({
        "field": args.field.display().to_string(),
        "field_basis_digest": field_digest,
        "decision": args.decision.as_ref().map(|p| p.display().to_string()),
        "spine_source": field["spine"]["source"],
        "matrix_manifest": field["matrix"]["source_manifest"],
        "matrix_csv": field["matrix"]["source_csv"],
        "changed_subject_source_ref": field["changed_subject"]["source_ref"],
    });

    let telos_concern = field["telos"]["serving_track_excerpt"]
        .as_str()
        .or_else(|| field["telos"]["goal"].as_str())
        .unwrap_or("no telos anchor supplied")
        .to_owned();

    let markdown = render_markdown(
        &field,
        &SelectionReading {
            affected_capabilities: &affected_capabilities,
            required_skills: &required_skills,
            missing_evidence_grades: &missing_evidence_grades,
            required_tests: &required_tests,
            disposition,
            disposition_rule,
            telos_concern: &telos_concern,
        },
    );

    Ok(json!({
        "schema": TEST_SELECTION_SCHEMA,
        "generated_at_unix_ms": now_ms(),
        "subject": field["changed_subject"],
        "source_refs": source_refs,
        "telos_ux_concern": telos_concern,
        "affected_practices": required_skills,
        "practice_selection_basis": if decision.is_some() { json!("practices selected by the contemplation decision (Jev, strictly above threshold)") } else { json!("no decision supplied: no practice is claimed affected") },
        "unavailable_practice_boundaries": unavailable_boundaries,
        "required_skills": required_skills,
        "affected_capabilities": affected_capabilities,
        "gitnexus_structural_findings": gitnexus_structural,
        "known_tests": field["tests_evidence"],
        "tests_now_required": required_tests,
        "negative_controls": negative_controls,
        "missing_evidence_grades": missing_evidence_grades,
        "evidence_grade_law": EVIDENCE_GRADE_LAW,
        "recommended_disposition": disposition,
        "disposition_rule": disposition_rule,
        "suggested_participant_split": participant_split,
        "markdown": markdown,
    }))
}

/// The already-computed parts of a test selection the Markdown reading renders.
struct SelectionReading<'a> {
    affected_capabilities: &'a [Value],
    required_skills: &'a [Value],
    missing_evidence_grades: &'a [Value],
    required_tests: &'a [Value],
    disposition: &'a str,
    disposition_rule: &'a str,
    telos_concern: &'a str,
}

fn render_markdown(field: &Value, reading: &SelectionReading<'_>) -> String {
    let SelectionReading {
        affected_capabilities,
        required_skills,
        missing_evidence_grades,
        required_tests,
        disposition,
        disposition_rule,
        telos_concern,
    } = *reading;
    let mut out = String::new();
    out.push_str("# Test selection\n\n");
    out.push_str(&format!("**Telos/UX concern:** {telos_concern}\n\n"));
    if let Some(subject) = field.get("changed_subject").filter(|v| !v.is_null()) {
        out.push_str(&format!(
            "**Subject:** {} @ {} → {}\n\n",
            subject["repo_name"].as_str().unwrap_or("?"),
            subject["base_revision"].as_str().unwrap_or("?"),
            subject["head_revision"].as_str().unwrap_or("?"),
        ));
    }
    out.push_str(&format!(
        "**Recommended disposition:** {disposition}\n\n_Rule: {disposition_rule}_\n\n"
    ));
    out.push_str("## Affected Practices/METHODS\n\n");
    for skill in required_skills {
        out.push_str(&format!(
            "- {} — {}\n",
            skill["practice_id"].as_str().unwrap_or("?"),
            skill["binding_status"].as_str().unwrap_or("?"),
        ));
    }
    out.push_str("\n## Affected capabilities\n\n");
    for capability in affected_capabilities {
        out.push_str(&format!(
            "- `{}` ({}) — {}\n",
            capability["capability_id"].as_str().unwrap_or("?"),
            capability["label"].as_str().unwrap_or("?"),
            capability["need"].as_str().unwrap_or(""),
        ));
    }
    out.push_str("\n## Tests now required (run each; account for every one)\n\n");
    for test in required_tests {
        let command = test["invocation"]["command"].as_str();
        out.push_str(&format!(
            "- `{}` → {}{}\n",
            test["capability_id"].as_str().unwrap_or("?"),
            match command {
                Some(c) => format!("`{c}`"),
                None => format!(
                    "`{}` (evidence document, not executable)",
                    test["test_ref"].as_str().unwrap_or("?")
                ),
            },
            match test["exists_at_head"].as_bool() {
                Some(true) => "",
                Some(false) => " — **missing at head**",
                None => " — existence not assessed (candidate capability)",
            },
        ));
    }
    out.push_str("\n## Negative controls (per implicated capability)\n\n");
    out.push_str("For each capability run at least one: disconnected producer (the test must fail without the \
real producer), stale (fails against the pre-change revision), wrong subject (does not pass on an \
unrelated subject). A control you cannot run is reported as blocked with the reason.\n\n");
    out.push_str("## Evidence earned so far\n\n");
    for grade in missing_evidence_grades {
        out.push_str(&format!(
            "- `{}`: none yet (declared tests at head: {})\n",
            grade["capability_id"].as_str().unwrap_or("?"),
            match grade["declared_tests_exist_at_head"].as_bool() {
                Some(true) => "yes",
                Some(false) => "no",
                None => "not assessed",
            },
        ));
    }
    out.push_str(&format!("\n{EVIDENCE_GRADE_LAW}\n\n"));
    out.push_str("**Completion:** every required test and control is accounted as passed / failed / \
blocked / unavailable with the exact command and the revision it ran against. A green subset is not \
completion.\n");
    out
}

// ---------------------------------------------------------------------
// 3. Redis publication of the field/decision/test-selection
// ---------------------------------------------------------------------

fn parse_basis_pair(raw: &str) -> Result<(String, String)> {
    let (key, value) = raw.split_once('=').ok_or_else(|| {
        fail(
            "contemplation_intel.basis_ref_invalid",
            "expected REF=REVISION",
        )
    })?;
    Ok((key.to_owned(), value.to_owned()))
}

/// Bound on each prepared-view excerpt: the hot view is a situating reading,
/// not a copy of the documents it points at.
const COMPACT_EXCERPT_BYTES: usize = 12 * 1024;

fn ids(values: &Value, key: &str) -> Vec<Value> {
    values
        .as_array()
        .map(|items| items.iter().map(|i| i[key].clone()).collect())
        .unwrap_or_default()
}

/// A compact, source-qualified reading of one contemplation document for the
/// prepared view. Explicit relations are named in full; bulk material
/// (joins, readings, answers) is counted and left at the route.
fn compact_projection(kind: &str, doc: &Value) -> String {
    let compact = match kind {
        "field" => {
            let mut caps: Vec<Value> = doc["joins"]
                .as_array()
                .map(|j| {
                    j.iter()
                        .filter(|j| j["basis"] == "explicit")
                        .map(|j| j["to"].clone())
                        .collect()
                })
                .unwrap_or_default();
            caps.sort_by_key(|v| v.to_string());
            caps.dedup();
            let change_reading = doc["code_lens"]["readings"]
                .as_array()
                .and_then(|r| r.iter().find(|r| r["kind"] == "detect_changes"))
                .and_then(|r| r["detail"].as_str())
                .map(|d| d.lines().take(3).collect::<Vec<_>>().join(" / "));
            json!({
                "schema": doc["schema"],
                "pass": doc["pass"],
                "subject": {
                    "repo": doc["changed_subject"]["repo"],
                    "base_revision": doc["changed_subject"]["base_revision"],
                    "head_revision": doc["changed_subject"]["head_revision"],
                    "changed_paths": doc["changed_subject"]["changed_paths"].as_array().map(|a| a.len()),
                },
                "telos_serving_track": doc["telos"]["serving_track"],
                "practice_bindings": doc["practice_binding_summary"],
                "capabilities_in_matrix": doc["matrix"]["capabilities"].as_array().map(|a| a.len()),
                "explicitly_implicated_capabilities": caps,
                "code_lens": {
                    "provider": doc["code_lens"]["provider"],
                    "version": doc["code_lens"]["version"],
                    "indexed": doc["code_lens"]["indexed"],
                    "detect_changes": change_reading,
                    "readings": doc["code_lens"]["readings"].as_array().map(|a| a.len()),
                },
                "tests_evidence_rows": doc["tests_evidence"].as_array().map(|a| a.len()),
            })
        }
        "decision" => json!({
            "schema": doc["schema"],
            "pass": doc["pass"],
            "standing": doc["standing"],
            "returned_model": doc["returned_model"],
            "usage": doc["usage"],
            "tariff_cost_microusd": doc["tariff_cost_microusd"],
            "question_set": [doc["question_set_schema"].clone(), doc["question_set_version"].clone()],
            "mandatory_capabilities": ids(&doc["mandatory"]["capability-implicated"], "capability_id"),
            "selected_practices": ids(&doc["selected"]["practice-applies"], "practice_id"),
            "selected_capability_candidates": ids(&doc["selected"]["capability-implicated"], "capability_id"),
            "evidence_sufficiency_flagged": ids(&doc["selected"]["evidence-sufficiency"], "capability_id"),
            "proposed_coverage_bindings": doc["proposed_coverage_bindings"].as_array().map(|a| a.len()),
            "proposed_coverage_bindings_standing": doc["proposed_coverage_bindings_standing"],
        }),
        "test-selection" => {
            return doc["markdown"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| doc.to_string());
        }
        _ => doc.clone(),
    };
    serde_json::to_string_pretty(&compact).unwrap_or_default()
}

fn item_for_document(
    kind: &str,
    path: &Path,
    schema_check: Option<&str>,
) -> Result<(NowContextItem, String)> {
    let (bytes, digest) = file_digest(path, kind, MAX_DOC_BYTES)?;
    let doc: Value = serde_json::from_slice(&bytes).map_err(|e| {
        fail(
            "contemplation_intel.document_invalid",
            format!("{kind}: {e}"),
        )
    })?;
    if let Some(expected) = schema_check {
        if doc["schema"] != expected {
            return Err(fail(
                "contemplation_intel.document_invalid",
                format!("{kind} does not carry schema {expected}"),
            ));
        }
    }
    let source_ref = ResourceRef::parse(format!("context-source/contemplation/{kind}"))?;
    let item = NowContextItem {
        source_ref: source_ref.clone(),
        source_revision: digest.clone(),
        title: format!("Contemplation {kind}"),
        // The hot view carries a compact reading; the full document is fetched
        // on demand from `route` at the stated revision, never inlined.
        excerpt: bounded_text(&compact_projection(kind, &doc), COMPACT_EXCERPT_BYTES),
        route: Some(
            std::fs::canonicalize(path)
                .unwrap_or_else(|_| path.to_path_buf())
                .display()
                .to_string(),
        ),
        agent_visibility: AgentVisibility::Payload,
        external_egress: ExternalEgress::Denied,
    };
    Ok((item, digest))
}

pub fn now_publish_intelligence(args: NowPublishIntelligenceArgs) -> Result<Value> {
    let config: RedisNowConfig = read_json(&args.config_file, "Redis NOW config", 256 * 1024)?;
    let participant_ref = ResourceRef::parse(&args.participant_ref)?;
    let project_ref = ResourceRef::parse(&args.project_ref)?;
    let now_ref = ResourceRef::parse(&args.now_ref)?;
    let agent_session = ResourceRef::parse(&args.agent_session)?;
    let secret = resolve_secret(&config, args.allow_env_import)?;
    let store = RedisNowStore::new(config)?;

    let (field_item, field_digest) = item_for_document(
        "field",
        &args.field_file,
        Some(crate::contemplation_field::FIELD_SCHEMA),
    )?;
    let mut items = vec![field_item];
    let mut jev_invocation_ref = None;
    let mut source_revisions = BTreeMap::from([(field_item_key(&args.field_file), field_digest)]);
    if let Some(path) = &args.decision_file {
        let (item, digest) = item_for_document("decision", path, Some(DECISION_SCHEMA))?;
        let decision: Value = read_json(path, "contemplation decision", 4 * 1024 * 1024)?;
        jev_invocation_ref = decision["invocation_ref"]
            .as_str()
            .map(ResourceRef::parse)
            .transpose()?;
        source_revisions.insert(field_item_key(path), digest);
        items.push(item);
    }
    if let Some(path) = &args.test_selection_file {
        let (item, digest) =
            item_for_document("test-selection", path, Some(TEST_SELECTION_SCHEMA))?;
        source_revisions.insert(field_item_key(path), digest);
        items.push(item);
    }

    if let (Some(root), Some(_)) = (&args.central_root, Some(())) {
        let ctrl = args
            .ctrl_bin
            .clone()
            .unwrap_or_else(central_file_map::executable);
        let runner = SystemRunner::new().with_timeout(std::time::Duration::from_secs(20));
        let day = central_action(&runner, &ctrl, root, "central.day.read", &json!({}))?;
        if let (Some(day_ref), Some(revision)) = (
            day["day_ref"].as_str(),
            day["revision"]["revision"].as_str(),
        ) {
            source_revisions.insert(format!("day:{day_ref}"), revision.to_owned());
        }
    }
    for raw in &args.now_basis_refs {
        let (key, value) = parse_basis_pair(raw)?;
        source_revisions.insert(key, value);
    }

    let ack = store.ack_cursor(&participant_ref, secret.as_ref())?;
    let last_delivery = store.last_delivery(&participant_ref, secret.as_ref())?;
    let change_cursor = ack.max(last_delivery.as_ref().map(|r| r.change_cursor).unwrap_or(0));
    let basis = NowContextBasis {
        source_revisions,
        dependency_revisions: BTreeMap::new(),
        disclosure_revision: args.disclosure_revision.clone(),
        factory_revision: None,
        change_cursor,
    };
    let view = PreparedNowContext {
        schema: NOW_PREPARED_SCHEMA.into(),
        project_ref,
        now_ref,
        participant_ref: participant_ref.clone(),
        agent_session,
        version: args.expected_version.checked_add(1).ok_or_else(|| {
            fail(
                "contemplation_intel.version_exhausted",
                "prepared version exhausted",
            )
        })?,
        basis_digest: basis.digest()?,
        basis,
        concern: args.concern.clone(),
        practice_refs: vec![],
        items,
        neighbours: vec![],
        factory: None,
        knowledge_frames: vec![],
        continuation: None,
        jev_invocation_ref,
        prepared_at_unix_ms: now_ms(),
    };
    view.validate(false)?;
    let published_version = store.publish(&view, args.expected_version, secret.as_ref())?;

    let mut appended = Vec::new();
    for item in &view.items {
        let change = NowContextChange {
            change_id: format!(
                "contemplation-publish-{}-{}",
                published_version, item.source_ref
            ),
            kind: "contemplation-intelligence-published".into(),
            source_ref: item.source_ref.clone(),
            source_revision: item.source_revision.clone(),
            detail: format!(
                "{} published at prepared version {published_version}",
                item.title
            ),
            observed_at_unix_ms: now_ms(),
        };
        let cursor = store.append_change(&participant_ref, &change, secret.as_ref())?;
        appended.push(json!({"source_ref": item.source_ref, "cursor": cursor}));
    }

    Ok(json!({
        "schema": "aikit.contemplation-publish/v1",
        "participantRef": participant_ref,
        "publishedVersion": published_version,
        "basisDigest": view.basis_digest,
        "jevInvocationRef": view.jev_invocation_ref,
        "items": view.items.iter().map(|i| json!({"source_ref": i.source_ref, "source_revision": i.source_revision})).collect::<Vec<_>>(),
        "appendedChanges": appended,
    }))
}

fn field_item_key(path: &Path) -> String {
    format!("document:{}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_field() -> Value {
        json!({
            "schema": crate::contemplation_field::FIELD_SCHEMA,
            "pass": "prospective",
            "telos": {"goal": "Ship the intelligence arm", "serving_track_excerpt": "the current track"},
            "spine": {
                "source": "/fixture/ux-spine-trace.json",
                "practices": [
                    {"id": "XP01", "purpose": "p1", "binding": {"status": "bound", "skill_ref": "skills/one"}, "stories_served": ["UX01"]},
                    {"id": "XP02", "purpose": "p2", "binding": {"status": "unbound"}, "stories_served": []},
                ],
            },
            "matrix": {
                "matrix_id": "matrix.fixture",
                "source_manifest": "/fixture/capability-matrix.json",
                "source_csv": "/fixture/capability-matrix.csv",
                "capabilities": [
                    {"id": "cap.explicit", "need": "n1", "operation": "o1", "outcome": "out1", "implementation_status": "implemented", "standing": "implementation-fact", "test_refs": ["tests/one.rs"]},
                    {"id": "cap.candidate.a", "need": "n2", "operation": "o2", "outcome": "out2", "implementation_status": "implemented", "standing": "implementation-fact", "test_refs": []},
                    {"id": "cap.candidate.b", "need": "n3", "operation": "o3", "outcome": "out3", "implementation_status": "implemented", "standing": "implementation-fact", "test_refs": []},
                ],
            },
            "changed_subject": {
                "repo_name": "fixture-repo",
                "source_ref": "source:git/fixture-repo",
                "base_revision": "git:aaa",
                "head_revision": "git:bbb",
                "changed_paths": ["src/one.rs"],
            },
            "code_lens": {
                "readings": [
                    {"kind": "symbol", "reference": {"path": "src/one.rs"}, "context": {"related": "src/one_test.rs"}, "impact_upstream": {}},
                ],
            },
            "joins": [
                {"from": "src/one.rs", "to": "cap.explicit", "relation": "changed-path-implements-capability", "basis": "explicit"},
                {"from": "src/two.rs", "to": "cap.candidate.a", "relation": "gitnexus-impacted-path-implicates-capability", "basis": "derived(gitnexus)"},
            ],
            "tests_evidence": [
                {"capability_id": "cap.explicit", "test_ref": "tests/one.rs", "exists_at_head": true},
            ],
            "experience_reading": Value::Null,
            "experience_reading_disclosure": "no --experience-reading was supplied",
            "now": Value::Null,
            "redis": {
                "participant_ref": "participant/intelligence-arm/implementer",
                "neighbours": [{"participant_ref": "participant/other", "task_ref": "task/x", "relation": "related-worker"}],
            },
            "return_document": Value::Null,
            "knowledge_frames": [],
        })
    }

    #[test]
    fn explicit_capability_is_mandatory_and_never_asked_as_a_question() {
        let field = fixture_field();
        let (request, built) =
            build_question_set(&field, "prospective", "jev-1.13.0".into()).unwrap();
        assert!(!request.questions.contains_key("capability/cap.explicit"));
        assert!(request.questions.contains_key("capability/cap.candidate.a"));
        assert!(request.questions.contains_key("capability/cap.candidate.b"));
        let mandatory = &built.mandatory["capability-implicated"];
        assert_eq!(mandatory.len(), 1);
        assert_eq!(mandatory[0]["capability_id"], "cap.explicit");
    }

    #[test]
    fn practices_below_threshold_ask_one_noul_per_practice() {
        let field = fixture_field();
        let (request, _built) =
            build_question_set(&field, "prospective", "jev-1.13.0".into()).unwrap();
        assert!(request.questions.contains_key("practice/XP01"));
        assert!(request.questions.contains_key("practice/XP02"));
        assert!(!request.questions.contains_key("practices"));
    }

    #[test]
    fn retrospective_pass_adds_the_five_warrant_questions_prospective_does_not() {
        let field = fixture_field();
        let (prospective, _) =
            build_question_set(&field, "prospective", "jev-1.13.0".into()).unwrap();
        let (retrospective, _) =
            build_question_set(&field, "retrospective", "jev-1.13.0".into()).unwrap();
        for (key, _) in RETROSPECTIVE_WARRANTS {
            assert!(!prospective.questions.contains_key(&format!("retro/{key}")));
            assert!(retrospective
                .questions
                .contains_key(&format!("retro/{key}")));
        }
        assert_eq!(
            retrospective.questions.len(),
            prospective.questions.len() + 5
        );
    }

    #[test]
    fn code_impact_candidates_come_from_derived_joins_only() {
        let field = fixture_field();
        let (request, built) =
            build_question_set(&field, "prospective", "jev-1.13.0".into()).unwrap();
        assert!(request.questions.contains_key("code-impact/000"));
        assert_eq!(
            built.meta["code-impact/000"].category,
            "code-impact-candidate"
        );
    }

    #[test]
    fn evidence_sufficiency_is_asked_once_per_explicit_capability_only() {
        let field = fixture_field();
        let (request, _built) =
            build_question_set(&field, "prospective", "jev-1.13.0".into()).unwrap();
        assert!(request
            .questions
            .contains_key("evidence-sufficiency/cap.explicit"));
        assert!(!request
            .questions
            .contains_key("evidence-sufficiency/cap.candidate.a"));
        match &request.questions["evidence-sufficiency/cap.explicit"] {
            Question::Choice { criteria, .. } => {
                assert_eq!(
                    criteria.keys().cloned().collect::<Vec<_>>(),
                    vec!["absent", "contradictory", "stale", "sufficient-at-grade"]
                );
            }
            other => panic!("expected a Choice question, got {other:?}"),
        }
    }

    #[test]
    fn neighbours_become_one_noul_question_each() {
        let field = fixture_field();
        let (request, _built) =
            build_question_set(&field, "prospective", "jev-1.13.0".into()).unwrap();
        assert!(request.questions.contains_key("neighbour/000"));
    }

    #[test]
    fn experience_candidates_are_resolved_against_inventory_repository_not_candidate_repository() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ux-reading.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "capability_inventory": [
                    {"repository": "ai-kit", "capability_id": "cap.story", "source_digest": "sha256:abc"},
                ],
                "capability_candidates": [
                    {"repository": "EpiLogos/ai-kit", "capability_id": "cap.story", "story_ids": ["UX01"], "reason": "r", "binding_status": "candidate"},
                ],
            }))
            .unwrap(),
        )
        .unwrap();
        let (bytes, digest) = file_digest(&path, "x", 1024 * 1024).unwrap();
        let _ = bytes;
        let mut field = fixture_field();
        field["experience_reading"] =
            json!({"source": path.display().to_string(), "source_revision": digest});

        let mut built = BuiltQuestions {
            questions: BTreeMap::new(),
            meta: BTreeMap::new(),
            mandatory: BTreeMap::new(),
            category_counts: BTreeMap::new(),
        };
        let mut budget = MAX_TOTAL_CANDIDATE_QUESTIONS;
        let disclosure = build_story_category(&mut built, &mut budget, &field).unwrap();
        assert_eq!(disclosure["available"], true);
        let subject = &built.meta["story-candidate/000"].subject;
        assert_eq!(subject["inventory_matched"], true);
        assert_eq!(subject["inventory_repository"], "ai-kit");
        assert_eq!(subject["source_digest"], "sha256:abc");
        assert_eq!(subject["candidate_repository"], "EpiLogos/ai-kit");
    }

    #[test]
    fn a_changed_experience_reading_is_refused_as_stale() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ux-reading.json");
        std::fs::write(
            &path,
            b"{\"capability_candidates\":[],\"capability_inventory\":[]}",
        )
        .unwrap();
        let mut field = fixture_field();
        field["experience_reading"] =
            json!({"source": path.display().to_string(), "source_revision": "blake3:stale"});
        let error = read_experience_candidates(&field).unwrap_err();
        assert_eq!(error.code(), "contemplation_intel.experience_reading_stale");
    }

    #[test]
    fn test_selection_labels_capabilities_explicit_derived_and_jev_candidate() {
        let field = fixture_field();
        let decision = json!({
            "field_basis_digest": "irrelevant-in-this-direct-call",
            "selected": {"capability-implicated": [{"capability_id": "cap.candidate.b"}]},
        });
        // Build directly rather than through the CLI arg path (no files on disk).
        let joins = field["joins"].as_array().cloned().unwrap();
        let mut explicit_ids = BTreeSet::new();
        let mut derived_ids = BTreeSet::new();
        for join in &joins {
            let to = join["to"].as_str().unwrap();
            match join["basis"].as_str() {
                Some("explicit") => {
                    explicit_ids.insert(to.to_owned());
                }
                Some(b) if b.starts_with("derived(") => {
                    derived_ids.insert(to.to_owned());
                }
                _ => {}
            }
        }
        assert_eq!(explicit_ids, BTreeSet::from(["cap.explicit".to_owned()]));
        assert_eq!(derived_ids, BTreeSet::from(["cap.candidate.a".to_owned()]));
        let mut jev_candidate_ids = BTreeSet::new();
        for subject in decision["selected"]["capability-implicated"]
            .as_array()
            .unwrap()
        {
            let id = subject["capability_id"].as_str().unwrap();
            if !explicit_ids.contains(id) && !derived_ids.contains(id) {
                jev_candidate_ids.insert(id.to_owned());
            }
        }
        assert_eq!(
            jev_candidate_ids,
            BTreeSet::from(["cap.candidate.b".to_owned()])
        );
    }

    /// Drive the real `now_test_selection` over a field written to disk and an
    /// optional decision bound to that field's digest.
    fn run_selection(field: &Value, selected_practices: Option<&[&str]>) -> Value {
        let dir = tempfile::tempdir().unwrap();
        let field_path = dir.path().join("field.json");
        std::fs::write(&field_path, serde_json::to_vec(field).unwrap()).unwrap();
        let decision = selected_practices.map(|ids| {
            let (_, digest) = file_digest(&field_path, "field", MAX_FIELD_BYTES).unwrap();
            let path = dir.path().join("decision.json");
            let practices: Vec<Value> = ids.iter().map(|id| json!({"practice_id": id})).collect();
            std::fs::write(
                &path,
                serde_json::to_vec(&json!({
                    "field_basis_digest": digest,
                    "selected": {"practice-applies": practices},
                }))
                .unwrap(),
            )
            .unwrap();
            path
        });
        now_test_selection(NowTestSelectionArgs {
            field: field_path,
            decision,
        })
        .unwrap()
    }

    #[test]
    fn without_a_decision_no_practice_is_claimed_affected() {
        let out = run_selection(&fixture_field(), None);
        assert_eq!(out["affected_practices"], json!([]));
        assert_ne!(
            out["disposition_rule"].as_str().unwrap_or_default(),
            "",
            "{out}"
        );
        assert!(
            !out["disposition_rule"]
                .as_str()
                .unwrap()
                .contains("no bound Skill"),
            "an unbound practice nobody selected must not drive the disposition: {out}"
        );
    }

    #[test]
    fn a_selected_unbound_practice_makes_the_disposition_factory() {
        // fixture_field has XP02 unbound.
        let out = run_selection(&fixture_field(), Some(&["XP02"]));
        assert_eq!(out["recommended_disposition"], "Factory", "{out}");
        assert!(out["disposition_rule"]
            .as_str()
            .unwrap()
            .contains("no bound Skill"));
    }

    #[test]
    fn a_legitimately_unavailable_practice_is_a_boundary_not_a_commission() {
        let mut field = fixture_field();
        let xp02 = field["spine"]["practices"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["id"] == "XP02")
            .unwrap();
        xp02["binding"]["classification"] = json!("legitimately-unavailable");
        xp02["binding"]["gap_owner"] = json!("#132 K8.1");
        let out = run_selection(&field, Some(&["XP02"]));
        assert!(
            !out["disposition_rule"]
                .as_str()
                .unwrap()
                .contains("no bound Skill"),
            "{out}"
        );
        assert_eq!(
            out["unavailable_practice_boundaries"][0]["practice_id"],
            "XP02"
        );
        assert_eq!(
            out["unavailable_practice_boundaries"][0]["gap_owner"],
            "#132 K8.1"
        );
    }

    #[test]
    fn prepared_items_are_compact_and_route_to_the_real_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut field = fixture_field();
        // Bulk that must not be inlined into the hot view.
        field["bulk"] = json!("x".repeat(300 * 1024));
        let path = dir.path().join("field.json");
        std::fs::write(&path, serde_json::to_vec(&field).unwrap()).unwrap();
        let (item, digest) = item_for_document(
            "field",
            &path,
            Some(crate::contemplation_field::FIELD_SCHEMA),
        )
        .unwrap();
        assert!(
            item.excerpt.len() <= COMPACT_EXCERPT_BYTES,
            "{}",
            item.excerpt.len()
        );
        assert!(!item.excerpt.contains("xxxxxxxx"));
        let route = std::path::PathBuf::from(item.route.as_deref().unwrap());
        let (_, routed_digest) = file_digest(&route, "field", MAX_FIELD_BYTES).unwrap();
        assert_eq!(
            routed_digest, digest,
            "the route must open the exact document at the stated revision"
        );
        assert_eq!(item.source_revision, digest);
    }

    #[test]
    fn declared_tests_map_to_runnable_commands_or_are_named_evidence_documents() {
        assert_eq!(
            test_invocation("crates/aikit-cli/tests/adopt_command.rs")["command"],
            "cargo test -p aikit-cli --test adopt_command"
        );
        assert_eq!(
            test_invocation("crates/aikit-core/src/routine.rs")["command"],
            "cargo test -p aikit-core --lib"
        );
        assert_eq!(
            test_invocation("scripts/jev-redis/joined_proof.py")["kind"],
            "python-script"
        );
        let doc = test_invocation("schemas/aikit.routine-invocation-evidence.v1.schema.json");
        assert_eq!(doc["kind"], "evidence-document");
        assert!(doc["command"].is_null());
    }

    #[test]
    fn an_existing_test_file_earns_no_grade_and_the_law_is_quoted_exactly() {
        let out = run_selection(&fixture_field(), None);
        for row in out["missing_evidence_grades"].as_array().unwrap() {
            assert!(row["earned_grade"].is_null(), "{row}");
        }
        let law = out["evidence_grade_law"].as_str().unwrap();
        assert!(law.contains("C = cross-product conformance"));
        assert!(law.contains("P = real provider/harness"));
        assert!(
            !law.contains("peer"),
            "P is not 'peer' in the O:I grade law"
        );
    }

    #[test]
    fn a_selected_candidate_capabilitys_declared_tests_are_required_and_unassessed() {
        let mut field = fixture_field();
        field["matrix"]["capabilities"][2]["test_refs"] =
            json!(["crates/demo/tests/candidate_b.rs"]);
        let dir = tempfile::tempdir().unwrap();
        let field_path = dir.path().join("field.json");
        std::fs::write(&field_path, serde_json::to_vec(&field).unwrap()).unwrap();
        let (_, digest) = file_digest(&field_path, "field", MAX_FIELD_BYTES).unwrap();
        let decision_path = dir.path().join("decision.json");
        std::fs::write(
            &decision_path,
            serde_json::to_vec(&json!({
                "field_basis_digest": digest,
                "selected": {"capability-implicated": [{"capability_id": "cap.candidate.b"}]},
            }))
            .unwrap(),
        )
        .unwrap();
        let out = now_test_selection(NowTestSelectionArgs {
            field: field_path,
            decision: Some(decision_path),
        })
        .unwrap();
        let required: Vec<&Value> = out["tests_now_required"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["capability_id"] == "cap.candidate.b")
            .collect();
        assert_eq!(required.len(), 1, "{out}");
        assert!(required[0]["exists_at_head"].is_null());
        assert_eq!(
            required[0]["invocation"]["command"],
            "cargo test -p demo --test candidate_b"
        );
        let grade = out["missing_evidence_grades"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["capability_id"] == "cap.candidate.b")
            .unwrap();
        assert!(
            grade["declared_tests_exist_at_head"].is_null(),
            "not assessed is not missing"
        );
    }

    #[test]
    fn batches_carry_the_whole_state_and_every_question_once_under_the_ceiling() {
        let mut questions = BTreeMap::new();
        for i in 0..40 {
            questions.insert(
                format!("capability/cap.{i:03}"),
                Question::Noul {
                    instructions: json!({"category": "capability-implicated", "capability_id": format!("cap.{i:03}"), "question": "Is this capability materially implicated by the current concern?"}),
                    criteria: None,
                },
            );
        }
        let request = JevRequest {
            model: "jev-1.13.0".into(),
            state: json!({"capability_matrix": "x".repeat(4000)}),
            questions,
        };
        let ceiling = 3000;
        let batches = split_into_batches(&request, ceiling).unwrap();
        assert!(batches.len() > 1, "this request must not fit one call");
        let mut seen = BTreeSet::new();
        for batch in &batches {
            assert_eq!(
                batch.state, request.state,
                "every batch carries the whole state"
            );
            assert!(estimated_input_tokens(batch) <= ceiling);
            for id in batch.questions.keys() {
                assert!(seen.insert(id.clone()), "question {id} asked twice");
            }
        }
        assert_eq!(seen.len(), 40);
        // A state that cannot fit alone is refused, never silently truncated.
        assert!(split_into_batches(&request, 1000).is_err());
    }

    #[test]
    fn an_individual_oversized_question_is_refused_before_any_batch_can_run() {
        let request = JevRequest {
            model: "jev-1.13.0".into(),
            state: json!({}),
            questions: BTreeMap::from([(
                "large".into(),
                Question::Noul {
                    instructions: json!("x".repeat(10000)),
                    criteria: None,
                },
            )]),
        };
        assert!(split_into_batches(&request, 1000).is_err());
    }

    #[test]
    fn batches_share_original_reservation_attempt_and_time_limits() {
        let limits = JevLimits {
            timeout_ms: 10000,
            max_attempts: 3,
            max_total_reserved_microusd: 300,
            tariff: aikit_core::jev::JevTariff {
                model_version: "jev-1.13.0".into(),
                source: "test tariff".into(),
                max_input_tokens_per_attempt: 1000,
                max_output_tokens_per_attempt: 1000,
                input_microusd_per_million_tokens: 50000,
                output_microusd_per_million_tokens: 50000,
            },
        };
        let remaining = remaining_batch_limits(&limits, 100, 1, 2000).unwrap();
        assert_eq!(remaining.max_total_reserved_microusd, 200);
        assert_eq!(remaining.max_attempts, 2);
        assert_eq!(remaining.timeout_ms, 8000);
        assert!(remaining_batch_limits(&limits, 250, 1, 2000).is_err());
        assert!(remaining_batch_limits(&limits, 100, 3, 2000).is_err());
        assert!(remaining_batch_limits(&limits, 100, 1, 10000).is_err());
    }

    #[test]
    fn a_noul_exactly_at_the_threshold_is_not_selected() {
        let (selected, _, _) = describe_answer(&Answer::Noul { noul: 0.5 }, 0.5);
        assert!(!selected);
        let (selected, _, _) = describe_answer(&Answer::Noul { noul: 0.51 }, 0.5);
        assert!(selected);
    }

    #[test]
    fn evidence_grade_never_claims_above_c() {
        assert!(EVIDENCE_GRADE_LAW.contains("Automation earns at most D, or C with such a receipt"));
        assert!(
            EVIDENCE_GRADE_LAW.contains(
                "never \
     earned by automation"
            ) || EVIDENCE_GRADE_LAW.contains("never earned by automation")
        );
    }
}
