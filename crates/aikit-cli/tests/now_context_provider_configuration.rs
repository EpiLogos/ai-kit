//! Configuring Redis-prepared NOW context on provider rows (Pi, Prime, any
//! other) through the native owner verb, with no Workcell or Factory involved.
//!
//! Before this, no shipped provider row carried `now_context`: neither
//! `epi-prime-ql` nor `pi` was delivered prepared context. The verb edits only
//! that field of the stored registration, validates the preparation request
//! against the Redis election now, and reports what the request elects (for
//! example Kev through `selection.mode = provider`).
#![cfg(unix)]

use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

struct Home {
    dir: TempDir,
}

impl Home {
    fn new() -> Self {
        Self {
            dir: TempDir::new().unwrap(),
        }
    }
    fn run(&self, args: &[&str]) -> (bool, Value, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.dir.path())
            .env("AIKIT_HOME", self.dir.path().join("aikit"))
            .arg("-C")
            .arg(self.dir.path())
            .args(args)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let value = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
        (
            output.status.success(),
            value,
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
    fn provider_file(&self, id: &str) -> Value {
        serde_json::from_slice(
            &fs::read(
                self.dir
                    .path()
                    .join("aikit/state/encounter-providers")
                    .join(format!("{id}.json")),
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn write(&self, name: &str, value: &Value) -> String {
        let path = self.dir.path().join(name);
        fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
        path.display().to_string()
    }
}

fn redis(port: u16) -> Value {
    json!({
        "schema": "aikit.redis-now-config/v1", "address": format!("127.0.0.1:{port}"),
        "database": 0, "key_prefix": "aikit-now", "username": null, "credential_ref": null,
        "allow_remote": false, "connect_timeout_ms": 1000, "io_timeout_ms": 1000,
        "prepared_ttl_seconds": 21600, "coordination_retention_seconds": 1209600
    })
}

fn prepare_request(redis: &Value, selection: Value) -> Value {
    json!({
        "schema": "aikit.now-preparation-request/v1", "redis": redis,
        "project_ref": "project/example", "now_ref": "now/example",
        "participant_ref": "agent/example", "agent_session": "agent-session/example",
        "concern": "Begin from prepared owner context", "disclosure_revision": "d1",
        "central": {"root": "/nonexistent-central-root", "project": "example"},
        "expected_version": 0, "external_provider": true, "selection": selection
    })
}

fn configure(home: &Home, provider: Value) {
    let (ok, _, stderr) = home.run(&[
        "encounter-configure",
        "--provider-json",
        &serde_json::to_string(&provider).unwrap(),
    ]);
    assert!(ok, "{stderr}");
}

#[test]
fn a_pi_row_and_a_prime_row_get_prepared_context_that_elects_kev_and_nothing_else_changes() {
    let home = Home::new();
    let argv = |name: &str| vec!["/bin/echo".to_string(), name.to_string()];
    configure(
        &home,
        json!({"protocol": "pi-rpc", "id": "pi", "label": "Pi", "argv": argv("pi")}),
    );
    configure(
        &home,
        json!({"protocol": "prime-rpc", "id": "epi-prime-ql", "label": "Epi-Logos Prime-QL",
               "argv": argv("prime"), "body_ref": "agent-body/epi-prime-ql",
               "body_revision": "2139d78ceed15f8359e75a8a185648f80a604b8a"}),
    );
    let before_pi = home.provider_file("pi");
    let before_prime = home.provider_file("epi-prime-ql");

    let redis = redis(6391);
    let election = home.write("redis-now.json", &redis);
    let request = home.write(
        "prepare-kev.json",
        &prepare_request(
            &redis,
            json!({"mode": "provider", "provider_file": "/x/decision-provider.json",
                   "state": {"undertaking": "ground the reader's question"},
                   "relevance_threshold": 0.4}),
        ),
    );

    for id in ["pi", "epi-prime-ql"] {
        let (ok, out, stderr) = home.run(&[
            "encounter-now-context-configure",
            "--provider-id",
            id,
            "--redis-config",
            &election,
            "--prepare-request",
            &request,
        ]);
        assert!(ok, "{id}: {stderr}");
        assert_eq!(out["configured"], true);
        assert_eq!(out["preparation"]["selection"]["mode"], "provider");
        assert_eq!(out["preparation"]["selection"]["relevance_threshold"], 0.4);
        // External-provider is the safe default; delivery is not "required".
        assert_eq!(out["now_context"]["external_provider"], true);
        assert_eq!(out["now_context"]["required"], false);
    }

    // Only now_context changed on each stored registration.
    for (id, before) in [("pi", before_pi), ("epi-prime-ql", before_prime)] {
        let mut after = home.provider_file(id);
        assert!(after["now_context"]["redis"]["address"]
            .as_str()
            .unwrap()
            .ends_with(":6391"));
        assert!(Path::new(after["now_context"]["prepare_request"].as_str().unwrap()).is_absolute());
        after.as_object_mut().unwrap().remove("now_context");
        assert_eq!(after, before, "{id}: nothing but now_context may change");
    }

    // Withdrawal restores the stored row exactly.
    let (ok, out, _) = home.run(&[
        "encounter-now-context-configure",
        "--provider-id",
        "pi",
        "--withdraw",
    ]);
    assert!(ok);
    assert_eq!(out["configured"], false);
    assert!(home.provider_file("pi").get("now_context").is_none());
}

#[test]
fn a_preparation_request_for_another_redis_is_refused_at_configuration_not_at_the_first_turn() {
    let home = Home::new();
    configure(
        &home,
        json!({"protocol": "pi-rpc", "id": "pi", "label": "Pi", "argv": ["/bin/echo", "pi"]}),
    );
    let election = home.write("redis-now.json", &redis(6391));
    let request = home.write(
        "prepare-other.json",
        &prepare_request(&redis(6999), json!({"mode": "all"})),
    );
    let (ok, _, stderr) = home.run(&[
        "encounter-now-context-configure",
        "--provider-id",
        "pi",
        "--redis-config",
        &election,
        "--prepare-request",
        &request,
    ]);
    assert!(!ok);
    assert!(
        stderr.contains("encounter_prepare_mismatch") || stderr.contains("differs"),
        "{stderr}"
    );
    assert!(
        home.provider_file("pi").get("now_context").is_none(),
        "a refused configuration leaves the row untouched"
    );

    // An unknown provider is named, not invented.
    let (ok, _, stderr) = home.run(&[
        "encounter-now-context-configure",
        "--provider-id",
        "missing",
        "--redis-config",
        &election,
    ]);
    assert!(!ok);
    assert!(
        stderr.contains("No configured encounter provider named missing"),
        "{stderr}"
    );
}
