//! AIKit adapters: providers, multiplexers, agent clients, composable harnesses and shells.

#![forbid(unsafe_code)]

pub mod agent_connection;
pub mod bkmr;
pub mod clients;
pub mod composition_topology;
pub mod connection_process;
pub mod deepseek_harness;
pub mod deepseek_live;
pub mod deepseek_maximal;
pub mod gitnexus;
pub mod interactive_connection;
pub mod local_source_discovery;
pub mod mux;
pub mod okf;
pub mod projectcentral;
pub mod runner;
pub mod session_space_connection;
pub mod session_space_observation;
pub mod session_space_reconstruction;
pub mod shells;
pub mod working_environment;
pub mod working_environment_control;

pub use agent_connection::{
    AcpV1ConnectionAdapter, AgentConnectionAdapter, CancelRequest,
    ClassicProcessConnectionAdapter, ConnectionCapabilities, ConnectionCommand,
    ConnectionDegradation, ConnectionDescriptor, ConnectionProtocol, ConnectionProtocolFamily,
    ConnectionSignal, ConnectionSignalKind, ConnectionState, NativePermissionChoice,
    NativePermissionRequest, NativeSessionBinding, PromptRequest, SessionOpenMode,
    SessionOpenRequest, ACP_STABLE_PROTOCOL_VERSION, AGENT_CONNECTION_ADAPTER_VERSION,
};
pub use composition_topology::{
    resolve_component_topology, ComponentContainment, HarnessCompositionTopology,
    HARNESS_COMPOSITION_TOPOLOGY_VERSION,
};
pub use connection_process::ConnectionProcess;
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
pub use interactive_connection::{
    AcpStableConnectionAdapter, AcpStableSessionCapabilities, InteractiveAgentConnectionAdapter,
    PermissionDecision,
};
pub use local_source_discovery::{
    discover_local_sources, DiscoveredLocalSource, LocalSourceDiscovery,
    LocalSourceDiscoveryLimits, NativeSourceRelation, LOCAL_SOURCE_DISCOVERY_VERSION,
};
pub use okf::{parse_authored_markdown_relations, parse_okf_markdown, render_okf_markdown};
pub use projectcentral::{ProjectCentralFileProvider, ProjectCentralFilesystemBinding};
pub use session_space_connection::connection_into_session_space;
pub use session_space_observation::{
    SessionSpaceFileObservationProvider, SessionSpaceObservationError,
    SESSION_SPACE_OBSERVATION_FILE_VERSION,
};
pub use session_space_reconstruction::session_space_native_observations;
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
