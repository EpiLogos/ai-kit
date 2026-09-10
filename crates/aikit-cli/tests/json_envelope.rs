//! The JSON envelope is a public interface, so it is pinned against literal
//! expected documents rather than field-by-field assertions. A change to the
//! shape must change these documents, deliberately.
//!
//! Comparison is on parsed `serde_json::Value`, not on byte strings: key order in
//! an object is not part of the contract, but the set of keys and their values
//! is.

use aikit_cli::json::{self, EnvelopeContext};
use aikit_core::AikitError;
use serde_json::json;

#[test]
fn a_success_envelope_carries_schema_context_data_and_warnings() {
    let ctx = EnvelopeContext {
        context_id: Some("ctx_01HZY".to_string()),
        session_id: Some("ses_01HZY".to_string()),
        project_root: Some("/work/payments".to_string()),
    };

    let envelope = json::success(&ctx, json!({ "count": 3 }), vec!["stale index".to_string()]);

    let expected = json!({
        "schema": 1,
        "ok": true,
        "context": {
            "context_id": "ctx_01HZY",
            "session_id": "ses_01HZY",
            "project_root": "/work/payments"
        },
        "data": { "count": 3 },
        "warnings": ["stale index"]
    });

    assert_eq!(envelope, expected);
}

#[test]
fn a_context_outside_any_project_or_session_still_reports_all_three_keys() {
    let ctx = EnvelopeContext {
        context_id: Some("ctx_01HZY".to_string()),
        session_id: None,
        project_root: None,
    };

    let envelope = json::success(&ctx, json!({}), vec![]);

    let expected = json!({
        "schema": 1,
        "ok": true,
        "context": {
            "context_id": "ctx_01HZY",
            "session_id": null,
            "project_root": null
        },
        "data": {},
        "warnings": []
    });

    assert_eq!(envelope, expected);
}

#[test]
fn a_resolution_failure_serialises_code_message_and_details() {
    let err = AikitError::new(
        "resolution.required_capability_disabled",
        "cannot enable skill/rust/review: required capability script/test/pytest is disabled by \
         the session scope",
    )
    .with("capability", "script/test/pytest")
    .with("required_by", "skill/rust/review")
    .with("scope", "session")
    .with("origin", "overlay.toml");

    let envelope = json::failure(&err);

    let expected = json!({
        "schema": 1,
        "ok": false,
        "error": {
            "code": "resolution.required_capability_disabled",
            "message": "cannot enable skill/rust/review: required capability script/test/pytest \
                        is disabled by the session scope",
            "details": {
                "capability": "script/test/pytest",
                "required_by": "skill/rust/review",
                "scope": "session",
                "origin": "overlay.toml"
            }
        }
    });

    assert_eq!(envelope, expected);
}

#[test]
fn exit_codes_follow_the_published_table() {
    // 0 ok is not an error; the table below is the error mapping.
    assert_eq!(json::exit_code(&err("resolution.required_capability_disabled")), 3);
    assert_eq!(json::exit_code(&err("resolution.conflict")), 3);
    assert_eq!(json::exit_code(&err("policy.denied")), 4);
    assert_eq!(json::exit_code(&err("trust.required")), 5);
    assert_eq!(json::exit_code(&err("lock.busy")), 6);
    assert_eq!(json::exit_code(&err("cli.usage")), 2);
    // Anything without a dedicated code is a generic failure.
    assert_eq!(json::exit_code(&err("generation.write_failed")), 1);
    assert_eq!(json::exit_code(&err("trust.query_failed")), 1);
}

fn err(code: &'static str) -> AikitError {
    AikitError::new(code, "message")
}
