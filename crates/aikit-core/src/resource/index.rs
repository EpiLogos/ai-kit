use std::collections::BTreeMap;

use super::{ResourceRecord, ResourceRef};

pub trait ResourceIndex {
    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord>;
    fn resources(&self) -> Vec<&ResourceRecord>;
}

#[derive(Debug, Clone, Default)]
pub struct MemoryResourceIndex {
    resources: BTreeMap<ResourceRef, ResourceRecord>,
}

impl MemoryResourceIndex {
    pub fn insert(&mut self, record: ResourceRecord) -> Option<ResourceRecord> {
        self.resources.insert(record.descriptor.id.clone(), record)
    }

    pub fn len(&self) -> usize {
        self.resources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }
}

impl ResourceIndex for MemoryResourceIndex {
    fn resource(&self, id: &ResourceRef) -> Option<&ResourceRecord> {
        self.resources.get(id)
    }

    fn resources(&self) -> Vec<&ResourceRecord> {
        self.resources.values().collect()
    }
}
