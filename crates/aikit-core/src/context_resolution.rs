//! V2 ContextResolution composition over the proven deterministic capsule resolver.
//!
//! This module does not replace `resolve`. It takes an already-resolved
//! [`ResolvedView`] and composes the wider typed resource field around it so
//! Factory Context can consume one explainable AIKit operational resolution.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::id::ProfileId;
use crate::platform::TargetId;
use crate::project::ProjectBinding;
use crate::resolve::ResolvedView;
use crate::resource::{
    Eligibility, ProviderState, ResourceIndex, ResourceKind, ResourceRecord, ResourceRef,
    SourceState,
};
use crate::scope::{ScopeKind, ScopeLayer};

pub const CONTEXT_RESOLUTION_VERSION: &str = "aikit.context-resolution/v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    Available,
    Unresolved { reasons: Vec<String> },
    Unavailable { reasons: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedResource {
    pub resource: ResourceRecord,
    /// Operational availability is deliberately independent from hard eligibility
    /// and preference, which remain on `resource`.
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ReferenceResolution {
    Resolved { resource: ResolvedResource },
    Missing { reference: ResourceRef, expected: ResourceKind },
    WrongKind {
        reference: ResourceRef,
        expected: ResourceKind,
        actual: ResourceKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeResolution {
    pub kind: ScopeKind,
    pub depth: u16,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionIntent {
    pub targets: Vec<TargetId>,
    /// Legacy active capsule identities are retained because the existing target
    /// projection machinery still consumes `ResolvedView` rather than V2 records.
    pub active_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalPlan {
    pub context_sources: Vec<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RequestedActors {
    #[serde(default)]
    pub agent: Option<ResourceRef>,
    #[serde(default)]
    pub agency: Option<ResourceRef>,
    #[serde(default)]
    pub host: Option<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextResolution {
    pub version: String,
    pub project_binding: ProjectBinding,
    pub legacy_resolution_hash: String,
    pub catalog_revision: String,
    pub profiles: Vec<ProfileId>,
    pub scopes: Vec<ScopeResolution>,
    #[serde(default)]
    pub agent: Option<ReferenceResolution>,
    #[serde(default)]
    pub agency: Option<ReferenceResolution>,
    #[serde(default)]
    pub host: Option<ReferenceResolution>,
    pub capabilities: Vec<ResolvedResource>,
    pub actions: Vec<ResolvedResource>,
    pub context_sources: Vec<ResolvedResource>,
    pub model_candidates: Vec<ResolvedResource>,
    pub harness_candidates: Vec<ResolvedResource>,
    pub execution_offers: Vec<ResolvedResource>,
    pub projection: ProjectionIntent,
    pub retrieval: RetrievalPlan,
    pub warnings: Vec<String>,
}

pub fn compose_context_resolution(
    legacy: &ResolvedView,
    project_binding: ProjectBinding,
    scope_layers: &[ScopeLayer],
    resources: &dyn ResourceIndex,
    requested: RequestedActors,
) -> ContextResolution {
    let mut grouped: BTreeMap<ResourceKind, Vec<ResolvedResource>> = BTreeMap::new();
    for record in resources.resources() {
        grouped
            .entry(record.descriptor.kind)
            .or_default()
            .push(ResolvedResource {
                resource: record.clone(),
                availability: availability(record),
            });
    }
    for values in grouped.values_mut() {
        values.sort_by(|left, right| left.resource.descriptor.id.cmp(&right.resource.descriptor.id));
    }

    let profiles = profiles(scope_layers);
    let scopes = scopes(scope_layers);
    let context_sources = take_group(&mut grouped, ResourceKind::ContextSource);
    let retrieval = RetrievalPlan {
        context_sources: context_sources
            .iter()
            .map(|resource| resource.resource.descriptor.id.clone())
            .collect(),
    };

    let mut targets = legacy.context.targets.clone();
    targets.sort();
    targets.dedup();
    let projection = ProjectionIntent {
        targets,
        active_capabilities: legacy.active.keys().map(ToString::to_string).collect(),
    };

    let agent = requested
        .agent
        .map(|reference| resolve_reference(resources, reference, ResourceKind::Agent));
    let agency = requested
        .agency
        .map(|reference| resolve_reference(resources, reference, ResourceKind::Agency));
    let host = requested
        .host
        .map(|reference| resolve_reference(resources, reference, ResourceKind::Host));

    ContextResolution {
        version: CONTEXT_RESOLUTION_VERSION.to_string(),
        project_binding,
        legacy_resolution_hash: legacy.hash.as_str().to_string(),
        catalog_revision: legacy.catalog_revision.clone(),
        profiles,
        scopes,
        agent,
        agency,
        host,
        capabilities: take_group(&mut grouped, ResourceKind::Capability),
        actions: take_group(&mut grouped, ResourceKind::Action),
        context_sources,
        model_candidates: take_group(&mut grouped, ResourceKind::Model),
        harness_candidates: take_group(&mut grouped, ResourceKind::Harness),
        execution_offers: take_group(&mut grouped, ResourceKind::ExecutionOffer),
        projection,
        retrieval,
        warnings: legacy.warnings.clone(),
    }
}

fn take_group(
    grouped: &mut BTreeMap<ResourceKind, Vec<ResolvedResource>>,
    kind: ResourceKind,
) -> Vec<ResolvedResource> {
    grouped.remove(&kind).unwrap_or_default()
}

fn resolve_reference(
    resources: &dyn ResourceIndex,
    reference: ResourceRef,
    expected: ResourceKind,
) -> ReferenceResolution {
    match resources.resource(&reference) {
        None => ReferenceResolution::Missing {
            reference,
            expected,
        },
        Some(record) if record.descriptor.kind != expected => ReferenceResolution::WrongKind {
            reference,
            expected,
            actual: record.descriptor.kind,
        },
        Some(record) => ReferenceResolution::Resolved {
            resource: ResolvedResource {
                resource: record.clone(),
                availability: availability(record),
            },
        },
    }
}

/// Resolve only operational observation. Eligibility and preference are retained
/// on the record as independent axes and never folded into this result.
pub fn availability(record: &ResourceRecord) -> Availability {
    let mut available = false;
    let mut unresolved = Vec::new();
    let mut unavailable = Vec::new();

    for source in &record.descriptor.sources {
        match &source.state {
            SourceState::Available => available = true,
            SourceState::Unresolved => unresolved.push(format!("source {} unresolved", source.source)),
            SourceState::Unavailable { reason } => {
                unavailable.push(format!("source {} unavailable: {reason}", source.source))
            }
        }
    }
    for provider in &record.providers {
        match &provider.state {
            ProviderState::Available => available = true,
            ProviderState::Unresolved => {
                unresolved.push(format!("provider {} unresolved", provider.provider))
            }
            ProviderState::Unavailable { reason } => unavailable.push(format!(
                "provider {} unavailable: {reason}",
                provider.provider
            )),
        }
    }

    if available {
        return Availability::Available;
    }
    if !unresolved.is_empty() {
        unresolved.extend(unavailable);
        unresolved.sort();
        return Availability::Unresolved { reasons: unresolved };
    }
    if !unavailable.is_empty() {
        unavailable.sort();
        return Availability::Unavailable {
            reasons: unavailable,
        };
    }

    // No source/provider observation exists. Even an eligible or preferred
    // resource is not thereby proven operationally available.
    let reason = match &record.eligibility {
        Eligibility::Eligible => "no source or provider availability has been observed",
        Eligibility::Undetermined => "availability and eligibility are both unresolved",
        Eligibility::Ineligible { .. } => "no source or provider availability has been observed",
    };
    Availability::Unresolved {
        reasons: vec![reason.to_string()],
    }
}

fn profiles(scope_layers: &[ScopeLayer]) -> Vec<ProfileId> {
    let mut profiles = BTreeSet::new();
    for layer in scope_layers {
        profiles.extend(layer.patch.profiles.iter().cloned());
        profiles.extend(layer.patch.uses.iter().map(|profile| profile.profile.clone()));
    }
    profiles.into_iter().collect()
}

fn scopes(scope_layers: &[ScopeLayer]) -> Vec<ScopeResolution> {
    let mut layers = scope_layers.to_vec();
    layers.sort_by_key(|layer| (layer.kind.rank(), layer.depth, layer.origin.to_string()));
    layers
        .into_iter()
        .map(|layer| ScopeResolution {
            kind: layer.kind,
            depth: layer.depth,
            origin: layer.origin.to_string(),
        })
        .collect()
}
