use std::collections::{BTreeMap, BTreeSet};

use aikit_core::project::ProjectRef;
use aikit_core::resource::{
    Eligibility, ProviderOffer, ProviderRef, ProviderState, ResourceDescriptor, ResourceKind,
    ResourceRecord, ResourceRef, ResourceSource, SourceAuthority, SourceRef, SourceRevision,
    SourceState,
};
use aikit_core::scope::ScopeKind;
use aikit_core::{
    AbsenceKind, AgentVisibility, Availability, ContextSourceEntry, ContextSourceIndex,
    ContextSourceOperation, ContextSourcePrivacy, ContextSourceProvider,
    ContextSourceProviderCapabilities, ContextSourceProviderDescriptor, ContextSourceProviderStatus,
    ContextSourceReadOutcome, ContextSourceReadRequest, DisclosureState, ExternalEgress, Freshness,
    HorizonRequest, ProviderReadResult, RetrievalTarget, StructuredAbsence,
};

fn provider_ref() -> ProviderRef {
    ProviderRef::parse("provider:test").unwrap()
}

fn project_ref() -> ProjectRef {
    ProjectRef::parse("factory:project:test").unwrap()
}

fn source_record(id: &str, provider_state: ProviderState) -> ResourceRecord {
    let mut descriptor = ResourceDescriptor::new(
        ResourceRef::parse(id).unwrap(),
        ResourceKind::ContextSource,
        id,
        format!("ContextSource descriptor for {id}"),
    );
    descriptor.sources.push(ResourceSource {
        source: SourceRef::parse(&format!("source:{id}")).unwrap(),
        authority: Some(SourceAuthority::Authored),
        revision: Some(SourceRevision::parse("rev:source-1").unwrap()),
        locator: None,
        state: SourceState::Available,
    });
    let mut record = ResourceRecord::new(descriptor);
    record.eligibility = Eligibility::Eligible;
    record.providers.push(ProviderOffer {
        provider: provider_ref(),
        locator: None,
        state: provider_state,
    });
    record
}

fn context_source_entry(id: &str, provider_state: ProviderState) -> ContextSourceEntry {
    let mut entry = ContextSourceEntry::new(source_record(id, provider_state)).unwrap();
    entry.relation.project = Some(project_ref());
    entry.relation.scope = Some(ScopeKind::Project);
    entry.freshness = Freshness::Current;
    entry.disclosure = DisclosureState {
        exists: true,
        known_to_exist: true,
        askable: true,
        retrieved: false,
        focused: false,
    };
    entry
}

#[derive(Debug)]
struct FakeProvider {
    provider: ProviderRef,
    status: ContextSourceProviderStatus,
    payloads: BTreeMap<ResourceRef, String>,
    read_count: usize,
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            provider: provider_ref(),
            status: ContextSourceProviderStatus::Available,
            payloads: BTreeMap::new(),
            read_count: 0,
        }
    }

    fn with_payload(mut self, resource: &str, payload: &str) -> Self {
        self.payloads
            .insert(ResourceRef::parse(resource).unwrap(), payload.to_string());
        self
    }
}

impl ContextSourceProvider for FakeProvider {
    fn provider(&self) -> &ProviderRef {
        &self.provider
    }

    fn status(&self) -> ContextSourceProviderStatus {
        self.status.clone()
    }

    fn capabilities(&self) -> ContextSourceProviderCapabilities {
        ContextSourceProviderCapabilities::with_operations([
            ContextSourceOperation::Discover,
            ContextSourceOperation::Read,
            ContextSourceOperation::Explain,
        ])
    }

    fn read(&mut self, request: &ContextSourceReadRequest) -> ProviderReadResult {
        self.read_count += 1;
        match self.payloads.get(&request.resource) {
            Some(payload) => ProviderReadResult::Retrieved {
                payload: payload.clone(),
                revision: Some(SourceRevision::parse("rev:provider-2").unwrap()),
                provenance: Vec::new(),
            },
            None => ProviderReadResult::Absent(StructuredAbsence::new(
                AbsenceKind::Unknown,
                "provider has no material for this resource",
            )),
        }
    }
}

#[test]
fn large_horizon_is_descriptor_searchable_without_provider_reads_or_payload_injection() {
    let mut index = ContextSourceIndex::default();
    let mut provider = FakeProvider::new();
    for number in 0..2048 {
        let id = format!("context-source:document:{number:04}");
        index.insert(context_source_entry(&id, ProviderState::Available));
        provider = provider.with_payload(&id, &format!("private payload {number}"));
    }

    let request = HorizonRequest::agent(Some(project_ref()));
    let horizon = index.horizon(&request);
    let result = index.search(&request, "1999");

    assert_eq!(horizon.len(), 2048);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].resource.as_str(), "context-source:document:1999");
    assert_eq!(provider.read_count, 0);
    assert!(index.search(&request, "private payload").is_empty());
}

#[test]
fn known_askable_retrieved_loaded_and_focused_are_independent_transitions() {
    let target = "context-source:document:target";
    let unrelated = "context-source:document:unrelated";
    let mut index = ContextSourceIndex::default();
    index.insert(context_source_entry(target, ProviderState::Available));
    index.insert(context_source_entry(unrelated, ProviderState::Available));
    let mut provider = FakeProvider::new().with_payload(target, "selected material");

    let outcome = index.retrieve(
        &ContextSourceReadRequest {
            resource: ResourceRef::parse(target).unwrap(),
            provider: provider_ref(),
            target: RetrievalTarget::LocalAgent,
        },
        &mut provider,
    );
    assert!(matches!(outcome, ContextSourceReadOutcome::Retrieved(_)));

    let target_ref = ResourceRef::parse(target).unwrap();
    let unrelated_ref = ResourceRef::parse(unrelated).unwrap();
    let target_after_retrieval = index.explain(&target_ref).unwrap();
    assert!(target_after_retrieval.disclosure.known_to_exist);
    assert!(target_after_retrieval.disclosure.askable);
    assert!(target_after_retrieval.disclosure.retrieved);
    assert!(!target_after_retrieval.disclosure.focused);
    assert!(!target_after_retrieval.operational.loaded);
    assert!(target_after_retrieval.operational.invoked);

    let unrelated_after_retrieval = index.explain(&unrelated_ref).unwrap();
    assert!(!unrelated_after_retrieval.disclosure.retrieved);
    assert!(!unrelated_after_retrieval.disclosure.focused);
    assert!(!unrelated_after_retrieval.operational.loaded);

    index.set_loaded(&target_ref, true).unwrap();
    let after_load = index.explain(&target_ref).unwrap();
    assert!(after_load.operational.loaded);
    assert!(!after_load.disclosure.focused);

    index.set_focused(&target_ref, true).unwrap();
    let after_focus = index.explain(&target_ref).unwrap();
    assert!(after_focus.disclosure.focused);
    assert!(after_focus.operational.loaded);
    assert!(!index.explain(&unrelated_ref).unwrap().disclosure.focused);
}

#[test]
fn disclosure_state_does_not_infer_operational_or_other_epistemic_states() {
    let id = ResourceRef::parse("context-source:nonlinear-disclosure").unwrap();
    let mut index = ContextSourceIndex::default();
    index.insert(context_source_entry(id.as_str(), ProviderState::Available));
    index
        .set_disclosure(
            &id,
            DisclosureState {
                exists: true,
                known_to_exist: false,
                askable: true,
                retrieved: false,
                focused: true,
            },
        )
        .unwrap();

    let explanation = index.explain(&id).unwrap();
    assert!(explanation.disclosure.exists);
    assert!(!explanation.disclosure.known_to_exist);
    assert!(explanation.disclosure.askable);
    assert!(!explanation.disclosure.retrieved);
    assert!(explanation.disclosure.focused);
    assert_eq!(explanation.operational, Default::default());
}

#[test]
fn canonical_identity_survives_provider_loss_degradation_reappearance_and_rebuild() {
    let id = "context-source:canon:architecture";
    let canonical = ResourceRef::parse(id).unwrap();

    let mut unavailable = ContextSourceIndex::default();
    unavailable.insert(context_source_entry(
        id,
        ProviderState::Unavailable {
            reason: "provider offline".into(),
        },
    ));
    let first = unavailable.explain(&canonical).unwrap();
    assert_eq!(first.resource, canonical);
    assert!(matches!(first.availability, Availability::Unavailable { .. }));

    let mut degraded_entry = context_source_entry(id, ProviderState::Available);
    degraded_entry.provider_descriptors.insert(
        provider_ref(),
        ContextSourceProviderDescriptor {
            status: ContextSourceProviderStatus::Degraded {
                reason: "semantic search unavailable; read remains available".into(),
            },
            capabilities: ContextSourceProviderCapabilities::with_operations([
                ContextSourceOperation::Discover,
                ContextSourceOperation::Read,
            ]),
        },
    );
    let mut degraded = ContextSourceIndex::default();
    degraded.insert(degraded_entry);
    let middle = degraded.explain(&canonical).unwrap();
    assert_eq!(middle.resource, canonical);
    assert_eq!(middle.availability, Availability::Available);
    assert!(matches!(
        &middle.provider_descriptors[&provider_ref()].status,
        ContextSourceProviderStatus::Degraded { .. }
    ));

    let mut rebuilt = ContextSourceIndex::default();
    rebuilt.insert(context_source_entry(id, ProviderState::Available));
    let final_state = rebuilt.explain(&canonical).unwrap();
    assert_eq!(final_state.resource, canonical);
    assert_eq!(final_state.availability, Availability::Available);
}

#[test]
fn all_structured_absence_meanings_are_distinguishable() {
    let cases = [
        (AbsenceKind::Open, "open"),
        (AbsenceKind::Latent, "latent"),
        (AbsenceKind::Unknown, "unknown"),
        (AbsenceKind::Irrelevant, "irrelevant"),
        (AbsenceKind::Bound, "bound"),
        (AbsenceKind::Missing, "missing"),
    ];
    let mut observed = BTreeSet::new();
    for (kind, suffix) in cases {
        let id = format!("context-source:absence:{suffix}");
        let resource = ResourceRef::parse(&id).unwrap();
        let mut entry = context_source_entry(&id, ProviderState::Available);
        entry.absence = Some(StructuredAbsence::new(kind, suffix));
        let mut index = ContextSourceIndex::default();
        index.insert(entry);
        let explanation = index.explain(&resource).unwrap();
        observed.insert(explanation.absence.unwrap().kind);
    }

    assert_eq!(observed.len(), 6);
}

#[test]
fn retrieval_carries_canonical_identity_revision_provider_provenance_and_eligibility() {
    let id = "context-source:paper:ql";
    let resource = ResourceRef::parse(id).unwrap();
    let mut index = ContextSourceIndex::default();
    index.insert(context_source_entry(id, ProviderState::Available));
    let mut provider = FakeProvider::new().with_payload(id, "paper material");

    let retrieval = match index.retrieve(
        &ContextSourceReadRequest {
            resource: resource.clone(),
            provider: provider_ref(),
            target: RetrievalTarget::Human,
        },
        &mut provider,
    ) {
        ContextSourceReadOutcome::Retrieved(value) => value,
        other => panic!("expected retrieval, got {other:?}"),
    };

    assert_eq!(retrieval.resource, resource);
    assert_eq!(retrieval.provider, provider_ref());
    assert_eq!(retrieval.revision.unwrap().as_str(), "rev:provider-2");
    assert_eq!(retrieval.freshness, Freshness::Current);
    assert_eq!(retrieval.eligibility, Eligibility::Eligible);
    assert!(retrieval.provenance.iter().any(|source| {
        source.authority == Some(SourceAuthority::Authored)
            && source.revision.as_ref().map(SourceRevision::as_str) == Some("rev:source-1")
    }));
    let explanation = index.explain(&resource).unwrap();
    assert!(explanation.provider_descriptors[&provider_ref()]
        .capabilities
        .supports(ContextSourceOperation::Read));
}

#[test]
fn privacy_excludes_never_agent_visible_material_and_denies_external_egress_before_read() {
    let hidden_id = "context-source:private:hidden";
    let local_id = "context-source:private:local-only";
    let mut hidden = context_source_entry(hidden_id, ProviderState::Available);
    hidden.privacy = ContextSourcePrivacy {
        agent_visibility: AgentVisibility::Hidden,
        external_egress: ExternalEgress::Denied,
    };
    let mut local = context_source_entry(local_id, ProviderState::Available);
    local.privacy = ContextSourcePrivacy {
        agent_visibility: AgentVisibility::Payload,
        external_egress: ExternalEgress::Denied,
    };

    let mut index = ContextSourceIndex::default();
    index.insert(hidden);
    index.insert(local);
    let agent_horizon = HorizonRequest::agent(Some(project_ref()));
    assert!(index.search(&agent_horizon, "hidden").is_empty());

    let mut provider = FakeProvider::new()
        .with_payload(hidden_id, "FORBIDDEN SECRET PAYLOAD")
        .with_payload(local_id, "local agent material");
    let hidden_outcome = index.retrieve(
        &ContextSourceReadRequest {
            resource: ResourceRef::parse(hidden_id).unwrap(),
            provider: provider_ref(),
            target: RetrievalTarget::LocalAgent,
        },
        &mut provider,
    );
    match hidden_outcome {
        ContextSourceReadOutcome::Absent(absence) => {
            assert_eq!(absence.kind, AbsenceKind::Bound);
            assert!(!absence.reason.contains("FORBIDDEN SECRET PAYLOAD"));
        }
        other => panic!("hidden source must be bound, got {other:?}"),
    }
    assert_eq!(provider.read_count, 0);

    let local_outcome = index.retrieve(
        &ContextSourceReadRequest {
            resource: ResourceRef::parse(local_id).unwrap(),
            provider: provider_ref(),
            target: RetrievalTarget::LocalAgent,
        },
        &mut provider,
    );
    assert!(matches!(local_outcome, ContextSourceReadOutcome::Retrieved(_)));
    assert_eq!(provider.read_count, 1);

    let external_outcome = index.retrieve(
        &ContextSourceReadRequest {
            resource: ResourceRef::parse(local_id).unwrap(),
            provider: provider_ref(),
            target: RetrievalTarget::ExternalProvider,
        },
        &mut provider,
    );
    assert!(matches!(
        external_outcome,
        ContextSourceReadOutcome::Absent(StructuredAbsence {
            kind: AbsenceKind::Bound,
            ..
        })
    ));
    assert_eq!(provider.read_count, 1);
}

#[test]
fn metadata_only_agent_disclosure_exposes_descriptor_not_payload() {
    let id = "context-source:private:metadata-only";
    let mut entry = context_source_entry(id, ProviderState::Available);
    entry.privacy = ContextSourcePrivacy {
        agent_visibility: AgentVisibility::MetadataOnly,
        external_egress: ExternalEgress::Denied,
    };
    let mut index = ContextSourceIndex::default();
    index.insert(entry);
    let request = HorizonRequest::agent(Some(project_ref()));
    assert_eq!(index.search(&request, "metadata-only").len(), 1);

    let mut provider = FakeProvider::new().with_payload(id, "SECRET BODY MUST NOT BE INDEXED");
    assert!(index.search(&request, "SECRET BODY").is_empty());
    let outcome = index.retrieve(
        &ContextSourceReadRequest {
            resource: ResourceRef::parse(id).unwrap(),
            provider: provider_ref(),
            target: RetrievalTarget::LocalAgent,
        },
        &mut provider,
    );
    assert!(matches!(
        outcome,
        ContextSourceReadOutcome::Absent(StructuredAbsence {
            kind: AbsenceKind::Bound,
            ..
        })
    ));
    assert_eq!(provider.read_count, 0);
}

#[test]
fn descriptor_search_is_deterministic_for_identical_explicit_inputs() {
    let mut index = ContextSourceIndex::default();
    for id in [
        "context-source:design:beta",
        "context-source:design:alpha",
        "context-source:design:gamma",
    ] {
        index.insert(context_source_entry(id, ProviderState::Available));
    }
    let request = HorizonRequest::human(Some(project_ref()));
    let first = index.search(&request, "design");
    let second = index.search(&request, "design");
    assert_eq!(first, second);
    assert_eq!(
        first
            .iter()
            .map(|hit| hit.resource.as_str())
            .collect::<Vec<_>>(),
        vec![
            "context-source:design:alpha",
            "context-source:design:beta",
            "context-source:design:gamma",
        ]
    );
}

#[test]
fn generic_context_source_seam_rejects_non_context_resources() {
    let descriptor = ResourceDescriptor::new(
        ResourceRef::parse("capability:not-a-source").unwrap(),
        ResourceKind::Capability,
        "not a source",
        "must not enter the ContextSource index",
    );
    let error = ContextSourceEntry::new(ResourceRecord::new(descriptor)).unwrap_err();
    assert_eq!(error.code(), "context_source.wrong_resource_kind");
}
