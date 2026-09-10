from pathlib import Path

core_path = Path("crates/aikit-core/src/resource/development_field.rs")
text = core_path.read_text()

const_anchor = 'pub const DEVELOPMENT_FIELD_BINDING_ANNOTATION: &str = "aikit.development-field-binding";\n'
const_insert = const_anchor + 'pub const QL_STRUCTURAL_CARRIER_CONTRACT_REF: &str = "ql.structural-carrier/1.0.0";\n'
if "QL_STRUCTURAL_CARRIER_CONTRACT_REF" not in text:
    if const_anchor not in text:
        raise SystemExit("Development Field constant anchor not found")
    text = text.replace(const_anchor, const_insert, 1)

start = text.index("/// One QL-owned shape address bound to an owner-native ResourceRef.")
end = text.index("/// A reference into Workcell-owned material state.")
new_block = r'''/// QL coordinate face carried as structural identity, without importing QL's
/// shape algebra into AIKit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QlShapeFaceCarrier {
    Direct,
    Conjugate,
}

/// Portable coordinate identity from the accepted QL structural-carrier seam.
/// AIKit validates only the stable 0..5 coordinate aperture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlShapeCoordinateCarrier {
    pub position: u8,
    pub face: QlShapeFaceCarrier,
}

impl QlShapeCoordinateCarrier {
    fn validate(&self) -> Result<()> {
        if self.position > 5 {
            return Err(AikitError::new(
                "resource.development_field_shape_coordinate_invalid",
                format!("QL carrier position must be 0..5, got {}", self.position),
            ));
        }
        Ok(())
    }
}

/// Portable view of QL's `ShapeMemberBinding = StructuralParticipation`.
/// `subject_ref` remains opaque caller identity and the coordinate is structural
/// participation only, never a semantic assertion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlShapeMemberBinding {
    pub subject_ref: String,
    pub coordinate: QlShapeCoordinateCarrier,
}

/// Portable view of the QL-owned `QlShapeAddress` used by relation bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlShapeAddressCarrier {
    pub row: QlShapeCoordinateCarrier,
    pub column: QlShapeCoordinateCarrier,
}

/// One caller-attributable semantic determination at a QL address.
/// QL requires evidence for an asserted relation; AIKit preserves that
/// attribution but does not decide whether the relation is true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlShapeRelationBinding {
    pub address: QlShapeAddressCarrier,
    pub relation_ref: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

/// Opaque caller/source/standing provenance from QL's accepted ShapeBinding.
/// Standing remains caller-world data, not an AIKit authority claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlCallerProvenance {
    pub caller_ref: String,
    pub source_ref: String,
    pub standing_ref: String,
}

/// Portable view of QL-MEF's accepted `ql.structural-carrier/1.0.0`
/// `ShapeBinding` seam.
///
/// AIKit pins and preserves the owner contract's identity/provenance fields. It
/// validates carrier integrity only: it does not reproduce QL's shape ontology,
/// reject positive partial/developed shapes, validate relation-field membership,
/// or generate meanings for addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QlShapeBindingCarrier {
    pub contract_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_revision: Option<SourceRevision>,
    pub subject_ref: ResourceRef,
    pub shape_ref: String,
    pub whole_ref: String,
    #[serde(default)]
    pub basis_refs: Vec<String>,
    #[serde(default)]
    pub members: Vec<QlShapeMemberBinding>,
    #[serde(default)]
    pub relation_bindings: Vec<QlShapeRelationBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_ref: Option<String>,
    #[serde(default)]
    pub return_refs: Vec<String>,
    pub provenance: QlCallerProvenance,
}

fn require_ql_carrier_ref(value: &str, field: &'static str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(AikitError::new(
            "resource.development_field_shape_invalid",
            format!("QL ShapeBinding {field} must not be empty"),
        ));
    }
    Ok(())
}

impl QlShapeBindingCarrier {
    fn validate(&self, subject: &ResourceRef) -> Result<()> {
        if &self.subject_ref != subject {
            return Err(AikitError::new(
                "resource.development_field_shape_subject_mismatch",
                format!(
                    "QL ShapeBinding subject {} does not match carrier {}",
                    self.subject_ref, subject
                ),
            ));
        }
        if self.contract_ref != QL_STRUCTURAL_CARRIER_CONTRACT_REF {
            return Err(AikitError::new(
                "resource.development_field_shape_contract_unsupported",
                format!(
                    "unsupported QL structural carrier {}; expected {}",
                    self.contract_ref, QL_STRUCTURAL_CARRIER_CONTRACT_REF
                ),
            ));
        }
        require_ql_carrier_ref(&self.shape_ref, "shape_ref")?;
        require_ql_carrier_ref(&self.whole_ref, "whole_ref")?;
        for reference in &self.basis_refs {
            require_ql_carrier_ref(reference, "basis_ref")?;
        }
        for member in &self.members {
            require_ql_carrier_ref(&member.subject_ref, "member subject_ref")?;
            member.coordinate.validate()?;
        }
        for relation in &self.relation_bindings {
            relation.address.row.validate()?;
            relation.address.column.validate()?;
            require_ql_carrier_ref(&relation.relation_ref, "relation_ref")?;
            if relation.evidence_refs.is_empty() {
                return Err(AikitError::new(
                    "resource.development_field_shape_relation_evidence_missing",
                    "QL semantic relation binding requires at least one evidence_ref",
                ));
            }
            for reference in &relation.evidence_refs {
                require_ql_carrier_ref(reference, "evidence_ref")?;
            }
        }
        if let Some(reference) = &self.derivation_ref {
            require_ql_carrier_ref(reference, "derivation_ref")?;
        }
        if let Some(reference) = &self.operator_ref {
            require_ql_carrier_ref(reference, "operator_ref")?;
        }
        for reference in &self.return_refs {
            require_ql_carrier_ref(reference, "return_ref")?;
        }
        require_ql_carrier_ref(&self.provenance.caller_ref, "caller_ref")?;
        require_ql_carrier_ref(&self.provenance.source_ref, "source_ref")?;
        require_ql_carrier_ref(&self.provenance.standing_ref, "standing_ref")?;
        Ok(())
    }
}

'''
text = text[:start] + new_block + text[end:]

old_test_start = "    #[test]\n    fn partial_ql_shape_binding_is_carried_without_semantic_inference() {"
next_test = "    #[test]\n    fn bounded_read_reports_explicit_relations_and_unknown_refs_without_guessing() {"
test_start = text.index(old_test_start)
test_end = text.index(next_test, test_start)
replacement = r'''    #[test]
    fn accepted_ql_shape_binding_round_trips_without_semantic_edge_inference() {
        let mut record = carrier("central:tier:2", DevelopmentFieldCarrierKind::TierBinding);
        let mut binding = DevelopmentFieldBinding::new(DevelopmentFieldCarrierKind::TierBinding);
        binding.shape_binding = Some(QlShapeBindingCarrier {
            contract_ref: QL_STRUCTURAL_CARRIER_CONTRACT_REF.into(),
            contract_revision: Some(
                SourceRevision::parse("34e0f3c7b1ccfa739d5a77f98c2dbedc755d8de1").unwrap(),
            ),
            subject_ref: record.descriptor.id.clone(),
            shape_ref: "ql:shape:1.0.0:constellation:partial-conjugate-9".into(),
            whole_ref: "external:whole:tier-2".into(),
            basis_refs: vec!["external:basis:tier-2".into()],
            members: vec![QlShapeMemberBinding {
                subject_ref: "central:tier:member:0".into(),
                coordinate: QlShapeCoordinateCarrier {
                    position: 0,
                    face: QlShapeFaceCarrier::Direct,
                },
            }],
            relation_bindings: vec![QlShapeRelationBinding {
                address: QlShapeAddressCarrier {
                    row: QlShapeCoordinateCarrier {
                        position: 0,
                        face: QlShapeFaceCarrier::Direct,
                    },
                    column: QlShapeCoordinateCarrier {
                        position: 1,
                        face: QlShapeFaceCarrier::Direct,
                    },
                },
                relation_ref: "external:relation:r1".into(),
                evidence_refs: vec!["external:evidence:receipt-1".into()],
            }],
            derivation_ref: Some("external:derivation:d1".into()),
            operator_ref: Some("ql:carrier:1.0.0:relation-field:cartesian-addresses".into()),
            return_refs: vec!["ql:return:0/1".into()],
            provenance: QlCallerProvenance {
                caller_ref: "external:caller:agent-3".into(),
                source_ref: "external:source:run-42".into(),
                standing_ref: "external:standing:observed".into(),
            },
        });
        attach_development_field_binding(&mut record, &binding).unwrap();
        let restored = development_field_binding(&record).unwrap().unwrap();
        let shape = restored.shape_binding.unwrap();
        assert_eq!(shape.contract_ref, QL_STRUCTURAL_CARRIER_CONTRACT_REF);
        assert_eq!(shape.relation_bindings.len(), 1);
        assert_eq!(shape.provenance.standing_ref, "external:standing:observed");
        assert!(
            restored.relations.is_empty(),
            "QL relation/address bindings remain attributable QL carrier content and do not auto-create semantic Resource edges"
        );
    }

    #[test]
    fn ql_relation_binding_without_evidence_is_rejected_by_the_accepted_carrier_floor() {
        let mut record = carrier("central:tier:3", DevelopmentFieldCarrierKind::TierBinding);
        let mut binding = DevelopmentFieldBinding::new(DevelopmentFieldCarrierKind::TierBinding);
        binding.shape_binding = Some(QlShapeBindingCarrier {
            contract_ref: QL_STRUCTURAL_CARRIER_CONTRACT_REF.into(),
            contract_revision: None,
            subject_ref: record.descriptor.id.clone(),
            shape_ref: "ql:shape:1.0.0:constellation:twofold".into(),
            whole_ref: "external:whole:tier-3".into(),
            basis_refs: Vec::new(),
            members: Vec::new(),
            relation_bindings: vec![QlShapeRelationBinding {
                address: QlShapeAddressCarrier {
                    row: QlShapeCoordinateCarrier {
                        position: 0,
                        face: QlShapeFaceCarrier::Direct,
                    },
                    column: QlShapeCoordinateCarrier {
                        position: 1,
                        face: QlShapeFaceCarrier::Direct,
                    },
                },
                relation_ref: "external:relation:r1".into(),
                evidence_refs: Vec::new(),
            }],
            derivation_ref: None,
            operator_ref: None,
            return_refs: Vec::new(),
            provenance: QlCallerProvenance {
                caller_ref: "external:caller:agent-3".into(),
                source_ref: "external:source:run-42".into(),
                standing_ref: "external:standing:observed".into(),
            },
        });
        let error = attach_development_field_binding(&mut record, &binding).unwrap_err();
        assert_eq!(
            error.code(),
            "resource.development_field_shape_relation_evidence_missing"
        );
    }

'''
text = text[:test_start] + replacement + text[test_end:]
core_path.write_text(text)

doc_path = Path("docs/DEVELOPMENT-FIELD-SUBSTRATE.md")
doc = doc_path.read_text()
old = "The core contract is `aikit.development-field-reading/v1`. A carrier remains an ordinary `ResourceRef` and therefore stays in the existing Search/Resolve field. Its optional `aikit.development-field-binding/v1` annotation adds only owner-declared carrier kind, explicit stable-ref relations, an attributable QL `ShapeBinding` carrier when supplied, and Workcell material references. A QL shape address is structural addressability, not a semantic Wiki edge, and partial/developed shapes are valid inputs."
new = "The core contract is `aikit.development-field-reading/v1`. A carrier remains an ordinary `ResourceRef` and therefore stays in the existing Search/Resolve field. Its optional `aikit.development-field-binding/v1` annotation adds only owner-declared carrier kind, explicit stable-ref relations, an attributable QL `ShapeBinding` carrier when supplied, and Workcell material references. The QL seam pins the accepted owner contract `ql.structural-carrier/1.0.0` (QL-MEF #122/#137): `subject_ref`, `shape_ref`, `whole_ref`, basis/member/relation bindings, optional derivation/operator refs, Return refs and opaque caller/source/standing provenance survive intact. QL relation bindings retain their evidence refs but never auto-create AIKit semantic Resource edges. Partial/developed shapes remain valid because AIKit does not re-run or replace QL shape semantics."
if old not in doc:
    raise SystemExit("Development Field documentation anchor not found")
doc_path.write_text(doc.replace(old, new, 1))
