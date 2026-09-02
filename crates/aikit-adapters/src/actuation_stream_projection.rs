//! Projection from AIKit provider-local connection signals into the portable
//! Actuation-owned `actuation.stream/v1` event vocabulary.
//!
//! `ConnectionSignal.sequence` preserves one adapter/wire ordering domain. It is
//! deliberately not promoted into canonical ActuationStream ordering: the caller
//! supplies the next Stream sequence when appending. Provider-native session ids
//! likewise remain provenance/material facts rather than AgentSession identity.

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::agent_connection::{ConnectionSignal, ConnectionSignalKind};

pub const ACTUATION_STREAM_SCHEMA: &str = "actuation.stream/v1";
pub const ACTUATION_STREAM_OWNER_REVISION: &str = "bece3da0da0369c8f7495d443944e60f849a3f8d";
pub const CONNECTION_SIGNAL_STREAM_PROJECTION_VERSION: &str =
    "aikit.connection-signal-actuation-stream/v1";

/// Canonical semantic identities supplied by the Actuation/SessionSpace caller.
/// The connection adapter never synthesizes these from provider-native ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActuationStreamProjectionContext {
    pub stream_ref: ResourceRef,
    pub actuation_ref: ResourceRef,
    pub agency_ref: ResourceRef,
    pub agent_session_ref: ResourceRef,
    pub connection_ref: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_ref: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_ref: Option<ResourceRef>,
}

impl ActuationStreamProjectionContext {
    pub fn validate(&self) -> Result<()> {
        let semantic = [
            ("stream_ref", &self.stream_ref),
            ("actuation_ref", &self.actuation_ref),
            ("agency_ref", &self.agency_ref),
            ("agent_session_ref", &self.agent_session_ref),
        ];
        for (index, (left_name, left)) in semantic.iter().enumerate() {
            for (right_name, right) in semantic.iter().skip(index + 1) {
                if left == right {
                    return Err(AikitError::new(
                        "actuation_stream_projection.identity_collapse",
                        format!(
                            "{left_name} and {right_name} must remain distinct semantic identities"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// One append-ready portable event plus the canonical Stream identity context it
/// belongs to. Runtime persistence/subscription remains Actuation/gateway-owned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActuationStreamAppendProjection {
    pub schema: String,
    pub stream_ref: ResourceRef,
    pub actuation_ref: ResourceRef,
    pub agency_ref: ResourceRef,
    pub agent_session_ref: ResourceRef,
    pub event: Value,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// Project one observed provider-local signal into one canonical portable Stream
/// event. `stream_sequence` is assigned by the Stream appender; it is not inferred
/// from `ConnectionSignal.sequence`.
pub fn project_connection_signal_to_actuation_stream(
    context: &ActuationStreamProjectionContext,
    signal: &ConnectionSignal,
    stream_sequence: u64,
) -> Result<ActuationStreamAppendProjection> {
    context.validate()?;
    if stream_sequence == 0 {
        return Err(AikitError::new(
            "actuation_stream_projection.invalid_sequence",
            "ActuationStream event sequence starts at 1",
        ));
    }

    let (kind, content, native) = portable_signal(signal);
    let mut actor = Map::new();
    actor.insert("agency_ref".into(), json!(context.agency_ref.to_string()));
    if let Some(agent) = &context.agent_ref {
        actor.insert("agent_ref".into(), json!(agent.to_string()));
    }

    let native_trace_ref = format!(
        "aikit-connection-signal/{}/{}",
        context.connection_ref, signal.sequence
    );
    let event_ref = format!("{}/event/{}", context.stream_ref, stream_sequence);
    let mut metadata = Map::new();
    metadata.insert(
        "projection_schema".into(),
        json!(CONNECTION_SIGNAL_STREAM_PROJECTION_VERSION),
    );
    metadata.insert(
        "connection_ref".into(),
        json!(context.connection_ref.to_string()),
    );
    metadata.insert("connection_signal_sequence".into(), json!(signal.sequence));
    if let Some(native_session_id) = &signal.native_session_id {
        metadata.insert("native_session_id".into(), json!(native_session_id));
    }
    if !signal.provenance.is_empty() {
        metadata.insert("connection_provenance".into(), json!(signal.provenance));
    }
    if let Some(native) = native {
        metadata.insert("native".into(), native);
    }

    let mut event = Map::new();
    event.insert("event_ref".into(), json!(event_ref));
    event.insert("sequence".into(), json!(stream_sequence));
    event.insert("kind".into(), json!(kind));
    event.insert("actor".into(), Value::Object(actor));
    event.insert("native_trace_ref".into(), json!(native_trace_ref));
    event.insert("disclosure".into(), json!("portable"));
    event.insert("metadata".into(), Value::Object(metadata));
    if let Some(surface) = &context.surface_ref {
        event.insert("surface_ref".into(), json!(surface.to_string()));
    }
    if let Some(content) = content {
        event.insert("content".into(), json!(content));
    }

    Ok(ActuationStreamAppendProjection {
        schema: ACTUATION_STREAM_SCHEMA.into(),
        stream_ref: context.stream_ref.clone(),
        actuation_ref: context.actuation_ref.clone(),
        agency_ref: context.agency_ref.clone(),
        agent_session_ref: context.agent_session_ref.clone(),
        event: Value::Object(event),
        provenance: vec![
            format!("Actuation owner revision {ACTUATION_STREAM_OWNER_REVISION}"),
            format!("AIKit connection signal {}", signal.sequence),
        ],
    })
}

fn portable_signal(signal: &ConnectionSignal) -> (&'static str, Option<String>, Option<Value>) {
    match &signal.kind {
        ConnectionSignalKind::SessionOpened { binding } => (
            "harness-event",
            None,
            Some(json!({
                "event": "session-opened",
                "opened_as": binding.opened_as,
                "native_session_id": binding.native_session_id,
                "bound_agent_session": binding.agent_session.as_ref().map(ToString::to_string),
                "agent": binding.agent.as_ref().map(ToString::to_string),
                "provenance": binding.provenance,
            })),
        ),
        ConnectionSignalKind::AgentMessageChunk { text } => {
            ("model-delta", Some(text.clone()), None)
        }
        ConnectionSignalKind::ToolCall { payload } => (
            "tool-request",
            None,
            Some(json!({ "event": "tool-call", "payload": payload })),
        ),
        ConnectionSignalKind::ToolResult { payload } => (
            "tool-result",
            None,
            Some(json!({ "event": "tool-result", "payload": payload })),
        ),
        ConnectionSignalKind::PermissionRequested { request } => (
            "permission",
            None,
            Some(json!({
                "event": "permission-requested",
                "native_request_id": request.native_request_id,
                "native_session_id": request.native_session_id,
                "tool_call_id": request.tool_call_id,
                "choices": request.choices,
                "provenance": request.provenance,
            })),
        ),
        ConnectionSignalKind::Completed { stop_reason } => (
            "model-result",
            None,
            Some(json!({ "event": "completed", "stop_reason": stop_reason })),
        ),
        ConnectionSignalKind::Cancelled => ("cancellation", None, None),
        ConnectionSignalKind::Status { message } => (
            "harness-event",
            Some(message.clone()),
            Some(json!({ "event": "status" })),
        ),
        ConnectionSignalKind::Degraded { degradation } => (
            "harness-event",
            None,
            Some(json!({
                "event": "degraded",
                "reason": degradation.reason,
                "unavailable": degradation.unavailable,
            })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_connection::{ConnectionDegradation, NativeSessionBinding, SessionOpenMode};

    fn r(value: &str) -> ResourceRef {
        ResourceRef::parse(value).unwrap()
    }

    fn context() -> ActuationStreamProjectionContext {
        ActuationStreamProjectionContext {
            stream_ref: r("actuation-stream/root"),
            actuation_ref: r("actuation/root"),
            agency_ref: r("agency/root"),
            agent_session_ref: r("agent-session/root"),
            connection_ref: r("connection/acp-root"),
            agent_ref: Some(r("agent/root")),
            surface_ref: Some(r("surface/cradle")),
        }
    }

    #[test]
    fn message_chunk_projects_to_model_delta_with_canonical_stream_order() {
        let signal = ConnectionSignal {
            sequence: 41,
            native_session_id: Some("native-7".into()),
            kind: ConnectionSignalKind::AgentMessageChunk {
                text: "hello".into(),
            },
            provenance: vec!["ACP session/update".into()],
        };
        let projected =
            project_connection_signal_to_actuation_stream(&context(), &signal, 9).unwrap();
        assert_eq!(projected.schema, ACTUATION_STREAM_SCHEMA);
        assert_eq!(projected.event["sequence"], 9);
        assert_eq!(projected.event["kind"], "model-delta");
        assert_eq!(projected.event["content"], "hello");
        assert_eq!(projected.event["surface_ref"], "surface/cradle");
        assert_eq!(
            projected.event["metadata"]["connection_signal_sequence"],
            41
        );
        assert_eq!(projected.event["metadata"]["native_session_id"], "native-7");
    }

    #[test]
    fn local_connection_sequence_is_never_promoted_to_stream_sequence() {
        let signal = ConnectionSignal {
            sequence: 900,
            native_session_id: None,
            kind: ConnectionSignalKind::Status {
                message: "reconnected".into(),
            },
            provenance: Vec::new(),
        };
        let projected =
            project_connection_signal_to_actuation_stream(&context(), &signal, 3).unwrap();
        assert_eq!(projected.event["sequence"], 3);
        assert_eq!(
            projected.event["metadata"]["connection_signal_sequence"],
            900
        );
    }

    #[test]
    fn session_open_keeps_provider_session_id_inside_native_evidence() {
        let signal = ConnectionSignal {
            sequence: 1,
            native_session_id: Some("provider-native".into()),
            kind: ConnectionSignalKind::SessionOpened {
                binding: NativeSessionBinding::unbound("provider-native", SessionOpenMode::Create),
            },
            provenance: Vec::new(),
        };
        let projected =
            project_connection_signal_to_actuation_stream(&context(), &signal, 1).unwrap();
        assert_eq!(projected.agent_session_ref, r("agent-session/root"));
        assert_eq!(
            projected.event["metadata"]["native"]["native_session_id"],
            "provider-native"
        );
        assert!(projected.event.get("agent_session_ref").is_none());
    }

    #[test]
    fn tool_payload_is_preserved_as_native_material_under_portable_event_kind() {
        let signal = ConnectionSignal {
            sequence: 12,
            native_session_id: None,
            kind: ConnectionSignalKind::ToolCall {
                payload: json!({"name":"factory.inspect","arguments":{"run":"run/7"}}),
            },
            provenance: Vec::new(),
        };
        let projected =
            project_connection_signal_to_actuation_stream(&context(), &signal, 5).unwrap();
        assert_eq!(projected.event["kind"], "tool-request");
        assert_eq!(
            projected.event["metadata"]["native"]["payload"]["name"],
            "factory.inspect"
        );
    }

    #[test]
    fn cancellation_and_degradation_are_not_flattened_into_chat_text() {
        let cancelled = ConnectionSignal {
            sequence: 2,
            native_session_id: None,
            kind: ConnectionSignalKind::Cancelled,
            provenance: Vec::new(),
        };
        let projected =
            project_connection_signal_to_actuation_stream(&context(), &cancelled, 2).unwrap();
        assert_eq!(projected.event["kind"], "cancellation");
        assert!(projected.event.get("content").is_none());

        let degraded = ConnectionSignal {
            sequence: 3,
            native_session_id: None,
            kind: ConnectionSignalKind::Degraded {
                degradation: ConnectionDegradation {
                    reason: "provider unavailable".into(),
                    unavailable: vec!["resume".into()],
                },
            },
            provenance: Vec::new(),
        };
        let projected =
            project_connection_signal_to_actuation_stream(&context(), &degraded, 3).unwrap();
        assert_eq!(projected.event["kind"], "harness-event");
        assert_eq!(
            projected.event["metadata"]["native"]["reason"],
            "provider unavailable"
        );
    }

    #[test]
    fn invalid_canonical_identity_or_zero_sequence_is_rejected() {
        let signal = ConnectionSignal {
            sequence: 1,
            native_session_id: None,
            kind: ConnectionSignalKind::Cancelled,
            provenance: Vec::new(),
        };
        assert_eq!(
            project_connection_signal_to_actuation_stream(&context(), &signal, 0)
                .unwrap_err()
                .code(),
            "actuation_stream_projection.invalid_sequence"
        );

        let mut collapsed = context();
        collapsed.actuation_ref = collapsed.stream_ref.clone();
        assert_eq!(
            project_connection_signal_to_actuation_stream(&collapsed, &signal, 1)
                .unwrap_err()
                .code(),
            "actuation_stream_projection.identity_collapse"
        );
    }
}
