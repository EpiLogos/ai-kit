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

/// `aikit --json -C <project> ARGS` as an agent session's own process runs it (`AIKIT_SESSION_ID`), or as the person asks it.
fn aikit_as(
    project: &std::path::Path,
    home: &std::path::Path,
    agent: Option<&str>,
    args: &[&str],
) -> serde_json::Value {
    let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin("aikit"));
    command
        .arg("--json")
        .arg("-C")
        .arg(project)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home.join("home"))
        .env("AIKIT_HOME", home.join("aikit-home"));
    if let Some(agent) = agent {
        command.env("AIKIT_SESSION_ID", agent);
    }
    let output = command.output().expect("the real aikit binary runs");
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "aikit {args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// O-I #745 (DLW §19.9 Sources, BD round 25): Search first, Link depth and Read budget are AIKit's settings on the placement
/// (encounter-session-settings `sources`), and AIKit's knowledge navigation honours them when the session's agent reads — it
/// searches before it reads, follows at most Link depth links from a hit, and reads no more than its budget, each refused in words.
/// What it read is AIKit's record of its run: each passage with its source and the revision it read, a moved one marked; Read
/// again reads it at the new revision and records the use; Forget drops AIKit's learned use of it, and the source stays as it was.
#[test]
fn an_agent_reads_under_its_sources_and_its_run_reads_back_again_and_forgotten() {
    use aikit_cli::encounter_service::EncounterService;
    use aikit_core::session_space::SessionSpaceRef;
    use aikit_core::session_space_application::{
        SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
    };
    use aikit_store::SessionSpaceApplicationStore;

    let temp = TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let wiki = |alpha_revision: u64| {
        serde_json::json!({"objects": [
            {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:need","revision":1,"provenance":[],"title":"Need",
             "parent_space_refs":[],"child_space_refs":[],"node_refs":["wiki:node:alpha","wiki:node:beta"]},
            {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:alpha","revision":alpha_revision,"provenance":[],"type":"Concept",
             "title":"Alpha","space_refs":["wiki:space:need"],"source_refs":[]},
            {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:beta","revision":1,"provenance":[],"type":"Concept",
             "title":"Beta","space_refs":["wiki:space:need"],"source_refs":[]}
        ]})
        .to_string()
    };
    fs::write(project.join("semantic-wiki.json"), wiki(1)).unwrap();
    let home = AikitHome::at(root.join("aikit-home"));
    home.ensure_layout().unwrap();
    let spaces = SessionSpaceApplicationStore::new(home.clone());
    let space = SessionSpaceRef::parse("session-space/sources").unwrap();
    spaces
        .apply(
            &spaces
                .stage(
                    None,
                    SessionSpaceMutation::Create {
                        id: space.clone(),
                        label: None,
                    },
                )
                .unwrap(),
        )
        .unwrap();
    let session = ResourceRef::parse("agent-session/sources-reader").unwrap();
    spaces
        .apply(
            &spaces
                .stage(
                    Some(&space),
                    SessionSpaceMutation::AttachAgentSession {
                        attachment: SessionSpaceAgentAttachmentIntent {
                            agent_session: session.clone(),
                            purpose: Some("Sources".into()),
                            provenance: vec!["isolated Sources test".into()],
                        },
                    },
                )
                .unwrap(),
        )
        .unwrap();
    let agent = aikit_core::SessionId::of_agent_session(&session).to_string();
    let address =
        |reference: &str| serde_json::json!({"kind": "wiki", "value": reference}).to_string();
    let agent_does = |args: &[&str]| aikit_as(&project, &root, Some(&agent), args);
    let refusal = |reply: &serde_json::Value| {
        (reply["ok"] == false).then(|| {
            reply["error"]["code"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
    };
    let nodes = |reply: &serde_json::Value| -> Vec<String> {
        reply["data"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|node| node["resource"].as_str().map(str::to_owned))
            .collect()
    };

    // Unplaced, the agent's reading is unbounded: the walk from Alpha reaches Beta, two links away.
    assert!(nodes(&aikit_as(
        &project,
        &root,
        None,
        &[
            "knowledge",
            "relations",
            "--depth",
            "2",
            "--",
            &address("wiki:node:alpha")
        ]
    ))
    .contains(&"wiki:node:beta".to_string()));

    // Sources placed on the agent's own chain: Search first (on by default), one link from a hit, two pages.
    let written = EncounterService::write_session_settings(
        &home,
        &session,
        serde_json::json!({"track": {"sources": true, "link_depth": 1, "read_budget": 2}}),
        None,
    )
    .unwrap();
    assert_eq!(
        written["sources"]["chain"],
        serde_json::json!({"placed": true, "search_first": true, "link_depth": 1, "read_budget": 2,
        "set": ["sources", "link_depth", "read_budget"]})
    );
    assert_eq!(
        written["sources"]["rack"]["placed"], false,
        "the rack's shared reading is not placed"
    );
    let refused = EncounterService::write_session_settings(
        &home,
        &session,
        serde_json::json!({"track": {"link_depth": 5}}),
        Some(&aikit_core::SourceRevision::parse(written["revision"].as_str().unwrap()).unwrap()),
    );
    assert!(refused
        .unwrap_err()
        .to_string()
        .contains("Link depth is a whole number from 0 to 4"));

    assert_eq!(
        refusal(&agent_does(&[
            "knowledge",
            "read",
            "--",
            &address("wiki:node:beta")
        ]))
        .as_deref(),
        Some("knowledge.sources_search_first"),
        "it searches before it reads"
    );
    let found = agent_does(&["knowledge", "search", "Alpha"]);
    assert_eq!(found["ok"], true, "{found}");
    assert_eq!(
        agent_does(&["knowledge", "read", "--", &address("wiki:node:alpha")])["ok"],
        true,
        "a page the search found reads"
    );
    let walked = agent_does(&[
        "knowledge",
        "relations",
        "--depth",
        "2",
        "--",
        &address("wiki:node:alpha"),
    ]);
    assert_eq!(walked["ok"], true, "{walked}");
    assert!(
        nodes(&walked).contains(&"wiki:space:need".to_string()),
        "one link from the hit: {walked}"
    );
    assert!(
        !nodes(&walked).contains(&"wiki:node:beta".to_string()),
        "the walk stops at Link depth: {walked}"
    );
    assert_eq!(
        refusal(&agent_does(&[
            "knowledge",
            "relations",
            "--",
            &address("wiki:space:need")
        ]))
        .as_deref(),
        Some("knowledge.sources_link_depth"),
        "no link past Link depth"
    );
    assert_eq!(
        agent_does(&["knowledge", "read", "--", &address("wiki:space:need")])["ok"],
        true,
        "a page one link from a hit reads"
    );
    let over = agent_does(&["knowledge", "read", "--", &address("wiki:node:alpha")]);
    assert_eq!(
        refusal(&over).as_deref(),
        Some("knowledge.sources_read_budget"),
        "the budget is two pages: {over}"
    );
    assert!(over["error"]["message"]
        .as_str()
        .unwrap()
        .contains("has read 2 of its 2 pages"));

    // Search first off and a larger budget: a page read without a search is its own hit.
    let settings = EncounterService::read_session_settings(&home, &session).unwrap();
    EncounterService::write_session_settings(
        &home,
        &session,
        serde_json::json!({"track": {"search_first": false, "read_budget": 3}}),
        Some(&aikit_core::SourceRevision::parse(settings["revision"].as_str().unwrap()).unwrap()),
    )
    .unwrap();
    assert_eq!(
        agent_does(&["knowledge", "read", "--", &address("wiki:node:beta")])["ok"],
        true
    );

    // Read: the run's passages, each with its source and the revision it read; Alpha's source moves.
    let person = |args: &[&str]| aikit_as(&project, &root, None, args);
    fs::write(project.join("semantic-wiki.json"), wiki(2)).unwrap();
    let before = fs::read(project.join("semantic-wiki.json")).unwrap();
    let read = person(&["knowledge", "reads", "--agent-session", session.as_str()]);
    assert_eq!(read["ok"], true, "{read}");
    let rows = read["data"]["readings"].as_array().unwrap().clone();
    let of = |rows: &[serde_json::Value], resource: &str| {
        rows.iter().find(|row| row["resource"] == resource).cloned()
    };
    assert_eq!(
        rows.iter()
            .map(|row| row["resource"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["wiki:node:alpha", "wiki:space:need", "wiki:node:beta"]
    );
    let alpha = of(&rows, "wiki:node:alpha").unwrap();
    assert_eq!(
        (
            alpha["revision"].as_str(),
            alpha["moved"].as_bool(),
            alpha["hops"].as_u64()
        ),
        (Some("1"), Some(true), Some(0)),
        "{alpha}"
    );
    assert_eq!(of(&rows, "wiki:space:need").unwrap()["hops"], 1);
    assert_eq!(of(&rows, "wiki:space:need").unwrap()["moved"], false);
    assert_eq!(read["data"]["sources"]["placement"], "chain");

    // Read again: Alpha at its new revision, the use recorded.
    let again = person(&[
        "knowledge",
        "read-again",
        "--agent-session",
        session.as_str(),
        "--reading",
        alpha["id"].as_str().unwrap(),
    ]);
    assert_eq!(again["ok"], true, "{again}");
    assert_eq!(again["data"]["reading"]["revision"], "2");
    let rows = person(&["knowledge", "reads", "--agent-session", session.as_str()])["data"]
        ["readings"]
        .as_array()
        .unwrap()
        .clone();
    let alpha = of(&rows, "wiki:node:alpha").unwrap();
    assert_eq!(
        (alpha["revision"].as_str(), alpha["moved"].as_bool()),
        (Some("2"), Some(false)),
        "it stands at the new revision"
    );

    // Forget: the reading is gone from the run, AIKit's learned use of it dropped; the source is untouched.
    let space_reading = of(&rows, "wiki:space:need").unwrap();
    let forgot = person(&[
        "knowledge",
        "forget",
        "reading",
        "--agent-session",
        session.as_str(),
        "--reading",
        space_reading["id"].as_str().unwrap(),
    ]);
    assert_eq!(forgot["ok"], true, "{forgot}");
    assert_eq!(
        forgot["data"]["forgot"]["DestinationIn"]["destination"],
        "wiki:space:need"
    );
    let rows = person(&["knowledge", "reads", "--agent-session", session.as_str()])["data"]
        ["readings"]
        .as_array()
        .unwrap()
        .clone();
    assert!(of(&rows, "wiki:space:need").is_none(), "forgotten");
    assert_eq!(
        fs::read(project.join("semantic-wiki.json")).unwrap(),
        before,
        "the source is untouched"
    );
    let gone = person(&[
        "knowledge",
        "forget",
        "reading",
        "--agent-session",
        session.as_str(),
        "--reading",
        space_reading["id"].as_str().unwrap(),
    ]);
    assert_eq!(
        refusal(&gone).as_deref(),
        Some("knowledge.session_reading_absent")
    );
}
