//! Source correlation fixture, distinct from the mandatory real Actuation lane.
use aikit_adapters::agency_admission::{admit_agency, AdmittedAgency, AgencySourceBasis};
use aikit_adapters::runner::ScriptedRunner;
use aikit_core::{ResourceRef, SourceRevision};
use serde_json::{json, Value};
use std::path::Path;

fn fixture(path: &Path) -> AdmittedAgency {
    fixture_at_scope(path, "scope:root")
}
fn fixture_at_scope(path: &Path, scope: &str) -> AdmittedAgency {
    let mut source: Value = serde_json::from_str(include_str!(
        "../../aikit-cli/tests/fixtures/caw-agency-request.json"
    ))
    .unwrap();
    source["differentiated_binding"]["world_ref"] = json!("central:root");
    source["differentiated_binding"]["scope_ref"] = json!(scope);
    let bytes = serde_json::to_vec(&source).unwrap();
    std::fs::write(path, &bytes).unwrap();
    let basis = AgencySourceBasis {
        source_ref: ResourceRef::parse("source:root").unwrap(),
        revision: SourceRevision::parse("rev/1").unwrap(),
        path: path.canonicalize().unwrap(),
        content_digest: format!("blake3:{}", blake3::hash(&bytes).to_hex()),
    };
    let mut receipt = source.clone();
    receipt["status"] = json!("actualised");
    receipt["bounds_refs"] = source["determination"]["bounds_refs"].clone();
    receipt["metagency"] = json!({"grant_ref":source["metagency_grant"]["grant_ref"],"authority_ref":source["metagency_grant"]["authority_ref"]});
    receipt["agent_identity"]["agent_ref"] = source["differentiated_binding"]["agent_ref"].clone();
    receipt["effects"] =
        json!({"materialisation":"not-performed","source_mutation":"not-performed"});
    admit_agency(
        &ScriptedRunner::new().on("actualise", &receipt.to_string()),
        "fixture-actuation",
        &basis,
        &ResourceRef::parse(
            source["differentiated_binding"]["agent_ref"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
        &ResourceRef::parse("central:root").unwrap(),
    )
    .unwrap()
}
#[test]
fn owner_identity_and_revision_are_retained_without_inventing_a_directory() {
    let temp = tempfile::tempdir().unwrap();
    let admitted = fixture(&temp.path().join("source.json"));
    let binding = serde_json::to_value(admitted.context_binding().unwrap()).unwrap();
    assert_eq!(binding["project"], "central:root");
    assert_eq!(binding["source"], "source:root");
    assert_eq!(binding["locator"]["kind"], "native-world");
    assert_eq!(binding["locator"]["source_revision"], "rev/1");
    assert_eq!(
        binding["locator"]["binding"],
        admitted.world_binding_ref.as_str()
    );
    assert!(binding["locator"].get("path").is_none());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}
#[test]
fn modified_native_identity_or_receipt_cannot_be_projected_as_root_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let admitted = fixture(&temp.path().join("source.json"));
    let mut changed = admitted.clone();
    changed.world_ref = ResourceRef::parse("world:other").unwrap();
    assert!(changed.context_binding().is_err());
    changed = admitted.clone();
    changed.scope_ref = ResourceRef::parse("scope:other").unwrap();
    assert!(changed.context_binding().is_err());
    changed = admitted.clone();
    changed.receipt["effects"]["source_mutation"] = json!("performed");
    assert!(changed.context_binding().is_err());
    std::fs::write(&admitted.basis.path, "changed").unwrap();
    assert_eq!(
        admitted.context_binding().unwrap_err().code(),
        "agency_admission.stale"
    );
}

#[test]
fn an_arbitrary_non_root_world_is_not_promoted_into_a_project() {
    let temp = tempfile::tempdir().unwrap();
    let admitted = fixture_at_scope(&temp.path().join("source.json"), "scope:child");
    assert_eq!(
        admitted.context_binding().unwrap_err().code(),
        "agency_admission.project_binding_required"
    );
}
