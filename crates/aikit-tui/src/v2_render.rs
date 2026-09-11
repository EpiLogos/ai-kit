//! ResourceRef-native renderer for the resting V2 human shell.
//!
//! Quick and Workspace are presentations of [`TuiState`], not alternate semantic
//! controllers. This renderer therefore knows only the application read model,
//! stable selection, contextual Actions, Workspace section, staging and overlays.

use ratatui::layout::Alignment;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use aikit_core::resource::ActionStageability;

use crate::application::{
    visible_contextual_actions, ActionOutcome, Overlay, PresentationMode, ResourceListItem,
    TuiState, WorkspaceSection,
};
use crate::layout::{Glyphs, Layout};
use crate::navigation::AmbientContext;
use crate::navigator_groups::{self, NavigatorRow};
use crate::compose_preview::compose_preview_lines;
use crate::project_workspace_render::{
    explain_lines, project_world_lines, workspace_section_label, WorkspaceReading,
};
use crate::theme::Theme;

/// Render the resting shell with an already-resolved host glyph capability.
///
/// `glyphs` is passed in rather than read from the environment here: the
/// capability is host state, resolved once at
/// [`crate::application_surface::ApplicationSurfaceController::new`] and
/// carried as data, so a drawn frame is a pure function of the semantic
/// state and that capability. `layout.rs`'s module header says why every
/// mark this renderer draws has to come out of one chosen set.
pub fn draw(frame: &mut Frame, state: &TuiState, glyphs: Glyphs) {
    draw_with_context(frame, state, &AmbientContext::default(), glyphs);
}

pub fn draw_with_context(
    frame: &mut Frame,
    state: &TuiState,
    ambient: &AmbientContext,
    glyphs: Glyphs,
) {
    draw_shell(frame, state, ambient, None, glyphs);
}

/// Render the live Workspace against the shared Project-world read model.
///
/// The world is presentation input only. Selection/staging remain in `TuiState`,
/// and this function has no access to retrieval, resolver or mutation services.
pub fn draw_with_project_world(
    frame: &mut Frame,
    state: &TuiState,
    ambient: &AmbientContext,
    reading: WorkspaceReading<'_>,
    glyphs: Glyphs,
) {
    draw_shell(frame, state, ambient, Some(reading), glyphs);
}

fn draw_shell(
    frame: &mut Frame,
    state: &TuiState,
    ambient: &AmbientContext,
    reading: Option<WorkspaceReading<'_>>,
    glyphs: Glyphs,
) {
    let theme = Theme::new();
    let area = frame.area();
    let sep = glyphs.separator();
    let mode = match state.presentation {
        PresentationMode::Quick => "Quick",
        PresentationMode::Workspace => "Workspace",
    };
    let base_title = format!("AIKit {sep} {mode}");
    let ambient_line = ambient.line(area.width.saturating_sub(20), glyphs);
    let title = if ambient_line.is_empty() {
        format!(" {base_title} ")
    } else {
        format!(" {base_title} {sep} {ambient_line} ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(glyphs.border_set())
        .border_style(theme.border())
        .title(title)
        .title_alignment(Alignment::Left);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let layout = Layout::for_width(inner.width);
    let panes = layout.split(inner);
    frame.render_widget(query_line(state, &theme, glyphs), panes.query);

    // `ApplicationSurfaceController::draw` (`application_surface.rs`) draws
    // the Relations panel directly on top of `panes.list` — at this exact
    // Rect, later in the same frame — whenever the Workspace is showing the
    // Knowledge section, regardless of `relation_view`. `Paragraph`/`Block`
    // only touch the cells their own content actually reaches (see
    // `ratatui::widgets::Clear`'s doc comment: "this will clear/reset the
    // area first" is something a caller has to ask for, not something
    // rendering does on its own), so whatever this function drew into that
    // Rect a moment ago — or what an earlier frame left there, since
    // `Terminal::draw`'s contract only promises a diff against the
    // previous frame, not a blanked buffer — stays behind as far as the
    // panel's own content is shorter than the row it sits over. Rather
    // than let the list content it will never let the viewer see reach the
    // buffer at all, this leaves `panes.list` genuinely blank for the
    // Relations panel to draw onto, the same way a popup clears before it
    // draws (`Clear`'s own example).
    let relations_panel_covers_list = state.presentation == PresentationMode::Workspace
        && state.workspace_section == WorkspaceSection::Knowledge;

    let compact_world_lines = if !relations_panel_covers_list
        && panes.preview.is_none()
        && state.presentation == PresentationMode::Workspace
    {
        reading
            .map(|reading| project_world_lines(state, reading, glyphs))
            .filter(|lines| !lines.is_empty())
    } else {
        None
    };
    if relations_panel_covers_list {
        frame.render_widget(Clear, panes.list);
    } else if let Some(lines) = compact_world_lines {
        frame.render_widget(project_world_pane(lines, &theme), panes.list);
    } else {
        draw_resources(frame, state, &theme, panes.list, glyphs);
    }

    if let Some(preview) = panes.preview {
        frame.render_widget(preview_pane(state, &theme, reading, glyphs), preview);
    }
    frame.render_widget(footer(state, &theme, glyphs), panes.footer);
}

fn query_line<'a>(state: &'a TuiState, theme: &Theme, glyphs: Glyphs) -> Paragraph<'a> {
    let mut spans = if let Some(action_query) = state.action_query.as_ref() {
        vec![
            Span::styled(": ", theme.accent()),
            if action_query.is_empty() {
                Span::styled("search actions for selection", theme.dim())
            } else {
                Span::styled(action_query.clone(), theme.base())
            },
        ]
    } else {
        vec![
            Span::styled("/ ", theme.accent()),
            if state.query.is_empty() {
                Span::styled("Search resources and actions", theme.dim())
            } else {
                Span::styled(state.query.clone(), theme.base())
            },
        ]
    };
    if state.presentation == PresentationMode::Workspace {
        spans.push(Span::raw("   "));
        spans.push(Span::styled("Search", theme.accent()));
        for section in WorkspaceSection::ALL.iter() {
            spans.push(Span::styled(format!(" {} ", glyphs.separator()), theme.dim()));
            spans.push(Span::styled(
                workspace_section_label(*section),
                if *section == state.workspace_section {
                    theme.selected()
                } else {
                    theme.dim()
                },
            ));
        }
    }
    Paragraph::new(Line::from(spans))
}

/// Render the resource pane: DESTINATIONS/RESOURCES/RECENT ROUTES headers in
/// Quick (Navigator) presentation (`docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md`
/// §3.3), the prior flat one-row-per-resource list everywhere else. Both
/// shapes share [`navigator_groups::resource_pane_rows`] and
/// [`navigator_groups::visible_window`] with `handle_mouse`'s hit-testing in
/// `application_surface.rs`, so a screen row a viewer clicks and a screen
/// row this function draws can never disagree about which resource (or
/// which non-selectable header/spacer) it is.
fn draw_resources(
    frame: &mut Frame,
    state: &TuiState,
    theme: &Theme,
    area: ratatui::layout::Rect,
    glyphs: Glyphs,
) {
    if state.read_model.resources.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                if state.query.is_empty() {
                    "no current, pinned, recent or familiar destinations"
                } else {
                    "nothing matches"
                },
                theme.dim(),
            ))),
            area,
        );
        return;
    }

    let rows = navigator_groups::resource_pane_rows(state);
    let grouped = state.presentation == PresentationMode::Quick;
    // Preserves the pre-grouping default: with nothing explicitly selected,
    // the first resource reads as selected. `row_position` re-expresses that
    // resource index in the (possibly header-bearing) row plan so the
    // highlight can never land on a `Header`/`Spacer` line.
    let selected_resource_index = state
        .selected
        .as_ref()
        .and_then(|selected| state.read_model.position(selected))
        .unwrap_or(0);
    let selected_row = navigator_groups::row_position(&rows, selected_resource_index);

    let height = area.height as usize;
    let (first, visible) = navigator_groups::visible_window(&rows, selected_row, height);
    let lines = visible
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let is_selected = selected_row == Some(first + offset);
            pane_row_line(state, theme, glyphs, row, is_selected, grouped, area.width)
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn pane_row_line<'a>(
    state: &TuiState,
    theme: &Theme,
    glyphs: Glyphs,
    row: &NavigatorRow<'a>,
    selected: bool,
    grouped: bool,
    width: u16,
) -> Line<'a> {
    match row {
        NavigatorRow::Header(group) => {
            Line::from(Span::styled(group.label(), theme.heading()))
        }
        NavigatorRow::Spacer => Line::raw(""),
        NavigatorRow::Item { item, .. } => {
            let indent = if grouped { 2 } else { 0 };
            resource_line(state, theme, glyphs, item, selected, indent, width)
        }
    }
}

fn resource_line<'a>(
    state: &TuiState,
    theme: &Theme,
    glyphs: Glyphs,
    item: &'a ResourceListItem,
    selected: bool,
    indent: usize,
    width: u16,
) -> Line<'a> {
    let staged = state.staged.get(&item.resource).is_some();
    let cursor = if selected { glyphs.list_cursor() } else { " " };
    let staged_mark = if staged { glyphs.staged() } else { ' ' };
    let kind = format!("[{}]", item.kind.as_str());
    let fixed = indent + 5 + kind.chars().count();
    let available = (width as usize).saturating_sub(fixed);
    let label_width = available.min(28);
    let summary_width = available.saturating_sub(label_width + 1);

    let mut spans = vec![Span::raw(" ".repeat(indent))];
    spans.push(Span::styled(
        format!("{cursor}{staged_mark} "),
        if staged { theme.staged() } else { theme.accent() },
    ));
    spans.push(Span::styled(
        format!("{} ", pad(&kind, 20.min(kind.chars().count().max(8)), glyphs)),
        theme.dim(),
    ));
    spans.push(Span::styled(
        pad(&item.label, label_width, glyphs),
        if selected { theme.selected() } else { theme.base() },
    ));
    if summary_width > 3 {
        spans.push(Span::styled(
            format!(
                " {}",
                truncate(&item.summary, summary_width.saturating_sub(1), glyphs)
            ),
            theme.dim(),
        ));
    }
    Line::from(spans)
}

fn preview_pane<'a>(
    state: &'a TuiState,
    theme: &Theme,
    reading: Option<WorkspaceReading<'a>>,
    glyphs: Glyphs,
) -> Paragraph<'a> {
    let world = reading.map(|reading| reading.world);
    let sep = glyphs.separator();
    if state.overlay == Some(Overlay::ModelRoster) {
        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled("Model roster", theme.heading())),
            Line::from(""),
        ];
        match state.model_roster.as_ref() {
            Some(roster) => lines.extend(
                crate::model_roster_matrix(roster, glyphs)
                    .into_iter()
                    .map(|line| Line::from(Span::raw(line))),
            ),
            None => lines.push(Line::from(Span::raw(
                "no Model roster is available here (no Project bound)".to_string(),
            ))),
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Esc returns", theme.staged())));
        return Paragraph::new(lines).wrap(Wrap { trim: false });
    }
    if state.overlay == Some(Overlay::ConfirmApply) {
        let summary = state
            .preview
            .as_ref()
            .map(|preview| preview.summary.as_str())
            .unwrap_or("preview unavailable");
        return Paragraph::new(vec![
            Line::from(Span::styled("Confirm composition", theme.heading())),
            Line::from(""),
            Line::from(Span::raw(summary.to_string())),
            Line::from(""),
            Line::from(Span::styled(
                format!("Ctrl+S applies {sep} Esc returns"),
                theme.staged(),
            )),
        ])
        .wrap(Wrap { trim: false });
    }
    if state.overlay == Some(Overlay::CompositionPreview) {
        let summary = state
            .preview
            .as_ref()
            .map(|preview| preview.summary.as_str())
            .unwrap_or("preview unavailable");
        let mut lines = vec![
            Line::from(Span::styled("Composition preview", theme.heading())),
            Line::from(""),
            Line::from(Span::raw(summary.to_string())),
        ];
        // The package-toggle summary above answers "what does applying this
        // change"; spec §5.1's Preview answers "what did this actually resolve
        // to" about the whole composed World. Both belong here — folded in the
        // same way `Overlay::Explain` folds in `explain_lines`, so Preview
        // stays one route rather than becoming a second destination.
        if let Some(world) = world {
            let world_lines = compose_preview_lines(state, world, glyphs);
            if !world_lines.is_empty() {
                lines.push(Line::from(""));
                lines.extend(
                    world_lines
                        .into_iter()
                        .map(|line| Line::from(Span::raw(line))),
                );
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("Ctrl+S proceeds to confirmation {sep} Esc returns"),
            theme.dim(),
        )));
        return Paragraph::new(lines).wrap(Wrap { trim: false });
    }
    if state.overlay == Some(Overlay::Explain) {
        if let Some(ActionOutcome::Explained { subject, summary }) = state.action_result.as_ref() {
            let mut lines = vec![
                Line::from(Span::styled(
                    format!("Explain {sep} {subject}"),
                    theme.heading(),
                )),
                Line::from(""),
                Line::from(Span::raw(summary.clone())),
            ];
            // Retiring the Projection Workspace tab (spec §17: "Explain is not
            // a top-level destination in the final IA") must not silently drop
            // its authored-intent/effective-state content — it is folded in
            // here, alongside the provider's own Explain evidence above,
            // whenever a Project world is available to render it from.
            if let Some(world) = world {
                let world_lines = explain_lines(state, world, glyphs);
                if !world_lines.is_empty() {
                    lines.push(Line::from(""));
                    lines.extend(
                        world_lines
                            .into_iter()
                            .map(|line| Line::from(Span::raw(line))),
                    );
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled("Esc returns", theme.dim())));
            return Paragraph::new(lines).wrap(Wrap { trim: false });
        }
    }

    if state.presentation == PresentationMode::Workspace {
        if let Some(reading) = reading {
            let lines = project_world_lines(state, reading, glyphs);
            if !lines.is_empty() {
                return project_world_pane(lines, theme);
            }
        }
    }

    let Some(item) = selected_item(state) else {
        return Paragraph::new(Line::from(Span::styled("nothing selected", theme.dim())));
    };
    let mut lines = vec![
        Line::from(Span::styled(item.label.clone(), theme.heading())),
        Line::from(Span::styled(item.kind.as_str(), theme.accent())),
        Line::from(""),
        Line::from(Span::raw(item.summary.clone())),
        Line::from(""),
        Line::from(Span::styled(item.resource.as_str().to_string(), theme.dim())),
    ];
    if state.contextual_actions_for.as_ref() == Some(&item.resource)
        && !state.contextual_actions.is_empty()
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            if state.action_query.is_some() {
                format!("Actions {sep} text mode")
            } else {
                format!("Actions {sep} press :")
            },
            theme.heading(),
        )));
        let actions = if state.action_query.is_some() {
            visible_contextual_actions(state)
        } else {
            state.contextual_actions.clone()
        };
        for (index, action) in actions.iter().enumerate() {
            let stage_marker = match action.stageability {
                ActionStageability::Stageable => "*",
                ActionStageability::NotStageable => glyphs.list_cursor(),
            };
            let cursor = if state.action_query.is_some() && index == state.action_cursor {
                glyphs.action_cursor()
            } else {
                " "
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{cursor}{stage_marker} "), theme.accent()),
                Span::styled(
                    action.label.clone(),
                    if state.action_query.is_some() && index == state.action_cursor {
                        theme.selected()
                    } else {
                        theme.base()
                    },
                ),
                Span::styled(format!(" {sep} {}", action.description), theme.dim()),
            ]));
        }
        if actions.is_empty() && state.action_query.is_some() {
            lines.push(Line::from(Span::styled(
                "no matching contextual actions",
                theme.dim(),
            )));
        }
    }
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn project_world_pane(lines: Vec<String>, theme: &Theme) -> Paragraph<'static> {
    let lines = lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                Line::from(Span::styled(line, theme.heading()))
            } else if line.starts_with("Intent") {
                Line::from(Span::styled(line, theme.accent()))
            } else if line.starts_with("Effective") {
                Line::from(Span::styled(line, theme.base()))
            } else if line.starts_with("Boundary") {
                Line::from(Span::styled(line, theme.dim()))
            } else {
                Line::from(Span::raw(line))
            }
        })
        .collect::<Vec<_>>();
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn footer<'a>(state: &'a TuiState, theme: &Theme, glyphs: Glyphs) -> Paragraph<'a> {
    if let Some(status) = &state.status {
        return Paragraph::new(Line::from(Span::styled(status.message.clone(), theme.dim())));
    }
    let scope = state
        .mutation_scope
        .map(|scope| scope.as_str())
        .unwrap_or("unresolved");
    let sep = glyphs.separator();
    let updown = glyphs.vertical_keys();
    let text = if state.action_query.is_some() {
        format!(
            "Action mode {sep} type to filter {sep} {updown} choose {sep} Enter invoke {sep} Space invoke if stageable {sep} Esc return"
        )
    } else if state.presentation == PresentationMode::Workspace {
        format!(
            "{} {sep} {} result{} {sep} {} staged {sep} scope {} {sep} Alt+{} fields {sep} : actions {sep} Ctrl+W Quick",
            workspace_section_label(state.workspace_section),
            state.read_model.resources.len(),
            if state.read_model.resources.len() == 1 { "" } else { "s" },
            state.staged.len(),
            scope,
            glyphs.horizontal_keys(),
        )
    } else {
        format!(
            "{} result{} {sep} {} staged {sep} scope {} {sep} {updown} navigate {sep} : actions {sep} Space stage {sep} Ctrl+S preview/apply {sep} Ctrl+W Workspace",
            state.read_model.resources.len(),
            if state.read_model.resources.len() == 1 { "" } else { "s" },
            state.staged.len(),
            scope,
        )
    };
    Paragraph::new(Line::from(Span::styled(text, theme.dim())))
}

fn selected_item(state: &TuiState) -> Option<&ResourceListItem> {
    let selected = state.selected.as_ref()?;
    state
        .read_model
        .resources
        .iter()
        .find(|item| &item.resource == selected)
}

fn pad(text: &str, width: usize, glyphs: Glyphs) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = truncate(text, width, glyphs);
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

/// Clip `text` to `width` cells, marking that something was dropped.
///
/// The elision mark's own width is taken from the glyph set rather than
/// assumed to be one cell: `Glyphs::ascii`'s mark is `...`, three cells
/// against Unicode's one, and reserving a single cell for it would draw
/// two cells past the column every ASCII row.
fn truncate(text: &str, width: usize, glyphs: Glyphs) -> String {
    if width == 0 {
        return String::new();
    }
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mark = glyphs.ellipsis();
    let mark_width = mark.chars().count();
    // Too narrow for the mark and any real character both. The mark alone,
    // itself clipped, still says "there is more here"; content with no mark
    // would silently claim to be whole.
    if width <= mark_width {
        return mark.chars().take(width).collect();
    }
    let mut out: String = text.chars().take(width - mark_width).collect();
    out.push_str(mark);
    out
}
