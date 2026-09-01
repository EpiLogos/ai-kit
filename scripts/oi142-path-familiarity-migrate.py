#!/usr/bin/env python3
from pathlib import Path


def patch(path: str, old: str, new: str, count: int = 1) -> None:
    target = Path(path)
    source = target.read_text()
    if source.count(old) < count:
        raise SystemExit(f"missing patch anchor in {path}: {old[:120]!r}")
    target.write_text(source.replace(old, new, count))


# --- Familiarity: ResolvePath is its own learned identity, not a synthetic route. ---
fam = "crates/aikit-core/src/familiarity.rs"
patch(
    fam,
    '''    ResolvePath {\n        route: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n        operative: OperativePathEvidence,\n    },\n''',
    '''    ResolvePath {\n        /// Present only when an actual canonical KnowledgeRoute participated in\n        /// the traversal. A general ResolvePath never manufactures route identity.\n        #[serde(default, skip_serializing_if = "Option::is_none")]\n        knowledge_route: Option<ResourceRef>,\n        steps: Vec<RouteStepEvidence>,\n        operative: OperativePathEvidence,\n    },\n''',
)
patch(
    fam,
    '''    pub fn resolve_path(\n        observation_id: impl Into<String>,\n        route: ResourceRef,\n        destination: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n        operative: OperativePathEvidence,\n        context: FamiliarityContext,\n        observed_at_ms: u64,\n    ) -> Result<Self> {\n        if steps.is_empty() {\n''',
    '''    pub fn resolve_path(\n        observation_id: impl Into<String>,\n        knowledge_route: Option<ResourceRef>,\n        destination: ResourceRef,\n        steps: Vec<RouteStepEvidence>,\n        operative: OperativePathEvidence,\n        context: FamiliarityContext,\n        observed_at_ms: u64,\n    ) -> Result<Self> {\n        if operative.path_identity.trim().is_empty() {\n            return Err(AikitError::new(\n                "familiarity.missing_resolve_path_identity",\n                "a ResolvePath familiarity observation needs the stable path identity",\n            ));\n        }\n        if steps.is_empty() {\n''',
)
patch(
    fam,
    '''            use_kind: FamiliarityUse::ResolvePath {\n                route,\n                steps,\n                operative,\n            },\n''',
    '''            use_kind: FamiliarityUse::ResolvePath {\n                knowledge_route,\n                steps,\n                operative,\n            },\n''',
)
patch(
    fam,
    '''pub struct AccessibilityAssessment {\n    pub destination: ResourceRef,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub route: Option<ResourceRef>,\n    pub observations: usize,\n''',
    '''pub struct AccessibilityAssessment {\n    pub destination: ResourceRef,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub route: Option<ResourceRef>,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub path_identity: Option<String>,\n    pub observations: usize,\n''',
)
patch(
    fam,
    '''pub enum ForgetScope {\n    Destination(ResourceRef),\n    Route(ResourceRef),\n    Project(ResourceRef),\n    All,\n}\n''',
    '''pub enum ForgetScope {\n    Destination(ResourceRef),\n    Route(ResourceRef),\n    ResolvePath(String),\n    Project(ResourceRef),\n    All,\n}\n\n#[derive(Debug, Clone, Copy)]\nenum FamiliaritySelector<'a> {\n    Destination,\n    Route(&'a ResourceRef),\n    ResolvePath(&'a str),\n}\n''',
)
patch(
    fam,
    '''    ) -> AccessibilityAssessment {\n        self.assess(destination, None, context, now_ms, half_life_ms)\n    }\n\n    pub fn assess_route(\n''',
    '''    ) -> AccessibilityAssessment {\n        self.assess(\n            destination,\n            FamiliaritySelector::Destination,\n            context,\n            now_ms,\n            half_life_ms,\n        )\n    }\n\n    pub fn assess_route(\n''',
)
patch(
    fam,
    '''    ) -> AccessibilityAssessment {\n        self.assess(destination, Some(route), context, now_ms, half_life_ms)\n    }\n\n    fn assess(\n        &self,\n        destination: &ResourceRef,\n        route: Option<&ResourceRef>,\n        context: &FamiliarityContext,\n        now_ms: u64,\n        half_life_ms: u64,\n    ) -> AccessibilityAssessment {\n''',
    '''    ) -> AccessibilityAssessment {\n        self.assess(\n            destination,\n            FamiliaritySelector::Route(route),\n            context,\n            now_ms,\n            half_life_ms,\n        )\n    }\n\n    pub fn assess_resolve_path(\n        &self,\n        path_identity: &str,\n        destination: &ResourceRef,\n        context: &FamiliarityContext,\n        now_ms: u64,\n        half_life_ms: u64,\n    ) -> AccessibilityAssessment {\n        self.assess(\n            destination,\n            FamiliaritySelector::ResolvePath(path_identity),\n            context,\n            now_ms,\n            half_life_ms,\n        )\n    }\n\n    fn assess(\n        &self,\n        destination: &ResourceRef,\n        selector: FamiliaritySelector<'_>,\n        context: &FamiliarityContext,\n        now_ms: u64,\n        half_life_ms: u64,\n    ) -> AccessibilityAssessment {\n''',
)
patch(
    fam,
    '''            let route_matches = match (route, &observation.use_kind) {\n                (None, FamiliarityUse::Destination) => true,\n                (Some(wanted), FamiliarityUse::Route { route, .. })\n                | (Some(wanted), FamiliarityUse::ResolvePath { route, .. }) => route == wanted,\n                _ => false,\n            };\n            if route_matches {\n''',
    '''            let identity_matches = match (selector, &observation.use_kind) {\n                (FamiliaritySelector::Destination, FamiliarityUse::Destination) => true,\n                (FamiliaritySelector::Route(wanted), FamiliarityUse::Route { route, .. }) => {\n                    route == wanted\n                }\n                (\n                    FamiliaritySelector::ResolvePath(wanted),\n                    FamiliarityUse::ResolvePath { operative, .. },\n                ) => operative.path_identity == wanted,\n                _ => false,\n            };\n            if identity_matches {\n''',
)
patch(
    fam,
    '''        AccessibilityAssessment {\n            destination: destination.clone(),\n            route: route.cloned(),\n            observations: matching.len(),\n''',
    '''        AccessibilityAssessment {\n            destination: destination.clone(),\n            route: match selector {\n                FamiliaritySelector::Route(route) => Some(route.clone()),\n                _ => None,\n            },\n            path_identity: match selector {\n                FamiliaritySelector::ResolvePath(identity) => Some(identity.to_string()),\n                _ => None,\n            },\n            observations: matching.len(),\n''',
)
patch(
    fam,
    '''            ForgetScope::Route(route) => match &observation.use_kind {\n                FamiliarityUse::Route {\n                    route: observed, ..\n                }\n                | FamiliarityUse::ResolvePath {\n                    route: observed, ..\n                } => observed != route,\n                _ => true,\n            },\n            ForgetScope::Project(project) => observation.context.project.as_ref() != Some(project),\n''',
    '''            ForgetScope::Route(route) => !matches!(\n                &observation.use_kind,\n                FamiliarityUse::Route { route: observed, .. } if observed == route\n            ),\n            ForgetScope::ResolvePath(path_identity) => !matches!(\n                &observation.use_kind,\n                FamiliarityUse::ResolvePath { operative, .. }\n                    if &operative.path_identity == path_identity\n            ),\n            ForgetScope::Project(project) => observation.context.project.as_ref() != Some(project),\n''',
)
patch(
    fam,
    '''            .filter_map(|observation| match &observation.use_kind {\n                FamiliarityUse::Route {\n                    route: observed,\n                    steps,\n                }\n                | FamiliarityUse::ResolvePath {\n                    route: observed,\n                    steps,\n                    ..\n                } if observed == route => Some(\n                    steps\n                        .iter()\n                        .map(|step| step.resource.clone())\n                        .collect::<Vec<_>>(),\n                ),\n                _ => None,\n            })\n            .collect()\n    }\n}\n''',
    '''            .filter_map(|observation| match &observation.use_kind {\n                FamiliarityUse::Route {\n                    route: observed,\n                    steps,\n                } if observed == route => Some(\n                    steps\n                        .iter()\n                        .map(|step| step.resource.clone())\n                        .collect::<Vec<_>>(),\n                ),\n                _ => None,\n            })\n            .collect()\n    }\n\n    pub fn resolve_path_steps(&self, path_identity: &str) -> BTreeSet<Vec<ResourceRef>> {\n        self.observations\n            .values()\n            .filter_map(|observation| match &observation.use_kind {\n                FamiliarityUse::ResolvePath {\n                    steps, operative, ..\n                } if operative.path_identity == path_identity => Some(\n                    steps\n                        .iter()\n                        .map(|step| step.resource.clone())\n                        .collect::<Vec<_>>(),\n                ),\n                _ => None,\n            })\n            .collect()\n    }\n}\n''',
)

# Export the operative evidence type as part of the accepted familiarity API.
lib = "crates/aikit-core/src/lib.rs"
patch(
    lib,
    '''    FamiliarityUse, FitnessEvidence, ForgetScope, RouteStepEvidence,\n''',
    '''    FamiliarityUse, FitnessEvidence, ForgetScope, OperativePathEvidence, RouteStepEvidence,\n''',
)

# --- Resource ranking: path-specific learned ease remains separate from destination ease. ---
index = "crates/aikit-core/src/resource/index.rs"
patch(
    index,
    '''    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub authored_preference_rank: Option<i32>,\n    #[serde(default)]\n    pub learned_observations: usize,\n''',
    '''    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub authored_preference_rank: Option<i32>,\n    #[serde(default)]\n    pub learned_path_observations: usize,\n    #[serde(default)]\n    pub learned_path_contextual_observations: usize,\n    #[serde(default)]\n    pub learned_path_frecency_milli: i64,\n    #[serde(default)]\n    pub learned_path_contextual_frecency_milli: i64,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub learned_path_contextual_fitness_milli: Option<i32>,\n    #[serde(default)]\n    pub learned_observations: usize,\n''',
)
patch(
    index,
    '''    fn resolve_ranking(&self, _id: &ResourceRef) -> ResolveRankingSignals {\n        ResolveRankingSignals::default()\n    }\n}\n''',
    '''    fn resolve_ranking(&self, _id: &ResourceRef) -> ResolveRankingSignals {\n        ResolveRankingSignals::default()\n    }\n\n    fn resolve_path_ranking(\n        &self,\n        _path_identity: &str,\n        id: &ResourceRef,\n    ) -> ResolveRankingSignals {\n        self.resolve_ranking(id)\n    }\n}\n''',
)

search = "crates/aikit-core/src/resource/search.rs"
patch(
    search,
    '''struct IndexedResource {\n    record: ResourceRecord,\n    evidence: Vec<NavigationEvidence>,\n    familiarity: Option<AccessibilityAssessment>,\n}\n''',
    '''struct IndexedResource {\n    record: ResourceRecord,\n    evidence: Vec<NavigationEvidence>,\n    familiarity: Option<AccessibilityAssessment>,\n    resolve_path_familiarity: BTreeMap<String, AccessibilityAssessment>,\n}\n''',
)
patch(
    search,
    '''                    record,\n                    evidence,\n                    familiarity: None,\n                },\n''',
    '''                    record,\n                    evidence,\n                    familiarity: None,\n                    resolve_path_familiarity: BTreeMap::new(),\n                },\n''',
)
patch(
    search,
    '''    pub fn insert_action(&mut self, action: ContextualActionDescriptor) -> Result<()> {\n''',
    '''    /// Apply learned accessibility for one exact Resolve expression identity.\n    /// This evidence is deliberately not added to generic zero-query navigation:\n    /// it is only meaningful while resolving the same praxis language again.\n    pub fn apply_resolve_path_familiarity(\n        &mut self,\n        familiarity: &FamiliarityStore,\n        path_identity: &str,\n        context: &FamiliarityContext,\n        now_ms: u64,\n        half_life_ms: u64,\n    ) {\n        for indexed in self.resources.values_mut() {\n            let assessment = familiarity.assess_resolve_path(\n                path_identity,\n                &indexed.record.descriptor.id,\n                context,\n                now_ms,\n                half_life_ms,\n            );\n            if assessment.is_empty() {\n                indexed.resolve_path_familiarity.remove(path_identity);\n            } else {\n                indexed\n                    .resolve_path_familiarity\n                    .insert(path_identity.to_string(), assessment);\n            }\n        }\n    }\n\n    pub fn insert_action(&mut self, action: ContextualActionDescriptor) -> Result<()> {\n''',
)
patch(
    search,
    '''    fn resolve_ranking(&self, id: &ResourceRef) -> ResolveRankingSignals {\n        let Some(indexed) = self.resources.get(id) else {\n            return ResolveRankingSignals::default();\n        };\n        let familiarity = indexed.familiarity.as_ref();\n        ResolveRankingSignals {\n            authored_preference_rank: indexed.record.preference.as_ref().map(|value| value.rank),\n            learned_observations: familiarity.map_or(0, |value| value.observations),\n            learned_contextual_observations: familiarity\n                .map_or(0, |value| value.contextual_observations),\n            learned_frecency_milli: familiarity.map_or(0, |value| quantise_milli(value.frecency)),\n            learned_contextual_frecency_milli: familiarity\n                .map_or(0, |value| quantise_milli(value.contextual_frecency)),\n            learned_contextual_fitness_milli: familiarity\n                .and_then(|value| value.contextual_fitness_milli)\n                .map(|value| value.round() as i32),\n        }\n    }\n}\n''',
    '''    fn resolve_ranking(&self, id: &ResourceRef) -> ResolveRankingSignals {\n        let Some(indexed) = self.resources.get(id) else {\n            return ResolveRankingSignals::default();\n        };\n        resolve_ranking_signals(indexed, None)\n    }\n\n    fn resolve_path_ranking(\n        &self,\n        path_identity: &str,\n        id: &ResourceRef,\n    ) -> ResolveRankingSignals {\n        let Some(indexed) = self.resources.get(id) else {\n            return ResolveRankingSignals::default();\n        };\n        resolve_ranking_signals(indexed, Some(path_identity))\n    }\n}\n\nfn resolve_ranking_signals(\n    indexed: &IndexedResource,\n    path_identity: Option<&str>,\n) -> ResolveRankingSignals {\n    let familiarity = indexed.familiarity.as_ref();\n    let path = path_identity.and_then(|identity| indexed.resolve_path_familiarity.get(identity));\n    ResolveRankingSignals {\n        authored_preference_rank: indexed.record.preference.as_ref().map(|value| value.rank),\n        learned_path_observations: path.map_or(0, |value| value.observations),\n        learned_path_contextual_observations: path\n            .map_or(0, |value| value.contextual_observations),\n        learned_path_frecency_milli: path.map_or(0, |value| quantise_milli(value.frecency)),\n        learned_path_contextual_frecency_milli: path\n            .map_or(0, |value| quantise_milli(value.contextual_frecency)),\n        learned_path_contextual_fitness_milli: path\n            .and_then(|value| value.contextual_fitness_milli)\n            .map(|value| value.round() as i32),\n        learned_observations: familiarity.map_or(0, |value| value.observations),\n        learned_contextual_observations: familiarity\n            .map_or(0, |value| value.contextual_observations),\n        learned_frecency_milli: familiarity.map_or(0, |value| quantise_milli(value.frecency)),\n        learned_contextual_frecency_milli: familiarity\n            .map_or(0, |value| quantise_milli(value.contextual_frecency)),\n        learned_contextual_fitness_milli: familiarity\n            .and_then(|value| value.contextual_fitness_milli)\n            .map(|value| value.round() as i32),\n    }\n}\n''',
)
# Acceptance at the ranking boundary: same relevance/authorship, learned exact praxis wins.
patch(
    search,
    '''    fn navigation_index_is_also_a_resource_index_without_loading_providers() {\n''',
    '''    fn learned_resolve_path_breaks_an_otherwise_equal_resolution_tie() {\n        use crate::familiarity::{\n            FamiliarityContext, FamiliarityObservation, FamiliarityStore, OperativePathEvidence,\n            RouteStepEvidence,\n        };\n        use super::super::{\n            parse_resolve_expression, resolve_expression, resolve_path_identity, AddressHorizon,\n            RelationOp, ResourceDescriptor,\n        };\n\n        let mut index = ResourceSearchIndex::default();\n        for id in ["action/alpha", "action/beta"] {\n            index.insert_resource(\n                ResourceRecord::new(ResourceDescriptor::new(\n                    ResourceRef::parse(id).unwrap(),\n                    ResourceKind::Action,\n                    "Verify",\n                    "verify current state",\n                )),\n                Vec::new(),\n            );\n        }\n        let expression = parse_resolve_expression("+ @5 verify").unwrap();\n        let path_identity = resolve_path_identity(&expression);\n        let beta = ResourceRef::parse("action/beta").unwrap();\n        let mut familiarity = FamiliarityStore::new();\n        familiarity\n            .record(\n                FamiliarityObservation::resolve_path(\n                    "evt/path/beta",\n                    None,\n                    beta.clone(),\n                    vec![RouteStepEvidence {\n                        resource: beta.clone(),\n                        provider: None,\n                        lens: None,\n                        revision: None,\n                    }],\n                    OperativePathEvidence {\n                        path_identity: path_identity.clone(),\n                        expression: expression.clone(),\n                        relation_ops: vec![RelationOp::Affirm],\n                        horizons: vec![AddressHorizon::H5],\n                        method: None,\n                        action: Some(beta.clone()),\n                        surface: None,\n                        activity: None,\n                        return_ref: None,\n                    },\n                    FamiliarityContext::default(),\n                    1_000,\n                )\n                .unwrap(),\n            )\n            .unwrap();\n        index.apply_resolve_path_familiarity(\n            &familiarity,\n            &path_identity,\n            &FamiliarityContext::default(),\n            1_001,\n            10_000,\n        );\n\n        let path = resolve_expression(&expression, &index, 16);\n        assert_eq!(path.candidates[0].resource, beta);\n        assert_eq!(\n            path.candidates[0].ranking.learned_path_contextual_observations,\n            1\n        );\n    }\n\n    #[test]\n    fn navigation_index_is_also_a_resource_index_without_loading_providers() {\n''',
)

# --- Resolve: expose stable identity and consult path-specific ordering evidence. ---
op = "crates/aikit-core/src/resource/operative.rs"
patch(
    op,
    '''    let mut steps = Vec::new();\n    let mut candidates = evaluate(expression, resources, &mut steps);\n''',
    '''    let identity = resolve_path_identity(expression);\n    let mut steps = Vec::new();\n    let mut candidates = evaluate(expression, resources, &identity, &mut steps);\n''',
)
patch(
    op,
    '''            .then_with(|| {\n                right\n                    .ranking\n                    .learned_contextual_frecency_milli\n                    .cmp(&left.ranking.learned_contextual_frecency_milli)\n            })\n''',
    '''            .then_with(|| {\n                right\n                    .ranking\n                    .learned_path_contextual_frecency_milli\n                    .cmp(&left.ranking.learned_path_contextual_frecency_milli)\n            })\n            .then_with(|| {\n                right\n                    .ranking\n                    .learned_path_contextual_fitness_milli\n                    .unwrap_or_default()\n                    .cmp(\n                        &left\n                            .ranking\n                            .learned_path_contextual_fitness_milli\n                            .unwrap_or_default(),\n                    )\n            })\n            .then_with(|| {\n                right\n                    .ranking\n                    .learned_path_frecency_milli\n                    .cmp(&left.ranking.learned_path_frecency_milli)\n            })\n            .then_with(|| {\n                right\n                    .ranking\n                    .learned_contextual_frecency_milli\n                    .cmp(&left.ranking.learned_contextual_frecency_milli)\n            })\n''',
)
patch(
    op,
    '''    let rendered = expression.render();\n    ResolvePath {\n        version: OPERATIVE_SYNTAX_VERSION.into(),\n        identity: stable_path_identity(&rendered),\n''',
    '''    ResolvePath {\n        version: OPERATIVE_SYNTAX_VERSION.into(),\n        identity,\n''',
)
patch(
    op,
    '''fn evaluate(\n    expression: &ResolveExpression,\n    resources: &dyn ResourceIndex,\n    steps: &mut Vec<ResolvePathStep>,\n) -> Vec<ResolveCandidate> {\n''',
    '''fn evaluate(\n    expression: &ResolveExpression,\n    resources: &dyn ResourceIndex,\n    path_identity: &str,\n    steps: &mut Vec<ResolvePathStep>,\n) -> Vec<ResolveCandidate> {\n''',
)
patch(
    op,
    '''            let candidates = subject_candidates(value, resources);\n''',
    '''            let candidates = subject_candidates(value, resources, path_identity);\n''',
)
# Recursive evaluate calls: five occurrences in current implementation.
patch(
    op,
    '''            let mut candidates = evaluate(expression, resources, steps);\n''',
    '''            let mut candidates = evaluate(expression, resources, path_identity, steps);\n''',
)
patch(
    op,
    '''            let candidates = evaluate(expression, resources, steps);\n''',
    '''            let candidates = evaluate(expression, resources, path_identity, steps);\n''',
)
patch(
    op,
    '''            let mut candidates = evaluate(left, resources, steps);\n            let right = evaluate(right, resources, steps);\n''',
    '''            let mut candidates = evaluate(left, resources, path_identity, steps);\n            let right = evaluate(right, resources, path_identity, steps);\n''',
)
patch(
    op,
    '''            evaluate(expression, resources, steps)\n''',
    '''            evaluate(expression, resources, path_identity, steps)\n''',
)
patch(
    op,
    '''fn subject_candidates(value: &str, resources: &dyn ResourceIndex) -> Vec<ResolveCandidate> {\n''',
    '''fn subject_candidates(\n    value: &str,\n    resources: &dyn ResourceIndex,\n    path_identity: &str,\n) -> Vec<ResolveCandidate> {\n''',
)
patch(
    op,
    '''                ranking: resources.resolve_ranking(&record.descriptor.id),\n''',
    '''                ranking: resources\n                    .resolve_path_ranking(path_identity, &record.descriptor.id),\n''',
)
patch(
    op,
    '''fn stable_path_identity(rendered: &str) -> String {\n''',
    '''pub fn resolve_path_identity(expression: &ResolveExpression) -> String {\n    stable_path_identity(&expression.render())\n}\n\nfn stable_path_identity(rendered: &str) -> String {\n''',
)

mod_rs = "crates/aikit-core/src/resource/mod.rs"
patch(
    mod_rs,
    '''    resolve_action_candidates, resolve_expression, resolve_search, six_horizon_disclosure,\n''',
    '''    resolve_action_candidates, resolve_expression, resolve_path_identity, resolve_search,\n    six_horizon_disclosure,\n''',
)

# --- Production Search applies same-path familiarity before final Resolve ordering. ---
service = "crates/aikit-tui/src/application_service.rs"
patch(
    service,
    '''    parse_or_search_expression, resolve_expression, ContextualActionDescriptor, NavigationEvidence,\n''',
    '''    parse_or_search_expression, resolve_expression, resolve_path_identity,\n    ContextualActionDescriptor, NavigationEvidence,\n''',
)
patch(
    service,
    '''        let path = resolve_expression(&expression, &index, 256);\n''',
    '''        if let Some(familiarity) = self.backend.familiarity()? {\n            index.apply_resolve_path_familiarity(\n                &familiarity,\n                &resolve_path_identity(&expression),\n                &familiarity_context(self.backend.context()),\n                now_ms(),\n                DEFAULT_FAMILIARITY_HALF_LIFE_MS,\n            );\n        }\n        let path = resolve_expression(&expression, &index, 256);\n''',
)
patch(
    service,
    '''                            FamiliarityUse::ResolvePath {\n                                route,\n                                steps,\n                                operative,\n                            } => {\n                                format!(\n                                    "resolve {} · route {route} · {} step{}",\n                                    operative.path_identity,\n                                    steps.len(),\n                                    plural(steps.len())\n                                )\n                            }\n''',
    '''                            FamiliarityUse::ResolvePath {\n                                knowledge_route,\n                                steps,\n                                operative,\n                            } => {\n                                let route = knowledge_route\n                                    .as_ref()\n                                    .map(|route| format!(" · route {route}"))\n                                    .unwrap_or_default();\n                                format!(\n                                    "resolve {}{route} · {} step{}",\n                                    operative.path_identity,\n                                    steps.len(),\n                                    plural(steps.len())\n                                )\n                            }\n''',
)

# --- Explain/History keeps ResolvePath distinct and reconstructable. ---
history = "crates/aikit-core/src/explain_history.rs"
patch(
    history,
    '''    Familiarity,\n    KnowledgeRoute,\n''',
    '''    Familiarity,\n    ResolvePath,\n    KnowledgeRoute,\n''',
)
patch(
    history,
    '''        FamiliarityUse::ResolvePath {\n            route,\n            steps,\n            operative,\n        } => {\n            canonical_refs.insert(route.clone());\n''',
    '''        FamiliarityUse::ResolvePath {\n            knowledge_route,\n            steps,\n            operative,\n        } => {\n            if let Some(route) = knowledge_route {\n                canonical_refs.insert(route.clone());\n            }\n''',
)
patch(
    history,
    '''            (\n                HistoryKind::KnowledgeRoute,\n                format!(\n                    "resolved operative path {} via {route} to {} through {} step{}",\n                    operative.path_identity,\n                    observation.destination,\n                    steps.len(),\n                    if steps.len() == 1 { "" } else { "s" }\n                ),\n                HistoryRecoverability::ReplayNavigation,\n            )\n''',
    '''            let route = knowledge_route\n                .as_ref()\n                .map(|route| format!(" via {route}"))\n                .unwrap_or_default();\n            (\n                HistoryKind::ResolvePath,\n                format!(\n                    "resolved operative path {}{route} to {} through {} step{}",\n                    operative.path_identity,\n                    observation.destination,\n                    steps.len(),\n                    if steps.len() == 1 { "" } else { "s" }\n                ),\n                HistoryRecoverability::ReplayNavigation,\n            )\n''',
)
# Add operative reconstruction detail after the common details map is created.
patch(
    history,
    '''    if let Some(fitness) = &observation.fitness {\n        details.insert("fitnessMilli".into(), fitness.score_milli.to_string());\n        details.insert("fitnessProvenance".into(), fitness.provenance.clone());\n    }\n\n    HistoryEvidence {\n''',
    '''    if let Some(fitness) = &observation.fitness {\n        details.insert("fitnessMilli".into(), fitness.score_milli.to_string());\n        details.insert("fitnessProvenance".into(), fitness.provenance.clone());\n    }\n    if let FamiliarityUse::ResolvePath {\n        knowledge_route,\n        operative,\n        ..\n    } = &observation.use_kind\n    {\n        details.insert("pathIdentity".into(), operative.path_identity.clone());\n        details.insert("expression".into(), operative.expression.render());\n        details.insert(\n            "relationOps".into(),\n            operative\n                .relation_ops\n                .iter()\n                .map(|op| op.symbol())\n                .collect::<Vec<_>>()\n                .join(" "),\n        );\n        details.insert(\n            "horizons".into(),\n            operative\n                .horizons\n                .iter()\n                .map(ToString::to_string)\n                .collect::<Vec<_>>()\n                .join(" "),\n        );\n        for (key, reference) in [\n            ("knowledgeRoute", knowledge_route.as_ref()),\n            ("method", operative.method.as_ref()),\n            ("action", operative.action.as_ref()),\n            ("surface", operative.surface.as_ref()),\n            ("activity", operative.activity.as_ref()),\n            ("return", operative.return_ref.as_ref()),\n        ] {\n            if let Some(reference) = reference {\n                details.insert(key.into(), reference.to_string());\n            }\n        }\n    }\n\n    HistoryEvidence {\n''',
)

# --- Acceptance: preserve independent destination/route/path learning and resets. ---
test = "crates/aikit-core/tests/familiarity_v2.rs"
patch(
    test,
    '''    FamiliarityObservation, FamiliaritySnapshotLoad, FamiliarityStore, FitnessEvidence,\n    ForgetScope, RouteStepEvidence, DEFAULT_FAMILIARITY_HALF_LIFE_MS, FAMILIARITY_SCHEMA_VERSION,\n''',
    '''    FamiliarityObservation, FamiliaritySnapshotLoad, FamiliarityStore, FitnessEvidence,\n    ForgetScope, OperativePathEvidence, RouteStepEvidence, DEFAULT_FAMILIARITY_HALF_LIFE_MS,\n    FAMILIARITY_SCHEMA_VERSION,\n''',
)
# The actual import is line-wrapped differently on current branch; repair alternate anchor if needed.
p = Path(test)
s = p.read_text()
if "OperativePathEvidence" not in s.split(";", 2)[1]:
    s = s.replace(
        "    FamiliarityUse, FitnessEvidence, ForgetScope, RouteStepEvidence,\n",
        "    FamiliarityUse, FitnessEvidence, ForgetScope, OperativePathEvidence, RouteStepEvidence,\n",
        1,
    )
    p.write_text(s)
patch(
    test,
    '''#[test]\nfn repeated_use_has_no_authority_to_change_eligibility_preference_or_trust_like_state() {\n''',
    '''#[test]\nfn resolve_path_familiarity_has_its_own_identity_and_reset_scope() {\n    use aikit_core::resource::{\n        parse_resolve_expression, resolve_path_identity, AddressHorizon, RelationOp,\n    };\n\n    let destination = r("action/verify");\n    let route = r("knowledge-route/spec-to-verify");\n    let ctx = context("project/app", "agent/operator", "verify");\n    let expression = parse_resolve_expression("+ @5 action/verify").unwrap();\n    let path_identity = resolve_path_identity(&expression);\n    let steps = vec![RouteStepEvidence {\n        resource: destination.clone(),\n        provider: Some(r("provider/native")),\n        lens: Some("action".into()),\n        revision: Some("r7".into()),\n    }];\n    let mut store = FamiliarityStore::new();\n\n    store\n        .record(\n            FamiliarityObservation::route(\n                "evt/route/independent",\n                route.clone(),\n                destination.clone(),\n                steps.clone(),\n                ctx.clone(),\n                1_000,\n            )\n            .unwrap(),\n        )\n        .unwrap();\n    store\n        .record(\n            FamiliarityObservation::resolve_path(\n                "evt/path/independent",\n                Some(route.clone()),\n                destination.clone(),\n                steps,\n                OperativePathEvidence {\n                    path_identity: path_identity.clone(),\n                    expression: expression.clone(),\n                    relation_ops: vec![RelationOp::Affirm],\n                    horizons: vec![AddressHorizon::H5],\n                    method: Some(r("method/verify")),\n                    action: Some(destination.clone()),\n                    surface: Some(r("surface/aikit/tui")),\n                    activity: None,\n                    return_ref: None,\n                },\n                ctx.clone(),\n                1_001,\n            )\n            .unwrap(),\n        )\n        .unwrap();\n\n    assert_eq!(\n        store\n            .assess_route(&route, &destination, &ctx, 1_002, 10_000)\n            .observations,\n        1\n    );\n    let path = store.assess_resolve_path(\n        &path_identity,\n        &destination,\n        &ctx,\n        1_002,\n        10_000,\n    );\n    assert_eq!(path.observations, 1);\n    assert_eq!(path.route, None);\n    assert_eq!(path.path_identity.as_deref(), Some(path_identity.as_str()));\n    assert_eq!(\n        store.resolve_path_steps(&path_identity),\n        std::collections::BTreeSet::from([vec![destination.clone()]])\n    );\n\n    assert_eq!(\n        store.forget(&ForgetScope::ResolvePath(path_identity.clone())),\n        1\n    );\n    assert!(store\n        .assess_resolve_path(&path_identity, &destination, &ctx, 1_003, 10_000)\n        .is_empty());\n    assert_eq!(\n        store\n            .assess_route(&route, &destination, &ctx, 1_003, 10_000)\n            .observations,\n        1,\n        "forgetting operative-path ease must not erase KnowledgeRoute evidence"\n    );\n}\n\n#[test]\nfn repeated_use_has_no_authority_to_change_eligibility_preference_or_trust_like_state() {\n''',
)
