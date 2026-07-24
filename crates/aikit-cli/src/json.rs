//! The stable JSON envelope and the exit-code table.
//!
//! `--json` output is a real public interface: alternative front-ends, shell
//! integrations and this crate's own tests match on it. Two things are therefore
//! promised and must not drift:
//!
//! * the **shape** — `schema`, `ok`, and then either `context`/`data`/`warnings`
//!   (success) or `error` (failure), exactly as in ARCHITECTURE.md §12;
//! * the **error codes** — [`aikit_core::AikitError::code`] is stable even though
//!   the message is not, and the exit code a shell sees is derived from it.
//!
//! The context block always reports all three of `context_id`, `session_id` and
//! `project_root`, using JSON `null` for the ones that do not apply, so a consumer
//! never has to distinguish "absent key" from "no session".

use aikit_core::AikitError;
use serde_json::{json, Value};

/// The envelope schema version. Bumped only on a breaking change to the shape.
pub const SCHEMA: u64 = 1;

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

/// Everything went as asked.
pub const EXIT_OK: i32 = 0;
/// A generic runtime failure with no more specific classification.
pub const EXIT_GENERIC: i32 = 1;
/// The command line itself was wrong (bad flags, missing argument).
pub const EXIT_USAGE: i32 = 2;
/// Resolution could not produce a view (a required capability was disabled, a
/// conflict, a cycle, a missing dependency…).
pub const EXIT_RESOLUTION: i32 = 3;
/// A managed policy denied the request.
pub const EXIT_POLICY: i32 = 4;
/// The action needs a trust review that has not happened.
pub const EXIT_TRUST: i32 = 5;
/// A per-context lock is held by another process.
pub const EXIT_LOCK: i32 = 6;

/// The exit code a shell should see for a given domain error.
///
/// The mapping is on the stable code, not the message. `policy.denied` and
/// `trust.required` are CLI-owned codes: the resolver reports a *denied* or
/// *trust-required* capability as an [`aikit_core::resolve::UnavailableReason`]
/// rather than a fatal error, so the specific "you asked for this and the answer
/// is no" outcomes are minted here, at the command boundary, where the user's
/// intent is known.
pub fn exit_code(error: &AikitError) -> i32 {
    match error.code() {
        "lock.busy" | "lock.unavailable" => EXIT_LOCK,
        "trust.required" => EXIT_TRUST,
        "policy.denied" => EXIT_POLICY,
        "cli.usage" => EXIT_USAGE,
        code if code.starts_with("resolution.") => EXIT_RESOLUTION,
        _ => EXIT_GENERIC,
    }
}

// ---------------------------------------------------------------------------
// Envelope construction
// ---------------------------------------------------------------------------

/// The context block of a success envelope.
///
/// Held as owned strings rather than the richer [`aikit_core::ContextDescriptor`]
/// because the envelope publishes exactly three fields and adding a field to the
/// descriptor must not silently widen the public contract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvelopeContext {
    pub context_id: Option<String>,
    pub session_id: Option<String>,
    pub project_root: Option<String>,
}

impl EnvelopeContext {
    /// Derive the envelope context from a resolved descriptor.
    pub fn from_descriptor(descriptor: &aikit_core::ContextDescriptor) -> Self {
        Self {
            context_id: Some(descriptor.context_id.to_string()),
            session_id: descriptor.session_id.as_ref().map(|s| s.to_string()),
            project_root: descriptor
                .project_root
                .as_ref()
                .map(|p| p.display().to_string()),
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "context_id": self.context_id,
            "session_id": self.session_id,
            "project_root": self.project_root,
        })
    }
}

/// A success envelope: `{ schema, ok: true, context, data, warnings }`.
pub fn success(context: &EnvelopeContext, data: Value, warnings: Vec<String>) -> Value {
    json!({
        "schema": SCHEMA,
        "ok": true,
        "context": context.to_value(),
        "data": data,
        "warnings": warnings,
    })
}

/// A failure envelope: `{ schema, ok: false, error: { code, message, details } }`.
///
/// The error block is deliberately self-contained: an error can be produced
/// before a context is known (a malformed argument, a lock held before discovery
/// runs), so the failure shape does not depend on having resolved a context.
pub fn failure(error: &AikitError) -> Value {
    let details: serde_json::Map<String, Value> = error
        .details()
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    json!({
        "schema": SCHEMA,
        "ok": false,
        "error": {
            "code": error.code(),
            "message": error.message(),
            "details": Value::Object(details),
        }
    })
}

/// Render a value as a single line of JSON, the form the CLI prints for `--json`.
pub fn line(value: &Value) -> String {
    value.to_string()
}

/// Render a value as pretty multi-line JSON, for the human-facing default when a
/// command has no bespoke text renderer.
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}
