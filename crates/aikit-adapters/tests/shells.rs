//! Shell integration, checked by running it through real shells.
//!
//! A shell snippet is the one piece of AIKit that ends up inside somebody's
//! `.bashrc` and runs on every single shell start, forever. Two properties
//! therefore matter more than anything else it does:
//!
//! * **it is idempotent.** Sourced twice — which happens constantly, via nested
//!   shells, `exec bash`, tmux, and rc files that source each other — it must not
//!   prepend the same directory to `PATH` a second time, and it must not register
//!   the directory-change hook twice.
//! * **it cannot break the shell.** If `aikit` is not on `PATH`, or fails, the
//!   snippet still has to leave the user with a working prompt.
//!
//! Both are tested by actually running `bash` and `zsh`, not by inspecting the
//! text. Where a shell is not installed the test prints a skip line rather than
//! failing.

use std::path::Path;
use std::process::Command;

use aikit_adapters::shells::{self, Shell};

// ---------------------------------------------------------------------------
// Availability
// ---------------------------------------------------------------------------

fn shell_binary(name: &str) -> Option<String> {
    let out = Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then_some(path)
}

macro_rules! require_shell {
    ($name:literal, $test:literal) => {
        match shell_binary($name) {
            Some(path) => path,
            None => {
                eprintln!(
                    "SKIP {}: {} is not installed on this machine, so its snippet was not \
                     executed",
                    $test, $name
                );
                return;
            }
        }
    };
}

/// Write a snippet to a file and return its path.
fn snippet_file(dir: &Path, shell: Shell) -> std::path::PathBuf {
    let path = dir.join(format!("init.{}", shell.as_str()));
    std::fs::write(&path, shells::init_snippet(shell)).unwrap();
    path
}

/// Run a script through a shell with a controlled environment, returning stdout.
fn run(shell_path: &str, script: &str, view: &Path) -> (bool, String, String) {
    let out = Command::new(shell_path)
        .arg("-c")
        .arg(script)
        .env("AIKIT_VIEW", view)
        // A PATH without `aikit` on it, so the "aikit is not installed" case is
        // the one being exercised.
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", view)
        .output()
        .unwrap_or_else(|e| panic!("could not run {shell_path}: {e}"));
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

// ---------------------------------------------------------------------------
// The snippets exist and say what they should
// ---------------------------------------------------------------------------

#[test]
fn every_supported_shell_has_a_snippet_that_puts_the_view_bin_on_the_path() {
    for shell in Shell::ALL {
        let snippet = shells::init_snippet(shell);
        assert!(!snippet.is_empty(), "{shell:?} has no snippet");
        assert!(
            snippet.contains("AIKIT_VIEW"),
            "{shell:?} never mentions the view root:\n{snippet}"
        );
        assert!(
            snippet.contains("bin"),
            "{shell:?} never mentions the bin directory"
        );
    }
}

#[test]
fn the_fish_snippet_is_written_in_fish_rather_than_in_posix_sh() {
    // fish is not POSIX, and a snippet that pretended otherwise would fail on the
    // first line in a way that is hard to read.
    let snippet = shells::init_snippet(Shell::Fish);

    assert!(snippet.contains("set -gx PATH"), "got:\n{snippet}");
    assert!(snippet.contains("--on-variable PWD"));
    assert!(
        !snippet.contains("export "),
        "`export` is not fish syntax:\n{snippet}"
    );
    assert!(
        !snippet.contains("PROMPT_COMMAND"),
        "PROMPT_COMMAND is a bash concept"
    );
}

#[test]
fn the_bash_and_zsh_snippets_use_the_hook_mechanism_each_shell_actually_has() {
    assert!(shells::init_snippet(Shell::Bash).contains("PROMPT_COMMAND"));
    assert!(
        shells::init_snippet(Shell::Zsh).contains("chpwd_functions"),
        "zsh has a real directory-change hook; abusing PROMPT_COMMAND there would run the \
         update on every prompt instead"
    );
}

// ---------------------------------------------------------------------------
// Syntax, according to the shells themselves
// ---------------------------------------------------------------------------

#[test]
fn the_bash_snippet_is_syntactically_valid_bash() {
    let bash = require_shell!("bash", "the_bash_snippet_is_syntactically_valid_bash");
    let dir = tempfile::tempdir().unwrap();
    let path = snippet_file(dir.path(), Shell::Bash);

    let out = Command::new(&bash).arg("-n").arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "bash -n rejected the snippet: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_zsh_snippet_is_syntactically_valid_zsh() {
    let zsh = require_shell!("zsh", "the_zsh_snippet_is_syntactically_valid_zsh");
    let dir = tempfile::tempdir().unwrap();
    let path = snippet_file(dir.path(), Shell::Zsh);

    let out = Command::new(&zsh).arg("-n").arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "zsh -n rejected the snippet: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_fish_snippet_is_syntactically_valid_fish() {
    let fish = require_shell!("fish", "the_fish_snippet_is_syntactically_valid_fish");
    let dir = tempfile::tempdir().unwrap();
    let path = snippet_file(dir.path(), Shell::Fish);

    let out = Command::new(&fish).arg("-n").arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "fish -n rejected the snippet: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---------------------------------------------------------------------------
// Idempotence, by actually sourcing it twice
// ---------------------------------------------------------------------------

fn path_entries_for(shell_path: &str, shell: Shell, sourcings: usize) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let view = dir.path().join("view");
    std::fs::create_dir_all(view.join("bin")).unwrap();
    let snippet = snippet_file(dir.path(), shell);

    let source = format!(". {}\n", snippet.display()).repeat(sourcings);
    let (ok, stdout, stderr) = run(
        shell_path,
        &format!("{source}printf '%s' \"$PATH\""),
        &view,
    );
    assert!(ok, "sourcing the snippet failed: {stderr}");

    let wanted = view.join("bin").display().to_string();
    stdout
        .split(':')
        .filter(|entry| *entry == wanted)
        .map(str::to_string)
        .collect()
}

#[test]
fn sourcing_the_bash_snippet_twice_puts_the_bin_directory_on_the_path_once() {
    let bash = require_shell!(
        "bash",
        "sourcing_the_bash_snippet_twice_puts_the_bin_directory_on_the_path_once"
    );

    assert_eq!(path_entries_for(&bash, Shell::Bash, 1).len(), 1);
    assert_eq!(
        path_entries_for(&bash, Shell::Bash, 2).len(),
        1,
        "a second sourcing duplicated the PATH entry"
    );
    assert_eq!(
        path_entries_for(&bash, Shell::Bash, 5).len(),
        1,
        "nested shells source rc files more than twice"
    );
}

#[test]
fn sourcing_the_zsh_snippet_twice_puts_the_bin_directory_on_the_path_once() {
    let zsh = require_shell!(
        "zsh",
        "sourcing_the_zsh_snippet_twice_puts_the_bin_directory_on_the_path_once"
    );

    assert_eq!(path_entries_for(&zsh, Shell::Zsh, 1).len(), 1);
    assert_eq!(path_entries_for(&zsh, Shell::Zsh, 3).len(), 1);
}

#[test]
fn sourcing_the_snippet_twice_registers_the_directory_hook_once() {
    let bash = require_shell!(
        "bash",
        "sourcing_the_snippet_twice_registers_the_directory_hook_once"
    );
    let dir = tempfile::tempdir().unwrap();
    let view = dir.path().join("view");
    std::fs::create_dir_all(view.join("bin")).unwrap();
    let snippet = snippet_file(dir.path(), Shell::Bash);

    let script = format!(
        ". {s}\n. {s}\n. {s}\nprintf '%s' \"$PROMPT_COMMAND\"",
        s = snippet.display()
    );
    let (ok, stdout, stderr) = run(&bash, &script, &view);
    assert!(ok, "{stderr}");

    assert_eq!(
        stdout.matches("__aikit_chpwd").count(),
        1,
        "the hook was registered more than once, so it would run N times per prompt: {stdout}"
    );
}

#[test]
fn the_snippet_prepends_rather_than_replacing_what_was_already_on_the_path() {
    let bash = require_shell!(
        "bash",
        "the_snippet_prepends_rather_than_replacing_what_was_already_on_the_path"
    );
    let dir = tempfile::tempdir().unwrap();
    let view = dir.path().join("view");
    std::fs::create_dir_all(view.join("bin")).unwrap();
    let snippet = snippet_file(dir.path(), Shell::Bash);

    let (ok, stdout, stderr) = run(
        &bash,
        &format!(". {}\nprintf '%s' \"$PATH\"", snippet.display()),
        &view,
    );
    assert!(ok, "{stderr}");

    let wanted = view.join("bin").display().to_string();
    assert!(
        stdout.starts_with(&wanted),
        "the contextual bin directory has to win over the ambient one: {stdout}"
    );
    assert!(
        stdout.contains("/usr/bin"),
        "the user's own PATH must survive: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// It cannot break the shell
// ---------------------------------------------------------------------------

#[test]
fn the_snippet_survives_aikit_not_being_installed() {
    let bash = require_shell!("bash", "the_snippet_survives_aikit_not_being_installed");
    let dir = tempfile::tempdir().unwrap();
    let view = dir.path().join("view");
    std::fs::create_dir_all(view.join("bin")).unwrap();
    let snippet = snippet_file(dir.path(), Shell::Bash);

    // `run` deliberately supplies a PATH with no `aikit` on it.
    let (ok, stdout, stderr) = run(
        &bash,
        &format!(
            ". {}\n__aikit_chpwd\nprintf '%s' alive",
            snippet.display()
        ),
        &view,
    );
    assert!(ok, "the snippet broke a shell with no aikit on PATH: {stderr}");
    assert_eq!(stdout, "alive");
}

#[test]
fn the_snippet_does_nothing_at_all_when_there_is_no_view_to_point_at() {
    let bash = require_shell!(
        "bash",
        "the_snippet_does_nothing_at_all_when_there_is_no_view_to_point_at"
    );
    let dir = tempfile::tempdir().unwrap();
    let snippet = snippet_file(dir.path(), Shell::Bash);

    let out = Command::new(&bash)
        .arg("-c")
        .arg(format!(". {}\nprintf '%s' \"$PATH\"", snippet.display()))
        .env_remove("AIKIT_VIEW")
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();

    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "/usr/bin:/bin",
        "with no AIKIT_VIEW there is no contextual bin directory, and inventing one would put \
         a nonexistent path on every PATH lookup"
    );
}

// ---------------------------------------------------------------------------
// Shims
// ---------------------------------------------------------------------------

#[test]
fn a_shim_is_a_posix_wrapper_that_hands_the_arguments_straight_through() {
    let script = shells::shim_script("cargo-nextest").unwrap();

    assert!(script.starts_with("#!/bin/sh\n"));
    assert!(script.contains("exec aikit run"));
    assert!(
        script.contains("\"$@\""),
        "arguments must survive quoting, so `aikit run x -- --filter 'a b'` still works"
    );
}

#[test]
fn a_shim_is_syntactically_valid_sh() {
    let sh = require_shell!("sh", "a_shim_is_syntactically_valid_sh");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cargo-nextest");
    std::fs::write(&path, shells::shim_script("cargo-nextest").unwrap()).unwrap();

    let out = Command::new(&sh).arg("-n").arg(&path).output().unwrap();
    assert!(
        out.status.success(),
        "sh -n rejected the shim: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_shim_name_that_would_break_out_of_the_command_is_refused() {
    // The name is interpolated into a shell script. A name containing a quote
    // would be an injection, so it never gets that far.
    for bad in ["", "  ", "a'; rm -rf /; '", "../evil", "with\nnewline"] {
        let error = shells::shim_script(bad).unwrap_err();
        assert_eq!(
            error.code(),
            "shell.invalid_shim_name",
            "`{bad}` should have been refused"
        );
    }
    assert!(shells::shim_script("cargo-nextest").is_ok());
    assert!(shells::shim_script("nt").is_ok());
}

#[test]
fn a_shim_really_runs_and_forwards_its_arguments() {
    let sh = require_shell!("sh", "a_shim_really_runs_and_forwards_its_arguments");
    let dir = tempfile::tempdir().unwrap();

    // A fake `aikit` that prints what it was asked to do.
    let fake = dir.path().join("aikit");
    std::fs::write(&fake, "#!/bin/sh\nprintf '%s|' \"$@\"\n").unwrap();
    std::fs::set_permissions(
        &fake,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();

    let shim = dir.path().join("cargo-nextest");
    std::fs::write(&shim, shells::shim_script("cargo-nextest").unwrap()).unwrap();

    let out = Command::new(&sh)
        .arg(&shim)
        .args(["--filter", "a b"])
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
        .output()
        .unwrap();

    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "run|cargo-nextest|--filter|a b|",
        "an argument with a space in it must arrive as one argument"
    );
}

// ---------------------------------------------------------------------------
// The shell enum
// ---------------------------------------------------------------------------

#[test]
fn a_shell_can_be_named_the_way_a_person_would_name_it() {
    assert_eq!("bash".parse::<Shell>().unwrap(), Shell::Bash);
    assert_eq!("/bin/zsh".parse::<Shell>().unwrap(), Shell::Zsh);
    assert_eq!("/opt/homebrew/bin/fish".parse::<Shell>().unwrap(), Shell::Fish);
    assert_eq!(
        "tcsh".parse::<Shell>().unwrap_err().code(),
        "shell.unsupported"
    );
}
