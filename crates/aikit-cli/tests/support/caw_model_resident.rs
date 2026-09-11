//! Extends the existing native delivery campaign and its disposable World, not
//! another execution harness. No Profile, Project or Factory is invented.
use super::*;
use aikit_adapters::credential_provider::EnvironmentImportProvider;
use aikit_core::credential::{CredentialRef, SecretProvider};
use aikit_core::resource::{CredentialCondition, DeclaredRoute, ModelCatalogueEntry, ModelRouteKind, ProviderRef, SourceRef};
use aikit_store::CredentialBindingStore;
use std::time::{SystemTime, UNIX_EPOCH};

const SECRET: &str = "CONTROLLED_MODEL_SECRET_NOT_USER_DATA";

fn publish_catalogue_fixture(w: &World, entry: &ModelCatalogueEntry) {
    // Actual documented authored catalogue source, loaded by the production
    // catalogue owner. This is not a hand-seeded Wiki/detection identity.
    let directory=w.home.root().join(aikit_store::model_catalogue::MODEL_CATALOGUE_DIR);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("controlled.json"), serde_json::to_vec(&vec![entry]).unwrap()).unwrap();
    assert_eq!(aikit_store::model_catalogue::resolved_catalogue(&w.home).0.get(&entry.model), Some(entry));
}

fn catalogue(w: &World) -> ModelCatalogueEntry {
    let entry = ModelCatalogueEntry {
        model:r("model:controlled-caw"), name:"Controlled model, not commercial evidence".into(),
        description:"Native caller and identity proof".into(), superseded_refs:Default::default(),
        routes:vec![DeclaredRoute { provider:ProviderRef::parse("provider:controlled-native").unwrap(),
            kind:ModelRouteKind::ProviderNative, native_ids:["controlled-model-v1".to_string()].into(),
            credential:CredentialCondition::Required { hint:"Explicit controlled environment source".into() } }],
        source:SourceRef::parse("source/controlled-catalogue").unwrap(), freshness:None,
    };
    publish_catalogue_fixture(w, &entry);
    entry
}

fn model_setup(w: &World, mode: &str, authority: bool) -> Value {
    let mut binding = w.attach("root", "pi");
    let mut source: Value = serde_json::from_slice(&fs::read(&binding.agency_source.path).unwrap()).unwrap();
    if authority { source["determination"]["delegated_autonomy"]["allowed_action_refs"] = json!(["action/aikit/encounter-send", "action/aikit/model-realise"]); }
    let bytes = serde_json::to_vec(&source).unwrap();
    fs::write(&binding.agency_source.path, &bytes).unwrap();
    binding.revision = rev("rev/2");
    binding.agency_source.revision = rev("rev/native-2");
    binding.agency_source.content_digest = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    w.cli(&["encounter-agency-configure".into(), "--agent-session".into(), "agent-session/root".into(),
        "--binding-json".into(), serde_json::to_string(&binding).unwrap(), "--expected-revision".into(), "rev/1".into()]);
    catalogue(w);
    let policy = json!({"schema":"aikit.model-dispatch-policy/v1", "agent_ref":"agent:root", "world_ref":"central:root",
        "authority_ref":"authority:project:delegation", "bounds_refs":["bound:project:delegation"],
        "model_ref":"model:controlled-caw", "provider_ref":"provider:controlled-native", "native_provider":"controlled", "provider_native_id":"controlled-model-v1",
        "expires_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() + 300_000,
        "credential":{"requirement_ref":"secret-requirement/model-test", "credential_ref":"credential/model-test",
            "target_env":"CAW_NATIVE_API_KEY", "from_env":"CAW_SOURCE_API_KEY"}});
    let path = w.temp.path().join("model-policy.json");
    let bytes = serde_json::to_vec(&policy).unwrap(); fs::write(&path, &bytes).unwrap();
    let provider = json!({"id":"root", "label":"Explicit model-selected native Pi fixture", "protocol":"pi-rpc",
        "argv":["python3", "-u", Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/caw_model_provider.py"), mode, w.temp.path().join("root.log")],
        "model_policy":{"source":"source/model-policy", "revision":"rev/model-1", "path":path,
            "content_digest":format!("blake3:{}", blake3::hash(&bytes).to_hex())}});
    fs::write(w.temp.path().join("model-provider.json"), provider.to_string()).unwrap();
    w.cli(&["encounter-configure".into(), "--provider-json".into(), provider.to_string()]);
    let admitted = admit_agency(&SystemRunner::new(), actuation().to_str().unwrap(), &binding.agency_source,
        &binding.agent_ref, &binding.world_ref).unwrap();
    json!({"action":"open-model", "request":{"space":"session-space/root", "agent_session":"agent-session/root", "cwd":w.temp.path(),
        "model_ref":"model:controlled-caw", "provider_ref":"provider:controlled-native", "body":"root", "expected_agency":admitted}})
}

fn start_model(w: &mut World, key: bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"));
    command.env("AIKIT_HOME", w.home.root()).env("UNRELATED_API_KEY", "MUST_NOT_LEAK")
        .env("CENTRAL_NATIVE_TOKEN", "MUST_NOT_LEAK").env("WORKCELL_CONTROL_TOKEN", "MUST_NOT_LEAK");
    if key { command.env("CAW_SOURCE_API_KEY", SECRET); } else { command.env_remove("CAW_SOURCE_API_KEY"); }
    w.child = Some(command.arg("-C").arg(w.temp.path()).args(["encounter-serve", "--socket"]).arg(&w.socket)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap());
    let end = Instant::now() + Duration::from_secs(15);
    while !w.socket.exists() { assert!(Instant::now() < end); std::thread::sleep(Duration::from_millis(25)); }
}
fn send(w: &World, delivery: &str) -> Value {
    let mut request = w.turn("root", delivery, "Use the explicitly selected model and context");
    request["turn"]["expected_binding_revision"] = json!("rev/2");
    w.request(request)
}
fn prompts(w: &World) -> usize {
    fs::read_to_string(w.temp.path().join("root.log")).unwrap_or_default().lines()
        .filter(|l| serde_json::from_str::<Value>(l).unwrap()["type"] == "prompt").count()
}
fn assert_no_secret(path: &Path) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() { assert_no_secret(&path); }
        else if path.is_file() { assert!(!fs::read(&path).unwrap().windows(SECRET.len()).any(|x| x == SECRET.as_bytes()), "secret persisted in {}", path.display()); }
    }
}

#[test]
#[ignore="actual pinned Actuation and native Pi protocol fixture; mandatory CAW campaign"]
fn catalogued_profileless_model_reaches_actual_resident_and_returns_with_scoped_credential() {
    let mut w = World::new(); let target = model_setup(&w, "normal", true); start_model(&mut w, true);
    let opened = w.request(target.clone()); assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(opened["data"]["executed"], false);
    assert_eq!(opened["data"]["model_selection"]["policy"]["model_ref"], "model:controlled-caw");
    assert_eq!(opened["data"]["model_observation"]["current_model_id"], "controlled-model-v1");
    assert_eq!(send(&w, "model-one")["ok"], true);
    assert_eq!(w.returned("root", "model-one")["data"]["phase"], "returned");
    let facts: Value = serde_json::from_slice(&fs::read(w.temp.path().join("root.facts.json")).unwrap()).unwrap();
    assert_eq!(facts["credential_delivered"], true); assert_eq!(facts["selected_context"], true);
    for field in ["source_credential_leaked", "unrelated_credential_leaked", "central_token_leaked", "workcell_token_leaked"] { assert_eq!(facts[field], false, "{facts}"); }
    assert_eq!(w.request(target)["ok"], true); assert_eq!(prompts(&w), 1);
    assert_eq!(send(&w, "model-one")["ok"], true); assert_eq!(prompts(&w), 1);
    assert!(!w.temp.path().join("ProjectCentral").exists());
    assert_no_secret(w.home.root());
    w.stop();
    println!("MODEL_SELECTED_NATIVE_RESPONSE_EXECUTED");
}

#[test]
#[ignore="actual native owners; mandatory CAW campaign"]
fn missing_authority_credential_and_mismatched_selection_refuse_before_provider_start() {
    for failure in ["authority", "credential", "model", "provider", "world", "bounds", "expired", "source"] {
        let mut w = World::new(); let mut target = model_setup(&w, "normal", failure != "authority");
        match failure {
            "model" => target["request"]["model_ref"] = json!("model:absent"),
            "provider" => target["request"]["provider_ref"] = json!("provider:other"),
            "world" => target["request"]["expected_agency"]["world_ref"] = json!("world:other"),
            "bounds" | "expired" => {
                let path = w.temp.path().join("model-policy.json"); let mut p:Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                if failure=="bounds" { p["bounds_refs"]=json!(["bound:ungranted"]); } else { p["expires_at_unix_ms"]=json!(1); }
                let bytes=serde_json::to_vec(&p).unwrap(); fs::write(&path,&bytes).unwrap();
                let mut provider: aikit_cli::encounter_service::EncounterProvider = serde_json::from_slice(&fs::read(w.temp.path().join("model-provider.json")).unwrap()).unwrap();
                provider.model_policy.as_mut().unwrap().content_digest=format!("blake3:{}",blake3::hash(&bytes).to_hex());
                w.cli(&["encounter-configure".into(),"--provider-json".into(),serde_json::to_string(&provider).unwrap()]);
            }
            "source" => { fs::remove_file(w.temp.path().join("model-policy.json")).unwrap(); }
            _ => {}
        }
        start_model(&mut w, failure != "credential");
        let outcome=w.request(target); assert_eq!(outcome["ok"],false,"{failure}: {outcome}");
        assert!(!w.temp.path().join("root.log").exists(),"provider started for {failure}"); w.stop();
    }
}

#[test]
#[ignore="actual native owners; mandatory CAW campaign"]
fn changed_catalogue_withheld_source_and_revoked_credential_stop_subsequent_dispatch() {
    for change in ["catalogue", "withheld", "revoked"] {
        let mut w=World::new(); let target=model_setup(&w,"normal",true); start_model(&mut w,true);
        assert_eq!(w.request(target)["ok"],true); assert_eq!(send(&w,"first")["ok"],true);
        assert_eq!(w.returned("root","first")["data"]["phase"],"returned");
        match change {
            "catalogue" => { let mut entry=catalogue(&w); entry.description.push_str(" changed"); publish_catalogue_fixture(&w, &entry); }
            "withheld" => { fs::write(w.temp.path().join(".no-agent-retrieval"),"withheld").unwrap(); }
            "revoked" => {
                let reference=CredentialRef::new("credential/model-test").unwrap();
                let provider=EnvironmentImportProvider::from_value(reference.clone(),"CAW_SOURCE_API_KEY",Some(SECRET.into())).unwrap();
                let mut state=provider.binding_state(&reference).unwrap().unwrap(); state.revoked=true;
                CredentialBindingStore::new(&w.home).save(&state).unwrap();
            }
            _=>unreachable!(),
        }
        let outcome=send(&w,"after-change"); assert_eq!(outcome["ok"],false,"{change}: {outcome}");
        assert_eq!(prompts(&w),1); assert_eq!(w.returned("root","first")["data"]["phase"],"returned"); w.stop();
    }
}

#[test]
#[ignore="actual native owners; mandatory CAW campaign"]
fn actual_pi_model_mismatch_is_never_a_successful_selected_response() {
    for mode in ["wrong-state", "wrong-response", "drift"] {
        let mut w=World::new(); let target=model_setup(&w,mode,true); start_model(&mut w,true);
        let opened=w.request(target);
        if mode=="wrong-state" { assert_eq!(opened["ok"],false); assert_eq!(prompts(&w),0); }
        else {
            assert_eq!(opened["ok"],true,"{opened}"); assert_eq!(send(&w,"first")["ok"],true);
            let returned=w.returned("root","first");
            if mode=="wrong-response" { assert_eq!(returned["data"]["phase"],"failed","{returned}"); }
            else { assert_eq!(returned["data"]["phase"],"returned"); assert_eq!(send(&w,"after-drift")["ok"],false); assert_eq!(prompts(&w),1); }
        }
        w.stop();
    }
}
