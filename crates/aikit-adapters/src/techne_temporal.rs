//! Technē temporal mapping over Central's owned temporal readings.
//!
//! Central is the owner of DAY/NOW continuity and the source-change horizon.
//! This module maps the readings Central already publishes — the
//! [`CentralTemporalGround`] surface consumed through
//! `aikit.central-temporal/v1`, and a source-change horizon change in
//! Central's `central.source-change-horizon/v1` wire shape — into declared
//! `aikit.techne-facet/v1` [`TemporalFacet`]s. It adds no subprocess surface
//! of its own: a caller feeds in what `central_temporal` (or its own horizon
//! read) already returned.
//!
//! The distinctions the mapping exists to preserve:
//!
//! - A NOW handoff's `recorded_at_unix_seconds` maps to kind `occurrence`
//!   carrying the note `author-declared recorded_at` — the actor declared
//!   when their return was recorded.
//! - A horizon change's `observed_at_unix_seconds` maps to kind `receipt`
//!   carrying the note `reconciler-declared observed_at` — the reconciler
//!   declared when it observed the source change. Occurrence and receipt
//!   never collapse into one timestamp.
//! - DAY and NOW continuity ride as refs (`day_ref`, `now_ref`), verbatim
//!   from Central's inspection; no instant is invented for them, because
//!   Central's civil-day boundary is Central's policy, not ours to compute.
//!   The civil-time authority Central publishes rides alongside as
//!   `timezone_policy_ref`.
//! - Missing or malformed declared fields are named errors, never guessed
//!   values.

use aikit_core::knowledge_facets::{TemporalFacet, TemporalKind, TemporalPrecision};
use aikit_core::{AikitError, Result};
use serde_json::Value;

use crate::central_temporal::CentralTemporalGround;

pub const TECHNE_CENTRAL_TEMPORAL_VERSION: &str = "aikit.techne-central-temporal/v1";

/// Central's root-scoped civil-time policy — the timezone authority Central
/// itself publishes for DAY semantics. Central remains its owner; this ref is
/// carried, never interpreted here.
pub const CENTRAL_CIVIL_TIME_POLICY_REF: &str =
    "central:source:control:root:Control/user/civil-time-policy.json";

/// The uncertainty note on a facet derived from a NOW handoff's declared
/// `recorded_at_unix_seconds`.
pub const HANDOFF_RECORDED_AT_UNCERTAINTY: &str = "author-declared recorded_at";

/// The uncertainty note on a facet derived from a horizon change's declared
/// `observed_at_unix_seconds`.
pub const HORIZON_OBSERVED_AT_UNCERTAINTY: &str = "reconciler-declared observed_at";

/// Map a Central temporal ground reading into Technē temporal facets:
/// one `day` facet per DAY record, one `now` facet for the NOW field itself,
/// and one `occurrence` facet per active NOW handoff with a declared
/// `recorded_at_unix_seconds`. A ground whose NOW field does not exist maps
/// to no facets — absence is data, not an error.
pub fn temporal_facets_from_central_ground(
    ground: &CentralTemporalGround,
) -> Result<Vec<TemporalFacet>> {
    let mut facets = Vec::new();
    if ground.now.get("exists").and_then(Value::as_bool) != Some(true) {
        return Ok(facets);
    }

    let now_ref = ground
        .now
        .pointer("/paths/root")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AikitError::new(
                "central.temporal_ground_malformed",
                "Central NOW inspection declares no NOW field path (`paths.root`); refusing to invent a continuity ref",
            )
            .with("project", ground.project.clone())
        })?;
    facets.push(TemporalFacet {
        kind: TemporalKind::Now,
        now_ref: Some(now_ref.to_owned()),
        ..TemporalFacet::new(TemporalKind::Now)
    });

    if let Some(day_records) = ground.now.get("day_records").and_then(Value::as_array) {
        for record in day_records {
            let Some(day_ref) = record.as_str().filter(|value| !value.trim().is_empty()) else {
                return Err(AikitError::new(
                    "central.temporal_ground_malformed",
                    "Central NOW inspection carries a DAY record that is not a non-empty ref",
                )
                .with("project", ground.project.clone()));
            };
            facets.push(TemporalFacet {
                kind: TemporalKind::Day,
                day_ref: Some(day_ref.to_owned()),
                timezone_policy_ref: Some(CENTRAL_CIVIL_TIME_POLICY_REF.to_owned()),
                ..TemporalFacet::new(TemporalKind::Day)
            });
        }
    }

    if let Some(active_items) = ground.now.get("active_items").and_then(Value::as_array) {
        for item in active_items {
            facets.push(occurrence_facet_from_handoff(item)?);
        }
    }

    Ok(facets)
}

/// Map one NOW handoff into an `occurrence` facet. The handoff's declared
/// `recorded_at_unix_seconds` is the occurrence instant; its session and run
/// continuity ride as the refs the handoff itself declares.
fn occurrence_facet_from_handoff(handoff: &Value) -> Result<TemporalFacet> {
    let id = handoff
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let recorded_at = handoff
        .get("recorded_at_unix_seconds")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AikitError::new(
                "central.temporal_handoff_malformed",
                "Central NOW handoff declares no `recorded_at_unix_seconds`; refusing to guess the occurrence time",
            )
            .with("id", id)
        })?;
    Ok(TemporalFacet {
        kind: TemporalKind::Occurrence,
        instant: Some(unix_seconds_to_rfc3339(
            recorded_at,
            "recorded_at_unix_seconds",
        )?),
        precision: Some(TemporalPrecision::Second),
        session_ref: declared_ref(handoff.get("session_ref")),
        run_ref: declared_ref(handoff.get("run_ref")),
        uncertainty: Some(HANDOFF_RECORDED_AT_UNCERTAINTY.to_owned()),
        ..TemporalFacet::new(TemporalKind::Occurrence)
    })
}

/// Map one `central.source-change-horizon/v1` change into a `receipt` facet.
/// The reconciler's declared `observed_at_unix_seconds` is when Central
/// observed the change — a receipt, distinct from any occurrence of the
/// underlying work.
pub fn receipt_facet_from_horizon_change(change: &Value) -> Result<TemporalFacet> {
    let change_ref = change
        .get("change_ref")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let source_ref = change
        .get("source_ref")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AikitError::new(
                "central.temporal_horizon_change_malformed",
                "source-change horizon change declares no `source_ref`; refusing to guess what was observed",
            )
            .with("change_ref", change_ref)
        })?;
    let observed_at = change
        .get("observed_at_unix_seconds")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AikitError::new(
                "central.temporal_horizon_change_malformed",
                "source-change horizon change declares no `observed_at_unix_seconds`; refusing to guess the receipt time",
            )
            .with("change_ref", change_ref)
        })?;
    Ok(TemporalFacet {
        kind: TemporalKind::Receipt,
        instant: Some(unix_seconds_to_rfc3339(
            observed_at,
            "observed_at_unix_seconds",
        )?),
        precision: Some(TemporalPrecision::Second),
        source_ref: Some(source_ref.to_owned()),
        session_ref: declared_ref(change.get("agent_session_ref")),
        uncertainty: Some(HORIZON_OBSERVED_AT_UNCERTAINTY.to_owned()),
        ..TemporalFacet::new(TemporalKind::Receipt)
    })
}

/// A declared optional ref from wire data: present only when a non-empty
/// string. A blank declared ref is carried as absence, never as a value.
fn declared_ref(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Unix seconds are absolute instants; the RFC 3339 rendering stays in UTC
/// (`Z`). Civil-day interpretation belongs to Central's timezone policy,
/// which rides as `timezone_policy_ref` — never re-enacted here.
fn unix_seconds_to_rfc3339(seconds: u64, field: &str) -> Result<String> {
    let timestamp = jiff::Timestamp::from_second(i64::try_from(seconds).map_err(|_| {
        AikitError::new(
            "central.temporal_ground_malformed",
            format!("declared `{field}` exceeds the representable instant range"),
        )
        .with("value", seconds.to_string())
    })?)
    .map_err(|error| {
        AikitError::new(
            "central.temporal_ground_malformed",
            format!("declared `{field}` is not a representable instant: {error}"),
        )
        .with("value", seconds.to_string())
    })?;
    Ok(timestamp.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The NOW-inspection payload shape Central's `projectcentral.now.inspect`
    /// publishes (paths, day_records, active_items with recorded handoffs).
    fn inspection_now() -> Value {
        json!({
            "project_root": "/home/me/Central/Work/example",
            "exists": true,
            "paths": {
                "root": "ProjectCentral/now",
                "user": "ProjectCentral/now/user",
                "agents": "ProjectCentral/now/agents",
                "day": "ProjectCentral/now/day",
                "policy": "ProjectCentral/now/policy.json",
                "promotions": "ProjectCentral/now/promotions"
            },
            "policy": {"schema": "central.projectcentral-now-policy/v1",
                       "carry_statuses": ["active", "waiting", "carried"],
                       "remove_statuses": ["resolved", "expired", "promoted"],
                       "protect_when_preserve_refs_exist": true,
                       "human_scratch_cleanup": "human-owned-manual",
                       "day_boundary": "caller-supplied-local-civil-date"},
            "human_scratch": ["ProjectCentral/now/user/current.md"],
            "active_items": [
                {"schema": "central.now-handoff/v1", "id": "h1",
                 "provenance": "agent-return", "actor": "agent:Epii", "kind": "handoff",
                 "recorded_at_unix_seconds": 1757941140,
                 "subject": "Current work", "result": "continue from live state",
                 "status": "active",
                 "session_ref": "zcode-session-2026-09-16-l5-techne-execution",
                 "source_refs": ["ProjectCentral/now/tmp/verification.md"]}
            ],
            "open_questions": [],
            "inactive_items": [],
            "invalid_items": [],
            "day_records": ["ProjectCentral/now/day/2026-09-15.md"],
            "promotions": [],
            "boundaries": []
        })
    }

    fn ground_with(now: Value) -> CentralTemporalGround {
        CentralTemporalGround {
            project: "example".to_owned(),
            now,
            day: None,
        }
    }

    #[test]
    fn a_central_ground_maps_day_now_and_handoff_occurrence_facets() {
        let facets = temporal_facets_from_central_ground(&ground_with(inspection_now())).unwrap();

        let now_facet = facets
            .iter()
            .find(|facet| facet.kind == TemporalKind::Now)
            .expect("NOW facet mapped");
        assert_eq!(now_facet.now_ref.as_deref(), Some("ProjectCentral/now"));
        assert!(
            now_facet.instant.is_none(),
            "no instant is invented for a NOW ref"
        );

        let day_facet = facets
            .iter()
            .find(|facet| facet.kind == TemporalKind::Day)
            .expect("DAY facet mapped");
        assert_eq!(
            day_facet.day_ref.as_deref(),
            Some("ProjectCentral/now/day/2026-09-15.md"),
            "Central's own ref is carried verbatim"
        );
        assert_eq!(
            day_facet.timezone_policy_ref.as_deref(),
            Some(CENTRAL_CIVIL_TIME_POLICY_REF)
        );

        let occurrence = facets
            .iter()
            .find(|facet| facet.kind == TemporalKind::Occurrence)
            .expect("occurrence facet mapped");
        assert_eq!(occurrence.instant.as_deref(), Some("2025-09-15T12:59:00Z"));
        assert_eq!(occurrence.precision, Some(TemporalPrecision::Second));
        assert_eq!(
            occurrence.session_ref.as_deref(),
            Some("zcode-session-2026-09-16-l5-techne-execution")
        );
        assert_eq!(
            occurrence.uncertainty.as_deref(),
            Some(HANDOFF_RECORDED_AT_UNCERTAINTY)
        );
    }

    #[test]
    fn an_absent_now_field_maps_to_no_facets() {
        let absent = json!({
            "project_root": "/home/me/Central/Work/example",
            "exists": false,
            "paths": {"root": "", "user": "", "agents": "", "day": "", "policy": "", "promotions": ""},
            "policy": null,
            "human_scratch": [],
            "active_items": [],
            "open_questions": [],
            "inactive_items": [],
            "invalid_items": [],
            "day_records": [],
            "promotions": [],
            "boundaries": []
        });
        assert!(temporal_facets_from_central_ground(&ground_with(absent))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn occurrence_and_receipt_stay_distinguishable_through_the_mapping() {
        let mut facets =
            temporal_facets_from_central_ground(&ground_with(inspection_now())).unwrap();
        facets.push(
            receipt_facet_from_horizon_change(&json!({
                "schema": "central.source-change-horizon/v1",
                "change_ref": "central:change:7",
                "cursor": 7,
                "source_ref": "central:source:control:root:.central/source-change-horizon.json",
                "kind": "modified",
                "observed_at_unix_seconds": 1757941440
            }))
            .unwrap(),
        );

        let occurrence = facets
            .iter()
            .find(|facet| facet.kind == TemporalKind::Occurrence)
            .expect("occurrence present");
        let receipt = facets
            .iter()
            .find(|facet| facet.kind == TemporalKind::Receipt)
            .expect("receipt present");

        assert_ne!(occurrence.kind, receipt.kind);
        assert_ne!(occurrence.uncertainty, receipt.uncertainty);
        assert_eq!(
            receipt.uncertainty.as_deref(),
            Some(HORIZON_OBSERVED_AT_UNCERTAINTY)
        );
        assert_eq!(receipt.instant.as_deref(), Some("2025-09-15T13:04:00Z"));
        assert_eq!(
            receipt.source_ref.as_deref(),
            Some("central:source:control:root:.central/source-change-horizon.json")
        );
        assert!(
            receipt.source_ref.is_some() && occurrence.source_ref.is_none(),
            "the receipt names the source it observed; the occurrence names none"
        );
    }

    #[test]
    fn a_handoff_without_a_recorded_at_is_a_named_error_not_a_guessed_time() {
        let mut now = inspection_now();
        now["active_items"][0]
            .as_object_mut()
            .unwrap()
            .remove("recorded_at_unix_seconds");

        let error = temporal_facets_from_central_ground(&ground_with(now)).unwrap_err();
        assert_eq!(error.code(), "central.temporal_handoff_malformed");
        assert_eq!(error.details().get("id").map(String::as_str), Some("h1"));
    }

    #[test]
    fn a_horizon_change_without_a_source_or_observed_at_is_rejected() {
        let error = receipt_facet_from_horizon_change(&json!({
            "change_ref": "central:change:8",
            "observed_at_unix_seconds": 1757941440
        }))
        .unwrap_err();
        assert_eq!(error.code(), "central.temporal_horizon_change_malformed");

        let error = receipt_facet_from_horizon_change(&json!({
            "change_ref": "central:change:8",
            "source_ref": "central:source:control:root:Control/user/day/2026-09-15/day.md"
        }))
        .unwrap_err();
        assert_eq!(error.code(), "central.temporal_horizon_change_malformed");
    }

    #[test]
    fn an_inspection_without_a_now_path_is_refused_rather_than_re_keyed() {
        let mut now = inspection_now();
        now["paths"].as_object_mut().unwrap().remove("root");

        let error = temporal_facets_from_central_ground(&ground_with(now)).unwrap_err();
        assert_eq!(error.code(), "central.temporal_ground_malformed");
    }

    #[test]
    fn mapped_facets_satisfy_the_declared_facet_contract() {
        use aikit_core::knowledge_facets::{
            parse_facets_from_extensions, write_facets_to_extensions, TechneFacets,
            TECHNE_FACET_EXTENSION,
        };
        use std::collections::BTreeMap;

        let facets = TechneFacets {
            temporal: temporal_facets_from_central_ground(&ground_with(inspection_now())).unwrap(),
            spatial: Vec::new(),
        };
        let mut extensions = BTreeMap::new();
        write_facets_to_extensions(&mut extensions, &facets).unwrap();
        assert!(extensions.contains_key(TECHNE_FACET_EXTENSION));

        let read_back = parse_facets_from_extensions(&extensions).unwrap();
        assert_eq!(read_back, facets);
    }
}
