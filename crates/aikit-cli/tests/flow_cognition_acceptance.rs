//! W4-C owner-side Flow cognition acceptance (wayfinder W1.4 + W1.5 owner half).
//!
//! One explicit Contemplate(FlowRef) runs deterministic preflight first and
//! discloses exactly what it will read and touch; the typed cognition reading
//! (`aikit.flow-cognition/v1`) exists only behind the validated preflight
//! record and a host executor — otherwise it is explicitly `unavailable` or
//! `refused`, never faked. Contemplate is never auto-invoked (#138 §7). A
//! successful contemplation records exactly one familiarity observation,
//! provable via log export replay (C2 precedent). The changed-since read
//! returns typed rows — changed sources, affected knowledge, unresolved —
//! each with provenance, with explicit empty/unavailable states.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use aikit_cli::app::Service;
use aikit_core::flow::{
    FlowContemplateExecutor, FlowContemplateGenerated, FlowContemplatePreflight,
    FlowMutationIntent, FLOW_CONTEXT_VERSION, FLOW_METHOD_REF,
};
use aikit_core::flow_cognition::ThoughtUnresolvedKind;
use aikit_core::knowledge_living::ContemplateGenerated;
use aikit_core::model_runtime::{
    AccessFieldReading, InferenceEngineForm, InferenceEngineReading, MaterialResourceReading,
    ModelAccessReading, ModelMaterialisationReading, ModelRuntimeReadModel, ModelRuntimeRelation,
    ModelSurfaceReading, ModelVariantReading, PlacementObservation, RuntimeChangeApplication,
};
use aikit_core::projectcentral::HumanSourceRevisionProposal;
use aikit_core::resource::{ProviderRef, ResourceRef, SourceRef, SourceRevision};
use aikit_core::{
    build_integrative_reading, living_wiki_provenance, FlowChangedSinceState, FlowCognition,
    FlowThoughtRecord, KnowledgeChangeHorizon, KnowledgeChangeKind, KnowledgeFreshness,
    KnowledgeObservedSource, KnowledgeSourceChange, ReadingBasisNode, ReadingReturnPath,
    RetractionMode, FLOW_CONTEMPLATE_USE_RECORDED,
};
use aikit_store::home::AikitHome;
use aikit_store::replay_familiarity;
use tempfile::TempDir;

const FLOW_REF: &str = "wiki:node:staged/test-flow";
const FLOW_SOURCE: &str = "source:file:test-flow-note";
const FLOW_REVISION: &str = "owner-r1";
const PROJECT_ID: &str = "project/test-flow-cognition";

fn write(path: &std::path::Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn resource(value: &str) -> ResourceRef {
    ResourceRef::parse(value).unwrap()
}
fn source(value: &str) -> SourceRef {
    SourceRef::parse(value).unwrap()
}
fn revision(value: &str) -> SourceRevision {
    SourceRevision::parse(value).unwrap()
}

/// Private AIKIT_HOME under /tmp (never a live user store) plus a project
/// carrying a ProjectCentral manifest, a Semantic Wiki Flow node with exact
/// source-revision provenance, and SourcePool material at that exact revision.
fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();
    write(&project.join(".aikit/profile.toml"), "schema = 1\n");
    write(
        &temp.path().join("aikit-home/scopes/global/profile.toml"),
        "schema = 1\n",
    );
    write(
        &project.join("ProjectCentral/project.json"),
        &format!(
            r#"{{
              "schema": "central.project/v1",
              "project_id": "{PROJECT_ID}",
              "human_source": "ProjectCentral/user",
              "wiki": {{
                "profile": "okf-wiki/v1",
                "source": "ProjectCentral/agents/wiki/wiki.json"
              }}
            }}"#
        ),
    );
    write(
        &project.join("ProjectCentral/agents/wiki/wiki.json"),
        r#"{"objects": []}"#,
    );
    write(
        &project.join("ProjectCentral/user/position.md"),
        "# Human position\n\nThe owner-authored ground the Flow thinks against.\n",
    );
    write(
        &project.join("semantic-wiki.json"),
        r#"{
          "objects": [
            {
              "profile": "okf-wiki/v1",
              "object": "node",
              "ref": "wiki:node:staged/test-flow",
              "revision": 3,
              "provenance": [{"source_ref":"source:file:test-flow-note","source_revision":"owner-r1"}],
              "type": "flow",
              "title": "Owner flow thread",
              "space_refs": [],
              "source_refs": ["source:file:test-flow-note"]
            }
          ]
        }"#,
    );
    write(
        &project.join("source-material.json"),
        r#"{
          "binding": {
            "source": "source:file:test-flow-note",
            "revision": "owner-r1",
            "title": "Owner flow note",
            "tags": ["flow", "owner"],
            "visibility": "public",
            "owners": [],
            "media_type": "text/markdown",
            "metadata": {"origin":"w4-c-fixture"}
          },
          "body": "Current Flow body, disclosed at the owner in this context."
        }"#,
    );
    Service::open(home, &project, |_| None).expect("open production application service")
}

fn runtime(session: &str) -> ModelRuntimeReadModel {
    ModelRuntimeReadModel {
        version: "aikit.model-runtime/v1".into(),
        project: Some(resource(PROJECT_ID)),
        agent: Some(resource("agent:test")),
        agency: Some(resource("agency:test")),
        harness: resource("harness:test"),
        agent_session: Some(session.to_string()),
        harness_composition_fingerprint: "w4-c-acceptance".into(),
        relation: ModelRuntimeRelation {
            model: ModelVariantReading {
                model: resource("model:test"),
                variant: "default".into(),
            },
            engine: InferenceEngineReading {
                engine: resource("engine:test"),
                provider: ProviderRef::parse("provider:test").unwrap(),
                form: InferenceEngineForm::External,
                revision: None,
                provider_native: BTreeMap::new(),
            },
            materialisation: ModelMaterialisationReading {
                binding_ref: "binding:test".into(),
                workcell_ref: None,
                placement: PlacementObservation::Local,
                endpoint: None,
                provider_native: BTreeMap::new(),
                resources: MaterialResourceReading::default(),
                lifetime_owner: "test".into(),
                retraction: RetractionMode::Live,
            },
            model_surface: ModelSurfaceReading {
                contract: None,
                protocol: "test".into(),
                capabilities: BTreeSet::new(),
                access: ModelAccessReading {
                    inference: AccessFieldReading::available(["text"]),
                    material_control: AccessFieldReading::unavailable("not required"),
                    interior: AccessFieldReading::unavailable("not required"),
                },
            },
            change_application: RuntimeChangeApplication::Live,
        },
        components: vec![],
        contracts: vec![],
        surfaces: vec![],
        unavailable: vec![],
    }
}

fn basis(session: &str) -> aikit_cli::app::FlowContemplateBasis {
    aikit_cli::app::FlowContemplateBasis {
        horizon: Some(horizon(FLOW_REVISION, 7, vec![])),
        runtime: Some(runtime(session)),
        agent: Some(resource("agent:test")),
        agency: Some(resource("agency:test")),
    }
}

fn horizon(
    observed_revision: &str,
    cursor: u64,
    changes: Vec<KnowledgeSourceChange>,
) -> KnowledgeChangeHorizon {
    KnowledgeChangeHorizon {
        provider: "owner-seam/test".into(),
        cursor,
        sources: vec![KnowledgeObservedSource {
            source: source(FLOW_SOURCE),
            revision: Some(revision(observed_revision)),
            available: true,
        }],
        changes,
    }
}

fn observation_events(service: &Service) -> usize {
    match replay_familiarity(service.index()).expect("replay familiarity") {
        aikit_store::FamiliarityReplay::Loaded {
            observation_events, ..
        } => observation_events,
        other => panic!("familiarity replay invalidated: {other:?}"),
    }
}

/// The host-side executor stand-in: returns one deliberate Agent/model
/// crossing. Mirrors the O:I kernel seam a later cell will fill.
#[derive(Default)]
struct OneCallExecutor {
    calls: usize,
}

impl FlowContemplateExecutor for OneCallExecutor {
    fn execute(
        &mut self,
        preflight: &FlowContemplatePreflight,
    ) -> aikit_core::Result<FlowContemplateGenerated> {
        self.calls += 1;
        let whole = resource("wiki:reading:flow-whole");
        let reading = aikit_core::knowledge_wiki::WikiReading {
            profile: aikit_core::knowledge_wiki::OKF_WIKI_PROFILE.into(),
            ref_id: whole.clone(),
            revision: 1,
            provenance: vec![living_wiki_provenance(
                preflight.standing.binding.source_ref.clone(),
                preflight.standing.binding.flow_revision.clone(),
            )],
            frame_ref: resource("wiki:frame:flow-contemplate"),
            reading_type: "integrative-flow".into(),
            artifact_ref: None,
            derived_by_ref: Some(resource("agent:test")),
            extensions: BTreeMap::new(),
        };
        let integrated = build_integrative_reading(
            reading,
            vec![ReadingBasisNode {
                resource: preflight.standing.binding.flow_ref.clone(),
                source: Some(preflight.standing.binding.source_ref.clone()),
                source_revision: Some(preflight.standing.binding.flow_revision.clone()),
                roles: vec!["flow-source".into()],
            }],
            vec![],
            vec![ReadingReturnPath {
                from_basis: preflight.standing.binding.flow_ref.clone(),
                through: vec![],
                to_whole: whole,
            }],
            KnowledgeFreshness::Fresh,
        )?;
        Ok(FlowContemplateGenerated {
            living: ContemplateGenerated {
                wiki_upserts: vec![aikit_core::knowledge_wiki::WikiObject::Reading(
                    integrated.reading.clone(),
                )],
                integrative_readings: vec![integrated],
                candidates: vec!["candidate understanding".into()],
                tensions: vec!["open question".into()],
                human_source_proposals: vec![HumanSourceRevisionProposal {
                    source: source("source:human-ground:test"),
                    reason: "Flow contemplation exposes a possible authored-position refinement"
                        .into(),
                    evidence: vec![preflight.standing.binding.source_ref.clone()],
                }],
            },
            flow_mutations: vec![FlowMutationIntent {
                version: FLOW_CONTEXT_VERSION.into(),
                flow_ref: preflight.standing.binding.flow_ref.clone(),
                expected_revision: preflight.standing.binding.flow_revision.clone(),
                replacement: "refined by contemplation".into(),
                actor: resource("agent:test"),
                agency: Some(resource("agency:test")),
                agent_session: preflight.standing.binding.agent_session.clone(),
                context_resolution_version: preflight
                    .standing
                    .binding
                    .context_resolution_version
                    .clone(),
                method: Some(resource(FLOW_METHOD_REF)),
                invocation_ref: Some(preflight.invocation_ref.clone()),
            }],
        })
    }
}

#[test]
fn preflight_without_owner_seams_is_explicitly_unavailable_and_records_nothing() {
    let temp = TempDir::new().unwrap();
    let mut service = open_service(&temp);

    let outcome = service
        .flow_contemplate_preflight(
            &resource(FLOW_REF),
            &aikit_cli::app::FlowContemplateBasis::default(),
        )
        .unwrap();
    let aikit_cli::app::FlowPreflightOutcome::Unavailable { flow, reason, .. } = outcome else {
        panic!("without owner seams the preflight outcome must be unavailable: {outcome:?}");
    };
    assert_eq!(flow.as_str(), FLOW_REF);
    assert!(
        reason.contains("horizon"),
        "the reason names the missing owner seam: {reason}"
    );
    assert_eq!(
        observation_events(&service),
        0,
        "an unavailable preflight records no familiarity"
    );
}

#[test]
fn preflight_discloses_exact_reads_and_never_auto_invokes() {
    let temp = TempDir::new().unwrap();
    let mut service = open_service(&temp);

    let outcome = service
        .flow_contemplate_preflight(&resource(FLOW_REF), &basis("agent-session/preflight"))
        .unwrap();
    let aikit_cli::app::FlowPreflightOutcome::Preflight {
        version,
        preflight,
        explain,
        automatic_agent_or_model_invocation,
        ..
    } = outcome
    else {
        panic!("with owner seams the preflight must be disclosed: {outcome:?}");
    };
    assert_eq!(version, "aikit.flow-cognition/v1");
    assert!(!automatic_agent_or_model_invocation);
    assert!(!preflight.automatic_agent_or_model_invocation);
    assert!(
        preflight
            .invocation_ref
            .as_str()
            .starts_with("flow-contemplate/"),
        "the invocation ref is a deterministic preflight digest"
    );
    assert_eq!(
        preflight.standing.disclosed_body(),
        Some("Current Flow body, disclosed at the owner in this context.")
    );
    // Explain disclosure follows the repo's ExplainEvidence shape and
    // includes the exact-reads facts plus the execution invariant.
    assert!(
        !explain.is_empty(),
        "preflight carries its Explain disclosure"
    );
    assert!(
        explain
            .iter()
            .any(|evidence| evidence.facts.iter().any(|fact| {
                fact.relation.contains("flow-contemplate") || fact.summary.contains("Flow")
            })),
        "the disclosure names what will be read: {explain:?}"
    );
    assert!(
        explain.iter().any(|evidence| evidence
            .facts
            .iter()
            .any(|fact| fact.summary.contains("never") || fact.summary.contains("explicit"))),
        "the disclosure carries the never-auto-invoked invariant: {explain:?}"
    );
    assert_eq!(
        observation_events(&service),
        0,
        "a deterministic preflight records no familiarity"
    );
}

#[test]
fn contemplate_without_host_executor_is_unavailable_and_records_nothing() {
    let temp = TempDir::new().unwrap();
    let mut service = open_service(&temp);

    let receipt = service
        .flow_contemplate(&resource(FLOW_REF), &basis("agent-session/no-host"), None)
        .unwrap();
    assert_eq!(receipt.version, "aikit.flow-cognition/v1");
    assert!(
        receipt.preflight.is_some(),
        "the reading still carries the preflight record that gated it"
    );
    let FlowCognition::Unavailable {
        flow_ref, reason, ..
    } = &receipt.cognition
    else {
        panic!(
            "without a host executor the reading is explicitly unavailable: {:?}",
            receipt.cognition
        );
    };
    assert_eq!(flow_ref.as_str(), FLOW_REF);
    assert!(
        reason.contains("ContemplateExecutor"),
        "reason names the missing host executor: {reason}"
    );
    assert_eq!(receipt.recorded, None);
    assert_eq!(receipt.observation_id, None);
    assert_eq!(
        observation_events(&service),
        0,
        "an unavailable contemplation records no familiarity"
    );
}

#[test]
fn one_successful_contemplate_records_exactly_one_familiarity_observation() {
    let temp = TempDir::new().unwrap();
    let mut service = open_service(&temp);

    // The record gate is structural: a drifted hand-built record cannot reach
    // the executor at all.
    let mut executor = OneCallExecutor::default();
    let receipt = service
        .flow_contemplate(
            &resource(FLOW_REF),
            &basis("agent-session/contemplate"),
            Some(&mut executor),
        )
        .unwrap();
    assert_eq!(
        executor.calls, 1,
        "exactly one deliberate Agent/model crossing"
    );

    let FlowCognition::Cognition {
        thought, impact, ..
    } = &receipt.cognition
    else {
        panic!(
            "a successful contemplation yields a typed cognition: {:?}",
            receipt.cognition
        );
    };
    assert_eq!(thought.flow_ref.as_str(), FLOW_REF);
    assert_eq!(thought.source_ref.as_str(), FLOW_SOURCE);
    assert_eq!(thought.basis_revision, revision(FLOW_REVISION));
    assert_eq!(thought.horizon_cursor, 7);
    assert!(
        thought.outcome.is_some(),
        "the thought carries the returned material"
    );
    assert!(
        !impact.automatic_agent_or_model_invocation,
        "the deterministic impact field stays false; only the cognition state records the crossing"
    );

    // Familiarity law (C2 precedent): exactly one observation per successful
    // contemplate, provable via log export replay.
    assert_eq!(
        receipt.recorded.as_deref(),
        Some(FLOW_CONTEMPLATE_USE_RECORDED)
    );
    let observation_id = receipt
        .observation_id
        .as_ref()
        .expect("a successful contemplate names its observation");
    assert!(
        observation_id.starts_with("flow-contemplate-use/"),
        "observation id: {observation_id}"
    );
    assert_eq!(observation_events(&service), 1);

    // A second successful contemplate records exactly one more.
    let second = service
        .flow_contemplate(
            &resource(FLOW_REF),
            &basis("agent-session/contemplate"),
            Some(&mut OneCallExecutor::default()),
        )
        .unwrap();
    assert!(matches!(second.cognition, FlowCognition::Cognition { .. }));
    assert_eq!(observation_events(&service), 2);
    assert_ne!(second.observation_id, receipt.observation_id);

    // Inert operations record nothing further.
    service
        .flow_contemplate_preflight(&resource(FLOW_REF), &basis("agent-session/preflight"))
        .unwrap();
    assert_eq!(observation_events(&service), 2);
}

#[test]
fn changed_since_returns_typed_rows_with_provenance_and_explicit_states() {
    let temp = TempDir::new().unwrap();
    let mut service = open_service(&temp);

    let mut executor = OneCallExecutor::default();
    let receipt = service
        .flow_contemplate(
            &resource(FLOW_REF),
            &basis("agent-session/contemplate"),
            Some(&mut executor),
        )
        .unwrap();
    let thought = receipt
        .cognition
        .thought()
        .cloned()
        .expect("a successful contemplation records its thought");

    // No owner horizon supplied: the reading is unavailable, never guessed.
    let without_horizon = service.flow_changed_since(&thought, None).unwrap();
    assert_eq!(without_horizon.thought, thought.invocation_ref);
    assert_eq!(
        without_horizon.reading.state,
        FlowChangedSinceState::Unavailable
    );
    assert!(without_horizon.reading.unavailable_reason.is_some());
    assert!(without_horizon.reading.changed_sources.is_empty());
    assert!(without_horizon.reading.affected_knowledge.is_empty());
    assert!(!without_horizon.reading.automatic_agent_or_model_invocation);

    // Quiet horizon (no rows above the thought cursor): the outcome's open
    // items are reported with provenance — an Available reading.
    let quiet = horizon(FLOW_REVISION, 8, vec![]);
    let still = service.flow_changed_since(&thought, Some(quiet)).unwrap();
    assert_eq!(still.reading.state, FlowChangedSinceState::Available);
    assert!(still.reading.changed_sources.is_empty());
    assert!(
        still
            .reading
            .unresolved
            .iter()
            .any(|row| row.kind == ThoughtUnresolvedKind::Tension),
        "the thought's returned tension remains open: {:?}",
        still.reading.unresolved
    );
    assert!(
        still
            .reading
            .unresolved
            .iter()
            .any(|row| row.kind == ThoughtUnresolvedKind::FlowMutationIntent),
        "the returned Flow mutation intent remains an owner request, not applied"
    );
    assert!(
        still
            .reading
            .unresolved
            .iter()
            .all(|row| !row.provenance.is_empty()),
        "every unresolved row carries provenance"
    );

    // A change row above the thought cursor, observed at the owner as
    // owner-r2: typed changed sources with before/after revisions and
    // provenance, plus affected knowledge.
    let changed = horizon(
        "owner-r2",
        8,
        vec![KnowledgeSourceChange {
            cursor: 9,
            world_ref: "world/test".into(),
            source: source(FLOW_SOURCE),
            roles: vec!["flow-source".into()],
            provenance: "owner seam observation (test fixture)".into(),
            standing: "observed".into(),
            before_revision: Some(revision(FLOW_REVISION)),
            after_revision: Some(revision("owner-r2")),
            kind: KnowledgeChangeKind::Modified,
            agent_retrieval_allowed: true,
        }],
    );
    let moved = service.flow_changed_since(&thought, Some(changed)).unwrap();
    assert_eq!(moved.reading.state, FlowChangedSinceState::Available);
    let row = moved
        .reading
        .changed_sources
        .first()
        .expect("the change row is reported relative to the thought");
    assert_eq!(row.source.as_str(), FLOW_SOURCE);
    assert_eq!(row.basis_revision, Some(revision(FLOW_REVISION)));
    assert_eq!(row.observed_revision, Some(revision("owner-r2")));
    assert!(!row.provenance.is_empty());
    assert!(row.available);
    assert!(
        !moved.reading.affected_knowledge.is_empty(),
        "the deterministic impact finds the affected knowledge: {:?}",
        moved.reading.affected_knowledge
    );
    assert!(
        moved
            .reading
            .affected_knowledge
            .iter()
            .all(|entry| !entry.relation.is_empty()),
        "every affected row carries its relation provenance"
    );
    assert!(!moved.reading.automatic_agent_or_model_invocation);
    assert_eq!(
        observation_events(&service),
        1,
        "the deterministic changed-since read records no familiarity"
    );

    // A thought that never reached a successful outcome reads Empty — an
    // explicit state, never faked rows.
    let quiet_thought = FlowThoughtRecord {
        version: "aikit.flow-cognition/v1".into(),
        invocation_ref: resource("flow-contemplate/nooutcome00000000"),
        flow_ref: resource(FLOW_REF),
        source_ref: source(FLOW_SOURCE),
        basis_revision: revision(FLOW_REVISION),
        horizon_cursor: 7,
        outcome: None,
    };
    let empty = service
        .flow_changed_since(&quiet_thought, Some(horizon(FLOW_REVISION, 7, vec![])))
        .unwrap();
    assert_eq!(empty.reading.state, FlowChangedSinceState::Empty);
    assert!(empty.reading.changed_sources.is_empty());
    assert!(empty.reading.affected_knowledge.is_empty());
    assert!(empty.reading.unresolved.is_empty());
}
