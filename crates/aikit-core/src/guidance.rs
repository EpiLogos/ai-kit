//! Guidance composition with a token budget.
//!
//! Guidance capsules are prose that gets injected into an agent's context. The
//! failure mode this module exists to prevent is the obvious one: a dozen
//! capsules each contributing "just a paragraph" until the composed block is
//! larger than the work it was supposed to inform, and nobody can say which
//! capsule caused it.
//!
//! So composition is bounded and accounted for. Every fragment either appears in
//! the composed text or appears in the record with a reason, the total is
//! measured, and a fragment that does not fit is dropped **whole** — truncating
//! prose mid-sentence produces instructions that mean something other than what
//! their author wrote, which is worse than omitting them.

use serde::{Deserialize, Serialize};

use crate::hooks::HookEventKind;
use crate::id::CapsuleId;
use crate::platform::TargetId;

/// Separator between two included fragments in the composed text.
const SEPARATOR: &str = "\n\n";

/// The budget cost of joining one more fragment onto the composition.
///
/// The separator normalizes to a single character, so one token per join is a
/// safe upper bound on what the join adds to [`estimate_tokens`] of the whole
/// composed text. Charging it keeps the invariant
/// `estimate_tokens(text) <= used_tokens <= budget` true by construction rather
/// than by luck.
const JOIN_COST: u32 = 1;

/// One capsule's contribution to a composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuidanceFragment {
    pub capsule: CapsuleId,
    pub order: i32,
    /// Fragments sharing a key say the same thing; only one is injected.
    #[serde(default)]
    pub dedup_key: Option<String>,
    pub body: String,
    /// The author's own claim about this fragment's size, from the manifest.
    #[serde(default)]
    pub per_fragment_budget: Option<u32>,
    /// Which fragment wins a `dedup_key` contest. Supplied by the caller because
    /// core does not decide it: the natural source is the rank of the scope that
    /// enabled the capsule (`ScopeKind::rank()`), so a session-scoped override
    /// beats the global default it was written to replace.
    #[serde(default)]
    pub precedence: i32,
}

impl GuidanceFragment {
    pub fn new(capsule: CapsuleId, body: impl Into<String>) -> Self {
        Self {
            capsule,
            order: 0,
            dedup_key: None,
            body: body.into(),
            per_fragment_budget: None,
            precedence: 0,
        }
    }

    #[must_use]
    pub fn with_order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    #[must_use]
    pub fn with_dedup_key(mut self, key: impl Into<String>) -> Self {
        self.dedup_key = Some(key.into());
        self
    }

    #[must_use]
    pub fn with_per_fragment_budget(mut self, budget: u32) -> Self {
        self.per_fragment_budget = Some(budget);
        self
    }

    #[must_use]
    pub fn with_precedence(mut self, precedence: i32) -> Self {
        self.precedence = precedence;
        self
    }

    /// Build a fragment from a guidance capsule's manifest section plus the body
    /// the store read from disk.
    pub fn from_section(
        capsule: CapsuleId,
        section: &crate::capsule::GuidanceSection,
        body: impl Into<String>,
        precedence: i32,
    ) -> Self {
        Self {
            capsule,
            order: section.order,
            dedup_key: section.dedup_key.clone(),
            body: body.into(),
            per_fragment_budget: section.token_budget,
            precedence,
        }
    }

    pub fn estimated_tokens(&self) -> u32 {
        estimate_tokens(&self.body)
    }
}

/// What a composition is being built for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionRequest {
    pub event: HookEventKind,
    pub target: TargetId,
    pub total_budget: u32,
}

impl CompositionRequest {
    pub fn new(event: HookEventKind, target: TargetId, total_budget: u32) -> Self {
        Self {
            event,
            target,
            total_budget,
        }
    }
}

/// Why a fragment did or did not make it into the composed text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum FragmentStatus {
    Included,
    /// Included, but larger than the budget its own manifest declared. Reported
    /// so the capsule's author finds out, not so the operator's budget changes.
    IncludedOverFragmentBudget {
        budget: u32,
    },
    SkippedDuplicate {
        winner: CapsuleId,
    },
    SkippedOverTotalBudget {
        remaining: u32,
    },
    SkippedEmpty,
}

impl FragmentStatus {
    pub fn is_included(&self) -> bool {
        matches!(
            self,
            FragmentStatus::Included | FragmentStatus::IncludedOverFragmentBudget { .. }
        )
    }

    pub fn describe(&self) -> String {
        match self {
            FragmentStatus::Included => "included".to_string(),
            FragmentStatus::IncludedOverFragmentBudget { budget } => {
                format!("included, over its own {budget}-token budget")
            }
            FragmentStatus::SkippedDuplicate { winner } => {
                format!("skipped: duplicate of {winner}")
            }
            FragmentStatus::SkippedOverTotalBudget { remaining } => {
                format!("skipped: only {remaining} tokens of budget remained")
            }
            FragmentStatus::SkippedEmpty => "skipped: empty".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionEntry {
    pub capsule: CapsuleId,
    pub estimated_tokens: u32,
    pub status: FragmentStatus,
}

/// The composed guidance plus the account of how it was arrived at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Composition {
    pub event: HookEventKind,
    pub target: TargetId,
    pub total_budget: u32,
    /// One entry per input fragment, in composition order.
    pub entries: Vec<CompositionEntry>,
    pub text: String,
    /// Tokens spent, including one per join. Always `<= total_budget`.
    pub used_tokens: u32,
}

impl Composition {
    pub fn entry(&self, capsule: &str) -> Option<&CompositionEntry> {
        self.entries
            .iter()
            .find(|e| e.capsule.to_string() == capsule)
    }

    pub fn included(&self) -> Vec<&CompositionEntry> {
        self.entries
            .iter()
            .filter(|e| e.status.is_included())
            .collect()
    }

    pub fn skipped(&self) -> Vec<&CompositionEntry> {
        self.entries
            .iter()
            .filter(|e| !e.status.is_included())
            .collect()
    }

    pub fn over_budget_fragments(&self) -> Vec<&CompositionEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, FragmentStatus::IncludedOverFragmentBudget { .. }))
            .collect()
    }

    pub fn remaining_tokens(&self) -> u32 {
        self.total_budget.saturating_sub(self.used_tokens)
    }

    /// The human-readable composition record.
    ///
    /// The total is not the sum of the rows: it also carries one token per join,
    /// which is what makes the printed figure an honest upper bound on the
    /// composed text rather than an optimistic one.
    pub fn render_record(&self) -> String {
        const TOTAL_LABEL: &str = "total";

        let id_width = self
            .entries
            .iter()
            .map(|e| e.capsule.to_string().chars().count())
            .chain(std::iter::once(TOTAL_LABEL.len()))
            .max()
            .unwrap_or(TOTAL_LABEL.len());
        let token_width = self
            .entries
            .iter()
            .map(|e| e.estimated_tokens.to_string().len())
            .chain(std::iter::once(self.used_tokens.to_string().len()))
            .max()
            .unwrap_or(1);

        let mut out = format!(
            "guidance composition — {} → {}\n",
            self.event.as_str(),
            self.target.as_str()
        );
        if self.entries.is_empty() {
            out.push_str("  nothing to compose\n");
        }
        for entry in &self.entries {
            out.push_str(&format!(
                "  {:<id_width$}  {:>token_width$} tokens  {}\n",
                entry.capsule.to_string(),
                entry.estimated_tokens,
                entry.status.describe(),
            ));
        }
        out.push_str(&format!(
            "  {:<id_width$}  {:>token_width$} / {} tokens\n",
            TOTAL_LABEL, self.used_tokens, self.total_budget
        ));
        out
    }
}

/// Compose `fragments` into a bounded block of guidance.
///
/// Order is `(declared order, capsule id)`. Fragments sharing a `dedup_key` are
/// resolved to the highest-precedence one, ties going to whichever comes first in
/// that order so the result is a function of the inputs alone.
///
/// A fragment that does not fit the remaining budget is skipped and composition
/// *continues*: stopping at the first oversized fragment would silently drop
/// every later instruction, and a short high-order note has done nothing to
/// deserve losing its place to a long one that happened to precede it.
pub fn compose(fragments: Vec<GuidanceFragment>, request: &CompositionRequest) -> Composition {
    let mut ordered = fragments;
    ordered.sort_by(|a, b| (a.order, &a.capsule).cmp(&(b.order, &b.capsule)));

    // Resolve dedup contests up front, so the winner is chosen by precedence and
    // not by whichever copy the budget happened to reach first.
    let winners: Vec<Option<CapsuleId>> = ordered
        .iter()
        .map(|fragment| {
            let key = fragment.dedup_key.as_ref()?;
            // `max_by_key` would keep the *last* maximum; ties must go to the
            // first fragment in composition order so the winner is a function of
            // the inputs and not of the vector's tail.
            let winner = ordered
                .iter()
                .filter(|other| other.dedup_key.as_ref() == Some(key))
                .reduce(|best, other| {
                    if other.precedence > best.precedence {
                        other
                    } else {
                        best
                    }
                })?;
            (winner.capsule != fragment.capsule).then(|| winner.capsule.clone())
        })
        .collect();

    let mut entries: Vec<CompositionEntry> = Vec::with_capacity(ordered.len());
    let mut bodies: Vec<String> = Vec::new();
    let mut used: u32 = 0;

    for (fragment, loser_of) in ordered.iter().zip(winners) {
        let tokens = fragment.estimated_tokens();

        if let Some(winner) = loser_of {
            entries.push(CompositionEntry {
                capsule: fragment.capsule.clone(),
                estimated_tokens: tokens,
                status: FragmentStatus::SkippedDuplicate { winner },
            });
            continue;
        }

        if tokens == 0 {
            entries.push(CompositionEntry {
                capsule: fragment.capsule.clone(),
                estimated_tokens: 0,
                status: FragmentStatus::SkippedEmpty,
            });
            continue;
        }

        let join = if bodies.is_empty() { 0 } else { JOIN_COST };
        let cost = tokens.saturating_add(join);
        if used.saturating_add(cost) > request.total_budget {
            entries.push(CompositionEntry {
                capsule: fragment.capsule.clone(),
                estimated_tokens: tokens,
                status: FragmentStatus::SkippedOverTotalBudget {
                    remaining: request.total_budget.saturating_sub(used),
                },
            });
            continue;
        }

        used += cost;
        bodies.push(fragment.body.trim().to_string());
        entries.push(CompositionEntry {
            capsule: fragment.capsule.clone(),
            estimated_tokens: tokens,
            status: match fragment.per_fragment_budget {
                Some(budget) if tokens > budget => {
                    FragmentStatus::IncludedOverFragmentBudget { budget }
                }
                _ => FragmentStatus::Included,
            },
        });
    }

    Composition {
        event: request.event.clone(),
        target: request.target.clone(),
        total_budget: request.total_budget,
        entries,
        text: bodies.join(SEPARATOR),
        used_tokens: used,
    }
}

/// A deterministic approximation of how many tokens a piece of text will cost.
///
/// Whitespace is normalized, then the character count is divided by four and
/// rounded up. This is an **estimate**, not a tokenizer: the real count depends on
/// the model, and AIKit deliberately does not carry a vocabulary per client.
///
/// An approximation is acceptable here because of what the number is used for. It
/// decides whether one more paragraph of *advice* is included, and it is reported
/// to the user so a wrong call is visible and adjustable. Being occasionally 15%
/// out costs a paragraph; carrying a tokenizer per client would cost a dependency
/// tree, a download, and a per-keystroke budget AIKit does not have. What the
/// estimate must be is *stable* — the same text always costs the same, whatever
/// its whitespace — so that a composition record is reproducible and a budget can
/// be tuned once.
pub fn estimate_tokens(text: &str) -> u32 {
    let characters: usize = text
        .split_whitespace()
        .map(|word| word.chars().count())
        .sum::<usize>()
        // One separating space between words, matching the normalized form.
        + text.split_whitespace().count().saturating_sub(1);
    (characters as u32).div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fragment_built_from_a_manifest_section_carries_its_declarations() {
        let src = r#"
schema = 1
id = "guidance/mode/research"
kind = "guidance"
name = "Research mode"
description = "Prefer reading over writing."

[guidance]
entry = "payload/guidance.md"
order = 30
token_budget = 400
dedup_key = "research-mode"
"#;
        let capsule = crate::capsule::Capsule::from_toml_str(src).unwrap();
        let section = capsule.guidance().unwrap();
        let fragment = GuidanceFragment::from_section(
            capsule.id.clone(),
            section,
            "Read first, write second.",
            4,
        );

        assert_eq!(fragment.order, 30);
        assert_eq!(fragment.per_fragment_budget, Some(400));
        assert_eq!(fragment.dedup_key.as_deref(), Some("research-mode"));
        assert_eq!(fragment.precedence, 4);
    }

    #[test]
    fn a_composition_reports_the_budget_it_did_not_spend() {
        let request = CompositionRequest::new(HookEventKind::SessionStart, TargetId::codex(), 100);
        let composition = compose(
            vec![GuidanceFragment::new(
                CapsuleId::parse("guidance/a/one").unwrap(),
                "x".repeat(40),
            )],
            &request,
        );
        assert_eq!(composition.used_tokens, 10);
        assert_eq!(composition.remaining_tokens(), 90);
    }
}
