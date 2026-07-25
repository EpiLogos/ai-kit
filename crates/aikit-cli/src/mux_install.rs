//! `aikit mux install` as a Procedure.
//!
//! Editing a user's `~/.tmux.conf` is exactly the case Spec II §1.3 is about: a
//! file AIKit does not own, edited through a **marked block** so applying twice
//! replaces rather than appends, and human prose outside the markers is never
//! touched. Staged and reversible like every other world mutation.

use std::path::PathBuf;

use aikit_core::procedure::{
    splice_marked_block, Inverse, Plan, Procedure, ProcedureKind, WorldEdit,
};
use aikit_core::{AikitError, MuxKind, Result};

use aikit_adapters::mux::{cmux::Cmux, tmux::Tmux, MuxAdapter};

use crate::app::Service;

/// Choose the multiplexer to install for: the named one, or whichever is actually
/// present. Detection beats assumption — installing tmux integration on a machine
/// without tmux writes a file nothing will ever read.
fn choose(named: Option<&str>) -> Result<MuxKind> {
    if let Some(raw) = named {
        return raw.parse::<MuxKind>();
    }
    if Tmux::system().detect().map(|p| p.installed).unwrap_or(false) {
        return Ok(MuxKind::Tmux);
    }
    if Cmux::system().detect().map(|p| p.installed).unwrap_or(false) {
        return Ok(MuxKind::Cmux);
    }
    Err(AikitError::new(
        "mux.none_detected",
        "no multiplexer was detected; name one explicitly if you want its integration installed",
    ))
}

/// Plan the multiplexer integration edit.
pub fn plan(service: &Service, named: Option<&str>) -> Result<Procedure> {
    let mux = choose(named)?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let (path, body) = match mux {
        MuxKind::Tmux => (
            home.join(".tmux.conf"),
            "# Open the AIKit palette in a real popup.\n\
             bind-key -n M-a display-popup -E -w 82% -h 70% -T AIKit 'aikit ui'\n\
             set -g @aikit_installed 1\n"
                .to_string(),
        ),
        MuxKind::Cmux => (
            home.join(".config/cmux/config.toml"),
            "# AIKit renders its palette inline in the focused terminal.\n\
             [keys]\nalt-a = \"aikit ui\"\n"
                .to_string(),
        ),
        MuxKind::Plain => {
            return Err(AikitError::new(
                "mux.nothing_to_install",
                "a plain terminal has no multiplexer configuration to install into",
            ))
        }
    };

    let existing = std::fs::read_to_string(&path).ok();
    // The comment leader comes from the file type, so a `#`-commented tmux config
    // and a `//`-commented one both get markers their own parser ignores.
    let leader = aikit_store::procedure::comment_leader(&path);
    let updated = splice_marked_block(existing.as_deref().unwrap_or(""), leader, &body);

    if existing.as_deref() == Some(updated.as_str()) {
        return Err(AikitError::new(
            "mux.already_installed",
            format!("{} already carries AIKit's block, unchanged", path.display()),
        )
        .with("path", path.display().to_string()));
    }

    let inverse = if existing.is_some() {
        Inverse::Restore { blob: aikit_core::procedure::BlobId::deferred() }
    } else {
        Inverse::Remove
    };

    let plan = Plan::new()
        .with_note(format!(
            "add AIKit's managed block to {} ({})",
            path.display(),
            mux.as_str()
        ))
        .with_edit(WorldEdit::WriteFile {
            path,
            contents: updated.into_bytes(),
            inverse,
        });

    aikit_store::procedure::plan_procedure(service.home(), ProcedureKind::MuxInstall { mux }, plan)
}
