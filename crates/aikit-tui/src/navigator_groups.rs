//! Grouped result presentation for the Ctrl+K Universal Navigator.
//!
//! `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §3.3 shows Navigator hits
//! grouped under three labels, blank-line separated:
//!
//! ```text
//! DESTINATIONS
//!   Compose / Praxis / Skills
//!
//! RESOURCES
//!   verification           Method
//!
//! RECENT ROUTES
//!   O:I → developer → verification
//! ```
//!
//! This module is presentation only. It partitions the one ordered result
//! list [`crate::application_service::ApplicationService::resolve_search`]
//! already returned (`TuiState::read_model`) into these three labels; it
//! never re-ranks, re-fetches, drops or duplicates a hit, and it never
//! decides a hit's group by matching its label/kind name as a string — see
//! [`group_for`]. `graph_layout.rs`'s prior defect (deciding graph structure
//! from a label match) is exactly the class of bug this module must not
//! repeat.
//!
//! Recent routes: the spec's own example renders a full breadcrumb
//! (`O:I → developer → verification`) reconstructed from Agent/SkillSet/
//! Method containment. `TuiState::navigation` (`NavigationPoint`) carries no
//! such containment chain — only the resource that was selected before the
//! viewer navigated onward (see `reduce_tui`'s `ActionFinished`/
//! `OpenSelection` arms in `application.rs`). Synthesising a containment
//! breadcrumb from data this presentation layer does not own would be the
//! same kind of invention constraint 3 forbids, so a recent route is
//! rendered exactly like any other resource row — same label, same kind
//! badge, same summary — merely filed under its own heading. This is a
//! disclosed divergence from the spec's illustrative formatting, not from
//! its grouping law.
//!
//! Grouping is scoped to the Navigator (`PresentationMode::Quick`) itself —
//! see [`resource_pane_rows`]. Workspace's own per-section resource list
//! keeps its prior flat presentation; §3.3 is explicitly about Ctrl+K
//! Universal Search, not about the Workspace section browser.

use crate::application::{NavigationPoint, PresentationMode, ResourceListItem, TuiState};
use aikit_core::resource::ResourceKind;

/// The three §3.3 groups, in the fixed order the spec's own example lists
/// them. `Ord` follows that same order so a group can be used as a
/// deterministic sort/iteration key without ever needing a `HashMap` or a
/// name comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NavigatorGroup {
    Destinations,
    Resources,
    RecentRoutes,
}

impl NavigatorGroup {
    /// Presentation order, matching §3.3's own example exactly.
    pub const ALL: [NavigatorGroup; 3] = [Self::Destinations, Self::Resources, Self::RecentRoutes];

    /// The header text §3.3 uses verbatim. Plain ASCII words — a Navigator
    /// group header carries no glyph of its own to swap between character
    /// sets.
    pub fn label(self) -> &'static str {
        match self {
            Self::Destinations => "DESTINATIONS",
            Self::Resources => "RESOURCES",
            Self::RecentRoutes => "RECENT ROUTES",
        }
    }
}

/// Which group a hit belongs to, read off what it structurally *is* —
/// never off a name/label match:
///
/// - a navigation Surface (`ResourceKind::Surface`, installed by
///   `crate::workspace_navigation` and any other Surface-publishing owner)
///   is always a destination;
/// - anything else the viewer has actually navigated through before (its
///   `ResourceRef` appears in `TuiState::navigation`) is a recent route;
/// - everything else is an ordinary resource.
///
/// Kind is checked before navigation history so a destination that also
/// happens to be a recent route (the viewer opened `Worlds` a moment ago)
/// still reads as a destination — the more structural fact wins.
pub fn group_for(item: &ResourceListItem, navigation: &[NavigationPoint]) -> NavigatorGroup {
    if item.kind == ResourceKind::Surface {
        NavigatorGroup::Destinations
    } else if navigation
        .iter()
        .any(|point| point.selected.as_ref() == Some(&item.resource))
    {
        NavigatorGroup::RecentRoutes
    } else {
        NavigatorGroup::Resources
    }
}

/// One flattened line of the resource pane: a group header or the blank
/// separator between two groups (both never selectable, never
/// hit-testable), or a real resource row carrying the index into
/// `TuiState::read_model.resources` that `Select`/`SelectNext`/
/// `SelectPrevious`/mouse hit-testing already address the model by. No row
/// here ever invents a resource identity of its own.
///
/// `Spacer` is its own variant, distinct from folding a blank line into
/// `Header`, so that one `NavigatorRow` always corresponds to exactly one
/// rendered terminal line — the invariant [`visible_window`] and mouse
/// hit-testing depend on to stay in agreement about which screen row a
/// given resource landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigatorRow<'a> {
    Header(NavigatorGroup),
    Spacer,
    Item {
        index: usize,
        item: &'a ResourceListItem,
    },
}

/// Build the deterministic DESTINATIONS/RESOURCES/RECENT ROUTES
/// presentation of `state.read_model.resources`.
///
/// Iteration is a fixed three-element array (`NavigatorGroup::ALL`), and
/// membership inside each group preserves `read_model.resources`' own
/// already-deterministic order (that order in turn comes from
/// `ResolvePath::candidates` over a `BTreeMap`-backed index — see
/// `aikit_core::resource::search`). No `HashMap`, no re-sort, no
/// re-ranking: this function only ever partitions.
///
/// A group with no members contributes no header at all — never an empty
/// `DESTINATIONS` / `RESOURCES` / `RECENT ROUTES` line — mirroring the
/// exact idiom `graph_presentation::grouped_lines` already established for
/// Knowledge Graph's own narrow/grouped fallback (`continue` past an absent
/// band rather than printing its label with nothing under it). A blank
/// `Spacer` line separates two present groups but never opens or closes the
/// list.
pub fn navigator_rows(state: &TuiState) -> Vec<NavigatorRow<'_>> {
    let mut rows = Vec::new();
    for group in NavigatorGroup::ALL {
        let mut members = state
            .read_model
            .resources
            .iter()
            .enumerate()
            .filter(|(_, item)| group_for(item, &state.navigation) == group)
            .peekable();
        if members.peek().is_none() {
            continue;
        }
        if !rows.is_empty() {
            rows.push(NavigatorRow::Spacer);
        }
        rows.push(NavigatorRow::Header(group));
        rows.extend(members.map(|(index, item)| NavigatorRow::Item { index, item }));
    }
    rows
}

/// The resource-pane row plan for the current presentation.
///
/// Grouping (§3.3) is a Navigator concept: only `PresentationMode::Quick`
/// gets `navigator_rows`' headers/spacers. Workspace's per-section browser
/// keeps the prior flat one-row-per-resource presentation — every resource
/// still appears, in the same order, just with no `Header`/`Spacer` rows
/// interleaved.
pub fn resource_pane_rows(state: &TuiState) -> Vec<NavigatorRow<'_>> {
    if state.presentation == PresentationMode::Quick {
        navigator_rows(state)
    } else {
        state
            .read_model
            .resources
            .iter()
            .enumerate()
            .map(|(index, item)| NavigatorRow::Item { index, item })
            .collect()
    }
}

/// The position of the resource at `read_model.resources[resource_index]`
/// within `rows`'s flattened order, if it survived grouping. Shared by the
/// renderer (to compute the scroll window) and mouse hit-testing (to map a
/// clicked screen row back to the same row plan), so the two can never
/// diverge about which line a given resource landed on.
pub fn row_position(rows: &[NavigatorRow<'_>], resource_index: usize) -> Option<usize> {
    rows.iter()
        .position(|row| matches!(row, NavigatorRow::Item { index, .. } if *index == resource_index))
}

/// The contiguous slice of `rows` visible in a pane `height` lines tall,
/// scrolled so the row at `selected_row` (when given) stays on screen —
/// generalising the flat resource list's prior "keep the selection in the
/// last visible line" policy to a row plan that may also carry headers and
/// spacers. Returns the scroll offset (`first`) alongside the slice so a
/// caller mapping a clicked screen line back to a row can undo the same
/// offset instead of recomputing it by hand — the one shared computation
/// the renderer and mouse hit-testing both call, so they cannot silently
/// diverge about which line is on screen.
pub fn visible_window<'r, 'a>(
    rows: &'r [NavigatorRow<'a>],
    selected_row: Option<usize>,
    height: usize,
) -> (usize, &'r [NavigatorRow<'a>]) {
    if height == 0 || rows.is_empty() {
        return (0, &[]);
    }
    let selected_row = selected_row.unwrap_or(0).min(rows.len() - 1);
    let first = selected_row.saturating_sub(height.saturating_sub(1));
    let end = (first + height).min(rows.len());
    (first, &rows[first..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{RelationView, ResourceListReadModel, WorkspaceSection};
    use aikit_core::resource::ResourceRef;

    fn item(id: &str, kind: ResourceKind) -> ResourceListItem {
        ResourceListItem {
            resource: ResourceRef::parse(id).unwrap(),
            kind,
            label: id.to_string(),
            summary: format!("summary for {id}"),
        }
    }

    fn point(resource: &str) -> NavigationPoint {
        NavigationPoint {
            selected: Some(ResourceRef::parse(resource).unwrap()),
            relation_view: RelationView::List,
            workspace_section: WorkspaceSection::Worlds,
        }
    }

    fn state_with(resources: Vec<ResourceListItem>, navigation: Vec<NavigationPoint>) -> TuiState {
        TuiState {
            presentation: PresentationMode::Quick,
            read_model: ResourceListReadModel {
                revision: "rev".into(),
                resources,
            },
            navigation,
            ..TuiState::default()
        }
    }

    fn row_label(row: &NavigatorRow<'_>) -> String {
        match row {
            NavigatorRow::Header(group) => group.label().to_string(),
            NavigatorRow::Spacer => String::new(),
            NavigatorRow::Item { item, .. } => item.label.clone(),
        }
    }

    #[test]
    fn surface_kind_is_always_a_destination_even_when_also_recently_visited() {
        let surface = item("surface/workspace/worlds", ResourceKind::Surface);
        let state = state_with(vec![surface], vec![point("surface/workspace/worlds")]);
        assert_eq!(
            group_for(&state.read_model.resources[0], &state.navigation),
            NavigatorGroup::Destinations
        );
    }

    #[test]
    fn navigation_history_membership_promotes_a_plain_resource_to_recent_routes() {
        let capability = item("capability/verify", ResourceKind::Capability);
        let state = state_with(vec![capability], vec![point("capability/verify")]);
        assert_eq!(
            group_for(&state.read_model.resources[0], &state.navigation),
            NavigatorGroup::RecentRoutes
        );
    }

    #[test]
    fn an_unvisited_ordinary_resource_is_just_a_resource() {
        let capability = item("capability/verify", ResourceKind::Capability);
        let state = state_with(vec![capability], vec![]);
        assert_eq!(
            group_for(&state.read_model.resources[0], &state.navigation),
            NavigatorGroup::Resources
        );
    }

    #[test]
    fn grouping_never_reorders_within_a_group_and_skips_empty_groups() {
        let a = item("capability/alpha", ResourceKind::Capability);
        let b = item("capability/beta", ResourceKind::Capability);
        let surface = item("surface/workspace/worlds", ResourceKind::Surface);
        // Deliberately out of any "natural" order: surface last, resources
        // in a specific order, to prove grouping partitions in place rather
        // than re-sorting by anything of its own.
        let state = state_with(vec![b, a, surface], vec![]);
        let rows = navigator_rows(&state);

        // No RECENT ROUTES header: nothing in `state.navigation`.
        assert!(!rows
            .iter()
            .any(|row| matches!(row, NavigatorRow::Header(NavigatorGroup::RecentRoutes))));

        let labels: Vec<String> = rows.iter().map(row_label).collect();
        assert_eq!(
            labels,
            vec![
                "DESTINATIONS",
                "surface/workspace/worlds",
                "", // spacer between DESTINATIONS and RESOURCES
                "RESOURCES",
                "capability/beta",
                "capability/alpha",
            ]
        );
    }

    #[test]
    fn all_three_groups_present_render_in_spec_order_with_spacers_between_only() {
        let surface = item("surface/workspace/worlds", ResourceKind::Surface);
        let resource = item("capability/verify", ResourceKind::Capability);
        let visited = item("capability/visited", ResourceKind::Capability);
        let state = state_with(
            vec![surface, resource, visited],
            vec![point("capability/visited")],
        );
        let rows = navigator_rows(&state);

        assert!(!matches!(rows.first(), Some(NavigatorRow::Spacer)));
        assert!(!matches!(rows.last(), Some(NavigatorRow::Spacer)));

        let headers: Vec<NavigatorGroup> = rows
            .iter()
            .filter_map(|row| match row {
                NavigatorRow::Header(group) => Some(*group),
                NavigatorRow::Spacer | NavigatorRow::Item { .. } => None,
            })
            .collect();
        assert_eq!(
            headers,
            vec![
                NavigatorGroup::Destinations,
                NavigatorGroup::Resources,
                NavigatorGroup::RecentRoutes,
            ]
        );
        let spacers = rows
            .iter()
            .filter(|row| matches!(row, NavigatorRow::Spacer))
            .count();
        assert_eq!(
            spacers, 2,
            "one spacer between each of the three present groups"
        );
    }

    #[test]
    fn row_position_locates_a_resource_after_headers_and_spacers_are_inserted() {
        let surface = item("surface/workspace/worlds", ResourceKind::Surface);
        let resource = item("capability/verify", ResourceKind::Capability);
        let state = state_with(vec![surface, resource], vec![]);
        let rows = navigator_rows(&state);
        // rows: [Header(Destinations), Item(0), Spacer, Header(Resources), Item(1)]
        assert_eq!(row_position(&rows, 0), Some(1));
        assert_eq!(row_position(&rows, 1), Some(4));
    }

    #[test]
    fn determinism_same_input_produces_the_same_order_every_time() {
        let surface = item("surface/workspace/worlds", ResourceKind::Surface);
        let a = item("capability/alpha", ResourceKind::Capability);
        let b = item("capability/beta", ResourceKind::Capability);
        let visited = item("capability/visited", ResourceKind::Capability);
        let state = state_with(
            vec![surface, a, b, visited],
            vec![point("capability/visited")],
        );

        let first = navigator_rows(&state);
        let second = navigator_rows(&state);
        let render = |rows: &[NavigatorRow<'_>]| rows.iter().map(row_label).collect::<Vec<_>>();
        assert_eq!(render(&first), render(&second));
    }

    #[test]
    fn workspace_presentation_keeps_the_prior_flat_row_plan() {
        let surface = item("surface/workspace/worlds", ResourceKind::Surface);
        let resource = item("capability/verify", ResourceKind::Capability);
        let mut state = state_with(vec![surface, resource], vec![]);
        state.presentation = PresentationMode::Workspace;

        let rows = resource_pane_rows(&state);
        assert_eq!(
            rows.len(),
            2,
            "no header/spacer rows in Workspace's flat list"
        );
        assert!(rows
            .iter()
            .all(|row| matches!(row, NavigatorRow::Item { .. })));
    }

    #[test]
    fn visible_window_keeps_the_selected_row_on_screen_and_hit_testing_can_undo_the_offset() {
        // Five plain resources -> five rows (Workspace-style flat plan, but
        // the windowing function itself is presentation-agnostic).
        let items: Vec<ResourceListItem> = (0..5)
            .map(|index| item(&format!("capability/r{index}"), ResourceKind::Capability))
            .collect();
        let state = state_with(items, vec![]);
        let rows: Vec<NavigatorRow<'_>> = state
            .read_model
            .resources
            .iter()
            .enumerate()
            .map(|(index, item)| NavigatorRow::Item { index, item })
            .collect();

        // A 2-line pane with the last row selected must scroll so that row
        // is the final visible line, exactly as the pre-grouping list did.
        let (first, visible) = visible_window(&rows, Some(4), 2);
        assert_eq!(first, 3);
        assert_eq!(visible.len(), 2);
        assert!(matches!(visible[1], NavigatorRow::Item { index: 4, .. }));

        // A screen row's hit-test undoes the same offset the renderer used.
        let clicked_screen_row = 1usize; // second visible line
        let clicked_row = first + clicked_screen_row;
        assert_eq!(clicked_row, 4);
    }
}
