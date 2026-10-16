//! The walk's named skips against a real filesystem: each rule leaves exactly
//! its directories unlisted, material beside them is still found, and a
//! skip decided by a file's bytes is observed by the discovery ledger.

use super::walk::{named_skip, WalkSkip, WorldPosition, CACHEDIR_TAG, CACHEDIR_TAG_SIGNATURE};
use super::*;

fn scratch() -> tempfile::TempDir {
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ProjectCentral/now/tmp");
    fs::create_dir_all(&scratch).unwrap();
    tempfile::Builder::new()
        .prefix("knowledge-walk-")
        .tempdir_in(&scratch)
        .unwrap()
}

fn source(reference: &str) -> String {
    serde_json::json!({
        "binding": {
            "source": reference, "revision": "rev-1", "title": reference, "tags": [],
            "visibility": "public", "owners": [], "media_type": "text/markdown", "metadata": {}
        },
        "body": reference,
    })
    .to_string()
}

fn place(root: &Path, relative: &str, reference: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source(reference)).unwrap();
}

fn tag(directory: &Path, bytes: &[u8]) {
    fs::create_dir_all(directory).unwrap();
    fs::write(directory.join(CACHEDIR_TAG), bytes).unwrap();
}

fn valid_tag() -> Vec<u8> {
    let mut bytes = CACHEDIR_TAG_SIGNATURE.to_vec();
    bytes.extend_from_slice(b"\n# This file is a cache directory tag created by cargo.\n");
    bytes
}

/// The source refs a bare walk finds.
fn found(root: &Path, world: Option<&Path>, discover_wiki: bool) -> BTreeSet<String> {
    let home = scratch();
    let mut absences = Vec::new();
    discover_material(root, home.path(), &mut absences, discover_wiki, world)
        .unwrap()
        .sources
        .into_keys()
        .map(|source| source.as_str().to_owned())
        .collect()
}

fn refs(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn central_machine_state_is_never_walked() {
    let root = scratch();
    place(
        root.path(),
        ".central/horizon.json",
        "source:walk:machine-state",
    );
    place(root.path(), "notes/source.json", "source:walk:notes");
    assert_eq!(found(root.path(), None, true), refs(&["source:walk:notes"]));
}

#[test]
fn a_cache_tagged_directory_is_not_walked_but_a_forged_tag_is_no_tag() {
    let root = scratch();
    place(root.path(), "build/out/source.json", "source:walk:cache");
    tag(&root.path().join("build"), &valid_tag());
    place(root.path(), "forged/source.json", "source:walk:forged");
    tag(
        &root.path().join("forged"),
        b"Signature: not the cache directory tag",
    );
    place(root.path(), "plain/source.json", "source:walk:plain");
    assert_eq!(
        found(root.path(), None, true),
        refs(&["source:walk:forged", "source:walk:plain"])
    );
}

#[test]
fn a_cache_tag_the_walk_read_is_observed_by_the_ledger() {
    let root = scratch();
    let home = scratch();
    place(root.path(), "build/source.json", "source:walk:cache");
    let build = root.path().join("build");
    tag(&build, &valid_tag());
    std::thread::sleep(std::time::Duration::from_millis(2_100));
    let through = |root: &Path| {
        let mut absences = Vec::new();
        discover_material_through_ledger(
            root,
            home.path(),
            &home.path().join("cache"),
            &mut absences,
            true,
            None,
        )
        .unwrap()
        .sources
        .into_keys()
        .map(|source| source.as_str().to_owned())
        .collect::<BTreeSet<_>>()
    };
    assert!(through(root.path()).is_empty());
    let walked = ledger::WALKS.with(std::cell::Cell::get);
    assert!(through(root.path()).is_empty());
    assert_eq!(
        ledger::WALKS.with(std::cell::Cell::get),
        walked,
        "the kept walk answers while the tag reads the same"
    );

    // Rewritten in place, same length: only the tag's own stamps change.
    let mut forged = valid_tag();
    forged[0] = b's';
    fs::write(build.join(CACHEDIR_TAG), &forged).unwrap();
    assert_eq!(through(root.path()), refs(&["source:walk:cache"]));
    assert_eq!(ledger::WALKS.with(std::cell::Cell::get), walked + 1);
}

#[test]
fn outside_a_world_the_now_field_and_the_root_members_are_still_walked() {
    let world = scratch();
    place(
        world.path(),
        "Control/agents/now/agents/return.json",
        "source:walk:root-now",
    );
    place(
        world.path(),
        "worktrees/seat/source.json",
        "source:walk:seat",
    );
    place(
        world.path(),
        "Work/P/ProjectCentral/now/return.json",
        "source:walk:project-now",
    );
    assert_eq!(
        found(world.path(), None, true),
        refs(&[
            "source:walk:root-now",
            "source:walk:seat",
            "source:walk:project-now"
        ])
    );
}

#[test]
fn inside_a_world_a_project_walk_leaves_its_now_field_to_the_now_provider() {
    let world = scratch();
    fs::create_dir_all(world.path().join("Control")).unwrap();
    let project = world.path().join("Work/P");
    place(
        &project,
        "ProjectCentral/now/2026-10-16/return.json",
        "source:walk:project-now",
    );
    place(
        &project,
        "ProjectCentral/user/source.json",
        "source:walk:project-user",
    );
    place(&project, "notes/source.json", "source:walk:project-notes");
    assert_eq!(
        found(&project, Some(world.path()), false),
        refs(&["source:walk:project-user", "source:walk:project-notes"])
    );
}

#[test]
fn a_world_root_walks_its_root_register_and_none_of_the_worlds_other_members() {
    let world = scratch();
    place(
        world.path(),
        "Control/user/source.json",
        "source:walk:control",
    );
    place(
        world.path(),
        "Control/agents/now/agents/return.json",
        "source:walk:root-now",
    );
    place(world.path(), "Work/P/source.json", "source:walk:project");
    place(
        world.path(),
        "worktrees/env/product/source.json",
        "source:walk:seat",
    );
    place(world.path(), ".claude/source.json", "source:walk:harness");
    place(world.path(), "top.json", "source:walk:root-file");
    assert_eq!(
        found(world.path(), Some(world.path()), false),
        refs(&["source:walk:control", "source:walk:root-file"])
    );
}

#[test]
fn every_now_provider_family_the_walk_could_read_lies_in_a_field_it_leaves_to_the_provider() {
    let world = Path::new("/world");
    let scope = aikit_adapters::now_field::NowFieldScope::standard(world);
    let mut checked = 0;
    for include in &scope.includes {
        let glob = include.glob.as_str();
        // The walk reads only JSON; a Markdown-only family is never walked.
        if !(glob.ends_with(".json") || glob.ends_with("/**")) {
            continue;
        }
        let (field, root, position) = if let Some(rest) = glob.strip_prefix("Work/*/") {
            assert!(rest.starts_with("ProjectCentral/now/"), "{glob}");
            let root = world.join("Work/P");
            (root.join("ProjectCentral/now"), root, WorldPosition::Member)
        } else {
            assert!(glob.starts_with("Control/agents/now/"), "{glob}");
            (
                world.join("Control/agents/now"),
                world.to_path_buf(),
                WorldPosition::Root,
            )
        };
        assert_eq!(
            named_skip(&field, field.parent().unwrap(), &root, position),
            Some(WalkSkip::LiveNowField),
            "{glob}"
        );
        checked += 1;
    }
    assert!(
        checked >= 4,
        "the standard scope's JSON families were checked"
    );
}

#[test]
fn the_named_rules_answer_by_name_and_place_only() {
    let world = Path::new("/world");
    let control = world.join("Control");
    assert_eq!(
        named_skip(&control, world, world, WorldPosition::Root),
        None
    );
    assert_eq!(
        named_skip(&world.join("Work"), world, world, WorldPosition::Root),
        Some(WalkSkip::OutsideRootRegister)
    );
    // Only the World root's own listing: a `Work` deeper down is ordinary.
    assert_eq!(
        named_skip(&control.join("Work"), &control, world, WorldPosition::Root),
        None
    );
    assert_eq!(
        named_skip(&world.join("Work"), world, world, WorldPosition::Member),
        None
    );
    assert_eq!(
        named_skip(
            &control.join(".central"),
            &control,
            world,
            WorldPosition::Standalone
        ),
        Some(WalkSkip::CentralMachineState)
    );
    // A `now` that is not a register's NOW field is ordinary material.
    assert_eq!(
        named_skip(
            &control.join("user/now"),
            &control.join("user"),
            world,
            WorldPosition::Root
        ),
        None
    );
}
