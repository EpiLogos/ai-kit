//! Controlled child replies are fixtures, not actual model/harness evidence.
//! These tests exercise the native consumer and prove that process success alone
//! cannot authorise or certify the requested relation.
use aikit_adapters::model_realisation::{
    instantiation_receipt, realise, RealisationOutcome, RealisationRequest,
};
use aikit_adapters::runner::SystemRunner;
use aikit_core::resource::{
    CredentialCondition, ModelRoute, ModelRouteKind, ProviderRef, ResourceRef, RouteAvailability,
};

fn request() -> RealisationRequest {
    RealisationRequest {
        actuation_ref: "actuation:caw".into(),
        agency_ref: "agency:caw".into(),
        world_binding_ref: "binding:caw".into(),
        agent_session_ref: Some("session:caw".into()),
        harness_ref: Some("harness/caw-fixture".into()),
        model: ResourceRef::parse("model:caw").unwrap(),
        route: ModelRoute {
            model: ResourceRef::parse("model:caw").unwrap(),
            provider: ProviderRef::parse("provider:caw").unwrap(),
            kind: ModelRouteKind::HarnessNative,
            provider_native_id: "native-caw".into(),
            endpoint: None,
            availability: RouteAvailability::Observed {
                detection_ref: "detection:caw".into(),
            },
            credential: CredentialCondition::NotRequired,
            provenance: vec!["fixture:caw".into()],
        },
        evidence_refs: vec!["fixture:caw".into()],
    }
}

#[cfg(unix)]
fn fixture_owner(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fixture-owner");
    std::fs::write(&path, r##"#!/usr/bin/env python3
import json, os, sys
receipt = json.load(open(sys.argv[-2]))
receipt['detection_ref'] = 'detection:caw'
receipt['harness_receipts'] = {'executable': '/fixture/caw'}
mode = os.environ.get('CAW_FIXTURE_MODE', 'valid')
if mode == 'denied':
    print(json.dumps({'ok': False, 'error': {'code': 'fixture.denied'}})); sys.exit(0)
if mode == 'exit-denied':
    print('controlled refusal', file=sys.stderr); sys.exit(2)
if mode == 'null': receipt = None
elif mode == 'empty': receipt = {}
elif mode == 'wrapped': receipt = {'receipt': receipt}
elif mode == 'schema': receipt['schema'] = 'actuation.instantiation/v99'
elif mode in ('actuation_ref', 'agency_ref', 'world_binding_ref', 'agent_session_ref', 'harness_ref'):
    receipt[mode] = 'other:identity'
elif mode == 'model': receipt['model_relation']['model_ref'] = 'model:other'
elif mode == 'variant': receipt['model_relation']['variant_ref'] = 'other-native'
elif mode == 'provider': receipt['model_relation']['engine']['provider_ref'] = 'provider:other'
elif mode == 'surface': receipt['model_relation']['inference_surface']['contract_ref'] = 'contract:other'
elif mode == 'access': receipt['access_profile']['control']['allowed'] = ['action:ungranted']
elif mode == 'missing-evidence': del receipt['harness_receipts']
elif mode == 'empty-detection': receipt['detection_ref'] = ''
print(json.dumps(receipt))
"##).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
#[cfg(unix)]
fn zero_exit_with_wrong_or_uncorrelated_receipt_never_certifies_instantiation() {
    let dir = tempfile::tempdir().unwrap();
    let owner = fixture_owner(dir.path());
    for mode in [
        "null",
        "empty",
        "wrapped",
        "denied",
        "schema",
        "actuation_ref",
        "agency_ref",
        "world_binding_ref",
        "agent_session_ref",
        "harness_ref",
        "model",
        "variant",
        "provider",
        "surface",
        "access",
        "missing-evidence",
        "empty-detection",
    ] {
        let result = realise(
            &SystemRunner::new().with_env("CAW_FIXTURE_MODE", mode),
            owner.to_str().unwrap(),
            &request(),
        );
        assert!(
            !matches!(result, RealisationOutcome::Instantiated { .. }),
            "{mode} was accepted: {result:?}"
        );
    }
}

#[test]
#[cfg(unix)]
fn a_correlated_controlled_receipt_and_a_process_refusal_remain_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let owner = fixture_owner(dir.path());
    let valid = realise(&SystemRunner::new(), owner.to_str().unwrap(), &request());
    let RealisationOutcome::Instantiated {
        receipt,
        detection_ref,
    } = valid
    else {
        panic!("{valid:?}")
    };
    assert_eq!(receipt["agency_ref"], "agency:caw");
    assert_eq!(detection_ref.as_deref(), Some("detection:caw"));
    let denied = realise(
        &SystemRunner::new().with_env("CAW_FIXTURE_MODE", "exit-denied"),
        owner.to_str().unwrap(),
        &request(),
    );
    assert!(matches!(denied, RealisationOutcome::Refused { .. }));
}

#[test]
fn a_missing_credential_or_different_route_model_cannot_be_recorded() {
    let mut input = request();
    input.route.credential = CredentialCondition::Required {
        hint: "fixture key absent".into(),
    };
    assert!(
        instantiation_receipt(&input).is_err(),
        "observed is not presently usable"
    );
    input.route.credential = CredentialCondition::NotRequired;
    input.route.model = ResourceRef::parse("model:other").unwrap();
    assert!(
        instantiation_receipt(&input).is_err(),
        "route must retain the selected Model"
    );
    input.route.model = input.model.clone();
    input.world_binding_ref = input.agency_ref.clone();
    assert!(
        instantiation_receipt(&input).is_err(),
        "identity roles cannot collapse"
    );
}
