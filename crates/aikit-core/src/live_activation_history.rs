//! Projection of provider-confirmed live activation observations into History.
//!
//! The source observation is owned by `SessionSpaceActivationDriver`: core cannot
//! synthesize `Active`. This module therefore accepts an observation that already
//! happened and classifies it as `Observed`; it does not execute a provider,
//! persist an event or infer live state from a Generation/Procedure/preview.

use std::collections::BTreeMap;

use crate::explain_history::{EvidenceProvenance, HistoryEvidence, HistoryKind, HistoryRecoverability};
use crate::resource::{ResourceRef, SourceAuthority};
use crate::session_space::{SessionSpaceActivationObservation, SessionSpaceRef};
use crate::{Result, EXPLAIN_HISTORY_VERSION};

pub fn live_activation_history_evidence(
    space: &SessionSpaceRef,
    agent_session: &ResourceRef,
    component: &ResourceRef,
    composition_fingerprint: &str,
    observation: &SessionSpaceActivationObservation,
    observed_at_unix_ms: u128,
) -> Result<HistoryEvidence> {
    let (provider, state, reason, provenance) = match observation {
        SessionSpaceActivationObservation::Active {
            provider,
            provenance,
        } => (provider, "active", None, provenance),
        SessionSpaceActivationObservation::Deactivated {
            provider,
            provenance,
        } => (provider, "deactivated", None, provenance),
        SessionSpaceActivationObservation::Degraded {
            provider,
            reason,
            provenance,
        } => (provider, "degraded", Some(reason.as_str()), provenance),
        SessionSpaceActivationObservation::Unavailable {
            provider,
            reason,
            provenance,
        } => (provider, "unavailable", Some(reason.as_str()), provenance),
    };

    let mut canonical_refs = vec![
        space.as_resource_ref().clone(),
        agent_session.clone(),
        component.clone(),
        provider.clone(),
    ];
    canonical_refs.sort();
    canonical_refs.dedup();

    let mut details = BTreeMap::new();
    details.insert(
        "compositionFingerprint".into(),
        composition_fingerprint.to_string(),
    );
    details.insert("activationState".into(), state.into());
    if let Some(reason) = reason {
        details.insert("reason".into(), reason.into());
    }
    if !provenance.is_empty() {
        details.insert("providerProvenance".into(), provenance.join(" | "));
    }

    Ok(HistoryEvidence {
        schema: EXPLAIN_HISTORY_VERSION.into(),
        id: format!(
            "live-activation:{}:{}:{}:{observed_at_unix_ms}",
            space, agent_session, component
        ),
        kind: HistoryKind::LiveActivation,
        subject: component.clone(),
        authorities: vec![SourceAuthority::Observed],
        occurred_at_unix_ms: Some(observed_at_unix_ms),
        summary: match reason {
            Some(reason) => format!(
                "provider {provider} observed {component} {state} for body {composition_fingerprint}: {reason}"
            ),
            None => format!(
                "provider {provider} observed {component} {state} for body {composition_fingerprint}"
            ),
        },
        canonical_refs,
        provenance: vec![EvidenceProvenance {
            provider: Some(provider.clone()),
            ..EvidenceProvenance::default()
        }],
        // An old provider observation is evidence, not a portable restore recipe.
        // Re-activation belongs to the current provider/SessionSpace operation.
        recoverability: HistoryRecoverability::NotRecoverable,
        details,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    #[test]
    fn provider_active_is_observed_live_history_not_generated_or_derived_truth() {
        let space = SessionSpaceRef::parse("session-space/work").unwrap();
        let observation = SessionSpaceActivationObservation::Active {
            provider: r("provider/cmux"),
            provenance: vec!["provider-confirmed".into()],
        };
        let evidence = live_activation_history_evidence(
            &space,
            &r("agent-session/one"),
            &r("component/editor"),
            "body-42",
            &observation,
            100,
        )
        .unwrap();

        assert_eq!(evidence.kind, HistoryKind::LiveActivation);
        assert_eq!(evidence.authorities, vec![SourceAuthority::Observed]);
        assert_eq!(evidence.recoverability, HistoryRecoverability::NotRecoverable);
        assert_eq!(
            evidence.details.get("compositionFingerprint"),
            Some(&"body-42".to_string())
        );
        assert!(evidence.canonical_refs.contains(&r("provider/cmux")));
        assert!(evidence.canonical_refs.contains(&r("component/editor")));
    }

    #[test]
    fn degraded_observation_remains_provider_observed_with_reason() {
        let space = SessionSpaceRef::parse("session-space/work").unwrap();
        let observation = SessionSpaceActivationObservation::Degraded {
            provider: r("provider/tmux"),
            reason: "pane disappeared".into(),
            provenance: vec!["tmux-list-panes".into()],
        };
        let evidence = live_activation_history_evidence(
            &space,
            &r("agent-session/one"),
            &r("component/terminal"),
            "body-43",
            &observation,
            101,
        )
        .unwrap();
        assert_eq!(evidence.authorities, vec![SourceAuthority::Observed]);
        assert_eq!(evidence.details.get("activationState"), Some(&"degraded".to_string()));
        assert_eq!(evidence.details.get("reason"), Some(&"pane disappeared".to_string()));
    }
}
