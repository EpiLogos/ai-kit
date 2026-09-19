//! The first-party registry ships inside the binary.
//!
//! The committed `registry/` tree at the repo root is the product's own
//! capability source: the AIKit-authored operational skills, and the vendored
//! default skillsets that `.aikit/profile.toml` and ADR 0002's
//! `mattpocock/wayfinder-foundation` declare. Both are validated in CI
//! (`builtin_registry.rs`, `scripts/verify-native-skills.py`), and the build
//! embeds the validated bytes — paths, permission bits, contents — into this
//! executable.
//!
//! [`ensure_materialized`] completes the managed install path the registry
//! README names: on a fresh `AIKIT_HOME` the binary materialises the registry
//! as `<home>/registries/ai-kit` before any catalogue is read, so the product
//! resolves its own declared skills on any machine instead of referencing
//! capabilities that exist nowhere. A directory or symlink already occupying
//! that name is the operator's — it is left exactly as found, which is what
//! makes a checkout-linked registry (a symlink to a development tree) and a
//! hand-curated registry equally respected.
//!
//! Materialised capsules are catalogued, not trusted: Skill activation still
//! requires the operator's `aikit trust record` for the observed revision.
//! Shipping the bytes and reviewing them are deliberately separate acts.

use std::fs;
use std::path::Path;

use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;

/// The registry name the first-party bytes materialise under.
pub const REGISTRY_NAME: &str = "ai-kit";

include!(concat!(env!("OUT_DIR"), "/first_party_registry.rs"));

/// Materialise the embedded first-party registry into `home` when its name is
/// free. Idempotent, and cheap when already present (one stat per call).
pub fn ensure_materialized(home: &AikitHome) -> Result<()> {
    let target = home.registry(REGISTRY_NAME);
    // `symlink_metadata` reads the link itself: an existing directory, a live
    // symlink, even a dangling one all count as "occupied" and are left alone.
    if target.symlink_metadata().is_ok() {
        return Ok(());
    }
    let files = materialize(&target)?;
    report_materialised(files, &target);
    Ok(())
}

fn materialize(target: &Path) -> Result<usize> {
    for (relative, mode, bytes) in FIRST_PARTY_REGISTRY {
        let path = target.join(relative);
        let parent = path.parent().ok_or_else(|| {
            AikitError::new(
                "first_party.path_without_parent",
                format!("{relative} has no parent directory"),
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "first_party.materialize_failed",
                format!("could not create {}: {error}", parent.display()),
            )
            .with("path", parent.display().to_string())
        })?;
        fs::write(&path, bytes).map_err(|error| {
            AikitError::new(
                "first_party.materialize_failed",
                format!("could not write {}: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        set_mode(&path, *mode)?;
    }
    Ok(FIRST_PARTY_REGISTRY.len())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|error| {
        AikitError::new(
            "first_party.materialize_failed",
            format!("could not set permissions on {}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })
}

#[cfg(not(unix))]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    let readonly = mode & 0o222 == 0;
    let mut permissions = fs::metadata(path)
        .map_err(|error| {
            AikitError::new(
                "first_party.materialize_failed",
                format!("could not stat {}: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?
        .permissions();
    permissions.set_readonly(readonly);
    fs::set_permissions(path, permissions).map_err(|error| {
        AikitError::new(
            "first_party.materialize_failed",
            format!("could not set permissions on {}: {error}", path.display()),
        )
        .with("path", path.display().to_string())
    })
}

/// Materialisation is silent on the happy path; a one-line note keeps the
/// first run legible without turning every command into a narrator.
fn report_materialised(files: usize, target: &Path) {
    if files > 0 {
        eprintln!(
            "aikit: materialised the first-party registry ({files} files) into {}",
            target.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::catalog::Catalog;
    use aikit_core::{CapsuleId, RegistrySource};
    use aikit_store::registry::load_registry;

    /// The ids `.aikit/profile.toml` enables and ADR 0002 names as the default
    /// foundation. A fresh home must hold every one of them after
    /// materialisation — that is the whole point of shipping the registry.
    const DECLARED_DEFAULTS: &[&str] = &[
        "skill/mattpocock/engineering/wayfinder",
        "skill/mattpocock/engineering/setup-matt-pocock-skills",
        "skill/mattpocock/productivity/grilling",
        "skill/mattpocock/engineering/domain-modeling",
        "skill/mattpocock/engineering/prototype",
        "skill/mattpocock/engineering/research",
        "skill/writing-guidance-tools/writing-guidance-tools",
    ];

    #[test]
    fn a_fresh_home_receives_the_first_party_registry() {
        let home_root = tempfile::tempdir().expect("tempdir");
        let home = AikitHome::at(home_root.path().join("home"));
        assert!(!home.registry(REGISTRY_NAME).exists());

        ensure_materialized(&home).expect("materialisation");

        let load = load_registry(
            &home.registry(REGISTRY_NAME),
            RegistrySource::new(REGISTRY_NAME),
        )
        .expect("registry loads");
        assert!(
            load.problems.is_empty(),
            "materialised registry must be valid: {:#?}",
            load.problems
        );
        for id in DECLARED_DEFAULTS {
            let id = CapsuleId::parse(id).expect("declared default parses");
            assert!(
                load.catalog.get(&id).is_some(),
                "{id} must ship with the first-party registry"
            );
        }
        // The AIKit-authored operational skills ride along: they are the same
        // defect class — first-party capabilities that referenced nowhere.
        let knowledge = CapsuleId::parse("skill/aikit/knowledge-navigation").unwrap();
        assert!(load.catalog.get(&knowledge).is_some());
    }

    #[test]
    fn materialisation_preserves_executable_hook_entries() {
        let home_root = tempfile::tempdir().expect("tempdir");
        let home = AikitHome::at(home_root.path().join("home"));

        ensure_materialized(&home).expect("materialisation");

        let load = load_registry(
            &home.registry(REGISTRY_NAME),
            RegistrySource::new(REGISTRY_NAME),
        )
        .expect("registry loads");
        let route = load
            .catalog
            .get(&CapsuleId::parse("hook/aikit/knowledge-route").unwrap())
            .expect("knowledge-route ships");
        let hook = route.hook().expect("knowledge-route declares a hook");
        let root = home
            .registry(REGISTRY_NAME)
            .join("capsules/hook/aikit/knowledge-route");
        let entry = root.join(&hook.entry);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&entry)
                .expect("entry exists")
                .permissions()
                .mode();
            assert_ne!(
                mode & 0o111,
                0,
                "hook entries are executed directly; the executable bit must survive \
                 materialisation ({})",
                entry.display()
            );
        }
        #[cfg(not(unix))]
        let _ = entry;
    }

    #[test]
    fn an_occupied_registry_name_is_never_touched() {
        let home_root = tempfile::tempdir().expect("tempdir");
        let home = AikitHome::at(home_root.path().join("home"));
        let target = home.registry(REGISTRY_NAME);
        std::fs::create_dir_all(&target).expect("mkdir");
        std::fs::write(target.join("operators-own.txt"), b"kept").expect("write");

        ensure_materialized(&home).expect("no-op materialisation");

        assert_eq!(
            std::fs::read(target.join("operators-own.txt")).expect("canary survives"),
            b"kept",
            "an existing registry of the same name belongs to the operator"
        );
        // And nothing else arrived: the name is occupied, the materialiser
        // walked away.
        assert_eq!(std::fs::read_dir(&target).expect("readable").count(), 1);
    }
}
