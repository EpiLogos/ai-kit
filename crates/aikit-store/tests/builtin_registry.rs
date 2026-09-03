//! Conformance for the capability registry committed with AIKit.
//!
//! Unit fixtures prove the loader's mechanics, but they cannot prove that the
//! product's own shipped manifests still inhabit the vocabulary the loader
//! accepts. A committed invalid capsule must therefore fail CI rather than
//! becoming a runtime `RegistryProblem` on the user's machine.

use std::path::PathBuf;

use aikit_core::RegistrySource;
use aikit_store::registry::load_registry;

#[test]
fn committed_builtin_registry_has_no_manifest_problems() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry_root = repo_root.join("skills/registry");

    let load = load_registry(&registry_root, RegistrySource::personal()).unwrap();

    assert!(
        !load.catalog.is_empty(),
        "the committed AIKit registry should contain capabilities"
    );
    assert!(
        load.problems.is_empty(),
        "the committed AIKit registry contains invalid manifests: {:#?}",
        load.problems
    );
}
