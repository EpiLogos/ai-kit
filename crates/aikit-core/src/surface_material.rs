//! Persistent Surface ↔ material binding disclosure for composable runtimes.
//!
//! AIKit owns the semantic relation from Project/Agent/Harness/AgentSession to
//! Surfaces. Workcell owns processes, services, endpoints, network placement and
//! lifecycle. This module joins those observations without turning a process,
//! gateway, daemon, URL, port, socket or Workcell allocation into Surface or Agent
//! identity.
//!
//! Target-native management remains target-native. Hermes and OpenClaw are source-
//! pinned conformance fixtures here, not implementations of a fabricated common
//! gateway protocol.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::composition::{HarnessComposition, SurfaceKind};
use crate::resource::{ResourceKind, ResourceRef};
use crate::{AikitError, Result};

pub const SURFACE_MATERIAL_VERSION: &str = "aikit.surface-material/v1";

/// Accepted Workcell source used by the collapsed-local pre-physical proof.
pub const WORKCELL_COLLAPSED_LOCAL_CONFORMANCE_REVISION: &str =
    "8184f9ba95c5efc0077701a4b58c1abb2943bd6e";

/// Current upstream source pins inspected for target-native persistent-host
/// management semantics. They are evidence, never semantic identities.
pub const HERMES_GATEWAY_CONFORMANCE_REVISION: &str = "41a80d52518c1c391d62c8d1852bdce83593b751";
pub const OPENCLAW_GATEWAY_CONFORMANCE_REVISION: &str = "49cdd54259291ae466244a110c3c39eb12b47568";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceMaterialHealth {
    Healthy,
    Degraded,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceAccessObservation {
    Authenticated,
    Unauthenticated,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceExposureScope {
    Local,
    Private,
    Public,
    Unknown,
}

/// A provider/material observation associated with one already-resolved Surface.
///
/// `logical_service_ref` is the caller/Workcell logical connectivity requirement;
/// `material_ref`, provider, endpoint and process facts describe the current
/// materialisation. Rebinding any of those fields must not mint a new Surface,
/// Agent, Harness or AgentSession.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceMaterialObservation {
    pub surface: ResourceRef,
    pub logical_service_ref: String,
    pub material_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workcell_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    pub health: SurfaceMaterialHealth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reachable: Option<bool>,
    pub access: SurfaceAccessObservation,
    pub exposure: SurfaceExposureScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<String>,
    /// PID, supervisor label, native gateway identity, state path and similar facts
    /// stay provenance. Consumers must never promote these values to semantic refs.
    #[serde(default)]
    pub provenance: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceMaterialReading {
    pub surface: ResourceRef,
    pub kind: SurfaceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_native_surface: Option<String>,
    #[serde(default)]
    pub action_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub non_action_refs: Vec<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<SurfaceMaterialObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceMaterialAbsence {
    pub surface: ResourceRef,
    pub reason: String,
}

/// UI-neutral current binding disclosure for CLI/TUI/agent consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentSurfaceReadModel {
    pub version: String,
    pub project: Option<ResourceRef>,
    pub agent: Option<ResourceRef>,
    pub agency: Option<ResourceRef>,
    pub harness: ResourceRef,
    pub agent_session: Option<String>,
    pub harness_composition_fingerprint: String,
    #[serde(default)]
    pub surfaces: Vec<SurfaceMaterialReading>,
    #[serde(default)]
    pub unavailable: Vec<SurfaceMaterialAbsence>,
}

/// Attach current Workcell/provider observations to an already-resolved Harness
/// composition. The function is intentionally read-only: material lifecycle remains
/// owned by Workcell and target-native managers.
pub fn disclose_surface_material(
    composition: &HarnessComposition,
    observations: impl IntoIterator<Item = SurfaceMaterialObservation>,
) -> Result<PersistentSurfaceReadModel> {
    let descriptors = composition
        .surfaces
        .iter()
        .map(|surface| (surface.resource.clone(), surface))
        .collect::<BTreeMap<_, _>>();

    let mut observed = BTreeMap::<ResourceRef, SurfaceMaterialObservation>::new();
    let mut material_refs = BTreeSet::<String>::new();
    for observation in observations {
        if !descriptors.contains_key(&observation.surface) {
            return Err(AikitError::new(
                "surface_material.unknown_surface",
                format!(
                    "material observation refers to Surface {} outside this HarnessComposition",
                    observation.surface
                ),
            )
            .with("surface", observation.surface.to_string()));
        }
        if observation.logical_service_ref.trim().is_empty() {
            return Err(AikitError::new(
                "surface_material.empty_logical_service",
                format!(
                    "Surface {} has an empty logical service ref",
                    observation.surface
                ),
            ));
        }
        if observation.material_ref.trim().is_empty() {
            return Err(AikitError::new(
                "surface_material.empty_material_ref",
                format!("Surface {} has an empty material ref", observation.surface),
            ));
        }
        if !material_refs.insert(observation.material_ref.clone()) {
            return Err(AikitError::new(
                "surface_material.duplicate_material_ref",
                format!(
                    "material ref {} is attached to more than one effective Surface binding",
                    observation.material_ref
                ),
            ));
        }
        let surface = observation.surface.clone();
        if observed.insert(surface.clone(), observation).is_some() {
            return Err(AikitError::new(
                "surface_material.duplicate_surface_binding",
                format!("Surface {surface} has more than one effective material binding"),
            )
            .with("surface", surface.to_string()));
        }
    }

    let mut unavailable = Vec::new();
    let mut surfaces = composition
        .surfaces
        .iter()
        .map(|descriptor| {
            let mut action_refs = Vec::new();
            let mut non_action_refs = Vec::new();
            for projection in composition
                .projections
                .iter()
                .filter(|projection| projection.surface == descriptor.resource)
            {
                if projection.canonical_kind == ResourceKind::Action {
                    action_refs.push(projection.canonical_ref.clone());
                } else {
                    non_action_refs.push(projection.canonical_ref.clone());
                }
            }
            action_refs.sort();
            non_action_refs.sort();

            let material = observed.remove(&descriptor.resource);
            match material.as_ref() {
                None => unavailable.push(SurfaceMaterialAbsence {
                    surface: descriptor.resource.clone(),
                    reason: "no current material binding observation was supplied".into(),
                }),
                Some(observation) if observation.health == SurfaceMaterialHealth::Unavailable => {
                    unavailable.push(SurfaceMaterialAbsence {
                        surface: descriptor.resource.clone(),
                        reason: observation
                            .provenance
                            .get("unavailable_reason")
                            .cloned()
                            .unwrap_or_else(|| "current material binding is unavailable".into()),
                    });
                }
                Some(observation)
                    if observation.access == SurfaceAccessObservation::Unavailable =>
                {
                    unavailable.push(SurfaceMaterialAbsence {
                        surface: descriptor.resource.clone(),
                        reason: "current material binding exists but access is unavailable".into(),
                    });
                }
                Some(_) => {}
            }

            SurfaceMaterialReading {
                surface: descriptor.resource.clone(),
                kind: descriptor.kind,
                target_native_surface: descriptor.target_native_id.clone(),
                action_refs,
                non_action_refs,
                material,
            }
        })
        .collect::<Vec<_>>();

    surfaces.sort_by(|left, right| left.surface.cmp(&right.surface));
    unavailable.sort_by(|left, right| left.surface.cmp(&right.surface));

    Ok(PersistentSurfaceReadModel {
        version: SURFACE_MATERIAL_VERSION.into(),
        project: composition.project.clone(),
        agent: composition.agent.clone(),
        agency: composition.agency.clone(),
        harness: composition.harness.clone(),
        agent_session: composition.session.clone(),
        harness_composition_fingerprint: composition.fingerprint.clone(),
        surfaces,
        unavailable,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistentServiceTargetFixture {
    pub target: &'static str,
    pub upstream_revision: &'static str,
    pub management_entrypoint: &'static str,
    pub lifecycle_operations: &'static [&'static str],
    pub native_surfaces: &'static [&'static str],
    pub material_shape: &'static str,
}

/// Comparative source-pinned target semantics. The differing management entrypoints,
/// lifecycle vocabularies and native surfaces are deliberate evidence that AIKit is
/// not normalising these targets into a universal gateway protocol.
pub fn persistent_service_target_fixtures() -> [PersistentServiceTargetFixture; 2] {
    [
        PersistentServiceTargetFixture {
            target: "hermes-agent",
            upstream_revision: HERMES_GATEWAY_CONFORMANCE_REVISION,
            management_entrypoint: "scripts/hermes-gateway",
            lifecycle_operations: &["run", "start", "stop", "restart", "status"],
            native_surfaces: &["messaging-platform-gateway"],
            material_shape: "standalone messaging gateway process with target-native systemd/launchd management",
        },
        PersistentServiceTargetFixture {
            target: "openclaw",
            upstream_revision: OPENCLAW_GATEWAY_CONFORMANCE_REVISION,
            management_entrypoint: "openclaw gateway",
            lifecycle_operations: &["status", "install", "restart", "stop"],
            native_surfaces: &[
                "websocket-control-rpc",
                "http-api",
                "control-ui",
                "hooks",
                "messaging-channels",
            ],
            material_shape: "one always-on multiplexed gateway process with target-native auth, reload and supervision semantics",
        },
    ]
}
