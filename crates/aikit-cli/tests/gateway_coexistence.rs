//! Gateway coexistence: detection against fixture service-manager listings,
//! the policy document round-trip, and the exclusive gate's refuse/allow
//! paths. No live Hermes dependency: the probe is data, and the live
//! collectors only fill it in.

use aikit_adapters::{
    detect, decide, exclusive_gate, load_coexistence, store_coexistence, CoexistenceDecision,
    CoexistenceDocument, CoexistencePolicy, CoexistenceProbe, ForeignBotIdentity,
    ForeignGateway, GatewayCoexistenceGate, GATEWAY_COEXISTENCE_FILE_NAME,
    GATEWAY_COEXISTENCE_SCHEMA,
};
use tempfile::TempDir;

fn hermes_probe() -> CoexistenceProbe {
    CoexistenceProbe {
        service_labels: vec![
            "com.apple.Finder".into(),
            "ai.hermes.gateway".into(),
        ],
        path_binaries: vec![],
        state_dirs: vec![],
    }
}

#[test]
fn detection_reads_every_evidence_channel_and_reports_what_it_saw_and_where() {
    // A quiet machine: no sightings, nothing invented.
    let empty = detect(&CoexistenceProbe::default());
    assert!(empty.is_empty(), "{empty:?}");

    // The Hermes specimen, seen through every channel at once.
    let mut probe = hermes_probe();
    probe.path_binaries = vec!["hermes".into()];
    probe.state_dirs = vec!["/Users/example/.hermes".into()];
    let sightings = detect(&probe);
    assert_eq!(sightings.len(), 1, "{sightings:?}");
    let hermes = &sightings[0];
    assert_eq!(hermes.harness, "hermes");
    assert_eq!(hermes.evidence.len(), 3, "{hermes:?}");
    assert!(
        hermes
            .evidence
            .iter()
            .any(|line| line.contains("ai.hermes.gateway") && line.contains("service manager")),
        "the launchd sighting is evidence: {hermes:?}"
    );
    assert!(
        hermes.evidence.iter().any(|line| line.contains("hermes")),
        "the binary sighting is evidence: {hermes:?}"
    );
    assert!(
        hermes
            .evidence
            .iter()
            .any(|line| line.contains(".hermes")),
        "the state-dir sighting is evidence: {hermes:?}"
    );

    // The openclaw equivalent is its own harness, with its own footprint.
    let openclaw = detect(&CoexistenceProbe {
        service_labels: vec!["ai.openclaw.gateway".into()],
        path_binaries: vec![],
        state_dirs: vec![],
    });
    assert_eq!(openclaw.len(), 1, "{openclaw:?}");
    assert_eq!(openclaw[0].harness, "openclaw");

    // An unrelated service label is never a sighting.
    let unrelated = detect(&CoexistenceProbe {
        service_labels: vec!["ai.something.else".into()],
        path_binaries: vec![],
        state_dirs: vec![],
    });
    assert!(unrelated.is_empty(), "{unrelated:?}");
}

#[test]
fn the_policy_read_against_the_sightings_decides_honestly() {
    let hermes = [ForeignGateway {
        harness: "hermes".into(),
        evidence: vec!["service ai.hermes.gateway is loaded in the user's service manager"
            .into()],
    }];

    // No sighting: exclusively ours, whatever the policy says.
    assert_eq!(
        decide(CoexistencePolicy::Exclusive, &[]),
        CoexistenceDecision::ExclusivelyOurs
    );

    // Exclusive with a sighting: the hold, naming who was seen.
    let decision = decide(CoexistencePolicy::Exclusive, &hermes);
    assert_eq!(
        decision,
        CoexistenceDecision::ExclusiveHold {
            foreign: hermes.to_vec()
        }
    );
    assert!(
        decision.summary().contains("hermes") && decision.summary().contains("exclusive"),
        "{}",
        decision.summary()
    );

    // Coexist with the same sighting: allowed, disclosed.
    let decision = decide(CoexistencePolicy::Coexist, &hermes);
    assert_eq!(
        decision,
        CoexistenceDecision::Coexisting {
            foreign: hermes.to_vec()
        }
    );
    assert!(
        decision.summary().contains("coexisting"),
        "{}",
        decision.summary()
    );
}

#[test]
fn the_exclusive_gate_refuses_recorded_identity_conflicts_and_allows_everything_else() {
    let hermes = vec![ForeignGateway {
        harness: "hermes".into(),
        evidence: vec!["the \"hermes\" binary is on PATH".into()],
    }];
    let recorded = vec![ForeignBotIdentity {
        harness: "hermes".into(),
        platform: "telegram".into(),
        bot_id: "@hermes_bot".into(),
    }];
    let gate = exclusive_gate(hermes.clone(), recorded);

    // The recorded conflict: telegram, owned by a detected hermes — refused,
    // by name, with the switch command.
    let error = gate
        .admit_connector("gateway-connector/telegram/main", "telegram")
        .unwrap_err();
    assert_eq!(error.code(), "gateway_coexistence.exclusive_conflict", "{error}");
    let message = error.to_string();
    assert!(message.contains("hermes"), "{message}");
    assert!(message.contains("@hermes_bot"), "{message}");
    assert!(
        message.contains("--policy coexist"),
        "the refusal names the way out: {message}"
    );

    // A platform the foreign gateway is not recorded to own: allowed.
    gate.admit_connector("gateway-connector/slack/main", "slack")
        .unwrap();

    // The same platform with nothing recorded: allowed — co-detection alone
    // refuses nothing, it is disclosed instead.
    let gate = exclusive_gate(hermes.clone(), Vec::new());
    gate.admit_connector("gateway-connector/telegram/main", "telegram")
        .unwrap();

    // A recorded identity whose harness is not detected right now: allowed.
    let gate = exclusive_gate(
        Vec::new(),
        vec![ForeignBotIdentity {
            harness: "hermes".into(),
            platform: "telegram".into(),
            bot_id: "@hermes_bot".into(),
        }],
    );
    gate.admit_connector("gateway-connector/telegram/main", "telegram")
        .unwrap();
}

#[test]
fn the_coexistence_document_round_trips_and_defaults_to_exclusive() {
    let root = TempDir::new().unwrap();
    let path = root.path().join(GATEWAY_COEXISTENCE_FILE_NAME);

    // A missing document is the default posture: exclusive, nothing recorded.
    let default = load_coexistence(&path).unwrap();
    assert_eq!(default, CoexistenceDocument::default());
    assert_eq!(default.policy, CoexistencePolicy::Exclusive);

    // The round-trip: policy and recorded identities survive the store.
    let document = CoexistenceDocument {
        schema: GATEWAY_COEXISTENCE_SCHEMA.into(),
        policy: CoexistencePolicy::Coexist,
        foreign_bot_identities: vec![ForeignBotIdentity {
            harness: "hermes".into(),
            platform: "telegram".into(),
            bot_id: "@hermes_bot".into(),
        }],
    };
    store_coexistence(&path, &document).unwrap();
    let loaded = load_coexistence(&path).unwrap();
    assert_eq!(loaded, document);
    let encoded = std::fs::read_to_string(&path).unwrap();
    assert!(encoded.contains("gateway-coexistence/v1"), "{encoded}");
    assert!(encoded.contains("coexist"), "{encoded}");

    // A wrong schema is a named error, never a silent default.
    let wrong = root.path().join("wrong.json");
    std::fs::write(
        &wrong,
        format!("{{\"schema\": \"aikit.gateway-connectors/v1\", \"policy\": \"coexist\"}}"),
    )
    .unwrap();
    let error = load_coexistence(&wrong).unwrap_err();
    assert_eq!(error.code(), "gateway_coexistence.invalid", "{error}");

    // A policy nobody declares is refused by name.
    assert!(CoexistencePolicy::parse("conquer").is_err());
    assert_eq!(
        CoexistencePolicy::parse("coexist").unwrap(),
        CoexistencePolicy::Coexist
    );
}
