//! The on-disk home.
//!
//! Every test here builds a real directory tree in a real temporary directory.
//! None of them may touch the developer's `~/.aikit`: an accessor that silently
//! fell back to the real home would be a data-loss bug, so the tests assert the
//! override is honoured rather than assuming it.

use std::fs;

use aikit_store::home::AikitHome;

#[test]
fn an_explicit_root_is_used_verbatim() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path());
    assert_eq!(home.root(), tmp.path());
}

#[test]
fn the_aikit_home_environment_variable_wins_over_the_user_home() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::from_env_values(
        Some(tmp.path().as_os_str()),
        Some(std::ffi::OsStr::new("/should/not/be/used")),
    )
    .unwrap();
    assert_eq!(home.root(), tmp.path());
}

#[test]
fn without_an_override_the_home_is_dot_aikit_under_the_user_home() {
    let home =
        AikitHome::from_env_values(None, Some(std::ffi::OsStr::new("/home/tester"))).unwrap();
    assert_eq!(home.root(), std::path::Path::new("/home/tester/.aikit"));
}

#[test]
fn with_neither_an_override_nor_a_home_the_error_says_which_variable_to_set() {
    let error = AikitHome::from_env_values(None, None).unwrap_err();
    assert_eq!(error.code(), "home.not_found");
    assert!(
        error.message().contains("AIKIT_HOME"),
        "the message must name the escape hatch, got: {}",
        error.message()
    );
}

#[test]
fn the_documented_layout_is_created_and_creating_it_twice_is_harmless() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path());
    home.ensure_layout().unwrap();

    // Drop a file in so a second `ensure_layout` has something to destroy if it
    // were implemented as "remove and recreate".
    let marker = home.inbox_ready().join("keep-me");
    fs::write(&marker, b"keep").unwrap();

    home.ensure_layout().unwrap();
    assert_eq!(fs::read(&marker).unwrap(), b"keep");

    for expected in [
        home.registries(),
        home.profiles(),
        home.inbox_ready(),
        home.inbox_quarantine(),
        home.inbox_rejected(),
        home.contexts(),
        home.sessions(),
        home.locks(),
        home.trust_dir(),
        home.cache(),
        home.logs(),
    ] {
        assert!(
            expected.is_dir(),
            "{} should have been created",
            expected.display()
        );
    }
}

#[test]
fn every_path_the_home_offers_is_inside_the_home() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path());
    for path in [
        home.config_file(),
        home.database(),
        home.event_log(),
        home.registry("personal"),
        home.registry_capsules("personal"),
        home.context_dir(&aikit_core::ContextId::generate()),
        home.session_dir(&aikit_core::SessionId::generate()),
        home.lock_file("ctx_abc"),
    ] {
        assert!(
            path.starts_with(tmp.path()),
            "{} escaped the home root",
            path.display()
        );
    }
}

#[test]
fn the_documented_file_names_are_the_ones_the_specification_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path());
    assert_eq!(home.config_file(), tmp.path().join("config.toml"));
    assert_eq!(
        home.database(),
        tmp.path().join("state").join("aikit.sqlite3")
    );
    assert_eq!(
        home.event_log(),
        tmp.path().join("logs").join("events.jsonl")
    );
    assert_eq!(
        home.registry_capsules("personal"),
        tmp.path()
            .join("registries")
            .join("personal")
            .join("capsules")
    );
}

#[test]
fn a_session_overlay_lives_under_the_session_it_belongs_to() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path());
    let session = aikit_core::SessionId::generate();
    assert_eq!(
        home.session_overlay(&session),
        home.session_dir(&session).join("overlay.toml")
    );
}

#[test]
fn ensuring_a_context_directory_creates_its_generations_directory_too() {
    let tmp = tempfile::tempdir().unwrap();
    let home = AikitHome::at(tmp.path());
    let ctx = aikit_core::ContextId::generate();
    let dir = home.ensure_context_dir(&ctx).unwrap();

    assert!(dir.is_dir());
    assert!(dir.join("generations").is_dir());
    assert_eq!(dir, home.context_dir(&ctx));
}

#[test]
fn a_home_whose_folder_is_gone_is_refused_and_a_first_run_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    // A first run: the folder that holds the home stands; the home does not exist yet.
    let first = AikitHome::at(tmp.path().join(".aikit"));
    first.ground_present().unwrap();
    first.ensure_layout().unwrap();
    first.ground_present().unwrap();

    // A World's home once its World has been removed.
    let world = tmp.path().join("world");
    fs::create_dir_all(world.join("home")).unwrap();
    let gone = AikitHome::at(world.join("home").join(".aikit"));
    gone.ground_present().unwrap();
    fs::remove_dir_all(&world).unwrap();
    let refused = gone.ground_present().unwrap_err();
    assert_eq!(refused.code(), "home.ground_gone", "{refused:?}");
    // The folder is named on its own, in the parentheses after the home's path (which holds it too).
    let named = format!("({})", world.join("home").display());
    assert!(
        refused.message().contains(&named),
        "the refusal names the folder that is gone, {named}: {}",
        refused.message()
    );
    assert!(!world.exists(), "checking the ground created nothing");

    // A relative home stands in the working directory, which is there.
    AikitHome::at(".aikit-not-created")
        .ground_present()
        .unwrap();
}

/// Laying out a home makes the home and what lies in it, never the folder that holds it: once a
/// World has been removed, no command that opens AIKit brings it back by laying out its home, and
/// a home removed after it was laid out has none of its own folders made again (O-I #1001).
#[test]
fn a_home_is_never_laid_out_in_a_world_that_is_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let world = tmp.path().join("world");
    let gone = AikitHome::at(world.join("home").join(".aikit"));
    let refused = gone.ensure_layout().unwrap_err();
    assert_eq!(refused.code(), "home.ground_gone", "{refused:?}");
    assert!(
        !world.exists(),
        "laying out the home made the removed World again"
    );

    fs::create_dir_all(world.join("home")).unwrap();
    gone.ensure_layout().unwrap();
    fs::remove_dir_all(&world).unwrap();
    let context = aikit_core::ContextId::generate();
    let refused = gone.ensure_context_dir(&context).unwrap_err();
    assert_eq!(refused.code(), "home.ground_gone", "{refused:?}");
    let session = aikit_core::SessionId::generate();
    let refused = gone.ensure_session_dir(&session).unwrap_err();
    assert_eq!(refused.code(), "home.ground_gone", "{refused:?}");
    assert!(
        !world.exists(),
        "a context's or a session's folder made the removed World again"
    );
}
