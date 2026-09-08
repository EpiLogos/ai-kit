//! Deterministic acceptance evidence for issue #211 workstream W2 ("typed
//! relation presentation + real Graph").
//!
//! Three registers of fixture are used, each for the coverage it is actually
//! suited to:
//!
//! - The resolver-fallback fixture (`skill/alpha` declaring `related_skills`
//!   on `skill/beta`/`skill/gamma`, the same shape `graph_surface_v2.rs`
//!   already exercises) drives the *full* `ApplicationSurfaceController`
//!   pipeline — real keyboard/mouse dispatch, real `reduce_tui`, real
//!   rendering — for the UI-mechanics acceptance criteria (state parity,
//!   recenter/back, layout-cache staticness, keyboard/mouse parity, narrow
//!   fallback, resize).
//! - A real [`aikit_core::SemanticWikiIndex`] + [`aikit_core::NativeSourcePoolProvider`]
//!   neighbourhood, reached through [`aikit_tui::application_service::ApplicationService`]
//!   over a minimal [`aikit_tui::backend::PaletteBackend`] that answers
//!   `knowledge_address`/`knowledge_relations` from a genuine
//!   [`aikit_core::KnowledgeApplication`] (the same construction idiom as
//!   `knowledge_service.rs`'s own `#[cfg(test)]` module and
//!   `application_service.rs`'s own relation tests), proves placement and
//!   provenance survive a real fetch -> typed view -> layout -> render round
//!   trip.
//! - Hand-built [`aikit_core::KnowledgeRelationView`]s (the same technique
//!   `graph_layout.rs`'s own unit tests use) exercise bounded/degraded input
//!   directly against `graph_layout::layout`/`graph_presentation::*`, which
//!   are pure functions and need no backend at all.

mod common;

use std::collections::BTreeMap;

use common::*;

use aikit_core::capsule::Capsule;
use aikit_core::catalog::MemoryCatalog;
use aikit_core::context::ContextDescriptor;
use aikit_core::id::{CapsuleId, GenerationId};
use aikit_core::policy::ManagedPolicy;
use aikit_core::resolve::{resolve, ResolveRequest, ResolvedView};
use aikit_core::resource::{ResourceKind, ResourceRef, SourceRef, SourceRevision};
use aikit_core::scope::ScopeKind;
use aikit_core::search::SearchDoc;
use aikit_core::trust::MemoryTrust;
use aikit_core::{
    parse_wiki_objects, FamiliarityContext, KnowledgeAddress, KnowledgeApplication,
    KnowledgeOperations, KnowledgeRelationView, NativeSourcePoolProvider, RelationDirection,
    RelationNode, RelationQuery, Result, SemanticWikiIndex, SemanticWikiProvider, SourceAuthority,
    SourceBinding, SourceMaterial, SourcePoolProvider, SourceVisibility,
};

use aikit_tui::application_service::ApplicationService;
use aikit_tui::application_surface::{ApplicationSurfaceController, ApplicationSurfaceRequest};
use aikit_tui::backend::{JobOutput, PaletteBackend, Projected, PromotionDraft, RunIntent, Toggle};
use aikit_tui::event::PaletteEvent;
use aikit_tui::graph_layout::{layout as graph_layout, GraphLayoutRequest, GraphViewport, RelationBand};
use aikit_tui::graph_presentation;
use aikit_tui::host::UiHost;
use aikit_tui::layout::Layout;
use aikit_tui::{RelationView, TuiApplicationService};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// Shared small helpers
// ---------------------------------------------------------------------------

fn key(code: KeyCode) -> PaletteEvent {
    PaletteEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn mouse(column: u16, row: u16, modifiers: KeyModifiers) -> PaletteEvent {
    PaletteEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers,
    })
}

fn draw(surface: &ApplicationSurfaceController, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    surface.draw_terminal(&mut terminal).unwrap();
    terminal
}

/// The rendered buffer as one row per terminal line, trailing blanks
/// trimmed. Readable for assertions/snapshots, unlike the single flattened
/// string other V2 surface tests use — a multi-row diff is legible in a
/// snapshot review; a single 3600-character line is not.
fn rendered_rows(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "))
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn lines_to_text(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn viewport_request(width: u16, height: u16) -> GraphLayoutRequest {
    GraphLayoutRequest::for_viewport(GraphViewport::new(width, height))
}

/// Two real skills resolved through the real catalogue/trust/resolve
/// pipeline, `alpha` declaring `beta` and `gamma` as `related_skills` — the
/// same resolver-fallback shape `graph_surface_v2.rs` and
/// `application_service.rs`'s own tests use.
fn resolver_fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let alpha = skill_with(
        "skill/alpha",
        "related_skills = [\"skill/beta\", \"skill/gamma\"]\n",
    );
    let backend = Fixture::new(dir.path(), vec![alpha, skill("skill/beta"), skill("skill/gamma")]);
    (dir, backend)
}

/// Enter the Graph projection with `alpha` selected, exactly as a viewer
/// would: resize, select the one search hit, then Ctrl+T through
/// List -> Tree -> Graph.
fn enter_graph(surface: &mut ApplicationSurfaceController, backend: &mut Fixture, width: u16, height: u16) {
    surface.handle(backend, PaletteEvent::Resize(width, height)).unwrap();
    surface.handle(backend, key(KeyCode::Down)).unwrap();
    for _ in 0..2 {
        surface
            .handle(backend, PaletteEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)))
            .unwrap();
    }
    assert_eq!(surface.semantic().relation_view, RelationView::Graph);
}

fn ctrl_t(surface: &mut ApplicationSurfaceController, backend: &mut Fixture) {
    surface
        .handle(backend, PaletteEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)))
        .unwrap();
}

// ===========================================================================
// 1. STATE PARITY — the headline acceptance criterion
// ===========================================================================

#[test]
fn selected_resource_survives_list_tree_graph_list_with_no_drift() {
    let (_dir, mut backend) = resolver_fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    surface.handle(&mut backend, PaletteEvent::Resize(120, 30)).unwrap();
    surface.handle(&mut backend, key(KeyCode::Down)).unwrap();

    assert_eq!(surface.semantic().relation_view, RelationView::List);
    let original = surface.semantic().selected.clone().expect("a selection exists");
    assert_eq!(original.as_str(), "skill/alpha");

    ctrl_t(&mut surface, &mut backend); // List -> Tree
    assert_eq!(surface.semantic().relation_view, RelationView::Tree);
    assert_eq!(surface.semantic().selected, Some(original.clone()), "Tree must not drift selection");

    ctrl_t(&mut surface, &mut backend); // Tree -> Graph
    assert_eq!(surface.semantic().relation_view, RelationView::Graph);
    assert_eq!(surface.semantic().selected, Some(original.clone()), "Graph must not drift selection");

    ctrl_t(&mut surface, &mut backend); // Graph -> List
    assert_eq!(surface.semantic().relation_view, RelationView::List);
    assert_eq!(
        surface.semantic().selected,
        Some(original),
        "the full List -> Tree -> Graph -> List round trip must return exactly the resource it started with"
    );
}

// ===========================================================================
// 2. RECENTER / BACK
// ===========================================================================

#[test]
fn recenter_changes_focus_and_pushes_history_and_back_restores_it() {
    let (_dir, mut backend) = resolver_fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);
    let alpha = ResourceRef::parse("skill/alpha").unwrap();
    assert_eq!(surface.semantic().graph.focus, Some(alpha.clone()));
    assert!(surface.semantic().graph.history.is_empty());

    surface.handle(&mut backend, key(KeyCode::Right)).unwrap();
    let neighbour = surface.semantic().selected.clone().unwrap();
    assert_ne!(neighbour, alpha);

    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    assert_eq!(
        surface.semantic().graph.focus,
        Some(neighbour.clone()),
        "recenter must move focus onto the highlighted node"
    );
    assert_eq!(
        surface.semantic().graph.history,
        vec![alpha.clone()],
        "recenter must push the previous focus onto history"
    );
    assert_eq!(
        surface.relation().unwrap().subject,
        neighbour,
        "the fetched neighbourhood must follow the new focus"
    );

    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert_eq!(
        surface.semantic().graph.focus,
        Some(alpha.clone()),
        "Back must restore the prior focus"
    );
    assert!(surface.semantic().graph.history.is_empty(), "the restored focus is popped, not retained");
    assert_eq!(surface.relation().unwrap().subject, alpha);
}

#[test]
fn back_with_empty_graph_history_falls_through_to_ordinary_back_without_being_swallowed() {
    let (_dir, mut backend) = resolver_fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);
    assert!(surface.semantic().graph.history.is_empty());

    // Nothing is on the ordinary navigation stack in this scenario either, so
    // ordinary Back is a documented no-op — the assertion is that Esc still
    // reaches `handle_key`'s ordinary Back arm (does not panic, and leaves
    // state exactly as ordinary Back would) rather than being silently
    // absorbed by a Graph-history pop that had nothing to pop.
    let before = surface.semantic().clone();
    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap();
    assert_eq!(surface.semantic().selected, before.selected);
    assert_eq!(surface.semantic().graph.focus, before.graph.focus);
    assert_eq!(surface.semantic().navigation, before.navigation);
}

// ===========================================================================
// 3. LAYOUT STABILITY — recomputed only on genuine input change
// ===========================================================================

#[test]
fn repeated_renders_of_unchanged_state_produce_byte_identical_geometry() {
    let (_dir, mut backend) = resolver_fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);

    let first = rendered_rows(&draw(&surface, 120, 30));
    let second = rendered_rows(&draw(&surface, 120, 30));
    assert_eq!(first, second, "drawing twice with nothing changed must render identically");
}

#[test]
fn cursor_movement_alone_never_recomputes_the_cached_layout_but_a_genuine_change_does() {
    let (_dir, mut backend) = resolver_fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);
    let after_entry = surface.graph_layout_recompute_count();
    assert!(after_entry > 0, "entering Graph must compute a layout at least once");

    // Cursor movement (arrow/hjkl) dispatches `GraphSelectNode`, which moves
    // `selected` but not `graph.focus` — the fetched relation view is
    // unchanged, so the cache key is unchanged, so this must be a pure hit.
    surface.handle(&mut backend, key(KeyCode::Right)).unwrap();
    surface.handle(&mut backend, key(KeyCode::Left)).unwrap();
    assert_eq!(
        surface.graph_layout_recompute_count(),
        after_entry,
        "cursor movement between already-laid-out nodes must never recompute the cached layout"
    );

    // A genuine change to the graph-local filter narrows the view fed to
    // `graph_layout::layout` — the cache key changes, so this must recompute.
    surface.handle(&mut backend, key(KeyCode::Char('/'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Char('b'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    assert!(
        surface.graph_layout_recompute_count() > after_entry,
        "a genuine filter change must recompute the cached layout"
    );
}

// ===========================================================================
// 4. PLACEMENT — against a real provider-bearing fixture
// ===========================================================================

/// `okf-wiki/v1` objects covering all four `RelationBand`s from a single
/// real [`aikit_core::SemanticWikiIndex`], queried from more than one
/// resource's own perspective so both halves of containment — the
/// container's outgoing side *and* the member's/child's incoming side
/// (added in commit cc1e152, `knowledge_wiki_provider.rs::relations`) — are
/// each proven from genuine provider output, never from a hand-authored
/// edge built to match the TUI's own placement table:
///
/// - `root --member--> wiki:node:member` and `root --child-space--> wiki:space:child`
///   are genuine `WikiSpace` membership, read from `root`'s own focus —
///   Contained.
/// - `wiki:space:child` has `parent_space_refs: [root]`, so focusing
///   `wiki:space:child` itself reaches the *enclosing* half of that same
///   membership: `root --child-space--> wiki:space:child`, now `Incoming`
///   relative to `child` — Context. Same relation string as the line
///   above, opposite band, because the structural role (which endpoint the
///   focus sits at) flipped, not the vocabulary.
/// - `wiki:node:member` has `space_refs: [root]`, so focusing
///   `wiki:node:member` itself reaches `root --member--> wiki:node:member`,
///   `Incoming` relative to `member`, at the Space's own revision (3) —
///   Context. This is the edge that did not exist before cc1e152: a node
///   focus could not previously reach its own enclosing Space at all.
/// - `root --cites--> wiki:node:cites-target` is a plain, non-containment
///   authored edge — Outgoing.
/// - `wiki:node:incoming-src --grounded-in--> root` is the same shape read
///   from root's own incoming side — Incoming.
/// - `root --member-of--> wiki:space:universe` is a plain authored
///   WikiEdge whose relation string a human happened to name like
///   containment. The provider asserts no containment meaning for it — it
///   is not one of the three exact strings (`member`, `child-space`,
///   `local-member`) `SemanticWikiProvider::relations` emits for genuine
///   membership — so it must land by `RelationDirection` alone: Outgoing.
///   This is deliberate negative coverage: an authored edge named like
///   containment is not containment.
const WIKI_FIXTURE_JSON: &str = r#"{"objects":[
  {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:root","revision":3,
   "title":"Root","child_space_refs":["wiki:space:child"],"node_refs":["wiki:node:member"]},
  {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:child","revision":1,
   "title":"Child","parent_space_refs":["wiki:space:root"]},
  {"profile":"okf-wiki/v1","object":"space","ref":"wiki:space:universe","revision":1,
   "title":"Universe"},
  {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:member","revision":1,
   "type":"Concept","title":"Refresh Token","space_refs":["wiki:space:root"],
   "source_refs":["source:spec"]},
  {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:cites-target","revision":1,
   "type":"Concept","title":"Token Rotation"},
  {"profile":"okf-wiki/v1","object":"node","ref":"wiki:node:incoming-src","revision":1,
   "type":"Concept","title":"Session Handling"},
  {"profile":"okf-wiki/v1","object":"edge","ref":"wiki:edge:cites","revision":1,
   "from_ref":"wiki:space:root","to_ref":"wiki:node:cites-target","relation":"cites","origin":"authored"},
  {"profile":"okf-wiki/v1","object":"edge","ref":"wiki:edge:grounded-in","revision":1,
   "from_ref":"wiki:node:incoming-src","to_ref":"wiki:space:root","relation":"grounded-in","origin":"authored"},
  {"profile":"okf-wiki/v1","object":"edge","ref":"wiki:edge:member-of","revision":1,
   "from_ref":"wiki:space:root","to_ref":"wiki:space:universe","relation":"member-of","origin":"authored"}
]}"#;

fn familiarity_context() -> FamiliarityContext {
    FamiliarityContext {
        project: Some(ResourceRef::parse("project:demo").unwrap()),
        actor: None,
        agency: None,
        focus: None,
    }
}

fn wiki_fixture() -> (SemanticWikiIndex, Vec<SourceMaterial>, NativeSourcePoolProvider) {
    let objects = parse_wiki_objects(WIKI_FIXTURE_JSON).unwrap();
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
        body: "Refresh tokens rotate on every session renewal.".into(),
    }];
    let mut sources = NativeSourcePoolProvider::new();
    sources.rebuild(&material).unwrap();
    (index, material, sources)
}

fn empty_resolved_view() -> ResolvedView {
    resolve(
        &MemoryCatalog::default(),
        &MemoryTrust::default(),
        &ResolveRequest {
            context: common::descriptor(),
            layers: vec![],
            policy: ManagedPolicy::default(),
        },
    )
    .expect("an empty layer stack always resolves")
}

/// A minimal, honest [`PaletteBackend`]: the one Knowledge subject/address
/// this backend recognises is answered from a real
/// [`aikit_core::KnowledgeApplication`] over the real Wiki fixture; every
/// operation the placement/provenance tests never exercise is left
/// `unimplemented!()` rather than faked.
struct KnowledgeBackend<'a> {
    context: ContextDescriptor,
    view: ResolvedView,
    subject: ResourceRef,
    address: KnowledgeAddress,
    app: KnowledgeApplication<'a>,
}

impl<'a> PaletteBackend for KnowledgeBackend<'a> {
    fn context(&self) -> &ContextDescriptor {
        &self.context
    }

    fn view(&self) -> &ResolvedView {
        &self.view
    }

    fn documents(&self) -> Vec<SearchDoc> {
        Vec::new()
    }

    fn capsule(&self, _id: &CapsuleId) -> Option<&Capsule> {
        None
    }

    fn preview(&self, _scope: ScopeKind, _toggles: &[Toggle]) -> Result<Projected> {
        unimplemented!("knowledge_graph_v2 placement/provenance tests never preview a composition")
    }

    fn apply(&mut self, _scope: ScopeKind, _toggles: &[Toggle]) -> Result<GenerationId> {
        unimplemented!("knowledge_graph_v2 placement/provenance tests never apply a composition")
    }

    fn start(&mut self, _intent: &RunIntent) -> Result<JobOutput> {
        unimplemented!("knowledge_graph_v2 placement/provenance tests never start a run")
    }

    fn recent(&self) -> Vec<RunIntent> {
        Vec::new()
    }

    fn promotion_drafts(&self) -> Vec<PromotionDraft> {
        Vec::new()
    }

    fn promote(&mut self, _draft: &PromotionDraft) -> Result<CapsuleId> {
        unimplemented!("knowledge_graph_v2 placement/provenance tests never promote a draft")
    }

    fn knowledge_address(&self, resource: &ResourceRef) -> Result<Option<KnowledgeAddress>> {
        Ok((resource == &self.subject).then(|| self.address.clone()))
    }

    fn knowledge_relations(
        &self,
        address: &KnowledgeAddress,
        depth: u8,
        max_nodes: usize,
        max_edges: usize,
    ) -> Result<Option<KnowledgeRelationView>> {
        Ok(Some(KnowledgeOperations::relations(
            &self.app, address, depth, max_nodes, max_edges,
        )?))
    }
}

fn find_edge<'a>(view: &'a KnowledgeRelationView, relation: &str) -> &'a aikit_core::RelationEdge {
    view.edges
        .iter()
        .find(|edge| edge.relation == relation)
        .unwrap_or_else(|| panic!("fixture must contain a {relation:?} edge; got {:?}", view.edges))
}

#[test]
fn incoming_outgoing_and_contained_relations_land_in_their_documented_band_and_a_containment_lookalike_does_not() {
    let (index, material, sources) = wiki_fixture();
    let subject = ResourceRef::parse("wiki:space:root").unwrap();
    let address = KnowledgeAddress::Wiki(subject.clone());
    let app = KnowledgeApplication::new(familiarity_context())
        .with_wiki(SemanticWikiProvider::new(&index))
        .with_source_pool(&sources, &material);
    let mut backend = KnowledgeBackend {
        context: common::descriptor(),
        view: empty_resolved_view(),
        subject: subject.clone(),
        address,
        app,
    };

    let service = ApplicationService::new(&mut backend);
    let relation = service.relations_at_depth(&subject, 1).unwrap();
    assert_eq!(relation.view.query.focus, subject, "the real provider's own focus must survive untouched");
    // Recomputed for the current fixture: root's own relations() call still
    // yields exactly cites, grounded-in, member, child-space and member-of —
    // root itself carries no `parent_space_refs`, so the enclosing-edge
    // faculty added in cc1e152 contributes nothing at *this* focus (it is
    // proven from `wiki:space:child`'s and `wiki:node:member`'s own focus
    // below, where it actually fires).
    assert_eq!(relation.view.edges.len(), 5, "exactly the fixture's five authored edges, no more");

    let laid = graph_layout(&relation.view, &viewport_request(120, 30));
    let band_of = |relation_name: &str| {
        laid.edges
            .iter()
            .find(|edge| edge.relation == relation_name)
            .unwrap_or_else(|| panic!("laid-out edges must include {relation_name:?}: {laid:?}"))
            .band
    };

    assert_eq!(band_of("cites"), RelationBand::Outgoing);
    assert_eq!(band_of("grounded-in"), RelationBand::Incoming);
    assert_eq!(band_of("member"), RelationBand::Contained);
    assert_eq!(band_of("child-space"), RelationBand::Contained);
    // "member-of" reads like containment vocabulary, but it is an ordinary
    // authored WikiEdge — not one of the three exact strings the provider
    // emits for genuine membership — so the provider asserts no containment
    // meaning for it, and it must land by plain `RelationDirection` alone:
    // root is `from`, direction is Outgoing, so Outgoing. Before this fix,
    // graph_layout's own speculative `REVERSE_CONTAINMENT_RELATIONS` table
    // placed any edge named `"member-of"` in Context regardless of what the
    // provider actually asserted; that table has been removed.
    assert_eq!(
        band_of("member-of"),
        RelationBand::Outgoing,
        "an authored edge merely named like containment is not containment"
    );

    // Every node the real provider actually returned must have made it into
    // the layout — this is the same defect class the typed-relation-boundary
    // fix (application_service.rs) closed, checked here one level up, after
    // the Graph-specific spatial layout too.
    for edge in &relation.view.edges {
        assert!(laid.nodes.iter().any(|node| node.resource == edge.from));
        assert!(laid.nodes.iter().any(|node| node.resource == edge.to));
    }
}

/// Proves the Context band from real provider output, not from a
/// hand-authored edge built to match the TUI's own placement table:
/// focusing the *node* `wiki:node:member` (which carries
/// `space_refs: ["wiki:space:root"]`) reaches the enclosing half of its own
/// membership — `root --member--> wiki:node:member`, `Incoming` relative to
/// `member`, stamped at the Space's own revision (3), exactly as
/// `knowledge_wiki_provider.rs::relations` case 4 describes. Before commit
/// cc1e152 added that enclosing-edge faculty, a node focus could not reach
/// its own enclosing Space at all — `find_edge` below would have panicked,
/// so this test would have failed on the parent commit.
#[test]
fn node_focus_reaches_its_enclosing_space_and_places_it_in_context() {
    let (index, material, sources) = wiki_fixture();
    let subject = ResourceRef::parse("wiki:node:member").unwrap();
    let address = KnowledgeAddress::Wiki(subject.clone());
    let app = KnowledgeApplication::new(familiarity_context())
        .with_wiki(SemanticWikiProvider::new(&index))
        .with_source_pool(&sources, &material);
    let mut backend = KnowledgeBackend {
        context: common::descriptor(),
        view: empty_resolved_view(),
        subject: subject.clone(),
        address,
        app,
    };

    let service = ApplicationService::new(&mut backend);
    let relation = service.relations_at_depth(&subject, 1).unwrap();
    let root = ResourceRef::parse("wiki:space:root").unwrap();

    let enclosing = find_edge(&relation.view, "member");
    assert_eq!(enclosing.from, root, "the Space, not the member, is `from`");
    assert_eq!(enclosing.to, subject);
    assert_eq!(enclosing.direction, RelationDirection::Incoming);
    assert_eq!(enclosing.origin.authority, SourceAuthority::Authored);
    assert_eq!(
        enclosing.origin.revision.as_deref(),
        Some("3"),
        "the enclosing Space's own revision must survive, not the member node's revision"
    );

    let laid = graph_layout(&relation.view, &viewport_request(120, 30));
    let root_node = laid
        .nodes
        .iter()
        .find(|node| node.resource == root)
        .unwrap_or_else(|| panic!("the enclosing Space must be laid out: {laid:?}"));
    assert_eq!(root_node.band, Some(RelationBand::Context));
    let edge_band = laid
        .edges
        .iter()
        .find(|edge| edge.relation == "member" && edge.from == root && edge.to == subject)
        .unwrap()
        .band;
    assert_eq!(edge_band, RelationBand::Context);
}

/// Proves that the *same* relation string (`"child-space"`) bands
/// differently depending on structural role — a container's own focus sees
/// its children as Contained, but a child's own focus sees its parent as
/// Context — which is exactly what makes `classify_band`'s `anchor_is_from`
/// parameter meaningful rather than redundant with the relation string
/// alone. The child-side half of this (`wiki:space:child` reaching its
/// parent) is the enclosing edge added in cc1e152 (case 5); before that
/// commit `find_edge` below would have panicked because `wiki:space:child`'s
/// own relations() call returned no edges at all.
#[test]
fn child_space_relation_bands_differently_by_which_side_is_focused() {
    let (index, material, sources) = wiki_fixture();
    let root = ResourceRef::parse("wiki:space:root").unwrap();
    let child = ResourceRef::parse("wiki:space:child").unwrap();

    let app = KnowledgeApplication::new(familiarity_context())
        .with_wiki(SemanticWikiProvider::new(&index))
        .with_source_pool(&sources, &material);

    // The container's own focus: its child is Contained.
    let root_view =
        KnowledgeOperations::relations(&app, &KnowledgeAddress::Wiki(root.clone()), 1, 96, 192).unwrap();
    let root_edge = find_edge(&root_view, "child-space");
    assert_eq!(root_edge.from, root);
    assert_eq!(root_edge.to, child);
    assert_eq!(root_edge.direction, RelationDirection::Outgoing);
    let root_laid = graph_layout(&root_view, &viewport_request(120, 30));
    let root_child_band = root_laid
        .edges
        .iter()
        .find(|edge| edge.relation == "child-space")
        .unwrap()
        .band;
    assert_eq!(root_child_band, RelationBand::Contained);

    // The child's own focus: its parent is Context — same relation string,
    // opposite band, because the focus is now the `to` endpoint.
    let child_view =
        KnowledgeOperations::relations(&app, &KnowledgeAddress::Wiki(child.clone()), 1, 96, 192).unwrap();
    let child_edge = find_edge(&child_view, "child-space");
    assert_eq!(child_edge.from, root, "the same logical membership edge, from the Space");
    assert_eq!(child_edge.to, child);
    assert_eq!(child_edge.direction, RelationDirection::Incoming);
    assert_eq!(child_edge.origin.authority, SourceAuthority::Authored);
    let child_laid = graph_layout(&child_view, &viewport_request(120, 30));
    let child_parent_band = child_laid
        .edges
        .iter()
        .find(|edge| edge.relation == "child-space")
        .unwrap()
        .band;
    assert_eq!(
        child_parent_band,
        RelationBand::Context,
        "the space's own parent must land in Context, not Contained"
    );
}

// ===========================================================================
// 5. PROVENANCE RETAINED — through the full fetch -> typed view -> layout ->
//    render round trip
// ===========================================================================

#[test]
fn provider_lens_authority_and_revision_are_all_still_readable_at_the_inspector() {
    let (index, material, sources) = wiki_fixture();
    let subject = ResourceRef::parse("wiki:space:root").unwrap();
    let address = KnowledgeAddress::Wiki(subject.clone());
    let app = KnowledgeApplication::new(familiarity_context())
        .with_wiki(SemanticWikiProvider::new(&index))
        .with_source_pool(&sources, &material);
    let mut backend = KnowledgeBackend {
        context: common::descriptor(),
        view: empty_resolved_view(),
        subject: subject.clone(),
        address,
        app,
    };
    let service = ApplicationService::new(&mut backend);
    let relation = service.relations_at_depth(&subject, 1).unwrap();

    // The `member` edge is real `WikiSpace` membership, which is the one
    // shape in this codebase's own Wiki provider that stamps all four
    // provenance fields — provider, lens, authority *and* revision (see
    // `knowledge_wiki_provider.rs`'s own `.at_revision(space.revision...)`).
    let member_edge = find_edge(&relation.view, "member");
    assert_eq!(member_edge.origin.authority, SourceAuthority::Authored);
    assert_eq!(
        member_edge.origin.provider.as_ref().unwrap().as_str(),
        aikit_core::NATIVE_SEMANTIC_WIKI_PROVIDER
    );
    assert_eq!(member_edge.origin.lens.as_deref(), Some("semantic-wiki"));
    assert_eq!(
        member_edge.origin.revision.as_deref(),
        Some("3"),
        "the space's own revision must survive into the edge's provenance"
    );

    let laid = graph_layout(&relation.view, &viewport_request(120, 30));
    let member_node = ResourceRef::parse("wiki:node:member").unwrap();
    let inspector = graph_presentation::inspector_lines(&laid, Some(&member_node), &aikit_tui::theme::Theme::new());
    let text = lines_to_text(&inspector);
    assert!(text.contains("member"), "the Inspector must name the relation:\n{text}");
    assert!(text.contains("authority: Authored"), "authority must render:\n{text}");
    assert!(
        text.contains(aikit_core::NATIVE_SEMANTIC_WIKI_PROVIDER),
        "provider must render:\n{text}"
    );
    assert!(text.contains("lens: semantic-wiki"), "lens must render:\n{text}");
    assert!(text.contains("revision: 3"), "revision must render, not just be present on the typed edge:\n{text}");

    // A second, independent real edge (SourcePool-cited provenance on a
    // plain node, reached through the same multi-provider
    // `KnowledgeApplication`) demonstrates this is not a one-fixture
    // coincidence: provider/lens/authority survive there too.
    let source_address = KnowledgeAddress::Wiki(member_node.clone());
    let source_relation =
        KnowledgeOperations::relations(&backend.app, &source_address, 1, 96, 192).unwrap();
    let source_edge = find_edge(&source_relation, "source");
    assert_eq!(source_edge.origin.authority, SourceAuthority::Authored);
    assert_eq!(
        source_edge.origin.provider.as_ref().unwrap().as_str(),
        aikit_core::NATIVE_SEMANTIC_WIKI_PROVIDER
    );
    assert_eq!(source_edge.to.as_str(), "source:spec");
}

// ===========================================================================
// 6. BOUNDED PERFORMANCE — at the core relation budgets
// ===========================================================================

#[test]
fn a_neighbourhood_at_the_core_relation_budgets_lays_out_and_renders_within_bounds() {
    let focus = ResourceRef::parse("knowledge-node/focus").unwrap();
    let mut view = KnowledgeRelationView::focus_only(
        RelationQuery::local(focus.clone()),
        RelationNode::new(focus.clone(), ResourceKind::KnowledgeNode, "Focus"),
    )
    .unwrap();
    // aikit_core::DEFAULT_RELATION_NODE_BUDGET / DEFAULT_RELATION_EDGE_BUDGET
    // are 96/192: one node short of the node budget, two edges per node (95 * 2
    // = 190), right up against the edge budget too.
    for i in 0..95 {
        let raw = format!("knowledge-node/n{i:03}");
        let node = RelationRefNode::parse(&raw);
        assert!(view.push_node(RelationNode::new(node.clone(), ResourceKind::KnowledgeNode, &raw)));
        let (from, to, direction) = if i % 2 == 0 {
            (focus.clone(), node.clone(), RelationDirection::Outgoing)
        } else {
            (node.clone(), focus.clone(), RelationDirection::Incoming)
        };
        view.push_edge(aikit_core::RelationEdge::new(
            from,
            to,
            if i % 2 == 0 { "cites" } else { "grounded-in" },
            direction,
            aikit_core::RelationOrigin::new(SourceAuthority::Authored),
        ))
        .unwrap();
        view.push_edge(aikit_core::RelationEdge::new(
            focus.clone(),
            node,
            "co-occurs",
            RelationDirection::Bidirectional,
            aikit_core::RelationOrigin::new(SourceAuthority::Derived).in_lens("resolver"),
        ))
        .unwrap();
    }
    assert_eq!(view.nodes.len(), 96, "node budget saturated but not exceeded");
    assert_eq!(view.edges.len(), 190, "edge count sits right up against the 192 edge budget too");
    assert!(!view.truncated, "under budget: the provider itself never marks this truncated");

    // A generously wide but modest-height terminal: nowhere near enough
    // rows to show 95 Incoming/Outgoing rows at one node per row.
    let request = viewport_request(120, 24);
    let laid = graph_layout(&view, &request);
    assert!(laid.truncated, "the viewport budget, not the provider, must be what truncates here");
    assert!(laid.dropped.nodes_dropped > 0);

    let glyphs = aikit_tui::graph_layout::GraphGlyphs::unicode();
    let theme = aikit_tui::theme::Theme::new();
    let rendered = graph_presentation::spatial_lines(&laid, None, &glyphs, &theme, GraphViewport::new(120, 24));

    // Structural bound, not a timing assertion: the canvas portion of the
    // rendering is exactly the viewport's own row count regardless of how
    // many nodes the neighbourhood carries — the whole point of a bounded
    // layout is that rendering cost tracks the *screen*, not the graph.
    let canvas_rows = rendered.iter().take(24).count();
    assert_eq!(canvas_rows, 24, "the canvas must stay exactly viewport-sized, never node-count-sized");
    assert!(
        rendered.len() < view.nodes.len() * 2,
        "the legend must stay close to one line per *visible* node, not blow up with the full 96-node neighbourhood: {} lines for {} nodes",
        rendered.len(),
        view.nodes.len()
    );
}

/// Tiny local helper: `graph_layout.rs`'s own tests reparse a `&str` per
/// call too (`ResourceRef::parse` has no cheaper constructor for a test
/// fixture); named here only so the budget test above reads as data, not
/// parsing boilerplate.
struct RelationRefNode;
impl RelationRefNode {
    fn parse(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }
}

// ===========================================================================
// 7. PROVIDER-DEGRADED / PARTIAL GRAPH
// ===========================================================================

#[test]
fn a_truncated_focus_only_view_with_warnings_renders_truthfully_in_both_projections() {
    let focus = ResourceRef::parse("wiki:node:degraded").unwrap();
    let mut view = KnowledgeRelationView::focus_only(
        RelationQuery::local(focus.clone()),
        RelationNode::new(focus.clone(), ResourceKind::KnowledgeNode, "Degraded"),
    )
    .unwrap();
    // A provider that could not complete its own traversal (a timeout, a
    // partial index, a lens that only answered for the focus itself) is
    // required to say so, never to report an empty neighbourhood as if it
    // were complete.
    view.truncated = true;
    view.warnings.push("provider truncated before any relation could be retrieved".into());
    assert!(view.nodes.len() == 1 && view.edges.is_empty(), "provider returned only the focus node");

    let laid = graph_layout(&view, &viewport_request(80, 24));
    assert_eq!(laid.nodes.len(), 1);
    assert!(laid.edges.is_empty());
    assert!(laid.truncated, "the layout must not silently drop the provider's own truncation");
    assert!(laid.warnings.iter().any(|w| w.contains("provider truncated")));

    let glyphs = aikit_tui::graph_layout::GraphGlyphs::unicode();
    let theme = aikit_tui::theme::Theme::new();

    let spatial = lines_to_text(&graph_presentation::spatial_lines(
        &laid,
        None,
        &glyphs,
        &theme,
        GraphViewport::new(80, 24),
    ));
    assert!(
        spatial.contains("truncated"),
        "the spatial canvas must say the neighbourhood is incomplete, not just show an empty legend:\n{spatial}"
    );

    let grouped = lines_to_text(&graph_presentation::grouped_lines(&laid, &glyphs, &theme));
    assert!(
        grouped.contains("no typed resource relations"),
        "the narrow fallback must not fabricate a relation:\n{grouped}"
    );
    assert!(
        grouped.contains("truncated"),
        "the narrow fallback must say the same thing the spatial canvas says about incompleteness — \
         an empty neighbourhood and an unfinished fetch must never render identically:\n{grouped}"
    );
}

// ===========================================================================
// 8. NARROW FALLBACK — and back
// ===========================================================================

#[test]
fn narrowing_falls_back_to_grouped_projection_and_widening_restores_the_spatial_one_of_the_same_state() {
    let (_dir, mut backend) = resolver_fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);
    surface.handle(&mut backend, key(KeyCode::Char('+'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Char('/'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Char('g'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    let depth = surface.semantic().graph.depth;
    let filter = surface.semantic().graph.filter.clone();
    assert_eq!(filter, "g");
    assert!(depth > 1);

    surface.handle(&mut backend, PaletteEvent::Resize(40, 20)).unwrap();
    assert_eq!(surface.semantic().relation_view, RelationView::Graph, "still semantically Graph");
    assert_eq!(surface.semantic().graph.depth, depth, "depth survives narrowing");
    assert_eq!(surface.semantic().graph.filter, filter, "filter survives narrowing");
    let narrow_text = rendered_rows(&draw(&surface, 40, 20));
    assert!(narrow_text.contains("gamma") || narrow_text.contains("no typed resource relations"));
    assert!(!narrow_text.contains("Inspector"), "narrow must use the grouped fallback:\n{narrow_text}");

    surface.handle(&mut backend, PaletteEvent::Resize(120, 30)).unwrap();
    assert_eq!(surface.semantic().graph.depth, depth, "depth survives widening back");
    assert_eq!(surface.semantic().graph.filter, filter, "filter survives widening back");
    let wide_text = rendered_rows(&draw(&surface, 120, 30));
    assert!(wide_text.contains("Inspector"), "widening must restore the spatial canvas:\n{wide_text}");
}

// ===========================================================================
// 9. KEYBOARD / MOUSE PARITY
// ===========================================================================

#[test]
fn clicking_a_node_and_keyboard_selecting_it_produce_the_same_semantic_action() {
    let (_dir, mut backend) = resolver_fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);
    let _ = draw(&surface, 120, 30);

    // Keyboard: arrow-right onto a neighbour.
    surface.handle(&mut backend, key(KeyCode::Right)).unwrap();
    let via_keyboard = surface.semantic().selected.clone().unwrap();
    assert_ne!(via_keyboard.as_str(), "skill/alpha");
    assert_eq!(surface.semantic().graph.focus, Some(ResourceRef::parse("skill/alpha").unwrap()));

    // Reset the highlight back to the focus, then reach the exact same node
    // through a mouse click, using the controller's own documented
    // content-rect/canvas-viewport math (mirrored from `graph_content_rect`/
    // `graph_viewport`) so the click coordinate is real, not guessed.
    surface.handle(&mut backend, key(KeyCode::Left)).unwrap();
    assert_eq!(surface.semantic().selected, Some(ResourceRef::parse("skill/alpha").unwrap()));

    let inner = Rect::new(1, 1, 118, 28);
    let list = Layout::for_width(inner.width).split(inner).list;
    let content = Rect::new(
        list.x.saturating_add(1),
        list.y.saturating_add(1),
        list.width.saturating_sub(2),
        list.height.saturating_sub(2),
    );
    const MIN_CANVAS_HEIGHT: u16 = 5;
    let canvas_height = if content.height <= MIN_CANVAS_HEIGHT {
        content.height
    } else {
        (content.height * 3 / 5).max(MIN_CANVAS_HEIGHT)
    };
    let relation = surface.relation().unwrap().clone();
    let laid = graph_layout(&relation.view, &viewport_request(content.width, canvas_height));
    let target = laid
        .nodes
        .iter()
        .find(|node| node.resource == via_keyboard)
        .expect("the keyboard-reached neighbour must also be a real laid-out node");
    let column = content.x + u16::try_from(target.position.x).unwrap();
    let row = content.y + u16::try_from(target.position.y).unwrap();

    surface.handle(&mut backend, mouse(column, row, KeyModifiers::NONE)).unwrap();
    let via_mouse = surface.semantic().selected.clone().unwrap();
    assert_eq!(
        via_mouse, via_keyboard,
        "a plain click and keyboard navigation must resolve to the identical target resource"
    );
    assert_eq!(
        surface.semantic().graph.focus,
        Some(ResourceRef::parse("skill/alpha").unwrap()),
        "neither modality recentres on a plain move"
    );

    // The recenter gesture (Enter) and a modifier-click must likewise agree.
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    let focus_via_enter = surface.semantic().graph.focus.clone();
    assert_eq!(focus_via_enter, Some(via_keyboard.clone()));

    surface.handle(&mut backend, key(KeyCode::Esc)).unwrap(); // back to alpha
    surface.handle(&mut backend, key(KeyCode::Right)).unwrap(); // reselect the same neighbour deterministically
    assert_eq!(surface.semantic().selected.as_ref(), Some(&via_keyboard));
    surface.handle(&mut backend, mouse(column, row, KeyModifiers::SHIFT)).unwrap();
    assert_eq!(
        surface.semantic().graph.focus,
        Some(via_keyboard),
        "a modifier-click must recentre onto exactly what Enter would have"
    );
}

// ===========================================================================
// 10. RESIZE preserves graph focus, filter and staged state
// ===========================================================================

#[test]
fn resize_preserves_graph_focus_filter_depth_and_staged_state() {
    let (_dir, mut backend) = resolver_fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup).with_query("alpha"),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, 120, 30);

    surface.handle(&mut backend, key(KeyCode::Char('+'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Char('/'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Char('b'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Char('e'))).unwrap();
    surface.handle(&mut backend, key(KeyCode::Enter)).unwrap();
    surface
        .handle(&mut backend, PaletteEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL)))
        .unwrap(); // stage the selected (still-focus) resource

    let before = surface.semantic().clone();
    assert_eq!(before.graph.depth, 2);
    assert_eq!(before.graph.filter, "be");
    assert_eq!(before.staged.len(), 1, "Ctrl+Space must have staged the selected resource");

    surface.handle(&mut backend, PaletteEvent::Resize(60, 22)).unwrap();

    assert_eq!(surface.semantic().graph.focus, before.graph.focus, "focus must survive a resize");
    assert_eq!(surface.semantic().graph.filter, before.graph.filter, "filter must survive a resize");
    assert_eq!(surface.semantic().graph.depth, before.graph.depth, "depth must survive a resize");
    assert_eq!(surface.semantic().staged, before.staged, "staged changes must survive a resize");
    assert_eq!(surface.semantic().area, (60, 22));
}

// ===========================================================================
// Snapshots — wide / medium / narrow, plus one degraded state
//
// Host glyph capability (ASCII vs. Unicode) is resolved once at
// `ApplicationSurfaceController::new` (`application_surface.rs`), from
// `Glyphs::from_env()` by default. Rendering it through the live process
// locale would make a snapshot recorded on one machine fail on another with
// a different `LANG`/`LC_ALL` — the exact defect this module's own
// `snapshot_wide_spatial_graph` etc. hit (goldens recorded with no locale
// set, i.e. ASCII, failing under CI's UTF-8 locale, i.e. Unicode). Setting
// the environment from the test is not an option either: `nextest` runs
// tests in parallel and the environment is process-global, so that would be
// racy. Instead every snapshot below pins its glyph set explicitly via
// `ApplicationSurfaceRequest::with_graph_glyphs`, and each width gets one
// ASCII and one Unicode golden — named accordingly — so the pair also
// stands as the proof (constraint 4) that the two glyph sets carry
// identical distinctions: a reviewer reading both goldens for the same
// width side by side sees the same information laid out identically, only
// the glyphs swapped.
// ===========================================================================

fn snapshot_spatial_graph(glyphs: aikit_tui::graph_layout::GraphGlyphs, width: u16, height: u16) -> String {
    let (_dir, mut backend) = resolver_fixture();
    let mut surface = ApplicationSurfaceController::new(
        &mut backend,
        ApplicationSurfaceRequest::new(UiHost::TmuxPopup)
            .with_query("alpha")
            .with_graph_glyphs(glyphs),
    )
    .unwrap();
    enter_graph(&mut surface, &mut backend, width, height);
    rendered_rows(&draw(&surface, width, height))
}

#[test]
fn snapshot_wide_spatial_graph_ascii() {
    let text = snapshot_spatial_graph(aikit_tui::graph_layout::GraphGlyphs::ascii(), 120, 30);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_wide_spatial_graph_unicode() {
    let text = snapshot_spatial_graph(aikit_tui::graph_layout::GraphGlyphs::unicode(), 120, 30);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_medium_spatial_graph_ascii() {
    let text = snapshot_spatial_graph(aikit_tui::graph_layout::GraphGlyphs::ascii(), 80, 24);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_medium_spatial_graph_unicode() {
    let text = snapshot_spatial_graph(aikit_tui::graph_layout::GraphGlyphs::unicode(), 80, 24);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_narrow_grouped_graph_ascii() {
    let text = snapshot_spatial_graph(aikit_tui::graph_layout::GraphGlyphs::ascii(), 40, 20);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_narrow_grouped_graph_unicode() {
    let text = snapshot_spatial_graph(aikit_tui::graph_layout::GraphGlyphs::unicode(), 40, 20);
    insta::assert_snapshot!(text);
}

#[test]
fn snapshot_degraded_truncated_focus_only_graph() {
    let focus = ResourceRef::parse("wiki:node:degraded").unwrap();
    let mut view = KnowledgeRelationView::focus_only(
        RelationQuery::local(focus.clone()),
        RelationNode::new(focus, ResourceKind::KnowledgeNode, "Degraded"),
    )
    .unwrap();
    view.truncated = true;
    view.warnings.push("provider truncated before any relation could be retrieved".into());
    let laid = graph_layout(&view, &viewport_request(80, 24));
    let glyphs = aikit_tui::graph_layout::GraphGlyphs::unicode();
    let theme = aikit_tui::theme::Theme::new();
    let text = lines_to_text(&graph_presentation::spatial_lines(
        &laid,
        None,
        &glyphs,
        &theme,
        GraphViewport::new(80, 24),
    ));
    insta::assert_snapshot!(text);
}

/// The four snapshots above are all Skill-focused (`resolver_fixture`),
/// which never produces a Context/Contained edge at all — none of them
/// freezes what a populated Context band actually looks like. This one
/// focuses `wiki:node:member` from the real Wiki fixture (`wiki_fixture`),
/// whose enclosing edge (`root --member--> wiki:node:member`, Incoming,
/// added in cc1e152) is proven programmatically in
/// `node_focus_reaches_its_enclosing_space_and_places_it_in_context`
/// above; this snapshot freezes its actual on-screen rendering — the
/// horizontal Context lane above the focus, carrying `wiki:space:root` —
/// via the same direct `graph_layout` -> `graph_presentation::spatial_lines`
/// pipeline `snapshot_degraded_truncated_focus_only_graph` already uses
/// (no full `ApplicationSurfaceController` is needed to render a spatial
/// canvas; both pure functions are the entire rendering path).
#[test]
fn snapshot_wide_spatial_graph_node_focus_context_band() {
    let (index, material, sources) = wiki_fixture();
    let subject = ResourceRef::parse("wiki:node:member").unwrap();
    let address = KnowledgeAddress::Wiki(subject.clone());
    let app = KnowledgeApplication::new(familiarity_context())
        .with_wiki(SemanticWikiProvider::new(&index))
        .with_source_pool(&sources, &material);
    let view = KnowledgeOperations::relations(&app, &address, 1, 96, 192).unwrap();

    let laid = graph_layout(&view, &viewport_request(120, 30));
    let glyphs = aikit_tui::graph_layout::GraphGlyphs::unicode();
    let theme = aikit_tui::theme::Theme::new();
    let text = lines_to_text(&graph_presentation::spatial_lines(
        &laid,
        None,
        &glyphs,
        &theme,
        GraphViewport::new(120, 30),
    ));
    insta::assert_snapshot!(text);
}
