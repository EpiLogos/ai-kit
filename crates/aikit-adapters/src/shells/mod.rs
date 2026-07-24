//! Shell integration.
//!
//! The init snippet is the one piece of AIKit that ends up inside somebody's
//! `.bashrc` and runs on every shell start for as long as it stays there. Two
//! properties therefore outrank everything else it does.
//!
//! ## It is idempotent
//!
//! Not "usually", and not "if you only source it once". Nested shells, `exec
//! bash`, tmux, direnv and rc files that source each other all mean a snippet is
//! evaluated several times per session in ordinary use. So the snippet never
//! blindly prepends: it checks `PATH` for the entry first, and it checks the hook
//! list before registering the directory-change hook. A guard variable alone
//! would not be enough — a sub-shell inherits the variable but not the `PATH`
//! edit, or the other way round, depending on how it was started.
//!
//! ## It cannot break the shell
//!
//! Every call out to `aikit` is guarded and its failure is swallowed. If AIKit is
//! not installed, is mid-upgrade, or errors, the user still gets a prompt. A
//! shell integration that can leave somebody without a working terminal is not
//! worth any feature it provides.
//!
//! ## Why there is a shim fallback
//!
//! The contextual `bin/` directory is normally populated with multicall symlinks
//! to the AIKit binary. Some filesystems, sync tools and container images handle
//! symlinks badly, and [`shim_script`] is the wrapper-script alternative: a
//! two-line POSIX `sh` file that execs `aikit run`.

use std::fmt;
use std::str::FromStr;

use aikit_core::{AikitError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    pub const ALL: [Shell; 3] = [Shell::Bash, Shell::Zsh, Shell::Fish];

    pub fn as_str(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
        }
    }

    /// The file a user is usually told to add the snippet to.
    pub fn rc_file(self) -> &'static str {
        match self {
            Shell::Bash => "~/.bashrc",
            Shell::Zsh => "~/.zshrc",
            Shell::Fish => "~/.config/fish/config.fish",
        }
    }

    /// The line that sources an installed snippet.
    pub fn source_line(self, path: &std::path::Path) -> String {
        match self {
            Shell::Fish => format!("source {}", path.display()),
            _ => format!(". {}", path.display()),
        }
    }
}

impl fmt::Display for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Shell {
    type Err = AikitError;

    /// Accepts a bare name or a path, because `$SHELL` is a path.
    fn from_str(s: &str) -> Result<Self> {
        let name = s.rsplit('/').next().unwrap_or(s).trim();
        Ok(match name {
            "bash" => Shell::Bash,
            "zsh" => Shell::Zsh,
            "fish" => Shell::Fish,
            other => {
                return Err(AikitError::new(
                    "shell.unsupported",
                    format!(
                        "`{other}` is not a shell AIKit has integration for (bash, zsh, fish); \
                         the contextual bin directory can still be put on PATH by hand"
                    ),
                )
                .with("shell", other))
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Init snippets
// ---------------------------------------------------------------------------

/// The integration snippet for a shell.
///
/// Safe to source any number of times.
pub fn init_snippet(shell: Shell) -> String {
    match shell {
        Shell::Bash => bash_snippet(),
        Shell::Zsh => zsh_snippet(),
        Shell::Fish => fish_snippet(),
    }
}

const PREAMBLE: &str = "\
# >>> aikit >>>
# Managed by AIKit. Safe to source more than once: every step below checks
# whether it has already happened.
";

const POSTAMBLE: &str = "# <<< aikit <<<\n";

fn bash_snippet() -> String {
    format!(
        "{PREAMBLE}
__aikit_path_prepend() {{
    # `case` rather than a regex: this runs on every shell start, and a
    # sub-process here is a measurable share of that budget.
    case \":${{PATH}}:\" in
        *\":$1:\"*) ;;
        *) PATH=\"$1:${{PATH}}\" ;;
    esac
}}

# Update the project context after a directory change.
#
# Every failure is swallowed. If aikit is missing, mid-upgrade, or broken, the
# user still gets a prompt — a shell integration that can strand somebody
# without a terminal is not worth what it provides.
__aikit_chpwd() {{
    [ -n \"${{AIKIT_VIEW:-}}\" ] || return 0
    __aikit_env=\"$(command aikit context env --shell bash --cwd \"$PWD\" 2>/dev/null)\" || return 0
    [ -n \"$__aikit_env\" ] && eval \"$__aikit_env\"
    return 0
}}

if [ -n \"${{AIKIT_VIEW:-}}\" ]; then
    __aikit_path_prepend \"${{AIKIT_VIEW}}/bin\"
    export PATH
    export AIKIT_SHELL=bash

    # Registered once. Bash has no directory-change hook, so PROMPT_COMMAND is
    # the only place to put this; appending a second copy would run the update
    # twice on every prompt.
    case \"${{PROMPT_COMMAND:-}}\" in
        *__aikit_chpwd*) ;;
        *) PROMPT_COMMAND=\"__aikit_chpwd${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}\" ;;
    esac

    __aikit_chpwd
fi
{POSTAMBLE}"
    )
}

fn zsh_snippet() -> String {
    format!(
        "{PREAMBLE}
__aikit_path_prepend() {{
    case \":${{PATH}}:\" in
        *\":$1:\"*) ;;
        *) PATH=\"$1:${{PATH}}\" ;;
    esac
}}

__aikit_chpwd() {{
    [ -n \"${{AIKIT_VIEW:-}}\" ] || return 0
    local __aikit_env
    __aikit_env=\"$(command aikit context env --shell zsh --cwd \"$PWD\" 2>/dev/null)\" || return 0
    [ -n \"$__aikit_env\" ] && eval \"$__aikit_env\"
    return 0
}}

if [ -n \"${{AIKIT_VIEW:-}}\" ]; then
    __aikit_path_prepend \"${{AIKIT_VIEW}}/bin\"
    export PATH
    export AIKIT_SHELL=zsh

    # zsh has a real directory-change hook, so the update runs on `cd` rather
    # than on every prompt redraw.
    typeset -ga chpwd_functions
    if [[ -z ${{chpwd_functions[(r)__aikit_chpwd]}} ]]; then
        chpwd_functions+=(__aikit_chpwd)
    fi

    __aikit_chpwd
fi
{POSTAMBLE}"
    )
}

fn fish_snippet() -> String {
    // fish is not POSIX. Its own idioms are used throughout rather than a
    // translation of the bash version, which would fail on the first line.
    format!(
        "{PREAMBLE}
function __aikit_chpwd --on-variable PWD --description 'Update the AIKit project context'
    if not set -q AIKIT_VIEW
        return 0
    end
    set -l rendered (command aikit context env --shell fish --cwd $PWD 2>/dev/null)
    or return 0
    for line in $rendered
        eval $line
    end
    return 0
end

if set -q AIKIT_VIEW
    if not contains -- \"$AIKIT_VIEW/bin\" $PATH
        set -gx PATH \"$AIKIT_VIEW/bin\" $PATH
    end
    set -gx AIKIT_SHELL fish
    __aikit_chpwd
end
{POSTAMBLE}"
    )
}

// ---------------------------------------------------------------------------
// Shims
// ---------------------------------------------------------------------------

/// A wrapper script for the contextual `bin/` directory.
///
/// The alternative to a multicall symlink, for filesystems and sync tools that
/// handle symlinks badly.
pub fn shim_script(name: &str) -> Result<String> {
    if !is_safe_command_name(name) {
        return Err(AikitError::new(
            "shell.invalid_shim_name",
            format!(
                "`{name}` is not a usable command name for a shim; the name is written into a \
                 shell script, so it may only contain letters, digits, `-`, `_` and `.`"
            ),
        )
        .with("name", name));
    }

    Ok(format!(
        "#!/bin/sh\n\
         # >>> aikit >>>\n\
         # Generated by AIKit as a wrapper for `{name}`, for filesystems where a\n\
         # multicall symlink is undesirable. Regenerated on every apply.\n\
         exec aikit run \"{name}\" \"$@\"\n\
         # <<< aikit <<<\n"
    ))
}

/// Deliberately narrower than what a filesystem allows.
///
/// The name is interpolated into a shell script, so anything that could end the
/// quoted string is out — and a name that needed escaping to be safe would also
/// be a name nobody could type at a prompt.
fn is_safe_command_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}
