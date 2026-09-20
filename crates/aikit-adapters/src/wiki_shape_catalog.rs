//! Authoring choices derived from AIKit's pinned QL structural client. This is
//! not semantic classification or an assertion that a corpus already has a form.
//! Role addresses are QL coordinates; geometry is a disclosed initial layout.
use aikit_core::knowledge_wiki_shape::{
    QL_SHAPE_CONTRACT_REF, QL_SHAPE_CONTRACT_VERSION, QL_SHAPE_UPSTREAM_BLOB,
    WikiQlConstellationGrain as Grain, WikiQlRelationFamily as Family,
};
use serde_json::{Value, json};

pub const CATALOG_SCHEMA: &str = "aikit.ql-authoring-forms/v1";

pub fn catalog() -> Value {
    let mut forms = Vec::new();
    let mut add = |grain: Grain, label: &str, key: String, coordinates: Vec<(u8, bool)>| {
        let shape_ref = format!("ql:shape:{QL_SHAPE_CONTRACT_VERSION}:constellation:{}", grain.as_str());
        let roles: Vec<_> = coordinates.into_iter().map(|(position, conjugate)| {
            let face = if conjugate {"conjugate"} else {"direct"};
            // This is initial presentation only; changing physical coordinates
            // never changes a role or creates a source relation.
            let angle = f64::from(position) * std::f64::consts::PI / 3.0 - std::f64::consts::FRAC_PI_2;
            let radius = if conjugate {1.45} else {1.0};
            json!({"role_ref":format!("{key}:role:{face}:{position}"), "label":format!("{position}{}", if conjugate {"′"} else {""}),
                "address":{"position":position,"conjugate":conjugate,"face":face,
                    "layout":{"x":angle.cos()*radius,"y":angle.sin()*radius,"z":if conjugate {0.25} else {0.0}}}})
        }).collect();
        forms.push(json!({"id":key,"label":label,"shape_ref":shape_ref,"contract_ref":QL_SHAPE_CONTRACT_REF,
            "roles":roles,"provenance":[],"standing":"proposed"}));
    };
    for family in [Family::A, Family::B, Family::C] {
        for (index, (a, b)) in family.pairs().into_iter().enumerate() {
            add(Grain::TwoFold, &format!("Twofold {}{} · {a}/{b}", family.as_str(), index),
                format!("ql:authoring:{QL_SHAPE_CONTRACT_VERSION}:pair:{}:{index}",family.as_str()),vec![(a,false),(b,false)]);
        }
    }
    for (grain, label, positions) in [
        (Grain::ThreeFold123,"Threefold 1–2–3",vec![1,2,3]),
        (Grain::ThreeFold450,"Threefold 4–5–0",vec![4,5,0]),
        (Grain::FourFold1234,"Fourfold 1–2–3–4",vec![1,2,3,4]),
        (Grain::FourPlusOneGround,"Fourfold + ground",vec![0,1,2,3,4]),
        (Grain::FourPlusOneSynthesis,"Fourfold + synthesis",vec![1,2,3,4,5]),
        (Grain::SixFold,"Sixfold",vec![0,1,2,3,4,5]),
    ] {
        add(grain,label,format!("ql:authoring:{QL_SHAPE_CONTRACT_VERSION}:{}",grain.as_str()),positions.into_iter().map(|p|(p,false)).collect());
    }
    add(Grain::TwelveFold,"Direct / conjugate sixfold",format!("ql:authoring:{QL_SHAPE_CONTRACT_VERSION}:twelvefold"),
        (0..6).map(|p|(p,false)).chain((0..6).map(|p|(p,true))).collect());
    json!({"schema":CATALOG_SCHEMA,"contract_ref":QL_SHAPE_CONTRACT_REF,"upstream_contract_blob":QL_SHAPE_UPSTREAM_BLOB,
        "forms":forms,"classification_inferred":false,"geometry_standing":"initial editable presentation; not a semantic inference",
        "coverage":"Pinned positional constellation forms; later QL forms and supplied native frames remain separately addressable."})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn catalogue_keeps_native_pairs_faces_and_open_authoring_standing() {
        let value=catalog();
        let forms=value["forms"].as_array().unwrap();
        assert_eq!(forms.len(),16);
        let mut ids=BTreeSet::new();
        for form in forms {
            assert!(ids.insert(form["id"].as_str().unwrap()));
            assert_eq!(form["standing"],"proposed");
            assert_eq!(form["contract_ref"],QL_SHAPE_CONTRACT_REF);
            let roles=form["roles"].as_array().unwrap();
            let distinct:BTreeSet<_>=roles.iter().map(|r|r["role_ref"].as_str().unwrap()).collect();
            assert_eq!(distinct.len(),roles.len());
            for role in roles {
                assert!(role["address"]["position"].as_u64().unwrap()<6);
                assert!(role["address"]["layout"]["x"].as_f64().unwrap().is_finite());
            }
        }
        assert_eq!(forms.last().unwrap()["roles"].as_array().unwrap().len(),12);
        assert_eq!(value["classification_inferred"],false);
        assert_eq!(forms[6]["roles"][0]["address"]["position"],0);
        assert_eq!(forms[6]["roles"][1]["address"]["position"],5);
    }
}
