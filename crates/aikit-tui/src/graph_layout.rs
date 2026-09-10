//! Deterministic, focus-centred geometry for one [`KnowledgeRelationView`].
//!
//! This module computes *where things go*, never *what they mean*. The
//! typed relation neighbourhood — nodes, edges, provider vocabulary,
//! provenance — is owned entirely by `aikit_core::knowledge`. Graph adds a
//! spatial reading on top of that view and nothing else: no relation store,
//! no resolver, no second ontology, no physics. Two calls with the same
//! `(view, request)` always produce byte-identical output, because a
//! terminal user re-opening the same neighbourhood should see the same
//! picture, not a picture that drifted because a `HashMap` iterated
//! differently this time.
//!
//! ## The four bands
//!
//! Design ground: `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §8.3.
//!
//! ```text
//!                     contextual / enclosing
//!                             ^
//!                             |
//!     incoming relations <-- [ FOCUS ] --> outgoing relations
//!                             |
//!                             v
//!                     contained / nested
//! ```
//!
//! A node's band is read off the *edge*, never off the relation label's
//! English meaning. Two structural facts decide it:
//!
//! - **`RelationEdge.containment`**, the provider's own structural
//!   assertion (see `aikit_core::knowledge::ContainmentRole`) that `from`
//!   encloses `to` or vice versa. Absent means the provider is not
//!   asserting containment — we do not guess from a word that merely
//!   sounds container-shaped (`"binds"`, `"owns"`, `"scopes"`, or even an
//!   authored edge a human named `"member-of"`, all stay plain
//!   Incoming/Outgoing).
//! - **`RelationDirection`**, for everything else. Outgoing/Incoming are
//!   used exactly as authored; `Bidirectional` has no directional default
//!   of its own, so it is placed on the Outgoing side by a documented,
//!   stable convention (see [`classify_band`]).
//!
//! `RelationEdge.relation` is never rewritten, collapsed or mapped onto a
//! shared enum here — only *classified* for placement. The original string
//! survives on every laid-out edge and every grouped-projection line.
//!
//! ## Lanes and multi-hop neighbourhoods
//!
//! Within a band, nodes discovered through edges that share a relation
//! name form one contiguous lane, labelled by that relation. A node's
//! band/lane is decided by the first edge that reaches it in a
//! breadth-first walk from the focus over a globally sorted edge list —
//! "first" here means first in that deterministic order, never first in
//! whatever order the provider happened to emit. Because the walk can
//! cross an intermediate (non-focus) node at `query.depth > 1`, containment
//! placement for a node beyond the first hop is relative to whichever node
//! discovered it, not to the focus directly; that is a documented
//! simplification, not an oversight — `DEFAULT_RELATION_DEPTH` is 1, so the
//! overwhelming majority of neighbourhoods never exercise it.
//!
//! The full edge list, by contrast, is classified per edge against the
//! focus directly wherever an edge touches the focus (the common case),
//! so `grouped_projection` always shows a node's *actual* relation to the
//! focus, even when that node's on-screen position was decided by a
//! different, earlier-discovered edge.
//!
//! ## Extent: reserving room for a node's own label
//!
//! A human-readable label needs more than one cell, so placement cannot
//! just hand out one column/row per node the way a letter-marker grid
//! could. Every node's [`LaidOutNode::label_width`] is *reserved* — never
//! shared, never overlapped — by the same placement pass that decides its
//! position, using [`choose_label_rung`] to pick the richest rung of the
//! label ladder (documented on that function) that fits the room actually
//! available there. This lives here, in geometry, and not in
//! `graph_presentation`, for one reason: placing node B must already know
//! how much room node A's label claimed, or B could be handed cells A is
//! about to draw into. `graph_presentation` calls the same pure function
//! again, with `available` set to the `label_width` this module already
//! reserved, to redraw the identical rung — the two can never disagree
//! about what a width means because there is only one function that
//! decides it.
//!
//! Each lane's own relation name is reserved and positioned the same way,
//! as a one-line gutter header ahead of (vertical bands) or beside
//! (horizontal bands) its members — see [`LaidOutLane`] — so the relation
//! that placed a node is readable in the spatial picture itself, not only
//! in the legend beneath it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use aikit_core::{
    ContainmentRole, KnowledgeRelationView, RelationDirection, RelationEdge, RelationOrigin,
    ResourceKind, ResourceRef,
};

// ---------------------------------------------------------------------------
// Bands
// ---------------------------------------------------------------------------

/// One of the four focus-relative placement regions from spec §8.3.
///
/// Declaration order is display order: it is the order `grouped_projection`
/// emits sections in, and the order `RelationBand`'s derived [`Ord`] sorts
/// by wherever a band needs to sort deterministically alongside other keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationBand {
    /// The edge points at the focus.
    Incoming,
    /// The edge points away from the focus.
    Outgoing,
    /// The other node contains the focus (the focus is contained *by* it).
    Context,
    /// The focus contains the other node.
    Contained,
}

impl RelationBand {
    pub fn label(self) -> &'static str {
        match self {
            Self::Incoming => "Incoming",
            Self::Outgoing => "Outgoing",
            Self::Context => "Context",
            Self::Contained => "Contained",
        }
    }
}

/// Classify the band of the node reached by `containment`/`direction`,
/// relative to a `from`-or-`to` anchor.
///
/// `anchor_is_from` says whether the node we are placing *from* sits at
/// `edge.from` (`true`) or `edge.to` (`false`); the placed node is always
/// the other endpoint. Containment is read directly off the provider's own
/// `RelationEdge.containment` assertion — never reconstructed from
/// `relation` text, which stays opaque provider vocabulary this module must
/// not interpret. `None` means the provider is not asserting containment
/// for this edge (including a relation that merely *looks*
/// container-shaped, such as `"member-of"` or `"part-of"`); such an edge
/// falls back to plain `RelationDirection`-based Incoming/Outgoing, same as
/// `"cites"` or `"grounded-in"`.
///
/// Non-containment relations fall back to `direction` taken at face value —
/// `Bidirectional` has no inherent Incoming/Outgoing default, so mutual
/// relations are surfaced once, on the Outgoing side, by convention; this
/// mirrors how `application_service.rs` already emits resolver "often used
/// with" pairings as `Bidirectional`.
fn classify_band(
    containment: Option<ContainmentRole>,
    direction: RelationDirection,
    anchor_is_from: bool,
) -> RelationBand {
    match containment {
        Some(ContainmentRole::Encloses) => {
            if anchor_is_from {
                RelationBand::Contained
            } else {
                RelationBand::Context
            }
        }
        Some(ContainmentRole::EnclosedBy) => {
            if anchor_is_from {
                RelationBand::Context
            } else {
                RelationBand::Contained
            }
        }
        None => match direction {
            RelationDirection::Bidirectional => RelationBand::Outgoing,
            RelationDirection::Outgoing | RelationDirection::Incoming => {
                if anchor_is_from {
                    RelationBand::Outgoing
                } else {
                    RelationBand::Incoming
                }
            }
        },
    }
}

/// Band for one edge in the *full* retained edge list, classified directly
/// against `focus` wherever the edge touches it (the common, depth-1 case),
/// and falling back to an already-placed endpoint's band otherwise.
fn edge_band(
    edge: &RelationEdge,
    focus: &ResourceRef,
    node_band: &BTreeMap<ResourceRef, RelationBand>,
) -> RelationBand {
    if edge.from == edge.to {
        // Self-loop. A reflexive relation on the focus itself is surfaced as
        // Outgoing by the same Bidirectional-style convention; a reflexive
        // relation on some other node inherits that node's own band.
        return if &edge.from == focus {
            RelationBand::Outgoing
        } else {
            node_band
                .get(&edge.from)
                .copied()
                .unwrap_or(RelationBand::Outgoing)
        };
    }
    if &edge.from == focus {
        return classify_band(edge.containment, edge.direction, true);
    }
    if &edge.to == focus {
        return classify_band(edge.containment, edge.direction, false);
    }
    // Neither endpoint is the focus: a depth>1 edge between two already
    // placed nodes, or an edge disconnected from the focus within this
    // view. Prefer `to`'s band (arbitrary but documented and deterministic),
    // then `from`'s, then Outgoing for a node this view never reached.
    node_band
        .get(&edge.to)
        .or_else(|| node_band.get(&edge.from))
        .copied()
        .unwrap_or(RelationBand::Outgoing)
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// One position in terminal cells, relative to the viewport's own origin
/// (the focus sits at the viewport's centre; other nodes are offset from
/// it). Panning/zooming the viewport is a request-level concern — this
/// layout is recomputed from `(view, request)`, never mutated in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphPoint {
    pub x: i32,
    pub y: i32,
}

/// The visible area this layout is being fitted into, in terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphViewport {
    pub width: u16,
    pub height: u16,
}

impl GraphViewport {
    pub fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// Input to [`layout`]. The `view`'s own `query.depth`/`max_nodes`/
/// `max_edges` are the provider-side budget; `viewport` is the presentation
/// budget on top of it — geometry this small simply cannot show everything
/// the provider was willing to hand back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphLayoutRequest {
    pub viewport: GraphViewport,
}

impl GraphLayoutRequest {
    pub fn for_viewport(viewport: GraphViewport) -> Self {
        Self { viewport }
    }
}

/// One node's screen intent, plus every provenance field it arrived with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaidOutNode {
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub label: String,
    pub state: Option<String>,
    /// `None` only for the focus node, which has no band relative to
    /// itself.
    pub band: Option<RelationBand>,
    /// The relation name of the edge that placed this node, mirrored from
    /// `LaidOutEdge.relation` for the discovering edge. `None` for the
    /// focus.
    pub lane: Option<String>,
    pub is_focus: bool,
    pub position: GraphPoint,
    /// How many cells, starting at `position` and growing in the reading
    /// direction `graph_presentation` draws in, are reserved for this
    /// node's own text — never claimed by any other node's placement. `1`
    /// for the focus (its single glyph) and for any non-focus node whose
    /// neighbourhood was too dense to give it more: that is the ladder's
    /// last rung, a letter marker keyed by the legend. See the module doc
    /// and [`choose_label_rung`], which is the single place both this
    /// value and `graph_presentation`'s rendering of it are decided.
    pub label_width: u16,
}

/// One retained edge, with its placement band alongside its full typed
/// identity — losing any of `relation`, `direction` or `origin` here would
/// be exactly the defect this module exists to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaidOutEdge {
    pub from: ResourceRef,
    pub to: ResourceRef,
    pub relation: String,
    pub direction: RelationDirection,
    pub origin: RelationOrigin,
    pub band: RelationBand,
}

/// What the viewport budget, as opposed to the provider's own budget,
/// forced this layout to drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GraphTruncation {
    pub nodes_dropped: usize,
    pub edges_dropped: usize,
}

/// Where one lane's own relation name is drawn directly in the spatial
/// canvas — the provider's relation string, verbatim, positioned as a
/// one-line gutter header ahead of that lane's members (see the module
/// doc's "Extent" section). Present only for a lane whose relation string
/// could actually be shown in the room available at its position;
/// [`lane_gutter_extent`] never elides or abbreviates a relation name; a
/// lane whose name does not fit is simply not drawn here (the legend still
/// names it in full).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaidOutLane {
    pub band: RelationBand,
    pub relation: String,
    pub position: GraphPoint,
}

/// The complete, deterministic output of [`layout`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphLayout {
    pub focus: ResourceRef,
    pub nodes: Vec<LaidOutNode>,
    pub edges: Vec<LaidOutEdge>,
    /// One entry per lane whose relation name is drawn on the canvas
    /// itself, sorted by `(band, relation)` for determinism.
    pub lanes: Vec<LaidOutLane>,
    /// True when the source view was already truncated, or this layout's
    /// own viewport budget dropped anything — never silently.
    pub truncated: bool,
    pub dropped: GraphTruncation,
    pub warnings: Vec<String>,
}

// One row of vertical geometry (Incoming/Outgoing) or one column of
// horizontal geometry (Context/Contained): a lane's relation name and its
// deterministically ordered member resources.
type Lane = (String, Vec<ResourceRef>);

// ---------------------------------------------------------------------------
// Label ladder
// ---------------------------------------------------------------------------
//
// Doctrine: `crates/aikit-tui/src/layout.rs`'s module header — "A
// description clipped to eight characters is worse than no description: it
// looks like information and is not" — applies here exactly. A node's own
// on-screen text degrades through four rungs, richest first, and never
// stops on a rung that would look like a complete answer while quietly
// being a mutilated one:
//
//   1. `Full`      — `"{label} ({kind})"`, the same pairing the legend has
//                     always shown, now inline at the node's own position.
//   2. `LabelOnly` — the kind hint dropped; still the reader's actual name
//                     for the resource, not a fragment of it.
//   3. `Elided`     — a genuine prefix of the label plus a mark that can
//                     only be read as "more was here", never mistaken for
//                     content. Always carries at least one real character
//                     of the name — a mark with nothing in front of it
//                     would tell the reader nothing.
//   4. `Marker`     — the pre-existing single/double-letter marker, keyed
//                     by the legend. This is the *last* rung, reached only
//                     when a node's position has less than
//                     [`MIN_ELIDED_EXTENT`] cells to work with — a
//                     genuinely dense neighbourhood, not a merely
//                     narrow terminal (wide/medium viewports give every
//                     vertical-band node the full distance to the
//                     viewport edge; see [`layout`]).
//
// [`choose_label_rung`] is the *only* place this ladder is decided. It is
// called once here, in `layout`, to reserve exactly the cells the chosen
// rung needs, and again in `graph_presentation`, with `available` set to
// the `label_width` this module already reserved, to redraw the identical
// rung from nothing but that number. That round trip only works because
// the function is idempotent: re-querying it with the width it already
// returned reproduces the same rung (worked through in the doc comment on
// the function itself). Presentation therefore never needs layout to hand
// it a literal string — which would go stale the instant presentation drew
// something shorter than what was reserved.

/// Upper bound on how many cells a single node's label may claim, however
/// much lane budget happens to be free — keeps one long label from
/// swallowing room a mostly-empty canvas never actually contended for.
const MAX_LABEL_EXTENT: u16 = 40;

/// Worst-case width of the elision mark presentation might draw:
/// `GraphGlyphs::ascii`'s `"..."` (three cells). `GraphGlyphs::unicode`'s
/// `"…"` is a single cell, i.e. only ever narrower, so reserving the ASCII
/// width here is always safe regardless of which glyph set ends up
/// rendering — geometry does not know that choice (it is presentation's),
/// so it reserves for the wider one.
const ELISION_RESERVE: u16 = 3;

/// The shortest a label may be shown elided: the worst-case elision mark
/// plus at least one real character of the label, so an elision is never
/// only the mark with nothing in front of it.
const MIN_ELIDED_EXTENT: u16 = ELISION_RESERVE + 1;

/// The final rung: a single letter marker (occasionally two, past 26
/// dense-neighbourhood nodes — `graph_presentation::assign_markers`
/// widens as needed, but this module always reserves for the common
/// one-letter case, since a marker's presence, not its exact width, is
/// what geometry needs to know).
const MARKER_EXTENT: u16 = 1;

/// Worst-case width of a band connector glyph plus its trailing space,
/// drawn ahead of a lane's relation name in its gutter header:
/// `GraphGlyphs::ascii`'s `"-> "`/`"<- "` (three cells) — the unicode
/// arrows are single cells, again only ever narrower.
const CONNECTOR_RESERVE: u16 = 3;

/// Which rung of the label ladder [`choose_label_rung`] chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelRung {
    Full,
    LabelOnly,
    Elided,
    Marker,
}

fn full_rung_len(label: &str, kind: ResourceKind) -> u16 {
    // "{label} ({kind})" == label + " (" + kind + ")"
    (label.chars().count() + 2 + kind.as_str().chars().count() + 1) as u16
}

fn label_only_len(label: &str) -> u16 {
    label.chars().count() as u16
}

/// Decide the richest rung of the label ladder that fits in `available`
/// cells, and exactly how many cells that rung needs. Pure and idempotent
/// — see the section doc above for why that property is load-bearing, not
/// incidental.
pub fn choose_label_rung(label: &str, kind: ResourceKind, available: u16) -> (LabelRung, u16) {
    let available = available.min(MAX_LABEL_EXTENT);
    let full = full_rung_len(label, kind);
    if full <= available {
        return (LabelRung::Full, full);
    }
    let only = label_only_len(label);
    if only <= available {
        return (LabelRung::LabelOnly, only);
    }
    if available >= MIN_ELIDED_EXTENT {
        return (LabelRung::Elided, available);
    }
    (LabelRung::Marker, MARKER_EXTENT)
}

/// Whether `relation` can be shown verbatim, alongside its band's own word
/// (`"Incoming"`/`"Outgoing"`/`"Context"`/`"Contained"` — doctrine: human-
/// readable words before glyph puzzles, so the connector glyph alone is
/// never the only way to tell bands apart on the canvas) and a band
/// connector glyph, in `available` cells — and how many cells the whole
/// gutter header needs if so. Relation strings are provider vocabulary —
/// spec requirement: never invented, abbreviated or normalised — so unlike
/// a node's own label, a relation name that does not fit is *omitted* from
/// the canvas gutter entirely, never elided into a fragment of itself. `0`
/// means omitted; the legend still names the relation in full regardless.
fn lane_gutter_extent(band: RelationBand, relation: &str, available: u16) -> u16 {
    let need = band.label().chars().count() as u16
        + 1
        + CONNECTOR_RESERVE
        + relation.chars().count() as u16;
    if need <= available {
        need
    } else {
        0
    }
}

/// Compute the focus-centred layout of one relation neighbourhood.
///
/// Pure: no I/O, no clock, no randomness, no interior mutability. Every
/// ordering decision goes through a `BTreeMap`/`BTreeSet` or an explicit
/// sort, never a `HashMap`, so shuffling `view.nodes`/`view.edges` before
/// calling this cannot change the result.
pub fn layout(view: &KnowledgeRelationView, request: &GraphLayoutRequest) -> GraphLayout {
    let focus = view.query.focus.clone();
    let mut warnings = view.warnings.clone();

    let nodes_by_ref: BTreeMap<ResourceRef, &aikit_core::RelationNode> = view
        .nodes
        .iter()
        .map(|node| (node.resource.clone(), node))
        .collect();

    let Some(focus_node) = nodes_by_ref.get(&focus) else {
        warnings.push(format!(
            "graph_layout: focus {focus} is not present among the view's nodes; nothing to lay out"
        ));
        return GraphLayout {
            focus,
            nodes: Vec::new(),
            edges: Vec::new(),
            lanes: Vec::new(),
            truncated: view.truncated,
            dropped: GraphTruncation::default(),
            warnings,
        };
    };

    // A globally sorted edge list is the single source of determinism: every
    // walk below iterates this Vec, never `view.edges` directly, so input
    // order (and any HashMap the provider may have iterated internally)
    // cannot leak into the result.
    let mut sorted_edges: Vec<&RelationEdge> = view.edges.iter().collect();
    sorted_edges.sort_by(|a, b| {
        (a.relation.as_str(), a.from.to_string(), a.to.to_string()).cmp(&(
            b.relation.as_str(),
            b.from.to_string(),
            b.to.to_string(),
        ))
    });

    // --- Node bands: breadth-first from the focus over the sorted edges. ---
    //
    // Complexity is O(nodes * edges) — each pop rescans the full sorted
    // list for incident edges — which is fine because both are already
    // bounded by the core relation budgets (`DEFAULT_RELATION_NODE_BUDGET`
    // = 96, `DEFAULT_RELATION_EDGE_BUDGET` = 192, and `view.query` can only
    // shrink those further): worst case is on the order of 96 * 192, a few
    // thousand comparisons, not a scale where an adjacency index would be
    // observable.
    let mut node_band: BTreeMap<ResourceRef, RelationBand> = BTreeMap::new();
    let mut node_lane: BTreeMap<ResourceRef, String> = BTreeMap::new();
    let mut visited: BTreeSet<ResourceRef> = BTreeSet::from([focus.clone()]);
    let mut queue: VecDeque<ResourceRef> = VecDeque::from([focus.clone()]);
    while let Some(current) = queue.pop_front() {
        for edge in &sorted_edges {
            if edge.from == edge.to {
                continue; // self-loops never discover a new node
            }
            let (other, anchor_is_from) = if edge.from == current {
                (&edge.to, true)
            } else if edge.to == current {
                (&edge.from, false)
            } else {
                continue;
            };
            if visited.contains(other) {
                continue;
            }
            if !nodes_by_ref.contains_key(other) {
                // push_edge on KnowledgeRelationView already forbids this,
                // but a hand-built or degraded view is not trusted here.
                continue;
            }
            let band = classify_band(edge.containment, edge.direction, anchor_is_from);
            node_band.insert(other.clone(), band);
            node_lane.insert(other.clone(), edge.relation.clone());
            visited.insert(other.clone());
            queue.push_back(other.clone());
        }
    }

    // Any node the provider returned but no edge in this view actually
    // reaches from the focus (a disconnected fragment) still needs a place
    // to sit; it takes the Outgoing lane under its own resource ref, and a
    // warning records that the position is not a real relation reading.
    for resource in nodes_by_ref.keys() {
        if resource != &focus && !visited.contains(resource) {
            node_band.insert(resource.clone(), RelationBand::Outgoing);
            node_lane.insert(resource.clone(), resource.to_string());
            warnings.push(format!(
                "graph_layout: {resource} is not reachable from focus {focus} within this view's edges; placed disconnected"
            ));
        }
    }

    // --- Group into deterministic lanes per band. ---
    let mut lanes_by_band: BTreeMap<RelationBand, BTreeMap<String, Vec<ResourceRef>>> =
        BTreeMap::new();
    for (resource, band) in &node_band {
        let lane = node_lane.get(resource).cloned().unwrap_or_default();
        lanes_by_band
            .entry(*band)
            .or_default()
            .entry(lane)
            .or_default()
            .push(resource.clone());
    }
    for lanes in lanes_by_band.values_mut() {
        for members in lanes.values_mut() {
            members.sort_by_key(|resource| resource.to_string());
        }
    }
    let ordered_lanes_for = |band: RelationBand| -> Vec<Lane> {
        lanes_by_band
            .get(&band)
            .into_iter()
            .flat_map(|lanes| lanes.iter())
            .map(|(relation, members)| (relation.clone(), members.clone()))
            .collect()
    };

    // --- Placement. Focus at the viewport centre; extent for every node's
    // own label — and every lane's own relation-name gutter header — is
    // reserved by the same pass that decides where it goes, so nothing
    // placed here can ever be handed cells another node already claimed.
    // Purely arithmetic, never a physics step: identical input always
    // yields identical coordinates. ---
    let cx = request.viewport.width as i32 / 2;
    let cy = request.viewport.height as i32 / 2;
    let width = request.viewport.width as i32;

    let mut positions: BTreeMap<ResourceRef, GraphPoint> = BTreeMap::new();
    let mut label_widths: BTreeMap<ResourceRef, u16> = BTreeMap::new();
    let mut lane_headers: Vec<LaidOutLane> = Vec::new();
    let mut dropped = GraphTruncation::default();
    let mut visible: BTreeSet<ResourceRef> = BTreeSet::from([focus.clone()]);
    positions.insert(focus.clone(), GraphPoint { x: cx, y: cy });
    label_widths.insert(focus.clone(), MARKER_EXTENT);

    // Vertical bands (Incoming/Outgoing): one row per member, so members
    // never contend with each other for width — every one gets the full
    // distance from its anchor to the viewport edge to work with. Only
    // vertical *rows* are a shared, contended budget (one reserved for the
    // focus's own row), and a lane's header row is part of that cost,
    // charged exactly once, only when at least one of its members
    // actually survives the budget.
    let vertical_capacity = request.viewport.height.saturating_sub(1) as i32;
    let (_incoming, incoming_rows) = place_vertical_band(
        ordered_lanes_for(RelationBand::Incoming),
        &nodes_by_ref,
        RelationBand::Incoming,
        cx - SIDE_OFFSET,
        false,
        width,
        cy,
        vertical_capacity,
        &mut positions,
        &mut label_widths,
        &mut lane_headers,
        &mut visible,
        &mut dropped,
    );
    let (_outgoing, outgoing_rows) = place_vertical_band(
        ordered_lanes_for(RelationBand::Outgoing),
        &nodes_by_ref,
        RelationBand::Outgoing,
        cx + SIDE_OFFSET,
        true,
        width,
        cy,
        vertical_capacity,
        &mut positions,
        &mut label_widths,
        &mut lane_headers,
        &mut visible,
        &mut dropped,
    );

    // Horizontal bands (Context/Contained) each get their own dedicated
    // row, pushed far enough from the focus row that it cannot land on any
    // vertical-band member's row — vertical members can legitimately sit
    // on the focus's own row (a lone lane centres on it), so a fixed
    // two-row offset is not always enough once a vertical band is tall;
    // this widens with whichever side actually grew taller, so the two
    // kinds of band can never end up sharing a row (and therefore never
    // sharing a cell) regardless of label width.
    let vertical_span = incoming_rows.max(outgoing_rows) / 2;
    let vertical_offset = (vertical_span + 2).max(2);
    let (_context, _) = place_horizontal_band(
        ordered_lanes_for(RelationBand::Context),
        &nodes_by_ref,
        RelationBand::Context,
        cy - vertical_offset,
        cx,
        width,
        &mut positions,
        &mut label_widths,
        &mut lane_headers,
        &mut visible,
        &mut dropped,
    );
    let (_contained, _) = place_horizontal_band(
        ordered_lanes_for(RelationBand::Contained),
        &nodes_by_ref,
        RelationBand::Contained,
        cy + vertical_offset,
        cx,
        width,
        &mut positions,
        &mut label_widths,
        &mut lane_headers,
        &mut visible,
        &mut dropped,
    );
    lane_headers.sort_by(|a, b| (a.band, a.relation.as_str()).cmp(&(b.band, b.relation.as_str())));

    // --- Materialise visible nodes. ---
    let mut out_nodes: Vec<LaidOutNode> = Vec::new();
    out_nodes.push(LaidOutNode {
        resource: focus.clone(),
        kind: focus_node.kind,
        label: focus_node.label.clone(),
        state: focus_node.state.clone(),
        band: None,
        lane: None,
        is_focus: true,
        position: positions[&focus],
        label_width: MARKER_EXTENT,
    });
    for resource in &visible {
        if resource == &focus {
            continue;
        }
        let Some(node) = nodes_by_ref.get(resource) else {
            continue;
        };
        out_nodes.push(LaidOutNode {
            resource: resource.clone(),
            kind: node.kind,
            label: node.label.clone(),
            state: node.state.clone(),
            band: node_band.get(resource).copied(),
            lane: node_lane.get(resource).cloned(),
            is_focus: false,
            position: positions
                .get(resource)
                .copied()
                .unwrap_or(GraphPoint { x: cx, y: cy }),
            label_width: label_widths.get(resource).copied().unwrap_or(MARKER_EXTENT),
        });
    }
    out_nodes.sort_by_key(|node| node.resource.to_string());

    // --- Materialise retained edges: both endpoints must still be visible. ---
    let mut out_edges: Vec<LaidOutEdge> = Vec::new();
    for edge in &sorted_edges {
        if !visible.contains(&edge.from) || !visible.contains(&edge.to) {
            dropped.edges_dropped += 1;
            continue;
        }
        out_edges.push(LaidOutEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            relation: edge.relation.clone(),
            direction: edge.direction,
            origin: edge.origin.clone(),
            band: edge_band(edge, &focus, &node_band),
        });
    }
    out_edges.sort_by(|a, b| {
        (
            a.band,
            a.relation.as_str(),
            a.from.to_string(),
            a.to.to_string(),
        )
            .cmp(&(
                b.band,
                b.relation.as_str(),
                b.from.to_string(),
                b.to.to_string(),
            ))
    });

    let truncated = view.truncated || dropped.nodes_dropped > 0 || dropped.edges_dropped > 0;
    if dropped.nodes_dropped > 0 || dropped.edges_dropped > 0 {
        warnings.push(format!(
            "graph_layout: viewport budget dropped {} node(s) and {} edge(s)",
            dropped.nodes_dropped, dropped.edges_dropped
        ));
    }

    GraphLayout {
        focus,
        nodes: out_nodes,
        edges: out_edges,
        lanes: lane_headers,
        truncated,
        dropped,
        warnings,
    }
}

/// Horizontal spacing, in cells, of a vertical band's anchor column from
/// the focus's own column — shared by [`layout`] and
/// [`place_vertical_band`].
const SIDE_OFFSET: i32 = 4;
/// Blank cells left between adjacent members, or adjacent lanes, along
/// whichever axis a band lays its content out on.
const LANE_GAP: i32 = 1;
/// Blank cells left between a lane's gutter header and its first member.
const GUTTER_GAP: i32 = 1;

/// Place one vertical band's (Incoming/Outgoing) lanes: one row per
/// member, plus one header row per lane that survives with at least one
/// member. Rows are a shared, contended budget (`budget_rows`); width is
/// not — every member sits on its own row, so it is given the full
/// distance from `anchor_x` to the viewport edge to work with,
/// independent of every other member in the band. `grows_right` says
/// which way that distance runs, and which way a member's/header's own
/// text grows from its position: `true` for Outgoing (away from the focus,
/// to the right), `false` for Incoming (away from the focus, to the left —
/// text is therefore laid out ending at `anchor_x`, not starting there, so
/// it grows away from the focus rather than into it).
///
/// Returns the kept lanes (for `grouped_projection`-adjacent callers that
/// might want them — currently unused beyond `layout` itself, kept for
/// symmetry with the pre-existing shape of this code) and the total row
/// count consumed, which `layout` uses to keep the horizontal bands from
/// ever landing on one of these rows.
#[allow(clippy::too_many_arguments)]
fn place_vertical_band(
    lanes: Vec<Lane>,
    nodes_by_ref: &BTreeMap<ResourceRef, &aikit_core::RelationNode>,
    band: RelationBand,
    anchor_x: i32,
    grows_right: bool,
    viewport_width: i32,
    cy: i32,
    budget_rows: i32,
    positions: &mut BTreeMap<ResourceRef, GraphPoint>,
    label_widths: &mut BTreeMap<ResourceRef, u16>,
    lane_headers: &mut Vec<LaidOutLane>,
    visible: &mut BTreeSet<ResourceRef>,
    dropped: &mut GraphTruncation,
) -> (Vec<Lane>, i32) {
    // Row-budget trim: a lane's header row is charged exactly once, only
    // once at least one of its members is actually kept.
    let mut kept_lanes: Vec<Lane> = Vec::new();
    let mut used_rows = 0i32;
    for (relation, members) in lanes {
        let mut kept_members = Vec::new();
        for member in members {
            let header_cost = if kept_members.is_empty() { 1 } else { 0 };
            if used_rows + header_cost + 1 > budget_rows {
                dropped.nodes_dropped += 1;
                continue;
            }
            used_rows += header_cost + 1;
            kept_members.push(member);
        }
        if !kept_members.is_empty() {
            kept_lanes.push((relation, kept_members));
        }
    }

    // Every row in this band gets the same fixed distance to the viewport
    // edge — rows never contend with each other for width.
    let available: u16 = if grows_right {
        (viewport_width - anchor_x).max(0) as u16
    } else {
        (anchor_x + 1).max(0) as u16
    };

    let total_rows: i32 = kept_lanes
        .iter()
        .map(|(_, members)| 1 + members.len() as i32)
        .sum::<i32>()
        + LANE_GAP * kept_lanes.len().saturating_sub(1) as i32;
    let mut y = cy - total_rows / 2;
    for (relation, members) in &kept_lanes {
        let gutter_extent = lane_gutter_extent(band, relation, available);
        if gutter_extent > 0 {
            let header_x = if grows_right {
                anchor_x
            } else {
                anchor_x - gutter_extent as i32 + 1
            };
            lane_headers.push(LaidOutLane {
                band,
                relation: relation.clone(),
                position: GraphPoint { x: header_x, y },
            });
        }
        y += 1;
        for member in members {
            let Some(node) = nodes_by_ref.get(member) else {
                y += 1;
                continue;
            };
            let (_, extent) = choose_label_rung(&node.label, node.kind, available);
            let x = if grows_right {
                anchor_x
            } else {
                anchor_x - extent as i32 + 1
            };
            positions.insert(member.clone(), GraphPoint { x, y });
            label_widths.insert(member.clone(), extent);
            visible.insert(member.clone());
            y += 1;
        }
        y += LANE_GAP;
    }
    (kept_lanes, total_rows)
}

/// Place one horizontal band's (Context/Contained) lanes along a single
/// shared row `y`, centred on `cx`. Unlike a vertical band, width here
/// *is* contended — every lane's gutter header and every member draws from
/// one running cell budget, spent left to right in deterministic lane
/// order then member order, so a member the budget cannot fit is dropped
/// exactly as a viewport-budget overflow always has been here (never drawn
/// as a collision with its neighbour).
///
/// Returns the kept lanes and this band's own row count (always `1` when
/// anything survived, `0` otherwise) — the latter unused by `layout` today
/// but kept for signature symmetry with [`place_vertical_band`].
#[allow(clippy::too_many_arguments)]
fn place_horizontal_band(
    lanes: Vec<Lane>,
    nodes_by_ref: &BTreeMap<ResourceRef, &aikit_core::RelationNode>,
    band: RelationBand,
    y: i32,
    cx: i32,
    viewport_width: i32,
    positions: &mut BTreeMap<ResourceRef, GraphPoint>,
    label_widths: &mut BTreeMap<ResourceRef, u16>,
    lane_headers: &mut Vec<LaidOutLane>,
    visible: &mut BTreeSet<ResourceRef>,
    dropped: &mut GraphTruncation,
) -> (Vec<Lane>, i32) {
    let mut cursor = 0i32; // relative to this band's own start; offset by `start_x` below
    let mut remaining = viewport_width;
    let mut kept_lanes: Vec<Lane> = Vec::new();
    let mut rel_headers: Vec<(String, i32, u16)> = Vec::new();
    let mut rel_members: Vec<(ResourceRef, i32, u16)> = Vec::new();

    for (relation, members) in lanes {
        let gutter_extent = lane_gutter_extent(band, &relation, remaining.max(0) as u16);
        if gutter_extent > 0 {
            rel_headers.push((relation.clone(), cursor, gutter_extent));
            cursor += gutter_extent as i32;
            remaining -= gutter_extent as i32;
            cursor += GUTTER_GAP;
            remaining -= GUTTER_GAP;
        }
        let mut kept_members = Vec::new();
        for (index, member) in members.into_iter().enumerate() {
            if index > 0 {
                cursor += LANE_GAP;
                remaining -= LANE_GAP;
            }
            if remaining < MARKER_EXTENT as i32 {
                dropped.nodes_dropped += 1;
                continue;
            }
            let Some(node) = nodes_by_ref.get(&member) else {
                continue;
            };
            let (_, extent) = choose_label_rung(&node.label, node.kind, remaining.max(0) as u16);
            rel_members.push((member.clone(), cursor, extent));
            cursor += extent as i32;
            remaining -= extent as i32;
            kept_members.push(member);
        }
        if !kept_members.is_empty() {
            kept_lanes.push((relation, kept_members));
        }
        cursor += LANE_GAP;
        remaining -= LANE_GAP;
    }

    let total_used = (cursor - LANE_GAP).max(0);
    let start_x = cx - total_used / 2;
    for (relation, rel_x, _) in rel_headers {
        lane_headers.push(LaidOutLane {
            band,
            relation,
            position: GraphPoint {
                x: start_x + rel_x,
                y,
            },
        });
    }
    for (resource, rel_x, extent) in rel_members {
        positions.insert(
            resource.clone(),
            GraphPoint {
                x: start_x + rel_x,
                y,
            },
        );
        label_widths.insert(resource.clone(), extent);
        visible.insert(resource);
    }
    let rows_used = if kept_lanes.is_empty() { 0 } else { 1 };
    (kept_lanes, rows_used)
}

// ---------------------------------------------------------------------------
// Narrow/grouped fallback projection
// ---------------------------------------------------------------------------

/// One line of the narrow-terminal fallback (spec §8.3 "Narrow fallback"):
/// `<relation> -> <target>`, grouped under a band header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedRelationLine {
    pub band: RelationBand,
    pub relation: String,
    pub target: ResourceRef,
    pub target_label: String,
    pub direction: RelationDirection,
    pub origin: RelationOrigin,
}

/// Project an already-computed [`GraphLayout`] into the narrow fallback
/// shape. Derived from the *same* laid-out edges (post-viewport-truncation)
/// so the narrow rendering never shows a node the spatial rendering
/// dropped, and a terminal that grows back out finds the same graph
/// semantic state waiting for it.
pub fn grouped_projection(layout: &GraphLayout) -> Vec<GroupedRelationLine> {
    let labels: BTreeMap<&ResourceRef, &str> = layout
        .nodes
        .iter()
        .map(|node| (&node.resource, node.label.as_str()))
        .collect();
    layout
        .edges
        .iter()
        .map(|edge| {
            let other = if edge.from == layout.focus {
                &edge.to
            } else {
                &edge.from
            };
            GroupedRelationLine {
                band: edge.band,
                relation: edge.relation.clone(),
                target: other.clone(),
                target_label: labels
                    .get(other)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| other.to_string()),
                direction: edge.direction,
                origin: edge.origin.clone(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Glyphs
// ---------------------------------------------------------------------------

/// The connector/arrow character set, in the manner of
/// `crates/aikit-tui/src/layout.rs`'s `Glyphs`: two complete sets rather
/// than a per-glyph fallback, so a rendering cannot end up three-quarters
/// Unicode because someone forgot one match arm. No Nerd Font codepoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphGlyphs {
    ascii: bool,
}

impl GraphGlyphs {
    pub fn unicode() -> Self {
        Self { ascii: false }
    }

    pub fn ascii() -> Self {
        Self { ascii: true }
    }

    pub fn is_ascii(&self) -> bool {
        self.ascii
    }

    /// The glyph for a node's band, drawn between the focus and that node.
    /// Every band carries a visually distinct glyph so the four regions
    /// stay legible with colour entirely absent.
    pub fn band_connector(&self, band: RelationBand) -> &'static str {
        match (band, self.ascii) {
            (RelationBand::Incoming, false) => "\u{2190}",
            (RelationBand::Incoming, true) => "<-",
            (RelationBand::Outgoing, false) => "\u{2192}",
            (RelationBand::Outgoing, true) => "->",
            (RelationBand::Context, false) => "\u{25b2}",
            (RelationBand::Context, true) => "^",
            (RelationBand::Contained, false) => "\u{25bc}",
            (RelationBand::Contained, true) => "v",
        }
    }

    /// The glyph for an edge's own `RelationDirection`, independent of
    /// which band it landed in — the Inspector (spec §8.3) shows both.
    pub fn direction_glyph(&self, direction: RelationDirection) -> &'static str {
        match (direction, self.ascii) {
            (RelationDirection::Outgoing, false) => "\u{2192}",
            (RelationDirection::Outgoing, true) => "->",
            (RelationDirection::Incoming, false) => "\u{2190}",
            (RelationDirection::Incoming, true) => "<-",
            (RelationDirection::Bidirectional, false) => "\u{2194}",
            (RelationDirection::Bidirectional, true) => "<->",
        }
    }

    /// The focus marker at the centre of the graph.
    pub fn focus_marker(&self) -> char {
        if self.ascii {
            '*'
        } else {
            '\u{25c6}'
        }
    }

    /// The elision mark for rung 3 of the label ladder (see
    /// `choose_label_rung`'s doc). Always narrower than or equal to
    /// [`ELISION_RESERVE`] — the width geometry reserved for it — so this
    /// never draws past a cell `graph_layout` already promised to another
    /// node; unicode's single-cell mark just leaves a little unused slack.
    pub fn ellipsis(&self) -> &'static str {
        if self.ascii {
            "..."
        } else {
            "\u{2026}"
        }
    }

    /// The shell glyph set this connector set belongs to, for the prose the
    /// Graph draws around its canvas — the Inspector's field separators, say.
    /// The inverse of `application_surface::graph_glyphs_for`, and the reason
    /// a mark like the separator is defined once, in `layout.rs`, instead of
    /// being copied into this type where the two could drift apart.
    pub fn shell(&self) -> crate::layout::Glyphs {
        if self.ascii {
            crate::layout::Glyphs::ascii()
        } else {
            crate::layout::Glyphs::unicode()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::{ProviderRef, RelationNode, RelationOrigin, RelationQuery, SourceAuthority};

    fn r(raw: &str) -> ResourceRef {
        ResourceRef::parse(raw).unwrap()
    }

    fn node(raw: &str, kind: ResourceKind, label: &str) -> RelationNode {
        RelationNode::new(r(raw), kind, label)
    }

    fn origin(authority: SourceAuthority) -> RelationOrigin {
        RelationOrigin::new(authority)
            .from_provider(ProviderRef::parse("provider/test").unwrap())
            .in_lens("test-lens")
            .at_revision("rev-1")
    }

    fn query(focus: &str, max_nodes: usize, max_edges: usize) -> RelationQuery {
        RelationQuery {
            focus: r(focus),
            depth: 2,
            max_nodes,
            max_edges,
            filters: Vec::new(),
        }
    }

    fn viewport(width: u16, height: u16) -> GraphLayoutRequest {
        GraphLayoutRequest::for_viewport(GraphViewport::new(width, height))
    }

    fn focus_view(focus_kind: ResourceKind) -> KnowledgeRelationView {
        KnowledgeRelationView::focus_only(
            query("knowledge-node/focus", 96, 192),
            node("knowledge-node/focus", focus_kind, "Focus"),
        )
        .unwrap()
    }

    // --- determinism -------------------------------------------------------

    #[test]
    fn identical_input_yields_identical_output() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_node(node("knowledge-node/a", ResourceKind::KnowledgeNode, "A"));
        view.push_node(node(
            "knowledge-source/b",
            ResourceKind::KnowledgeSource,
            "B",
        ));
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-node/a"),
            "cites",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Authored),
        ))
        .unwrap();
        view.push_edge(RelationEdge::new(
            r("knowledge-source/b"),
            r("knowledge-node/focus"),
            "grounded-in",
            RelationDirection::Incoming,
            origin(SourceAuthority::Observed),
        ))
        .unwrap();

        let request = viewport(80, 24);
        let first = layout(&view, &request);
        let second = layout(&view, &request);
        assert_eq!(first, second);
    }

    #[test]
    fn shuffled_input_vectors_yield_the_same_layout() {
        let mut ordered = focus_view(ResourceKind::KnowledgeNode);
        ordered.push_node(node("knowledge-node/a", ResourceKind::KnowledgeNode, "A"));
        ordered.push_node(node("knowledge-node/b", ResourceKind::KnowledgeNode, "B"));
        ordered.push_node(node("knowledge-node/c", ResourceKind::KnowledgeNode, "C"));
        ordered
            .push_edge(RelationEdge::new(
                r("knowledge-node/focus"),
                r("knowledge-node/a"),
                "cites",
                RelationDirection::Outgoing,
                origin(SourceAuthority::Authored),
            ))
            .unwrap();
        ordered
            .push_edge(RelationEdge::new(
                r("knowledge-node/b"),
                r("knowledge-node/focus"),
                "cites",
                RelationDirection::Incoming,
                origin(SourceAuthority::Authored),
            ))
            .unwrap();
        ordered
            .push_edge(RelationEdge::new(
                r("knowledge-node/focus"),
                r("knowledge-node/c"),
                "member",
                RelationDirection::Outgoing,
                origin(SourceAuthority::Authored),
            ))
            .unwrap();

        // Rebuild the same content with nodes/edges pushed in reverse order.
        // The public `push_node`/`push_edge` refuse duplicate/out-of-order
        // endpoints in ways that make literally shuffling the same view
        // awkward to construct twice, so this reconstructs an
        // observationally identical view (same focus, same node set, same
        // edge set) from a different insertion order and checks the layout
        // is byte-identical regardless.
        let mut shuffled = focus_view(ResourceKind::KnowledgeNode);
        shuffled.push_node(node("knowledge-node/c", ResourceKind::KnowledgeNode, "C"));
        shuffled.push_node(node("knowledge-node/b", ResourceKind::KnowledgeNode, "B"));
        shuffled.push_node(node("knowledge-node/a", ResourceKind::KnowledgeNode, "A"));
        shuffled
            .push_edge(RelationEdge::new(
                r("knowledge-node/focus"),
                r("knowledge-node/c"),
                "member",
                RelationDirection::Outgoing,
                origin(SourceAuthority::Authored),
            ))
            .unwrap();
        shuffled
            .push_edge(RelationEdge::new(
                r("knowledge-node/b"),
                r("knowledge-node/focus"),
                "cites",
                RelationDirection::Incoming,
                origin(SourceAuthority::Authored),
            ))
            .unwrap();
        shuffled
            .push_edge(RelationEdge::new(
                r("knowledge-node/focus"),
                r("knowledge-node/a"),
                "cites",
                RelationDirection::Outgoing,
                origin(SourceAuthority::Authored),
            ))
            .unwrap();

        let request = viewport(80, 24);
        assert_eq!(layout(&ordered, &request), layout(&shuffled, &request));
    }

    // --- placement -----------------------------------------------------------

    #[test]
    fn outgoing_edge_lands_in_outgoing_band() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_node(node("knowledge-node/a", ResourceKind::KnowledgeNode, "A"));
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-node/a"),
            "cites",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Authored),
        ))
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let a = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-node/a"))
            .unwrap();
        assert_eq!(a.band, Some(RelationBand::Outgoing));
        assert_eq!(result.edges[0].band, RelationBand::Outgoing);
    }

    #[test]
    fn incoming_edge_lands_in_incoming_band() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_node(node("knowledge-node/a", ResourceKind::KnowledgeNode, "A"));
        view.push_edge(RelationEdge::new(
            r("knowledge-node/a"),
            r("knowledge-node/focus"),
            "grounded-in",
            RelationDirection::Incoming,
            origin(SourceAuthority::Observed),
        ))
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let a = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-node/a"))
            .unwrap();
        assert_eq!(a.band, Some(RelationBand::Incoming));
        assert_eq!(result.edges[0].band, RelationBand::Incoming);
    }

    #[test]
    fn focus_containing_other_lands_in_contained_band() {
        let mut view = focus_view(ResourceKind::KnowledgeSpace);
        view.push_node(node(
            "knowledge-node/member",
            ResourceKind::KnowledgeNode,
            "Member",
        ));
        view.push_edge(
            RelationEdge::new(
                r("knowledge-node/focus"),
                r("knowledge-node/member"),
                "member",
                RelationDirection::Outgoing,
                origin(SourceAuthority::Authored),
            )
            .with_containment(ContainmentRole::Encloses),
        )
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let member = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-node/member"))
            .unwrap();
        assert_eq!(member.band, Some(RelationBand::Contained));
        assert_eq!(result.edges[0].band, RelationBand::Contained);
    }

    #[test]
    fn other_containing_focus_lands_in_context_band() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_node(node(
            "knowledge-space/whole",
            ResourceKind::KnowledgeSpace,
            "Whole",
        ));
        view.push_edge(
            RelationEdge::new(
                r("knowledge-space/whole"),
                r("knowledge-node/focus"),
                "member",
                RelationDirection::Outgoing,
                origin(SourceAuthority::Authored),
            )
            .with_containment(ContainmentRole::Encloses),
        )
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let whole = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-space/whole"))
            .unwrap();
        assert_eq!(whole.band, Some(RelationBand::Context));
        assert_eq!(result.edges[0].band, RelationBand::Context);
    }

    #[test]
    fn relation_named_like_containment_without_provider_backing_falls_through_to_plain_direction() {
        // "member-of" *looks* container-shaped, but no provider asserts
        // `RelationEdge.containment` for it here — it is just an authored
        // WikiEdge whose human-chosen relation name happens to resemble
        // containment. This module must not guess containment from the
        // word; it must place the node by `RelationDirection` alone,
        // exactly like `"cites"` or any other ordinary relation.
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_node(node(
            "knowledge-space/container",
            ResourceKind::KnowledgeSpace,
            "Container",
        ));
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-space/container"),
            "member-of",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Authored),
        ))
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let container = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-space/container"))
            .unwrap();
        assert_eq!(
            container.band,
            Some(RelationBand::Outgoing),
            "an authored edge named like containment is not containment: only the provider \
             can assert that, and for an arbitrary relation string it asserts none"
        );
        assert_eq!(result.edges[0].band, RelationBand::Outgoing);
    }

    #[test]
    fn provider_asserted_containment_places_the_band_regardless_of_relation_name() {
        // "belongs-to-collection" is not, and never was, one of the old
        // hardcoded strings (`member`, `child-space`, `local-member`) —
        // proving placement now comes from `RelationEdge.containment`
        // alone, not from recognising a relation name. A string-table
        // classifier could never place this edge correctly.
        let mut view = focus_view(ResourceKind::KnowledgeSpace);
        view.push_node(node(
            "knowledge-node/item",
            ResourceKind::KnowledgeNode,
            "Item",
        ));
        view.push_edge(
            RelationEdge::new(
                r("knowledge-node/focus"),
                r("knowledge-node/item"),
                "belongs-to-collection",
                RelationDirection::Outgoing,
                origin(SourceAuthority::Authored),
            )
            .with_containment(ContainmentRole::Encloses),
        )
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let item = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-node/item"))
            .unwrap();
        assert_eq!(
            item.band,
            Some(RelationBand::Contained),
            "a provider's own containment assertion must place the node, no matter what the \
             relation string is spelled"
        );
        assert_eq!(result.edges[0].band, RelationBand::Contained);
    }

    #[test]
    fn relation_named_exactly_member_without_containment_assertion_is_not_containment() {
        // The inverse of the previous test, and the proof the old
        // `FORWARD_CONTAINMENT_RELATIONS` string table is truly gone: an
        // edge named exactly `"member"` — one of the three strings that
        // table used to hardcode as containment — must NOT land in the
        // Contained band when the provider does not assert
        // `RelationEdge.containment` for it. Under the old string-match
        // classifier this edge was indistinguishable from real wiki
        // membership; it must now fall through to plain direction, exactly
        // like any other relation.
        let mut view = focus_view(ResourceKind::KnowledgeSpace);
        view.push_node(node(
            "knowledge-node/member",
            ResourceKind::KnowledgeNode,
            "Member",
        ));
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-node/member"),
            "member",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Authored),
        ))
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let member = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-node/member"))
            .unwrap();
        assert_eq!(
            member.band,
            Some(RelationBand::Outgoing),
            "the relation string \"member\" alone must never imply containment; only \
             RelationEdge.containment may"
        );
        assert_eq!(result.edges[0].band, RelationBand::Outgoing);
    }

    #[test]
    fn enclosed_by_role_places_the_container_in_context_and_the_member_in_contained() {
        // No provider in this codebase emits `EnclosedBy` today (every
        // authored wiki containment edge asserts `Encloses` from the
        // container's side), but the classifier must still handle it
        // correctly wherever a future provider asserts a `from`-is-contained
        // edge: the *other* endpoint is the container.
        let mut enclosed_by_focus = focus_view(ResourceKind::KnowledgeNode);
        enclosed_by_focus.push_node(node(
            "knowledge-space/whole",
            ResourceKind::KnowledgeSpace,
            "Whole",
        ));
        enclosed_by_focus
            .push_edge(
                RelationEdge::new(
                    r("knowledge-node/focus"),
                    r("knowledge-space/whole"),
                    "belongs-to-collection",
                    RelationDirection::Outgoing,
                    origin(SourceAuthority::Authored),
                )
                .with_containment(ContainmentRole::EnclosedBy),
            )
            .unwrap();

        let result = layout(&enclosed_by_focus, &viewport(80, 24));
        let whole = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-space/whole"))
            .unwrap();
        assert_eq!(
            whole.band,
            Some(RelationBand::Context),
            "focus is EnclosedBy `to`, so `to` is the enclosing container and belongs in Context"
        );
        assert_eq!(result.edges[0].band, RelationBand::Context);

        let mut enclosed_by_other = focus_view(ResourceKind::KnowledgeSpace);
        enclosed_by_other.push_node(node(
            "knowledge-node/member",
            ResourceKind::KnowledgeNode,
            "Member",
        ));
        enclosed_by_other
            .push_edge(
                RelationEdge::new(
                    r("knowledge-node/member"),
                    r("knowledge-node/focus"),
                    "belongs-to-collection",
                    RelationDirection::Incoming,
                    origin(SourceAuthority::Authored),
                )
                .with_containment(ContainmentRole::EnclosedBy),
            )
            .unwrap();

        let result = layout(&enclosed_by_other, &viewport(80, 24));
        let member = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-node/member"))
            .unwrap();
        assert_eq!(
            member.band,
            Some(RelationBand::Contained),
            "`from` is EnclosedBy focus (`to`), so `from` is the contained member"
        );
        assert_eq!(result.edges[0].band, RelationBand::Contained);
    }

    #[test]
    fn bidirectional_relation_has_a_stable_documented_band() {
        let mut view = focus_view(ResourceKind::Capability);
        view.push_node(node(
            "capability/sibling",
            ResourceKind::Capability,
            "Sibling",
        ));
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("capability/sibling"),
            "related-skill",
            RelationDirection::Bidirectional,
            origin(SourceAuthority::Derived),
        ))
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let sibling = result
            .nodes
            .iter()
            .find(|n| n.resource == r("capability/sibling"))
            .unwrap();
        assert_eq!(sibling.band, Some(RelationBand::Outgoing));

        // And the same relation discovered from the opposite endpoint
        // (focus is `to`, not `from`) still lands Outgoing — the documented
        // convention does not depend on which side the focus is on.
        let mut reversed = focus_view(ResourceKind::Capability);
        reversed.push_node(node(
            "capability/sibling",
            ResourceKind::Capability,
            "Sibling",
        ));
        reversed
            .push_edge(RelationEdge::new(
                r("capability/sibling"),
                r("knowledge-node/focus"),
                "related-skill",
                RelationDirection::Bidirectional,
                origin(SourceAuthority::Derived),
            ))
            .unwrap();
        let reversed_result = layout(&reversed, &viewport(80, 24));
        let sibling = reversed_result
            .nodes
            .iter()
            .find(|n| n.resource == r("capability/sibling"))
            .unwrap();
        assert_eq!(sibling.band, Some(RelationBand::Outgoing));
    }

    // --- lanes -----------------------------------------------------------

    #[test]
    fn shared_relation_name_forms_one_contiguous_deterministic_lane() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_node(node("knowledge-node/z", ResourceKind::KnowledgeNode, "Z"));
        view.push_node(node("knowledge-node/a", ResourceKind::KnowledgeNode, "A"));
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-node/z"),
            "cites",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Authored),
        ))
        .unwrap();
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-node/a"),
            "cites",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Authored),
        ))
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let a = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-node/a"))
            .unwrap();
        let z = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-node/z"))
            .unwrap();
        assert_eq!(a.lane.as_deref(), Some("cites"));
        assert_eq!(z.lane.as_deref(), Some("cites"));
        // Lexicographic within the lane: A's row is above Z's.
        assert!(a.position.y < z.position.y);
    }

    // --- provenance --------------------------------------------------------

    #[test]
    fn origin_survives_layout_intact() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_node(node("knowledge-node/a", ResourceKind::KnowledgeNode, "A"));
        let full_origin = RelationOrigin::new(SourceAuthority::Learned)
            .from_provider(ProviderRef::parse("provider/gitnexus").unwrap())
            .in_lens("code-index")
            .at_revision("git-abc123");
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-node/a"),
            "calls",
            RelationDirection::Outgoing,
            full_origin.clone(),
        ))
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        assert_eq!(result.edges[0].origin, full_origin);
        assert_eq!(
            result.edges[0].origin.provider.as_ref().unwrap().as_str(),
            "provider/gitnexus"
        );
        assert_eq!(result.edges[0].origin.lens.as_deref(), Some("code-index"));
        assert_eq!(
            result.edges[0].origin.revision.as_deref(),
            Some("git-abc123")
        );
        assert_eq!(result.edges[0].origin.authority, SourceAuthority::Learned);
    }

    // --- bounds --------------------------------------------------------------

    #[test]
    fn large_neighbourhood_respects_viewport_budget_and_reports_truncation() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        for i in 0..40 {
            let raw = format!("knowledge-node/n{i:02}");
            view.push_node(node(&raw, ResourceKind::KnowledgeNode, &raw));
            view.push_edge(RelationEdge::new(
                r("knowledge-node/focus"),
                r(&raw),
                "cites",
                RelationDirection::Outgoing,
                origin(SourceAuthority::Authored),
            ))
            .unwrap();
        }
        // 40 Outgoing nodes sharing one "cites" lane; a 10-row-tall viewport
        // has 9 rows of vertical budget (one reserved for the focus's own
        // row), and this lane's single gutter header row (see
        // `place_vertical_band`) is charged out of that same budget, so 8
        // members remain, not 9.
        let result = layout(&view, &viewport(80, 10));
        assert!(result.truncated);
        assert_eq!(result.dropped.nodes_dropped, 40 - 8);
        let outgoing_visible = result
            .nodes
            .iter()
            .filter(|n| n.band == Some(RelationBand::Outgoing))
            .count();
        assert_eq!(outgoing_visible, 8);
        // Every remaining edge still has both endpoints among the visible
        // nodes.
        let visible: BTreeSet<&ResourceRef> = result.nodes.iter().map(|n| &n.resource).collect();
        for edge in &result.edges {
            assert!(visible.contains(&edge.from) && visible.contains(&edge.to));
        }
    }

    #[test]
    fn core_relation_budget_neighbourhood_lays_out_without_panicking() {
        // A synthetic neighbourhood near aikit-core's own default budgets
        // (96 nodes / 192 edges): two edges per node keeps us under the
        // edge budget while exercising a realistic fan-out. This is a
        // structural completion check, not a timing assertion — the BFS is
        // bounded by nodes*edges (see the comment in `layout`), so a
        // neighbourhood this size finishing at all demonstrates it is not
        // accidentally quadratic in anything unbounded.
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        for i in 0..90 {
            let raw = format!("knowledge-node/n{i:03}");
            assert!(view.push_node(node(&raw, ResourceKind::KnowledgeNode, &raw)));
        }
        for i in 0..90 {
            let raw = format!("knowledge-node/n{i:03}");
            let relation = if i % 2 == 0 { "cites" } else { "grounded-in" };
            let direction = if i % 2 == 0 {
                RelationDirection::Outgoing
            } else {
                RelationDirection::Incoming
            };
            let (from, to) = if i % 2 == 0 {
                (r("knowledge-node/focus"), r(&raw))
            } else {
                (r(&raw), r("knowledge-node/focus"))
            };
            view.push_edge(RelationEdge::new(
                from,
                to,
                relation,
                direction,
                origin(SourceAuthority::Authored),
            ))
            .unwrap();
        }
        assert_eq!(view.nodes.len(), 91);
        assert_eq!(view.edges.len(), 90);

        let result = layout(&view, &viewport(200, 200));
        assert_eq!(result.nodes.len(), 91);
        assert_eq!(result.edges.len(), 90);
        assert!(!result.truncated);
    }

    // --- grouped_projection ---------------------------------------------------

    #[test]
    fn grouped_projection_preserves_nodes_relations_and_bands() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_node(node("knowledge-node/a", ResourceKind::KnowledgeNode, "A"));
        view.push_node(node(
            "knowledge-space/whole",
            ResourceKind::KnowledgeSpace,
            "Whole",
        ));
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-node/a"),
            "cites",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Authored),
        ))
        .unwrap();
        view.push_edge(
            RelationEdge::new(
                r("knowledge-space/whole"),
                r("knowledge-node/focus"),
                "member",
                RelationDirection::Outgoing,
                origin(SourceAuthority::Authored),
            )
            .with_containment(ContainmentRole::Encloses),
        )
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        let lines = grouped_projection(&result);
        assert_eq!(lines.len(), result.edges.len());

        let cites = lines.iter().find(|l| l.relation == "cites").unwrap();
        assert_eq!(cites.band, RelationBand::Outgoing);
        assert_eq!(cites.target, r("knowledge-node/a"));
        assert_eq!(cites.target_label, "A");

        let member = lines.iter().find(|l| l.relation == "member").unwrap();
        assert_eq!(member.band, RelationBand::Context);
        assert_eq!(member.target, r("knowledge-space/whole"));
        assert_eq!(member.target_label, "Whole");
    }

    // --- degraded input ------------------------------------------------------

    #[test]
    fn empty_view_lays_out_to_nothing_without_panicking() {
        let view = KnowledgeRelationView {
            query: query("knowledge-node/missing", 8, 8),
            nodes: Vec::new(),
            edges: Vec::new(),
            truncated: false,
            warnings: Vec::new(),
        };
        let result = layout(&view, &viewport(80, 24));
        assert!(result.nodes.is_empty());
        assert!(result.edges.is_empty());
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn focus_only_view_lays_out_to_just_the_focus() {
        let view = focus_view(ResourceKind::KnowledgeNode);
        let result = layout(&view, &viewport(80, 24));
        assert_eq!(result.nodes.len(), 1);
        assert!(result.nodes[0].is_focus);
        assert!(result.edges.is_empty());
        assert!(!result.truncated);
    }

    #[test]
    fn already_truncated_view_stays_truncated() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.truncated = true;
        let result = layout(&view, &viewport(80, 24));
        assert!(result.truncated);
    }

    #[test]
    fn self_loop_on_focus_does_not_panic_and_lands_outgoing() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-node/focus"),
            "supersedes",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Authored),
        ))
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        assert_eq!(result.edges.len(), 1);
        assert_eq!(result.edges[0].band, RelationBand::Outgoing);
    }

    #[test]
    fn cycle_through_a_non_focus_node_does_not_panic() {
        let mut view = focus_view(ResourceKind::KnowledgeNode);
        view.push_node(node("knowledge-node/a", ResourceKind::KnowledgeNode, "A"));
        view.push_edge(RelationEdge::new(
            r("knowledge-node/focus"),
            r("knowledge-node/a"),
            "cites",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Authored),
        ))
        .unwrap();
        // Closes the cycle back onto the focus.
        view.push_edge(RelationEdge::new(
            r("knowledge-node/a"),
            r("knowledge-node/focus"),
            "cited-by",
            RelationDirection::Outgoing,
            origin(SourceAuthority::Derived),
        ))
        .unwrap();

        let result = layout(&view, &viewport(80, 24));
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.edges.len(), 2);
        // BFS discovers A through whichever incident edge sorts first
        // ("cited-by" < "cites"): that edge has the focus at `to`, so A's
        // own placement is Incoming — a deterministic consequence of the
        // sorted walk, not a special case. Neither edge's *own* band is
        // corrupted by this: each is classified directly against the focus.
        let a_node = result
            .nodes
            .iter()
            .find(|n| n.resource == r("knowledge-node/a"))
            .unwrap();
        assert_eq!(a_node.band, Some(RelationBand::Incoming));
        let outgoing_edge = result.edges.iter().find(|e| e.relation == "cites").unwrap();
        assert_eq!(outgoing_edge.band, RelationBand::Outgoing);
        let closing_edge = result
            .edges
            .iter()
            .find(|e| e.relation == "cited-by")
            .unwrap();
        assert_eq!(closing_edge.band, RelationBand::Incoming);
    }

    #[test]
    fn glyph_sets_carry_identical_distinctions() {
        let ascii = GraphGlyphs::ascii();
        let unicode = GraphGlyphs::unicode();
        let bands = [
            RelationBand::Incoming,
            RelationBand::Outgoing,
            RelationBand::Context,
            RelationBand::Contained,
        ];
        let ascii_glyphs: BTreeSet<&str> = bands.iter().map(|b| ascii.band_connector(*b)).collect();
        let unicode_glyphs: BTreeSet<&str> =
            bands.iter().map(|b| unicode.band_connector(*b)).collect();
        assert_eq!(ascii_glyphs.len(), bands.len());
        assert_eq!(unicode_glyphs.len(), bands.len());
        assert_ne!(ascii.focus_marker(), unicode.focus_marker());
    }
}
