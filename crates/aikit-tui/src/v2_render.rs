//! ResourceRef-native renderer for the resting V2 human shell.
//!
//! Quick and Workspace are presentations of [`TuiState`], not alternate semantic
//! controllers. This renderer therefore knows only the application read model,
//! stable selection, contextual Actions, Workspace section, staging and overlays.
//! Capability-specific forms and run output continue to use the compatibility
//! renderer while those operations are migrated to first-class V2 Actions.

use ratatui::layout::Alignment;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use aikit_core::resource::ActionStageability;

use crate::application::{
    visible_contextual_actions, ActionOutcome, Overlay, PresentationMode, ResourceListItem,
    TuiState, WorkspaceSection,
};
use crate::layout::Layout;
use crate::navigation::AmbientContext;
use crate::theme::Theme;

pub fn draw(frame: &mut Frame, state: &TuiState) {
    draw_with_context(frame, state, &AmbientContext::default());
}

pub fn draw_with_context(frame: &mut Frame, state: &TuiState, ambient: &AmbientContext) {
    let theme = Theme::new();
    let area = frame.area();
    let base_title = match state.presentation {
        PresentationMode::Quick => "AIKit · Quick",
        PresentationMode::Workspace => "AIKit · Workspace",
    };
    let ambient_line = ambient.line(area.width.saturating_sub(20));
    let title = if ambient_line.is_empty() {
        format!(" {base_title} ")
    } else {
        format!(" {base_title} · {ambient_line} ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type())
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
    frame.render_widget(query_line(state, &theme), panes.query);
    draw_resources(frame, state, &theme, panes.list);
    if let Some(preview) = panes.preview {
        frame.render_widget(preview_pane(state, &theme), preview);
    }
    frame.render_widget(footer(state, &theme), panes.footer);
}

fn query_line<'a>(state: &'a TuiState, theme: &Theme) -> Paragraph<'a> {
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
                Span::styled("search resources and actions", theme.dim())
            } else {
                Span::styled(state.query.clone(), theme.base())
            },
        ]
    };
    if state.presentation == PresentationMode::Workspace {
        spans.push(Span::raw("   "));
        for (index, section) in WorkspaceSection::ALL.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(" · ", theme.dim()));
            }
            spans.push(Span::styled(
                section.as_str(),
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

fn draw_resources(frame: &mut Frame, state: &TuiState, theme: &Theme, area: ratatui::layout::Rect) {
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

    let height = area.height as usize;
    let selected_index = state
        .selected
        .as_ref()
        .and_then(|selected| state.read_model.position(selected))
        .unwrap_or(0);
    let first = selected_index.saturating_sub(height.saturating_sub(1));
    let lines = state
        .read_model
        .resources
        .iter()
        .enumerate()
        .skip(first)
        .take(height)
        .map(|(index, item)| resource_line(state, theme, item, index == selected_index, area.width))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

fn resource_line<'a>(
    state: &TuiState,
    theme: &Theme,
    item: &'a ResourceListItem,
    selected: bool,
    width: u16,
) -> Line<'a> {
    let staged = state.staged.get(&item.resource).is_some();
    let cursor = if selected { '›' } else { ' ' };
    let staged_mark = if staged { '*' } else { ' ' };
    let kind = format!("[{}]", item.kind.as_str());
    let fixed = 5 + kind.chars().count();
    let available = (width as usize).saturating_sub(fixed);
    let label_width = available.min(28);
    let summary_width = available.saturating_sub(label_width + 1);

    let mut spans = vec![
        Span::styled(
            format!("{cursor}{staged_mark} "),
            if staged { theme.staged() } else { theme.accent() },
        ),
        Span::styled(format!("{} ", pad(&kind, 20.min(kind.chars().count().max(8)))), theme.dim()),
        Span::styled(
            pad(&item.label, label_width),
            if selected { theme.selected() } else { theme.base() },
        ),
    ];
    if summary_width > 3 {
        spans.push(Span::styled(
            format!(" {}", truncate(&item.summary, summary_width.saturating_sub(1))),
            theme.dim(),
        ));
    }
    Line::from(spans)
}

fn preview_pane<'a>(state: &'a TuiState, theme: &Theme) -> Paragraph<'a> {
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
            Line::from(Span::styled("Ctrl+S applies · Esc returns", theme.staged())),
        ])
        .wrap(Wrap { trim: false });
    }
    if state.overlay == Some(Overlay::CompositionPreview) {
        let summary = state
            .preview
            .as_ref()
            .map(|preview| preview.summary.as_str())
            .unwrap_or("preview unavailable");
        return Paragraph::new(vec![
            Line::from(Span::styled("Composition preview", theme.heading())),
            Line::from(""),
            Line::from(Span::raw(summary.to_string())),
            Line::from(""),
            Line::from(Span::styled(
                "Ctrl+S proceeds to confirmation · Esc returns",
                theme.dim(),
            )),
        ])
        .wrap(Wrap { trim: false });
    }
    if state.overlay == Some(Overlay::Explain) {
        if let Some(ActionOutcome::Explained { subject, summary }) = state.action_result.as_ref() {
            return Paragraph::new(vec![
                Line::from(Span::styled(format!("Explain · {subject}"), theme.heading())),
                Line::from(""),
                Line::from(Span::raw(summary.clone())),
                Line::from(""),
                Line::from(Span::styled("Esc returns", theme.dim())),
            ])
            .wrap(Wrap { trim: false });
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
                "Actions · text mode"
            } else {
                "Actions · press :"
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
                ActionStageability::NotStageable => "›",
            };
            let cursor = if state.action_query.is_some() && index == state.action_cursor {
                "→"
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
                Span::styled(format!(" · {}", action.description), theme.dim()),
            ]));
        }
        if actions.is_empty() && state.action_query.is_some() {
            lines.push(Line::from(Span::styled("no matching contextual actions", theme.dim())));
        }
    }
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn footer<'a>(state: &'a TuiState, theme: &Theme) -> Paragraph<'a> {
    if let Some(status) = &state.status {
        return Paragraph::new(Line::from(Span::styled(status.message.clone(), theme.dim())));
    }
    let scope = state
        .mutation_scope
        .map(|scope| scope.as_str())
        .unwrap_or("unresolved");
    let text = if state.action_query.is_some() {
        "Action mode · type to filter · ↑↓ choose · Enter invoke · Space invoke if stageable · Esc return"
            .to_string()
    } else if state.presentation == PresentationMode::Workspace {
        format!(
            "{} · {} result{} · {} staged · scope {} · Alt+←/→ sections · : actions · Ctrl+W Quick",
            state.workspace_section.as_str(),
            state.read_model.resources.len(),
            if state.read_model.resources.len() == 1 { "" } else { "s" },
            state.staged.len(),
            scope,
        )
    } else {
        format!(
            "{} result{} · {} staged · scope {} · ↑↓ navigate · : actions · Space stage · Ctrl+S preview/apply · Ctrl+W Workspace",
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

fn pad(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = truncate(text, width);
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn truncate(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut out: String = text.chars().take(width - 1).collect();
    out.push('…');
    out
}
