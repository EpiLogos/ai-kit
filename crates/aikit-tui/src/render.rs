//! Drawing.
//!
//! Every frame is a pure function of [`AppState`]: `draw` reads, never writes,
//! and never asks the backend anything. That is what makes the snapshot tests
//! meaningful — a snapshot is a statement about a state, not about a sequence of
//! calls that happened to have been made first.
//!
//! The palette is one bordered frame containing a query line, a body and a
//! footer. Which body depends on the mode; how much of it is drawn depends on the
//! width, and [`crate::layout`] owns that decision. Dialogs are drawn over the
//! body rather than in a second window, because a palette that opens windows is
//! on its way to being an application.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use aikit_core::search::DocStatus;

use crate::app::{key_map, lane_hints, AppState, ConfirmKind, Level, Mode};
use crate::layout::{state_note, Declared, Layout};
use crate::search::Row;
use crate::staging::ProblemKind;
use crate::theme::Theme;

/// Draw the whole palette.
pub fn draw(frame: &mut Frame, state: &AppState) {
    let theme = Theme::new();
    let area = frame.area();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type())
        .border_style(theme.border())
        .title(title(state))
        .title_alignment(Alignment::Left);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let layout = Layout::for_width(inner.width);
    let panes = layout.split(inner);

    frame.render_widget(query_line(state, &theme, &layout), panes.query);
    frame.render_widget(footer(state, &theme), panes.footer);

    match state.mode {
        Mode::Search | Mode::Preview | Mode::Confirm => {
            draw_list(frame, state, &theme, &layout, panes.list);
            if let Some(preview) = panes.preview {
                frame.render_widget(preview_pane(state, &theme), preview);
            } else if state.mode == Mode::Preview {
                frame.render_widget(preview_pane(state, &theme), panes.list);
            }
        }
        Mode::ArgForm => {
            frame.render_widget(form_pane(state, &theme), panes.list);
            if let Some(preview) = panes.preview {
                frame.render_widget(run_preview_pane(state, &theme), preview);
            }
        }
        Mode::StagedDiff => {
            let body = merge(panes.list, panes.preview);
            frame.render_widget(staged_pane(state, &theme), body);
        }
        Mode::Promotion => {
            let body = merge(panes.list, panes.preview);
            frame.render_widget(promotion_pane(state, &theme), body);
        }
        Mode::JobOutput => {
            let body = merge(panes.list, panes.preview);
            frame.render_widget(job_pane(state, &theme), body);
        }
        Mode::Help => {
            let body = merge(panes.list, panes.preview);
            frame.render_widget(help_pane(state, &theme), body);
        }
    }

    if state.mode == Mode::Confirm {
        draw_confirm(frame, state, &theme, inner);
    }
}

fn merge(list: Rect, preview: Option<Rect>) -> Rect {
    match preview {
        Some(preview) => Rect {
            width: list.width + preview.width,
            ..list
        },
        None => list,
    }
}

// ---------------------------------------------------------------------------
// Chrome
// ---------------------------------------------------------------------------

fn title(state: &AppState) -> String {
    // Mode and context are stable for the whole frame. Scope remains a footer
    // fact because it changes with Tab.
    format!(" AIKit palette · {} ", state.descriptor.label())
}

/// The query box. `/` rather than a chevron, because the chevron is the row
/// cursor and two identical marks on adjacent lines mean neither of them reads.
fn query_line<'a>(state: &'a AppState, theme: &Theme, layout: &Layout) -> Paragraph<'a> {
    let mut spans = vec![Span::styled("/ ", theme.accent())];
    if state.query.is_empty() {
        spans.push(Span::styled(hint(layout), theme.dim()));
    } else {
        spans.push(Span::styled(state.query.clone(), theme.base()));
    }
    Paragraph::new(Line::from(spans))
}

/// The lane hint, which loses its prose before it loses its characters: knowing
/// the four lanes exist is most of the value, and a clipped sentence is none.
fn hint(layout: &Layout) -> String {
    let lanes = lane_hints();
    if layout.has_room_for_prose() {
        lanes
            .into_iter()
            .map(|(c, what)| format!("{c} {what}"))
            .collect::<Vec<_>>()
            .join("   ")
    } else {
        lanes
            .into_iter()
            .map(|(c, _)| c.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn footer<'a>(state: &'a AppState, theme: &Theme) -> Paragraph<'a> {
    if let Some(status) = &state.status {
        let style = match status.level {
            Level::Error => theme.error(),
            Level::Warning => theme.unavailable(),
            Level::Info => theme.dim(),
        };
        let text = match status.code {
            Some(code) => format!("{}  [{code}]", status.message),
            None => status.message.clone(),
        };
        return Paragraph::new(Line::from(Span::styled(text, style)));
    }
    if let Some(problem) = state.staged_problem() {
        return Paragraph::new(Line::from(Span::styled(
            format!("{}  [{}]", problem.headline(), problem.code()),
            theme.error(),
        )));
    }
    if let Some(diff) = state.staged_diff() {
        let mut spans = vec![Span::styled(diff.footer(), theme.staged())];
        for effect in &diff.client_effects {
            spans.push(Span::styled(
                format!("  ·  {}", effect.describe()),
                theme.dim(),
            ));
        }
        return Paragraph::new(Line::from(spans));
    }
    let resting = match state.mode {
        Mode::ArgForm => "Enter runs · Esc goes back · * required".to_string(),
        Mode::Promotion => "Enter promotes · Esc goes back".to_string(),
        Mode::JobOutput => "Enter or Esc returns to the list".to_string(),
        Mode::Help => "Esc returns".to_string(),
        _ => format!(
            "{} {} · writing to {} {} · Ctrl-T tree · ? for keys",
            state.rows.len(),
            if state.rows.len() == 1 {
                "capability"
            } else {
                "capabilities"
            },
            state.scope.current(),
            state.glyphs.scope_badge(Some(state.scope.current()))
        ),
    };
    Paragraph::new(Line::from(Span::styled(resting, theme.dim())))
}

// ---------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------

fn draw_list(frame: &mut Frame, state: &AppState, theme: &Theme, layout: &Layout, area: Rect) {
    if area.height == 0 {
        return;
    }
    if state.in_manage_lane() {
        let lines: Vec<Line> = state
            .manage_rows()
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let style = if index == state.cursor {
                    theme.selected()
                } else {
                    theme.base()
                };
                Line::from(vec![
                    Span::styled(
                        format!(
                            "{} {}",
                            if index == state.cursor {
                                state.glyphs.selected()
                            } else {
                                ' '
                            },
                            action.label()
                        ),
                        style,
                    ),
                    Span::styled(format!("  {}", action.description()), theme.dim()),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), area);
        return;
    }

    if state.rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "nothing matches — Esc clears the query",
                theme.dim(),
            ))),
            area,
        );
        return;
    }

    // Keep the cursor on screen without scrolling more than necessary: the list
    // moves under the user only when it has to.
    let height = area.height as usize;
    let first = state.cursor.saturating_sub(height.saturating_sub(1));
    let lines: Vec<Line> = state
        .rows
        .iter()
        .enumerate()
        .skip(first)
        .take(height)
        .map(|(index, row)| row_line(state, theme, layout, area.width, row, index == state.cursor))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// One row, sized to the pane it is drawn in.
///
/// Columns are budgeted from the actual width rather than fixed, because the
/// alternative — constants that fit a 120-column terminal — writes the
/// description straight through the preview pane at 100 columns.
fn row_line<'a>(
    state: &AppState,
    theme: &Theme,
    layout: &Layout,
    width: u16,
    row: &'a Row,
    selected: bool,
) -> Line<'a> {
    let glyphs = state.glyphs;
    let doc = &row.doc;

    let declared = if state.view.is_declared_enabled(&doc.id) {
        Declared::Enabled
    } else if state.view.is_declared_disabled(&doc.id) {
        Declared::Disabled
    } else {
        Declared::Undeclared
    };
    let staged = state.staged.state_of(&doc.id);

    let cursor = if selected { glyphs.selected() } else { ' ' };
    let stage_mark = if staged.is_some() {
        glyphs.staged()
    } else {
        ' '
    };

    let mut spans = vec![
        Span::styled(
            format!("{cursor}{stage_mark}"),
            if staged.is_some() {
                theme.staged()
            } else {
                theme.accent()
            },
        ),
        Span::styled(
            format!(" {}", glyphs.declared(declared)),
            theme.dim(),
        ),
        Span::styled(
            format!("{}", glyphs.effective(doc.status)),
            status_style(theme, doc.status),
        ),
        Span::styled(
            format!(" {} ", glyphs.scope_badge(doc.scope)),
            theme.accent(),
        ),
    ];

    // Eight cells are already spent on the cursor, the two state marks and the
    // scope badge.
    let mut remaining = (width as usize).saturating_sub(8);
    let kind_width = if layout.shows_kind_column() { 10 } else { 0 };
    let trust_width = if layout.shows_trust_column() { 11 } else { 0 };
    remaining = remaining.saturating_sub(kind_width + trust_width);

    let name_width = if layout.shows_description() {
        remaining.min(24)
    } else {
        remaining
    };
    remaining = remaining.saturating_sub(name_width);

    spans.push(Span::styled(
        pad(&doc.name, name_width),
        if selected { theme.selected() } else { theme.base() },
    ));

    if layout.shows_description() && remaining > 2 {
        spans.push(Span::styled(
            format!(" {}", pad(&doc.description, remaining - 1)),
            theme.dim(),
        ));
    }
    if layout.shows_kind_column() {
        spans.push(Span::styled(
            format!(" {}", pad(doc.kind.as_str(), kind_width - 1)),
            theme.dim(),
        ));
    }
    if layout.shows_trust_column() {
        spans.push(Span::styled(
            format!(" {}", pad(&doc.trust.to_string(), trust_width - 1)),
            theme.dim(),
        ));
    }
    Line::from(spans)
}

fn status_style(theme: &Theme, status: DocStatus) -> Style {
    match status {
        DocStatus::Active => theme.active(),
        DocStatus::Inactive => theme.dim(),
        DocStatus::Unavailable => theme.unavailable(),
    }
}

fn pad(text: &str, width: usize) -> String {
    if width == 0 {
        return text.to_string();
    }
    let mut out = truncate(text, width);
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// Panes
// ---------------------------------------------------------------------------

/// The capability preview: core's own explanation, verbatim.
///
/// Not a re-rendering of it. `aikit explain` and the palette must not describe
/// the same decision two different ways, and the cheapest guarantee of that is to
/// print the same string.
fn preview_pane<'a>(state: &AppState, theme: &Theme) -> Paragraph<'a> {
    let Some(row) = state.selected_row() else {
        return Paragraph::new(Line::from(Span::styled("nothing selected", theme.dim())));
    };
    let mut lines: Vec<Line> = Vec::new();
    match state.view.explain(&row.doc.id) {
        Some(explanation) => {
            for line in explanation.render().lines() {
                lines.push(Line::from(Span::raw(line.to_string())));
            }
        }
        None => {
            lines.push(Line::from(Span::styled(
                state_note(row.doc.status, None),
                theme.dim(),
            )));
        }
    }
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn form_pane<'a>(state: &AppState, theme: &Theme) -> Paragraph<'a> {
    let Some(form) = &state.form else {
        return Paragraph::new(Line::from(Span::styled("no form", theme.dim())));
    };
    let mut lines: Vec<Line> = Vec::new();
    for (index, field) in form.fields().iter().enumerate() {
        let focused = index == form.focused();
        let marker = if focused { state.glyphs.selected() } else { ' ' };
        let requirement = if field.required() { "*" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{marker}{requirement} {}", pad(field.spec.display_label(), 16)),
                if focused { theme.heading() } else { theme.base() },
            ),
            Span::styled(
                format!(" {}", pad(&field.display(), 24)),
                if focused { theme.selected() } else { theme.base() },
            ),
            Span::styled(format!("  {}", field.type_hint()), theme.dim()),
        ]));
        if let Some(error) = field.error() {
            lines.push(Line::from(Span::styled(
                format!("   {error}"),
                theme.error(),
            )));
        } else if let Some(help) = &field.spec.help {
            lines.push(Line::from(Span::styled(
                format!("   {help}"),
                theme.dim(),
            )));
        }
    }
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn run_preview_pane<'a>(state: &AppState, theme: &Theme) -> Paragraph<'a> {
    let (Some(form), Some(capsule)) = (&state.form, state.form_capsule()) else {
        return Paragraph::new(Line::from(Span::styled("no run to preview", theme.dim())));
    };
    let context = state.form_context();
    let preview = form
        .intent(capsule, &state.descriptor)
        .map(|intent| crate::form::RunPreview::of(capsule, &intent, &context));
    let mut lines: Vec<Line> = Vec::new();
    match preview {
        Ok(preview) => {
            for (label, value) in preview.rows() {
                lines.push(Line::from(vec![
                    Span::styled(pad(label, 18), theme.dim()),
                    Span::raw(value.clone()),
                ]));
            }
        }
        Err(error) => lines.push(Line::from(Span::styled(
            format!("incomplete — {}", error.message()),
            theme.dim(),
        ))),
    }
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn staged_pane<'a>(state: &AppState, theme: &Theme) -> Paragraph<'a> {
    let mut lines: Vec<Line> = Vec::new();
    if let Some(problem) = state.staged_problem() {
        lines.push(Line::from(Span::styled(
            problem.headline(),
            theme.error(),
        )));
        lines.push(Line::from(Span::styled(
            format!("code: {}", problem.code()),
            theme.dim(),
        )));
        if let ProblemKind::Conflict { left, right } = &problem.kind {
            lines.push(Line::from(Span::raw(format!(
                "{left} and {right} cannot both be active in this context."
            ))));
        }
        let choices = problem.choices();
        if !choices.is_empty() {
            lines.push(Line::from(Span::styled("Choose:", theme.heading())));
            for choice in choices {
                lines.push(Line::from(Span::raw(format!("  · {choice}"))));
            }
        }
        lines.push(Line::from(Span::styled(
            "Esc to go back — nothing has been written.",
            theme.dim(),
        )));
        return Paragraph::new(lines).wrap(Wrap { trim: false });
    }

    let Some(diff) = state.staged_diff() else {
        return Paragraph::new(Line::from(Span::styled("nothing staged", theme.dim())));
    };
    lines.push(Line::from(Span::styled("Staged", theme.heading())));
    for toggle in &diff.requested {
        let verb = if toggle.enable { "enable " } else { "disable" };
        lines.push(Line::from(Span::raw(format!("  {verb}  {}", toggle.capsule))));
    }
    if !diff.added_dependencies.is_empty() {
        lines.push(Line::from(Span::styled("Comes with", theme.heading())));
        for id in &diff.added_dependencies {
            lines.push(Line::from(Span::raw(format!("  + {id}"))));
        }
    }
    if !diff.dropped_dependencies.is_empty() {
        lines.push(Line::from(Span::styled("Goes away", theme.heading())));
        for id in &diff.dropped_dependencies {
            lines.push(Line::from(Span::raw(format!("  - {id}"))));
        }
    }
    if !diff.still_unavailable.is_empty() {
        lines.push(Line::from(Span::styled(
            "Declared but still held back",
            theme.heading(),
        )));
        for (id, reason) in &diff.still_unavailable {
            lines.push(Line::from(Span::styled(
                format!("  ! {id} — {reason}"),
                theme.unavailable(),
            )));
        }
    }
    lines.push(Line::from(Span::styled("Clients", theme.heading())));
    for effect in &diff.client_effects {
        lines.push(Line::from(Span::raw(format!("  {}", effect.describe()))));
    }
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn promotion_pane<'a>(state: &AppState, theme: &Theme) -> Paragraph<'a> {
    let Some(draft) = state.promotion_draft() else {
        return Paragraph::new(Line::from(Span::styled(
            "the inbox is empty",
            theme.dim(),
        )));
    };
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(draft.candidate.title.clone(), theme.heading())),
        Line::from(Span::raw(format!("would become  {}", draft.edits.id))),
        Line::from(Span::raw(format!("description   {}", draft.edits.description))),
        Line::from(Span::raw(format!(
            "exports       {}",
            if draft.edits.exports.is_empty() {
                "none".to_string()
            } else {
                draft.edits.exports.join(", ")
            }
        ))),
        Line::from(Span::raw(format!(
            "maturity      {}",
            draft.edits.maturity.as_str()
        ))),
    ];

    if !draft.similar.is_empty() {
        lines.push(Line::from(Span::styled("Similar to", theme.heading())));
        for similarity in &draft.similar {
            lines.push(Line::from(Span::raw(format!(
                "  {}%  {}  — {}",
                similarity.percentage, similarity.other, similarity.summary
            ))));
        }
    }

    match draft.withheld_reason() {
        Some(reason) => {
            // The body is not merely hidden here — it was never stored. See
            // `PromotionDraft::with_body`.
            lines.push(Line::from(Span::styled(
                format!("Withheld: {reason}"),
                theme.error(),
            )));
            lines.push(Line::from(Span::styled(
                "Its text is not shown and it cannot be promoted.",
                theme.dim(),
            )));
            for finding in &draft.candidate.findings {
                lines.push(Line::from(Span::styled(
                    format!("  {} — {}", finding.rule, finding.preview),
                    theme.unavailable(),
                )));
            }
        }
        None => {
            lines.push(Line::from(Span::styled("Body", theme.heading())));
            for line in draft.body() {
                lines.push(Line::from(Span::raw(format!("  {line}"))));
            }
        }
    }
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn job_pane<'a>(state: &AppState, theme: &Theme) -> Paragraph<'a> {
    let Some(job) = &state.job else {
        return Paragraph::new(Line::from(Span::styled("no output", theme.dim())));
    };
    let mut lines: Vec<Line> = Vec::new();
    let header = match job.status {
        Some(0) => Span::styled("finished successfully", theme.active()),
        Some(code) => Span::styled(format!("exited with status {code}"), theme.error()),
        None => Span::styled("running", theme.dim()),
    };
    lines.push(Line::from(header));
    for line in &job.lines {
        lines.push(Line::from(Span::raw(line.clone())));
    }
    if job.truncated {
        lines.push(Line::from(Span::styled(
            "… output truncated",
            theme.dim(),
        )));
    }
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn help_pane<'a>(state: &AppState, theme: &Theme) -> Paragraph<'a> {
    let mut lines: Vec<Line> = vec![Line::from(Span::styled("Keys", theme.heading()))];
    for (key, what) in key_map(state.mode) {
        lines.push(Line::from(vec![
            Span::styled(pad(key, 14), theme.accent()),
            Span::raw(what),
        ]));
    }
    lines.push(Line::from(Span::styled("Lanes", theme.heading())));
    for (character, what) in lane_hints() {
        lines.push(Line::from(vec![
            Span::styled(pad(&character.to_string(), 14), theme.accent()),
            Span::raw(what),
        ]));
    }
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

fn draw_confirm(frame: &mut Frame, state: &AppState, theme: &Theme, area: Rect) {
    let Some(confirm) = &state.confirm else {
        return;
    };
    let width = area.width.saturating_sub(4).clamp(1, 72);
    let height = area.height.saturating_sub(2).clamp(1, 9);
    let dialog = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    let heading = match &confirm.kind {
        ConfirmKind::WriteScope(scope) => format!("Write to the {scope} profile"),
        ConfirmKind::RunUnreviewed(id) => format!("Run an unreviewed revision of {id}"),
        ConfirmKind::Promote(_) => "Promote a capture".to_string(),
    };
    let lines = vec![
        Line::from(Span::styled(confirm.prompt.clone(), theme.heading())),
        Line::from(Span::raw(confirm.detail.clone())),
        Line::from(Span::styled(
            "Enter to confirm · Esc to go back",
            theme.dim(),
        )),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type())
        .border_style(theme.error())
        .title(format!(" {heading} "));

    frame.render_widget(Clear, dialog);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(block),
        dialog,
    );
}
