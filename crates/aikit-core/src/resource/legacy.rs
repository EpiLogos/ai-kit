use crate::capsule::Capsule;
use crate::{AikitError, Result};

use super::{
    ResourceDescriptor, ResourceKind, ResourceLocator, ResourceRecord, ResourceRef, ResourceSource,
    SourceRef, SourceRevision, SourceState,
};

/// Preserve the current Capsule identity while exposing it in the wider V2 field
/// as a Capability. A registry remains source provenance; it is not silently
/// promoted into a provider or semantic owner.
///
/// Conversion is fallible so malformed legacy provenance is made explicit rather
/// than entering the V2 index through an unchecked identity constructor.
impl TryFrom<&Capsule> for ResourceRecord {
    type Error = AikitError;

    fn try_from(capsule: &Capsule) -> Result<Self> {
        let mut descriptor = ResourceDescriptor::new(
            ResourceRef::parse(&capsule.id.to_string())?,
            ResourceKind::Capability,
            capsule.name.clone(),
            capsule.description.clone(),
        );

        if let Some(source) = &capsule.source {
            descriptor.sources.push(ResourceSource {
                source: SourceRef::parse(&format!("registry:{}", source.as_str()))?,
                authority: None,
                revision: capsule
                    .revision
                    .as_ref()
                    .map(|revision| SourceRevision::parse(revision.as_str()))
                    .transpose()?,
                locator: capsule.root.clone().map(ResourceLocator::Path),
                state: SourceState::Available,
            });
        }

        Ok(Self::new(descriptor))
    }
}
