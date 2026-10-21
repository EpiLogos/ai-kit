//! Two owner reads over the existing Living Knowledge and ProjectMap
//! federation, both answered by the library's own deterministic operations.
//!
//! * `knowledge impact`: the transitive Living Knowledge impact of an owner
//!   change horizon over the dependency manifests this project's Semantic
//!   Wiki carries (`wiki_living_dependencies`), narrowed to a set of readings
//!   when the caller names them. It is the same closure `flow changed-since`
//!   and the Contemplate preflight compute, asked for readings rather than
//!   for one recorded thought.
//! * `knowledge reflection verify`: the verdict of a target-owned reflection
//!   law (`verify_reflection_law`) against the explicit bindings of the
//!   runtime's ProjectMap.
//!
//! Neither records anything, invokes an Agent or model, or reads a source's
//! payload. The horizon and the law are owner seams the caller supplies; the
//! surface fabricates neither.

use std::collections::BTreeSet;

use aikit_core::knowledge_living::KnowledgeChangeHorizon;
use aikit_core::knowledge_living_relations::{
    deterministic_transitive_knowledge_impact, KnowledgeTransitiveImpact,
};
use aikit_core::project_reflection::{
    verify_reflection_law, ReflectionLaw, ReflectionVerification,
};
use aikit_core::resource::ResourceRef;
use aikit_core::wiki_living_dependencies;
use serde::{Deserialize, Serialize};

use super::Service;

pub const KNOWLEDGE_IMPACT_READING_VERSION: &str = "aikit.knowledge-impact-reading/v1";
pub const REFLECTION_VERDICT_VERSION: &str = "aikit.reflection-verdict/v1";

/// The impact of one horizon, for every reading the Wiki's manifests reach or
/// for the named readings only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeImpactReading {
    pub version: String,
    /// The readings asked for; empty means every resource the closure reached.
    pub readings: Vec<ResourceRef>,
    /// How many Wiki objects and dependency manifests the closure followed.
    pub wiki_objects: usize,
    pub source_dependencies: usize,
    pub resource_dependencies: usize,
    /// The library's closure. With named readings, `direct.affected`,
    /// `transitive`, `paths` and `pending_integration` keep only those
    /// readings' rows; `changed_sources` and `truncated` stay as computed.
    pub impact: KnowledgeTransitiveImpact,
    /// Deterministic: never an Agent or model.
    pub automatic_agent_or_model_invocation: bool,
}

/// One target-owned reflection law's verdict over this project's ProjectMap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReflectionVerdict {
    pub version: String,
    pub law: String,
    /// How many explicit bindings the map carried when the law was verified.
    pub map_bindings: usize,
    pub verification: ReflectionVerification,
    pub automatic_agent_or_model_invocation: bool,
}

impl Service {
    pub fn knowledge_impact(
        &self,
        horizon: &KnowledgeChangeHorizon,
        readings: &[ResourceRef],
        max_depth: usize,
        max_affected: usize,
    ) -> aikit_core::Result<KnowledgeImpactReading> {
        self.with_knowledge(|runtime, _| {
            let mut objects = Vec::new();
            if let Some(index) = runtime.wiki_index() {
                for resource in index.discover() {
                    if let Some(object) = index.resolve(&resource) {
                        objects.push(object);
                    }
                }
            }
            let (source_dependencies, resource_dependencies) = wiki_living_dependencies(&objects)?;
            let mut impact = deterministic_transitive_knowledge_impact(
                horizon,
                &source_dependencies,
                &resource_dependencies,
                max_depth,
                max_affected,
            )?;
            if !readings.is_empty() {
                let wanted = readings.iter().collect::<BTreeSet<_>>();
                impact
                    .direct
                    .affected
                    .retain(|row| wanted.contains(&row.resource));
                impact
                    .transitive
                    .retain(|row| wanted.contains(&row.resource));
                impact.paths.retain(|path| wanted.contains(&path.resource));
                impact
                    .pending_integration
                    .retain(|resource| wanted.contains(resource));
            }
            Ok(KnowledgeImpactReading {
                version: KNOWLEDGE_IMPACT_READING_VERSION.into(),
                readings: readings.to_vec(),
                wiki_objects: objects.len(),
                source_dependencies: source_dependencies.len(),
                resource_dependencies: resource_dependencies.len(),
                impact,
                automatic_agent_or_model_invocation: false,
            })
        })
    }

    pub fn reflection_verdict(&self, law: &ReflectionLaw) -> aikit_core::Result<ReflectionVerdict> {
        self.with_knowledge(|runtime, _| {
            let map = runtime.project_map();
            Ok(ReflectionVerdict {
                version: REFLECTION_VERDICT_VERSION.into(),
                law: law.id.clone(),
                map_bindings: map.bindings().len(),
                verification: verify_reflection_law(map, law),
                automatic_agent_or_model_invocation: false,
            })
        })
    }
}
