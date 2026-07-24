//! Fuzzy matching, in this process.
//!
//! ## Why nucleo and not `fzf`
//!
//! The keystroke budget is 16 ms (`ARCHITECTURE.md` §13). A subprocess per
//! keystroke cannot meet it, and piping the catalog through a chooser would move
//! the ranking policy into a shell pipeline where neither `aikit search --json`
//! nor a test could see it. nucleo runs in-process against a `Vec<SearchDoc>` the
//! palette already holds.
//!
//! ## Matching is ours, ranking is core's
//!
//! This module answers exactly one question — *how well does this text match what
//! the user typed* — and normalizes the answer into `[0, 1]`. Everything after
//! that is [`aikit_core::search::score`]: what a query means, which documents a
//! filter admits, whether an exact command name beats a habit, how usage decays.
//! Duplicating any of that here would let the palette and the CLI disagree about
//! the same catalog.
//!
//! ## Normalization is per-query, not per-result-set
//!
//! A raw nucleo score is unbounded and only comparable within one needle. Dividing
//! by the score the needle achieves against *itself* gives a stable ratio: 1.0 for
//! a perfect hit, and the same number for the same pair whatever else is in the
//! list. Normalizing against the best score in the current result set would have
//! been easier and would have made a row's rank depend on its neighbours, so that
//! adding an unrelated capability to the catalog could reorder two others.
//!
//! ## This runs off the render path
//!
//! [`Matcher::rank`] is called from an effect, never from a draw call. The
//! reducer receives the finished rows as `Action::ResultsUpdated`. That seam is
//! deliberate: it is where a worker thread goes when a catalog gets large enough
//! to need one, and moving it there changes nothing else.

use aikit_core::search::{score, Query, RankingSignals, SearchDoc};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Utf32Str};

/// How much less each successive field is worth.
///
/// The field *order* is core's — [`aikit_core::capsule::Capsule::search_fields`]
/// documents it as descending weight — and these are the numbers that order
/// implies. A hit in an exported command name is the strongest signal a user can
/// give; a hit in a description is real but weak, and must not push a
/// coincidentally-worded capability above the command someone meant.
const FIELD_DECAY: f32 = 0.1;
const FIELD_FLOOR: f32 = 0.5;

/// One ranked result.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub doc: SearchDoc,
    /// The final ranking score, from [`aikit_core::search::score`].
    pub score: f32,
    /// The normalized text relevance that was fed into it. Kept so the palette
    /// can highlight and so a test can re-derive the score independently.
    pub text_score: f32,
}

/// A query compiled once per keystroke rather than once per document.
pub struct Prepared {
    pattern: Pattern,
    /// The score the needle achieves against itself: the denominator that turns
    /// an unbounded nucleo score into a ratio.
    perfect: f32,
    empty: bool,
}

/// The in-process fuzzy matcher.
///
/// Owns nucleo's scratch allocations, so the palette keeps one for the lifetime
/// of the invocation instead of rebuilding matrices on every keystroke.
pub struct Matcher {
    inner: nucleo_matcher::Matcher,
    signals: RankingSignals,
}

impl Default for Matcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Matcher {
    pub fn new() -> Self {
        Self::with_signals(RankingSignals::default())
    }

    pub fn with_signals(signals: RankingSignals) -> Self {
        Self {
            inner: nucleo_matcher::Matcher::new(Config::DEFAULT),
            signals,
        }
    }

    pub fn signals(&self) -> &RankingSignals {
        &self.signals
    }

    /// Compile a query's free text into a reusable pattern.
    pub fn prepare(&mut self, query: &Query) -> Prepared {
        let text = query.text.trim();
        let pattern = Pattern::parse(text, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let perfect = pattern
            .score(Utf32Str::new(text, &mut buf), &mut self.inner)
            .filter(|s| *s > 0)
            .map(|s| s as f32)
            .unwrap_or(1.0);
        Prepared {
            pattern,
            perfect,
            empty: text.is_empty(),
        }
    }

    /// Text relevance in `[0, 1]`, or `None` when the query excludes the document.
    ///
    /// An empty query is not an exclusion and not a match: it says nothing, so
    /// every document scores 1.0 and the remaining signals do the ordering.
    pub fn score_text(&mut self, prepared: &Prepared, doc: &SearchDoc) -> Option<f32> {
        if prepared.empty {
            return Some(1.0);
        }
        let mut buf: Vec<char> = Vec::new();
        let mut best: Option<f32> = None;
        for (index, field) in haystacks(doc).enumerate() {
            let weight = (1.0 - index as f32 * FIELD_DECAY).max(FIELD_FLOOR);
            let Some(raw) = prepared
                .pattern
                .score(Utf32Str::new(field, &mut buf), &mut self.inner)
            else {
                continue;
            };
            // A haystack can out-score the needle against itself when its own word
            // boundaries earn bonuses the needle cannot; clamp rather than let a
            // relevance leave the unit interval core's weighting assumes.
            let normalized = (raw as f32 / prepared.perfect).min(1.0);
            let weighted = weight * normalized;
            if best.is_none_or(|b| weighted > b) {
                best = Some(weighted);
            }
        }
        best
    }

    /// Convenience for a single document; the palette uses [`Self::rank`].
    pub fn text_score(&mut self, query: &Query, doc: &SearchDoc) -> Option<f32> {
        let prepared = self.prepare(query);
        self.score_text(&prepared, doc)
    }

    /// Filter, match and rank a document set.
    ///
    /// Ties break on capsule id so that a list never reshuffles between two
    /// keystrokes that produce the same scores — a list that moves under the
    /// cursor is how a palette runs the wrong thing.
    pub fn rank(&mut self, query: &Query, docs: &[SearchDoc]) -> Vec<Row> {
        let prepared = self.prepare(query);
        let mut rows: Vec<Row> = Vec::with_capacity(docs.len());
        for doc in docs {
            if !query.matches_filters(doc) {
                continue;
            }
            let Some(text_score) = self.score_text(&prepared, doc) else {
                continue;
            };
            rows.push(Row {
                score: score(query, doc, text_score, &self.signals),
                text_score,
                doc: doc.clone(),
            });
        }
        rows.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.doc.id.cmp(&b.doc.id))
        });
        rows
    }
}

/// The searchable text of a document, in core's documented weight order.
fn haystacks(doc: &SearchDoc) -> impl Iterator<Item = &str> {
    doc.exports
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(doc.name.as_str()))
        .chain(std::iter::once(doc.id.path()))
        .chain(doc.tags.iter().map(String::as_str))
        .chain(std::iter::once(doc.description.as_str()))
}

/// Rank with the default signals. The palette keeps a [`Matcher`]; this is for
/// callers that rank once.
pub fn rank(query: &Query, docs: &[SearchDoc]) -> Vec<Row> {
    Matcher::default().rank(query, docs)
}
