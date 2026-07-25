//! Hosting the palette.
//!
//! The palette's *placement* is a CLI concern — it depends on where the binary is
//! running (inside tmux? a tiny terminal? was `--fullscreen` asked for?) — while
//! everything the palette *does* is the shared [`crate::app::Service`]. This
//! module builds a [`TerminalProfile`] from the environment and lets
//! [`aikit_tui::UiHost::choose`] make the placement decision, then hands the
//! chosen host and the service to [`aikit_tui::run`].
//!
//! AIKit never embeds a terminal emulator: the tmux popup is a real
//! `display-popup`, and the inline and fullscreen hosts draw into the terminal
//! that is already there.

use aikit_core::platform::MuxKind;
use aikit_core::Result;

use aikit_tui::host::{TerminalProfile, UiHost};
use aikit_tui::tree_driver::{TreeOutcome, TreeRequest};
use aikit_tui::{PaletteOutcome, PaletteRequest};

use crate::app::Service;

/// Build a terminal profile from an environment lookup and the `--fullscreen`
/// flag.
///
/// A very small terminal is *not* forced fullscreen here — that escalation is
/// [`UiHost::choose`]'s call, which knows the inline minimum. What this function
/// contributes is the raw facts: the size, whether a multiplexer is present, and
/// whether the user explicitly asked for fullscreen.
pub fn terminal_profile<F>(env: F, fullscreen: bool) -> TerminalProfile
where
    F: Fn(&str) -> Option<String>,
{
    let (cols, rows) = terminal_size(&env);
    let mut profile = TerminalProfile::new(cols, rows);

    if env("TMUX").is_some() {
        profile = profile.in_mux(MuxKind::Tmux);
    } else if env("CMUX").is_some() || env("CMUX_SURFACE").is_some() {
        profile = profile.in_mux(MuxKind::Cmux);
    }

    if fullscreen {
        profile = profile.requested(UiHost::Fullscreen);
    }
    profile
}

/// Best-effort terminal size: the real `COLUMNS`/`LINES` the shell exports, else a
/// conventional 80×24. The palette re-measures on its own backend at draw time;
/// this only seeds the host choice.
fn terminal_size<F>(env: &F) -> (u16, u16)
where
    F: Fn(&str) -> Option<String>,
{
    let parse = |key: &str, default: u16| {
        env(key)
            .and_then(|v| v.trim().parse::<u16>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(default)
    };
    (parse("COLUMNS", 80), parse("LINES", 24))
}

/// Open the palette over the service and run it to completion.
pub fn run(
    service: &mut Service,
    query: Option<String>,
    fullscreen: bool,
) -> Result<PaletteOutcome> {
    let profile = terminal_profile(|k| std::env::var(k).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    let mut request = PaletteRequest::new(host);
    if let Some(query) = query {
        request = request.with_query(query);
    }
    aikit_tui::run(service, request)
}

/// Open the exact palette row and immediately hand it to its natural action.
pub fn run_activation(
    service: &mut Service,
    query: String,
    fullscreen: bool,
) -> Result<PaletteOutcome> {
    let profile = terminal_profile(|k| std::env::var(k).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    aikit_tui::run(
        service,
        PaletteRequest::new(host)
            .with_query(query)
            .activating_initial(),
    )
}

/// Open the organising tree over the same live service as the palette.
pub fn run_tree(service: &Service, fullscreen: bool) -> Result<TreeOutcome> {
    let profile = terminal_profile(|k| std::env::var(k).ok(), fullscreen);
    let host = UiHost::choose(&profile);
    let state = crate::tree_build::build(service)?;
    let scope = service.descriptor().default_mutation_scope();
    let mut request = TreeRequest::new(host);
    if scope.requires_confirmation_to_write() {
        request = request.with_apply_confirmation(
            format!("Write staged changes to the {scope} profile?"),
            match scope {
                aikit_core::scope::ScopeKind::Global => {
                    "~/.aikit/profiles applies to every project on this machine."
                }
                aikit_core::scope::ScopeKind::Project => {
                    "<repo>/.aikit/profile.toml is committed and affects every collaborator."
                }
                _ => "This change is durable.",
            },
        );
    }
    aikit_tui::run_tree(state, request)
}
