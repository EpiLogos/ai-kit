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
//! standings are separate values rather than one "unavailable". It becomes
//! `NotExposed` only when the roster itself could not be read, which is the
//! one Continuity case where nothing can honestly be said.

use aikit_core::context_resolution::Availability;
use aikit_core::credential_world::ProviderRosterKnowledge;
use aikit_core::session_space_application::SessionSpaceAuthoredState;
use serde::{Deserialize, Serialize};

use crate::application::TuiState;
use crate::compose_preview::availability_label;
use crate::layout::Glyphs;
use crate::project_workspace_render::{SessionSpaceRoster, WorkspaceReading};

/// The ten §5.1 steps, in spec order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ComposeStep {
    #[default]
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

    /// Move along the spine, clamping at both ends. Clamped rather than
    /// wrapping: the spine is an ordered path from intention to work, and
    /// wrapping from `Enter work` back to `Intention` would suggest a cycle
    /// the composition does not have.
    pub fn relative(self, delta: isize) -> Self {
        let current = Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .unwrap_or(0);
        let last = Self::ALL.len().saturating_sub(1);
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            (current + delta as usize).min(last)
        };
        Self::ALL[next]
    }

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
pub fn compose_spine(state: &TuiState, reading: WorkspaceReading<'_>) -> Vec<ComposeSpineRow> {
    ComposeStep::ALL
        .into_iter()
        .map(|step| ComposeSpineRow {
            step,
            standing: standing_for(step, state, reading),
        })
        .collect()
}

fn standing_for(
    step: ComposeStep,
    state: &TuiState,
    reading: WorkspaceReading<'_>,
) -> StepStanding {
    let world = reading.world;
    match step {
        ComposeStep::Intention => {
            StepStanding::NotExposed("no intention contract at this application boundary".into())
        }

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
            let detail = format!(
                "{profiles} profile{}, {scopes} scope{}",
                s(profiles),
                s(scopes)
            );
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
        ComposeStep::Praxis => {
            StepStanding::NotExposed(format!(
            "no Profile/SkillSet/Skill/Method contract here; {} capabilit{}, {} action{} resolve",
            world.capability_horizon.capabilities.len(),
            if world.capability_horizon.capabilities.len() == 1 { "y" } else { "ies" },
            world.capability_horizon.actions.len(),
            s(world.capability_horizon.actions.len()),
        ))
        }

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
                let branch = versioned.repository.branch.as_deref().unwrap_or(
                    if versioned.repository.detached {
                        "detached"
                    } else {
                        "unnamed"
                    },
                );
                detail.push_str(&format!(", material {branch}"));
            }
            detail.push_str(&format!(
                ", {} projection target{}, {} effective capabilit{}",
                world.projection.targets.len(),
                s(world.projection.targets.len()),
                world.projection.active_capabilities.len(),
                if world.projection.active_capabilities.len() == 1 {
                    "y"
                } else {
                    "ies"
                },
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
                .filter(|resource| {
                    matches!(resource.effective.availability, Availability::Available)
                })
                .count();
            let mut detail = format!(
                "{} harness{}, {} model{}, {ready} available",
                runtime.harnesses.len(),
                if runtime.harnesses.len() == 1 {
                    ""
                } else {
                    "es"
                },
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

        // The boundary publishes `SessionSpaceApplicationProjection` — list,
        // discover, show, open, stage, apply — so continuity is a real open
        // choice, and this row reports the actual authored SessionSpaces
        // rather than asserting that some exist.
        //
        // "Names this Project" is read from each space's own
        // `project_contexts`, which is the space's authored claim about which
        // Projects it carries context for. It is not a binding: no contract
        // binds a SessionSpace to a resolved world, so the row never says one
        // is bound.
        ComposeStep::Continuity => {
            // An unreadable roster is not an absence of SessionSpaces. It is
            // the one Continuity case where nothing can be said, so it is the
            // one case that reads as unexposed rather than open.
            let Some(spaces) = reading.session_spaces.observed() else {
                let SessionSpaceRoster::Unreadable { reason } = reading.session_spaces else {
                    unreachable!("observed() is None only for Unreadable")
                };
                return StepStanding::NotExposed(format!(
                    "SessionSpace roster could not be read: {reason}"
                ));
            };

            let project = &world.project.project;
            let naming: Vec<&SessionSpaceAuthoredState> = spaces
                .iter()
                .filter(|space| space.project_contexts.contains_key(project))
                .collect();
            let discovered = spaces.len();

            if naming.is_empty() {
                return StepStanding::Open(if discovered == 0 {
                    "no authored SessionSpace discovered".into()
                } else {
                    format!(
                        "{discovered} SessionSpace{} discovered, none names this Project",
                        s(discovered)
                    )
                });
            }
            let focused = naming.iter().filter(|space| space.focus.is_some()).count();
            let mut detail = if let [only] = naming.as_slice() {
                only.definition.id.as_resource_ref().as_str().to_string()
            } else {
                format!("{} name this Project", naming.len())
            };
            if focused > 0 {
                detail.push_str(&format!(", {focused} focused"));
            }
            detail.push_str(", none bound to this reading");
            StepStanding::Open(detail)
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
    reading: WorkspaceReading<'_>,
    glyphs: Glyphs,
) -> Vec<String> {
    let sep = glyphs.separator();
    let rows = compose_spine(state, reading);
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
        let here = row.step == state.compose_step;
        lines.push(format!(
            "{} {:>2} {} {:<14}{}",
            if here { glyphs.selected() } else { ' ' },
            index + 1,
            row.standing.marker(glyphs),
            row.step.as_str(),
            row.standing.detail(),
        ));
    }

    // The step in hand discloses what stands behind its one-line summary.
    // Alt+Up/Down walks the spine; without this the spine would be a legend
    // rather than a place to work, and the detail each step already has would
    // stay unreachable.
    let detail = step_detail(state.compose_step, state, reading, glyphs);
    if !detail.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "{} {sep} Alt+{}/{} walks the spine",
            state.compose_step.as_str(),
            glyphs.step_up(),
            glyphs.step_down(),
        ));
        lines.extend(detail);
    }
    lines
}

/// What the step in hand discloses beyond its summary row.
///
/// Every branch reads an owner's read model that is already in the reading.
/// A step with no boundary contract discloses *which* contract is missing
/// rather than an empty pane: "nothing here" and "nothing can be here" are the
/// same distinction the standings keep, and it survives into the detail.
fn step_detail(
    step: ComposeStep,
    state: &TuiState,
    reading: WorkspaceReading<'_>,
    glyphs: Glyphs,
) -> Vec<String> {
    let sep = glyphs.separator();
    let world = reading.world;
    match step {
        ComposeStep::Intention => vec![
            "  A natural-language intention is the spine's first step and has".into(),
            "  no contract at this application boundary - nothing here can".into(),
            "  capture one, and nothing has silently captured one elsewhere.".into(),
        ],

        ComposeStep::Identity => {
            let mut lines = Vec::new();
            for (label, actor) in [
                ("Agent", &world.actor_runtime.agent),
                ("Agency", &world.actor_runtime.agency),
                ("Host", &world.actor_runtime.host),
            ] {
                let value = match (actor.effective.as_ref(), actor.requested.as_ref()) {
                    (Some(effective), _) => format!(
                        "{} {sep} {}",
                        effective.resource,
                        availability_label(&effective.effective.availability)
                    ),
                    (None, Some(requested)) => format!("{requested} {sep} requested, not resolved"),
                    (None, None) => "not requested".to_string(),
                };
                lines.push(format!("  {label:<8}{value}"));
                if let Some(warning) = actor.warning.as_ref() {
                    lines.push(format!("          {warning}"));
                }
            }
            lines
        }

        ComposeStep::Governance => {
            let mut lines = Vec::new();
            for profile in &world.resolution_basis.profiles {
                lines.push(format!("  profile  {profile}"));
            }
            for scope in &world.resolution_basis.scopes {
                lines.push(format!(
                    "  scope    {:?} {sep} depth {} {sep} {}",
                    scope.kind, scope.depth, scope.origin
                ));
            }
            if lines.is_empty() {
                lines.push("  No authored profile or scope resolved for this world.".into());
            }
            lines
        }

        ComposeStep::Praxis => vec![
            "  aikit-core publishes praxis - resolve_praxis, PraxisResolution,".into(),
            "  SelectedMethod - but no praxis contract crosses this application".into(),
            "  boundary, so no Profile/SkillSet/Skill/Method can be chosen here.".into(),
            format!(
                "  What does resolve: {} capabilit{}, {} action{}.",
                world.capability_horizon.capabilities.len(),
                if world.capability_horizon.capabilities.len() == 1 {
                    "y"
                } else {
                    "ies"
                },
                world.capability_horizon.actions.len(),
                s(world.capability_horizon.actions.len()),
            ),
        ],

        ComposeStep::Information => {
            let mut lines = Vec::new();
            for source in world.information_horizon.sources.iter().take(DETAIL_ROWS) {
                // Eligible and retrieved are orthogonal: this read model never
                // retrieves, so `retrieved` is an observation about the source,
                // not a consequence of looking at it here.
                lines.push(format!(
                    "  {} {sep} {} {sep} {}",
                    source.name,
                    if source.eligibility.is_eligible() {
                        "eligible"
                    } else {
                        "not eligible"
                    },
                    if source.disclosure.retrieved {
                        "retrieved"
                    } else {
                        "not retrieved"
                    },
                ));
            }
            if world.information_horizon.sources.len() > DETAIL_ROWS {
                lines.push(format!(
                    "  and {} more",
                    world.information_horizon.sources.len() - DETAIL_ROWS
                ));
            }
            if lines.is_empty() {
                lines.push("  No ContextSource is visible in this world's horizon.".into());
            }
            lines
        }

        ComposeStep::WorldsAndBounds => {
            let mut lines = vec![format!("  Project  {}", world.project.project)];
            if let Some(root) = world.context.project_root.as_ref() {
                lines.push(format!("  Root     {}", root.display()));
            }
            match world.versioned_world.as_ref() {
                Some(versioned) => lines.push(format!(
                    "  Material {} {sep} {}",
                    versioned.repository.branch.as_deref().unwrap_or("unnamed"),
                    versioned.repository.head.as_str(),
                )),
                None => {
                    lines.push("  Material no versioned provider attached to this reading".into())
                }
            }
            for target in &world.projection.targets {
                lines.push(format!("  target   {target}"));
            }
            lines
        }

        ComposeStep::Runtime => {
            let runtime = &world.actor_runtime;
            let mut lines = Vec::new();
            for resource in runtime
                .harnesses
                .iter()
                .chain(runtime.models.iter())
                .chain(runtime.execution_offers.iter())
                .take(DETAIL_ROWS)
            {
                lines.push(format!(
                    "  {} {sep} {}",
                    resource.resource,
                    availability_label(&resource.effective.availability)
                ));
            }
            if lines.is_empty() {
                lines.push("  No Harness, model or execution offer resolved.".into());
            }
            lines
        }

        ComposeStep::Continuity => match reading.session_spaces.observed().map(Vec::as_slice) {
            None => vec!["  The SessionSpace roster could not be read; see the row above.".into()],
            Some([]) => {
                vec!["  No authored SessionSpace exists for this Project.".into()]
            }
            Some(spaces) => spaces
                .iter()
                .take(DETAIL_ROWS)
                .map(|space| {
                    format!(
                        "  {} {sep} {} Project context{} {sep} {}",
                        space.definition.id.as_resource_ref(),
                        space.project_contexts.len(),
                        s(space.project_contexts.len()),
                        if space.focus.is_some() {
                            "focused"
                        } else {
                            "not focused"
                        },
                    )
                })
                .collect(),
        },

        ComposeStep::Preview => {
            if state.staged.is_empty() {
                vec!["  Nothing is staged; Ctrl+S still previews the resolved world.".into()]
            } else {
                vec![format!(
                    "  Ctrl+S previews {} staged change{}, then Ctrl+S confirms.",
                    state.staged.len(),
                    s(state.staged.len())
                )]
            }
        }

        ComposeStep::EnterWork => vec![
            "  There are two ways in: a direct session and Factory work. Neither".into(),
            "  has a contract at this application boundary - no session-start,".into(),
            "  and Factory publishes no Run/Journey status (ai-kit#227).".into(),
        ],
    }
}

/// How many rows a step's detail lists before it stops enumerating.
const DETAIL_ROWS: usize = 6;

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
    use aikit_core::project::{
        ProjectBinding, ProjectBindingLocator, ProjectConstituentRef, ProjectRef,
    };
    use aikit_core::resource::ResourceRef;
    use aikit_core::session_space::SessionSpaceRef;
    use aikit_core::session_space_application::{
        ContextResolutionBasis, ContextResolutionEvidence, SessionSpaceFocus,
    };
    use aikit_core::ProjectWorldReadModel;

    use super::*;
    use crate::project_workspace_render::HistoryReading;

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

    fn reading<'a>(
        world: &'a ProjectWorldReadModel,
        session_spaces: &'a SessionSpaceRoster,
    ) -> WorkspaceReading<'a> {
        WorkspaceReading::new(
            world,
            session_spaces,
            NO_HISTORY.get_or_init(Default::default),
        )
    }

    static NO_HISTORY: std::sync::OnceLock<HistoryReading> = std::sync::OnceLock::new();

    fn observed(spaces: Vec<SessionSpaceAuthoredState>) -> SessionSpaceRoster {
        SessionSpaceRoster::Observed(spaces)
    }

    /// An authored SessionSpace that claims context for `project`.
    fn space(id: &str, project: &str, focused: bool) -> SessionSpaceAuthoredState {
        let project = ProjectRef::parse(project).unwrap();
        let binding = ProjectBinding::new(
            project.clone(),
            ProjectConstituentRef::parse("source:working-tree").unwrap(),
            ProjectBindingLocator::Remote {
                locator: "https://example.invalid/space".into(),
            },
        );
        // `ContextResolutionRef` has no public constructor — it is
        // content-addressed by the resolver that mints it — so the evidence is
        // built through its own serde representation rather than by widening
        // the owner's API for a test.
        let evidence: ContextResolutionEvidence = serde_json::from_value(serde_json::json!({
            "reference": "context-resolution/test",
            "basis": ContextResolutionBasis {
                project_binding: binding,
                resolver_hash: "hash".into(),
                catalog_revision: "catalog-1".into(),
                scopes: Vec::new(),
                context_sources: Vec::new(),
                host: None,
                context_activations: Vec::new(),
                observed_source_resources: Vec::new(),
            },
            "provenance": [],
        }))
        .unwrap();
        let mut state = SessionSpaceAuthoredState::new(SessionSpaceRef::parse(id).unwrap());
        state.project_contexts.insert(project, evidence);
        if focused {
            state.focus = Some(SessionSpaceFocus {
                target: ResourceRef::parse("surface/terminal").unwrap(),
                region: None,
                provenance: Vec::new(),
            });
        }
        state
    }

    fn row(rows: &[ComposeSpineRow], step: ComposeStep) -> &ComposeSpineRow {
        rows.iter()
            .find(|row| row.step == step)
            .expect("a row per step")
    }

    /// A spine with gaps silently renumbers a person's sense of where they
    /// are, so every step keeps its place whatever its standing.
    #[test]
    fn the_spine_is_always_ten_steps_in_spec_order() {
        let rows = compose_spine(
            &TuiState::default(),
            reading(&world(), &observed(Vec::new())),
        );
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
        let rows = compose_spine(
            &TuiState::default(),
            reading(&world(), &observed(Vec::new())),
        );
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
        let rows = compose_spine(
            &TuiState::default(),
            reading(&world(), &observed(Vec::new())),
        );
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
        let rows = compose_spine(&TuiState::default(), reading(&world, &observed(Vec::new())));
        let detail = row(&rows, ComposeStep::Praxis)
            .standing
            .detail()
            .to_string();
        assert!(detail.contains("0 capabilities"));
        assert!(detail.contains("0 actions"));
    }

    /// The boundary publishes `SessionSpaceApplicationProjection`, so
    /// continuity is a real choice — not an unexposed one. If this ever flips
    /// to `NotExposed` the spine has started lying about what exists.
    #[test]
    fn continuity_is_open_because_the_boundary_publishes_session_spaces() {
        let rows = compose_spine(
            &TuiState::default(),
            reading(&world(), &observed(Vec::new())),
        );
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
        let rows = compose_spine(&TuiState::default(), reading(&world, &observed(Vec::new())));
        let StepStanding::Open(detail) = &row(&rows, ComposeStep::Identity).standing else {
            panic!("an unresolved request is not a determined identity");
        };
        assert!(detail.contains("agent:researcher"));
        assert!(detail.contains("not resolved"));
    }

    /// A SessionSpace naming this Project is real continuity to report — but
    /// nothing binds one to a resolved world, so the row must never say bound.
    #[test]
    fn continuity_names_the_session_space_that_claims_this_project() {
        let world = world();
        let spaces = vec![space("session-space/alpha", "project:aikit", false)];
        let rows = compose_spine(&TuiState::default(), reading(&world, &observed(spaces)));
        let detail = row(&rows, ComposeStep::Continuity)
            .standing
            .detail()
            .to_string();
        assert!(detail.contains("session-space/alpha"), "got {detail}");
        assert!(detail.contains("none bound to this reading"));
    }

    /// A SessionSpace that exists but claims a different Project is not this
    /// world's continuity, and must not be counted as if it were.
    #[test]
    fn a_session_space_for_another_project_is_not_this_worlds_continuity() {
        let world = world();
        let spaces = vec![space("session-space/other", "project:elsewhere", false)];
        let rows = compose_spine(&TuiState::default(), reading(&world, &observed(spaces)));
        let detail = row(&rows, ComposeStep::Continuity)
            .standing
            .detail()
            .to_string();
        assert!(
            detail.contains("1 SessionSpace discovered, none names this Project"),
            "got {detail}"
        );
        assert!(!detail.contains("session-space/other"));
    }

    /// No SessionSpaces at all is a different reading from some existing but
    /// none matching, and a person acts differently on each.
    #[test]
    fn an_empty_roster_reads_differently_from_a_non_matching_one() {
        let world = world();
        let empty = compose_spine(&TuiState::default(), reading(&world, &observed(Vec::new())));
        let other = vec![space("session-space/other", "project:elsewhere", false)];
        let non_matching = compose_spine(&TuiState::default(), reading(&world, &observed(other)));
        assert_ne!(
            row(&empty, ComposeStep::Continuity).standing,
            row(&non_matching, ComposeStep::Continuity).standing
        );
        assert!(row(&empty, ComposeStep::Continuity)
            .standing
            .detail()
            .contains("no authored SessionSpace discovered"));
    }

    #[test]
    fn a_focused_session_space_says_so() {
        let world = world();
        let spaces = vec![
            space("session-space/alpha", "project:aikit", true),
            space("session-space/beta", "project:aikit", false),
        ];
        let rows = compose_spine(&TuiState::default(), reading(&world, &observed(spaces)));
        let detail = row(&rows, ComposeStep::Continuity)
            .standing
            .detail()
            .to_string();
        assert!(detail.contains("2 name this Project"));
        assert!(detail.contains("1 focused"));
    }

    /// A roster that could not be read is not an absence of SessionSpaces.
    /// This is the one Continuity case where nothing can honestly be said, and
    /// it must not render as "no authored SessionSpace discovered".
    #[test]
    fn an_unreadable_roster_is_not_an_empty_one() {
        let world = world();
        let unreadable = SessionSpaceRoster::Unreadable {
            reason: "application home unavailable".into(),
        };
        let rows = compose_spine(&TuiState::default(), reading(&world, &unreadable));
        let standing = &row(&rows, ComposeStep::Continuity).standing;

        assert!(matches!(standing, StepStanding::NotExposed(_)));
        assert!(standing.detail().contains("application home unavailable"));
        assert!(!standing
            .detail()
            .contains("no authored SessionSpace discovered"));

        let empty = compose_spine(&TuiState::default(), reading(&world, &observed(Vec::new())));
        assert_ne!(standing, &row(&empty, ComposeStep::Continuity).standing);
    }

    /// The spine is an ordered path from intention to work, not a cycle.
    /// Wrapping from `Enter work` back to `Intention` would suggest a return
    /// the composition does not have, so movement clamps at both ends.
    #[test]
    fn walking_the_spine_clamps_rather_than_wraps() {
        assert_eq!(ComposeStep::Intention.relative(-1), ComposeStep::Intention);
        assert_eq!(ComposeStep::EnterWork.relative(1), ComposeStep::EnterWork);
        assert_eq!(ComposeStep::Intention.relative(1), ComposeStep::Identity);
        assert_eq!(ComposeStep::EnterWork.relative(-1), ComposeStep::Preview);
    }

    /// A step with no boundary contract still discloses something: which
    /// contract is missing. An empty detail pane would read as "nothing to
    /// see", which is the misreading the standings exist to prevent.
    #[test]
    fn an_unexposed_step_in_hand_discloses_the_contract_that_is_missing() {
        let world = world();
        let roster = observed(Vec::new());
        for (step, expected) in [
            (
                ComposeStep::Praxis,
                "no praxis contract crosses this application",
            ),
            (
                ComposeStep::Intention,
                "no contract at this application boundary",
            ),
            (ComposeStep::EnterWork, "no session-start"),
        ] {
            let state = TuiState {
                compose_step: step,
                ..Default::default()
            };
            let lines = compose_spine_lines(&state, reading(&world, &roster), Glyphs::unicode());
            let rendered = lines.join("\n");
            assert!(
                rendered.contains(expected),
                "{step:?} must name the missing contract; got:\n{rendered}"
            );
        }
    }

    /// Every step in hand renders detail, and every one of them stays ASCII
    /// under ASCII glyphs — the prose included, which is where a stray `§` or
    /// em dash gets in.
    #[test]
    fn every_step_discloses_detail_and_stays_ascii() {
        let world = world();
        let roster = observed(Vec::new());
        for step in ComposeStep::ALL {
            let state = TuiState {
                compose_step: step,
                ..Default::default()
            };
            let lines = compose_spine_lines(&state, reading(&world, &roster), Glyphs::ascii());
            assert!(
                lines.len() > ComposeStep::ALL.len() + 2,
                "{step:?} in hand must disclose detail beneath the spine"
            );
            for line in &lines {
                assert!(line.is_ascii(), "non-ASCII with {step:?} in hand: {line}");
            }
        }
    }

    /// The cursor marks exactly one step: the one in hand.
    #[test]
    fn exactly_one_step_carries_the_cursor() {
        let world = world();
        let roster = observed(Vec::new());
        let state = TuiState {
            compose_step: ComposeStep::Runtime,
            ..Default::default()
        };
        let lines = compose_spine_lines(&state, reading(&world, &roster), Glyphs::unicode());
        let cursor = Glyphs::unicode().selected();
        let marked: Vec<&String> = lines
            .iter()
            .filter(|line| line.starts_with(cursor))
            .collect();
        assert_eq!(marked.len(), 1, "got {marked:?}");
        assert!(marked[0].contains("Runtime"));
    }

    /// Progress counts only the steps a person can act on. Counting against
    /// all ten would report unexposed steps as work they had failed to do.
    #[test]
    fn progress_is_counted_against_available_steps_not_all_ten() {
        let lines = compose_spine_lines(
            &TuiState::default(),
            reading(&world(), &observed(Vec::new())),
            Glyphs::unicode(),
        );
        let heading = &lines[0];
        let rows = compose_spine(
            &TuiState::default(),
            reading(&world(), &observed(Vec::new())),
        );
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
        let lines = compose_spine_lines(
            &TuiState::default(),
            reading(&world(), &observed(Vec::new())),
            Glyphs::ascii(),
        );
        for line in &lines {
            assert!(line.is_ascii(), "non-ASCII in ASCII spine row: {line}");
        }
    }
}
