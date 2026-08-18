//! AIKit core: a context-scoped capability router for agentic terminal work.
//!
//! The central object is not a global "active set" but an **effective capability
//! view resolved for a context**, where a context is roughly
//!
//! ```text
//! user + host + project scope chain + session space + task + target client
//! ```
//!
//! Everything else in AIKit — the registry, the palette, multiplexers,
//! integrations, the hook bank, the capture pipeline — is a view or a consumer of
//! that resolution.
//!
//! This crate is deliberately free of I/O. The resolver takes a catalog, a stack
//! of scope layers and an environment, and returns a resolved graph plus an
//! explanation and a deterministic hash. That is what makes it testable without a
//! filesystem, and what makes generations content-addressable.

#![forbid(unsafe_code)]

pub mod actor_bootstrap;
pub mod application_context;
pub mod arg;
pub mod capsule;
pub mod catalog;
pub mod composition;
pub mod composition_mutation;
pub mod composition_view;
pub mod composition_workspace;
pub mod context;
pub mod context_resolution;
pub mod context_source;
pub mod credential;
pub mod duration;
pub mod effects;
pub mod error;
pub mod familiarity;
pub mod frecency;
pub mod guidance;
pub mod hooks;
pub mod id;
pub mod knowledge;
pub mod knowledge_code;
pub mod knowledge_navigation;
pub mod knowledge_operations;
pub mod knowledge_okf;
pub mod knowledge_source_pool;
pub mod knowledge_wiki;
pub mod knowledge_wiki_index;
pub mod knowledge_wiki_provider;
pub mod lifecycle;
pub mod model_runtime;
pub mod platform;
pub mod policy;
pub mod procedure;
pub mod profile;
pub mod project;
pub mod project_map;
pub mod project_world;
pub mod projection;
pub mod ql;
pub mod resolve;
pub mod resource;
pub mod scope;
pub mod search;
pub mod session;
pub mod session_space;
pub mod session_space_application;
pub mod session_space_contribution;
pub mod skillset;
pub mod surface_material;
pub mod surfacing;
pub mod trust;

pub use error::{AikitError, Result};

pub use actor_bootstrap::{
    project_actor_bootstrap, ActorBootstrap, ActorBootstrapRequest, BootstrapReference,
    HarnessCompositionPointer, ResourceSetSummary, RuntimeBodyInspection, ACTOR_BOOTSTRAP_VERSION,
    BOOTSTRAP_RESOURCE_SAMPLE_LIMIT,
};
pub use application_context::application_context_resolution;
pub use capsule::{
    BypassPolicy, Capsule, Facets, Facing, FailurePolicy, HookPhase, Kind, LanguageFacet, Maturity,
    Payload, Requirement, Surface,
};
pub use catalog::{Catalog, MemoryCatalog};
pub use composition::{
    resolve_harness_composition, ActivationScope, ActivationScopeKind, ComponentBinding,
    ComponentContribution, ComponentDescriptor, ComponentRequirement, ComponentSelection,
    CompositionAbsence, CompositionActivationMode, CompositionCatalog, CompositionRelationKind,
    CompositionState, ContractBinding, ContractProvider, ContributionKind, HarnessComposition,
    HarnessCompositionRequest, LifetimeOwner, LifetimeOwnerKind, ProjectionBinding,
    RequirementStrength, ResolutionScope, RetractionMode, SurfaceDescriptor, SurfaceKind,
    TargetNativeComponentBinding, HARNESS_COMPOSITION_VERSION,
};
pub use composition_mutation::{
    apply_confirmed_harness_composition, preview_harness_composition_change,
    ConfirmedHarnessCompositionPreview, HarnessCompositionMutation, HarnessCompositionPreview,
    StagedHarnessComposition,
};
pub use composition_view::{
    diff_harness_compositions, explain_composed_component, ComponentCompositionExplanation,
    ContractRebinding, HarnessCompositionDiff, RequirementExplanation, RequirementResolution,
};
pub use context::{ContextBinding, ContextDescriptor, Isolation};
pub use context_resolution::{
    availability as resource_availability, compose_context_resolution, Availability,
    ContextResolution, ProjectionIntent, ReferenceResolution, RequestedActors, ResolvedResource,
    RetrievalPlan, ScopeResolution, CONTEXT_RESOLUTION_VERSION,
};
pub use context_source::{
    AbsenceKind, AgentVisibility, ContextSourceEntry, ContextSourceExplanation, ContextSourceHit,
    ContextSourceIndex, ContextSourceOperation, ContextSourceOperationalState,
    ContextSourcePrivacy, ContextSourceProvider, ContextSourceProviderCapabilities,
    ContextSourceProviderDescriptor, ContextSourceProviderStatus, ContextSourceReadOutcome,
    ContextSourceReadRequest, ContextSourceRetrieval, ContextSourceScope, DisclosureState,
    ExternalEgress, Freshness, HorizonRequest, ProviderReadResult, RetrievalTarget, SearchAudience,
    StructuredAbsence, CONTEXT_SOURCE_INDEX_VERSION,
};
pub use credential::{
    resolve_credential, CredentialBindingState, CredentialProviderRejection, CredentialRef,
    CredentialResolution, CredentialResolutionRequest, ProviderResolutionExplanation,
    SecretMaterialisationClass, SecretProviderDescriptor, SecretProviderRef, SecretProviderTier,
    SecretRequirement, SecretRequirementRef, CREDENTIAL_RESOLUTION_VERSION,
};
pub use duration::HumanDuration;
pub use effects::{EffectClass, Effects};
pub use familiarity::{
    AccessibilityAssessment, AccessibilitySignal, AccessibilitySignalClass, FamiliarityContext,
    FamiliarityObservation, FamiliaritySnapshot, FamiliaritySnapshotLoad, FamiliarityStore,
    FamiliarityUse, FitnessEvidence, ForgetScope, RouteStepEvidence,
    DEFAULT_FAMILIARITY_HALF_LIFE_MS, FAMILIARITY_SCHEMA_VERSION,
};
pub use frecency::{Candidate, Jump, Tiebreak};
pub use guidance::{
    compose, estimate_tokens, Composition, CompositionEntry, CompositionRequest, FragmentStatus,
    GuidanceFragment,
};
pub use hooks::{
    build_chains, matches as hook_matches, BypassScope, BypassToken, Denial, Dispatcher,
    ExecutionGroup, HookChain, HookDecision, HookEvent, HookEventKind, HookStep, StepOutcome,
    StepRecord, StepResult, StepVerdict,
};
pub use id::{
    CapsuleId, ContextId, EventId, GenerationId, InboxId, ProcedureId, ProfileId, ProjectId,
    RegistrySource, Revision, SessionId,
};
pub use knowledge::{
    ContextPackBudget, KnowledgeContextPack, KnowledgeReading, KnowledgeRelationView,
    KnowledgeRoute, KnowledgeRouteStep, RelationDirection, RelationEdge, RelationNode,
    RelationOrigin, RelationQuery, DEFAULT_RELATION_DEPTH, DEFAULT_RELATION_EDGE_BUDGET,
    DEFAULT_RELATION_NODE_BUDGET,
};
pub use knowledge_code::{
    CodeContext, CodeImpact, CodeIndexCapabilities, CodeIndexProvider, CodeIndexStatus,
    CodeReference, CodeSearchHit, CodeTrace, GITNEXUS_TESTED_VERSION,
};
pub use knowledge_navigation::{
    KnowledgeAddress, KnowledgeApplication, KnowledgeExplanation, KnowledgeProviderStatus,
    KnowledgeRankingEvidence, KnowledgeSearchHit, KnowledgeSearchResult, SourcePoolBinding,
    KNOWLEDGE_APPLICATION_VERSION,
};
pub use knowledge_operations::{
    KnowledgeOperations, KnowledgeSources, KNOWLEDGE_OPERATIONS_VERSION,
};
pub use knowledge_okf::{validate_okf, OkfDocument, OKF_VERSION};
pub use knowledge_source_pool::{
    material_for_actor, NativeSourcePoolProvider, SourceBinding, SourceHit, SourceMaterial,
    SourcePool, SourcePoolProvider, SourceProviderCapabilities, SourceProviderStatus,
    SourceSearchMode, SourceVisibility, BKMR_GLADE_CONFORMANCE_VERSION,
};
pub use knowledge_wiki::{
    parse_wiki_objects, OkfWikiBundle, SemanticRevision, WikiConstellation,
    WikiConstellationMember, WikiConstellationReturn, WikiEdge, WikiEdgeOrigin, WikiFrame,
    WikiNode, WikiObject, WikiProvenanceRef, WikiReading as SemanticWikiReading, WikiSpace,
    WikiSurfaceKind, OKF_WIKI_PROFILE,
};
pub use knowledge_wiki_index::{
    SemanticWikiIndex, WikiIndexStatus, WikiLocalWhole, WikiMutationProposal, WikiNeighbour,
    WikiObjectEnvelope, WikiRelationDirection, WikiSearchHit, DEFAULT_WIKI_NEIGHBOUR_LIMIT,
    DEFAULT_WIKI_SEARCH_LIMIT, SEMANTIC_WIKI_INDEX_VERSION,
};
pub use knowledge_wiki_provider::{
    SemanticWikiProvider, SemanticWikiProviderStatus, WikiExplanation,
    NATIVE_SEMANTIC_WIKI_PROVIDER,
};
pub use lifecycle::{CapabilityLifecycle, LifecycleThresholds};
pub use platform::{MuxKind, Platform, TargetId};
pub use policy::ManagedPolicy;
pub use procedure::{
    absent_fields, render_marked_block, select_isolation, splice_marked_block, FieldOrigin,
    FieldOrigins, Fidelity, Inverse, MutationIsolation, Plan, PlanDigest, Procedure, ProcedureKind,
    RegistryOwnership, UndoRecord, UndoStep, WorldEdit,
};
pub use profile::{ConfigMerge, ConfigTable, PoolPatch, Profile};
pub use project_map::{
    ProjectLens, ProjectMap, ProjectMapBinding, ProjectMapEndpoint, ProjectMapStep,
    PROJECT_MAP_VERSION,
};
pub use project_world::{
    disclose_project_world, ActorDisclosure, ActorRuntimeDisclosure, CapabilityHorizonDisclosure,
    EffectiveRevisionDisclosure, InformationHorizonDisclosure, ProjectWorldReadModel,
    ProjectWorldResource, ProjectionDisclosure, ResolutionBasisDisclosure,
    ResourceEffectiveDisclosure, ResourceIntentDisclosure, PROJECT_WORLD_VERSION,
};
pub use projection::{
    target_label, ActivationEffect, MaterializationMode, ProjectionItem, ProjectionPlan,
    ResolvedContext, TargetAdapter, TargetCapabilities,
};
pub use ql::{
    project_context_with_ql, QlAttachment, QlClientSubject, QlInputLimits, QlInputRefRevision,
    QlMode, QlOperation, QlProjectedContext, QlProjectedRefraction, QlProjectionRequest,
    QlProvenance, QlProviderCapabilities, QlProviderClass, QlProviderClient, QlProviderDiscovery,
    QlProviderFailure, QlProviderHealth, QlProviderRef, QlProviderState, QlReading,
    QlRefractionRequest, QlResultClass, QlTargetView, QL_MEF_REGISTRY_VERSION,
    QL_OUTPUT_SCHEMA_VERSION, QL_PROVENANCE_SCHEMA_VERSION,
};
pub use resolve::{
    resolve, resolve_diagnostic, ActiveCapability, Diagnosis, ResolveRequest, ResolvedView,
    ResolutionHash, UnavailableReason,
};
pub use scope::{LayerOrigin, ScopeKind, ScopeLayer};
pub use search::{
    parse_query, score, DocStatus, FastPrefix, Query, RankingSignals, SearchDoc, StatusFilter,
    UsageStats,
};
pub use session::{
    compile as compile_session, Attach, BackendSpec, Direction, Lifecycle, PaneSpec, PaneStep,
    Placement, Restart, SessionPlan, SessionSpec, Split, TaskSpec, ViewPlan, ViewSpec,
};
pub use session_space::{
    SessionSpaceActivationDriver, SessionSpaceActivationObservation, SessionSpaceActivationRequest,
    SessionSpaceActivationState, SessionSpaceAgentSession, SessionSpaceAuthorityState,
    SessionSpaceComponent, SessionSpaceConnection, SessionSpaceConnectionState,
    SessionSpaceDefinition, SessionSpaceLease, SessionSpaceLifecycle, SessionSpaceReadModel,
    SessionSpaceRef, SessionSpaceRuntime, SessionSpaceSurfaceReading, SESSION_SPACE_VERSION,
};
pub use session_space_contribution::{
    SessionSpaceContributionDefinition, SessionSpaceContributionRef,
    SessionSpaceContributionRegistration, SessionSpaceContributionRegistry,
    SessionSpaceContributionRegistryReadModel, SessionSpaceContributionRemoval,
    SESSION_SPACE_CONTRIBUTION_REGISTRY_VERSION, SESSION_SPACE_CONTRIBUTION_VERSION,
};
pub use skillset::{SetMembership, SetProjection, SetProvenance, SkillSet, Withheld, WithheldReason};
pub use surfacing::{plan_surfacing, DisplayContext, SurfacingPlan};
pub use trust::{TrustKey, TrustOracle, TrustState};