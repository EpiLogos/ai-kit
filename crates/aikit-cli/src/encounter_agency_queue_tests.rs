//! Queued A2A delivery through the encounter spool, against the real
//! admission seam (a real OS process standing in as the native owner binary)
//! and a controlled ACP provider fixture. Replies are protocol FIXTURES, never
//! model proof. The joined acceptance against the installed native owner and a
//! real harness lives in the CAW native gate.
#![cfg(unix)]
use super::EncounterA2aFraming;
use crate::encounter_service::{
    EncounterAddressedTurn, EncounterContextAdmission, EncounterContextPacket, EncounterInteract,
    EncounterProtocol, EncounterProvider, EncounterRequest, EncounterRequiredSource,
    EncounterService,
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

pub(super) struct QueueWorld {
    pub(super) _temp: tempfile::TempDir,
    pub(super) home: AikitHome,
    pub(super) cwd: PathBuf,
    actuation_bin: PathBuf,
}

impl QueueWorld {
    pub(super) fn new() -> Self {
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
        self.attach_with(
            id,
            &["human:owner", "agent:sender"],
            &["source/shared"],
            "caw_provider.py",
        )
    }
    /// The same attachment with an explicit disclosure (who may address this
    /// session, which packet sources it may receive) and provider fixture.
    pub(super) fn attach_with(
        &self,
        id: &str,
        senders: &[&str],
        sources: &[&str],
        provider: &str,
    ) -> (SessionSpaceRef, ResourceRef) {
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
            allowed_senders: senders.iter().map(|s| r(s)).collect(),
            allowed_packet_sources: sources.iter().map(|s| r(s)).collect(),
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
            central_profile_ref: None,
            central_identity_world: None,
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
                        .join("tests/fixtures")
                        .join(provider)
                        .into_os_string()
                        .into_string()
                        .unwrap(),
                    "acp".into(),
                    self.cwd.join(format!("{id}.log")).display().to_string(),
                ],
                body_ref: None,
                body_revision: None,
                from_profile: None,
                argv_fallback: Vec::new(),
                env: Default::default(),
                cwd: None,
                required_context: None,
                model_policy: None,
                now_context: None,
            },
        )
        .unwrap();
        (space, session)
    }

    pub(super) fn open(
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
        interact: None,
    })
}

fn send_deliberately(
    service: &EncounterService,
    session: &ResourceRef,
    turn: EncounterAddressedTurn,
    reason: &str,
) -> Result<Value, AikitError> {
    service.apply(EncounterRequest::Send {
        agent_session: session.clone(),
        turn,
        interact: Some(EncounterInteract {
            reason: reason.into(),
        }),
    })
}

#[test]
fn a_send_to_a_session_that_waits_on_the_person_is_refused_unless_deliberate() {
    let world = QueueWorld::new();
    let (_space, session) = world.attach("one");
    let service = EncounterService::new(world.home.clone()).unwrap();
    crate::encounter_service::tests::hold_permission(&service, &session, "perm-1");
    // Positive evidence that the session waits on the person: nothing is sent
    // or queued behind its back, and the refusal names the next operation.
    let refused = send(
        &service,
        &session,
        turn("held-1", "while you wait", "agent:one", None),
    )
    .unwrap_err();
    assert_eq!(refused.code(), "encounter.target_needs_input");
    assert_eq!(
        refused.message().lines().nth(1),
        Some("Nothing was sent or queued.")
    );
    assert!(service
        .store
        .delivery(&session, &r("delivery/held-1"))
        .unwrap()
        .is_none());
    // Deliberately: recorded first, then queued for the next turn boundary.
    let queued = send_deliberately(
        &service,
        &session,
        turn("held-1", "while you wait", "agent:one", None),
        "the reviewer asked for it now",
    )
    .unwrap();
    assert_eq!(queued["queued"], json!(true), "{queued}");
    let journal = service.store.events(&session, 0, 100).unwrap().events;
    let override_at = journal
        .iter()
        .position(|event| event.event["kind"] == "prompt-override")
        .expect("the override is recorded");
    assert_eq!(journal[override_at].event["action"], "send");
    assert_eq!(
        journal[override_at].event["reason"],
        "the reviewer asked for it now"
    );
    assert!(
        world.prompts("one").is_empty(),
        "queued, never typed into the waiting session"
    );
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

/// A session's status says how its last finished turn stopped, as the host saw
/// it on the wire, so a reader (Factory's task reading) can tell a turn that
/// completed from one that failed, and why. None before the first turn.
#[test]
fn status_carries_how_the_last_turn_stopped() {
    let world = QueueWorld::new();
    let (space, session) = world.attach("last");
    let service = EncounterService::new(world.home.clone()).unwrap();
    let opened = world.open(&service, &space, &session, "last");
    assert_eq!(opened["resident"], json!(true), "{opened}");
    let status = || {
        service
            .apply(EncounterRequest::Status {
                agent_session: session.clone(),
            })
            .unwrap()
    };
    let before = status();
    assert!(before["last_turn"].is_null(), "{before}");
    assert!(before.get("last_turn").is_some(), "{before}");

    send(
        &service,
        &session,
        turn("live-1", "one live turn", "agent:last", None),
    )
    .unwrap();
    assert_eq!(
        wait_settled(&service, &session, "live-1")["phase"],
        json!("returned")
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let after = loop {
        let now = status();
        if !now["last_turn"].is_null() || Instant::now() > deadline {
            break now;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(
        after["last_turn"]["stop"],
        json!({"Completed": {"stop_reason": "end_turn"}}),
        "{after}"
    );
    assert!(after["last_turn"]["interruption"].is_null(), "{after}");
    assert!(after["last_turn"]["last_sequence"].is_u64(), "{after}");
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(shutdown["shutdown"], json!(true));
}

/// O-I #1007 (with #1016): a session AIKit opens for a conversation records its start in the
/// lifecycle its harness's hooks keep, under `aikit:session:<client>:<agent session>`; opening
/// the live session again records nothing. The conversation offers its End as the registry's
/// `aikit.session.end`; End releases the idle body and records the end once. A session whose
/// body was already released still ends from its record, and one that never opened is refused.
#[test]
fn an_encounter_session_starts_on_its_open_and_ends_once_on_its_end() {
    use crate::session_lifecycle_ops::{harness_session, SessionLifecycleServiceOps};
    use aikit_core::session_lifecycle::SessionLifecycleEventKind::{SessionEnded, SessionStarted};
    let world = QueueWorld::new();
    let (space, session) = world.attach("ending");
    let service = EncounterService::new(world.home.clone()).unwrap();
    let never = service
        .apply(EncounterRequest::End {
            agent_session: session.clone(),
        })
        .unwrap_err();
    assert_eq!(never.code(), "encounter.end_unopened", "{never}");

    let session_ref = "aikit:session:ending:agent-session/ending";
    let kinds = || {
        world
            .home
            .session_lifecycle_history(&harness_session(session_ref).unwrap())
            .map(|history| history.iter().map(|event| event.kind).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let opened = world.open(&service, &space, &session, "ending");
    assert_eq!(
        opened["lifecycle"],
        json!({"session_ref": session_ref, "start": "first"}),
        "{opened}"
    );
    let again = world.open(&service, &space, &session, "ending");
    assert_eq!(again["lifecycle"]["start"], "replayed", "{again}");
    assert_eq!(kinds(), [SessionStarted]);

    let view = || {
        service
            .apply(EncounterRequest::View {
                agent_session: session.clone(),
                before: None,
                after: None,
            })
            .unwrap()
    };
    let end_control = |view: &Value| {
        view["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|action| action["ref"] == "aikit.session.end")
            .cloned()
            .unwrap_or_else(|| panic!("no End control: {view}"))
    };
    let live = view();
    assert_eq!(end_control(&live)["enabled"], true, "{live}");
    assert_eq!(
        live["lifecycle"],
        json!({"session_ref": session_ref, "running": true})
    );

    let ended = service
        .apply(EncounterRequest::End {
            agent_session: session.clone(),
        })
        .unwrap();
    assert_eq!(ended["schema"], "aikit.encounter-end/v1", "{ended}");
    assert_eq!(ended["session_ref"], session_ref, "{ended}");
    assert_eq!(ended["ended"], true, "{ended}");
    assert_eq!(
        ended["released"]["receipt"]["cleanup_confirmed"], true,
        "the idle body is released: {ended}"
    );
    assert_eq!(kinds(), [SessionStarted, SessionEnded]);
    let closed = view();
    assert_eq!(closed["connection"]["resident"], false, "{closed}");
    assert_eq!(end_control(&closed)["enabled"], false, "{closed}");
    assert_eq!(closed["lifecycle"]["running"], false, "{closed}");

    let twice = service
        .apply(EncounterRequest::End {
            agent_session: session.clone(),
        })
        .unwrap();
    assert_eq!(twice["ended"], false, "{twice}");
    assert!(twice["released"].is_null(), "{twice}");
    assert_eq!(kinds(), [SessionStarted, SessionEnded]);

    // A session whose body was released on its own is still running in its record, and its End
    // closes the record with no body to release.
    let (space, session) = world.attach("released");
    let session_ref = "aikit:session:released:agent-session/released";
    let opened = world.open(&service, &space, &session, "released");
    assert_eq!(opened["lifecycle"]["start"], "first", "{opened}");
    service
        .apply(EncounterRequest::ReleaseNative {
            agent_session: session.clone(),
            expected_native_session_id: opened["native_session_id"].as_str().unwrap().to_owned(),
            expected_generation: opened["connection_generation"].as_str().unwrap().to_owned(),
        })
        .unwrap();
    let ended = service
        .apply(EncounterRequest::End {
            agent_session: session.clone(),
        })
        .unwrap();
    assert_eq!(ended["ended"], true, "{ended}");
    assert!(ended["released"].is_null(), "{ended}");
    assert_eq!(
        world
            .home
            .session_lifecycle_history(&harness_session(session_ref).unwrap())
            .unwrap()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        [SessionStarted, SessionEnded]
    );
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(shutdown["shutdown"], json!(true));
}

/// The `aikit.session.end` control a session's view offers.
fn end_control(view: &Value) -> Value {
    view["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["ref"] == "aikit.session.end")
        .cloned()
        .unwrap_or_else(|| panic!("no End control: {view}"))
}

fn view_of(service: &EncounterService, session: &ResourceRef) -> Value {
    service
        .apply(EncounterRequest::View {
            agent_session: session.clone(),
            before: None,
            after: None,
        })
        .unwrap()
}

/// The kinds of the events the record `session_ref` names keeps, none for a record never started.
fn record_of(
    home: &AikitHome,
    session_ref: &str,
) -> Vec<aikit_core::session_lifecycle::SessionLifecycleEventKind> {
    use crate::session_lifecycle_ops::{harness_session, SessionLifecycleServiceOps};
    home.session_lifecycle_history(&harness_session(session_ref).unwrap())
        .map(|history| history.iter().map(|event| event.kind).collect())
        .unwrap_or_default()
}

/// ai-kit#640 (R640-1): an End and an open of one session go one at a time. While a reconnect is
/// under way (held at its opening lease, before it reserves or launches anything) the view's End is
/// off, saying why, and an End is refused in words; the reconnect then answers, and the session runs
/// in its record as in its body. Unfenced, the End closed the record while the reconnect brought the
/// body back.
#[test]
fn an_end_waits_for_an_open_under_way_so_the_record_keeps_with_the_body() {
    use aikit_core::session_lifecycle::SessionLifecycleEventKind::{SessionEnded, SessionStarted};
    let world = QueueWorld::new();
    let (space, session) = world.attach("fenced");
    let service = std::sync::Arc::new(EncounterService::new(world.home.clone()).unwrap());
    let session_ref = "aikit:session:fenced:agent-session/fenced";
    let opened = world.open(&service, &space, &session, "fenced");
    assert_eq!(opened["lifecycle"]["start"], "first", "{opened}");
    service
        .apply(EncounterRequest::ReleaseNative {
            agent_session: session.clone(),
            expected_native_session_id: opened["native_session_id"].as_str().unwrap().to_owned(),
            expected_generation: opened["connection_generation"].as_str().unwrap().to_owned(),
        })
        .unwrap();

    let (arrived, release) = service.hold_opens_at_their_lease();
    let reconnecting = {
        let (service, space, session, cwd) = (
            service.clone(),
            space.clone(),
            session.clone(),
            world.cwd.clone(),
        );
        std::thread::spawn(move || {
            service.apply(EncounterRequest::Reconnect {
                space,
                agent_session: session,
                provider: "fenced".into(),
                cwd,
            })
        })
    };
    arrived
        .recv_timeout(Duration::from_secs(10))
        .expect("the reconnect reached its opening lease");
    let opening = view_of(&service, &session);
    let end = end_control(&opening);
    assert_eq!(end["enabled"], false, "{opening}");
    assert!(
        end["reason"]
            .as_str()
            .is_some_and(|why| why.contains("under way")),
        "{end}"
    );
    let refused = service
        .apply(EncounterRequest::End {
            agent_session: session.clone(),
        })
        .map(|answer| answer.to_string())
        .unwrap_err();
    assert_eq!(
        refused.code(),
        "encounter.native_open_in_progress",
        "{refused}"
    );
    assert!(refused.message().contains("under way"), "{refused}");
    release.send(()).unwrap();
    let reconnected = reconnecting
        .join()
        .unwrap()
        .map_err(|failure| failure.to_string())
        .unwrap();
    assert_eq!(
        reconnected["lifecycle"],
        json!({"session_ref": session_ref, "start": "replayed"}),
        "{reconnected}"
    );
    assert_eq!(record_of(&world.home, session_ref), [SessionStarted]);
    let live = view_of(&service, &session);
    assert_eq!(live["connection"]["resident"], true, "{live}");
    assert_eq!(live["lifecycle"]["running"], true, "{live}");
    assert_eq!(end_control(&live)["enabled"], true, "{live}");

    // Once the open has answered, the End ends both.
    let ended = service
        .apply(EncounterRequest::End {
            agent_session: session.clone(),
        })
        .unwrap();
    assert_eq!(ended["ended"], true, "{ended}");
    assert_eq!(
        record_of(&world.home, session_ref),
        [SessionStarted, SessionEnded]
    );
    assert_eq!(view_of(&service, &session)["connection"]["resident"], false);
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(shutdown["shutdown"], json!(true));
}

/// ai-kit#640 (R640-3): the record a session's start was kept under is the one its binding keeps. Its
/// provider configured anew after the open (here as one derived from the claude-code profile, whose
/// hooks spell another client) moves neither the view's record nor the End's.
#[test]
fn a_session_keeps_the_record_its_start_chose_when_its_provider_changes() {
    use aikit_core::session_lifecycle::SessionLifecycleEventKind::{SessionEnded, SessionStarted};
    let world = QueueWorld::new();
    let (space, session) = world.attach("moved");
    let service = EncounterService::new(world.home.clone()).unwrap();
    let session_ref = "aikit:session:moved:agent-session/moved";
    let opened = world.open(&service, &space, &session, "moved");
    assert_eq!(
        opened["lifecycle"],
        json!({"session_ref": session_ref, "start": "first"}),
        "{opened}"
    );
    EncounterService::configure(
        &world.home,
        EncounterProvider {
            protocol: EncounterProtocol::Acp,
            id: "moved".into(),
            label: "The same id, now derived from a harness profile".into(),
            argv: Vec::new(),
            argv_fallback: Vec::new(),
            env: Default::default(),
            cwd: None,
            from_profile: Some("claude-code".into()),
            required_context: None,
            model_policy: None,
            body_ref: None,
            body_revision: None,
            now_context: None,
        },
    )
    .unwrap();
    let live = view_of(&service, &session);
    assert_eq!(
        live["lifecycle"],
        json!({"session_ref": session_ref, "running": true}),
        "{live}"
    );
    let ended = service
        .apply(EncounterRequest::End {
            agent_session: session.clone(),
        })
        .unwrap();
    assert_eq!(ended["session_ref"], session_ref, "{ended}");
    assert_eq!(ended["ended"], true, "{ended}");
    assert_eq!(
        record_of(&world.home, session_ref),
        [SessionStarted, SessionEnded]
    );
    assert!(record_of(&world.home, "aikit:session:claude:agent-session/moved").is_empty());
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(shutdown["shutdown"], json!(true));
}

/// ai-kit#640 (R640-2): the body an open launches is handed its session's record
/// (`AIKIT_SESSION_LIFECYCLE_REF`) and the open that launched it (`AIKIT_SESSION_LIFECYCLE_OPEN`,
/// its connection generation), so the body's own hooks record its end there, never on a second
/// record under the harness's own id. The fixture writes what it was handed to its log.
#[test]
fn the_body_an_open_launches_is_handed_its_sessions_record() {
    let world = QueueWorld::new();
    let (space, session) = world.attach("handed");
    let service = EncounterService::new(world.home.clone()).unwrap();
    let opened = world.open(&service, &space, &session, "handed");
    assert_eq!(
        handed_to_the_body(&world, "handed"),
        json!({
            "method": "fixture/environment",
            "AIKIT_SESSION_LIFECYCLE_REF": "aikit:session:handed:agent-session/handed",
            "AIKIT_SESSION_LIFECYCLE_OPEN": opened["connection_generation"],
        }),
        "{opened}"
    );
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(shutdown["shutdown"], json!(true));
}

/// What the body an open launched was handed of its session's record, as the fixture logged it.
fn handed_to_the_body(world: &QueueWorld, provider: &str) -> Value {
    let handed: Vec<Value> = fs::read_to_string(world.cwd.join(format!("{provider}.log")))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|line| line["method"] == "fixture/environment")
        .collect();
    assert_eq!(handed.len(), 1, "one body launched: {handed:?}");
    handed[0].clone()
}

/// One hook a harness of `client` dispatches for its native session `id`, as
/// `aikit hook dispatch <client> <event>` runs the session boundary in a process handed `handed`.
fn hook_in_body(
    home: &AikitHome,
    client: &str,
    kind: aikit_core::hooks::HookEventKind,
    id: &str,
    handed: &Value,
) {
    let event = aikit_core::hooks::HookEvent::new(client, kind.clone(), json!({"session_id": id}));
    let mut decision = aikit_core::hooks::HookDecision {
        event: kind,
        allowed: true,
        denial: None,
        payload: json!({}),
        injected: vec![],
        warnings: vec![],
        steps: vec![],
        groups: vec![],
        bypass_consumed: false,
    };
    let handed = aikit_adapters::connection_process::SessionLifecycleHandoff::from_env(|name| {
        handed[name].as_str().map(str::to_owned)
    });
    crate::temporal::session_boundary(
        &mut decision,
        &event,
        None,
        Some(home),
        Some(&crate::temporal::FloorLedger::in_state(&home.state())),
        &aikit_adapters::runner::ScriptedRunner::new(),
        false,
        handed.as_ref(),
    );
    assert!(decision.warnings.is_empty(), "{:?}", decision.warnings);
}

/// ai-kit#640 (re-read F1): a claude the body launches in turn (`claude -p`, an Agent SDK script,
/// `aikit route launch`) inherits what the body was handed and runs AIKit's hooks on it. The
/// session's record is the body's: two such sessions, one after the other, each start and end on
/// their own records; the session runs on in its record as in its body, and its End ends it,
/// answering ended. Unfenced, the first nested end ended the body's record while the body ran, the
/// next nested start resumed it, and the End then answered ended false.
#[test]
fn a_harness_the_body_launches_in_turn_leaves_the_sessions_record_to_its_end() {
    use aikit_core::hooks::HookEventKind::{SessionEnd, SessionStart};
    use aikit_core::session_lifecycle::SessionLifecycleEventKind::{SessionEnded, SessionStarted};
    let world = QueueWorld::new();
    let (space, session) = world.attach("nesting");
    let service = EncounterService::new(world.home.clone()).unwrap();
    let session_ref = "aikit:session:nesting:agent-session/nesting";
    let opened = world.open(&service, &space, &session, "nesting");
    assert_eq!(opened["lifecycle"]["start"], "first", "{opened}");
    let handed = handed_to_the_body(&world, "nesting");
    hook_in_body(&world.home, "nesting", SessionStart, "body", &handed);
    for nested in ["nested-1", "nested-2"] {
        hook_in_body(&world.home, "nesting", SessionStart, nested, &handed);
        hook_in_body(&world.home, "nesting", SessionEnd, nested, &handed);
        assert_eq!(
            record_of(&world.home, session_ref),
            [SessionStarted],
            "{nested} left the session's record running"
        );
        assert_eq!(
            record_of(&world.home, &format!("aikit:session:nesting:{nested}")),
            [SessionStarted, SessionEnded],
            "{nested} kept its own record"
        );
    }
    let live = view_of(&service, &session);
    assert_eq!(live["connection"]["resident"], true, "{live}");
    assert_eq!(live["lifecycle"]["running"], true, "{live}");
    let ended = service
        .apply(EncounterRequest::End {
            agent_session: session.clone(),
        })
        .unwrap();
    assert_eq!(ended["ended"], true, "{ended}");
    assert_eq!(
        record_of(&world.home, session_ref),
        [SessionStarted, SessionEnded]
    );
    assert!(record_of(&world.home, "aikit:session:nesting:body").is_empty());
    let shutdown = service
        .apply(EncounterRequest::Shutdown {
            expected_pid: std::process::id(),
        })
        .unwrap();
    assert_eq!(shutdown["shutdown"], json!(true));
}
