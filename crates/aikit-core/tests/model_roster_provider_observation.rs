use serde_json::Value;

#[test]
fn current_provider_fixture_is_provenance_bearing_and_not_fitness() {
    let value: Value =
        serde_json::from_str(include_str!("fixtures/openai-gpt-5.4-2026-08-17.json")).unwrap();

    assert_eq!(value["provider_ref"], "provider:openai");
    assert_eq!(value["model_ref"], "model:gpt-5.4");
    assert_eq!(value["pricing"]["currency"], "USD");
    assert_eq!(value["pricing"]["unit"], "1m-tokens");
    assert_eq!(value["pricing"]["input"], 2.50);
    assert_eq!(value["pricing"]["cached_input"], 0.25);
    assert_eq!(value["pricing"]["output"], 15.00);
    assert_eq!(value["context"]["context_window_tokens"], 1_050_000);
    assert_eq!(value["features"]["structured_outputs"], true);
    assert_eq!(value["modalities"]["image_input"], true);
    assert!(value["source"].as_str().unwrap().starts_with("https://"));
    assert!(value["observed_at"]
        .as_str()
        .unwrap()
        .starts_with("2026-08-17"));

    let object = value.as_object().unwrap();
    assert!(!object.contains_key("fitness"));
    assert!(!object.contains_key("authored_preference"));
    assert!(!object.contains_key("frecency"));
}
