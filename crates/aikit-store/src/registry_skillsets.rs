//! Registry SkillSets: portable repertoire declared beside a registry.
//!
//! A home set (`<home>/skillsets/<name>/`) is machine-local authoring. A
//! registry set travels with the repository that owns it:
//!
//! ```text
//! <root>/skillsets/index.toml         [[skillset]] semantic_ref, directory, description,
//!                                     child_refs, [skillset.package]
//! <root>/skillsets/<directory>/members  one capsule id per line
//! ```
//!
//! Roots are discovered from what the home already registers — never from a
//! second registry of registries:
//!
//! * every `<home>/registries/<name>/` (AIKit's first-party registry is linked
//!   there), and
//! * every directory skill source, whose sibling `skillsets/` is the owner's
//!   portable repertoire (Central's `skills/` source ⇒ `Work/Central/skillsets`).
//!
//! A registry set is addressed by its semantic ref (`aikit:project-author`,
//! `central:documentation`). Loading one never trusts, enables or projects a
//! member: the set remains a request, answered per member by resolution.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use aikit_core::id::CapsuleId;
use aikit_core::skillset::{SetMembership, SetProvenance, SkillSet};
use aikit_core::{AikitError, Result};

use crate::home::{io_error, AikitHome};

pub const REGISTRY_SKILLSET_INDEX: &str = "skillsets/index.toml";

/// `<root>/skillsets/index.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySetIndex {
    pub schema: u32,
    #[serde(default)]
    pub skillset: Vec<RegistrySetEntry>,
}

/// One declared registry set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrySetEntry {
    pub semantic_ref: String,
    pub directory: String,
    #[serde(default)]
    pub description: String,
    /// Sets carried by reference (semantic refs or home set names).
    #[serde(default)]
    pub child_refs: Vec<String>,
    /// Neutral portable-package metadata. Kept as a value here so the index
    /// stays readable by every tool; the package SDK owns its typed reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<toml::Value>,
}

/// One loaded registry set with where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistrySet {
    pub root: PathBuf,
    pub entry: RegistrySetEntry,
    pub set: SkillSet,
}

/// A semantic ref is `<namespace>:<name>`; home set names never contain `:`.
pub fn is_semantic_ref(value: &str) -> bool {
    match value.split_once(':') {
        Some((namespace, name)) => {
            !namespace.is_empty()
                && !name.is_empty()
                && namespace
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && !name.contains('/')
        }
        None => false,
    }
}

/// Every registry root that declares sets, in a stable order, deduplicated by
/// canonical path.
pub fn roots(home: &AikitHome) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(home.root().join("registries")) {
        for entry in entries.flatten() {
            candidates.push(entry.path());
        }
    }
    if let Ok(entries) = std::fs::read_dir(home.root().join("sources")) {
        for entry in entries.flatten() {
            if let Some(path) = directory_source_path(&entry.path().join("source.toml")) {
                candidates.push(path.clone());
                if let Some(parent) = path.parent() {
                    candidates.push(parent.to_path_buf());
                }
            }
        }
    }
    let mut seen: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    for candidate in candidates {
        if !candidate.join(REGISTRY_SKILLSET_INDEX).is_file() {
            continue;
        }
        let canonical = candidate.canonicalize().unwrap_or(candidate.clone());
        seen.entry(canonical).or_insert(candidate);
    }
    seen.into_values().collect()
}

fn directory_source_path(source_toml: &Path) -> Option<PathBuf> {
    #[derive(Deserialize)]
    struct SourceNote {
        kind: String,
        #[serde(default)]
        path: Option<PathBuf>,
    }
    let text = std::fs::read_to_string(source_toml).ok()?;
    let note: SourceNote = toml::from_str(&text).ok()?;
    (note.kind == "directory").then_some(note.path).flatten()
}

/// Read one registry root's index and every set it declares (references left
/// unresolved; [`crate::skillsets::resolve_references`] attaches them).
pub fn load_root(root: &Path) -> Result<Vec<RegistrySet>> {
    let index_path = root.join(REGISTRY_SKILLSET_INDEX);
    let text = std::fs::read_to_string(&index_path)
        .map_err(|e| io_error("skillset.registry_unreadable", &index_path, &e))?;
    let index: RegistrySetIndex = toml::from_str(&text).map_err(|e| {
        AikitError::new(
            "skillset.registry_malformed",
            format!("{} is not a readable set index: {e}", index_path.display()),
        )
        .with("path", index_path.display().to_string())
    })?;
    if index.schema != 1 {
        return Err(AikitError::new(
            "skillset.registry_schema",
            format!(
                "{} declares schema {}, expected 1",
                index_path.display(),
                index.schema
            ),
        ));
    }
    let mut out = Vec::new();
    for entry in index.skillset {
        if !is_semantic_ref(&entry.semantic_ref) {
            return Err(AikitError::new(
                "skillset.registry_ref_invalid",
                format!(
                    "`{}` in {} is not a `<namespace>:<name>` semantic ref",
                    entry.semantic_ref,
                    index_path.display()
                ),
            ));
        }
        let set_dir = root.join("skillsets").join(&entry.directory);
        let members_path = set_dir.join("members");
        let members_text = std::fs::read_to_string(&members_path)
            .map_err(|e| io_error("skillset.registry_unreadable", &members_path, &e))?;
        let mut set = SkillSet::new(entry.directory.clone(), SetProvenance::Composed);
        set.description = entry.description.clone();
        set.semantic_ref = Some(entry.semantic_ref.clone());
        set.child_refs = entry.child_refs.clone();
        for line in members_text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let id = CapsuleId::parse(line)
                .map_err(|e| e.with("path", members_path.display().to_string()))?;
            if set.members.insert(id, SetMembership::Explicit).is_some() {
                return Err(AikitError::new(
                    "skillset.registry_duplicate_member",
                    format!("{} names `{line}` twice", members_path.display()),
                ));
            }
        }
        set.revision = Some(entry_revision(&entry, &members_text));
        out.push(RegistrySet {
            root: root.to_path_buf(),
            entry,
            set,
        });
    }
    Ok(out)
}

/// Every registry set the home can see.
pub fn load_all(home: &AikitHome) -> Result<Vec<RegistrySet>> {
    let mut out = Vec::new();
    for root in roots(home) {
        out.extend(load_root(&root)?);
    }
    Ok(out)
}

/// Find one registry set by semantic ref. Two roots declaring the same ref is
/// an ambiguity, never resolved by order.
pub fn find(home: &AikitHome, semantic_ref: &str) -> Result<Option<SkillSet>> {
    Ok(find_entry(home, semantic_ref)?.map(|found| found.set))
}

/// Find one registry set, with its root and index entry.
pub fn find_entry(home: &AikitHome, semantic_ref: &str) -> Result<Option<RegistrySet>> {
    let mut matches: Vec<RegistrySet> = load_all(home)?
        .into_iter()
        .filter(|found| found.entry.semantic_ref == semantic_ref)
        .collect();
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => Err(AikitError::new(
            "skillset.registry_ref_ambiguous",
            format!(
                "`{semantic_ref}` is declared by more than one registry: {}",
                matches
                    .iter()
                    .map(|found| found.root.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

fn entry_revision(entry: &RegistrySetEntry, members_text: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aikit-registry-skillset-revision-v1\n");
    hasher.update(entry.semantic_ref.as_bytes());
    hasher.update(&[0]);
    hasher.update(entry.description.as_bytes());
    hasher.update(&[0]);
    for child in &entry.child_refs {
        hasher.update(child.as_bytes());
        hasher.update(&[0]);
    }
    if let Some(package) = &entry.package {
        hasher.update(package.to_string().as_bytes());
    }
    hasher.update(&[0]);
    hasher.update(members_text.as_bytes());
    format!("blake3:{}", hasher.finalize().to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_refs_are_namespaced_and_never_paths() {
        assert!(is_semantic_ref("aikit:operator"));
        assert!(is_semantic_ref("central:core-development"));
        assert!(!is_semantic_ref("mattpocock/wayfinder-foundation"));
        assert!(!is_semantic_ref(":x"));
        assert!(!is_semantic_ref("a:b/c"));
    }

    #[test]
    fn registry_root_loads_sets_with_refs_and_revision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("skillsets/docs")).unwrap();
        std::fs::write(
            root.join("skillsets/index.toml"),
            "schema = 1\n[[skillset]]\nsemantic_ref = \"demo:docs\"\ndirectory = \"docs\"\ndescription = \"Docs\"\nchild_refs = [\"demo:accounts\"]\n[skillset.package]\nname = \"demo-docs\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("skillsets/docs/members"),
            "# docs\nskill/demo/vision\n",
        )
        .unwrap();
        let sets = load_root(root).unwrap();
        assert_eq!(sets.len(), 1);
        let set = &sets[0].set;
        assert_eq!(set.semantic_ref.as_deref(), Some("demo:docs"));
        assert_eq!(set.child_refs, vec!["demo:accounts".to_string()]);
        assert_eq!(set.members.len(), 1);
        assert!(set.revision.as_deref().unwrap().starts_with("blake3:"));
        assert!(sets[0].entry.package.is_some());
    }

    #[test]
    fn duplicate_registry_members_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("skillsets/docs")).unwrap();
        std::fs::write(
            root.join("skillsets/index.toml"),
            "schema = 1\n[[skillset]]\nsemantic_ref = \"demo:docs\"\ndirectory = \"docs\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("skillsets/docs/members"),
            "skill/demo/vision\nskill/demo/vision\n",
        )
        .unwrap();
        let error = load_root(root).unwrap_err();
        assert_eq!(error.code(), "skillset.registry_duplicate_member");
    }
}
