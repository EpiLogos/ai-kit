//! TUI-facing rendering and interaction over [`crate::graph_layout`].
//!
//! `graph_layout` computes *where things go*; this module decides *how a
//! terminal shows that* and *how a key press or click turns into a semantic
//! Action*. The split matters: `graph_layout` is pure geometry with no
//! knowledge of ratatui, colour, or key bindings, so its determinism tests
//! stay cheap and its output is reusable by any future host. Everything here
//! is presentation, and none of it mutates `TuiState` directly — every
//! function either renders (`&GraphLayout -> Vec<Line>`) or answers "what
//! node/action does this input correspond to", leaving the actual mutation to
//! `reduce_tui` in `application.rs` via the ordinary `UiAction` path.
//!
//! ## Filtering is presentation, not a second query
//!
//! The graph-local filter (`/`) narrows the *already-fetched*
//! [`aikit_core::KnowledgeRelationView`] before handing it to
//! [`crate::graph_layout::layout`]; it never re-asks a provider. See
//! [`filtered_relation_view`].
//!
//! ## The spatial canvas draws words, not a letter grid
//!
//! `graph_layout` reserves each node exactly the cells its own label ladder
//! rung needs (see `graph_layout::choose_label_rung`), so the spatial canvas
//! ([`spatial_lines`]) draws that rung's text — the node's actual label,
//! with a kind hint where there is room for one — directly at its laid-out
//! position, and each lane's own relation name directly in the canvas as a
//! gutter header ahead of its members (`GraphLayout::lanes`). The
//! pre-existing single/double-letter marker is now only the ladder's *last*
//! rung, reached for a node whose position genuinely has no room for even a
//! short elision — and only markers, not every node, are what the legend
//! beneath the canvas still exists to key; see [`legend_lines`]'s own doc
//! for exactly what it does and does not repeat.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::{KnowledgeRelationView, RelationDirection, RelationEdge, RelationNode, ResourceRef};
use ratatui::text::{Line, Span};

use crate::graph_layout::{
    choose_label_rung, GraphGlyphs, GraphLayout, GraphPoint, GraphViewport, LabelRung,
    LaidOutNode, RelationBand,
};
use crate::theme::Theme;

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Narrow an already-fetched relation view to nodes whose label or Resource
/// identity contains `filter` (case-insensitive), plus the focus itself —
/// dropping the focus would make [`crate::graph_layout::layout`] refuse the
/// view outright, and a filter that hides what you are looking *from* is not
/// useful. Edges survive only when both endpoints survive. An empty/blank
/// filter is a no-op clone, so callers can always run the view through this
/// unconditionally.
pub fn filtered_relation_view(view: &KnowledgeRelationView, filter: &str) -> KnowledgeRelationView {
    let filter = filter.trim();
    if filter.is_empty() {
        return view.clone();
    }
    let needle = filter.to_lowercase();
    let focus = view.query.focus.clone();
    let nodes: Vec<RelationNode> = view
        .nodes
        .iter()
        .filter(|node| {
            node.resource == focus
                || node.label.to_lowercase().contains(&needle)
                || node.resource.as_str().to_lowercase().contains(&needle)
        })
        .cloned()
        .collect();
    let kept: std::collections::BTreeSet<&ResourceRef> = nodes.iter().map(|n| &n.resource).collect();
    let edges: Vec<RelationEdge> = view
        .edges
        .iter()
        .filter(|edge| kept.contains(&edge.from) && kept.contains(&edge.to))
        .cloned()
        .collect();
    let mut warnings = view.warnings.clone();
    if nodes.len() < view.nodes.len() {
        warnings.push(format!(
            "graph filter \"{filter}\" narrowed {} node(s) to {}",
            view.nodes.len(),
            nodes.len()
        ));
    }
    KnowledgeRelationView {
        query: view.query.clone(),
        nodes,
        edges,
        truncated: view.truncated,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Movement
// ---------------------------------------------------------------------------

/// Deterministic "sensible spatial movement" for arrow/hjkl navigation while
/// the Graph projection is active.
///
/// From the currently highlighted node's laid-out position (falling back to
/// the focus when nothing is highlighted, or the highlighted resource is not
/// among this layout's visible nodes — e.g. right after a recenter), every
/// *other* visible node (the focus included) is a candidate exactly when its
/// position lies strictly on the requested side along the requested axis
/// (`direction` is a unit vector: `(-1,0)`/`(1,0)`/`(0,-1)`/`(0,1)` for
/// left/right/up/down — both arrow keys and hjkl resolve to these same four
/// before calling this). Among candidates, the nearest wins, weighting
/// distance along the requested axis three times over perpendicular drift —
/// so moving down a lane prefers the next node in that same lane over a node
/// one row down but several lanes over. Ties (identical score) break on the
/// resource's own string ordering, so the same graph always resolves the
/// same key press to the same target: no hidden iteration-order dependency.
pub fn move_selection(
    layout: &GraphLayout,
    current: Option<&ResourceRef>,
    direction: (i32, i32),
) -> Option<ResourceRef> {
    let current_position = current
        .and_then(|resource| layout.nodes.iter().find(|node| &node.resource == resource))
        .or_else(|| layout.nodes.iter().find(|node| node.is_focus))?
        .position;

    let axis_is_horizontal = direction.0 != 0;
    let axis_sign = if axis_is_horizontal { direction.0 } else { direction.1 };

    let mut best: Option<(i32, ResourceRef)> = None;
    for node in &layout.nodes {
        if Some(&node.resource) == current {
            continue;
        }
        let ddx = node.position.x - current_position.x;
        let ddy = node.position.y - current_position.y;
        let (primary, perpendicular) = if axis_is_horizontal { (ddx, ddy) } else { (ddy, ddx) };
        if primary * axis_sign <= 0 {
            continue; // not on the requested side, or exactly level
        }
        let score = primary.abs() * 3 + perpendicular.abs();
        best = Some(match best {
            None => (score, node.resource.clone()),
            Some((best_score, best_resource)) => {
                if score < best_score
                    || (score == best_score && node.resource.as_str() < best_resource.as_str())
                {
                    (score, node.resource.clone())
                } else {
                    (best_score, best_resource)
                }
            }
        });
    }
    best.map(|(_, resource)| resource)
}

/// Find the visible node, if any, whose own text or marker occupies
/// `(x, y)` in the spatial canvas's own coordinate space (the same space
/// [`spatial_lines`] draws into). Used by mouse hit-testing so a click
/// resolves to exactly the node a viewer would say they clicked on — the
/// node's full reserved width, per [`node_display_width`], not just its
/// first cell.
pub fn node_at(layout: &GraphLayout, x: i32, y: i32) -> Option<&ResourceRef> {
    let markers = assign_markers(layout);
    layout.nodes.iter().find_map(|node| {
        if node.position.y != y {
            return None;
        }
        let width = node_display_width(node, &markers);
        (x >= node.position.x && x < node.position.x + width).then_some(&node.resource)
    })
}

fn node_display_width(node: &LaidOutNode, markers: &BTreeMap<ResourceRef, String>) -> i32 {
    if node.is_focus {
        1
    } else if let Some(marker) = markers.get(&node.resource) {
        marker.len() as i32
    } else {
        node.label_width as i32
    }
}

// ---------------------------------------------------------------------------
// Label ladder rendering
// ---------------------------------------------------------------------------

/// Whether `node` landed on the label ladder's last rung — the pre-existing
/// letter marker — rather than any width of its own text. Pure function of
/// `(label, kind, label_width)`, so it always agrees with what
/// `graph_layout::layout` actually reserved; see that module's
/// `choose_label_rung` doc for why this is safe to recompute here instead
/// of layout handing presentation a flag.
fn is_marker_rung(node: &LaidOutNode) -> bool {
    matches!(
        choose_label_rung(&node.label, node.kind, node.label_width).0,
        LabelRung::Marker
    )
}

/// The exact text to draw at a non-focus node's position — one rung of the
/// label ladder documented on `graph_layout::choose_label_rung`, chosen by
/// calling that same pure function again with `available` set to
/// `node.label_width` (the width `layout` already reserved for it), which
/// is guaranteed to reproduce the identical rung. `None` means the ladder
/// bottomed out at the letter-marker rung; the caller falls back to
/// [`assign_markers`].
fn node_text(node: &LaidOutNode, glyphs: &GraphGlyphs) -> Option<String> {
    let (rung, extent) = choose_label_rung(&node.label, node.kind, node.label_width);
    match rung {
        LabelRung::Full => Some(format!("{} ({})", node.label, node.kind.as_str())),
        LabelRung::LabelOnly => Some(node.label.clone()),
        LabelRung::Elided => {
            let ellipsis = glyphs.ellipsis();
            let ellipsis_len = ellipsis.chars().count() as u16;
            let prefix_len = extent.saturating_sub(ellipsis_len).max(1) as usize;
            let prefix: String = node.label.chars().take(prefix_len).collect();
            Some(format!("{prefix}{ellipsis}"))
        }
        LabelRung::Marker => None,
    }
}

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

/// Assign every visible node still on the label ladder's last rung
/// ([`is_marker_rung`]) a short, unique, deterministic marker: spreadsheet-
/// column style (`a`..`z`, `aa`..`az`, ...) over `layout.nodes`' own stable
/// order (sorted by Resource string — see `graph_layout::layout`), so the
/// same neighbourhood always assigns the same letters to the same nodes. A
/// node whose own label or a shorter elision of it fit at its position
/// never receives a marker at all — its name is already on the canvas.
/// Two-letter markers only appear past 26 marker-rung nodes in one layout,
/// which a genuinely dense neighbourhood can still reach even though most
/// nodes around it are shown by name.
fn assign_markers(layout: &GraphLayout) -> BTreeMap<ResourceRef, String> {
    let mut markers = BTreeMap::new();
    let mut index = 0u32;
    for node in &layout.nodes {
        if node.is_focus || !is_marker_rung(node) {
            continue;
        }
        markers.insert(node.resource.clone(), marker_str(index));
        index += 1;
    }
    markers
}

fn marker_str(mut index: u32) -> String {
    let mut chars = Vec::new();
    loop {
        let remainder = index % 26;
        chars.push((b'a' + remainder as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    chars.iter().rev().collect()
}

// ---------------------------------------------------------------------------
// Spatial rendering
// ---------------------------------------------------------------------------

/// The combined spatial rendering: a one-cell-per-node canvas at each node's
/// exact laid-out position, a legend translating every marker back to its
/// label/kind/relation/provenance grouped by band and lane, and an Inspector
/// section for whichever node `selected` currently names (see the module
/// doc for why full labels cannot be drawn inline).
pub fn spatial_lines(
    layout: &GraphLayout,
    selected: Option<&ResourceRef>,
    glyphs: &GraphGlyphs,
    theme: &Theme,
    viewport: GraphViewport,
) -> Vec<Line<'static>> {
    let mut lines = canvas_lines(layout, selected, glyphs, theme, viewport);
    lines.push(Line::raw(""));
    lines.extend(legend_lines(layout, selected, glyphs, theme));
    lines.push(Line::raw(""));
    lines.extend(inspector_lines(layout, selected, glyphs, theme));
    if layout.truncated {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!(
                "truncated: {} node(s), {} edge(s) not shown",
                layout.dropped.nodes_dropped, layout.dropped.edges_dropped
            ),
            theme.unavailable(),
        )));
    }
    lines
}

/// Which style a canvas cell carries — disjoint from the character drawn
/// there, exactly like `layout.rs`'s own `Declared`/`DocStatus` split: no
/// state is inferred from another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellStyle {
    /// An ordinary node's own text or marker.
    Node,
    /// The currently-selected node's own text or marker.
    Selected,
    /// A lane's relation-name gutter header.
    Gutter,
}

fn canvas_lines(
    layout: &GraphLayout,
    selected: Option<&ResourceRef>,
    glyphs: &GraphGlyphs,
    theme: &Theme,
    viewport: GraphViewport,
) -> Vec<Line<'static>> {
    let width = viewport.width.max(1) as usize;
    let height = viewport.height.max(1) as usize;
    let mut grid: Vec<Vec<char>> = vec![vec![' '; width]; height];
    let mut cell_style: Vec<Vec<CellStyle>> = vec![vec![CellStyle::Node; width]; height];
    let markers = assign_markers(layout);

    let mut put = |position: GraphPoint, text: &str, style: CellStyle| {
        if position.y < 0 || position.y as usize >= height {
            return;
        }
        let row_glyphs = &mut grid[position.y as usize];
        let row_style = &mut cell_style[position.y as usize];
        for (col, ch) in (position.x..).zip(text.chars()) {
            if col >= 0 && (col as usize) < width {
                row_glyphs[col as usize] = ch;
                row_style[col as usize] = style;
            }
        }
    };

    // Gutter headers first: node text is drawn after, and reserved extents
    // never overlap by construction (see `graph_layout`'s "Extent" doc), so
    // draw order does not matter for correctness — but should a bug ever
    // put the two in the same cell, a node's own name should win over a
    // relation name, not the reverse.
    for lane in &layout.lanes {
        let text = format!(
            "{} {} {}",
            lane.band.label(),
            glyphs.band_connector(lane.band),
            lane.relation
        );
        put(lane.position, &text, CellStyle::Gutter);
    }

    for node in &layout.nodes {
        let is_selected = selected == Some(&node.resource) && !node.is_focus;
        let style = if is_selected { CellStyle::Selected } else { CellStyle::Node };
        if node.is_focus {
            put(node.position, &glyphs.focus_marker().to_string(), CellStyle::Node);
        } else if let Some(text) = node_text(node, glyphs) {
            put(node.position, &text, style);
        } else {
            let marker = markers.get(&node.resource).cloned().unwrap_or_default();
            put(node.position, &marker, style);
        }
    }

    grid.into_iter()
        .zip(cell_style)
        .map(|(row, style_row)| row_to_line(row, style_row, theme))
        .collect()
}

fn row_to_line(row: Vec<char>, style_row: Vec<CellStyle>, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let mut current_style = CellStyle::Node;
    let mut first = true;
    for (ch, style) in row.into_iter().zip(style_row) {
        if first {
            current_style = style;
            first = false;
        } else if style != current_style {
            spans.push(styled_span(std::mem::take(&mut current), current_style, theme));
            current_style = style;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        spans.push(styled_span(current, current_style, theme));
    }
    if spans.is_empty() {
        spans.push(Span::raw(""));
    }
    Line::from(spans)
}

fn styled_span(text: String, style: CellStyle, theme: &Theme) -> Span<'static> {
    let style = match style {
        CellStyle::Selected => theme.selected(),
        CellStyle::Gutter => theme.dim(),
        CellStyle::Node => theme.base(),
    };
    Span::styled(text, style)
}

/// The legend beneath the canvas is no longer a full key for every node —
/// once a node's own name is readable at its laid-out position (which the
/// wide/medium canvas gives almost every node room for), repeating it here
/// would be exactly the duplication the ladder doctrine warns against: it
/// would look like extra information and not be any. So a `(band, lane)`
/// group appears here only for what the canvas could not say on its own:
///
/// - any member that bottomed out at the letter-marker rung
///   ([`is_marker_rung`]) — the legend is that marker's only key, same as
///   before;
/// - every member of a lane whose own relation name did not fit the canvas
///   gutter at all (`lane_named_on_canvas` false) — without this, a viewer
///   would have no way to learn what relation grouped those nodes, since
///   neither the gutter nor a marker carries it in that case.
fn legend_lines(
    layout: &GraphLayout,
    selected: Option<&ResourceRef>,
    glyphs: &GraphGlyphs,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let markers = assign_markers(layout);
    let lane_named_on_canvas: BTreeSet<(RelationBand, String)> = layout
        .lanes
        .iter()
        .map(|lane| (lane.band, lane.relation.clone()))
        .collect();
    let mut by_group: BTreeMap<(RelationBand, String), Vec<&LaidOutNode>> = BTreeMap::new();
    for node in &layout.nodes {
        let Some(band) = node.band else { continue };
        let lane = node.lane.clone().unwrap_or_default();
        let key = (band, lane);
        if lane_named_on_canvas.contains(&key) && !is_marker_rung(node) {
            continue;
        }
        by_group.entry(key).or_default().push(node);
    }

    let mut lines = Vec::new();
    for band in [
        RelationBand::Incoming,
        RelationBand::Outgoing,
        RelationBand::Context,
        RelationBand::Contained,
    ] {
        let groups: Vec<_> = by_group
            .iter()
            .filter(|((b, _), _)| *b == band)
            .collect();
        if groups.is_empty() {
            continue;
        }
        lines.push(Line::from(Span::styled(
            format!("{} {}", band.label(), glyphs.band_connector(band)),
            theme.heading(),
        )));
        for ((_, lane), members) in groups {
            let lane_label = if lane.is_empty() { "(direct)" } else { lane.as_str() };
            lines.push(Line::from(Span::styled(
                format!("  {lane_label}"),
                theme.dim(),
            )));
            for member in members {
                let text = match markers.get(&member.resource) {
                    Some(marker) => format!(
                        "    {marker}  {} ({})",
                        member.label,
                        member.kind.as_str()
                    ),
                    None => format!("    {} ({})", member.label, member.kind.as_str()),
                };
                let style = if selected == Some(&member.resource) {
                    theme.selected()
                } else {
                    theme.base()
                };
                lines.push(Line::from(Span::styled(text, style)));
            }
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// Inspector
// ---------------------------------------------------------------------------

/// Everything supplied about the currently selected node/edge — nothing
/// fabricated. A field the provider did not give renders as an explicit
/// absence, never a guess.
pub fn inspector_lines(
    layout: &GraphLayout,
    selected: Option<&ResourceRef>,
    glyphs: &GraphGlyphs,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled("Inspector", theme.heading()))];
    let Some(selected) = selected else {
        lines.push(Line::from(Span::styled(
            "nothing selected",
            theme.dim(),
        )));
        return lines;
    };
    let Some(node) = layout.nodes.iter().find(|n| &n.resource == selected) else {
        lines.push(Line::from(Span::styled(
            "selected subject is not part of the current graph layout",
            theme.dim(),
        )));
        return lines;
    };

    lines.push(Line::raw(format!("subject: {}", node.resource)));
    lines.push(Line::raw(format!("kind: {}", node.kind.as_str())));
    lines.push(Line::raw(format!(
        "state: {}",
        node.state.as_deref().unwrap_or("(not supplied)")
    )));
    if node.is_focus {
        lines.push(Line::raw("role: focus of this neighbourhood"));
    } else {
        lines.push(Line::raw(format!(
            "band: {}",
            node.band.map(RelationBand::label).unwrap_or("(none)")
        )));
        lines.push(Line::raw(format!(
            "lane: {}",
            node.lane.as_deref().unwrap_or("(none)")
        )));
    }

    let laid: Vec<_> = layout
        .edges
        .iter()
        .filter(|edge| &edge.from == selected || &edge.to == selected)
        .collect();
    if laid.is_empty() {
        lines.push(Line::raw("relations: (none in this layout)"));
    } else {
        lines.push(Line::raw("relations:"));
        for edge in laid {
            let direction = match edge.direction {
                RelationDirection::Outgoing => "outgoing",
                RelationDirection::Incoming => "incoming",
                RelationDirection::Bidirectional => "bidirectional",
            };
            lines.push(Line::raw(format!(
                "  {} {} {} ({} -> {})",
                edge.relation,
                glyphs.shell().separator(),
                direction,
                edge.from,
                edge.to
            )));
            lines.push(Line::raw(format!(
                "    authority: {:?}  route: {}",
                edge.origin.authority,
                route_kind(edge),
            )));
            lines.push(Line::raw(format!(
                "    provider: {}  lens: {}  revision: {}",
                edge.origin
                    .provider
                    .as_ref()
                    .map(|p| p.as_str().to_string())
                    .unwrap_or_else(|| "(not supplied)".into()),
                edge.origin.lens.as_deref().unwrap_or("(not supplied)"),
                edge.origin.revision.as_deref().unwrap_or("(not supplied)"),
            )));
        }
    }
    lines
}

/// `canonical` when the edge carries a real owning provider (an assertion
/// this codebase's own providers can be held to); `learned/derived` when
/// authority says the edge was inferred rather than owned (`Derived`,
/// `Learned`, `Generated`) or no provider was supplied at all. This is read
/// directly off the supplied `origin`, never guessed from the relation name.
fn route_kind(edge: &crate::graph_layout::LaidOutEdge) -> &'static str {
    use aikit_core::SourceAuthority;
    match (edge.origin.authority, edge.origin.provider.is_some()) {
        (SourceAuthority::Authored | SourceAuthority::Observed, true) => "canonical",
        _ => "learned/derived",
    }
}

// ---------------------------------------------------------------------------
// Narrow fallback
// ---------------------------------------------------------------------------

/// Spec §8.3 "Narrow fallback": the same semantic Graph state, projected as
/// band-grouped `relation -> target` lines rather than a spatial canvas.
/// Built from [`crate::graph_layout::grouped_projection`] so narrow and
/// spatial rendering can never disagree about which nodes/edges survived the
/// viewport budget.
pub fn grouped_lines(layout: &GraphLayout, glyphs: &GraphGlyphs, theme: &Theme) -> Vec<Line<'static>> {
    let focus_label = layout
        .nodes
        .iter()
        .find(|n| n.is_focus)
        .map(|n| n.label.clone())
        .unwrap_or_else(|| layout.focus.to_string());
    let mut lines = vec![Line::from(Span::styled(focus_label, theme.heading()))];

    let grouped = crate::graph_layout::grouped_projection(layout);
    if grouped.is_empty() {
        lines.push(Line::from(Span::styled(
            "no typed resource relations",
            theme.dim(),
        )));
    } else {
        let mut by_band: BTreeMap<RelationBand, Vec<_>> = BTreeMap::new();
        for line in &grouped {
            by_band.entry(line.band).or_default().push(line);
        }
        for band in [
            RelationBand::Incoming,
            RelationBand::Outgoing,
            RelationBand::Context,
            RelationBand::Contained,
        ] {
            let Some(members) = by_band.get(&band) else { continue };
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                format!("{} {}", band.label(), glyphs.band_connector(band)),
                theme.heading(),
            )));
            for member in members {
                lines.push(Line::raw(format!(
                    "  {} {} {}",
                    member.relation,
                    glyphs.direction_glyph(member.direction),
                    member.target_label
                )));
            }
        }
    }
    // Checked unconditionally, not folded into the `grouped.is_empty()`
    // branch above: a degraded/partial neighbourhood can be truncated with
    // *zero* edges surviving into this view (e.g. the provider budget was
    // exhausted before any relation could be retained) — "no typed resource
    // relations" would otherwise read as a complete, empty neighbourhood
    // rather than the truthful "we don't know, more may exist" this spec
    // requires (see `spatial_lines`, which already surfaces this
    // unconditionally; the narrow fallback must not disagree).
    if layout.truncated {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!(
                "truncated: {} node(s), {} edge(s) not shown",
                layout.dropped.nodes_dropped, layout.dropped.edges_dropped
            ),
            theme.unavailable(),
        )));
    }
    lines
}
