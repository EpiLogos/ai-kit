#!/usr/bin/env python3
from pathlib import Path


def patch(path: str, old: str, new: str, count: int = 1) -> None:
    target = Path(path)
    source = target.read_text()
    if source.count(old) < count:
        raise SystemExit(f"missing patch anchor in {path}: {old[:100]!r}")
    target.write_text(source.replace(old, new, count))


# ResourceIndex remains the identity/read boundary, but may now expose derived
# ordering evidence. Memory/general indexes default to no ranking signal; the
# production ResourceSearchIndex supplies already-contextualised #29 familiarity.
index = "crates/aikit-core/src/resource/index.rs"
patch(
    index,
    '''use std::collections::BTreeMap;\n\nuse super::{ResourceRecord, ResourceRef};\n\npub trait ResourceIndex {\n    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord>;\n    fn resources(&self) -> Vec<&ResourceRecord>;\n}\n''',
    '''use std::collections::BTreeMap;\n\nuse serde::{Deserialize, Serialize};\n\nuse super::{ResourceRecord, ResourceRef};\n\n/// Derived ordering inputs visible to Resolve without conferring authority or\n/// mutating Resource identity. Search relevance remains primary; authored\n/// preference then learned contextual accessibility break otherwise comparable\n/// candidates exactly as the production navigation field already does.\n#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]\npub struct ResolveRankingSignals {\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub authored_preference_rank: Option<i32>,\n    #[serde(default)]\n    pub learned_observations: usize,\n    #[serde(default)]\n    pub learned_contextual_observations: usize,\n    #[serde(default)]\n    pub learned_frecency_milli: i64,\n    #[serde(default)]\n    pub learned_contextual_frecency_milli: i64,\n    #[serde(default, skip_serializing_if = "Option::is_none")]\n    pub learned_contextual_fitness_milli: Option<i32>,\n}\n\npub trait ResourceIndex {\n    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord>;\n    fn resources(&self) -> Vec<&ResourceRecord>;\n\n    fn resolve_ranking(&self, _id: &ResourceRef) -> ResolveRankingSignals {\n        ResolveRankingSignals::default()\n    }\n}\n''',
)

mod_rs = "crates/aikit-core/src/resource/mod.rs"
patch(
    mod_rs,
    '''pub use index::{MemoryResourceIndex, ResourceIndex};\n''',
    '''pub use index::{MemoryResourceIndex, ResolveRankingSignals, ResourceIndex};\n''',
)

search = "crates/aikit-core/src/resource/search.rs"
patch(
    search,
    '''use super::{ResourceIndex, ResourceKind, ResourceRecord, ResourceRef};\n''',
    '''use super::{\n    ResolveRankingSignals, ResourceIndex, ResourceKind, ResourceRecord, ResourceRef,\n};\n''',
)
patch(
    search,
    '''impl ResourceIndex for ResourceSearchIndex {\n    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord> {\n        self.resources.get(id).map(|indexed| &indexed.record)\n    }\n\n    fn resources(&self) -> Vec<&ResourceRecord> {\n        self.resources\n            .values()\n            .map(|indexed| &indexed.record)\n            .collect()\n    }\n}\n''',
    '''impl ResourceIndex for ResourceSearchIndex {\n    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord> {\n        self.resources.get(id).map(|indexed| &indexed.record)\n    }\n\n    fn resources(&self) -> Vec<&ResourceRecord> {\n        self.resources\n            .values()\n            .map(|indexed| &indexed.record)\n            .collect()\n    }\n\n    fn resolve_ranking(&self, id: &ResourceRef) -> ResolveRankingSignals {\n        let Some(indexed) = self.resources.get(id) else {\n            return ResolveRankingSignals::default();\n        };\n        let familiarity = indexed.familiarity.as_ref();\n        ResolveRankingSignals {\n            authored_preference_rank: indexed.record.preference.as_ref().map(|value| value.rank),\n            learned_observations: familiarity.map_or(0, |value| value.observations),\n            learned_contextual_observations: familiarity\n                .map_or(0, |value| value.contextual_observations),\n            learned_frecency_milli: familiarity\n                .map_or(0, |value| quantise_milli(value.frecency)),\n            learned_contextual_frecency_milli: familiarity\n                .map_or(0, |value| quantise_milli(value.contextual_frecency)),\n            learned_contextual_fitness_milli: familiarity\n                .and_then(|value| value.contextual_fitness_milli)\n                .map(|value| value.round() as i32),\n        }\n    }\n}\n''',
)

operative = "crates/aikit-core/src/resource/operative.rs"
# Bare @/@N are genuine apertures: use the empty subject as 'all currently
# addressable resources', not as a synthetic named resource. This also makes the
# issue's representative `@# @5` expression executable.
patch(
    operative,
    '''            Some(Token::Address(horizon)) => {\n                self.next();\n                Ok(ResolveExpression::Address {\n                    horizon,\n                    expression: Box::new(self.parse_unary()?),\n                })\n            }\n''',
    '''            Some(Token::Address(horizon)) => {\n                self.next();\n                let expression = if self.peek().is_none()\n                    || matches!(self.peek(), Some(Token::RParen))\n                {\n                    ResolveExpression::Subject {\n                        value: String::new(),\n                    }\n                } else {\n                    self.parse_unary()?\n                };\n                Ok(ResolveExpression::Address {\n                    horizon,\n                    expression: Box::new(expression),\n                })\n            }\n''',
)
patch(
    operative,
    '''pub struct ResolveCandidate {\n    pub resource: ResourceRef,\n    pub kind: ResourceKind,\n    pub horizons: BTreeSet<AddressHorizon>,\n    pub exact: bool,\n    pub score: i64,\n}\n''',
    '''pub struct ResolveCandidate {\n    pub resource: ResourceRef,\n    pub kind: ResourceKind,\n    pub horizons: BTreeSet<AddressHorizon>,\n    pub exact: bool,\n    /// Primary textual/exact relevance. Derived ranking signals remain separate\n    /// so learned use cannot numerically overpower relevance or authorship.\n    pub score: i64,\n    #[serde(default)]\n    pub ranking: super::ResolveRankingSignals,\n}\n''',
)
patch(
    operative,
    '''    candidates.sort_by(|left, right| {\n        right\n            .exact\n            .cmp(&left.exact)\n            .then_with(|| right.score.cmp(&left.score))\n            .then_with(|| left.resource.cmp(&right.resource))\n    });\n''',
    '''    candidates.sort_by(|left, right| {\n        right\n            .exact\n            .cmp(&left.exact)\n            .then_with(|| right.score.cmp(&left.score))\n            .then_with(|| {\n                right\n                    .ranking\n                    .authored_preference_rank\n                    .is_some()\n                    .cmp(&left.ranking.authored_preference_rank.is_some())\n            })\n            .then_with(|| {\n                right\n                    .ranking\n                    .authored_preference_rank\n                    .unwrap_or_default()\n                    .cmp(&left.ranking.authored_preference_rank.unwrap_or_default())\n            })\n            .then_with(|| {\n                right\n                    .ranking\n                    .learned_contextual_frecency_milli\n                    .cmp(&left.ranking.learned_contextual_frecency_milli)\n            })\n            .then_with(|| {\n                right\n                    .ranking\n                    .learned_contextual_fitness_milli\n                    .unwrap_or_default()\n                    .cmp(&left.ranking.learned_contextual_fitness_milli.unwrap_or_default())\n            })\n            .then_with(|| {\n                right\n                    .ranking\n                    .learned_frecency_milli\n                    .cmp(&left.ranking.learned_frecency_milli)\n            })\n            .then_with(|| left.resource.cmp(&right.resource))\n    });\n''',
)
patch(
    operative,
    '''            Some(ResolveCandidate {\n                resource: record.descriptor.id.clone(),\n                kind: record.descriptor.kind,\n                horizons: horizons_for_resource(record),\n                exact,\n                score,\n            })\n''',
    '''            Some(ResolveCandidate {\n                resource: record.descriptor.id.clone(),\n                kind: record.descriptor.kind,\n                horizons: horizons_for_resource(record),\n                exact,\n                score,\n                ranking: resources.resolve_ranking(&record.descriptor.id),\n            })\n''',
)
# Add direct acceptance beside the existing ordinary Search projection test.
patch(
    operative,
    '''    #[test]\n    fn ordinary_search_is_potential_universal_resolution() {\n        let expression = parse_or_search_expression("orient project").unwrap();\n        assert_eq!(expression, ResolveExpression::ordinary_search("orient project"));\n        assert_eq!(expression.render(), "@# @ \\\"orient project\\\"");\n    }\n''',
    '''    #[test]\n    fn ordinary_search_is_potential_universal_resolution() {\n        let expression = parse_or_search_expression("orient project").unwrap();\n        assert_eq!(expression, ResolveExpression::ordinary_search("orient project"));\n        assert_eq!(expression.render(), "@# @ \\\"orient project\\\"");\n    }\n\n    #[test]\n    fn bare_horizon_is_an_open_aperture_and_potential_horizon_is_executable() {\n        let mut resources = MemoryResourceIndex::default();\n        resources.insert(record("action:verify", ResourceKind::Action));\n        resources.insert(record("project:demo", ResourceKind::Project));\n\n        let horizon = parse_resolve_expression("@5").unwrap();\n        let path = resolve_expression(&horizon, &resources, 16);\n        assert_eq!(horizon.render(), "@5 \\\"\\\"");\n        assert_eq!(path.candidates.len(), 1);\n        assert_eq!(path.candidates[0].resource.as_str(), "action:verify");\n\n        let potential = parse_resolve_expression("@# @5").unwrap();\n        let potential_path = resolve_expression(&potential, &resources, 16);\n        assert_eq!(potential_path.candidates.len(), 1);\n        assert_eq!(potential_path.candidates[0].resource.as_str(), "action:verify");\n    }\n''',
)
