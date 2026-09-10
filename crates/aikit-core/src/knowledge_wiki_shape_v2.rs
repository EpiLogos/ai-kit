//! Shape-contract wholesale consumption (W10 rev 3, V9): families A–C with
//! their D1/D2/D3 completion degrees always together, `shape_ref`
//! declarations on constellations validated through the contract (structural
//! floor only), node stance as declared data, and the 6+6′ compression
//! formation through the 0 // 1 trinity.
//!
//! Openness law: the wiki consumes shapes as versioned declared data and
//! validates only the structural floor — position range, unique-per-face,
//! conjugate-requires-direct, returns-through-declared-anchor with declared
//! ground kind. Unknown or unversioned shape refs are preserved like any
//! unknown extension; richer shape intelligence stays with QL-MEF. Engine
//! releases never hard-code a constellation typology.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::knowledge_wiki::{WikiConstellation, WikiNode};
use crate::knowledge_wiki_shape::{
    positioned_members_public, structural_ref_parts, wiki_constellation_grain_public,
    wiki_ql_shape_fields, QL_SHAPE_CONTRACT_REF, QL_SHAPE_UPSTREAM_BLOB,
    QL_STRUCTURAL_CONTRACT_VERSION, WIKI_QL_SHAPE_VERSION,
};
use crate::{AikitError, Result};

/// The declared node-stance extension (rev 3): a bare node is `1` (null
/// state — relations allowed, no QL organisation demanded); a QL-ready node
/// is a `0/1`. The paśu identity grammar (nara / agent / agent-set) is the
/// first typed family of 0/1 anchors. Stance is data, never an engine enum.
pub const QL_NODE_STANCE_EXTENSION: &str = "aikit.ql-stance/v1";
/// The declared per-constellation shape extension.
pub const QL_SHAPE_DECLARATION_EXTENSION: &str = "aikit.ql-shape/v1";

/// Degrees of square completion — the bases of compositional extensibility.
/// A family + pair + degree is one operator identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WikiQlCompletionDegree {
    /// 2-fold: the direct/conjugate pair alone.
    D1,
    /// 3-fold: the pair plus its same-position generated relation.
    D2,
    /// 4-fold: the completed square (the 4×4 field).
    D3,
}

impl WikiQlCompletionDegree {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::D1 => "D1",
            Self::D2 => "D2",
            Self::D3 => "D3",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "D1" => Some(Self::D1),
            "D2" => Some(Self::D2),
            "D3" => Some(Self::D3),
            _ => None,
        }
    }
}

/// The versioned structural operator ref with the concrete family letter and
/// pair index — e.g. `ql:structural:2.0.0:field:A:1:D3`. No abstract
/// FAMILY/PAIR placeholders.
pub fn structural_shape_ref(
    family: crate::knowledge_wiki_shape::WikiQlRelationFamily,
    pair_index: u8,
    degree: WikiQlCompletionDegree,
) -> String {
    format!(
        "ql:structural:{QL_STRUCTURAL_CONTRACT_VERSION}:field:{}:{pair_index}:{}",
        family.as_str(),
        degree.as_str()
    )
}

/// Node stance as declared data. Absence = undeclared (the node carries no
/// stance claim); a declared stance must be exactly `1` or `0/1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WikiQlNodeStance {
    /// The bare node: relations allowed, no QL organisation demanded.
    NullState,
    /// The QL-ready node: an anchor that can hold a whole.
    UnitWhole,
}

impl WikiQlNodeStance {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NullState => "1",
            Self::UnitWhole => "0/1",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "1" => Some(Self::NullState),
            "0/1" => Some(Self::UnitWhole),
            _ => None,
        }
    }
}

/// Read a node's declared stance. `Ok(None)` = not declared; a malformed
/// declaration is an error (declared data must be parseable).
pub fn wiki_node_stance(node: &WikiNode) -> Result<Option<WikiQlNodeStance>> {
    let Some(declared) = node.extensions.get(QL_NODE_STANCE_EXTENSION) else {
        return Ok(None);
    };
    let raw = declared
        .get("stance")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_invalid_stance",
                "node stance declaration carries no stance",
            )
        })?;
    WikiQlNodeStance::parse(raw).map(Some).ok_or_else(|| {
        AikitError::new(
            "knowledge.wiki_invalid_stance",
            format!("node stance `{raw}` is neither `1` nor `0/1`"),
        )
    })
}

/// The structural floor for a constellation that *declares* its shape.
/// Returns the validated shape fields the declaration names. Unknown
/// shape refs pass through untouched (preserved like any unknown
/// extension) — the contract only vouches for refs it can name.
pub fn validate_constellation_shape_declaration(
    constellation: &WikiConstellation,
) -> Result<Vec<crate::knowledge_wiki_shape::WikiQlShapeField>> {
    let Some(declaration) = constellation.extensions.get(QL_SHAPE_DECLARATION_EXTENSION) else {
        // An undeclared constellation still validates through the ordinary
        // structural floor when its fields are computed.
        return Ok(Vec::new());
    };

    let shape_ref = declaration
        .get("shape_ref")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AikitError::new(
                "knowledge.wiki_invalid_shape_declaration",
                "shape declaration carries no shape_ref",
            )
        })?;

    // Declared grain (if present) must agree with the computed grain.
    if let Some(declared_grain) = declaration.get("grain").and_then(Value::as_str) {
        let computed = wiki_constellation_grain_public(constellation)?;
        if computed.as_str() != declared_grain {
            return Err(AikitError::new(
                "knowledge.wiki_invalid_shape_declaration",
                format!(
                    "declared grain `{declared_grain}` does not match the computed grain `{}`",
                    computed.as_str()
                ),
            ));
        }
    }

    // Whole-anchor law: the anchor is the 0/1 whole, never a positioned
    // member — a seventh member would flatten the trinity into a ladder.
    if constellation
        .members
        .iter()
        .any(|member| member.ref_id == constellation.anchor_ref && member.position.is_some())
    {
        return Err(AikitError::new(
            "knowledge.wiki_invalid_shape_declaration",
            "the whole-anchor must not be a positioned member of its own constellation",
        ));
    }

    // A structural ref the contract can name must be constructible and the
    // named field must actually exist on this constellation.
    if let Some((family, pair_index, degree)) = structural_ref_parts(shape_ref) {
        let fields = wiki_ql_shape_fields(constellation)?;
        let expected = structural_shape_ref(family, pair_index, degree);
        if shape_ref != expected {
            return Err(AikitError::new(
                "knowledge.wiki_invalid_shape_declaration",
                format!("malformed structural shape ref `{shape_ref}` (expected `{expected}`)"),
            ));
        }
        let family_pairs = family.pairs();
        let Some(&(left, right)) = family_pairs.get(pair_index as usize) else {
            return Err(AikitError::new(
                "knowledge.wiki_invalid_shape_declaration",
                format!("family {} has no pair index {pair_index}", family.as_str()),
            ));
        };
        let positioned = positioned_members_public(constellation)?;
        let present = [(left, false), (right, false)]
            .iter()
            .all(|key| positioned.contains_key(key));
        if !present {
            return Err(AikitError::new(
                "knowledge.wiki_invalid_shape_declaration",
                format!("shape ref `{shape_ref}` names a pair this constellation does not carry"),
            ));
        }
        // The v1 fields carry the structural ref as their derivation_ref;
        // accept either seat so the same operator identity is found.
        return Ok(fields
            .into_iter()
            .filter(|field| {
                field.shape_ref.as_deref() == Some(shape_ref)
                    || field.derivation_ref.as_deref() == Some(shape_ref)
            })
            .collect());
    }

    // Unversioned or unknown refs: preserved, not vouched for.
    Ok(Vec::new())
}

/// The 6+6′ compression formation: two conjugate sixfolds married in the
/// six generated relations compress through the 0 // 1 trinity into a single
/// 0/1 encapsulation. This is declared-data assembly over the structural
/// floor, not a new ontology: everything reads back through the ordinary
/// shape fields and return canon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiQlSixFoldCompression {
    pub version: String,
    pub contract_ref: String,
    pub upstream_contract_blob: String,
    /// The whole the compression encapsulates.
    pub anchor_ref: crate::ResourceRef,
    /// Position-by-position direct/conjugate/generated triples (the 6+6′).
    pub composites: Vec<WikiQlComposite>,
    /// The trinity reading: `6 : 6+6′ : 6′ = 0 : / : 1`.
    pub trinity: WikiQlTrinity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WikiQlComposite {
    pub position: u8,
    pub direct_ref: crate::ResourceRef,
    pub conjugate_ref: crate::ResourceRef,
    pub generated_ref: crate::ResourceRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiQlTrinity {
    /// `0` — the operative face (6).
    pub zero: String,
    /// `/` — the generated articulation (6+6′).
    pub slash: String,
    /// `1` — the conjugate face (6′).
    pub one: String,
}

/// Compress a complete direct/conjugate sixfold + its generated relations
/// into the single 0/1 encapsulation. Structural floor only: positions
/// complete per face, generated relations present per position, returns
/// already validated through the declared anchor by the model.
pub fn compress_six_plus_six_prime(
    constellation: &WikiConstellation,
    generated_by_position: &BTreeMap<u8, crate::ResourceRef>,
) -> Result<WikiQlSixFoldCompression> {
    let members = positioned_members_public(constellation)?;
    let mut composites = Vec::new();
    for position in 0_u8..6 {
        let Some(direct) = members.get(&(position, false)) else {
            return Err(AikitError::new(
                "knowledge.wiki_compression_incomplete",
                format!("position {position} has no direct member"),
            ));
        };
        let Some(conjugate) = members.get(&(position, true)) else {
            return Err(AikitError::new(
                "knowledge.wiki_compression_incomplete",
                format!("position {position} has no conjugate member"),
            ));
        };
        let Some(generated) = generated_by_position.get(&position) else {
            return Err(AikitError::new(
                "knowledge.wiki_compression_incomplete",
                format!("position {position} has no generated relation"),
            ));
        };
        composites.push(WikiQlComposite {
            position,
            direct_ref: direct.ref_id.clone(),
            conjugate_ref: conjugate.ref_id.clone(),
            generated_ref: generated.clone(),
        });
    }
    Ok(WikiQlSixFoldCompression {
        version: WIKI_QL_SHAPE_VERSION.to_owned(),
        contract_ref: QL_SHAPE_CONTRACT_REF.to_owned(),
        upstream_contract_blob: QL_SHAPE_UPSTREAM_BLOB.to_owned(),
        anchor_ref: constellation.anchor_ref.clone(),
        composites,
        trinity: WikiQlTrinity {
            zero: "6".into(),
            slash: "6+6′".into(),
            one: "6′".into(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_wiki::{
        WikiConstellationMember, WikiConstellationReturn, OKF_WIKI_PROFILE,
    };
    use crate::{ResourceRef, SemanticRevision, WikiProvenanceRef};

    fn reference(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    /// The Arbitration cluster (Return of Zero, T09):
    /// `arbitration-hybris-regard-anamnesis` — 6 / 6′ with the six
    /// generated `Xi-through-Xi′` relations, anchor as the whole.
    fn arbitration_constellation() -> WikiConstellation {
        let direct = [
            "continuity",
            "criterion",
            "delineation",
            "arbitration",
            "con-text",
            "resolution",
        ];
        let conjugate = [
            "indeterminacy",
            "distinction",
            "difference",
            "crisis",
            "diaphaneity",
            "reconciliation",
        ];
        let mut members = Vec::new();
        for (position, name) in direct.iter().enumerate() {
            members.push(WikiConstellationMember {
                ref_id: reference(&format!("wiki:node:t09:{name}")),
                position: Some(position as u8),
                conjugate: false,
                extensions: BTreeMap::new(),
            });
        }
        for (position, name) in conjugate.iter().enumerate() {
            members.push(WikiConstellationMember {
                ref_id: reference(&format!("wiki:node:t09:{name}")),
                position: Some(position as u8),
                conjugate: true,
                extensions: BTreeMap::new(),
            });
        }
        let mut extensions = BTreeMap::new();
        extensions.insert(
            QL_SHAPE_DECLARATION_EXTENSION.to_owned(),
            serde_json::json!({
                "shape_ref": structural_shape_ref(
                    crate::knowledge_wiki_shape::WikiQlRelationFamily::A,
                    2,
                    WikiQlCompletionDegree::D3,
                ),
                "grain": "twelvefold"
            }),
        );
        WikiConstellation {
            anchor_ref: reference("wiki:node:t09:arbitration-whole"),
            members,
            returns: vec![WikiConstellationReturn {
                through_anchor_ref: reference("wiki:node:t09:arbitration-whole"),
                ground_ref: reference("wiki:node:t09:arbitration-whole"),
                ground_kind: Some("own".into()),
                extensions: BTreeMap::new(),
            }],
            conjugate_ref: None,
            extensions,
        }
    }

    fn member_node(name: &str) -> crate::WikiObject {
        let mut extensions = BTreeMap::new();
        extensions.insert(
            QL_NODE_STANCE_EXTENSION.to_owned(),
            serde_json::json!({"stance": "0/1"}),
        );
        crate::WikiObject::Node(WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: reference(&format!("wiki:node:t09:{name}")),
            revision: 1,
            provenance: vec![WikiProvenanceRef {
                source_ref: crate::SourceRef::parse("test/arbitration-cluster").unwrap(),
                source_revision: Some(SemanticRevision::Text("living".into())),
                producer_ref: None,
                generation_ref: None,
                extensions: BTreeMap::new(),
            }],
            node_type: "concept".into(),
            title: Some(name.to_owned()),
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions,
        })
    }

    #[test]
    fn structural_refs_are_concrete_per_family_pair_and_degree() {
        use crate::knowledge_wiki_shape::WikiQlRelationFamily;
        assert_eq!(
            structural_shape_ref(WikiQlRelationFamily::A, 1, WikiQlCompletionDegree::D3),
            "ql:structural:2.0.0:field:A:1:D3"
        );
        assert_eq!(
            structural_shape_ref(WikiQlRelationFamily::C, 0, WikiQlCompletionDegree::D1),
            "ql:structural:2.0.0:field:C:0:D1"
        );
        assert_eq!(
            WikiQlCompletionDegree::parse("D2").map(|d| d.as_str()),
            Some("D2")
        );
        assert_eq!(WikiQlCompletionDegree::parse("D4"), None);
    }

    #[test]
    fn node_stance_is_declared_data_never_an_engine_kind() {
        let node = match member_node("arbitration") {
            crate::WikiObject::Node(node) => node,
            _ => unreachable!(),
        };
        assert_eq!(
            wiki_node_stance(&node).unwrap(),
            Some(WikiQlNodeStance::UnitWhole)
        );
        // A bare node declares nothing; stance stays undeclared, not defaulted.
        let bare = WikiNode {
            profile: OKF_WIKI_PROFILE.into(),
            ref_id: reference("wiki:node:bare"),
            revision: 1,
            provenance: Vec::new(),
            node_type: "note".into(),
            title: None,
            space_refs: Vec::new(),
            source_refs: Vec::new(),
            local_space_ref: None,
            extensions: BTreeMap::new(),
        };
        assert_eq!(wiki_node_stance(&bare).unwrap(), None);
        // A malformed declaration is refused, not coerced.
        let mut broken = bare.clone();
        broken.extensions.insert(
            QL_NODE_STANCE_EXTENSION.to_owned(),
            serde_json::json!({"stance": "0.5"}),
        );
        assert!(wiki_node_stance(&broken).is_err());
    }

    #[test]
    fn constellation_shape_declaration_validates_through_the_contract() {
        let constellation = arbitration_constellation();
        let fields = validate_constellation_shape_declaration(&constellation).unwrap();
        assert_eq!(fields.len(), 1);
        // The field's v1 shape_ref and v2 derivation_ref are one operator
        // identity: family A, pair 2, the completed square.
        assert_eq!(
            fields[0].shape_ref.as_deref(),
            Some("ql:shape:1.0.0:4x4:A:2")
        );
        assert_eq!(
            fields[0].derivation_ref.as_deref(),
            Some("ql:structural:2.0.0:field:A:2:D3")
        );

        // A declared grain disagreeing with the computed grain is refused.
        let mut wrong = constellation.clone();
        wrong.extensions.insert(
            QL_SHAPE_DECLARATION_EXTENSION.to_owned(),
            serde_json::json!({"shape_ref": "ql:shape:1.0.0:6x6:direct-conjugate", "grain": "sixfold"}),
        );
        assert!(validate_constellation_shape_declaration(&wrong).is_err());

        // The whole-anchor may never be a seventh (positioned) member.
        let mut seventh = constellation.clone();
        seventh.members.push(WikiConstellationMember {
            ref_id: seventh.anchor_ref.clone(),
            position: Some(0),
            conjugate: false,
            extensions: BTreeMap::new(),
        });
        assert!(validate_constellation_shape_declaration(&seventh).is_err());

        // Unknown/unversioned shape refs are preserved, not refused.
        let mut unknown = constellation.clone();
        unknown.extensions.insert(
            QL_SHAPE_DECLARATION_EXTENSION.to_owned(),
            serde_json::json!({"shape_ref": "ql:experimental:9.9:field:Z:7:D9"}),
        );
        assert!(validate_constellation_shape_declaration(&unknown).is_ok());
    }

    #[test]
    fn return_canon_declares_ground_kind_and_refuses_unknown() {
        let mut constellation = arbitration_constellation();
        constellation.returns[0].ground_kind = Some("conjugate".into());
        assert!(constellation.validate().is_ok());
        constellation.returns[0].ground_kind = Some("benevolent".into());
        assert!(constellation.validate().is_err());
    }

    /// The compression acceptance: the Arbitration field's 6 and 6′ married
    /// in the six generated relations compress through the 0 // 1 trinity
    /// into a single 0/1 encapsulation (the whole-field reading of
    /// `WHOLE-FIELD.md`).
    #[test]
    fn six_plus_six_prime_compresses_through_the_trinity() {
        let constellation = arbitration_constellation();
        let mut generated = std::collections::BTreeMap::new();
        for (position, name) in [
            "continuity-in-indeterminacy",
            "criterion-through-distinction",
            "delineation-through-difference",
            "arbitration-in-crisis",
            "con-text-through-diaphaneity",
            "resolution-in-reconciliation",
        ]
        .into_iter()
        .enumerate()
        {
            generated.insert(position as u8, reference(&format!("wiki:node:t09:{name}")));
        }
        let compression = compress_six_plus_six_prime(&constellation, &generated).unwrap();
        assert_eq!(compression.composites.len(), 6);
        assert_eq!(compression.trinity.zero, "6");
        assert_eq!(compression.trinity.slash, "6+6′");
        assert_eq!(compression.trinity.one, "6′");
        // #3 is exact: arbitration-in-crisis sits at position 3.
        assert_eq!(
            compression.composites[3].direct_ref.as_str(),
            "wiki:node:t09:arbitration"
        );
        assert_eq!(
            compression.composites[3].conjugate_ref.as_str(),
            "wiki:node:t09:crisis"
        );
        assert_eq!(
            compression.composites[3].generated_ref.as_str(),
            "wiki:node:t09:arbitration-in-crisis"
        );
        // Incomplete fields do not compress.
        let mut partial = generated.clone();
        partial.remove(&5);
        assert!(compress_six_plus_six_prime(&constellation, &partial).is_err());
    }
}
