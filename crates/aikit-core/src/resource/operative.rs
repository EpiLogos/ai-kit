//! General O:I operative address/relation syntax over native `ResourceRef`s.
//!
//! This layer is deliberately provider-neutral.  It gives CLI/TUI/structured
//! Agent clients one typed expression object and one deterministic heterogeneous
//! resolver without requiring QL-MEF.  A richer provider may attach its own
//! reading to the same expression/path; it does not replace this substrate.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::context_resolution::{Availability, ContextResolution};
use crate::{AikitError, Result};

use super::{OwnerRef, ResourceIndex, ResourceKind, ResourceRecord, ResourceRef, ResourceSource};

pub const OPERATIVE_SYNTAX_VERSION: &str = "aikit.operative-resolve/v1";

/// Soft relational horizon through which an address is being resolved.
///
/// These are not exclusive resource kinds.  The same canonical `ResourceRef`
/// may participate in several horizons because of its relations/context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AddressHorizon {
    H0,
    H1,
    H2,
    H3,
    H4,
    H5,
}

impl AddressHorizon {
    pub const ALL: [Self; 6] = [Self::H0, Self::H1, Self::H2, Self::H3, Self::H4, Self::H5];

    pub fn index(self) -> u8 {
        match self {
            Self::H0 => 0,
            Self::H1 => 1,
            Self::H2 => 2,
            Self::H3 => 3,
            Self::H4 => 4,
            Self::H5 => 5,
        }
    }

    pub fn meaning(self) -> &'static str {
        match self {
            Self::H0 => "ground / knowing / available knowledge",
            Self::H1 => "original / determining structure",
            Self::H2 => "reflection / meaning",
            Self::H3 => "language / form",
            Self::H4 => "world / context / story",
            Self::H5 => "power / techne / praxis",
        }
    }
}

impl fmt::Display for AddressHorizon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.index())
    }
}

/// Operative relation carried by syntax and retained on a resolved path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationOp {
    Potential,
    Distinguish,
    Affirm,
    Relate,
    Contextualise,
    Express,
}

impl RelationOp {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Potential => "@#",
            Self::Distinguish => "-",
            Self::Affirm => "+",
            Self::Relate => "x",
            Self::Contextualise => "/",
            Self::Express => "=",
        }
    }
}

/// Shared AST constructed directly by structured clients or by the text parser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "kebab-case")]
pub enum ResolveExpression {
    Subject {
        value: String,
    },
    Address {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        horizon: Option<AddressHorizon>,
        expression: Box<ResolveExpression>,
    },
    Unary {
        op: RelationOp,
        expression: Box<ResolveExpression>,
    },
    Binary {
        op: RelationOp,
        left: Box<ResolveExpression>,
        right: Box<ResolveExpression>,
    },
    Frame {
        expression: Box<ResolveExpression>,
    },
}

impl ResolveExpression {
    pub fn subject(value: impl Into<String>) -> Self {
        Self::Subject {
            value: value.into(),
        }
    }

    pub fn universal(expression: Self) -> Self {
        Self::Address {
            horizon: None,
            expression: Box::new(expression),
        }
    }

    pub fn horizon(horizon: AddressHorizon, expression: Self) -> Self {
        Self::Address {
            horizon: Some(horizon),
            expression: Box::new(expression),
        }
    }

    pub fn potential(expression: Self) -> Self {
        Self::Unary {
            op: RelationOp::Potential,
            expression: Box::new(expression),
        }
    }

    /// Ordinary unqualified Search is the potential resolution of a universal
    /// address.  This helper makes that semantic lowering explicit without
    /// forcing human callers to type the expanded form.
    pub fn ordinary_search(value: impl Into<String>) -> Self {
        Self::potential(Self::universal(Self::subject(value)))
    }

    pub fn render(&self) -> String {
        match self {
            Self::Subject { value } => render_subject(value),
            Self::Address {
                horizon,
                expression,
            } => format!(
                "{} {}",
                horizon.map_or_else(|| "@".to_string(), |value| value.to_string()),
                render_child(expression)
            ),
            Self::Unary { op, expression } => {
                format!("{} {}", op.symbol(), render_child(expression))
            }
            Self::Binary { op, left, right } => format!(
                "{} {} {}",
                render_child(left),
                op.symbol(),
                render_child(right)
            ),
            Self::Frame { expression } => format!("( {} )", expression.render()),
        }
    }
}

fn render_child(expression: &ResolveExpression) -> String {
    match expression {
        ResolveExpression::Binary { .. } => format!("( {} )", expression.render()),
        _ => expression.render(),
    }
}

fn render_subject(value: &str) -> String {
    let reserved = matches!(
        value,
        "@" | "@#"
            | "@0"
            | "@1"
            | "@2"
            | "@3"
            | "@4"
            | "@5"
            | "-"
            | "+"
            | "x"
            | "/"
            | "="
            | "("
            | ")"
    );
    if value.is_empty()
        || reserved
        || value.chars().any(char::is_whitespace)
        || value.starts_with('@')
    {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Address(Option<AddressHorizon>),
    Relation(RelationOp),
    LParen,
    RParen,
    Atom(String),
}

/// Parse the literal operative syntax.  Operators are recognized only at
/// expression boundaries, so punctuation inside filesystem paths, refs,
/// `key=value`, hyphenated identifiers and ordinary words remains literal.
pub fn parse_resolve_expression(raw: &str) -> Result<ResolveExpression> {
    let tokens = lex(raw)?;
    if tokens.is_empty() {
        return Err(AikitError::new(
            "resolve.empty_expression",
            "operative Resolve expression is empty",
        ));
    }
    let mut parser = Parser { tokens, cursor: 0 };
    let expression = parser.parse_expression()?;
    if parser.peek().is_some() {
        return Err(AikitError::new(
            "resolve.trailing_expression",
            "unexpected material after operative Resolve expression",
        ));
    }
    Ok(expression)
}

/// Human-shell entry point: syntax is parsed when present; plain text is lowered
/// to `@# (@ text)` rather than being assigned a special front-character lane.
pub fn parse_or_search_expression(raw: &str) -> Result<ResolveExpression> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(ResolveExpression::ordinary_search(""));
    }
    if trimmed.starts_with('"') || trimmed.starts_with('\'') || trimmed.contains('\\') {
        return match parse_resolve_expression(trimmed)? {
            ResolveExpression::Subject { value } => Ok(ResolveExpression::ordinary_search(value)),
            expression => Ok(expression),
        };
    }
    if has_operative_syntax(trimmed) {
        parse_resolve_expression(trimmed)
    } else {
        Ok(ResolveExpression::ordinary_search(trimmed))
    }
}

fn has_operative_syntax(raw: &str) -> bool {
    let Ok(tokens) = lex(raw) else {
        return true;
    };
    tokens.iter().any(|token| !matches!(token, Token::Atom(_)))
}

fn lex(raw: &str) -> Result<Vec<Token>> {
    let chars = raw.char_indices().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (_, ch) = chars[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '(' {
            tokens.push(Token::LParen);
            i += 1;
            continue;
        }
        if ch == ')' {
            tokens.push(Token::RParen);
            i += 1;
            continue;
        }
        if ch == '\'' || ch == '"' {
            let quote = ch;
            i += 1;
            let mut value = String::new();
            let mut closed = false;
            while i < chars.len() {
                let (_, current) = chars[i];
                if current == '\\' {
                    i += 1;
                    if i >= chars.len() {
                        return Err(AikitError::new(
                            "resolve.dangling_escape",
                            "quoted Resolve subject ends with an escape",
                        ));
                    }
                    value.push(chars[i].1);
                    i += 1;
                } else if current == quote {
                    i += 1;
                    closed = true;
                    break;
                } else {
                    value.push(current);
                    i += 1;
                }
            }
            if !closed {
                return Err(AikitError::new(
                    "resolve.unclosed_quote",
                    "quoted Resolve subject is not closed",
                ));
            }
            tokens.push(Token::Atom(value));
            continue;
        }

        let previous_boundary =
            i == 0 || chars[i - 1].1.is_whitespace() || matches!(chars[i - 1].1, '(' | ')');
        let next = |offset: usize| chars.get(i + offset).map(|(_, value)| *value);
        let boundary_after = |offset: usize| {
            next(offset).is_none_or(|value| value.is_whitespace() || matches!(value, '(' | ')'))
        };

        if ch == '@' && previous_boundary {
            if next(1) == Some('#') && boundary_after(2) {
                tokens.push(Token::Relation(RelationOp::Potential));
                i += 2;
                continue;
            }
            if let Some(digit @ '0'..='5') = next(1) {
                if boundary_after(2) {
                    let horizon = match digit {
                        '0' => AddressHorizon::H0,
                        '1' => AddressHorizon::H1,
                        '2' => AddressHorizon::H2,
                        '3' => AddressHorizon::H3,
                        '4' => AddressHorizon::H4,
                        '5' => AddressHorizon::H5,
                        _ => unreachable!(),
                    };
                    tokens.push(Token::Address(Some(horizon)));
                    i += 2;
                    continue;
                }
            }
            if boundary_after(1) {
                tokens.push(Token::Address(None));
                i += 1;
                continue;
            }
        }

        if previous_boundary && boundary_after(1) {
            let relation = match ch {
                '-' => Some(RelationOp::Distinguish),
                '+' => Some(RelationOp::Affirm),
                'x' => Some(RelationOp::Relate),
                '/' => Some(RelationOp::Contextualise),
                '=' => Some(RelationOp::Express),
                _ => None,
            };
            if let Some(relation) = relation {
                tokens.push(Token::Relation(relation));
                i += 1;
                continue;
            }
        }

        let mut value = String::new();
        while i < chars.len() {
            let (_, current) = chars[i];
            if current.is_whitespace() || matches!(current, '(' | ')') {
                break;
            }
            if current == '\\' {
                i += 1;
                if i >= chars.len() {
                    return Err(AikitError::new(
                        "resolve.dangling_escape",
                        "Resolve subject ends with an escape",
                    ));
                }
                value.push(chars[i].1);
                i += 1;
            } else {
                value.push(current);
                i += 1;
            }
        }
        if !value.is_empty() {
            tokens.push(Token::Atom(value));
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.cursor).cloned();
        self.cursor += usize::from(token.is_some());
        token
    }

    fn parse_expression(&mut self) -> Result<ResolveExpression> {
        self.parse_express()
    }

    fn parse_express(&mut self) -> Result<ResolveExpression> {
        let mut left = self.parse_context()?;
        while matches!(self.peek(), Some(Token::Relation(RelationOp::Express))) {
            self.next();
            let right = self.parse_context()?;
            left = ResolveExpression::Binary {
                op: RelationOp::Express,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_context(&mut self) -> Result<ResolveExpression> {
        let mut left = self.parse_relate()?;
        while matches!(
            self.peek(),
            Some(Token::Relation(RelationOp::Contextualise))
        ) {
            self.next();
            let right = self.parse_relate()?;
            left = ResolveExpression::Binary {
                op: RelationOp::Contextualise,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_relate(&mut self) -> Result<ResolveExpression> {
        let mut left = self.parse_unary()?;
        while matches!(self.peek(), Some(Token::Relation(RelationOp::Relate))) {
            self.next();
            let right = self.parse_unary()?;
            left = ResolveExpression::Binary {
                op: RelationOp::Relate,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<ResolveExpression> {
        match self.peek().cloned() {
            Some(Token::Address(horizon)) => {
                self.next();
                let expression =
                    if self.peek().is_none() || matches!(self.peek(), Some(Token::RParen)) {
                        ResolveExpression::Subject {
                            value: String::new(),
                        }
                    } else {
                        self.parse_unary()?
                    };
                Ok(ResolveExpression::Address {
                    horizon,
                    expression: Box::new(expression),
                })
            }
            Some(Token::Relation(
                op @ (RelationOp::Potential | RelationOp::Distinguish | RelationOp::Affirm),
            )) => {
                self.next();
                Ok(ResolveExpression::Unary {
                    op,
                    expression: Box::new(self.parse_unary()?),
                })
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<ResolveExpression> {
        match self.next() {
            Some(Token::LParen) => {
                let expression = self.parse_expression()?;
                match self.next() {
                    Some(Token::RParen) => Ok(ResolveExpression::Frame {
                        expression: Box::new(expression),
                    }),
                    _ => Err(AikitError::new(
                        "resolve.unclosed_frame",
                        "operative Resolve frame is not closed",
                    )),
                }
            }
            Some(Token::Atom(first)) => {
                let mut parts = vec![first];
                while matches!(self.peek(), Some(Token::Atom(_))) {
                    if let Some(Token::Atom(value)) = self.next() {
                        parts.push(value);
                    }
                }
                Ok(ResolveExpression::Subject {
                    value: parts.join(" "),
                })
            }
            Some(Token::RParen) => Err(AikitError::new(
                "resolve.unexpected_frame_end",
                "unexpected ')' in operative Resolve expression",
            )),
            Some(Token::Relation(op)) => Err(AikitError::new(
                "resolve.unexpected_relation",
                format!("unexpected relation operator {}", op.symbol()),
            )),
            Some(Token::Address(_)) => unreachable!("addresses are consumed by parse_unary"),
            None => Err(AikitError::new(
                "resolve.missing_subject",
                "operative Resolve expression is missing a subject",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveCandidate {
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub horizons: BTreeSet<AddressHorizon>,
    pub exact: bool,
    /// Primary textual/exact relevance. Derived ranking signals remain separate
    /// so contextual or learned use cannot numerically overpower relevance.
    pub score: i64,
    #[serde(default)]
    pub ranking: super::ResolveRankingSignals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ResolvePathStep {
    Subject {
        value: String,
        candidates: Vec<ResourceRef>,
    },
    Address {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        horizon: Option<AddressHorizon>,
        candidates: Vec<ResourceRef>,
    },
    Relation {
        op: RelationOp,
    },
    Frame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvePath {
    pub version: String,
    pub identity: String,
    pub expression: ResolveExpression,
    pub steps: Vec<ResolvePathStep>,
    pub candidates: Vec<ResolveCandidate>,
}

impl ResolvePath {
    pub fn destination(&self) -> Option<&ResourceRef> {
        self.candidates.first().map(|candidate| &candidate.resource)
    }
}

/// Resolve one expression against the heterogeneous Resource field. Exact
/// ResourceRef/text relevance remains primary; authored and present-context
/// evidence then order otherwise comparable candidates before learned familiarity.
pub fn resolve_expression(
    expression: &ResolveExpression,
    resources: &dyn ResourceIndex,
    limit: usize,
) -> ResolvePath {
    let identity = resolve_path_identity(expression);
    let mut steps = Vec::new();
    let mut candidates = evaluate(expression, resources, &identity, &mut steps);
    candidates.sort_by(|left, right| {
        right
            .exact
            .cmp(&left.exact)
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| {
                right
                    .ranking
                    .authored_preference_rank
                    .is_some()
                    .cmp(&left.ranking.authored_preference_rank.is_some())
            })
            .then_with(|| {
                right
                    .ranking
                    .authored_preference_rank
                    .unwrap_or_default()
                    .cmp(&left.ranking.authored_preference_rank.unwrap_or_default())
            })
            .then_with(|| right.ranking.current_project.cmp(&left.ranking.current_project))
            .then_with(|| right.ranking.active_in_context.cmp(&left.ranking.active_in_context))
            .then_with(|| {
                right
                    .ranking
                    .learned_path_contextual_frecency_milli
                    .cmp(&left.ranking.learned_path_contextual_frecency_milli)
            })
            .then_with(|| {
                right
                    .ranking
                    .learned_path_contextual_fitness_milli
                    .unwrap_or_default()
                    .cmp(
                        &left
                            .ranking
                            .learned_path_contextual_fitness_milli
                            .unwrap_or_default(),
                    )
            })
            .then_with(|| {
                right
                    .ranking
                    .learned_path_frecency_milli
                    .cmp(&left.ranking.learned_path_frecency_milli)
            })
            .then_with(|| {
                right
                    .ranking
                    .learned_contextual_frecency_milli
                    .cmp(&left.ranking.learned_contextual_frecency_milli)
            })
            .then_with(|| {
                right
                    .ranking
                    .learned_contextual_fitness_milli
                    .unwrap_or_default()
                    .cmp(
                        &left
                            .ranking
                            .learned_contextual_fitness_milli
                            .unwrap_or_default(),
                    )
            })
            .then_with(|| {
                right
                    .ranking
                    .learned_frecency_milli
                    .cmp(&left.ranking.learned_frecency_milli)
            })
            .then_with(|| left.resource.cmp(&right.resource))
    });
    candidates.dedup_by(|left, right| left.resource == right.resource);
    if limit > 0 {
        candidates.truncate(limit);
    } else {
        candidates.clear();
    }
    ResolvePath {
        version: OPERATIVE_SYNTAX_VERSION.into(),
        identity,
        expression: expression.clone(),
        steps,
        candidates,
    }
}

pub fn resolve_search(query: &str, resources: &dyn ResourceIndex, limit: usize) -> ResolvePath {
    resolve_expression(
        &ResolveExpression::ordinary_search(query.trim()),
        resources,
        limit,
    )
}

fn evaluate(
    expression: &ResolveExpression,
    resources: &dyn ResourceIndex,
    path_identity: &str,
    steps: &mut Vec<ResolvePathStep>,
) -> Vec<ResolveCandidate> {
    match expression {
        ResolveExpression::Subject { value } => {
            let candidates = subject_candidates(value, resources, path_identity);
            steps.push(ResolvePathStep::Subject {
                value: value.clone(),
                candidates: candidates
                    .iter()
                    .map(|candidate| candidate.resource.clone())
                    .collect(),
            });
            candidates
        }
        ResolveExpression::Address {
            horizon,
            expression,
        } => {
            let mut candidates = evaluate(expression, resources, path_identity, steps);
            if let Some(horizon) = horizon {
                candidates.retain(|candidate| candidate.horizons.contains(horizon));
            }
            steps.push(ResolvePathStep::Address {
                horizon: *horizon,
                candidates: candidates
                    .iter()
                    .map(|candidate| candidate.resource.clone())
                    .collect(),
            });
            candidates
        }
        ResolveExpression::Unary { op, expression } => {
            let candidates = evaluate(expression, resources, path_identity, steps);
            steps.push(ResolvePathStep::Relation { op: *op });
            candidates
        }
        ResolveExpression::Binary { op, left, right } => {
            let mut candidates = evaluate(left, resources, path_identity, steps);
            let right = evaluate(right, resources, path_identity, steps);
            steps.push(ResolvePathStep::Relation { op: *op });
            candidates.extend(right);
            dedupe_candidates(candidates)
        }
        ResolveExpression::Frame { expression } => {
            steps.push(ResolvePathStep::Frame);
            evaluate(expression, resources, path_identity, steps)
        }
    }
}

fn dedupe_candidates(candidates: Vec<ResolveCandidate>) -> Vec<ResolveCandidate> {
    let mut by_ref = BTreeMap::<ResourceRef, ResolveCandidate>::new();
    for candidate in candidates {
        by_ref
            .entry(candidate.resource.clone())
            .and_modify(|existing| {
                existing.exact |= candidate.exact;
                existing.score = existing.score.max(candidate.score);
                existing.horizons.extend(candidate.horizons.iter().copied());
            })
            .or_insert(candidate);
    }
    by_ref.into_values().collect()
}

fn subject_candidates(
    value: &str,
    resources: &dyn ResourceIndex,
    path_identity: &str,
) -> Vec<ResolveCandidate> {
    let query = value.trim().to_lowercase();
    let exact_ref = ResourceRef::parse(value.trim()).ok();
    resources
        .resources()
        .into_iter()
        .filter_map(|record| {
            let exact = exact_ref
                .as_ref()
                .is_some_and(|reference| reference == &record.descriptor.id);
            let score = if exact {
                Some(100_000)
            } else if query.is_empty() {
                Some(0)
            } else {
                text_score(&query, record)
            }?;
            Some(ResolveCandidate {
                resource: record.descriptor.id.clone(),
                kind: record.descriptor.kind,
                horizons: horizons_for_resource(record),
                exact,
                score,
                ranking: resources.resolve_path_ranking(path_identity, &record.descriptor.id),
            })
        })
        .collect()
}

fn text_score(query: &str, record: &ResourceRecord) -> Option<i64> {
    let id = record.descriptor.id.as_str().to_lowercase();
    let name = record.descriptor.name.to_lowercase();
    let description = record.descriptor.description.to_lowercase();
    if id == query || name == query {
        return Some(90_000);
    }

    for annotation in ["aikit.search-exports", "aikit.search-tags"] {
        if let Some(handles) = record.descriptor.annotations.get(annotation) {
            for handle in handles.split(',').map(str::trim).filter(|value| !value.is_empty()) {
                let handle = handle.to_lowercase();
                if handle == query {
                    return Some(90_000);
                }
            }
        }
    }

    let mut score = None;
    if id.starts_with(query) || name.starts_with(query) {
        score = score.max(Some(20_000 - query.len() as i64));
    }
    if id.contains(query) || name.contains(query) {
        score = score.max(Some(10_000 - query.len() as i64));
    }
    if description.contains(query) {
        score = score.max(Some(5_000 - query.len() as i64));
    }
    for annotation in ["aikit.search-exports", "aikit.search-tags"] {
        if let Some(handles) = record.descriptor.annotations.get(annotation) {
            for handle in handles.split(',').map(str::trim).filter(|value| !value.is_empty()) {
                let handle = handle.to_lowercase();
                if handle.starts_with(query) {
                    score = score.max(Some(20_000 - query.len() as i64));
                } else if handle.contains(query) {
                    score = score.max(Some(10_000 - query.len() as i64));
                }
            }
        }
    }
    if score.is_some() {
        return score;
    }

    let terms = query.split_whitespace().collect::<Vec<_>>();
    let annotations_match = |term: &&str| {
        ["aikit.search-exports", "aikit.search-tags"]
            .iter()
            .filter_map(|key| record.descriptor.annotations.get(*key))
            .any(|value| value.to_lowercase().contains(*term))
    };
    (!terms.is_empty()
        && terms.iter().all(|term| {
            id.contains(term) || name.contains(term) || description.contains(term) || annotations_match(term)
        }))
    .then_some(1_000 - terms.len() as i64)
}

/// Native default horizon participation.  `oi.address-horizons` may add further
/// comma/space-separated `0..5` positions without changing canonical identity.
pub fn horizons_for_resource(record: &ResourceRecord) -> BTreeSet<AddressHorizon> {
    let mut horizons = match record.descriptor.kind {
        ResourceKind::KnowledgeSource
        | ResourceKind::KnowledgeSpace
        | ResourceKind::ContextSource => BTreeSet::from([AddressHorizon::H0]),
        ResourceKind::KnowledgeNode
        | ResourceKind::KnowledgeFrame
        | ResourceKind::KnowledgeRoute => BTreeSet::from([AddressHorizon::H0, AddressHorizon::H2]),
        ResourceKind::CodeReference | ResourceKind::Contract | ResourceKind::Component => {
            BTreeSet::from([AddressHorizon::H1, AddressHorizon::H3])
        }
        ResourceKind::Profile => BTreeSet::from([AddressHorizon::H1, AddressHorizon::H4]),
        ResourceKind::Method => BTreeSet::from([AddressHorizon::H2, AddressHorizon::H5]),
        ResourceKind::Project => BTreeSet::from([AddressHorizon::H4]),
        ResourceKind::Agent | ResourceKind::Agency => {
            BTreeSet::from([AddressHorizon::H2, AddressHorizon::H4, AddressHorizon::H5])
        }
        ResourceKind::Surface => {
            BTreeSet::from([AddressHorizon::H3, AddressHorizon::H4, AddressHorizon::H5])
        }
        ResourceKind::SkillSet
        | ResourceKind::Capability
        | ResourceKind::Action
        | ResourceKind::Procedure
        | ResourceKind::Harness
        | ResourceKind::ExecutionOffer => BTreeSet::from([AddressHorizon::H5]),
        ResourceKind::Model | ResourceKind::Host => {
            BTreeSet::from([AddressHorizon::H1, AddressHorizon::H4])
        }
    };
    if let Some(extra) = record.descriptor.annotations.get("oi.address-horizons") {
        for value in extra.split(|ch: char| ch == ',' || ch.is_whitespace()) {
            let horizon = match value.trim().trim_start_matches('@').trim_start_matches('h') {
                "0" => Some(AddressHorizon::H0),
                "1" => Some(AddressHorizon::H1),
                "2" => Some(AddressHorizon::H2),
                "3" => Some(AddressHorizon::H3),
                "4" => Some(AddressHorizon::H4),
                "5" => Some(AddressHorizon::H5),
                _ => None,
            };
            if let Some(horizon) = horizon {
                horizons.insert(horizon);
            }
        }
    }
    horizons
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionRef(pub ResourceRef);

impl ActionRef {
    pub fn parse(reference: ResourceRef, resources: &dyn ResourceIndex) -> Result<Self> {
        let Some(record) = resources.resource(&reference) else {
            return Err(AikitError::new(
                "resolve.action_missing",
                format!("ActionRef {reference} is absent from the Resource field"),
            ));
        };
        if record.descriptor.kind != ResourceKind::Action {
            return Err(AikitError::new(
                "resolve.action_wrong_kind",
                format!(
                    "{reference} is {}, not an Action",
                    record.descriptor.kind.as_str()
                ),
            ));
        }
        Ok(Self(reference))
    }

    pub fn resource(&self) -> &ResourceRef {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedActionCandidate {
    pub action: ActionRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon: Option<AddressHorizon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<RelationOp>,
    /// ContextResolution remains the authority for whether this Action is
    /// actually available for invocation in the current world.
    pub available_in_context: bool,
}

/// Contextual semantic qualification of one real ActionRef. The profile is
/// derived from current ResolvePath + Resource/subject evidence; it is not a new
/// Action registry and it confers no execution authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionSemanticProfile {
    pub action_ref: ActionRef,
    #[serde(default)]
    pub relation_affinities: BTreeSet<RelationOp>,
    #[serde(default)]
    pub horizon_affinities: BTreeSet<AddressHorizon>,
    #[serde(default)]
    pub subject_ref_kinds: BTreeSet<ResourceKind>,
    #[serde(default)]
    pub method_relations: Vec<ResourceRef>,
    #[serde(default)]
    pub focus_relations: Vec<String>,
    #[serde(default)]
    pub expected_return_forms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_owner: Option<OwnerRef>,
    #[serde(default)]
    pub provenance: Vec<ResourceSource>,
}

/// Lift Action candidates from a path while preserving the actual Action identity
/// and asking current ContextResolution—not syntax—to decide operativity.
pub fn resolve_action_candidates(
    path: &ResolvePath,
    resources: &dyn ResourceIndex,
    context: &ContextResolution,
) -> Vec<ResolvedActionCandidate> {
    let horizon = path.steps.iter().rev().find_map(|step| match step {
        ResolvePathStep::Address { horizon, .. } => *horizon,
        _ => None,
    });
    let relation = path.steps.iter().rev().find_map(|step| match step {
        ResolvePathStep::Relation { op } => Some(*op),
        _ => None,
    });
    path.candidates
        .iter()
        .filter(|candidate| candidate.kind == ResourceKind::Action)
        .filter_map(|candidate| {
            let action = ActionRef::parse(candidate.resource.clone(), resources).ok()?;
            let available_in_context = context.actions.iter().any(|resolved| {
                resolved.resource.descriptor.id == candidate.resource
                    && matches!(resolved.availability, Availability::Available)
            });
            Some(ResolvedActionCandidate {
                action,
                horizon,
                relation,
                available_in_context,
            })
        })
        .collect()
}

/// Describe why one syntax-resolved Action is meaningful for the selected subject.
/// Relation affinities are the operators that actually participated in this
/// qualification; horizon affinities come from the general resource classifier.
/// Method and Focus relations are likewise current path/context evidence rather
/// than invented declarations. Expected return forms are native-owner annotations.
pub fn action_semantic_profile(
    candidate: &ResolvedActionCandidate,
    path: &ResolvePath,
    subject: &ResourceRef,
    focus: Option<&str>,
    resources: &dyn ResourceIndex,
) -> Result<ActionSemanticProfile> {
    let action_record = resources
        .resource(candidate.action.resource())
        .ok_or_else(|| {
            AikitError::new(
                "resolve.action_profile_missing",
                format!(
                    "Action {} disappeared during semantic qualification",
                    candidate.action.resource()
                ),
            )
        })?;
    let subject_record = resources.resource(subject).ok_or_else(|| {
        AikitError::new(
            "resolve.action_subject_missing",
            format!("Action subject {subject} is absent from the Resource field"),
        )
    })?;

    let relation_affinities = path
        .steps
        .iter()
        .filter_map(|step| match step {
            ResolvePathStep::Relation { op } => Some(*op),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let horizon_affinities = horizons_for_resource(action_record);
    let method_relations = path
        .candidates
        .iter()
        .filter(|resolved| resolved.kind == ResourceKind::Method)
        .map(|resolved| resolved.resource.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let focus_relations = focus
        .filter(|value| !value.trim().is_empty())
        .map(|value| vec![value.to_string()])
        .unwrap_or_default();
    let expected_return_forms = action_record
        .descriptor
        .annotations
        .get("action.expected-return-forms")
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    Ok(ActionSemanticProfile {
        action_ref: candidate.action.clone(),
        relation_affinities,
        horizon_affinities,
        subject_ref_kinds: BTreeSet::from([subject_record.descriptor.kind]),
        method_relations,
        focus_relations,
        expected_return_forms,
        native_owner: action_record.descriptor.owner.clone(),
        provenance: action_record.descriptor.sources.clone(),
    })
}

/// Build the thin sixfold disclosure object from real currently-addressable refs.
/// Each member remains the same canonical ref used by Search/Explain/History.
pub fn six_horizon_disclosure(
    resources: &dyn ResourceIndex,
    refs: impl IntoIterator<Item = ResourceRef>,
) -> ResolveExpression {
    let wanted = refs.into_iter().collect::<BTreeSet<_>>();
    let mut clauses = Vec::new();
    for horizon in AddressHorizon::ALL {
        let members = resources
            .resources()
            .into_iter()
            .filter(|record| wanted.contains(&record.descriptor.id))
            .filter(|record| horizons_for_resource(record).contains(&horizon))
            .map(|record| {
                ResolveExpression::horizon(
                    horizon,
                    ResolveExpression::subject(record.descriptor.id.to_string()),
                )
            })
            .collect::<Vec<_>>();
        for member in members {
            clauses.push(ResolveExpression::Unary {
                op: RelationOp::Affirm,
                expression: Box::new(member),
            });
        }
    }
    let expression = clauses
        .into_iter()
        .reduce(|left, right| ResolveExpression::Binary {
            op: RelationOp::Contextualise,
            left: Box::new(left),
            right: Box::new(right),
        })
        .unwrap_or_else(|| ResolveExpression::ordinary_search(""));
    ResolveExpression::Frame {
        expression: Box::new(expression),
    }
}

pub fn resolve_path_identity(expression: &ResolveExpression) -> String {
    stable_path_identity(&expression.render())
}

fn stable_path_identity(rendered: &str) -> String {
    // FNV-1a is not an authority/content-security hash; it is a stable compact
    // identity for matching familiarity observations of the same rendered AST.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in rendered.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("resolve-path:{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{MemoryResourceIndex, ResourceDescriptor};

    fn record(id: &str, kind: ResourceKind) -> ResourceRecord {
        ResourceRecord::new(ResourceDescriptor::new(
            ResourceRef::parse(id).unwrap(),
            kind,
            id,
            id,
        ))
    }

    #[test]
    fn literal_syntax_round_trips_as_one_ast() {
        let cases = [
            "@ project:demo",
            "@0 knowledge:ground",
            "@# ( @ method:orient )",
            "- @1 source:canon",
            "+ @5 action:verify",
            "@2 reflection:one x @3 language:form",
            "@4 project:demo / @5 method:operate",
            "@ subject:a = @ subject:b",
            "( + @0 knowledge:ground / + @5 action:verify )",
        ];
        for raw in cases {
            let parsed = parse_resolve_expression(raw).unwrap();
            let rendered = parsed.render();
            let reparsed = parse_resolve_expression(&rendered).unwrap();
            assert_eq!(parsed, reparsed, "{raw} -> {rendered}");
        }
    }

    #[test]
    fn punctuation_inside_real_world_subjects_is_literal() {
        let cases = [
            "/home/satya/Central/Work/demo",
            "source:repo/path/to/file",
            "key=value",
            "agent-native-runtime",
            "matrix",
            "project:demo/ref=x-y/z",
            "\"literal x / + @5 = material\"",
            "source:escaped\\ x",
        ];
        for raw in cases {
            let expression = parse_or_search_expression(raw).unwrap();
            assert_eq!(
                expression,
                ResolveExpression::ordinary_search(match raw {
                    "\"literal x / + @5 = material\"" => "literal x / + @5 = material",
                    "source:escaped\\ x" => "source:escaped x",
                    _ => raw,
                })
            );
        }
    }
    #[test]
    fn operator_boundaries_do_not_steal_exact_resource_refs() {
        let mut resources = MemoryResourceIndex::default();
        for id in [
            "/tmp/a/b",
            "source:repo/path",
            "config:key=value",
            "agent-native-runtime",
            "literal-x",
        ] {
            resources.insert(record(id, ResourceKind::KnowledgeSource));
            let path = resolve_search(id, &resources, 10);
            assert_eq!(path.destination().map(ResourceRef::as_str), Some(id));
            assert!(path.candidates[0].exact);
        }
    }

    #[test]
    fn horizon_is_soft_and_can_be_extended_without_identity_duplication() {
        let mut record = record("method:orient", ResourceKind::Method);
        record
            .descriptor
            .annotations
            .insert("oi.address-horizons".into(), "0,3".into());
        let horizons = horizons_for_resource(&record);
        assert!(horizons.contains(&AddressHorizon::H0));
        assert!(horizons.contains(&AddressHorizon::H2));
        assert!(horizons.contains(&AddressHorizon::H3));
        assert!(horizons.contains(&AddressHorizon::H5));
    }

    #[test]
    fn ordinary_search_is_potential_universal_resolution() {
        let expression = parse_or_search_expression("orient project").unwrap();
        assert_eq!(
            expression,
            ResolveExpression::ordinary_search("orient project")
        );
        assert_eq!(expression.render(), "@# @ \"orient project\"");
    }

    #[test]
    fn bare_horizon_is_an_open_aperture_and_potential_horizon_is_executable() {
        let mut resources = MemoryResourceIndex::default();
        resources.insert(record("action:verify", ResourceKind::Action));
        resources.insert(record("project:demo", ResourceKind::Project));

        let horizon = parse_resolve_expression("@5").unwrap();
        let path = resolve_expression(&horizon, &resources, 16);
        assert_eq!(horizon.render(), "@5 \"\"");
        assert_eq!(path.candidates.len(), 1);
        assert_eq!(path.candidates[0].resource.as_str(), "action:verify");

        let potential = parse_resolve_expression("@# @5").unwrap();
        let potential_path = resolve_expression(&potential, &resources, 16);
        assert_eq!(potential_path.candidates.len(), 1);
        assert_eq!(
            potential_path.candidates[0].resource.as_str(),
            "action:verify"
        );
    }

    #[test]
    fn heterogeneous_resolve_returns_real_refs_and_action_type_is_checked() {
        let mut resources = MemoryResourceIndex::default();
        resources.insert(record("project:demo", ResourceKind::Project));
        resources.insert(record("knowledge:ground", ResourceKind::KnowledgeSource));
        resources.insert(record("method:orient", ResourceKind::Method));
        resources.insert(record("action:verify", ResourceKind::Action));

        let expression =
            parse_resolve_expression("+ @4 project:demo / + @5 action:verify").unwrap();
        let path = resolve_expression(&expression, &resources, 10);
        assert!(path
            .candidates
            .iter()
            .any(|candidate| candidate.resource.as_str() == "project:demo"));
        assert!(path
            .candidates
            .iter()
            .any(|candidate| candidate.resource.as_str() == "action:verify"));
        assert!(ActionRef::parse(ResourceRef::parse("action:verify").unwrap(), &resources).is_ok());
        assert!(
            ActionRef::parse(ResourceRef::parse("method:orient").unwrap(), &resources).is_err()
        );
    }

    #[test]
    fn search_handles_are_part_of_resolve_without_changing_identity() {
        let mut descriptor = ResourceDescriptor::new(
            ResourceRef::parse("script/cargo-nextest").unwrap(),
            ResourceKind::Capability,
            "Cargo nextest",
            "test runner",
        );
        descriptor
            .annotations
            .insert("aikit.search-exports".into(), "nextest,nx".into());
        let mut resources = MemoryResourceIndex::default();
        resources.insert(ResourceRecord::new(descriptor));

        let path = resolve_search("nx", &resources, 10);
        assert_eq!(
            path.destination().map(ResourceRef::as_str),
            Some("script/cargo-nextest")
        );
    }

    #[test]
    fn disclosure_uses_same_refs_and_all_six_horizons_when_present() {
        let mut resources = MemoryResourceIndex::default();
        let specimens = [
            ("knowledge:ground", ResourceKind::KnowledgeSource),
            ("code:schema", ResourceKind::CodeReference),
            ("wiki:reading", ResourceKind::KnowledgeNode),
            ("surface:cli", ResourceKind::Surface),
            ("project:demo", ResourceKind::Project),
            ("action:verify", ResourceKind::Action),
        ];
        for (id, kind) in specimens {
            resources.insert(record(id, kind));
        }
        let expression = six_horizon_disclosure(
            &resources,
            specimens
                .iter()
                .map(|(id, _)| ResourceRef::parse(id).unwrap()),
        );
        let rendered = expression.render();
        for horizon in AddressHorizon::ALL {
            assert!(rendered.contains(&horizon.to_string()), "{rendered}");
        }
        let path = resolve_expression(&expression, &resources, 64);
        assert!(!path.candidates.is_empty());
    }
}