use std::fs;

use aikit_cli::app::Service;
use aikit_core::resource::ResourceRef;
use aikit_core::{ForgetScope, KnowledgeAddress, DEFAULT_FAMILIARITY_HALF_LIFE_MS};
use aikit_store::AikitHome;
use aikit_tui::application::TuiApplicationService;
use aikit_tui::application_service::ApplicationService;
use aikit_tui::backend::PaletteBackend;
use tempfile::TempDir;

fn open_service(temp: &TempDir) -> Service {
    let home = AikitHome::at(temp.path().join("aikit-home"));
    Service::open(home, temp.path(), |_| None).expect("open production application service")
}

fn write_project_knowledge(temp: &TempDir) {
    let wiki = r#"{
      "objects": [
        {
          "profile": "okf-wiki/v1",
          "object": "node",
          "ref": "wiki:node:authentication",
          "revision": 7,
          "provenance": [{"source_ref":"source:paper:authentication","source_revision":"rev-3"}],
          "type": "Concept",
          "title": "Authentication architecture",
          "space_refs": [],
          "source_refs": ["source:paper:authentication"]
        }
      ]
    }"#;
    fs::write(temp.path().join("semantic-wiki.json"), wiki).unwrap();

    let source = r#"{
      "binding": {
        "source": "source:paper:authentication",
        "revision": "rev-3",
        "title": "Authentication source paper",
        "tags": ["authentication", "architecture"],
        "visibility": "public",
        "owners": [],
        "media_type": "text/markdown",
        "metadata": {"origin":"test-fixture"}
      },
      "body": "Authentication architecture keeps source evidence distinct from compiled semantic knowledge."
    }"#;
    fs::write(temp.path().join("source-material.json"), source).unwrap();
}

#[test]
fn one_production_service_materialises_routes_history_forget_and_tui_views() {
    let temp = TempDir::new().unwrap();
    write_project_knowledge(&temp);

    let (route_ref, destination, context) = {
        let mut service = open_service(&temp);
        let result = service.knowledge_search("authentication", 50).unwrap();
        let wiki = result
            .hits
            .iter()
            .find(|hit| matches!(hit.address, KnowledgeAddress::Wiki(_)))
            .expect("SemanticWiki hit")
            .address
            .clone();
        let source = result
            .hits
            .iter()
            .find(|hit| matches!(hit.address, KnowledgeAddress::Source(_)))
            .expect("SourcePool hit")
            .address
            .clone();

        let route = service
            .knowledge_route(
                Some("authentication evidence"),
                &[wiki.clone(), source.clone()],
            )
            .unwrap();
        assert_eq!(route.steps.len(), 2);
        assert_eq!(route.steps[0].lens.as_deref(), Some("semantic-wiki"));
        assert_eq!(route.steps[1].lens.as_deref(), Some("source-pool"));
        assert_eq!(
            route.steps[1].transition.as_deref(),
            Some("project-map:source"),
            "cross-lens transition retains ProjectMap provenance instead of counterfeiting a provider-native edge"
        );

        let repeated = service
            .knowledge_route(
                Some("authentication evidence"),
                &[wiki.clone(), source.clone()],
            )
            .unwrap();
        assert_eq!(
            repeated.route, route.route,
            "identical traversal has stable route identity"
        );

        let learned = service.knowledge_search("authentication", 50).unwrap();
        let learned_source = learned
            .hits
            .iter()
            .find(|hit| hit.resource.as_str() == "source:paper:authentication")
            .expect("learned SourcePool destination remains discoverable");
        let ranking = learned_source
            .ranking
            .as_ref()
            .expect("production search discloses learned ranking separately");
        assert_eq!(
            ranking
                .route
                .as_ref()
                .map(|assessment| assessment.observations),
            Some(2)
        );
        assert!(ranking.navigation_score > ranking.provider_score);

        let exact = service
            .knowledge_search("wiki:node:authentication", 50)
            .unwrap();
        assert_eq!(
            exact.hits.first().map(|hit| hit.resource.as_str()),
            Some("wiki:node:authentication"),
            "learned ease must not hide an exact addressed result"
        );

        let explanation = service.knowledge_explain(&source).unwrap();
        let detail = explanation.detail.expect("Explain carries ranking evidence");
        assert_eq!(detail["ranking"]["route"]["observations"], 2);
        assert_eq!(detail["signalClasses"][0], "provider-relevance");
        assert_eq!(detail["signalClasses"][1], "frecency");
        assert_eq!(detail["signalClasses"][2], "context");

        let frame = service
            .knowledge_frame(Some("authentication evidence"), &[wiki.clone(), source])
            .unwrap();
        assert_eq!(frame.selected.len(), 2);
        assert_eq!(frame.readings.len(), 2);
        assert_eq!(frame.routes.len(), 1);
        assert!(frame.contradictions.is_empty());

        let cached = service
            .knowledge_address(&ResourceRef::parse("wiki:node:authentication").unwrap())
            .unwrap();
        assert_eq!(cached, Some(wiki));

        (
            route.route.clone(),
            route.steps.last().unwrap().resource.clone(),
            route.context.clone(),
        )
    };

    {
        let service = open_service(&temp);
        let history = service.knowledge_history(None).unwrap();
        assert_eq!(
            history.len(),
            3,
            "two route uses and the frame survive reopen"
        );
        let learned = PaletteBackend::familiarity(&service)
            .unwrap()
            .expect("route use is replayed from the production familiarity stream");
        let assessment = learned.assess_route(
            &route_ref,
            &destination,
            &context,
            u64::MAX,
            DEFAULT_FAMILIARITY_HALF_LIFE_MS,
        );
        assert_eq!(assessment.observations, 2);
    }

    {
        let mut service = open_service(&temp);
        let mut tui = ApplicationService::new(&mut service);
        let search = TuiApplicationService::search(&tui, "authentication").unwrap();
        assert!(search
            .resources
            .iter()
            .any(|item| item.resource.as_str() == "wiki:node:authentication"));
        assert!(search
            .resources
            .iter()
            .any(|item| item.resource.as_str() == "source:paper:authentication"));

        let address =
            KnowledgeAddress::Wiki(ResourceRef::parse("wiki:node:authentication").unwrap());
        let reading = TuiApplicationService::knowledge_read(&tui, &address)
            .unwrap()
            .expect("final TUI uses production Knowledge read");
        assert_eq!(reading.revision.as_deref(), Some("7"));
        let relations = TuiApplicationService::relations(
            &tui,
            &ResourceRef::parse("wiki:node:authentication").unwrap(),
        )
        .unwrap();
        assert_eq!(
            relations.value["query"]["focus"],
            "wiki:node:authentication"
        );

        assert!(TuiApplicationService::knowledge_forget(
            &mut tui,
            ForgetScope::Route(route_ref.clone()),
        )
        .unwrap());
    }

    let service = open_service(&temp);
    let search_after_forget = service.knowledge_search("authentication", 50).unwrap();
    let source_after_forget = search_after_forget
        .hits
        .iter()
        .find(|hit| hit.resource.as_str() == "source:paper:authentication")
        .unwrap();
    let ranking_after_forget = source_after_forget.ranking.as_ref().unwrap();
    assert!(ranking_after_forget.route.is_none());
    assert_eq!(
        ranking_after_forget.navigation_score, ranking_after_forget.provider_score,
        "forget removes learned ranking influence without changing provider relevance"
    );
    let learned = PaletteBackend::familiarity(&service)
        .unwrap()
        .expect("familiarity store remains readable after reset");
    let assessment = learned.assess_route(
        &route_ref,
        &destination,
        &context,
        u64::MAX,
        DEFAULT_FAMILIARITY_HALF_LIFE_MS,
    );
    assert_eq!(
        assessment.observations, 0,
        "forget removes learned route influence"
    );
    assert_eq!(
        service.knowledge_history(None).unwrap().len(),
        3,
        "forget does not erase Knowledge audit receipts"
    );
}
