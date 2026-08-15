//! AIKit core: a context-scoped capability router for agentic terminal work.
//!
//! The central object is not a global "active set" but an **effective capability
//! view resolved for a context**, where a context is roughly
//!
//! ```text
//! user + host + project scope chain + session space + task + target client
//! ```
//!
//! Everything else in AIKit — the registry, the palette, multiplexer
//! integrations, the hook bank, the capture pipeline — is a view or a consumer of
//! that resolution.
//!
//! This crate is deliberately free of I/O. The resolver takes a catalog, a stack
//! of scope layers and an environment, and returns a resolved graph plus an
//! explanation and a deterministic hash. That is what makes it testable without a
//! filesystem, and what makes generations content-addressable.

#![forbid(unsafe_code)]

pub mod arg;
pub mod capsule;
pub mod catalog;
pub mod composition;
pub mod context;
pub mod context_resolution;
pub mod context_source;
pub mod duration;
pub mod effects;
pub mod error;
pub mod frecency;
pub mod guidance;
pub mod hooks;
pub mod id;
pub mod lifecycle;
pub mod platform;
pub mod policy;
pub mod procedure;
pub mod profile;
pub mod project;
pub mod projection;
pub mod ql;
pub mod resolve;
pub mod resource;
pub mod scope;
pub mod search;
pub mod session;
pub mod skillset;
pub mod surfacing;
pub mod trust;

pub use error::{AikitError, Result};

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
pub use duration::HumanDuration;
pub use effects::{EffectClass, Effects};
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
pub use lifecycle::{CapabilityLifecycle, LifecycleThresholds};
pub use platform::{MuxKind, Platform, TargetId};
pub use policy::ManagedPolicy;
pub use procedure::{
    absent_fields, render_marked_block, select_isolation, splice_marked_block, FieldOrigin,
    FieldOrigins, Fidelity, Inverse, MutationIsolation, Plan, PlanDigest, Procedure, ProcedureKind,
    RegistryOwnership, UndoRecord, UndoStep, WorldEdit,
};
pub use profile::{ConfigMerge, ConfigTable, PoolPatch, Profile};
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
pub use skillset::{SetMembership, SetProjection, SetProvenance, SkillSet, Withheld, WithheldReason};
pub use surfacing::{plan_surfacing, DisplayContext, SurfacingPlan};
pub use trust::{TrustKey, TrustOracle, TrustState};
