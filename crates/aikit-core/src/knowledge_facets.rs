//! Declared Technē facets over the open extension maps: temporal, spatial and
//! exact source selectors (`aikit.techne-facet/v1`).
//!
//! Semantic authority stays with Quaternal-Logic's language-neutral
//! `ql.techne/v1` reading (schemas/techne/ql-techne-reading-v1.schema.json,
//! L5 wayfinder §2). AIKit mirrors those semantics as a *declared extension*
//! on objects it already owns — WikiObject/Node/Edge/Frame/Reading and
//! [`WikiProvenanceRef`] extension maps — and never mints a parallel temporal
//! or spatial ontology.
//!
//! The seam exists so instruments (timeline, map, canvas) can read richness
//! without flattening native identity. The laws it owns:
//!
//! - **Absence is data.** An extension map without the facet key parses to an
//!   empty [`TechneFacets`]; a write that carries no facets writes nothing, so
//!   an object without facets round-trips byte-equivalent.
//! - **Validate-but-preserve.** Malformed *declared* facet data is a
//!   namespaced error (same discipline as `aikit.ql-stance/v1`); unknown
//!   extensions and unknown contract versions under *other* keys are
//!   preserved untouched. An unknown version under this key names the version
//!   it found rather than guessing.
//! - **Time is not one timestamp.** Occurrence, receipt, validity,
//!   source-created/modified and DAY/NOW/Session/Run continuity stay distinct
//!   kinds; a facet must declare at least one temporal carrier (instant,
//!   interval, or a continuity ref) and may never be collapsed to a single
//!   creation time.
//! - **Native refs stay opaque.** `day_ref`, `now_ref`, `session_ref`,
//!   `run_ref`, `place_ref`, `source_ref` and friends are carried verbatim;
//!   this module never re-keys or shortens them.
//! - **Exact selectors ride provenance.** A [`SourceSelector`] attaches to a
//!   provenance entry's extension map, so a span/range/region returns to the
//!   same source unit from every instrument.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::knowledge_wiki::WikiProvenanceRef;
use crate::{AikitError, Result};

/// The declared facet contract key. The same key carries the facet payload on
/// object extension maps and the selector payload on provenance extension
/// maps; both payloads self-identify through their `contract` field.
pub const TECHNE_FACET_EXTENSION: &str = "aikit.techne-facet/v1";

/// The facet contract's error type. Facet failures are ordinary namespaced
/// [`AikitError`]s (`knowledge.facet_*` codes), so they surface in the same
/// `--json` envelope as every other knowledge error.
pub type FacetError = AikitError;

// ---------------------------------------------------------------------------
// Wire payloads
// ---------------------------------------------------------------------------

/// The facet payload found under [`TECHNE_FACET_EXTENSION`] on an object's
/// extension map. Both facet lists are optional — absence is data, not an
/// error to complete.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechneFacetDeclaration {
    pub contract: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub temporal: Vec<TemporalFacet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spatial: Vec<PlaceFacet>,
}

/// The selector payload found under [`TECHNE_FACET_EXTENSION`] on a
/// provenance entry's extension map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechneSelectorDeclaration {
    pub contract: String,
    pub selector: SourceSelector,
}

// ---------------------------------------------------------------------------
// Temporal facet
// ---------------------------------------------------------------------------

/// One labelled native time fact. Occurrence, receipt, validity and
/// continuity stay distinct; consumers never collapse them to `created_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalFacet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facet_ref: Option<String>,
    pub kind: TemporalKind,
    /// RFC 3339 date-time. Absent when the facet carries a ref or interval
    /// instead — Central's DAY/NOW continuity refs are real carriers, not
    /// degraded instants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<TemporalInterval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<TemporalPrecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone_policy_ref: Option<String>,
    /// Declared uncertainty, e.g. `author-declared recorded_at`. Uncertainty
    /// is data about how the fact was declared, never a reason to drop it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
}

impl TemporalFacet {
    /// A facet with one kind and no carriers yet; the carrier rule rejects it
    /// until a caller supplies at least one.
    pub fn new(kind: TemporalKind) -> Self {
        Self {
            facet_ref: None,
            kind,
            instant: None,
            interval: None,
            precision: None,
            day_ref: None,
            now_ref: None,
            session_ref: None,
            run_ref: None,
            timezone_policy_ref: None,
            uncertainty: None,
            source_ref: None,
        }
    }

    /// The temporal carrier rule: at least one of instant / interval /
    /// day_ref / now_ref / session_ref / run_ref. A kind label alone is not a
    /// time fact.
    pub fn has_carrier(&self) -> bool {
        self.instant.is_some()
            || self.interval.is_some()
            || self.day_ref.is_some()
            || self.now_ref.is_some()
            || self.session_ref.is_some()
            || self.run_ref.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemporalKind {
    Occurrence,
    Receipt,
    Valid,
    SourceCreated,
    SourceModified,
    Day,
    Now,
    Session,
    Run,
    Generation,
    Presentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TemporalPrecision {
    Millennium,
    Century,
    Decade,
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    Subsecond,
}

/// A half-open or open interval whose bounds may each carry their own
/// precision — a Factory Run bounded by handoff-declared windows, a
/// validity interval known only to the decade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TemporalInterval {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_precision: Option<TemporalPrecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_precision: Option<TemporalPrecision>,
}

// ---------------------------------------------------------------------------
// Spatial facet
// ---------------------------------------------------------------------------

/// A temporally valid Place identity, independent of coordinates. Precision
/// and hierarchy are inspectable; uncertainty is data, never smoothed away.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceFacet {
    pub place_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<PlaceIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<PlaceGeometry>,
    pub precision: PlacePrecision,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hierarchy: Vec<PlaceHierarchyEntry>,
    /// Plain strings on purpose: the canonical schema types validity bounds as
    /// strings, not date-times, because historical places are valid across
    /// periods no RFC 3339 instant can name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observer_frame: Option<String>,
    /// Owner-declared spatial qualification from ql.techne/v1; never inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
}

impl PlaceFacet {
    pub fn new(place_ref: impl Into<String>, precision: PlacePrecision) -> Self {
        Self {
            place_ref: place_ref.into(),
            identity: None,
            geometry: None,
            precision,
            hierarchy: Vec::new(),
            valid_from: None,
            valid_to: None,
            observer_frame: None,
            uncertainty: None,
            source_ref: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PlaceIdentity {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<PlaceName>,
}

/// One place name with its own validity window — an institution renamed in
/// 1851 keeps both names without becoming two places.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceName {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlaceGeometryType {
    Point,
    Polygon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceGeometry {
    #[serde(rename = "type")]
    pub geometry_type: PlaceGeometryType,
    /// GeoJSON-shaped coordinate payload (`[lon, lat]` for a point, a ring of
    /// rings for a polygon), carried as declared — coordinates are the
    /// producer's data, not this module's to normalise.
    pub coordinates: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlacePrecision {
    Exact,
    Approximate,
    Region,
    Unlocated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceHierarchyEntry {
    pub place_ref: String,
    /// Provider/native containment vocabulary, preserved verbatim (`within`,
    /// `part-of`, `in` — never normalised here).
    pub relation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
}

// ---------------------------------------------------------------------------
// Source selector
// ---------------------------------------------------------------------------

/// The exact source unit a reading came from. `unit` is the wire
/// discriminator, so every instrument returns to the same span, range or
/// region.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit", rename_all = "snake_case")]
pub enum SourceSelector {
    /// A character span of a text source; `anchor_ref` optionally names a
    /// stable anchor (a heading, a note) when raw offsets are not enough.
    TextSpan {
        start: u64,
        end: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor_ref: Option<String>,
    },
    /// An audio/video range, bounds as RFC 3339 date-times.
    TimestampRange { from: String, to: String },
    /// A rectangular region of an image, in the producer's own coordinate
    /// space.
    ImageRegion {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    /// A producer-declared selector this contract does not enumerate. The
    /// kind stays producer vocabulary, carried verbatim.
    Other { kind: String, value: String },
}

/// The additional-properties floor the canonical schema puts on each selector
/// unit; serde's internally-tagged enums cannot enforce it, so the parse path
/// does.
fn selector_allowed_keys(unit: &str) -> Option<&'static [&'static str]> {
    match unit {
        "text_span" => Some(&["unit", "start", "end", "anchor_ref"]),
        "timestamp_range" => Some(&["unit", "from", "to"]),
        "image_region" => Some(&["unit", "x", "y", "width", "height"]),
        "other" => Some(&["unit", "kind", "value"]),
        _ => None,
    }
}

fn validate_selector_shape(selector: &Value) -> Result<()> {
    let object = selector.as_object().ok_or_else(|| {
        AikitError::new(
            "knowledge.facet_invalid_selector",
            "a source selector must be a JSON object with a `unit` discriminator",
        )
    })?;
    let unit = object.get("unit").and_then(Value::as_str).ok_or_else(|| {
        AikitError::new(
            "knowledge.facet_invalid_selector",
            "a source selector requires a `unit` of text_span, timestamp_range, image_region or other",
        )
    })?;
    let allowed = selector_allowed_keys(unit).ok_or_else(|| {
        AikitError::new(
            "knowledge.facet_invalid_selector",
            format!("unknown selector unit `{unit}`"),
        )
        .with("unit", unit)
    })?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(AikitError::new(
            "knowledge.facet_invalid_selector",
            format!("selector unit `{unit}` carries unexpected field `{key}`"),
        )
        .with("unit", unit)
        .with("field", key.clone()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Read / write over extension maps
// ---------------------------------------------------------------------------

/// The facets an extension map declares. Empty when the key is absent —
/// absence is data, not an error.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TechneFacets {
    pub temporal: Vec<TemporalFacet>,
    pub spatial: Vec<PlaceFacet>,
}

/// Read the declared Technē facets from an object's extension map. Absent key
/// yields an empty reading; a declaration under a different contract version
/// or malformed declared data is a namespaced error.
pub fn parse_facets_from_extensions(extensions: &BTreeMap<String, Value>) -> Result<TechneFacets> {
    let Some(declared) = extensions.get(TECHNE_FACET_EXTENSION) else {
        return Ok(TechneFacets::default());
    };
    let declaration: TechneFacetDeclaration = decode_declaration(
        declared,
        &["contract", "temporal", "spatial"],
        "knowledge.facet_extension_invalid",
    )?;
    let mut facets = TechneFacets::default();
    for (index, facet) in declaration.temporal.iter().enumerate() {
        validate_temporal_facet(facet).map_err(|error| error.with("index", index.to_string()))?;
        facets.temporal.push(facet.clone());
    }
    for (index, facet) in declaration.spatial.iter().enumerate() {
        validate_place_facet(facet).map_err(|error| error.with("index", index.to_string()))?;
        facets.spatial.push(facet.clone());
    }
    Ok(facets)
}

/// Write facets under the declared contract key. A reading with neither
/// temporal nor spatial facets writes nothing: an object without facets must
/// round-trip byte-equivalent, not gain an empty declaration.
pub fn write_facets_to_extensions(
    extensions: &mut BTreeMap<String, Value>,
    facets: &TechneFacets,
) -> Result<()> {
    if facets.temporal.is_empty() && facets.spatial.is_empty() {
        return Ok(());
    }
    for (index, facet) in facets.temporal.iter().enumerate() {
        validate_temporal_facet(facet).map_err(|error| error.with("index", index.to_string()))?;
    }
    for (index, facet) in facets.spatial.iter().enumerate() {
        validate_place_facet(facet).map_err(|error| error.with("index", index.to_string()))?;
    }
    let declaration = TechneFacetDeclaration {
        contract: TECHNE_FACET_EXTENSION.to_owned(),
        temporal: facets.temporal.clone(),
        spatial: facets.spatial.clone(),
    };
    extensions.insert(
        TECHNE_FACET_EXTENSION.to_owned(),
        serde_json::to_value(declaration).map_err(|error| {
            AikitError::new(
                "knowledge.facet_extension_invalid",
                format!("facet declaration failed to serialize: {error}"),
            )
        })?,
    );
    Ok(())
}

/// Read the exact source selector declared on a provenance entry, if any.
/// Absent key, or a declaration with a null/absent selector, means the native
/// producer supplied no exact selector — data, not an error.
pub fn parse_source_selector(
    extensions: &BTreeMap<String, Value>,
) -> Result<Option<SourceSelector>> {
    let Some(declared) = extensions.get(TECHNE_FACET_EXTENSION) else {
        return Ok(None);
    };
    let object = declared.as_object().ok_or_else(|| {
        AikitError::new(
            "knowledge.facet_extension_invalid",
            "the selector declaration must be a JSON object carrying its `contract`",
        )
    })?;
    require_declared_contract(object)?;
    if let Some(key) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "contract" | "selector"))
    {
        return Err(AikitError::new(
            "knowledge.facet_extension_invalid",
            format!("selector declaration carries unexpected field `{key}`"),
        )
        .with("field", key.clone()));
    }
    match object.get("selector") {
        None | Some(Value::Null) => Ok(None),
        Some(selector) => {
            validate_selector_shape(selector)?;
            let decoded: SourceSelector =
                serde_json::from_value(selector.clone()).map_err(|error| {
                    AikitError::new(
                        "knowledge.facet_invalid_selector",
                        format!("malformed declared source selector: {error}"),
                    )
                })?;
            // Declared data must satisfy the field laws on read too, not only
            // on write: a timestamp_range whose bounds are not date-times is
            // refused, never surfaced as a plausible-looking range.
            validate_selector_fields(&decoded)?;
            Ok(Some(decoded))
        }
    }
}

/// Attach an exact source selector to a provenance entry under the declared
/// contract key, validating first.
pub fn write_source_selector(
    extensions: &mut BTreeMap<String, Value>,
    selector: &SourceSelector,
) -> Result<()> {
    let selector_value = serde_json::to_value(selector).map_err(|error| {
        AikitError::new(
            "knowledge.facet_invalid_selector",
            format!("source selector failed to serialize: {error}"),
        )
    })?;
    validate_selector_shape(&selector_value)?;
    validate_selector_fields(selector)?;
    extensions.insert(
        TECHNE_FACET_EXTENSION.to_owned(),
        serde_json::to_value(TechneSelectorDeclaration {
            contract: TECHNE_FACET_EXTENSION.to_owned(),
            selector: selector.clone(),
        })
        .map_err(|error| {
            AikitError::new(
                "knowledge.facet_extension_invalid",
                format!("selector declaration failed to serialize: {error}"),
            )
        })?,
    );
    Ok(())
}

/// Attach an exact source selector to a provenance entry.
pub fn attach_source_selector(
    provenance: &mut WikiProvenanceRef,
    selector: &SourceSelector,
) -> Result<()> {
    write_source_selector(&mut provenance.extensions, selector)
}

/// Read the exact source selector from a provenance entry, if declared.
pub fn read_source_selector(provenance: &WikiProvenanceRef) -> Result<Option<SourceSelector>> {
    parse_source_selector(&provenance.extensions)
}

// ---------------------------------------------------------------------------
// Validation — the canonical schema's laws, mirrored
// ---------------------------------------------------------------------------

fn validate_temporal_facet(facet: &TemporalFacet) -> Result<()> {
    if !facet.has_carrier() {
        return Err(AikitError::new(
            "knowledge.facet_invalid_temporal",
            "a temporal facet must carry at least one of instant, interval, day_ref, now_ref, session_ref or run_ref",
        )
        .with("kind", format!("{:?}", facet.kind)));
    }
    if let Some(instant) = &facet.instant {
        require_rfc3339(
            "knowledge.facet_invalid_temporal",
            instant,
            "temporal.instant",
        )?;
    }
    if let Some(interval) = &facet.interval {
        if let Some(from) = &interval.from {
            require_rfc3339(
                "knowledge.facet_invalid_temporal",
                from,
                "temporal.interval.from",
            )?;
        }
        if let Some(to) = &interval.to {
            require_rfc3339(
                "knowledge.facet_invalid_temporal",
                to,
                "temporal.interval.to",
            )?;
        }
    }
    for (field, value) in [
        ("facet_ref", &facet.facet_ref),
        ("day_ref", &facet.day_ref),
        ("now_ref", &facet.now_ref),
        ("session_ref", &facet.session_ref),
        ("run_ref", &facet.run_ref),
        ("timezone_policy_ref", &facet.timezone_policy_ref),
        ("source_ref", &facet.source_ref),
    ] {
        if let Some(value) = value {
            require_non_empty("knowledge.facet_invalid_temporal", value, field)?;
        }
    }
    Ok(())
}

fn validate_place_facet(facet: &PlaceFacet) -> Result<()> {
    require_non_empty(
        "knowledge.facet_invalid_place",
        &facet.place_ref,
        "place_ref",
    )?;
    if let Some(identity) = &facet.identity {
        for name in &identity.names {
            require_non_empty(
                "knowledge.facet_invalid_place",
                &name.name,
                "place.identity.names.name",
            )?;
        }
    }
    for entry in &facet.hierarchy {
        require_non_empty(
            "knowledge.facet_invalid_place",
            &entry.place_ref,
            "place.hierarchy.place_ref",
        )?;
        require_non_empty(
            "knowledge.facet_invalid_place",
            &entry.relation,
            "place.hierarchy.relation",
        )?;
    }
    if let Some(source_ref) = &facet.source_ref {
        require_non_empty(
            "knowledge.facet_invalid_place",
            source_ref,
            "place.source_ref",
        )?;
    }
    if let Some(observer_frame) = &facet.observer_frame {
        require_non_empty(
            "knowledge.facet_invalid_place",
            observer_frame,
            "place.observer_frame",
        )?;
    }
    Ok(())
}

fn validate_selector_fields(selector: &SourceSelector) -> Result<()> {
    match selector {
        SourceSelector::TimestampRange { from, to } => {
            require_rfc3339("knowledge.facet_invalid_selector", from, "selector.from")?;
            require_rfc3339("knowledge.facet_invalid_selector", to, "selector.to")?;
        }
        SourceSelector::Other { kind, value } => {
            require_non_empty("knowledge.facet_invalid_selector", kind, "selector.kind")?;
            require_non_empty("knowledge.facet_invalid_selector", value, "selector.value")?;
        }
        SourceSelector::TextSpan { anchor_ref, .. } => {
            if let Some(anchor_ref) = anchor_ref {
                require_non_empty(
                    "knowledge.facet_invalid_selector",
                    anchor_ref,
                    "selector.anchor_ref",
                )?;
            }
        }
        SourceSelector::ImageRegion { .. } => {}
    }
    Ok(())
}

fn require_non_empty(code: &'static str, value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(AikitError::new(
            code,
            format!(
                "`{field}` must be non-empty when present (native refs are opaque, never blank)"
            ),
        )
        .with("field", field));
    }
    Ok(())
}

fn require_rfc3339(code: &'static str, value: &str, field: &str) -> Result<()> {
    value
        .parse::<jiff::Timestamp>()
        .map(|_| ())
        .map_err(|error| {
            AikitError::new(
                code,
                format!("`{field}` must be an RFC 3339 date-time: {value} ({error})"),
            )
            .with("field", field)
        })
}

// ---------------------------------------------------------------------------
// Shared declaration decoding
// ---------------------------------------------------------------------------

/// Decode a declared payload that must self-identify through `contract`,
/// rejecting unknown top-level fields (the canonical schema's
/// additionalProperties floor) and naming a foreign version it finds.
fn decode_declaration<T: for<'de> Deserialize<'de>>(
    declared: &Value,
    allowed_keys: &[&str],
    invalid_code: &'static str,
) -> Result<T> {
    let object = declared.as_object().ok_or_else(|| {
        AikitError::new(
            invalid_code,
            "the facet declaration must be a JSON object carrying its `contract`",
        )
    })?;
    require_declared_contract(object)?;
    if let Some(key) = object
        .keys()
        .find(|key| !allowed_keys.contains(&key.as_str()))
    {
        return Err(AikitError::new(
            invalid_code,
            format!("facet declaration carries unexpected field `{key}`"),
        )
        .with("field", key.clone()));
    }
    serde_json::from_value(declared.clone()).map_err(|error| {
        AikitError::new(
            invalid_code,
            format!("malformed declared facet payload: {error}"),
        )
    })
}

fn require_declared_contract(object: &serde_json::Map<String, Value>) -> Result<()> {
    let contract = object
        .get("contract")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.facet_extension_invalid",
                format!("a facet declaration must name its `contract` (expected `{TECHNE_FACET_EXTENSION}`)"),
            )
        })?;
    if contract == TECHNE_FACET_EXTENSION {
        Ok(())
    } else {
        Err(AikitError::new(
            "knowledge.facet_contract_unsupported",
            format!(
                "unsupported facet contract `{contract}`; expected `{TECHNE_FACET_EXTENSION}` — declared data under an unknown version is refused, not reinterpreted"
            ),
        )
        .with("found", contract)
        .with("expected", TECHNE_FACET_EXTENSION))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_absent_key_parses_to_an_empty_reading_and_a_default_write_is_a_no_op() {
        let mut extensions: BTreeMap<String, Value> = BTreeMap::new();
        extensions.insert("unknown-extension".to_owned(), json!({"kept": true}));
        let before = extensions.clone();

        let facets = parse_facets_from_extensions(&extensions).unwrap();
        assert!(facets.temporal.is_empty() && facets.spatial.is_empty());

        write_facets_to_extensions(&mut extensions, &facets).unwrap();
        assert_eq!(extensions, before, "an empty write must not touch the map");
    }

    #[test]
    fn a_facet_without_a_carrier_is_refused_even_when_constructed_in_rust() {
        let facet = TemporalFacet::new(TemporalKind::Occurrence);
        let error = validate_temporal_facet(&facet).unwrap_err();
        assert_eq!(error.code(), "knowledge.facet_invalid_temporal");
        assert!(!facet.has_carrier());

        let carried = TemporalFacet {
            now_ref: Some("central:now:control:root:x".to_owned()),
            ..TemporalFacet::new(TemporalKind::Now)
        };
        assert!(validate_temporal_facet(&carried).is_ok());
    }

    #[test]
    fn a_non_rfc3339_instant_is_a_named_error() {
        let facet = TemporalFacet {
            instant: Some("15/09/2026".to_owned()),
            ..TemporalFacet::new(TemporalKind::Receipt)
        };
        let error = validate_temporal_facet(&facet).unwrap_err();
        assert_eq!(error.code(), "knowledge.facet_invalid_temporal");
        assert_eq!(
            error.details().get("field").map(String::as_str),
            Some("temporal.instant")
        );
    }

    #[test]
    fn a_selector_carrying_fields_its_unit_does_not_declare_is_refused() {
        let selector = json!({"unit": "text_span", "start": 0, "end": 5, "width": 3});
        let error = validate_selector_shape(&selector).unwrap_err();
        assert_eq!(error.code(), "knowledge.facet_invalid_selector");
        assert_eq!(
            error.details().get("field").map(String::as_str),
            Some("width")
        );

        let unknown_unit = json!({"unit": "glyph_range", "start": 0, "end": 5});
        let error = validate_selector_shape(&unknown_unit).unwrap_err();
        assert_eq!(error.code(), "knowledge.facet_invalid_selector");
    }

    #[test]
    fn provenance_helpers_round_trip_through_the_same_contract_key() {
        let mut provenance = WikiProvenanceRef {
            source_ref: crate::SourceRef::parse("example:source:paper").unwrap(),
            source_revision: None,
            producer_ref: None,
            generation_ref: None,
            extensions: BTreeMap::new(),
        };
        attach_source_selector(
            &mut provenance,
            &SourceSelector::TimestampRange {
                from: "2026-09-15T09:10:00Z".to_owned(),
                to: "2026-09-15T16:40:00Z".to_owned(),
            },
        )
        .unwrap();
        let read = read_source_selector(&provenance).unwrap().unwrap();
        assert_eq!(
            read,
            SourceSelector::TimestampRange {
                from: "2026-09-15T09:10:00Z".to_owned(),
                to: "2026-09-15T16:40:00Z".to_owned()
            }
        );
    }
}
