//! Controlled Prime peer, real native store/handler/Prime RPC/stdio and the
//! real Node locator over a controlled Prime package. This is not live
//! inference and never an installed Prime.
#![cfg(unix)]
use aikit_cli::encounter_service::{
    EncounterProtocol, EncounterProvider, EncounterRequest, EncounterService,
};
use aikit_core::{
    session_space::SessionSpaceRef,
    session_space_application::{SessionSpaceAgentAttachmentIntent, SessionSpaceMutation},
    ResourceRef,
};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc};

struct PrimeRig {
    temp: tempfile::TempDir,
    home: AikitHome,
    service: Arc<EncounterService>,
    space: SessionSpaceRef,
    session: ResourceRef,
}

impl PrimeRig {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path().join("aikit"));
        let store = SessionSpaceApplicationStore::new(home.clone());
        let space = SessionSpaceRef::parse("session-space/prime-controlled").unwrap();
        let session = ResourceRef::parse("agent-session/prime-controlled").unwrap();
        store
            .apply(
                &store
                    .stage(
                        None,
                        SessionSpaceMutation::Create {
                            id: space.clone(),
                            label: Some("Controlled Prime session".into()),
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
                                purpose: Some("Controlled Prime continuity regression".into()),
                                provenance: vec!["test-only".into()],
                            },
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        // The installed-package shape the owner's locator reads: the release
        // in package.json and Prime's own getSessionsDir in dist/config.js.
        let package = temp.path().join("prime-package");
        fs::create_dir_all(package.join("dist/cli")).unwrap();
        fs::write(
            package.join("package.json"),
            br#"{"name":"controlled-prime","version":"0.9.4","type":"module"}"#,
        )
        .unwrap();
        fs::write(package.join("dist/cli/prime.js"), b"// controlled\n").unwrap();
        fs::write(
            package.join("dist/config.js"),
            format!(
                "export function getSessionsDir(){{return {};}}\n",
                serde_json::to_string(&temp.path().join("sessions")).unwrap()
            ),
        )
        .unwrap();
        let rig = Self {
            service: Arc::new(EncounterService::new(home.clone()).unwrap()),
            temp,
            home,
            space,
            session,
        };
        rig.configure("source-revision-1");
        rig
    }

    fn configure(&self, body_revision: &str) {
        let peer =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/prime_rpc_peer.py");
        EncounterService::configure(
            &self.home,
            EncounterProvider {
                protocol: EncounterProtocol::PrimeRpc,
                id: "controlled-prime".into(),
                label: "Controlled Prime peer (not a model)".into(),
                argv: vec![
                    "python3".into(),
                    "-u".into(),
                    peer.display().to_string(),
                    self.record().display().to_string(),
                    self.temp.path().join("sessions").display().to_string(),
                    "--prime-bin".into(),
                    self.temp
                        .path()
                        .join("prime-package/dist/cli/prime.js")
                        .display()
                        .to_string(),
                ],
                body_ref: Some("agent-body/controlled-prime".into()),
                body_revision: Some(body_revision.into()),
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
    }

    fn record(&self) -> PathBuf {
        self.temp.path().join("peer-record.jsonl")
    }

    fn records(&self) -> Vec<Value> {
        fs::read_to_string(self.record())
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn open(&self, reconnect: bool) -> EncounterRequest {
        let (space, agent_session, provider, cwd) = (
            self.space.clone(),
            self.session.clone(),
            "controlled-prime".to_owned(),
            self.temp.path().to_path_buf(),
        );
        if reconnect {
            EncounterRequest::Reconnect {
                space,
                agent_session,
                provider,
                cwd,
            }
        } else {
            EncounterRequest::Open {
                space,
                agent_session,
                provider,
                cwd,
            }
        }
    }

    fn native(&self) -> String {
        self.service
            .apply(EncounterRequest::View {
                agent_session: self.session.clone(),
                before: None,
            })
            .unwrap()["connection"]["native_session_id"]
            .as_str()
            .expect("a bound native session")
            .to_owned()
    }

    fn restart(&mut self) {
        self.service
            .apply(EncounterRequest::Shutdown {
                expected_pid: std::process::id(),
            })
            .unwrap();
        self.service = Arc::new(EncounterService::new(self.home.clone()).unwrap());
    }
}

#[test]
fn prime_reconnect_resumes_the_recorded_native_session_after_restart() {
    let mut rig = PrimeRig::new();
    rig.service.apply(rig.open(false)).unwrap();
    let native = rig.native();
    let recorded_file = rig
        .temp
        .path()
        .join("sessions")
        .join(format!("{native}.jsonl"));
    assert!(recorded_file.is_file());

    rig.restart();
    let resumed = rig.service.apply(rig.open(true));
    assert!(
        resumed.is_ok(),
        "Prime reconnect must resume the recorded native session: {:?}",
        resumed
            .err()
            .map(|failure| (failure.code().to_owned(), failure.to_string()))
    );
    assert_eq!(rig.native(), native, "the same native session continues");

    let records = rig.records();
    let switched = records
        .iter()
        .filter(|record| record["command"] == "switch_session")
        .map(|record| record["payload"]["sessionPath"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        switched,
        [serde_json::json!(recorded_file.display().to_string())],
        "the replacement process switched to exactly the recorded native file: {records:?}"
    );
    // Each launch is Prime's own launcher and carries the canonical context
    // once: this session, and that generation's private directory.
    let launches = records
        .iter()
        .filter_map(|record| record["argv"].as_array())
        .collect::<Vec<_>>();
    assert_eq!(launches.len(), 2, "{records:?}");
    let mut directories = Vec::new();
    for argv in launches {
        let flag = |name: &str| {
            let at = argv
                .iter()
                .enumerate()
                .filter(|(_, arg)| *arg == name)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            assert_eq!(at.len(), 1, "{name} once in {argv:?}");
            argv[at[0] + 1].as_str().unwrap().to_owned()
        };
        assert_eq!(flag("--agent-session"), rig.session.as_str());
        directories.push(PathBuf::from(flag("--child-message-dir")));
    }
    assert_ne!(
        directories[0], directories[1],
        "one directory per generation"
    );
    for directory in directories {
        assert!(directory.is_dir());
        assert!(directory.starts_with(rig.home.state().join("encounter-child-messages")));
    }
    rig.restart();
}

#[test]
fn prime_reconnect_refuses_a_changed_body_pin_before_any_launch() {
    let mut rig = PrimeRig::new();
    rig.service.apply(rig.open(false)).unwrap();
    rig.restart();
    rig.configure("source-revision-2");
    let launches_before = rig
        .records()
        .iter()
        .filter(|r| r["argv"].is_array())
        .count();

    let refused = rig
        .service
        .apply(rig.open(true))
        .expect_err("a changed body revision is another body, never a resume");
    assert_eq!(refused.code(), "encounter.reconnect_basis", "{refused}");
    assert_eq!(
        rig.records()
            .iter()
            .filter(|r| r["argv"].is_array())
            .count(),
        launches_before,
        "no replacement process was started"
    );
}
