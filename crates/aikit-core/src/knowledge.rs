//! Provider-neutral Knowledge Navigation contracts.
//!
//! These types are deliberately **not** a graph store. SemanticWiki, SourcePool,
//! CodeIndex/GitNexus and future providers continue to own their native graph,
//! search and retrieval semantics. AIKit needs only a stable application language
//! for asking for a bounded neighbourhood, recording the route an actor actually
//! traversed, and projecting selected readings into Context.
//!
//! The key distinction is between **provider relations** and **operational
//! traversal**. A [`KnowledgeRoute`] may walk across several provider/lens
//! boundaries, but recording that route never manufactures a WikiEdge, code edge
//! or source relation.

use serde::{Deserialize, Serialize};

use crate::familiarity::{FamiliarityContext, FamiliarityObservation, RouteStepEvidence};
use crate::resource::{ProviderRef, ResourceKind, ResourceRef, SourceAuthority, SourceRef};
use crate::{AikitError, Result};

pub const DEFAULT_RELATION_DEPTH: u8 = 1;
pub const DEFAULT_RELATION_NODE_BUDGET: usize = 96;
pub const DEFAULT_RELATION_EDGE_BUDGET: usize = 192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationDirection {
    Outgoing,
    Incoming,
    Bidirectional,
}

/// Where one relation assertion came from.
///
/// `relation` itself remains provider vocabulary. AIKit does not normalize a
/// GitNexus `CALLS`, Wiki edge type and source citation into one universal edge.
/// `authority` reuses the canonical Resource-source epistemic classes so Explain,
/// History and Knowledge navigation cannot drift into parallel vocabularies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationOrigin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    pub authority: SourceAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

impl RelationOrigin {
    pub fn new(authority: SourceAuthority) -> Self {
        Self {
            provider: None,
            lens: None,
            authority,
            revision: None,
        }
    }

    #[must_use]
    pub fn from_provider(mut self, provider: ProviderRef) -> Self {
        self.provider = Some(provider);
        self
    }

    #[must_use]
    pub fn in_lens(mut self, lens: impl Into<String>) -> Self {
        self.lens = Some(lens.into());
        self
    }

    #[must_use]
    pub fn at_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationNode {
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

impl RelationNode {
    pub fn new(resource: ResourceRef, kind: ResourceKind, label: impl Into<String>) -> Self {
        Self {
            resource,
            kind,
            label: label.into(),
            state: None,
        }
    }

    #[must_use]
    pub fn with_state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationEdge {
    pub from: ResourceRef,
    pub to: ResourceRef,
    /// Provider/native relation name. This string is intentionally not a global
    /// relation enum: meanings must survive federation without being collapsed.
    pub relation: String,
    pub direction: RelationDirection,
    pub origin: RelationOrigin,
}

impl RelationEdge {
    pub fn new(
        from: ResourceRef,
        to: ResourceRef,
        relation: impl Into<String>,
        direction: RelationDirection,
        origin: RelationOrigin,
    ) -> Self {
        Self {
            from,
            to,
            relation: relation.into(),
            direction,
            origin,
        }
    }
}

/// Bounded relation-expansion request shared by list/tree/graph presentations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationQuery {
    pub focus: ResourceRef,
    pub depth: u8,
    pub max_nodes: usize,
    pub max_edges: usize,
    #[serde(default)]
    pub filters: Vec<String>,
}

impl RelationQuery {
    pub fn local(focus: ResourceRef) -> Self {
        Self {
            focus,
            depth: DEFAULT_RELATION_DEPTH,
            max_nodes: DEFAULT_RELATION_NODE_BUDGET,
            max_edges: DEFAULT_RELATION_EDGE_BUDGET,
            filters: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.max_nodes == 0 || self.max_edges == 0 {
            return Err(AikitError::new(
                "knowledge.invalid_relation_budget",
                "relation expansion requires non-zero node and edge budgets",
            ));
        }
        Ok(())
    }
}

/// One bounded, provenance-bearing relation neighbourhood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRelationView {
    pub query: RelationQuery,
    pub nodes: Vec<RelationNode>,
    pub edges: Vec<RelationEdge>,
    pub truncated: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl KnowledgeRelationView {
    pub fn focus_only(query: RelationQuery, focus: RelationNode) -> Result<Self> {
        query.validate()?;
        if query.focus != focus.resource {
            return Err(AikitError::new(
                "knowledge.relation_focus_mismatch",
                "the relation view focus node must match the requested focus",
            ));
        }
        Ok(Self {
            query,
            nodes: vec![focus],
            edges: Vec::new(),
            truncated: false,
            warnings: Vec::new(),
        })
    }

    pub fn push_node(&mut self, node: RelationNode) -> bool {
        if self
            .nodes
            .iter()
            .any(|existing| existing.resource == node.resource)
        {
            return true;
        }
        if self.nodes.len() >= self.query.max_nodes {
            self.truncated = true;
            return false;
        }
        self.nodes.push(node);
        true
    }

    /// Add an edge only when both endpoint identities are already represented.
    /// Providers therefore cannot create invisible vertices by accident.
    pub fn push_edge(&mut self, edge: RelationEdge) -> Result<bool> {
        let known =
            |resource: &ResourceRef| self.nodes.iter().any(|node| &node.resource == resource);
        if !known(&edge.from) || !known(&edge.to) {
            return Err(AikitError::new(
                "knowledge.relation_endpoint_missing",
                "relation edges require both endpoint Resources in the current view",
            )
            .with("from", edge.from.to_string())
            .with("to", edge.to.to_string()));
        }
        if self.edges.len() >= self.query.max_edges {
            self.truncated = true;
            return Ok(false);
        }
        self.edges.push(edge);
        Ok(true)
    }
}

/// One actual step taken by an actor through the federated project field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRouteStep {
    pub resource: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    /// Provider/native relation used to arrive here, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub authority: SourceAuthority,
}

impl KnowledgeRouteStep {
    pub fn new(resource: ResourceRef, authority: SourceAuthority) -> Self {
        Self {
            resource,
            provider: None,
            lens: None,
            transition: None,
            revision: None,
            authority,
        }
    }
}

/// Operational route identity and traversal evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRoute {
    pub route: ResourceRef,
    pub context: FamiliarityContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub steps: Vec<KnowledgeRouteStep>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl KnowledgeRoute {
    pub fn new(route: ResourceRef, context: FamiliarityContext) -> Self {
        Self {
            route,
            context,
            query: None,
            steps: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }

    pub fn destination(&self) -> Option<&ResourceRef> {
        self.steps.last().map(|step| &step.resource)
    }

    /// Convert a completed route to the existing familiarity evidence grammar.
    /// This is the only automatic bridge: operational traversal can teach ease of
    /// access, but it cannot write provider edges or canonical project knowledge.
    pub fn familiarity_observation(
        &self,
        observation_id: impl Into<String>,
        observed_at_ms: u64,
    ) -> Result<FamiliarityObservation> {
        let destination = self.destination().cloned().ok_or_else(|| {
            AikitError::new(
                "knowledge.empty_route",
                "an empty KnowledgeRoute cannot become familiarity evidence",
            )
        })?;
        let steps = self
            .steps
            .iter()
            .map(|step| {
                Ok(RouteStepEvidence {
                    resource: step.resource.clone(),
                    provider: step
                        .provider
                        .as_ref()
                        .map(|provider| ResourceRef::parse(provider.as_str()))
                        .transpose()?,
                    lens: step.lens.clone(),
                    revision: step.revision.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        FamiliarityObservation::route(
            observation_id,
            self.route.clone(),
            destination,
            steps,
            self.context.clone(),
            observed_at_ms,
        )
    }
}

/// Provider-neutral reading selected into a derived context pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeReading {
    pub resource: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
    pub authority: SourceAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default)]
    pub evidence: Vec<SourceRef>,
    pub why_selected: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ContextPackBudget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_limit: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialised_tokens: Option<usize>,
    #[serde(default)]
    pub truncated: bool,
}

/// Derived retrieval/context result. It is not a canonical ContextSource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeContextPack {
    pub context: FamiliarityContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default)]
    pub selected: Vec<ResourceRef>,
    #[serde(default)]
    pub routes: Vec<KnowledgeRoute>,
    #[serde(default)]
    pub readings: Vec<KnowledgeReading>,
    #[serde(default)]
    pub absences: Vec<String>,
    #[serde(default)]
    pub explanations: Vec<String>,
    /// Conflicts observed in the materialised retrieval result. This never authors
    /// a provider relation or resolves the contradiction on the provider's behalf.
    #[serde(default)]
    pub contradictions: Vec<String>,
    /// Questions left open by explicit provider/read absences.
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub budget: ContextPackBudget,
}

impl KnowledgeContextPack {
    pub fn new(context: FamiliarityContext) -> Self {
        Self {
            context,
            query: None,
            selected: Vec::new(),
            routes: Vec::new(),
            readings: Vec::new(),
            absences: Vec::new(),
            explanations: Vec::new(),
            contradictions: Vec::new(),
            open_questions: Vec::new(),
            budget: ContextPackBudget::default(),
        }
    }

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
mod tests {
    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    #[test]
    fn relation_view_refuses_edges_to_invisible_nodes_and_respects_budget() {
        let query = RelationQuery {
            focus: r("knowledge-node/auth"),
            depth: 1,
            max_nodes: 2,
            max_edges: 1,
            filters: Vec::new(),
        };
        let mut view = KnowledgeRelationView::focus_only(
            query,
            RelationNode::new(
                r("knowledge-node/auth"),
                ResourceKind::KnowledgeNode,
                "Auth",
            ),
        )
        .unwrap();
        assert!(view.push_node(RelationNode::new(
            r("knowledge-source/spec"),
            ResourceKind::KnowledgeSource,
            "Spec",
        )));
        assert!(!view.push_node(RelationNode::new(
            r("knowledge-source/extra"),
            ResourceKind::KnowledgeSource,
            "Extra",
        )));
        assert!(view.truncated);

        view.push_edge(RelationEdge::new(
            r("knowledge-node/auth"),
            r("knowledge-source/spec"),
            "cites",
            RelationDirection::Outgoing,
            RelationOrigin::new(SourceAuthority::Authored).in_lens("semantic-wiki"),
        ))
        .unwrap();
        assert!(view
            .push_edge(RelationEdge::new(
                r("knowledge-node/auth"),
                r("knowledge-source/missing"),
                "cites",
                RelationDirection::Outgoing,
                RelationOrigin::new(SourceAuthority::Authored),
            ))
            .is_err());
    }

    #[test]
    fn knowledge_route_becomes_familiarity_without_becoming_a_provider_edge() {
        let mut route = KnowledgeRoute::new(
            r("knowledge-route/auth-to-code"),
            FamiliarityContext {
                project: Some(r("project/app")),
                actor: None,
                agency: None,
                focus: Some("auth".into()),
            },
        );
        route.steps.push(KnowledgeRouteStep {
            resource: r("knowledge-node/auth"),
            provider: Some(ProviderRef::parse("provider/wiki").unwrap()),
            lens: Some("semantic-wiki".into()),
            transition: None,
            revision: Some("wiki-r3".into()),
            authority: SourceAuthority::Authored,
        });
        route.steps.push(KnowledgeRouteStep {
            resource: r("code-reference/session-auth"),
            provider: Some(ProviderRef::parse("provider/gitnexus").unwrap()),
            lens: Some("code-index".into()),
            transition: Some("project-map-binding".into()),
            revision: Some("git-abc".into()),
            authority: SourceAuthority::Derived,
        });

        let observation = route.familiarity_observation("event/1", 42).unwrap();
        assert_eq!(observation.destination, r("code-reference/session-auth"));
        assert_eq!(observation.observed_at_ms, 42);
        match observation.use_kind {
            crate::FamiliarityUse::Route { route, steps } => {
                assert_eq!(route, r("knowledge-route/auth-to-code"));
                assert_eq!(steps.len(), 2);
                assert_eq!(steps[0].lens.as_deref(), Some("semantic-wiki"));
                assert_eq!(
                    steps[1].provider.as_ref().unwrap().as_str(),
                    "provider/gitnexus"
                );
            }
            crate::FamiliarityUse::ResolvePath { .. } => {
                panic!("KnowledgeRoute familiarity must remain route evidence")
            }
            crate::FamiliarityUse::Destination => panic!("route evidence must remain a route"),
        }
    }
}
