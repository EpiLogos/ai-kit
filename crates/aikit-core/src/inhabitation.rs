//! World inhabitation readings: the joined `whoami` shape and the Refocus
//! delivery law (O:I `WORLD-INHABITATION-V1` §4).
//!
//! Nothing here owns World state. Central owns the Local World, Project World,
//! Position definitions and the NOW horizon; Actuation owns occupancy and
//! tenure; Factory owns work custody; AIKit composes body, context, session and
//! this joined reading. The types below only carry what those owners answered,
//! facet by facet, with the source that answered and — when it did not — the
//! exact reason. Absence and ambiguity are results, never errors.
//!
//! The module is I/O-free. The CLI joins owners through its command seam and
//! hands the answers to these types; tests drive the Refocus state machine
//! directly.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const INHABITATION_READING_SCHEMA: &str = "aikit.inhabitation-reading/v1";
pub const REFOCUS_READING_SCHEMA: &str = "aikit.refocus-reading/v1";
pub const REFOCUS_STATE_SCHEMA: &str = "aikit.refocus-state/v1";

/// Upper bound of a rendered Refocus text, in characters.
pub const REFOCUS_TEXT_MAX_CHARS: usize = 2_500;
/// Default number of prompts of sustained work before a Refocus is due.
pub const DEFAULT_SUSTAINED_PROMPTS: u64 = 40;

// ---------------------------------------------------------------------------
// Facets
// ---------------------------------------------------------------------------

/// The closed facet vocabulary of every joined read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FacetState {
    Present,
    Absent,
    Ambiguous,
    Unavailable,
    NotAttempted,
}

impl FacetState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Ambiguous => "ambiguous",
            Self::Unavailable => "unavailable",
            Self::NotAttempted => "not-attempted",
        }
    }
}

impl std::fmt::Display for FacetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One facet of the joined reading: `{state, value?, reason?, source, next?}`.
///
/// `summary` is the one-line compact projection (usually the primary ref) that
/// the compact JSON and the text block carry; `value` is the owner's answer
/// and appears only in the full reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Facet {
    pub state: FacetState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

impl Facet {
    fn new(state: FacetState, source: impl Into<String>) -> Self {
        Self {
            state,
            summary: None,
            value: None,
            reason: None,
            source: source.into(),
            next: None,
        }
    }

    pub fn present(source: impl Into<String>, summary: impl Into<String>, value: Value) -> Self {
        let mut facet = Self::new(FacetState::Present, source);
        facet.summary = Some(summary.into());
        facet.value = Some(value);
        facet
    }

    pub fn absent(source: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(FacetState::Absent, source).because(reason)
    }

    pub fn ambiguous(source: impl Into<String>, reason: impl Into<String>, value: Value) -> Self {
        let mut facet = Self::new(FacetState::Ambiguous, source).because(reason);
        facet.value = Some(value);
        facet
    }

    /// The owner could not answer. `reason` carries the exact failing command
    /// and its error — never a paraphrase.
    pub fn unavailable(source: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(FacetState::Unavailable, source).because(reason)
    }

    pub fn not_attempted(source: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(FacetState::NotAttempted, source).because(reason)
    }

    #[must_use]
    pub fn because(mut self, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        if !reason.trim().is_empty() {
            self.reason = Some(reason);
        }
        self
    }

    #[must_use]
    pub fn with_next(mut self, next: impl Into<String>) -> Self {
        self.next = Some(next.into());
        self
    }

    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    #[must_use]
    pub fn with_value(mut self, value: Value) -> Self {
        self.value = Some(value);
        self
    }

    pub fn is_present(&self) -> bool {
        self.state == FacetState::Present
    }

    /// The compact allowlist projection: every key but the owner's `value`.
    pub fn compact(&self) -> Value {
        let mut out = serde_json::Map::new();
        out.insert("state".into(), Value::from(self.state.as_str()));
        if let Some(summary) = &self.summary {
            out.insert("summary".into(), Value::from(summary.clone()));
        }
        if let Some(reason) = &self.reason {
            out.insert("reason".into(), Value::from(reason.clone()));
        }
        out.insert("source".into(), Value::from(self.source.clone()));
        if let Some(next) = &self.next {
            out.insert("next".into(), Value::from(next.clone()));
        }
        Value::Object(out)
    }

    /// The best one-line rendering for text surfaces.
    pub fn line(&self) -> String {
        match (&self.summary, &self.reason) {
            (Some(summary), Some(reason)) if self.state != FacetState::Present => {
                format!("{summary} — {reason}")
            }
            (Some(summary), _) => summary.clone(),
            (None, Some(reason)) => reason.clone(),
            (None, None) => String::new(),
        }
    }
}

/// The eighteen contract facets, in contract order.
pub const FACET_NAMES: [&str; 18] = [
    "local_world",
    "project_world",
    "position",
    "occupancy",
    "agent",
    "agency",
    "agent_session",
    "session_space",
    "body",
    "workcell",
    "root_now",
    "child_now",
    "current_work",
    "peers",
    "prepared_context",
    "authority",
    "working_surface",
    "return_destination",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InhabitationFacets {
    pub local_world: Facet,
    pub project_world: Facet,
    pub position: Facet,
    pub occupancy: Facet,
    pub agent: Facet,
    pub agency: Facet,
    pub agent_session: Facet,
    pub session_space: Facet,
    pub body: Facet,
    pub workcell: Facet,
    pub root_now: Facet,
    pub child_now: Facet,
    pub current_work: Facet,
    pub peers: Facet,
    pub prepared_context: Facet,
    pub authority: Facet,
    pub working_surface: Facet,
    pub return_destination: Facet,
}

impl InhabitationFacets {
    /// Every facet `not-attempted` from one source: the starting point a join
    /// fills in, so a facet the join never reached still says so.
    pub fn unattempted(source: &str, reason: &str) -> Self {
        let f = || Facet::not_attempted(source, reason);
        Self {
            local_world: f(),
            project_world: f(),
            position: f(),
            occupancy: f(),
            agent: f(),
            agency: f(),
            agent_session: f(),
            session_space: f(),
            body: f(),
            workcell: f(),
            root_now: f(),
            child_now: f(),
            current_work: f(),
            peers: f(),
            prepared_context: f(),
            authority: f(),
            working_surface: f(),
            return_destination: f(),
        }
    }

    pub fn entries(&self) -> [(&'static str, &Facet); 18] {
        [
            ("local_world", &self.local_world),
            ("project_world", &self.project_world),
            ("position", &self.position),
            ("occupancy", &self.occupancy),
            ("agent", &self.agent),
            ("agency", &self.agency),
            ("agent_session", &self.agent_session),
            ("session_space", &self.session_space),
            ("body", &self.body),
            ("workcell", &self.workcell),
            ("root_now", &self.root_now),
            ("child_now", &self.child_now),
            ("current_work", &self.current_work),
            ("peers", &self.peers),
            ("prepared_context", &self.prepared_context),
            ("authority", &self.authority),
            ("working_surface", &self.working_surface),
            ("return_destination", &self.return_destination),
        ]
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Facet> {
        Some(match name {
            "local_world" => &mut self.local_world,
            "project_world" => &mut self.project_world,
            "position" => &mut self.position,
            "occupancy" => &mut self.occupancy,
            "agent" => &mut self.agent,
            "agency" => &mut self.agency,
            "agent_session" => &mut self.agent_session,
            "session_space" => &mut self.session_space,
            "body" => &mut self.body,
            "workcell" => &mut self.workcell,
            "root_now" => &mut self.root_now,
            "child_now" => &mut self.child_now,
            "current_work" => &mut self.current_work,
            "peers" => &mut self.peers,
            "prepared_context" => &mut self.prepared_context,
            "authority" => &mut self.authority,
            "working_surface" => &mut self.working_surface,
            "return_destination" => &mut self.return_destination,
            _ => return None,
        })
    }

    pub fn states(&self) -> BTreeMap<String, FacetState> {
        self.entries()
            .iter()
            .map(|(name, facet)| ((*name).to_owned(), facet.state))
            .collect()
    }
}

/// How the Position was resolved. The chain is fixed: flag, then the
/// launch-stamped environment, then an occupancy naming the current
/// AgentSession, then nothing — never text, pane titles or recency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedBy {
    Flag,
    Env,
    AgentSession,
    None,
}

impl ResolvedBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Env => "env",
            Self::AgentSession => "agent-session",
            Self::None => "none",
        }
    }
}

/// How far a join reaches. `Lean` is the SessionStart entry: identity, work,
/// NOW and Return only. `Standard` is `aikit whoami`. `Full` adds the
/// ActorBootstrap composition and every owner answer verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReadingDepth {
    Lean,
    Standard,
    Full,
}

/// One owner command the join ran, with its outcome word from the shared probe
/// vocabulary (`ok | unreachable | unsupported | timed-out | refused`) or
/// `skipped` when the budget ended before it could run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerCallRecord {
    pub command: String,
    pub outcome: String,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Where a hot reading came from, when it was served from the Redis World
/// projection instead of live owner joins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotProvenance {
    /// `served | absent | unavailable` — the latter two mean the reading below
    /// is live and why the hot read did not answer.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The refs, revisions and cursors of one joined reading — the only material
/// the hot World projection may hold. Every value is recomputable from its
/// owner, which is why losing the projection loses no identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InhabitationIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_world_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_world_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupant_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_ordinal: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agency_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_space_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workcell_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_now_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_now_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_now_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_now_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_work_outcome: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub current_work_refs: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_work_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_destination: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared_context: Option<String>,
    /// Peer Position ref → occupancy word (`occupied:<generation>`,
    /// `vacant`, `unknown`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub peers: BTreeMap<String, String>,
}

impl InhabitationIdentity {
    /// A stable content digest: struct and BTreeMap order are deterministic.
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        format!("blake3:{}", blake3::hash(&bytes).to_hex())
    }

    /// Every string the identity holds, for bounding checks.
    pub fn strings(&self) -> Vec<&str> {
        let mut out: Vec<&str> = [
            &self.local_world_ref,
            &self.project_world_ref,
            &self.position_ref,
            &self.position_revision,
            &self.occupant_generation,
            &self.agent_ref,
            &self.agency_ref,
            &self.agent_session_ref,
            &self.session_space_ref,
            &self.workcell_ref,
            &self.root_now_ref,
            &self.root_now_revision,
            &self.child_now_ref,
            &self.child_now_revision,
            &self.current_work_outcome,
            &self.current_work_digest,
            &self.return_destination,
            &self.prepared_context,
        ]
        .into_iter()
        .filter_map(|value| value.as_deref())
        .collect();
        for (key, value) in self.current_work_refs.iter().chain(self.peers.iter()) {
            out.push(key);
            out.push(value);
        }
        out
    }
}

/// `aikit.inhabitation-reading/v1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InhabitationReading {
    pub schema: String,
    pub resolved_by: ResolvedBy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupant_generation: Option<String>,
    pub depth: ReadingDepth,
    pub observed_at_unix_ms: u64,
    pub facets: InhabitationFacets,
    pub identity: InhabitationIdentity,
    pub basis_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot: Option<HotProvenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<OwnerCallRecord>,
}

impl InhabitationReading {
    /// The compact `--json` projection: an allowlist of identity keys and
    /// every facet without its owner `value`. A new owner field therefore
    /// appears only in the full reading.
    pub fn compact(&self) -> Value {
        let facets: serde_json::Map<String, Value> = self
            .facets
            .entries()
            .iter()
            .map(|(name, facet)| ((*name).to_owned(), facet.compact()))
            .collect();
        let mut out = serde_json::json!({
            "schema": self.schema,
            "projection": "compact",
            "resolved_by": self.resolved_by,
            "position_ref": self.position_ref,
            "occupant_generation": self.occupant_generation,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "basis_digest": self.basis_digest,
            "facets": facets,
        });
        if let Some(hot) = &self.hot {
            out["hot"] = serde_json::to_value(hot).unwrap_or(Value::Null);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Refocus reading
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefocusTrigger {
    Explicit,
    Fresh,
    Compaction,
    Transition,
    Sustained,
}

impl RefocusTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Fresh => "fresh",
            Self::Compaction => "compaction",
            Self::Transition => "transition",
            Self::Sustained => "sustained",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim() {
            "explicit" => Self::Explicit,
            "fresh" => Self::Fresh,
            "compaction" => Self::Compaction,
            "transition" => Self::Transition,
            "sustained" => Self::Sustained,
            _ => return None,
        })
    }

    /// Why this Refocus is being delivered, in the words the occupant reads.
    pub fn why(&self, prompts: u64) -> String {
        match self {
            Self::Explicit => "on request".into(),
            Self::Fresh => "fresh occupancy — orient before acting".into(),
            Self::Compaction => "just compacted — your picture is lossy".into(),
            Self::Transition => "your current work changed since the last refocus".into(),
            Self::Sustained => {
                format!("{prompts} prompts of sustained work since the last refocus")
            }
        }
    }
}

/// One hop of the trace from the current operation to the ProjectCentral
/// ground: a ref and revision, or an explicit gap naming why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefocusHop {
    /// `operation | workflow-unit | run | journey | intent | ground`.
    pub hop: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "ref")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<String>,
    pub source: String,
}

impl RefocusHop {
    pub fn found(
        hop: &str,
        reference: impl Into<String>,
        revision: Option<String>,
        detail: Option<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            hop: hop.into(),
            reference: Some(reference.into()),
            revision,
            detail,
            gap: None,
            source: source.into(),
        }
    }

    pub fn gap(hop: &str, reason: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            hop: hop.into(),
            reference: None,
            revision: None,
            detail: None,
            gap: Some(reason.into()),
            source: source.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NearbyWork {
    pub position_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub occupancy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedSource {
    #[serde(rename = "ref")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

/// `aikit.refocus-reading/v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefocusReading {
    pub schema: String,
    pub trigger: RefocusTrigger,
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occupant_generation: Option<String>,
    pub chain: Vec<RefocusHop>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_now: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_now: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default)]
    pub nearby: Vec<NearbyWork>,
    /// `None` when there is no earlier delivery to compare against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed: Option<Vec<ChangedSource>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_target: Option<String>,
    /// ref → revision of every hop and NOW that answered; the basis a later
    /// delivery compares against to name changed sources.
    pub basis: BTreeMap<String, String>,
    pub basis_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_digest: Option<String>,
}

/// Digest of a ref → revision basis.
pub fn basis_digest(basis: &BTreeMap<String, String>) -> String {
    let bytes = serde_json::to_vec(basis).unwrap_or_default();
    format!("blake3:{}", blake3::hash(&bytes).to_hex())
}

/// Sources whose revision differs between a delivered basis and the current
/// one, including refs that appeared or disappeared.
pub fn changed_sources(
    previous: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Vec<ChangedSource> {
    let mut out = Vec::new();
    for (reference, revision) in current {
        match previous.get(reference) {
            Some(before) if before == revision => {}
            before => out.push(ChangedSource {
                reference: reference.clone(),
                from: before.cloned(),
                to: Some(revision.clone()),
            }),
        }
    }
    for (reference, revision) in previous {
        if !current.contains_key(reference) {
            out.push(ChangedSource {
                reference: reference.clone(),
                from: Some(revision.clone()),
                to: None,
            });
        }
    }
    out
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Bound `text` to `max` characters on a char boundary, marking the cut.
pub fn bounded_text(text: &str, max: usize) -> String {
    clip(text, max)
}

impl RefocusReading {
    /// The text delivered into the turn. `omit_identity` drops the Position,
    /// NOW and body lines when a lean entry carrying them rides the same turn.
    pub fn render(&self, omit_identity: bool) -> String {
        let mut lines = vec![
            format!(
                "REFOCUS ({}) — {}. Answer briefly, out loud, before your next move:",
                self.why, REFOCUS_READING_SCHEMA
            ),
            "1. What outcome does the person need from this work — not your task, the outcome?"
                .to_owned(),
            "2. Does what you are doing right now move that outcome? If you cannot say, stop and say so."
                .to_owned(),
            "3. What have you concluded without opening the source or running the thing?".to_owned(),
            "Trace (current operation → ProjectCentral ground):".to_owned(),
        ];
        for hop in &self.chain {
            let line = match (&hop.reference, &hop.gap) {
                (Some(reference), _) => {
                    let mut line = format!("  {:<13} {}", hop.hop, clip(reference, 120));
                    if let Some(revision) = &hop.revision {
                        line.push_str(&format!(" @{}", clip(revision, 60)));
                    }
                    if let Some(detail) = &hop.detail {
                        line.push_str(&format!(" — {}", clip(detail, 110)));
                    }
                    line
                }
                (None, Some(gap)) => {
                    format!("  {:<13} TRACE GAP — {}", hop.hop, clip(gap, 170))
                }
                (None, None) => format!("  {:<13} TRACE GAP — no answer", hop.hop),
            };
            lines.push(line);
        }
        if !omit_identity {
            if let Some(position) = &self.position {
                let generation = self
                    .occupant_generation
                    .as_deref()
                    .map(|generation| format!(" (occupant {generation})"))
                    .unwrap_or_default();
                lines.push(format!("Position: {}{generation}", clip(position, 160)));
            }
            let now = match (&self.root_now, &self.child_now) {
                (Some(root), Some(child)) => {
                    format!("root {} · child {}", clip(root, 110), clip(child, 110))
                }
                (Some(root), None) => format!("root {}", clip(root, 160)),
                (None, Some(child)) => format!("child {}", clip(child, 160)),
                (None, None) => "none resolved".to_owned(),
            };
            lines.push(format!("NOW: {now}"));
            if let Some(body) = &self.body {
                lines.push(format!("Body: {}", clip(body, 160)));
            }
        }
        if !self.nearby.is_empty() {
            let shown: Vec<String> = self
                .nearby
                .iter()
                .take(6)
                .map(|peer| {
                    let name = peer.handle.as_deref().unwrap_or(peer.position_ref.as_str());
                    match &peer.work {
                        Some(work) => format!("{name} [{}]: {}", peer.occupancy, clip(work, 60)),
                        None => format!("{name} [{}]", peer.occupancy),
                    }
                })
                .collect();
            let more = self.nearby.len().saturating_sub(shown.len());
            lines.push(format!(
                "Nearby work: {}{}",
                shown.join("; "),
                if more > 0 {
                    format!("; +{more} more (aikit whoami)")
                } else {
                    String::new()
                }
            ));
        }
        match &self.changed {
            None => lines.push("Changed since last refocus: no earlier delivery".to_owned()),
            Some(changed) if changed.is_empty() => {
                lines.push("Changed since last refocus: nothing".to_owned())
            }
            Some(changed) => {
                let shown: Vec<String> = changed
                    .iter()
                    .take(5)
                    .map(|change| {
                        format!(
                            "{} {}→{}",
                            clip(&change.reference, 70),
                            change.from.as_deref().unwrap_or("∅"),
                            change.to.as_deref().unwrap_or("∅")
                        )
                    })
                    .collect();
                let more = changed.len().saturating_sub(shown.len());
                lines.push(format!(
                    "Changed since last refocus: {}{}",
                    shown.join("; "),
                    if more > 0 {
                        format!("; +{more} more")
                    } else {
                        String::new()
                    }
                ));
            }
        }
        lines.push(format!(
            "Return: {}",
            self.return_target
                .as_deref()
                .map(|target| clip(target, 160))
                .unwrap_or_else(|| "no Return target resolved — name one before closing".into())
        ));
        lines.push(
            "Consequential work retrieves the governing source on demand; this trace is pointers, not authority."
                .to_owned(),
        );
        clip(&lines.join("\n"), REFOCUS_TEXT_MAX_CHARS)
    }
}

// ---------------------------------------------------------------------------
// Refocus delivery law
// ---------------------------------------------------------------------------

/// SessionStart's `source` field, as the harness reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSource {
    Startup,
    Resume,
    Clear,
    Compact,
    Other,
}

impl SessionSource {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim) {
            Some("startup") => Self::Startup,
            Some("resume") => Self::Resume,
            Some("clear") => Self::Clear,
            Some("compact") => Self::Compact,
            _ => Self::Other,
        }
    }
}

/// One lifecycle signal the delivery law observes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefocusSignal {
    SessionStart(SessionSource),
    /// `work` is `None` when the current-work digest was not checked this
    /// prompt, `Some(None)` when it was checked and could not be known.
    UserPromptSubmit {
        work: Option<Option<String>>,
    },
    PreCompact,
    PostCompact,
    Stop,
}

impl RefocusSignal {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::SessionStart(_) => "SessionStart",
            Self::UserPromptSubmit { .. } => "UserPromptSubmit",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::Stop => "Stop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefocusPolicy {
    pub sustained_prompts: u64,
}

impl Default for RefocusPolicy {
    fn default() -> Self {
        Self {
            sustained_prompts: DEFAULT_SUSTAINED_PROMPTS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefocusDecision {
    /// Emit a Refocus into this turn. Delivery is recorded only once the
    /// harness output carrying it has actually been written.
    Deliver(RefocusTrigger),
    /// Remember that a Refocus is due; this event emits nothing.
    MarkPending(RefocusTrigger),
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRefocus {
    pub trigger: RefocusTrigger,
    pub on: String,
    pub at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveredRefocus {
    pub trigger: RefocusTrigger,
    pub on: String,
    pub at_unix_ms: u64,
    #[serde(default)]
    pub basis: BTreeMap<String, String>,
    pub basis_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_digest: Option<String>,
}

/// Per-(AgentSession, occupant generation) delivery state. A new occupant
/// generation is a new state: its baseline is its own and a predecessor's
/// pending delivery is never read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefocusState {
    pub schema: String,
    pub agent_session: String,
    pub occupant_generation: String,
    pub position_ref: String,
    pub baseline_at_unix_ms: u64,
    #[serde(default)]
    pub prompts_since_delivery: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingRefocus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered: Option<DeliveredRefocus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_work_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_work_check_unix_ms: Option<u64>,
}

impl RefocusState {
    pub fn new(
        agent_session: impl Into<String>,
        occupant_generation: impl Into<String>,
        position_ref: impl Into<String>,
        now_ms: u64,
    ) -> Self {
        Self {
            schema: REFOCUS_STATE_SCHEMA.into(),
            agent_session: agent_session.into(),
            occupant_generation: occupant_generation.into(),
            position_ref: position_ref.into(),
            baseline_at_unix_ms: now_ms,
            prompts_since_delivery: 0,
            pending: None,
            delivered: None,
            observed_work_digest: None,
            last_work_check_unix_ms: None,
        }
    }

    /// Observe one signal: update counters and pending state, and decide.
    /// Never records a delivery — see [`RefocusState::record_delivered`].
    pub fn observe(
        &mut self,
        signal: &RefocusSignal,
        policy: RefocusPolicy,
        now_ms: u64,
    ) -> RefocusDecision {
        let threshold = policy.sustained_prompts.max(1);
        match signal {
            RefocusSignal::SessionStart(SessionSource::Startup | SessionSource::Clear) => {
                RefocusDecision::Deliver(RefocusTrigger::Fresh)
            }
            RefocusSignal::SessionStart(SessionSource::Compact) => {
                RefocusDecision::Deliver(RefocusTrigger::Compaction)
            }
            RefocusSignal::SessionStart(SessionSource::Resume | SessionSource::Other) => self
                .pending
                .as_ref()
                .map(|pending| RefocusDecision::Deliver(pending.trigger))
                .unwrap_or(RefocusDecision::Nothing),
            RefocusSignal::PreCompact | RefocusSignal::PostCompact => {
                // A compaction supersedes any earlier pending reason: the
                // picture is lossy now, whatever was due before.
                self.pending = Some(PendingRefocus {
                    trigger: RefocusTrigger::Compaction,
                    on: signal.event_name().into(),
                    at_unix_ms: now_ms,
                });
                RefocusDecision::MarkPending(RefocusTrigger::Compaction)
            }
            RefocusSignal::Stop => {
                if self.pending.is_none() && self.prompts_since_delivery >= threshold {
                    self.pending = Some(PendingRefocus {
                        trigger: RefocusTrigger::Sustained,
                        on: "Stop".into(),
                        at_unix_ms: now_ms,
                    });
                    RefocusDecision::MarkPending(RefocusTrigger::Sustained)
                } else {
                    RefocusDecision::Nothing
                }
            }
            RefocusSignal::UserPromptSubmit { work } => {
                self.prompts_since_delivery = self.prompts_since_delivery.saturating_add(1);
                if let Some(pending) = &self.pending {
                    return RefocusDecision::Deliver(pending.trigger);
                }
                if let Some(Some(digest)) = work {
                    let prior = self
                        .delivered
                        .as_ref()
                        .and_then(|delivered| delivered.work_digest.clone())
                        .or_else(|| self.observed_work_digest.clone());
                    self.observed_work_digest = Some(digest.clone());
                    if prior.is_some_and(|prior| &prior != digest) {
                        return RefocusDecision::Deliver(RefocusTrigger::Transition);
                    }
                }
                if self.prompts_since_delivery >= threshold {
                    return RefocusDecision::Deliver(RefocusTrigger::Sustained);
                }
                RefocusDecision::Nothing
            }
        }
    }

    /// Record a delivery that was actually emitted into the turn. Callers run
    /// this only after the harness output carrying the text was written.
    pub fn record_delivered(
        &mut self,
        trigger: RefocusTrigger,
        on: &str,
        basis: BTreeMap<String, String>,
        work_digest: Option<String>,
        now_ms: u64,
    ) {
        let digest = basis_digest(&basis);
        if work_digest.is_some() {
            self.observed_work_digest = work_digest.clone();
        }
        self.delivered = Some(DeliveredRefocus {
            trigger,
            on: on.into(),
            at_unix_ms: now_ms,
            basis,
            basis_digest: digest,
            work_digest,
        });
        self.pending = None;
        self.prompts_since_delivery = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> RefocusState {
        RefocusState::new(
            "session-a",
            "actuation:generation:g1",
            "central:position:project:O-I:x",
            1,
        )
    }

    fn prompt() -> RefocusSignal {
        RefocusSignal::UserPromptSubmit { work: None }
    }

    fn delivered(state: &mut RefocusState, decision: RefocusDecision, work: Option<&str>) {
        let RefocusDecision::Deliver(trigger) = decision else {
            panic!("expected a delivery, got {decision:?}");
        };
        state.record_delivered(trigger, "test", BTreeMap::new(), work.map(str::to_owned), 2);
    }

    #[test]
    fn fresh_and_clear_starts_deliver_fresh_compact_start_delivers_compaction() {
        let policy = RefocusPolicy::default();
        let mut s = state();
        assert_eq!(
            s.observe(
                &RefocusSignal::SessionStart(SessionSource::Startup),
                policy,
                1
            ),
            RefocusDecision::Deliver(RefocusTrigger::Fresh)
        );
        assert_eq!(
            s.observe(
                &RefocusSignal::SessionStart(SessionSource::Clear),
                policy,
                1
            ),
            RefocusDecision::Deliver(RefocusTrigger::Fresh)
        );
        assert_eq!(
            s.observe(
                &RefocusSignal::SessionStart(SessionSource::Compact),
                policy,
                1
            ),
            RefocusDecision::Deliver(RefocusTrigger::Compaction)
        );
        assert_eq!(
            s.observe(
                &RefocusSignal::SessionStart(SessionSource::Resume),
                policy,
                1
            ),
            RefocusDecision::Nothing,
            "a resume with nothing pending emits nothing"
        );
    }

    #[test]
    fn precompact_and_stop_only_mark_pending_and_the_next_prompt_delivers_it() {
        let policy = RefocusPolicy::default();
        let mut s = state();
        assert_eq!(
            s.observe(&RefocusSignal::PreCompact, policy, 5),
            RefocusDecision::MarkPending(RefocusTrigger::Compaction)
        );
        assert_eq!(s.pending.as_ref().unwrap().on, "PreCompact");
        assert_eq!(
            s.observe(&RefocusSignal::Stop, policy, 6),
            RefocusDecision::Nothing,
            "Stop never overwrites a compaction pending"
        );
        let decision = s.observe(&prompt(), policy, 7);
        assert_eq!(
            decision,
            RefocusDecision::Deliver(RefocusTrigger::Compaction)
        );
        assert!(s.pending.is_some(), "observe never consumes pending");
        delivered(&mut s, decision, None);
        assert!(s.pending.is_none());
        assert_eq!(s.observe(&prompt(), policy, 8), RefocusDecision::Nothing);
    }

    #[test]
    fn an_undelivered_decision_stays_due_until_it_is_recorded() {
        let policy = RefocusPolicy::default();
        let mut s = state();
        s.observe(&RefocusSignal::PostCompact, policy, 1);
        for _ in 0..3 {
            assert_eq!(
                s.observe(&prompt(), policy, 2),
                RefocusDecision::Deliver(RefocusTrigger::Compaction),
                "a write that never happened leaves the Refocus due"
            );
        }
    }

    #[test]
    fn sustained_threshold_fires_once_per_threshold_not_every_turn() {
        let policy = RefocusPolicy {
            sustained_prompts: 3,
        };
        let mut s = state();
        assert_eq!(s.observe(&prompt(), policy, 1), RefocusDecision::Nothing);
        assert_eq!(s.observe(&prompt(), policy, 1), RefocusDecision::Nothing);
        let third = s.observe(&prompt(), policy, 1);
        assert_eq!(third, RefocusDecision::Deliver(RefocusTrigger::Sustained));
        delivered(&mut s, third, None);
        for _ in 0..2 {
            assert_eq!(
                s.observe(&prompt(), policy, 1),
                RefocusDecision::Nothing,
                "the turn after a delivery is quiet"
            );
        }
        assert_eq!(
            s.observe(&prompt(), policy, 1),
            RefocusDecision::Deliver(RefocusTrigger::Sustained)
        );
    }

    #[test]
    fn stop_past_the_threshold_marks_sustained_pending() {
        let policy = RefocusPolicy {
            sustained_prompts: 1,
        };
        let mut s = state();
        s.prompts_since_delivery = 1;
        assert_eq!(
            s.observe(&RefocusSignal::Stop, policy, 1),
            RefocusDecision::MarkPending(RefocusTrigger::Sustained)
        );
    }

    #[test]
    fn transition_fires_on_a_changed_work_digest_against_the_last_delivery() {
        let policy = RefocusPolicy::default();
        let mut s = state();
        let fresh = s.observe(
            &RefocusSignal::SessionStart(SessionSource::Startup),
            policy,
            1,
        );
        delivered(&mut s, fresh, Some("work-a"));
        let same = RefocusSignal::UserPromptSubmit {
            work: Some(Some("work-a".into())),
        };
        assert_eq!(s.observe(&same, policy, 2), RefocusDecision::Nothing);
        let changed = RefocusSignal::UserPromptSubmit {
            work: Some(Some("work-b".into())),
        };
        let decision = s.observe(&changed, policy, 3);
        assert_eq!(
            decision,
            RefocusDecision::Deliver(RefocusTrigger::Transition)
        );
        delivered(&mut s, decision, Some("work-b"));
        assert_eq!(s.observe(&changed, policy, 4), RefocusDecision::Nothing);
    }

    #[test]
    fn unknown_work_never_counts_as_a_transition() {
        let policy = RefocusPolicy::default();
        let mut s = state();
        let unknown = RefocusSignal::UserPromptSubmit { work: Some(None) };
        assert_eq!(s.observe(&unknown, policy, 1), RefocusDecision::Nothing);
        // The first known digest is a baseline, not a transition.
        let first = RefocusSignal::UserPromptSubmit {
            work: Some(Some("work-a".into())),
        };
        assert_eq!(s.observe(&first, policy, 2), RefocusDecision::Nothing);
        let second = RefocusSignal::UserPromptSubmit {
            work: Some(Some("work-b".into())),
        };
        assert_eq!(
            s.observe(&second, policy, 3),
            RefocusDecision::Deliver(RefocusTrigger::Transition)
        );
    }

    #[test]
    fn changed_sources_name_revised_new_and_removed_refs() {
        let before = BTreeMap::from([
            ("run:a".to_owned(), "2".to_owned()),
            ("now:x".to_owned(), "r1".to_owned()),
        ]);
        let after = BTreeMap::from([
            ("run:a".to_owned(), "3".to_owned()),
            ("journey:j".to_owned(), "1".to_owned()),
        ]);
        let changed = changed_sources(&before, &after);
        assert_eq!(changed.len(), 3);
        assert!(changed
            .iter()
            .any(|c| c.reference == "run:a" && c.from.as_deref() == Some("2")));
        assert!(changed
            .iter()
            .any(|c| c.reference == "journey:j" && c.from.is_none()));
        assert!(changed
            .iter()
            .any(|c| c.reference == "now:x" && c.to.is_none()));
    }

    #[test]
    fn rendered_refocus_is_small_and_names_gaps() {
        let reading = RefocusReading {
            schema: REFOCUS_READING_SCHEMA.into(),
            trigger: RefocusTrigger::Compaction,
            why: RefocusTrigger::Compaction.why(0),
            position: Some("central:position:project:O-I:x".into()),
            occupant_generation: Some("actuation:generation:g1".into()),
            chain: vec![
                RefocusHop::found(
                    "run",
                    "run:1",
                    Some("2".into()),
                    None,
                    "factory development run",
                ),
                RefocusHop::gap("intent", "no telos", "ProjectCentral"),
            ],
            root_now: None,
            child_now: None,
            body: None,
            nearby: vec![],
            changed: None,
            return_target: None,
            basis: BTreeMap::new(),
            basis_digest: basis_digest(&BTreeMap::new()),
            work_digest: None,
        };
        let text = reading.render(false);
        assert!(text.starts_with("REFOCUS (just compacted"));
        assert!(text.contains("TRACE GAP — no telos"));
        assert!(text.contains("run:1 @2"));
        assert!(text.chars().count() <= REFOCUS_TEXT_MAX_CHARS);
        assert!(!reading.render(true).contains("Position:"));
    }

    #[test]
    fn identity_digest_is_stable_and_compact_projection_drops_values() {
        let identity = InhabitationIdentity {
            position_ref: Some("central:position:project:O-I:x".into()),
            ..InhabitationIdentity::default()
        };
        assert_eq!(identity.digest(), identity.clone().digest());
        let facet = Facet::present("owner", "ref", serde_json::json!({"body": "large"}));
        let compact = facet.compact();
        assert!(compact.get("value").is_none());
        assert_eq!(compact["state"], "present");
    }
}
