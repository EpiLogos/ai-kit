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
            &format!(
                "---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n"
            ),
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
        skill(&home_path, "skill/test/member-a", "member-a", "Set member that projects.");
        // member-b: in the NESTED child set, not enabled → withheld, named.
        skill(&home_path, "skill/test/member-b", "member-b", "Nested member that does not project.");
        // extra: outside the set, individually named → the exception, preserved.
        skill(&home_path, "skill/test/extra", "extra", "Individually named exception.");

        // The nested set membership: demo -> demo-inner -> member-b.
        write(
            &home_path.join("skillsets/demo-inner/members"),
            "skill/test/member-b\n",
        );
        write(
            &home_path.join("skillsets/demo/set.toml"),
            "children = [\"demo-inner\"]\n",
        );
        write(&home_path.join("skillsets/demo/members"), "skill/test/member-a\n");

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
        let load = load_registry(&home.registry("personal"), RegistrySource::new("personal"))
            .unwrap();
        for id in ["skill/test/member-a", "skill/test/extra"] {
            let capsule_id = CapsuleId::parse(id).unwrap();
            let capsule = load
                .catalog
                .get(&capsule_id)
                .unwrap_or_else(|| panic!("the seeded registry holds {id}"));
            let revision = capsule.revision.clone().expect("a loaded capsule has a revision");
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
        digests.iter().map(|digest| digest.reference.clone()).collect()
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
        let (digests_again, withheld_again, text_again) =
            material(&service, &profile).unwrap();
        assert_eq!(digests, digests_again);
        assert_eq!(withheld, withheld_again);
        assert_eq!(text, text_again);
    }

    /// An individually named Skill that does not project refuses the
    /// preparation — a named Skill is a requirement, not a request.
    #[test]
    fn an_individually_named_unavailable_skill_refuses() {
        let (_root, service, mut profile) = fixture();
        profile.skill_refs
            .push(ResourceRef::parse("skill/test/member-b").unwrap());
        let error = material(&service, &profile).unwrap_err();
        assert_eq!(error.code(), "capabilities.not_active", "{error:?}");
    }
}

