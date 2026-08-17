//! V2 learned accessibility for Resources and navigation routes.
//!
//! This is deliberately **learned ease**, not learned truth. The store records
//! observed traversal/use evidence and can assess deterministic contextual
//! frecency and outcome fitness. It owns no trust, eligibility, authored
//! preference, freshness, provider graph, activation or projection state, so it
//! has no API through which repeated use can silently mutate those things.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::resource::ResourceRef;
use crate::{AikitError, Result};

pub const FAMILIARITY_SCHEMA_VERSION: &str = "aikit.familiarity/v2";
pub const DEFAULT_FAMILIARITY_HALF_LIFE_MS: u64 = 14 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct FamiliarityContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteStepEvidence {
    pub resource: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FitnessEvidence {
    /// Normalised signed outcome evidence in [-1000, 1000]. This is reported as a
    /// separate signal and is never folded into frecency at storage time.
    pub score_milli: i16,
    pub provenance: String,
}

impl FitnessEvidence {
    pub fn new(score_milli: i16, provenance: impl Into<String>) -> Result<Self> {
        if !(-1000..=1000).contains(&score_milli) {
            return Err(AikitError::new(
                "familiarity.invalid_fitness",
                "fitness evidence must be between -1000 and 1000",
            )
            .with("score_milli", score_milli.to_string()));
        }
        Ok(Self {
            score_milli,
            provenance: provenance.into(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FamiliarityUse {
    Destination,
    Route {
        route: ResourceRef,
        steps: Vec<RouteStepEvidence>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamiliarityObservation {
    /// Durable event/trace identity supplied by the observer. Learned state is
    /// rebuildable from these observations and never invents canonical identity.
    pub observation_id: String,
    pub destination: ResourceRef,
    pub context: FamiliarityContext,
    pub use_kind: FamiliarityUse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_surface: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_action: Option<ResourceRef>,
    /// Caller-supplied time makes tests and replay deterministic.
    pub observed_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fitness: Option<FitnessEvidence>,
}

impl FamiliarityObservation {
    pub fn destination(
        observation_id: impl Into<String>,
        destination: ResourceRef,
        context: FamiliarityContext,
        observed_at_ms: u64,
    ) -> Self {
        Self {
            observation_id: observation_id.into(),
            destination,
            context,
            use_kind: FamiliarityUse::Destination,
            source_surface: None,
            source_action: None,
            observed_at_ms,
            fitness: None,
        }
    }

    pub fn route(
        observation_id: impl Into<String>,
        route: ResourceRef,
        destination: ResourceRef,
        steps: Vec<RouteStepEvidence>,
        context: FamiliarityContext,
        observed_at_ms: u64,
    ) -> Result<Self> {
        if steps.is_empty() {
            return Err(AikitError::new(
                "familiarity.empty_route",
                "a route familiarity observation must contain at least one step",
            ));
        }
        if steps.last().is_none_or(|step| step.resource != destination) {
            return Err(AikitError::new(
                "familiarity.route_destination_mismatch",
                "the final route step must be the recorded destination",
            )
            .with("route", route.to_string())
            .with("destination", destination.to_string()));
        }
        Ok(Self {
            observation_id: observation_id.into(),
            destination,
            context,
            use_kind: FamiliarityUse::Route { route, steps },
            source_surface: None,
            source_action: None,
            observed_at_ms,
            fitness: None,
        })
    }

    #[must_use]
    pub fn from_surface(mut self, surface: ResourceRef) -> Self {
        self.source_surface = Some(surface);
        self
    }

    #[must_use]
    pub fn via_action(mut self, action: ResourceRef) -> Self {
        self.source_action = Some(action);
        self
    }

    #[must_use]
    pub fn with_fitness(mut self, fitness: FitnessEvidence) -> Self {
        self.fitness = Some(fitness);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessibilitySignalClass {
    Frecency,
    Context,
    Fitness,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessibilitySignal {
    pub class: AccessibilitySignalClass,
    pub value: f64,
    pub evidence_count: usize,
    pub explanation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityAssessment {
    pub destination: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<ResourceRef>,
    pub observations: usize,
    pub contextual_observations: usize,
    pub frecency: f64,
    pub contextual_frecency: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contextual_fitness_milli: Option<f64>,
    pub signals: Vec<AccessibilitySignal>,
    pub evidence_ids: Vec<String>,
}

impl AccessibilityAssessment {
    pub fn is_empty(&self) -> bool {
        self.observations == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamiliaritySnapshot {
    pub schema: String,
    pub observations: Vec<FamiliarityObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FamiliaritySnapshotLoad {
    Loaded(FamiliarityStore),
    /// Unknown schema explicitly invalidates learned influence. Canonical
    /// ResourceRefs live outside this store and are therefore untouched.
    Invalidated {
        found_schema: String,
        observations_discarded: usize,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForgetScope {
    Destination(ResourceRef),
    Route(ResourceRef),
    Project(ResourceRef),
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FamiliarityStore {
    observations: BTreeMap<String, FamiliarityObservation>,
}

impl FamiliarityStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, observation: FamiliarityObservation) -> Result<()> {
        if observation.observation_id.trim().is_empty() {
            return Err(AikitError::new(
                "familiarity.missing_observation_id",
                "a familiarity observation needs a durable event/trace identity",
            ));
        }
        if self.observations.contains_key(&observation.observation_id) {
            return Err(AikitError::new(
                "familiarity.duplicate_observation",
                format!(
                    "familiarity observation {} already exists",
                    observation.observation_id
                ),
            )
            .with("observation_id", observation.observation_id));
        }
        self.observations
            .insert(observation.observation_id.clone(), observation);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.observations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    pub fn snapshot(&self) -> FamiliaritySnapshot {
        FamiliaritySnapshot {
            schema: FAMILIARITY_SCHEMA_VERSION.to_string(),
            observations: self.observations.values().cloned().collect(),
        }
    }

    pub fn load(snapshot: FamiliaritySnapshot) -> Result<FamiliaritySnapshotLoad> {
        if snapshot.schema != FAMILIARITY_SCHEMA_VERSION {
            return Ok(FamiliaritySnapshotLoad::Invalidated {
                found_schema: snapshot.schema,
                observations_discarded: snapshot.observations.len(),
                reason: format!(
                    "learned accessibility schema changed; expected {FAMILIARITY_SCHEMA_VERSION}"
                ),
            });
        }
        let mut store = FamiliarityStore::new();
        for observation in snapshot.observations {
            store.record(observation)?;
        }
        Ok(FamiliaritySnapshotLoad::Loaded(store))
    }

    pub fn assess_destination(
        &self,
        destination: &ResourceRef,
        context: &FamiliarityContext,
        now_ms: u64,
        half_life_ms: u64,
    ) -> AccessibilityAssessment {
        self.assess(destination, None, context, now_ms, half_life_ms)
    }

    pub fn assess_route(
        &self,
        route: &ResourceRef,
        destination: &ResourceRef,
        context: &FamiliarityContext,
        now_ms: u64,
        half_life_ms: u64,
    ) -> AccessibilityAssessment {
        self.assess(destination, Some(route), context, now_ms, half_life_ms)
    }

    fn assess(
        &self,
        destination: &ResourceRef,
        route: Option<&ResourceRef>,
        context: &FamiliarityContext,
        now_ms: u64,
        half_life_ms: u64,
    ) -> AccessibilityAssessment {
        let half_life_ms = half_life_ms.max(1) as f64;
        let mut matching = Vec::new();
        for observation in self.observations.values() {
            if &observation.destination != destination {
                continue;
            }
            let route_matches = match (route, &observation.use_kind) {
                (None, FamiliarityUse::Destination) => true,
                (Some(wanted), FamiliarityUse::Route { route, .. }) => route == wanted,
                _ => false,
            };
            if route_matches {
                matching.push(observation);
            }
        }

        let contextual = matching
            .iter()
            .copied()
            .filter(|observation| contexts_match(context, &observation.context))
            .collect::<Vec<_>>();
        let decay = |observation: &FamiliarityObservation| {
            let age = now_ms.saturating_sub(observation.observed_at_ms) as f64;
            0.5f64.powf(age / half_life_ms)
        };
        let frecency = matching.iter().map(|observation| decay(observation)).sum();
        let contextual_frecency = contextual
            .iter()
            .map(|observation| decay(observation))
            .sum();
        let fitness = contextual
            .iter()
            .filter_map(|observation| observation.fitness.as_ref())
            .collect::<Vec<_>>();
        let contextual_fitness_milli = (!fitness.is_empty()).then(|| {
            fitness
                .iter()
                .map(|evidence| evidence.score_milli as f64)
                .sum::<f64>()
                / fitness.len() as f64
        });

        let mut signals = Vec::new();
        if !matching.is_empty() {
            signals.push(AccessibilitySignal {
                class: AccessibilitySignalClass::Frecency,
                value: frecency,
                evidence_count: matching.len(),
                explanation: format!(
                    "{} observed use{} with deterministic recency decay",
                    matching.len(),
                    if matching.len() == 1 { "" } else { "s" }
                ),
            });
        }
        if !contextual.is_empty() {
            signals.push(AccessibilitySignal {
                class: AccessibilitySignalClass::Context,
                value: contextual_frecency,
                evidence_count: contextual.len(),
                explanation: format!(
                    "{} use{} matched the requested Project/actor/Agency/Focus context",
                    contextual.len(),
                    if contextual.len() == 1 { "" } else { "s" }
                ),
            });
        }
        if let Some(value) = contextual_fitness_milli {
            signals.push(AccessibilitySignal {
                class: AccessibilitySignalClass::Fitness,
                value,
                evidence_count: fitness.len(),
                explanation: format!(
                    "{} contextual outcome observation{} contributed fitness evidence",
                    fitness.len(),
                    if fitness.len() == 1 { "" } else { "s" }
                ),
            });
        }

        AccessibilityAssessment {
            destination: destination.clone(),
            route: route.cloned(),
            observations: matching.len(),
            contextual_observations: contextual.len(),
            frecency,
            contextual_frecency,
            contextual_fitness_milli,
            signals,
            evidence_ids: matching
                .iter()
                .map(|observation| observation.observation_id.clone())
                .collect(),
        }
    }

    pub fn forget(&mut self, scope: &ForgetScope) -> usize {
        let before = self.observations.len();
        self.observations.retain(|_, observation| match scope {
            ForgetScope::Destination(destination) => {
                !(matches!(observation.use_kind, FamiliarityUse::Destination)
                    && &observation.destination == destination)
            }
            ForgetScope::Route(route) => !matches!(
                &observation.use_kind,
                FamiliarityUse::Route {
                    route: observed, ..
                } if observed == route
            ),
            ForgetScope::Project(project) => observation.context.project.as_ref() != Some(project),
            ForgetScope::All => false,
        });
        before - self.observations.len()
    }

    pub fn route_steps(&self, route: &ResourceRef) -> BTreeSet<Vec<ResourceRef>> {
        self.observations
            .values()
            .filter_map(|observation| match &observation.use_kind {
                FamiliarityUse::Route {
                    route: observed,
                    steps,
                } if observed == route => Some(
                    steps
                        .iter()
                        .map(|step| step.resource.clone())
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .collect()
    }
}

fn contexts_match(requested: &FamiliarityContext, observed: &FamiliarityContext) -> bool {
    optional_axis_matches(&requested.project, &observed.project)
        && optional_axis_matches(&requested.actor, &observed.actor)
        && optional_axis_matches(&requested.agency, &observed.agency)
        && optional_axis_matches(&requested.focus, &observed.focus)
}

fn optional_axis_matches<T: Eq>(requested: &Option<T>, observed: &Option<T>) -> bool {
    requested
        .as_ref()
        .is_none_or(|value| observed.as_ref() == Some(value))
}
