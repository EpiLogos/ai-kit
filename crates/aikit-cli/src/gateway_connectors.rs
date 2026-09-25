//! Gateway connector configuration: `aikit gateway connector add|list|remove`.
//!
//! The connectors file (`state/gateway-connectors.json`) declares which
//! connectors a gateway service runs. It stores locations, never material:
//! `add` verifies the token location resolves (owner-only file or a declared
//! secret ref) and stores only the location. A token value on the command
//! line is refused outright. `serve` builds the connector instances from this
//! file inside the service; an unknown implementation is a startup error
//! naming the implementation.

use aikit_adapters::{
    build_connector_factory, load_gateway_connectors, store_gateway_connectors,
    ConnectorTokenLocation, GatewayConnectorEntry, GatewayConnectorFactory,
    GatewayConnectorsFile, GATEWAY_CONNECTORS_FILE_NAME,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use aikit_store::AikitHome;
use serde_json::{json, Value};

use crate::cli::{GatewayConnectorCmd, GatewayConnectorSub};
use crate::gateway_contact::three_part;

/// The connectors document of this AIKit home.
pub fn connectors_path(home: &AikitHome) -> std::path::PathBuf {
    home.state().join(GATEWAY_CONNECTORS_FILE_NAME)
}

fn load(home: &AikitHome) -> Result<GatewayConnectorsFile> {
    load_gateway_connectors(&connectors_path(home))
}

fn store(home: &AikitHome, file: &GatewayConnectorsFile) -> Result<()> {
    store_gateway_connectors(&connectors_path(home), file)
}

/// What `aikit gateway connector …` answers with. Human output is plain text;
/// `--json` keeps the document form for the envelope.
pub enum ConnectorOutput {
    Text(String),
    Data(Value),
}

pub fn connector_command(home: &AikitHome, command: GatewayConnectorCmd) -> Result<ConnectorOutput> {
    match command.command {
        GatewayConnectorSub::Add {
            platform,
            connector_ref,
            token_location,
            implementation,
            program,
            configuration_ref,
            disable,
            token,
        } => add(
            home,
            platform,
            connector_ref,
            token_location,
            implementation,
            program,
            configuration_ref,
            disable,
            token,
        ),
        GatewayConnectorSub::List { json } => list(home, json),
        GatewayConnectorSub::Remove { connector_ref } => remove(home, connector_ref),
    }
}

#[allow(clippy::too_many_arguments)]
fn add(
    home: &AikitHome,
    platform: String,
    connector_ref: String,
    token_location: Option<String>,
    implementation: Option<String>,
    program: Option<String>,
    configuration_ref: Option<String>,
    disable: bool,
    token: Option<String>,
) -> Result<ConnectorOutput> {
    if token.is_some() {
        return Err(three_part(
            "gateway.connector_token_on_command_line",
            "A connector token was passed on the command line, where shell history and process \
             listings would keep it.",
            "No connector was added.",
            "Store the token in an owner-only file and declare it with --token-location \
             file:/absolute/path.",
        ));
    }
    if platform.trim().is_empty() {
        return Err(AikitError::new(
            "cli.usage",
            "a connector needs --platform (for example: --platform telegram)",
        ));
    }
    ResourceRef::parse(&connector_ref).map_err(|error| {
        AikitError::new(
            "gateway.connector_ref_invalid",
            format!("parse connector ref {connector_ref}: {error}"),
        )
    })?;
    let implementation = implementation.unwrap_or_else(|| platform.clone());
    let program = match &program {
        Some(raw) => shell_words::split(raw).map_err(|error| {
            AikitError::new(
                "gateway.connector_program_invalid",
                format!("parse --program {raw:?}: {error}"),
            )
        })?,
        None => Vec::new(),
    };
    // The location is verified now, so a typo surfaces at declaration time,
    // and resolved again by the service when it builds the connector.
    if let Some(raw) = &token_location {
        ConnectorTokenLocation::parse(raw)?.resolve()?;
    }
    let entry = GatewayConnectorEntry {
        connector_ref: connector_ref.clone(),
        platform: platform.clone(),
        implementation: implementation.clone(),
        enabled: !disable,
        token_location: token_location.as_deref().map(|raw| {
            ConnectorTokenLocation::parse(raw)
                .unwrap_or_else(|_| ConnectorTokenLocation::File(std::path::PathBuf::from(raw)))
                .render()
        }),
        configuration_ref,
        program,
        provenance: vec!["aikit gateway connector add".into()],
    };
    entry.validate()?;
    let mut file = load(home)?;
    file.connectors
        .retain(|existing| existing.connector_ref != connector_ref);
    file.connectors.push(entry.clone());
    file.connectors
        .sort_by(|a, b| a.connector_ref.cmp(&b.connector_ref));
    store(home, &file)?;
    Ok(ConnectorOutput::Data(json!({
        "declared": entry,
        "path": connectors_path(home).display().to_string(),
    })))
}

fn list(home: &AikitHome, json: bool) -> Result<ConnectorOutput> {
    let file = load(home)?;
    if json {
        return Ok(ConnectorOutput::Data(json!({
            "schema": file.schema,
            "connectors": file.connectors,
            "path": connectors_path(home).display().to_string(),
        })));
    }
    if file.connectors.is_empty() {
        return Ok(ConnectorOutput::Text(format!(
            "no connectors declared; add one with `aikit gateway connector add --platform \
             telegram --ref gateway-connector/telegram/main --token-location file:…` (document: \
             {})",
            connectors_path(home).display()
        )));
    }
    let mut lines = Vec::new();
    for entry in &file.connectors {
        lines.push(format!(
            "{}\n    platform {} · implementation {} · {} · token {}{}{}",
            entry.connector_ref,
            entry.platform,
            entry.implementation,
            if entry.enabled { "enabled" } else { "disabled" },
            entry
                .token_location
                .as_deref()
                .unwrap_or("none"),
            if entry.program.is_empty() {
                String::new()
            } else {
                format!("\n    program: {}", shell_words::join(&entry.program))
            },
            match &entry.configuration_ref {
                Some(reference) => format!("\n    configuration: {reference}"),
                None => String::new(),
            },
        ));
    }
    Ok(ConnectorOutput::Text(lines.join("\n")))
}

fn remove(home: &AikitHome, connector_ref: String) -> Result<ConnectorOutput> {
    let mut file = load(home)?;
    let before = file.connectors.len();
    file.connectors
        .retain(|existing| existing.connector_ref != connector_ref);
    if file.connectors.len() == before {
        return Err(AikitError::new(
            "gateway.connector_unknown",
            format!(
                "no connector {connector_ref} is declared in {}; list what is with `aikit \
                 gateway connector list`",
                connectors_path(home).display()
            ),
        ));
    }
    store(home, &file)?;
    Ok(ConnectorOutput::Data(json!({
        "removed": connector_ref,
        "path": connectors_path(home).display().to_string(),
    })))
}

/// Build the factories `serve` will run. An unknown implementation is a
/// startup error naming it, before any carrier binds.
pub fn connector_factories(
    home: &AikitHome,
) -> Result<Vec<Box<dyn GatewayConnectorFactory>>> {
    let file = load(home)?;
    file.connectors
        .into_iter()
        .map(build_connector_factory)
        .collect()
}
