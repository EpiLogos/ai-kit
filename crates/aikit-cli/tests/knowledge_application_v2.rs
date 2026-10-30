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
        let status = service.knowledge_status().unwrap();
        let wiki_status = status.wiki.expect("Wiki provider is materialised");
        assert_eq!(
            wiki_status.provider.as_str(),
            "provider/semantic-wiki/sqlite",
            "the production dispatch surface reads through the SQLite provider"
        );
        let cache = temp.path().join("aikit-home/cache/knowledge/wiki");
        assert_eq!(
            fs::read_dir(cache).unwrap().count(),
            1,
            "one project horizon owns one deletable SQLite projection"
        );
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
        let detail = explanation
            .detail
            .expect("Explain carries ranking evidence");
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
        let edges = relations.value["edges"]
            .as_array()
            .expect("final TUI relation read model contains provider-bearing edges");
        let source_edge = edges
            .iter()
            .find(|edge| {
                edge["from"] == "wiki:node:authentication"
                    && edge["to"] == "source:paper:authentication"
                    && edge["relation"] == "source"
            })
            .expect("SemanticWiki source edge reaches the final TUI relation service");
        assert_eq!(source_edge["origin"]["lens"], "semantic-wiki");
        assert!(
            source_edge["origin"]["provider"]
                .as_str()
                .is_some_and(|provider| !provider.is_empty()),
            "provider identity survives the application relation projection"
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
    assert!(
        source_after_forget.ranking.is_none(),
        "forget removes the only learned ranking signal instead of retaining inert learned metadata"
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

/// O-I #741 (DLW §18.4: "Each world you work in keeps its own [history] … Press ✕ beside it and the line forgets it"). The
/// search line records each open as a route and forgets one item from the world it stands in: `aikit knowledge forget here
/// <RESOURCE>` forgets the destination's learned use in this invocation's own context (its project), routes and plain uses
/// alike, and leaves the same destination's use in another world, and the operation history, as they were.
#[test]
fn forget_here_forgets_one_item_in_the_world_it_is_asked_in_and_no_other() {
    use aikit_cli::cli::Cli;
    use clap::Parser;

    let temp = TempDir::new().unwrap();
    write_project_knowledge(&temp);
    let world = |project: &'static str| {
        let home = AikitHome::at(temp.path().join("aikit-home"));
        Service::open(home, temp.path(), move |key| {
            (key == "AIKIT_PROJECT_ID").then(|| project.to_string())
        })
        .expect("open the production service in one world")
    };
    let source = world("prj_alpha")
        .knowledge_search("authentication", 50)
        .unwrap()
        .hits
        .into_iter()
        .find(|hit| matches!(hit.address, KnowledgeAddress::Source(_)))
        .expect("SourcePool hit")
        .address;
    let resource = ResourceRef::parse("source:paper:authentication").unwrap();
    let used_here = |service: &Service| {
        service
            .knowledge_search("authentication", 50)
            .unwrap()
            .hits
            .into_iter()
            .find(|hit| hit.resource == resource)
            .and_then(|hit| hit.ranking)
            .and_then(|ranking| ranking.route)
            .map(|route| route.contextual_observations)
            .unwrap_or(0)
    };
    {
        let mut alpha = world("prj_alpha");
        alpha
            .knowledge_route(None, std::slice::from_ref(&source))
            .unwrap();
        alpha
            .knowledge_route(None, std::slice::from_ref(&source))
            .unwrap();
        let mut beta = world("prj_beta");
        beta.knowledge_route(None, std::slice::from_ref(&source))
            .unwrap();
        assert_eq!(used_here(&alpha), 2, "alpha counts its own two opens");
        assert_eq!(used_here(&beta), 1, "beta counts its own one");
    }
    let scope = world("prj_alpha")
        .knowledge_forget_here(resource.clone())
        .unwrap();
    assert_eq!(
        scope,
        ForgetScope::DestinationIn {
            destination: resource.clone(),
            project: Some(ResourceRef::parse("project/prj_alpha").unwrap()),
        },
        "the forget names the world it was asked in"
    );
    assert_eq!(used_here(&world("prj_alpha")), 0, "alpha forgot it");
    assert_eq!(used_here(&world("prj_beta")), 1, "beta keeps its own");
    assert_eq!(
        world("prj_alpha").knowledge_history(None).unwrap().len(),
        2,
        "the operation history is preserved, as every forget preserves it"
    );

    let cli = Cli::try_parse_from([
        "aikit",
        "knowledge",
        "forget",
        "here",
        "source:paper:authentication",
    ])
    .expect("`aikit knowledge forget here RESOURCE` parses");
    assert_eq!(cli.command.unwrap().route(), "cmd_knowledge_forget_here");
}

/// The native owner a scratch Central answers world reads with, from its own filesystem, the way the real owner does: the
/// world map (`central.world`, both canonical wikis), a project's World facet and its effective sources (root lineage).
const WORLD_OWNER_STUB: &str = r##"#!/bin/sh
root=""; action=""; input="{}"; prev=""
for a in "$@"; do
  if [ "$prev" = "--root" ]; then root="$a"; fi
  if [ -n "$action" ] && [ "$input" = "{}" ]; then case "$a" in '{'*) input="$a" ;; esac; fi
  case "$a" in central.*) [ -z "$action" ] && action="$a" ;; esac
  prev="$a"
done
wiki() { printf '{"present":true,"path":"%s","space_ref":"%s"}' "$1" "$2"; }
case "$action" in
  central.world)
    printf '{"ok":true,"data":{"schema":"central.world-map/v1","root":"%s","control":{"agent_wiki":{"wiki":%s}},"work":{"projects":[' "$root" "$(wiki Control/agents/wiki/wiki.json central:wiki:root)"
    sep=""
    for m in "$root"/Work/*/; do
      name=$(basename "$m")
      printf '%s{"name":"%s","path":"Work/%s","projectcentral":{"agent_wiki":{"wiki":%s}}}' "$sep" "$name" "$name" "$(wiki "Work/$name/ProjectCentral/agents/wiki/wiki.json" "central:wiki:project:$name")"
      sep=","
    done
    printf ']}}}' ;;
  central.world.here)
    project=$(printf '%s' "$input" | sed -n 's/.*"project":"\([^"]*\)".*/\1/p')
    id=$(sed -n 's/.*"project_id":"\([^"]*\)".*/\1/p' "$root/Work/$project/ProjectCentral/project.json")
    printf '{"ok":true,"data":{"schema":"central.world-here/v1","project_world":{"name":"%s","state":"present","ref":"project:%s","parent_ref":"control:root","path":"Work/%s","work_member_present":true}}}' "$project" "$id" "$project" ;;
  central.world.effective-sources)
    ref=$(printf '%s' "$input" | sed -n 's/.*"world_ref":"\([^"]*\)".*/\1/p')
    hops='"control:root"'; [ "$ref" = "control:root" ] || hops="\"control:root\",\"$ref\""
    printf '{"ok":true,"data":{"world_ref":"%s","sources":[{"ref":"central:source:control:root:Control","state":"available","effective_revision":"stub-root-1","effective_source_world":"control:root","authority":"stub","source_treatment":"canonical","effective_treatment":"canonical","propagation_path":[%s],"provenance":[]}]}}' "$ref" "$hops" ;;
  *) printf '%s' '{"ok":false,"error":{"code":"central.world_declaration_absent","message":"not stubbed"}}'; exit 2 ;;
esac
"##;

fn wiki_with(space: &str, node: &str, title: &str) -> String {
    serde_json::json!({"profile": "okf-wiki/v1", "objects": [
        {"profile": "okf-wiki/v1", "object": "space", "ref": space, "revision": 1, "provenance": [], "title": title,
         "anchor_ref": node, "node_refs": [node], "child_space_refs": [], "parent_space_refs": []},
        {"profile": "okf-wiki/v1", "object": "node", "ref": node, "revision": 1, "provenance": [], "type": "Concept",
         "title": title, "space_refs": [space], "source_refs": []}]})
    .to_string()
}

/// A scratch Central: a root wiki every world finds (Compass) and two project worlds, Work/Alpha and Work/Beta, each with
/// its own ProjectCentral identity; an isolated HOME and AIKit home; the stub owner. Returns the Central root.
fn central_with_two_projects(temp: &TempDir) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let root = temp.path().join("Central");
    fs::create_dir_all(root.join(".aikit")).unwrap();
    fs::create_dir_all(root.join("Control/agents/wiki")).unwrap();
    fs::create_dir_all(temp.path().join("home")).unwrap();
    fs::write(
        root.join("Control/agents/wiki/wiki.json"),
        wiki_with("central:wiki:root", "wiki:node:compass", "Compass"),
    )
    .unwrap();
    for name in ["Alpha", "Beta"] {
        let project = root.join("Work").join(name).join("ProjectCentral");
        fs::create_dir_all(project.join("agents/wiki")).unwrap();
        fs::write(
            project.join("project.json"),
            serde_json::json!({"schema": "central.project/v1", "project_id": name, "human_source": "ProjectCentral/user",
                "wiki": {"profile": "okf-wiki/v1", "source": "ProjectCentral/agents/wiki/wiki.json", "adopted_sources": []}})
            .to_string(),
        )
        .unwrap();
        fs::write(
            project.join("agents/wiki/wiki.json"),
            wiki_with(
                &format!("central:wiki:project:{name}"),
                &format!("wiki:node:{}-notes", name.to_lowercase()),
                &format!("{name} notes"),
            ),
        )
        .unwrap();
    }
    let stub = temp.path().join("ctrl");
    fs::write(&stub, WORLD_OWNER_STUB).unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    root
}

/// `aikit --json -C <cwd> ARGS` in the scratch Central, as the O:I kernel asks it: the world is the ground it is asked in.
fn aikit_in(temp: &TempDir, cwd: &std::path::Path, args: &[&str]) -> serde_json::Value {
    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"))
        .arg("--json")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temp.path().join("home"))
        .env("AIKIT_HOME", temp.path().join("aikit-home"))
        .env("CENTRAL_ROOT", temp.path().join("Central"))
        .env("CENTRAL_CTRL_BIN", temp.path().join("ctrl"))
        .output()
        .expect("the real aikit binary runs");
    assert!(
        output.status.success(),
        "aikit {args:?} in {}: {}{}",
        cwd.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["data"].take()
}

/// O-I #820 (DLW §18.4: "Each world you work in keeps its own … Press ✕ beside it and the line forgets it"). The kernel asks
/// AIKit in each world's own ground; AIKit takes the world from that ground: a Work project by its ProjectCentral identity
/// (`project:<id>`), the person's own world (Central's root) as the unscoped context, where every use recorded before
/// worlds reached AIKit already stands — so that learned use stays the person's own. A use in Alpha ranks in Alpha only,
/// and ✕ in Alpha leaves Beta's and the person's.
#[test]
fn each_world_keeps_its_own_use_and_forgets_only_its_own() {
    let temp = TempDir::new().unwrap();
    let root = central_with_two_projects(&temp);
    let (alpha, beta) = (root.join("Work/Alpha"), root.join("Work/Beta"));
    let compass = r#"{"kind":"wiki","value":"wiki:node:compass"}"#;
    let used = |cwd: &std::path::Path| -> u64 {
        let found = aikit_in(
            &temp,
            cwd,
            &["knowledge", "search", "--limit", "50", "--", "compass"],
        );
        let hit = found["hits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|hit| hit["resource"] == "wiki:node:compass")
            .unwrap_or_else(|| panic!("Compass is found from {}: {found}", cwd.display()))
            .clone();
        ["destination", "route"]
            .iter()
            .map(|kind| {
                hit["ranking"][kind]["contextual_observations"]
                    .as_u64()
                    .unwrap_or(0)
            })
            .sum()
    };
    let history = |cwd: &std::path::Path| {
        aikit_in(&temp, cwd, &["knowledge", "history"])
            .as_array()
            .unwrap()
            .len()
    };

    // Learned use recorded before worlds reached AIKit carries no world.
    used(&root);
    aikit_store::append_familiarity_observation(
        &aikit_store::Index::open(&AikitHome::at(temp.path().join("aikit-home")).database())
            .unwrap(),
        aikit_core::FamiliarityObservation::destination(
            "knowledge-open-use/legacy",
            ResourceRef::parse("wiki:node:compass").unwrap(),
            aikit_core::FamiliarityContext::default(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        ),
    )
    .unwrap();

    aikit_in(&temp, &alpha, &["knowledge", "route", "--", compass]);
    aikit_in(&temp, &alpha, &["knowledge", "route", "--", compass]);
    aikit_in(&temp, &beta, &["knowledge", "route", "--", compass]);
    assert_eq!(
        (used(&alpha), used(&beta), used(&root)),
        (2, 1, 1),
        "Alpha counts its two opens, Beta its one, your world the use it already had"
    );
    assert_eq!((history(&alpha), history(&beta), history(&root)), (2, 1, 0));

    let forgot = aikit_in(
        &temp,
        &alpha,
        &["knowledge", "forget", "here", "wiki:node:compass"],
    );
    assert_eq!(
        forgot["forgot"]["DestinationIn"]["project"], "project:Alpha",
        "the forget names the world it was asked in"
    );
    assert_eq!((used(&alpha), used(&beta), used(&root)), (0, 1, 1));

    let forgot = aikit_in(
        &temp,
        &root,
        &["knowledge", "forget", "here", "wiki:node:compass"],
    );
    assert!(
        forgot["forgot"]["DestinationIn"]["project"].is_null(),
        "your world is the unscoped one"
    );
    assert_eq!((used(&alpha), used(&beta), used(&root)), (0, 1, 0));
}
