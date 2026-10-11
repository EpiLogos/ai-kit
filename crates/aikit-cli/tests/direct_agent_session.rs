//! Controlled native validation, not a person's Agent or live-model evidence.
use aikit_cli::direct_agent_session::{find, validate_review, PrepareRequest};
use aikit_core::ResourceRef;
use serde_json::{json, Value};
fn fixture() -> (PrepareRequest, ResourceRef, Value) {
    let request = PrepareRequest {
        request_id: "controlled-request-12345".into(),
        profile_ref: "agent-profile:test".into(),
        expected_revision: "r1".into(),
        expected_content_digest: format!("sha256:{}", "a".repeat(64)),
        expected_acceptance_ref: "agent-profile-acceptance:controlled".into(),
    };
    let agent = ResourceRef::parse("agent:test").unwrap();
    let profile = json!({"schema":"central.agent-profile/v1","ref":request.profile_ref,"revision":"r1","agent_ref":agent,"scope":"personal","world_ref":"central:root","ratified_world_refs":["central:root"],"purpose":"Read explicit sources","intent_provenance":{"schema":"central.agent-profile-provenance/v1","intent_expression":"Read explicit sources","origin_action":"agent-profile.express","authorship":"generated-proposal","recognition":"unrecognised"}});
    let review = json!({"schema":"central.agent-profile-review/v1","accepted":true,"execution_authority_granted":false,"profile":profile,"scope_ref":"control:root","content_digest":request.expected_content_digest,"acceptance":{"schema":"central.agent-profile-acceptance/v1","acceptance_ref":request.expected_acceptance_ref,"profile_ref":request.profile_ref,"agent_ref":agent,"profile_revision":"r1","content_digest":request.expected_content_digest,"scope_ref":"control:root","principal_ref":"human:controlled-test","authority_ref":"authority:controlled-test","authority_revision":"policy:1"}});
    (request, agent, review)
}
#[test]
fn native_exact_acceptance_does_not_rewrite_generated_standing() {
    let (request, agent, review) = fixture();
    validate_review(&request, &agent, &review).unwrap();
    assert_eq!(
        review["profile"]["intent_provenance"]["recognition"],
        "unrecognised"
    );
}
#[test]
fn unaccepted_stale_or_different_identity_receipts_are_not_admission() {
    let (request, agent, review) = fixture();
    for (pointer, value) in [
        ("/accepted", json!(false)),
        ("/execution_authority_granted", json!(true)),
        ("/profile/revision", json!("r2")),
        ("/content_digest", json!("sha256:changed")),
        ("/profile/agent_ref", json!("agent:other")),
        ("/acceptance/agent_ref", json!("agent:other")),
        ("/acceptance/scope_ref", json!("project:other")),
        ("/acceptance/acceptance_ref", json!("acceptance:other")),
        ("/acceptance/authority_ref", json!("")),
        ("/acceptance/principal_ref", Value::Null),
    ] {
        let mut changed = review.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            validate_review(&request, &agent, &changed).is_err(),
            "{pointer}"
        );
    }
}
#[test]
fn ingress_cannot_supply_agent_identity_launch_argv_or_human_token() {
    let (request, _, _) = fixture();
    for field in [
        "agent_ref",
        "agent_session",
        "accepted",
        "argv",
        "token",
        "model",
        "authority_granted",
    ] {
        let mut value = serde_json::to_value(&request).unwrap();
        value[field] = json!("forged");
        assert!(
            serde_json::from_value::<PrepareRequest>(value).is_err(),
            "{field}"
        );
    }
}
#[test]
fn reading_an_unknown_request_never_creates_a_session_or_store() {
    let temp = tempfile::tempdir().unwrap();
    let home = aikit_store::AikitHome::at(temp.path().join("absent"));
    assert_eq!(
        find(&home, "controlled-request-12345").unwrap(),
        Value::Null
    );
    assert!(!home.root().exists());
    assert!(find(&home, "../redirect").is_err());
}
#[test]
#[cfg(unix)]
fn direct_binding_read_refuses_directory_redirect() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
    home.ensure_layout().unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, home.state().join("encounter-agents")).unwrap();
    assert!(find(&home, "controlled-request-12345").is_err());
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

// ---------------------------------------------------------------------------
// A4: the session-space prepare path resolves `skill_set_refs` into the
// effective repertoire — nested membership included — names withheld members
// with the resolver's own reason, and preserves individually named exceptions.
// ---------------------------------------------------------------------------

mod repertoire {
    use aikit_adapters::central_agent_profile::CentralAgentProfileProjection;
    use aikit_cli::app::Service;
    use aikit_cli::direct_agent_session::{material, SkillDigest};
    use aikit_core::trust::{TrustKey, TrustState};
    use aikit_core::{CapsuleId, Catalog, RegistrySource, ResourceRef};
    use aikit_store::home::AikitHome;
    use aikit_store::index::Index;
    use aikit_store::registry::load_registry;
    use aikit_store::trust::TrustStore;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    const CONTEXT_ID: &str = "ctx_01HZAW94REPERTOIRE00000000";

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// One real Skill capsule in the personal registry.
    fn skill(home: &Path, id: &str, name: &str, description: &str) {
        let capsule = home.join(format!("registries/personal/capsules/{id}"));
        write(
            &capsule.join("manifest.toml"),
            &format!(
                r#"schema = 1
id = "{id}"
kind = "skill"
name = "{name}"
description = "{description}"

[skill]
root = "payload"
"#
            ),
        );
        write(
            &capsule.join("payload/SKILL.md"),
            &format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n"),
        );
    }

    /// A home with a parent SkillSet `demo` that nests `demo-inner`, a project
    /// that enables exactly two of the three skills, and the profile under
    /// test: SkillSet `demo` plus one individually named exception.
    fn fixture() -> (tempfile::TempDir, Service, CentralAgentProfileProjection) {
        let root = tempfile::tempdir().unwrap();
        let home_path = root.path().join("home");
        let project = root.path().join("project");
        fs::create_dir_all(&project).unwrap();

        // member-a: in the parent set, enabled → delivered.
        skill(
            &home_path,
            "skill/test/member-a",
            "member-a",
            "Set member that projects.",
        );
        // member-b: in the NESTED child set, not enabled → withheld, named.
        skill(
            &home_path,
            "skill/test/member-b",
            "member-b",
            "Nested member that does not project.",
        );
        // extra: outside the set, individually named → the exception, preserved.
        skill(
            &home_path,
            "skill/test/extra",
            "extra",
            "Individually named exception.",
        );

        // The nested set membership: demo -> demo-inner -> member-b.
        write(
            &home_path.join("skillsets/demo-inner/members"),
            "skill/test/member-b\n",
        );
        write(
            &home_path.join("skillsets/demo/set.toml"),
            "children = [\"demo-inner\"]\n",
        );
        write(
            &home_path.join("skillsets/demo/members"),
            "skill/test/member-a\n",
        );

        write(
            &project.join(".aikit/profile.toml"),
            "schema = 1\nenable = [\"skill/test/member-a\", \"skill/test/extra\"]\n",
        );

        // A deliberate human trust review for the two skills that must
        // project — the same review `aikit trust record` writes. Skills stay
        // inert until reviewed; the nested member stays unreviewed, which is
        // exactly the withheld reason the preparation must name.
        let home = AikitHome::at(&home_path);
        home.ensure_layout().unwrap();
        let index = Index::open(&home.database()).unwrap();
        let load =
            load_registry(&home.registry("personal"), RegistrySource::new("personal")).unwrap();
        for id in ["skill/test/member-a", "skill/test/extra"] {
            let capsule_id = CapsuleId::parse(id).unwrap();
            let capsule = load
                .catalog
                .get(&capsule_id)
                .unwrap_or_else(|| panic!("the seeded registry holds {id}"));
            let revision = capsule
                .revision
                .clone()
                .expect("a loaded capsule has a revision");
            TrustStore::new(&index)
                .record(
                    &TrustKey::new(RegistrySource::new("personal"), capsule_id, revision),
                    TrustState::Trusted,
                    Some("test-fixture review"),
                )
                .unwrap();
        }

        let profile = CentralAgentProfileProjection::parse(&json!({
            "schema": "central.agent-profile/v1",
            "ref": "agent-profile:repertoire",
            "revision": "r1",
            "agent_ref": "agent:repertoire",
            "scope": "personal",
            "world_ref": "central:root",
            "ratified_world_refs": ["central:root"],
            "purpose": "Prove SkillSet repertoire resolution for direct work",
            "skill_refs": ["skill/test/extra"],
            "skill_set_refs": ["demo"],
        }))
        .unwrap();

        let mut env = BTreeMap::new();
        env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
        let service = Service::open(home, &project, move |key| env.get(key).cloned()).unwrap();
        (root, service, profile)
    }

    fn references(digests: &[SkillDigest]) -> Vec<String> {
        digests
            .iter()
            .map(|digest| digest.reference.clone())
            .collect()
    }

    #[test]
    fn a_skillset_resolves_with_nested_members_and_preserves_the_exception() {
        let (_root, service, profile) = fixture();
        let (digests, withheld, text) = material(&service, &profile).unwrap();

        // The individually named exception is delivered, then the set members
        // that actually project (deterministic order: individuals first, then
        // the set's deduplicated membership).
        assert_eq!(
            references(&digests),
            vec![
                "skill/test/extra".to_string(),
                "skill/test/member-a".to_string(),
            ],
            "the exception and the projecting member are the effective repertoire"
        );

        // The nested member that does not project is withheld with a named
        // reason — never silently dropped, never failing the whole set.
        assert_eq!(withheld.len(), 1, "one withheld member: {withheld:?}");
        assert_eq!(withheld[0].reference, "skill/test/member-b");
        assert!(!withheld[0].reason.is_empty(), "the reason is named");

        // The delivered payload carries the effective source, quoted.
        assert!(text.contains("skill/test/extra"));
        assert!(text.contains("skill/test/member-a"));
        assert!(!text.contains("\"ref\":\"skill/test/member-b\""));
    }

    /// Round trip: a re-resolve (what `prompt` does against the prepared
    /// binding) reaches byte-identical digests and withheld members. A
    /// divergence is `direct_agent.skill_context_stale`, never silent
    /// replacement.
    #[test]
    fn re_resolution_round_trips_to_identical_repertoire() {
        let (_root, service, profile) = fixture();
        let (digests, withheld, text) = material(&service, &profile).unwrap();
        let (digests_again, withheld_again, text_again) = material(&service, &profile).unwrap();
        assert_eq!(digests, digests_again);
        assert_eq!(withheld, withheld_again);
        assert_eq!(text, text_again);
    }

    /// An individually named Skill that does not project refuses the
    /// preparation — a named Skill is a requirement, not a request.
    #[test]
    fn an_individually_named_unavailable_skill_refuses() {
        let (_root, service, mut profile) = fixture();
        profile
            .skill_refs
            .push(ResourceRef::parse("skill/test/member-b").unwrap());
        let error = material(&service, &profile).unwrap_err();
        assert_eq!(error.code(), "capabilities.not_active", "{error:?}");
    }
}

// ---------------------------------------------------------------------------
// Self-source delivery: a prepared session whose accepted profile carries a
// `self_definition` composes the exact accepted first-person orientation into
// the parent-session prompt payload through the shared identity compositor,
// refuses drift named, and stays byte-identical across restarts. Central is a
// controlled ctrl STUB (a simulated service, labelled as such — it answers
// `agent-profile.review`, `agent-profile.list` and
// `central.world.effective-sources` from fixed tables and panics on anything
// else); the compositor, pins and digest law are the real shared code.
// ---------------------------------------------------------------------------

mod self_source_delivery {
    use aikit_adapters::central_agent_profile::CentralAgentProfileProjection;
    use aikit_adapters::identity_composition::{
        compose_operative_orientation, resolve_self_sources, DIRECT_DELIVERY_MODE,
        IDENTITY_COMPOSITOR_VERSION,
    };
    use aikit_adapters::runner::{CommandRunner, Output};
    use aikit_cli::app::Service;
    use aikit_cli::direct_agent_session::{find, prepare_with, prompt_with, PrepareRequest};
    use aikit_core::ResourceRef;
    use aikit_store::AikitHome;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    /// The accepted first-person sources, byte-exact. The digests below are
    /// sha256 over exactly these bytes.
    const SELF_BYTES: &[u8] =
        b"# My standing\n\nI keep the World's registers honest, in the owner's voice.\n";
    const LOGOS_BYTES: &[u8] = b"You act from within a relation.\n";
    const SELF_DIGEST: &str =
        "sha256:84fad6f901f4b61a3f1a1e4c1bb73758321f9432d47b4235d082832b84f1272d";
    const LOGOS_DIGEST: &str =
        "sha256:8a8977236b1bdf2d3ab150ea5c3838385997d2b3ce14d1d258df85906d50fac7";
    const CONTEXT_ID: &str = "ctx_01HZAW94SELFSOURCE000000000";
    const REQUEST_ID: &str = "controlled-self-source-12345";
    const PROFILE_REF: &str = "agent-profile:self-proof";
    const AGENT_REF: &str = "agent:self-proof";

    /// `central_root_enclosing` reads the process environment, so the tests
    /// that drive the real prepare path hold one guard while they set and
    /// clear `CENTRAL_ROOT` around their Central-root work.
    static CENTRAL_ROOT_GUARD: Mutex<()> = Mutex::new(());

    fn request() -> PrepareRequest {
        PrepareRequest {
            request_id: REQUEST_ID.into(),
            profile_ref: PROFILE_REF.into(),
            expected_revision: "r1".into(),
            expected_content_digest: format!("sha256:{}", "a".repeat(64)),
            expected_acceptance_ref: "agent-profile-acceptance:controlled".into(),
        }
    }

    fn profile(with_self: bool) -> Value {
        let mut profile = json!({
            "schema":"central.agent-profile/v1",
            "ref":PROFILE_REF,
            "revision":"r1",
            "agent_ref":AGENT_REF,
            "scope":"personal",
            "world_ref":"central:root",
            "ratified_world_refs":["central:root"],
            "purpose":"Deliver the accepted self orientation",
            "intent_provenance":{"schema":"central.agent-profile-provenance/v1","intent_expression":"Deliver the accepted self orientation","origin_action":"agent-profile.express","authorship":"generated-proposal","recognition":"unrecognised"},
        });
        if with_self {
            profile["self_definition"] = json!({
                "source":{"reference":"central:source:control:root:Control/agents/self/definition.md","content_digest":SELF_DIGEST},
                "relational_logos":{"reference":"central:source:control:root:Control/agents/self/logos.md","content_digest":LOGOS_DIGEST},
            });
        }
        profile
    }

    /// The review envelope the owner answers for `agent-profile.review`, in
    /// the exact shape `validate_review` accepts.
    fn review(request: &PrepareRequest, profile: &Value) -> Value {
        json!({
            "schema":"central.agent-profile-review/v1",
            "accepted":true,
            "execution_authority_granted":false,
            "profile":profile,
            "scope_ref":"control:root",
            "content_digest":request.expected_content_digest,
            "acceptance":{
                "schema":"central.agent-profile-acceptance/v1",
                "acceptance_ref":request.expected_acceptance_ref,
                "profile_ref":request.profile_ref,
                "agent_ref":AGENT_REF,
                "profile_revision":request.expected_revision,
                "content_digest":request.expected_content_digest,
                "scope_ref":"control:root",
                "principal_ref":"human:controlled-test",
                "authority_ref":"authority:controlled-test",
                "authority_revision":"policy:1",
            }
        })
    }

    /// The controlled ctrl stub (simulated Central). It never executes a
    /// process; every delivery path under test consults exactly the three
    /// actions below and anything else panics the test.
    struct CentralStub {
        review: Value,
        list: Value,
        world: Value,
    }

    impl CommandRunner for CentralStub {
        fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
            let action = argv
                .iter()
                .position(|arg| arg == "run")
                .and_then(|position| argv.get(position + 1))
                .map(String::as_str)
                .unwrap_or_default()
                .to_owned();
            let data = match action.as_str() {
                "agent-profile.review" => &self.review,
                "agent-profile.list" => &self.list,
                "central.world.effective-sources" => &self.world,
                other => panic!("the controlled ctrl stub must not be asked for {other}: {argv:?}"),
            };
            Ok(Output::success(
                json!({"ok":true,"status":"success","data":data}).to_string(),
            ))
        }

        fn run_with_limits(
            &self,
            argv: &[String],
            _remaining_timeout: Duration,
            _remaining_bytes: usize,
            _strict_utf8: bool,
        ) -> aikit_core::Result<Output> {
            self.run(argv)
        }
    }

    /// The ground: a Central-shaped root (the self sources, the profile source
    /// file the listing names), the aikit home, and the stub answering for it.
    struct Ground {
        _temp: tempfile::TempDir,
        root: PathBuf,
        home: AikitHome,
        stub: CentralStub,
        review: Value,
    }

    impl Ground {
        /// `with_self` decides whether the accepted profile carries the
        /// self-definition pins; `write_sources` decides whether the pinned
        /// files exist on disk.
        fn new(with_self: bool, write_sources: bool) -> Ground {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("central");
            for directory in [
                "Control/agents/profiles",
                "Control/agents/self",
                "Control/machines",
                "Control/user/private",
                "Work",
                ".central",
            ] {
                std::fs::create_dir_all(root.join(directory)).unwrap();
            }
            if write_sources {
                std::fs::write(root.join("Control/agents/self/definition.md"), SELF_BYTES).unwrap();
                std::fs::write(root.join("Control/agents/self/logos.md"), LOGOS_BYTES).unwrap();
            }
            let profile = profile(with_self);
            std::fs::write(
                root.join("Control/agents/profiles/self-proof.json"),
                serde_json::to_vec_pretty(&profile).unwrap(),
            )
            .unwrap();
            let root = root.canonicalize().unwrap();
            let review = review(&request(), &profile);
            let stub = CentralStub {
                review: review.clone(),
                list: json!({
                    "profiles": [{
                        "profile": profile,
                        "source_path": "Control/agents/profiles/self-proof.json",
                    }]
                }),
                world: json!({"world_ref":"control:root","sources":[]}),
            };
            let home = AikitHome::at(temp.path().join("home"));
            Ground {
                _temp: temp,
                root,
                home,
                stub,
                review,
            }
        }

        fn self_sources_path(&self) -> PathBuf {
            self.root.join("Control/agents/self/definition.md")
        }
    }

    fn with_central_root<T>(root: &Path, work: impl FnOnce() -> T) -> T {
        let _guard = CENTRAL_ROOT_GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::set_var("CENTRAL_ROOT", root);
        let result = work();
        std::env::remove_var("CENTRAL_ROOT");
        result
    }

    /// Drive the real prepare path against the stub: `central_root_enclosing`
    /// resolves through `CENTRAL_ROOT`, every native owner read goes through
    /// the controlled stub.
    fn prepare(ground: &Ground) -> aikit_core::Result<Value> {
        with_central_root(&ground.root, || {
            let mut env = BTreeMap::new();
            env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
            env.insert(
                "CENTRAL_ROOT".to_string(),
                ground.root.display().to_string(),
            );
            let service = Service::open(ground.home.clone(), &ground.root, move |key| {
                env.get(key).cloned()
            })
            .unwrap();
            prepare_with(&service, request(), &ground.stub)
        })
    }

    fn prepared_session(ground: &Ground) -> ResourceRef {
        let prepared = prepare(ground).unwrap();
        assert_eq!(prepared["prepared"], json!(true), "{prepared}");
        ResourceRef::parse(prepared["agent_session"].as_str().unwrap()).unwrap()
    }

    fn prompt(
        ground: &Ground,
        session: &ResourceRef,
        text: &str,
    ) -> aikit_core::Result<(String, Option<Value>)> {
        with_central_root(&ground.root, || {
            prompt_with(&ground.home, session, text, &ground.stub)
        })
    }

    #[test]
    fn a_prepared_session_delivers_the_exact_accepted_self_sources_and_evidence() {
        let ground = Ground::new(true, true);
        let session = prepared_session(&ground);
        let (payload, evidence) = prompt(&ground, &session, "Do the thing").unwrap();
        let evidence = evidence.expect("delivery evidence");

        // The exact composed header, recomputed from the same accepted pins.
        let profile = CentralAgentProfileProjection::parse(&ground.review["profile"]).unwrap();
        let definition = profile.self_definition.as_ref().unwrap();
        let resolved = resolve_self_sources(&ground.root, definition).unwrap();
        let expected_header =
            compose_operative_orientation(AGENT_REF, PROFILE_REF, "r1", &resolved);
        assert!(payload.starts_with(&expected_header), "{payload}");

        // The EXACT accepted source bytes are embedded verbatim, self before
        // relational Logos, and the human task still closes the payload.
        let self_text = String::from_utf8_lossy(SELF_BYTES).into_owned();
        let logos_text = String::from_utf8_lossy(LOGOS_BYTES).into_owned();
        let self_at = payload.find(&self_text).expect("exact self source bytes");
        let logos_at = payload.find(&logos_text).expect("exact logos bytes");
        assert!(self_at < logos_at, "self source precedes its Logos");
        assert!(payload.contains("Selected reusable Agent definition"));
        assert!(payload.contains("\"Do the thing\""));

        // The evidence carries the receipt fields and the compositor identity.
        assert_eq!(evidence["delivery"], DIRECT_DELIVERY_MODE);
        assert_eq!(evidence["identity_compositor"], IDENTITY_COMPOSITOR_VERSION);
        assert_eq!(
            evidence["self_source_ref"],
            definition.source.reference.as_str()
        );
        assert_eq!(evidence["self_source_digest"], SELF_DIGEST);
        assert_eq!(
            evidence["relational_logos_ref"],
            definition.relational_logos.reference.as_str()
        );
        assert_eq!(evidence["relational_logos_digest"], LOGOS_DIGEST);
    }

    #[test]
    fn an_edited_self_source_refuses_the_turn_by_name() {
        let ground = Ground::new(true, true);
        let session = prepared_session(&ground);
        std::fs::write(ground.self_sources_path(), b"changed bytes\n").unwrap();
        let error = prompt(&ground, &session, "Do the thing").unwrap_err();
        assert_eq!(error.code(), "direct_agent.self_source_digest_mismatch");
    }

    #[test]
    fn an_unresolved_pin_refuses_preparation_and_publishes_no_binding() {
        let ground = Ground::new(true, false);
        let error = prepare(&ground).unwrap_err();
        assert_eq!(error.code(), "direct_agent.self_source_missing");
        assert_eq!(find(&ground.home, REQUEST_ID).unwrap(), Value::Null);
        let bindings = ground.home.state().join("encounter-agents");
        assert!(
            !bindings.exists() || std::fs::read_dir(&bindings).unwrap().next().is_none(),
            "no binding may be published over unresolved identity"
        );
    }

    #[test]
    fn a_profile_without_a_self_definition_keeps_the_legacy_payload_and_evidence() {
        let ground = Ground::new(false, false);
        let session = prepared_session(&ground);
        let prepared = find(&ground.home, REQUEST_ID).unwrap();
        assert_eq!(prepared["self_definition"], Value::Null);
        let (payload, evidence) = prompt(&ground, &session, "Do the thing").unwrap();
        assert!(
            !payload.starts_with("# Operative orientation of the selected Agent"),
            "no orientation section rides a profile without pins: {payload}"
        );
        assert!(payload.contains("Selected reusable Agent definition"));
        let evidence = evidence.expect("legacy evidence still present");
        assert!(evidence.get("self_source_ref").is_none());
        assert!(evidence.get("self_source_digest").is_none());
        assert!(evidence.get("relational_logos_ref").is_none());
        assert!(evidence.get("relational_logos_digest").is_none());
    }

    #[test]
    fn repeated_prompts_are_byte_identical_across_restarts() {
        let ground = Ground::new(true, true);
        let session = prepared_session(&ground);
        let (first, first_evidence) = prompt(&ground, &session, "Do the thing").unwrap();
        let (second, second_evidence) = prompt(&ground, &session, "Do the thing").unwrap();
        assert_eq!(first, second);
        assert_eq!(first_evidence, second_evidence);
    }
}
