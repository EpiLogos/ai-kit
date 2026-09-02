//! Fast ResourceRef-native navigation search for the V2 human shell.
//!
//! This is deliberately a shallow index. It never invokes ContextSource,
//! semantic, knowledge-graph or QL providers. Deep providers may contribute
//! already-addressable resources elsewhere; the Quick path only ranks the
//! descriptors and navigation evidence already present here.
//!
//! Familiarity is applied as derived navigation evidence. Search relevance and
//! authored preference remain lexicographically above learned accessibility, so
//! repetition can make an otherwise-equivalent destination easier to recover but
//! can never turn use into authority, preference, eligibility or truth.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::familiarity::{AccessibilityAssessment, FamiliarityContext, FamiliarityStore};
use crate::{AikitError, Result};

use super::{ResourceIndex, ResourceKind, ResourceRecord, ResourceRef};

const FAMILIARITY_EVIDENCE_PREFIX: &str = "familiarity:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NavigationEvidenceClass {
    CurrentContext,
    ExplicitPin,
    Recent,
    LearnedUsage,
    ChangedProject,
}

impl NavigationEvidenceClass {
    fn zero_query_rank(self) -> i32 {
        match self {
            Self::CurrentContext => 500,
            Self::ExplicitPin => 400,
            Self::ChangedProject => 300,
            Self::Recent => 200,
            Self::LearnedUsage => 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationEvidence {
    pub class: NavigationEvidenceClass,
    #[serde(default)]
    pub detail: Option<String>,
}

impl NavigationEvidence {
    pub fn new(class: NavigationEvidenceClass) -> Self {
        Self {
            class,
            detail: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionStageability {
    NotStageable,
    Stageable,
}

impl ActionStageability {
    pub fn is_stageable(self) -> bool {
        matches!(self, Self::Stageable)
    }
}

/// One canonical Action made contextual to a ResourceRef.
///
/// `action` names one unique `ResourceKind::Action` record in this same index.
/// `subject` remains the Resource on which that Action currently applies. The
/// relation never manufactures a second Action identity merely because the same
/// operation is available on another subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextualActionDescriptor {
    pub action: ResourceRef,
    pub subject: ResourceRef,
    pub label: String,
    pub description: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub stageability: ActionStageability,
}

impl ContextualActionDescriptor {
    pub fn new(
        action: ResourceRef,
        subject: ResourceRef,
        label: impl Into<String>,
        description: impl Into<String>,
        stageability: ActionStageability,
    ) -> Self {
        Self {
            action,
            subject,
            label: label.into(),
            description: description.into(),
            keywords: Vec::new(),
            stageability,
        }
    }

    #[must_use]
    pub fn with_keywords(mut self, keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceSearchHitKind {
    /// A unique resource in the navigation field. Actions are returned here once
    /// by their canonical Action ResourceRef; contextual applicability is loaded
    /// separately through `actions_for(subject)`.
    Resource,
}

/// Explainable ranking inputs for one shallow-search hit.
///
/// All learned floating-point assessments are quantised only for deterministic
/// ordering/display here. Their source observations and full assessment remain in
/// the familiarity subsystem. No field in this structure changes resource
/// eligibility, trust or authored state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResourceRankingSignals {
    pub text_relevance: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_preference_rank: Option<i32>,
    #[serde(default)]
    pub learned_observations: usize,
    #[serde(default)]
    pub learned_contextual_observations: usize,
    #[serde(default)]
    pub learned_frecency_milli: i64,
    #[serde(default)]
    pub learned_contextual_frecency_milli: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learned_contextual_fitness_milli: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceSearchHit {
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub hit_kind: ResourceSearchHitKind,
    pub label: String,
    pub summary: String,
    /// Compatibility/display score for the primary search/navigation signal.
    /// Ordering additionally uses the explicit fields in `ranking`.
    pub score: i64,
    #[serde(default)]
    pub ranking: ResourceRankingSignals,
    #[serde(default)]
    pub navigation_evidence: Vec<NavigationEvidence>,
}

#[derive(Debug, Clone)]
struct IndexedResource {
    record: ResourceRecord,
    evidence: Vec<NavigationEvidence>,
    familiarity: Option<AccessibilityAssessment>,
}

#[derive(Debug, Clone, Default)]
pub struct ResourceSearchIndex {
    resources: BTreeMap<ResourceRef, IndexedResource>,
    actions: BTreeMap<(ResourceRef, ResourceRef), ContextualActionDescriptor>,
}

impl ResourceSearchIndex {
    pub fn insert_resource(
        &mut self,
        record: ResourceRecord,
        evidence: Vec<NavigationEvidence>,
    ) -> Option<ResourceRecord> {
        let id = record.descriptor.id.clone();
        self.resources
            .insert(
                id,
                IndexedResource {
                    record,
                    evidence,
                    familiarity: None,
                },
            )
            .map(|previous| previous.record)
    }

    pub fn add_evidence(
        &mut self,
        resource: &ResourceRef,
        evidence: NavigationEvidence,
    ) -> Result<()> {
        let Some(indexed) = self.resources.get_mut(resource) else {
            return Err(AikitError::new(
                "resource.search_unknown_resource",
                format!("cannot attach navigation evidence to unknown resource {resource}"),
            ));
        };
        indexed.evidence.push(evidence);
        Ok(())
    }

    /// Apply rebuildable learned-accessibility evidence to this already-resolved
    /// navigation field.
    ///
    /// Calling this repeatedly is idempotent with respect to familiarity evidence:
    /// only summaries produced by this method are replaced. Other learned evidence
    /// (for example V1 successful-run history) is retained until it is deliberately
    /// migrated into the unified observation stream. Canonical resource records are
    /// never mutated.
    pub fn apply_familiarity(
        &mut self,
        familiarity: &FamiliarityStore,
        context: &FamiliarityContext,
        now_ms: u64,
        half_life_ms: u64,
    ) {
        for indexed in self.resources.values_mut() {
            indexed.evidence.retain(|evidence| {
                !(evidence.class == NavigationEvidenceClass::LearnedUsage
                    && evidence
                        .detail
                        .as_deref()
                        .is_some_and(|detail| detail.starts_with(FAMILIARITY_EVIDENCE_PREFIX)))
            });

            let assessment = familiarity.assess_destination(
                &indexed.record.descriptor.id,
                context,
                now_ms,
                half_life_ms,
            );
            if assessment.is_empty() {
                indexed.familiarity = None;
                continue;
            }

            let mut detail = format!(
                "{FAMILIARITY_EVIDENCE_PREFIX} {} observed use{}; {} contextual; frecency {:.3}; contextual {:.3}",
                assessment.observations,
                if assessment.observations == 1 { "" } else { "s" },
                assessment.contextual_observations,
                assessment.frecency,
                assessment.contextual_frecency,
            );
            if let Some(fitness) = assessment.contextual_fitness_milli {
                detail.push_str(&format!("; contextual fitness {fitness:.0}/1000"));
            }
            indexed.evidence.push(
                NavigationEvidence::new(NavigationEvidenceClass::LearnedUsage).with_detail(detail),
            );
            indexed.familiarity = Some(assessment);
        }
    }

    pub fn insert_action(&mut self, action: ContextualActionDescriptor) -> Result<()> {
        let Some(action_record) = self.resources.get(&action.action) else {
            return Err(AikitError::new(
                "resource.search_unknown_action",
                format!(
                    "contextual action {} is not present in the resource index",
                    action.action
                ),
            ));
        };
        if action_record.record.descriptor.kind != ResourceKind::Action {
            return Err(AikitError::new(
                "resource.search_wrong_action_kind",
                format!(
                    "contextual action {} has kind {}, expected action",
                    action.action,
                    action_record.record.descriptor.kind.as_str()
                ),
            ));
        }
        if !self.resources.contains_key(&action.subject) {
            return Err(AikitError::new(
                "resource.search_unknown_subject",
                format!(
                    "contextual action subject {} is not present in the resource index",
                    action.subject
                ),
            ));
        }
        self.actions
            .insert((action.subject.clone(), action.action.clone()), action);
        Ok(())
    }

    /// Actions currently applicable to one selected Resource.
    ///
    /// The returned relation carries subject and stageability while preserving one
    /// canonical Action identity across any number of subjects.
    pub fn actions_for(&self, subject: &ResourceRef) -> Vec<&ContextualActionDescriptor> {
        self.actions
            .iter()
            .filter_map(|((candidate, _), action)| (candidate == subject).then_some(action))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.resources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    /// Search the already-resolved navigation field.
    ///
    /// Ordering is deliberately lexicographic rather than a blended magic score:
    /// primary text/current-navigation relevance → explicit authored preference →
    /// learned contextual accessibility/fitness → deterministic identity ties.
    /// This makes it impossible for repetition to outrank a more relevant match or
    /// authored preference merely by growing a large numeric weight.
    pub fn search(&self, query: &str, limit: usize) -> Vec<ResourceSearchHit> {
        if limit == 0 {
            return Vec::new();
        }
        let query = query.trim().to_lowercase();
        let mut hits = if query.is_empty() {
            self.zero_query_hits()
        } else {
            self.query_hits(&query)
        };
        hits.sort_by(compare_hits);
        hits.truncate(limit);
        hits
    }

    fn zero_query_hits(&self) -> Vec<ResourceSearchHit> {
        self.resources
            .values()
            .filter(|indexed| !indexed.evidence.is_empty())
            .map(|indexed| {
                let score = indexed
                    .evidence
                    .iter()
                    .map(|evidence| evidence.class.zero_query_rank() as i64)
                    .max()
                    .unwrap_or_default();
                resource_hit(indexed, score)
            })
            .collect()
    }

    fn query_hits(&self, query: &str) -> Vec<ResourceSearchHit> {
        let mut hits = Vec::new();
        for indexed in self.resources.values() {
            let descriptor = &indexed.record.descriptor;
            let mut score = [
                descriptor.name.as_str(),
                descriptor.description.as_str(),
                descriptor.id.as_str(),
                descriptor.kind.as_str(),
            ]
            .iter()
            .filter_map(|candidate| fuzzy_score(query, candidate))
            .max();

            // Action relationship vocabulary is useful for finding the canonical
            // Action resource, but it may never create one row per subject.
            if descriptor.kind == ResourceKind::Action {
                for contextual in self
                    .actions
                    .values()
                    .filter(|action| action.action == descriptor.id)
                {
                    score = score.max(fuzzy_score(query, &contextual.label));
                    score = score.max(fuzzy_score(query, &contextual.description));
                    for keyword in &contextual.keywords {
                        score = score.max(fuzzy_score(query, keyword));
                    }
                }
            }

            if let Some(score) = score {
                hits.push(resource_hit(indexed, score));
            }
        }
        hits
    }
}

/// The shallow human-navigation field is also a valid read-only `ResourceIndex`.
/// Project-world disclosure and ContextResolution can therefore consume the same
/// descriptor records without constructing a parallel resource registry or
/// invoking any deep provider.
impl ResourceIndex for ResourceSearchIndex {
    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord> {
        self.resources.get(id).map(|indexed| &indexed.record)
    }

    fn resources(&self) -> Vec<&ResourceRecord> {
        self.resources
            .values()
            .map(|indexed| &indexed.record)
            .collect()
    }
}

fn resource_hit(indexed: &IndexedResource, score: i64) -> ResourceSearchHit {
    let familiarity = indexed.familiarity.as_ref();
    ResourceSearchHit {
        resource: indexed.record.descriptor.id.clone(),
        kind: indexed.record.descriptor.kind,
        hit_kind: ResourceSearchHitKind::Resource,
        label: indexed.record.descriptor.name.clone(),
        summary: indexed.record.descriptor.description.clone(),
        score,
        ranking: ResourceRankingSignals {
            text_relevance: score,
            authored_preference_rank: indexed.record.preference.as_ref().map(|value| value.rank),
            learned_observations: familiarity.map_or(0, |value| value.observations),
            learned_contextual_observations: familiarity
                .map_or(0, |value| value.contextual_observations),
            learned_frecency_milli: familiarity.map_or(0, |value| quantise_milli(value.frecency)),
            learned_contextual_frecency_milli: familiarity
                .map_or(0, |value| quantise_milli(value.contextual_frecency)),
            learned_contextual_fitness_milli: familiarity
                .and_then(|value| value.contextual_fitness_milli)
                .map(|value| value.round() as i32),
        },
        navigation_evidence: indexed.evidence.clone(),
    }
}

fn compare_hits(left: &ResourceSearchHit, right: &ResourceSearchHit) -> std::cmp::Ordering {
    right
        .ranking
        .text_relevance
        .cmp(&left.ranking.text_relevance)
        .then_with(|| {
            right
                .ranking
                .authored_preference_rank
                .is_some()
                .cmp(&left.ranking.authored_preference_rank.is_some())
        })
        .then_with(|| {
            right
                .ranking
                .authored_preference_rank
                .unwrap_or_default()
                .cmp(&left.ranking.authored_preference_rank.unwrap_or_default())
        })
        .then_with(|| {
            right
                .ranking
                .learned_contextual_frecency_milli
                .cmp(&left.ranking.learned_contextual_frecency_milli)
        })
        .then_with(|| {
            right
                .ranking
                .learned_contextual_fitness_milli
                .unwrap_or_default()
                .cmp(
                    &left
                        .ranking
                        .learned_contextual_fitness_milli
                        .unwrap_or_default(),
                )
        })
        .then_with(|| {
            right
                .ranking
                .learned_frecency_milli
                .cmp(&left.ranking.learned_frecency_milli)
        })
        .then_with(|| left.label.cmp(&right.label))
        .then_with(|| left.resource.cmp(&right.resource))
}

fn quantise_milli(value: f64) -> i64 {
    let scaled = value * 1_000.0;
    if !scaled.is_finite() {
        return 0;
    }
    scaled.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

/// Small deterministic fzf-like subsequence score.
///
/// The algorithm is deliberately O(haystack) and allocation-light after
/// lower-casing. Consecutive and word-boundary matches are rewarded; large gaps
/// and late starts are penalised. It is not semantic search and therefore cannot
/// block on deep providers.
fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    let candidate = candidate.to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    if let Some(position) = candidate.find(query) {
        let prefix = if position == 0 { 2_000 } else { 0 };
        return Some(10_000 + prefix - position as i64 * 5 - candidate.len() as i64);
    }

    let mut query_chars = query.chars();
    let mut wanted = query_chars.next()?;
    let mut score = 0_i64;
    let mut matched = 0_i64;
    let mut first = None;
    let mut last_match = None;
    let mut previous = None;

    for (index, current) in candidate.chars().enumerate() {
        if current == wanted {
            first.get_or_insert(index);
            matched += 1;
            score += 100;
            if last_match == Some(index.saturating_sub(1)) {
                score += 60;
            }
            if index == 0 || previous.is_some_and(|c: char| !c.is_alphanumeric()) {
                score += 40;
            }
            last_match = Some(index);
            match query_chars.next() {
                Some(next) => wanted = next,
                None => {
                    let start_penalty = first.unwrap_or_default() as i64 * 3;
                    let span = index.saturating_sub(first.unwrap_or_default()) as i64;
                    return Some(score + matched * 25 - start_penalty - span);
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

    #[test]
    fn fuzzy_subsequence_prefers_compact_prefixes() {
        let prefix = fuzzy_score("proj", "project").unwrap();
        let scattered = fuzzy_score("proj", "profile object relation job").unwrap();
        assert!(prefix > scattered);
        assert!(fuzzy_score("xyz", "project").is_none());
    }

    #[test]
    fn navigation_index_is_also_a_resource_index_without_loading_providers() {
        use super::super::{ResourceDescriptor, ResourceIndex};

        let mut index = ResourceSearchIndex::default();
        let id = ResourceRef::parse("project:capability:review").unwrap();
        index.insert_resource(
            ResourceRecord::new(ResourceDescriptor::new(
                id.clone(),
                ResourceKind::Capability,
                "Review",
                "review capability",
            )),
            Vec::new(),
        );

        assert_eq!(ResourceIndex::resources(&index).len(), 1);
        assert_eq!(
            ResourceIndex::resource(&index, &id).unwrap().descriptor.id,
            id
        );
    }
}
