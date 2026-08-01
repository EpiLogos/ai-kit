//! Loading registries from disk.
//!
//! The properties that matter here are the ones that decide whether trust means
//! anything: the content revision has to move when *any* byte of the payload
//! moves, the registry source has to be stamped truthfully, and one unparseable
//! manifest must not be able to hide the rest of the tree.

mod common;

use common::*;

use aikit_core::catalog::Catalog;
use aikit_core::{Kind, RegistrySource};
use aikit_store::registry::{load_project_local, load_registry};

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

#[test]
fn a_capsule_is_loaded_from_its_manifest_and_stamped_with_registry_facts() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    let dir = fixture.script("script/test/cargo-nextest");

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);

    let capsule = load.catalog.get(&cid("script/test/cargo-nextest")).unwrap();
    assert_eq!(capsule.kind, Kind::Script);
    assert_eq!(
        capsule.source.as_ref().unwrap(),
        &RegistrySource::personal()
    );
    assert_eq!(capsule.root.as_deref(), Some(dir.as_path()));
    assert!(capsule.revision.is_some());
}

#[test]
fn capsules_of_every_kind_are_found_at_their_documented_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.script("script/test/nt");
    fixture.skill("skill/rust/review");
    fixture.hook("hook/gate/secrets");
    fixture.guidance("guidance/mode/research");

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.problems.is_empty(), "{:?}", load.problems);
    let ids: Vec<String> = load
        .catalog
        .capsules()
        .iter()
        .map(|c| c.id.to_string())
        .collect();
    assert_eq!(
        ids,
        vec![
            "guidance/mode/research",
            "hook/gate/secrets",
            "script/test/nt",
            "skill/rust/review",
        ]
    );
}

#[test]
fn a_capsule_nested_deeper_than_group_slash_name_is_still_found() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.script("script/team/payments/reconcile");

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load
        .catalog
        .get(&cid("script/team/payments/reconcile"))
        .is_some());
}

#[test]
fn an_empty_registry_loads_as_an_empty_catalog_rather_than_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.catalog.capsules().is_empty());
    assert!(load.problems.is_empty());
}

#[test]
fn a_registry_directory_that_does_not_exist_yet_is_not_an_error() {
    // A fresh install has no registries. Refusing to start would be worse than
    // reporting an empty catalog.
    let tmp = tempfile::tempdir().unwrap();
    let load = load_registry(
        &tmp.path().join("never-created"),
        RegistrySource::personal(),
    )
    .unwrap();
    assert!(load.catalog.is_empty());
}

// ---------------------------------------------------------------------------
// The content revision
// ---------------------------------------------------------------------------

#[test]
fn editing_a_payload_file_changes_the_revision() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.script("script/test/nt");

    let before = load_registry(fixture.root(), RegistrySource::personal())
        .unwrap()
        .catalog
        .get(&cid("script/test/nt"))
        .unwrap()
        .revision
        .clone()
        .unwrap();

    fixture.write_payload(
        "script/test/nt",
        "payload/run.sh",
        "#!/bin/sh\necho pwned\n",
    );

    let after = load_registry(fixture.root(), RegistrySource::personal())
        .unwrap()
        .catalog
        .get(&cid("script/test/nt"))
        .unwrap()
        .revision
        .clone()
        .unwrap();

    assert_ne!(
        before, after,
        "a payload edit must produce a new revision, or trust would survive it"
    );
}

#[test]
fn adding_a_payload_file_changes_the_revision() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.script("script/test/nt");
    let before = revision_of(&fixture, "script/test/nt");

    fixture.write_payload("script/test/nt", "payload/lib/helper.sh", "true\n");
    assert_ne!(before, revision_of(&fixture, "script/test/nt"));
}

#[test]
fn moving_a_payload_file_changes_the_revision_even_when_the_bytes_are_the_same() {
    // Hashing contents without paths would call these two trees identical.
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.script("script/test/nt");
    fixture.write_payload("script/test/nt", "payload/a", "same\n");
    let before = revision_of(&fixture, "script/test/nt");

    std::fs::remove_file(fixture.capsule_dir("script/test/nt").join("payload/a")).unwrap();
    fixture.write_payload("script/test/nt", "payload/b", "same\n");
    assert_ne!(before, revision_of(&fixture, "script/test/nt"));
}

#[cfg(unix)]
#[test]
fn changing_executable_permissions_changes_the_trust_revision() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.script("script/test/nt");
    let before = revision_of(&fixture, "script/test/nt");
    let script = fixture.capsule_dir("script/test/nt").join("payload/run.sh");
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(permissions.mode() ^ 0o100);
    std::fs::set_permissions(&script, permissions).unwrap();

    assert_ne!(
        before,
        revision_of(&fixture, "script/test/nt"),
        "permission-only executable changes must require a new trust decision"
    );
}

#[test]
fn editing_the_manifest_changes_the_revision() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.script("script/test/nt");
    let before = revision_of(&fixture, "script/test/nt");

    fixture.capsule(
        "script/test/nt",
        "script",
        "entry = \"payload/run.sh\"",
        "tags = [\"testing\"]",
        &[],
    );
    assert_ne!(before, revision_of(&fixture, "script/test/nt"));
}

#[test]
fn an_unchanged_capsule_has_a_stable_revision_across_loads_and_across_registries() {
    let tmp = tempfile::tempdir().unwrap();
    let one = RegistryFixture::at(tmp.path().join("one"));
    let two = RegistryFixture::at(tmp.path().join("two"));
    one.script("script/test/nt");
    two.script("script/test/nt");

    assert_eq!(
        revision_of(&one, "script/test/nt"),
        revision_of(&one, "script/test/nt")
    );
    assert_eq!(
        revision_of(&one, "script/test/nt"),
        revision_of(&two, "script/test/nt"),
        "the revision is content identity, not location identity"
    );
}

fn revision_of(fixture: &RegistryFixture, id: &str) -> String {
    load_registry(fixture.root(), RegistrySource::personal())
        .unwrap()
        .catalog
        .get(&cid(id))
        .unwrap_or_else(|| panic!("{id} should be in the fixture registry"))
        .revision
        .clone()
        .unwrap()
        .to_string()
}

// ---------------------------------------------------------------------------
// Per-file problems
// ---------------------------------------------------------------------------

#[test]
fn one_unparseable_manifest_does_not_blind_the_rest_of_the_registry() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.script("script/test/good");
    fixture.raw_capsule(
        "script/test/broken",
        "schema = 1\nid = \"script/test/broken\"\nthis is not toml",
    );

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();

    assert!(load.catalog.get(&cid("script/test/good")).is_some());
    assert_eq!(load.problems.len(), 1);
    assert_eq!(load.problems[0].error.code(), "manifest.parse_error");
    assert!(load.problems[0].path.ends_with("manifest.toml"));
}

#[test]
fn a_manifest_whose_id_disagrees_with_its_directory_is_a_problem_not_a_silent_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.raw_capsule(
        "script/test/on-disk",
        "schema = 1\nid = \"script/test/in-manifest\"\nkind = \"script\"\n\
         name = \"x\"\ndescription = \"Mismatched.\"\n\n[script]\nentry = \"payload/run.sh\"\n",
    );

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.catalog.is_empty());
    assert_eq!(load.problems.len(), 1);
    assert_eq!(load.problems[0].error.code(), "registry.id_path_mismatch");
}

#[test]
fn a_manifest_that_declares_its_own_trust_is_refused_by_the_loader_too() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.raw_capsule(
        "script/test/sneaky",
        "schema = 1\nid = \"script/test/sneaky\"\nkind = \"script\"\nname = \"x\"\n\
         description = \"Declares trust.\"\ntrust = \"trusted\"\n\n[script]\nentry = \"payload/run.sh\"\n",
    );

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.catalog.is_empty());
    assert_eq!(
        load.problems[0].error.code(),
        "manifest.trust_not_self_declarable"
    );
}

#[test]
fn a_directory_without_a_manifest_is_ignored_rather_than_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.script("script/test/nt");
    std::fs::create_dir_all(tmp.path().join("capsules/script/test/nt/payload/nested")).unwrap();
    std::fs::create_dir_all(tmp.path().join("capsules/skill")).unwrap();

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert_eq!(load.catalog.len(), 1);
    assert!(load.problems.is_empty(), "{:?}", load.problems);
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

#[test]
fn profiles_are_loaded_from_the_profiles_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.profile(
        "profile/code/rust",
        "description = \"Rust baseline.\"\nenable = [\"script/test/nt\"]\n",
    );

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    let profile = load.catalog.profile(&pid("profile/code/rust")).unwrap();
    assert_eq!(profile.description, "Rust baseline.");
    assert_eq!(profile.patch.enable, vec![cid("script/test/nt")]);
}

#[test]
fn a_profile_whose_id_disagrees_with_its_path_is_a_problem() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    let path = tmp.path().join("profiles/code/rust.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "schema = 1\nid = \"profile/code/python\"\n").unwrap();

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.catalog.profiles().is_empty());
    assert_eq!(load.problems[0].error.code(), "registry.id_path_mismatch");
}

#[test]
fn a_broken_profile_does_not_stop_a_good_one_loading() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = RegistryFixture::at(tmp.path());
    fixture.profile("profile/code/rust", "enable = [\"script/test/nt\"]");
    let bad = tmp.path().join("profiles/code/broken.toml");
    std::fs::write(
        &bad,
        "schema = 1\nid = \"profile/code/broken\"\nenable = 3\n",
    )
    .unwrap();

    let load = load_registry(fixture.root(), RegistrySource::personal()).unwrap();
    assert!(load.catalog.profile(&pid("profile/code/rust")).is_some());
    assert_eq!(load.problems.len(), 1);
}

// ---------------------------------------------------------------------------
// The project-local registry
// ---------------------------------------------------------------------------

#[test]
fn a_project_local_registry_is_loaded_from_dot_aikit_and_stamped_project_local() {
    let repo = tempfile::tempdir().unwrap();
    let local = RegistryFixture::at(repo.path().join(".aikit"));
    local.script("script/project/deploy");

    let load = load_project_local(repo.path()).unwrap();
    let capsule = load.catalog.get(&cid("script/project/deploy")).unwrap();
    let source = capsule.source.clone().unwrap();

    assert_eq!(source, RegistrySource::project_local());
    assert!(
        source.is_project_local(),
        "a project-local capsule must be distinguishable, because its trust is separate"
    );
}

#[test]
fn a_repository_without_a_dot_aikit_capsules_tree_yields_an_empty_project_registry() {
    let repo = tempfile::tempdir().unwrap();
    let load = load_project_local(repo.path()).unwrap();
    assert!(load.catalog.is_empty());
    assert!(load.problems.is_empty());
}

#[test]
fn identical_content_in_two_registries_shares_a_revision_but_not_a_trust_key() {
    // This is the property that makes "project-local trust never inherits the
    // personal registry's" enforceable: the revision matches, so nothing else
    // would distinguish them; the source is what keeps the keys apart.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let personal = RegistryFixture::at(tmp.path());
    let local = RegistryFixture::at(repo.path().join(".aikit"));
    personal.script("script/test/nt");
    local.script("script/test/nt");

    let personal_load = load_registry(personal.root(), RegistrySource::personal()).unwrap();
    let local_load = load_project_local(repo.path()).unwrap();

    let a = personal_load.catalog.get(&cid("script/test/nt")).unwrap();
    let b = local_load.catalog.get(&cid("script/test/nt")).unwrap();

    assert_eq!(a.revision, b.revision);
    assert_ne!(a.source, b.source);
    assert_ne!(
        aikit_core::TrustKey::new(
            a.source.clone().unwrap(),
            a.id.clone(),
            a.revision.clone().unwrap()
        ),
        aikit_core::TrustKey::new(
            b.source.clone().unwrap(),
            b.id.clone(),
            b.revision.clone().unwrap()
        )
    );
}

// ---------------------------------------------------------------------------
// Merging
// ---------------------------------------------------------------------------

#[test]
fn merging_layers_a_project_registry_over_a_personal_one_and_records_the_shadow() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let personal = RegistryFixture::at(tmp.path());
    let local = RegistryFixture::at(repo.path().join(".aikit"));
    personal.script("script/test/nt");
    personal.script("script/test/only-personal");
    local.capsule(
        "script/test/nt",
        "script",
        "entry = \"payload/run.sh\"",
        "tags = [\"project\"]",
        &[("payload/run.sh", "#!/bin/sh\necho project\n")],
    );

    let mut load = load_registry(personal.root(), RegistrySource::personal()).unwrap();
    let shadowed = load.merge(load_project_local(repo.path()).unwrap());

    let winner = load.catalog.get(&cid("script/test/nt")).unwrap();
    assert_eq!(
        winner.source.as_ref().unwrap(),
        &RegistrySource::project_local(),
        "the nearer registry wins"
    );
    assert!(load
        .catalog
        .get(&cid("script/test/only-personal"))
        .is_some());
    assert_eq!(shadowed, vec![cid("script/test/nt")]);
}

#[test]
fn merging_carries_both_registries_problems_forward() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let personal = RegistryFixture::at(tmp.path());
    let local = RegistryFixture::at(repo.path().join(".aikit"));
    personal.raw_capsule("script/test/a", "not toml at all");
    local.raw_capsule("script/test/b", "also not toml");

    let mut load = load_registry(personal.root(), RegistrySource::personal()).unwrap();
    load.merge(load_project_local(repo.path()).unwrap());
    assert_eq!(load.problems.len(), 2);
}
