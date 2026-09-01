//! UI-neutral Explain/History evidence read models.
//!
//! This module deliberately introduces no persistence authority. It classifies and
//! projects evidence already owned by Resource, familiarity, KnowledgeRoute and
//! HarnessComposition contracts so CLI/TUI/native consumers do not flatten those
//! distinctions into presentation strings or invent a second History store.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::composition_view::HarnessCompositionDiff;
use crate::familiarity::{FamiliarityObservation, FamiliarityUse};
use crate::resource::{ResourceExplanation, ResourceRef, SourceAuthority};

pub const EXPLAIN_HISTORY_VERSION: &str = "aikit.explain-history/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HistoryKind {
    Recent,
    Familiarity,
    ResolvePath,
    KnowledgeRoute,
    KnowledgeFrame,
    Generation,
    HarnessComposition,
    SessionSpace,
    Procedure,
    LiveActivation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HistoryRecoverability {
    /// Evidence can be inspected/compared but has no generic restore operation.
    InspectOnly,
    /// The evidence owner exposes an explicit current-authority restaging path.
    RestageThroughCurrentAuthority,
    /// The historical route can be revisited as navigation without becoming truth.
    ReplayNavigation,
    /// Provider/native detail is historical evidence only and cannot be restored.
    NotRecoverable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EvidenceProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Provider/native identity is evidence only. It is never inserted into
    /// `canonical_refs` by this read model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEvidence {
    pub schema: String,
    pub id: String,
    pub kind: HistoryKind,
    pub subject: ResourceRef,
    /// More than one epistemic class may truthfully apply. For example a
    /// SessionSpace receipt is generated evidence of an authored semantic change.
    pub authorities: Vec<SourceAuthority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at_unix_ms: Option<u128>,
    pub summary: String,
    #[serde(default)]
    pub canonical_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<EvidenceProvenance>,
    pub recoverability: HistoryRecoverability,
    #[serde(default)]
    pub details: BTreeMap<String, String>,
}

impl HistoryEvidence {
    pub fn matches(&self, resource: &ResourceRef) -> bool {
        &self.subject == resource
            || self
                .canonical_refs
                .iter()
                .any(|candidate| candidate == resource)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HistoryReadModel {
    pub schema: String,
    pub entries: Vec<HistoryEvidence>,
}

impl HistoryReadModel {
    pub fn new(entries: Vec<HistoryEvidence>) -> Self {
        Self {
            schema: EXPLAIN_HISTORY_VERSION.into(),
            entries,
        }
    }

    pub fn for_resource(&self, resource: &ResourceRef) -> Self {
        Self::new(
            self.entries
                .iter()
                .filter(|entry| entry.matches(resource))
                .cloned()
                .collect(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainFact {
    pub relation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<SourceAuthority>,
    pub summary: String,
    #[serde(default)]
    pub canonical_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<EvidenceProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainEvidence {
    pub schema: String,
    pub subject: ResourceRef,
    pub facts: Vec<ExplainFact>,
}

impl ExplainEvidence {
    pub fn push(&mut self, fact: ExplainFact) {
        self.facts.push(fact);
    }
}

/// Preserve the epistemic classes already present on a canonical Resource record.
/// Missing source authority remains missing rather than being guessed by Explain.
pub fn explain_resource_evidence(explanation: &ResourceExplanation) -> ExplainEvidence {
    let mut facts = Vec::new();

    if let Some(owner) = &explanation.owner {
        facts.push(ExplainFact {
            relation: "owner".into(),
            authority: Some(SourceAuthority::Authored),
            summary: format!("owned by {owner}"),
            canonical_refs: Vec::new(),
            provenance: Vec::new(),
        });
    }

    for source in &explanation.sources {
        facts.push(ExplainFact {
            relation: "source".into(),
            authority: source.authority,
            summary: format!("source {} is {:?}", source.source, source.state),
            canonical_refs: ResourceRef::parse(source.source.as_str())
                .into_iter()
                .collect(),
            provenance: vec![EvidenceProvenance {
                source: ResourceRef::parse(source.source.as_str()).ok(),
                revision: source.revision.as_ref().map(ToString::to_string),
                ..EvidenceProvenance::default()
            }],
        });
    }

    for provider in &explanation.providers {
        let provider_ref = ResourceRef::parse(provider.provider.as_str()).ok();
        facts.push(ExplainFact {
            relation: "provider".into(),
            authority: Some(SourceAuthority::Observed),
            summary: format!("provider {} is {:?}", provider.provider, provider.state),
            canonical_refs: provider_ref.clone().into_iter().collect(),
            provenance: vec![EvidenceProvenance {
                provider: provider_ref,
                ..EvidenceProvenance::default()
            }],
        });
    }

    facts.push(ExplainFact {
        relation: "eligibility".into(),
        authority: Some(SourceAuthority::Derived),
        summary: format!("effective eligibility is {:?}", explanation.eligibility),
        canonical_refs: Vec::new(),
        provenance: Vec::new(),
    });

    if let Some(preference) = &explanation.preference {
        facts.push(ExplainFact {
            relation: "authored-preference".into(),
            authority: Some(SourceAuthority::Authored),
            summary: format!(
                "authored rank {} from {}",
                preference.rank, preference.source
            ),
            canonical_refs: ResourceRef::parse(preference.source.as_str())
                .into_iter()
                .collect(),
            provenance: vec![EvidenceProvenance {
                source: ResourceRef::parse(preference.source.as_str()).ok(),
                ..EvidenceProvenance::default()
            }],
        });
    }

    ExplainEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        subject: explanation.id.clone(),
        facts,
    }
}

/// Learned navigation evidence remains learned evidence. A KnowledgeRoute is an
/// operational traversal record; its provider/lens/revision trail is preserved but
/// the route is never promoted to a provider relation or semantic edge.
pub fn familiarity_history_evidence(observation: &FamiliarityObservation) -> HistoryEvidence {
    let mut canonical_refs = BTreeSet::new();
    canonical_refs.insert(observation.destination.clone());
    if let Some(surface) = &observation.source_surface {
        canonical_refs.insert(surface.clone());
    }
    if let Some(action) = &observation.source_action {
        canonical_refs.insert(action.clone());
    }

    let mut provenance = Vec::new();
    let (kind, summary, recoverability) = match &observation.use_kind {
        FamiliarityUse::Destination => (
            HistoryKind::Familiarity,
            format!("used destination {}", observation.destination),
            HistoryRecoverability::ReplayNavigation,
        ),
        FamiliarityUse::Route { route, steps } => {
            canonical_refs.insert(route.clone());
            for step in steps {
                canonical_refs.insert(step.resource.clone());
                if let Some(provider) = &step.provider {
                    provenance.push(EvidenceProvenance {
                        provider: Some(provider.clone()),
                        lens: step.lens.clone(),
                        revision: step.revision.clone(),
                        ..EvidenceProvenance::default()
                    });
                } else if step.lens.is_some() || step.revision.is_some() {
                    provenance.push(EvidenceProvenance {
                        lens: step.lens.clone(),
                        revision: step.revision.clone(),
                        ..EvidenceProvenance::default()
                    });
                }
            }
            (
                HistoryKind::KnowledgeRoute,
                format!(
                    "traversed route {route} to {} through {} step{}",
                    observation.destination,
                    steps.len(),
                    if steps.len() == 1 { "" } else { "s" }
                ),
                HistoryRecoverability::ReplayNavigation,
            )
        }
        FamiliarityUse::ResolvePath {
            knowledge_route,
            steps,
            operative,
        } => {
            if let Some(route) = knowledge_route {
                canonical_refs.insert(route.clone());
            }
            for reference in [
                &operative.method,
                &operative.action,
                &operative.surface,
                &operative.activity,
                &operative.return_ref,
            ]
            .into_iter()
            .flatten()
            {
                canonical_refs.insert(reference.clone());
            }
            for step in steps {
                canonical_refs.insert(step.resource.clone());
                if let Some(provider) = &step.provider {
                    provenance.push(EvidenceProvenance {
                        provider: Some(provider.clone()),
                        lens: step.lens.clone(),
                        revision: step.revision.clone(),
                        ..EvidenceProvenance::default()
                    });
                } else if step.lens.is_some() || step.revision.is_some() {
                    provenance.push(EvidenceProvenance {
                        lens: step.lens.clone(),
                        revision: step.revision.clone(),
                        ..EvidenceProvenance::default()
                    });
                }
            }
            let route = knowledge_route
                .as_ref()
                .map(|route| format!(" via {route}"))
                .unwrap_or_default();
            (
                HistoryKind::ResolvePath,
                format!(
                    "resolved operative path {}{route} to {} through {} step{}",
                    operative.path_identity,
                    observation.destination,
                    steps.len(),
                    if steps.len() == 1 { "" } else { "s" }
                ),
                HistoryRecoverability::ReplayNavigation,
            )
        }
    };

    let mut details = BTreeMap::new();
    details.insert(
        "observedAtMs".into(),
        observation.observed_at_ms.to_string(),
    );
    if let Some(fitness) = &observation.fitness {
        details.insert("fitnessMilli".into(), fitness.score_milli.to_string());
        details.insert("fitnessProvenance".into(), fitness.provenance.clone());
    }
    if let FamiliarityUse::ResolvePath {
        knowledge_route,
        operative,
        ..
    } = &observation.use_kind
    {
        details.insert("pathIdentity".into(), operative.path_identity.clone());
        details.insert("expression".into(), operative.expression.render());
        details.insert(
            "relationOps".into(),
            operative
                .relation_ops
                .iter()
                .map(|op| op.symbol())
                .collect::<Vec<_>>()
                .join(" "),
        );
        details.insert(
            "horizons".into(),
            operative
                .horizons
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" "),
        );
        for (key, reference) in [
            ("knowledgeRoute", knowledge_route.as_ref()),
            ("method", operative.method.as_ref()),
            ("action", operative.action.as_ref()),
            ("surface", operative.surface.as_ref()),
            ("activity", operative.activity.as_ref()),
            ("return", operative.return_ref.as_ref()),
        ] {
            if let Some(reference) = reference {
                details.insert(key.into(), reference.to_string());
            }
        }
    }

    HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: observation.observation_id.clone(),
        kind,
        subject: observation.destination.clone(),
        authorities: vec![SourceAuthority::Learned],
        occurred_at_unix_ms: Some(observation.observed_at_ms as u128),
        summary,
        canonical_refs: canonical_refs.into_iter().collect(),
        provenance,
        recoverability,
        details,
    }
}

/// Harness body history is derived from resolver-owned compositions. Component,
/// Contract, contribution and Surface identities stay canonical; target-native
/// provider identifiers remain in the resolver's explanation and are never used as
/// replacement identity here.
pub fn harness_composition_history_evidence(
    subject: ResourceRef,
    diff: &HarnessCompositionDiff,
) -> HistoryEvidence {
    let mut canonical_refs = BTreeSet::new();
    canonical_refs.insert(subject.clone());
    canonical_refs.extend(diff.mounted_components.iter().cloned());
    canonical_refs.extend(diff.retracted_components.iter().cloned());
    canonical_refs.extend(diff.added_contributions.iter().cloned());
    canonical_refs.extend(diff.removed_contributions.iter().cloned());
    canonical_refs.extend(diff.added_surfaces.iter().cloned());
    canonical_refs.extend(diff.removed_surfaces.iter().cloned());
    for binding in &diff.rebound_contracts {
        canonical_refs.insert(binding.consumer_component.clone());
        canonical_refs.insert(binding.contract.clone());
        canonical_refs.insert(binding.before_provider.clone());
        canonical_refs.insert(binding.after_provider.clone());
    }

    let mut details = BTreeMap::new();
    details.insert("beforeFingerprint".into(), diff.before_fingerprint.clone());
    details.insert("afterFingerprint".into(), diff.after_fingerprint.clone());
    details.insert("mounted".into(), diff.mounted_components.len().to_string());
    details.insert(
        "retracted".into(),
        diff.retracted_components.len().to_string(),
    );
    details.insert(
        "reboundContracts".into(),
        diff.rebound_contracts.len().to_string(),
    );

    HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: format!(
            "harness-composition:{}:{}",
            diff.before_fingerprint, diff.after_fingerprint
        ),
        kind: HistoryKind::HarnessComposition,
        subject,
        authorities: vec![SourceAuthority::Derived],
        occurred_at_unix_ms: None,
        summary: format!(
            "runtime body {} -> {}: +{} component{}, -{} component{}, {} provider rebind{}",
            diff.before_fingerprint,
            diff.after_fingerprint,
            diff.mounted_components.len(),
            if diff.mounted_components.len() == 1 {
                ""
            } else {
                "s"
            },
            diff.retracted_components.len(),
            if diff.retracted_components.len() == 1 {
                ""
            } else {
                "s"
            },
            diff.rebound_contracts.len(),
            if diff.rebound_contracts.len() == 1 {
                ""
            } else {
                "s"
            },
        ),
        canonical_refs: canonical_refs.into_iter().collect(),
        provenance: Vec::new(),
        recoverability: HistoryRecoverability::InspectOnly,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::familiarity::{FamiliarityContext, RouteStepEvidence};

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    #[test]
    fn knowledge_route_history_is_learned_navigation_not_relation_truth() {
        let observation = FamiliarityObservation::route(
            "obs-7",
            r("knowledge-route/spec-to-code"),
            r("code-reference/handler"),
            vec![
                RouteStepEvidence {
                    resource: r("knowledge-node/spec"),
                    provider: Some(r("provider/wiki")),
                    lens: Some("semantic-wiki".into()),
                    revision: Some("wiki-r4".into()),
                },
                RouteStepEvidence {
                    resource: r("code-reference/handler"),
                    provider: Some(r("provider/gitnexus")),
                    lens: Some("code-index".into()),
                    revision: Some("git-abc".into()),
                },
            ],
            FamiliarityContext::default(),
            42,
        )
        .unwrap();

        let history = familiarity_history_evidence(&observation);
        assert_eq!(history.kind, HistoryKind::KnowledgeRoute);
        assert_eq!(history.authorities, vec![SourceAuthority::Learned]);
        assert_eq!(
            history.recoverability,
            HistoryRecoverability::ReplayNavigation
        );
        assert!(history
            .canonical_refs
            .contains(&r("knowledge-route/spec-to-code")));
        assert!(history.canonical_refs.contains(&r("knowledge-node/spec")));
        assert!(history
            .canonical_refs
            .contains(&r("code-reference/handler")));
        assert_eq!(history.provenance.len(), 2);
    }

    #[test]
    fn history_filter_navigates_by_canonical_ref_not_provider_native_id() {
        let observation = FamiliarityObservation::destination(
            "obs-8",
            r("project/app"),
            FamiliarityContext::default(),
            12,
        )
        .from_surface(r("surface/aikit/tui"));
        let model = HistoryReadModel::new(vec![familiarity_history_evidence(&observation)]);
        assert_eq!(model.for_resource(&r("project/app")).entries.len(), 1);
        assert_eq!(model.for_resource(&r("project/other")).entries.len(), 0);
    }
}
