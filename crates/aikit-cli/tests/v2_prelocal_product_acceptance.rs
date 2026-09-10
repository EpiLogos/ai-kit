//! The cloud-achievable V2 product route as one acceptance lane.
//!
//! This deliberately crosses the production contracts instead of introducing a
//! test-only application architecture: real CLI `Service`/store resolution and
//! Generation, ResourceRef-native search, native SemanticWiki + SourcePool
//! traversal, KnowledgeRoute familiarity evidence, and the canonical
//! HarnessComposition resolver/mutation grammar.
//!
//! Physical target materialisation is intentionally not asserted here. Accepting
//! a confirmed HarnessComposition preview yields a desired `Resolved` body; a
//! target/provider must separately prove stronger material truth.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use aikit_cli::app::{AikitApplication, ApplyRequest, Service};
use aikit_core::resource::{ResourceKind, ResourceRef, SourceRef, SourceRevision};
use aikit_core::{
    apply_confirmed_harness_composition, diff_harness_compositions,
    preview_harness_composition_change, resolve_harness_composition, ActivationScope,
    ActivationScopeKind, ComponentContribution, ComponentDescriptor, ComponentSelection,
    CompositionActivationMode, CompositionCatalog, CompositionState, ContributionKind,
    FamiliarityContext, FamiliarityStore, HarnessCompositionRequest, KnowledgeAddress,
    KnowledgeApplication, LifetimeOwner, LifetimeOwnerKind, NativeSourcePoolProvider,
    ResolutionScope, RetractionMode, SemanticWikiIndex, SemanticWikiProvider, SourceBinding,
    SourceMaterial, SourcePoolProvider, SourceVisibility, StagedHarnessComposition,
    SurfaceDescriptor, SurfaceKind, DEFAULT_FAMILIARITY_HALF_LIFE_MS,
};
use aikit_core::scope::ScopeKind;
use aikit_store::home::AikitHome;
use aikit_tui::PaletteBackend;
use tempfile::TempDir;

const CONTEXT_ID: &str = "ctx_01HZYPRELOCAL0000000000000";

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn seed_service(home: &Path, project: &Path) -> Service {
    let base = home.join("registries/personal/capsules/script/demo/greet");
    write(
        &base.join("manifest.toml"),
        r#"schema = 1
id = "script/demo/greet"
kind = "script"
name = "greet"
description = "Pre-local acceptance capability."

[script]
entry = "payload/run.sh"
exports = ["greet"]
"#,
    );
    let run = base.join("payload/run.sh");
    write(&run, "#!/bin/sh\necho hi\n");
    let mut perms = fs::metadata(&run).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&run, perms).unwrap();
    write(
        &project.join(".aikit/profile.toml"),
        "schema = 1\nenable = [\"script/demo/greet\"]\n",
    );

    let mut env = BTreeMap::new();
    env.insert("AIKIT_CONTEXT_ID".to_string(), CONTEXT_ID.to_string());
    Service::open(AikitHome::at(home), project, move |key| env.get(key).cloned()).unwrap()
}

fn knowledge_fixture() -> (
    SemanticWikiIndex,
    NativeSourcePoolProvider,
    Vec<SourceMaterial>,
    FamiliarityContext,
) {
    let objects = aikit_core::parse_wiki_objects(
        r#"{"objects":[
          {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:root","revision":1,
           "provenance":[],"title":"Root","parent_space_refs":[],"child_space_refs":[],
           "node_refs":["wiki:node:auth"]},
          {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:auth","revision":1,
           "provenance":[{"source_ref":"source:spec"}],"type":"Concept","title":"Authentication",
           "space_refs":["wiki:space:root"],"source_refs":["source:spec"]}
        ]}"#,
    )
    .unwrap();
    let index = SemanticWikiIndex::rebuild(objects).unwrap();
    let material = vec![SourceMaterial {
        binding: SourceBinding {
            source: SourceRef::parse("source:spec").unwrap(),
            revision: SourceRevision::parse("sha256:spec").unwrap(),
            title: "Auth spec".into(),
            tags: vec!["auth".into()],
            visibility: SourceVisibility::Team,
            owners: Vec::new(),
            media_type: "text/markdown".into(),
            locator: None,
            metadata: BTreeMap::new(),
        },
        body: "Authentication rotates session tokens.".into(),
    }];
    let mut sources = NativeSourcePoolProvider::new();
    sources.rebuild(&material).unwrap();
    let context = FamiliarityContext {
        project: Some(r("project:acceptance")),
        actor: Some(r("agent/reviewer")),
        agency: Some(r("agency/acceptance")),
        focus: Some("authentication".into()),
    };
    (index, sources, material, context)
}

fn selection(component: &ResourceRef) -> ComponentSelection {
    ComponentSelection {
        component: component.clone(),
        resolution_scope: ResolutionScope::new(ScopeKind::Project, ".aikit/project.toml"),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("session:acceptance"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::Generation)
            .with_reference("generation:acceptance"),
        activation_mode: CompositionActivationMode::Generated,
    }
}

fn composition_catalog() -> (CompositionCatalog, ResourceRef, ResourceRef, ResourceRef, ResourceRef) {
    let component = r("component/review-runtime");
    let action = r("action/review");
    let tui = r("surface/aikit/tui");
    let agent_tool = r("surface/aikit/agent-tool");
    let mut catalog = CompositionCatalog::default();

    for (surface, kind, native) in [
        (tui.clone(), SurfaceKind::Tui, "workspace.review"),
        (agent_tool.clone(), SurfaceKind::AgentTool, "review"),
    ] {
        catalog.insert_surface(SurfaceDescriptor {
            resource: surface,
            kind,
            target_native_id: Some(native.into()),
            owner_component: Some(component.clone()),
        });
    }

    let contribution = |id: &str, surface: ResourceRef| ComponentContribution {
        id: r(id),
        component: component.clone(),
        kind: ContributionKind::ActionProjection,
        target_contract: None,
        exposed_ref: Some(action.clone()),
        exposed_kind: Some(ResourceKind::Action),
        surface: Some(surface),
        activation_scope: ActivationScope::new(ActivationScopeKind::AgentSession)
            .with_reference("session:acceptance"),
        lifetime_owner: LifetimeOwner::new(LifetimeOwnerKind::Generation)
            .with_reference("generation:acceptance"),
        activation_mode: CompositionActivationMode::Generated,
        retraction_mode: RetractionMode::NextSession,
        provenance: vec!["acceptance:v2-prelocal-product-route".into()],
    };

    let mut descriptor = ComponentDescriptor::new(component.clone());
    descriptor.supported_surfaces = vec![tui.clone(), agent_tool.clone()];
    descriptor.contributions = vec![
        contribution("contribution/review/tui", tui.clone()),
        contribution("contribution/review/agent", agent_tool.clone()),
    ];
    descriptor.activation_modes = BTreeSet::from([CompositionActivationMode::Generated]);
    catalog.insert_component(descriptor);
    (catalog, component, action, tui, agent_tool)
}

#[test]
fn project_knowledge_familiarity_composition_generation_and_agent_projection_form_one_route() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let mut service = seed_service(home.path(), project.path());

    // Project -> resolved Context -> universal ResourceRef-native Search.
    let field = <Service as PaletteBackend>::navigation_index(&service);
    let hits = field.search("greet", 10);
    assert!(hits
        .iter()
        .any(|hit| hit.resource.as_str() == "script/demo/greet"));
    assert_eq!(<Service as PaletteBackend>::context(&service).context_id.to_string(), CONTEXT_ID);

    // SemanticWiki Node -> Explain -> real Wiki->Source relation traversal -> route.
    let (wiki, sources, material, familiarity_context) = knowledge_fixture();
    let knowledge = KnowledgeApplication::new(familiarity_context.clone())
        .with_wiki(SemanticWikiProvider::new(&wiki))
        .with_source_pool(&sources, &material);
    let node = KnowledgeAddress::Wiki(r("wiki:node:auth"));
    let source = KnowledgeAddress::Source(SourceRef::parse("source:spec").unwrap());
    assert!(knowledge.search("Authentication", 10).hits.iter().any(|hit| hit.resource == r("wiki:node:auth")));
    let explanation = knowledge.explain(&node).unwrap();
    assert!(explanation.summary.starts_with("node r1;"));
    assert!(explanation.sources.contains(&SourceRef::parse("source:spec").unwrap()));
    let relations = knowledge.relations(&node, 1, 16, 16).unwrap();
    assert!(relations.nodes.iter().any(|related| related.resource == r("source:spec")));
    let route = knowledge
        .route(Some("authentication"), &[node.clone(), source.clone()])
        .unwrap();
    assert_eq!(route.destination(), Some(&r("source:spec")));

    // Route -> familiarity. Learned accessibility is evidence only; it does not
    // rewrite Resource identity or provider relations.
    let observation = route
        .familiarity_observation("obs:prelocal:route:1", 1_000)
        .unwrap()
        .from_surface(r("surface/aikit/tui"))
        .via_action(r("action/knowledge/open"));
    let mut familiarity = FamiliarityStore::new();
    familiarity.record(observation).unwrap();
    let assessment = familiarity.assess_route(
        &route.route,
        route.destination().unwrap(),
        &familiarity_context,
        1_001,
        DEFAULT_FAMILIARITY_HALF_LIFE_MS,
    );
    assert_eq!(assessment.observations, 1);
    assert_eq!(assessment.contextual_observations, 1);

    // Durable package composition -> immutable Generation on the real store.
    let generation = AikitApplication::apply(
        &mut service,
        ApplyRequest {
            scope: ScopeKind::Project,
            toggles: vec![],
            label: Some("prelocal-product-route".into()),
        },
    )
    .unwrap();
    assert_eq!(
        service.current_generation_properties().get("label").map(String::as_str),
        Some("prelocal-product-route")
    );

    // Compose actor/runtime -> inspect HarnessComposition -> stage Component /
    // Surface projection -> preview -> explicit confirm -> desired resolved body.
    let (catalog, component, action, tui, agent_tool) = composition_catalog();
    let request = HarnessCompositionRequest {
        harness: r("harness/deepseek/acceptance"),
        project: Some(r("project:acceptance")),
        agent: Some(r("agent/reviewer")),
        agency: Some(r("agency/acceptance")),
        session: Some("session:acceptance".into()),
        model: Some(r("model/deepseek/acceptance")),
        selections: Vec::new(),
        target_revision: Some("deepseek:acceptance-r1".into()),
        generation: Some(generation.id.to_string()),
    };
    let before = resolve_harness_composition(&catalog, request).unwrap();
    let mut staged = StagedHarnessComposition::new();
    staged.select(selection(&component));
    let preview = preview_harness_composition_change(&catalog, &before, staged).unwrap();
    assert_eq!(preview.diff.mounted_components, vec![component.clone()]);
    assert_eq!(
        preview.diff.added_surfaces.iter().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([tui.clone(), agent_tool.clone()])
    );
    let desired = apply_confirmed_harness_composition(preview.confirm());
    assert_eq!(desired.state, CompositionState::Resolved);
    assert_eq!(desired.generation.as_deref(), Some(generation.id.to_string().as_str()));

    // Human and agent-facing projections are the same canonical Action identity,
    // not target-native replacements.
    assert_eq!(desired.projections.len(), 2);
    assert!(desired
        .projections
        .iter()
        .all(|projection| projection.canonical_ref == action));
    assert_eq!(
        desired
            .projections
            .iter()
            .map(|projection| projection.surface.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([tui, agent_tool])
    );

    // Explain/History/reuse: the existing diff grammar describes the accepted
    // body and a retraction can recover the prior body through the same resolver.
    let history = diff_harness_compositions(&before, &desired).unwrap();
    assert_eq!(history.mounted_components, vec![component.clone()]);
    let mut retract = StagedHarnessComposition::new();
    retract.retract(component);
    let restore = preview_harness_composition_change(&catalog, &desired, retract).unwrap();
    assert!(restore.projected.component_bindings.is_empty());
    assert_eq!(restore.projected.generation, before.generation);
    assert_eq!(route.destination(), Some(&r("source:spec")), "the prior KnowledgeRoute remains reusable and identity-stable");
}