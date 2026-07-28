//! Installing AIKit's tmux binding into a real `tmux.conf`.
//!
//! Editing somebody's shell or multiplexer config is an intrusion, so the rules
//! are strict: one clearly marked block, never a second copy, never a line
//! outside the markers, and never a key the user did not choose. A tool that
//! appends to `~/.tmux.conf` every time it runs is a tool people uninstall.

use std::path::Path;

use aikit_adapters::mux::tmux::{self, InstallAction, BLOCK_END, BLOCK_START};

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn installing_into_a_missing_config_creates_it_with_one_marked_block() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("tmux.conf");

    let outcome = tmux::install(&config, "a").unwrap();
    assert_eq!(outcome.action, InstallAction::Created);
    assert!(outcome.changed);

    let contents = read(&config);
    assert_eq!(occurrences(&contents, BLOCK_START), 1);
    assert_eq!(occurrences(&contents, BLOCK_END), 1);
    assert!(
        contents.contains("display-popup -E -w 82% -h 70% -d '#{pane_current_path}' -T AIKit"),
        "the block has to actually bind the palette: {contents}"
    );
}

#[test]
fn installing_twice_does_not_duplicate_the_block() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("tmux.conf");

    tmux::install(&config, "a").unwrap();
    let after_first = read(&config);

    let second = tmux::install(&config, "a").unwrap();
    assert_eq!(second.action, InstallAction::Unchanged);
    assert!(!second.changed, "nothing needed doing the second time");

    let after_second = read(&config);
    assert_eq!(
        after_first, after_second,
        "a second install must be a byte-for-byte no-op"
    );
    assert_eq!(occurrences(&after_second, BLOCK_START), 1);
}

#[test]
fn the_users_own_configuration_is_left_exactly_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("tmux.conf");
    let original = "set -g mouse on\nset -g prefix C-a\nbind-key a last-window\n";
    std::fs::write(&config, original).unwrap();

    tmux::install(&config, "e").unwrap();
    tmux::install(&config, "e").unwrap();

    let contents = read(&config);
    assert!(
        contents.starts_with(original),
        "the user's own lines must survive untouched and in order: {contents}"
    );
    assert_eq!(occurrences(&contents, BLOCK_START), 1);
}

#[test]
fn the_key_is_the_users_choice_because_a_default_would_collide() {
    // `bind-key a` is already `last-window` in the config above; a tool that
    // hard-coded its own key would silently steal someone's binding.
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("tmux.conf");

    tmux::install(&config, "C-Space").unwrap();
    let contents = read(&config);
    assert!(
        contents.contains("bind-key -n C-Space display-popup"),
        "got: {contents}"
    );
    assert!(!contents.contains("bind-key -n a "));
}

#[test]
fn changing_the_key_rewrites_the_block_in_place_rather_than_adding_another() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("tmux.conf");

    tmux::install(&config, "a").unwrap();
    let outcome = tmux::install(&config, "k").unwrap();

    assert_eq!(outcome.action, InstallAction::Updated);
    assert!(outcome.changed);

    let contents = read(&config);
    assert_eq!(occurrences(&contents, BLOCK_START), 1);
    assert!(contents.contains("bind-key -n k display-popup"));
    assert!(
        !contents.contains("bind-key -n a display-popup"),
        "the old binding has to go, or the user ends up with two palettes: {contents}"
    );
}

#[test]
fn the_managed_binding_opens_the_unified_surface() {
    let block = tmux::config_block("M-a");

    assert!(block.contains("bind-key -n M-a display-popup"));
    assert!(block.contains("'aikit ui'"));
    assert!(!block.contains("aikit palette"));
}

#[test]
fn the_block_is_removable_by_deleting_exactly_what_is_between_the_markers() {
    // The markers are a promise: everything between them is AIKit's, everything
    // outside them is the user's. Uninstall relies on it, and so does the user.
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("tmux.conf");
    std::fs::write(&config, "set -g mouse on\n").unwrap();
    tmux::install(&config, "a").unwrap();

    let contents = read(&config);
    let start = contents.find(BLOCK_START).unwrap();
    let end = contents.find(BLOCK_END).unwrap() + BLOCK_END.len();
    let without = format!("{}{}", &contents[..start], &contents[end..]);

    assert_eq!(without.trim(), "set -g mouse on");
}

#[test]
fn an_empty_key_is_refused_rather_than_written_as_a_broken_binding() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("tmux.conf");

    let error = tmux::install(&config, "  ").unwrap_err();
    assert_eq!(error.code(), "mux.invalid_key");
    assert!(
        !config.exists(),
        "a refused install must not leave a half-written config behind"
    );
}

#[test]
fn a_config_whose_directory_does_not_exist_yet_is_still_installable() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("nested/deeper/tmux.conf");

    tmux::install(&config, "a").unwrap();
    assert!(config.exists());
}
