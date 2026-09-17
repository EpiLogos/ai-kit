//! The Claude-grammar hooks map, shared by the harnesses whose settings spell
//! hooks as `{ <Event>: [ { matcher?, hooks: [...] } ] }` (claude-code's
//! settings.json; codex's per-project `.codex/hooks.json` reads the same
//! shape, as observed on real installs).
//!
//! The grammar itself — matcher policies, the stale sweep by dispatch-command
//! identity, empty-key pruning — lives once, in [`crate::layers`]. This module
//! is the string seam the claude and codex adapters call: whole files in,
//! whole files out.

use aikit_core::hooks::HookEventKind;
use aikit_core::{AikitError, Result};

pub use crate::layers::{dispatch_command, MatcherPolicy};
use crate::layers::{parse_existing_document, render_document};

/// Merge AIKit's dispatcher entries into a Claude-grammar settings document,
/// as the exact serialized JSON to write.
///
/// Everything that is not AIKit's is preserved; a previous AIKit entry is
/// replaced, never joined, because the two together would fire the whole chain
/// twice. The [`crate::layers::MergeReport`] of what the merge did is dropped
/// here — the string seam predates it.
pub fn merge_hook_map_entries(
    existing: Option<&str>,
    events: &[(HookEventKind, String)],
    client: &str,
    matchers: MatcherPolicy,
) -> Result<String> {
    let document = parse_existing_document(existing, |e| {
        AikitError::new(
            "client.settings_unreadable",
            format!(
                "the existing settings are not valid JSON ({e}); AIKit will not overwrite \
                 a file it cannot read"
            ),
        )
    })?;
    let (merged, _report) = crate::layers::claude_hook_map(&document, events, matchers, client)
        .map_err(AikitError::from)?;
    render_document(&merged, |e| {
        AikitError::new(
            "client.settings_unreadable",
            format!("could not render the merged settings: {e}"),
        )
    })
}
