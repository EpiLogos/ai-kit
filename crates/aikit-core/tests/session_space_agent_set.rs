//! A SessionSpace names the Central agent set it runs (O-I #684, the
//! Agentset rack's Join): the tie is authored on the space as a typed
//! mutation, bound and unbound through the ordinary stage, carried by the
//! space's own record, and absent from a space that names none, so a record
//! written before the tie keeps its bytes and its basis.
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    stage_session_space, SessionSpaceAgentSetBinding, SessionSpaceAuthoredState,
    SessionSpaceMutation,
};

fn space() -> SessionSpaceAuthoredState {
    let id = SessionSpaceRef::parse("session-space/central-team").unwrap();
    stage_session_space(
        None,
        SessionSpaceMutation::Create {
            id,
            label: Some("Central native team".into()),
        },
    )
    .unwrap()
    .proposed
}

fn tie(reference: &str) -> SessionSpaceAgentSetBinding {
    SessionSpaceAgentSetBinding {
        agent_set_ref: reference.into(),
        revision: Some("r1".into()),
        provenance: vec!["test fixture".into()],
    }
}

#[test]
fn a_space_binds_the_agent_set_it_runs_and_unbinds_it() {
    let state = space();
    let bound = stage_session_space(
        Some(&state),
        SessionSpaceMutation::BindAgentSet {
            binding: tie("anima"),
        },
    )
    .unwrap();
    assert_eq!(bound.proposed.agent_set, Some(tie("anima")));
    assert_eq!(bound.proposed.revision, state.revision + 1);
    assert!(bound
        .changed
        .iter()
        .any(|c| c.relation == "agent-set" && c.subject == "anima" && c.change == "bound"));
    let wire = serde_json::to_value(&bound.proposed).unwrap();
    assert_eq!(wire["agent_set"]["agent_set_ref"], "anima");
    assert_eq!(wire["agent_set"]["revision"], "r1");
    let back: SessionSpaceAuthoredState = serde_json::from_value(wire).unwrap();
    assert_eq!(back, bound.proposed);

    let unbound =
        stage_session_space(Some(&bound.proposed), SessionSpaceMutation::UnbindAgentSet).unwrap();
    assert_eq!(unbound.proposed.agent_set, None);
    assert!(unbound
        .changed
        .iter()
        .any(|c| c.relation == "agent-set" && c.subject == "anima" && c.change == "unbound"));
}

#[test]
fn a_space_that_names_no_set_carries_no_tie_and_keeps_its_basis() {
    let state = space();
    let wire = serde_json::to_value(&state).unwrap();
    assert!(wire.get("agent_set").is_none(), "no tie, no field: {wire}");
    // A record written before the tie reads back the same, basis and all.
    let read: SessionSpaceAuthoredState = serde_json::from_value(wire).unwrap();
    assert_eq!(read.basis().unwrap(), state.basis().unwrap());
}

#[test]
fn a_tie_that_names_no_set_is_refused() {
    let state = space();
    for reference in ["", "  ", &"x".repeat(300), "anima\nset"] {
        let refused = stage_session_space(
            Some(&state),
            SessionSpaceMutation::BindAgentSet {
                binding: tie(reference),
            },
        );
        assert!(refused.is_err(), "{reference:?}");
    }
}
