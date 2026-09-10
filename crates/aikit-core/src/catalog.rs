//! The catalog: everything eligible for selection.
//!
//! Membership in the catalog says nothing about activation. A capsule that has
//! just been pulled in by a registry sync is catalogued and inert until something
//! explicitly names it.

use std::collections::BTreeMap;

use crate::capsule::Capsule;
use crate::id::{CapsuleId, ProfileId};
use crate::profile::Profile;

/// Read access to capsules and profiles.
///
/// Object-safe so the resolver can take `&dyn Catalog` and the store can back it
/// with SQLite without the core crate learning about SQLite.
pub trait Catalog {
    fn get(&self, id: &CapsuleId) -> Option<&Capsule>;
    fn profile(&self, id: &ProfileId) -> Option<&Profile>;
    fn capsules(&self) -> Vec<&Capsule>;
    fn profiles(&self) -> Vec<&Profile>;

    /// A revision marker for the whole catalog, used to key resolver caches.
    fn catalog_revision(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        for capsule in self.capsules() {
            hasher.update(capsule.id.to_string().as_bytes());
            hasher.update(b"@");
            if let Some(rev) = &capsule.revision {
                hasher.update(rev.as_str().as_bytes());
            }
            hasher.update(b";");
        }
        for profile in self.profiles() {
            hasher.update(profile.id.to_string().as_bytes());
            hasher.update(b";");
        }
        hasher.finalize().to_hex().to_string()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryCatalog {
    capsules: BTreeMap<CapsuleId, Capsule>,
    profiles: BTreeMap<ProfileId, Profile>,
}

impl MemoryCatalog {
    pub fn insert(&mut self, capsule: Capsule) {
        self.capsules.insert(capsule.id.clone(), capsule);
    }

    pub fn insert_profile(&mut self, profile: Profile) {
        self.profiles.insert(profile.id.clone(), profile);
    }

    pub fn len(&self) -> usize {
        self.capsules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.capsules.is_empty()
    }
}

impl Catalog for MemoryCatalog {
    fn get(&self, id: &CapsuleId) -> Option<&Capsule> {
        self.capsules.get(id)
    }

    fn profile(&self, id: &ProfileId) -> Option<&Profile> {
        self.profiles.get(id)
    }

    fn capsules(&self) -> Vec<&Capsule> {
        self.capsules.values().collect()
    }

    fn profiles(&self) -> Vec<&Profile> {
        self.profiles.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::Revision;

    fn capsule(id: &str) -> Capsule {
        let leaf = id.rsplit('/').next().unwrap();
        let src = format!(
            r#"
schema = 1
id = "{id}"
kind = "script"
name = "{leaf}"
description = "Test."

[script]
entry = "payload/run.sh"
"#
        );
        Capsule::from_toml_str(&src).unwrap()
    }

    #[test]
    fn capsules_iterate_in_deterministic_id_order() {
        let mut catalog = MemoryCatalog::default();
        catalog.insert(capsule("script/z/last"));
        catalog.insert(capsule("script/a/first"));
        let ids: Vec<String> = catalog
            .capsules()
            .iter()
            .map(|c| c.id.to_string())
            .collect();
        assert_eq!(ids, vec!["script/a/first", "script/z/last"]);
    }

    #[test]
    fn the_catalog_revision_changes_when_a_capsule_revision_changes() {
        let mut catalog = MemoryCatalog::default();
        let mut c = capsule("script/a/first");
        c.revision = Some(Revision::from_raw("one"));
        catalog.insert(c.clone());
        let before = catalog.catalog_revision();

        c.revision = Some(Revision::from_raw("two"));
        catalog.insert(c);
        assert_ne!(before, catalog.catalog_revision());
    }

    #[test]
    fn the_catalog_revision_is_stable_for_identical_contents() {
        let build = || {
            let mut catalog = MemoryCatalog::default();
            catalog.insert(capsule("script/a/first"));
            catalog.insert(capsule("script/b/second"));
            catalog
        };
        assert_eq!(build().catalog_revision(), build().catalog_revision());
    }
}
