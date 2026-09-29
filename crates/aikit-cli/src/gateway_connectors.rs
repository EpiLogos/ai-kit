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
    AgentHostTurnSource, ConnectorTokenLocation, ConversationHarnessProtocol,
    GatewayConnectorEntry, GatewayConnectorFactory, GatewayConnectorsFile,
    GatewayTurnSourceResolver, GATEWAY_CONNECTORS_FILE_NAME,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use aikit_store::AikitHome;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use crate::cli::{GatewayConnectorCmd, GatewayConnectorSub};
use crate::encounter_profile_provider::resolve_provider;
use crate::encounter_service::EncounterProtocol;
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

pub fn connector_command(
    home: &AikitHome,
    command: GatewayConnectorCmd,
) -> Result<ConnectorOutput> {
    match command.command {
        GatewayConnectorSub::Add {
            platform,
            connector_ref,
            token_location,
            implementation,
            program,
            configuration_ref,
            agent_backing,
            no_stream_replies,
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
            agent_backing,
            !no_stream_replies,
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
    agent_backing: Option<String>,
    stream_replies: bool,
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
        agent_backing,
        stream_replies,
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
            "{}\n    platform {} · implementation {} · {} · token {}{}{}{}",
            entry.connector_ref,
            entry.platform,
            entry.implementation,
            if entry.enabled { "enabled" } else { "disabled" },
            entry.token_location.as_deref().unwrap_or("none"),
            if entry.program.is_empty() {
                String::new()
            } else {
                format!("\n    program: {}", shell_words::join(&entry.program))
            },
            match &entry.configuration_ref {
                Some(reference) => format!("\n    configuration: {reference}"),
                None => String::new(),
            },
            match &entry.agent_backing {
                Some(harness) => format!("\n    agent backing: {harness}"),
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
pub fn connector_factories(home: &AikitHome) -> Result<Vec<Box<dyn GatewayConnectorFactory>>> {
    let file = load(home)?;
    file.connectors
        .into_iter()
        .map(build_connector_factory)
        .collect()
}

/// The conversation engine's turn sources: the encounter plane's own provider
/// registry (`state/encounter-providers/*.json`, profile-derived entries
/// resolved at load) read through the connector entries' `agent_backing`
/// names. No second harness registry is invented here: a connector backs its
/// conversations with a provider the encounter plane already declares.
///
/// A declared backing that no provider answers, or whose connection facts are
/// unreachable, is a serve-time startup error naming it — the gateway must
/// not silently run a connector with conversations that can never be answered.
pub fn conversation_turn_resolver(home: &AikitHome) -> Result<Arc<dyn GatewayTurnSourceResolver>> {
    let file = load(home)?;
    let mut backings: BTreeMap<String, String> = BTreeMap::new();
    for entry in &file.connectors {
        if let Some(harness) = &entry.agent_backing {
            backings.insert(entry.connector_ref.clone(), harness.clone());
        }
    }
    if backings.is_empty() {
        return Ok(Arc::new(NoAgentBacking));
    }
    let providers = load_encounter_providers(home)?;
    let mut resolved: BTreeMap<String, ResolvedProvider> = BTreeMap::new();
    for (connector_ref, harness) in &backings {
        let provider = providers
            .iter()
            .find(|provider| &provider.id == harness)
            .ok_or_else(|| {
                three_part(
                    "gateway.agent_backing_unknown",
                    format!(
                        "Connector {connector_ref} declares agent backing {harness:?}, but no \
                         encounter provider of that name is declared in {}.",
                        home.state().join("encounter-providers").display()
                    ),
                    "The gateway service was not started.",
                    format!(
                        "Declare the provider with `aikit encounter providers add` (or remove \
                         --agent-backing from connector {connector_ref})."
                    ),
                )
            })?;
        if provider.argv.is_empty() {
            return Err(three_part(
                "gateway.agent_backing_unresolved",
                format!(
                    "Connector {connector_ref} declares agent backing {harness:?}, but that \
                     provider's connection facts carry no launch command."
                ),
                "The gateway service was not started.",
                "Declare the provider with an explicit argv or a resolvable profile.",
            ));
        }
        crate::encounter_profile_provider::ensure_connection_facts_reachable(provider).map_err(
            |error| {
                three_part(
                    "gateway.agent_backing_unreachable",
                    format!(
                        "Connector {connector_ref} declares agent backing {harness:?}, but its \
                         launch command is unusable: {error}"
                    ),
                    "The gateway service was not started.",
                    "Fix the provider's connection facts, or remove --agent-backing.",
                )
            },
        )?;
        resolved.insert(
            harness.clone(),
            ResolvedProvider {
                protocol: harness_protocol(provider.protocol),
                argv: provider.argv.clone(),
                cwd: provider
                    .cwd
                    .as_ref()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                    }),
            },
        );
    }
    Ok(Arc::new(ProviderTurnResolver {
        backings,
        resolved,
        sources: Mutex::new(BTreeMap::new()),
    }))
}

struct ResolvedProvider {
    protocol: ConversationHarnessProtocol,
    argv: Vec<String>,
    cwd: std::path::PathBuf,
}

struct NoAgentBacking;

impl GatewayTurnSourceResolver for NoAgentBacking {
    fn turn_source_for(
        &self,
        _connector_ref: &ResourceRef,
        _platform: &str,
    ) -> Option<Arc<dyn aikit_adapters::ConversationTurnSource>> {
        None
    }
}

struct ProviderTurnResolver {
    backings: BTreeMap<String, String>,
    resolved: BTreeMap<String, ResolvedProvider>,
    /// One lazily launched harness per backing name: the same process carries
    /// every conversation that names it.
    sources: Mutex<BTreeMap<String, Arc<dyn aikit_adapters::ConversationTurnSource>>>,
}

impl GatewayTurnSourceResolver for ProviderTurnResolver {
    fn turn_source_for(
        &self,
        connector_ref: &ResourceRef,
        _platform: &str,
    ) -> Option<Arc<dyn aikit_adapters::ConversationTurnSource>> {
        let harness = self.backings.get(connector_ref.as_str())?;
        let mut sources = self.sources.lock().ok()?;
        if let Some(source) = sources.get(harness) {
            return Some(Arc::clone(source));
        }
        let resolved = self.resolved.get(harness)?;
        let source: Arc<dyn aikit_adapters::ConversationTurnSource> =
            Arc::new(AgentHostTurnSource::new(
                harness.clone(),
                resolved.protocol,
                resolved.argv.clone(),
                resolved.cwd.clone(),
            ));
        sources.insert(harness.clone(), Arc::clone(&source));
        Some(source)
    }
}

fn harness_protocol(protocol: EncounterProtocol) -> ConversationHarnessProtocol {
    match protocol {
        EncounterProtocol::Acp => ConversationHarnessProtocol::Acp,
        EncounterProtocol::PiRpc => ConversationHarnessProtocol::PiRpc,
        EncounterProtocol::PrimeRpc => ConversationHarnessProtocol::PrimeRpc,
    }
}

/// The encounter plane's own provider load: every `encounter-providers/*.json`
/// of this home, with profile-derived entries resolved from their embedded
/// profiles at load time. One registry, read the same way.
fn load_encounter_providers(
    home: &AikitHome,
) -> Result<Vec<crate::encounter_service::EncounterProvider>> {
    let root = home.state().join("encounter-providers");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|error| {
        AikitError::new(
            "gateway.agent_backing_providers",
            format!("read {}: {error}", root.display()),
        )
    })? {
        let path = entry
            .map_err(|error| {
                AikitError::new(
                    "gateway.agent_backing_providers",
                    format!("read {}: {error}", root.display()),
                )
            })?
            .path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|error| {
            AikitError::new(
                "gateway.agent_backing_providers",
                format!("read {}: {error}", path.display()),
            )
        })?;
        let provider: crate::encounter_service::EncounterProvider = serde_json::from_slice(&bytes)
            .map_err(|error| {
                AikitError::new(
                    "gateway.agent_backing_providers",
                    format!("decode {}: {error}", path.display()),
                )
            })?;
        rows.push(resolve_provider(provider).map_err(|failure| {
            AikitError::new(failure.code(), format!("{}: {failure}", path.display()))
        })?);
    }
    Ok(rows)
}
