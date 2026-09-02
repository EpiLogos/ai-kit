//! World-relative AIKit disclosure and participation semantics for O:I inhabitation.
//!
//! This module composes existing AIKit owners rather than introducing a second
//! knowledge store, participant registry, session registry or authority system.
//! Durable knowledge remains in SourcePool/SemanticWiki/ProjectCentral; canonical
//! AgentSessions remain SessionSpace/SessionEcology identities; invocation authority
//! remains an explicit SessionInvocationRelation. The types here answer the missing
//! situated questions: what knowledge is eligible in this World, who is being
//! addressed, and exactly what another AgentSession may disclose in this encounter?

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::projectcentral::{HumanSourceRevisionProposal, ProjectCentralStanding};
use crate::resource::{ResourceRef, SourceRef, SourceRevision};
use crate::session_ecology::{SessionInvocationMode, SessionInvocationRelation};
use crate::{AikitError, Result};

pub const WORLD_KNOWLEDGE_VERSION: &str = "aikit.world-knowledge/v1";
pub const PARTICIPANT_ADDRESS_VERSION: &str = "aikit.participant-address/v1";
pub const CO_INTERNAL_DISCLOSURE_VERSION: &str = "aikit.co-internal-disclosure/v1";

/// Why a knowledge relation is available to an Agent in one World.
///
/// `PersonalGeneral` still requires an explicit World binding: personal/general
/// eligibility is not ambient prompt injection. `Inherited` preserves the source
/// World and revision that made the relation available downstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldKnowledgeStanding {
    PersonalGeneral,
    WorldLocalAgentMaintained,
    Inherited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorldKnowledgeDisclosure {
    /// The relation may be named without retrieving its payload.
    #[serde(default)]
    pub known_to_exist: bool,
    /// Eligibility in this World. This is distinct from durable source ownership.
    #[serde(default)]
    pub eligible: bool,
    /// Payload retrieval is an observed operation, never implied by eligibility.
    #[serde(default)]
    pub retrieved: bool,
}

/// One explicit Agent↔World knowledge relation over an existing knowledge/source
/// identity. `world` is the disclosure/eligibility horizon; `source_world` records
/// provenance and never becomes a second SourceRef.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldKnowledgeBinding {
    pub knowledge: ResourceRef,
    pub agent: ResourceRef,
    pub world: ResourceRef,
    pub standing: WorldKnowledgeStanding,
    pub source: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<SourceRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_world: Option<ResourceRef>,
    pub disclosure: WorldKnowledgeDisclosure,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl WorldKnowledgeBinding {
    pub fn validate(&self) -> Result<()> {
        if self.provenance.iter().any(|item| item.trim().is_empty()) {
            return Err(AikitError::new(
                "world_knowledge.invalid_provenance",
                "World knowledge provenance entries must not be empty",
            ));
        }
        match self.standing {
            WorldKnowledgeStanding::Inherited => {
                if self.source_world.is_none() || self.source_revision.is_none() {
                    return Err(AikitError::new(
                        "world_knowledge.inherited_provenance_missing",
                        format!(
                            "inherited knowledge {} requires source World and exact source revision",
                            self.knowledge
                        ),
                    ));
                }
            }
            WorldKnowledgeStanding::PersonalGeneral
            | WorldKnowledgeStanding::WorldLocalAgentMaintained => {}
        }
        Ok(())
    }
}

/// The bounded knowledge horizon for one Agent in one World.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldKnowledgeProfile {
    pub version: String,
    pub agent: ResourceRef,
    pub world: ResourceRef,
    #[serde(default)]
    pub bindings: Vec<WorldKnowledgeBinding>,
}

/// Select World-relative knowledge without retrieving any payload.
pub fn disclose_world_knowledge(
    agent: ResourceRef,
    world: ResourceRef,
    bindings: &[WorldKnowledgeBinding],
) -> Result<WorldKnowledgeProfile> {
    let mut selected = Vec::new();
    for binding in bindings {
        binding.validate()?;
        if binding.agent == agent && binding.world == world && binding.disclosure.eligible {
            selected.push(binding.clone());
        }
    }
    selected.sort_by(|left, right| left.knowledge.cmp(&right.knowledge));
    Ok(WorldKnowledgeProfile {
        version: WORLD_KNOWLEDGE_VERSION.into(),
        agent,
        world,
        bindings: selected,
    })
}

/// Natural-language `remember this` resolves to the existing Agent-maintained Wiki
/// owner for the current World. This plan carries no representation for mutating a
/// human-authored source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldRememberPlan {
    pub version: String,
    pub agent: ResourceRef,
    pub world: ResourceRef,
    pub knowledge: ResourceRef,
    pub agent_wiki: SourceRef,
    pub standing: ProjectCentralStanding,
    #[serde(default)]
    pub provenance: Vec<String>,
}

pub fn plan_world_remember(
    agent: ResourceRef,
    world: ResourceRef,
    knowledge: ResourceRef,
    agent_wiki: SourceRef,
    provenance: Vec<String>,
) -> Result<WorldRememberPlan> {
    if provenance.iter().any(|item| item.trim().is_empty()) {
        return Err(AikitError::new(
            "world_knowledge.remember_invalid_provenance",
            "remember-this provenance entries must not be empty",
        ));
    }
    Ok(WorldRememberPlan {
        version: WORLD_KNOWLEDGE_VERSION.into(),
        agent,
        world,
        knowledge,
        agent_wiki,
        standing: ProjectCentralStanding::AgentMaintained,
        provenance,
    })
}

/// Promotion/generalisation is an explicit proposal toward an owner source. AIKit
/// can correlate a later Recognition but cannot manufacture it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldKnowledgeGeneralisation {
    pub version: String,
    pub from_world: ResourceRef,
    pub proposal: HumanSourceRevisionProposal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recognition_ref: Option<ResourceRef>,
}

pub fn propose_world_generalisation(
    from_world: ResourceRef,
    source: SourceRef,
    reason: impl Into<String>,
    evidence: Vec<SourceRef>,
) -> Result<WorldKnowledgeGeneralisation> {
    let reason = reason.into();
    if reason.trim().is_empty() {
        return Err(AikitError::new(
            "world_knowledge.generalisation_reason_empty",
            "knowledge generalisation proposal requires a reason",
        ));
    }
    Ok(WorldKnowledgeGeneralisation {
        version: WORLD_KNOWLEDGE_VERSION.into(),
        from_world,
        proposal: HumanSourceRevisionProposal {
            source,
            reason,
            evidence,
        },
        recognition_ref: None,
    })
}

/// Semantic addressee kind. These are target relations, not runtime invocation
/// permissions and not SessionSpace membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParticipantTargetKind {
    Human,
    Agent,
    AgentSet,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ParticipantTarget {
    pub kind: ParticipantTargetKind,
    pub participant: ResourceRef,
    /// Stable user-facing address token, normally rendered as a `To:` chip or `@mention`.
    pub address: String,
}

impl ParticipantTarget {
    pub fn new(
        kind: ParticipantTargetKind,
        participant: ResourceRef,
        address: impl Into<String>,
    ) -> Result<Self> {
        let address = address.into();
        if !address.starts_with('@')
            || address.len() < 2
            || address.chars().any(char::is_whitespace)
        {
            return Err(AikitError::new(
                "participant_address.invalid",
                format!("participant address `{address}` must be a non-empty @token"),
            ));
        }
        Ok(Self {
            kind,
            participant,
            address,
        })
    }
}

/// Structured `To:` + `@` grammar shared by human, Agent and AgentSet targets.
/// Invocation authority is intentionally absent from this value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantAddress {
    pub version: String,
    #[serde(default)]
    pub to: Vec<ParticipantTarget>,
    #[serde(default)]
    pub mentions: Vec<ParticipantTarget>,
}

impl ParticipantAddress {
    pub fn new(to: Vec<ParticipantTarget>, mentions: Vec<ParticipantTarget>) -> Result<Self> {
        let mut seen = BTreeSet::new();
        for target in to.iter().chain(mentions.iter()) {
            let key = (
                target.kind,
                target.participant.clone(),
                target.address.clone(),
            );
            if !seen.insert(key) {
                return Err(AikitError::new(
                    "participant_address.duplicate_target",
                    format!("participant target {} is duplicated", target.address),
                ));
            }
        }
        Ok(Self {
            version: PARTICIPANT_ADDRESS_VERSION.into(),
            to,
            mentions,
        })
    }
}

/// Explicit disclosure permission for one canonical AgentSession pair. This is
/// separate from SessionInvocationRelation: invocable does not mean disclosed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoInternalDisclosureGrant {
    pub source_agent_session: ResourceRef,
    pub target_agent_session: ResourceRef,
    pub mode: SessionInvocationMode,
    #[serde(default)]
    pub resources: BTreeSet<ResourceRef>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoInternalReadRequest {
    pub source_agent_session: ResourceRef,
    pub target_agent_session: ResourceRef,
    pub mode: SessionInvocationMode,
    #[serde(default)]
    pub resources: BTreeSet<ResourceRef>,
    pub purpose: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoInternalDecision {
    Denied,
    Allowed,
}

/// Auditable result of an explicit co-internal read decision. No provider-native
/// conversation or peer prompt/history appears here; only the exact allowed
/// ResourceRefs cross the disclosure boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoInternalReadDecision {
    pub version: String,
    pub source_agent_session: ResourceRef,
    pub target_agent_session: ResourceRef,
    pub mode: SessionInvocationMode,
    pub decision: CoInternalDecision,
    pub invocation_authorised: bool,
    pub disclosure_authorised: bool,
    #[serde(default)]
    pub disclosed_resources: BTreeSet<ResourceRef>,
    /// Hard invariant for this seam: peer prompt/history is never ambiently disclosed.
    pub peer_prompt_history_disclosed: bool,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// Decide one bounded co-internal retrieval against the SAME canonical
/// AgentSessions used by SessionEcology. Merely discovering the target session
/// produces a denied decision with zero disclosure unless both invocation and
/// disclosure relations are explicitly present.
pub fn decide_co_internal_read(
    request: &CoInternalReadRequest,
    invocation_relations: &[SessionInvocationRelation],
    grants: &[CoInternalDisclosureGrant],
) -> Result<CoInternalReadDecision> {
    if request.purpose.trim().is_empty() {
        return Err(AikitError::new(
            "co_internal.purpose_empty",
            "co-internal retrieval requires an explicit purpose",
        ));
    }
    if request.source_agent_session == request.target_agent_session {
        return Err(AikitError::new(
            "co_internal.self_target",
            "co-internal retrieval requires two distinct AgentSessions",
        ));
    }

    let invocation = invocation_relations.iter().find(|relation| {
        relation.source_agent_session == request.source_agent_session
            && relation.target_agent_session == request.target_agent_session
            && relation.mode == request.mode
    });
    let invocation_authorised = invocation.is_some_and(SessionInvocationRelation::invocable);

    let grant = grants.iter().find(|grant| {
        grant.source_agent_session == request.source_agent_session
            && grant.target_agent_session == request.target_agent_session
            && grant.mode == request.mode
    });
    let disclosure_authorised = grant.is_some();

    let allowed = if invocation_authorised {
        grant
            .map(|grant| {
                request
                    .resources
                    .intersection(&grant.resources)
                    .cloned()
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default()
    } else {
        BTreeSet::new()
    };
    let decision = if invocation_authorised && disclosure_authorised {
        CoInternalDecision::Allowed
    } else {
        CoInternalDecision::Denied
    };
    let provenance = invocation
        .into_iter()
        .flat_map(|relation| relation.provenance.clone())
        .chain(grant.into_iter().flat_map(|grant| grant.provenance.clone()))
        .collect();

    Ok(CoInternalReadDecision {
        version: CO_INTERNAL_DISCLOSURE_VERSION.into(),
        source_agent_session: request.source_agent_session.clone(),
        target_agent_session: request.target_agent_session.clone(),
        mode: request.mode,
        decision,
        invocation_authorised,
        disclosure_authorised,
        disclosed_resources: allowed,
        peer_prompt_history_disclosed: false,
        provenance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_space::SessionSpaceAuthorityState;

    fn resource(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn source(raw: &str) -> SourceRef {
        SourceRef::parse(raw).unwrap()
    }

    fn revision(raw: &str) -> SourceRevision {
        SourceRevision::new(raw).unwrap()
    }

    #[test]
    fn same_agent_gets_distinct_attributable_knowledge_in_two_worlds() {
        let agent = resource("agent/developer");
        let world_a = resource("world/project-a");
        let world_b = resource("world/project-b");
        let bindings = vec![
            WorldKnowledgeBinding {
                knowledge: resource("knowledge/a"),
                agent: agent.clone(),
                world: world_a.clone(),
                standing: WorldKnowledgeStanding::WorldLocalAgentMaintained,
                source: source("source:project-a-agent-wiki"),
                source_revision: Some(revision("rev-a")),
                source_world: Some(world_a.clone()),
                disclosure: WorldKnowledgeDisclosure {
                    known_to_exist: true,
                    eligible: true,
                    retrieved: false,
                },
                provenance: vec!["Project A Agent-Wiki".into()],
            },
            WorldKnowledgeBinding {
                knowledge: resource("knowledge/b"),
                agent: agent.clone(),
                world: world_b.clone(),
                standing: WorldKnowledgeStanding::Inherited,
                source: source("source:project-a-agent-wiki"),
                source_revision: Some(revision("rev-a")),
                source_world: Some(world_a.clone()),
                disclosure: WorldKnowledgeDisclosure {
                    known_to_exist: true,
                    eligible: true,
                    retrieved: false,
                },
                provenance: vec!["propagated from world/project-a at rev-a".into()],
            },
        ];

        let a = disclose_world_knowledge(agent.clone(), world_a.clone(), &bindings).unwrap();
        let b = disclose_world_knowledge(agent, world_b, &bindings).unwrap();
        assert_eq!(a.bindings.len(), 1);
        assert_eq!(a.bindings[0].knowledge, resource("knowledge/a"));
        assert_eq!(b.bindings.len(), 1);
        assert_eq!(b.bindings[0].source_world, Some(world_a));
        assert_eq!(b.bindings[0].source_revision, Some(revision("rev-a")));
        assert!(!b.bindings[0].disclosure.retrieved);
    }

    #[test]
    fn remember_targets_agent_wiki_and_generalisation_stays_a_proposal() {
        let remembered = plan_world_remember(
            resource("agent/developer"),
            resource("world/project"),
            resource("knowledge/decision"),
            source("source:project-agent-wiki"),
            vec!["human said remember this".into()],
        )
        .unwrap();
        assert_eq!(remembered.standing, ProjectCentralStanding::AgentMaintained);

        let proposal = propose_world_generalisation(
            resource("world/project"),
            source("source:central-general"),
            "This relation now appears stable beyond the Project World",
            vec![source("source:project-agent-wiki")],
        )
        .unwrap();
        assert!(proposal.recognition_ref.is_none());
        assert_eq!(proposal.proposal.source, source("source:central-general"));
    }

    #[test]
    fn participant_grammar_round_trips_human_agent_and_agent_set_without_authority() {
        let address = ParticipantAddress::new(
            vec![
                ParticipantTarget::new(
                    ParticipantTargetKind::Human,
                    resource("human/frank"),
                    "@frank",
                )
                .unwrap(),
                ParticipantTarget::new(
                    ParticipantTargetKind::AgentSet,
                    resource("agent-set/development"),
                    "@development",
                )
                .unwrap(),
            ],
            vec![ParticipantTarget::new(
                ParticipantTargetKind::Agent,
                resource("agent/guardian"),
                "@guardian",
            )
            .unwrap()],
        )
        .unwrap();
        let value = serde_json::to_value(&address).unwrap();
        let round_trip: ParticipantAddress = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(round_trip, address);
        assert!(value.get("authority").is_none());
        assert!(value.get("invocation").is_none());
    }

    fn invocation(authorised: bool) -> SessionInvocationRelation {
        SessionInvocationRelation {
            source_agent_session: resource("agent-session/source"),
            target_agent_session: resource("agent-session/target"),
            mode: SessionInvocationMode::SessionContribution,
            authority: SessionSpaceAuthorityState {
                capability: Some(resource("capability/co-internal")),
                capability_available: true,
                capability_granted: authorised,
                action: Some(resource("action/session-contribution")),
                action_authorised: authorised,
                provenance: vec!["explicit authority".into()],
            },
            provenance: vec!["SessionEcology relation".into()],
        }
    }

    fn request() -> CoInternalReadRequest {
        CoInternalReadRequest {
            source_agent_session: resource("agent-session/source"),
            target_agent_session: resource("agent-session/target"),
            mode: SessionInvocationMode::SessionContribution,
            resources: [resource("context/decision"), resource("context/private")]
                .into_iter()
                .collect(),
            purpose: "Retrieve the bounded decision context needed for this contribution".into(),
        }
    }

    #[test]
    fn discovery_or_invocation_authority_alone_discloses_zero_peer_context() {
        let decision = decide_co_internal_read(&request(), &[invocation(true)], &[]).unwrap();
        assert_eq!(decision.decision, CoInternalDecision::Denied);
        assert!(decision.invocation_authorised);
        assert!(!decision.disclosure_authorised);
        assert!(decision.disclosed_resources.is_empty());
        assert!(!decision.peer_prompt_history_disclosed);
    }

    #[test]
    fn explicit_grant_discloses_only_the_requested_allowed_resources() {
        let grant = CoInternalDisclosureGrant {
            source_agent_session: resource("agent-session/source"),
            target_agent_session: resource("agent-session/target"),
            mode: SessionInvocationMode::SessionContribution,
            resources: [resource("context/decision")].into_iter().collect(),
            provenance: vec!["target-session disclosure grant".into()],
        };
        let decision = decide_co_internal_read(&request(), &[invocation(true)], &[grant]).unwrap();
        assert_eq!(decision.decision, CoInternalDecision::Allowed);
        assert_eq!(
            decision.disclosed_resources,
            [resource("context/decision")].into_iter().collect()
        );
        assert!(!decision.peer_prompt_history_disclosed);
    }
}
