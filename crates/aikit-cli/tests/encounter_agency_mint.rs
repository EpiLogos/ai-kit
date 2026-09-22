//! The per-project agency mint. Real ProjectCentral ground, a real OS process
//! standing in as the native owner binary (echoing the exact receipt shape the
//! real `actuation agency actualise` returns), and the real admission seam.
//! The joined acceptance against the *installed* native owner lives outside
//! this suite; here the structural law is what is pinned.
#![cfg(unix)]
use aikit_adapters::{
    agency_admission::{admit_agency, AdmittedAgency, AgencySourceBasis},
    runner::{CommandRunner, Output, SystemRunner},
};
use aikit_cli::encounter_service::{mint_per_project_agency, mint_request_document};
use aikit_core::ResourceRef;
use aikit_store::AikitHome;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}

fn template() -> Value {
    serde_json::from_str(include_str!("fixtures/mint-agency-template.json")).unwrap()
}

/// A python3 executable that answers `agency actualise <file> --json` with the
/// exact receipt the real Actuation command returns for an admitted request:
/// every request field echoed back, status actualised, no side effects. It is
/// a real OS process, not a pretend return value.
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(
            &bin,
            format!("#!/bin/sh\nexec python3 \"{}\" \"$@\"\n", script.display()),
        )
        .unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

/// A real ProjectCentral ground: the manifest is the public filesystem
/// contract the canonical project resolver reads, exactly as the native
/// `project-context` test exercises it.
struct MintWorld {
    _temp: tempfile::TempDir,
    project: PathBuf,
    home: AikitHome,
    actuation_bin: PathBuf,
}

impl MintWorld {
    fn new(project_id: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let project = root.join("Work").join("mint-fixture");
        // The `.aikit` marker is the project-root discovery marker; the
        // ProjectCentral manifest then supplies the canonical identity.
        fs::create_dir_all(project.join(".aikit")).unwrap();
        fs::create_dir_all(project.join("ProjectCentral")).unwrap();
        // A Central-shaped root carrying the owner's standing template at the
        // default location the mint resolves when no override is set.
        let template_directory = root
            .join("Work")
            .join("O-I")
            .join(".aikit")
            .join("sf6-agency");
        fs::create_dir_all(&template_directory).unwrap();
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/mint-agency-template.json"),
            template_directory.join("agency-request.json"),
        )
        .unwrap();
        let _ = fs::create_dir_all(root.join("Control"));
        fs::write(
            project.join("ProjectCentral/project.json"),
            json!({"schema":"central.project/v1","project_id":project_id,"human_source":"ProjectCentral/user","wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}}).to_string(),
        )
        .unwrap();
        let home = AikitHome::at(root.join("home"));
        Self {
            _temp: temp,
            project,
            home,
            actuation_bin: write_actuation_stub(&root),
        }
    }
}

fn binding_path(home: &AikitHome, session: &ResourceRef) -> PathBuf {
    home.state().join("encounter-agencies").join(format!(
        "{}.json",
        blake3::hash(session.as_str().as_bytes()).to_hex()
    ))
}

/// A runner that answers Central's `agent-profile.list` from a fixed table and
/// delegates every actualise call to the real stub binary, so a mint that
/// consults a declared profile still exercises the full native seam.
struct ProfileRunner {
    profiles: Value,
}

impl CommandRunner for ProfileRunner {
    fn run(&self, argv: &[String]) -> aikit_core::Result<Output> {
        if argv.iter().any(|arg| arg == "agent-profile.list") {
            return Ok(Output::success(
                json!({"ok":true,"status":"success","action":"agent-profile.list","data":{"profiles":self.profiles,"scope":"project","source_payloads_disclosed":false}}).to_string(),
            ));
        }
        let output = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .unwrap();
        Ok(Output {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[test]
fn minted_request_carries_template_authority_and_mints_fresh_refs() {
    let (request, tag) =
        mint_request_document(&template(), "agent/factory-chat", "project:factory").unwrap();
    assert_eq!(request["schema"], "actuation.agency-actualisation/v1");
    // The owner's standing authority is carried verbatim, never reinvented.
    assert_eq!(request["requester_ref"], "human:sf6-owner");
    assert_eq!(
        request["governing_binding"],
        template()["governing_binding"]
    );
    assert_eq!(request["metagency_grant"], template()["metagency_grant"]);
    assert_eq!(
        request["determination"]["bounds_refs"],
        template()["determination"]["bounds_refs"]
    );
    assert_eq!(
        request["determination"]["authority_refs"],
        template()["determination"]["authority_refs"]
    );
    assert_eq!(
        request["determination"]["delegated_autonomy"]["denied_action_refs"],
        template()["determination"]["delegated_autonomy"]["denied_action_refs"]
    );
    assert_eq!(
        request["determination"]["delegated_autonomy"]["may_determine_within_bounds"],
        template()["determination"]["delegated_autonomy"]["may_determine_within_bounds"]
    );
    assert_eq!(request["differentiated_binding"]["scope_ref"], "scope:root");
    // The chat Actions a session cannot work without are delegated.
    for action in aikit_cli::encounter_service::REQUIRED_MINTED_ACTIONS {
        assert!(
            request["determination"]["delegated_autonomy"]["allowed_action_refs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|allowed| allowed == action)
        );
    }
    // Every per-agent/project ref is freshly minted, not the template's.
    assert_ne!(request["request_ref"], template()["request_ref"]);
    assert_ne!(
        request["determination"]["determination_ref"],
        template()["determination"]["determination_ref"]
    );
    assert_ne!(
        request["determination"]["differentiated_agency_ref"],
        template()["determination"]["differentiated_agency_ref"]
    );
    assert_ne!(
        request["determination"]["world_binding_ref"],
        template()["determination"]["world_binding_ref"]
    );
    assert_ne!(
        request["determination"]["return_policy"]["return_relation_ref"],
        template()["determination"]["return_policy"]["return_relation_ref"]
    );
    assert_ne!(
        request["differentiated_binding"]["binding_ref"],
        template()["differentiated_binding"]["binding_ref"]
    );
    assert_ne!(
        request["differentiated_binding"]["agency_ref"],
        template()["differentiated_binding"]["agency_ref"]
    );
    assert_ne!(
        request["differentiated_binding"]["return_relation_ref"],
        template()["differentiated_binding"]["return_relation_ref"]
    );
    assert_ne!(
        request["differentiated_binding"]["continuity_ref"],
        template()["differentiated_binding"]["continuity_ref"]
    );
    assert_ne!(
        request["agent_identity"]["evidence_refs"],
        template()["agent_identity"]["evidence_refs"]
    );
    assert_ne!(
        request["provenance"]["source_refs"],
        template()["provenance"]["source_refs"]
    );
    // The tag ties every fresh ref to this agent+project pair.
    for (_, value) in request.as_object().unwrap() {
        let text = value.to_string();
        if text.contains("aikit-mint-") {
            assert!(text.contains(tag.as_str()), "{text} does not carry {tag}");
        }
    }
    // A delegation never claims to actualise a new identity.
    assert_eq!(request["agent_identity"]["standing"], "existing");
    assert_eq!(
        request["differentiated_binding"]["world_ref"],
        "project:factory"
    );
    assert_eq!(
        request["differentiated_binding"]["agent_ref"],
        "agent/factory-chat"
    );
    // The three native identities stay distinct, as admission requires.
    assert_ne!(
        request["differentiated_binding"]["agent_ref"],
        request["differentiated_binding"]["agency_ref"]
    );
    assert_ne!(
        request["differentiated_binding"]["agency_ref"],
        request["differentiated_binding"]["binding_ref"]
    );
}

#[test]
fn minted_refs_are_fresh_per_agent_and_per_project() {
    let (factory_a, _) =
        mint_request_document(&template(), "agent/factory-chat", "project:factory").unwrap();
    let (factory_b, _) =
        mint_request_document(&template(), "agent/other-chat", "project:factory").unwrap();
    let (central_a, _) =
        mint_request_document(&template(), "agent/factory-chat", "project:Central").unwrap();
    for (left, right) in [
        (&factory_a, &factory_b),
        (&factory_a, &central_a),
        (&factory_b, &central_a),
    ] {
        assert_ne!(left["request_ref"], right["request_ref"]);
        assert_ne!(
            left["determination"]["determination_ref"],
            right["determination"]["determination_ref"]
        );
        assert_ne!(
            left["differentiated_binding"]["agency_ref"],
            right["differentiated_binding"]["agency_ref"]
        );
        assert_ne!(
            left["differentiated_binding"]["binding_ref"],
            right["differentiated_binding"]["binding_ref"]
        );
    }
    // The same agent+project pair is deterministic: byte-identical request.
    let (again, _) =
        mint_request_document(&template(), "agent/factory-chat", "project:factory").unwrap();
    assert_eq!(
        serde_json::to_vec(&factory_a).unwrap(),
        serde_json::to_vec(&again).unwrap()
    );
    // The template's own refs never leak into a mint for another world.
    assert_eq!(
        central_a["differentiated_binding"]["world_ref"],
        "project:Central"
    );
}

#[test]
fn a_non_delegation_template_is_refused_not_silently_recast() {
    let mut not_delegation = template();
    not_delegation["determination"]["kind"] = json!("derivation");
    let error = mint_request_document(&not_delegation, "agent/x", "project:x").unwrap_err();
    assert_eq!(error.code(), "agency_mint.template_invalid");
}

#[test]
fn mint_provisions_the_session_binding_through_the_real_native_seam() {
    let world = MintWorld::new("project:mint-fixture");
    let session = r("agent-session/mint-proof");
    let output = mint_per_project_agency(
        &world.home,
        &world.project,
        &session,
        None,
        &world.actuation_bin,
        &SystemRunner::new(),
    )
    .unwrap();
    assert_eq!(output["ok"], true);
    assert_eq!(output["data"]["configured"], true);
    assert_eq!(output["data"]["standing"], "minted-per-project-agency");
    // Identity resolved through the canonical project resolution, and the
    // derived project agent is clearly attributed.
    assert_eq!(output["data"]["world_ref"], "project:mint-fixture");
    assert_eq!(output["data"]["agent_ref"], "agent/mint-fixture-chat");
    assert_eq!(
        output["data"]["agent_identity_source"],
        "derived-project-agent"
    );

    // The minted source persists, content-addressed, and matches its digest.
    let source_path = PathBuf::from(output["data"]["source_path"].as_str().unwrap());
    let bytes = fs::read(&source_path).unwrap();
    assert_eq!(format!("blake3:{}", blake3::hash(&bytes).to_hex()), {
        let binding: Value =
            serde_json::from_slice(&fs::read(binding_path(&world.home, &session)).unwrap())
                .unwrap();
        binding["agency_source"]["content_digest"]
            .as_str()
            .unwrap()
            .to_owned()
    });
    let source: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(source["schema"], "actuation.agency-actualisation/v1");
    assert_eq!(
        source["differentiated_binding"]["agent_ref"],
        "agent/mint-fixture-chat"
    );
    assert_eq!(source["requester_ref"], "human:sf6-owner");

    // The persisted binding carries the minted identities and the template's
    // human sender, and passes the same admission every later turn applies.
    let binding_bytes = fs::read(binding_path(&world.home, &session)).unwrap();
    let binding: Value = serde_json::from_slice(&binding_bytes).unwrap();
    assert_eq!(binding["revision"], output["data"]["revision"]);
    assert_eq!(binding["agent_ref"], "agent/mint-fixture-chat");
    assert_eq!(binding["world_ref"], "project:mint-fixture");
    assert_eq!(binding["active"], true);
    assert_eq!(
        binding["allowed_senders"],
        json!(["human:sf6-owner"]),
        "the template's requester is the permitted sender, never a fabricated one"
    );
    assert_eq!(binding["allowed_packet_sources"], json!([]));
    let basis: AgencySourceBasis =
        serde_json::from_value(binding["agency_source"].clone()).unwrap();
    assert_eq!(basis.path, source_path);
    assert_eq!(output["data"]["agency_source"], binding["agency_source"]);
    let admitted = admit_agency(
        &SystemRunner::new(),
        &world.actuation_bin.to_string_lossy(),
        &basis,
        &r("agent/mint-fixture-chat"),
        &r("project:mint-fixture"),
    )
    .unwrap();
    let returned_admission: AdmittedAgency =
        serde_json::from_value(output["data"]["agency_admission"].clone()).unwrap();
    assert_eq!(returned_admission, admitted);
    assert_eq!(admitted.agency_ref.as_str(), output["data"]["agency_ref"]);
    assert!(admitted.authorises(&r("action/aikit/encounter-send")));
    assert!(admitted.authorises(&r("action/aikit/model-realise")));
}

#[test]
fn reminting_the_same_session_and_project_is_a_clean_cas_bump() {
    let world = MintWorld::new("project:mint-fixture");
    let session = r("agent-session/mint-proof");
    let first = mint_per_project_agency(
        &world.home,
        &world.project,
        &session,
        None,
        &world.actuation_bin,
        &SystemRunner::new(),
    )
    .unwrap();
    let second = mint_per_project_agency(
        &world.home,
        &world.project,
        &session,
        None,
        &world.actuation_bin,
        &SystemRunner::new(),
    )
    .unwrap();
    // Same source (deterministic derivation), fresh revision.
    assert_eq!(first["data"]["source_path"], second["data"]["source_path"]);
    assert_ne!(first["data"]["revision"], second["data"]["revision"]);
    let binding: Value =
        serde_json::from_slice(&fs::read(binding_path(&world.home, &session)).unwrap()).unwrap();
    assert_eq!(binding["revision"], second["data"]["revision"]);
    assert_eq!(binding["agent_ref"], first["data"]["agent_ref"]);
}

#[test]
fn minting_a_different_agent_into_a_bound_session_is_refused() {
    let world = MintWorld::new("project:mint-fixture");
    let session = r("agent-session/mint-proof");
    mint_per_project_agency(
        &world.home,
        &world.project,
        &session,
        None,
        &world.actuation_bin,
        &SystemRunner::new(),
    )
    .unwrap();
    let error = mint_per_project_agency(
        &world.home,
        &world.project,
        &session,
        Some(r("agent/other-chat")),
        &world.actuation_bin,
        &SystemRunner::new(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "encounter.agent_identity_changed");
}

#[test]
fn a_declared_central_profile_supplies_the_agent_identity() {
    let world = MintWorld::new("project:mint-fixture");
    let session = r("agent-session/mint-proof");
    let runner = ProfileRunner {
        profiles: json!([{"profile":{"agent_ref":"agent/mint-fixture-warden","role":"product-guardian"}}]),
    };
    let output = mint_per_project_agency(
        &world.home,
        &world.project,
        &session,
        None,
        &world.actuation_bin,
        &runner,
    )
    .unwrap();
    assert_eq!(output["data"]["agent_ref"], "agent/mint-fixture-warden");
    assert_eq!(
        output["data"]["agent_identity_source"],
        "central-agent-profile"
    );
}

#[test]
fn an_ambiguous_project_scope_is_refused_rather_than_guessed() {
    let world = MintWorld::new("project:mint-fixture");
    let session = r("agent-session/mint-proof");
    let runner = ProfileRunner {
        profiles: json!([
            {"profile":{"agent_ref":"agent/mint-fixture-warden"}},
            {"profile":{"agent_ref":"agent/mint-fixture-clerk"}}
        ]),
    };
    let error = mint_per_project_agency(
        &world.home,
        &world.project,
        &session,
        None,
        &world.actuation_bin,
        &runner,
    )
    .unwrap_err();
    assert_eq!(error.code(), "agency_mint.agent_ambiguous");
    assert!(error.message().contains("agent/mint-fixture-warden"));
    assert!(error.message().contains("--agent-ref"));
}

#[test]
fn a_missing_standing_template_is_refused_with_its_path() {
    let manifest = json!({"schema":"central.project/v1","project_id":"project:mint-fixture",
        "human_source":"ProjectCentral/user",
        "wiki":{"profile":"okf-wiki/v1","source":"ProjectCentral/agents/wiki/wiki.json"}})
    .to_string();
    // No Central root (Control/ + Work/) above the project and no override:
    // the mint cannot find the owner's standing chain and says so.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let project = root.join("Work").join("mint-fixture");
    fs::create_dir_all(project.join(".aikit")).unwrap();
    fs::create_dir_all(project.join("ProjectCentral")).unwrap();
    fs::write(
        project.join("ProjectCentral/project.json"),
        manifest.clone(),
    )
    .unwrap();
    let home = AikitHome::at(root.join("home"));
    let error = mint_per_project_agency(
        &home,
        &project,
        &r("agent-session/mint-proof"),
        None,
        &write_actuation_stub(&root),
        &SystemRunner::new(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "agency_mint.template_missing");
    assert!(error.message().contains("AIKIT_AGENCY_MINT_TEMPLATE"));

    // With the root present but the template file absent, the exact default
    // path is named.
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("Control")).unwrap();
    fs::create_dir_all(
        root.join("Work")
            .join("O-I")
            .join(".aikit")
            .join("sf6-agency"),
    )
    .unwrap();
    let project = root.join("Work").join("mint-fixture");
    fs::create_dir_all(project.join(".aikit")).unwrap();
    fs::create_dir_all(project.join("ProjectCentral")).unwrap();
    fs::write(project.join("ProjectCentral/project.json"), manifest).unwrap();
    let home = AikitHome::at(root.join("home"));
    let error = mint_per_project_agency(
        &home,
        &project,
        &r("agent-session/mint-proof"),
        None,
        &write_actuation_stub(&root),
        &SystemRunner::new(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "agency_mint.template_missing");
    assert!(error.message().contains("sf6-agency/agency-request.json"));
}

#[test]
fn a_non_session_ref_is_refused_before_any_native_effect() {
    let world = MintWorld::new("project:mint-fixture");
    let error = mint_per_project_agency(
        &world.home,
        &world.project,
        &r("display-name/not-a-session"),
        None,
        &world.actuation_bin,
        &SystemRunner::new(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "agency_mint.session_invalid");
    assert!(
        !world.home.state().join("agency-mints").exists(),
        "a refused mint must not persist a minted source"
    );
}

#[test]
fn an_unbound_project_cannot_be_minted_against() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let project = root.join("loose");
    fs::create_dir_all(&project).unwrap();
    let home = AikitHome::at(root.join("home"));
    // Under a Central-shaped root so the template is found and the failure is
    // genuinely the project resolution, not the template walk.
    fs::create_dir_all(root.join("Control")).unwrap();
    fs::create_dir_all(
        root.join("Work")
            .join("O-I")
            .join(".aikit")
            .join("sf6-agency"),
    )
    .unwrap();
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mint-agency-template.json"),
        root.join("Work")
            .join("O-I")
            .join(".aikit")
            .join("sf6-agency")
            .join("agency-request.json"),
    )
    .unwrap();
    let stub = write_actuation_stub(&root);
    let error = mint_per_project_agency(
        &home,
        &project,
        &r("agent-session/mint-proof"),
        None,
        &stub,
        &SystemRunner::new(),
    )
    .unwrap_err();
    assert_eq!(error.code(), "agency_mint.project_unresolved");
}
