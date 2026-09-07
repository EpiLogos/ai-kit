//! Shared admission-census helpers for the dispatch-projection adapters.
//!
//! The architecture law (PROGRAMME §3.1): Actuation owns what each harness
//! IS; AIKit owns what it does about it. These adapters admit through the
//! harness-adapter contract (`aikit.harness-adapter/v1`) while their dispatch
//! facts — native events, seams, blocking, wake — stay owned by Actuation's
//! capability descriptor and are consumed at install time. The census
//! therefore cites the descriptor as evidence; it never restates the facts as
//! its own source.

use aikit_core::harness_admission::{FacultySupport, HarnessFacultyObservation};

use crate::actuation_harness_capability::HarnessCapability;

/// The stable evidence ref for an Actuation capability descriptor intake.
pub fn descriptor_evidence_ref(capability: &HarnessCapability) -> String {
    format!(
        "actuation:harness-capability/{}@r{}",
        capability.harness_slug,
        capability.provenance.catalog_revision.unwrap_or(0)
    )
}

/// The hook faculty as the descriptor states it — the one dispatch-relevant
/// faculty the census claims. Without an intake the faculty is honestly
/// Unknown: install refuses without a descriptor, and the census never
/// guesses one.
pub fn descriptor_session_start_hook(
    capability: &Option<HarnessCapability>,
) -> HarnessFacultyObservation {
    match capability {
        Some(capability) => {
            let supports = capability
                .native_events
                .iter()
                .any(|event| event.event == "session-start");
            HarnessFacultyObservation {
                faculty: aikit_core::harness_admission::HarnessFaculty::SessionStartHook,
                support: if supports {
                    FacultySupport::Supported
                } else {
                    FacultySupport::Unsupported
                },
                evidence_refs: vec![
                    descriptor_evidence_ref(capability),
                    format!("native:{}", capability.install_seam.config_path),
                ],
                note: Some(
                    "dispatch facts are read from Actuation's descriptor at install time; \
                     this census restates none of them"
                        .to_string(),
                ),
            }
        }
        None => HarnessFacultyObservation {
            faculty: aikit_core::harness_admission::HarnessFaculty::SessionStartHook,
            support: FacultySupport::Unknown,
            evidence_refs: Vec::new(),
            note: Some(
                "capability descriptor intake unavailable at admission time; install refuses \
                 without one rather than guess the harness"
                    .to_string(),
            ),
        },
    }
}
