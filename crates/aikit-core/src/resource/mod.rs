//! V2 typed resource and provider foundation.
//!
//! `ResourceRef` / `ResourceRecord` own application identity. Package formats such
//! as Capsule remain source/catalog concerns and may be translated only at their
//! ingestion boundary; canonical resource semantics do not depend on a Capsule
//! conversion shim.

mod action_search;
mod factory;
mod index;
mod model;
mod refs;
mod search;

pub use action_search::search_contextual_actions;
pub use factory::{FactoryInteropView, FactoryResourceImport};
pub use index::{MemoryResourceIndex, ResourceIndex};
pub use model::{
    Eligibility, PreferenceIntent, ProviderOffer, ProviderState, ResourceDescriptor,
    ResourceExplanation, ResourceKind, ResourceLocator, ResourceRecord, ResourceSource,
    SourceAuthority, SourceState,
};
pub use refs::{OwnerRef, ProviderRef, ResourceRef, SourceRef, SourceRevision};
pub use search::{
    ActionStageability, ContextualActionDescriptor, NavigationEvidence, NavigationEvidenceClass,
    ResourceRankingSignals, ResourceSearchHit, ResourceSearchHitKind, ResourceSearchIndex,
};
