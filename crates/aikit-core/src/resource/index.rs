use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ResourceRecord, ResourceRef};

/// Derived ordering inputs visible to Resolve without conferring authority or
/// mutating Resource identity. Search relevance remains primary; authored and
/// current-context evidence then break otherwise comparable candidates before
/// learned accessibility. None of these signals changes eligibility, trust or
/// canonical identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ResolveRankingSignals {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_preference_rank: Option<i32>,
    /// The Resource is declared by the current Project/ProjectLocal scope.
    #[serde(default)]
    pub current_project: bool,
    /// The Resource is active/present in the current resolved operating context.
    #[serde(default)]
    pub active_in_context: bool,
    #[serde(default)]
    pub learned_path_observations: usize,
    #[serde(default)]
    pub learned_path_contextual_observations: usize,
    #[serde(default)]
    pub learned_path_frecency_milli: i64,
    #[serde(default)]
    pub learned_path_contextual_frecency_milli: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learned_path_contextual_fitness_milli: Option<i32>,
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

pub trait ResourceIndex {
    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord>;
    fn resources(&self) -> Vec<&ResourceRecord>;

    fn resolve_ranking(&self, _id: &ResourceRef) -> ResolveRankingSignals {
        ResolveRankingSignals::default()
    }

    fn resolve_path_ranking(
        &self,
        _path_identity: &str,
        id: &ResourceRef,
    ) -> ResolveRankingSignals {
        self.resolve_ranking(id)
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryResourceIndex {
    resources: BTreeMap<ResourceRef, ResourceRecord>,
}

impl MemoryResourceIndex {
    pub fn insert(&mut self, record: ResourceRecord) -> Option<ResourceRecord> {
        self.resources.insert(record.descriptor.id.clone(), record)
    }

    pub fn len(&self) -> usize {
        self.resources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }
}

impl ResourceIndex for MemoryResourceIndex {
    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord> {
        self.resources.get(id)
    }

    fn resources(&self) -> Vec<&ResourceRecord> {
        self.resources.values().collect()
    }
}
