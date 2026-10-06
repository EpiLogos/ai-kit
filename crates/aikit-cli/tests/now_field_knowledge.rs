//! The CLI knowledge surface reaches the NOW field through the one search
//! service: `aikit knowledge search` over a Central-shaped ground returns
//! NOW-field hits without any new command, and the provider's identity is
//! disclosed through `knowledge status`.

use std::fs;

use aikit_cli::app::Service;
use aikit_core::KnowledgeAddress;
use aikit_store::AikitHome;
use tempfile::TempDir;

#[test]
fn knowledge_search_reaches_the_now_field_through_the_one_service() {
    if !aikit_adapters::ripgrep::available() {
        eprintln!("ripgrep is not installed; the NOW-field CLI integration test skipped");
        return;
    }
    let temp = TempDir::new().unwrap();
    let clearing = temp.path().join("Control/agents/now/clearings/abc");
    fs::create_dir_all(&clearing).unwrap();
    fs::write(
        clearing.join("now.json"),
        r#"{"schema":"central.now-clearing/v1","task_ref":"control:task:telemetry-correlation-proof","purpose":"the telemetry correlation proof joins Factory attempts to the Day"}"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("Work")).unwrap();

    // The NOW field answers through the attached native owner, and the owner
    // answers its registered members: admit the clearing record the search
    // asserts, against the root map's basis.
    let ctrl = std::env::var_os("CENTRAL_CTRL_BIN")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("ctrl"));
    let action = |action: &str, input: serde_json::Value| {
        let output = std::process::Command::new(&ctrl)
            .args([
                "--json",
                "--root",
                temp.path().to_str().unwrap(),
                "action",
                "run",
                action,
                &input.to_string(),
            ])
            .output()
            .expect("the real ctrl runs the fixture map action");
        let envelope: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("native action receipt parses");
        assert_eq!(envelope["ok"], true, "fixture action {action}: {envelope}");
        envelope["data"].clone()
    };
    action("central.init", serde_json::json!({}));
    let basis = action(
        "central.file-map.inspect",
        serde_json::json!({"resources": false}),
    )["result"]["revision"]
        .as_str()
        .expect("the map basis carries its revision")
        .to_owned();
    action(
        "central.file-map.register",
        serde_json::json!({
            "path": "Control/agents/now/clearings/abc/now.json",
            "expected_revision": basis
        }),
    );

    let home = AikitHome::at(temp.path().join("aikit-home"));
    let service =
        Service::open(home, temp.path(), |_| None).expect("open production application service");

    let status = service.knowledge_status().unwrap();
    let now_field = status
        .sources
        .iter()
        .find(|provider| provider.provider.as_str() == "provider/source-pool/now-field")
        .expect("the NOW-field provider is materialised");
    assert!(
        now_field.available,
        "ripgrep is present, so the provider is available"
    );
    // With the native owner attached, the status discloses the owner
    // delegation; the scope allowlist remains the un-owned variant's phrase.
    assert!(
        now_field.detail.contains("attached native owner"),
        "status discloses that identity and payload ride the owner, got: {}",
        now_field.detail
    );

    let result = service
        .knowledge_search("telemetry correlation proof", 50)
        .unwrap();
    let hit = result
        .hits
        .iter()
        .find(|hit| {
            matches!(hit.address, KnowledgeAddress::Source(_))
                && hit.resource.as_str()
                    == "central:source:control:root:Control/agents/now/clearings/abc/now.json"
        })
        .expect("the clearing record is findable through the ordinary knowledge search");
    assert_eq!(hit.provider.as_str(), "provider/source-pool/now-field");

    // Findable implies openable: the same service reads the source back
    // through the provider's owner-authorised live read.
    let reading = service
        .knowledge_read(&hit.address)
        .expect("the NOW-field hit is readable through the one service");
    assert!(reading
        .content
        .as_deref()
        .unwrap_or_default()
        .contains("telemetry correlation proof"));
}
