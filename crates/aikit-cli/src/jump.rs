//! `aikit z <words…>` — the single verb.
//!
//! Builds ranked candidates from the resolved view plus the store's usage records,
//! and decides what to do: act on one clear winner, open the palette pre-filtered
//! when the top is contested, or report nothing. The ranking policy itself lives in
//! `aikit_core::frecency`; this module is the wiring that gives it real data.
//!
//! **It never activates anything.** `z` on a capability that no scope selects
//! proposes *running* it, which is a different act from making it active (Part I
//! rule 6). The distinction is not pedantry: activation changes what every future
//! session in this context sees, and a fuzzy match is not consent to that.

use aikit_core::capsule::Kind;
use aikit_core::frecency::{self, Candidate, Jump, DEFAULT_HALF_LIFE};
use aikit_core::id::CapsuleId;
use aikit_core::Result;

use crate::app::Service;

/// What `z` would do, with the evidence behind it.
#[derive(Debug, Clone)]
pub struct JumpPlan {
    pub query: String,
    pub jump: Jump,
    /// Every candidate that matched at all, best first.
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

/// Rank the catalogue against `query` and decide.
///
/// Every catalogued capability is a candidate, not only the active ones: `z` is how
/// you reach something you have not enabled, and restricting it to the active set
/// would make it useless for exactly the case it exists for.
pub fn plan(service: &Service, query: &str) -> Result<JumpPlan> {
    let view = service.resolved();
    let exports = view.exported_commands();
    let trimmed = query.trim();

    let mut ranked: Vec<Candidate> = Vec::new();
    for (id, entry) in &view.catalog_index {
        let score = frecency::match_quality(trimmed, id);
        // An exported command name is its own strongest handle: `z nextest` should
        // find the capsule that exports `nextest` even if its id says otherwise.
        let exact_export = exports
            .iter()
            .any(|(name, owner)| owner == id && name.eq_ignore_ascii_case(trimmed));
        let export_score = entry
            .exports
            .iter()
            .map(|name| export_match_quality(trimmed, name))
            .fold(0.0f32, f32::max);

        let score = score.max(export_score);
        if score <= 0.0 && !exact_export {
            continue;
        }

        let mut candidate = Candidate::new(id.clone(), if exact_export { 1.0 } else { score });
        candidate.exact_export_name = exact_export;
        candidate.active_in_context = view.is_active(id);
        candidate.in_current_project = view
            .declared
            .get(id)
            .map(|d| {
                matches!(
                    d.scope,
                    aikit_core::scope::ScopeKind::Project
                        | aikit_core::scope::ScopeKind::ProjectLocal
                )
            })
            .unwrap_or(false);
        candidate.usage = service.index().usage(id).unwrap_or_default();
        ranked.push(candidate);
    }

    frecency::rank(&mut ranked, DEFAULT_HALF_LIFE);
    let jump = frecency::decide(&ranked);

    Ok(JumpPlan {
        query: trimmed.to_string(),
        jump,
        ranked,
    })
}

/// Match quality against an export name, which has no path segments — so the whole
/// name is the tail and the leaf rules apply directly.
fn export_match_quality(query: &str, export: &str) -> f32 {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return 0.0;
    }
    let export = export.to_lowercase();
    if export == query {
        1.0
    } else if export.starts_with(&query) {
        0.9
    } else if export.contains(&query) {
        0.7
    } else {
        0.0
    }
}
