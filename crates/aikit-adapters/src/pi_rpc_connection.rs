//! Pi 0.84 JSONL RPC through the existing protocol-neutral connection host.
//! One Pi process has one resident session. Attach is observed binding, not a
//! claim of resume or permission parity. Pi owns its native history and model.
use std::collections::{BTreeMap, BTreeSet};

use aikit_core::{AikitError, ResourceRef, Result};
use serde_json::{json, Value};

use crate::agent_connection::*;
use crate::interactive_connection::{
    InteractiveAgentConnectionAdapter, NativeModeControls, NativeModelControls, PermissionDecision,
};

const PI_NO_PERMISSION_MODES: &str =
    "Pi RPC publishes no in-session permission modes (no tool-permission gate to switch)";

#[derive(Debug, Clone)]
enum Pending {
    Initialize,
    Attach(ResourceRef),
    Prompt,
    Control,
    SetModel {
        native_session_id: String,
        selection_id: String,
        provider: String,
        model_id: String,
    },
}

#[derive(Debug, Clone)]
struct PiNativeModel {
    provider: String,
    model_id: String,
    advertised: NativeAdvertisedModel,
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
    models_discovered: bool,
    native_models: BTreeMap<String, PiNativeModel>,
    model_observation: Option<crate::agent_connection::NativeModelObservation>,
    abort_acknowledged: bool,
    /// The person asked for this turn to stop (an `abort` was sent). Pi may
    /// acknowledge it only after the turn has already settled.
    abort_requested: bool,
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
            models_discovered: false,
            native_models: BTreeMap::new(),
            model_observation: None,
            abort_acknowledged: false,
            abort_requested: false,
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

    fn selection_id(provider: &str, model_id: &str) -> String {
        format!("{provider}/{model_id}")
    }

    fn discover_models(&mut self, data: &Value) -> Result<()> {
        let models = data["models"].as_array().ok_or_else(|| {
            error(
                "connection.pi_rpc.invalid_model_catalogue",
                "Pi get_available_models returned no model list",
            )
        })?;
        let mut discovered = BTreeMap::new();
        for model in models {
            let provider = model["provider"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    error(
                        "connection.pi_rpc.invalid_model_catalogue",
                        "Pi advertised a model without a provider",
                    )
                })?;
            let model_id = model["id"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    error(
                        "connection.pi_rpc.invalid_model_catalogue",
                        "Pi advertised a model without an id",
                    )
                })?;
            let selection_id = Self::selection_id(provider, model_id);
            if discovered.contains_key(&selection_id) {
                return Err(error(
                    "connection.pi_rpc.duplicate_model",
                    "Pi advertised a duplicate provider/model identity",
                ));
            }
            let native_name = model["name"]
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(model_id);
            discovered.insert(
                selection_id.clone(),
                PiNativeModel {
                    provider: provider.to_owned(),
                    model_id: model_id.to_owned(),
                    advertised: NativeAdvertisedModel {
                        roster_identity: None,
                        model_id: selection_id,
                        name: format!("{native_name} · {provider}"),
                        description: model["description"].as_str().map(ToOwned::to_owned),
                    },
                },
            );
        }
        self.native_models = discovered;
        self.models_discovered = true;
        Ok(())
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
                return Err(error(
                    "connection.pi_rpc.model_mismatch",
                    "Pi native state does not confirm the selected provider/model; no default or fallback is admitted",
                ));
            }
        }
        // Model observation belongs to the native session even when no durable
        // AIKit policy selected it. Keep Pi's human name; a missing name is an
        // explicit absence, never a provider transport id posing as a title.
        let provider = data["model"]["provider"]
            .as_str()
            .filter(|value| !value.trim().is_empty());
        let model = data["model"]["id"]
            .as_str()
            .filter(|value| !value.trim().is_empty());
        self.model_observation = match (provider, model, self.native_models.is_empty()) {
            (Some(provider), Some(model), false) => {
                let selection_id = Self::selection_id(provider, model);
                Some(NativeModelObservation {
                    native_provider: Some(provider.to_owned()),
                    current_model_id: selection_id,
                    available_models: self.native_models.values().map(|model| model.advertised.clone()).collect(),
                    reasoning_effort: None,
                    standing: "Pi native get_available_models + get_state; configuration, not an inference receipt".into(),
                })
            }
            (_, Some(model), _) => Some(NativeModelObservation {
                native_provider: provider.map(str::to_owned),
                current_model_id: model.into(),
                available_models: vec![NativeAdvertisedModel {
                    roster_identity: provider.map(|provider| {
                        crate::agent_connection::NativeModelRosterIdentity {
                            provider_ref: format!("provider:{provider}"),
                            provider_native_id: model.to_owned(),
                            harness_slug: Some("pi".into()),
                        }
                    }),
                    model_id: model.into(),
                    name: data["model"]["name"]
                        .as_str()
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or("Unnamed model")
                        .into(),
                    description: None,
                }],
                reasoning_effort: None,
                standing: "Pi native get_state; configuration, not an inference receipt".into(),
            }),
            _ => None,
        };
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
        let operation = if self.expected_model.is_some() {
            "get_state"
        } else {
            "get_available_models"
        };
        Ok(self.request(operation, json!({}), Pending::Initialize))
    }

    fn open_session(&mut self, request: SessionOpenRequest) -> Result<ConnectionCommand> {
        if request.mode != SessionOpenMode::Attach {
            return Err(error(
                "connection.pi_rpc.unsupported_open",
                "Pi RPC adapter attaches to its process's observed session; create/load/resume are not claimed",
            ));
        }
        if self.binding.is_some()
            || self
                .pending
                .values()
                .any(|p| matches!(p, Pending::Attach(_)))
        {
            return Err(error(
                "connection.pi_rpc.single_session",
                "A Pi process carries one resident session; use a separate native connection for another encounter",
            ));
        }
        if request.cwd != self.cwd
            || !request.additional_directories.is_empty()
            || !request.mcp_servers.is_empty()
        {
            return Err(error(
                "connection.pi_rpc.unsupported_context",
                "Pi context belongs to its native process launch; this adapter cannot change directories or MCP configuration",
            ));
        }
        if !self.models_discovered && self.observed_session.is_none() {
            return Err(error(
                "connection.pi_rpc.not_initialized",
                "Read Pi native state before attachment",
            ));
        }
        if let (Some(requested), Some(observed)) = (
            request.native_session_id.as_ref(),
            self.observed_session.as_ref(),
        ) {
            if requested != observed {
                return Err(error(
                    "connection.pi_rpc.session_mismatch",
                    "Requested native Pi session does not match the observed process",
                ));
            }
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
        self.abort_requested = false;
        Ok(self.request("prompt", json!({"message": text}), Pending::Prompt))
    }

    fn cancel(&mut self, request: CancelRequest) -> Result<ConnectionCommand> {
        self.require_session(&request.native_session_id)?;
        self.abort_requested = true;
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
                    let status = if message["data"].get("models").is_some() {
                        self.discover_models(&message["data"])?;
                        "Pi native model catalogue observed; model configuration is not an inference receipt"
                    } else {
                        self.observe_state(&message["data"])?;
                        "Pi native session observed; permission and resume faculties are not claimed"
                    };
                    Ok(vec![self.signal(ConnectionSignalKind::Status {
                        message: status.into(),
                    })])
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
                Pending::SetModel {
                    native_session_id,
                    selection_id,
                    provider,
                    model_id,
                } => {
                    if message["command"].as_str() != Some("set_model")
                        || message["data"]["provider"].as_str() != Some(provider.as_str())
                        || message["data"]["id"].as_str() != Some(model_id.as_str())
                    {
                        return Err(error(
                            "connection.pi_rpc.model_configuration_unconfirmed",
                            "Pi set_model response did not confirm the exact requested provider/model",
                        ));
                    }
                    self.require_session(&native_session_id)?;
                    let mut observation = self.model_observation.clone().ok_or_else(|| {
                        error(
                            "connection.pi_rpc.model_selection_unsupported",
                            "Pi resident session has no native model observation",
                        )
                    })?;
                    observation.current_model_id = selection_id;
                    self.model_observation = Some(observation.clone());
                    if let Some(binding) = &mut self.binding {
                        binding.model_observation = Some(observation.clone());
                    }
                    Ok(vec![self.signal(ConnectionSignalKind::ModelConfigured {
                        model_observation: observation,
                    })])
                }
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
                            return Err(error(
                                "connection.pi_rpc.response_model_mismatch",
                                "Assistant result does not name the selected native provider/model; response remains failed, not attributed to the requested Model",
                            ));
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
                    // After the person's abort, the in-flight tool or request
                    // ends with Pi's own "operation was aborted" error: that is
                    // the stop, not a provider failure. Pi may acknowledge the
                    // abort only after this settles, so the request counts.
                    "error"
                        if self.abort_acknowledged
                            || (self.abort_requested
                                && detail
                                    .as_deref()
                                    .is_some_and(|text| text.contains("aborted"))) =>
                    {
                        ConnectionSignalKind::Cancelled
                    }
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
    /// Pi RPC has queue-delivery modes (steering/follow-up) but no permission
    /// modes: it publishes no tool-permission gate to switch. Stated, not
    /// inherited, so a later Pi protocol change has one place to land.
    fn session_mode_controls(&self, _native_session_id: &str) -> NativeModeControls {
        NativeModeControls::unavailable(PI_NO_PERMISSION_MODES)
    }

    fn set_session_mode(
        &mut self,
        _native_session_id: &str,
        _provider_mode_id: &str,
    ) -> Result<ConnectionCommand> {
        Err(error(
            "connection.pi_rpc.mode_selection_unsupported",
            PI_NO_PERMISSION_MODES,
        ))
    }

    fn session_model_controls(&self, native_session_id: &str) -> NativeModelControls {
        if self.expected_model.is_some() {
            return NativeModelControls::unavailable(
                "Pi model is fixed by the admitted launch-time owner configuration",
            );
        }
        if self
            .binding
            .as_ref()
            .is_none_or(|binding| binding.native_session_id != native_session_id)
        {
            return NativeModelControls::unavailable(
                "Pi model configuration requires the observed, bound native session",
            );
        }
        match &self.model_observation {
            Some(observation) if !observation.available_models.is_empty() => NativeModelControls {
                model_selection: true,
                reasoning_effort_selection: false,
                reason: None,
            },
            _ => NativeModelControls::unavailable(
                "Pi did not advertise any configured native models",
            ),
        }
    }

    fn set_session_model(
        &mut self,
        native_session_id: &str,
        provider_model_id: &str,
    ) -> Result<ConnectionCommand> {
        self.require_session(native_session_id)?;
        if self.expected_model.is_some() {
            return Err(error(
                "connection.pi_rpc.model_selection_unsupported",
                "Pi model is fixed by the admitted launch-time owner configuration",
            ));
        }
        let model = self.native_models.get(provider_model_id).ok_or_else(|| {
            error(
                "connection.pi_rpc.model_not_advertised",
                "Requested model was not advertised by Pi get_available_models",
            )
        })?;
        let provider = model.provider.clone();
        let model_id = model.model_id.clone();
        Ok(self.request(
            "set_model",
            json!({"provider": provider, "modelId": model_id}),
            Pending::SetModel {
                native_session_id: native_session_id.to_owned(),
                selection_id: provider_model_id.to_owned(),
                provider,
                model_id,
            },
        ))
    }

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
    fn set_session_reasoning_effort(
        &mut self,
        _native_session_id: &str,
        _provider_reasoning_effort: &str,
    ) -> Result<ConnectionCommand> {
        Err(AikitError::new(
            "connection.reasoning_effort_selection_unsupported",
            "this provider does not advertise a bounded ACP reasoning-effort selector",
        ))
    }

    fn disconnect(&mut self) -> Result<ConnectionCommand> {
        Err(error(
            "connection.pi_rpc.disconnect_unsupported",
            "Use the native host shutdown operation to release the Pi process",
        ))
    }
    fn reconnect(&mut self) -> Result<ConnectionCommand> {
        Err(error(
            "connection.pi_rpc.reconnect_unsupported",
            "Pi session continuity must be established by its owner before another connection is attached",
        ))
    }
}

fn error(code: &'static str, detail: &str) -> AikitError {
    AikitError::new(code, detail)
}
