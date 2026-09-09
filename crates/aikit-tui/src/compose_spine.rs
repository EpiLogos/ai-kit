//! §5.1's human composition spine, as the actual structure of Compose.
//!
//! Spec `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §5.1 names ten ordered steps
//! from a person's intention to entering work:
//!
//! ```text
//! Intention -> Identity/expression -> Governance -> Praxis -> Information
//! -> Worlds & bounds -> Runtime -> Workspace/continuity -> Preview -> Enter work
//! ```
//!
//! Compose previously rendered four read-model horizon counts
//! (`Capabilities 12 · 8 actions`). That says how much resolved, not how far a
//! person has got composing. The horizons are still the right grouping *of the
//! read model* — [`crate::project_workspace::ComposeHorizon`] keeps that job —
//! but they are not the spine, and a person cannot read their own progress off
//! them. This module is the spine.
//!
//! Every step reports one of three standings, and the difference between the
//! last two is the whole point:
//!
//! - **determined** — this step resolved to something, and the row says what;
//! - **open** — the step is available and nothing has been determined yet;
//! - **not exposed** — this application boundary publishes no contract for the
//!   step at all, so no answer is possible here in either direction.
//!
//! Collapsing "not exposed" into "open" would tell a person to go and choose
//! something they cannot choose; collapsing it into "determined" would be a
//! fabrication. Two steps are genuinely `NotExposed` on this boundary today:
//!
//! - **Praxis** — `aikit_core::praxis` exists (`resolve_praxis`,
//!   `PraxisResolution`, `SelectedMethod`) but nothing of it crosses the TUI
//!   application boundary: `ApplicationService` publishes search, relations,
//!   knowledge, preview/apply, explain, history and contextual actions, and no
//!   praxis contract among them. This row must not imply a Profile/SkillSet/
//!   Skill/Method picker exists.
//! - **Intention** — no contract captures a natural-language intention at this
//!   boundary.
//!
//! `Workspace / continuity` is deliberately *not* in that list: the boundary
//! does publish `SessionSpaceApplicationProjection`, so that step is `Open`
//! rather than `NotExposed` — a real difference, and the reason the two
//! standings are separate values rather than one "unavailable".

use aikit_core::context_resolution::Availability;
use aikit_core::credential_world::ProviderRosterKnowledge;
use aikit_core::ProjectWorldReadModel;

use crate::application::TuiState;
use crate::layout::Glyphs;

/// The ten §5.1 steps, in spec order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeStep {
    Intention,
    Identity,
    Governance,
    Praxis,
    Information,
    WorldsAndBounds,
    Runtime,
    Continuity,
    Preview,
    EnterWork,
}

impl ComposeStep {
    pub const ALL: [Self; 10] = [
        Self::Intention,
        Self::Identity,
        Self::Governance,
        Self::Praxis,
        Self::Information,
        Self::WorldsAndBounds,
        Self::Runtime,
        Self::Continuity,
        Self::Preview,
        Self::EnterWork,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intention => "Intention",
            Self::Identity => "Identity",
            Self::Governance => "Governance",
            Self::Praxis => "Praxis",
            Self::Information => "Information",
            Self::WorldsAndBounds => "Worlds/bounds",
            Self::Runtime => "Runtime",
            Self::Continuity => "Continuity",
            Self::Preview => "Preview",
            Self::EnterWork => "Enter work",
        }
    }
}

/// What a step currently stands at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStanding {
    /// Resolved to something; the detail says what.
    Determined(String),
    /// Available to determine, nothing determined yet.
    Open(String),
    /// This application boundary publishes no contract for the step. The
    /// reason names what is missing, so the row is actionable as a gap rather
    /// than read as a refusal.
    NotExposed(String),
}

impl StepStanding {
    fn detail(&self) -> &str {
        match self {
            Self::Determined(detail) | Self::Open(detail) => detail,
            Self::NotExposed(reason) => reason,
        }
    }

    /// Single-glyph standing marker. ASCII-safe: `Glyphs` decides.
    fn marker(&self, glyphs: Glyphs) -> char {
        match self {
            Self::Determined(_) => glyphs.step_determined(),
            Self::Open(_) => glyphs.step_open(),
            Self::NotExposed(_) => glyphs.step_not_exposed(),
        }
    }
}

/// One spine row: the step, where it stands, and the detail behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeSpineRow {
    pub step: ComposeStep,
    pub standing: StepStanding,
}

/// The whole spine for a resolved world, in spec order. Always ten rows: a
/// step that cannot be answered still occupies its place, because a spine with
/// gaps silently renumbers a person's sense of where they are.
pub fn compose_spine(state: &TuiState, world: &ProjectWorldReadModel) -> Vec<ComposeSpineRow> {
    ComposeStep::ALL
        .into_iter()
        .map(|step| ComposeSpineRow {
            step,
            standing: standing_for(step, state, world),
        })
        .collect()
}

fn standing_for(step: ComposeStep, state: &TuiState, world: &ProjectWorldReadModel) -> StepStanding {
    match step {
        ComposeStep::Intention => StepStanding::NotExposed(
            "no intention contract at this application boundary".into(),
        ),

        ComposeStep::Identity => match (
            world.actor_runtime.agent.effective.as_ref(),
            world.actor_runtime.agent.requested.as_ref(),
        ) {
            (Some(agent), _) => StepStanding::Determined(agent.resource.as_str().to_string()),
            // A requested agent that did not resolve is not a determined
            // identity, and saying so is more useful than either extreme.
            (None, Some(requested)) => {
                StepStanding::Open(format!("{requested} requested, not resolved"))
            }
            (None, None) => StepStanding::Open("no Agent requested".into()),
        },

        ComposeStep::Governance => {
            let profiles = world.resolution_basis.profiles.len();
            let scopes = world.resolution_basis.scopes.len();
            let detail = format!("{profiles} profile{}, {scopes} scope{}", s(profiles), s(scopes));
            if profiles == 0 && scopes == 0 {
                StepStanding::Open("no authored profile or scope resolved".into())
            } else {
                StepStanding::Determined(detail)
            }
        }

        // `aikit_core::praxis` is real; its absence here is a boundary fact,
        // not a claim that praxis does not exist in the product. Capabilities
        // and Actions — §5.1's "where relevant" tail of this step — *do*
        // resolve, so the row says what it has rather than reading as a total
        // blank on a step that is partly answerable.
        ComposeStep::Praxis => StepStanding::NotExposed(format!(
            "no Profile/SkillSet/Skill/Method contract here; {} capabilit{}, {} action{} resolve",
            world.capability_horizon.capabilities.len(),
            if world.capability_horizon.capabilities.len() == 1 { "y" } else { "ies" },
            world.capability_horizon.actions.len(),
            s(world.capability_horizon.actions.len()),
        )),

        ComposeStep::Information => {
            let eligible = world
                .information_horizon
                .sources
                .iter()
                .filter(|source| source.eligibility.is_eligible())
                .count();
            let planned = world.information_horizon.planned_retrieval.len();
            if eligible == 0 && planned == 0 {
                StepStanding::Open("no eligible ContextSource in this horizon".into())
            } else {
                StepStanding::Determined(format!(
                    "{eligible} eligible source{}, {planned} planned retrieval{}",
                    s(eligible),
                    s(planned)
                ))
            }
        }

        ComposeStep::WorldsAndBounds => {
            let mut detail = world.project.project.to_string();
            if let Some(versioned) = world.versioned_world.as_ref() {
                let branch = versioned
                    .repository
                    .branch
                    .as_deref()
                    .unwrap_or(if versioned.repository.detached { "detached" } else { "unnamed" });
                detail.push_str(&format!(", material {branch}"));
            }
            detail.push_str(&format!(
                ", {} projection target{}, {} effective capabilit{}",
                world.projection.targets.len(),
                s(world.projection.targets.len()),
                world.projection.active_capabilities.len(),
                if world.projection.active_capabilities.len() == 1 { "y" } else { "ies" },
            ));
            StepStanding::Determined(detail)
        }

        ComposeStep::Runtime => {
            let runtime = &world.actor_runtime;
            let bodies = runtime.harnesses.len() + runtime.models.len();
            if bodies == 0 {
                return StepStanding::Open("no Harness or model candidate resolved".into());
            }
            let ready = runtime
                .harnesses
                .iter()
                .chain(runtime.models.iter())
                .filter(|resource| matches!(resource.effective.availability, Availability::Available))
                .count();
            let mut detail = format!(
                "{} harness{}, {} model{}, {ready} available",
                runtime.harnesses.len(),
                if runtime.harnesses.len() == 1 { "" } else { "es" },
                runtime.models.len(),
                s(runtime.models.len()),
            );
            // A body that cannot get its credentials is not a determined
            // runtime, so the credential world travels with this step.
            if let ProviderRosterKnowledge::Observed { .. } = world.credential_world.providers {
                let unmet = world
                    .credential_world
                    .credentials
                    .values()
                    .filter(|status| !status.is_selected())
                    .count();
                if unmet > 0 {
                    detail.push_str(&format!(", {unmet} credential{} unmet", s(unmet)));
                }
            }
            StepStanding::Determined(detail)
        }

        // The boundary does publish `SessionSpaceApplicationProjection`, so
        // this is an open choice rather than an unexposed one.
        ComposeStep::Continuity => {
            StepStanding::Open("SessionSpace selectable, none bound to this reading".into())
        }

        ComposeStep::Preview => StepStanding::Open(format!(
            "Ctrl+S previews {} staged change{}",
            state.staged.len(),
            s(state.staged.len())
        )),

        // §6.2's Factory route needs the Run/Journey status contract #227
        // records as absent; the direct route needs a session-start contract
        // this boundary does not publish either.
        ComposeStep::EnterWork => StepStanding::NotExposed(
            "no session-start or Factory Run contract at this application boundary".into(),
        ),
    }
}

/// The Compose pane's spine block.
pub fn compose_spine_lines(
    state: &TuiState,
    world: &ProjectWorldReadModel,
    glyphs: Glyphs,
) -> Vec<String> {
    let sep = glyphs.separator();
    let rows = compose_spine(state, world);
    let determined = rows
        .iter()
        .filter(|row| matches!(row.standing, StepStanding::Determined(_)))
        .count();
    let exposed = rows
        .iter()
        .filter(|row| !matches!(row.standing, StepStanding::NotExposed(_)))
        .count();

    // Progress is stated against the steps a person can actually act on,
    // never against all ten — otherwise an unexposed step reads as work the
    // person has failed to do. It rides the heading line rather than taking a
    // row of its own: the spine shares this pane with the read-model horizons
    // and the selected Resource's detail, and a row spent on chrome is a row
    // of someone's actual composition pushed off the pane.
    let mut lines = vec![
        format!(
            "Compose {sep} intention to operative world {sep} {determined}/{exposed} determined",
        ),
        String::new(),
    ];
    for (index, row) in rows.iter().enumerate() {
        lines.push(format!(
            "{:>2} {} {:<14}{}",
            index + 1,
            row.standing.marker(glyphs),
            row.step.as_str(),
            row.standing.detail(),
        ));
    }
    lines
}

fn s(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use aikit_core::context::ContextDescriptor;
    use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
    use aikit_core::resource::ResourceRef;

    use super::*;

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

    fn row(rows: &[ComposeSpineRow], step: ComposeStep) -> &ComposeSpineRow {
        rows.iter().find(|row| row.step == step).expect("a row per step")
    }

    /// A spine with gaps silently renumbers a person's sense of where they
    /// are, so every step keeps its place whatever its standing.
    #[test]
    fn the_spine_is_always_ten_steps_in_spec_order() {
        let rows = compose_spine(&TuiState::default(), &world());
        assert_eq!(rows.len(), 10);
        assert_eq!(
            rows.iter().map(|row| row.step).collect::<Vec<_>>(),
            ComposeStep::ALL.to_vec()
        );
    }

    /// The distinction the whole module exists for. `Open` sends a person to
    /// make a choice; `NotExposed` says no contract publishes that choice
    /// here. Drawing them alike would send someone after a picker that does
    /// not exist.
    #[test]
    fn open_and_not_exposed_are_different_standings_with_different_marks() {
        let rows = compose_spine(&TuiState::default(), &world());
        let praxis = &row(&rows, ComposeStep::Praxis).standing;
        let continuity = &row(&rows, ComposeStep::Continuity).standing;

        assert!(matches!(praxis, StepStanding::NotExposed(_)));
        assert!(matches!(continuity, StepStanding::Open(_)));
        assert_ne!(
            praxis.marker(Glyphs::unicode()),
            continuity.marker(Glyphs::unicode())
        );
        assert_ne!(
            praxis.marker(Glyphs::ascii()),
            continuity.marker(Glyphs::ascii())
        );
    }

    /// `aikit_core::praxis` is real (`resolve_praxis`, `PraxisResolution`);
    /// what is absent is any praxis contract on the TUI's application
    /// boundary. The row must say the second thing, and must not imply a
    /// Profile/SkillSet/Skill/Method picker exists.
    #[test]
    fn praxis_names_the_missing_boundary_contract_not_a_missing_feature() {
        let rows = compose_spine(&TuiState::default(), &world());
        let StepStanding::NotExposed(detail) = &row(&rows, ComposeStep::Praxis).standing else {
            panic!("Praxis has no application-boundary contract and must read as NotExposed");
        };
        assert!(detail.contains("no Profile/SkillSet/Skill/Method contract here"));
    }

    /// Praxis is only partly unanswerable: §5.1's "Capabilities / Actions
    /// where relevant" tail does resolve, and is named rather than lost with
    /// the part that does not.
    #[test]
    fn an_unexposed_step_still_reports_the_part_of_it_that_does_resolve() {
        let mut world = world();
        world.capability_horizon.actions = Vec::new();
        let rows = compose_spine(&TuiState::default(), &world);
        let detail = row(&rows, ComposeStep::Praxis).standing.detail().to_string();
        assert!(detail.contains("0 capabilities"));
        assert!(detail.contains("0 actions"));
    }

    /// The boundary publishes `SessionSpaceApplicationProjection`, so
    /// continuity is a real choice — not an unexposed one. If this ever flips
    /// to `NotExposed` the spine has started lying about what exists.
    #[test]
    fn continuity_is_open_because_the_boundary_publishes_session_spaces() {
        let rows = compose_spine(&TuiState::default(), &world());
        assert!(matches!(
            row(&rows, ComposeStep::Continuity).standing,
            StepStanding::Open(_)
        ));
    }

    /// A requested-but-unresolved Agent is not a determined identity, and is
    /// not "no Agent requested" either.
    #[test]
    fn a_requested_agent_that_did_not_resolve_is_open_and_says_which_agent() {
        let mut world = world();
        world.actor_runtime.agent.requested = Some(ResourceRef::parse("agent:researcher").unwrap());
        let rows = compose_spine(&TuiState::default(), &world);
        let StepStanding::Open(detail) = &row(&rows, ComposeStep::Identity).standing else {
            panic!("an unresolved request is not a determined identity");
        };
        assert!(detail.contains("agent:researcher"));
        assert!(detail.contains("not resolved"));
    }

    /// Progress counts only the steps a person can act on. Counting against
    /// all ten would report unexposed steps as work they had failed to do.
    #[test]
    fn progress_is_counted_against_available_steps_not_all_ten() {
        let lines = compose_spine_lines(&TuiState::default(), &world(), Glyphs::unicode());
        let heading = &lines[0];
        let rows = compose_spine(&TuiState::default(), &world());
        let exposed = rows
            .iter()
            .filter(|row| !matches!(row.standing, StepStanding::NotExposed(_)))
            .count();
        assert!(exposed < 10, "this world has unexposed steps");
        assert!(
            heading.contains(&format!("/{exposed} determined")),
            "progress must be stated against available steps; heading was {heading}"
        );
    }

    /// The spine must survive an ASCII rendering: the marks are one cell in
    /// both sets and never drawn as literals.
    #[test]
    fn the_spine_renders_pure_ascii_under_ascii_glyphs() {
        let lines = compose_spine_lines(&TuiState::default(), &world(), Glyphs::ascii());
        for line in &lines {
            assert!(line.is_ascii(), "non-ASCII in ASCII spine row: {line}");
        }
    }
}
