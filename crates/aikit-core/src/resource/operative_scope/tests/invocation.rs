use super::*;
use crate::resource::operative_scope::invocation::{
    ScopedActionAttemptEvidence, ScopedActionAttemptOutcome, ScopedActionAuthority,
    ScopedActionInput, ScopedActionInvocation, ScopedActionReturnEvidence, ScopedInputDisposition,
    ScopedReturnArrival,
};
use crate::resource::{
    ResourceDescriptor, ResourceRecord, ResourceSource, SourceAuthority, SourceRef, SourceState,
};

fn executable_index() -> MemoryResourceIndex {
    let mut index = super::index();
    let mut action = ResourceDescriptor::new(
        ResourceRef::parse("action/verify").unwrap(),
        ResourceKind::Action,
        "verify",
        "verify the selected subject",
    );
    action.sources.push(ResourceSource {
        source: SourceRef::parse("source/aikit/test-action").unwrap(),
        revision: Some(SourceRevision::parse("r1").unwrap()),
        locator: None,
        authority: Some(SourceAuthority::Authored),
        state: SourceState::Available,
    });
    index.insert(ResourceRecord::new(action));
    index
}

fn admitted() -> (
    ScopedActionInvocation,
    ScopedContextResolution,
    ObservingProvider,
    ContextResolution,
) {
    let index = executable_index();
    let context = super::context(&index);
    let provider = ObservingProvider::new();
    let scoped = compose_scoped_context(&super::request(), &index, &context, 16, &provider).unwrap();
    let candidate = scoped
        .action(&ResourceRef::parse("action/verify").unwrap(), &index)
        .unwrap();
    let subject = ResourceRef::parse("project/one").unwrap();
    let authority = ScopedActionAuthority {
        authority_ref: ResourceRef::parse("authority/test-owner").unwrap(),
        authority_revision: Some(SourceRevision::parse("authority-r1").unwrap()),
        action: candidate.action.clone(),
        subject: subject.clone(),
        granted: true,
        unattended: false,
        evidence: vec![ResourceRef::parse("evidence/authority/one").unwrap()],
    };
    let input = ScopedActionInput {
        input_ref: ResourceRef::parse("input/prompt/one").unwrap(),
        revision: Some(SourceRevision::parse("input-r1").unwrap()),
        digest: "a".repeat(64),
        disposition: ScopedInputDisposition::NewInput,
    };
    let invocation =
        ScopedActionInvocation::admit(&scoped, &candidate, subject, authority, input).unwrap();
    (invocation, scoped, provider, context)
}

#[test]
fn retry_keeps_invocation_identity_while_attempt_identity_advances() {
    let (invocation, _, _, _) = admitted();
    let first = ScopedActionAttemptEvidence::finish(
        &invocation,
        1,
        10,
        20,
        ScopedActionAttemptOutcome::Failed {
            code: "run.spawn_failed".into(),
            message: "provider unavailable".into(),
        },
    )
    .unwrap();
    let retry = ScopedActionAttemptEvidence::finish(
        &invocation,
        2,
        30,
        40,
        ScopedActionAttemptOutcome::Completed {
            status: 0,
            detached: false,
            result_digest: "b".repeat(64),
        },
    )
    .unwrap();
    assert_eq!(first.invocation_ref, retry.invocation_ref);
    assert_ne!(first.attempt_ref, retry.attempt_ref);
    assert_eq!(first.ordinal, 1);
    assert_eq!(retry.ordinal, 2);
}

#[test]
fn replay_is_not_new_input_even_with_identical_content() {
    let (new_input, scoped, _, _) = admitted();
    let candidate = ResolvedActionCandidate {
        action: new_input.action.clone(),
        horizon: None,
        relation: None,
        available_in_context: true,
    };
    let replay = ScopedActionInvocation::admit(
        &scoped,
        &candidate,
        new_input.subject.clone(),
        new_input.authority.clone(),
        ScopedActionInput {
            input_ref: new_input.input.input_ref.clone(),
            revision: new_input.input.revision.clone(),
            digest: new_input.input.digest.clone(),
            disposition: ScopedInputDisposition::Replay {
                original_invocation: new_input.invocation_ref.clone(),
            },
        },
    )
    .unwrap();
    assert_ne!(new_input.invocation_ref, replay.invocation_ref);
    assert!(matches!(
        replay.input.disposition,
        ScopedInputDisposition::Replay { .. }
    ));
}

#[test]
fn cancelled_attempt_can_receive_late_stale_and_reordered_returns_without_rewriting_origin() {
    let (invocation, scoped, provider, context) = admitted();
    let cancelled = ScopedActionAttemptEvidence::finish(
        &invocation,
        1,
        100,
        120,
        ScopedActionAttemptOutcome::Cancelled {
            reason: "caller cancelled".into(),
        },
    )
    .unwrap();

    provider.current.borrow_mut().sources[0].revision = SourceRevision::parse("source-r2").unwrap();
    let completion = scoped.completion_observations(&context, &provider);
    assert!(matches!(
        completion[0].observation,
        ScopeObservation::Stale { .. }
    ));

    let second_from_producer = ScopedActionReturnEvidence::observe(
        &invocation,
        &cancelled,
        completion.clone(),
        ScopedReturnArrival {
            observed_at_unix_ms: 200,
            producer_sequence: Some(2),
            delivery_sequence: 1,
        },
        Some("c".repeat(64)),
        vec![ResourceRef::parse("evidence/return/two").unwrap()],
    )
    .unwrap();
    let first_from_producer = ScopedActionReturnEvidence::observe(
        &invocation,
        &cancelled,
        completion,
        ScopedReturnArrival {
            observed_at_unix_ms: 220,
            producer_sequence: Some(1),
            delivery_sequence: 2,
        },
        Some("d".repeat(64)),
        vec![ResourceRef::parse("evidence/return/one").unwrap()],
    )
    .unwrap();

    assert_eq!(second_from_producer.original_scopes, invocation.original_scopes);
    assert_eq!(first_from_producer.original_scopes, invocation.original_scopes);
    assert_eq!(second_from_producer.arrival.producer_sequence, Some(2));
    assert_eq!(first_from_producer.arrival.producer_sequence, Some(1));
    assert_ne!(second_from_producer.return_ref, first_from_producer.return_ref);
}

#[test]
fn denied_or_mismatched_authority_never_becomes_an_invocation() {
    let (invocation, scoped, _, _) = admitted();
    let candidate = ResolvedActionCandidate {
        action: invocation.action.clone(),
        horizon: None,
        relation: None,
        available_in_context: true,
    };
    let mut denied = invocation.authority.clone();
    denied.granted = false;
    assert!(ScopedActionInvocation::admit(
        &scoped,
        &candidate,
        invocation.subject.clone(),
        denied,
        invocation.input.clone(),
    )
    .is_err());

    let mut wrong_subject = invocation.authority.clone();
    wrong_subject.subject = ResourceRef::parse("project/other").unwrap();
    assert!(ScopedActionInvocation::admit(
        &scoped,
        &candidate,
        invocation.subject,
        wrong_subject,
        invocation.input,
    )
    .is_err());
}
