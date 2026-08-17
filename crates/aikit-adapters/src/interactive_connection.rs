//! Full interactive connection control layered over the protocol-neutral base seam.
//!
//! ACP is bidirectional JSON-RPC: an Agent can issue permission requests to the
//! Client while a prompt is active. Stable ACP v1 also allows string, numeric or
//! null request ids. This layer preserves those ids exactly, coordinates prompt
//! cancellation with pending permission responses, and exposes session/transport
//! closure without changing AgentSession identity.

use std::collections::BTreeMap;

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent_connection::{
    AcpV1ConnectionAdapter, AgentConnectionAdapter, CancelRequest, ClassicProcessConnectionAdapter,
    ConnectionCommand, ConnectionDegradation, ConnectionDescriptor, ConnectionSignal,
    ConnectionSignalKind, NativePermissionChoice, NativePermissionRequest, PromptRequest,
    SessionOpenRequest,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PermissionDecision {
    Selected { option_id: String },
    Cancelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpStableSessionCapabilities {
    pub close: bool,
    pub list: bool,
}

/// The connection seam used by an interactive UI/controller. It is deliberately
/// protocol-neutral: ACP can yield several wire commands for one semantic cancel
/// (permission cancellation responses + session/cancel), while a classic process
/// normally yields one interrupt command.
pub trait InteractiveAgentConnectionAdapter: AgentConnectionAdapter {
    fn respond_permission(
        &mut self,
        request: &NativePermissionRequest,
        decision: PermissionDecision,
    ) -> Result<ConnectionCommand>;

    fn coordinated_cancel(&mut self, request: CancelRequest) -> Result<Vec<ConnectionCommand>>;

    /// Release one protocol-native session where the target has such a concept.
    /// This is not deletion of canonical AgentSession identity.
    fn close_native_session(&mut self, native_session_id: &str) -> Result<ConnectionCommand>;

    /// Close the connection/agent-process transport. Session/Agent identity is not
    /// inferred from or deleted by this operation.
    fn disconnect(&mut self) -> Result<ConnectionCommand>;

    /// Re-establish transport only when the adapter can prove how. Unsupported
    /// reconnect is an explicit degradation, never a silent fresh session.
    fn reconnect(&mut self) -> Result<ConnectionCommand>;
}

#[derive(Debug, Clone)]
struct PendingAcpControl {
    operation: String,
    native_session_id: Option<String>,
}

/// Stable-ACP wrapper over the base v1 encoder/decoder. The wrapper exists to
/// preserve bidirectional JSON-RPC ids exactly and track the stable session
/// lifecycle capabilities that are orthogonal to create/load/resume.
#[derive(Debug, Clone)]
pub struct AcpStableConnectionAdapter {
    inner: AcpV1ConnectionAdapter,
    next_sequence: u64,
    next_control_id: u64,
    request_ids: BTreeMap<String, Value>,
    pending_permission_session: BTreeMap<String, String>,
    pending_control: BTreeMap<String, PendingAcpControl>,
    session_capabilities: AcpStableSessionCapabilities,
}

impl AcpStableConnectionAdapter {
    pub fn new(connection_ref: ResourceRef, provenance: Vec<String>) -> Self {
        Self {
            inner: AcpV1ConnectionAdapter::new(connection_ref, provenance),
            next_sequence: 1,
            next_control_id: 1,
            request_ids: BTreeMap::new(),
            pending_permission_session: BTreeMap::new(),
            pending_control: BTreeMap::new(),
            session_capabilities: AcpStableSessionCapabilities::default(),
        }
    }

    pub fn negotiated_session_capabilities(&self) -> &AcpStableSessionCapabilities {
        &self.session_capabilities
    }

    fn resequence(&mut self, mut signal: ConnectionSignal) -> ConnectionSignal {
        signal.sequence = self.next_sequence;
        self.next_sequence += 1;
        signal
    }

    fn signal(
        &mut self,
        native_session_id: Option<String>,
        kind: ConnectionSignalKind,
    ) -> ConnectionSignal {
        let descriptor = self.inner.descriptor();
        let signal = ConnectionSignal {
            sequence: self.next_sequence,
            native_session_id,
            kind,
            provenance: descriptor.provenance,
        };
        self.next_sequence += 1;
        signal
    }

    fn capture_stable_session_capabilities(&mut self, message: &Value) {
        let Some(result) = message.get("result") else {
            return;
        };
        if result.get("protocolVersion").is_none() {
            return;
        }
        let session = result
            .get("agentCapabilities")
            .and_then(|capabilities| capabilities.get("sessionCapabilities"));
        self.session_capabilities.close = session
            .and_then(|value| value.get("close"))
            .is_some_and(capability_present);
        self.session_capabilities.list = session
            .and_then(|value| value.get("list"))
            .is_some_and(capability_present);
    }

    fn ingest_permission_request(&mut self, message: &Value) -> Result<Vec<ConnectionSignal>> {
        let id = message.get("id").cloned().ok_or_else(|| {
            AikitError::new(
                "connection.acp.permission_request_missing_id",
                "ACP session/request_permission must be a JSON-RPC request with an id",
            )
        })?;
        let token = request_id_token(&id)?;
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let native_session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AikitError::new(
                    "connection.acp.invalid_permission_request",
                    "ACP permission request has no sessionId",
                )
            })?
            .to_string();
        let tool_call_id = params
            .get("toolCall")
            .and_then(|tool| tool.get("toolCallId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let choices = params
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|option| {
                Some(NativePermissionChoice {
                    option_id: option.get("optionId")?.as_str()?.to_string(),
                    label: option.get("name")?.as_str()?.to_string(),
                })
            })
            .collect();

        self.request_ids.insert(token.clone(), id);
        self.pending_permission_session
            .insert(token.clone(), native_session_id.clone());

        let descriptor = self.inner.descriptor();
        let signal = ConnectionSignal {
            sequence: self.next_sequence,
            native_session_id: Some(native_session_id.clone()),
            kind: ConnectionSignalKind::PermissionRequested {
                request: NativePermissionRequest {
                    native_request_id: token,
                    native_session_id,
                    tool_call_id,
                    choices,
                    provenance: descriptor.provenance.clone(),
                },
            },
            provenance: descriptor.provenance,
        };
        self.next_sequence += 1;
        Ok(vec![signal])
    }

    fn ingest_control_response(
        &mut self,
        message: &Value,
    ) -> Result<Option<Vec<ConnectionSignal>>> {
        let Some(id) = message.get("id") else {
            return Ok(None);
        };
        let token = request_id_token(id)?;
        let Some(pending) = self.pending_control.remove(&token) else {
            return Ok(None);
        };

        let native_session_id = pending.native_session_id;
        let kind = if let Some(error) = message.get("error") {
            ConnectionSignalKind::Degraded {
                degradation: ConnectionDegradation {
                    reason: format!("ACP {} failed: {error}", pending.operation),
                    unavailable: vec![pending.operation],
                },
            }
        } else {
            ConnectionSignalKind::Status {
                message: format!("ACP {} acknowledged", pending.operation),
            }
        };
        Ok(Some(vec![self.signal(native_session_id, kind)]))
    }

    fn next_control_request(
        &mut self,
        operation: &str,
        native_session_id: Option<String>,
    ) -> Result<Value> {
        let id = Value::String(format!("aikit-control-{}", self.next_control_id));
        self.next_control_id += 1;
        let token = request_id_token(&id)?;
        self.pending_control.insert(
            token,
            PendingAcpControl {
                operation: operation.to_string(),
                native_session_id,
            },
        );
        Ok(id)
    }
}

impl AgentConnectionAdapter for AcpStableConnectionAdapter {
    fn descriptor(&self) -> ConnectionDescriptor {
        self.inner.descriptor()
    }

    fn initialize(&mut self) -> Result<ConnectionCommand> {
        self.inner.initialize()
    }

    fn open_session(&mut self, request: SessionOpenRequest) -> Result<ConnectionCommand> {
        self.inner.open_session(request)
    }

    fn prompt(&mut self, request: PromptRequest) -> Result<ConnectionCommand> {
        self.inner.prompt(request)
    }

    fn cancel(&mut self, request: CancelRequest) -> Result<ConnectionCommand> {
        self.inner.cancel(request)
    }

    fn ingest(&mut self, message: Value) -> Result<Vec<ConnectionSignal>> {
        self.capture_stable_session_capabilities(&message);
        if message.get("method").is_none() {
            if let Some(signals) = self.ingest_control_response(&message)? {
                return Ok(signals);
            }
        }
        if message
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(|method| method == "session/request_permission")
        {
            return self.ingest_permission_request(&message);
        }
        let signals = self.inner.ingest(message)?;
        Ok(signals
            .into_iter()
            .map(|signal| self.resequence(signal))
            .collect())
    }
}

impl InteractiveAgentConnectionAdapter for AcpStableConnectionAdapter {
    fn respond_permission(
        &mut self,
        request: &NativePermissionRequest,
        decision: PermissionDecision,
    ) -> Result<ConnectionCommand> {
        if let PermissionDecision::Selected { option_id } = &decision {
            if !request
                .choices
                .iter()
                .any(|choice| &choice.option_id == option_id)
            {
                return Err(AikitError::new(
                    "connection.acp.permission_option_unknown",
                    format!("permission option {option_id} was not offered by the agent"),
                ));
            }
        }

        let id = self
            .request_ids
            .remove(&request.native_request_id)
            .ok_or_else(|| {
                AikitError::new(
                    "connection.acp.unknown_permission_request",
                    format!(
                        "permission request {} is no longer pending",
                        request.native_request_id
                    ),
                )
            })?;
        self.pending_permission_session
            .remove(&request.native_request_id);
        let outcome = match decision {
            PermissionDecision::Selected { option_id } => {
                json!({ "outcome": "selected", "optionId": option_id })
            }
            PermissionDecision::Cancelled => json!({ "outcome": "cancelled" }),
        };
        Ok(ConnectionCommand {
            operation: "session/request_permission:response".into(),
            payload: json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "outcome": outcome }
            }),
        })
    }

    fn coordinated_cancel(&mut self, request: CancelRequest) -> Result<Vec<ConnectionCommand>> {
        let pending = self
            .pending_permission_session
            .iter()
            .filter(|(_, session)| session.as_str() == request.native_session_id.as_str())
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        let mut commands = Vec::new();
        for token in pending {
            let id = self.request_ids.remove(&token).ok_or_else(|| {
                AikitError::new(
                    "connection.acp.unknown_permission_request",
                    format!("pending permission token {token} lost its JSON-RPC id"),
                )
            })?;
            self.pending_permission_session.remove(&token);
            commands.push(ConnectionCommand {
                operation: "session/request_permission:response".into(),
                payload: json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "outcome": { "outcome": "cancelled" } }
                }),
            });
        }
        commands.push(self.inner.cancel(request)?);
        Ok(commands)
    }

    fn close_native_session(&mut self, native_session_id: &str) -> Result<ConnectionCommand> {
        if !self.session_capabilities.close {
            return Err(AikitError::new(
                "connection.session_close_unsupported",
                "ACP target does not advertise sessionCapabilities.close",
            ));
        }
        let id = self.next_control_request("session/close", Some(native_session_id.to_string()))?;
        Ok(ConnectionCommand {
            operation: "session/close".into(),
            payload: json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "session/close",
                "params": { "sessionId": native_session_id }
            }),
        })
    }

    fn disconnect(&mut self) -> Result<ConnectionCommand> {
        Ok(ConnectionCommand {
            operation: "disconnect-transport".into(),
            payload: json!({
                "connectionRef": self.inner.descriptor().connection_ref.to_string(),
                "semantics": "close ACP transport/process; do not rewrite Agent or AgentSession identity"
            }),
        })
    }

    fn reconnect(&mut self) -> Result<ConnectionCommand> {
        if !self.inner.descriptor().capabilities.reconnect {
            return Err(AikitError::new(
                "connection.reconnect_unsupported",
                "ACP transport does not advertise a proven reconnect binding; resume is session lifecycle, not transport identity",
            ));
        }
        Ok(ConnectionCommand {
            operation: "reconnect-transport".into(),
            payload: json!({ "connectionRef": self.inner.descriptor().connection_ref.to_string() }),
        })
    }
}

impl InteractiveAgentConnectionAdapter for ClassicProcessConnectionAdapter {
    fn respond_permission(
        &mut self,
        _request: &NativePermissionRequest,
        _decision: PermissionDecision,
    ) -> Result<ConnectionCommand> {
        Err(AikitError::new(
            "connection.permissions_unsupported",
            "classic process connection exposes no native permission-request protocol",
        ))
    }

    fn coordinated_cancel(&mut self, request: CancelRequest) -> Result<Vec<ConnectionCommand>> {
        Ok(vec![self.cancel(request)?])
    }

    fn close_native_session(&mut self, _native_session_id: &str) -> Result<ConnectionCommand> {
        Err(AikitError::new(
            "connection.session_close_unsupported",
            "classic process connection has no independent protocol-native session close",
        ))
    }

    fn disconnect(&mut self) -> Result<ConnectionCommand> {
        Ok(ConnectionCommand {
            operation: "terminate-process".into(),
            payload: json!({
                "connectionRef": self.descriptor().connection_ref.to_string(),
                "semantics": "terminate process transport only"
            }),
        })
    }

    fn reconnect(&mut self) -> Result<ConnectionCommand> {
        Err(AikitError::new(
            "connection.reconnect_unsupported",
            "classic process fixture has no proven reconnect binding; launch a new connection explicitly",
        ))
    }
}

fn request_id_token(id: &Value) -> Result<String> {
    match id {
        Value::String(value) => Ok(format!("s:{value}")),
        Value::Number(value) if value.as_i64().is_some() => Ok(format!("n:{value}")),
        Value::Null => Ok("null".into()),
        _ => Err(AikitError::new(
            "connection.acp.invalid_request_id",
            "ACP JSON-RPC request id must be a string, integer or null",
        )),
    }
}

fn capability_present(value: &Value) -> bool {
    !value.is_null() && value != &Value::Bool(false)
}
