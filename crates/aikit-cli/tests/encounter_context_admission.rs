//! Real source files, native encounter owner and OS process boundary; no ACP mock.
use aikit_cli::encounter_service::{
    EncounterContextAdmission, EncounterProvider, EncounterRequest, EncounterRequiredSource,
    EncounterService,
};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
};
use aikit_core::{ResourceRef, SourceRevision};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use std::fs;

fn context(path: std::path::PathBuf, bytes: &[u8]) -> EncounterContextAdmission {
    EncounterContextAdmission {
        sources: vec![EncounterRequiredSource {
            source: ResourceRef::parse("source/acceptance/governance").unwrap(),
            revision: SourceRevision::parse("owner-revision/accepted-1").unwrap(),
            path,
            content_digest: format!("blake3:{}", blake3::hash(bytes).to_hex()),
        }],
        source_activations: vec![],
        projection: None,
        activation: None,
    }
}

fn attached(home: &AikitHome) -> (SessionSpaceRef, ResourceRef) {
    let store = SessionSpaceApplicationStore::new(home.clone());
    let space = SessionSpaceRef::parse("session-space/admission-test").unwrap();
    let session = ResourceRef::parse("agent-session/admission-test").unwrap();
    let create = store
        .stage(
            None,
            SessionSpaceMutation::Create {
                id: space.clone(),
                label: None,
            },
        )
        .unwrap();
    store.apply(&create).unwrap();
    let attach = store
        .stage(
            Some(&space),
            SessionSpaceMutation::AttachAgentSession {
                attachment: SessionSpaceAgentAttachmentIntent {
                    agent_session: session.clone(),
                    purpose: Some("Verify admission before a real process side effect".into()),
                    provenance: vec!["explicit temporary acceptance commission".into()],
                },
            },
        )
        .unwrap();
    store.apply(&attach).unwrap();
    (space, session)
}

#[test]
fn required_material_is_checked_without_claiming_runtime_loading() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("law.md");
    fs::write(&path, b"Keep the commissioned operation bounded.\n").unwrap();
    let required = context(path.clone(), b"Keep the commissioned operation bounded.\n");
    required.verify().unwrap();
    assert!(required.source_activations.is_empty());
    assert!(required.activation.is_none());
    fs::write(&path, b"A different source revision.\n").unwrap();
    assert_eq!(
        required.verify().unwrap_err().code(),
        "encounter.context_stale"
    );
    fs::remove_file(path).unwrap();
    assert_eq!(
        required.verify().unwrap_err().code(),
        "encounter.context_unavailable"
    );
}

#[test]
#[cfg(unix)]
fn missing_and_stale_required_source_refuse_before_real_provider_process_effect() {
    let temp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(temp.path().join("aikit"));
    let (space, session) = attached(&home);
    let service = EncounterService::new(home.clone()).unwrap();
    let source = temp.path().join("law.md");
    let marker = temp.path().join("provider-started");
    let provider = EncounterProvider {
        id: "native-boundary".into(),
        label: "Real process admission boundary".into(),
        // touch is an actual OS effect, not a pretend ACP implementation. It
        // must never execute in either denied case. It cannot pass ACP setup.
        argv: vec!["/usr/bin/touch".into(), marker.display().to_string()],
        required_context: Some(context(source.clone(), b"admitted source\n")),
    };
    EncounterService::configure(&home, provider.clone()).unwrap();
    let open = || EncounterRequest::Open {
        space: space.clone(),
        agent_session: session.clone(),
        provider: provider.id.clone(),
        cwd: temp.path().to_path_buf(),
    };
    let missing = service.apply(open()).unwrap_err();
    assert_eq!(missing.code(), "encounter.context_unavailable");
    assert!(!marker.exists());
    fs::write(&source, b"changed source\n").unwrap();
    assert_eq!(
        service.apply(open()).unwrap_err().code(),
        "encounter.context_stale"
    );
    assert!(!marker.exists());
    let page = service
        .apply(EncounterRequest::Read {
            agent_session: session.clone(),
            after: 0,
            limit: 20,
        })
        .unwrap();
    let events = page["events"].as_array().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"]["kind"] == "context-admission-refused")
            .count(),
        2
    );

    // Optional context retains its original behavior: it reaches the real OS
    // process. Expected ACP negotiation fails, because touch is not an agent.
    EncounterService::configure(
        &home,
        EncounterProvider {
            required_context: None,
            ..provider
        },
    )
    .unwrap();
    assert!(service
        .apply(EncounterRequest::Open {
            space,
            agent_session: session,
            provider: "native-boundary".into(),
            cwd: temp.path().to_path_buf()
        })
        .is_err());
    assert!(
        marker.exists(),
        "optional context must not invent an admission restriction"
    );
}
