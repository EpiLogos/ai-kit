//! V2 typed resource and provider foundation.
//!
//! The legacy capsule resolver remains the proven activation mechanism. This
//! module widens the indexed operational field without claiming ownership of the
//! external semantics it references.

mod action_search;
mod factory;
mod index;
mod legacy;
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
    ResourceSearchHit, ResourceSearchHitKind, ResourceSearchIndex,
};