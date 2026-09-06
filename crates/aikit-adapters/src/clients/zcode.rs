//! ZCode.
//!
//! ZCode reads hook dispatcher entries from the `hooks` block of its
//! configuration JSON (`~/.zcode/cli/config.json` by default). Configuration-file
//! hooks are inert until `hooks.enabled` is true, so the projection owns that
//! flag too — Actuation's descriptor declares the whole seam. The native surface
//! is whatever the descriptor says it is: the installed set is the descriptor's
//! mapped events, never a local list, and native events outside AIKit's boundary
//! vocabulary stay disclosed as the customs they are.
//!
//! Match semantics are the harness's own: a hook entry may carry a matcher (a
//! case-sensitive regular expression over the event's match value), and an
//! omitted matcher matches everything. AIKit's dispatch entries omit the
//! matcher — inventing one would narrow what the dispatcher sees.
//!
//! ## What this adapter will never do
//!
//! It never touches anything outside the descriptor's config-json seam, and it
//! never drops foreign hooks: unrelated top-level keys, foreign events, and the
//! user's own hooks inside events AIKit also uses all survive an install.

use std::path::Path;

use aikit_core::hooks::HookEventKind;
use aikit_core::platform::TargetId;
use aikit_core::projection::{
    ActivationEffect, ProjectionItem, ProjectionPlan, ResolvedContext, TargetAdapter,
    TargetCapabilities,
};
use aikit_core::{AikitError, Result};

use crate::actuation_harness_capability::{CapabilityOutcome, HarnessCapability};

use super::ClientAdapter;

/// The client's own name for itself in a hook command.
pub const CLIENT: &str = "zcode";

/// The events AIKit installs are read from Actuation's capability descriptor,
/// never hard-coded here: the AIKit boundary and the native name the harness
/// spells in its configuration.
pub type DescriptorEvents = Vec<(HookEventKind, String)>;

pub struct ZcodeAdapter {
    binary: String,
    capability: Option<HarnessCapability>,
}

impl ZcodeAdapter {
    pub fn new() -> Self {
        Self {
            binary: CLIENT.to_string(),
            capability: None,
        }
    }

    /// Install derives its events and seam from Actuation's capability
    /// descriptor. Without one the adapter refuses: hard-coding harness facts
    /// here is exactly the rediscovery the ownership split forbids.
    #[must_use]
    pub fn with_capability(mut self, capability: HarnessCapability) -> Self {
        self.capability = Some(capability);
        self
    }

    fn descriptor_events(&self) -> Result<DescriptorEvents> {
        match &self.capability {
            Some(capability) => {
                let (mut mapped, unrouted) =
                    CapabilityOutcome::Descriptor(Box::new(capability.clone())).dispatch_events();
                mapped.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
                if mapped.is_empty() {
                    return Err(AikitError::new(
                        "client.capability_without_dispatch_events",
                        format!(
                            "the {} capability descriptor maps none of its native events onto AIKit's dispatch boundaries (unrouted: {unrouted:?})",
                            capability.harness_slug
                        ),
                    ));
                }
                Ok(mapped)
            }
            None => Err(AikitError::new(
                "client.capability_unavailable",
                "no capability descriptor was supplied: AIKit installs only what Actuation \
                 declares the harness to be, and guessing is not installation",
            )),
        }
    }

    #[must_use]
    pub fn with_binary(mut self, binary: impl Into<String>) -> Self {
        self.binary = binary.into();
        self
    }
}

impl Default for ZcodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TargetAdapter for ZcodeAdapter {
    fn target(&self) -> TargetId {
        TargetId::new(CLIENT)
    }

    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            // The dispatch entries are read once at session start, and no native
            // skill projection exists yet — capabilities are reached through the
            // broker, which is also the honest isolation story.
            live_reload: false,
            symlinks: false,
            isolated_per_context: false,
            requires_isolated_tree_for_isolation: false,
            brokered_fallback: true,
            watches_for_changes: false,
        }
    }

    fn plan(&self, _context: &ResolvedContext) -> Result<ProjectionPlan> {
        Ok(ProjectionPlan::new(
            self.target(),
            ActivationEffect::brokered(
                "no native skill projection is built for zcode; capabilities are reached \
                 through AIKit's broker surfaces",
            ),
        )
        .with_note(
            "the dispatch projection lands in the descriptor's config-json seam via \
             `aikit client install zcode`"
                .to_string(),
        ))
    }

    fn activation_effect(
        &self,
        old: Option<&ProjectionPlan>,
        new: &ProjectionPlan,
    ) -> ActivationEffect {
        if new.is_noop_against(old) {
            ActivationEffect::immediate("already projected")
        } else {
            new.effect.clone()
        }
    }
}

impl ClientAdapter for ZcodeAdapter {
    fn launch_command(&self, _context: &ResolvedContext) -> Vec<String> {
        vec![self.binary.clone()]
    }

    fn install(&self, config_dir: &Path) -> Result<Vec<ProjectionItem>> {
        let events = self.descriptor_events()?;
        let file_name = self
            .capability
            .as_ref()
            .map(|capability| {
                Path::new(&capability.install_seam.config_path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "config.json".to_string())
            })
            .unwrap_or_else(|| "config.json".to_string());
        let path = config_dir.join(&file_name);
        let existing = match std::fs::read_to_string(&path) {
            Ok(contents) => Some(contents),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(AikitError::new(
                    "client.settings_unreadable",
                    format!("could not read {}: {e}", path.display()),
                )
                .with("path", path.display().to_string()))
            }
        };

        let merged = merge_dispatcher_entries(existing.as_deref(), &events)?;
        Ok(vec![ProjectionItem::write(&file_name, merged)?])
    }
}

// ---------------------------------------------------------------------------
// Configuration merging
// ---------------------------------------------------------------------------

/// The command AIKit installs for one event.
pub fn dispatch_command(event: &HookEventKind) -> String {
    format!("aikit hook dispatch {CLIENT} {event}")
}

/// Is this an AIKit dispatcher entry — including a stale one from an older
/// install that spelled the event differently?
fn is_aikit_entry(command: &str) -> bool {
    command
        .trim()
        .starts_with(&format!("aikit hook dispatch {CLIENT}"))
}

/// Merge AIKit's dispatcher entries into a zcode configuration document.
///
/// The seam is the descriptor's: a top-level `hooks` block shaped
/// `{ enabled: true, events: { <Event>: [ { matcher?, hooks: [...] } ] } }`.
/// Configuration-file hooks are inert without `enabled: true`, so the merged
/// document carries it — unless the user disabled hooks explicitly, in which
/// case install refuses rather than silently re-enabling the user's own hooks
/// alongside AIKit's.
///
/// Everything that is not AIKit's is preserved: unrelated top-level keys,
/// foreign events, and the user's own hooks inside events AIKit also uses. A
/// previous AIKit entry is *not* preserved, because leaving one behind next to
/// a new one would fire the whole chain twice.
pub fn merge_dispatcher_entries(
    existing: Option<&str>,
    events: &DescriptorEvents,
) -> Result<String> {
    let mut document: serde_json::Value = match existing {
        None => serde_json::json!({}),
        Some(raw) if raw.trim().is_empty() => serde_json::json!({}),
        Some(raw) => serde_json::from_str(raw).map_err(|e| {
            AikitError::new(
                "client.settings_unreadable",
                format!(
                    "the existing zcode configuration is not valid JSON ({e}); AIKit will not \
                     overwrite a file it cannot read"
                ),
            )
        })?,
    };

    if !document.is_object() {
        return Err(AikitError::new(
            "client.settings_unreadable",
            "the existing zcode configuration is not a JSON object",
        ));
    }

    let hooks = document
        .as_object_mut()
        .and_then(|o| {
            o.entry("hooks")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
        })
        .ok_or_else(|| {
            AikitError::new(
                "client.settings_unreadable",
                "the existing `hooks` value is not an object",
            )
        })?;

    if hooks.get("enabled") == Some(&serde_json::Value::Bool(false)) {
        return Err(AikitError::new(
            "client.hooks_disabled_by_user",
            "zcode's configuration-file hooks are explicitly disabled \
             (`hooks.enabled: false`); enabling the runner would also activate hooks the \
             user kept disabled, so AIKit refuses instead of flipping the flag",
        ));
    }
    hooks.insert("enabled".to_string(), serde_json::Value::Bool(true));

    let events_map = hooks
        .entry("events".to_string())
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            AikitError::new(
                "client.settings_unreadable",
                "the existing `hooks.events` value is not an object",
            )
        })?;

    // A previous install may have written an entry under an event AIKit no
    // longer dispatches, or under a misspelling. Sweep those first, everywhere.
    for entries in events_map.values_mut() {
        if let Some(matchers) = entries.as_array_mut() {
            for matcher in matchers.iter_mut() {
                if let Some(list) = matcher.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                    list.retain(|hook| {
                        !hook
                            .get("command")
                            .and_then(|c| c.as_str())
                            .is_some_and(is_aikit_entry)
                    });
                }
            }
            matchers.retain(|matcher| {
                matcher
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .is_none_or(|list| !list.is_empty())
            });
        }
    }

    for (event, native_name) in events {
        // No matcher: omitted matches everything, and a matcher AIKit invented
        // would narrow what the dispatcher sees.
        let entry = serde_json::json!({
            "hooks": [{ "type": "command", "command": dispatch_command(event) }]
        });

        let list = events_map
            .entry(native_name.clone())
            .or_insert_with(|| serde_json::json!([]));
        match list.as_array_mut() {
            Some(array) => array.push(entry),
            None => {
                return Err(AikitError::new(
                    "client.settings_unreadable",
                    format!("the existing `hooks.events.{event}` value is not an array"),
                ))
            }
        }
    }

    // Remove any event key that ended up empty after the sweep, so an old install
    // does not leave `"Stop": []` behind forever.
    events_map.retain(|_, entries| entries.as_array().is_none_or(|a| !a.is_empty()));

    let mut rendered = serde_json::to_string_pretty(&document).map_err(|e| {
        AikitError::new(
            "client.settings_unreadable",
            format!("could not render the merged configuration: {e}"),
        )
    })?;
    rendered.push('\n');
    Ok(rendered)
}
