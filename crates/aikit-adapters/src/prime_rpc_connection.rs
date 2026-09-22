//! Prime Agent 0.9.4 JSONL RPC through AIKit's protocol-neutral encounter host.
//!
//! The wire contract is pinned to PrimeIntellect-ai/prime-agent
//! f771dfcedd684d1afff84ca2c6fa95c7a21efbc2. One launched Prime process
//! carries one native session. AIKit binds that native id to an explicit
//! canonical AgentSession; it never promotes process/model/session identity.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::{AikitError, ResourceRef, Result};
use serde_json::{json, Value};

use crate::agent_connection::*;
use crate::interactive_connection::{InteractiveAgentConnectionAdapter, PermissionDecision};

pub const PRIME_RPC_ADAPTER_VERSION: &str = "aikit.prime-rpc-connection/v1";
pub const PRIME_AGENT_RELEASE: &str = "0.9.4";
pub const PRIME_AGENT_RELEASE_REVISION: &str = "f771dfcedd684d1afff84ca2c6fa95c7a21efbc2";

#[derive(Debug, Clone)]
enum Pending {
    Initialize,
    Attach(ResourceRef),
    Prompt,
    Control,
}

#[derive(Debug, Clone)]
pub struct PrimeRpcConnectionAdapter {
    connection: ResourceRef,
    cwd: String,
    provenance: Vec<String>,
    request_sequence: u64,
    signal_sequence: u64,
    pending: BTreeMap<String, Pending>,
    observed_session: Option<String>,
    binding: Option<NativeSessionBinding>,
    expected_model: Option<(String, String)>,
    model_observation: Option<NativeModelObservation>,
    stop: Option<(String, Option<String>)>,
    abort_acknowledged: bool,
}

impl PrimeRpcConnectionAdapter {
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
            expected_model: None,
            model_observation: None,
            stop: None,
            abort_acknowledged: false,
        }
    }

    pub fn with_selected_model(mut self, provider: &str, model_id: &str) -> Result<Self> {
        if provider.trim().is_empty() || model_id.trim().is_empty() {
            return Err(error(
                "connection.prime_rpc.model_selection",
                "Prime native provider and model id are required",
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
        let id = format!("aikit-prime-{}", self.request_sequence);
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
                "connection.prime_rpc.session_not_bound",
                "Prime operation requires the observed, bound native session",
            ))
        }
    }

    fn observe_state(&mut self, data: &Value, require_idle: bool) -> Result<String> {
        let id = data["sessionId"]
            .as_str()
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                error(
                    "connection.prime_rpc.invalid_state",
                    "Prime returned no native session identity",
                )
            })?;
        if self
            .observed_session
            .as_deref()
            .is_some_and(|old| old != id)
        {
            return Err(error(
                "connection.prime_rpc.session_changed",
                "Prime changed its resident session; canonical continuity is not established",
            ));
        }
        if require_idle
            && (data["isStreaming"] != false
                || data["isCompacting"] != false
                || data["sessionActions"]["queuedCount"]
                    .as_u64()
                    .unwrap_or_default()
                    != 0)
        {
            return Err(error(
                "connection.prime_rpc.session_busy",
                "Attach requires an idle Prime session with no queued session actions",
            ));
        }
        if let Some((provider, model)) = &self.expected_model {
            if data["model"]["provider"].as_str() != Some(provider.as_str())
                || data["model"]["id"].as_str() != Some(model.as_str())
            {
                return Err(error(
                    "connection.prime_rpc.model_mismatch",
                    "Prime native state does not confirm the selected provider/model; no default or fallback is admitted",
                ));
            }
            self.model_observation = Some(NativeModelObservation {
                current_model_id: model.clone(),
                available_models: vec![NativeAdvertisedModel {
                    model_id: model.clone(),
                    name: data["model"]["name"].as_str().unwrap_or(model).into(),
                    description: Some(format!("Prime Agent {PRIME_AGENT_RELEASE} launch-selected model")),
                }],
                reasoning_effort: None,
                standing: format!(
                    "Prime native get_state at release {PRIME_AGENT_RELEASE}; provider={provider}; configuration, not inference proof"
                ),
            });
        }
        self.observed_session = Some(id.into());
        Ok(id.into())
    }
}

impl AgentConnectionAdapter for PrimeRpcConnectionAdapter {
    fn descriptor(&self) -> ConnectionDescriptor {
        ConnectionDescriptor {
            adapter_ref: ResourceRef::parse("connection-adapter/prime-rpc/0.9.4").unwrap(),
            connection_ref: self.connection.clone(),
            protocol: ConnectionProtocol {
                family: ConnectionProtocolFamily::PrimeRpc,
                version: PRIME_AGENT_RELEASE.into(),
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
            return Err(error(
                "connection.prime_rpc.unsupported_open",
                "Prime RPC binds the process's observed native session; create/load/resume are not claimed",
            ));
        }
        if self.binding.is_some()
            || self
                .pending
                .values()
                .any(|pending| matches!(pending, Pending::Attach(_)))
        {
            return Err(error(
                "connection.prime_rpc.single_session",
                "One Prime process carries one native session; use another connection for another canonical encounter",
            ));
        }
        if request.cwd != self.cwd
            || !request.additional_directories.is_empty()
            || !request.mcp_servers.is_empty()
        {
            return Err(error(
                "connection.prime_rpc.unsupported_context",
                "Prime World/Skill context belongs to its admitted process launch; this adapter does not rewrite cwd, additional directories or MCP servers",
            ));
        }
        if self.observed_session.is_none() {
            return Err(error(
                "connection.prime_rpc.not_initialized",
                "Read Prime native state before attachment",
            ));
        }
        if request
            .native_session_id
            .as_ref()
            .is_some_and(|id| Some(id) != self.observed_session.as_ref())
        {
            return Err(error(
                "connection.prime_rpc.session_mismatch",
                "Requested Prime native session differs from the observed process",
            ));
        }
        let canonical = request.agent_session.ok_or_else(|| {
            error(
                "connection.prime_rpc.identity_required",
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
                    "connection.prime_rpc.unsupported_prompt",
                    "Prime RPC encounter accepts a text prompt",
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
                    "connection.prime_rpc.request_failed",
                    message["error"]
                        .as_str()
                        .unwrap_or("Prime refused the native RPC request"),
                ));
            }
            return match pending {
                Pending::Initialize => {
                    self.observe_state(&message["data"], true)?;
                    Ok(vec![self.signal(ConnectionSignalKind::Status {
                        message: format!(
                            "Prime Agent {PRIME_AGENT_RELEASE} native session observed from pinned revision {PRIME_AGENT_RELEASE_REVISION}"
                        ),
                    })])
                }
                Pending::Attach(canonical) => {
                    let id = self.observe_state(&message["data"], true)?;
                    let mut binding = NativeSessionBinding::unbound(id, SessionOpenMode::Attach)
                        .bind_agent_session(canonical);
                    binding.provenance = self.provenance.clone();
                    binding.model_observation = self.model_observation.clone();
                    self.binding = Some(binding.clone());
                    Ok(vec![self.signal(ConnectionSignalKind::SessionOpened {
                        binding,
                    })])
                }
                Pending::Prompt => Ok(Vec::new()),
                Pending::Control => {
                    if message["command"] == "abort" {
                        self.abort_acknowledged = true;
                    }
                    Ok(vec![self.signal(ConnectionSignalKind::Status {
                        message: format!("Prime control response: {message}"),
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
                            "connection.prime_rpc.invalid_delta",
                            "Prime text delta is not text",
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
                                "connection.prime_rpc.response_model_mismatch",
                                "Prime assistant result does not name the selected provider/model; it is not attributed to the requested Model",
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
            Some("agent_end") => {
                let (reason, detail) = self.stop.take().unwrap_or(("unknown".into(), None));
                Some(match reason.as_str() {
                    "aborted" => ConnectionSignalKind::Cancelled,
                    "unknown" if self.abort_acknowledged => ConnectionSignalKind::Cancelled,
                    "unknown" => ConnectionSignalKind::Failed {
                        reason: "Prime ended without a terminal assistant message".into(),
                    },
                    "error" => ConnectionSignalKind::Failed {
                        reason: detail.unwrap_or_else(|| "Prime reported an assistant error".into()),
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
                    reason:
                        "Prime requested extension UI that the mode-default connection does not expose"
                            .into(),
                    unavailable: vec!["extension-ui".into()],
                },
            }),
            Some(
                "agent_start"
                | "turn_start"
                | "turn_end"
                | "message_start"
                | "auto_retry_start"
                | "auto_retry_end"
                | "compaction_start"
                | "compaction_end"
                | "extension_error",
            ) => Some(ConnectionSignalKind::Status {
                message: message.to_string(),
            }),
            _ => None,
        };
        Ok(kind.map(|kind| self.signal(kind)).into_iter().collect())
    }
}

impl InteractiveAgentConnectionAdapter for PrimeRpcConnectionAdapter {
    fn set_session_model(
        &mut self,
        _native_session_id: &str,
        _provider_model_id: &str,
    ) -> Result<ConnectionCommand> {
        Err(error(
            "connection.prime_rpc.model_selection_launch_owned",
            "Prime mode-default model selection is delivered by AIKit through its declared launch argv and then verified by get_state; in-session replacement is not the durable policy route",
        ))
    }

    fn respond_permission(
        &mut self,
        _: &NativePermissionRequest,
        _: PermissionDecision,
    ) -> Result<ConnectionCommand> {
        Err(error(
            "connection.prime_rpc.permission_unsupported",
            "Prime RPC extension UI is not an admitted Actuation authority or provider permission route",
        ))
    }

    fn coordinated_cancel(&mut self, request: CancelRequest) -> Result<Vec<ConnectionCommand>> {
        self.require_session(&request.native_session_id)?;
        Ok(vec![self.cancel(request)?])
    }

    fn close_native_session(&mut self, _: &str) -> Result<ConnectionCommand> {
        Err(error(
            "connection.prime_rpc.close_unsupported",
            "Closing a view does not close the Prime session; Encounter host shutdown owns the process",
        ))
    }

    fn set_session_reasoning_effort(
        &mut self,
        _native_session_id: &str,
        _provider_reasoning_effort: &str,
    ) -> Result<ConnectionCommand> {
        Err(AikitError::new(
            "connection.reasoning_effort_selection_unsupported",
            "Prime reasoning level is launch/session-owner configuration; no generic durable selector is implied",
        ))
    }

    fn disconnect(&mut self) -> Result<ConnectionCommand> {
        Err(error(
            "connection.prime_rpc.disconnect_unsupported",
            "Use native Encounter host shutdown to release the Prime process",
        ))
    }

    fn reconnect(&mut self) -> Result<ConnectionCommand> {
        Err(error(
            "connection.prime_rpc.reconnect_unsupported",
            "Prime process-local continuity must be re-resolved from retained source/session evidence; this adapter does not fabricate load semantics",
        ))
    }
}

fn error(code: &'static str, detail: &str) -> AikitError {
    AikitError::new(code, detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(streaming: bool) -> Value {
        json!({
            "sessionId":"prime-native-1",
            "isStreaming":streaming,
            "isCompacting":false,
            "sessionActions":{"queuedCount":0},
            "model":{"provider":"zai","id":"glm-5.3-flash","name":"GLM-5.3-Flash"}
        })
    }

    #[test]
    fn attach_requires_idle_observed_prime_and_keeps_canonical_identity() {
        let mut adapter = PrimeRpcConnectionAdapter::new(
            ResourceRef::parse("connection/prime/test").unwrap(),
            "/work".into(),
            vec!["prime-source-lock".into()],
        )
        .with_selected_model("zai", "glm-5.3-flash")
        .unwrap();
        let init = adapter.initialize().unwrap();
        let init_id = init.payload["id"].as_str().unwrap().to_owned();
        adapter
            .ingest(json!({"type":"response","id":init_id,"command":"get_state","success":true,"data":state(false)}))
            .unwrap();
        let open = adapter
            .open_session(SessionOpenRequest {
                mode: SessionOpenMode::Attach,
                native_session_id: None,
                cwd: "/work".into(),
                additional_directories: vec![],
                mcp_servers: vec![],
                agent_session: Some(ResourceRef::parse("agent-session/prime-test").unwrap()),
            })
            .unwrap();
        let id = open.payload["id"].as_str().unwrap().to_owned();
        let signals = adapter
            .ingest(json!({"type":"response","id":id,"command":"get_state","success":true,"data":state(false)}))
            .unwrap();
        let ConnectionSignalKind::SessionOpened { binding } = &signals[0].kind else {
            panic!("expected session opened");
        };
        assert_eq!(binding.native_session_id, "prime-native-1");
        assert_eq!(
            binding.agent_session.as_ref().unwrap().as_str(),
            "agent-session/prime-test"
        );
        assert_eq!(
            adapter.descriptor().protocol.family,
            ConnectionProtocolFamily::PrimeRpc
        );
    }

    #[test]
    fn a_changed_or_busy_prime_state_is_refused() {
        let mut adapter = PrimeRpcConnectionAdapter::new(
            ResourceRef::parse("connection/prime/test").unwrap(),
            "/work".into(),
            vec![],
        );
        let init = adapter.initialize().unwrap();
        let id = init.payload["id"].as_str().unwrap().to_owned();
        let error = adapter
            .ingest(json!({"type":"response","id":id,"command":"get_state","success":true,"data":state(true)}))
            .unwrap_err();
        assert_eq!(error.code(), "connection.prime_rpc.session_busy");
    }

    #[test]
    fn prompt_stream_and_abort_end_on_the_native_agent_end() {
        let mut adapter = PrimeRpcConnectionAdapter::new(
            ResourceRef::parse("connection/prime/test").unwrap(),
            "/work".into(),
            vec![],
        );
        adapter.observed_session = Some("prime-native-1".into());
        adapter.binding = Some(
            NativeSessionBinding::unbound("prime-native-1", SessionOpenMode::Attach)
                .bind_agent_session(ResourceRef::parse("agent-session/prime-test").unwrap()),
        );
        let prompt = adapter
            .prompt(PromptRequest {
                native_session_id: "prime-native-1".into(),
                prompt: json!("hello"),
            })
            .unwrap();
        let prompt_id = prompt.payload["id"].as_str().unwrap().to_owned();
        adapter
            .ingest(json!({"type":"response","id":prompt_id,"command":"prompt","success":true}))
            .unwrap();
        let signals = adapter
            .ingest(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"hi"}}))
            .unwrap();
        assert!(matches!(
            signals[0].kind,
            ConnectionSignalKind::AgentMessageChunk { .. }
        ));
        adapter
            .ingest(json!({"type":"message_end","message":{"role":"assistant","stopReason":"aborted"}}))
            .unwrap();
        let ended = adapter.ingest(json!({"type":"agent_end"})).unwrap();
        assert!(matches!(ended[0].kind, ConnectionSignalKind::Cancelled));
    }
}
