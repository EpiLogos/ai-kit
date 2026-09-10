//! Owner-authored Model catalogue entries on disk.
//!
//! The first-party seed ships in `aikit-core`; this module is the owner's half.
//! Entries live in `<home>/model-catalogue/*.json` and layer *over* the seed by
//! canonical `ModelRef`, so an owner can correct or extend what AIKit knows
//! without editing the product, and without forking a Model into two identities.
//!
//! This module reads authored ground. It never writes catalogue entries from
//! detection: a Model that exists only because something is installed here today
//! would not be a stable identity at all.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use aikit_core::resource::{
    catalogue_from_observations, ModelCatalogue, ModelCatalogueEntry, ProviderCatalogDocument,
    MODEL_CATALOGUE_VERSION,
};
use aikit_core::{AikitError, Result};

use crate::home::AikitHome;

/// The owner catalogue directory, relative to the AIKit home.
pub const MODEL_CATALOGUE_DIR: &str = "model-catalogue";

/// One owner catalogue file. A bare array of entries is accepted too, so the
/// smallest useful file is a two-line JSON list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum CatalogueFile {
    Document {
        schema_version: String,
        entries: Vec<ModelCatalogueEntry>,
    },
    Entries(Vec<ModelCatalogueEntry>),
}

/// What loading the owner catalogue yielded: the entries that parsed, and an
/// honest problem line per file that did not. An unreadable file is disclosed,
/// never silently skipped — a Model missing because its file has a typo looks
/// exactly like a Model that was never authored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnerCatalogueLoad {
    pub catalogue: ModelCatalogue,
    pub problems: Vec<String>,
    pub files: Vec<PathBuf>,
}

/// Load the owner-authored catalogue from `<home>/model-catalogue`. A missing
/// directory is an empty catalogue with no problems: not having authored any
/// entries is a normal state, not a fault.
pub fn load_owner_catalogue(home: &AikitHome) -> OwnerCatalogueLoad {
    let dir = home.root().join(MODEL_CATALOGUE_DIR);
    let mut load = OwnerCatalogueLoad::default();
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return load,
        Err(error) => {
            load.problems
                .push(format!("{}: {error}", dir.display()));
            return load;
        }
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    for path in paths {
        match read_catalogue_file(&path) {
            Ok(entries) => {
                load.files.push(path);
                for entry in entries {
                    if let Err(error) = load.catalogue.insert(entry) {
                        load.problems.push(error.to_string());
                    }
                }
            }
            Err(error) => load.problems.push(error.to_string()),
        }
    }
    load
}

fn read_catalogue_file(path: &PathBuf) -> Result<Vec<ModelCatalogueEntry>> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        AikitError::new(
            "model_catalogue.unreadable",
            format!("{}: {error}", path.display()),
        )
    })?;
    let file: CatalogueFile = serde_json::from_str(&text).map_err(|error| {
        AikitError::new(
            "model_catalogue.invalid",
            format!("{}: {error}", path.display()),
        )
    })?;
    match file {
        CatalogueFile::Document {
            schema_version,
            entries,
        } if schema_version == MODEL_CATALOGUE_VERSION => Ok(entries),
        CatalogueFile::Document { schema_version, .. } => Err(AikitError::new(
            "model_catalogue.unexpected_schema",
            format!(
                "{}: schema_version {schema_version:?} (expected {MODEL_CATALOGUE_VERSION})",
                path.display()
            ),
        )),
        CatalogueFile::Entries(entries) => Ok(entries),
    }
}

/// The catalogue AIKit resolves with: the first-party seed, with owner-authored
/// entries layered over it by canonical ModelRef.
pub fn resolved_catalogue(home: &AikitHome) -> (ModelCatalogue, Vec<String>) {
    // Three layers, weakest first: what AIKit itself knows, what Provider
    // Sources publish, and what the owner authored. Later layers win by
    // canonical ModelRef, so nothing forks a Model into two identities.
    let mut catalogue = ModelCatalogue::first_party_seed();
    let mut notes = Vec::new();

    let (documents, problems) = load_provider_catalogs(home);
    notes.extend(problems);
    for document in &documents {
        match catalogue_from_observations(&document.observations) {
            Ok(published) => {
                notes.push(format!(
                    "{} Model(s) published by {} (read {})",
                    published.len(),
                    document.listed_by,
                    document.observed_at
                ));
                catalogue.extend(published);
            }
            Err(error) => notes.push(format!(
                "Provider Source {} unusable: {error}",
                document.listed_by
            )),
        }
    }
    if documents.is_empty() {
        notes.push(
            "no Provider Source has been read on this machine — the catalogue is the \
             first-party seed plus any owner entries; refresh one with \
             `aikit model-catalogue refresh`"
                .to_string(),
        );
    }

    let load = load_owner_catalogue(home);
    let owner_count = load.catalogue.len();
    catalogue.extend(load.catalogue);
    notes.extend(load.problems);
    if owner_count > 0 {
        notes.push(format!(
            "{owner_count} owner-authored Model catalogue entr{} layered over everything else",
            if owner_count == 1 { "y" } else { "ies" }
        ));
    }
    (catalogue, notes)
}

// ---------------------------------------------------------------------------
// Provider Source cache
// ---------------------------------------------------------------------------

/// Where Provider Source readings are cached. Deliberately under `state/`, not
/// beside the owner's authored entries: this is observed material with a
/// timestamp, and it must never be mistaken for authored ground.
pub const PROVIDER_CATALOG_DIR: &str = "state/provider-catalog";

/// Persist one Provider Source reading, keyed by the listing provider.
pub fn save_provider_catalog(home: &AikitHome, document: &ProviderCatalogDocument) -> Result<PathBuf> {
    let dir = home.root().join(PROVIDER_CATALOG_DIR);
    std::fs::create_dir_all(&dir).map_err(|error| {
        AikitError::new(
            "provider_catalog.unwritable",
            format!("{}: {error}", dir.display()),
        )
    })?;
    let slug = document
        .listed_by
        .as_str()
        .rsplit(':')
        .next()
        .unwrap_or("provider")
        .replace(['/', '\\'], "-");
    let path = dir.join(format!("{slug}.json"));
    let body = serde_json::to_string_pretty(document).map_err(|error| {
        AikitError::new("provider_catalog.unserialisable", error.to_string())
    })?;
    std::fs::write(&path, body).map_err(|error| {
        AikitError::new(
            "provider_catalog.unwritable",
            format!("{}: {error}", path.display()),
        )
    })?;
    Ok(path)
}

/// Every cached Provider Source reading, plus a problem line per file that
/// could not be read. A missing directory is "no Provider Source has been
/// read yet" — a normal state, not a fault, and never a claim that no
/// provider publishes anything.
pub fn load_provider_catalogs(home: &AikitHome) -> (Vec<ProviderCatalogDocument>, Vec<String>) {
    let dir = home.root().join(PROVIDER_CATALOG_DIR);
    let mut documents = Vec::new();
    let mut problems = Vec::new();
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (documents, problems),
        Err(error) => {
            problems.push(format!("{}: {error}", dir.display()));
            return (documents, problems);
        }
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    for path in paths {
        match std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                serde_json::from_str::<ProviderCatalogDocument>(&text).map_err(|e| e.to_string())
            }) {
            Ok(document) => documents.push(document),
            Err(error) => problems.push(format!("{}: {error}", path.display())),
        }
    }
    (documents, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::resource::{canonical_model_ref, ProviderRef};

    fn home() -> (tempfile::TempDir, AikitHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().to_path_buf());
        (dir, home)
    }

    fn write(home: &AikitHome, name: &str, body: &str) {
        let dir = home.root().join(MODEL_CATALOGUE_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    const ENTRY: &str = r#"[{
      "model": "model:owner-pinned",
      "name": "Owner pinned",
      "description": "authored by the owner",
      "routes": [{
        "provider": "provider:ollama",
        "kind": "local-serving",
        "provider_native_ids": ["owner-pinned:7b"],
        "credential": {"condition": "not-required"}
      }],
      "source": "source/owner"
    }]"#;

    #[test]
    fn no_catalogue_directory_is_an_empty_catalogue_not_a_fault() {
        let (_dir, home) = home();
        let load = load_owner_catalogue(&home);
        assert!(load.catalogue.is_empty());
        assert!(load.problems.is_empty());
    }

    #[test]
    fn owner_entries_load_and_layer_over_the_seed() {
        let (_dir, home) = home();
        write(&home, "owner.json", ENTRY);
        let (catalogue, notes) = resolved_catalogue(&home);
        assert!(catalogue.len() > 1, "the seed is still there");
        let ollama = ProviderRef::parse("provider:ollama").unwrap();
        let (entry, _) = catalogue.claiming(&ollama, "owner-pinned:7b").unwrap();
        assert_eq!(entry.model, canonical_model_ref("model:owner-pinned").unwrap());
        assert!(notes.iter().any(|note| note.contains("owner-authored")));
    }

    #[test]
    fn an_unparsable_file_is_disclosed_never_silently_skipped() {
        let (_dir, home) = home();
        write(&home, "broken.json", "{not json");
        let load = load_owner_catalogue(&home);
        assert!(load.catalogue.is_empty());
        assert_eq!(load.problems.len(), 1);
        assert!(load.problems[0].contains("broken.json"));
    }

    #[test]
    fn a_non_canonical_model_ref_is_refused_at_the_door() {
        let (_dir, home) = home();
        write(
            &home,
            "stale.json",
            &ENTRY.replace("model:owner-pinned", "model/owner-pinned"),
        );
        let load = load_owner_catalogue(&home);
        assert!(load.catalogue.is_empty());
        assert!(load.problems[0].contains("canonical"));
    }

    #[test]
    fn a_document_wrapper_must_carry_the_expected_schema_version() {
        let (_dir, home) = home();
        write(
            &home,
            "doc.json",
            &format!(r#"{{"schema_version":"{MODEL_CATALOGUE_VERSION}","entries":{ENTRY}}}"#),
        );
        assert_eq!(load_owner_catalogue(&home).catalogue.len(), 1);
        drop(home);

        let (_dir2, home2) = self::home();
        write(
            &home2,
            "doc.json",
            &format!(r#"{{"schema_version":"aikit.model-catalogue/v99","entries":{ENTRY}}}"#),
        );
        let load = load_owner_catalogue(&home2);
        assert!(load.catalogue.is_empty());
        assert!(load.problems[0].contains("v99"));
    }
}
