//! V2 typed resource and provider foundation.
//!
//! `ResourceRef` / `ResourceRecord` own application identity. Package formats such
//! as Capsule remain source/catalog concerns and may be translated only at their
//! ingestion boundary; canonical resource semantics do not depend on a Capsule
//! conversion shim.

mod action_search;
mod development_field;
mod factory;
mod index;
mod model;
#[path = "../model_catalogue.rs"]
mod model_catalogue;
#[path = "../model_roster.rs"]
mod model_roster;
#[path = "../model_route.rs"]
mod model_route;
mod operative;
mod operative_provider;
mod refs;
#[path = "../routine.rs"]
pub mod routine;
mod search;
mod versioned_world;

pub use action_search::search_contextual_actions;
pub use development_field::*;
pub use factory::{FactoryInteropView, FactoryResourceImport};
pub use index::{MemoryResourceIndex, ResolveRankingSignals, ResourceIndex};
pub use model::{
    Eligibility, PreferenceIntent, ProviderOffer, ProviderState, ResourceDescriptor,
    ResourceExplanation, ResourceKind, ResourceLocator, ResourceRecord, ResourceSource,
    SourceAuthority, SourceState,
};
pub use model_catalogue::{
    canonical_model_ref, catalogue_from_observations, migrate_model_ref, DeclaredRoute,
    ModelCatalogue, ModelCatalogueEntry, ProviderCatalogDocument, ProviderCatalogObservation,
    FIRST_PARTY_CATALOGUE_SOURCE, MODEL_CATALOGUE_VERSION, MODEL_REF_PREFIX,
    PROVIDER_CATALOG_OBSERVATION_SCHEMA, PROVIDER_CATALOG_SOURCE,
};
pub use model_roster::{
    candidates_from_routes, rank_model_roster, select_model, ExactSpendObservation,
    FitnessObservation, FitnessScope, ModelAccessProfileView, ModelPriceObservation,
    ModelRankingExplanation, ModelRankingPolicy, ModelRoster, ModelRosterCandidate,
    ModelRosterDemand, ModelRosterEntry, ModelSelection, RankingComponent, MODEL_ROSTER_VERSION,
};
pub use model_route::{
    CredentialCondition, ModelRoute, ModelRouteKind, ModelRouteSet, RouteAvailability,
    RouteUsability, UnmatchedModelOffer, MODEL_ROUTE_VERSION,
};
pub use operative::{
    action_semantic_profile, horizons_for_kind, horizons_for_resource, parse_or_search_expression,
    parse_resolve_expression, resolve_action_candidates, resolve_expression, resolve_path_identity,
    resolve_search, resolve_subjects, six_horizon_disclosure, ActionRef, ActionSemanticProfile,
    AddressHorizon, RelationOp, ResolveCandidate, ResolveExpression, ResolvePath, ResolvePathStep,
    ResolvedActionCandidate, OPERATIVE_SYNTAX_VERSION,
};
pub use operative_provider::{
    OperativeSemanticOperation, OperativeSemanticProvider, OperativeSemanticProviderCapabilities,
    OperativeSemanticProviderDescriptor, OperativeSemanticProviderStatus,
    OPERATIVE_SEMANTIC_PROVIDER_VERSION,
};
pub use refs::{OwnerRef, ProviderRef, ResourceRef, SourceRef, SourceRevision};
pub use routine::{
    prove_method, MethodProofInput, ProvenMethodBasis, Routine, RoutineAuthority,
    RoutineAuthorityStanding, RoutineAuthorityValidation, RoutineExplanation, RoutineInvocation,
    RoutineInvocationAuthorisationRequest, RoutineInvocationEvidence, RoutineInvocationOccurrence,
    RoutineProofStanding, RoutineProviderDelivery, RoutineSchedulerBinding, RoutineSchedulerState,
    RoutineState, RoutineTrigger, RoutineTriggerObservation, METHOD_PROOF_VERSION,
    ROUTINE_INVOCATION_EVIDENCE_VERSION, ROUTINE_VERSION,
};
pub use search::{
    ActionStageability, ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass,
    ResourceRankingSignals, ResourceSearchHit, ResourceSearchHitKind, ResourceSearchIndex,
};
pub use versioned_world::{
    CreateWorktreeRequest, GitRepositoryRelation, GitWorkingState, GitWorktreeRelation,
    VersionDiff, VersionDiffRequest, VersionHistoryEntry, VersionHistoryRequest, VersionRevision,
    VersionedProjectWorld, VersionedWorldCapability, VersionedWorldProvider,
    VersionedWorldProviderDescriptor, VersionedWorldProviderStatus, VERSIONED_WORLD_VERSION,
};
