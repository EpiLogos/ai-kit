mod tests {
    use super::*;
    fn initial() -> String {
        json!({"profile":"okf-wiki/v1","native_extra":{"preserve":true},"objects":[
        {"object":"space","profile":"okf-wiki/v1","ref":"wiki:project","revision":1,"node_refs":["wiki:a","wiki:b"]},
        {"object":"node","profile":"okf-wiki/v1","ref":"wiki:a","revision":1,"type":"Document","space_refs":["wiki:project"],"title":"Ordinary linked writing A"},
        {"object":"node","profile":"okf-wiki/v1","ref":"wiki:b","revision":1,"type":"Document","space_refs":["wiki:project"],"title":"Ordinary linked writing B"}
    ]}).to_string()
    }
    fn request(frame: &str, revision: u64, id: &str, changes: Value) -> Request {
        serde_json::from_value(json!({"schema":ACTION,"frame_ref":frame,"expected_revision":revision,"actor_ref":"human:author","operation_ref":id,"changes":changes})).unwrap()
    }
    fn create(frame: &str, anchor: &str) -> Request {
        request(
            frame,
            0,
            &format!("{frame}:create"),
            json!([{"change":"create","anchor_ref":anchor,"title":"Question","inquiry":{"question":"How do these sources differ?"},"space_refs":["wiki:project"]}]),
        )
    }
    fn source(start: u64, end: u64) -> Value {
        json!({"source_ref":"central:source:essay.md","source_revision":"r4","aikit.techne-facet/v1":{"contract":"aikit.techne-facet/v1","selector":{"unit":"text_span","start":start,"end":end,"anchor_ref":"heading:argument"}}})
    }
    fn member(id: &str, subject: &str) -> Value {
        json!({"change":"member_add","member":{"subject_ref":subject,"participation":{"participation_ref":id,"sources":[source(4,18)]}}})
    }
    fn relation(id: &str, expected: Option<u64>, from: &str, to: &str) -> Value {
        json!({"change":"relation_put","relation":{"relation_ref":id,"expected_revision":expected,"from_participation_ref":from,"to_participation_ref":to,"relation":"questions","direction":"directed","standing":"proposed","evidence":[source(4,18)]}})
    }
    fn frame(content: &str, reference: &str) -> Value {
        inspect(content, &ResourceRef::parse(reference).unwrap()).unwrap()
    }

    #[test]
    fn participation_temporal_set_preserves_places_and_source_objects_and_refuses_stale_or_invalid_facts() {
        let created=apply(&initial(), &create("wiki:timed", "wiki:timed-anchor")).unwrap();
        let placed=apply(&created.content,&request("wiki:timed",1,"operation:place",json!([
            member("part:timed", "wiki:a"), member("part:other", "wiki:a"),
            {"change":"place_set","participation_ref":"part:timed","places":[{"place_ref":"place:declared","precision":"unlocated","source_ref":"central:source:essay.md"}]}
        ]))).unwrap();
        let facts=json!([
            {"kind":"occurrence","instant":"2024-06-03T10:30:00Z","precision":"minute","source_ref":"central:source:essay.md"},
            {"kind":"valid","interval":{"from":"2024-01-01T00:00:00Z","to":"2024-12-31T23:59:59Z","from_precision":"year","to_precision":"second"},"source_ref":"central:source:essay.md"}
        ]);
        let command=request("wiki:timed",2,"operation:time",json!([{"change":"temporal_set","participation_ref":"part:timed","temporal":facts}]));
        let timed=apply(&placed.content,&command).unwrap();
        let reading=frame(&timed.content,"wiki:timed");
        let members=&reading["frame"]["constellations"][0]["members"];
        assert_eq!(members[0][TECHNE_FACET_EXTENSION]["temporal"],facts);
        assert_eq!(members[0][TECHNE_FACET_EXTENSION]["spatial"],frame(&placed.content,"wiki:timed")["frame"]["constellations"][0]["members"][0][TECHNE_FACET_EXTENSION]["spatial"]);
        assert!(members[1].get(TECHNE_FACET_EXTENSION).is_none(),"another participation over the same source gains no time");
        let old=WikiDocument::parse(&placed.content).unwrap();let new=WikiDocument::parse(&timed.content).unwrap();
        assert_eq!(old.object(&ResourceRef::parse("wiki:a").unwrap()),new.object(&ResourceRef::parse("wiki:a").unwrap()));
        assert_eq!(apply(&timed.content,&command).unwrap().content,timed.content,"operation replay is byte-exact idempotent");
        let stale=request("wiki:timed",2,"operation:stale",json!([{"change":"temporal_set","participation_ref":"part:timed","temporal":[]}]));
        assert!(apply(&timed.content,&stale).is_err());
        for (id,part,invalid) in [
            ("missing-source","part:timed",json!([{"kind":"occurrence","instant":"2024-01-01T00:00:00Z"}])),
            ("missing-carrier","part:timed",json!([{"kind":"occurrence","source_ref":"central:source:essay.md"}])),
            ("invalid-date","part:timed",json!([{"kind":"occurrence","instant":"not-a-date","source_ref":"central:source:essay.md"}])),
            ("wrong-participation","part:absent",facts.clone()),
            ("over-budget","part:timed",Value::Array(vec![facts[0].clone();257])),
        ] {
            let attempt=request("wiki:timed",3,&format!("operation:{id}"),json!([{"change":"temporal_set","participation_ref":part,"temporal":invalid}]));
            assert!(apply(&timed.content,&attempt).is_err(),"{id} must refuse before persistence");
        }
        let cleared=apply(&timed.content,&request("wiki:timed",3,"operation:clear-time",json!([{"change":"temporal_set","participation_ref":"part:timed","temporal":[]}]))).unwrap();
        let cleared=frame(&cleared.content,"wiki:timed");
        assert!(cleared["frame"]["constellations"][0]["members"][0][TECHNE_FACET_EXTENSION].get("temporal").is_none());
        assert_eq!(cleared["frame"]["constellations"][0]["members"][0][TECHNE_FACET_EXTENSION]["spatial"],members[0][TECHNE_FACET_EXTENSION]["spatial"]);
    }
    #[test]
    fn native_empty_creation_reopens_and_repeated_return_is_byte_exact() {
        let input = initial();
        let command = create("wiki:inquiry", "wiki:whole");
        let saved = apply(&input, &command).unwrap();
        assert_eq!(saved.revision, 1);
        assert!(!saved.idempotent);
        let reading = frame(&saved.content, "wiki:inquiry");
        assert_eq!(reading["frame"]["constellations"][0]["members"], json!([]));
        assert_eq!(
            reading["construction"]["inquiry"]["question"],
            "How do these sources differ?"
        );
        let repeated = apply(&saved.content, &command).unwrap();
        assert!(repeated.idempotent);
        assert_eq!(repeated.content, saved.content);
        let doc: Value = serde_json::from_str(&saved.content).unwrap();
        assert_eq!(doc["native_extra"]["preserve"], true);
        let mut changed = command.clone();
        changed.actor_ref = ResourceRef::parse("agent:another").unwrap();
        assert!(
            apply(&saved.content, &changed).is_err(),
            "same operation key with altered authority/content is refused"
        );
    }
    #[test]
    fn passages_and_contextual_memberships_are_not_global_document_roles() {
        let a = apply(&initial(), &create("wiki:inquiry", "wiki:whole")).unwrap();
        let a = apply(
            &a.content,
            &request(
                "wiki:inquiry",
                1,
                "act:members",
                json!([
                    member("part:a", "wiki:a"),
                    member("part:a2", "wiki:a"),
                    member("part:b", "wiki:b")
                ]),
            ),
        )
        .unwrap();
        let reading = frame(&a.content, "wiki:inquiry");
        let members = reading["frame"]["constellations"][0]["members"]
            .as_array()
            .unwrap();
        assert_eq!(members.len(), 3);
        assert_eq!(members[0]["ref"], members[1]["ref"]);
        assert_ne!(
            members[0][PARTICIPATION]["participation_ref"],
            members[1][PARTICIPATION]["participation_ref"]
        );
        assert_eq!(members[0][PARTICIPATION]["sources"][0], source(4, 18));
        let original = WikiDocument::parse(&a.content)
            .unwrap()
            .object(&ResourceRef::parse("wiki:a").unwrap())
            .unwrap()
            .clone();
        assert!(matches!(original,WikiObject::Node(n) if n.revision==1 && n.extensions.is_empty()));
    }
    #[test]
    fn duplicate_endpoint_relations_stay_distinct_and_retraction_is_not_hiding() {
        let a = apply(&initial(), &create("wiki:inquiry", "wiki:whole")).unwrap();
        let a = apply(
            &a.content,
            &request(
                "wiki:inquiry",
                1,
                "act:relations",
                json!([
                    member("part:a", "wiki:a"),
                    member("part:b", "wiki:b"),
                    relation("edge:one", None, "part:a", "part:b"),
                    relation("edge:two", None, "part:a", "part:b")
                ]),
            ),
        )
        .unwrap();
        assert_eq!(
            frame(&a.content, "wiki:inquiry")["relations"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(apply(
            &a.content,
            &request(
                "wiki:inquiry",
                2,
                "act:bad-remove",
                json!([{"change":"member_remove","participation_ref":"part:a"}])
            )
        )
        .is_err());
        let mut reconnect = relation("edge:one", Some(1), "part:b", "part:a");
        reconnect["relation"]["standing"] = json!("uncertain");
        let b = apply(
            &a.content,
            &request("wiki:inquiry", 2, "act:reconnect", json!([reconnect])),
        )
        .unwrap();
        assert_eq!(
            frame(&b.content, "wiki:inquiry")["relations"][0]["from_ref"],
            "wiki:b"
        );
        let b=apply(&b.content,&request("wiki:inquiry",3,"act:remove",json!([
            {"change":"relation_retract","relation_ref":"edge:one","expected_revision":2,"reason":"changed inquiry"},
            {"change":"relation_retract","relation_ref":"edge:two","expected_revision":1,"reason":"changed inquiry"},
            {"change":"member_remove","participation_ref":"part:a"}]))).unwrap();
        assert_eq!(
            frame(&b.content, "wiki:inquiry")["relations"][0][RELATION]["standing"],
            "retracted"
        );
        assert_eq!(
            frame(&b.content, "wiki:inquiry")["frame"]["constellations"][0]["members"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn source_and_construction_revisions_are_independent_and_stale_edits_are_atomic() {
        let a = apply(&initial(), &create("wiki:inquiry", "wiki:whole")).unwrap();
        let b = apply(
            &a.content,
            &request(
                "wiki:inquiry",
                1,
                "human:change",
                json!([member("part:a", "wiki:a")]),
            ),
        )
        .unwrap();
        let stale = request(
            "wiki:inquiry",
            1,
            "agent:late",
            json!([member("part:b", "wiki:b")]),
        );
        assert_eq!(
            apply(&b.content, &stale).unwrap_err().code(),
            "knowledge.constellation_revision_conflict"
        );
        let mut bad = member("part:b", "wiki:b");
        bad["member"]["participation"]["sources"][0]["source_revision"] = Value::Null;
        assert!(apply(
            &b.content,
            &request("wiki:inquiry", 2, "bad:source", json!([bad]))
        )
        .is_err());
        assert_eq!(frame(&b.content, "wiki:inquiry")["frame"]["revision"], 2);
    }
    #[test]
    fn full_semantic_whole_exceeds_the_renderer_budget() {
        let a = apply(&initial(), &create("wiki:inquiry", "wiki:whole")).unwrap();
        let changes = (0..40)
            .map(|i| {
                member(
                    &format!("part:{i}"),
                    if i % 2 == 0 { "wiki:a" } else { "wiki:b" },
                )
            })
            .collect::<Vec<_>>();
        let a = apply(
            &a.content,
            &request("wiki:inquiry", 1, "act:40", json!(changes)),
        )
        .unwrap();
        assert_eq!(
            frame(&a.content, "wiki:inquiry")["frame"]["constellations"][0]["members"]
                .as_array()
                .unwrap()
                .len(),
            40
        );
    }
    #[test]
    fn frame_first_open_roles_allow_proposed_interpretation_without_certification() {
        let mut command = create("wiki:inquiry", "wiki:whole");
        if let Change::Create { frame, .. } = &mut command.changes[0] {
            *frame = Some(AuthoredFrame {
                shape_ref: "ql:shape:1.0.0:constellation:twofold".into(),
                contract_ref: "ql.shape@1.0.0".into(),
                roles: vec![
                    FrameRole {
                        role_ref: ResourceRef::parse("role:question").unwrap(),
                        label: "Question".into(),
                        address: json!({"position":0,"conjugate":false}),
                    },
                    FrameRole {
                        role_ref: ResourceRef::parse("role:answer").unwrap(),
                        label: "Answer".into(),
                        address: json!({"position":5,"conjugate":false}),
                    },
                ],
                provenance: vec![],
                standing: "proposed".into(),
            });
        }
        let a = apply(&initial(), &command).unwrap();
        assert_eq!(
            frame(&a.content, "wiki:inquiry")["construction"]["frame"]["roles"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let mut add = member("part:a", "wiki:a");
        add["member"]["participation"]["role_ref"] = json!("role:answer");
        let a = apply(
            &a.content,
            &request("wiki:inquiry", 1, "act:fill", json!([add])),
        )
        .unwrap();
        assert!(apply(&a.content,&request("wiki:inquiry",2,"act:bad-role",json!([{"change":"role_set","participation_ref":"part:a","role_ref":"role:foreign"}]))).is_err());
    }
    #[test]
    fn independent_variants_and_nested_cycle_refusal() {
        let a = apply(&initial(), &create("wiki:inquiry", "wiki:whole")).unwrap();
        let a = apply(
            &a.content,
            &request(
                "wiki:inquiry",
                1,
                "act:member",
                json!([member("part:a", "wiki:a")]),
            ),
        )
        .unwrap();
        let mut variant = create("wiki:variant", "wiki:variant:whole");
        if let Change::Create { variant_of, .. } = &mut variant.changes[0] {
            *variant_of = Some(WholeReference {
                whole_ref: ResourceRef::parse("wiki:inquiry").unwrap(),
                revision: 2,
                kind: "constellation".into(),
            });
        }
        let b = apply(&a.content, &variant).unwrap();
        assert_eq!(
            frame(&b.content, "wiki:variant")["frame"]["constellations"][0]["members"][0]["ref"],
            "wiki:a"
        );
        let mut nested = member("part:variant", "wiki:variant:whole");
        nested["member"]["participation"]["nested"] =
            json!({"whole_ref":"wiki:variant","revision":1,"kind":"constellation"});
        let b = apply(
            &b.content,
            &request("wiki:inquiry", 2, "act:nested", json!([nested])),
        )
        .unwrap();
        let mut back = member("part:back", "wiki:whole");
        back["member"]["participation"]["nested"] =
            json!({"whole_ref":"wiki:inquiry","revision":3,"kind":"constellation"});
        assert!(apply(
            &b.content,
            &request("wiki:variant", 1, "act:cycle", json!([back]))
        )
        .is_err());
    }
    #[test]
    fn composition_return_references_native_artifact_and_same_save_is_noop() {
        let a = apply(&initial(), &create("wiki:inquiry", "wiki:whole")).unwrap();
        let attach = json!({"change":"composition_attach","composition":{"reference":"expression:work","revision":"r7","kind":"palace","source":{"source_ref":"central:source:work.expression.json","source_revision":"r7"},"derivation_refs":["wiki:inquiry"]}});
        let b = apply(
            &a.content,
            &request("wiki:inquiry", 1, "return:1", json!([attach.clone()])),
        )
        .unwrap();
        let c = apply(
            &b.content,
            &request("wiki:inquiry", 2, "return:again", json!([attach])),
        )
        .unwrap();
        assert!(c.idempotent);
        assert_eq!(b.content, c.content);
        assert_eq!(c.revision, 2);
        let reading = frame(&c.content, "wiki:inquiry");
        assert_eq!(
            reading["construction"]["compositions"][0]["reference"],
            "expression:work"
        );
        assert_eq!(
            reading["frame"]["constellations"][0]["returns"][0]["through_anchor_ref"],
            "wiki:whole"
        );
        assert_eq!(
            reading["frame"]["constellations"][0]["returns"][0]["ground_ref"],
            "expression:work"
        );
    }
    #[test]
    fn malformed_and_readonly_native_constructions_refuse_before_effect() {
        let a = apply(&initial(), &create("wiki:inquiry", "wiki:whole")).unwrap();
        let mut doc: Value = serde_json::from_str(&a.content).unwrap();
        for object in doc["objects"].as_array_mut().unwrap() {
            if object["ref"] == "wiki:inquiry" {
                object["read_only"] = json!(true);
            }
        }
        assert!(apply(
            &doc.to_string(),
            &request(
                "wiki:inquiry",
                1,
                "act:forbidden",
                json!([member("part:a", "wiki:a")])
            )
        )
        .is_err());
        let mut bad = serde_json::to_value(create("wiki:other", "wiki:other:whole")).unwrap();
        bad["unexpected_authority"] = json!("write-all");
        assert!(serde_json::from_value::<Request>(bad).is_err());
    }
    #[test]
    fn variant_relations_have_distinct_identity_and_exact_remapped_participations() {
        let a = apply(&initial(), &create("wiki:inquiry", "wiki:whole")).unwrap();
        let a = apply(
            &a.content,
            &request(
                "wiki:inquiry",
                1,
                "act:bind",
                json!([
                    member("part:a", "wiki:a"),
                    member("part:b", "wiki:b"),
                    relation("edge:source", None, "part:a", "part:b")
                ]),
            ),
        )
        .unwrap();
        let mut command = create("wiki:variant", "wiki:variant:whole");
        if let Change::Create { variant_of, .. } = &mut command.changes[0] {
            *variant_of = Some(WholeReference {
                whole_ref: ResourceRef::parse("wiki:inquiry").unwrap(),
                revision: 2,
                kind: "constellation".into(),
            });
        }
        let b = apply(&a.content, &command).unwrap();
        let reading = frame(&b.content, "wiki:variant");
        assert_eq!(reading["relations"][0]["ref"], "wiki:variant:relation:0");
        assert_eq!(
            reading["relations"][0][RELATION]["from_participation_ref"],
            "wiki:variant:participation:0"
        );
        assert_eq!(
            reading["relations"][0]["derived_from_relation"]["reference"],
            "edge:source"
        );
        assert_eq!(
            frame(&b.content, "wiki:inquiry")["relations"][0]["ref"],
            "edge:source"
        );
    }
}
