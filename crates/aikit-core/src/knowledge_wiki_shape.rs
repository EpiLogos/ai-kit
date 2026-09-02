//! Canonical QL shape operation over SemanticWiki / Living Knowledge.
//!
//! AIKit consumes the portable deterministic shape contract owned by QL-MEF.
//! This is not a second QL ontology and does not require a live QL-MEF provider.
//! Shape addresses identify where relations may be inspected or generated; they
//! never assert semantic edges merely by existing.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::knowledge_living::{
    ContemplateGenerated, ContemplateOutcome, ContemplateRequest, IntegrativeWikiReading,
};
use crate::knowledge_living_context::{
    bounded_contemplate_preflight, explicit_bounded_contemplate, BoundedContemplateExecutor,
    BoundedContemplatePreflight,
};
use crate::knowledge_living_relations::KnowledgeResourceDependency;
use crate::knowledge_wiki::{WikiConstellation, WikiConstellationMember, WikiObject};
use crate::resource::ResourceRef;
use crate::{AikitError, Result};

pub const WIKI_QL_SHAPE_VERSION: &str = "aikit.wiki-ql-shape/v1";
pub const QL_SHAPE_CONTRACT_VERSION: &str = "1.0.0";
pub const QL_SHAPE_CONTRACT_REF: &str = "ql.shape@1.0.0";
pub const QL_SHAPE_UPSTREAM_BLOB: &str = "9056a460a9ce25e727cab83e6db25355e60a0b85";
pub const QL_STRUCTURAL_CONTRACT_VERSION: &str = "2.0.0";
pub const QL_SIX_BY_SIX_SHAPE_REF: &str = "ql:shape:1.0.0:6x6:direct-conjugate";
pub const QL_RELATIONAL_SIXFOLD_SHAPE_REF: &str = "ql:shape:1.0.0:6-plus-6-prime";
pub const QL_RELATIONAL_SIXFOLD_OPERATOR_REF: &str =
    "ql:shape:1.0.0:generation:same-position-direct-conjugate";
pub const QL_RELATIONAL_GENERATION_EXTENSION: &str = "aikit.ql-relational-generation/v1";
pub const DEFAULT_QL_SHAPE_BUDGET: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WikiQlRelationFamily {
    A,
    B,
    C,
}

impl WikiQlRelationFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
        }
    }

    pub const fn pairs(self) -> [(u8, u8); 3] {
        match self {
            Self::A => [(0, 1), (2, 3), (4, 5)],
            Self::B => [(1, 2), (3, 4), (5, 0)],
            Self::C => [(0, 5), (1, 4), (2, 3)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WikiQlConstellationGrain {
    AnchorOnly,
    TwoFold,
    ThreeFold123,
    ThreeFold450,
    FourFold1234,
    FourPlusOneGround,
    FourPlusOneSynthesis,
    SixFold,
    PartialConjugate8,
    PartialConjugate9,
    PartialConjugate10,
    PartialConjugate11,
    TwelveFold,
    Other { direct: u8, conjugate: u8 },
}

impl WikiQlConstellationGrain {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnchorOnly => "anchor-only",
            Self::TwoFold => "twofold",
            Self::ThreeFold123 => "threefold-123",
            Self::ThreeFold450 => "threefold-450",
            Self::FourFold1234 => "fourfold-1234",
            Self::FourPlusOneGround => "four-plus-one-ground",
            Self::FourPlusOneSynthesis => "four-plus-one-synthesis",
            Self::SixFold => "sixfold",
            Self::PartialConjugate8 => "partial-conjugate-8",
            Self::PartialConjugate9 => "partial-conjugate-9",
            Self::PartialConjugate10 => "partial-conjugate-10",
            Self::PartialConjugate11 => "partial-conjugate-11",
            Self::TwelveFold => "twelvefold",
            Self::Other { .. } => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WikiQlShapeKind {
    Constellation,
    FourByFour,
    SixBySix,
    RelationalSixfold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiQlCoordinate {
    pub resource: ResourceRef,
    pub position: u8,
    pub conjugate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiQlShapeAddress {
    pub row: WikiQlCoordinate,
    pub column: WikiQlCoordinate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiQlGenerationSite {
    pub position: u8,
    pub direct_ref: ResourceRef,
    pub conjugate_ref: ResourceRef,
    pub operator_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiQlShapeField {
    pub contract_ref: String,
    pub upstream_contract_blob: String,
    pub anchor_ref: ResourceRef,
    pub kind: WikiQlShapeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<WikiQlConstellationGrain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<WikiQlRelationFamily>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair_index: Option<u8>,
    #[serde(default)]
    pub coordinates: Vec<WikiQlCoordinate>,
    #[serde(default)]
    pub addresses: Vec<WikiQlShapeAddress>,
    #[serde(default)]
    pub generation_sites: Vec<WikiQlGenerationSite>,
    pub return_anchor_ref: ResourceRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QlShapedContemplatePreflight {
    pub version: String,
    pub base: BoundedContemplatePreflight,
    #[serde(default)]
    pub shapes: Vec<WikiQlShapeField>,
    pub shape_budget: usize,
    pub truncated: bool,
    /// Shape assembly remains deterministic and never opens the Agent/model aperture.
    pub automatic_agent_or_model_invocation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlRelationalGenerationAttribution {
    pub contract_ref: String,
    pub source_shape_ref: String,
    pub operator_ref: String,
    pub frame_ref: ResourceRef,
    #[serde(default)]
    pub basis_refs: Vec<ResourceRef>,
    #[serde(default)]
    pub generation_positions: Vec<u8>,
    pub generated_ref: ResourceRef,
    pub return_anchor_ref: ResourceRef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QlShapedContemplateOutcome {
    pub preflight: QlShapedContemplatePreflight,
    pub outcome: ContemplateOutcome,
}

pub trait QlShapedContemplateExecutor {
    /// Explicit Agent/model execution over the bounded deterministic Wiki + QL shape field.
    fn execute(&mut self, preflight: &QlShapedContemplatePreflight) -> Result<ContemplateGenerated>;
}

fn positioned_members(
    constellation: &WikiConstellation,
) -> Result<BTreeMap<(u8, bool), &WikiConstellationMember>> {
    constellation.validate()?;
    let mut members = BTreeMap::new();
    for member in &constellation.members {
        let Some(position) = member.position else {
            continue;
        };
        members.insert((position, member.conjugate), member);
    }
    for position in 0_u8..6 {
        if members.contains_key(&(position, true)) && !members.contains_key(&(position, false)) {
            return Err(AikitError::new(
                "knowledge.wiki_ql_shape_conjugate_without_direct",
                "QL shape parity requires every conjugate coordinate to retain its same-position direct basis",
            )
            .with("anchor", constellation.anchor_ref.to_string())
            .with("position", position.to_string()));
        }
    }
    Ok(members)
}

fn coordinate(member: &WikiConstellationMember) -> WikiQlCoordinate {
    WikiQlCoordinate {
        resource: member.ref_id.clone(),
        position: member.position.expect("positioned member"),
        conjugate: member.conjugate,
    }
}

fn cartesian_addresses(
    rows: &[WikiQlCoordinate],
    columns: &[WikiQlCoordinate],
) -> Vec<WikiQlShapeAddress> {
    let mut addresses = Vec::with_capacity(rows.len() * columns.len());
    for row in rows {
        for column in columns {
            addresses.push(WikiQlShapeAddress {
                row: row.clone(),
                column: column.clone(),
            });
        }
    }
    addresses
}

pub fn wiki_constellation_grain(
    constellation: &WikiConstellation,
) -> Result<WikiQlConstellationGrain> {
    let members = positioned_members(constellation)?;
    let direct = (0_u8..6)
        .filter(|position| members.contains_key(&(*position, false)))
        .collect::<Vec<_>>();
    let conjugate = (0_u8..6)
        .filter(|position| members.contains_key(&(*position, true)))
        .collect::<Vec<_>>();

    if direct.is_empty() && conjugate.is_empty() {
        return Ok(WikiQlConstellationGrain::AnchorOnly);
    }
    if conjugate.is_empty() {
        return Ok(match direct.as_slice() {
            [_, _] => WikiQlConstellationGrain::TwoFold,
            [1, 2, 3] => WikiQlConstellationGrain::ThreeFold123,
            [0, 4, 5] => WikiQlConstellationGrain::ThreeFold450,
            [1, 2, 3, 4] => WikiQlConstellationGrain::FourFold1234,
            [0, 1, 2, 3, 4] => WikiQlConstellationGrain::FourPlusOneGround,
            [1, 2, 3, 4, 5] => WikiQlConstellationGrain::FourPlusOneSynthesis,
            [0, 1, 2, 3, 4, 5] => WikiQlConstellationGrain::SixFold,
            _ => WikiQlConstellationGrain::Other {
                direct: direct.len() as u8,
                conjugate: 0,
            },
        });
    }
    if direct.as_slice() == [0, 1, 2, 3, 4, 5] {
        return Ok(match conjugate.len() {
            2 => WikiQlConstellationGrain::PartialConjugate8,
            3 => WikiQlConstellationGrain::PartialConjugate9,
            4 => WikiQlConstellationGrain::PartialConjugate10,
            5 => WikiQlConstellationGrain::PartialConjugate11,
            6 => WikiQlConstellationGrain::TwelveFold,
            _ => WikiQlConstellationGrain::Other {
                direct: 6,
                conjugate: conjugate.len() as u8,
            },
        });
    }
    Ok(WikiQlConstellationGrain::Other {
        direct: direct.len() as u8,
        conjugate: conjugate.len() as u8,
    })
}

pub fn wiki_ql_shape_fields(constellation: &WikiConstellation) -> Result<Vec<WikiQlShapeField>> {
    let members = positioned_members(constellation)?;
    let grain = wiki_constellation_grain(constellation)?;
    let mut fields = vec![WikiQlShapeField {
        contract_ref: QL_SHAPE_CONTRACT_REF.into(),
        upstream_contract_blob: QL_SHAPE_UPSTREAM_BLOB.into(),
        anchor_ref: constellation.anchor_ref.clone(),
        kind: WikiQlShapeKind::Constellation,
        grain: Some(grain),
        shape_ref: None,
        derivation_ref: None,
        family: None,
        pair_index: None,
        coordinates: members.values().map(|member| coordinate(member)).collect(),
        addresses: Vec::new(),
        generation_sites: Vec::new(),
        return_anchor_ref: constellation.anchor_ref.clone(),
    }];

    for family in [
        WikiQlRelationFamily::A,
        WikiQlRelationFamily::B,
        WikiQlRelationFamily::C,
    ] {
        for (pair_index, (left, right)) in family.pairs().into_iter().enumerate() {
            let required = [
                (left, false),
                (right, false),
                (left, true),
                (right, true),
            ];
            let Some(axis) = required
                .iter()
                .map(|key| members.get(key).copied())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let coordinates = axis.into_iter().map(coordinate).collect::<Vec<_>>();
            fields.push(WikiQlShapeField {
                contract_ref: QL_SHAPE_CONTRACT_REF.into(),
                upstream_contract_blob: QL_SHAPE_UPSTREAM_BLOB.into(),
                anchor_ref: constellation.anchor_ref.clone(),
                kind: WikiQlShapeKind::FourByFour,
                grain: None,
                shape_ref: Some(format!(
                    "ql:shape:{QL_SHAPE_CONTRACT_VERSION}:4x4:{}:{pair_index}",
                    family.as_str()
                )),
                derivation_ref: Some(format!(
                    "ql:structural:{QL_STRUCTURAL_CONTRACT_VERSION}:field:{}:{pair_index}:D3",
                    family.as_str()
                )),
                family: Some(family),
                pair_index: Some(pair_index as u8),
                addresses: cartesian_addresses(&coordinates, &coordinates),
                coordinates,
                generation_sites: Vec::new(),
                return_anchor_ref: constellation.anchor_ref.clone(),
            });
        }
    }

    let complete = (0_u8..6).all(|position| {
        members.contains_key(&(position, false)) && members.contains_key(&(position, true))
    });
    if complete {
        let direct = (0_u8..6)
            .map(|position| coordinate(members[&(position, false)]))
            .collect::<Vec<_>>();
        let conjugate = (0_u8..6)
            .map(|position| coordinate(members[&(position, true)]))
            .collect::<Vec<_>>();
        let mut coordinates = direct.clone();
        coordinates.extend(conjugate.clone());
        fields.push(WikiQlShapeField {
            contract_ref: QL_SHAPE_CONTRACT_REF.into(),
            upstream_contract_blob: QL_SHAPE_UPSTREAM_BLOB.into(),
            anchor_ref: constellation.anchor_ref.clone(),
            kind: WikiQlShapeKind::SixBySix,
            grain: None,
            shape_ref: Some(QL_SIX_BY_SIX_SHAPE_REF.into()),
            derivation_ref: None,
            family: None,
            pair_index: None,
            coordinates: coordinates.clone(),
            addresses: cartesian_addresses(&direct, &conjugate),
            generation_sites: Vec::new(),
            return_anchor_ref: constellation.anchor_ref.clone(),
        });
        fields.push(WikiQlShapeField {
            contract_ref: QL_SHAPE_CONTRACT_REF.into(),
            upstream_contract_blob: QL_SHAPE_UPSTREAM_BLOB.into(),
            anchor_ref: constellation.anchor_ref.clone(),
            kind: WikiQlShapeKind::RelationalSixfold,
            grain: None,
            shape_ref: Some(QL_RELATIONAL_SIXFOLD_SHAPE_REF.into()),
            derivation_ref: Some(QL_RELATIONAL_SIXFOLD_OPERATOR_REF.into()),
            family: None,
            pair_index: None,
            coordinates,
            addresses: Vec::new(),
            generation_sites: (0_u8..6)
                .map(|position| WikiQlGenerationSite {
                    position,
                    direct_ref: members[&(position, false)].ref_id.clone(),
                    conjugate_ref: members[&(position, true)].ref_id.clone(),
                    operator_ref: format!(
                        "{QL_RELATIONAL_SIXFOLD_OPERATOR_REF}:position-{position}"
                    ),
                })
                .collect(),
            return_anchor_ref: constellation.anchor_ref.clone(),
        });
    }

    Ok(fields)
}

fn frame_is_relevant(frame: &crate::knowledge_wiki::WikiFrame, relevant: &BTreeSet<ResourceRef>) -> bool {
    relevant.contains(&frame.ref_id)
        || frame
            .inquiry_ref
            .as_ref()
            .is_some_and(|value| relevant.contains(value))
        || frame.member_refs.iter().any(|value| relevant.contains(value))
        || frame.constellations.iter().any(|constellation| {
            relevant.contains(&constellation.anchor_ref)
                || constellation
                    .members
                    .iter()
                    .any(|member| relevant.contains(&member.ref_id))
        })
}

pub fn ql_shaped_contemplate_preflight(
    request: &ContemplateRequest<'_>,
    resource_dependencies: &[KnowledgeResourceDependency],
    max_objects: usize,
    relation_depth: usize,
    shape_budget: usize,
) -> Result<QlShapedContemplatePreflight> {
    if shape_budget == 0 {
        return Err(AikitError::new(
            "knowledge.wiki_ql_shape_budget_invalid",
            "QL shape budget must be greater than zero",
        ));
    }
    let base = bounded_contemplate_preflight(
        request,
        resource_dependencies,
        max_objects,
        relation_depth,
    )?;
    let relevant = base
        .field
        .objects
        .iter()
        .map(|object| object.resource.clone())
        .chain(base.field.focus.iter().cloned())
        .collect::<BTreeSet<_>>();

    let mut shapes = Vec::new();
    let mut truncated = false;
    for object in request.current_wiki_objects {
        let WikiObject::Frame(frame) = object else {
            continue;
        };
        if !frame_is_relevant(frame, &relevant) {
            continue;
        }
        for constellation in &frame.constellations {
            for shape in wiki_ql_shape_fields(constellation)? {
                if shapes.len() >= shape_budget {
                    truncated = true;
                    break;
                }
                shapes.push(shape);
            }
            if truncated {
                break;
            }
        }
        if truncated {
            break;
        }
    }

    Ok(QlShapedContemplatePreflight {
        version: WIKI_QL_SHAPE_VERSION.into(),
        base,
        shapes,
        shape_budget,
        truncated,
        automatic_agent_or_model_invocation: false,
    })
}

pub fn attribute_ql_relational_generation(
    mut reading: IntegrativeWikiReading,
    mut attribution: QlRelationalGenerationAttribution,
) -> Result<IntegrativeWikiReading> {
    if attribution.contract_ref.trim().is_empty() {
        attribution.contract_ref = QL_SHAPE_CONTRACT_REF.into();
    }
    if attribution.contract_ref != QL_SHAPE_CONTRACT_REF {
        return Err(AikitError::new(
            "knowledge.wiki_ql_shape_contract_mismatch",
            "relational generation must name the pinned QL shape contract",
        ));
    }
    if attribution.generated_ref != reading.reading.ref_id {
        return Err(AikitError::new(
            "knowledge.wiki_ql_generation_identity_mismatch",
            "generated relational determination must retain the WikiReading identity being attributed",
        ));
    }
    if attribution.frame_ref != reading.reading.frame_ref {
        return Err(AikitError::new(
            "knowledge.wiki_ql_generation_frame_mismatch",
            "generated relational determination must retain the WikiReading frame",
        ));
    }
    let available_basis = reading
        .basis
        .iter()
        .map(|basis| basis.resource.clone())
        .collect::<BTreeSet<_>>();
    if attribution
        .basis_refs
        .iter()
        .any(|basis| !available_basis.contains(basis))
    {
        return Err(AikitError::new(
            "knowledge.wiki_ql_generation_basis_missing",
            "relational generation basis must already be an attributable integrative-reading basis",
        ));
    }
    attribution.generation_positions.sort_unstable();
    attribution.generation_positions.dedup();
    if attribution
        .generation_positions
        .iter()
        .any(|position| *position > 5)
    {
        return Err(AikitError::new(
            "knowledge.wiki_ql_generation_position_invalid",
            "QL relational-generation positions must be 0..=5",
        ));
    }
    let value = serde_json::to_value(&attribution).map_err(|error| {
        AikitError::new(
            "knowledge.wiki_ql_generation_serialization_failed",
            format!("failed to serialize QL relational-generation attribution: {error}"),
        )
    })?;
    reading
        .reading
        .extensions
        .insert(QL_RELATIONAL_GENERATION_EXTENSION.into(), value);
    WikiObject::Reading(reading.reading.clone()).validate()?;
    Ok(reading)
}

struct QlShapedExecutorAdapter<'a> {
    shaped: &'a QlShapedContemplatePreflight,
    executor: &'a mut dyn QlShapedContemplateExecutor,
}

impl BoundedContemplateExecutor for QlShapedExecutorAdapter<'_> {
    fn execute(&mut self, preflight: &BoundedContemplatePreflight) -> Result<ContemplateGenerated> {
        if preflight != &self.shaped.base {
            return Err(AikitError::new(
                "knowledge.wiki_ql_shape_preflight_drift",
                "bounded Contemplate field changed between QL-shape assembly and explicit execution",
            ));
        }
        self.executor.execute(self.shaped)
    }
}

pub fn explicit_ql_shaped_contemplate(
    request: &ContemplateRequest<'_>,
    resource_dependencies: &[KnowledgeResourceDependency],
    max_objects: usize,
    relation_depth: usize,
    shape_budget: usize,
    executor: &mut dyn QlShapedContemplateExecutor,
) -> Result<QlShapedContemplateOutcome> {
    let preflight = ql_shaped_contemplate_preflight(
        request,
        resource_dependencies,
        max_objects,
        relation_depth,
        shape_budget,
    )?;
    let mut adapter = QlShapedExecutorAdapter {
        shaped: &preflight,
        executor,
    };
    let bounded = explicit_bounded_contemplate(
        request,
        resource_dependencies,
        max_objects,
        relation_depth,
        &mut adapter,
    )?;
    Ok(QlShapedContemplateOutcome {
        preflight,
        outcome: bounded.outcome,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_living::{KnowledgeFreshness, ReadingBasisNode};
    use crate::knowledge_wiki::{WikiConstellationReturn, WikiReading, OKF_WIKI_PROFILE};

    fn resource(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn member(position: u8, conjugate: bool) -> WikiConstellationMember {
        WikiConstellationMember {
            ref_id: resource(&format!(
                "wiki:node-{position}{}",
                if conjugate { "-prime" } else { "" }
            )),
            position: Some(position),
            conjugate,
            extensions: BTreeMap::new(),
        }
    }

    fn complete_constellation() -> WikiConstellation {
        let anchor = resource("wiki:anchor");
        let mut members = (0_u8..6)
            .map(|position| member(position, false))
            .collect::<Vec<_>>();
        members.extend((0_u8..6).map(|position| member(position, true)));
        WikiConstellation {
            anchor_ref: anchor.clone(),
            members,
            returns: vec![WikiConstellationReturn {
                through_anchor_ref: anchor.clone(),
                ground_ref: resource("wiki:ground"),
                extensions: BTreeMap::new(),
            }],
            conjugate_ref: None,
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn complete_constellation_exposes_positive_grain_nine_d3_fields_and_higher_shapes() {
        let shapes = wiki_ql_shape_fields(&complete_constellation()).unwrap();
        assert_eq!(shapes.len(), 12);
        assert_eq!(shapes[0].grain, Some(WikiQlConstellationGrain::TwelveFold));
        assert_eq!(
            shapes
                .iter()
                .filter(|shape| shape.kind == WikiQlShapeKind::FourByFour)
                .count(),
            9
        );
        assert!(shapes.iter().any(|shape| {
            shape.kind == WikiQlShapeKind::SixBySix && shape.addresses.len() == 36
        }));
        assert!(shapes.iter().any(|shape| {
            shape.kind == WikiQlShapeKind::RelationalSixfold
                && shape.generation_sites.len() == 6
        }));
    }

    #[test]
    fn equal_vertex_d3_routes_retain_a_and_c_provenance() {
        let shapes = wiki_ql_shape_fields(&complete_constellation()).unwrap();
        let a = shapes
            .iter()
            .find(|shape| {
                shape.family == Some(WikiQlRelationFamily::A) && shape.pair_index == Some(1)
            })
            .unwrap();
        let c = shapes
            .iter()
            .find(|shape| {
                shape.family == Some(WikiQlRelationFamily::C) && shape.pair_index == Some(2)
            })
            .unwrap();
        let a_refs = a
            .coordinates
            .iter()
            .map(|coordinate| coordinate.resource.clone())
            .collect::<BTreeSet<_>>();
        let c_refs = c
            .coordinates
            .iter()
            .map(|coordinate| coordinate.resource.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(a_refs, c_refs);
        assert_ne!(a.shape_ref, c.shape_ref);
        assert_ne!(a.derivation_ref, c.derivation_ref);
    }

    #[test]
    fn direct_sixfold_is_a_valid_shape_without_forced_conjugate_completion() {
        let anchor = resource("wiki:anchor");
        let constellation = WikiConstellation {
            anchor_ref: anchor,
            members: (0_u8..6)
                .map(|position| member(position, false))
                .collect(),
            returns: Vec::new(),
            conjugate_ref: None,
            extensions: BTreeMap::new(),
        };
        let shapes = wiki_ql_shape_fields(&constellation).unwrap();
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].grain, Some(WikiQlConstellationGrain::SixFold));
    }

    #[test]
    fn relational_generation_is_attributed_without_promoting_it_to_authored_ground() {
        let generated = resource("wiki:generated");
        let frame = resource("wiki:frame");
        let basis = resource("wiki:basis");
        let reading = IntegrativeWikiReading {
            reading: WikiReading {
                profile: OKF_WIKI_PROFILE.into(),
                ref_id: generated.clone(),
                revision: 1,
                provenance: Vec::new(),
                frame_ref: frame.clone(),
                reading_type: "integrative/relational-v1".into(),
                artifact_ref: None,
                derived_by_ref: None,
                extensions: BTreeMap::new(),
            },
            basis: vec![ReadingBasisNode {
                resource: basis.clone(),
                source: None,
                source_revision: None,
                roles: vec!["relational-basis".into()],
            }],
            relations: Vec::new(),
            return_paths: Vec::new(),
            freshness: KnowledgeFreshness::Fresh,
        };
        let attributed = attribute_ql_relational_generation(
            reading,
            QlRelationalGenerationAttribution {
                contract_ref: QL_SHAPE_CONTRACT_REF.into(),
                source_shape_ref: QL_RELATIONAL_SIXFOLD_SHAPE_REF.into(),
                operator_ref: QL_RELATIONAL_SIXFOLD_OPERATOR_REF.into(),
                frame_ref: frame,
                basis_refs: vec![basis],
                generation_positions: vec![0, 1],
                generated_ref: generated,
                return_anchor_ref: resource("wiki:anchor"),
            },
        )
        .unwrap();
        assert!(
            attributed
                .reading
                .extensions
                .contains_key(QL_RELATIONAL_GENERATION_EXTENSION)
        );
    }

    #[test]
    fn mirrored_fixture_pins_the_exact_upstream_blob_and_cardinalities() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../fixtures/knowledge/ql-shape-contract-v1.json"
        ))
        .unwrap();
        assert_eq!(fixture["upstream"]["blob"], QL_SHAPE_UPSTREAM_BLOB);
        assert_eq!(fixture["four_by_four"]["address_cardinality"], 16);
        assert_eq!(fixture["six_by_six"]["address_cardinality"], 36);
        assert_eq!(fixture["relational_sixfold"]["site_cardinality"], 6);
        assert_eq!(
            fixture["relational_sixfold"]["operator_ref"],
            QL_RELATIONAL_SIXFOLD_OPERATOR_REF
        );
    }
}
