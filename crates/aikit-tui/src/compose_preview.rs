//! §5 Preview — "what did this actually resolve to", answered from the world
//! that already resolved.
//!
//! Spec `docs/v2/23-TUI-HUMAN-EXPERIENCE-SPEC.md` §5.1 ends the composition
//! spine with a Preview that answers nine specific questions before a person
//! enters work. This module answers them, one row each, in the spec's own
//! order — so the pane can be read against the spec line by line rather than
//! by inference.
//!
//! It introduces no owner, no ontology and no second resolver: every row is a
//! projection of [`ProjectWorldReadModel`] plus the live
//! [`crate::application::StagedChanges`], which is W0's acceptance condition
//! ("no new ontology/owner is introduced merely to make UI code convenient").
//! It renders inside the existing `Overlay::CompositionPreview`, folded in
//! beside the package-toggle summary exactly as `Overlay::Explain` folds in
//! `project_workspace_render::explain_lines` — the one
//! staging -> preview -> confirm -> apply route, not a second one.
//!
//! Two of the nine questions have no application-boundary answer yet — the
//! selected working environment (§5.1 Workspace/continuity, which needs the
//! `WorkingEnvironmentProvider` surfacing W6 will bring) and what needs
//! restart/reprojection. Those rows say so in this codebase's established
//! "not exposed by application boundary" idiom rather than being dropped,
//! because a missing Preview row reads as "nothing to report" — the precise
//! misreading Preview exists to prevent.

use aikit_core::context_resolution::Availability;
use aikit_core::credential_world::{CredentialStatusKnowledge, ProviderRosterKnowledge};
use aikit_core::resource::{Eligibility, SourceAuthority};
use aikit_core::{ContextSourceHit, ProjectWorldReadModel, ProjectWorldResource};

use crate::application::TuiState;
use crate::layout::Glyphs;

/// How many named examples a row carries before it stops enumerating. A
/// Preview that lists forty ineligible resources has stopped being a preview.
const NAMED_EXAMPLES: usize = 3;

/// The §5.1 Preview block, in spec order. Empty only when there is no resolved
/// world to preview at all.
pub fn compose_preview_lines(
    state: &TuiState,
    world: &ProjectWorldReadModel,
    glyphs: Glyphs,
) -> Vec<String> {
    let sep = glyphs.separator();
    let mut lines = vec![
        format!("Preview {sep} what this composes to"),
        String::new(),
    ];

    lines.extend(resolved_to(world, sep));
    lines.push(authored(world, sep));
    lines.push(effective(world, sep));
    lines.extend(withheld(world, sep));
    lines.extend(carried_by(world, sep));
    lines.push(information(world, sep));
    lines.push(material(world, sep));
    lines.push("Environment    working environment not exposed by application boundary".into());
    lines.push(activates(state, world, sep));
    lines.push("Reprojection   not exposed by application boundary".into());
    lines
}

/// Q1 — what Agent/World did this actually resolve to?
fn resolved_to(world: &ProjectWorldReadModel, sep: &str) -> Vec<String> {
    let mut lines = vec![format!(
        "Resolved to    {} {sep} {}",
        world.project.project,
        world
            .context
            .project_root
            .as_ref()
            .map(|root| root.display().to_string())
            .unwrap_or_else(|| "no project root".to_string()),
    )];
    for (label, actor) in [
        ("agent", &world.actor_runtime.agent),
        ("agency", &world.actor_runtime.agency),
        ("host", &world.actor_runtime.host),
    ] {
        // A requested-but-unresolved actor is the single most important thing
        // Preview can say, so it is never collapsed into the resolved case.
        let value = match (actor.effective.as_ref(), actor.requested.as_ref()) {
            (Some(effective), _) => effective.resource.as_str().to_string(),
            (None, Some(requested)) => format!("{requested} requested, not resolved"),
            (None, None) => "not requested".to_string(),
        };
        lines.push(format!("               {label:<7}{value}"));
    }
    lines
}

/// Q2 — what is authored? Authorship is a property of a Resource's own
/// sources, never inferred from the fact that something resolved.
fn authored(world: &ProjectWorldReadModel, sep: &str) -> String {
    let resources = all_resources(world);
    let total = resources.len();
    let authored = resources
        .iter()
        .filter(|resource| {
            resource
                .intent
                .sources
                .iter()
                .any(|source| source.authority == Some(SourceAuthority::Authored))
        })
        .count();
    format!(
        "Authored       {} profile{} {sep} {} scope{} {sep} {authored} of {total} resources carry an authored source",
        world.resolution_basis.profiles.len(),
        plural(world.resolution_basis.profiles.len()),
        world.resolution_basis.scopes.len(),
        plural(world.resolution_basis.scopes.len()),
    )
}

/// Q3 — what is effective? `Unresolved` is kept apart from `Unavailable`:
/// one is an open question, the other a determined no.
fn effective(world: &ProjectWorldReadModel, sep: &str) -> String {
    let resources = all_resources(world);
    let total = resources.len();
    let available = resources
        .iter()
        .filter(|resource| matches!(resource.effective.availability, Availability::Available))
        .count();
    let unresolved = resources
        .iter()
        .filter(|resource| matches!(resource.effective.availability, Availability::Unresolved { .. }))
        .count();
    format!(
        "Effective      {available} of {total} available {sep} {unresolved} unresolved",
    )
}

/// Q4 — what is withheld or unavailable, and why. Reasons are the point of
/// this row: a count with no reason cannot be acted on.
fn withheld(world: &ProjectWorldReadModel, sep: &str) -> Vec<String> {
    let resources = all_resources(world);
    let mut reasons: Vec<String> = Vec::new();
    for resource in &resources {
        if let Eligibility::Ineligible { reasons: why } = &resource.intent.eligibility {
            reasons.push(format!("{} ineligible: {}", resource.resource, join_reasons(why)));
        }
        if let Availability::Unavailable { reasons: why } = &resource.effective.availability {
            reasons.push(format!("{} unavailable: {}", resource.resource, join_reasons(why)));
        }
    }
    let unaskable = world
        .information_horizon
        .sources
        .iter()
        .filter(|source| !source.disclosure.askable)
        .count();

    let mut lines = vec![format!(
        "Withheld       {} withheld resource{} {sep} {unaskable} source{} not askable",
        reasons.len(),
        plural(reasons.len()),
        plural(unaskable),
    )];
    for reason in reasons.iter().take(NAMED_EXAMPLES) {
        lines.push(format!("               {reason}"));
    }
    if reasons.len() > NAMED_EXAMPLES {
        lines.push(format!(
            "               and {} more",
            reasons.len() - NAMED_EXAMPLES
        ));
    }
    for warning in &world.warnings {
        lines.push(format!("               boundary: {warning}"));
    }
    lines
}

/// Q5 — what provider/body will carry it, including the credential world the
/// body needs to actually start.
fn carried_by(world: &ProjectWorldReadModel, sep: &str) -> Vec<String> {
    let runtime = &world.actor_runtime;
    let mut lines = vec![format!(
        "Carried by     {} harness{} {sep} {} model{} {sep} {} execution offer{}",
        runtime.harnesses.len(),
        if runtime.harnesses.len() == 1 { "" } else { "es" },
        runtime.models.len(),
        plural(runtime.models.len()),
        runtime.execution_offers.len(),
        plural(runtime.execution_offers.len()),
    )];
    for resource in runtime.harnesses.iter().chain(runtime.models.iter()).take(NAMED_EXAMPLES) {
        lines.push(format!(
            "               {} {sep} {}",
            resource.resource,
            availability_label(&resource.effective.availability),
        ));
    }

    let credentials = &world.credential_world;
    let credential_row = match &credentials.providers {
        ProviderRosterKnowledge::Unknown { .. } => {
            "credential world not observed for this body".to_string()
        }
        ProviderRosterKnowledge::Observed { providers } if credentials.credentials.is_empty() => {
            format!("{} provider{} observed {sep} no credential required", providers.len(), plural(providers.len()))
        }
        ProviderRosterKnowledge::Observed { providers } => {
            let selected = credentials
                .credentials
                .values()
                .filter(|status| status.is_selected())
                .count();
            let unattempted = credentials
                .credentials
                .values()
                .filter(|status| matches!(status, CredentialStatusKnowledge::Unresolved { .. }))
                .count();
            let mut row = format!(
                "{} provider{} observed {sep} {selected}/{} credential{} resolved",
                providers.len(),
                plural(providers.len()),
                credentials.credentials.len(),
                plural(credentials.credentials.len()),
            );
            if unattempted > 0 {
                row.push_str(&format!(" {sep} {unattempted} not attempted"));
            }
            row
        }
    };
    lines.push(format!("               {credential_row}"));
    lines
}

/// Q6 — what information is eligible vs actually retrieved. This read model
/// never retrieves, so `retrieved` here is observation, not a side effect.
fn information(world: &ProjectWorldReadModel, sep: &str) -> String {
    let sources: &[ContextSourceHit] = &world.information_horizon.sources;
    let eligible = sources
        .iter()
        .filter(|source| source.eligibility.is_eligible())
        .count();
    let retrieved = sources.iter().filter(|source| source.disclosure.retrieved).count();
    format!(
        "Information    {eligible} eligible {sep} {retrieved} retrieved {sep} {} planned retrieval{}",
        world.information_horizon.planned_retrieval.len(),
        plural(world.information_horizon.planned_retrieval.len()),
    )
}

/// Q7 — what material is selected. Absent a versioned provider this is an
/// unread material world, not a clean one.
fn material(world: &ProjectWorldReadModel, sep: &str) -> String {
    let Some(versioned) = world.versioned_world.as_ref() else {
        return "Material       no versioned material provider attached to this reading".to_string();
    };
    let branch = versioned
        .repository
        .branch
        .as_deref()
        .unwrap_or(if versioned.repository.detached { "detached" } else { "unnamed" });
    let cleanliness = if versioned.working.is_clean() {
        "clean".to_string()
    } else {
        format!(
            "{} staged {sep} {} unstaged {sep} {} untracked",
            versioned.working.staged.len(),
            versioned.working.unstaged.len(),
            versioned.working.untracked.len(),
        )
    };
    format!(
        "Material       {branch} {sep} {} {sep} {cleanliness}",
        versioned.repository.head.as_str(),
    )
}

/// Q8 — what will be generated or activated, including anything staged but
/// not yet applied. Staged changes are named as *not yet* applied.
fn activates(state: &TuiState, world: &ProjectWorldReadModel, sep: &str) -> String {
    let staged = state.staged.len();
    let staged_row = if staged == 0 {
        "nothing staged".to_string()
    } else {
        format!("{staged} staged change{} not yet applied", plural(staged))
    };
    format!(
        "Activates      {} target{} {sep} {} effective capabilit{} {sep} {staged_row}",
        world.projection.targets.len(),
        plural(world.projection.targets.len()),
        world.projection.active_capabilities.len(),
        if world.projection.active_capabilities.len() == 1 { "y" } else { "ies" },
    )
}

/// Every Resource the reading discloses, across horizons. Preview counts the
/// whole composed world, not one horizon that happens to be in view.
fn all_resources(world: &ProjectWorldReadModel) -> Vec<&ProjectWorldResource> {
    world
        .capability_horizon
        .capabilities
        .iter()
        .chain(world.capability_horizon.actions.iter())
        .chain(world.information_horizon.resolved_sources.iter())
        .chain(world.actor_runtime.models.iter())
        .chain(world.actor_runtime.harnesses.iter())
        .chain(world.actor_runtime.execution_offers.iter())
        .chain(world.actor_runtime.agent.effective.iter())
        .chain(world.actor_runtime.agency.effective.iter())
        .chain(world.actor_runtime.host.effective.iter())
        .collect()
}

fn availability_label(availability: &Availability) -> String {
    match availability {
        Availability::Available => "available".to_string(),
        Availability::Unresolved { reasons } => format!("unresolved: {}", join_reasons(reasons)),
        Availability::Unavailable { reasons } => format!("unavailable: {}", join_reasons(reasons)),
    }
}

fn join_reasons(reasons: &[String]) -> String {
    if reasons.is_empty() {
        "no reason given".to_string()
    } else {
        reasons.join("; ")
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use aikit_core::context::ContextDescriptor;
    use aikit_core::context_resolution::Availability;
    use aikit_core::project::{ProjectBinding, ProjectConstituentRef, ProjectRef};
    use aikit_core::resource::{
        Eligibility, ResourceKind, ResourceRef, ResourceSource, SourceAuthority, SourceRef,
        SourceState,
    };
    use aikit_core::{ResourceEffectiveDisclosure, ResourceIntentDisclosure};

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

    fn resource(
        id: &str,
        authority: Option<SourceAuthority>,
        eligibility: Eligibility,
        availability: Availability,
    ) -> ProjectWorldResource {
        ProjectWorldResource {
            resource: ResourceRef::parse(id).unwrap(),
            kind: ResourceKind::Capability,
            name: id.into(),
            description: String::new(),
            intent: ResourceIntentDisclosure {
                eligibility,
                preference: None,
                sources: vec![ResourceSource {
                    source: SourceRef::parse("source:working-tree").unwrap(),
                    authority,
                    revision: None,
                    locator: None,
                    state: SourceState::Available,
                }],
            },
            effective: ResourceEffectiveDisclosure {
                availability,
                providers: Vec::new(),
            },
        }
    }

    fn find(lines: &[String], label: &str) -> String {
        lines
            .iter()
            .find(|line| line.starts_with(label))
            .unwrap_or_else(|| panic!("no `{label}` row in {lines:#?}"))
            .clone()
    }

    /// The spec asks nine questions. Preview answers all nine every time — a
    /// question with no answer still gets a row saying so, because a dropped
    /// row reads as "nothing to report".
    #[test]
    fn every_section_5_question_gets_a_row_even_when_unanswerable() {
        let lines = compose_preview_lines(&TuiState::default(), &world(), Glyphs::unicode());
        for label in [
            "Resolved to", "Authored", "Effective", "Withheld", "Carried by",
            "Information", "Material", "Environment", "Activates", "Reprojection",
        ] {
            find(&lines, label);
        }
    }

    /// Authorship is read from a Resource's own sources. A resource that
    /// merely resolved is not thereby authored — that conflation is what the
    /// authored/effective split exists to prevent.
    #[test]
    fn resolving_does_not_make_a_resource_authored() {
        let mut world = world();
        world.capability_horizon.capabilities = vec![
            resource("capability:a", Some(SourceAuthority::Authored), Eligibility::Eligible, Availability::Available),
            resource("capability:b", Some(SourceAuthority::Generated), Eligibility::Eligible, Availability::Available),
            resource("capability:c", None, Eligibility::Eligible, Availability::Available),
        ];

        let lines = compose_preview_lines(&TuiState::default(), &world, Glyphs::unicode());
        assert!(find(&lines, "Authored").contains("1 of 3 resources carry an authored source"));
        assert!(find(&lines, "Effective").contains("3 of 3 available"));
    }

    /// `Unresolved` is an open question and `Unavailable` is a determined no.
    /// Preview must not add the first to the second.
    #[test]
    fn unresolved_is_counted_apart_from_unavailable() {
        let mut world = world();
        world.capability_horizon.capabilities = vec![
            resource("capability:a", None, Eligibility::Eligible, Availability::Available),
            resource("capability:b", None, Eligibility::Eligible, Availability::Unresolved { reasons: vec!["no provider yet".into()] }),
            resource("capability:c", None, Eligibility::Eligible, Availability::Unavailable { reasons: vec!["host offline".into()] }),
        ];

        let lines = compose_preview_lines(&TuiState::default(), &world, Glyphs::unicode());
        let effective = find(&lines, "Effective");
        assert!(effective.contains("1 of 3 available"));
        assert!(effective.contains("1 unresolved"));

        // Only the determined no is withheld; the open question is not.
        let withheld = find(&lines, "Withheld");
        assert!(withheld.contains("1 withheld resource"), "got {withheld}");
    }

    /// A withheld resource without its reason cannot be acted on, so the
    /// reason travels with the count.
    #[test]
    fn a_withheld_resource_carries_the_reason_it_was_withheld() {
        let mut world = world();
        world.capability_horizon.capabilities = vec![resource(
            "capability:secret",
            None,
            Eligibility::Ineligible { reasons: vec!["scope forbids it".into()] },
            Availability::Available,
        )];

        let lines = compose_preview_lines(&TuiState::default(), &world, Glyphs::unicode());
        assert!(lines.iter().any(|line| line.contains("capability:secret")
            && line.contains("scope forbids it")));
    }

    /// An unread material world is not a clean one.
    #[test]
    fn an_absent_versioned_provider_does_not_read_as_a_clean_tree() {
        let lines = compose_preview_lines(&TuiState::default(), &world(), Glyphs::unicode());
        let material = find(&lines, "Material");
        assert!(material.contains("no versioned material provider"));
        assert!(!material.contains("clean"));
    }

    /// Nothing staged reads as nothing staged, never as an applied change.
    #[test]
    fn an_unstaged_world_does_not_claim_staged_changes() {
        let lines = compose_preview_lines(&TuiState::default(), &world(), Glyphs::unicode());
        assert!(find(&lines, "Activates").contains("nothing staged"));
    }

    /// The two questions with no application-boundary answer say so, rather
    /// than reading as a chosen absence.
    #[test]
    fn unexposed_questions_do_not_read_as_chosen_absences() {
        let lines = compose_preview_lines(&TuiState::default(), &world(), Glyphs::unicode());
        assert_eq!(
            find(&lines, "Environment"),
            "Environment    working environment not exposed by application boundary"
        );
        assert_eq!(
            find(&lines, "Reprojection"),
            "Reprojection   not exposed by application boundary"
        );
    }

    /// A requested-but-unresolved actor is the single most important thing
    /// Preview can say about identity; it must not collapse into "not
    /// requested".
    #[test]
    fn a_requested_but_unresolved_actor_is_not_reported_as_unrequested() {
        let mut world = world();
        world.actor_runtime.agent.requested = Some(ResourceRef::parse("agent:researcher").unwrap());

        let lines = compose_preview_lines(&TuiState::default(), &world, Glyphs::unicode());
        let agent = lines
            .iter()
            .find(|line| line.contains("agent  "))
            .expect("an agent row");
        assert!(agent.contains("agent:researcher"));
        assert!(agent.contains("requested, not resolved"));
        assert!(!agent.contains("not requested"));
    }
}
