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
    JevResponse::parse_for(&serde_json::to_vec(value).unwrap(), &request())
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
fn actual_version_is_required_instead_of_echoed_alias() {
    for model in [
        "",
        "jev-latest",
        "jev-preview",
        "another-model",
        "jev-1.13",
        "jev-1.x.0",
    ] {
        let mut value = response();
        value["model"] = json!(model);
        assert!(parse(&value).is_err());
    }
}
#[test]
fn duplicate_json_members_are_rejected_at_every_level() {
    let raw = br#"{"model":"jev-1.13.0","state":{},"questions":{"q":{"type":"noul","instructions":"Question","instructions":"Substitution"}}}"#;
    assert!(JevRequest::parse(raw).is_err());
    let raw = br#"{"model":"jev-1.13.0","state":{},"questions":{"q":{"type":"noul","instructions":"Question"},"q":{"type":"noul","instructions":"Another"}}}"#;
    assert!(JevRequest::parse(raw).is_err());
    let raw = br#"{"model":"jev-1.13.0","model":"jev-1.13.0","answers":{},"usage":{"input_tokens":1,"output_tokens":1}}"#;
    assert!(JevResponse::parse_for(raw, &request()).is_err());
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
