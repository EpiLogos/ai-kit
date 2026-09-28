//! The gateway connector configuration plane: `state/gateway-connectors.json`.
//!
//! The file declares which connectors a gateway service runs. It carries
//! locations, never material: a connector's token is a `file:` path or a
//! declared secret ref, resolved once when the service builds the connector —
//! the same discipline the WebSocket carrier uses for its bearer token. A
//! token value on the command line or in the file is refused.
//!
//! The loader turns each entry into a [`GatewayConnectorFactory`] by
//! implementation name inside the service. An unknown implementation is a
//! startup error naming it, not a silent skip.

use std::path::{Path, PathBuf};

use aikit_core::credential::SecretValue;
use aikit_core::resource::ResourceRef;
use aikit_core::secret_ref::{SecretRef, SecretResolver as _};
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};

use crate::gateway_connector::{
    ConnectorDescriptor, ConnectorFuture, ConnectorHealth, ConnectorHello, ConnectorOperation,
    DeliveryReceipt, GatewayConnector, InboundEvent, OutboundOperation,
    GATEWAY_CONNECTOR_SDK_VERSION,
};
use crate::gateway_connector_wire::StdioWireConnector;
use crate::slack_bot_api::{SlackConnector, SlackConnectorConfig};
use crate::slack_gateway_curl::SlackCurlTransport;
use crate::telegram_gateway::{TelegramConnector, TelegramConnectorConfig};
use crate::telegram_gateway_curl::TelegramCurlTransport;

pub const GATEWAY_CONNECTORS_SCHEMA: &str = "aikit.gateway-connectors/v1";

/// The well-known file name inside an AIKit home's `state/` directory.
pub const GATEWAY_CONNECTORS_FILE_NAME: &str = "gateway-connectors.json";

/// One declared connector the gateway service runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConnectorEntry {
    pub connector_ref: String,
    pub platform: String,
    pub implementation: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_ref: Option<String>,
    /// External connector command (argv) for the `stdio` implementation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub program: Vec<String>,
    /// The harness backing this connector's conversations, named as the
    /// encounter plane names its providers. The gateway conversation engine
    /// resolves the name at serve time and drives real agent turns for the
    /// connector's bindings. A name here is a route to configuration the
    /// owner already declared — never credentials, never a model key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_backing: Option<String>,
    /// Progressive replies: when allowed (the default) and the built
    /// connector can edit its own messages, the deployed connector declares
    /// the Streaming capability and the conversation engine lets the sender
    /// watch a reply grow — a typing pulse for the whole turn, the first
    /// text as a real message, later text as throttled edits of it, the
    /// final text settled at completion. `false` keeps the reply to one
    /// final message. A connector that cannot edit never declares Streaming.
    #[serde(default = "default_stream_replies")]
    pub stream_replies: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance: Vec<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_stream_replies() -> bool {
    true
}

impl GatewayConnectorEntry {
    pub fn validate(&self) -> Result<()> {
        ResourceRef::parse(&self.connector_ref).map_err(|error| {
            AikitError::new(
                "gateway_connector_config.connector_ref",
                format!("connector ref {}: {error}", self.connector_ref),
            )
        })?;
        if self.platform.trim().is_empty() {
            return Err(AikitError::new(
                "gateway_connector_config.empty_platform",
                "a connector entry must name its platform",
            ));
        }
        if self.implementation.trim().is_empty() {
            return Err(AikitError::new(
                "gateway_connector_config.empty_implementation",
                format!(
                    "connector {} must name its implementation",
                    self.connector_ref
                ),
            ));
        }
        match self.implementation.as_str() {
            "telegram" => {
                if self.token_location.is_none() {
                    return Err(AikitError::new(
                        "gateway_connector_config.token_location_required",
                        format!(
                            "connector {} is a telegram connector; declare --token-location \
                             (an owner-only file: path or a keychain/pass/op/varlock ref)",
                            self.connector_ref
                        ),
                    ));
                }
            }
            "slack" => {
                if self.token_location.is_none() {
                    return Err(AikitError::new(
                        "gateway_connector_config.token_location_required",
                        format!(
                            "connector {} is a slack connector; declare --token-location \
                             (an owner-only file: path or a keychain/pass/op/varlock ref)",
                            self.connector_ref
                        ),
                    ));
                }
            }
            "stdio" => {
                if self.program.is_empty() || self.program[0].trim().is_empty() {
                    return Err(AikitError::new(
                        "gateway_connector_config.program_required",
                        format!(
                            "connector {} uses the stdio implementation; declare --program COMMAND",
                            self.connector_ref
                        ),
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayConnectorsFile {
    pub schema: String,
    #[serde(default)]
    pub connectors: Vec<GatewayConnectorEntry>,
}

impl Default for GatewayConnectorsFile {
    fn default() -> Self {
        Self {
            schema: GATEWAY_CONNECTORS_SCHEMA.into(),
            connectors: Vec::new(),
        }
    }
}

/// Load the connectors document at `path`; a missing file means no connectors.
pub fn load_gateway_connectors(path: &Path) -> Result<GatewayConnectorsFile> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(GatewayConnectorsFile::default())
        }
        Err(error) => {
            return Err(AikitError::new(
                "gateway_connector_config.unreadable",
                format!("read {}: {error}", path.display()),
            ))
        }
    };
    let file: GatewayConnectorsFile = serde_json::from_slice(&bytes).map_err(|error| {
        AikitError::new(
            "gateway_connector_config.invalid",
            format!(
                "{} is not a valid {GATEWAY_CONNECTORS_SCHEMA} document: {error}",
                path.display()
            ),
        )
    })?;
    if file.schema != GATEWAY_CONNECTORS_SCHEMA {
        return Err(AikitError::new(
            "gateway_connector_config.invalid",
            format!("{} has schema {}", path.display(), file.schema),
        ));
    }
    for entry in &file.connectors {
        entry.validate()?;
    }
    Ok(file)
}

/// Store the connectors document atomically.
pub fn store_gateway_connectors(path: &Path, file: &GatewayConnectorsFile) -> Result<()> {
    let write = |error: std::io::Error| {
        AikitError::new(
            "gateway_connector_config.write",
            format!("write {}: {error}", path.display()),
        )
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(write)?;
    }
    let bytes = serde_json::to_vec_pretty(file)
        .map_err(|error| AikitError::new("gateway_connector_config.write", error.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(write)?;
    std::fs::rename(&tmp, path).map_err(write)
}

/// Where a connector's token lives. A location, never material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectorTokenLocation {
    File(PathBuf),
    Declared(SecretRef),
}

impl ConnectorTokenLocation {
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if let Some(path) = raw.strip_prefix("file:") {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(AikitError::new(
                    "gateway_connector_config.invalid_location",
                    format!("file: token locations must be absolute paths, got {raw}"),
                ));
            }
            return Ok(Self::File(path));
        }
        let secret_ref = SecretRef::parse(raw)?;
        if matches!(secret_ref, SecretRef::Env { .. }) {
            return Err(AikitError::new(
                "gateway_connector_config.env_refused",
                "env:// names a transient process variable, not a store; declare a file: \
                 location or a keychain/pass/op/varlock ref",
            ));
        }
        Ok(Self::Declared(secret_ref))
    }

    pub fn render(&self) -> String {
        match self {
            Self::File(path) => format!("file:{}", path.display()),
            Self::Declared(secret_ref) => secret_ref.to_string(),
        }
    }

    /// Materialise at the moment the service builds the connector. A file
    /// location must be owner-only and non-empty.
    pub fn resolve(&self) -> Result<SecretValue> {
        match self {
            Self::File(path) => read_owner_only(path),
            Self::Declared(secret_ref) => {
                crate::secret_resolver::SuiteSecretResolver::default().resolve(secret_ref)
            }
        }
    }
}

fn read_owner_only(path: &Path) -> Result<SecretValue> {
    const MAX_TOKEN_FILE_BYTES: u64 = 4096;
    let metadata = std::fs::metadata(path).map_err(|error| {
        AikitError::new(
            "gateway_connector_config.token_unusable",
            format!("token file {} cannot be read: {error}", path.display()),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(AikitError::new(
                "gateway_connector_config.token_permissions_too_open",
                format!(
                    "token file {} has mode {:o}; it must be readable by its owner only \
                     (chmod 600 {})",
                    path.display(),
                    mode & 0o777,
                    path.display()
                ),
            ));
        }
    }
    if metadata.len() > MAX_TOKEN_FILE_BYTES {
        return Err(AikitError::new(
            "gateway_connector_config.token_too_large",
            format!(
                "token file {} is {} bytes; a connector credential is at most {MAX_TOKEN_FILE_BYTES}",
                path.display(),
                metadata.len()
            ),
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|error| {
        AikitError::new(
            "gateway_connector_config.token_unusable",
            format!("token file {} cannot be read: {error}", path.display()),
        )
    })?;
    SecretValue::new(text.trim().to_owned()).map_err(|_| {
        AikitError::new(
            "gateway_connector_config.token_empty",
            format!("token file {} is empty", path.display()),
        )
    })
}

/// Builds one connector instance for a configured entry, inside the service.
pub trait GatewayConnectorFactory: Send + Sync {
    fn entry(&self) -> &GatewayConnectorEntry;
    fn build(&self) -> Result<Box<dyn GatewayConnector>>;
}

/// Map an entry to its implementation's factory. An unknown implementation is
/// a named startup error, never a silent skip. When the entry allows
/// progressive replies (the default), the built connector gains the
/// Streaming declaration — composing the owner's allowance with the
/// connector's own ability to edit; see [`declare_streaming`].
pub fn build_connector_factory(
    entry: GatewayConnectorEntry,
) -> Result<Box<dyn GatewayConnectorFactory>> {
    entry.validate()?;
    let inner: Box<dyn GatewayConnectorFactory> = match entry.implementation.as_str() {
        "telegram" => Box::new(TelegramConnectorFactory {
            entry: entry.clone(),
        }),
        "slack" => Box::new(SlackConnectorFactory {
            entry: entry.clone(),
        }),
        "stdio" => Box::new(StdioConnectorFactory {
            entry: entry.clone(),
        }),
        other => {
            return Err(AikitError::new(
                "gateway_connector_config.unknown_implementation",
                format!(
                    "connector {} names implementation {other:?}; this build knows `telegram`, \
                     `slack` and `stdio`",
                    entry.connector_ref
                ),
            ))
        }
    };
    if !entry.stream_replies {
        return Ok(inner);
    }
    Ok(Box::new(StreamingDeclarationFactory { inner }))
}

/// The streaming declaration a connectors entry composes. The owner allows
/// progressive replies on this connector; when the connector can edit its own
/// messages, the deployed connector advertises Streaming so the conversation
/// engine may let a reply grow. Nothing else about the connector changes: its
/// operations, ingress, delivery and health are exactly the built
/// connector's, and its own descriptor is what still validates every
/// operation it executes (no operation requires Streaming).
struct StreamingDeclarationFactory {
    inner: Box<dyn GatewayConnectorFactory>,
}

impl GatewayConnectorFactory for StreamingDeclarationFactory {
    fn entry(&self) -> &GatewayConnectorEntry {
        self.inner.entry()
    }

    fn build(&self) -> Result<Box<dyn GatewayConnector>> {
        Ok(declare_streaming(self.inner.build()?))
    }
}

/// Add the Streaming capability to a connector that can edit. A connector
/// that cannot edit — or that already declares Streaming — is returned
/// unchanged: streaming without the ability to grow a message is a claim
/// nothing could keep.
fn declare_streaming(connector: Box<dyn GatewayConnector>) -> Box<dyn GatewayConnector> {
    let mut descriptor = connector.descriptor();
    if !descriptor
        .capabilities
        .operations
        .contains(&ConnectorOperation::Edit)
        || descriptor
            .capabilities
            .operations
            .contains(&ConnectorOperation::Streaming)
    {
        return connector;
    }
    descriptor
        .capabilities
        .operations
        .insert(ConnectorOperation::Streaming);
    descriptor
        .provenance
        .push("stream_replies: the connectors entry allows progressive replies".into());
    Box::new(StreamDeclaredConnector {
        inner: connector,
        descriptor,
    })
}

/// A connector whose hello carries the composed Streaming declaration. Every
/// lifecycle method delegates to the built connector; only the descriptor
/// the kernel registers names the extra capability.
struct StreamDeclaredConnector {
    inner: Box<dyn GatewayConnector>,
    descriptor: ConnectorDescriptor,
}

impl GatewayConnector for StreamDeclaredConnector {
    fn descriptor(&self) -> ConnectorDescriptor {
        self.descriptor.clone()
    }

    fn connect(&mut self) -> ConnectorFuture<'_, ConnectorHello> {
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            let mut hello = self.inner.connect().await?;
            hello.descriptor = descriptor;
            Ok(hello)
        })
    }

    fn next_event(&mut self) -> ConnectorFuture<'_, Option<InboundEvent>> {
        Box::pin(async move { self.inner.next_event().await })
    }

    fn execute(&mut self, operation: OutboundOperation) -> ConnectorFuture<'_, DeliveryReceipt> {
        Box::pin(async move { self.inner.execute(operation).await })
    }

    fn health(&mut self) -> ConnectorFuture<'_, ConnectorHealth> {
        Box::pin(async move { self.inner.health().await })
    }

    fn disconnect(&mut self) -> ConnectorFuture<'_, ()> {
        Box::pin(async move { self.inner.disconnect().await })
    }
}

/// The Telegram implementation constructs its connector with the token resolved
/// from the declared location and the live Bot API carrier: the system curl
/// (see [`crate::telegram_gateway_curl`]). Long polling, delivery and edits
/// ride the same transport the live proof exercises.
struct TelegramConnectorFactory {
    entry: GatewayConnectorEntry,
}

impl GatewayConnectorFactory for TelegramConnectorFactory {
    fn entry(&self) -> &GatewayConnectorEntry {
        &self.entry
    }

    fn build(&self) -> Result<Box<dyn GatewayConnector>> {
        let connector_ref = ResourceRef::parse(&self.entry.connector_ref).map_err(|error| {
            AikitError::new(
                "gateway_connector_config.connector_ref",
                format!("connector ref {}: {error}", self.entry.connector_ref),
            )
        })?;
        let configuration_ref = match &self.entry.configuration_ref {
            Some(raw) => Some(ResourceRef::parse(raw).map_err(|error| {
                AikitError::new(
                    "gateway_connector_config.configuration_ref",
                    format!("configuration ref {raw}: {error}"),
                )
            })?),
            None => None,
        };
        let raw_location = self.entry.token_location.as_deref().ok_or_else(|| {
            AikitError::new(
                "gateway_connector_config.token_location_required",
                format!(
                    "connector {} has no token location",
                    self.entry.connector_ref
                ),
            )
        })?;
        let token = ConnectorTokenLocation::parse(raw_location)?.resolve()?;
        let transport = TelegramCurlTransport::from_token(token.expose())?;
        let connector = TelegramConnector::new(
            transport,
            TelegramConnectorConfig {
                connector_ref,
                configuration_ref,
                // The connector worker services its outbound queue (typing
                // pulses, streamed segments, tool lines) between event polls,
                // so the poll cycle bounds how live the conversation feels —
                // and Telegram expires a typing indicator after ~5s. Short
                // cycle; the Bot API charges nothing for it.
                poll_timeout_seconds: 3,
                allowed_updates: Vec::new(),
                provenance: vec!["gateway connectors file".into()],
            },
        )?;
        Ok(Box::new(connector))
    }
}

/// The Slack implementation constructs its connector with the token resolved
/// from the declared location (the same [`ConnectorTokenLocation`] resolver
/// Telegram uses) and the live Web API carrier: the system curl (see
/// [`crate::slack_gateway_curl`]). Ingress polls `conversations.history` for
/// the channels declared in the connector's configuration ref material — the
/// factory starts delivery-only (no ingress channels), matching the
/// egress-first posture; polling channels are a configuration concern.
struct SlackConnectorFactory {
    entry: GatewayConnectorEntry,
}

impl GatewayConnectorFactory for SlackConnectorFactory {
    fn entry(&self) -> &GatewayConnectorEntry {
        &self.entry
    }

    fn build(&self) -> Result<Box<dyn GatewayConnector>> {
        let connector_ref = ResourceRef::parse(&self.entry.connector_ref).map_err(|error| {
            AikitError::new(
                "gateway_connector_config.connector_ref",
                format!("connector ref {}: {error}", self.entry.connector_ref),
            )
        })?;
        let configuration_ref = match &self.entry.configuration_ref {
            Some(raw) => Some(ResourceRef::parse(raw).map_err(|error| {
                AikitError::new(
                    "gateway_connector_config.configuration_ref",
                    format!("configuration ref {raw}: {error}"),
                )
            })?),
            None => None,
        };
        let raw_location = self.entry.token_location.as_deref().ok_or_else(|| {
            AikitError::new(
                "gateway_connector_config.token_location_required",
                format!(
                    "connector {} has no token location",
                    self.entry.connector_ref
                ),
            )
        })?;
        let token = ConnectorTokenLocation::parse(raw_location)?.resolve()?;
        let transport = SlackCurlTransport::from_token(token.expose())?;
        let connector = SlackConnector::new(
            transport,
            SlackConnectorConfig {
                connector_ref,
                configuration_ref,
                ingress_channels: Vec::new(),
                ingest_backlog: false,
                history_poll_limit: 100,
                provenance: vec!["gateway connectors file".into()],
            },
        )?;
        Ok(Box::new(connector))
    }
}

struct StdioConnectorFactory {
    entry: GatewayConnectorEntry,
}

impl GatewayConnectorFactory for StdioConnectorFactory {
    fn entry(&self) -> &GatewayConnectorEntry {
        &self.entry
    }

    fn build(&self) -> Result<Box<dyn GatewayConnector>> {
        let connector_ref = ResourceRef::parse(&self.entry.connector_ref).map_err(|error| {
            AikitError::new(
                "gateway_connector_config.connector_ref",
                format!("connector ref {}: {error}", self.entry.connector_ref),
            )
        })?;
        let configuration_ref = match &self.entry.configuration_ref {
            Some(raw) => Some(ResourceRef::parse(raw).map_err(|error| {
                AikitError::new(
                    "gateway_connector_config.configuration_ref",
                    format!("configuration ref {raw}: {error}"),
                )
            })?),
            None => None,
        };
        Ok(Box::new(StdioWireConnector::new(
            self.entry.program.clone(),
            connector_ref,
            self.entry.platform.clone(),
            configuration_ref,
        )))
    }
}

/// The descriptor a stdio connector advertises before its Hello arrives: an
/// empty-capability shell so health can attach to a registered connector while
/// the child starts. The Hello's descriptor replaces it.
pub fn stdio_shell_descriptor(
    connector_ref: &ResourceRef,
    platform: &str,
    configuration_ref: Option<&ResourceRef>,
) -> ConnectorDescriptor {
    ConnectorDescriptor {
        version: GATEWAY_CONNECTOR_SDK_VERSION.into(),
        connector_ref: connector_ref.clone(),
        platform: platform.to_owned(),
        implementation: "stdio".into(),
        capabilities: Default::default(),
        configuration_ref: configuration_ref.cloned(),
        provenance: vec!["aikit stdio wire host".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(implementation: &str) -> GatewayConnectorEntry {
        GatewayConnectorEntry {
            connector_ref: "gateway-connector/telegram/main".into(),
            platform: "telegram".into(),
            implementation: implementation.into(),
            enabled: true,
            token_location: Some("file:/run/token".into()),
            configuration_ref: None,
            program: Vec::new(),
            agent_backing: None,
            stream_replies: true,
            provenance: Vec::new(),
        }
    }

    #[test]
    fn the_connectors_file_round_trips_and_defaults_to_enabled() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(GATEWAY_CONNECTORS_FILE_NAME);
        let mut file = GatewayConnectorsFile::default();
        let mut specimen = entry("stdio");
        specimen.connector_ref = "gateway-connector/specimen/main".into();
        specimen.platform = "specimen".into();
        specimen.token_location = None;
        specimen.program = vec!["gateway-connector-specimen".into()];
        file.connectors.push(entry("telegram"));
        file.connectors.push(specimen);
        store_gateway_connectors(&path, &file).unwrap();
        let loaded = load_gateway_connectors(&path).unwrap();
        assert_eq!(loaded, file);
        let encoded = std::fs::read_to_string(&path).unwrap();
        assert!(encoded.contains("gateway-connectors/v1"));
        assert!(!encoded.contains("token-value"), "never a token material");
    }

    #[test]
    fn an_unknown_implementation_is_a_named_startup_error() {
        let error = match build_connector_factory(entry("carrier-pigeon")) {
            Err(error) => error,
            Ok(_) => panic!("an unknown implementation must be refused"),
        };
        assert_eq!(
            error.code(),
            "gateway_connector_config.unknown_implementation"
        );
        assert!(error.to_string().contains("carrier-pigeon"), "{error}");
    }

    #[test]
    fn telegram_entries_need_a_token_location_and_stdio_entries_need_a_program() {
        let mut no_token = entry("telegram");
        no_token.token_location = None;
        assert_eq!(
            no_token.validate().unwrap_err().code(),
            "gateway_connector_config.token_location_required"
        );
        let mut no_program = entry("stdio");
        no_program.program = Vec::new();
        assert_eq!(
            no_program.validate().unwrap_err().code(),
            "gateway_connector_config.program_required"
        );
    }

    #[test]
    fn token_locations_refuse_relative_files_env_refs_and_loose_permissions() {
        assert_eq!(
            ConnectorTokenLocation::parse("file:relative/token")
                .unwrap_err()
                .code(),
            "gateway_connector_config.invalid_location"
        );
        assert_eq!(
            ConnectorTokenLocation::parse("env://TELEGRAM_TOKEN")
                .unwrap_err()
                .code(),
            "gateway_connector_config.env_refused"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("token");
            std::fs::write(&path, "token-value").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            let location =
                ConnectorTokenLocation::parse(&format!("file:{}", path.display())).unwrap();
            let error = location.resolve().unwrap_err();
            assert_eq!(
                error.code(),
                "gateway_connector_config.token_permissions_too_open"
            );
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(location.resolve().unwrap().expose(), "token-value");
        }
    }

    #[test]
    fn factories_build_their_named_implementations() {
        // The factory resolves the declared token location when the service
        // builds the connector, so the token file must be real and
        // owner-only here, exactly as in deployment.
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("bot.token");
        std::fs::write(&token, "bot-token-value\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut telegram_entry = entry("telegram");
        telegram_entry.token_location = Some(format!("file:{}", token.display()));
        let telegram = build_connector_factory(telegram_entry).unwrap();
        assert_eq!(telegram.entry().implementation, "telegram");
        let connector = telegram.build().unwrap();
        assert_eq!(connector.descriptor().platform, "telegram");
        // The factory wires the live curl transport; whether the real Bot API
        // answers is physical evidence (telegram_gateway_live.rs), never a
        // deterministic suite call.

        // Slack mirrors telegram: the same token-location resolver, the live
        // Web API curl transport, egress-first capabilities.
        let mut slack_entry = entry("slack");
        slack_entry.connector_ref = "gateway-connector/slack/main".into();
        slack_entry.platform = "slack".into();
        slack_entry.token_location = Some(format!("file:{}", token.display()));
        let slack = build_connector_factory(slack_entry).unwrap();
        assert_eq!(slack.entry().implementation, "slack");
        let connector = slack.build().unwrap();
        assert_eq!(connector.descriptor().platform, "slack");
        use crate::gateway_connector::ConnectorOperation;
        let operations = &connector.descriptor().capabilities.operations;
        assert!(operations.contains(&ConnectorOperation::Send));
        assert!(
            !operations.contains(&ConnectorOperation::Typing),
            "Slack has no typing API; the capability is never advertised"
        );
        assert!(
            !operations.contains(&ConnectorOperation::Media),
            "files.upload v2 is outside this cut; the capability is never advertised"
        );

        let mut no_token = entry("slack");
        no_token.connector_ref = "gateway-connector/slack/main".into();
        no_token.token_location = None;
        assert_eq!(
            no_token.validate().unwrap_err().code(),
            "gateway_connector_config.token_location_required"
        );

        let mut specimen = entry("stdio");
        specimen.connector_ref = "gateway-connector/specimen/main".into();
        specimen.platform = "specimen".into();
        specimen.token_location = None;
        specimen.program = vec!["/bin/cat".into()];
        let stdio = build_connector_factory(specimen).unwrap();
        let connector = stdio.build().unwrap();
        assert_eq!(
            connector.descriptor().connector_ref.to_string(),
            "gateway-connector/specimen/main"
        );
        assert!(
            connector.descriptor().capabilities.operations.is_empty(),
            "the pre-Hello shell advertises nothing"
        );
    }

    #[test]
    fn a_connectors_entry_allowing_progressive_replies_declares_streaming_on_an_editing_connector()
    {
        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("bot.token");
        std::fs::write(&token, "bot-token-value\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let use_token = |entry: &mut GatewayConnectorEntry| {
            entry.token_location = Some(format!("file:{}", token.display()));
        };

        // Telegram can edit its own messages, so the default entry declares
        // Streaming: the owner's allowance composed with the connector's
        // ability to grow a message.
        let mut telegram_entry = entry("telegram");
        use_token(&mut telegram_entry);
        let connector = build_connector_factory(telegram_entry)
            .unwrap()
            .build()
            .unwrap();
        let operations = &connector.descriptor().capabilities.operations;
        assert!(operations.contains(&ConnectorOperation::Edit));
        assert!(
            operations.contains(&ConnectorOperation::Streaming),
            "an editing connector with the default entry declares Streaming"
        );
        assert!(
            connector
                .descriptor()
                .provenance
                .iter()
                .any(|line| line.contains("stream_replies")),
            "the declaration says where it came from"
        );

        // The opt-out keeps the connector exactly as it was built: no
        // Streaming declaration, reply as one final message.
        let mut quiet_entry = entry("telegram");
        quiet_entry.stream_replies = false;
        use_token(&mut quiet_entry);
        let connector = build_connector_factory(quiet_entry)
            .unwrap()
            .build()
            .unwrap();
        let operations = &connector.descriptor().capabilities.operations;
        assert!(operations.contains(&ConnectorOperation::Edit));
        assert!(
            !operations.contains(&ConnectorOperation::Streaming),
            "the opt-out declares no Streaming"
        );

        // A connector that cannot edit never declares Streaming, whatever
        // the entry allows: the claim would be unkeepable. (The stdio shell
        // advertises nothing before its Hello — same law, empty operations.)
        let mut shell_entry = entry("stdio");
        shell_entry.connector_ref = "gateway-connector/specimen/main".into();
        shell_entry.platform = "specimen".into();
        shell_entry.token_location = None;
        shell_entry.program = vec!["/bin/cat".into()];
        let connector = build_connector_factory(shell_entry)
            .unwrap()
            .build()
            .unwrap();
        assert!(
            !connector
                .descriptor()
                .capabilities
                .operations
                .contains(&ConnectorOperation::Streaming),
            "a connector that cannot edit never declares Streaming"
        );

        // An entry that omits the flag reads as the default (allowed), so
        // older connectors files load unchanged.
        let file: GatewayConnectorEntry = serde_json::from_value(json!({
            "connector_ref": "gateway-connector/telegram/main",
            "platform": "telegram",
            "implementation": "telegram",
            "token_location": "file:/run/token"
        }))
        .unwrap();
        assert!(file.stream_replies, "the flag defaults to allowed");
    }
}
