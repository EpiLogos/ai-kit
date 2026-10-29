//! Targeted facts on existing Wiki objects, without replacing their other
//! properties or requiring an otherwise unrelated construction. Persistence
//! uses the same native Wiki transaction owner as constructive actions.
use crate::knowledge_facets::{
    parse_facets_from_extensions, validate_route_order, write_facets_to_extensions, PlaceFacet,
    TemporalFacet, TECHNE_FACET_EXTENSION,
};
use crate::knowledge_wiki::WikiSpace;
use crate::knowledge_wiki_write::{apply_wiki_mutation, WikiDocument};
use crate::resource::ResourceRef;
use crate::{AikitError, Result, WikiObject};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const ACTION: &str = "aikit.wiki-facts-action/v1";
pub const READING: &str = "aikit.wiki-facts/v1";
const RECEIPTS: &str = "aikit.wiki-facts-operations/v1";
/// The world body a register's root space carries (manual §17.2 "Build a
/// planet"): the settings and seed a generator turns into a body. Locations
/// on the body are not part of it, so replacing the body never moves one.
pub const WORLD_BODY_EXTENSION: &str = "aikit.world-body/v1";
/// The one generator this owner names: fractal noise on the unit sphere.
pub const WORLD_BODY_GENERATOR: &str = "fbm-sphere";
/// The object whose facts change. A `Space` is a register's root space row:
/// the only object that carries a world body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Node { r#ref: ResourceRef },
    Whole { r#ref: ResourceRef },
    Space { r#ref: ResourceRef },
}
impl Target {
    pub fn reference(&self) -> &ResourceRef {
        match self {
            Self::Node { r#ref } | Self::Whole { r#ref } | Self::Space { r#ref } => r#ref,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    TemporalSet {
        temporal: Vec<TemporalFacet>,
    },
    PlaceSet {
        places: Vec<PlaceFacet>,
    },
    /// Replace the register's world body whole. A reroll is a BodySet with a
    /// new seed and the same settings.
    BodySet {
        body: WorldBody,
    },
}
/// `aikit.world-body/v1`: the body of one register's world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldBody {
    pub body_ref: ResourceRef,
    pub seed: u32,
    pub generator: BodyGenerator,
    pub landmass: BodyLandmass,
    pub climate: BodyClimate,
    pub sky: BodySky,
    /// Where this body lands as a place on another body (a project's world on
    /// your planet). Absent: it lands nowhere (your own planet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing: Option<BodyLanding>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyGenerator {
    pub id: String,
    pub version: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyCoast {
    Smooth,
    Fjord,
    Archipelago,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyLandmass {
    pub sea_level: f64,
    pub continents: f64,
    pub relief: f64,
    pub coast: BodyCoast,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyClimate {
    pub warmth: f64,
    pub moisture: f64,
    pub ice_caps: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodySky {
    pub atmosphere: f64,
    pub clouds: f64,
    pub axial_tilt_deg: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyLanding {
    /// The body it lands on.
    pub on_ref: ResourceRef,
    pub lat: f64,
    pub lon: f64,
}
/// The ranges a body's settings take, as the World device draws them.
pub const CONTINENTS_RANGE: (f64, f64) = (0.6, 3.6);
pub const AXIAL_TILT_RANGE: (f64, f64) = (0.0, 45.0);
fn within(value: f64, (min, max): (f64, f64), what: &str) -> Result<()> {
    if !value.is_finite() || value < min || value > max {
        return Err(err(format!(
            "{what} is {value}; it runs from {min} to {max}, and the body is refused rather than clamped"
        )));
    }
    Ok(())
}
impl WorldBody {
    /// Refuse a body any of whose settings fall outside its range, in plain words.
    pub fn validate(&self) -> Result<()> {
        for reference in std::iter::once(&self.body_ref)
            .chain(self.landing.as_ref().map(|landing| &landing.on_ref))
        {
            ResourceRef::parse(reference.as_str())
                .map_err(|_| err(format!("`{reference}` is not a body reference")))?;
        }
        if self.generator.id != WORLD_BODY_GENERATOR {
            return Err(err(format!(
                "the generator `{}` is not one this owner knows; a body is generated by `{WORLD_BODY_GENERATOR}`",
                self.generator.id
            )));
        }
        if self.generator.version == 0 {
            return Err(err("the generator version starts at 1"));
        }
        let unit = (0.0, 1.0);
        within(self.landmass.sea_level, unit, "sea level")?;
        within(self.landmass.continents, CONTINENTS_RANGE, "continents")?;
        within(self.landmass.relief, unit, "relief")?;
        within(self.climate.warmth, unit, "warmth")?;
        within(self.climate.moisture, unit, "moisture")?;
        within(self.climate.ice_caps, unit, "ice caps")?;
        within(self.sky.atmosphere, unit, "atmosphere")?;
        within(self.sky.clouds, unit, "clouds")?;
        within(
            self.sky.axial_tilt_deg,
            AXIAL_TILT_RANGE,
            "axial tilt (degrees)",
        )?;
        if let Some(landing) = &self.landing {
            within(landing.lat, (-90.0, 90.0), "the landing's latitude")?;
            within(landing.lon, (-180.0, 180.0), "the landing's longitude")?;
            if landing.on_ref == self.body_ref {
                return Err(err("a body cannot land on itself"));
            }
        }
        Ok(())
    }
}
/// The world body an object's extensions carry, validated; None when it carries none.
pub fn world_body_of(extensions: &BTreeMap<String, Value>) -> Result<Option<WorldBody>> {
    let Some(value) = extensions.get(WORLD_BODY_EXTENSION) else {
        return Ok(None);
    };
    let body: WorldBody = serde_json::from_value(value.clone())
        .map_err(|e| err(format!("the stored world body is malformed: {e}")))?;
    body.validate()?;
    Ok(Some(body))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub target: Target,
    pub expected_revision: u64,
    pub actor_ref: ResourceRef,
    pub operation_ref: ResourceRef,
    pub changes: Vec<Change>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    actor_ref: ResourceRef,
    request_digest: String,
    basis_revision: u64,
    result_revision: u64,
}
#[derive(Debug, Serialize)]
pub struct Applied {
    pub target: Target,
    pub revision: u64,
    pub idempotent: bool,
    pub content: String,
    pub objects_changed: Vec<String>,
    pub warnings: Vec<String>,
}
fn err(message: impl Into<String>) -> AikitError {
    AikitError::new("knowledge.wiki_facts_refused", message)
}
fn conflict() -> AikitError {
    AikitError::new(
        "knowledge.wiki_facts_revision_conflict",
        "the exact native target changed; reread before editing its facts",
    )
}
pub(crate) fn require_writable(extensions: &BTreeMap<String, Value>) -> Result<()> {
    if extensions.get("read_only") == Some(&Value::Bool(true))
        || extensions.contains_key("shared_projection_ref")
    {
        return Err(err(
            "read-only shared material requires an explicit local derivative",
        ));
    }
    Ok(())
}
fn extensions<'a>(
    object: &'a mut WikiObject,
    target: &Target,
) -> Result<&'a mut BTreeMap<String, Value>> {
    match (object, target) {
        (WikiObject::Node(row), Target::Node { .. }) => Ok(&mut row.extensions),
        (WikiObject::Frame(row), Target::Whole { .. }) => Ok(&mut row.extensions),
        (WikiObject::Space(row), Target::Space { .. }) => Ok(&mut row.extensions),
        _ => Err(err(
            "target kind does not match the actual native Wiki object",
        )),
    }
}
/// A world body is carried by a register's root space alone, and a space
/// carries nothing but its body: refuse every other pairing by name.
fn require_pairing(target: &Target, changes: &[Change]) -> Result<()> {
    let space = matches!(target, Target::Space { .. });
    for change in changes {
        match (space, matches!(change, Change::BodySet { .. })) {
            (true, false) => {
                return Err(err(
                    "a register's space carries only its world body; time and place facts belong on a node or a whole",
                ))
            }
            (false, true) => {
                return Err(err(
                    "a world body belongs on a register's root space, not on a node or a whole",
                ))
            }
            _ => {}
        }
    }
    Ok(())
}
/// Shared validation/replacement for native node, whole and participation
/// actions. Only the selected facet family changes; unrelated declarations
/// and extension keys retain their exact values.
pub(crate) fn replace(extensions: &mut BTreeMap<String, Value>, change: &Change) -> Result<()> {
    // Validate into a separate map first, so callers never receive a partial
    // in-memory mutation when native validation refuses.
    let mut next = extensions.clone();
    match change {
        Change::BodySet { body } => {
            // The body is replaced whole; facets and every other key keep their exact values.
            body.validate()?;
            next.insert(
                WORLD_BODY_EXTENSION.into(),
                serde_json::to_value(body).map_err(|e| err(e.to_string()))?,
            );
        }
        Change::TemporalSet { temporal } => {
            if temporal.len() > 256
                || temporal
                    .iter()
                    .any(|f| f.source_ref.as_deref().is_none_or(|s| s.trim().is_empty()))
            {
                return Err(err(
                    "at most 256 temporal facts, each with a native source basis, are required",
                ));
            }
            let mut facets = parse_facets_from_extensions(extensions)?;
            facets.temporal = temporal.clone();
            next.remove(TECHNE_FACET_EXTENSION);
            write_facets_to_extensions(&mut next, &facets)?;
        }
        Change::PlaceSet { places } => {
            if places.len() > 256
                || places
                    .iter()
                    .any(|f| f.source_ref.as_deref().is_none_or(|s| s.trim().is_empty()))
            {
                return Err(err(
                    "at most 256 place facts, each with a native source basis, are required",
                ));
            }
            validate_route_order(places)?;
            let mut facets = parse_facets_from_extensions(extensions)?;
            facets.spatial = places.clone();
            next.remove(TECHNE_FACET_EXTENSION);
            write_facets_to_extensions(&mut next, &facets)?;
        }
    }
    *extensions = next;
    Ok(())
}
pub fn inspect(input: &str, reference: &ResourceRef) -> Result<Value> {
    let doc = WikiDocument::parse(input)?;
    let object = doc
        .object(reference)
        .ok_or_else(|| err("native facts target is absent"))?;
    let (kind, value) = match object {
        WikiObject::Node(row) => ("node", serde_json::to_value(row)),
        WikiObject::Frame(row) => ("frame", serde_json::to_value(row)),
        WikiObject::Space(row) => ("space", serde_json::to_value(row)),
        _ => {
            return Err(err(
                "facts target must be an existing Wiki node, whole frame or space",
            ))
        }
    };
    let mut object = value.map_err(|e| err(e.to_string()))?;
    object["object"] = json!(kind);
    let mut reading = json!({"schema":READING,"object":object,"native_owner":"ai-kit","actions":["aikit.wiki.facts.apply"]});
    if let WikiObject::Space(row) = object_of(&doc, reference)? {
        // The space's world body as the owner validates it: the record, or null while it carries none.
        reading["root_space"] = json!(nested_under(&doc, row).is_none());
        reading["world_body"] = serde_json::to_value(world_body_of(&row.extensions)?)
            .map_err(|e| err(e.to_string()))?;
    }
    Ok(reading)
}
pub const PLACES_READING: &str = "aikit.wiki-places/v1";
/// Every place one register's Wiki declares: each spatial facet on a node, a
/// whole frame or a space, with the object it stands on. A place with a
/// `body_ref` stands on that world body; without one it stands on the Earth.
/// An object whose facets cannot be read is named, never dropped. Reads only.
pub fn places(input: &str) -> Result<Value> {
    let doc = WikiDocument::parse(input)?;
    let mut places = Vec::new();
    let mut unreadable = Vec::new();
    for object in doc.objects() {
        let (kind, reference, title, extensions) = match object {
            WikiObject::Node(row) => ("node", &row.ref_id, row.title.clone(), &row.extensions),
            WikiObject::Frame(row) => ("frame", &row.ref_id, None, &row.extensions),
            WikiObject::Space(row) => ("space", &row.ref_id, row.title.clone(), &row.extensions),
            _ => continue,
        };
        match parse_facets_from_extensions(extensions) {
            Ok(facets) => {
                for place in facets.spatial {
                    let name = place
                        .identity
                        .as_ref()
                        .and_then(|identity| identity.names.first())
                        .map(|name| name.name.clone());
                    places.push(json!({
                        "place_ref": place.place_ref,
                        "name": name,
                        "stands_on": place.body_ref.clone().map_or(json!("earth"), |body| json!({"body_ref": body})),
                        "on": {"object": kind, "ref": reference, "title": title},
                        "place": place,
                    }));
                }
            }
            Err(error) => unreadable
                .push(json!({"object": kind, "ref": reference, "reason": error.to_string()})),
        }
    }
    Ok(
        json!({"schema":PLACES_READING,"native_owner":"ai-kit","places":places,"unreadable":unreadable}),
    )
}
fn object_of<'a>(doc: &'a WikiDocument, reference: &ResourceRef) -> Result<&'a WikiObject> {
    doc.object(reference)
        .ok_or_else(|| err("native facts target is absent"))
}
/// The space of the same register a space sits under, if any. A register's
/// root space has none here (a project's root names `central:wiki:root`, which
/// stands in a peer file).
fn nested_under<'a>(doc: &WikiDocument, space: &'a WikiSpace) -> Option<&'a ResourceRef> {
    space
        .parent_space_refs
        .iter()
        .find(|parent| matches!(doc.object(parent), Some(WikiObject::Space(_))))
}
pub fn apply(input: &str, request: &Request) -> Result<Applied> {
    for reference in [
        request.target.reference(),
        &request.actor_ref,
        &request.operation_ref,
    ] {
        ResourceRef::parse(reference.as_str())?;
    }
    if request.schema != ACTION
        || request.expected_revision == 0
        || request.expected_revision >= 9_007_199_254_740_991
        || request.changes.is_empty()
        || request.changes.len() > 2
    {
        return Err(err(
            "facts require an existing exact target revision and one change per facet family",
        ));
    }
    if request.changes.len() == 2
        && std::mem::discriminant(&request.changes[0])
            == std::mem::discriminant(&request.changes[1])
    {
        return Err(err(
            "a facet family may be replaced only once per operation",
        ));
    }
    require_pairing(&request.target, &request.changes)?;
    let digest = blake3::hash(
        serde_json::to_string(request)
            .map_err(|e| err(e.to_string()))?
            .as_bytes(),
    )
    .to_hex()
    .to_string();
    let doc = WikiDocument::parse(input)?;
    let mut object = doc
        .object(request.target.reference())
        .cloned()
        .ok_or_else(|| err("native facts target is absent"))?;
    let revision = object.revision();
    if let WikiObject::Space(row) = &object {
        if let Some(parent) = nested_under(&doc, row) {
            return Err(err(format!(
                "a world body belongs on the register's root space; {} sits under {parent} in the same register",
                row.ref_id
            )));
        }
    }
    let ext = extensions(&mut object, &request.target)?;
    require_writable(ext)?;
    let mut receipts: BTreeMap<String, Receipt> = ext
        .get(RECEIPTS)
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| err(e.to_string())))
        .transpose()?
        .unwrap_or_default();
    if let Some(prior) = receipts.get(request.operation_ref.as_str()) {
        if prior.request_digest != digest || prior.actor_ref != request.actor_ref {
            return Err(err(
                "operation identity was reused with different content or attribution",
            ));
        }
        return Ok(Applied {
            target: request.target.clone(),
            revision,
            idempotent: true,
            content: input.into(),
            objects_changed: vec![],
            warnings: vec![],
        });
    }
    if revision != request.expected_revision {
        return Err(conflict());
    }
    if receipts.len() >= 4096 {
        return Err(err(
            "fact operation receipt budget reached; preserve history before continuing",
        ));
    }
    for change in &request.changes {
        replace(ext, change)?;
    }
    receipts.insert(
        request.operation_ref.to_string(),
        Receipt {
            actor_ref: request.actor_ref.clone(),
            request_digest: digest,
            basis_revision: revision,
            result_revision: revision + 1,
        },
    );
    ext.insert(
        RECEIPTS.into(),
        serde_json::to_value(receipts).map_err(|e| err(e.to_string()))?,
    );
    let (content, outcome) = apply_wiki_mutation(input, |doc, ledger| {
        if doc
            .object(request.target.reference())
            .is_none_or(|o| o.revision() != request.expected_revision)
        {
            return Err(conflict());
        }
        ledger.record(doc.update_object(object)?);
        Ok(())
    })?;
    Ok(Applied {
        target: request.target.clone(),
        revision: revision + 1,
        idempotent: false,
        content,
        objects_changed: outcome.touched.into_iter().map(|t| t.resource).collect(),
        warnings: outcome.warnings,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    // A register as the root wiki stands (Control/agents/wiki/wiki.json): its root space, a child
    // space under it, and a node; the root space keeps an anchor and an unknown extension.
    fn register() -> String {
        json!({"profile":"okf-wiki/v1","objects":[
            {"object":"space","profile":"okf-wiki/v1","ref":"central:wiki:root","revision":3,
             "child_space_refs":["wiki:space:child"],"node_refs":["wiki:a"],"anchor_ref":"wiki:a","native_extra":{"kept":true}},
            {"object":"space","profile":"okf-wiki/v1","ref":"wiki:space:child","revision":1,"parent_space_refs":["central:wiki:root"]},
            {"object":"node","profile":"okf-wiki/v1","ref":"wiki:a","revision":1,"type":"Document","space_refs":["central:wiki:root"]}
        ]})
        .to_string()
    }
    fn body(seed: u32) -> Value {
        json!({"body_ref":"body:central","seed":seed,"generator":{"id":"fbm-sphere","version":1},
            "landmass":{"sea_level":0.5,"continents":1.7,"relief":0.55,"coast":"smooth"},
            "climate":{"warmth":0.58,"moisture":0.6,"ice_caps":0.3},
            "sky":{"atmosphere":0.6,"clouds":0.35,"axial_tilt_deg":18.0}})
    }
    fn request(target: Value, revision: u64, operation: &str, changes: Value) -> Request {
        serde_json::from_value(
            json!({"schema":ACTION,"target":target,"expected_revision":revision,
            "actor_ref":"human:author","operation_ref":operation,"changes":changes}),
        )
        .unwrap()
    }
    fn root() -> Value {
        json!({"kind":"space","ref":"central:wiki:root"})
    }
    fn body_set(body: Value) -> Value {
        json!([{"change":"body_set","body":body}])
    }
    fn reading(content: &str) -> Value {
        inspect(content, &ResourceRef::parse("central:wiki:root").unwrap()).unwrap()
    }
    fn refusal(content: &str, request: &Request) -> String {
        apply(content, request).unwrap_err().to_string()
    }

    #[test]
    fn body_set_writes_the_root_space_with_cas_and_a_receipt_and_replays_idempotently() {
        assert_eq!(reading(&register())["world_body"], Value::Null);
        assert_eq!(reading(&register())["root_space"], true);
        let command = request(root(), 3, "operation:body", body_set(body(20261018)));
        let saved = apply(&register(), &command).unwrap();
        assert_eq!(saved.revision, 4);
        assert!(!saved.idempotent);
        let read = reading(&saved.content);
        assert_eq!(read["world_body"], body(20261018));
        assert_eq!(read["object"][WORLD_BODY_EXTENSION], body(20261018));
        assert_eq!(read["object"]["revision"], 4);
        assert_eq!(
            read["object"][RECEIPTS]["operation:body"]["result_revision"],
            4
        );
        // Everything else the row carries keeps its exact value.
        assert_eq!(read["object"]["anchor_ref"], "wiki:a");
        assert_eq!(read["object"]["native_extra"], json!({"kept":true}));
        let replay = apply(&saved.content, &command).unwrap();
        assert!(replay.idempotent);
        assert_eq!(replay.content, saved.content, "replay is byte-exact");
        let stale = request(root(), 3, "operation:stale", body_set(body(7)));
        assert!(refusal(&saved.content, &stale).contains("reread"));
    }

    #[test]
    fn a_reroll_is_a_body_set_with_a_new_seed_and_nothing_else_moves() {
        let first = apply(
            &register(),
            &request(root(), 3, "operation:build", body_set(body(1))),
        )
        .unwrap();
        let rerolled = apply(
            &first.content,
            &request(root(), 4, "operation:reroll", body_set(body(2))),
        )
        .unwrap();
        let before = reading(&first.content)["world_body"].clone();
        let after = reading(&rerolled.content)["world_body"].clone();
        assert_eq!(after["seed"], 2);
        for key in ["body_ref", "generator", "landmass", "climate", "sky"] {
            assert_eq!(after[key], before[key], "{key} is unchanged by a reroll");
        }
        let old = WikiDocument::parse(&first.content).unwrap();
        let new = WikiDocument::parse(&rerolled.content).unwrap();
        for other in ["wiki:a", "wiki:space:child"] {
            let reference = ResourceRef::parse(other).unwrap();
            assert_eq!(old.object(&reference), new.object(&reference));
        }
    }

    #[test]
    fn out_of_range_settings_are_refused_in_plain_words() {
        let cases: [(&str, Value, &str); 9] = [
            (
                "/landmass/sea_level",
                json!(1.5),
                "sea level is 1.5; it runs from 0 to 1",
            ),
            (
                "/landmass/continents",
                json!(0.5),
                "continents is 0.5; it runs from 0.6 to 3.6",
            ),
            ("/landmass/relief", json!(-0.1), "relief is -0.1"),
            ("/climate/ice_caps", json!(2), "ice caps is 2"),
            ("/sky/clouds", json!(1.01), "clouds is 1.01"),
            (
                "/sky/axial_tilt_deg",
                json!(50),
                "axial tilt (degrees) is 50; it runs from 0 to 45",
            ),
            (
                "/generator/id",
                json!("perlin"),
                "the generator `perlin` is not one this owner knows",
            ),
            (
                "/generator/version",
                json!(0),
                "the generator version starts at 1",
            ),
            (
                "/landing",
                json!({"on_ref":"body:central","lat":0,"lon":0}),
                "a body cannot land on itself",
            ),
        ];
        for (pointer, value, words) in cases {
            let mut wrong = body(9);
            match wrong.pointer_mut(pointer) {
                Some(slot) => *slot = value,
                None => {
                    wrong["landing"] = value;
                }
            }
            let message = refusal(
                &register(),
                &request(root(), 3, "operation:wrong", body_set(wrong)),
            );
            assert!(message.contains(words), "{pointer}: {message}");
        }
        let mut far = body(9);
        far["landing"] = json!({"on_ref":"body:central-root","lat":95.0,"lon":0});
        assert!(refusal(
            &register(),
            &request(root(), 3, "operation:far", body_set(far))
        )
        .contains("the landing's latitude is 95"));
        let mut unknown = body(9);
        unknown["mood"] = json!("stormy");
        assert!(serde_json::from_value::<Request>(json!({"schema":ACTION,"target":root(),"expected_revision":3,
            "actor_ref":"human:author","operation_ref":"operation:unknown","changes":body_set(unknown)})).is_err());
        let mut coast = body(9);
        coast["landmass"]["coast"] = json!("jagged");
        assert!(serde_json::from_value::<Request>(
            json!({"schema":ACTION,"target":root(),"expected_revision":3,
            "actor_ref":"human:author","operation_ref":"operation:coast","changes":body_set(coast)})
        )
        .is_err());
    }

    #[test]
    fn a_body_stands_only_on_a_registers_root_space_and_a_space_takes_nothing_else() {
        let node = request(
            json!({"kind":"node","ref":"wiki:a"}),
            1,
            "operation:node",
            body_set(body(3)),
        );
        assert!(refusal(&register(), &node).contains("belongs on a register's root space"));
        let child = request(
            json!({"kind":"space","ref":"wiki:space:child"}),
            1,
            "operation:child",
            body_set(body(3)),
        );
        assert!(refusal(&register(), &child).contains("sits under central:wiki:root"));
        let places = request(
            root(),
            3,
            "operation:places",
            json!([{"change":"place_set","places":[]}]),
        );
        assert!(refusal(&register(), &places).contains("carries only its world body"));
        let twice = request(
            root(),
            3,
            "operation:twice",
            json!([{"change":"body_set","body":body(1)},{"change":"body_set","body":body(2)}]),
        );
        assert!(refusal(&register(), &twice).contains("only once per operation"));
        let wrong_kind = request(
            json!({"kind":"whole","ref":"central:wiki:root"}),
            3,
            "operation:kind",
            json!([{"change":"temporal_set","temporal":[]}]),
        );
        assert!(refusal(&register(), &wrong_kind).contains("does not match"));
    }

    #[test]
    fn a_place_names_the_body_it_stands_on_and_absent_it_stands_on_the_earth() {
        // A location on your own body: the PlaceSet carries the body's ref, kept verbatim and read back; a place
        // with no body_ref is an Earth place and serialises exactly as before bodies existed.
        let on_body = json!([{"change":"place_set","places":[
            {"place_ref":"wiki:place:study","precision":"approximate","source_ref":"central:file:notes",
             "geometry":{"type":"point","coordinates":[10.0,22.0]},"identity":{"names":[{"name":"The study"}]},
             "body_ref":"body:central:wiki:root"},
            {"place_ref":"wiki:place:athens","precision":"exact","source_ref":"central:file:notes",
             "geometry":{"type":"point","coordinates":[23.72,37.98]}}
        ]}]);
        let saved = apply(
            &register(),
            &request(
                json!({"kind":"node","ref":"wiki:a"}),
                1,
                "operation:study",
                on_body,
            ),
        )
        .unwrap();
        let read = inspect(&saved.content, &ResourceRef::parse("wiki:a").unwrap()).unwrap();
        let spatial = &read["object"]["aikit.techne-facet/v1"]["spatial"];
        assert_eq!(spatial[0]["body_ref"], "body:central:wiki:root");
        assert!(
            spatial[1].get("body_ref").is_none(),
            "an Earth place carries no body_ref: absence is the Earth"
        );
        let blank = request(
            json!({"kind":"node","ref":"wiki:a"}),
            1,
            "operation:blank",
            json!([{"change":"place_set","places":[
                {"place_ref":"wiki:place:x","precision":"approximate","source_ref":"central:file:notes","body_ref":"  "}
            ]}]),
        );
        assert!(refusal(&register(), &blank).contains("place.body_ref"));
    }

    #[test]
    fn a_stop_of_a_route_carries_its_relation_and_its_place_in_the_order() {
        // A route is a subject's places of one relation in order (O-I #746; DLW manual §17.2 "choose Route as the place kind and
        // join them in order"): each stop names the relation verbatim and its place in the order; both are read back as written.
        // A place without them is a place as before and serialises exactly as it did.
        let route = json!([{"change":"place_set","places":[
            {"place_ref":"wiki:place:athens","precision":"approximate","source_ref":"central:file:notes",
             "geometry":{"type":"point","coordinates":[23.72,37.98]},"relation":"TRAVELLED_TO","sequence":2},
            {"place_ref":"wiki:place:syracuse","precision":"approximate","source_ref":"central:file:notes",
             "geometry":{"type":"point","coordinates":[15.29,37.08]},"relation":"TRAVELLED_TO","sequence":1},
            {"place_ref":"wiki:place:home","precision":"exact","source_ref":"central:file:notes"}
        ]}]);
        let saved = apply(
            &register(),
            &request(
                json!({"kind":"node","ref":"wiki:a"}),
                1,
                "operation:route",
                route,
            ),
        )
        .unwrap();
        let read = inspect(&saved.content, &ResourceRef::parse("wiki:a").unwrap()).unwrap();
        let spatial = &read["object"]["aikit.techne-facet/v1"]["spatial"];
        assert_eq!(spatial[0]["relation"], "TRAVELLED_TO");
        assert_eq!(spatial[0]["sequence"], 2);
        assert_eq!(spatial[1]["sequence"], 1);
        assert!(
            spatial[2].get("relation").is_none() && spatial[2].get("sequence").is_none(),
            "a place that is no stop carries neither"
        );
        let stops = |places: Value| {
            request(
                json!({"kind":"node","ref":"wiki:a"}),
                1,
                "operation:bad-route",
                json!([{"change":"place_set","places":places}]),
            )
        };
        let unrelated = stops(json!([
            {"place_ref":"wiki:place:x","precision":"approximate","source_ref":"central:file:notes","sequence":1}
        ]));
        assert!(
            refusal(&register(), &unrelated).contains("place.sequence"),
            "an order is an order within a relation's route"
        );
        let blank = stops(json!([
            {"place_ref":"wiki:place:x","precision":"approximate","source_ref":"central:file:notes","relation":" "}
        ]));
        assert!(refusal(&register(), &blank).contains("place.relation"));
        let twice = stops(json!([
            {"place_ref":"wiki:place:x","precision":"approximate","source_ref":"central:file:notes","relation":"TRAVELLED_TO","sequence":1},
            {"place_ref":"wiki:place:y","precision":"approximate","source_ref":"central:file:notes","relation":"TRAVELLED_TO","sequence":1}
        ]));
        assert!(
            refusal(&register(), &twice).contains("same place in the order"),
            "two stops of one route never hold one place in its order"
        );
        let other_routes = stops(json!([
            {"place_ref":"wiki:place:x","precision":"approximate","source_ref":"central:file:notes","relation":"TRAVELLED_TO","sequence":1},
            {"place_ref":"wiki:place:y","precision":"approximate","source_ref":"central:file:notes","relation":"OPERATED_IN","sequence":1}
        ]));
        apply(&register(), &other_routes).expect("each relation's route keeps its own order");
    }

    #[test]
    fn a_project_registers_root_space_takes_its_body_under_the_peer_root() {
        let project = json!({"profile":"okf-wiki/v1","objects":[
            {"object":"space","profile":"okf-wiki/v1","ref":"central:wiki:project:O-I","revision":2,"parent_space_refs":["central:wiki:root"]}
        ]})
        .to_string();
        let mut landed = body(11);
        landed["body_ref"] = json!("body:o-i");
        landed["landing"] = json!({"on_ref":"body:central","lat":12.5,"lon":-40.25});
        let saved = apply(
            &project,
            &request(
                json!({"kind":"space","ref":"central:wiki:project:O-I"}),
                2,
                "operation:land",
                body_set(landed.clone()),
            ),
        )
        .unwrap();
        let read = inspect(
            &saved.content,
            &ResourceRef::parse("central:wiki:project:O-I").unwrap(),
        )
        .unwrap();
        assert_eq!(read["root_space"], true);
        assert_eq!(read["world_body"], landed);
    }
}
