//! Hook event normalization, chain planning and dispatch interpretation.
//!
//! One permanent dispatcher entry is registered per client event:
//!
//! ```text
//! PreToolUse → aikit hook dispatch claude PreToolUse
//! ```
//!
//! The dispatcher normalizes the client's event into a [`HookEvent`], loads the
//! immutable [`HookChain`] for that event from the current generation, and runs it
//! through the phases `gate → transform → verify → inject → observe → capture`.
//!
//! This module does **no** process execution. It plans the chain and interprets
//! the results; the CLI supplies a runner closure that actually spawns capsules.
//! That split is what makes the interesting properties testable without a
//! filesystem or a subprocess:
//!
//! * The short-circuit is real: after a denial, later steps are never handed to
//!   the runner at all.
//! * Observe and capture steps run in a *finally* stage. They can never deny, and
//!   a crashed observer can never block the user's work — not even under a
//!   `closed` failure policy, which would otherwise turn a logging bug into an
//!   outage.
//! * A **system failure** and a **policy denial** are different things everywhere.
//!   Conflating "the gate said no" with "the gate fell over" is how a security
//!   control quietly stops working, so the two are separate outcomes in the record
//!   and the denial itself carries the distinction.
//! * A bypass is a scoped token, never an environment switch, and every use — or
//!   refusal — is recorded.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::capsule::{BypassPolicy, FailurePolicy, HookPhase, Kind};
use crate::catalog::Catalog;
use crate::duration::HumanDuration;
use crate::error::{AikitError, Result};
use crate::id::CapsuleId;
use crate::profile::ConfigTable;
use crate::resolve::ResolvedView;

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// A client hook event, normalized but not flattened.
///
/// `Other` exists because clients add events faster than AIKit can release, and
/// silently discarding an unknown event would silently disable whatever a user
/// had wired to it. An unrecognized name is carried through verbatim instead.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum HookEventKind {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    Stop,
    SessionEnd,
    PreCompact,
    Notification,
    Other(String),
}

impl HookEventKind {
    pub const KNOWN: [HookEventKind; 8] = [
        HookEventKind::SessionStart,
        HookEventKind::UserPromptSubmit,
        HookEventKind::PreToolUse,
        HookEventKind::PostToolUse,
        HookEventKind::Stop,
        HookEventKind::SessionEnd,
        HookEventKind::PreCompact,
        HookEventKind::Notification,
    ];

    /// Parse an event name.
    ///
    /// The client's own spelling (`PreToolUse`) is canonical, but manifests are
    /// hand-written TOML where every other key is kebab-case, so `pre-tool-use`
    /// and `pre_tool_use` are accepted too. Rendering always returns the client's
    /// spelling, because that is what has to go back over the wire.
    pub fn parse(raw: &str) -> Self {
        let folded: String = raw
            .chars()
            .filter(|c| *c != '-' && *c != '_')
            .flat_map(char::to_lowercase)
            .collect();
        for known in Self::KNOWN {
            if folded == known.as_str().to_lowercase() {
                return known;
            }
        }
        Self::Other(raw.trim().to_string())
    }

    pub fn as_str(&self) -> &str {
        match self {
            HookEventKind::SessionStart => "SessionStart",
            HookEventKind::UserPromptSubmit => "UserPromptSubmit",
            HookEventKind::PreToolUse => "PreToolUse",
            HookEventKind::PostToolUse => "PostToolUse",
            HookEventKind::Stop => "Stop",
            HookEventKind::SessionEnd => "SessionEnd",
            HookEventKind::PreCompact => "PreCompact",
            HookEventKind::Notification => "Notification",
            HookEventKind::Other(raw) => raw,
        }
    }

    /// Events that carry a tool name, and where a matcher is therefore meaningful.
    pub fn carries_tool_name(&self) -> bool {
        matches!(self, HookEventKind::PreToolUse | HookEventKind::PostToolUse)
    }
}

impl std::fmt::Display for HookEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for HookEventKind {
    fn from(raw: String) -> Self {
        Self::parse(&raw)
    }
}

impl From<HookEventKind> for String {
    fn from(kind: HookEventKind) -> Self {
        kind.as_str().to_string()
    }
}

/// A normalized client event.
///
/// The payload stays an opaque `serde_json::Value`: AIKit has no business
/// re-typing every client's event schema, and a transform hook has to be able to
/// rewrite fields core has never heard of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookEvent {
    /// The client that raised the event (`claude`, `codex`, ...).
    pub client: String,
    pub kind: HookEventKind,
    #[serde(default)]
    pub tool_name: Option<String>,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

impl HookEvent {
    pub fn new(client: impl Into<String>, kind: HookEventKind, payload: serde_json::Value) -> Self {
        Self {
            client: client.into(),
            kind,
            tool_name: None,
            payload,
            cwd: None,
        }
    }

    #[must_use]
    pub fn with_tool_name(mut self, tool: impl Into<String>) -> Self {
        self.tool_name = Some(tool.into());
        self
    }

    #[must_use]
    pub fn in_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

/// One capsule's participation in one event's chain.
///
/// Flattened out of the manifest at chain-build time so the dispatcher never
/// needs the catalog: the generation's `hooks/` directory is immutable, and a
/// dispatch must not depend on a registry that may have been synced since.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookStep {
    pub capsule: CapsuleId,
    /// Capsule-relative entry path. Resolved against the capsule root by the CLI.
    pub entry: String,
    pub phase: HookPhase,
    pub order: i32,
    #[serde(default)]
    pub timeout: Option<HumanDuration>,
    pub failure: FailurePolicy,
    /// Serial by default; a capsule must opt in to being run in parallel.
    pub serial: bool,
    #[serde(default)]
    pub matcher: Option<String>,
    pub bypass: BypassPolicy,
    #[serde(default)]
    pub config: ConfigTable,
}

impl HookStep {
    /// A step with the conservative defaults: serial, fail closed, no bypass.
    pub fn new(capsule: CapsuleId, entry: impl Into<String>, phase: HookPhase) -> Self {
        Self {
            capsule,
            entry: entry.into(),
            phase,
            order: 0,
            timeout: None,
            failure: FailurePolicy::default(),
            serial: true,
            matcher: None,
            bypass: BypassPolicy::default(),
            config: ConfigTable::new(),
        }
    }

    /// The base sort key. Dependencies constrain it further; see [`HookChain::plan`].
    fn sort_key(&self) -> (HookPhase, i32, &CapsuleId) {
        (self.phase, self.order, &self.capsule)
    }

    /// May this step ever be grouped with its neighbours for parallel execution?
    ///
    /// Gates and transforms are always serial no matter what the manifest says. A
    /// gate is the one place a capsule can veto, and a transform rewrites the
    /// payload the next step sees; in both cases the order *is* the semantics.
    fn groupable(&self) -> bool {
        !self.serial && !matches!(self.phase, HookPhase::Gate | HookPhase::Transform)
    }

    pub fn matches(&self, event: &HookEvent) -> bool {
        matches(self, event)
    }
}

/// Does this step apply to this event?
///
/// The matcher is an unanchored regular expression tested against the tool name,
/// which is the clients' own convention — `Edit` therefore also matches
/// `MultiEdit`, and an author who wants exactness writes `^Edit$`. An event with
/// no tool name is tested against the empty string, so `.*` still covers it while
/// `^Bash$` does not.
///
/// A matcher that does not compile matches nothing. [`build_chains`] rejects such
/// a matcher outright, so a live chain cannot contain one.
pub fn matches(step: &HookStep, event: &HookEvent) -> bool {
    let Some(pattern) = &step.matcher else {
        return true;
    };
    match regex::Regex::new(pattern) {
        Ok(re) => re.is_match(event.tool_name.as_deref().unwrap_or("")),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Chains
// ---------------------------------------------------------------------------

/// A run of steps the CLI may execute together.
///
/// Grouping is advisory *for execution* only. The dispatcher always folds results
/// in chain order, so parallelism can change how long a dispatch takes but never
/// what it decides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionGroup {
    pub phase: HookPhase,
    pub parallel: bool,
    pub capsules: Vec<CapsuleId>,
}

/// The ordered steps for one event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookChain {
    pub event: HookEventKind,
    pub steps: Vec<HookStep>,
    /// In-chain dependency edges, kept beside the steps so grouping never has to
    /// go back to the catalog. Restricted to capsules that are actually in this
    /// chain.
    #[serde(default)]
    pub dependencies: BTreeMap<CapsuleId, Vec<CapsuleId>>,
}

impl HookChain {
    /// Order `steps` by `(phase, order, capsule id)`, subject to the dependency
    /// edges in `dependencies`.
    ///
    /// Dependencies win over the declared order: a hook that requires another has
    /// asked to see its effect, and honouring the number instead would make the
    /// requirement a lie. What dependencies may *not* do is move a step into a
    /// different phase — the phases are stages with distinct meanings, and
    /// silently promoting an observer into the verify stage would grant it a veto
    /// its author never asked for. That case is rejected rather than repaired.
    pub fn plan(
        event: HookEventKind,
        steps: Vec<HookStep>,
        dependencies: &BTreeMap<CapsuleId, Vec<CapsuleId>>,
    ) -> Result<Self> {
        let mut steps = steps;
        steps.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

        let phases: BTreeMap<CapsuleId, HookPhase> =
            steps.iter().map(|s| (s.capsule.clone(), s.phase)).collect();

        // Only edges whose endpoints are both in this chain constrain it.
        let mut edges: BTreeMap<CapsuleId, Vec<CapsuleId>> = BTreeMap::new();
        for step in &steps {
            let mut kept: Vec<CapsuleId> = Vec::new();
            for dep in dependencies.get(&step.capsule).into_iter().flatten() {
                let Some(dep_phase) = phases.get(dep) else {
                    continue;
                };
                if *dep_phase > step.phase {
                    return Err(AikitError::new(
                        "hook.chain_order_impossible",
                        format!(
                            "{} runs in the {} phase but requires {}, which runs in the later {} \
                             phase; no order satisfies both",
                            step.capsule,
                            step.phase.as_str(),
                            dep,
                            dep_phase.as_str()
                        ),
                    )
                    .with("capability", step.capsule.to_string())
                    .with("requires", dep.to_string())
                    .with("phase", step.phase.as_str())
                    .with("dependency_phase", dep_phase.as_str()));
                }
                kept.push(dep.clone());
            }
            if !kept.is_empty() {
                edges.insert(step.capsule.clone(), kept);
            }
        }

        let order = topological_order(&steps, &edges).ok_or_else(|| {
            AikitError::new(
                "hook.chain_order_impossible",
                format!(
                    "the hooks in the {event} chain depend on each other in a cycle, so the chain \
                     cannot be ordered"
                ),
            )
            .with("event", event.as_str())
            .with(
                "capabilities",
                steps
                    .iter()
                    .map(|s| s.capsule.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        })?;

        let mut by_id: BTreeMap<CapsuleId, HookStep> =
            steps.into_iter().map(|s| (s.capsule.clone(), s)).collect();
        let steps: Vec<HookStep> = order.iter().filter_map(|id| by_id.remove(id)).collect();

        Ok(Self {
            event,
            steps,
            dependencies: edges,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn step(&self, capsule: &CapsuleId) -> Option<&HookStep> {
        self.steps.iter().find(|s| &s.capsule == capsule)
    }

    /// Partition the chain into runs the CLI may execute together.
    ///
    /// Consecutive steps are grouped when they share a phase, all opted into
    /// parallel execution, and none of them depends on another already in the
    /// group. Everything else is a run of one.
    pub fn execution_groups(&self) -> Vec<ExecutionGroup> {
        let mut groups: Vec<ExecutionGroup> = Vec::new();
        for step in &self.steps {
            let extends = groups.last().is_some_and(|group| {
                group.parallel
                    && group.phase == step.phase
                    && step.groupable()
                    && !group
                        .capsules
                        .iter()
                        .any(|c| self.depends_on(&step.capsule, c))
            });
            match (extends, groups.last_mut()) {
                (true, Some(group)) => group.capsules.push(step.capsule.clone()),
                _ => groups.push(ExecutionGroup {
                    phase: step.phase,
                    parallel: step.groupable(),
                    capsules: vec![step.capsule.clone()],
                }),
            }
        }
        // A "parallel" run of one is just a serial step; saying otherwise would
        // invite the CLI to pay for a thread it cannot use.
        for group in &mut groups {
            if group.capsules.len() == 1 {
                group.parallel = false;
            }
        }
        groups
    }

    fn depends_on(&self, dependent: &CapsuleId, dependency: &CapsuleId) -> bool {
        self.dependencies
            .get(dependent)
            .is_some_and(|deps| deps.contains(dependency))
    }
}

/// Kahn's algorithm over `edges` (dependent → its dependencies), always taking the
/// lowest remaining base-order step so the result is a deterministic function of
/// the inputs rather than of any hash iteration order. `None` means a cycle.
fn topological_order(
    steps: &[HookStep],
    edges: &BTreeMap<CapsuleId, Vec<CapsuleId>>,
) -> Option<Vec<CapsuleId>> {
    let position: BTreeMap<&CapsuleId, usize> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| (&s.capsule, i))
        .collect();

    let mut remaining: BTreeMap<&CapsuleId, usize> = steps
        .iter()
        .map(|s| (&s.capsule, edges.get(&s.capsule).map_or(0, Vec::len)))
        .collect();
    let mut dependents: BTreeMap<&CapsuleId, Vec<&CapsuleId>> = BTreeMap::new();
    for (dependent, deps) in edges {
        for dep in deps {
            dependents.entry(dep).or_default().push(dependent);
        }
    }

    let mut ordered: Vec<CapsuleId> = Vec::with_capacity(steps.len());
    while ordered.len() < steps.len() {
        let next = remaining
            .iter()
            .filter(|(_, blocked)| **blocked == 0)
            .map(|(id, _)| *id)
            .min_by_key(|id| position.get(id).copied().unwrap_or(usize::MAX))?;
        remaining.remove(next);
        for dependent in dependents.get(next).into_iter().flatten() {
            if let Some(blocked) = remaining.get_mut(*dependent) {
                *blocked = blocked.saturating_sub(1);
            }
        }
        ordered.push(next.clone());
    }
    Some(ordered)
}

// ---------------------------------------------------------------------------
// Building chains from a resolved view
// ---------------------------------------------------------------------------

/// Build one chain per event from the active hook capsules in a resolved view.
///
/// Deviates from a plain `BTreeMap` return by yielding a `Result`: a chain whose
/// dependencies cannot be ordered, or whose matcher does not compile, must fail
/// visibly when the generation is built rather than at the first tool call of a
/// session.
pub fn build_chains(
    view: &ResolvedView,
    capsules: &dyn Catalog,
) -> Result<BTreeMap<String, HookChain>> {
    let mut by_event: BTreeMap<String, (HookEventKind, Vec<HookStep>)> = BTreeMap::new();
    let mut dependencies: BTreeMap<CapsuleId, Vec<CapsuleId>> = BTreeMap::new();

    for active in view.active_of_kind(Kind::Hook) {
        // The view and the catalog disagreeing means the catalog moved under us.
        // Skipping is honest; inventing a step from stale metadata is not.
        let Some(section) = capsules.get(&active.id).and_then(|c| c.hook().cloned()) else {
            continue;
        };

        if let Some(pattern) = &section.matcher {
            if let Err(e) = regex::Regex::new(pattern) {
                return Err(AikitError::new(
                    "hook.invalid_matcher",
                    format!(
                        "{}'s matcher `{pattern}` is not a valid regex: {e}",
                        active.id
                    ),
                )
                .with("capability", active.id.to_string())
                .with("matcher", pattern.clone()));
            }
        }

        dependencies.insert(
            active.id.clone(),
            active
                .dependencies
                .iter()
                .filter(|d| d.kind() == Kind::Hook)
                .cloned()
                .collect(),
        );

        let step = HookStep {
            capsule: active.id.clone(),
            entry: section.entry.clone(),
            phase: section.phase,
            order: section.order,
            timeout: section.timeout,
            failure: section.failure,
            serial: section.serial,
            matcher: section.matcher.clone(),
            bypass: section.bypass.clone(),
            config: active.config.clone(),
        };

        for declared in &section.events {
            let kind = HookEventKind::parse(declared);
            by_event
                .entry(kind.as_str().to_string())
                .or_insert_with(|| (kind, Vec::new()))
                .1
                .push(step.clone());
        }
    }

    let mut chains = BTreeMap::new();
    for (key, (kind, steps)) in by_event {
        chains.insert(key, HookChain::plan(kind, steps, &dependencies)?);
    }
    Ok(chains)
}

// ---------------------------------------------------------------------------
// Bypass
// ---------------------------------------------------------------------------

/// How long a bypass token is good for.
///
/// There is deliberately no "until I turn it off" scope. A bypass that outlives
/// the reason it was issued for is indistinguishable from having no gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BypassScope {
    /// Spent by the first event it covers.
    NextEvent,
    /// Lasts for the session space that issued it.
    Session,
    /// Lasts a wall-clock window. Expiry is the store's job; core only records it.
    Duration(HumanDuration),
}

impl BypassScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            BypassScope::NextEvent => "next-event",
            BypassScope::Session => "session",
            BypassScope::Duration(_) => "duration",
        }
    }

    /// Whether dispatching one event exhausts the token.
    pub fn is_spent_by_one_event(&self) -> bool {
        matches!(self, BypassScope::NextEvent)
    }
}

/// A short-lived, scoped permission to skip hooks that allow being skipped.
///
/// Validity in *time* is not decided here: core has no clock and does no I/O, so
/// the store issues, expires and revokes tokens. What this module decides is
/// *applicability* — whether a still-valid token covers a particular step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BypassToken {
    pub scope: BypassScope,
    #[serde(default)]
    pub reason: Option<String>,
    /// The capsule this token was issued for. `None` covers the whole chain.
    #[serde(default)]
    pub issued_for: Option<CapsuleId>,
}

impl BypassToken {
    pub fn new(scope: BypassScope) -> Self {
        Self {
            scope,
            reason: None,
            issued_for: None,
        }
    }

    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    #[must_use]
    pub fn for_capsule(mut self, capsule: CapsuleId) -> Self {
        self.issued_for = Some(capsule);
        self
    }

    fn applies_to(&self, step: &HookStep) -> BypassRuling {
        if !step.bypass.allowed {
            return BypassRuling::Forbidden;
        }
        if self.issued_for.as_ref().is_some_and(|t| t != &step.capsule) {
            return BypassRuling::NotIssuedForThisStep;
        }
        let reason = self
            .reason
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty());
        match (step.bypass.reason_required, reason) {
            (true, None) => BypassRuling::ReasonRequired,
            (_, given) => BypassRuling::Applies {
                reason: given.unwrap_or("no reason given").to_string(),
            },
        }
    }
}

enum BypassRuling {
    Applies { reason: String },
    Forbidden,
    ReasonRequired,
    NotIssuedForThisStep,
}

// ---------------------------------------------------------------------------
// Step results
// ---------------------------------------------------------------------------

/// What a runner reports about one executed step.
#[derive(Debug, Clone, PartialEq)]
pub enum StepVerdict {
    Allow,
    Deny {
        reason: String,
    },
    /// Replace the event payload every later step sees.
    Transform {
        payload: serde_json::Value,
    },
    Inject {
        text: String,
    },
    /// The capsule crashed, timed out, or could not be executed at all. This is
    /// *not* a decision, and is never reported as one.
    SystemFailure {
        error: String,
    },
}

/// A verdict plus how long the step took.
///
/// The duration is optional because core cannot measure it; the CLI fills it in
/// so `aikit hook dispatch --json` and the palette can show where a slow chain
/// spends its time.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    pub verdict: StepVerdict,
    pub duration: Option<Duration>,
}

impl StepResult {
    pub fn new(verdict: StepVerdict) -> Self {
        Self {
            verdict,
            duration: None,
        }
    }

    pub fn allow() -> Self {
        Self::new(StepVerdict::Allow)
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self::new(StepVerdict::Deny {
            reason: reason.into(),
        })
    }

    pub fn transform(payload: serde_json::Value) -> Self {
        Self::new(StepVerdict::Transform { payload })
    }

    pub fn inject(text: impl Into<String>) -> Self {
        Self::new(StepVerdict::Inject { text: text.into() })
    }

    pub fn system_failure(error: impl Into<String>) -> Self {
        Self::new(StepVerdict::SystemFailure {
            error: error.into(),
        })
    }

    #[must_use]
    pub fn taking(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }
}

// ---------------------------------------------------------------------------
// The decision record
// ---------------------------------------------------------------------------

/// What happened to one step. `Denied` and `SystemFailure` are deliberately
/// separate variants and never collapse into one another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum StepOutcome {
    Allowed,
    /// The capsule made a decision: it said no.
    Denied,
    Transformed,
    Injected,
    /// The capsule could not run. The failure policy decides what that means.
    SystemFailure {
        policy: FailurePolicy,
    },
    /// Covered by a bypass token; never executed.
    Bypassed,
    /// The matcher did not select this step for this event.
    NotMatched,
    /// An earlier step denied the event and this phase runs before the finally stage.
    ShortCircuited,
}

impl StepOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            StepOutcome::Allowed => "allowed",
            StepOutcome::Denied => "denied",
            StepOutcome::Transformed => "transformed",
            StepOutcome::Injected => "injected",
            StepOutcome::SystemFailure { .. } => "system-failure",
            StepOutcome::Bypassed => "bypassed",
            StepOutcome::NotMatched => "not-matched",
            StepOutcome::ShortCircuited => "short-circuited",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    pub capsule: CapsuleId,
    pub phase: HookPhase,
    pub outcome: StepOutcome,
    /// Filled in by the runner; `None` when the step never executed.
    #[serde(default)]
    pub duration: Option<Duration>,
    pub bypassed: bool,
    #[serde(default)]
    pub denial_reason: Option<String>,
}

/// Why the event was stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Denial {
    pub capsule: CapsuleId,
    pub phase: HookPhase,
    pub reason: String,
    /// True when a crashed hook was resolved to a denial by its failure policy,
    /// rather than the hook having decided anything. The message the user sees
    /// must not claim their action was refused when in fact a check broke.
    pub from_system_failure: bool,
}

impl Denial {
    pub fn describe(&self) -> String {
        if self.from_system_failure {
            format!(
                "{} could not run and is configured to fail closed: {}",
                self.capsule, self.reason
            )
        } else {
            format!("{} denied this event: {}", self.capsule, self.reason)
        }
    }
}

/// The complete outcome of dispatching one event through one chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookDecision {
    pub event: HookEventKind,
    pub allowed: bool,
    #[serde(default)]
    pub denial: Option<Denial>,
    /// The payload after any transforms.
    pub payload: serde_json::Value,
    pub injected: Vec<String>,
    pub warnings: Vec<String>,
    pub steps: Vec<StepRecord>,
    pub groups: Vec<ExecutionGroup>,
    /// True when a `next-event` bypass token was spent covering this dispatch.
    pub bypass_consumed: bool,
}

impl HookDecision {
    pub fn step(&self, capsule: &str) -> Option<&StepRecord> {
        self.steps.iter().find(|s| s.capsule.to_string() == capsule)
    }

    /// Injected fragments joined for handing back to the client.
    pub fn injected_text(&self) -> String {
        self.injected.join("\n\n")
    }

    pub fn was_bypassed(&self) -> bool {
        self.steps.iter().any(|s| s.bypassed)
    }
}

// ---------------------------------------------------------------------------
// The dispatcher
// ---------------------------------------------------------------------------

/// Interprets a chain against an event.
///
/// Holds no resources: the CLI constructs one per dispatch and hands it a runner.
#[derive(Debug, Clone, Default)]
pub struct Dispatcher {
    bypass: Option<BypassToken>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_bypass(token: BypassToken) -> Self {
        Self {
            bypass: Some(token),
        }
    }

    /// Run `chain` against `event`, delegating execution to `run_step`.
    ///
    /// Steps are folded strictly in chain order even when
    /// [`HookChain::execution_groups`] says a run may be parallelized: the CLI is
    /// free to have already executed a group and to serve `run_step` from a cache,
    /// but the decision must not depend on which of two verifiers happened to
    /// finish first, or two dispatches of the same event would blame different
    /// capsules.
    pub fn run<F>(&self, chain: &HookChain, event: &HookEvent, run_step: &mut F) -> HookDecision
    where
        F: FnMut(&HookStep, &HookEvent) -> StepResult,
    {
        let mut working = event.clone();
        let mut decision = HookDecision {
            event: chain.event.clone(),
            allowed: true,
            denial: None,
            payload: event.payload.clone(),
            injected: Vec::new(),
            warnings: Vec::new(),
            steps: Vec::new(),
            groups: chain.execution_groups(),
            bypass_consumed: false,
        };

        // Two passes: the deciding phases, then a finally stage that runs whatever
        // the outcome. Observers exist to record what happened, which is most
        // valuable precisely when something was refused.
        for step in chain.steps.iter().filter(|s| !s.phase.is_terminal_stage()) {
            if decision.denial.is_some() {
                decision.steps.push(StepRecord {
                    capsule: step.capsule.clone(),
                    phase: step.phase,
                    outcome: StepOutcome::ShortCircuited,
                    duration: None,
                    bypassed: false,
                    denial_reason: None,
                });
                continue;
            }
            self.run_one(step, &mut working, &mut decision, run_step);
        }

        for step in chain.steps.iter().filter(|s| s.phase.is_terminal_stage()) {
            self.run_one(step, &mut working, &mut decision, run_step);
        }

        decision.allowed = decision.denial.is_none();
        decision.payload = working.payload;
        decision
    }

    fn run_one<F>(
        &self,
        step: &HookStep,
        working: &mut HookEvent,
        decision: &mut HookDecision,
        run_step: &mut F,
    ) where
        F: FnMut(&HookStep, &HookEvent) -> StepResult,
    {
        if !matches(step, working) {
            decision.steps.push(StepRecord {
                capsule: step.capsule.clone(),
                phase: step.phase,
                outcome: StepOutcome::NotMatched,
                duration: None,
                bypassed: false,
                denial_reason: None,
            });
            return;
        }

        if let Some(token) = &self.bypass {
            match token.applies_to(step) {
                BypassRuling::Applies { reason } => {
                    decision.warnings.push(format!(
                        "BYPASSED {} ({} scope): {reason}",
                        step.capsule,
                        token.scope.as_str()
                    ));
                    if token.scope.is_spent_by_one_event() {
                        decision.bypass_consumed = true;
                    }
                    decision.steps.push(StepRecord {
                        capsule: step.capsule.clone(),
                        phase: step.phase,
                        outcome: StepOutcome::Bypassed,
                        duration: None,
                        bypassed: true,
                        denial_reason: None,
                    });
                    return;
                }
                BypassRuling::Forbidden => decision.warnings.push(format!(
                    "{} does not permit bypass; the bypass token was ignored",
                    step.capsule
                )),
                BypassRuling::ReasonRequired => decision.warnings.push(format!(
                    "{} requires a reason to be bypassed and the token carries none; it ran \
                     normally",
                    step.capsule
                )),
                BypassRuling::NotIssuedForThisStep => {}
            }
        }

        let result = run_step(step, working);
        let duration = result.duration;
        let (outcome, denial_reason) = match result.verdict {
            StepVerdict::Allow => (StepOutcome::Allowed, None),

            StepVerdict::Transform { payload } => {
                working.payload = payload;
                (StepOutcome::Transformed, None)
            }

            StepVerdict::Inject { text } => {
                if !text.is_empty() {
                    decision.injected.push(text);
                }
                (StepOutcome::Injected, None)
            }

            StepVerdict::Deny { reason } => {
                if step.phase.can_deny() {
                    decision.denial = Some(Denial {
                        capsule: step.capsule.clone(),
                        phase: step.phase,
                        reason: reason.clone(),
                        from_system_failure: false,
                    });
                } else {
                    // Recorded, not enforced. Dropping it silently would leave a
                    // capsule author believing their check was doing something.
                    decision.warnings.push(format!(
                        "{} returned a denial from the {} phase, which cannot deny; the event \
                         continued: {reason}",
                        step.capsule,
                        step.phase.as_str()
                    ));
                }
                (StepOutcome::Denied, Some(reason))
            }

            StepVerdict::SystemFailure { error } => {
                let policy = step.failure;
                let denies = policy == FailurePolicy::Closed && step.phase.can_deny();
                if denies {
                    decision.denial = Some(Denial {
                        capsule: step.capsule.clone(),
                        phase: step.phase,
                        reason: error.clone(),
                        from_system_failure: true,
                    });
                    decision.warnings.push(format!(
                        "{} failed to run and is configured to fail closed: {error}",
                        step.capsule
                    ));
                } else {
                    // A closed policy on a phase that cannot deny is worth naming:
                    // the author probably expected teeth the phase cannot have.
                    let policy_note = match policy {
                        FailurePolicy::Closed => format!(
                            "fail-closed, but the {} phase cannot deny",
                            step.phase.as_str()
                        ),
                        FailurePolicy::Open => "fail-open".to_string(),
                        FailurePolicy::Warn => "fail-warn".to_string(),
                    };
                    decision.warnings.push(format!(
                        "{} failed to run ({policy_note}): {error}",
                        step.capsule
                    ));
                }
                (StepOutcome::SystemFailure { policy }, Some(error))
            }
        };

        decision.steps.push(StepRecord {
            capsule: step.capsule.clone(),
            phase: step.phase,
            outcome,
            duration,
            bypassed: false,
            denial_reason,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kinds_survive_a_json_round_trip_including_unknown_ones() {
        for kind in [
            HookEventKind::PreToolUse,
            HookEventKind::Other("SubagentStop".into()),
        ] {
            let encoded = serde_json::to_string(&kind).unwrap();
            let decoded: HookEventKind = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, kind);
        }
    }

    #[test]
    fn a_bypass_scope_with_a_window_records_its_length_and_is_not_single_use() {
        let scope = BypassScope::Duration(HumanDuration::parse("10m").unwrap());
        assert_eq!(scope.as_str(), "duration");
        assert!(!scope.is_spent_by_one_event());
        assert!(BypassScope::NextEvent.is_spent_by_one_event());
    }

    #[test]
    fn an_empty_chain_allows_the_event_and_records_nothing() {
        let chain = HookChain::plan(HookEventKind::Stop, vec![], &BTreeMap::new()).unwrap();
        let event = HookEvent::new("claude", HookEventKind::Stop, serde_json::json!({}));
        let mut runner = |_: &HookStep, _: &HookEvent| StepResult::allow();
        let decision = Dispatcher::new().run(&chain, &event, &mut runner);
        assert!(decision.allowed);
        assert!(decision.steps.is_empty());
        assert!(decision.groups.is_empty());
    }

    #[test]
    fn a_denial_describes_itself_differently_when_it_came_from_a_crash() {
        let policy = Denial {
            capsule: CapsuleId::parse("hook/gate/a").unwrap(),
            phase: HookPhase::Gate,
            reason: "outside the project".into(),
            from_system_failure: false,
        };
        let crash = Denial {
            from_system_failure: true,
            ..policy.clone()
        };
        assert!(policy.describe().contains("denied this event"));
        assert!(crash.describe().contains("could not run"));
    }
}
