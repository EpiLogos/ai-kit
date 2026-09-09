use std::path::{Path, PathBuf};

use aikit_adapters::{
    factory_developmental::{
        read_factory_developmental, start_factory_work, FactoryDevelopmentalBinding,
        FACTORY_COMMISSION_REQUEST_SCHEMA_SHA256, FACTORY_COMMISSION_SCHEMA_SHA256,
        FACTORY_DEVELOPMENTAL_CONTRACT_OWNER_REVISION, FACTORY_DEVELOPMENTAL_SCHEMA_SHA256,
    },
    runner::{CommandRunner, SystemRunner},
};
use serde_json::Value;

fn owner_state(executable: &Path, state: &Path) -> Value {
    let argv = vec![
        executable.display().to_string(),
        "conformance".into(),
        "developmental-state".into(),
        state.display().to_string(),
        "--json".into(),
    ];
    let output = SystemRunner::new()
        .run(&argv)
        .unwrap()
        .require(&argv, "test.factory_conformance_state_failed")
        .unwrap();
    serde_json::from_str(&output.stdout).unwrap()
}

#[test]
#[ignore = "run by the mandatory Factory owner conformance CI job"]
fn consumes_the_real_factory_cli_and_owner_generated_state() {
    let executable = PathBuf::from(
        std::env::var_os("AIKIT_FACTORY_REAL_BIN").expect("AIKIT_FACTORY_REAL_BIN is required"),
    );
    let directory = tempfile::tempdir().unwrap();
    let state = std::env::var_os("AIKIT_FACTORY_TEST_STATE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.path().join("developmental-state.json"));
    let manifest = owner_state(&executable, &state);
    if let Some(path) = std::env::var_os("AIKIT_FACTORY_TEST_MANIFEST_OUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    }
    assert_eq!(
        manifest["contract"],
        "factory.developmental-conformance-manifest/v1"
    );
    let project_ref = manifest["projectRef"].as_str().unwrap();
    let observation = read_factory_developmental(
        &SystemRunner::new(),
        &FactoryDevelopmentalBinding::new(executable, state, project_ref).unwrap(),
    )
    .unwrap();
    for contract in [
        "factory.project-reading/v1",
        "factory.journey-reading/v1",
        "factory.run-reading/v1",
        "factory.routine-continuation-reading/v1",
        "factory.workflow-unit-reading/v1",
        "factory.execution-telemetry-reading/v1",
    ] {
        assert!(
            observation
                .readings
                .iter()
                .any(|reading| reading.contract == contract),
            "missing {contract}"
        );
    }
    let continuation = observation
        .readings
        .iter()
        .find(|reading| reading.contract == "factory.routine-continuation-reading/v1")
        .unwrap();
    assert_eq!(
        continuation.value["continuation"]["invocationEvidence"]["proof_standing"],
        "current-on-supplied-basis"
    );
    let telemetry = observation
        .readings
        .iter()
        .find(|reading| reading.contract == "factory.execution-telemetry-reading/v1")
        .unwrap();
    assert_eq!(telemetry.value["modelUsage"]["availability"], "unavailable");
    assert_eq!(
        telemetry.value["materialUsage"]["availability"],
        "unavailable"
    );
    assert!(observation.resources.iter().all(|record| record
        .descriptor
        .annotations
        .get("factory.contract-owner-revision")
        .map(String::as_str)
        == Some(FACTORY_DEVELOPMENTAL_CONTRACT_OWNER_REVISION)));
    assert!(observation.resources.iter().all(|record| record
        .descriptor
        .annotations
        .get("factory.contract-schema-sha256")
        .map(String::as_str)
        == Some(FACTORY_DEVELOPMENTAL_SCHEMA_SHA256)));
}

#[test]
#[ignore = "run by the mandatory Factory owner conformance CI job"]
fn starts_factory_work_through_the_real_owner_cli_and_reads_back_the_commission() {
    let executable = PathBuf::from(
        std::env::var_os("AIKIT_FACTORY_REAL_BIN").expect("AIKIT_FACTORY_REAL_BIN is required"),
    );
    let request = PathBuf::from(
        std::env::var_os("AIKIT_FACTORY_COMMISSION_REQUEST")
            .expect("AIKIT_FACTORY_COMMISSION_REQUEST is required"),
    );
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("commissioned-state.json");
    let started = start_factory_work(&SystemRunner::new(), executable, &state, request).unwrap();

    assert_eq!(started.receipt["contract"], "factory.commission-receipt/v1");
    assert_eq!(started.receipt["status"], "applied");
    assert_eq!(
        started.receipt["commission"]["request"]["centralComposition"]["authorityStanding"],
        "membership-non-authoritative"
    );
    assert_eq!(
        started.receipt["commission"]["request"]["rootAct"]["standing"],
        "commissioned-not-executed"
    );
    let request_ref = started.receipt["commission"]["request"]["requestRef"]
        .as_str()
        .unwrap();
    assert!(started.observation.readings.iter().any(|reading| {
        reading.contract == "factory.commission-reading/v1"
            && reading.subject_ref == request_ref
            && reading.value["commission"] == started.receipt["commission"]
    }));
    assert!(started.observation.resources.iter().any(|resource| resource
        .descriptor
        .annotations
        .contains_key(&format!("factory.commission.{request_ref}"))));
    assert!(state.exists());
    assert_eq!(
        FACTORY_DEVELOPMENTAL_CONTRACT_OWNER_REVISION,
        "12a721dbbb51e3c70d52ef00220efa859ef930fd"
    );
    assert_eq!(
        FACTORY_DEVELOPMENTAL_SCHEMA_SHA256,
        "6d29a65744f70af16b5a348ebbd0a803ddd0b295c7bda7587316c9e8a0d0c0ec"
    );
    assert_eq!(
        FACTORY_COMMISSION_REQUEST_SCHEMA_SHA256,
        "78dd34ae441ab585c82fcc1f30614ca4d116b347af896ba4ab2404566a530c69"
    );
    assert_eq!(
        FACTORY_COMMISSION_SCHEMA_SHA256,
        "51c45a601685dbf24ebb766d9cc059f9b81a7258f27819f61b69c450cb7aa214"
    );
}
