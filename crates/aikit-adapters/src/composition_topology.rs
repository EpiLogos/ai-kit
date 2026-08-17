//! Target-neutral nested Component topology over a resolved HarnessComposition.
//!
//! The base core resolver deliberately models selection, requirements, providers,
//! contributions, scopes and surfaces. Rich plugin targets also need to preserve
//! *containment*: a plugin may be mounted inside another plugin/group while both
//! retain independent Component identity. This module adds that one reusable
//! distinction without turning any target's loader tree into AIKit ontology.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, HarnessComposition, Result};
use serde::{Deserialize, Serialize};

pub const HARNESS_COMPOSITION_TOPOLOGY_VERSION: &str = "aikit.harness-composition-topology/v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ComponentContainment {
    pub parent: ResourceRef,
    pub child: ResourceRef,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl ComponentContainment {
    pub fn new(parent: ResourceRef, child: ResourceRef) -> Self {
        Self {
            parent,
            child,
            provenance: Vec::new(),
        }
    }

    pub fn with_provenance(mut self, provenance: impl Into<String>) -> Self {
        self.provenance.push(provenance.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCompositionTopology {
    pub composition_fingerprint: String,
    pub roots: Vec<ResourceRef>,
    pub containments: Vec<ComponentContainment>,
}

impl HarnessCompositionTopology {
    pub fn children_of(&self, parent: &ResourceRef) -> Vec<&ResourceRef> {
        self.containments
            .iter()
            .filter(|edge| &edge.parent == parent)
            .map(|edge| &edge.child)
            .collect()
    }

    pub fn parent_of(&self, child: &ResourceRef) -> Option<&ResourceRef> {
        self.containments
            .iter()
            .find(|edge| &edge.child == child)
            .map(|edge| &edge.parent)
    }
}

/// Validate and normalize target-native parent/child relations against the
/// already-resolved body. Both endpoints must be selected Components, each child
/// has at most one parent, and cycles are rejected. Roots are derived rather than
/// declared so target adapters cannot silently create a second selection truth.
pub fn resolve_component_topology(
    composition: &HarnessComposition,
    mut containments: Vec<ComponentContainment>,
) -> Result<HarnessCompositionTopology> {
    let mounted = composition
        .component_bindings
        .iter()
        .map(|binding| binding.component.clone())
        .collect::<BTreeSet<_>>();

    containments.sort();
    containments.dedup();

    let mut parent_by_child = BTreeMap::<ResourceRef, ResourceRef>::new();
    for edge in &containments {
        if edge.parent == edge.child {
            return Err(AikitError::new(
                "composition.containment_self_cycle",
                format!("component {} cannot contain itself", edge.child),
            ));
        }
        for endpoint in [&edge.parent, &edge.child] {
            if !mounted.contains(endpoint) {
                return Err(AikitError::new(
                    "composition.containment_unmounted_component",
                    format!(
                        "containment endpoint {endpoint} is not mounted in this HarnessComposition"
                    ),
                ));
            }
        }
        if let Some(previous) = parent_by_child.insert(edge.child.clone(), edge.parent.clone()) {
            if previous != edge.parent {
                return Err(AikitError::new(
                    "composition.containment_multiple_parents",
                    format!(
                        "component {} is contained by both {} and {}",
                        edge.child, previous, edge.parent
                    ),
                ));
            }
        }
    }

    for component in &mounted {
        let mut seen = BTreeSet::new();
        let mut cursor = component;
        while let Some(parent) = parent_by_child.get(cursor) {
            if !seen.insert(cursor.clone()) {
                return Err(AikitError::new(
                    "composition.containment_cycle",
                    format!("component containment cycle reaches {cursor}"),
                ));
            }
            cursor = parent;
        }
    }

    let roots = mounted
        .iter()
        .filter(|component| !parent_by_child.contains_key(*component))
        .cloned()
        .collect::<Vec<_>>();

    Ok(HarnessCompositionTopology {
        composition_fingerprint: composition.fingerprint.clone(),
        roots,
        containments,
    })
}
