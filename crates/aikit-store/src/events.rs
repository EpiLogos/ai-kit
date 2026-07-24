//! Structured events.
//!
//! ## What an event may never contain
//!
//! Prompt text, transcript content and secret argument values are not "sensitive
//! fields to be careful with" — they are simply not representable here. [`Event`]
//! has no field they could go in, and [`Event::with_arguments`] consults the
//! [`ArgSpec`] rather than the value, so an argument declared `secret = true`
//! whose type is an ordinary string is masked exactly like an `ArgType::Secret`
//! is. Redacting at the sink instead would mean every new sink has to remember;
//! redacting at construction means the un-redacted value never enters the record.
//!
//! ## Two sinks, one record
//!
//! Events go to SQLite (queryable: usage ranking, `aikit explain`, the palette's
//! history) and to `logs/events.jsonl` (greppable, tailable, survives the
//! database being deleted). [`EventRecorder`] writes both from one value so the
//! two can never describe different things.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use aikit_core::arg::{ArgSpec, ArgValues};
use aikit_core::error::err;
use aikit_core::{
    AikitError, CapsuleId, ContextId, EventId, GenerationId, Kind, MuxKind, ProjectId, Result,
    Revision, ScopeKind, SessionId, TargetId,
};

use crate::home::io_error;
use crate::index::Index;

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

/// An instant, rendered as RFC 3339 and stored as nanoseconds.
///
/// Two representations because the two consumers want different things: the
/// JSONL line has to be readable by a person with `less`, and the database has to
/// sort and difference without parsing. Keeping both derived from one value is
/// what stops them drifting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(jiff::Timestamp);

impl Timestamp {
    pub fn now() -> Self {
        Self(jiff::Timestamp::now())
    }

    pub fn from_nanos(nanos: i64) -> Self {
        Self(
            jiff::Timestamp::from_nanosecond(nanos as i128)
                .unwrap_or(jiff::Timestamp::UNIX_EPOCH),
        )
    }

    /// Nanoseconds since the Unix epoch, saturated into an `i64` so it can be a
    /// SQLite INTEGER. The saturation point is the year 2262; AIKit will have
    /// other problems by then.
    pub fn as_nanos(&self) -> i64 {
        self.0.as_nanosecond().clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// How long ago this was, or zero if it is somehow in the future.
    pub fn age(&self, now: Timestamp) -> Duration {
        let delta = now.as_nanos().saturating_sub(self.as_nanos());
        Duration::from_nanos(delta.max(0) as u64)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Timestamp {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        s.parse::<jiff::Timestamp>()
            .map(Self)
            .map_err(|e| AikitError::new("event.bad_timestamp", format!("`{s}` is not a timestamp: {e}")))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Actions and outcomes
// ---------------------------------------------------------------------------

/// What happened. Open-ended enough to cover the verbs AIKit has, closed enough
/// that a query for "runs" cannot silently miss a spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventAction {
    Run,
    Apply,
    Rollback,
    Enable,
    Disable,
    HookDispatch,
    BypassIssued,
    Capture,
    Promote,
    TrustReview,
    RegistrySync,
    SessionUp,
    Gc,
}

impl EventAction {
    pub fn as_str(self) -> &'static str {
        match self {
            EventAction::Run => "run",
            EventAction::Apply => "apply",
            EventAction::Rollback => "rollback",
            EventAction::Enable => "enable",
            EventAction::Disable => "disable",
            EventAction::HookDispatch => "hook-dispatch",
            EventAction::BypassIssued => "bypass-issued",
            EventAction::Capture => "capture",
            EventAction::Promote => "promote",
            EventAction::TrustReview => "trust-review",
            EventAction::RegistrySync => "registry-sync",
            EventAction::SessionUp => "session-up",
            EventAction::Gc => "gc",
        }
    }
}

impl FromStr for EventAction {
    type Err = AikitError;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "run" => EventAction::Run,
            "apply" => EventAction::Apply,
            "rollback" => EventAction::Rollback,
            "enable" => EventAction::Enable,
            "disable" => EventAction::Disable,
            "hook-dispatch" => EventAction::HookDispatch,
            "bypass-issued" => EventAction::BypassIssued,
            "capture" => EventAction::Capture,
            "promote" => EventAction::Promote,
            "trust-review" => EventAction::TrustReview,
            "registry-sync" => EventAction::RegistrySync,
            "session-up" => EventAction::SessionUp,
            "gc" => EventAction::Gc,
            other => return err("event.unknown_action", format!("`{other}` is not an action")),
        })
    }
}

/// How it ended.
///
/// A *policy denial* and a *system failure* are separate variants because
/// `ARCHITECTURE.md` §8 requires them to stay distinguishable: conflating "the
/// gate said no" with "the gate fell over" is how a control quietly stops working.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Outcome {
    Success,
    Failure { code: String },
    Denied { code: String },
    Skipped { reason: String },
}

impl Outcome {
    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::Failure { .. } => "failure",
            Outcome::Denied { .. } => "denied",
            Outcome::Skipped { .. } => "skipped",
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Outcome::Success => None,
            Outcome::Failure { code } | Outcome::Denied { code } => Some(code),
            Outcome::Skipped { reason } => Some(reason),
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Outcome::Success)
    }

    pub(crate) fn from_parts(label: &str, detail: Option<String>) -> Self {
        match label {
            "success" => Outcome::Success,
            "denied" => Outcome::Denied {
                code: detail.unwrap_or_default(),
            },
            "skipped" => Outcome::Skipped {
                reason: detail.unwrap_or_default(),
            },
            _ => Outcome::Failure {
                code: detail.unwrap_or_default(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// The event
// ---------------------------------------------------------------------------

/// One recorded fact about something AIKit did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event_id: EventId,
    pub timestamp: Timestamp,
    pub action: EventAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capsule: Option<CapsuleId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<Revision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<TargetId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mux: Option<MuxKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeKind>,
    pub outcome: Outcome,
    /// Wall-clock duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<GenerationId>,
    /// Present exactly when a hook was skipped under a bypass token. Its presence
    /// is what makes a bypass visible after the fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bypass_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event: Option<EventId>,
    /// Argument names to **already redacted** values. See [`Event::with_arguments`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arguments: BTreeMap<String, String>,
}

/// The string a masked argument is recorded as.
pub const REDACTED: &str = "••••••";

impl Event {
    pub fn new(action: EventAction) -> Self {
        Self {
            event_id: EventId::generate(),
            timestamp: Timestamp::now(),
            action,
            session: None,
            context: None,
            project: None,
            capsule: None,
            revision: None,
            kind: None,
            client: None,
            mux: None,
            scope: None,
            outcome: Outcome::Success,
            duration_ms: None,
            generation: None,
            bypass_reason: None,
            parent_event: None,
            arguments: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn at(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = timestamp;
        self
    }

    #[must_use]
    pub fn for_capsule(mut self, capsule: CapsuleId, revision: Revision) -> Self {
        self.kind = Some(capsule.kind());
        self.capsule = Some(capsule);
        self.revision = Some(revision);
        self
    }

    #[must_use]
    pub fn in_context(mut self, context: ContextId, session: Option<SessionId>) -> Self {
        self.context = Some(context);
        self.session = session;
        self
    }

    #[must_use]
    pub fn for_project(mut self, project: ProjectId) -> Self {
        self.project = Some(project);
        self
    }

    #[must_use]
    pub fn with_outcome(mut self, outcome: Outcome) -> Self {
        self.outcome = outcome;
        self
    }

    #[must_use]
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration_ms = Some(duration.as_millis().min(u64::MAX as u128) as u64);
        self
    }

    #[must_use]
    pub fn with_generation(mut self, generation: GenerationId) -> Self {
        self.generation = Some(generation);
        self
    }

    #[must_use]
    pub fn with_client(mut self, client: TargetId) -> Self {
        self.client = Some(client);
        self
    }

    #[must_use]
    pub fn with_mux(mut self, mux: MuxKind) -> Self {
        self.mux = Some(mux);
        self
    }

    #[must_use]
    pub fn with_scope(mut self, scope: ScopeKind) -> Self {
        self.scope = Some(scope);
        self
    }

    #[must_use]
    pub fn with_bypass_reason(mut self, reason: impl Into<String>) -> Self {
        self.bypass_reason = Some(reason.into());
        self
    }

    #[must_use]
    pub fn caused_by(mut self, parent: EventId) -> Self {
        self.parent_event = Some(parent);
        self
    }

    /// Record the arguments a run was given, masking every one the capsule
    /// declared secret.
    ///
    /// The mask is decided by the *specification*, not by the value's runtime
    /// type: `secret = true` on a `string` argument is exactly as binding as
    /// `type = "secret"`. An argument with no declaration at all is recorded
    /// masked, because an undeclared value is one nobody has said is safe.
    #[must_use]
    pub fn with_arguments(mut self, specs: &[ArgSpec], values: &ArgValues) -> Self {
        for (name, value) in values {
            let masked = match specs.iter().find(|s| &s.name == name) {
                Some(spec) if !spec.is_secret() => value.to_argv_string(),
                _ => REDACTED.to_string(),
            };
            self.arguments.insert(name.clone(), masked);
        }
        self
    }

    /// The JSONL line this event contributes to `logs/events.jsonl`.
    pub fn to_json_line(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| {
            AikitError::new("event.serialize_failed", format!("could not encode event: {e}"))
        })
    }
}

// ---------------------------------------------------------------------------
// The recorder
// ---------------------------------------------------------------------------

/// Writes one event to both sinks.
pub struct EventRecorder<'a> {
    index: &'a Index,
    log_path: PathBuf,
}

impl<'a> EventRecorder<'a> {
    pub fn new(index: &'a Index, log_path: impl Into<PathBuf>) -> Self {
        Self {
            index,
            log_path: log_path.into(),
        }
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Record to SQLite and append to the JSONL log.
    ///
    /// The database is written first: it is the sink queries depend on, and a
    /// half-written log line is easier to live with than a run that is invisible
    /// to `aikit explain`.
    pub fn record(&self, event: &Event) -> Result<()> {
        self.index.record_event(event)?;
        self.append(event)
    }

    fn append(&self, event: &Event) -> Result<()> {
        if let Some(parent) = self.log_path.parent() {
            crate::home::create_dir_all(parent)?;
        }
        let line = event.to_json_line()?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| io_error("event.log_unwritable", &self.log_path, &e))?;
        writeln!(file, "{line}").map_err(|e| io_error("event.log_unwritable", &self.log_path, &e))
    }
}
