use aikit_core::jev::{
    estimated_input_tokens_range, JevLimits, JevRequest, JevResponse, JevTariff, TokenUsage,
};
use serde_json::{json, Value};

fn request() -> JevRequest {
    JevRequest::parse(&serde_json::to_vec(&json!({
        "model":"jev-1.13.0", "state":{"undertaking":"Choose a suitable time for a maintenance window","observations":["an interactive user is active","backup is incomplete"]},
        "questions": {
            "defer":{"type":"noul","instructions":"Should maintenance wait until the backup completes?"},
            "action":{"type":"choice","instructions":{"question":"Which action fits the observations?"},"criteria":{"wait":"Preserve active work","stop":null}},
            "risk":{"type":"score","instructions":"How disruptive would a restart be?","criteria":["low","moderate","high"]}
        }
    })).unwrap()).unwrap()
}
fn response() -> Value {
    json!({"model":"jev-1.13.0","answers":{
        "defer":{"type":"noul","noul":0.96},
        "action":{"type":"choice","choice":"wait","probabilities":{"wait":0.9,"stop":0.1},"confidence":0.8},
        "risk":{"type":"score","score":1.7,"legend":{"0":"low","1":"moderate","2":"high"},"probabilities":{"0":0.0,"1":0.3,"2":0.7},"confidence":0.5}
    },"usage":{"input_tokens":700,"output_tokens":80}})
}
fn parse(value: &Value) -> aikit_core::Result<JevResponse> {
    let response = JevResponse::parse(&serde_json::to_vec(value).unwrap())?;
    response.validate_for(&request())?;
    Ok(response)
}
fn parse_only(value: &Value) -> aikit_core::Result<JevResponse> {
    JevResponse::parse(&serde_json::to_vec(value).unwrap())
}
#[test]
fn general_questions_outside_document_practice_are_first_class() {
    let parsed = parse(&response()).unwrap();
    assert_eq!(parsed.model, "jev-1.13.0");
    assert_eq!(parsed.usage.input_tokens, 700);
    assert_eq!(parsed.answers.len(), 3);
    assert!(request().digest().unwrap().starts_with("blake3:"));
}
#[test]
fn missing_surplus_or_wrong_kind_answers_never_become_success() {
    for key in ["defer", "action", "risk"] {
        let mut value = response();
        value["answers"].as_object_mut().unwrap().remove(key);
        assert_eq!(parse(&value).unwrap_err().code(), "jev.invalid_answer");
    }
    let mut value = response();
    value["answers"]["unexpected"] = json!({"type":"noul","noul":0.7});
    assert!(parse(&value).is_err());
    value = response();
    value["answers"]["risk"] = json!({"type":"noul","noul":0.7});
    assert!(parse(&value).is_err());
}
#[test]
fn a_missing_usage_field_is_not_zero_cost() {
    let mut value = response();
    value.as_object_mut().unwrap().remove("usage");
    assert!(parse(&value).is_err());
    value = response();
    value["usage"]
        .as_object_mut()
        .unwrap()
        .remove("input_tokens");
    assert!(parse(&value).is_err());
    value = response();
    value["usage"]["input_tokens"] = json!(-1);
    assert!(parse(&value).is_err());
}
#[test]
fn distributions_must_be_complete_normalised_and_have_the_declared_winner() {
    for bad in [
        json!({"wait":0.9}),
        json!({"wait":0.8,"stop":0.1}),
        json!({"wait":1.1,"stop":-0.1}),
        json!({"wait":0.1,"stop":0.9}),
    ] {
        let mut value = response();
        value["answers"]["action"]["probabilities"] = bad;
        assert!(parse(&value).is_err());
    }
    let mut value = response();
    value["answers"]["action"]["choice"] = json!("a-new-option");
    assert!(parse(&value).is_err());
    value = response();
    value["answers"]["action"]["confidence"] = json!(1.1);
    assert!(parse(&value).is_err());
}
#[test]
fn score_uses_the_actual_ordered_rubric_and_expected_value() {
    let mut value = response();
    value["answers"]["risk"]["score"] = json!(0.1);
    assert!(parse(&value).is_err());
    value = response();
    value["answers"]["risk"]["legend"]["0"] = json!("high");
    assert!(parse(&value).is_err());
    value = response();
    value["answers"]["risk"]["probabilities"] = json!({"0":0.1,"1":0.1,"unexpected":0.8});
    assert!(parse(&value).is_err());
}
#[test]
fn provider_identity_law_lives_at_the_typesafe_boundary_not_the_protocol() {
    // The provider-neutral protocol requires a usable model identity but does
    // not dictate its shape: a local SystemOne-compatible server admits its
    // own model names.
    for (model, neutral_ok) in [
        ("kev-latest", true),
        ("jaredpalmer/kev-0.8b@r10", true),
        ("jev-1.13.0", true),
        ("", false),
    ] {
        let mut value = response();
        value["model"] = json!(model);
        assert_eq!(parse(&value).is_ok(), neutral_ok, "neutral parse of {model}");
    }
    // The TypeSafe boundary keeps its own law: concrete evaluated `jev-x.y.z`
    // versions only, never an echoed alias or a foreign identity.
    for model in [
        "kev-latest",
        "jev-latest",
        "jev-preview",
        "another-model",
        "jev-1.13",
        "jev-1.x.0",
    ] {
        let mut value = response();
        value["model"] = json!(model);
        let parsed = parse(&value).expect("parseable response");
        assert!(
            parsed.validate_typesafe_for(&request()).is_err(),
            "TypeSafe law must refuse {model}"
        );
    }
    let foreign_request_value = json!({
        "model": "kev-latest", "state": {},
        "questions": {"q": {"type": "noul", "instructions": "Question"}}
    });
    let foreign_request = JevRequest::parse(&serde_json::to_vec(&foreign_request_value).unwrap())
        .expect("a local model selector is a valid protocol request");
    let parsed = parse(&response()).unwrap();
    assert!(parsed.validate_for(&foreign_request).is_err());
}

#[test]
fn bf16_serving_precision_is_admitted_at_the_endpoint_standing_never_at_the_hosted_one() {
    // A bf16 endpoint's softmax sums to ~1.0001: unrepresentable at the hosted
    // 1e-5 bound, honest at the documented endpoint bound.
    let mut value = response();
    value["answers"]["action"]["probabilities"] = json!({"wait": 0.9001, "stop": 0.1});
    let parsed = parse_only(&value).expect("structurally parseable response");
    assert!(parsed.validate_for(&request()).is_err(), "hosted-exact law refuses the drift");
    assert!(parsed
        .validate_for_with_tolerance(&request(), aikit_core::jev::ENDPOINT_TOLERANCE)
        .is_ok(), "endpoint standing admits bf16 serving precision");
    // Coverage and range laws do not widen with the bound.
    let mut uncovered = response();
    uncovered["answers"]["action"]["probabilities"] = json!({"wait": 0.9001});
    let parsed = parse(&uncovered).unwrap_err();
    assert_eq!(parsed.code(), "jev.invalid_answer");
}

#[test]
fn a_local_model_selector_is_a_valid_request_but_not_a_typesafe_one() {
    let local = json!({
        "model": "kev-latest", "state": {"ticket": "double charge"},
        "questions": {"billing": {"type": "noul", "instructions": "Is this billing?"}}
    });
    let request = JevRequest::parse(&serde_json::to_vec(&local).unwrap()).unwrap();
    request.validate().unwrap();
}
#[test]
fn duplicate_json_members_are_rejected_at_every_level() {
    let raw = br#"{"model":"jev-1.13.0","state":{},"questions":{"q":{"type":"noul","instructions":"Question","instructions":"Substitution"}}}"#;
    assert!(JevRequest::parse(raw).is_err());
    let raw = br#"{"model":"jev-1.13.0","state":{},"questions":{"q":{"type":"noul","instructions":"Question"},"q":{"type":"noul","instructions":"Another"}}}"#;
    assert!(JevRequest::parse(raw).is_err());
    let raw = br#"{"model":"jev-1.13.0","model":"jev-1.13.0","answers":{},"usage":{"input_tokens":1,"output_tokens":1}}"#;
    assert!(JevResponse::parse(raw).is_err());
}
#[test]
fn validation_is_not_only_a_parser_guard() {
    let mut held = request();
    held.state = Value::Bool(true);
    assert!(held.validate().is_err());
    held = request();
    held.questions.clear();
    assert!(held.validate().is_err());
}
#[test]
fn reserve_spend_before_an_attempt_and_do_not_overflow_tariff_arithmetic() {
    let mut limits = JevLimits {
        timeout_ms: 5000,
        max_attempts: 2,
        max_total_reserved_microusd: 6000,
        tariff: JevTariff {
            model_version: "jev-1.13.0".into(),
            source: "https://docs.typesafe.ai/models#current-models; checked 2026-09-22".into(),
            max_input_tokens_per_attempt: 64_000,
            max_output_tokens_per_attempt: 64_000,
            input_microusd_per_million_tokens: 42_000,
            output_microusd_per_million_tokens: 0,
        },
    };
    limits.validate(&request()).unwrap();
    assert_eq!(limits.tariff.reservation().unwrap(), 2688);
    assert_eq!(
        limits
            .tariff
            .cost_microusd(&TokenUsage {
                input_tokens: 700,
                output_tokens: 80
            })
            .unwrap(),
        30
    );
    limits.max_total_reserved_microusd = 1000;
    assert_eq!(
        limits.validate(&request()).unwrap_err().code(),
        "jev.budget_exhausted"
    );
    limits.max_total_reserved_microusd = 6000;
    limits.timeout_ms = 0;
    assert!(limits.validate(&request()).is_err());
    limits.tariff.input_microusd_per_million_tokens = u64::MAX;
    limits.tariff.output_microusd_per_million_tokens = u64::MAX;
    assert!(limits
        .tariff
        .cost_microusd(&TokenUsage {
            input_tokens: u64::MAX,
            output_tokens: u64::MAX
        })
        .is_err());
}

#[test]
fn the_input_token_estimate_is_a_range_and_never_refuses_by_itself() {
    let limits: JevLimits = serde_json::from_value(json!({
        "timeout_ms": 1000,
        "max_attempts": 1,
        "max_total_reserved_microusd": 5000,
        "tariff": {
            "model_version": "jev-1.13.0",
            "source": "observed provider ceiling",
            "max_input_tokens_per_attempt": 32_768,
            "max_output_tokens_per_attempt": 4_096,
            "input_microusd_per_million_tokens": 42_000,
            "output_microusd_per_million_tokens": 0
        }
    }))
    .unwrap();
    let mut large = request();
    large.state = json!({"catalogue": "x".repeat(100_000)});
    let (low, high) = estimated_input_tokens_range(&large);
    assert!(low < high && low > 20_000 && high > 32_768, "{low}-{high}");
    // A byte ratio cannot decide it: a request the provider may accept is not refused locally.
    assert!(limits.validate(&large).is_ok());
}
