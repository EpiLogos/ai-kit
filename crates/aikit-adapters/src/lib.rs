//! AIKit adapters: providers, multiplexers, agent clients, composable harnesses and shells.

#![forbid(unsafe_code)]

extern crate self as aikit_adapters;

pub mod actuation_stream_projection;
pub mod agent_connection;
pub mod authored_wiki_living;
pub mod authored_wiki_read;
pub mod authored_wiki_source;
pub mod bkmr;
pub mod clients;
pub mod composition_topology;
pub mod connection_process;
pub mod credential_provider;
pub mod deepseek_harness;
pub mod deepseek_live;
pub mod deepseek_maximal;
pub mod factory_run_thought_authored_wiki;
pub mod flow_authored_wiki;
pub mod gateway_connector;
#[allow(unused_imports)]
pub mod gateway_runtime;
pub mod gateway_service;
pub mod gitnexus;
pub mod herdr;
pub mod hyprland;
pub mod interactive_connection;
pub mod local_source_discovery;
pub mod mux;
pub mod native_git;
pub mod okf;
pub mod projectcentral;
pub mod projectcentral_authored_wiki;
pub mod runner;
pub mod session_space_connection;
pub mod session_space_observation;
pub mod session_space_reconstruction;
pub mod shells;
mod telegram_bot_api;
pub mod telegram_gateway;
pub mod working_environment;
pub mod working_environment_control;

pub use actuation_stream_projection::{
    project_connection_signal_to_actuation_stream, ActuationStreamAppendProjection,
    ActuationStreamProjectionContext, ACTUATION_STREAM_OWNER_REVISION, ACTUATION_STREAM_SCHEMA,
    CONNECTION_SIGNAL_STREAM_PROJECTION_VERSION,
};
pub use agent_connection::{
    AcpV1ConnectionAdapter, AgentConnectionAdapter, CancelRequest, ClassicProcessConnectionAdapter,
    ConnectionCapabilities, ConnectionCommand, ConnectionDegradation, ConnectionDescriptor,
    ConnectionProtocol, ConnectionProtocolFamily, ConnectionSignal, ConnectionSignalKind,
    ConnectionState, NativePermissionChoice, NativePermissionRequest, NativeSessionBinding,
    PromptRequest, SessionOpenMode, SessionOpenRequest, ACP_STABLE_PROTOCOL_VERSION,
    AGENT_CONNECTION_ADAPTER_VERSION,
};
pub use authored_wiki_living::{authored_wiki_knowledge_impact, AUTHORED_WIKI_LIVING_VERSION};
pub use authored_wiki_read::{
    authored_wiki_subject_relations, AuthoredWikiSubjectRelations, AUTHORED_WIKI_READ_VERSION,
};
pub use authored_wiki_source::{
    authored_relation_dependencies, compile_authored_wiki_relations, parse_authored_wiki_source,
    parse_authored_wiki_source_with_authority, rebuild_semantic_wiki_with_authored_relations,
    AuthoredWikiRelationCompilation, AuthoredWikiSourceProjection, PendingAuthoredRelation,
    AUTHORED_WIKI_SOURCE_VERSION,
};
pub use composition_topology::{
    resolve_component_topology, ComponentContainment, HarnessCompositionTopology,
    HARNESS_COMPOSITION_TOPOLOGY_VERSION,
};
pub use connection_process::ConnectionProcess;
#[cfg(target_os = "linux")]
pub use credential_provider::LinuxEncryptedFallbackProvider;
pub use credential_provider::{
    EnvironmentImportProvider, NativeSecureStoreProvider, NativeSecureStoreStatus,
};
pub use deepseek_harness::{
    deepseek_harness_conformance, DeepSeekHarnessConformance, DeepSeekShellProvider,
    DEEPSEEK_HARNESS_RELEASE, DEEPSEEK_HARNESS_UPSTREAM_REVISION,
};
pub use deepseek_live::{
    deepseek_live_cordis_composition, CordisActivationGrant, CordisActivationOperation,
    CordisProcessActivationDriver, CordisProcessSpec, DeepSeekLiveComposition,
    DEEPSEEK_CORDIS_WEB_PORT, DEEPSEEK_LIVE_CORDIS_COMPONENTS,
};
pub use deepseek_maximal::{
    deepseek_maximal_conformance, DeepSeekMaximalConformance, DEEPSEEK_CORDIS_REVISION,
};
pub use factory_run_thought_authored_wiki::{
    factory_run_thought_authored_wiki, FactoryBuildCognitiveProvenance,
    FactoryBuildCognitiveSnapshot, FactoryBuildCognitiveView, FactoryRunThought,
    FactoryRunThoughtAuthoredWiki, FactoryRunThoughtAuthoredWikiStatus, FactoryRunThoughtPassage,
    FactoryRunThoughtProducer, FactoryRunThoughtProjection, FactoryRunThoughtSourceDisclosure,
    FACTORY_BUILD_COGNITIVE_PROVIDER_CONTRACT, FACTORY_BUILD_COGNITIVE_VIEW_CONTRACT,
    FACTORY_RUN_THOUGHT_AUTHORED_WIKI_VERSION,
};
pub use flow_authored_wiki::{standing_flow_authored_wiki_source, FLOW_AUTHORED_WIKI_VERSION};
pub use gateway_connector::{
    verify_connector_descriptor, ConnectorCapabilities, ConnectorConformance,
    ConnectorConnectionState, ConnectorDescriptor, ConnectorFuture, ConnectorHealth,
    ConnectorHello, ConnectorOperation, ConnectorWireFrame, ConversationAddress, DeliveryReceipt,
    DeliveryState, GatewayConnector, InboundEvent, InboundEventKind, MediaReference,
    OutboundOperation, OutboundOperationKind, SenderIdentity, SenderKind,
    GATEWAY_CONNECTOR_SCHEMA_PATH, GATEWAY_CONNECTOR_SDK_VERSION, GATEWAY_CONNECTOR_WIRE_VERSION,
};
pub use gateway_runtime::{
    connector_descriptor, execute_gateway_command, text_send, AgencyGateway,
    GatewayActuationControlIntent, GatewayActuationControlOperation, GatewayBinding,
    GatewayCommand, GatewayDiscovery, GatewayErrorEnvelope, GatewayIngressDecision,
    GatewayIngressPolicy, GatewayIngressResult, GatewayReplay, GatewayRequestEnvelope,
    GatewayResponse, GatewayResponseEnvelope, GatewaySnapshot, GatewayStatus, GatewayStreamEvent,
    GatewayStreamJournal, ACTUATION_STREAM_SCHEMA as GATEWAY_ACTUATION_STREAM_SCHEMA,
    AGENCY_GATEWAY_VERSION,
};
pub use gateway_service::{
    persist_gateway_state, restore_gateway_state, run_gateway_service, GatewayServiceConfig,
    DEFAULT_GATEWAY_MAX_FRAME_BYTES, GATEWAY_SERVICE_CARRIER_VERSION,
};
pub use herdr::{
    parse_herdr_snapshot, HerdrAgentObservation, HerdrAgentStatus, HerdrSnapshot,
    HerdrWorkingEnvironment, HERDR_PROVIDER_VERSION, HERDR_UPSTREAM_REVISION,
};
pub use hyprland::{
    parse_hyprland_clients, HyprlandWindowObservation, HyprlandWorkingEnvironment,
    HYPRLAND_PROVIDER_VERSION, HYPRLAND_UPSTREAM_REVISION,
};
pub use interactive_connection::{
    AcpStableConnectionAdapter, AcpStableSessionCapabilities, InteractiveAgentConnectionAdapter,
    PermissionDecision,
};
pub use local_source_discovery::{
    discover_local_sources, DiscoveredLocalSource, LocalSourceDiscovery,
    LocalSourceDiscoveryLimits, NativeSourceRelation, LOCAL_SOURCE_DISCOVERY_VERSION,
};
pub use native_git::{NativeGitProvider, NATIVE_GIT_PROVIDER_REF, NATIVE_GIT_PROVIDER_VERSION};
pub use okf::{parse_authored_markdown_relations, parse_okf_markdown, render_okf_markdown};
pub use projectcentral::{ProjectCentralFileProvider, ProjectCentralFilesystemBinding};
pub use projectcentral_authored_wiki::{
    projectcentral_authored_wiki, ProjectCentralAuthoredWiki, ProjectCentralAuthoredWikiStatus,
    PROJECTCENTRAL_AUTHORED_WIKI_VERSION,
};
pub use session_space_connection::connection_into_session_space;
pub use session_space_observation::{
    SessionSpaceFileObservationProvider, SessionSpaceObservationError,
    SESSION_SPACE_OBSERVATION_FILE_VERSION,
};
pub use session_space_reconstruction::session_space_native_observations;
pub use telegram_gateway::{
    TelegramBotApiTransport, TelegramBotIdentity, TelegramConnector, TelegramConnectorConfig,
    TELEGRAM_BOT_API_BASE, TELEGRAM_GATEWAY_CONNECTOR_VERSION,
};
pub use working_environment::{
    MuxSessionSpaceActivationDriver, MuxWorkingEnvironment, NativeBindingKind,
    ProviderNativeBinding, WorkingEnvironmentCapabilities, WorkingEnvironmentHealth,
    WorkingEnvironmentObservation, WorkingEnvironmentProvider,
    WORKING_ENVIRONMENT_PROVIDER_VERSION,
};
pub use working_environment_control::{
    AgentSessionSurfaceBinding, AgentSessionWorkingEnvironmentProvider,
    WorkingEnvironmentControlClient, WORKING_ENVIRONMENT_CONTROL_VERSION,
};
