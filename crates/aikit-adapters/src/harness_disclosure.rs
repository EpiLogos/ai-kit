//! The layer-uniform harness disclosure read model: one [`HarnessDisclosure`]
//! per harness, assembled purely from the harness profile, the caller's
//! [`NativeObservation`] and the composed [`ToolSourceEntry`] set, so status,
//! the TUI and disclosure surfaces render every harness through one shape.
//!
//! The invariant this module owns is *posture-gated drift*. Drift is computed
//! only where AIKit manages a layer: in a managed tools layer, a native
//! record carrying [`TOOLS_PROJECTION_OWNERSHIP`] that is no longer in the
//! composed set is disclosed as pending sweep, and a composed source absent
//! from the native file is disclosed as awaiting projection. Brokered and
//! observed layers never acquire a drift concept — AIKit writes nothing
//! there, so nothing of AIKit's can drift. Foreign native entries are
//! disclosed as native and never as drift: the harness's own configuration
//! is not a problem to fix. The profile is the only source of layer
//! structure — a layer the profile does not declare is absent from the
//! disclosure, never rendered as an empty entry.
//!
//! This module never touches the filesystem or a subprocess. The native
//! facts arrive through [`NativeObservation`], extracted by the caller from
//! whatever observed them (Actuation's mcp-config facet inventory, a native
//! hook-seam read); disclosure is a pure join of the three inputs.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use aikit_core::harness_admission::HarnessEditionKind;
use aikit_core::harness_profile::{
    ActivationEffectName, HarnessProfile, HooksLayer, LayerPosture, ToolsLayer,
};

use crate::tool_sources::{ToolSourceEntry, TOOLS_PROJECTION_OWNERSHIP};

/// One entry the harness natively carries: its name and, where the caller
/// redacted one, the command detail. `detail` is what the ownership-marker
/// match reads — AIKit-projected records carry the marker inside their
/// projected command, and the redacted detail keeps it visible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One composed capability source the profile's tools layer would project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposedEntry {
    pub name: String,
}

/// The kind of tools-layer drift. Foreign native entries are never drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriftKind {
    /// A native entry carries the AIKit ownership marker but is no longer in
    /// the composed set: a sweep is pending.
    OwnedNotComposed,
    /// A composed entry the native file does not carry yet: the projection
    /// has not been applied or the harness has not picked it up.
    ComposedNotNative,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftEntry {
    pub kind: DriftKind,
    pub name: String,
    pub note: String,
}

/// One layer of the disclosure: the posture, what the harness natively
/// carries there, what AIKit has composed, and — managed tools layers only —
/// the drift between the two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerDisclosure {
    pub layer: String,
    pub posture: LayerPosture,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native: Vec<NativeEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composed: Vec<ComposedEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<ActivationEffectName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drift: Vec<DriftEntry>,
}

/// The whole-harness read model: one entry per layer the profile declares,
/// in profile layer order (skills, guidance, hooks, tools, models, sessions,
/// settings). A profile says only what is true; absent layers are absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessDisclosure {
    pub slug: String,
    pub edition: HarnessEditionKind,
    pub layers: Vec<LayerDisclosure>,
}

/// The caller-extracted native facts disclosure joins against. Kept small
/// and honest: the MCP server names with redacted command details (as
/// Actuation's mcp-config facet inventory already provides) and whether the
/// native hook seam is present, when the caller observed it. `None` means
/// not observed — never a silent no.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NativeObservation {
    #[serde(default)]
    pub mcp_servers: Vec<NativeEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_seam_present: Option<bool>,
}

/// Assemble the disclosure read model for one harness: the profile's
/// declared layers in profile order, joined to the native observation and
/// the composed tool sources. Pure — the caller owns how the inputs were
/// observed.
pub fn disclose(
    profile: &HarnessProfile,
    native: &NativeObservation,
    composed: &[ToolSourceEntry],
) -> HarnessDisclosure {
    let mut layers = Vec::new();
    if let Some(skills) = &profile.skills {
        layers.push(LayerDisclosure {
            layer: "skills".to_string(),
            posture: skills.posture,
            native: skills
                .observe
                .as_ref()
                .map(|observe| {
                    observe
                        .paths
                        .iter()
                        .map(|path| NativeEntry {
                            name: path.clone(),
                            detail: None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            composed: Vec::new(),
            activation: None,
            drift: Vec::new(),
        });
    }
    if let Some(guidance) = &profile.guidance {
        layers.push(LayerDisclosure {
            layer: "guidance".to_string(),
            posture: guidance.posture,
            native: guidance
                .observe
                .iter()
                .map(|path| NativeEntry {
                    name: path.clone(),
                    detail: None,
                })
                .collect(),
            composed: Vec::new(),
            activation: None,
            drift: Vec::new(),
        });
    }
    if let Some(hooks) = &profile.hooks {
        layers.push(LayerDisclosure {
            layer: "hooks".to_string(),
            posture: hooks.posture,
            native: hooks_native_entries(hooks, native),
            composed: Vec::new(),
            activation: None,
            drift: Vec::new(),
        });
    }
    if let Some(tools) = &profile.tools {
        layers.push(tools_disclosure(tools, native, composed));
    }
    if let Some(models) = &profile.models {
        layers.push(LayerDisclosure {
            layer: "models".to_string(),
            posture: models.posture,
            native: Vec::new(),
            composed: Vec::new(),
            activation: None,
            drift: Vec::new(),
        });
    }
    if let Some(sessions) = &profile.sessions {
        layers.push(LayerDisclosure {
            layer: "sessions".to_string(),
            posture: sessions.posture,
            native: Vec::new(),
            composed: Vec::new(),
            activation: None,
            drift: Vec::new(),
        });
    }
    if let Some(settings) = &profile.settings {
        layers.push(LayerDisclosure {
            layer: "settings".to_string(),
            posture: settings.posture,
            native: settings
                .observe
                .iter()
                .map(|path| NativeEntry {
                    name: path.clone(),
                    detail: None,
                })
                .collect(),
            composed: Vec::new(),
            activation: None,
            drift: Vec::new(),
        });
    }
    HarnessDisclosure {
        slug: profile.slug.clone(),
        edition: profile.edition,
        layers,
    }
}

/// The hooks layer's native entries: the events the profile observes, plus
/// the caller-extracted seam fact when one arrived. The seam entry names the
/// project file the profile declares, because that is the file whose state
/// the caller observed.
fn hooks_native_entries(hooks: &HooksLayer, native: &NativeObservation) -> Vec<NativeEntry> {
    let mut entries: Vec<NativeEntry> = hooks
        .observe
        .iter()
        .flat_map(|declaration| declaration.events.iter())
        .map(|event| NativeEntry {
            name: event.clone(),
            detail: None,
        })
        .collect();
    if let Some(present) = native.hook_seam_present {
        entries.push(NativeEntry {
            name: hooks
                .project
                .as_ref()
                .map(|project| project.file.clone())
                .unwrap_or_else(|| "hook seam".to_string()),
            detail: Some(
                if present {
                    "hook seam present"
                } else {
                    "hook seam absent"
                }
                .to_string(),
            ),
        });
    }
    entries
}

/// The tools layer is the drift-bearing layer, and only a managed one: AIKit
/// projects and sweeps there, so only there can what it projects and what
/// the harness carries fall out of step.
fn tools_disclosure(
    tools: &ToolsLayer,
    native: &NativeObservation,
    composed: &[ToolSourceEntry],
) -> LayerDisclosure {
    let native_entries = native.mcp_servers.clone();
    let composed_entries: Vec<ComposedEntry> = composed
        .iter()
        .map(|entry| ComposedEntry {
            name: entry.export_name.clone(),
        })
        .collect();
    let mut drift = Vec::new();
    if tools.posture == LayerPosture::Managed {
        let composed_names: BTreeSet<&str> = composed
            .iter()
            .map(|entry| entry.export_name.as_str())
            .collect();
        let native_names: BTreeSet<&str> = native_entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        for entry in &native_entries {
            let owned = entry
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains(TOOLS_PROJECTION_OWNERSHIP));
            if owned && !composed_names.contains(entry.name.as_str()) {
                drift.push(DriftEntry {
                    kind: DriftKind::OwnedNotComposed,
                    name: entry.name.clone(),
                    note: format!(
                        "{} still carries the {} ownership marker in the native configuration \
                         but is no longer in the composed set; run the tools sweep to remove it",
                        entry.name, TOOLS_PROJECTION_OWNERSHIP
                    ),
                });
            }
        }
        for entry in &composed_entries {
            if !native_names.contains(entry.name.as_str()) {
                drift.push(DriftEntry {
                    kind: DriftKind::ComposedNotNative,
                    name: entry.name.clone(),
                    note: format!(
                        "{} is composed but not present in the native configuration yet; the \
                         projection has not been applied or the harness has not restarted",
                        entry.name
                    ),
                });
            }
        }
    }
    LayerDisclosure {
        layer: "tools".to_string(),
        posture: tools.posture,
        native: native_entries,
        composed: if tools.posture == LayerPosture::Managed {
            composed_entries
        } else {
            Vec::new()
        },
        activation: tools.activation,
        drift,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::profiles::for_slug;
    use crate::tool_sources::ToolSourceEntry;
    use aikit_core::capsule::ToolServerRecord;
    use aikit_core::harness_profile::ActivationEffectName;

    fn bimba_entry() -> ToolSourceEntry {
        ToolSourceEntry {
            export_name: "bimba".to_string(),
            server: ToolServerRecord {
                command: Some(
                    "/Users/admin/Central/Work/epi/bimba-portable/bimba-mcp.sh".to_string(),
                ),
                args: vec!["--port".to_string(), "8080".to_string()],
                env: BTreeMap::new(),
                cwd: None,
                url: None,
            },
        }
    }

    fn new_source_entry() -> ToolSourceEntry {
        ToolSourceEntry {
            export_name: "new-source".to_string(),
            server: ToolServerRecord {
                command: Some("npx".to_string()),
                args: vec![
                    "-y".to_string(),
                    "@modelcontextprotocol/server-echo".to_string(),
                ],
                env: BTreeMap::new(),
                cwd: None,
                url: None,
            },
        }
    }

    fn observation(mcp_servers: Vec<NativeEntry>) -> NativeObservation {
        NativeObservation {
            mcp_servers,
            hook_seam_present: None,
        }
    }

    fn foreign_linear() -> NativeEntry {
        NativeEntry {
            name: "linear-server".to_string(),
            detail: Some("url https://mcp.linear.app/sse".to_string()),
        }
    }

    fn owned_bimba() -> NativeEntry {
        NativeEntry {
            name: "bimba".to_string(),
            detail: Some("command aikit tool-protocol serve bimba".to_string()),
        }
    }

    fn tools_layer(disclosure: &HarnessDisclosure) -> &LayerDisclosure {
        disclosure
            .layers
            .iter()
            .find(|layer| layer.layer == "tools")
            .expect("the profile declares a tools layer")
    }

    #[test]
    fn an_openclaw_disclosure_joins_foreign_and_owned_native_servers_around_the_composed_set() {
        let profile = for_slug("openclaw").expect("openclaw carries an embedded profile");
        let native = observation(vec![foreign_linear(), owned_bimba()]);
        let composed = vec![bimba_entry()];

        let disclosure = disclose(profile, &native, &composed);

        assert_eq!(disclosure.slug, "openclaw");
        assert_eq!(
            disclosure.edition,
            aikit_core::harness_admission::HarnessEditionKind::Cli
        );
        let tools = tools_layer(&disclosure);
        assert_eq!(tools.posture, LayerPosture::Managed);
        assert_eq!(tools.native, vec![foreign_linear(), owned_bimba()]);
        assert_eq!(
            tools.composed,
            vec![ComposedEntry {
                name: "bimba".to_string()
            }]
        );
        assert!(
            tools.drift.is_empty(),
            "the owned native server is composed and the foreign one is not drift: {:?}",
            tools.drift
        );
        assert_eq!(tools.activation, Some(ActivationEffectName::RestartClient));
    }

    #[test]
    fn an_owned_native_entry_missing_from_the_composed_set_is_disclosed_as_pending_sweep() {
        let profile = for_slug("openclaw").expect("openclaw carries an embedded profile");
        let native = observation(vec![NativeEntry {
            name: "retired-source".to_string(),
            detail: Some("command aikit tool-protocol serve retired".to_string()),
        }]);

        let disclosure = disclose(profile, &native, &[]);

        let tools = tools_layer(&disclosure);
        assert_eq!(
            tools.drift,
            vec![DriftEntry {
                kind: DriftKind::OwnedNotComposed,
                name: "retired-source".to_string(),
                note: "retired-source still carries the aikit tool-protocol ownership marker \
                       in the native configuration but is no longer in the composed set; run \
                       the tools sweep to remove it"
                    .to_string(),
            }]
        );
    }

    #[test]
    fn a_composed_entry_missing_from_the_native_file_is_disclosed_as_awaiting_projection() {
        let profile = for_slug("openclaw").expect("openclaw carries an embedded profile");
        let native = observation(vec![foreign_linear()]);

        let disclosure = disclose(profile, &native, &[new_source_entry()]);

        let tools = tools_layer(&disclosure);
        assert_eq!(
            tools.drift,
            vec![DriftEntry {
                kind: DriftKind::ComposedNotNative,
                name: "new-source".to_string(),
                note: "new-source is composed but not present in the native configuration \
                       yet; the projection has not been applied or the harness has not \
                       restarted"
                    .to_string(),
            }]
        );
    }

    #[test]
    fn a_brokered_tools_layer_discloses_no_composed_entries_and_no_drift_even_when_composition_exists(
    ) {
        let profile = for_slug("pi").expect("pi carries an embedded profile");
        let native = observation(vec![foreign_linear()]);

        let disclosure = disclose(profile, &native, &[bimba_entry(), new_source_entry()]);

        let tools = tools_layer(&disclosure);
        assert_eq!(tools.layer, "tools");
        assert_eq!(tools.posture, LayerPosture::Brokered);
        assert!(
            tools.composed.is_empty(),
            "a brokered layer composes through disclosure only: {:?}",
            tools.composed
        );
        assert!(
            tools.drift.is_empty(),
            "AIKit writes nothing into a brokered layer, so nothing drifts: {:?}",
            tools.drift
        );
        assert_eq!(tools.native, vec![foreign_linear()]);
        assert!(
            !disclosure.layers.is_empty(),
            "the brokered harness still renders every layer it declares"
        );
    }

    #[test]
    fn an_observed_tools_layer_discloses_its_native_servers_without_drift() {
        let profile = for_slug("gemini").expect("gemini carries an embedded profile");
        let native = observation(vec![owned_bimba()]);

        let disclosure = disclose(profile, &native, &[]);

        let tools = tools_layer(&disclosure);
        assert_eq!(tools.posture, LayerPosture::Observed);
        assert_eq!(tools.native, vec![owned_bimba()]);
        assert!(
            tools.composed.is_empty(),
            "an observed layer takes no composed entries: {:?}",
            tools.composed
        );
        assert!(
            tools.drift.is_empty(),
            "the posture gates drift, not the marker alone: {:?}",
            tools.drift
        );
    }

    #[test]
    fn the_disclosure_serializes_with_layers_in_profile_order() {
        let profile = for_slug("claude-code").expect("claude-code carries an embedded profile");
        let native = observation(vec![foreign_linear()]);
        let composed = vec![bimba_entry()];

        let disclosure = disclose(profile, &native, &composed);
        let json = serde_json::to_string_pretty(&disclosure).expect("the disclosure serializes");
        let parsed: HarnessDisclosure =
            serde_json::from_str(&json).expect("the serialized disclosure reparses");

        assert_eq!(
            parsed, disclosure,
            "the disclosure round-trips through JSON"
        );
        assert_eq!(
            parsed
                .layers
                .iter()
                .map(|layer| layer.layer.as_str())
                .collect::<Vec<_>>(),
            vec!["skills", "guidance", "hooks", "tools", "models", "sessions"],
            "layers render in profile order; undeclared layers are absent"
        );
    }

    #[test]
    fn the_zcode_skills_layer_discloses_the_native_tree_it_loads_without_drift_or_composition() {
        // Posture honesty: zcode loads the codex-managed ~/.agents/skills
        // shared tree natively, so the disclosure renders an observed layer
        // naming that tree — never a brokered "never projected" claim, and
        // never composed entries or drift for a layer AIKit does not write.
        let profile = for_slug("zcode").expect("zcode carries an embedded profile");
        let disclosure = disclose(profile, &NativeObservation::default(), &[]);

        let skills = disclosure
            .layers
            .iter()
            .find(|layer| layer.layer == "skills")
            .expect("zcode declares a skills layer");
        assert_eq!(skills.posture, LayerPosture::Observed);
        assert!(
            skills
                .native
                .iter()
                .any(|entry| entry.name == "~/.agents/skills"),
            "the shared tree zcode demonstrably loads is disclosed as native: {:?}",
            skills.native
        );
        assert!(
            skills.composed.is_empty() && skills.drift.is_empty(),
            "AIKit writes no zcode skill seam, so nothing composes or drifts there"
        );
    }

    #[test]
    fn detection_only_layers_disclose_their_observed_paths_and_the_hook_seam_fact_as_native_entries(
    ) {
        let profile = for_slug("claude-code").expect("claude-code carries an embedded profile");
        let native = NativeObservation {
            mcp_servers: vec![],
            hook_seam_present: Some(true),
        };

        let disclosure = disclose(profile, &native, &[]);

        let layer_named = |name: &str| {
            disclosure
                .layers
                .iter()
                .find(|layer| layer.layer == name)
                .unwrap_or_else(|| panic!("claude-code declares a {name} layer"))
        };
        let skills = layer_named("skills");
        assert_eq!(
            skills.native,
            vec![NativeEntry {
                name: "~/.claude/skills".to_string(),
                detail: None
            }]
        );
        let guidance = layer_named("guidance");
        assert_eq!(
            guidance.native,
            vec![NativeEntry {
                name: "~/.claude/CLAUDE.md".to_string(),
                detail: None
            }]
        );
        let hooks = layer_named("hooks");
        assert_eq!(
            hooks.native.last(),
            Some(&NativeEntry {
                name: "~/.claude/settings.json".to_string(),
                detail: Some("hook seam present".to_string()),
            }),
            "the observed seam fact joins the hooks layer naming the file it was read from"
        );
        assert_eq!(
            hooks.native.len(),
            9,
            "eight observed events plus the seam fact"
        );
        for layer in &disclosure.layers {
            assert!(
                layer.drift.is_empty() || layer.layer == "tools",
                "only the tools layer carries a drift concept: {}",
                layer.layer
            );
        }
    }

    #[test]
    fn pi_hooks_disclose_the_extension_event_census_as_observed_without_drift() {
        let profile = for_slug("pi").expect("pi carries an embedded profile");
        let disclosure = disclose(profile, &NativeObservation::default(), &[]);

        let hooks = disclosure
            .layers
            .iter()
            .find(|layer| layer.layer == "hooks")
            .expect("the 2026-09-18 census gives pi a declared hooks layer");
        assert_eq!(hooks.posture, LayerPosture::Observed);
        assert_eq!(
            hooks.native,
            vec![
                NativeEntry {
                    name: "session-start".to_string(),
                    detail: None
                },
                NativeEntry {
                    name: "user-prompt-submit".to_string(),
                    detail: None
                },
                NativeEntry {
                    name: "pre-tool-use".to_string(),
                    detail: None
                },
                NativeEntry {
                    name: "post-tool-use".to_string(),
                    detail: None
                },
                NativeEntry {
                    name: "session-end".to_string(),
                    detail: None
                },
                NativeEntry {
                    name: "pre-compact".to_string(),
                    detail: None
                },
            ],
            "the census events render as the layer's native entries"
        );
        assert!(
            hooks.composed.is_empty(),
            "an observed layer takes no composed entries: {:?}",
            hooks.composed
        );
        assert!(
            hooks.drift.is_empty(),
            "AIKit writes no pi hooks, so nothing of AIKit's can drift: {:?}",
            hooks.drift
        );
    }
}
