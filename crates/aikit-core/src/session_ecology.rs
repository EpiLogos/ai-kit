//! Authorised ecology reading over the existing SessionSpace runtime.
//!
//! This module does not create a second session registry. It composes the
//! canonical SessionSpace authored/runtime state with externally-owned
//! Agent/Agency/Actuation/ActuationStream refs, explicit session lineage evidence,
//! existing Surface material observations and explicit invocation authority.
//!
//! The resulting reading answers a bounded question for humans and agents:
//! "what other situated sessions are present in this working world, how are they
//! currently embodied/reachable, and which relations may I actually invoke?"

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::resource::ResourceRef;
use crate::session_space::{
    SessionSpaceAgentSession, SessionSpaceAuthorityState, SessionSpaceConnection,
    SessionSpaceReadModel, SessionSpaceRef, SessionSpaceSurfaceReading,
};
use crate::session_space_application::{SessionSpaceAuthoredState, SessionSpaceFocus};
use crate::surface_material::SurfaceMaterialObservation;
use crate::{AikitError, Result};

pub const SESSION_ECOLOGY_VERSION: &str = "aikit.session-ecology/v1";

/// Cross-product semantic refs correlated to one canonical AgentSession.
///
/// AIKit does not acquire ownership of Agent, Agency, Actuation or ActuationStream
/// by carrying these refs. They are supplied by their native owners and remain
/// independently addressable identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionSemanticBinding {
    pub agent_session: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actuation: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actuation_stream: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// Why the current AgentSession/context condition relates to an earlier one.
///
/// This is lineage evidence, not mutation of historical ActuationStream material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentSessionLineageRelation {
    Root,
    Continue,
    Refine,
    Fork,
    Recompose,
}

/// Explicit continuity/context lineage around one AgentSession.
///
/// `context_revision_ref` should normally cite exact ContextResolution evidence
/// already produced by AIKit. Provider-native cache/session handles remain opaque
/// evidence and never become canonical AgentSession identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionLineage {
    pub agent_session: ResourceRef,
    pub relation: AgentSessionLineageRelation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_revision_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_agent_session: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_actuation_stream: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_stream_sequence: Option<u64>,
    #[serde(default)]
    pub provider_continuation_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl AgentSessionLineage {
    pub fn validate(&self) -> Result<()> {
        match self.relation {
            AgentSessionLineageRelation::Root => {
                if self.parent_agent_session.is_some()
                    || self.parent_actuation_stream.is_some()
                    || self.parent_stream_sequence.is_some()
                {
                    return Err(AikitError::new(
                        "session_ecology.root_has_parent",
                        format!(
                            "root AgentSession lineage {} cannot declare a parent session/Stream position",
                            self.agent_session
                        ),
                    ));
                }
            }
            AgentSessionLineageRelation::Continue
            | AgentSessionLineageRelation::Refine
            | AgentSessionLineageRelation::Fork
            | AgentSessionLineageRelation::Recompose => {
                if self.parent_agent_session.is_none() && self.parent_actuation_stream.is_none() {
                    return Err(AikitError::new(
                        "session_ecology.lineage_missing_parent",
                        format!(
                            "{:?} lineage for {} needs a parent AgentSession or ActuationStream ref",
                            self.relation, self.agent_session
                        ),
                    ));
                }
            }
        }
        if self.parent_stream_sequence.is_some() && self.parent_actuation_stream.is_none() {
            return Err(AikitError::new(
                "session_ecology.sequence_without_stream",
                format!(
                    "AgentSession {} cites a parent Stream sequence without a parent ActuationStream",
                    self.agent_session
                ),
            ));
        }
        Ok(())
    }
}

/// Native relations through which one situated session may address another.
///
/// A2A can project these relations externally, but this enum describes the
/// richer relation visible when both sessions inhabit the same authorised AIKit
/// working field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionInvocationMode {
    Communique,
    SessionContribution,
    Delegation,
    SessionFork,
    CoActuation,
}

/// Explicit authority-bearing relation from one AgentSession to another.
/// Presence of the target session never creates this relation by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInvocationRelation {
    pub source_agent_session: ResourceRef,
    pub target_agent_session: ResourceRef,
    pub mode: SessionInvocationMode,
    pub authority: SessionSpaceAuthorityState,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl SessionInvocationRelation {
    pub fn invocable(&self) -> bool {
        self.authority.has_authority()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionEcologyPresence {
    AuthoredIntent,
    RuntimeObserved,
    RuntimeOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEcologySurfaceReading {
    pub reading: SessionSpaceSurfaceReading,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<SurfaceMaterialObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInvocationReading {
    pub target_agent_session: ResourceRef,
    pub mode: SessionInvocationMode,
    pub authority: SessionSpaceAuthorityState,
    pub invocable: bool,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionEcologyReading {
    pub agent_session: ResourceRef,
    pub presence: SessionEcologyPresence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_binding: Option<SessionSpaceAgentSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<AgentSessionSemanticBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<AgentSessionLineage>,
    #[serde(default)]
    pub surfaces: Vec<SessionEcologySurfaceReading>,
    #[serde(default)]
    pub connections: Vec<SessionSpaceConnection>,
    #[serde(default)]
    pub invocations: Vec<SessionInvocationReading>,
}

/// Read-only ecology of one existing SessionSpace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEcologyReadModel {
    pub version: String,
    pub space: SessionSpaceRef,
    pub authored_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<SessionSpaceFocus>,
    #[serde(default)]
    pub sessions: Vec<AgentSessionEcologyReading>,
}

/// Compose the current SessionSpace ecology without creating new semantic state.
pub fn disclose_session_ecology(
    authored: &SessionSpaceAuthoredState,
    runtime: Option<&SessionSpaceReadModel>,
    semantic_bindings: &[AgentSessionSemanticBinding],
    lineages: &[AgentSessionLineage],
    material_observations: &[SurfaceMaterialObservation],
    invocation_relations: &[SessionInvocationRelation],
) -> Result<SessionEcologyReadModel> {
    authored.validate()?;
    if let Some(runtime) = runtime {
        if runtime.id != *authored.id() {
            return Err(AikitError::new(
                "session_ecology.space_identity_mismatch",
                format!(
                    "runtime SessionSpace {} cannot be disclosed as authored SessionSpace {}",
                    runtime.id,
                    authored.id()
                ),
            ));
        }
    }

    let authored_sessions = authored.agent_sessions.keys().cloned().collect::<BTreeSet<_>>();
    let runtime_sessions = runtime
        .into_iter()
        .flat_map(|runtime| runtime.agent_sessions.iter().map(|binding| binding.agent_session.clone()))
        .collect::<BTreeSet<_>>();
    let known_sessions = authored_sessions
        .union(&runtime_sessions)
        .cloned()
        .collect::<BTreeSet<_>>();

    let semantics = unique_by_session(
        semantic_bindings,
        |binding| &binding.agent_session,
        "semantic binding",
    )?;
    for session in semantics.keys() {
        require_known_session(session, &known_sessions, "semantic binding")?;
    }

    let lineage = unique_by_session(lineages, |lineage| &lineage.agent_session, "lineage")?;
    for item in lineage.values() {
        require_known_session(&item.agent_session, &known_sessions, "lineage")?;
        item.validate()?;
    }

    let mut material_by_surface = BTreeMap::<ResourceRef, SurfaceMaterialObservation>::new();
    for observation in material_observations {
        if material_by_surface
            .insert(observation.surface.clone(), observation.clone())
            .is_some()
        {
            return Err(AikitError::new(
                "session_ecology.duplicate_surface_material",
                format!(
                    "Surface {} has more than one material observation in one ecology reading",
                    observation.surface
                ),
            ));
        }
    }

    let mut invocations_by_source = BTreeMap::<ResourceRef, Vec<SessionInvocationReading>>::new();
    let mut invocation_keys = BTreeSet::new();
    for relation in invocation_relations {
        require_known_session(
            &relation.source_agent_session,
            &known_sessions,
            "invocation source",
        )?;
        require_known_session(
            &relation.target_agent_session,
            &known_sessions,
            "invocation target",
        )?;
        if relation.source_agent_session == relation.target_agent_session {
            return Err(AikitError::new(
                "session_ecology.self_invocation",
                format!(
                    "AgentSession {} cannot advertise itself as another session invocation target",
                    relation.source_agent_session
                ),
            ));
        }
        let key = (
            relation.source_agent_session.clone(),
            relation.target_agent_session.clone(),
            relation.mode,
        );
        if !invocation_keys.insert(key) {
            return Err(AikitError::new(
                "session_ecology.duplicate_invocation",
                format!(
                    "duplicate {:?} relation from {} to {}",
                    relation.mode, relation.source_agent_session, relation.target_agent_session
                ),
            ));
        }
        invocations_by_source
            .entry(relation.source_agent_session.clone())
            .or_default()
            .push(SessionInvocationReading {
                target_agent_session: relation.target_agent_session.clone(),
                mode: relation.mode,
                authority: relation.authority.clone(),
                invocable: relation.invocable(),
                provenance: relation.provenance.clone(),
            });
    }
    for invocations in invocations_by_source.values_mut() {
        invocations.sort_by(|left, right| {
            left.target_agent_session
                .cmp(&right.target_agent_session)
                .then_with(|| left.mode.cmp(&right.mode))
        });
    }

    let mut sessions = Vec::new();
    for session in known_sessions {
        let authored_intent = authored.agent_sessions.get(&session);
        let runtime_binding = runtime.and_then(|runtime| {
            runtime
                .agent_sessions
                .iter()
                .find(|binding| binding.agent_session == session)
                .cloned()
        });
        let presence = match (authored_intent.is_some(), runtime_binding.is_some()) {
            (true, true) => SessionEcologyPresence::RuntimeObserved,
            (true, false) => SessionEcologyPresence::AuthoredIntent,
            (false, true) => SessionEcologyPresence::RuntimeOnly,
            (false, false) => unreachable!("known session came from authored/runtime union"),
        };

        let mut surfaces = runtime
            .into_iter()
            .flat_map(|runtime| runtime.surfaces.iter())
            .filter(|surface| surface.agent_session == session)
            .map(|surface| SessionEcologySurfaceReading {
                reading: surface.clone(),
                material: material_by_surface.get(&surface.surface).cloned(),
            })
            .collect::<Vec<_>>();
        surfaces.sort_by(|left, right| left.reading.surface.cmp(&right.reading.surface));

        let mut connections = runtime
            .into_iter()
            .flat_map(|runtime| runtime.connections.iter())
            .filter(|connection| connection.agent_session == session)
            .cloned()
            .collect::<Vec<_>>();
        connections.sort_by(|left, right| left.connection.cmp(&right.connection));

        sessions.push(AgentSessionEcologyReading {
            agent_session: session.clone(),
            presence,
            purpose: authored_intent.and_then(|intent| intent.purpose.clone()),
            runtime_binding,
            semantic: semantics.get(&session).cloned().cloned(),
            lineage: lineage.get(&session).cloned().cloned(),
            surfaces,
            connections,
            invocations: invocations_by_source.remove(&session).unwrap_or_default(),
        });
    }
    sessions.sort_by(|left, right| left.agent_session.cmp(&right.agent_session));

    Ok(SessionEcologyReadModel {
        version: SESSION_ECOLOGY_VERSION.into(),
        space: authored.id().clone(),
        authored_revision: authored.revision,
        runtime_revision: runtime.map(|runtime| runtime.revision),
        focus: authored.focus.clone(),
        sessions,
    })
}

fn unique_by_session<'a, T, F>(
    values: &'a [T],
    session: F,
    label: &str,
) -> Result<BTreeMap<ResourceRef, &'a T>>
where
    F: Fn(&T) -> &ResourceRef,
{
    let mut indexed = BTreeMap::new();
    for value in values {
        let key = session(value).clone();
        if indexed.insert(key.clone(), value).is_some() {
            return Err(AikitError::new(
                "session_ecology.duplicate_session_evidence",
                format!("AgentSession {key} has more than one {label}"),
            ));
        }
    }
    Ok(indexed)
}

fn require_known_session(
    session: &ResourceRef,
    known: &BTreeSet<ResourceRef>,
    relation: &str,
) -> Result<()> {
    if known.contains(session) {
        return Ok(());
    }
    Err(AikitError::new(
        "session_ecology.unknown_agent_session",
        format!("{relation} refers to AgentSession {session} outside this SessionSpace"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_space::{
        SessionSpaceLifecycle, SessionSpaceReadModel,
    };
    use crate::session_space_application::SessionSpaceAgentAttachmentIntent;

    fn r(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn authored() -> SessionSpaceAuthoredState {
        let mut state = SessionSpaceAuthoredState::new(SessionSpaceRef::parse("session-space/main").unwrap());
        state.revision = 7;
        state.agent_sessions.insert(
            r("agent-session/a"),
            SessionSpaceAgentAttachmentIntent {
                agent_session: r("agent-session/a"),
                purpose: Some("develop gateway".into()),
                provenance: vec!["authored fixture".into()],
            },
        );
        state
    }

    fn runtime() -> SessionSpaceReadModel {
        SessionSpaceReadModel {
            version: crate::session_space::SESSION_SPACE_VERSION.into(),
            id: SessionSpaceRef::parse("session-space/main").unwrap(),
            lifecycle: SessionSpaceLifecycle::Open,
            revision: 11,
            projects: Vec::new(),
            agent_sessions: vec![
                SessionSpaceAgentSession {
                    agent_session: r("agent-session/a"),
                    harness: r("harness/codex"),
                    native_session_id: Some("provider-session-a".into()),
                    provider: Some(r("provider/codex")),
                    provenance: vec!["runtime fixture".into()],
                },
                SessionSpaceAgentSession {
                    agent_session: r("agent-session/b"),
                    harness: r("harness/pi"),
                    native_session_id: Some("provider-session-b".into()),
                    provider: Some(r("provider/pi")),
                    provenance: vec!["runtime fixture".into()],
                },
            ],
            components: Vec::new(),
            surfaces: Vec::new(),
            connections: Vec::new(),
            provenance: Vec::new(),
        }
    }

    fn semantics() -> Vec<AgentSessionSemanticBinding> {
        vec![AgentSessionSemanticBinding {
            agent_session: r("agent-session/a"),
            agent: Some(r("agent/root")),
            agency: Some(r("agency/root")),
            actuation: Some(r("actuation/root")),
            actuation_stream: Some(r("actuation-stream/root")),
            focus: Some(r("focus/gateway")),
            provenance: vec!["Actuation owner refs".into()],
        }]
    }

    #[test]
    fn ecology_unions_authored_and_runtime_sessions_without_collapsing_native_identity() {
        let reading = disclose_session_ecology(
            &authored(),
            Some(&runtime()),
            &semantics(),
            &[],
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(reading.version, SESSION_ECOLOGY_VERSION);
        assert_eq!(reading.sessions.len(), 2);
        let a = &reading.sessions[0];
        assert_eq!(a.agent_session, r("agent-session/a"));
        assert_eq!(a.presence, SessionEcologyPresence::RuntimeObserved);
        assert_eq!(a.purpose.as_deref(), Some("develop gateway"));
        assert_eq!(
            a.runtime_binding.as_ref().unwrap().native_session_id.as_deref(),
            Some("provider-session-a")
        );
        assert_eq!(
            a.semantic.as_ref().unwrap().actuation_stream.as_ref(),
            Some(&r("actuation-stream/root"))
        );
        let b = &reading.sessions[1];
        assert_eq!(b.presence, SessionEcologyPresence::RuntimeOnly);
        assert!(b.semantic.is_none());
    }

    #[test]
    fn presence_never_implies_invocation_authority() {
        let denied = SessionInvocationRelation {
            source_agent_session: r("agent-session/a"),
            target_agent_session: r("agent-session/b"),
            mode: SessionInvocationMode::Delegation,
            authority: SessionSpaceAuthorityState {
                capability: Some(r("capability/delegate")),
                capability_available: true,
                capability_granted: false,
                action: Some(r("action/delegate")),
                action_authorised: false,
                provenance: vec!["policy denied".into()],
            },
            provenance: vec!["explicit relation".into()],
        };
        let reading = disclose_session_ecology(
            &authored(),
            Some(&runtime()),
            &[],
            &[],
            &[],
            &[denied],
        )
        .unwrap();
        assert_eq!(reading.sessions[0].invocations.len(), 1);
        assert!(!reading.sessions[0].invocations[0].invocable);
    }

    #[test]
    fn explicit_grant_makes_one_relation_invocable_without_granting_other_modes() {
        let granted = SessionInvocationRelation {
            source_agent_session: r("agent-session/a"),
            target_agent_session: r("agent-session/b"),
            mode: SessionInvocationMode::Communique,
            authority: SessionSpaceAuthorityState {
                capability: Some(r("capability/communique")),
                capability_available: true,
                capability_granted: true,
                action: Some(r("action/communique")),
                action_authorised: true,
                provenance: vec!["policy grant".into()],
            },
            provenance: Vec::new(),
        };
        let reading = disclose_session_ecology(
            &authored(),
            Some(&runtime()),
            &[],
            &[],
            &[],
            &[granted],
        )
        .unwrap();
        assert!(reading.sessions[0].invocations[0].invocable);
        assert_eq!(
            reading.sessions[0].invocations[0].mode,
            SessionInvocationMode::Communique
        );
        assert_eq!(reading.sessions[0].invocations.len(), 1);
    }

    #[test]
    fn refine_and_fork_lineage_cite_prior_session_stream_and_exact_context_revision() {
        let lineages = vec![AgentSessionLineage {
            agent_session: r("agent-session/b"),
            relation: AgentSessionLineageRelation::Refine,
            context_revision_ref: Some(r("context-resolution/next")),
            parent_agent_session: Some(r("agent-session/a")),
            parent_actuation_stream: Some(r("actuation-stream/root")),
            parent_stream_sequence: Some(84),
            provider_continuation_refs: vec![r("provider-continuation/cache-prefix-7")],
            provenance: vec!["explicit human refinement".into()],
        }];
        let reading = disclose_session_ecology(
            &authored(),
            Some(&runtime()),
            &[],
            &lineages,
            &[],
            &[],
        )
        .unwrap();
        let b = reading
            .sessions
            .iter()
            .find(|session| session.agent_session == r("agent-session/b"))
            .unwrap();
        let lineage = b.lineage.as_ref().unwrap();
        assert_eq!(lineage.parent_stream_sequence, Some(84));
        assert_eq!(
            lineage.context_revision_ref.as_ref(),
            Some(&r("context-resolution/next"))
        );
    }

    #[test]
    fn stream_sequence_without_stream_is_rejected() {
        let invalid = AgentSessionLineage {
            agent_session: r("agent-session/b"),
            relation: AgentSessionLineageRelation::Fork,
            context_revision_ref: None,
            parent_agent_session: Some(r("agent-session/a")),
            parent_actuation_stream: None,
            parent_stream_sequence: Some(12),
            provider_continuation_refs: Vec::new(),
            provenance: Vec::new(),
        };
        let error = disclose_session_ecology(
            &authored(),
            Some(&runtime()),
            &[],
            &[invalid],
            &[],
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code(), "session_ecology.sequence_without_stream");
    }

    #[test]
    fn evidence_cannot_smuggle_an_unknown_session_into_the_ecology() {
        let unknown = AgentSessionSemanticBinding {
            agent_session: r("agent-session/elsewhere"),
            agent: None,
            agency: None,
            actuation: None,
            actuation_stream: None,
            focus: None,
            provenance: Vec::new(),
        };
        let error = disclose_session_ecology(
            &authored(),
            Some(&runtime()),
            &[unknown],
            &[],
            &[],
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code(), "session_ecology.unknown_agent_session");
    }
}
