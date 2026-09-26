//! The resting World view's orientation and next steps (spec §1.5, §4.2).
//!
//! `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §1.5: "The default terminal view
//! must tell the user where they are and present a small set of meaningful
//! actions: Continue, Start Direct work, Start Factory work, Compose Agent,
//! Search/Explore, and Repair where needed." The Worlds pane already tells the
//! person where they are (`project_workspace_render::project_world_lines`);
//! what it lacked is the few meaningful actions.
//!
//! This module is that small set, as a pure function of the readings the
//! controller already holds. It introduces no new owner, no second action
//! registry and no per-render probing: every step either dispatches an
//! existing `UiAction` or names the specific owner operation that is not
//! bound at this application boundary. A disabled step is a named gap, never
//! a silently absent row and never a fabricated result — the same discipline
//! the compose spine's standings keep.
//!
//! Rendering and input stay in lockstep by construction: the surface renders
//! [`next_step_rows`] into a bottom-pinned block of the Worlds pane and
//! hit-tests clicks against the same rows in the same rect, the way
//! `navigator_groups` keeps `draw_resources` and mouse hit-testing on one
//! row plan.

use aikit_core::doctor_world::DoctorSeverity;
use aikit_core::credential_world::ProviderRosterKnowledge;
use aikit_core::ProjectWorldReadModel;
use ratatui::text::{Line, Span};

use crate::application::{PresentationMode, TuiState, UiAction, WorkspaceSection};
use crate::backend::FactoryWorkEntry;
use crate::layout::Glyphs;
use crate::project_workspace_render::WorkspaceReading;
use crate::theme::Theme;

/// One actionable next step on the resting World view (or the Compose
/// Enter-work step, which shares this vocabulary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextStep {
    /// The key that dispatches this step: `1`-based position, so the block
    /// never grows a second chord vocabulary. Mouse clicks resolve to the
    /// same `UiAction` as the key.
    pub key: char,
    pub label: String,
    /// What the step does when taken — the context-aware help sentence, not
    /// an implementation name.
    pub outcome: String,
    pub availability: StepAvailability,
    /// What taking the step dispatches. Explicit rather than derived from
    /// the label: the resting World view and the Enter-work step both have
    /// a step whose label is a verb like "Start", and the same words must
    /// never silently mean two different dispatches.
    pub action: StepAction,
}

impl NextStep {
    /// Whether taking the step now would dispatch anything.
    pub fn is_ready(&self) -> bool {
        matches!(self.availability, StepAvailability::Ready)
    }
}

/// The semantic dispatch a step resolves to. A small closed vocabulary —
/// every variant maps to one existing `UiAction` in [`step_action`], so
/// keyboard and mouse can never disagree about what a step does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAction {
    /// Open the Work destination (Continue).
    OpenWork,
    /// Open the Compose destination (Compose Agent).
    OpenCompose,
    /// Open the Universal Navigator (Search/Explore).
    OpenNavigator,
    /// Leave to the diff-first doctor repair flow (Repair).
    RequestDoctorFix,
    /// Save the composed Agent source.
    SaveAgent,
    /// Enter/resume Direct work (the compound, or Start/Continue on the
    /// stage ladder).
    StartDirectWork,
    /// Submit the configured Factory Commission.
    StartFactory,
}

/// Why a step can or cannot be taken right now. A disabled step always
/// carries the specific reason: which owner operation is missing, or which
/// authored input is still absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepAvailability {
    Ready,
    Disabled { reason: String },
}

/// Which native operations this application backend binds for the
/// Agent-work lifecycle. Read once at surface construction, like the glyph
/// capability — never probed per render. Every `false` is a named gap in the
/// owner operations Worker A is completing, not a UI decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentWorkBindings {
    pub save: bool,
    pub accept: bool,
    pub readiness: bool,
    pub prepare: bool,
    pub launch: bool,
}

impl AgentWorkBindings {
    /// No owner operation is bound. The honest default: every lifecycle
    /// step below save reads as unbound until a backend binds it.
    pub fn none() -> Self {
        Self {
            save: false,
            accept: false,
            readiness: false,
            prepare: false,
            launch: false,
        }
    }

    pub fn all_bound(&self) -> bool {
        self.save && self.accept && self.readiness && self.prepare && self.launch
    }

    /// The names of the lifecycle operations this boundary does not bind, as
    /// a disabled step's reason.
    pub fn unbound_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if !self.save {
            names.push("agent-profile save");
        }
        if !self.accept {
            names.push("agent-profile accept");
        }
        if !self.readiness {
            names.push("world readiness");
        }
        if !self.prepare {
            names.push("agent-session prepare");
        }
        if !self.launch {
            names.push("encounter launch");
        }
        names
    }
}

/// The resting World view's next steps, in spec §4.2's order: Continue,
/// Start Direct work, Start Factory work, Compose Agent, Search/Explore,
/// Repair where needed. Repair only occupies a row when the readings show
/// something needing repair — "where needed" is a rendering decision made
/// from evidence, not a permanent button.
pub fn world_next_steps(
    world: &ProjectWorldReadModel,
    factory: &FactoryWorkEntry,
    bindings: AgentWorkBindings,
) -> Vec<NextStep> {
    let mut steps = vec![
        NextStep {
            key: '1',
            label: "Continue".into(),
            outcome: "opens Work: the direct sessions and Factory work this world already has".into(),
            availability: StepAvailability::Ready,
            action: StepAction::OpenWork,
        },
        NextStep {
            key: '2',
            label: "Start Direct work".into(),
            outcome: "prepares a session for this world and starts the Agent body".into(),
            availability: direct_work_availability(bindings),
            action: StepAction::StartDirectWork,
        },
        NextStep {
            key: '3',
            label: "Start Factory work".into(),
            outcome:
                "submits the configured Commission through Factory's own owner operation; this does not execute it".into(),
            availability: match factory {
                FactoryWorkEntry::Ready => StepAvailability::Ready,
                FactoryWorkEntry::Unavailable { reason } => StepAvailability::Disabled {
                    reason: reason.clone(),
                },
            },
            action: StepAction::StartFactory,
        },
        NextStep {
            key: '4',
            label: "Compose Agent".into(),
            outcome: "opens Compose: purpose, repertoire and review over this world".into(),
            availability: StepAvailability::Ready,
            action: StepAction::OpenCompose,
        },
        NextStep {
            key: '5',
            label: "Search/Explore".into(),
            outcome: "opens the Universal Navigator over destinations, resources and actions".into(),
            availability: StepAvailability::Ready,
            action: StepAction::OpenNavigator,
        },
    ];
    if needs_repair(world) {
        steps.push(NextStep {
            key: '6',
            label: "Repair".into(),
            outcome: "leaves to the diff-first doctor repair flow on the restored terminal".into(),
            availability: StepAvailability::Ready,
            action: StepAction::RequestDoctorFix,
        });
    }
    steps
}

/// Why Start Direct work can or cannot be taken at this boundary.
fn direct_work_availability(bindings: AgentWorkBindings) -> StepAvailability {
    if bindings.all_bound() {
        return StepAvailability::Ready;
    }
    let unbound = bindings.unbound_names().join(", ");
    StepAvailability::Disabled {
        reason: format!(
            "this application boundary does not bind: {unbound} (owner operations pending)"
        ),
    }
}

/// Whether the readings show something needing repair. Every source is a
/// real reading the world already carries; nothing here probes or guesses.
fn needs_repair(world: &ProjectWorldReadModel) -> bool {
    if !world.warnings.is_empty() {
        return true;
    }
    if let Some(errors) = world.doctor.count(DoctorSeverity::Error) {
        if errors > 0 {
            return true;
        }
    }
    if let Some(warnings) = world.doctor.count(DoctorSeverity::Warning) {
        if warnings > 0 {
            return true;
        }
    }
    if let ProviderRosterKnowledge::Observed { .. } = world.credential_world.providers {
        if world
            .credential_world
            .credentials
            .values()
            .any(|status| !status.is_selected())
        {
            return true;
        }
    }
    false
}

/// The `UiAction` a ready step dispatches — the one mapping both the key
/// handler and the mouse hit-test resolve through, so keyboard and mouse can
/// never disagree about what a step does. A disabled step dispatches
/// nothing.
pub fn step_action(step: &NextStep) -> Option<UiAction> {
    if !step.is_ready() {
        return None;
    }
    let action = match step.action {
        StepAction::OpenWork => UiAction::SetWorkspaceSection(WorkspaceSection::Work),
        StepAction::OpenCompose => UiAction::SetWorkspaceSection(WorkspaceSection::Compose),
        StepAction::OpenNavigator => UiAction::SetPresentation(PresentationMode::Quick),
        StepAction::RequestDoctorFix => UiAction::RequestDoctorFix,
        StepAction::SaveAgent => UiAction::ComposeSaveAgent,
        StepAction::StartDirectWork => UiAction::ComposeStartDirectWork,
        StepAction::StartFactory => UiAction::StartFactoryWork,
    };
    Some(action)
}

/// The steps for whatever view the operator stands on — the resting World
/// view's set, or the Compose Enter-work primaries. One dispatcher so the
/// renderer, the key handler and the mouse hit-test can never disagree
/// about which steps are on screen.
pub fn steps_for_view(state: &TuiState, reading: &WorkspaceReading<'_>) -> Vec<NextStep> {
    if state.workspace_section == WorkspaceSection::Compose {
        return enter_work_steps(state, reading.factory_work_entry, reading.agent_work_bindings);
    }
    world_next_steps(
        reading.world,
        reading.factory_work_entry,
        reading.agent_work_bindings,
    )
}

/// The rendered rows of the next-steps block, one per step, in step order.
/// Disabled steps carry their reason inline: the row itself is the named
/// gap. Widths beyond `width` are clipped with the shell's own elision mark
/// so the block stays inside the pane at every breakpoint.
pub fn next_step_rows(steps: &[NextStep], width: usize, glyphs: Glyphs) -> Vec<String> {
    steps
        .iter()
        .map(|step| plain_step_row(step, width, glyphs))
        .collect()
}

fn plain_step_row(step: &NextStep, width: usize, glyphs: Glyphs) -> String {
    let mut row = format!("{}) {}", step.key, step.label);
    if let StepAvailability::Disabled { reason } = &step.availability {
        row.push_str(&format!("  unavailable: {reason}"));
    }
    crate::v2_render::clip(&row, width, glyphs)
}

/// The styled rows the shell renders: ready steps in the base style,
/// disabled steps dimmed — the same words, a quieter voice, so an
/// unavailable capability stays readable exactly where it stands.
pub fn next_step_lines(steps: &[NextStep], width: usize, glyphs: Glyphs) -> Vec<Line<'static>> {
    let theme = Theme::new();
    steps
        .iter()
        .map(|step| {
            let row = plain_step_row(step, width, glyphs);
            Line::from(Span::styled(
                row,
                if step.is_ready() {
                    theme.base()
                } else {
                    theme.dim()
                },
            ))
        })
        .collect()
}

/// The rect the next-steps block occupies, pinned to the bottom of the
/// world pane. Both the renderer and the mouse hit-test derive the block
/// from this one function, so a click can never land one row away from the
/// row that was drawn. A zero-row block is a zero-height rect: nothing is
/// drawn and nothing is clickable.
pub fn bottom_block(pane: ratatui::layout::Rect, rows: usize) -> ratatui::layout::Rect {
    let height = u16::try_from(rows).unwrap_or(u16::MAX).min(pane.height);
    ratatui::layout::Rect {
        x: pane.x,
        y: pane.y.saturating_add(pane.height.saturating_sub(height)),
        width: pane.width,
        height,
    }
}

/// The Compose Enter-work step's primary actions (spec §1.3): Save Agent,
/// Save and start Direct work, Start Factory work, and Start/Continue for an
/// already-accepted Agent — in that order. Five coequal implementation-step
/// buttons are deliberately not reproduced; the compound is listed once and
/// its component effects are named in the step's review detail.
pub fn enter_work_steps(
    state: &TuiState,
    factory: &FactoryWorkEntry,
    bindings: AgentWorkBindings,
) -> Vec<NextStep> {
    let purpose_authored = !state.compose_purpose.trim().is_empty();
    let mut steps = Vec::new();

    steps.push(NextStep {
        key: '1',
        label: "Save Agent".into(),
        outcome: "saves the authored profile source through Central; saved is not accepted".into(),
        availability: step_availability(
            (!purpose_authored).then_some("an exact purpose must be authored first"),
            bindings.save,
            "Central agent-profile save",
        ),
        action: StepAction::SaveAgent,
    });
    steps.push(NextStep {
        key: '2',
        label: "Save and start Direct work".into(),
        outcome: "save, then accept, then readiness, then prepare, then launch - each stage named as it lands".into(),
        availability: if !purpose_authored {
            StepAvailability::Disabled {
                reason: "an exact purpose must be authored first".into(),
            }
        } else if bindings.all_bound() {
            StepAvailability::Ready
        } else {
            StepAvailability::Disabled {
                reason: format!(
                    "this application boundary does not bind: {} (owner operations pending)",
                    bindings.unbound_names().join(", ")
                ),
            }
        },
        action: StepAction::StartDirectWork,
    });

    steps.push(NextStep {
        key: '3',
        label: "Start Factory work".into(),
        outcome: "submits the configured Commission through Factory's own owner operation; developmental work is a separate choice".into(),
        availability: match factory {
            FactoryWorkEntry::Ready => StepAvailability::Ready,
            FactoryWorkEntry::Unavailable { reason } => StepAvailability::Disabled {
                reason: reason.clone(),
            },
        },
        action: StepAction::StartFactory,
    });

    steps.push(NextStep {
        key: '4',
        label: start_label(&state.agent_work),
        outcome: start_outcome(&state.agent_work),
        availability: start_availability(&state.agent_work, bindings),
        action: StepAction::StartDirectWork,
    });

    steps
}

fn step_availability(
    authored_block: Option<&str>,
    bound: bool,
    operation: &'static str,
) -> StepAvailability {
    if let Some(reason) = authored_block {
        return StepAvailability::Disabled {
            reason: reason.to_string(),
        };
    }
    if bound {
        StepAvailability::Ready
    } else {
        StepAvailability::Disabled {
            reason: format!("this application boundary does not bind {operation} (owner operation pending)"),
        }
    }
}

fn start_label(stage: &crate::application::AgentWorkStage) -> String {
    match stage {
        crate::application::AgentWorkStage::Accepted { .. } => "Start".into(),
        crate::application::AgentWorkStage::Prepared { .. } => "Start".into(),
        crate::application::AgentWorkStage::Running { .. } => "Continue".into(),
        _ => "Start".into(),
    }
}

fn start_outcome(stage: &crate::application::AgentWorkStage) -> String {
    match stage {
        crate::application::AgentWorkStage::Accepted { .. } => {
            "prepares a session for the accepted Agent and starts its body".into()
        }
        crate::application::AgentWorkStage::Prepared { agent_session, .. } => {
            format!("launches the prepared session {agent_session}")
        }
        crate::application::AgentWorkStage::Running { agent_session } => {
            format!("returns to the running session {agent_session}")
        }
        crate::application::AgentWorkStage::Saved { .. } => {
            "needs acceptance first: saved is not accepted".into()
        }
        crate::application::AgentWorkStage::Failed { .. } => {
            "resumes the failed stage only; earlier stages stand".into()
        }
        crate::application::AgentWorkStage::Draft => {
            "needs an accepted Agent: nothing is saved yet".into()
        }
    }
}

fn start_availability(
    stage: &crate::application::AgentWorkStage,
    bindings: AgentWorkBindings,
) -> StepAvailability {
    use crate::application::AgentWorkStage as S;
    match stage {
        S::Accepted { .. } if bindings.prepare && bindings.launch => StepAvailability::Ready,
        S::Accepted { .. } => StepAvailability::Disabled {
            reason: "this application boundary does not bind prepare/launch (owner operations pending)".into(),
        },
        S::Prepared { .. } if bindings.launch => StepAvailability::Ready,
        S::Prepared { .. } => StepAvailability::Disabled {
            reason: "this application boundary does not bind encounter launch (owner operation pending)".into(),
        },
        S::Running { .. } => StepAvailability::Ready,
        S::Saved { .. } => StepAvailability::Disabled {
            reason: "saved is not accepted; accept the reviewed source first".into(),
        },
        S::Failed { failed, .. } => StepAvailability::Disabled {
            reason: format!(
                "stage {} failed; retry it from the same action, earlier stages stand",
                failed.as_str()
            ),
        },
        S::Draft => StepAvailability::Disabled {
            reason: "no Agent is saved yet".into(),
        },
    }
}

/// Context-aware help (`?`): what this view is for, what its keys do here,
/// and — where the next-steps block is drawn — what each step's outcome is.
/// Help explains outcomes, not implementation names: every sentence says
/// what the operator gets, in the same words the rows themselves use.
pub fn help_lines(
    state: &TuiState,
    reading: Option<&WorkspaceReading<'_>>,
    glyphs: Glyphs,
) -> Vec<String> {
    let sep = glyphs.separator();
    let mode = match state.presentation {
        PresentationMode::Quick => "Quick",
        PresentationMode::Workspace => "Workspace",
    };
    let mut lines = vec![
        format!(
            "Help {sep} {mode} {sep} {}",
            crate::project_workspace_render::workspace_section_label(state.workspace_section)
        ),
        String::new(),
    ];

    match (state.presentation, state.workspace_section) {
        (PresentationMode::Workspace, WorkspaceSection::Worlds) => {
            lines.push(
                "The resting World view: where you are, what this world resolves to, \
                 and what you can do next."
                    .into(),
            );
        }
        (PresentationMode::Workspace, WorkspaceSection::Compose) => {
            if state.compose_step == crate::compose_spine::ComposeStep::EnterWork {
                lines.push(
                    "The Enter-work step: author the exact purpose, review, then take \
                     one primary action. Saved is not accepted; accepted is not running."
                        .into(),
                );
            } else {
                lines.push(
                    "The composition spine: walk the steps with Alt+Up/Alt+Down; the \
                     step in hand shows what stands behind it."
                        .into(),
                );
            }
        }
        (PresentationMode::Workspace, WorkspaceSection::Work) => {
            lines.push(
                "Active work: the direct sessions and Factory work this world \
                 already has, each named by its own native subject."
                    .into(),
            );
        }
        _ => {
            lines.push(
                "Search finds destinations, resources and actions alike; Enter opens \
                 the selection, : lists its Actions."
                    .into(),
            );
        }
    }
    lines.push(String::new());

    if steps_drawn_here(state) {
        if let Some(reading) = reading {
            lines.push("Next steps here".into());
            for step in steps_for_view(state, reading) {
                let mut row = format!("{}) {} - {}", step.key, step.label, step.outcome);
                if let StepAvailability::Disabled { reason } = &step.availability {
                    row.push_str(&format!(" (unavailable: {reason})"));
                }
                lines.push(crate::v2_render::clip(&row, 200, glyphs));
            }
            lines.push(String::new());
        }
    }
    if state.workspace_section == WorkspaceSection::Compose
        && state.compose_step == crate::compose_spine::ComposeStep::EnterWork
        && state.presentation == PresentationMode::Workspace
    {
        lines.push("Enter authors the purpose; Tab moves to the optional name".into());
    }
    lines.push(format!(
        "Ctrl+K Universal Navigator {sep} Ctrl+S preview/apply {sep} : Actions"
    ));
    lines.push(format!(
        "Alt+{} switch Workspace fields {sep} Ctrl+W toggle Quick/Workspace",
        glyphs.horizontal_keys(),
    ));
    lines.push("Esc return {sep} Ctrl+Q exit".to_string());
    lines
}

/// Whether the next-steps block is drawn — and its digit keys live — where
/// the operator stands: the resting World view and the Compose Enter-work
/// step. An open overlay is modal, so the block yields. Everywhere else the
/// digits keep their ordinary meaning (typing into the query).
pub fn steps_active_here(state: &TuiState) -> bool {
    state.overlay.is_none() && steps_drawn_here(state)
}

/// The view-level half of [`steps_active_here`]: where the block belongs,
/// ignoring modality. Help is rendered while an overlay is open, and its
/// step explanations describe exactly this set, so help reads from here.
pub fn steps_drawn_here(state: &TuiState) -> bool {
    state.presentation == PresentationMode::Workspace
        && state.query.is_empty()
        && state.action_query.is_none()
        && (state.workspace_section == WorkspaceSection::Worlds
            || (state.workspace_section == WorkspaceSection::Compose
                && state.compose_step == crate::compose_spine::ComposeStep::EnterWork))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::FactoryWorkEntry;
    use aikit_core::context::ContextDescriptor;
    use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};

    fn world() -> ProjectWorldReadModel {
        let context = ContextDescriptor::for_project("/work/aikit");
        ProjectWorldReadModel::empty(
            ProjectBinding::from_legacy_context(
                ProjectRef::parse("project:aikit").unwrap(),
                ProjectConstituentRef::parse("source:working-tree").unwrap(),
                &context,
            )
            .unwrap(),
            context,
        )
    }

    fn ready_factory() -> FactoryWorkEntry {
        FactoryWorkEntry::Ready
    }

    fn unbound() -> AgentWorkBindings {
        AgentWorkBindings::none()
    }

    /// Spec §4.2's set, in order, with Continue/Search/Compose always ready.
    #[test]
    fn the_resting_view_offers_the_spec_actions_in_order() {
        let steps = world_next_steps(&world(), &ready_factory(), unbound());
        let labels: Vec<&str> = steps.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Continue",
                "Start Direct work",
                "Start Factory work",
                "Compose Agent",
                "Search/Explore",
            ]
        );
        for (index, step) in steps.iter().enumerate() {
            assert_eq!(step.key, char::from_digit(index as u32 + 1, 10).unwrap());
        }
        assert!(steps[0].is_ready());
        assert!(steps[3].is_ready());
        assert!(steps[4].is_ready());
    }

    /// The Direct-work step names the exact unbound owner operations rather
    /// than reading as a refusal or vanishing.
    #[test]
    fn direct_work_names_the_unbound_owner_operations() {
        let steps = world_next_steps(&world(), &ready_factory(), unbound());
        let StepAvailability::Disabled { reason } = &steps[1].availability else {
            panic!("with nothing bound, direct work must be disabled");
        };
        assert!(reason.contains("agent-profile save"), "{reason}");
        assert!(reason.contains("encounter launch"), "{reason}");
        assert!(reason.contains("owner operations pending"), "{reason}");
    }

    #[test]
    fn fully_bound_operations_make_direct_work_ready() {
        let bindings = AgentWorkBindings {
            save: true,
            accept: true,
            readiness: true,
            prepare: true,
            launch: true,
        };
        let steps = world_next_steps(&world(), &ready_factory(), bindings);
        assert!(steps[1].is_ready());
    }

    /// An unavailable Factory binding keeps its row and carries the
    /// boundary's own reason — never silently dropped, never collapsed into
    /// a generic "unavailable".
    #[test]
    fn factory_unavailable_keeps_its_row_and_names_the_reason() {
        let factory = FactoryWorkEntry::Unavailable {
            reason: "no Factory Commission binding supplied to this application".into(),
        };
        let steps = world_next_steps(&world(), &factory, unbound());
        let StepAvailability::Disabled { reason } = &steps[2].availability else {
            panic!("an unbound Factory entry must be disabled");
        };
        assert_eq!(
            reason,
            "no Factory Commission binding supplied to this application"
        );
        let rows = next_step_rows(&steps, 200, Glyphs::ascii());
        assert!(rows[2].contains("unavailable: no Factory Commission binding"),);
    }

    /// "Repair where needed": no warnings, no doctor findings and no unmet
    /// credentials means no Repair row at all.
    #[test]
    fn repair_only_appears_when_a_reading_shows_a_need() {
        let steps = world_next_steps(&world(), &ready_factory(), unbound());
        assert!(steps.iter().all(|step| step.label != "Repair"));
    }

    #[test]
    fn a_world_warning_calls_for_repair() {
        let mut world = world();
        world.warnings.push("one gateway Surface degraded".into());
        let steps = world_next_steps(&world, &ready_factory(), unbound());
        let repair = steps
            .iter()
            .find(|step| step.label == "Repair")
            .expect("a warned world needs repair");
        assert!(repair.is_ready());
        assert_eq!(
            step_action(repair),
            Some(UiAction::RequestDoctorFix),
            "Repair rides the existing diff-first doctor repair flow"
        );
    }

    /// Every ready step resolves to an existing UiAction, and a disabled
    /// step never does.
    #[test]
    fn ready_steps_map_to_existing_actions_and_disabled_steps_map_to_none() {
        let steps = world_next_steps(&world(), &ready_factory(), unbound());
        for step in &steps {
            if step.is_ready() {
                assert!(step_action(step).is_some(), "{} must dispatch", step.label);
            } else {
                assert_eq!(step_action(step), None);
            }
        }
        assert_eq!(
            step_action(&steps[0]),
            Some(UiAction::SetWorkspaceSection(WorkspaceSection::Work))
        );
        assert_eq!(
            step_action(&steps[3]),
            Some(UiAction::SetWorkspaceSection(WorkspaceSection::Compose))
        );
        assert_eq!(
            step_action(&steps[4]),
            Some(UiAction::SetPresentation(PresentationMode::Quick))
        );
    }

    /// Rows stay inside narrow panes and stay ASCII under ASCII glyphs.
    #[test]
    fn rows_clip_to_width_and_stay_ascii_under_ascii_glyphs() {
        let steps = world_next_steps(&world(), &ready_factory(), unbound());
        for width in [20, 40, 80, 200] {
            let rows = next_step_rows(&steps, width, Glyphs::ascii());
            assert_eq!(rows.len(), steps.len());
            for row in &rows {
                assert!(
                    row.chars().count() <= width,
                    "row {row:?} exceeds width {width}"
                );
                assert!(row.is_ascii(), "non-ASCII under ASCII glyphs: {row}");
            }
        }
    }

    /// The Enter-work primary actions: the compound is one action, disabled
    /// with the exact unbound operations until the owner bindings land.
    #[test]
    fn enter_work_offers_the_four_primary_actions_with_honest_standings() {
        let state = TuiState {
            compose_purpose: "Prove the command-encounter slice".into(),
            ..TuiState::default()
        };
        let steps = enter_work_steps(&state, &ready_factory(), unbound());
        let labels: Vec<&str> = steps.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Save Agent",
                "Save and start Direct work",
                "Start Factory work",
                "Start",
            ]
        );
        // Save is authored-ready but unbound at this boundary.
        let StepAvailability::Disabled { reason } = &steps[0].availability else {
            panic!("save without the owner binding must be disabled");
        };
        assert!(reason.contains("agent-profile save"), "{reason}");
        // The compound names every unbound stage.
        let StepAvailability::Disabled { reason } = &steps[1].availability else {
            panic!("the compound without bindings must be disabled");
        };
        assert!(reason.contains("world readiness"), "{reason}");
        // Factory is real today where the binding is ready.
        assert!(steps[2].is_ready());
        // Start without an accepted Agent says what is missing.
        let StepAvailability::Disabled { reason } = &steps[3].availability else {
            panic!("Start without an accepted Agent must be disabled");
        };
        assert!(reason.contains("no Agent is saved yet"), "{reason}");
    }

    /// Without an authored purpose both save actions refuse for the same
    /// named reason — the purpose is the one input the person must author.
    #[test]
    fn save_actions_demand_an_authored_purpose_first() {
        let state = TuiState::default();
        let steps = enter_work_steps(&state, &ready_factory(), unbound());
        for step in &steps[..2] {
            let StepAvailability::Disabled { reason } = &step.availability else {
                panic!("{} must be disabled without a purpose", step.label);
            };
            assert!(reason.contains("purpose must be authored"), "{reason}");
        }
    }

    /// An accepted Agent flips Start to ready (with bindings) and a running
    /// session becomes Continue.
    #[test]
    fn start_and_continue_follow_the_stage_ladder() {
        let bindings = AgentWorkBindings {
            save: true,
            accept: true,
            readiness: true,
            prepare: true,
            launch: true,
        };
        let accepted = TuiState {
            agent_work: crate::application::AgentWorkStage::Accepted {
                profile_ref: "agent-profile/probe".into(),
                revision: "r1".into(),
                content_digest: "d1".into(),
            },
            ..TuiState::default()
        };
        let steps = enter_work_steps(&accepted, &ready_factory(), bindings);
        assert_eq!(steps[3].label, "Start");
        assert!(steps[3].is_ready());

        let running = TuiState {
            agent_work: crate::application::AgentWorkStage::Running {
                agent_session: "agent-session/live".into(),
            },
            ..TuiState::default()
        };
        let steps = enter_work_steps(&running, &ready_factory(), bindings);
        assert_eq!(steps[3].label, "Continue");
        assert!(steps[3].is_ready());
    }

    /// The steps are active only where they are drawn: the resting Worlds
    /// view and Compose's Enter-work step. Everywhere else digits keep
    /// meaning query typing.
    #[test]
    fn step_keys_are_active_only_on_the_views_that_draw_them() {
        let state = |workspace_section, compose_step, query, overlay| TuiState {
            presentation: PresentationMode::Workspace,
            workspace_section,
            compose_step,
            query,
            overlay,
            ..TuiState::default()
        };
        assert!(steps_active_here(&state(
            WorkspaceSection::Worlds,
            crate::compose_spine::ComposeStep::default(),
            String::new(),
            None,
        )));

        assert!(
            !steps_active_here(&state(
                WorkspaceSection::Compose,
                crate::compose_spine::ComposeStep::default(),
                String::new(),
                None,
            )),
            "plain Compose is not Enter-work"
        );
        assert!(
            steps_active_here(&state(
                WorkspaceSection::Compose,
                crate::compose_spine::ComposeStep::EnterWork,
                String::new(),
                None,
            )),
            "Enter-work carries the block"
        );

        assert!(
            !steps_active_here(&state(
                WorkspaceSection::Compose,
                crate::compose_spine::ComposeStep::EnterWork,
                "x".into(),
                None,
            )),
            "a live query owns the digits"
        );

        assert!(
            !steps_active_here(&state(
                WorkspaceSection::Worlds,
                crate::compose_spine::ComposeStep::default(),
                String::new(),
                Some(crate::application::Overlay::Help),
            )),
            "an open overlay is modal"
        );

        assert!(
            !steps_active_here(&TuiState {
                presentation: PresentationMode::Quick,
                workspace_section: WorkspaceSection::Worlds,
                ..TuiState::default()
            }),
            "Quick is the Navigator"
        );
    }
}
