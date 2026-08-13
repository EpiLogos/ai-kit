//! V2 typed resource and provider foundation.
//!
//! The legacy capsule resolver remains the proven activation mechanism. This
//! module widens the indexed operational field without claiming ownership of the
//! external semantics it references.

mod index;
mod legacy;
mod model;
mod refs;

pub use index::{MemoryResourceIndex, ResourceIndex};
pub use model::{
    Eligibility, PreferenceIntent, ProviderOffer, ProviderState, ResourceDescriptor,
    ResourceExplanation, ResourceKind, ResourceLocator, ResourceRecord, ResourceSource,
    SourceAuthority, SourceState,
};
pub use refs::{OwnerRef, ProviderRef, ResourceRef, SourceRef, SourceRevision};
