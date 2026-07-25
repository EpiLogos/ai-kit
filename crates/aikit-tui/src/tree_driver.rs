//! Interactive terminal host for the pure tree model.
//!
//! This module owns only interaction mechanics: terminal events become
//! [`TreeAction`]s, the reducer changes [`TreeState`], and effects/outcomes are
//! returned to the CLI. Registry, resolver and mutation semantics stay outside
//! the TUI.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};

use aikit_core::error::AikitError;
use aikit_core::id::CapsuleId;
use aikit_core::Result;

use crate::event::{CrosstermEvents, EventSource, PaletteEvent};
use crate::host::UiHost;
use crate::layout::Glyphs;
use crate::theme::Theme;
use crate::tree::{self, TreeAction, TreeEffect, TreeGlyphs, TreeState};

const ASCII_BORDER: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// How the CLI asks for the interactive tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRequest {
    pub host: UiHost,
    pub glyphs: Glyphs,
    pub apply_confirmation: Option<ApplyConfirmation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyConfirmation {
    pub prompt: String,
    pub detail: String,
}

impl TreeRequest {
    pub fn new(host: UiHost) -> Self {
        Self {
            host,
            glyphs: Glyphs::from_env(),
            apply_confirmation: None,
        }
    }

    #[must_use]
    pub fn with_glyphs(mut self, glyphs: Glyphs) -> Self {
        self.glyphs = glyphs;
        self
    }

    #[must_use]
    pub fn with_apply_confirmation(
        mut self,
        prompt: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        self.apply_confirmation = Some(ApplyConfirmation {
            prompt: prompt.into(),
            detail: detail.into(),
        });
        self
    }
}

/// What the tree asks its host to do after the terminal has been restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeOutcome {
    Closed,
    Apply(Vec<CapsuleId>),
    Effect(TreeEffect),
}

#[derive(Debug, Clone, Copy)]
struct ViewportState {
    list: Rect,
    scroll: usize,
}

#[derive(Debug, Clone)]
struct DragState {
    source_index: usize,
    source_set: Option<String>,
    moved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditPrompt {
    Create(String),
    Rename { from: String, input: String },
    Delete { set: String },
}

#[derive(Debug, Clone, Copy)]
struct DrawMode<'a> {
    filtering: bool,
    confirming: bool,
    editing: Option<&'a EditPrompt>,
    help: bool,
    confirmation: Option<&'a ApplyConfirmation>,
}

/// Drive an interactive tree on any ratatui backend and event source.
pub fn event_loop<B, E>(
    terminal: &mut Terminal<B>,
    events: &mut E,
    mut state: TreeState,
    request: TreeRequest,
) -> Result<TreeOutcome>
where
    B: Backend,
    B::Error: std::fmt::Display,
    E: EventSource + ?Sized,
{
    let mut filtering = false;
    let mut confirming = false;
    let mut editing: Option<EditPrompt> = None;
    let mut help = false;
    let mut pending_g = false;
    let mut pending_z = false;
    let mut last_clicked = None;
    let mut dragging = None;
    let mut centered_on = None;

    loop {
        let size = terminal
            .size()
            .map_err(|e| draw_error("could not measure the terminal", e))?;
        let viewport = viewport(size.into(), &state, centered_on);
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &state,
                    request.glyphs,
                    DrawMode {
                        filtering,
                        confirming,
                        editing: editing.as_ref(),
                        help,
                        confirmation: request.apply_confirmation.as_ref(),
                    },
                    viewport,
                )
            })
            .map_err(|e| draw_error("could not draw the tree", e))?;

        let Some(event) = events.next()? else {
            return Ok(TreeOutcome::Closed);
        };

        if confirming {
            match event {
                PaletteEvent::Key(key)
                    if key.kind != KeyEventKind::Release && key.code == KeyCode::Enter =>
                {
                    return Ok(TreeOutcome::Apply(state.staged.iter().cloned().collect()));
                }
                PaletteEvent::Key(key)
                    if key.kind != KeyEventKind::Release && key.code == KeyCode::Esc =>
                {
                    confirming = false;
                    continue;
                }
                PaletteEvent::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && mouse.row == viewport.list.bottom() =>
                {
                    match modal_control(mouse.column, viewport.list.x) {
                        Some(ModalControl::Confirm) => {
                            return Ok(TreeOutcome::Apply(state.staged.iter().cloned().collect()));
                        }
                        Some(ModalControl::Cancel) => confirming = false,
                        None => {}
                    }
                    continue;
                }
                _ => continue,
            }
        }

        if let Some(prompt) = &mut editing {
            if let PaletteEvent::Mouse(mouse) = event {
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && mouse.row == viewport.list.bottom()
                {
                    match modal_control(mouse.column, viewport.list.x) {
                        Some(ModalControl::Confirm) => {
                            if let Some(effect) = edit_effect(prompt) {
                                return Ok(TreeOutcome::Effect(effect));
                            }
                        }
                        Some(ModalControl::Cancel) => editing = None,
                        None => {}
                    }
                }
                continue;
            }
            let PaletteEvent::Key(key) = event else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Esc => editing = None,
                KeyCode::Backspace => match prompt {
                    EditPrompt::Create(input) | EditPrompt::Rename { input, .. } => {
                        input.pop();
                    }
                    EditPrompt::Delete { .. } => {}
                },
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    match prompt {
                        EditPrompt::Create(input) | EditPrompt::Rename { input, .. } => {
                            input.push(character);
                        }
                        EditPrompt::Delete { .. } => {}
                    }
                }
                KeyCode::Enter => {
                    if let Some(effect) = edit_effect(prompt) {
                        return Ok(TreeOutcome::Effect(effect));
                    }
                }
                _ => {}
            }
            continue;
        }

        if help {
            match &event {
                PaletteEvent::Key(key)
                    if key.kind != KeyEventKind::Release
                        && matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) =>
                {
                    help = false
                }
                PaletteEvent::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && mouse.row == viewport.list.bottom()
                        && modal_control(mouse.column, viewport.list.x).is_some() =>
                {
                    help = false
                }
                _ => {}
            }
            continue;
        }

        if let PaletteEvent::Key(key) = &event {
            if key.kind != KeyEventKind::Release {
                match key.code {
                    KeyCode::Char('a') => {
                        editing = Some(EditPrompt::Create(String::new()));
                        continue;
                    }
                    KeyCode::Char('r') => {
                        if let Some(set) = selected_writable_set(&state) {
                            editing = Some(EditPrompt::Rename {
                                input: set.clone(),
                                from: set,
                            });
                        }
                        continue;
                    }
                    KeyCode::Char('D') => {
                        if let Some(set) = selected_writable_set(&state) {
                            editing = Some(EditPrompt::Delete { set });
                        }
                        continue;
                    }
                    KeyCode::Char('?') => {
                        help = true;
                        continue;
                    }
                    KeyCode::Char('z') if pending_z => {
                        pending_z = false;
                        centered_on = Some(state.selected);
                        continue;
                    }
                    KeyCode::Char('z') => {
                        pending_z = true;
                        continue;
                    }
                    _ => pending_z = false,
                }
            }
        }

        if let PaletteEvent::Mouse(mouse) = &event {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && mouse.row == viewport.list.bottom()
            {
                match footer_control_at(mouse.column, viewport.list.x, &state) {
                    Some(FooterControl::Apply) if !state.staged.is_empty() => {
                        if request.apply_confirmation.is_some() {
                            confirming = true;
                        } else {
                            return Ok(TreeOutcome::Apply(state.staged.iter().cloned().collect()));
                        }
                    }
                    Some(FooterControl::Add) => {
                        editing = Some(EditPrompt::Create(String::new()));
                    }
                    Some(FooterControl::Rename) => {
                        if let Some(set) = selected_writable_set(&state) {
                            editing = Some(EditPrompt::Rename {
                                input: set.clone(),
                                from: set,
                            });
                        }
                    }
                    Some(FooterControl::Delete) => {
                        if let Some(set) = selected_writable_set(&state) {
                            editing = Some(EditPrompt::Delete { set });
                        }
                    }
                    Some(FooterControl::Remove) => {
                        let effects = tree::reduce(&mut state, TreeAction::RemoveFromSet);
                        if let Some(effect) = effects.into_iter().next() {
                            return Ok(TreeOutcome::Effect(effect));
                        }
                    }
                    Some(FooterControl::Center) => centered_on = Some(state.selected),
                    Some(FooterControl::Help) => help = true,
                    Some(FooterControl::Close) => return Ok(TreeOutcome::Closed),
                    Some(FooterControl::Apply) | None => {}
                }
                continue;
            }
        }

        let actions = actions_for(
            &event,
            &state,
            viewport,
            &mut filtering,
            &mut pending_g,
            &mut last_clicked,
            &mut dragging,
        );
        for command in actions {
            match command {
                TreeCommand::Action(action) => {
                    let previously_selected = state.selected;
                    let effects = tree::reduce(&mut state, action);
                    if state.selected != previously_selected {
                        centered_on = None;
                    }
                    if let Some(effect) = effects.into_iter().next() {
                        return Ok(TreeOutcome::Effect(effect));
                    }
                }
                TreeCommand::Apply => {
                    if !state.staged.is_empty() {
                        if request.apply_confirmation.is_some() {
                            confirming = true;
                            continue;
                        }
                        return Ok(TreeOutcome::Apply(state.staged.iter().cloned().collect()));
                    }
                }
                TreeCommand::Close => return Ok(TreeOutcome::Closed),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TreeCommand {
    Action(TreeAction),
    Apply,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FooterControl {
    Apply,
    Add,
    Rename,
    Delete,
    Remove,
    Center,
    Help,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalControl {
    Confirm,
    Cancel,
}

fn footer_controls(state: &TreeState) -> [(String, FooterControl); 8] {
    [
        (
            format!("[apply {}]", state.staged.len()),
            FooterControl::Apply,
        ),
        ("[add]".to_string(), FooterControl::Add),
        ("[rename]".to_string(), FooterControl::Rename),
        ("[delete]".to_string(), FooterControl::Delete),
        ("[remove]".to_string(), FooterControl::Remove),
        ("[center]".to_string(), FooterControl::Center),
        ("[help]".to_string(), FooterControl::Help),
        ("[close]".to_string(), FooterControl::Close),
    ]
}

fn footer_control_at(column: u16, start: u16, state: &TreeState) -> Option<FooterControl> {
    let relative = usize::from(column.saturating_sub(start));
    let mut cursor = 0;
    for (label, control) in footer_controls(state) {
        let end = cursor + label.len();
        if (cursor..end).contains(&relative) {
            return Some(control);
        }
        cursor = end + 1;
    }
    None
}

fn modal_control(column: u16, start: u16) -> Option<ModalControl> {
    let relative = column.saturating_sub(start);
    match relative {
        0..=8 => Some(ModalControl::Confirm),
        10..=17 => Some(ModalControl::Cancel),
        _ => None,
    }
}

fn edit_effect(prompt: &EditPrompt) -> Option<TreeEffect> {
    match prompt {
        EditPrompt::Create(input) if !input.trim().is_empty() => Some(TreeEffect::CreateSet {
            set: input.trim().to_string(),
        }),
        EditPrompt::Rename { from, input } if !input.trim().is_empty() => {
            Some(TreeEffect::RenameSet {
                from: from.clone(),
                to: input.trim().to_string(),
            })
        }
        EditPrompt::Delete { set } => Some(TreeEffect::DeleteSet { set: set.clone() }),
        _ => None,
    }
}

fn actions_for(
    event: &PaletteEvent,
    state: &TreeState,
    viewport: ViewportState,
    filtering: &mut bool,
    pending_g: &mut bool,
    last_clicked: &mut Option<(usize, Instant)>,
    dragging: &mut Option<DragState>,
) -> Vec<TreeCommand> {
    match event {
        PaletteEvent::Key(key) => {
            *last_clicked = None;
            key_commands(*key, state, filtering, pending_g)
        }
        PaletteEvent::Mouse(mouse) => {
            mouse_commands(*mouse, state, viewport, filtering, last_clicked, dragging)
        }
        PaletteEvent::Resize(_, _) | PaletteEvent::Idle => Vec::new(),
    }
}

fn key_commands(
    key: KeyEvent,
    state: &TreeState,
    filtering: &mut bool,
    pending_g: &mut bool,
) -> Vec<TreeCommand> {
    if key.kind == KeyEventKind::Release {
        return Vec::new();
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if *filtering {
        return match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                *filtering = false;
                Vec::new()
            }
            KeyCode::Backspace => vec![TreeCommand::Action(TreeAction::Filter(
                state
                    .filter
                    .chars()
                    .rev()
                    .skip(1)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect(),
            ))],
            KeyCode::Char(c) if !ctrl => {
                vec![TreeCommand::Action(TreeAction::Filter(format!(
                    "{}{c}",
                    state.filter
                )))]
            }
            _ => Vec::new(),
        };
    }

    let command = match key.code {
        KeyCode::Esc | KeyCode::Char('q') => TreeCommand::Close,
        KeyCode::Down | KeyCode::Char('j') => TreeCommand::Action(TreeAction::Down),
        KeyCode::Up | KeyCode::Char('k') => TreeCommand::Action(TreeAction::Up),
        KeyCode::Right | KeyCode::Char('l') => TreeCommand::Action(TreeAction::Expand),
        KeyCode::Left | KeyCode::Char('h') => TreeCommand::Action(TreeAction::Collapse),
        KeyCode::Home => TreeCommand::Action(TreeAction::First),
        KeyCode::End | KeyCode::Char('G') => TreeCommand::Action(TreeAction::Last),
        KeyCode::PageDown => TreeCommand::Action(TreeAction::PageDown),
        KeyCode::PageUp => TreeCommand::Action(TreeAction::PageUp),
        KeyCode::Char('d') if ctrl => TreeCommand::Action(TreeAction::PageDown),
        KeyCode::Char('u') if ctrl => TreeCommand::Action(TreeAction::PageUp),
        KeyCode::Enter if ctrl => TreeCommand::Apply,
        KeyCode::Char('s') if ctrl => TreeCommand::Apply,
        // A printable fallback matters over PTY bridges whose parent line
        // discipline reserves Ctrl-S for flow control before raw mode begins.
        KeyCode::Char('S') => TreeCommand::Apply,
        KeyCode::Enter => TreeCommand::Action(TreeAction::Activate),
        KeyCode::Char(' ') => TreeCommand::Action(TreeAction::Stage),
        KeyCode::Char('y') => TreeCommand::Action(TreeAction::Yank),
        KeyCode::Char('p') => TreeCommand::Action(TreeAction::Put),
        KeyCode::Char('d') => TreeCommand::Action(TreeAction::RemoveFromSet),
        KeyCode::Char('/') => {
            *filtering = true;
            return Vec::new();
        }
        KeyCode::Char('g') if *pending_g => {
            *pending_g = false;
            TreeCommand::Action(TreeAction::First)
        }
        KeyCode::Char('g') => {
            *pending_g = true;
            return Vec::new();
        }
        _ => return Vec::new(),
    };
    if !matches!(key.code, KeyCode::Char('g')) {
        *pending_g = false;
    }
    vec![command]
}

fn mouse_commands(
    mouse: MouseEvent,
    state: &TreeState,
    viewport: ViewportState,
    filtering: &mut bool,
    last_clicked: &mut Option<(usize, Instant)>,
    dragging: &mut Option<DragState>,
) -> Vec<TreeCommand> {
    match mouse.kind {
        MouseEventKind::ScrollDown => {
            *last_clicked = None;
            vec![TreeCommand::Action(TreeAction::Down)]
        }
        MouseEventKind::ScrollUp => {
            *last_clicked = None;
            vec![TreeCommand::Action(TreeAction::Up)]
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if mouse.row == viewport.list.bottom().saturating_add(1) {
                *filtering = true;
                return Vec::new();
            }
            if mouse.row < viewport.list.y || mouse.row >= viewport.list.bottom() {
                return Vec::new();
            }
            let index = viewport.scroll + usize::from(mouse.row - viewport.list.y);
            let Some(row) = state.rows().get(index).cloned() else {
                return Vec::new();
            };
            let mut commands = vec![TreeCommand::Action(TreeAction::Select(index))];
            let marker_x =
                viewport.list.x + u16::try_from(row.depth.saturating_mul(2)).unwrap_or(u16::MAX);
            let stage_x = marker_x.saturating_add(2);
            if matches!(row.node.kind, tree::NodeKind::Capability { .. })
                && (stage_x..=stage_x.saturating_add(2)).contains(&mouse.column)
            {
                *dragging = None;
                commands.push(TreeCommand::Action(TreeAction::Stage));
            } else if row.node.expandable && mouse.column <= marker_x.saturating_add(1) {
                *dragging = None;
                commands.push(TreeCommand::Action(if row.expanded {
                    TreeAction::Collapse
                } else {
                    TreeAction::Expand
                }));
            } else if last_clicked.is_some_and(|(previous, at)| {
                previous == index && at.elapsed() <= Duration::from_millis(500)
            }) {
                commands.push(TreeCommand::Action(TreeAction::Activate));
            } else if matches!(row.node.kind, tree::NodeKind::Capability { .. }) {
                *dragging = Some(DragState {
                    source_index: index,
                    source_set: writable_set_for_row(state, index),
                    moved: false,
                });
                commands.push(TreeCommand::Action(TreeAction::Yank));
            }
            *last_clicked = Some((index, Instant::now()));
            commands
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if let Some(drag) = dragging.as_mut() {
                drag.moved = true;
            }
            row_at(state, viewport, mouse.row)
                .map(|index| vec![TreeCommand::Action(TreeAction::Select(index))])
                .unwrap_or_default()
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let Some(drag) = dragging.take() else {
                return Vec::new();
            };
            if !drag.moved {
                return Vec::new();
            }
            let Some(index) = row_at(state, viewport, mouse.row) else {
                return drag
                    .source_set
                    .map(|_| {
                        vec![
                            TreeCommand::Action(TreeAction::Select(drag.source_index)),
                            TreeCommand::Action(TreeAction::RemoveFromSet),
                        ]
                    })
                    .unwrap_or_default();
            };
            let rows = state.rows();
            let Some(row) = rows.get(index) else {
                return Vec::new();
            };
            if matches!(
                row.node.kind,
                tree::NodeKind::Set {
                    observed: false,
                    ..
                }
            ) {
                vec![
                    TreeCommand::Action(TreeAction::Select(index)),
                    TreeCommand::Action(TreeAction::Put),
                ]
            } else if drag.source_set.is_some()
                && writable_set_for_row(state, index) != drag.source_set
            {
                vec![
                    TreeCommand::Action(TreeAction::Select(drag.source_index)),
                    TreeCommand::Action(TreeAction::RemoveFromSet),
                ]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

fn writable_set_for_row(state: &TreeState, index: usize) -> Option<String> {
    let rows = state.rows();
    let row = rows.get(index)?;
    rows[..=index].iter().rev().find_map(|candidate| {
        let is_ancestor =
            candidate.path == row.path || row.path.starts_with(&format!("{}/", candidate.path));
        match &candidate.node.kind {
            tree::NodeKind::Set {
                name,
                observed: false,
            } if is_ancestor => Some(name.clone()),
            _ => None,
        }
    })
}

fn row_at(state: &TreeState, viewport: ViewportState, screen_row: u16) -> Option<usize> {
    if screen_row < viewport.list.y || screen_row >= viewport.list.bottom() {
        return None;
    }
    let index = viewport.scroll + usize::from(screen_row - viewport.list.y);
    (index < state.rows().len()).then_some(index)
}

fn selected_writable_set(state: &TreeState) -> Option<String> {
    let row = state.selected_row()?;
    match row.node.kind {
        tree::NodeKind::Set {
            name,
            observed: false,
        } => Some(name),
        _ => None,
    }
}

fn viewport(area: Rect, state: &TreeState, centered_on: Option<usize>) -> ViewportState {
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let list_height = inner.height.saturating_sub(2);
    let list = Rect {
        height: list_height,
        ..inner
    };
    let visible = usize::from(list_height.max(1));
    let row_count = state.rows().len();
    let max_scroll = row_count.saturating_sub(visible);
    let scroll = centered_on
        .map(|selected| selected.saturating_sub(visible / 2).min(max_scroll))
        .unwrap_or_else(|| {
            state
                .selected
                .saturating_add(1)
                .saturating_sub(visible)
                .min(max_scroll)
        });
    ViewportState { list, scroll }
}

fn draw(
    frame: &mut Frame,
    state: &TreeState,
    glyphs: Glyphs,
    mode: DrawMode<'_>,
    viewport: ViewportState,
) {
    let theme = Theme::new();
    let area = frame.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme.border_type())
        .border_set(if glyphs == Glyphs::ascii() {
            ASCII_BORDER
        } else {
            border::PLAIN
        })
        .border_style(theme.border())
        .title(" AIKit tree ")
        .title_alignment(Alignment::Left);
    frame.render_widget(block, area);

    let tree_glyphs = TreeGlyphs::for_glyphs(glyphs);
    let lines = tree::render_lines(state, tree_glyphs);
    let visible: Vec<Line> = lines
        .into_iter()
        .enumerate()
        .skip(viewport.scroll)
        .take(usize::from(viewport.list.height))
        .map(|(index, line)| {
            let style = if index == state.selected {
                theme.selected()
            } else {
                Style::default()
            };
            Line::from(Span::styled(line, style))
        })
        .collect();
    frame.render_widget(Paragraph::new(visible), viewport.list);

    let footer = Rect {
        y: viewport.list.bottom(),
        height: 1,
        ..viewport.list
    };
    if let Some(prompt) = mode.editing {
        let text = match prompt {
            EditPrompt::Create(input) => format!("new set: {input}"),
            EditPrompt::Rename { from, input } => format!("rename {from}: {input}"),
            EditPrompt::Delete { set } => format!("delete set {set}?"),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("[confirm] [cancel] ", theme.staged()),
                Span::raw(fold_text(glyphs, &text)),
            ])),
            footer,
        );
    } else if mode.help {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("[close] ", theme.staged()),
                Span::raw(fold_text(
                    glyphs,
                    "j/k move  l/h expand  Space stage  S apply  y/p/d set  a/r/D manage  zz center",
                )),
            ])),
            footer,
        );
    } else if mode.confirming {
        let prompt = fold_text(
            glyphs,
            mode.confirmation
                .map(|value| value.prompt.as_str())
                .unwrap_or("Apply?"),
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("[confirm] [cancel] ", theme.staged()),
                Span::raw(prompt),
            ])),
            footer,
        );
    } else {
        let controls = footer_controls(state)
            .into_iter()
            .map(|(label, _)| label)
            .collect::<Vec<_>>()
            .join(" ");
        let selection = fold_text(glyphs, &state.describe_selection());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(controls, theme.staged()),
                Span::raw("  "),
                Span::styled(selection, theme.dim()),
            ])),
            footer,
        );
    }
    let filter = Rect {
        y: footer.bottom(),
        height: 1,
        ..viewport.list
    };
    let prefix = if mode.filtering { "/ " } else { "filter /  " };
    let filter_text = if mode.editing.is_some() || mode.help {
        "Esc returns to the tree".to_string()
    } else if mode.confirming {
        mode.confirmation
            .map(|value| value.detail.clone())
            .unwrap_or_default()
    } else {
        state.filter.clone()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prefix, theme.accent()),
            Span::raw(fold_text(glyphs, &filter_text)),
        ])),
        filter,
    );
}

fn fold_text(glyphs: Glyphs, text: &str) -> String {
    if glyphs == Glyphs::ascii() {
        tree::ascii_fold(text)
    } else {
        text.to_string()
    }
}

fn draw_error(what: &str, error: impl std::fmt::Display) -> AikitError {
    AikitError::new("tui.render_failed", format!("{what}: {error}"))
}

/// Set up the real terminal, enable mouse reporting, and restore both on every
/// return path.
pub(crate) fn run_on_terminal(state: TreeState, request: TreeRequest) -> Result<TreeOutcome> {
    let io_error = |what: &str, error: io::Error| {
        AikitError::new("tui.terminal_setup_failed", format!("{what}: {error}"))
    };
    crossterm::terminal::enable_raw_mode()
        .map_err(|error| io_error("could not enter raw mode", error))?;
    let fullscreen = request.host == UiHost::Fullscreen;
    if fullscreen {
        if let Err(error) =
            crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)
        {
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(io_error("could not enter the alternate screen", error));
        }
    }
    if let Err(error) = crossterm::execute!(io::stdout(), EnableMouseCapture) {
        if fullscreen {
            let _ = crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        }
        let _ = crossterm::terminal::disable_raw_mode();
        return Err(io_error("could not enable mouse input", error));
    }

    let outcome = run_inner(state, request);

    let _ = crossterm::execute!(io::stdout(), DisableMouseCapture);
    if fullscreen {
        let _ = crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
    }
    let _ = crossterm::terminal::disable_raw_mode();
    outcome
}

fn run_inner(state: TreeState, request: TreeRequest) -> Result<TreeOutcome> {
    let backend = CrosstermBackend::new(io::stdout());
    let options = TerminalOptions {
        viewport: match request.host {
            UiHost::Inline(rows) => Viewport::Inline(rows),
            UiHost::TmuxPopup | UiHost::Fullscreen => Viewport::Fullscreen,
        },
    };
    let mut terminal = Terminal::with_options(backend, options)
        .map_err(|error| AikitError::new("tui.terminal_setup_failed", format!("{error}")))?;
    let mut events = CrosstermEvents::default();
    let outcome = event_loop(&mut terminal, &mut events, state, request);
    let _ = terminal.clear();
    let _ = terminal.show_cursor();
    outcome
}
