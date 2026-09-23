//! Reusable interactive Agent connection adapter contract.
//!
//! This module deliberately keeps transport/session/process identities separate:
//! a connection is not an AgentSession, a protocol-native session id does not
//! become an AgentSessionRef without an explicit binding, and a permission request
//! remains transport-native until a higher product chooses to project it into its
//! own human-authority model.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const AGENT_CONNECTION_ADAPTER_VERSION: &str = "aikit.connection-adapter/v1";
pub const ACP_STABLE_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionProtocolFamily {
    Acp,
    ClassicProcess,
    PiRpc,
    PrimeRpc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionProtocol {
    pub family: ConnectionProtocolFamily,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionOpenMode {
    Create,
    Load,
    Resume,
    Attach,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ConnectionCapabilities {
    #[serde(default)]
    pub session_open: BTreeSet<SessionOpenMode>,
    pub ordered_streaming: bool,
    pub cancellation: bool,
    pub permission_requests: bool,
    pub reconnect: bool,
    pub additional_directories: bool,
    pub mcp_servers: bool,
}

impl ConnectionCapabilities {
    pub fn supports(&self, mode: SessionOpenMode) -> bool {
        self.session_open.contains(&mode)
    }
}

/// The negotiated ACP `agentCapabilities.sessionCapabilities` lifecycle facts:
/// the post-pin session operations the target advertised at initialize.
/// Absent means false — a target that did not advertise an operation does not
/// have it, and nothing is inferred from a protocol version alone. The
/// `resume` fact also appears as [`SessionOpenMode::Resume`] in
/// [`ConnectionCapabilities::session_open`]; this carries the raw negotiated
/// flags for read models that disclose them as such.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AcpSessionCapabilities {
    /// The target advertised `sessionCapabilities.resume`: `session/resume`
    /// (stabilized 2026-04-23) opens an existing native session with no
    /// history replay.
    #[serde(default)]
    pub resume: bool,
    /// The target advertised `sessionCapabilities.close`.
    #[serde(default)]
    pub close: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionDescriptor {
    pub adapter_ref: ResourceRef,
    pub connection_ref: ResourceRef,
    pub protocol: ConnectionProtocol,
    pub capabilities: ConnectionCapabilities,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// Model configuration disclosed by the native ACP provider. This is a
/// provider report, not AIKit catalog availability or proof of inference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeModelObservation {
    /// Native provider identity when the harness discloses it (RPC launch selection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_provider: Option<String>,
    pub current_model_id: String,
    pub available_models: Vec<NativeAdvertisedModel>,
    /// Provider-advertised execution-budget selector, if exposed by ACP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<NativeConfigSelector>,
    pub standing: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeAdvertisedModel {
    pub model_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
/// A bounded provider-advertised select control. Disclosure grants no write route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeConfigSelector {
    pub config_id: String,
    pub current_value: String,
    pub options: Vec<NativeConfigOption>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeConfigOption {
    pub value: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
/// A native route is identified by its provider model id, never by its label.
/// Preserve the first native display name and wire order while merging repeated
/// advertisements of the same selectable route.
fn unique_advertised_models(models: Vec<NativeAdvertisedModel>) -> Vec<NativeAdvertisedModel> {
    let mut seen = BTreeSet::new();
    models
        .into_iter()
        .filter(|model| seen.insert(model.model_id.clone()))
        .collect()
}

impl NativeModelObservation {
    pub(crate) fn from_acp(value: &Value) -> Result<Self> {
        let current = value
            .get("currentModelId")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                AikitError::new(
                    "connection.acp.invalid_model_observation",
                    "models.currentModelId must be non-empty",
                )
            })?;
        let models: Vec<NativeAdvertisedModel> = serde_json::from_value(
            value
                .get("availableModels")
                .cloned()
                .unwrap_or_else(|| json!([])),
        )
        .map_err(|error| {
            AikitError::new(
                "connection.acp.invalid_model_observation",
                error.to_string(),
            )
        })?;
        if models
            .iter()
            .any(|model| model.model_id.trim().is_empty() || model.name.trim().is_empty())
        {
            return Err(AikitError::new(
                "connection.acp.invalid_model_observation",
                "advertised Model id/name must be non-empty",
            ));
        }
        Ok(Self {
            native_provider: None,
            current_model_id: current.into(),
            available_models: unique_advertised_models(models),
            reasoning_effort: None,
            standing:
                "provider-reported-configuration-not-independent-selection-or-inference-proof"
                    .into(),
        })
    }

    /// Read the single standard ACP `model` config selector. This is provider
    /// disclosure only; its values are neither an AIKit catalogue nor a durable
    /// ModelPolicy selection.
    pub(crate) fn from_acp_model_config_options(value: &Value) -> Result<Option<Self>> {
        let Some(options) = value.as_array() else {
            return Ok(None);
        };
        let Some(model) = options.iter().find(|option| {
            option.get("id").and_then(Value::as_str) == Some("model")
                && option.get("category").and_then(Value::as_str) == Some("model")
                && option.get("type").and_then(Value::as_str) == Some("select")
        }) else {
            return Ok(None);
        };
        let current_model_id = model
            .get("currentValue")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                AikitError::new(
                    "connection.acp.invalid_model_config",
                    "ACP model config has no non-empty currentValue",
                )
            })?
            .to_owned();
        let available_models = model
            .get("options")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AikitError::new(
                    "connection.acp.invalid_model_config",
                    "ACP model config selector has no options",
                )
            })?
            .iter()
            .map(|option| {
                let model_id = option
                    .get("value")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| {
                        AikitError::new(
                            "connection.acp.invalid_model_config",
                            "ACP model option has no non-empty value",
                        )
                    })?;
                let name = option
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                    .ok_or_else(|| {
                        AikitError::new(
                            "connection.acp.invalid_model_config",
                            "ACP model option has no non-empty name",
                        )
                    })?;
                Ok(NativeAdvertisedModel {
                    model_id: model_id.to_owned(),
                    name: name.to_owned(),
                    description: option
                        .get("description")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if !available_models
            .iter()
            .any(|model| model.model_id == current_model_id)
        {
            return Err(AikitError::new(
                "connection.acp.invalid_model_config",
                "ACP model config currentValue is absent from its advertised options",
            ));
        }
        let reasoning_effort = Self::select_config(value, "reasoning_effort")?;
        Ok(Some(Self {
            native_provider: None,
            current_model_id,
            available_models: unique_advertised_models(available_models),
            reasoning_effort,
            standing:
                "provider-reported-configuration-not-independent-selection-or-inference-proof"
                    .into(),
        }))
    }

    /// Only model and its advertised execution-budget selector are admitted.
    /// Arbitrary ACP configuration remains outside this public route.
    pub(crate) fn select_config(
        value: &Value,
        config_id: &str,
    ) -> Result<Option<NativeConfigSelector>> {
        let Some(selectors) = value.as_array() else {
            return Ok(None);
        };
        let Some(selector) = selectors.iter().find(|option| {
            option.get("id").and_then(Value::as_str) == Some(config_id)
                && option.get("type").and_then(Value::as_str) == Some("select")
        }) else {
            return Ok(None);
        };
        let current_value = selector
            .get("currentValue")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                AikitError::new(
                    "connection.acp.invalid_model_config",
                    format!("ACP {config_id} selector has no non-empty currentValue"),
                )
            })?
            .to_owned();
        let options = selector
            .get("options")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AikitError::new(
                    "connection.acp.invalid_model_config",
                    format!("ACP {config_id} selector has no options"),
                )
            })?
            .iter()
            .map(|option| {
                let value = option
                    .get("value")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        AikitError::new(
                            "connection.acp.invalid_model_config",
                            format!("ACP {config_id} option has no non-empty value"),
                        )
                    })?;
                let name = option
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                    .ok_or_else(|| {
                        AikitError::new(
                            "connection.acp.invalid_model_config",
                            format!("ACP {config_id} option has no non-empty name"),
                        )
                    })?;
                Ok(NativeConfigOption {
                    value: value.to_owned(),
                    name: name.to_owned(),
                    description: option
                        .get("description")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if !options.iter().any(|option| option.value == current_value) {
            return Err(AikitError::new(
                "connection.acp.invalid_model_config",
                format!("ACP {config_id} currentValue is absent from advertised options"),
            ));
        }
        Ok(Some(NativeConfigSelector {
            config_id: config_id.to_owned(),
            current_value,
            options,
        }))
    }
}

/// Session permission modes disclosed by the native ACP provider (`modes` on
/// session/new|load|resume, `current_mode_update` afterwards). The provider
/// decides what each mode allows; AIKit carries the advertised ids exactly and
/// never invents, renames or orders them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeModeObservation {
    pub current_mode_id: String,
    pub available_modes: Vec<NativeModeOption>,
    pub standing: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeModeOption {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub const NATIVE_MODE_STANDING: &str =
    "provider-reported-configuration-not-independent-selection-or-inference-proof";

impl NativeModeObservation {
    /// Read an ACP `modes` block. A malformed block yields `None`: the agent
    /// advertised nothing AIKit can offer exactly, so no control is invented.
    pub fn from_acp(value: &Value) -> Option<Self> {
        let current = value
            .get("currentModeId")
            .or_else(|| value.get("modeId"))
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())?;
        let modes = value
            .get("availableModes")
            .and_then(Value::as_array)?
            .iter()
            .map(|mode| {
                let id = mode
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())?;
                let name = mode
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())?;
                Some(NativeModeOption {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    description: mode
                        .get("description")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        if modes.is_empty() || !modes.iter().any(|mode| mode.id == current) {
            return None;
        }
        Some(Self {
            current_mode_id: current.to_owned(),
            available_modes: modes,
            standing: NATIVE_MODE_STANDING.into(),
        })
    }

    /// The same advertised set with another advertised mode current. `None`
    /// when the mode was never advertised.
    pub fn with_current(&self, mode_id: &str) -> Option<Self> {
        self.advertises(mode_id).then(|| Self {
            current_mode_id: mode_id.to_owned(),
            ..self.clone()
        })
    }

    pub fn advertises(&self, mode_id: &str) -> bool {
        self.available_modes.iter().any(|mode| mode.id == mode_id)
    }
}

/// Explicit bridge between a transport-native session and canonical AIKit
/// identity. `agent_session` is intentionally optional: transport session ids are
/// not promoted automatically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSessionBinding {
    pub native_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<ResourceRef>,
    pub opened_as: SessionOpenMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_observation: Option<NativeModelObservation>,
    /// Provider-advertised session permission modes, when the agent has any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_observation: Option<NativeModeObservation>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl NativeSessionBinding {
    pub fn unbound(native_session_id: impl Into<String>, opened_as: SessionOpenMode) -> Self {
        Self {
            native_session_id: native_session_id.into(),
            agent_session: None,
            agent: None,
            opened_as,
            model_observation: None,
            mode_observation: None,
            provenance: Vec::new(),
        }
    }

    pub fn bind_agent_session(mut self, agent_session: ResourceRef) -> Self {
        self.agent_session = Some(agent_session);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOpenRequest {
    pub mode: SessionOpenMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_id: Option<String>,
    pub cwd: String,
    #[serde(default)]
    pub additional_directories: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<Value>,
    /// Optional pre-existing canonical identity supplied by the caller. Adapters
    /// never synthesize this from a native id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<ResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptRequest {
    pub native_session_id: String,
    pub prompt: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelRequest {
    pub native_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionCommand {
    pub operation: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionState {
    Connecting,
    Connected,
    Degraded,
    Reconnecting,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionDegradation {
    pub reason: String,
    #[serde(default)]
    pub unavailable: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePermissionChoice {
    pub option_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// A transport-native permission request. This is deliberately *not* a Factory
/// HumanRequest or any other product-authority object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePermissionRequest {
    pub native_request_id: String,
    pub native_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_call: Value,
    #[serde(default)]
    pub raw: Value,
    #[serde(default)]
    pub choices: Vec<NativePermissionChoice>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ConnectionSignalKind {
    SessionOpened {
        binding: NativeSessionBinding,
    },
    AgentMessageChunk {
        text: String,
    },
    /// Provider-exposed thinking, never reconstructed hidden reasoning.
    AgentThoughtChunk {
        text: String,
        content: Value,
    },
    ToolCall {
        payload: Value,
    },
    ToolResult {
        payload: Value,
    },
    PermissionRequested {
        request: NativePermissionRequest,
    },
    Completed {
        stop_reason: String,
    },
    Cancelled,
    Failed {
        reason: String,
    },
    Status {
        message: String,
    },
    /// A provider confirmed its standard ACP model config selector after an
    /// explicit request. It changes no canonical AgentSession identity.
    ModelConfigured {
        model_observation: NativeModelObservation,
    },
    /// The provider's current session permission mode, either confirmed after
    /// an explicit `session/set_mode` or reported by the agent itself
    /// (`current_mode_update`). It changes no canonical AgentSession identity.
    ModeConfigured {
        mode_observation: NativeModeObservation,
    },
    /// Provider history emitted while an explicit native session load is still
    /// pending. It is preserved as source evidence, separately from new live
    /// encounter events, because ACP v1 has no stable history-item identity.
    HistoryReplay {
        update: Value,
    },
    Degraded {
        degradation: ConnectionDegradation,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionSignal {
    /// Adapter-local monotonically increasing ordering token. It preserves the
    /// observed wire order; it is not a global Event id.
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_id: Option<String>,
    pub kind: ConnectionSignalKind,
    #[serde(default)]
    pub provenance: Vec<String>,
}

/// Pure protocol adapter. Process/socket ownership sits outside this contract;
/// this seam converts semantic requests into native commands and native messages
/// into ordered, provenance-bearing signals.
pub trait AgentConnectionAdapter {
    fn descriptor(&self) -> ConnectionDescriptor;
    fn initialize(&mut self) -> Result<ConnectionCommand>;
    fn open_session(&mut self, request: SessionOpenRequest) -> Result<ConnectionCommand>;
    fn prompt(&mut self, request: PromptRequest) -> Result<ConnectionCommand>;
    fn cancel(&mut self, request: CancelRequest) -> Result<ConnectionCommand>;
    fn ingest(&mut self, message: Value) -> Result<Vec<ConnectionSignal>>;
}

/// ACP v1 JSON-RPC adapter. Optional session lifecycle operations are enabled
/// only from negotiated `agentCapabilities`; unsupported operations fail before a
/// wire message is emitted.
#[derive(Debug, Clone)]
pub struct AcpV1ConnectionAdapter {
    connection_ref: ResourceRef,
    adapter_ref: ResourceRef,
    next_request_id: u64,
    next_sequence: u64,
    capabilities: ConnectionCapabilities,
    session_capabilities: AcpSessionCapabilities,
    pending: BTreeMap<u64, PendingAcpRequest>,
    provenance: Vec<String>,
}

#[derive(Debug, Clone)]
enum PendingAcpRequest {
    Initialize,
    Open {
        mode: SessionOpenMode,
        canonical_agent_session: Option<ResourceRef>,
        requested_native_session_id: Option<String>,
    },
    Prompt {
        native_session_id: String,
    },
}

impl AcpV1ConnectionAdapter {
    pub fn new(connection_ref: ResourceRef, provenance: Vec<String>) -> Self {
        Self {
            connection_ref,
            adapter_ref: ResourceRef::parse("connection-adapter/acp/v1")
                .expect("static ACP adapter ref must be valid"),
            next_request_id: 1,
            next_sequence: 1,
            capabilities: ConnectionCapabilities {
                session_open: BTreeSet::from([SessionOpenMode::Create]),
                ordered_streaming: true,
                cancellation: true,
                permission_requests: true,
                reconnect: false,
                additional_directories: false,
                mcp_servers: false,
            },
            session_capabilities: AcpSessionCapabilities::default(),
            pending: BTreeMap::new(),
            provenance,
        }
    }

    pub fn negotiated_capabilities(&self) -> &ConnectionCapabilities {
        &self.capabilities
    }

    /// The negotiated `agentCapabilities.sessionCapabilities` facts, parsed at
    /// initialize. Absent capabilities are false.
    pub fn negotiated_session_capabilities(&self) -> &AcpSessionCapabilities {
        &self.session_capabilities
    }

    /// Open an existing native session through ACP `session/resume` — the
    /// operation stabilized 2026-04-23 that, unlike `session/load`, performs
    /// no history replay: the response carries no replayed history and none is
    /// fabricated here. Mirrors the `session/new` open shape (session id, cwd,
    /// mcp servers) and returns the same opened-session command; the response
    /// resolves through the same `SessionOpened` binding path. Gated on the
    /// negotiated `sessionCapabilities.resume`; a target that did not
    /// advertise it is refused before any wire message is emitted.
    pub fn session_resume(
        &mut self,
        native_session_id: impl Into<String>,
        cwd: impl Into<String>,
        mcp_servers: Vec<Value>,
    ) -> Result<ConnectionCommand> {
        self.open_session(SessionOpenRequest {
            mode: SessionOpenMode::Resume,
            native_session_id: Some(native_session_id.into()),
            cwd: cwd.into(),
            additional_directories: Vec::new(),
            mcp_servers,
            agent_session: None,
        })
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        pending: PendingAcpRequest,
    ) -> ConnectionCommand {
        let id = self.next_request_id;
        self.next_request_id += 1;
        self.pending.insert(id, pending);
        ConnectionCommand {
            operation: method.to_string(),
            payload: json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }),
        }
    }

    fn signal(
        &mut self,
        native_session_id: Option<String>,
        kind: ConnectionSignalKind,
    ) -> ConnectionSignal {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        ConnectionSignal {
            sequence,
            native_session_id,
            kind,
            provenance: self.provenance.clone(),
        }
    }

    fn apply_initialize_result(&mut self, result: &Value) -> Result<()> {
        let version = result
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                AikitError::new(
                    "connection.acp.invalid_initialize",
                    "ACP initialize response has no integer protocolVersion",
                )
            })?;
        if version != u64::from(ACP_STABLE_PROTOCOL_VERSION) {
            return Err(AikitError::new(
                "connection.acp.unsupported_protocol",
                format!(
                    "ACP negotiated protocol version {version}; AIKit adapter supports stable v{ACP_STABLE_PROTOCOL_VERSION}"
                ),
            ));
        }

        let agent = result
            .get("agentCapabilities")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let sessions = agent
            .get("sessionCapabilities")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let mut open = BTreeSet::from([SessionOpenMode::Create]);
        if capability_present(&agent, "loadSession") || capability_present(&sessions, "load") {
            open.insert(SessionOpenMode::Load);
        }
        let resume = capability_present(&sessions, "resume");
        if resume {
            open.insert(SessionOpenMode::Resume);
        }
        // ACP v1 grew post-pin session lifecycle operations under
        // `sessionCapabilities` while `protocolVersion` stayed 1. Attach has
        // no dedicated method: where the target advertises `resume`, the
        // stabilized `session/resume` operation (2026-04-23) is the one honest
        // route to an existing native session, and open_session refuses
        // naming that capability when it is absent.
        self.session_capabilities = AcpSessionCapabilities {
            resume,
            close: capability_present(&sessions, "close"),
        };
        self.capabilities.session_open = open;
        self.capabilities.additional_directories =
            capability_present(&sessions, "additionalDirectories");
        self.capabilities.mcp_servers = capability_present(&agent, "mcpCapabilities")
            || capability_present(&agent, "mcpServers");
        Ok(())
    }

    fn ingest_response(&mut self, id: u64, message: &Value) -> Result<Vec<ConnectionSignal>> {
        let pending = self.pending.remove(&id).ok_or_else(|| {
            AikitError::new(
                "connection.acp.unknown_response",
                format!("ACP response id {id} has no pending AIKit request"),
            )
        })?;
        if let Some(error) = message.get("error") {
            return Err(AikitError::new(
                "connection.acp.remote_error",
                format!("ACP request {id} failed: {error}"),
            ));
        }
        let result = message.get("result").cloned().unwrap_or_else(|| json!({}));
        match pending {
            PendingAcpRequest::Initialize => {
                self.apply_initialize_result(&result)?;
                Ok(vec![self.signal(
                    None,
                    ConnectionSignalKind::Status {
                        message: format!("ACP v{ACP_STABLE_PROTOCOL_VERSION} negotiated"),
                    },
                )])
            }
            PendingAcpRequest::Open {
                mode,
                canonical_agent_session,
                requested_native_session_id,
            } => {
                // ACP load/resume retains the identity the client supplied. Load
                // may return null after replay; neither operation must mint an id.
                if !result.is_object() && !result.is_null() {
                    return Err(AikitError::new(
                        "connection.acp.invalid_session_response",
                        "Invalid session result",
                    ));
                }
                let returned = match result.get("sessionId") {
                    Some(Value::String(id)) if !id.trim().is_empty() => Some(id.clone()),
                    None => None,
                    _ => {
                        return Err(AikitError::new(
                            "connection.acp.invalid_session_response",
                            "Invalid returned sessionId",
                        ));
                    }
                };
                let native_session_id = if mode == SessionOpenMode::Create {
                    returned.ok_or_else(|| {
                        AikitError::new(
                            "connection.acp.invalid_session_response",
                            "ACP new response has no sessionId",
                        )
                    })?
                } else {
                    let requested = requested_native_session_id.ok_or_else(|| {
                        AikitError::new(
                            "connection.native_session_id_required",
                            "Load/resume has no requested identity",
                        )
                    })?;
                    if returned.as_ref().is_some_and(|id| id != &requested) {
                        return Err(AikitError::new(
                            "connection.acp.session_identity_changed",
                            "ACP load/resume contradicted the requested native identity",
                        ));
                    }
                    requested
                };
                let mut binding = NativeSessionBinding::unbound(native_session_id.clone(), mode);
                binding.agent_session = canonical_agent_session;
                binding.provenance = self.provenance.clone();
                binding.model_observation = result
                    .get("models")
                    .filter(|v| !v.is_null())
                    .map(NativeModelObservation::from_acp)
                    .transpose()?;
                binding.mode_observation = result
                    .get("modes")
                    .filter(|v| !v.is_null())
                    .and_then(NativeModeObservation::from_acp);
                Ok(vec![self.signal(
                    Some(native_session_id),
                    ConnectionSignalKind::SessionOpened { binding },
                )])
            }
            PendingAcpRequest::Prompt { native_session_id } => {
                let stop_reason = result
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .unwrap_or("end_turn")
                    .to_string();
                let kind = if stop_reason == "cancelled" {
                    ConnectionSignalKind::Cancelled
                } else {
                    ConnectionSignalKind::Completed { stop_reason }
                };
                Ok(vec![self.signal(Some(native_session_id), kind)])
            }
        }
    }

    fn ingest_notification(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<Vec<ConnectionSignal>> {
        match method {
            "session/update" => {
                let native_session_id = string_field(params, "sessionId")?;
                let update = params.get("update").cloned().unwrap_or(Value::Null);
                let kind_name = update
                    .get("sessionUpdate")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let kind = match kind_name {
                    "agent_message_chunk" => ConnectionSignalKind::AgentMessageChunk {
                        text: extract_text(&update),
                    },
                    "agent_thought_chunk" => ConnectionSignalKind::AgentThoughtChunk {
                        text: extract_text(&update),
                        content: update,
                    },
                    "tool_call" | "tool_call_update" => {
                        ConnectionSignalKind::ToolCall { payload: update }
                    }
                    "tool_result" => ConnectionSignalKind::ToolResult { payload: update },
                    _ => ConnectionSignalKind::Status {
                        message: format!("ACP session update: {kind_name}"),
                    },
                };
                Ok(vec![self.signal(Some(native_session_id), kind)])
            }
            "session/cancel" => {
                let native_session_id = string_field(params, "sessionId")?;
                Ok(vec![self.signal(
                    Some(native_session_id),
                    ConnectionSignalKind::Cancelled,
                )])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn ingest_permission_request(
        &mut self,
        id: u64,
        params: &Value,
    ) -> Result<Vec<ConnectionSignal>> {
        let native_session_id = string_field(params, "sessionId")?;
        let tool_call_id = params
            .get("toolCall")
            .and_then(|tool| tool.get("toolCallId"))
            .or_else(|| params.get("toolCallId"))
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
                    kind: option
                        .get("kind")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    label: option
                        .get("name")
                        .or_else(|| option.get("label"))?
                        .as_str()?
                        .to_string(),
                })
            })
            .collect();
        let request = NativePermissionRequest {
            native_request_id: id.to_string(),
            native_session_id: native_session_id.clone(),
            tool_call_id,
            tool_call: params.get("toolCall").cloned().unwrap_or(Value::Null),
            raw: params.clone(),
            choices,
            provenance: self.provenance.clone(),
        };
        Ok(vec![self.signal(
            Some(native_session_id),
            ConnectionSignalKind::PermissionRequested { request },
        )])
    }
}

impl AgentConnectionAdapter for AcpV1ConnectionAdapter {
    fn descriptor(&self) -> ConnectionDescriptor {
        ConnectionDescriptor {
            adapter_ref: self.adapter_ref.clone(),
            connection_ref: self.connection_ref.clone(),
            protocol: ConnectionProtocol {
                family: ConnectionProtocolFamily::Acp,
                version: ACP_STABLE_PROTOCOL_VERSION.to_string(),
            },
            capabilities: self.capabilities.clone(),
            provenance: self.provenance.clone(),
        }
    }

    fn initialize(&mut self) -> Result<ConnectionCommand> {
        Ok(self.request(
            "initialize",
            json!({
                "protocolVersion": ACP_STABLE_PROTOCOL_VERSION,
                "clientCapabilities": {},
                "clientInfo": {
                    "name": "AIKit",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
            PendingAcpRequest::Initialize,
        ))
    }

    fn open_session(&mut self, request: SessionOpenRequest) -> Result<ConnectionCommand> {
        // Resume and attach both ride the stabilized `session/resume`
        // operation; both are gated on the negotiated
        // `agentCapabilities.sessionCapabilities.resume` fact, and the refusal
        // names that capability rather than a protocol-generation claim.
        let resume_routed = matches!(
            request.mode,
            SessionOpenMode::Resume | SessionOpenMode::Attach
        );
        if !self.capabilities.supports(request.mode)
            && !(resume_routed && self.capabilities.supports(SessionOpenMode::Resume))
        {
            return Err(AikitError::new(
                "connection.session_operation_unsupported",
                if resume_routed {
                    "ACP target does not advertise agentCapabilities.sessionCapabilities.resume; \
                     there is no protocol operation to open an existing native session"
                        .to_owned()
                } else {
                    format!("ACP target does not advertise {:?}", request.mode)
                },
            ));
        }
        if !request.additional_directories.is_empty() && !self.capabilities.additional_directories {
            return Err(AikitError::new(
                "connection.additional_directories_unsupported",
                "ACP target does not advertise additionalDirectories",
            ));
        }

        let method = match request.mode {
            SessionOpenMode::Create => "session/new",
            SessionOpenMode::Load => "session/load",
            SessionOpenMode::Resume | SessionOpenMode::Attach => "session/resume",
        };
        let mut params = json!({
            "cwd": request.cwd,
            "mcpServers": request.mcp_servers,
        });
        if let Some(object) = params.as_object_mut() {
            if !request.additional_directories.is_empty() {
                object.insert(
                    "additionalDirectories".into(),
                    json!(request.additional_directories),
                );
            }
            if request.mode != SessionOpenMode::Create {
                let native_session_id = request
                    .native_session_id
                    .clone()
                    .filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| {
                        AikitError::new(
                            "connection.native_session_id_required",
                            format!("{method} requires a protocol-native session id"),
                        )
                    })?;
                object.insert("sessionId".into(), json!(native_session_id));
            }
        }
        Ok(self.request(
            method,
            params,
            PendingAcpRequest::Open {
                mode: request.mode,
                canonical_agent_session: request.agent_session,
                requested_native_session_id: request.native_session_id,
            },
        ))
    }

    fn prompt(&mut self, request: PromptRequest) -> Result<ConnectionCommand> {
        Ok(self.request(
            "session/prompt",
            json!({
                "sessionId": request.native_session_id.clone(),
                "prompt": request.prompt,
            }),
            PendingAcpRequest::Prompt {
                native_session_id: request.native_session_id,
            },
        ))
    }

    fn cancel(&mut self, request: CancelRequest) -> Result<ConnectionCommand> {
        if !self.capabilities.cancellation {
            return Err(AikitError::new(
                "connection.cancellation_unsupported",
                "ACP target does not support cancellation",
            ));
        }
        Ok(ConnectionCommand {
            operation: "session/cancel".into(),
            payload: json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": request.native_session_id }
            }),
        })
    }

    fn ingest(&mut self, message: Value) -> Result<Vec<ConnectionSignal>> {
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let id = message.get("id").and_then(Value::as_u64);
        match (method.as_deref(), id) {
            (Some("session/request_permission"), Some(id)) => {
                let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                self.ingest_permission_request(id, &params)
            }
            (Some(method), _) => {
                let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                self.ingest_notification(method, &params)
            }
            (None, Some(id)) => self.ingest_response(id, &message),
            (None, None) => Err(AikitError::new(
                "connection.acp.invalid_message",
                "ACP message is neither a request/notification nor a response",
            )),
        }
    }
}

/// Classic process-shaped adapter for clients without an interactive ACP-style
/// protocol. It proves the shared seam does not make ACP concepts universal.
#[derive(Debug, Clone)]
pub struct ClassicProcessConnectionAdapter {
    descriptor: ConnectionDescriptor,
    argv: Vec<String>,
    next_sequence: u64,
}

impl ClassicProcessConnectionAdapter {
    pub fn new(connection_ref: ResourceRef, argv: Vec<String>, provenance: Vec<String>) -> Self {
        Self {
            descriptor: ConnectionDescriptor {
                adapter_ref: ResourceRef::parse("connection-adapter/classic-process/v1")
                    .expect("static classic adapter ref must be valid"),
                connection_ref,
                protocol: ConnectionProtocol {
                    family: ConnectionProtocolFamily::ClassicProcess,
                    version: "1".into(),
                },
                capabilities: ConnectionCapabilities {
                    session_open: BTreeSet::from([SessionOpenMode::Create]),
                    ordered_streaming: true,
                    cancellation: true,
                    permission_requests: false,
                    reconnect: false,
                    additional_directories: false,
                    mcp_servers: false,
                },
                provenance,
            },
            argv,
            next_sequence: 1,
        }
    }

    fn signal(
        &mut self,
        native_session_id: Option<String>,
        kind: ConnectionSignalKind,
    ) -> ConnectionSignal {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        ConnectionSignal {
            sequence,
            native_session_id,
            kind,
            provenance: self.descriptor.provenance.clone(),
        }
    }
}

impl AgentConnectionAdapter for ClassicProcessConnectionAdapter {
    fn descriptor(&self) -> ConnectionDescriptor {
        self.descriptor.clone()
    }

    fn initialize(&mut self) -> Result<ConnectionCommand> {
        Ok(ConnectionCommand {
            operation: "launch".into(),
            payload: json!({ "argv": self.argv }),
        })
    }

    fn open_session(&mut self, request: SessionOpenRequest) -> Result<ConnectionCommand> {
        if request.mode != SessionOpenMode::Create {
            return Err(AikitError::new(
                "connection.session_operation_unsupported",
                "classic process connection supports create only",
            ));
        }
        Ok(ConnectionCommand {
            operation: "create".into(),
            payload: json!({
                "cwd": request.cwd,
                "agentSession": request.agent_session.map(|value| value.to_string()),
            }),
        })
    }

    fn prompt(&mut self, request: PromptRequest) -> Result<ConnectionCommand> {
        Ok(ConnectionCommand {
            operation: "stdin".into(),
            payload: json!({
                "nativeSessionId": request.native_session_id,
                "prompt": request.prompt,
            }),
        })
    }

    fn cancel(&mut self, request: CancelRequest) -> Result<ConnectionCommand> {
        Ok(ConnectionCommand {
            operation: "interrupt".into(),
            payload: json!({ "nativeSessionId": request.native_session_id }),
        })
    }

    fn ingest(&mut self, message: Value) -> Result<Vec<ConnectionSignal>> {
        let native_session_id = message
            .get("nativeSessionId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let kind = match message.get("kind").and_then(Value::as_str) {
            Some("text") => ConnectionSignalKind::AgentMessageChunk {
                text: message
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            Some("completed") => ConnectionSignalKind::Completed {
                stop_reason: message
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .unwrap_or("completed")
                    .to_string(),
            },
            Some("cancelled") => ConnectionSignalKind::Cancelled,
            Some(other) => ConnectionSignalKind::Status {
                message: format!("classic process event: {other}"),
            },
            None => ConnectionSignalKind::Status {
                message: "classic process event".into(),
            },
        };
        Ok(vec![self.signal(native_session_id, kind)])
    }
}

fn capability_present(value: &Value, field: &str) -> bool {
    value
        .get(field)
        .is_some_and(|capability| !capability.is_null() && capability != &Value::Bool(false))
}

fn string_field(value: &Value, field: &str) -> Result<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            AikitError::new(
                "connection.acp.invalid_message",
                format!("ACP message has no string `{field}`"),
            )
        })
}

fn extract_text(update: &Value) -> String {
    update
        .get("content")
        .and_then(|content| content.get("text"))
        .or_else(|| update.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
