//! Query parsing and ranking policy for the palette.
//!
//! The split of responsibility matters. The TUI owns *text matching* — it feeds
//! keystrokes to nucleo and gets a relevance number back, in-process, inside the
//! 16 ms keystroke budget. This module owns *policy*: what a query means, which
//! documents a filter admits, and what beats what once relevance is known. Keeping
//! the policy here is what lets `aikit search --json` and the palette agree, and
//! what makes the ranking testable without a terminal.
//!
//! ## Usage is a signal, never a promotion
//!
//! The specification is explicit that nothing is promoted by usage count. This
//! module honours that in ranking too: the usage contribution is bounded, and it
//! **decays with a half-life** (see [`RankingSignals::usage_half_life`]). A
//! command that was run four hundred times last quarter is a fact about last
//! quarter; without decay it would sit at the top of the palette forever and the
//! list would slowly become a museum rather than a tool.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::capsule::Kind;
use crate::id::CapsuleId;
use crate::resolve::ResolvedView;
use crate::resource::{parse_or_search_expression, ResolveExpression};
use crate::scope::ScopeKind;
use crate::trust::TrustState;

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// The single-character lanes at the front of the query box.
///
/// They exist because the four things a user reaches for have different shapes:
/// running something, changing what is active, moving between session spaces, and
/// operating AIKit itself. Making them one flat list would mean every query
/// searches three kinds of noun the user was not thinking about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FastPrefix {
    /// `>` — runnable scripts and tools.
    Run,
    /// `+` — capability management.
    Capabilities,
    /// `@` — sessions and tasks.
    Sessions,
    /// `:` — AIKit management actions.
    Manage,
}

impl FastPrefix {
    /// Only non-conflicting historical lanes remain front-character shortcuts.
    /// `+` and `@` stay deserializable enum values for compatibility, but new
    /// input parses them as O:I operative syntax.
    pub const ALL: [FastPrefix; 2] = [FastPrefix::Run, FastPrefix::Manage];

    pub fn from_char(c: char) -> Option<Self> {
        Some(match c {
            '>' => FastPrefix::Run,
            ':' => FastPrefix::Manage,
            _ => return None,
        })
    }

    pub fn as_char(self) -> char {
        match self {
            FastPrefix::Run => '>',
            FastPrefix::Capabilities => '+',
            FastPrefix::Sessions => '@',
            FastPrefix::Manage => ':',
        }
    }

    /// The hint shown next to an empty query box.
    pub fn describe(self) -> &'static str {
        match self {
            FastPrefix::Run => "run a script or tool",
            FastPrefix::Capabilities => "manage capabilities",
            FastPrefix::Sessions => "sessions and tasks",
            FastPrefix::Manage => "AIKit actions",
        }
    }
}

/// The `status:` filter, which is how the palette lets a user ask the question the
/// three-state rendering raises: "show me the things that are *not* working".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StatusFilter {
    Active,
    Inactive,
    Unavailable,
}

impl StatusFilter {
    fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "active" => StatusFilter::Active,
            "inactive" => StatusFilter::Inactive,
            "unavailable" => StatusFilter::Unavailable,
            _ => return None,
        })
    }
}

/// A parsed query.
///
/// Filters within one key widen (OR); different keys narrow (AND). Free text is
/// never used as a filter here — that would silently overrule a fuzzy match with
/// a substring check.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub raw: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<ResolveExpression>,
    #[serde(default)]
    pub prefix: Option<FastPrefix>,
    pub kinds: Vec<Kind>,
    pub tags: Vec<String>,
    pub scopes: Vec<ScopeKind>,
    #[serde(default)]
    pub status: Option<StatusFilter>,
    pub trust: Vec<TrustState>,
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && !self.has_filters() && self.prefix.is_none()
    }

    pub fn has_filters(&self) -> bool {
        !self.kinds.is_empty()
            || !self.tags.is_empty()
            || !self.scopes.is_empty()
            || self.status.is_some()
            || !self.trust.is_empty()
    }

    /// Does this document survive the query's filters?
    pub fn matches_filters(&self, doc: &SearchDoc) -> bool {
        if let Some(prefix) = self.prefix {
            let admitted = match prefix {
                // "Runnable" is the resolved view's answer, not a kind lookup: a
                // blocked or quarantined script is not something you can run.
                FastPrefix::Run => doc.runnable,
                FastPrefix::Sessions => doc.kind == Kind::Session,
                // These two switch the palette to a different source (capability
                // toggles, AIKit's own actions). Narrowing capsules by them here
                // would be inventing a rule the lanes do not have.
                FastPrefix::Capabilities | FastPrefix::Manage => true,
            };
            if !admitted {
                return false;
            }
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&doc.kind) {
            return false;
        }
        if !self.tags.is_empty() && !self.tags.iter().any(|t| doc.tags.contains(t)) {
            return false;
        }
        if !self.scopes.is_empty() && !doc.scope.is_some_and(|s| self.scopes.contains(&s)) {
            return false;
        }
        if let Some(status) = self.status {
            if doc.status.as_filter() != status {
                return false;
            }
        }
        if !self.trust.is_empty() && !self.trust.contains(&doc.trust) {
            return false;
        }
        true
    }
}

/// Parse a palette query.
///
/// Unknown filter keys and unknown filter values are treated as free text rather
/// than as errors. A palette that rejects a half-typed query is a palette people
/// stop using, and `note:remember` is a perfectly reasonable thing to search for.
pub fn parse_query(raw: &str) -> Query {
    let mut query = Query {
        raw: raw.to_string(),
        ..Default::default()
    };

    query.expression = parse_or_search_expression(raw).ok();

    let trimmed = raw.trim_start();
    let body = match trimmed.chars().next().and_then(FastPrefix::from_char) {
        Some(prefix) => {
            query.prefix = Some(prefix);
            &trimmed[prefix.as_char().len_utf8()..]
        }
        None => trimmed,
    };

    let mut free: Vec<&str> = Vec::new();
    for token in body.split_whitespace() {
        match token.split_once(':') {
            Some((key, value)) if !key.is_empty() && !value.is_empty() => {
                if !apply_filter(&mut query, &key.to_lowercase(), &value.to_lowercase()) {
                    free.push(token);
                }
            }
            _ => free.push(token),
        }
    }
    query.text = free.join(" ");
    query
}

/// Returns false when the token is not a recognized filter, so the caller can
/// keep it as free text.
fn apply_filter(query: &mut Query, key: &str, value: &str) -> bool {
    match key {
        "kind" => match value.parse::<Kind>() {
            Ok(kind) => {
                if !query.kinds.contains(&kind) {
                    query.kinds.push(kind);
                }
                true
            }
            Err(_) => false,
        },
        "tag" => {
            if !query.tags.iter().any(|t| t == value) {
                query.tags.push(value.to_string());
            }
            true
        }
        "scope" => match value.parse::<ScopeKind>() {
            Ok(scope) => {
                if !query.scopes.contains(&scope) {
                    query.scopes.push(scope);
                }
                true
            }
            Err(_) => false,
        },
        "status" => match StatusFilter::parse(value) {
            Some(status) => {
                query.status = Some(status);
                true
            }
            None => false,
        },
        "trust" => match value.parse::<TrustState>() {
            Ok(state) => {
                if !query.trust.contains(&state) {
                    query.trust.push(state);
                }
                true
            }
            Err(_) => false,
        },
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

/// *Available*, *enabled* and *loaded* are three different things; this is the
/// one the palette colours a row by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DocStatus {
    Active,
    Inactive,
    /// Declared, but held back — policy, trust, platform, a missing dependency.
    Unavailable,
}

impl DocStatus {
    fn as_filter(self) -> StatusFilter {
        match self {
            DocStatus::Active => StatusFilter::Active,
            DocStatus::Inactive => StatusFilter::Inactive,
            DocStatus::Unavailable => StatusFilter::Unavailable,
        }
    }
}

/// Operational usage facts, from the store's event log.
///
/// Only *successful* runs carry a boost. Surfacing a command because it keeps
/// failing would be actively unhelpful.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageStats {
    pub successful_runs: u32,
    pub failed_runs: u32,
    /// How long ago the last *successful* run was. `None` means never.
    #[serde(default)]
    pub last_success_age: Option<Duration>,
}

/// A flat, cheap row: everything ranking and filtering need, and nothing else.
///
/// Deliberately owns its strings rather than borrowing the view. The palette
/// rebuilds and re-ranks this list on every keystroke, and a borrow would force
/// the whole resolved view to stay pinned in the TUI's state for the lifetime of
/// the search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchDoc {
    pub id: CapsuleId,
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub exports: Vec<String>,
    pub status: DocStatus,
    /// The scope that declared it, when any did.
    #[serde(default)]
    pub scope: Option<ScopeKind>,
    pub trust: TrustState,
    /// Declared by this repository rather than inherited from the user's global
    /// or host profile.
    pub in_current_project: bool,
    /// In the effective view for the context the palette was opened in.
    pub in_active_context: bool,
    /// Whether `aikit run` would accept it right now.
    pub runnable: bool,
    pub usage: UsageStats,
}

impl SearchDoc {
    /// Build a document for one catalogued capsule.
    ///
    /// Returns `None` for a capsule the view has never heard of: a row the palette
    /// could not explain or act on is worse than no row.
    pub fn from_view(view: &ResolvedView, id: &CapsuleId, usage: UsageStats) -> Option<Self> {
        let entry = view.catalog_index.get(id)?;
        let declared = view.declared.get(id);
        let scope = declared.map(|d| d.scope);

        let status = if view.is_active(id) {
            DocStatus::Active
        } else if view.unavailable.contains_key(id) {
            DocStatus::Unavailable
        } else {
            DocStatus::Inactive
        };

        Some(Self {
            id: id.clone(),
            kind: entry.kind,
            name: entry.name.clone(),
            description: entry.description.clone(),
            tags: entry.tags.clone(),
            exports: entry.exports.clone(),
            status,
            scope,
            trust: entry.trust,
            in_current_project: matches!(
                scope,
                Some(ScopeKind::Project) | Some(ScopeKind::ProjectLocal)
            ),
            in_active_context: view.is_active(id),
            runnable: view.can_run(id),
            usage,
        })
    }

    /// Does the query name one of this capability's commands outright?
    fn is_exact_command_match(&self, query: &Query) -> bool {
        let typed = query.text.trim();
        if typed.is_empty() {
            return false;
        }
        self.exports.iter().any(|e| e == typed) || self.id.leaf() == typed
    }
}

// ---------------------------------------------------------------------------
// Ranking
// ---------------------------------------------------------------------------

/// The weights the ranking policy uses.
///
/// Exposed as data so the CLI can print them and a future preference can tune
/// them, but the *shape* of the formula — bounded, decaying usage that cannot
/// outweigh what the user typed — is a policy decision and stays here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankingSignals {
    /// Weight on the text relevance the TUI supplies from nucleo.
    pub text_weight: f32,
    /// Typing a command's exact name is an unambiguous statement of intent.
    pub exact_command_weight: f32,
    /// Declared by this repository.
    pub current_project_weight: f32,
    /// Already active in this context.
    pub active_context_weight: f32,
    /// The ceiling on the recent-successful-use contribution. Kept well below
    /// `text_weight` so habit can order near-ties but never beat a direct hit.
    pub usage_weight: f32,
    /// How long it takes a usage boost to halve.
    ///
    /// Two weeks: long enough that a weekly task keeps most of its standing,
    /// short enough that a project you finished last quarter falls off the top of
    /// the palette within a couple of months.
    pub usage_half_life: Duration,
    /// Run count at which the boost reaches half its ceiling. Small, because the
    /// difference between "used once" and "used ten times" is meaningful and the
    /// difference between "four hundred" and "four thousand" is not.
    pub usage_saturation: f32,
}

impl Default for RankingSignals {
    fn default() -> Self {
        Self {
            text_weight: 1.0,
            exact_command_weight: 0.6,
            current_project_weight: 0.2,
            active_context_weight: 0.35,
            usage_weight: 0.25,
            usage_half_life: Duration::from_secs(14 * 24 * 60 * 60),
            usage_saturation: 3.0,
        }
    }
}

impl RankingSignals {
    /// The recent-successful-use contribution, bounded by `usage_weight`.
    ///
    /// Two factors multiply: a saturating function of how often the capability
    /// succeeded, and an exponential decay in how long ago that was. The decay is
    /// the important half — without it, frequency alone would freeze the top of
    /// the palette.
    pub fn usage_boost(&self, usage: &UsageStats) -> f32 {
        if usage.successful_runs == 0 {
            return 0.0;
        }
        let Some(age) = usage.last_success_age else {
            return 0.0;
        };
        let runs = usage.successful_runs as f32;
        let frequency = runs / (runs + self.usage_saturation.max(f32::EPSILON));

        let half_life = self.usage_half_life.as_secs_f64().max(f64::EPSILON);
        let recency = 0.5f64.powf(age.as_secs_f64() / half_life) as f32;

        self.usage_weight * frequency * recency
    }
}

/// Rank one document against one query.
///
/// `text_score` is the fuzzy relevance the TUI got from nucleo, normalized to
/// `[0, 1]`. Everything else is AIKit's own opinion about what makes a row worth
/// showing: it belongs to this project, it is already live in this context, the
/// user typed its command exactly, and they have recently had success with it.
pub fn score(query: &Query, doc: &SearchDoc, text_score: f32, signals: &RankingSignals) -> f32 {
    let mut total = signals.text_weight * text_score;

    if doc.is_exact_command_match(query) {
        total += signals.exact_command_weight;
    }
    if doc.in_current_project {
        total += signals.current_project_weight;
    }
    if doc.in_active_context {
        total += signals.active_context_weight;
    }
    total += signals.usage_boost(&doc.usage);
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fast_prefix_has_a_distinct_character_and_a_hint() {
        let mut seen = std::collections::BTreeSet::new();
        for prefix in FastPrefix::ALL {
            assert!(seen.insert(prefix.as_char()), "duplicate prefix character");
            assert_eq!(FastPrefix::from_char(prefix.as_char()), Some(prefix));
            assert!(!prefix.describe().is_empty());
        }
    }

    #[test]
    fn an_empty_query_is_recognized_as_empty() {
        assert!(parse_query("").is_empty());
        assert!(parse_query("   ").is_empty());
        assert!(!parse_query("kind:script").is_empty());
        assert!(!parse_query(">").is_empty());
    }

    #[test]
    fn a_bare_colon_token_is_not_read_as_a_filter() {
        let q = parse_query("what: ");
        assert_eq!(q.text, "what:");
        assert!(!q.has_filters());
    }

    #[test]
    fn operative_at_and_plus_are_not_legacy_fast_prefixes() {
        let addressed = parse_query("@ project:demo");
        assert_eq!(addressed.prefix, None);
        assert!(matches!(
            addressed.expression,
            Some(ResolveExpression::Address { .. })
        ));

        let affirmed = parse_query("+ @5 action:verify");
        assert_eq!(affirmed.prefix, None);
        assert!(matches!(
            affirmed.expression,
            Some(ResolveExpression::Unary { .. })
        ));
    }

    #[test]
    fn the_usage_boost_is_bounded_by_its_weight_however_absurd_the_run_count() {
        let signals = RankingSignals::default();
        let boost = signals.usage_boost(&UsageStats {
            successful_runs: u32::MAX,
            failed_runs: 0,
            last_success_age: Some(Duration::ZERO),
        });
        assert!(boost <= signals.usage_weight);
        assert!(boost > signals.usage_weight * 0.99);
    }
}
