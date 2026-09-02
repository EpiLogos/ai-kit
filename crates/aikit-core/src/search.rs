//! Query parsing and compatibility search policy.
//!
//! Canonical V2 Search now resolves through the ResourceRef-native application
//! service and operative Resolve path. This module remains the flat `SearchDoc`
//! compatibility surface used by older package/palette callers: it parses the
//! historical query filters and, where those callers still rank rows, applies the
//! same deterministic ordering law as `aikit z`.
//!
//! ## Match quality is primary
//!
//! Context and successful use are evidence for ordered tiebreaks. They are never
//! added to text relevance. The resulting order is therefore explainable and
//! stable:
//!
//! `match quality -> exact command -> current project -> active context -> frecency -> id`.
//!
//! Usage is successful-use-only and recency-decayed. It can order an otherwise
//! equivalent destination, but cannot turn habit into relevance, authority,
//! eligibility, trust or activation.

use std::cmp::Ordering;
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

impl UsageStats {
    /// Recency-decayed successful use, shared by deterministic ranking callers.
    /// Failed invocations and mere disclosure never contribute.
    pub fn frecency(&self, half_life: Duration) -> f32 {
        if self.successful_runs == 0 {
            return 0.0;
        }
        let Some(age) = self.last_success_age else {
            return 0.0;
        };
        let half_life = half_life.as_secs_f32().max(f32::EPSILON);
        let decay = 0.5f32.powf(age.as_secs_f32() / half_life);
        self.successful_runs as f32 * decay
    }
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

/// Retained compatibility/config shape for flat `SearchDoc` callers.
///
/// The historical per-signal weights are still deserializable so existing config
/// does not break, but they no longer blend contextual evidence into relevance.
/// `usage_half_life` remains operative because frecency is an ordered tiebreak.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankingSignals {
    pub text_weight: f32,
    pub exact_command_weight: f32,
    pub current_project_weight: f32,
    pub active_context_weight: f32,
    pub usage_weight: f32,
    pub usage_half_life: Duration,
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
    /// Historical bounded display helper retained for source compatibility.
    /// Ranking itself uses [`UsageStats::frecency`] only after stronger signals tie.
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

/// Explainable signal which decided the ordering of two compatibility rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RankingDecision {
    MatchQuality,
    ExactCommand,
    CurrentProject,
    ActiveContext,
    Frecency,
    CapsuleId,
}

/// Compatibility score is match quality alone.
///
/// Keep this function for callers which still compute a numeric text score, but
/// never fold exact/project/context/use into that number. New application search
/// should consume the typed Resource/Resolve service instead.
pub fn score(_query: &Query, _doc: &SearchDoc, text_score: f32, _signals: &RankingSignals) -> f32 {
    text_score
}

/// Order two already-matched compatibility rows using the accepted deterministic
/// `z` law. Higher text quality wins outright; every other signal is a tiebreak.
pub fn compare(
    query: &Query,
    left: (&SearchDoc, f32),
    right: (&SearchDoc, f32),
    signals: &RankingSignals,
) -> Ordering {
    let (left_doc, left_text_score) = left;
    let (right_doc, right_text_score) = right;

    right_text_score
        .partial_cmp(&left_text_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| {
            right_doc
                .is_exact_command_match(query)
                .cmp(&left_doc.is_exact_command_match(query))
        })
        .then_with(|| right_doc.in_current_project.cmp(&left_doc.in_current_project))
        .then_with(|| right_doc.in_active_context.cmp(&left_doc.in_active_context))
        .then_with(|| {
            right_doc
                .usage
                .frecency(signals.usage_half_life)
                .partial_cmp(&left_doc.usage.frecency(signals.usage_half_life))
                .unwrap_or(Ordering::Equal)
        })
        .then_with(|| left_doc.id.cmp(&right_doc.id))
}

/// State which field first distinguishes two rows under [`compare`].
pub fn deciding_signal(
    query: &Query,
    left: (&SearchDoc, f32),
    right: (&SearchDoc, f32),
    signals: &RankingSignals,
) -> Option<RankingDecision> {
    let (left_doc, left_text_score) = left;
    let (right_doc, right_text_score) = right;
    if left_text_score != right_text_score {
        return Some(RankingDecision::MatchQuality);
    }
    if left_doc.is_exact_command_match(query) != right_doc.is_exact_command_match(query) {
        return Some(RankingDecision::ExactCommand);
    }
    if left_doc.in_current_project != right_doc.in_current_project {
        return Some(RankingDecision::CurrentProject);
    }
    if left_doc.in_active_context != right_doc.in_active_context {
        return Some(RankingDecision::ActiveContext);
    }
    if left_doc.usage.frecency(signals.usage_half_life)
        != right_doc.usage.frecency(signals.usage_half_life)
    {
        return Some(RankingDecision::Frecency);
    }
    (left_doc.id != right_doc.id).then_some(RankingDecision::CapsuleId)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str) -> SearchDoc {
        SearchDoc {
            id: CapsuleId::parse(id).unwrap(),
            kind: Kind::Script,
            name: id.to_string(),
            description: String::new(),
            tags: Vec::new(),
            exports: Vec::new(),
            status: DocStatus::Inactive,
            scope: None,
            trust: TrustState::Trusted,
            in_current_project: false,
            in_active_context: false,
            runnable: true,
            usage: UsageStats::default(),
        }
    }

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
    fn compatibility_score_is_match_quality_only() {
        let query = parse_query("alpha");
        let mut contextual = doc("script:alpha-contextual");
        contextual.in_current_project = true;
        contextual.in_active_context = true;
        contextual.usage = UsageStats {
            successful_runs: u32::MAX,
            failed_runs: 0,
            last_success_age: Some(Duration::ZERO),
        };
        assert_eq!(
            score(&query, &contextual, 0.42, &RankingSignals::default()),
            0.42
        );
    }

    #[test]
    fn stronger_match_cannot_be_overpowered_by_context_or_frecency() {
        let query = parse_query("alpha");
        let stronger = doc("script:stronger");
        let mut habitual = doc("script:habitual");
        habitual.in_current_project = true;
        habitual.in_active_context = true;
        habitual.usage = UsageStats {
            successful_runs: u32::MAX,
            failed_runs: 0,
            last_success_age: Some(Duration::ZERO),
        };
        let signals = RankingSignals::default();
        assert_eq!(
            compare(&query, (&stronger, 0.9), (&habitual, 0.8), &signals),
            Ordering::Less
        );
        assert_eq!(
            deciding_signal(&query, (&stronger, 0.9), (&habitual, 0.8), &signals),
            Some(RankingDecision::MatchQuality)
        );
    }

    #[test]
    fn equal_matches_follow_exact_project_context_frecency_then_id() {
        let mut exact = doc("script:exact");
        exact.exports.push("alpha".into());
        let mut project = doc("script:project");
        project.in_current_project = true;
        let mut active = doc("script:active");
        active.in_active_context = true;
        let mut used = doc("script:used");
        used.usage = UsageStats {
            successful_runs: 10,
            failed_runs: 0,
            last_success_age: Some(Duration::ZERO),
        };
        let plain = doc("script:plain");
        let query = parse_query("alpha");
        let signals = RankingSignals::default();

        assert_eq!(
            deciding_signal(&query, (&exact, 1.0), (&project, 1.0), &signals),
            Some(RankingDecision::ExactCommand)
        );
        assert_eq!(
            deciding_signal(&query, (&project, 1.0), (&active, 1.0), &signals),
            Some(RankingDecision::CurrentProject)
        );
        assert_eq!(
            deciding_signal(&query, (&active, 1.0), (&used, 1.0), &signals),
            Some(RankingDecision::ActiveContext)
        );
        assert_eq!(
            deciding_signal(&query, (&used, 1.0), (&plain, 1.0), &signals),
            Some(RankingDecision::Frecency)
        );
    }
}