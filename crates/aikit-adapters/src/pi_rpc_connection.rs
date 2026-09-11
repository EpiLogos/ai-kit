//! Pi 0.84 JSONL RPC through the existing protocol-neutral connection host.
//! One Pi process has one resident session. Attach is observed binding, not a
//! claim of resume or permission parity. Pi owns its native history and model.
use std::collections::{BTreeMap, BTreeSet};

use aikit_core::{AikitError, ResourceRef, Result};
use serde_json::{json, Value};

use crate::agent_connection::*;
use crate::interactive_connection::{InteractiveAgentConnectionAdapter, PermissionDecision};

#[derive(Debug, Clone)]
enum Pending {
    Initialize,
    Attach(ResourceRef),
    Prompt,
    Control,
}

#[derive(Debug, Clone)]
pub struct PiRpcConnectionAdapter {
    connection: ResourceRef,
    cwd: String,
    provenance: Vec<String>,
    request_sequence: u64,
    signal_sequence: u64,
    pending: BTreeMap<String, Pending>,
    observed_session: Option<String>,
    binding: Option<NativeSessionBinding>,
    stop: Option<(String, Option<String>)>,
    expected_model: Option<(String, String)>,
    model_observation: Option<crate::agent_connection::NativeModelObservation>,
    abort_acknowledged: bool,
}

impl PiRpcConnectionAdapter {
    pub fn new(connection: ResourceRef, cwd: String, provenance: Vec<String>) -> Self {
        Self {
            connection,
            cwd,
            provenance,
            request_sequence: 0,
            signal_sequence: 0,
            pending: BTreeMap::new(),
            observed_session: None,
            binding: None,
            stop: None,
            expected_model: None,
            model_observation: None,
            abort_acknowledged: false,
        }
    }

    pub fn with_selected_model(mut self, provider: &str, model_id: &str) -> Result<Self> {
        if provider.trim().is_empty() || model_id.trim().is_empty() {
            return Err(error(
                "connection.pi_rpc.model_selection",
                "Native provider and model id are required",
            ));
        }
        self.expected_model = Some((provider.into(), model_id.into()));
        Ok(self)
    }

    fn request(
        &mut self,
        operation: &str,
        mut payload: Value,
        pending: Pending,
    ) -> ConnectionCommand {
        self.request_sequence += 1;
        let id = format!("aikit-pi-{}", self.request_sequence);
        payload["id"] = json!(id);
        payload["type"] = json!(operation);
        self.pending.insert(id, pending);
        ConnectionCommand {
            operation: operation.into(),
            payload,
        }
    }

    fn signal(&mut self, kind: ConnectionSignalKind) -> ConnectionSignal {
        self.signal_sequence += 1;
        ConnectionSignal {
            sequence: self.signal_sequence,
            native_session_id: self.observed_session.clone(),
            kind,
            provenance: self.provenance.clone(),
        }
    }

    fn require_session(&self, native: &str) -> Result<()> {
        if self
            .binding
            .as_ref()
            .is_some_and(|binding| binding.native_session_id == native)
        {
            Ok(())
        } else {
            Err(error(
                "connection.pi_rpc.session_not_bound",
                "Pi operation requires the observed, bound native session",
            ))
        }
    }

    fn observe_state(&mut self, data: &Value) -> Result<String> {
        let id = data["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                error(
                    "connection.pi_rpc.invalid_state",
                    "Pi returned no native session identity",
                )
            })?;
        if self
            .observed_session
            .as_deref()
            .is_some_and(|old| old != id)
        {
            return Err(error(
                "connection.pi_rpc.session_changed",
                "Pi changed its resident session; canonical continuity is not established",
            ));
        }
        if data["isStreaming"] != false
            || data["isCompacting"] != false
            || data["pendingMessageCount"] != 0
        {
            return Err(error(
                "connection.pi_rpc.session_busy",
                "Attach requires an idle Pi session with no pending messages",
            ));
        }
        if let Some((provider, model)) = &self.expected_model {
            if data["model"]["provider"].as_str() != Some(provider.as_str())
                || data["model"]["id"].as_str() != Some(model.as_str())
            {
                return Err(error("connection.pi_rpc.model_mismatch", "Pi native state does not confirm the selected provider/model; no default or fallback is admitted"));
            }
            self.model_observation = Some(NativeModelObservation {
                current_model_id: model.clone(),
                available_models: vec![NativeAdvertisedModel {
                    model_id: model.clone(),
                    name: data["model"]["name"].as_str().unwrap_or(model).into(),
                    description: None,
                }],
                standing: format!("Pi native get_state; provider={provider}; configuration, not an inference receipt"),
            });
        }
        self.observed_session = Some(id.into());
        Ok(id.into())
    }
}

impl AgentConnectionAdapter for PiRpcConnectionAdapter {
    fn descriptor(&self) -> ConnectionDescriptor {
        ConnectionDescriptor {
            adapter_ref: ResourceRef::parse("connection-adapter/pi-rpc/0.84").unwrap(),
            connection_ref: self.connection.clone(),
            protocol: ConnectionProtocol {
                family: ConnectionProtocolFamily::PiRpc,
                version: "0.84".into(),
            },
            capabilities: ConnectionCapabilities {
                session_open: BTreeSet::from([SessionOpenMode::Attach]),
                ordered_streaming: true,
                cancellation: true,
                ..Default::default()
            },
            provenance: self.provenance.clone(),
        }
    }

    fn initialize(&mut self) -> Result<ConnectionCommand> {
        Ok(self.request("get_state", json!({}), Pending::Initialize))
    }

    fn open_session(&mut self, request: SessionOpenRequest) -> Result<ConnectionCommand> {
        if request.mode != SessionOpenMode::Attach {
            return Err(error("connection.pi_rpc.unsupported_open", "Pi RPC adapter attaches to its process's observed session; create/load/resume are not claimed"));
        }
        if self.binding.is_some()
            || self
                .pending
                .values()
                .any(|p| matches!(p, Pending::Attach(_)))
        {
            return Err(error("connection.pi_rpc.single_session", "A Pi process carries one resident session; use a separate native connection for another encounter"));
        }
        if request.cwd != self.cwd
            || !request.additional_directories.is_empty()
            || !request.mcp_servers.is_empty()
        {
            return Err(error("connection.pi_rpc.unsupported_context", "Pi context belongs to its native process launch; this adapter cannot change directories or MCP configuration"));
        }
        if self.observed_session.is_none() {
            return Err(error(
                "connection.pi_rpc.not_initialized",
                "Read Pi native state before attachment",
            ));
        }
        if request
            .native_session_id
            .as_ref()
            .is_some_and(|id| Some(id) != self.observed_session.as_ref())
        {
            return Err(error(
                "connection.pi_rpc.session_mismatch",
                "Requested native Pi session does not match the observed process",
            ));
        }
        let canonical = request.agent_session.ok_or_else(|| {
            error(
                "connection.pi_rpc.identity_required",
                "An explicit canonical AgentSession is required",
            )
        })?;
        Ok(self.request("get_state", json!({}), Pending::Attach(canonical)))
    }

    fn prompt(&mut self, request: PromptRequest) -> Result<ConnectionCommand> {
        self.require_session(&request.native_session_id)?;
        let text = request
            .prompt
            .as_str()
            .or_else(|| request.prompt.get("text").and_then(Value::as_str))
            .ok_or_else(|| {
                error(
                    "connection.pi_rpc.unsupported_prompt",
                    "This Pi connection accepts a text prompt",
                )
            })?;
        self.stop = None;
        self.abort_acknowledged = false;
        Ok(self.request("prompt", json!({"message": text}), Pending::Prompt))
    }

    fn cancel(&mut self, request: CancelRequest) -> Result<ConnectionCommand> {
        self.require_session(&request.native_session_id)?;
        Ok(self.request("abort", json!({}), Pending::Control))
    }

    fn ingest(&mut self, message: Value) -> Result<Vec<ConnectionSignal>> {
        if message["type"] == "response" {
            let pending = message["id"]
                .as_str()
                .and_then(|id| self.pending.remove(id));
            let Some(pending) = pending else {
                return Ok(Vec::new());
            };
            if message["success"] != true {
                return Err(error(
                    "connection.pi_rpc.request_failed",
                    message["error"]
                        .as_str()
                        .unwrap_or("Pi refused the native request"),
                ));
            }
            return match pending {
                Pending::Initialize => {
                    self.observe_state(&message["data"])?;
                    Ok(vec![self.signal(ConnectionSignalKind::Status { message: "Pi native session observed; permission and resume faculties are not claimed".into() })])
                }
                Pending::Attach(canonical) => {
                    let id = self.observe_state(&message["data"])?;
                    let mut binding = NativeSessionBinding::unbound(id, SessionOpenMode::Attach)
                        .bind_agent_session(canonical);
                    binding.provenance = self.provenance.clone();
                    binding.model_observation = self.model_observation.clone();
                    self.binding = Some(binding.clone());
                    Ok(vec![
                        self.signal(ConnectionSignalKind::SessionOpened { binding })
                    ])
                }
                Pending::Prompt => Ok(Vec::new()), // Acceptance is not completion.
                Pending::Control => {
                    if message["command"] == "abort" {
                        self.abort_acknowledged = true;
                    }
                    Ok(vec![self.signal(ConnectionSignalKind::Status {
                        message: format!("Pi control response: {}", message),
                    })])
                }
            };
        }
        let kind = match message["type"].as_str() {
            Some("message_update") if message["assistantMessageEvent"]["type"] == "text_delta" => {
                let text = message["assistantMessageEvent"]["delta"]
                    .as_str()
                    .ok_or_else(|| {
                        error(
                            "connection.pi_rpc.invalid_delta",
                            "Pi text delta is not text",
                        )
                    })?;
                Some(ConnectionSignalKind::AgentMessageChunk { text: text.into() })
            }
            Some("message_end") => {
                let result = &message["message"];
                if result["role"] == "assistant" {
                    if let Some((provider, model)) = &self.expected_model {
                        if result["provider"].as_str() != Some(provider.as_str())
                            || result["model"].as_str() != Some(model.as_str())
                        {
                            return Err(error("connection.pi_rpc.response_model_mismatch", "Assistant result does not name the selected native provider/model; response remains failed, not attributed to the requested Model"));
                        }
                    }
                    self.stop = Some((
                        result["stopReason"].as_str().unwrap_or("unknown").into(),
                        result["errorMessage"].as_str().map(str::to_owned),
                    ));
                }
                Some(ConnectionSignalKind::Status {
                    message: message.to_string(),
                })
            }
            Some("agent_settled") => {
                let (reason, detail) = self.stop.take().unwrap_or(("unknown".into(), None));
                Some(match reason.as_str() {
                    "aborted" => ConnectionSignalKind::Cancelled,
                    "unknown" if self.abort_acknowledged => ConnectionSignalKind::Cancelled,
                    "unknown" => ConnectionSignalKind::Failed {
                        reason: "Pi settled without a terminal assistant result".into(),
                    },
                    "error" => ConnectionSignalKind::Failed {
                        reason: detail.unwrap_or_else(|| "Pi reported an assistant error".into()),
                    },
                    _ => ConnectionSignalKind::Completed {
                        stop_reason: reason,
                    },
                })
            }
            Some("tool_execution_start") | Some("tool_execution_update") => {
                Some(ConnectionSignalKind::ToolCall {
                    payload: message.clone(),
                })
            }
            Some("tool_execution_end") => Some(ConnectionSignalKind::ToolResult {
                payload: message.clone(),
            }),
            Some("extension_ui_request") => Some(ConnectionSignalKind::Degraded {
                degradation: ConnectionDegradation {
                    reason: "Pi requested extension UI that this connection does not implement"
                        .into(),
                    unavailable: vec!["extension-ui".into()],
                },
            }),
            // Retain lifecycle evidence without treating a low-level agent_end,
            // intermediate turn, automatic retry or compaction as settlement.
            Some(
                "agent_end" | "agent_start" | "turn_start" | "turn_end" | "auto_retry_start"
                | "auto_retry_end" | "compaction_start" | "compaction_end" | "extension_error",
            ) => Some(ConnectionSignalKind::Status {
                message: message.to_string(),
            }),
            _ => None,
        };
        Ok(kind.map(|kind| self.signal(kind)).into_iter().collect())
    }
}

impl InteractiveAgentConnectionAdapter for PiRpcConnectionAdapter {
    fn respond_permission(
        &mut self,
        _: &NativePermissionRequest,
        _: PermissionDecision,
    ) -> Result<ConnectionCommand> {
        Err(error(
            "connection.pi_rpc.permission_unsupported",
            "Pi extension UI is not an admitted tool-permission authority",
        ))
    }
    fn coordinated_cancel(&mut self, request: CancelRequest) -> Result<Vec<ConnectionCommand>> {
        self.require_session(&request.native_session_id)?;
        let clear = self.request("clear_queue", json!({}), Pending::Control);
        Ok(vec![clear, self.cancel(request)?])
    }
    fn close_native_session(&mut self, _: &str) -> Result<ConnectionCommand> {
        Err(error(
            "connection.pi_rpc.close_unsupported",
            "Closing a view does not close the Pi session; the host owns process shutdown",
        ))
    }
    fn disconnect(&mut self) -> Result<ConnectionCommand> {
        Err(error(
            "connection.pi_rpc.disconnect_unsupported",
            "Use the native host shutdown operation to release the Pi process",
        ))
    }
    fn reconnect(&mut self) -> Result<ConnectionCommand> {
        Err(error("connection.pi_rpc.reconnect_unsupported", "Pi session continuity must be established by its owner before another connection is attached"))
    }
}

fn error(code: &'static str, detail: &str) -> AikitError {
    AikitError::new(code, detail)
}
