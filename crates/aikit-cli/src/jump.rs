//! `aikit z <words…>` — the single verb.
//!
//! `z` no longer owns a catalogue matcher or ranking policy. It consumes AIKit's
//! canonical shallow ResourceRef field and the same operative resolver/ranking law
//! as Search/TUI, then applies only its stricter interaction decision: act on one
//! textually clear package winner, open the palette when direct intent is
//! contested, or report nothing.
//!
//! Keeping this on the shallow Resource index preserves the zoxide-style quick
//! path: deep Knowledge providers are not invoked just to jump to a known local
//! capability. Context, Project and learned familiarity may order otherwise equal
//! destinations for disclosure, but they never turn ambiguity into consent. And
//! **`z` never activates anything**: an executable is run; guidance is shown;
//! activation remains a separate explicit operation.

use std::time::{SystemTime, UNIX_EPOCH};

use aikit_core::capsule::Kind;
use aikit_core::frecency::{self, Candidate, Jump};
use aikit_core::id::CapsuleId;
use aikit_core::resource::{
    resolve_expression, resolve_path_identity, ResolveExpression, ResourceRef,
};
use aikit_core::{FamiliarityContext, Result, DEFAULT_FAMILIARITY_HALF_LIFE_MS};
use aikit_tui::backend::PaletteBackend;

use crate::app::Service;

/// What `z` would do, with the evidence behind it.
#[derive(Debug, Clone)]
pub struct JumpPlan {
    pub query: String,
    pub jump: Jump,
    /// Package-backed candidates from canonical Resolve, already best first.
    pub ranked: Vec<Candidate>,
}

impl JumpPlan {
    /// The action a caller should take for the winner, if there is one.
    pub fn action(&self, service: &Service) -> Option<JumpAction> {
        let Jump::Act { capsule } = &self.jump else {
            return None;
        };
        Some(action_for(service, capsule))
    }
}

/// The natural action for a capability kind. Running and activating are different
/// acts, and `z` only ever proposes the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JumpAction {
    /// A script or alias: run it.
    Run { capsule: CapsuleId },
    /// A skill, guidance or template: show it. Never activated by `z`.
    Open { capsule: CapsuleId },
    /// A session capsule: attach or bring it up.
    Session { capsule: CapsuleId },
}

impl JumpAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            JumpAction::Run { .. } => "run",
            JumpAction::Open { .. } => "open",
            JumpAction::Session { .. } => "session",
        }
    }

    pub fn capsule(&self) -> &CapsuleId {
        match self {
            JumpAction::Run { capsule }
            | JumpAction::Open { capsule }
            | JumpAction::Session { capsule } => capsule,
        }
    }
}

fn action_for(service: &Service, capsule: &CapsuleId) -> JumpAction {
    let kind = service
        .resolved()
        .catalog_index
        .get(capsule)
        .map(|entry| entry.kind);
    match kind {
        Some(Kind::Script) | Some(Kind::Alias) => JumpAction::Run {
            capsule: capsule.clone(),
        },
        Some(Kind::Session) => JumpAction::Session {
            capsule: capsule.clone(),
        },
        _ => JumpAction::Open {
            capsule: capsule.clone(),
        },
    }
}

/// Resolve package destinations through the canonical shallow Resource field and
/// apply only `z`'s act/disambiguate consent boundary.
///
/// Every catalogued capability remains reachable whether active or inactive.
/// Non-package Resources may participate in general Search but are not executable
/// `z` destinations, so filtering them here does not create a second identity or
/// ranking pass; retained package rows keep their canonical relative order.
pub fn plan(service: &Service, query: &str) -> Result<JumpPlan> {
    let trimmed = query.trim();
    let mut index = PaletteBackend::navigation_index(service);
    let context = familiarity_context(service);
    let now = now_ms();
    if let Some(familiarity) = PaletteBackend::familiarity(service)? {
        index.apply_familiarity(
            &familiarity,
            &context,
            now,
            DEFAULT_FAMILIARITY_HALF_LIFE_MS,
        );
    }

    let expression = ResolveExpression::ordinary_search(trimmed);
    if let Some(familiarity) = PaletteBackend::familiarity(service)? {
        index.apply_resolve_path_familiarity(
            &familiarity,
            &resolve_path_identity(&expression),
            &context,
            now,
            DEFAULT_FAMILIARITY_HALF_LIFE_MS,
        );
    }
    let path = resolve_expression(&expression, &index, 256);
    let exported_commands = service.resolved().exported_commands();

    let mut ranked = Vec::new();
    for resolved_candidate in path.candidates {
        let Ok(id) = CapsuleId::parse(resolved_candidate.resource.as_str()) else {
            continue;
        };
        if !service.resolved().catalog_index.contains_key(&id) {
            continue;
        }

        // Preserve the historical `z` consent distinction: an exact exported
        // command is an unambiguous act request even when another destination has
        // equal textual relevance. This affects the decision only; canonical
        // Resolve has already determined ordering.
        let exact_export = exported_commands
            .iter()
            .any(|(name, owner)| owner == &id && name.eq_ignore_ascii_case(trimmed));
        let direct_score = if exact_export {
            1.0
        } else {
            (resolved_candidate.score as f32 / 100_000.0).clamp(0.0, 1.0)
        };
        let mut candidate = Candidate::new(id.clone(), direct_score);
        candidate.exact_export_name = exact_export;
        candidate.in_current_project = resolved_candidate.ranking.current_project;
        candidate.active_in_context = resolved_candidate.ranking.active_in_context;
        // Retained only for `z --json` evidence compatibility. Successful Run
        // history already participates in canonical ordering through the unified
        // Familiarity replay; there is deliberately no `frecency::rank` call here.
        candidate.usage = service.index().usage(&id).unwrap_or_default();
        ranked.push(candidate);
    }

    let jump = frecency::decide(&ranked);
    Ok(JumpPlan {
        query: trimmed.to_string(),
        jump,
        ranked,
    })
}

fn familiarity_context(service: &Service) -> FamiliarityContext {
    FamiliarityContext {
        project: service
            .descriptor()
            .project_id
            .as_ref()
            .and_then(|project| ResourceRef::parse(format!("project/{project}")).ok()),
        actor: None,
        agency: None,
        focus: service.descriptor().task.clone(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}
