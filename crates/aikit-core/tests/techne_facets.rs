//! `aikit.techne-facet/v1` declared facets over the open extension maps:
//! temporal, spatial and exact source selectors.
//!
//! Canonical semantic authority: Quaternal-Logic `ql.techne/v1`
//! (schemas/techne/ql-techne-reading-v1.schema.json, wayfinder §2). The laws
//! under test here: absence of facets is data, not an error; malformed
//! *declared* facet data is a namespaced error while everything else on the
//! object is preserved; occurrence/receipt/validity stay distinct; native refs
//! stay opaque; an object without facets round-trips byte-equivalent.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use aikit_core::knowledge_facets::{
    parse_facets_from_extensions, parse_source_selector, write_facets_to_extensions,
    write_source_selector, PlaceFacet, PlacePrecision, SourceSelector, TechneFacets, TemporalFacet,
    TemporalInterval, TemporalKind, TemporalPrecision, TECHNE_FACET_EXTENSION,
};
use aikit_core::knowledge_wiki::{WikiObject, WikiProvenanceRef};
use aikit_core::knowledge_wiki_shape_v2::{wiki_node_stance, QL_NODE_STANCE_EXTENSION};
use aikit_core::SourceRef;

fn extensions_from(value: Value) -> BTreeMap<String, Value> {
    let mut map = BTreeMap::new();
    let object = value
        .as_object()
        .expect("test extensions are a JSON object");
    for (key, value) in object {
        map.insert(key.clone(), value.clone());
    }
    map
}

#[test]
fn an_object_without_facets_round_trips_byte_equivalent_through_a_write_that_writes_nothing() {
    let original = json!({
        "producer_extension": {"unknown": "preserve me"},
        "aikit.ql-stance/v1": {"stance": "0/1"}
    });
    let extensions = extensions_from(original.clone());

    // Absence is data: parsing an extension map with no facet key succeeds
    // with an empty reading and — because a default reading writes nothing —
    // the map that comes back must be byte-equivalent to the original.
    let facets = parse_facets_from_extensions(&extensions).unwrap();
    assert!(facets.temporal.is_empty());
    assert!(facets.spatial.is_empty());

    let mut written = extensions.clone();
    write_facets_to_extensions(&mut written, &facets).unwrap();
    assert_eq!(
        serde_json::to_value(written).unwrap(),
        original,
        "a write that carries no facets must not touch the extension map"
    );
}

#[test]
fn temporal_and_spatial_facets_round_trip_through_the_declared_extension() {
    let facets_json = json!({
        "contract": "aikit.techne-facet/v1",
        "temporal": [
            {"kind": "occurrence", "instant": "2026-09-15T14:59:00Z", "precision": "minute",
             "session_ref": "zcode-session-2026-09-16-l5-techne-execution",
             "uncertainty": "author-declared recorded_at"},
            {"kind": "day", "day_ref": "central:day:control:root:2026-09-15",
             "timezone_policy_ref": "central:source:control:root:Control/user/civil-time-policy.json"}
        ],
        "spatial": [
            {"place_ref": "place:fixture:royal-observatory-greenwich",
             "identity": {"names": [{"name": "Royal Observatory, Greenwich", "valid_from": "1675"}]},
             "geometry": {"type": "point", "coordinates": [0.0015, 51.4769]},
             "precision": "approximate",
             "hierarchy": [{"place_ref": "place:fixture:greenwich", "relation": "within"}]}
        ]
    });
    let mut extensions = extensions_from(json!({"unrelated": true}));
    extensions.insert(TECHNE_FACET_EXTENSION.to_owned(), facets_json.clone());

    let facets = parse_facets_from_extensions(&extensions).unwrap();
    assert_eq!(facets.temporal.len(), 2);
    assert_eq!(facets.temporal[0].kind, TemporalKind::Occurrence);
    assert_eq!(facets.temporal[1].kind, TemporalKind::Day);
    assert_eq!(
        facets.temporal[1].day_ref.as_deref(),
        Some("central:day:control:root:2026-09-15")
    );
    assert_eq!(facets.spatial.len(), 1);
    assert_eq!(facets.spatial[0].precision, PlacePrecision::Approximate);
    assert_eq!(facets.spatial[0].hierarchy.len(), 1);

    let mut written = BTreeMap::new();
    write_facets_to_extensions(&mut written, &facets).unwrap();
    assert_eq!(
        written.get(TECHNE_FACET_EXTENSION).unwrap(),
        &facets_json,
        "round trip must reproduce the declared payload verbatim"
    );
}

#[test]
fn a_temporal_facet_without_any_carrier_is_a_named_error_not_a_silently_kept_claim() {
    let mut extensions = extensions_from(json!({"unrelated": "kept"}));
    extensions.insert(
        TECHNE_FACET_EXTENSION.to_owned(),
        json!({
            "contract": "aikit.techne-facet/v1",
            "temporal": [{"kind": "occurrence", "precision": "minute"}]
        }),
    );

    let error = parse_facets_from_extensions(&extensions).unwrap_err();
    assert_eq!(error.code(), "knowledge.facet_invalid_temporal");
    // The rest of the map is untouched: the caller keeps the object it came from.
    assert_eq!(
        extensions.get("unrelated").and_then(Value::as_str),
        Some("kept")
    );
}

#[test]
fn an_unknown_facet_contract_names_the_version_found() {
    let mut extensions = BTreeMap::new();
    extensions.insert(
        TECHNE_FACET_EXTENSION.to_owned(),
        json!({"contract": "aikit.techne-facet/v2", "temporal": []}),
    );

    let error = parse_facets_from_extensions(&extensions).unwrap_err();
    assert_eq!(error.code(), "knowledge.facet_contract_unsupported");
    assert!(
        error.message().contains("aikit.techne-facet/v2"),
        "the error must name the version it found, got: {}",
        error.message()
    );
}

#[test]
fn a_wiki_node_carrying_ql_stance_and_techne_facets_preserves_both() {
    let node_json = json!({
        "profile": "okf-wiki/v1", "object": "node",
        "ref": "example:knowledge:techne-node", "revision": 1,
        "provenance": [], "type": "Concept",
        "aikit.ql-stance/v1": {"stance": "0/1"},
        "aikit.techne-facet/v1": {
            "contract": "aikit.techne-facet/v1",
            "temporal": [{"kind": "receipt", "instant": "2026-09-16T11:20:00Z",
                          "precision": "minute",
                          "source_ref": "central:source:control:root:.central/source-change-horizon.json",
                          "uncertainty": "reconciler-declared observed_at"}]
        }
    });

    let object = WikiObject::parse(&node_json).unwrap();
    object.validate().unwrap();
    let WikiObject::Node(node) = object else {
        panic!("expected node");
    };
    assert_eq!(
        wiki_node_stance(&node)
            .unwrap()
            .map(|stance| stance.as_str()),
        Some("0/1")
    );
    let facets = parse_facets_from_extensions(&node.extensions).unwrap();
    assert_eq!(facets.temporal[0].kind, TemporalKind::Receipt);

    let round_tripped: Value = serde_json::to_value(&node).unwrap();
    assert!(round_tripped.get(QL_NODE_STANCE_EXTENSION).is_some());
    assert!(round_tripped.get(TECHNE_FACET_EXTENSION).is_some());
    assert_eq!(
        round_tripped[TECHNE_FACET_EXTENSION]["temporal"][0]["kind"],
        json!("receipt")
    );
}

#[test]
fn a_source_selector_rides_provenance_extensions_and_round_trips() {
    let mut provenance = WikiProvenanceRef {
        source_ref: SourceRef::parse("central:source:control:root:Control/user/placement.json")
            .unwrap(),
        source_revision: None,
        producer_ref: None,
        generation_ref: None,
        extensions: BTreeMap::new(),
    };

    write_source_selector(
        &mut provenance.extensions,
        &SourceSelector::TextSpan {
            start: 0,
            end: 741,
            anchor_ref: None,
        },
    )
    .unwrap();

    let written = serde_json::to_value(&provenance).unwrap();
    assert_eq!(
        written[TECHNE_FACET_EXTENSION],
        json!({"contract": "aikit.techne-facet/v1",
               "selector": {"unit": "text_span", "start": 0, "end": 741}}),
        "absent optional fields stay absent on the wire"
    );

    let read_back = parse_source_selector(&provenance.extensions)
        .unwrap()
        .expect("selector declared");
    assert_eq!(
        read_back,
        SourceSelector::TextSpan {
            start: 0,
            end: 741,
            anchor_ref: None
        }
    );

    // A selector on the same provenance entry as an existing extension map key
    // must not disturb it.
    let mut with_company = BTreeMap::new();
    with_company.insert("other-declared-extension".to_owned(), json!({"v": 1}));
    with_company.extend(provenance.extensions.clone());
    assert!(with_company.contains_key("other-declared-extension"));
    assert_eq!(
        parse_source_selector(&with_company).unwrap().unwrap(),
        read_back
    );
}

#[test]
fn a_selector_without_a_timestamp_or_unit_is_rejected() {
    let mut extensions = BTreeMap::new();
    extensions.insert(
        TECHNE_FACET_EXTENSION.to_owned(),
        json!({"contract": "aikit.techne-facet/v1",
               "selector": {"unit": "timestamp_range", "from": "not-a-timestamp", "to": "2026-09-16T11:20:00Z"}}),
    );
    let error = parse_source_selector(&extensions).unwrap_err();
    assert_eq!(error.code(), "knowledge.facet_invalid_selector");

    let mut ununit = BTreeMap::new();
    ununit.insert(
        TECHNE_FACET_EXTENSION.to_owned(),
        json!({"contract": "aikit.techne-facet/v1",
               "selector": {"unit": "glyph_range", "start": 1, "end": 2}}),
    );
    let error = parse_source_selector(&ununit).unwrap_err();
    assert_eq!(error.code(), "knowledge.facet_invalid_selector");
}

#[test]
fn every_temporal_kind_and_precision_and_place_precision_parses() {
    let kinds = [
        ("occurrence", TemporalKind::Occurrence),
        ("receipt", TemporalKind::Receipt),
        ("valid", TemporalKind::Valid),
        ("source-created", TemporalKind::SourceCreated),
        ("source-modified", TemporalKind::SourceModified),
        ("day", TemporalKind::Day),
        ("now", TemporalKind::Now),
        ("session", TemporalKind::Session),
        ("run", TemporalKind::Run),
        ("generation", TemporalKind::Generation),
        ("presentation", TemporalKind::Presentation),
    ];
    for (raw, kind) in kinds {
        let mut extensions = BTreeMap::new();
        extensions.insert(
            TECHNE_FACET_EXTENSION.to_owned(),
            json!({"contract": "aikit.techne-facet/v1",
                   "temporal": [{"kind": raw, "instant": "2026-09-15T00:00:00Z"}]}),
        );
        assert_eq!(
            parse_facets_from_extensions(&extensions).unwrap().temporal[0].kind,
            kind,
            "{raw} must parse"
        );
    }

    let mut place = BTreeMap::new();
    place.insert(
        TECHNE_FACET_EXTENSION.to_owned(),
        json!({"contract": "aikit.techne-facet/v1",
               "spatial": [{"place_ref": "place:x", "precision": "unlocated"}]}),
    );
    assert_eq!(
        parse_facets_from_extensions(&place).unwrap().spatial[0].precision,
        PlacePrecision::Unlocated
    );

    // An interval carries its own per-bound precision.
    let mut interval = BTreeMap::new();
    interval.insert(
        TECHNE_FACET_EXTENSION.to_owned(),
        json!({"contract": "aikit.techne-facet/v1",
               "temporal": [{"kind": "run",
                             "interval": {"from": "2026-09-15T09:10:00Z", "to": "2026-09-15T16:40:00Z",
                                          "from_precision": "minute", "to_precision": "minute"},
                             "uncertainty": "Factory orders by revision, not wall-clock"}]}),
    );
    let facets = parse_facets_from_extensions(&interval).unwrap();
    assert_eq!(
        facets.temporal[0].interval,
        Some(TemporalInterval {
            from: Some("2026-09-15T09:10:00Z".to_owned()),
            to: Some("2026-09-15T16:40:00Z".to_owned()),
            from_precision: Some(TemporalPrecision::Minute),
            to_precision: Some(TemporalPrecision::Minute),
        })
    );
}

#[test]
fn a_facet_constructed_in_rust_validates_before_it_writes() {
    // An empty place_ref is an empty opaque ref — the schema floor is minLength 1.
    let facets = TechneFacets {
        temporal: vec![TemporalFacet {
            kind: TemporalKind::Valid,
            interval: Some(TemporalInterval {
                from: Some("1675-01-01T00:00:00Z".to_owned()),
                to: None,
                from_precision: Some(TemporalPrecision::Year),
                to_precision: None,
            }),
            ..TemporalFacet::new(TemporalKind::Valid)
        }],
        spatial: vec![PlaceFacet {
            place_ref: String::new(),
            ..PlaceFacet::new("place:x", PlacePrecision::Region)
        }],
    };

    let mut extensions = BTreeMap::new();
    let error = write_facets_to_extensions(&mut extensions, &facets).unwrap_err();
    assert_eq!(error.code(), "knowledge.facet_invalid_place");
    assert!(
        !extensions.contains_key(TECHNE_FACET_EXTENSION),
        "a failed write must not leave a partial declaration behind"
    );
}
