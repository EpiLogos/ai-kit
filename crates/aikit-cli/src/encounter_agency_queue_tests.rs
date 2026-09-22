//! Queued A2A delivery through the encounter spool, against the real
//! admission seam (a real OS process standing in as the native owner binary)
//! and a controlled ACP provider fixture. Replies are protocol FIXTURES, never
//! model proof. The joined acceptance against the installed native owner and a
//! real harness lives in the CAW native gate.
#![cfg(unix)]
use super::EncounterA2aFraming;
use crate::encounter_service::{
    EncounterAddressedTurn, EncounterContextAdmission, EncounterContextPacket, EncounterProtocol,
    EncounterProvider, EncounterRequest, EncounterRequiredSource, EncounterService,
};
use aikit_adapters::agency_admission::AgencySourceBasis;
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
};
use aikit_core::{AikitError, ResourceRef, SourceRevision};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}
fn rev(s: &str) -> SourceRevision {
    SourceRevision::parse(s).unwrap()
}

/// A python3 executable that answers `agency actualise <file> --json` with the
/// exact receipt the real Actuation command returns for an admitted request.
fn write_actuation_stub(directory: &Path) -> PathBuf {
    let script = directory.join("actuation-stub.py");
    fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json, sys
args = sys.argv[1:]
if len(args) < 3 or args[0] != "agency" or args[1] != "actualise":
    sys.stderr.write("usage: actuation agency actualise <file> --json\n")
    sys.exit(2)
with open(args[2]) as handle:
    request = json.load(handle)
determination = request["determination"]
child = request["differentiated_binding"]
sys.stdout.write(json.dumps({
    "schema": "actuation.agency-actualisation/v1",
    "receipt_ref": request["request_ref"] + ":receipt",
    "request_ref": request["request_ref"],
    "requester_ref": request["requester_ref"],
    "status": "actualised",
    "governing_binding": request["governing_binding"],
    "differentiated_binding": child,
    "metagency": {
        "grant_ref": request["metagency_grant"]["grant_ref"],
        "authority_ref": request["metagency_grant"]["authority_ref"],
        "operations_used": ["determine-agency"],
    },
    "determination": determination,
    "lineage": {
        "determination_refs": [determination["determination_ref"]],
        "agency_refs": [determination["determining_agency_ref"], determination["differentiated_agency_ref"]],
    },
    "bounds_refs": determination["bounds_refs"],
    "return_relation": {
        "mode": determination["return_policy"]["mode"],
        "return_relation_ref": determination["return_policy"]["return_relation_ref"],
    },
    "agent_identity": {
        "standing": request["agent_identity"]["standing"],
        "agent_ref": child["agent_ref"],
        "evidence_refs": request["agent_identity"]["evidence_refs"],
    },
    "effects": {
        "semantic_relation": "actualised",
        "materialisation": "not-performed",
        "factory_recognition": "not-performed",
        "source_mutation": "not-performed",
    },
    "provenance": {
        "source_refs": request["provenance"]["source_refs"],
        "context_refs": request["provenance"].get("context_refs", []),
    },
}))
"#,
    )
    .unwrap();
    let bin = directory.join("actuation-stub");
    fs::write(
        &bin,
        format!("#!/bin/sh\nexec python3 \"{}\" \"$@\"\n", script.display()),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

struct QueueWorld {
    _temp: tempfile::TempDir,
    home: AikitHome,
    cwd: PathBuf,
    actuation_bin: PathBuf,
}

impl QueueWorld {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = AikitHome::at(root.join("home"));
        let cwd = root.join("work");
        fs::create_dir_all(&cwd).unwrap();
        let actuation_bin = write_actuation_stub(&root);
        Self {
            _temp: temp,
            home,
            cwd,
            actuation_bin,
        }
    }

    /// Attach one session to its own space, configure its selected Agency
    /// (staged against the stub native owner) and its controlled ACP provider.
    fn attach(&self, id: &str) -> (SessionSpaceRef, ResourceRef) {
        let space = SessionSpaceRef::parse(&format!("session-space/{id}")).unwrap();
        let session = r(&format!("agent-session/{id}"));
        let store = SessionSpaceApplicationStore::new(self.home.clone());
        store
            .apply(
                &store
                    .stage(
                        None,
                        SessionSpaceMutation::Create {
                            id: space.clone(),
                            label: None,
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        store
            .apply(
                &store
                    .stage(
                        Some(&space),
                        SessionSpaceMutation::AttachAgentSession {
                            attachment: SessionSpaceAgentAttachmentIntent {
                                agent_session: session.clone(),
                                purpose: Some("Queued delivery test".into()),
                                provenance: vec!["explicit fixture".into()],
                            },
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        let source_path = self.cwd.join(format!("{id}-agency.json"));
        let mut source: Value =
            serde_json::from_str(include_str!("../tests/fixtures/caw-agency-request.json"))
                .unwrap();
        source["differentiated_binding"]["agent_ref"] = json!(format!("agent:{id}"));
        source["differentiated_binding"]["agency_ref"] = json!(format!("agency:{id}"));
        source["differentiated_binding"]["binding_ref"] = json!(format!("binding:{id}"));
        source["determination"]["differentiated_agency_ref"] = json!(format!("agency:{id}"));
        source["determination"]["world_binding_ref"] = json!(format!("binding:{id}"));
        let bytes = serde_json::to_vec(&source).unwrap();
        fs::write(&source_path, &bytes).unwrap();
        let context = self.cwd.join(format!("{id}-context.md"));
        let context_text = format!("SELECTED_CONTEXT_{id}\n");
        fs::write(&context, &context_text).unwrap();
        let binding = super::EncounterAgencyBinding {
            revision: rev("rev/1"),
            active: true,
            agent_ref: r(&format!("agent:{id}")),
            agency_ref: r(&format!("agency:{id}")),
            world_ref: r("central:project:Example"),
            world_binding_ref: r(&format!("binding:{id}")),
            agency_source: AgencySourceBasis {
                source_ref: r(&format!("source/{id}")),
                revision: rev("rev/native-1"),
                path: source_path,
                content_digest: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
            },
            actuation_bin: self.actuation_bin.clone(),
            allowed_senders: [r("human:owner"), r("agent:sender")].into(),
            allowed_packet_sources: [r("source/shared")].into(),
            context: Some(EncounterContextAdmission {
                sources: vec![EncounterRequiredSource {
                    source: r(&format!("source/{id}-context")),
                    revision: rev("rev/context-1"),
                    path: context,
                    content_digest: format!(
                        "blake3:{}",
                        blake3::hash(context_text.as_bytes()).to_hex()
                    ),
                }],
                source_activations: vec![],
                projection: None,
                activation: None,
            }),
        };
        EncounterService::configure_agency(&self.home, &session, &binding, None).unwrap();
        EncounterService::configure(
            &self.home,
            EncounterProvider {
                protocol: EncounterProtocol::Acp,
                id: id.to_owned(),
                label: "Controlled fixture, not installed harness proof".into(),
                argv: vec![
                    "python3".into(),
                    "-u".into(),
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/caw_provider.py")
                        .into_os_string()
                        .into_string()
                        .unwrap(),
                    "acp".into(),
                    self.cwd.join(format!("{id}.log")).display().to_string(),
                ],
                body_ref: None,
                body_revision: None,
                required_context: None,
                model_policy: None,
            },
        )
        .unwrap();
        (space, session)
    }

    fn open(
        &self,
        service: &EncounterService,
        space: &SessionSpaceRef,
        session: &ResourceRef,
        provider: &str,
    ) -> Value {
        service
            .apply(EncounterRequest::Open {
                space: space.clone(),
                agent_session: session.clone(),
                provider: provider.to_owned(),
                cwd: self.cwd.clone(),
            })
            .map_err(|failure| failure.to_string())
            .unwrap()
    }
    fn prompts(&self, id: &str) -> Vec<Value> {
        let Ok(log) = fs::read_to_string(self.cwd.join(format!("{id}.log"))) else {
            // No provider process has run yet: no prompts can exist.
            return Vec::new();
        };
        log.lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|message| message["method"] == "session/prompt")
            .collect()
    }
}

fn turn(
    delivery: &str,
    text: &str,
    audience: &str,
    a2a: Option<EncounterA2aFraming>,
) -> EncounterAddressedTurn {
    EncounterAddressedTurn {
        delivery_ref: r(&format!("delivery/{delivery}")),
        sender: r("agent:sender"),
        expected_binding_revision: rev("rev/1"),
        expected_task: None,
        packet: EncounterContextPacket {
            text: text.to_owned(),
            source_refs: [r("source/shared")].into(),
            audience: [r(audience)].into(),
        },
        a2a,
    }
}

fn send(
    service: &EncounterService,
    session: &ResourceRef,
    turn: EncounterAddressedTurn,
) -> Result<Value, AikitError> {
    service.apply(EncounterRequest::Send {
        agent_session: session.clone(),
        turn,
    })
}

fn wait_settled(service: &EncounterService, session: &ResourceRef, delivery: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let row = service
            .store
            .delivery(session, &r(&format!("delivery/{delivery}")))
            .unwrap()
            .expect("delivery row");
        if matches!(row.phase.as_str(), "returned" | "failed" | "cancelled") {
            return json!(row);
        }
        assert!(Instant::now() < deadline, "queued delivery did not settle");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn queued_delivery_waits_drains_in_order_and_answers_with_the_a2a_receipt() {
    let world = QueueWorld::new();
    let (space, session) = world.attach("one");
    let service = EncounterService::new(world.home.clone()).unwrap();

    // No live resident: the full sender-side preflight passes, and the
    // delivery waits durably instead of failing as not-ready.
    let text_one = "A2A_ONE queued while you were away";
    let framing_one = EncounterA2aFraming {
        message_id: "msg-1".into(),
        text: text_one.into(),
        purpose: Some("catch-up".into()),
        exchange_operation_id: Some("operation/1".into()),
    };
    let queued = send(
        &service,
        &session,
        turn("a2a-1", text_one, "agent:one", Some(framing_one)),
    )
    .unwrap();
    assert_eq!(queued["queued"], json!(true), "{queued}");
    assert_eq!(queued["fresh"], json!(true));
    assert_eq!(queued["delivery"]["phase"], json!("queued"));
    assert_eq!(queued["exchange_ref"], json!("a2a-exchange:msg-1"));
    assert!(world.prompts("one").is_empty());
    // The queued row holds the session's single delivery slot.
    assert_eq!(
        send(
            &service,
            &session,
            turn("blocked", "must wait for the slot", "agent:one", None),
        )
        .unwrap_err()
        .code(),
        "encounter.delivery_pending"
    );
    assert_eq!(service.store.queued_deliveries(&session).unwrap().len(), 1);

    // The resident becomes ready: the queued delivery drains inside the open,
    // re-admitted, delivered through the same prompt path as a live send.
    let opened = world.open(&service, &space, &session, "one");
    assert_eq!(opened["resident"], json!(true), "{opened}");
    let delivered = opened["queued_drain"]["delivered"].as_array().unwrap();
    assert_eq!(delivered.len(), 1, "{opened}");
    assert_eq!(
        delivered[0]["a2a"]["exchange_ref"],
        json!("a2a-exchange:msg-1")
    );
    assert_eq!(
        delivered[0]["a2a"]["transport_result"],
        json!({"kind":"turn","ref":"delivery/a2a-1"})
    );
    assert_eq!(
        delivered[0]["a2a"]["exchange_authority"],
        json!({
            "grant_ref": "exchange-grant:encounter-send:operation/1",
            "operation_id": "operation/1"
        })
    );
    assert_eq!(delivered[0]["a2a"]["admission"], json!("pending"));
    assert_eq!(
        wait_settled(&service, &session, "a2a-1")["phase"],
        json!("returned")
    );
    assert!(service
        .store
        .queued_deliveries(&session)
        .unwrap()
        .is_empty());
    let prompts = world.prompts("one");
    assert_eq!(prompts.len(), 1);
    let prompt = prompts[0]["params"]["prompt"].to_string();
    assert!(prompt.contains("SELECTED_CONTEXT_one"), "{prompt}");
    assert!(prompt.contains(text_one), "{prompt}");

    // A second wait, drained at the next readiness moment in order: shut the
    // resident down, queue again, and reconnect — the queued message is
    // delivered into the resumed body's turn, after the first one.
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(shutdown["shutdown"], json!(true), "{shutdown}");
    drop(service);
    let service = EncounterService::new(world.home.clone()).unwrap();
    let text_two = "A2A_TWO queued after the shutdown";
    let queued = send(
        &service,
        &session,
        turn("a2a-2", text_two, "agent:one", None),
    )
    .unwrap();
    assert_eq!(queued["queued"], json!(true), "{queued}");
    let reconnected = service
        .apply(EncounterRequest::Reconnect {
            space: space.clone(),
            agent_session: session.clone(),
            provider: "one".into(),
            cwd: world.cwd.clone(),
        })
        .unwrap();
    assert_eq!(reconnected["resident"], json!(true), "{reconnected}");
    let delivered = reconnected["queued_drain"]["delivered"].as_array().unwrap();
    assert_eq!(delivered.len(), 1, "{reconnected}");
    assert_eq!(delivered[0]["delivery_ref"], json!("delivery/a2a-2"));
    assert!(delivered[0]["a2a"].is_null());
    assert_eq!(
        wait_settled(&service, &session, "a2a-2")["phase"],
        json!("returned")
    );
    let prompts = world.prompts("one");
    assert_eq!(prompts.len(), 2, "delivered in order, no replacement");
    let journal = prompts
        .iter()
        .map(|prompt| prompt["params"]["prompt"].to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let one = journal.find(text_one).expect("first queued text");
    let two = journal.find(text_two).expect("second queued text");
    assert!(one < two, "oldest queued delivery is delivered first");
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(shutdown["shutdown"], json!(true));
}

#[test]
fn queueing_stays_gated_by_the_full_agency_preflight() {
    let world = QueueWorld::new();
    let (_space, session) = world.attach("one");
    let service = EncounterService::new(world.home.clone()).unwrap();

    let attempt =
        |service: &EncounterService, session: &ResourceRef, turn: EncounterAddressedTurn| {
            send(service, session, turn).unwrap_err().code().to_owned()
        };

    // An unauthorised sender cannot queue.
    let mut stranger = turn("denied-sender", "must not queue", "agent:one", None);
    stranger.sender = r("agent:stranger");
    assert_eq!(
        attempt(&service, &session, stranger),
        "encounter.disclosure_denied"
    );
    // A packet source outside the disclosed set cannot queue.
    let mut private_source = turn("denied-source", "must not queue", "agent:one", None);
    private_source.packet.source_refs = [r("source/private")].into();
    assert_eq!(
        attempt(&service, &session, private_source),
        "encounter.disclosure_denied"
    );
    // A stale binding revision cannot queue.
    let mut stale = turn("denied-revision", "must not queue", "agent:one", None);
    stale.expected_binding_revision = rev("rev/2");
    assert_eq!(
        attempt(&service, &session, stale),
        "encounter.binding_changed"
    );
    // A2A framing must not carry different content than the addressed packet.
    let mut rewritten = turn("denied-a2a", "the actual packet text", "agent:one", None);
    rewritten.a2a = Some(EncounterA2aFraming {
        message_id: "msg-x".into(),
        text: "a different text smuggled through the framing".into(),
        purpose: None,
        exchange_operation_id: None,
    });
    assert_eq!(
        attempt(&service, &session, rewritten),
        "encounter.a2a_invalid"
    );
    // A withdrawn participant cannot queue — covered by the denied cases
    // above, which all run the full preflight before any queue write.
    // Nothing queued, nothing delivered.
    assert!(service
        .store
        .queued_deliveries(&session)
        .unwrap()
        .is_empty());
    assert!(world.prompts("one").is_empty());

    // A session with no selected Agency at all cannot queue either.
    let bare_space = SessionSpaceRef::parse("session-space/bare").unwrap();
    let bare = r("agent-session/bare");
    let store = SessionSpaceApplicationStore::new(world.home.clone());
    store
        .apply(
            &store
                .stage(
                    None,
                    SessionSpaceMutation::Create {
                        id: bare_space.clone(),
                        label: None,
                    },
                )
                .unwrap(),
        )
        .unwrap();
    store
        .apply(
            &store
                .stage(
                    Some(&bare_space),
                    SessionSpaceMutation::AttachAgentSession {
                        attachment: SessionSpaceAgentAttachmentIntent {
                            agent_session: bare.clone(),
                            purpose: Some("no agency".into()),
                            provenance: vec!["explicit fixture".into()],
                        },
                    },
                )
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        attempt(
            &service,
            &bare,
            turn("denied-bare", "must not queue", "agent:bare", None)
        ),
        "encounter.agency_required"
    );
    assert!(service.store.queued_deliveries(&bare).unwrap().is_empty());
}
