//! A SessionSpace benches a session it holds (O-I manual §19.15, the Agent
//! rack's chain list: "Activator — Benches the agent or the set: it stays in
//! the rack and is given no work"): the bench is authored on the space as a
//! typed mutation, staged and applied as every other relation is, carried by
//! the space's own record, and absent from a space that benches nothing, so a
//! record written before the bench keeps its bytes and its basis.
use aikit_core::resource::ResourceRef;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    stage_session_space, SessionSpaceAgentAttachmentIntent, SessionSpaceAuthoredState,
    SessionSpaceMutation,
};

fn session(name: &str) -> ResourceRef {
    ResourceRef::parse(format!("agent-session/{name}")).unwrap()
}

fn space_holding(sessions: &[&str]) -> SessionSpaceAuthoredState {
    let id = SessionSpaceRef::parse("session-space/rack").unwrap();
    let mut state = stage_session_space(
        None,
        SessionSpaceMutation::Create {
            id,
            label: Some("the rack".into()),
        },
    )
    .unwrap()
    .proposed;
    for name in sessions {
        state = stage_session_space(
            Some(&state),
            SessionSpaceMutation::AttachAgentSession {
                attachment: SessionSpaceAgentAttachmentIntent {
                    agent_session: session(name),
                    purpose: Some(format!("{name}'s chain")),
                    provenance: Vec::new(),
                },
            },
        )
        .unwrap()
        .proposed;
    }
    state
}

fn bench(
    state: &SessionSpaceAuthoredState,
    name: &str,
    benched: bool,
) -> aikit_core::Result<SessionSpaceAuthoredState> {
    stage_session_space(
        Some(state),
        SessionSpaceMutation::BenchAgentSession {
            agent_session: session(name),
            benched,
        },
    )
    .map(|preview| preview.proposed)
}

#[test]
fn a_space_benches_a_session_it_holds_and_lifts_the_bench() {
    let state = space_holding(&["ada", "cy"]);
    let staged = stage_session_space(
        Some(&state),
        SessionSpaceMutation::BenchAgentSession {
            agent_session: session("ada"),
            benched: true,
        },
    )
    .unwrap();
    assert!(staged.proposed.benched.contains(&session("ada")));
    assert!(
        staged.proposed.agent_sessions.contains_key(&session("ada")),
        "a benched session stays in the space"
    );
    assert_eq!(staged.proposed.revision, state.revision + 1);
    assert!(staged.changed.iter().any(|c| c.relation == "agent-session"
        && c.subject == "agent-session/ada"
        && c.change == "benched"));
    let wire = serde_json::to_value(&staged.proposed).unwrap();
    assert_eq!(wire["benched"], serde_json::json!(["agent-session/ada"]));
    let back: SessionSpaceAuthoredState = serde_json::from_value(wire).unwrap();
    assert_eq!(back, staged.proposed);

    let lifted = stage_session_space(
        Some(&staged.proposed),
        SessionSpaceMutation::BenchAgentSession {
            agent_session: session("ada"),
            benched: false,
        },
    )
    .unwrap();
    assert!(lifted.proposed.benched.is_empty());
    assert!(lifted
        .changed
        .iter()
        .any(|c| c.subject == "agent-session/ada" && c.change == "bench-lifted"));
}

#[test]
fn a_space_that_benches_nothing_carries_no_bench_and_keeps_its_basis() {
    let state = space_holding(&["ada"]);
    let wire = serde_json::to_value(&state).unwrap();
    assert!(wire.get("benched").is_none(), "no bench, no field: {wire}");
    let read: SessionSpaceAuthoredState = serde_json::from_value(wire).unwrap();
    assert_eq!(read.basis().unwrap(), state.basis().unwrap());
}

#[test]
fn a_session_the_space_does_not_hold_is_refused_in_words() {
    let state = space_holding(&["ada"]);
    let refused = bench(&state, "nobody", true).unwrap_err();
    assert_eq!(refused.code(), "session_space.bench_unattached");
    assert!(
        refused
            .to_string()
            .contains("agent-session/nobody is not a session of session-space/rack"),
        "{refused}"
    );
}

#[test]
fn a_detached_session_leaves_the_bench_with_it() {
    let state = bench(&space_holding(&["ada", "cy"]), "ada", true).unwrap();
    let detached = stage_session_space(
        Some(&state),
        SessionSpaceMutation::DetachAgentSession {
            agent_session: session("ada"),
        },
    )
    .unwrap();
    assert!(
        detached.proposed.benched.is_empty(),
        "no bench outlives its session's attachment"
    );
}

#[test]
fn a_bench_on_a_session_no_longer_held_is_invalid_state() {
    let mut state = bench(&space_holding(&["ada"]), "ada", true).unwrap();
    state.agent_sessions.clear();
    let refused = state.validate().unwrap_err();
    assert_eq!(refused.code(), "session_space.bench_unattached");
}
