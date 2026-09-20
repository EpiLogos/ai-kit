//! Controlled native validation, not a person's Agent or live-model evidence.
use aikit_cli::direct_agent_session::{PrepareRequest, find, validate_review};
use aikit_core::ResourceRef;
use serde_json::{Value, json};
fn fixture() -> (PrepareRequest, ResourceRef, Value) {
    let request = PrepareRequest {
        request_id: "controlled-request-12345".into(),
        profile_ref: "agent-profile:test".into(),
        expected_revision: "r1".into(),
        expected_content_digest: format!("sha256:{}", "a".repeat(64)),
        expected_acceptance_ref: "agent-profile-acceptance:controlled".into(),
    };
    let agent = ResourceRef::parse("agent:test").unwrap();
    let profile = json!({"schema":"central.agent-profile/v1","ref":request.profile_ref,"revision":"r1","agent_ref":agent,"scope":"personal","world_ref":"central:root","ratified_world_refs":["central:root"],"purpose":"Read explicit sources","intent_provenance":{"schema":"central.agent-profile-provenance/v1","intent_expression":"Read explicit sources","origin_action":"agent-profile.express","authorship":"generated-proposal","recognition":"unrecognised"}});
    let review = json!({"schema":"central.agent-profile-review/v1","accepted":true,"profile":profile,"scope_ref":"control:root","content_digest":request.expected_content_digest,"acceptance":{"schema":"central.agent-profile-acceptance/v1","acceptance_ref":request.expected_acceptance_ref,"profile_ref":request.profile_ref,"agent_ref":agent,"profile_revision":"r1","content_digest":request.expected_content_digest,"scope_ref":"control:root","principal_ref":"human:controlled-test","authority_ref":"authority:controlled-test","authority_revision":"policy:1"}});
    (request, agent, review)
}
#[test]
fn native_exact_acceptance_does_not_rewrite_generated_standing() {
    let (request, agent, review) = fixture();
    validate_review(&request, &agent, &review).unwrap();
    assert_eq!(
        review["profile"]["intent_provenance"]["recognition"],
        "unrecognised"
    );
}
#[test]
fn unaccepted_stale_or_different_identity_receipts_are_not_admission() {
    let (request, agent, review) = fixture();
    for (pointer, value) in [
        ("/accepted", json!(false)),
        ("/profile/revision", json!("r2")),
        ("/content_digest", json!("sha256:changed")),
        ("/profile/agent_ref", json!("agent:other")),
        ("/acceptance/agent_ref", json!("agent:other")),
        ("/acceptance/scope_ref", json!("project:other")),
        ("/acceptance/acceptance_ref", json!("acceptance:other")),
        ("/acceptance/authority_ref", json!("")),
        ("/acceptance/principal_ref", Value::Null),
    ] {
        let mut changed = review.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            validate_review(&request, &agent, &changed).is_err(),
            "{pointer}"
        );
    }
}
#[test]
fn ingress_cannot_supply_agent_identity_launch_argv_or_human_token() {
    let (request, _, _) = fixture();
    for field in [
        "agent_ref",
        "agent_session",
        "accepted",
        "argv",
        "token",
        "model",
        "authority_granted",
    ] {
        let mut value = serde_json::to_value(&request).unwrap();
        value[field] = json!("forged");
        assert!(
            serde_json::from_value::<PrepareRequest>(value).is_err(),
            "{field}"
        );
    }
}
#[test]
fn reading_an_unknown_request_never_creates_a_session_or_store() {
    let temp = tempfile::tempdir().unwrap();
    let home = aikit_store::AikitHome::at(temp.path().join("absent"));
    assert_eq!(
        find(&home, "controlled-request-12345").unwrap(),
        Value::Null
    );
    assert!(!home.root().exists());
    assert!(find(&home, "../redirect").is_err());
}
#[test]
#[cfg(unix)]
fn direct_binding_read_refuses_directory_redirect() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
    home.ensure_layout().unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, home.state().join("encounter-agents")).unwrap();
    assert!(find(&home, "controlled-request-12345").is_err());
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}
