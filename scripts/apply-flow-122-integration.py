from pathlib import Path


def replace(path: str, old: str, new: str, count: int = 1):
    p = Path(path)
    text = p.read_text()
    actual = text.count(old)
    if actual < count:
        raise SystemExit(f"anchor missing in {path}: expected {count}, got {actual}: {old[:120]!r}")
    p.write_text(text.replace(old, new, count))

flow = "crates/aikit-core/src/flow.rs"

# Prove provider neutrality with a genuinely distinct provider/identity/source namespace.
replace(
    flow,
    '''        fn central_style(container: &str) -> Self {
            Self {
                provider: resource("provider:central-flow"),
                flow: FlowSourceDescriptor {
                    flow_ref: resource("central:flow:project:test:thread-1"),
                    source_ref: source("central:source:project:test:notes%2Fthread.md"),
                    revision: revision("central.content-fnv1a64/v1:7:aaaaaaaaaaaaaaaa"),
                    provider: resource("provider:central-flow"),
                    lifecycle: FlowLifecycle::Active,
                    title: Some("Thread".into()),
                    scope: Some(resource("project:test")),
                    container_hint: Some(container.into()),
                    capabilities: FlowCapabilities {
                        read: true,
                        write: true,
                        history: true,
                    },
                    provenance: vec!["owner FlowRef + revision".into()],
                },
                body: "current Flow body".into(),
                disclose: true,
                writes: 0,
            }
        }
''',
    '''        fn central_style(container: &str) -> Self {
            Self {
                provider: resource("provider:central-flow"),
                flow: FlowSourceDescriptor {
                    flow_ref: resource("central:flow:project:test:thread-1"),
                    source_ref: source("central:source:project:test:notes%2Fthread.md"),
                    revision: revision("central.content-fnv1a64/v1:7:aaaaaaaaaaaaaaaa"),
                    provider: resource("provider:central-flow"),
                    lifecycle: FlowLifecycle::Active,
                    title: Some("Thread".into()),
                    scope: Some(resource("project:test")),
                    container_hint: Some(container.into()),
                    capabilities: FlowCapabilities {
                        read: true,
                        write: true,
                        history: true,
                    },
                    provenance: vec!["owner FlowRef + revision".into()],
                },
                body: "current Flow body".into(),
                disclose: true,
                writes: 0,
            }
        }

        fn non_central_style(container: &str) -> Self {
            Self {
                provider: resource("provider:notes-house-flow"),
                flow: FlowSourceDescriptor {
                    flow_ref: resource("flow:notes-house:thread-1"),
                    source_ref: source("source:notes-house:thread-1"),
                    revision: revision("notes-house-r7"),
                    provider: resource("provider:notes-house-flow"),
                    lifecycle: FlowLifecycle::Active,
                    title: Some("Thread".into()),
                    scope: Some(resource("project:test")),
                    container_hint: Some(container.into()),
                    capabilities: FlowCapabilities {
                        read: true,
                        write: true,
                        history: true,
                    },
                    provenance: vec!["mock non-Central source-house Flow capability".into()],
                },
                body: "current Flow body".into(),
                disclose: true,
                writes: 0,
            }
        }
''')
replace(
    flow,
    '''        let central = MemoryFlowProvider::central_style("ProjectCentral/now/flows/2026-08-24-0100.md");
        let notes = MemoryFlowProvider::central_style("notes/2026-08-24-0100.md");''',
    '''        let central = MemoryFlowProvider::central_style("ProjectCentral/now/flows/2026-08-24-0100.md");
        let notes = MemoryFlowProvider::non_central_style("notes/2026-08-24-0100.md");''')
replace(
    flow,
    '''        assert_eq!(a.binding.flow_ref, b.binding.flow_ref);
        assert_eq!(a.disclosed_body(), Some("current Flow body"));''',
    '''        assert_ne!(a.binding.flow_ref, b.binding.flow_ref);
        assert_ne!(a.binding.provider, b.binding.provider);
        assert_eq!(b.binding.provider.as_str(), "provider:notes-house-flow");
        assert_eq!(notes.flow.container_hint.as_deref(), Some("notes/2026-08-24-0100.md"));
        assert_eq!(a.disclosed_body(), Some("current Flow body"));''')

# Make Flow change prove an explicitly old integrative basis becomes pending, still with zero invocation.
replace(
    flow,
    '''    fn flow_change_uses_living_knowledge_impact_without_invocation() {
        let provider = MemoryFlowProvider::central_style("notes/thread.md");
        let dependency = flow_knowledge_dependency(
            resource("wiki:reading:thread"),
            &provider.flow,
            "integrates-flow",
            Some(resource("wiki:reading:thread")),
            true,
        );
        let impact = crate::deterministic_knowledge_impact(&horizon(&provider.flow), &[dependency])
            .unwrap();
        assert!(!impact.automatic_agent_or_model_invocation);
        assert_eq!(impact.changed_sources, vec![provider.flow.source_ref.clone()]);
    }
''',
    '''    fn flow_change_uses_living_knowledge_impact_without_invocation() {
        let provider = MemoryFlowProvider::central_style("notes/thread.md");
        let basis = provider.flow.clone();
        let dependency = flow_knowledge_dependency(
            resource("wiki:reading:thread"),
            &basis,
            "integrates-flow",
            Some(resource("wiki:reading:thread")),
            true,
        );
        let mut current = basis.clone();
        current.revision = revision("central.content-fnv1a64/v1:9:bbbbbbbbbbbbbbbb");
        let impact = crate::deterministic_knowledge_impact(&horizon(&current), &[dependency])
            .unwrap();
        assert!(!impact.automatic_agent_or_model_invocation);
        assert_eq!(impact.changed_sources, vec![basis.source_ref.clone()]);
        assert_eq!(impact.affected.len(), 1);
        assert_eq!(impact.affected[0].freshness, crate::KnowledgeFreshness::IntegrationPending);
    }
''')

# A single deliberate contemplation can return a Flow mutation, an integrative WikiReading,
# a human-Ground proposal and open knowledge while each retains its own authority path.
replace(
    flow,
    '''            Ok(FlowContemplateGenerated {
                living: ContemplateGenerated {
                    wiki_upserts: Vec::<WikiObject>::new(),
                    integrative_readings: vec![],
                    candidates: vec!["candidate understanding".into()],
                    tensions: vec!["open question".into()],
                    human_source_proposals: Vec::<HumanSourceRevisionProposal>::new(),
                },''',
    '''            let whole = resource("wiki:reading:flow-whole");
            let reading = crate::knowledge_wiki::WikiReading {
                profile: crate::knowledge_wiki::OKF_WIKI_PROFILE.into(),
                ref_id: whole.clone(),
                revision: 1,
                provenance: vec![crate::living_wiki_provenance(
                    preflight.standing.binding.source_ref.clone(),
                    preflight.standing.binding.flow_revision.clone(),
                )],
                frame_ref: resource("wiki:frame:flow-contemplate"),
                reading_type: "integrative-flow".into(),
                artifact_ref: None,
                derived_by_ref: Some(resource("agent:test")),
                extensions: BTreeMap::new(),
            };
            let integrated = crate::build_integrative_reading(
                reading,
                vec![crate::ReadingBasisNode {
                    resource: preflight.standing.binding.flow_ref.clone(),
                    source: Some(preflight.standing.binding.source_ref.clone()),
                    source_revision: Some(preflight.standing.binding.flow_revision.clone()),
                    roles: vec!["flow-source".into()],
                }],
                vec![],
                vec![crate::ReadingReturnPath {
                    from_basis: preflight.standing.binding.flow_ref.clone(),
                    through: vec![],
                    to_whole: whole,
                }],
                crate::KnowledgeFreshness::Fresh,
            )?;
            Ok(FlowContemplateGenerated {
                living: ContemplateGenerated {
                    wiki_upserts: vec![WikiObject::Reading(integrated.reading.clone())],
                    integrative_readings: vec![integrated],
                    candidates: vec!["candidate understanding".into()],
                    tensions: vec!["open question".into()],
                    human_source_proposals: vec![HumanSourceRevisionProposal {
                        source: source("source:human-ground:test"),
                        reason: "Flow contemplation exposes a possible authored-position refinement".into(),
                        evidence: vec![preflight.standing.binding.source_ref.clone()],
                    }],
                },''')

replace(
    flow,
    '''            FlowAuthorityRef {
                authority: FlowContextAuthority::Claim,
                reference: resource("claim:external:test"),
            },''',
    '''            FlowAuthorityRef {
                authority: FlowContextAuthority::WikiReading,
                reference: resource("wiki:reading:prior-flow"),
            },
            FlowAuthorityRef {
                authority: FlowContextAuthority::Claim,
                reference: resource("claim:external:test"),
            },''')
replace(
    flow,
    '''            FlowAuthorityRef {
                authority: FlowContextAuthority::Ground,
                reference: resource("ground:human:test"),
            },
            FlowAuthorityRef {''',
    '''            FlowAuthorityRef {
                authority: FlowContextAuthority::Ground,
                reference: resource("ground:human:test"),
            },
            FlowAuthorityRef {
                authority: FlowContextAuthority::Run,
                reference: resource("run:external:test"),
            },
            FlowAuthorityRef {''')
replace(
    flow,
    '''        assert_eq!(outcome.living.candidates, vec!["candidate understanding"]);
        assert!(outcome.living.agent_wiki.human_source_proposals.is_empty());''',
    '''        assert_eq!(outcome.living.candidates, vec!["candidate understanding"]);
        assert_eq!(outcome.living.integrative_readings.len(), 1);
        assert_eq!(outcome.living.integrative_readings[0].reading.reading_type, "integrative-flow");
        assert!(outcome
            .living
            .agent_wiki
            .next_objects
            .iter()
            .any(|object| matches!(object, WikiObject::Reading(reading) if reading.ref_id.as_str() == "wiki:reading:flow-whole")));
        assert_eq!(outcome.living.agent_wiki.human_source_proposals.len(), 1);
        assert_eq!(
            outcome.living.agent_wiki.human_source_proposals[0].source.as_str(),
            "source:human-ground:test"
        );
        assert!(outcome.preflight.authority_refs.iter().any(|entry| {
            entry.authority == FlowContextAuthority::Claim
                && entry.reference.as_str() == "claim:external:test"
        }));
        assert!(outcome.preflight.authority_refs.iter().any(|entry| {
            entry.authority == FlowContextAuthority::Run
                && entry.reference.as_str() == "run:external:test"
        }));''')

# Wire the module and its public owner-neutral application types into the V2 core surface.
lib = "crates/aikit-core/src/lib.rs"
replace(lib, "pub mod familiarity;\npub mod frecency;", "pub mod familiarity;\npub mod flow;\npub mod frecency;")
replace(
    lib,
    "pub use frecency::{Candidate, Jump, Tiebreak};\npub use guidance::{",
    '''pub use flow::{
    apply_flow_mutation, bind_flow_for_act, explicit_flow_contemplate,
    first_party_flow_guidance, first_party_flow_method, first_party_flow_resource_records,
    flow_contemplate_preflight, flow_knowledge_dependency, FlowAuthorityRef, FlowBinding,
    FlowCapabilities, FlowContextAuthority, FlowContemplateExecutor, FlowContemplateGenerated,
    FlowContemplateOutcome, FlowContemplatePreflight, FlowContemplateRequest, FlowLifecycle,
    FlowMutationIntent, FlowProvider, FlowReadOutcome, FlowSourceDescriptor, FlowStandingContext,
    FlowStandingDisclosure, FlowWriteRequest, FlowWriteResult, FLOW_CONTEXT_VERSION,
    FLOW_CONTEMPLATE_ACTION_REF, FLOW_CONTEMPLATE_VERSION, FLOW_GUIDANCE_CAPSULE,
    FLOW_KNOWLEDGE_NAVIGATION_REF, FLOW_LIVING_KNOWLEDGE_REF, FLOW_METHOD_REF,
    FLOW_METHOD_SOURCE, FLOW_SKILL_REF,
};
pub use frecency::{Candidate, Jump, Tiebreak};
pub use guidance::{''')

Path("scripts/apply-flow-122-integration.py").unlink()
Path(".github/workflows/flow-122-integration.yml").unlink()
