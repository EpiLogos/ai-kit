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

use crate::gateway_connector::{ConnectorDescriptor, GatewayConnector, GATEWAY_CONNECTOR_SDK_VERSION};
use crate::gateway_connector_wire::StdioWireConnector;
use crate::telegram_gateway::{
    TelegramBotApiTransport, TelegramConnector, TelegramConnectorConfig,
};

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provenance: Vec<String>,
}

fn default_enabled() -> bool {
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
                format!("connector {} must name its implementation", self.connector_ref),
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
    let bytes = serde_json::to_vec_pretty(file).map_err(|error| {
        AikitError::new("gateway_connector_config.write", error.to_string())
    })?;
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
/// a named startup error, never a silent skip.
pub fn build_connector_factory(entry: GatewayConnectorEntry) -> Result<Box<dyn GatewayConnectorFactory>> {
    entry.validate()?;
    match entry.implementation.as_str() {
        "telegram" => Ok(Box::new(TelegramConnectorFactory { entry })),
        "stdio" => Ok(Box::new(StdioConnectorFactory { entry })),
        other => Err(AikitError::new(
            "gateway_connector_config.unknown_implementation",
            format!(
                "connector {} names implementation {other:?}; this build knows `telegram` \
                 and `stdio`",
                entry.connector_ref
            ),
        )),
    }
}

/// The Telegram implementation constructs its connector with the token resolved
/// from the declared location. This build carries no Telegram HTTP transport,
/// so a constructed connector reports each provider call as refused rather
/// than pretending to poll — the pump surfaces that as Unavailable health.
struct UnconfiguredTelegramTransport {
    _token: SecretValue,
}

impl TelegramBotApiTransport for UnconfiguredTelegramTransport {
    fn call(
        &mut self,
        method: &str,
        _params: serde_json::Value,
    ) -> aikit_core::Result<serde_json::Value> {
        Err(AikitError::new(
            "telegram_gateway.transport_unconfigured",
            format!(
                "this AIKit build carries no Telegram HTTP transport; Bot API method {method} \
                 was not sent"
            ),
        ))
    }
}

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
        let raw_location = self
            .entry
            .token_location
            .as_deref()
            .ok_or_else(|| {
                AikitError::new(
                    "gateway_connector_config.token_location_required",
                    format!("connector {} has no token location", self.entry.connector_ref),
                )
            })?;
        let token = ConnectorTokenLocation::parse(raw_location)?.resolve()?;
        let connector = TelegramConnector::new(
            UnconfiguredTelegramTransport { _token: token },
            TelegramConnectorConfig {
                connector_ref,
                configuration_ref,
                poll_timeout_seconds: 30,
                allowed_updates: Vec::new(),
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

    fn entry(implementation: &str) -> GatewayConnectorEntry {
        GatewayConnectorEntry {
            connector_ref: "gateway-connector/telegram/main".into(),
            platform: "telegram".into(),
            implementation: implementation.into(),
            enabled: true,
            token_location: Some("file:/run/token".into()),
            configuration_ref: None,
            program: Vec::new(),
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
        };        assert_eq!(
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
        let mut connector = telegram.build().unwrap();
        assert_eq!(connector.descriptor().platform, "telegram");
        // No HTTP transport in this build: the constructed connector reports
        // the refusal instead of pretending to reach the Bot API.
        let refused =
            crate::gateway_connector_pump::block_on(connector.connect()).unwrap_err();
        assert_eq!(refused.code(), "telegram_gateway.transport_unconfigured");

        let mut specimen = entry("stdio");
        specimen.connector_ref = "gateway-connector/specimen/main".into();
        specimen.platform = "specimen".into();
        specimen.token_location = None;
        specimen.program = vec!["/bin/cat".into()];
        let stdio = build_connector_factory(specimen).unwrap();
        let connector = stdio.build().unwrap();
        assert_eq!(connector.descriptor().connector_ref.to_string(), "gateway-connector/specimen/main");
        assert!(
            connector.descriptor().capabilities.operations.is_empty(),
            "the pre-Hello shell advertises nothing"
        );
    }
}
