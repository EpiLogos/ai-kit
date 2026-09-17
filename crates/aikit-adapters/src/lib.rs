//! AIKit adapters: providers, multiplexers, agent clients, composable harnesses and shells.

#![forbid(unsafe_code)]

extern crate self as aikit_adapters;

pub mod actor_composition;
pub mod actuation_harness_capability;
pub mod actuation_harness_detection;
pub mod actuation_instantiation;
pub mod actuation_model_routes;
pub mod actuation_stream_projection;
pub mod agent_connection;
pub mod agent_session_host;
pub mod authored_wiki_living;
pub mod authored_wiki_read;
pub mod authored_wiki_source;
pub mod bkmr;
pub mod central_agent_profile;
pub mod central_development_field;
pub mod central_temporal;
pub mod clients;
pub mod composition_topology;
pub mod connection_process;
pub mod credential_provider;
pub mod deepseek_harness;
pub mod deepseek_live;
pub mod deepseek_maximal;
pub mod factory_developmental;
pub mod factory_run_thought_authored_wiki;
pub mod flow_authored_wiki;
pub mod gateway_client;
pub mod gateway_connector;
#[allow(unused_imports)]
pub mod gateway_runtime;
pub mod gateway_service;
pub mod gitnexus;
pub mod harness_disclosure;
pub mod herdr;
pub mod home_agent_profile;
pub mod hyprland;
pub mod interactive_connection;
pub mod layers;
pub mod profiles;
pub mod local_source_discovery;
pub mod model_realisation;
pub mod mux;
pub mod native_git;
pub mod openai_realtime;
pub mod okf;
pub mod place_technology;
pub mod projectcentral;
pub mod projectcentral_authored_wiki;
pub mod provider_catalog_source;
pub mod ql_provider;
pub mod runner;
pub mod secret_resolver;
pub mod session_space_connection;
pub mod session_space_observation;
pub mod session_space_reconstruction;
pub mod shells;
mod telegram_bot_api;
pub mod telegram_gateway;
pub mod tool_sources;
pub mod workcell_instance_intake;
pub mod working_environment;
pub mod working_environment_control;

pub use actuation_stream_projection::{
    ACTUATION_STREAM_OWNER_REVISION, ACTUATION_STREAM_SCHEMA, ActuationStreamAppendProjection,
    ActuationStreamProjectionContext, CONNECTION_SIGNAL_STREAM_PROJECTION_VERSION,
    project_connection_signal_to_actuation_stream,
};
pub use agent_connection::{
    ACP_STABLE_PROTOCOL_VERSION, AGENT_CONNECTION_ADAPTER_VERSION, AcpV1ConnectionAdapter,
    AgentConnectionAdapter, CancelRequest, ClassicProcessConnectionAdapter, ConnectionCapabilities,
    ConnectionCommand, ConnectionDegradation, ConnectionDescriptor, ConnectionProtocol,
    ConnectionProtocolFamily, ConnectionSignal, ConnectionSignalKind, ConnectionState,
    NativePermissionChoice, NativePermissionRequest, NativeSessionBinding, PromptRequest,
    SessionOpenMode, SessionOpenRequest,
};
pub use agent_session_host::{
    AGENT_SESSION_HOST_VERSION, AgentSessionHost, AgentSessionHostLimits,
    DEFAULT_MAX_SIGNALS_PER_TURN, HostEvent, InterruptOrigin, InterruptReceipt, SessionIdentity,
    SessionLane, SessionLaneState, TurnHandle, TurnInterruption, TurnRecord, TurnStop, WaitOutcome,
};
pub use authored_wiki_living::{AUTHORED_WIKI_LIVING_VERSION, authored_wiki_knowledge_impact};
pub use authored_wiki_read::{
    AUTHORED_WIKI_READ_VERSION, AuthoredWikiSubjectRelations, authored_wiki_subject_relations,
};
pub use authored_wiki_source::{
    AUTHORED_WIKI_SOURCE_VERSION, AuthoredWikiRelationCompilation, AuthoredWikiSourceProjection,
    PendingAuthoredRelation, authored_relation_dependencies, compile_authored_wiki_relations,
    parse_authored_wiki_source, parse_authored_wiki_source_with_authority,
    rebuild_semantic_wiki_with_authored_relations,
};
pub use composition_topology::{
    ComponentContainment, HARNESS_COMPOSITION_TOPOLOGY_VERSION, HarnessCompositionTopology,
    resolve_component_topology,
};
pub use connection_process::ConnectionProcess;
#[cfg(target_os = "linux")]
pub use credential_provider::LinuxEncryptedFallbackProvider;
pub use credential_provider::{
    EnvironmentImportProvider, NativeSecureStoreProvider, NativeSecureStoreStatus,
};
pub use deepseek_harness::{
    DEEPSEEK_HARNESS_RELEASE, DEEPSEEK_HARNESS_UPSTREAM_REVISION, DeepSeekHarnessConformance,
    DeepSeekShellProvider, deepseek_harness_conformance,
};
pub use deepseek_live::{
    CordisActivationGrant, CordisActivationOperation, CordisProcessActivationDriver,
    CordisProcessSpec, DEEPSEEK_CORDIS_WEB_PORT, DEEPSEEK_LIVE_CORDIS_COMPONENTS,
    DeepSeekLiveComposition, deepseek_live_cordis_composition,
};
pub use deepseek_maximal::{
    DEEPSEEK_CORDIS_REVISION, DeepSeekMaximalConformance, deepseek_maximal_conformance,
};
pub use factory_run_thought_authored_wiki::{
    FACTORY_BUILD_COGNITIVE_PROVIDER_CONTRACT, FACTORY_BUILD_COGNITIVE_VIEW_CONTRACT,
    FACTORY_RUN_THOUGHT_AUTHORED_WIKI_VERSION, FactoryBuildCognitiveProvenance,
    FactoryBuildCognitiveSnapshot, FactoryBuildCognitiveView, FactoryRunThought,
    FactoryRunThoughtAuthoredWiki, FactoryRunThoughtAuthoredWikiStatus, FactoryRunThoughtPassage,
    FactoryRunThoughtProducer, FactoryRunThoughtProjection, FactoryRunThoughtSourceDisclosure,
    factory_run_thought_authored_wiki,
};
pub use flow_authored_wiki::{FLOW_AUTHORED_WIKI_VERSION, standing_flow_authored_wiki_source};
pub use gateway_client::{
    GATEWAY_CLIENT_VERSION, GatewayCarrierTarget, gateway_command, gateway_request,
};
pub use gateway_connector::{
    ConnectorCapabilities, ConnectorConformance, ConnectorConnectionState, ConnectorDescriptor,
    ConnectorFuture, ConnectorHealth, ConnectorHello, ConnectorOperation, ConnectorWireFrame,
    ConversationAddress, DeliveryReceipt, DeliveryState, GATEWAY_CONNECTOR_SCHEMA_PATH,
    GATEWAY_CONNECTOR_SDK_VERSION, GATEWAY_CONNECTOR_WIRE_VERSION, GatewayConnector, InboundEvent,
    InboundEventKind, MediaReference, OutboundOperation, OutboundOperationKind, SenderIdentity,
    SenderKind, verify_connector_descriptor,
};
pub use gateway_runtime::{
    ACTUATION_STREAM_SCHEMA as GATEWAY_ACTUATION_STREAM_SCHEMA, AGENCY_GATEWAY_VERSION,
    AgencyGateway, GATEWAY_ECOLOGY_AUTHORITY_LAW, GATEWAY_INVOCATION_MODES,
    GatewayActuationControlIntent, GatewayActuationControlOperation, GatewayBinding,
    GatewayCommand, GatewayDiscovery, GatewayEcology, GatewayEcologyAgency, GatewayEcologySession,
    GatewayEcologyStream, GatewayEcologySurface, GatewayErrorEnvelope, GatewayForkOrigin,
    GatewayIngressDecision, GatewayIngressPolicy, GatewayIngressResult, GatewayInvocationMode,
    GatewayReplay, GatewayRequestEnvelope, GatewayResponse, GatewayResponseEnvelope,
    GatewaySnapshot, GatewayStatus, GatewayStreamEvent, GatewayStreamJournal, connector_descriptor,
    execute_gateway_command, text_send,
};
pub use gateway_service::{
    DEFAULT_GATEWAY_MAX_FRAME_BYTES, GATEWAY_SERVICE_CARRIER_VERSION, GatewayServiceConfig,
    persist_gateway_state, restore_gateway_state, run_gateway_service,
};
pub use harness_disclosure::{
    ComposedEntry, DriftEntry, DriftKind, HarnessDisclosure, NativeEntry, NativeObservation,
    disclose,
};
pub use herdr::{
    HERDR_PROVIDER_VERSION, HERDR_UPSTREAM_REVISION, HerdrAgentObservation, HerdrAgentStatus,
    HerdrSnapshot, HerdrWorkingEnvironment, parse_herdr_snapshot,
};
pub use hyprland::{
    HYPRLAND_PROVIDER_VERSION, HYPRLAND_UPSTREAM_REVISION, HyprlandWindowObservation,
    HyprlandWorkingEnvironment, parse_hyprland_clients,
};
pub use interactive_connection::{
    AcpStableConnectionAdapter, AcpStableSessionCapabilities, InteractiveAgentConnectionAdapter,
    PermissionDecision,
};
pub use layers::{
    LayerMergeError, MatcherPolicy, MergeArgs, MergeReport, apply_merge, claude_hook_map,
    mcp_servers_record, zcode_hook_wrapper,
};
pub use local_source_discovery::{
    DiscoveredLocalSource, LOCAL_SOURCE_DISCOVERY_VERSION, LocalSourceDiscovery,
    LocalSourceDiscoveryLimits, NativeSourceRelation, discover_local_sources,
};
pub use native_git::{NATIVE_GIT_PROVIDER_REF, NATIVE_GIT_PROVIDER_VERSION, NativeGitProvider};
pub use okf::{parse_authored_markdown_relations, parse_okf_markdown, render_okf_markdown};
pub use projectcentral::{ProjectCentralFileProvider, ProjectCentralFilesystemBinding};
pub use projectcentral_authored_wiki::{
    PROJECTCENTRAL_AUTHORED_WIKI_VERSION, ProjectCentralAuthoredWiki,
    ProjectCentralAuthoredWikiStatus, projectcentral_authored_wiki,
};
pub use ql_provider::{QL_CLI_PROVIDER_VERSION, QlCliClient, QlOperativeProvider};
pub use session_space_connection::connection_into_session_space;
pub use session_space_observation::{
    SESSION_SPACE_OBSERVATION_FILE_VERSION, SessionSpaceFileObservationProvider,
    SessionSpaceObservationError,
};
pub use session_space_reconstruction::session_space_native_observations;
pub use telegram_gateway::{
    TELEGRAM_BOT_API_BASE, TELEGRAM_GATEWAY_CONNECTOR_VERSION, TelegramBotApiTransport,
    TelegramBotIdentity, TelegramConnector, TelegramConnectorConfig,
};
pub use tool_sources::{
    TOOLS_PROJECTION_OWNERSHIP, ToolSourceEntry, ToolSourceError, ToolsProjectionOutcome,
    ToolsProjectionPlan, ToolServerRecord, plan_tools_projection,
};
pub use working_environment::{
    MuxSessionSpaceActivationDriver, MuxWorkingEnvironment, NativeBindingKind,
    ProviderNativeBinding, WORKING_ENVIRONMENT_PROVIDER_VERSION, WorkingEnvironmentCapabilities,
    WorkingEnvironmentHealth, WorkingEnvironmentObservation, WorkingEnvironmentProvider,
};
pub use working_environment_control::{
    AgentSessionSurfaceBinding, AgentSessionWorkingEnvironmentProvider,
    WORKING_ENVIRONMENT_CONTROL_VERSION, WorkingEnvironmentControlClient,
};

/// Pi native RPC connection; no ACP or permission parity is implied.
pub mod pi_rpc_connection;

pub mod capability_matrix;
pub mod central_entities;
pub mod oi_explore;
pub mod central_wiki;
pub mod central_world_sources;
mod session_event_queue;
pub mod techne_temporal;

pub mod agency_admission;
pub mod central_file_map;
pub mod central_placement;
pub mod placement_enforcement;
