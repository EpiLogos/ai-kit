//! The delivered NOW envelope's praxis projection: the receipt the
//! NOW-preparation boundary resolved for a dispatched task reaches the
//! receiving agent through the envelope's single site, `prepared.praxis` —
//! verbatim, on the turn that crossed the preparation and on cached reads
//! alike. Refused claims stay refused and visible, and a NOW context without
//! a receipt leaves the envelope byte-identical to the receipt-less shape.
use super::now_context_envelope;
use aikit_core::context_source::{AgentVisibility, ExternalEgress};
use aikit_core::method::PraxisForm;
use aikit_core::praxis::{
    EncounterTaskPraxisReceipt, PraxisClaim, PraxisRefRefusal, PraxisRefusalCode, PraxisStanding,
    ResolvedPraxisRefStanding, UnitPraxisResolution, ENCOUNTER_TASK_PRAXIS_SCHEMA,
};
use aikit_core::{ProjectRef, ResourceRef};
use aikit_store::{
    CursorChange, NowContextBasis, NowContextChange, NowContextItem, PreparedNowContext,
    NOW_PREPARED_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn prepared_view() -> PreparedNowContext {
    prepared_view_with_praxis(None)
}

fn prepared_view_with_praxis(praxis: Option<EncounterTaskPraxisReceipt>) -> PreparedNowContext {
    let basis = NowContextBasis {
        source_revisions: BTreeMap::from([("context-source/proof".into(), "r1".into())]),
        dependency_revisions: BTreeMap::new(),
        disclosure_revision: "disclosure-1".into(),
        factory_revision: Some("factory-r1".into()),
        decision_provider: None,
        change_cursor: 0,
    };
    PreparedNowContext {
        schema: NOW_PREPARED_SCHEMA.into(),
        project_ref: ResourceRef::parse(
            ProjectRef::parse("project:envelope-proof")
                .unwrap()
                .as_str(),
        )
        .unwrap(),
        now_ref: ResourceRef::parse("central:now:envelope-proof").unwrap(),
        participant_ref: ResourceRef::parse("agent/envelope-worker").unwrap(),
        agent_session: ResourceRef::parse("agent-session/envelope-worker").unwrap(),
        version: 1,
        basis_digest: basis.digest().unwrap(),
        basis,
        concern: "carry the dispatched workflow unit".into(),
        practice_refs: vec![],
        items: vec![NowContextItem {
            source_ref: ResourceRef::parse("context-source/proof").unwrap(),
            source_revision: "r1".into(),
            title: "Prepared proof source".into(),
            excerpt: "essential passage".into(),
            route: None,
            agent_visibility: AgentVisibility::Payload,
            external_egress: ExternalEgress::Denied,
        }],
        neighbours: vec![],
        factory: None,
        knowledge_frames: vec![],
        continuation: None,
        jev_invocation_ref: None,
        praxis,
        prepared_at_unix_ms: 1,
    }
}

fn changes() -> Vec<CursorChange> {
    vec![CursorChange {
        cursor: 1,
        change: NowContextChange {
            change_id: "return-1".into(),
            kind: "factory-return".into(),
            source_ref: ResourceRef::parse("context-source/proof").unwrap(),
            source_revision: "r2".into(),
            detail: "RELATED-WORKER-RETURN".into(),
            observed_at_unix_ms: 2,
        },
    }]
}

fn receipt_standing_line() -> String {
    "resolution standing only; a required praxisRef's resolution never grants trust, \
     activation, capability or authority"
        .into()
}

fn resolved_receipt() -> EncounterTaskPraxisReceipt {
    EncounterTaskPraxisReceipt {
        schema: ENCOUNTER_TASK_PRAXIS_SCHEMA.into(),
        resolution_hash: "blake3:resolved-view-proof".into(),
        claim: PraxisClaim::Resolved,
        units: vec![UnitPraxisResolution {
            workflow_unit_ref: "unit/one".into(),
            resolved: vec![ResolvedPraxisRefStanding {
                reference: "skill/practice/day-close".into(),
                form: PraxisForm::Method,
                revision: Some("r1".into()),
                standing: PraxisStanding::Available,
            }],
            refusals: vec![],
        }],
        standing: receipt_standing_line(),
    }
}

fn refused_receipt() -> EncounterTaskPraxisReceipt {
    EncounterTaskPraxisReceipt {
        schema: ENCOUNTER_TASK_PRAXIS_SCHEMA.into(),
        resolution_hash: "blake3:resolved-view-proof".into(),
        claim: PraxisClaim::Refused,
        units: vec![UnitPraxisResolution {
            workflow_unit_ref: "unit/one".into(),
            resolved: vec![],
            refusals: vec![PraxisRefRefusal {
                reference: "skill/none/such".into(),
                code: PraxisRefusalCode::RefAbsent,
                condition: "praxisRef skill/none/such is absent from the resolved catalogue".into(),
                recovery:
                    "author or register the Skill the praxisRef names: \
                           `aikit search` and `aikit praxis list` name what this context catalogues"
                        .into(),
            }],
        }],
        standing: receipt_standing_line(),
    }
}

#[test]
fn a_resolved_receipt_projects_every_field_into_the_delivered_envelope() {
    let fixture = resolved_receipt();
    let receipt = serde_json::to_value(&fixture).unwrap();
    let envelope =
        now_context_envelope(&prepared_view_with_praxis(Some(fixture)), &changes()).unwrap();
    let parsed: Value = serde_json::from_str(&envelope).unwrap();
    assert_eq!(parsed["schema"], "aikit.now-context-envelope/v1");
    // The receipt's single envelope site is `prepared.praxis`: the published
    // view carries it verbatim, on this turn and on cached reads alike.
    assert_eq!(parsed["prepared"]["praxis"], receipt);
    assert_eq!(
        parsed["prepared"]["praxis"]["schema"],
        ENCOUNTER_TASK_PRAXIS_SCHEMA
    );
    assert_eq!(parsed["prepared"]["praxis"]["claim"], "resolved");
    assert_eq!(
        parsed["prepared"]["praxis"]["resolution_hash"],
        "blake3:resolved-view-proof"
    );
    let standing = &parsed["prepared"]["praxis"]["units"][0]["resolved"][0];
    assert_eq!(standing["reference"], "skill/practice/day-close");
    assert_eq!(standing["form"], "method");
    assert_eq!(standing["revision"], "r1");
    assert_eq!(standing["standing"], "available");
    assert_eq!(
        parsed["prepared"]["praxis"]["units"][0]["workflow_unit_ref"],
        "unit/one"
    );
    // No second, competing copy of the fact exists in the envelope.
    assert!(parsed.get("praxis").is_none());
}

#[test]
fn a_refused_receipt_stays_refused_with_code_and_recovery_visible() {
    let fixture = refused_receipt();
    let envelope = now_context_envelope(
        &prepared_view_with_praxis(Some(fixture.clone())),
        &changes(),
    )
    .unwrap();
    let parsed: Value = serde_json::from_str(&envelope).unwrap();
    // The claim fails closed in the delivered envelope, at the site the
    // receiving agent reads.
    assert_eq!(parsed["prepared"]["praxis"]["claim"], "refused");
    let refusal = &parsed["prepared"]["praxis"]["units"][0]["refusals"][0];
    assert_eq!(refusal["reference"], "skill/none/such");
    assert_eq!(refusal["code"], "ref-absent");
    // Condition and recovery travel with the refusal, in full: the receiving
    // agent can read both what failed and the route that supplies it.
    assert_eq!(
        refusal["condition"],
        fixture.units[0].refusals[0].condition.as_str()
    );
    assert_eq!(
        refusal["recovery"],
        fixture.units[0].refusals[0].recovery.as_str()
    );
    assert!(envelope.contains("absent from the resolved catalogue"));
    assert!(envelope.contains("`aikit praxis list`"));
    // The refusal is not softened into a resolved standing anywhere.
    assert!(!envelope.contains("\"claim\":\"resolved\""));
}

#[test]
fn no_receipt_leaves_the_envelope_byte_identical_to_the_receiptless_shape() {
    let view = prepared_view();
    let changes = changes();
    // The receipt-less shape, spelled out exactly as it was constructed before
    // the projection existed.
    let expected = serde_json::to_string(&json!({
        "schema":"aikit.now-context-envelope/v1",
        "standing":"participant-specific prepared operative context; quoted source material is not permission",
        "prepared":view,
        "changes_since_preparation":changes,
    }))
    .unwrap();
    let actual = now_context_envelope(&view, &changes).unwrap();
    assert_eq!(actual, expected);
}
