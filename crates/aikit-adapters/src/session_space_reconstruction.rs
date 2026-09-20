//! Adapter from the landed #63/#66 working-environment evidence seam into the
//! #62 SessionSpace reconstruction reading.
//!
//! The conversion preserves provider-owned identity. A provider-native id is
//! evidence/provenance only; only bindings carrying an explicit `canonical_ref`
//! can participate in SessionSpace reconstruction. In particular this adapter
//! never emits AgentSession continuity evidence.

use aikit_core::session_space_application::{ObservedRelationState, SessionSpaceNativeObservation};

use crate::working_environment::{WorkingEnvironmentHealth, WorkingEnvironmentObservation};

pub fn session_space_native_observations(
    observation: &WorkingEnvironmentObservation,
) -> Vec<SessionSpaceNativeObservation> {
    observation
        .bindings
        .iter()
        .filter_map(|binding| {
            let reference = binding.canonical_ref.clone()?;
            let state = match observation.health {
                WorkingEnvironmentHealth::Healthy => ObservedRelationState::Available,
                WorkingEnvironmentHealth::Degraded => ObservedRelationState::Degraded,
                WorkingEnvironmentHealth::Unavailable => ObservedRelationState::Unavailable,
            };
            let reason = match observation.health {
                WorkingEnvironmentHealth::Healthy => None,
                WorkingEnvironmentHealth::Degraded => Some(format!(
                    "provider {} is degraded; native binding {} remains observation only",
                    observation.provider, binding.native_id
                )),
                WorkingEnvironmentHealth::Unavailable => Some(format!(
                    "provider {} is unavailable; native binding {} does not prove recovery",
                    observation.provider, binding.native_id
                )),
            };
            Some(SessionSpaceNativeObservation {
                reference,
                state,
                provider: Some(observation.provider.clone()),
                reason,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::working_environment::{
        NativeBindingKind, ProviderNativeBinding, WorkingEnvironmentCapabilities,
        WORKING_ENVIRONMENT_PROVIDER_VERSION,
    };
    use aikit_core::resource::ResourceRef;

    #[test]
    fn native_id_without_explicit_canonical_ref_is_not_promoted_to_identity() {
        let observation = WorkingEnvironmentObservation {
            schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
            provider: ResourceRef::parse("provider/tmux").unwrap(),
            provider_version: Some("3.7c".into()),
            health: WorkingEnvironmentHealth::Healthy,
            capabilities: WorkingEnvironmentCapabilities::default(),
            bindings: vec![ProviderNativeBinding {
                kind: NativeBindingKind::AgentSession,
                native_id: "%42".into(),
                canonical_ref: None,
                provenance: vec!["tmux observation".into()],
            }],
            focused_native_id: Some("%42".into()),
            provenance: vec!["real tmux".into()],
        };
        assert!(session_space_native_observations(&observation).is_empty());
    }

    #[test]
    fn explicit_canonical_binding_is_preserved_with_provider_evidence() {
        let canonical = ResourceRef::parse("surface/editor").unwrap();
        let provider = ResourceRef::parse("provider/vscode").unwrap();
        let observation = WorkingEnvironmentObservation {
            schema: WORKING_ENVIRONMENT_PROVIDER_VERSION.into(),
            provider: provider.clone(),
            provider_version: Some("1.133.0".into()),
            health: WorkingEnvironmentHealth::Healthy,
            capabilities: WorkingEnvironmentCapabilities::default(),
            bindings: vec![ProviderNativeBinding {
                kind: NativeBindingKind::Surface,
                native_id: "editor:1".into(),
                canonical_ref: Some(canonical.clone()),
                provenance: vec!["explicit VS Code surface binding".into()],
            }],
            focused_native_id: Some("editor:1".into()),
            provenance: vec!["VS Code extension host".into()],
        };
        let readings = session_space_native_observations(&observation);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].reference, canonical);
        assert_eq!(readings[0].provider.as_ref(), Some(&provider));
        assert_eq!(readings[0].state, ObservedRelationState::Available);
    }
}
