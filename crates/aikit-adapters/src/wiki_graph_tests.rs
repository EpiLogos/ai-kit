use super::*;
use aikit_core::KnowledgeAddress;
use aikit_core::resource::{ProviderRef, ResourceKind, ResourceRef, SourceAuthority};

fn hit(reference: &str) -> KnowledgeSearchHit {
    KnowledgeSearchHit {
        address: KnowledgeAddress::Wiki(ResourceRef::parse(reference).unwrap()),
        resource: ResourceRef::parse(reference).unwrap(),
        kind: ResourceKind::KnowledgeNode,
        label: reference.into(), score: 1.0, snippet: String::new(),
        provider: ProviderRef::parse("provider/semantic-wiki").unwrap(),
        authority: SourceAuthority::Authored, ranking: None,
    }
}
fn objects() -> Vec<WikiObject> {
    let member = |part: &str, role: &str, source: &str| json!({"ref":source, "aikit.constellation-participation/v1":{
        "participation_ref":part, "role_ref":role, "sources":[], "note":"PRIVATE_QUOTE_NOT_GRAPH_METADATA"}});
    serde_json::from_value(json!([
        {"object":"node","profile":"okf-wiki/v1","ref":"wiki:source","revision":1,"type":"Note","title":"Source"},
        {"object":"node","profile":"okf-wiki/v1","ref":"wiki:anchor","revision":1,"type":"Constellation","title":"Whole"},
        {"object":"frame","profile":"okf-wiki/v1","ref":"wiki:frame","revision":2,"member_refs":["wiki:anchor","wiki:source"],"external_refs":[],
          "aikit.constellation/v1":{"frame":{"shape_ref":"ql:shape:1.0.0:constellation:twofold","contract_ref":"ql.shape@1.0.0",
            "roles":[{"role_ref":"role:0","address":{"position":0,"layout":{"x":-1.0,"y":0.0,"z":0.0},"secret":"PRIVATE_ROLE_METADATA"}},
                     {"role_ref":"role:1","address":{"position":1,"layout":{"x":1.0,"y":0.0,"z":0.0}}}]}},
          "constellations":[{"anchor_ref":"wiki:anchor","members":[member("part:one","role:0","wiki:source"),member("part:two","role:1","wiki:source"),member("PRIVATE_PART","PRIVATE_ROLE","PRIVATE_SOURCE")],"returns":[]}]},
        {"object":"edge","profile":"okf-wiki/v1","ref":"edge:qualifies","revision":3,"from_ref":"wiki:source","to_ref":"wiki:source","relation":"qualifies","origin":"authored","origin_ref":"wiki:frame",
         "aikit.constellation-relation/v1":{"from_participation_ref":"part:one","to_participation_ref":"part:two","standing":"proposed"}}
    ])).unwrap()
}
#[test]
fn repeated_source_roles_keep_native_participations_without_disclosing_private_context() {
    let value = project_graph(&[hit("wiki:source"),hit("wiki:anchor"),hit("wiki:frame")], &objects(), &[], 32, 64, &[]);
    let nodes=value["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(),5);
    let first=nodes.iter().find(|n|n["resource"]=="part:one").unwrap();
    let second=nodes.iter().find(|n|n["resource"]=="part:two").unwrap();
    assert_eq!(first["subject_ref"],second["subject_ref"]);
    assert_ne!(first["resource"],second["resource"]);
    let edge=value["edges"].as_array().unwrap().iter().find(|e|e["reference"]=="edge:qualifies").unwrap();
    assert_eq!(edge["from"],"part:one"); assert_eq!(edge["to"],"part:two");
    assert_eq!(edge["family"],"ql-authored"); assert_eq!(edge["standing"],"proposed");
    assert_eq!(value["formations"][0]["members"].as_array().unwrap().len(),2);
    assert_eq!(value["formations"][0]["partial"],true);
    assert!(!value.to_string().contains("PRIVATE_"));
}
#[test]
fn budgeted_participation_does_not_reclassify_the_source_as_a_smaller_whole() {
    let value = project_graph(&[hit("wiki:source"),hit("wiki:anchor"),hit("wiki:frame")], &objects(), &[], 4, 2, &[]);
    assert_eq!(value["nodes"].as_array().unwrap().len(),4);
    assert_eq!(value["truncated"],true);
    assert_eq!(value["formations"][0]["partial"],true);
    assert_eq!(value["formations"][0]["members"].as_array().unwrap().len(),1);
    assert_eq!(value["formations"][0]["members"][0]["ref"],"part:one");
}
