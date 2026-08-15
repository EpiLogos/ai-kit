//! Shared widening of the shallow V2 navigation field from already-resolved facts.
//!
//! `PaletteBackend::navigation_index` supplies context-native Resources and the
//! catalogue. This layer may add only identities already proven by the resolved
//! view; it never reads project files, invokes providers, or invents preference.

use std::collections::BTreeSet;

use aikit_core::resource::{
    NavigationEvidence, NavigationEvidenceClass, ResourceDescriptor, ResourceKind, ResourceRecord,
    ResourceRef, ResourceSearchIndex,
};

use crate::backend::PaletteBackend;

pub fn resolved_navigation_index(backend: &dyn PaletteBackend) -> ResourceSearchIndex {
    let mut index = backend.navigation_index();
    let mut profiles = BTreeSet::new();

    for operation in &backend.view().selection_log {
        if let Some(profile) = operation.via_profile.as_ref() {
            profiles.insert(profile.clone());
        }
    }
    for overlays in backend.view().skill_usage_overlays.values() {
        for overlay in overlays {
            if let Some(profile) = overlay.via_profile.as_ref() {
                profiles.insert(profile.clone());
            }
        }
    }

    for profile in profiles {
        // ProfileId already renders as `profile/...`; ResourceRef therefore keeps
        // the exact AIKit-owned identity rather than manufacturing an alias.
        let Ok(resource) = ResourceRef::parse(&profile.to_string()) else {
            continue;
        };
        index.insert_resource(
            ResourceRecord::new(ResourceDescriptor::new(
                resource,
                ResourceKind::Profile,
                profile
                    .path()
                    .rsplit('/')
                    .next()
                    .unwrap_or(profile.path())
                    .to_string(),
                format!("resolved Profile · {}", profile),
            )),
            vec![NavigationEvidence::new(NavigationEvidenceClass::CurrentContext)
                .with_detail("participated in resolved scope composition")],
        );
    }

    index
}
