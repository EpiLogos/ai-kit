//! The encounter-side MCP composition seam: what an open native session may
//! carry as `mcpServers`, decided in one place.
//!
//! The invariant this module owns is *a session's tool surface is disclosed,
//! never silently dropped*. Exactly three outcomes exist for an open: the
//! composed wire values ride on the open request, the connection protocol
//! refuses per-session MCP servers and says so naming the protocol, or nothing
//! is composed. There is no fourth state in which tool capsules exist but
//! quietly never reach the session — a caller that cannot tell "nothing to
//! carry" from "carried nothing by mistake" cannot disclose what it did.
//!
//! The ACP v1 wire shapes here are taken from the stable `schema/v1`
//! (`agent-client-protocol`), which the [`crate::aikit_adapters`] passthrough
//! forwards verbatim: a stdio entry is `{name, command, args, env}` with `args`
//! and `env` required even when empty and `env` an array of `{name, value}`
//! pairs — notably *without* a `type` discriminator; a remote entry is
//! `{type: "http", name, url, headers}`. A record's `cwd` has no ACP stdio
//! field and is not carried on the wire.

use std::collections::BTreeMap;
use std::path::Path;

use aikit_adapters::agent_connection::{SessionOpenMode, SessionOpenRequest};
use aikit_adapters::tool_sources::ToolSourceEntry;
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::AikitHome;
use serde_json::{json, Value};

/// Shape resolved `tool-protocol` entries into the ACP `session/new`
/// `mcpServers` array, in the exact form the connection adapter passes
/// through.
///
/// Entries must carry a capsule-validated server record — exactly one of
/// `command` or `url`, which `aikit_core::capsule` enforces at parse; a
/// hand-built record with neither is not representable on the wire and yields
/// no entry, which is why the only production caller
/// ([`active_tool_source_entries`]) re-checks the record and refuses before
/// shaping is ever reached.
pub fn session_mcp_wire_values(entries: impl IntoIterator<Item = ToolSourceEntry>) -> Vec<Value> {
    entries
        .into_iter()
        .filter_map(|entry| {
            let server = entry.server;
            if let Some(command) = server.command {
                return Some(json!({
                    "name": entry.export_name,
                    "command": command,
                    "args": server.args,
                    "env": server
                        .env
                        .iter()
                        .map(|(name, value)| json!({"name": name, "value": value}))
                        .collect::<Vec<_>>(),
                }));
            }
            server.url.map(|url| {
                json!({
                    "type": "http",
                    "name": entry.export_name,
                    "url": url,
                    "headers": [],
                })
            })
        })
        .collect()
}

/// What an open native session may carry as `mcpServers`.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionMcpResolution {
    /// The composed wire values to send.
    Supplied(Vec<Value>),
    /// The connection protocol accepts no per-session MCP servers. `reason`
    /// names the protocol; the open proceeds without servers rather than
    /// pretending to supply them.
    UnsupportedByProtocol { reason: String },
    /// The protocol supports MCP servers but no `tool-protocol` capsules are
    /// composed into this context.
    NotComposed,
}

/// Gate the composed tool surface on the negotiated connection capability.
///
/// A protocol whose capabilities advertise no MCP-server support (pi-rpc, or an
/// ACP agent without `agentCapabilities.mcpCapabilities`) yields
/// [`SessionMcpResolution::UnsupportedByProtocol`] naming the protocol — never
/// an empty pretend-supply.
pub fn session_mcp_resolution(
    protocol: &str,
    protocol_supports_mcp: bool,
    entries: impl IntoIterator<Item = ToolSourceEntry>,
) -> SessionMcpResolution {
    if !protocol_supports_mcp {
        return SessionMcpResolution::UnsupportedByProtocol {
            reason: format!(
                "the {protocol} connection protocol accepts no per-session MCP servers; \
                 the composed tool capsules are not carried into this session"
            ),
        };
    }
    let values = session_mcp_wire_values(entries);
    if values.is_empty() {
        return SessionMcpResolution::NotComposed;
    }
    SessionMcpResolution::Supplied(values)
}

/// Build the `SessionOpenRequest` an encounter opens with.
///
/// An [`SessionMcpResolution::UnsupportedByProtocol`] or
/// [`SessionMcpResolution::NotComposed`] resolution leaves `mcp_servers` empty,
/// which is the honest wire form for both.
pub fn build_session_open_request(
    mode: SessionOpenMode,
    native_session_id: Option<String>,
    cwd: &str,
    mcp: SessionMcpResolution,
    agent_session: Option<ResourceRef>,
) -> SessionOpenRequest {
    SessionOpenRequest {
        mode,
        native_session_id,
        cwd: cwd.to_owned(),
        additional_directories: Vec::new(),
        mcp_servers: match mcp {
            SessionMcpResolution::Supplied(values) => values,
            SessionMcpResolution::UnsupportedByProtocol { .. }
            | SessionMcpResolution::NotComposed => Vec::new(),
        },
        agent_session,
    }
}

/// Resolve the currently active `tool-protocol` capsules for the context at
/// `cwd`, through the one application engine — the same resolution a human's
/// `aikit status` sees.
pub fn active_tool_source_entries(home: &AikitHome, cwd: &Path) -> Result<Vec<ToolSourceEntry>> {
    let service = crate::app::Service::open(home.clone(), cwd, |key| std::env::var(key).ok())?;
    tool_source_entries_from_service(&service)
}

/// Extract wire-ready tool entries from an already-resolved application view.
///
/// Every active `tool-protocol` capsule contributes one entry under its export
/// name. Two capsules claiming one export name are refused: a wire array with
/// duplicate names cannot be honoured distinctly.
pub fn tool_source_entries_from_service(
    service: &crate::app::Service,
) -> Result<Vec<ToolSourceEntry>> {
    use aikit_core::capsule::Kind;
    use aikit_core::catalog::Catalog;

    let mut claimed: BTreeMap<String, ResourceRef> = BTreeMap::new();
    let mut entries = Vec::new();
    for capability in service.resolved().active_of_kind(Kind::ToolProtocol) {
        let id = &capability.id;
        let capsule = service.snapshot().get(id).ok_or_else(|| {
            AikitError::new(
                "encounter.mcp_source_unavailable",
                format!(
                    "active tool capsule {id} is not in the loaded catalogue; refresh the AIKit \
                     home before opening an encounter"
                ),
            )
            .with("capsule", id.to_string())
        })?;
        let Some(section) = capsule.tool_protocol() else {
            return Err(AikitError::new(
                "encounter.mcp_source_unavailable",
                format!(
                    "active tool capsule {id} carries no [tool-protocol] section, so there is no \
                     server record to compose; fix or remove the capsule"
                ),
            )
            .with("capsule", id.to_string()));
        };
        // Capsule validation forbids both or neither of command/url at parse;
        // this guard keeps the encounter wire honest even if that law changes.
        if section.server.command.is_none() && section.server.url.is_none()
            || section.server.command.is_some() && section.server.url.is_some()
        {
            return Err(AikitError::new(
                "encounter.mcp_unrepresentable",
                format!(
                    "tool capsule {id} carries a server record that is neither launchable nor \
                     remote; set exactly one of the capsule's [tool-protocol.server] command or url"
                ),
            )
            .with("capsule", id.to_string()));
        }
        let export_name = section
            .export_name
            .clone()
            .unwrap_or_else(|| id.leaf().to_string());
        let capsule_ref = ResourceRef::parse(id.to_string())?;
        if let Some(previous) = claimed.insert(export_name.clone(), capsule_ref.clone()) {
            return Err(AikitError::new(
                "encounter.mcp_export_collision",
                format!(
                    "tool capsules {previous} and {capsule_ref} both export an MCP server named \
                     {export_name}; the session wire cannot carry both — rename one capsule's \
                     [tool-protocol] export_name"
                ),
            )
            .with("export_name", export_name)
            .with("capsule", capsule_ref.to_string())
            .with("other_capsule", previous.to_string()));
        }
        entries.push(ToolSourceEntry {
            export_name,
            server: section.server.clone(),
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aikit_adapters::agent_connection::SessionOpenMode;
    use aikit_adapters::tool_sources::ToolSourceEntry;
    use aikit_adapters::{AcpV1ConnectionAdapter, AgentConnectionAdapter};
    use aikit_core::capsule::ToolServerRecord;
    use aikit_core::{CapsuleId, ResourceRef, TrustKey, TrustState};
    use aikit_store::trust::TrustStore;
    use aikit_store::AikitHome;
    use serde_json::{json, Value};
    use tempfile::TempDir;

    use super::*;

    fn stdio_entry() -> ToolSourceEntry {
        ToolSourceEntry {
            export_name: "bimba".to_string(),
            server: ToolServerRecord {
                command: Some("/opt/bimba/bimba-mcp".to_string()),
                args: vec!["--port".to_string(), "8080".to_string()],
                env: BTreeMap::from([("BIMBA_TOKEN".to_string(), "sk-test".to_string())]),
                cwd: Some("/opt/bimba".to_string()),
                url: None,
            },
        }
    }

    fn bare_stdio_entry() -> ToolSourceEntry {
        ToolSourceEntry {
            export_name: "fs-docs".to_string(),
            server: ToolServerRecord {
                command: Some("npx".to_string()),
                args: vec!["-y".to_string()],
                env: BTreeMap::new(),
                cwd: None,
                url: None,
            },
        }
    }

    fn url_entry() -> ToolSourceEntry {
        ToolSourceEntry {
            export_name: "linear".to_string(),
            server: ToolServerRecord {
                command: None,
                args: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                url: Some("https://mcp.example/sse".to_string()),
            },
        }
    }

    fn unsupported(protocol: &str) -> SessionMcpResolution {
        session_mcp_resolution(protocol, false, Vec::new())
    }

    // -- wire shaping -------------------------------------------------------

    #[test]
    fn a_stdio_record_shapes_into_the_acp_stdio_entry_with_env_as_name_value_pairs() {
        let values = session_mcp_wire_values([stdio_entry()]);

        assert_eq!(
            values,
            vec![json!({
                "name": "bimba",
                "command": "/opt/bimba/bimba-mcp",
                "args": ["--port", "8080"],
                "env": [{"name": "BIMBA_TOKEN", "value": "sk-test"}],
            })],
            "the record's cwd has no ACP stdio field and must not appear on the wire"
        );
    }

    #[test]
    fn a_stdio_record_without_env_still_carries_the_required_empty_env_array() {
        let values = session_mcp_wire_values([bare_stdio_entry()]);

        assert_eq!(
            values,
            vec![json!({
                "name": "fs-docs",
                "command": "npx",
                "args": ["-y"],
                "env": [],
            })],
            "args and env are required ACP stdio fields even when empty"
        );
    }

    #[test]
    fn a_url_record_shapes_into_the_acp_http_entry_with_its_type_discriminator() {
        let values = session_mcp_wire_values([url_entry()]);

        assert_eq!(
            values,
            vec![json!({
                "type": "http",
                "name": "linear",
                "url": "https://mcp.example/sse",
                "headers": [],
            })],
            "the http entry carries the type discriminator and the required headers array"
        );
    }

    #[test]
    fn an_empty_entry_set_shapes_an_empty_wire_array() {
        assert_eq!(session_mcp_wire_values(Vec::new()), Vec::<Value>::new());
    }

    // -- capability gating --------------------------------------------------

    #[test]
    fn a_capability_bearing_protocol_supplies_the_shaped_wire_values() {
        let resolution = session_mcp_resolution("acp", true, [stdio_entry(), url_entry()]);

        assert_eq!(
            resolution,
            SessionMcpResolution::Supplied(session_mcp_wire_values([stdio_entry(), url_entry()])),
        );
    }

    #[test]
    fn a_protocol_without_mcp_capability_yields_unsupported_naming_the_protocol_and_never_a_supply()
    {
        let resolution = session_mcp_resolution("pi-rpc", false, [stdio_entry()]);

        let SessionMcpResolution::UnsupportedByProtocol { reason } = &resolution else {
            panic!("a protocol without MCP capability must not pretend to supply: {resolution:?}");
        };
        assert!(
            reason.contains("pi-rpc"),
            "the reason names the protocol: {reason}"
        );
    }

    #[test]
    fn a_supporting_protocol_with_nothing_composed_is_reported_as_not_composed() {
        let resolution = session_mcp_resolution("acp", true, Vec::new());

        assert_eq!(resolution, SessionMcpResolution::NotComposed);
    }

    // -- the open request ---------------------------------------------------

    #[test]
    fn a_supplied_resolution_carries_mcp_servers_onto_the_session_open_request() {
        let supplied = session_mcp_wire_values([stdio_entry()]);
        let request = build_session_open_request(
            SessionOpenMode::Create,
            None,
            "/workspace/project",
            SessionMcpResolution::Supplied(supplied.clone()),
            Some(ResourceRef::parse("agent-session/test").unwrap()),
        );

        assert_eq!(request.mcp_servers, supplied);
        assert_eq!(request.cwd, "/workspace/project");
        assert_eq!(
            serde_json::to_value(&request).unwrap()["mcp_servers"],
            serde_json::to_value(&supplied).unwrap(),
            "the servers survive request serialisation; the adapter names the field mcpServers"
        );
    }

    #[test]
    fn an_unsupported_or_uncomposed_resolution_leaves_the_session_open_request_without_servers() {
        for mcp in [unsupported("pi-rpc"), SessionMcpResolution::NotComposed] {
            let request = build_session_open_request(
                SessionOpenMode::Create,
                None,
                "/workspace/project",
                mcp,
                None,
            );

            assert!(
                request.mcp_servers.is_empty(),
                "no resolution other than Supplied may put servers on the request"
            );
            assert_eq!(
                serde_json::to_value(&request).unwrap()["mcp_servers"],
                json!([]),
            );
        }
    }

    #[test]
    fn a_supplied_resolution_rides_the_acp_session_new_payload_verbatim() {
        let mut adapter =
            AcpV1ConnectionAdapter::new(ResourceRef::parse("connection/acp/test").unwrap(), vec![]);
        let init = adapter.initialize().unwrap();
        adapter
            .ingest(json!({
                "jsonrpc": "2.0",
                "id": init.payload["id"],
                "result": {
                    "protocolVersion": 1,
                    "agentCapabilities": { "mcpCapabilities": {} }
                }
            }))
            .unwrap();

        let supplied = session_mcp_wire_values([stdio_entry()]);
        let command = adapter
            .open_session(build_session_open_request(
                SessionOpenMode::Create,
                None,
                "/workspace/project",
                SessionMcpResolution::Supplied(supplied.clone()),
                Some(ResourceRef::parse("agent-session/test").unwrap()),
            ))
            .unwrap();

        assert_eq!(command.operation, "session/new");
        assert_eq!(command.payload["params"]["mcpServers"], json!(supplied));
    }

    // -- resolution from a real home ----------------------------------------

    const SOURCE: &str = "test";

    fn bimba_manifest() -> &'static str {
        "schema = 1\n\
         id = \"tool-protocol/test/bimba\"\n\
         kind = \"tool-protocol\"\n\
         name = \"Bimba test server\"\n\
         description = \"A real tool-protocol capsule for the encounter MCP composition test\"\n\
         \n\
         [tool-protocol]\n\
         export_name = \"bimba\"\n\
         \n\
         [tool-protocol.server]\n\
         command = \"/usr/bin/true\"\n\
         args = [\"--port\", \"8080\"]\n\
         \n\
         [tool-protocol.server.env]\n\
         BIMBA_TOKEN = \"sk-test\"\n"
    }

    fn linear_manifest() -> &'static str {
        "schema = 1\n\
         id = \"tool-protocol/test/linear\"\n\
         kind = \"tool-protocol\"\n\
         name = \"Linear test server\"\n\
         description = \"A remote tool-protocol capsule for the encounter MCP composition test\"\n\
         \n\
         [tool-protocol.server]\n\
         url = \"https://mcp.example/sse\"\n"
    }

    /// A real AIKit home on disk with real capsule manifests under a real
    /// registry, a real scope profile enabling `enabled`, and a real trusted
    /// trust record — then the entries resolved out of that home.
    fn resolved_entries(
        manifests: &[&str],
        id_paths: &[&str],
        enable: &[&str],
        trust: bool,
    ) -> aikit_core::Result<Vec<ToolSourceEntry>> {
        let tmp = TempDir::new().unwrap();
        let home = AikitHome::at(tmp.path().join("home"));
        home.ensure_layout().unwrap();
        for (manifest, id_path) in manifests.iter().zip(id_paths) {
            let dir = home.registry(SOURCE).join("capsules").join(id_path);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
        }
        let mut profile = String::from("schema = 1\n");
        if !enable.is_empty() {
            profile.push_str(&format!(
                "enable = [{}]\n",
                enable
                    .iter()
                    .map(|id| format!("\"{id}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        std::fs::create_dir_all(home.root().join("scopes/global")).unwrap();
        std::fs::write(home.global_profile(), profile).unwrap();

        let cwd = tmp.path().join("world");
        std::fs::create_dir_all(&cwd).unwrap();
        let mut service = crate::app::Service::open(home, &cwd, |_: &str| None).unwrap();
        if trust {
            {
                let store = TrustStore::new(service.index());
                for id_path in id_paths {
                    let id = CapsuleId::parse(id_path).unwrap();
                    let capsule = {
                        use aikit_core::catalog::Catalog;
                        service
                            .snapshot()
                            .get(&id)
                            .unwrap_or_else(|| panic!("the registry loaded {id_path}"))
                            .clone()
                    };
                    store
                        .record(
                            &TrustKey::new(
                                capsule.source.clone().unwrap(),
                                id,
                                capsule.revision.clone().unwrap(),
                            ),
                            TrustState::Trusted,
                            Some("encounter mcp composition test review"),
                        )
                        .unwrap();
                }
            }
            service.refresh().unwrap();
        }
        tool_source_entries_from_service(&service)
    }

    #[test]
    fn the_active_trusted_tool_protocol_capsules_of_a_home_resolve_as_wire_ready_entries() {
        let entries = resolved_entries(
            &[bimba_manifest(), linear_manifest()],
            &["tool-protocol/test/bimba", "tool-protocol/test/linear"],
            &["tool-protocol/test/bimba", "tool-protocol/test/linear"],
            true,
        )
        .unwrap();

        assert_eq!(
            entries,
            vec![
                ToolSourceEntry {
                    export_name: "bimba".to_string(),
                    server: ToolServerRecord {
                        command: Some("/usr/bin/true".to_string()),
                        args: vec!["--port".to_string(), "8080".to_string()],
                        env: BTreeMap::from([("BIMBA_TOKEN".to_string(), "sk-test".to_string())]),
                        cwd: None,
                        url: None,
                    },
                },
                ToolSourceEntry {
                    export_name: "linear".to_string(),
                    server: ToolServerRecord {
                        command: None,
                        args: Vec::new(),
                        env: BTreeMap::new(),
                        cwd: None,
                        url: Some("https://mcp.example/sse".to_string()),
                    },
                },
            ],
            "entries come out ordered by capsule id, wire-ready"
        );
        let values = session_mcp_wire_values(entries);
        assert_eq!(values.len(), 2, "every resolved entry reaches the wire");
    }

    #[test]
    fn an_untrusted_tool_protocol_capsule_stays_uncomposed() {
        let entries = resolved_entries(
            &[bimba_manifest()],
            &["tool-protocol/test/bimba"],
            &["tool-protocol/test/bimba"],
            false,
        )
        .unwrap();

        assert!(
            entries.is_empty(),
            "trust is never self-declared; an unreviewed tool capsule is not supplied: {entries:?}"
        );
    }

    #[test]
    fn a_tool_protocol_capsule_not_enabled_in_any_scope_stays_uncomposed() {
        let entries = resolved_entries(
            &[bimba_manifest()],
            &["tool-protocol/test/bimba"],
            &[],
            true,
        )
        .unwrap();

        assert!(
            entries.is_empty(),
            "activation is deliberate; a trusted but never enabled tool capsule is not supplied: {entries:?}"
        );
    }

    #[test]
    fn two_capsules_claiming_one_export_name_refuse_rather_than_collide_on_the_wire() {
        let colliding = "schema = 1\n\
         id = \"tool-protocol/test/other\"\n\
         kind = \"tool-protocol\"\n\
         name = \"Other test server\"\n\
         description = \"A second capsule claiming the same export name\"\n\
         \n\
         [tool-protocol]\n\
         export_name = \"bimba\"\n\
         \n\
         [tool-protocol.server]\n\
         command = \"/usr/bin/false\"\n";

        let error = resolved_entries(
            &[bimba_manifest(), colliding],
            &["tool-protocol/test/bimba", "tool-protocol/test/other"],
            &["tool-protocol/test/bimba", "tool-protocol/test/other"],
            true,
        )
        .unwrap_err();

        assert_eq!(error.code(), "encounter.mcp_export_collision");
        assert!(
            error.to_string().contains("bimba"),
            "the refusal names the contested export name: {error}"
        );
    }
}
