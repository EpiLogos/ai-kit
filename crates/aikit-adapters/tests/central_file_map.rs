use aikit_adapters::central_file_map::{call, CentralFileMapProvider};
use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::knowledge_source_pool::SourcePoolProvider;
use aikit_core::resource::SourceRef;
use aikit_core::Result;
use serde_json::{json, Value};
use std::{path::Path, sync::Mutex};

struct Owner {
    replies: Mutex<Vec<Value>>,
    calls: Mutex<Vec<Vec<String>>>,
}
impl Owner {
    fn new(replies: Vec<Value>) -> Self {
        Self {
            replies: Mutex::new(replies),
            calls: Mutex::new(Vec::new()),
        }
    }
}
impl CommandRunner for Owner {
    fn run(&self, args: &[String]) -> Result<Output> {
        self.calls.lock().unwrap().push(args.to_vec());
        let value = self.replies.lock().unwrap().remove(0);
        Ok(Output::success(value.to_string()))
    }
}
fn envelope(data: Value) -> Value {
    json!({"ok":true,"data":data})
}
fn source() -> Value {
    json!({"source_ref":"central:source:test","revision":"r1","path":"note.txt","retrieval_allowed":true,"options":{},"kind":"file"})
}
fn roster() -> Value {
    envelope(
        json!({"schema":"central.file-map/v1","maps":[{"configured":true,"version":"bkmr 7.6.7","embeddings_ready":false,"sources":[source()]}]}),
    )
}

#[test]
fn source_read_returns_to_owner_and_does_not_rebuild() {
    let owner = Owner::new(vec![
        roster(),
        envelope(json!({"schema":"central.file-map/v1","source":source(),"content":"live bytes"})),
    ]);
    let mut provider =
        CentralFileMapProvider::connect(&owner, "ctrl", "/world", Some("alpha")).unwrap();
    assert!(provider.descriptors()[0].body.is_empty());
    assert!(provider.rebuild(&[]).is_err());
    let material = provider
        .read(&SourceRef::parse("central:source:test").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(material.body, "live bytes");
    let calls = owner.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0][6], "central.map.inspect");
    assert_eq!(calls[1][6], "central.map.resolve");
    let input: Value = serde_json::from_str(&calls[0][7]).unwrap();
    assert_eq!(input["project"], "alpha");
    assert_eq!(input["include_root"], true);
}
#[test]
fn false_or_wrong_schema_owner_envelopes_are_not_accepted() {
    for answer in [
        json!({"ok":false,"data":{"schema":"central.file-map/v1"}}),
        envelope(json!({"schema":"another-contract"})),
    ] {
        let owner = Owner::new(vec![answer]);
        assert!(call(
            &owner,
            Path::new("ctrl"),
            Path::new("/world"),
            "inspect",
            &json!({})
        )
        .is_err());
    }
}
#[test]
fn revoked_source_payload_is_not_accepted() {
    let mut withheld = source();
    withheld["retrieval_allowed"] = json!(false);
    let owner = Owner::new(vec![
        roster(),
        envelope(
            json!({"schema":"central.file-map/v1","source":withheld,"content":"must not return"}),
        ),
    ]);
    let provider = CentralFileMapProvider::connect(&owner, "ctrl", "/world", None).unwrap();
    assert!(provider
        .read(&SourceRef::parse("central:source:test").unwrap())
        .is_err());
}
#[test]
fn missing_native_binary_does_not_advertise_search() {
    let mut reading = roster();
    reading["data"]["maps"][0]["version"] = Value::Null;
    let owner = Owner::new(vec![reading]);
    let provider = CentralFileMapProvider::connect(&owner, "ctrl", "/world", None).unwrap();
    assert!(!provider.capabilities().fulltext);
    assert!(!provider.capabilities().hybrid);
}
#[test]
fn disposable_provider_refuses_central_owned_database() {
    use aikit_adapters::{bkmr::BkmrSourcePoolProvider, runner::SystemRunner};
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join(".central/bkmr/map.db");
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    std::fs::write(&database, b"persistent owner sentinel").unwrap();
    let mut provider =
        BkmrSourcePoolProvider::with_binary(SystemRunner::new(), "missing-bkmr", &database, false);
    assert!(provider.rebuild(&[]).is_err());
    assert_eq!(
        std::fs::read(&database).unwrap(),
        b"persistent owner sentinel"
    );
}
