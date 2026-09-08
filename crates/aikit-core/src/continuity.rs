//! Continuity tuning: the W1 reaction engine's composition law.
//!
//! The descope constitution (PROGRAMME §4): the default profile activates the
//! minimum — today's temporal reground, owned by Central, is the **floor** and
//! is law, not a capability. Every continuity capability beyond the floor
//! enters as a named first-party hook capability in the `continuity`
//! namespace, and it is operative only when the active composition selects
//! it. Nothing here is global mutable config: the tuning is read from the
//! resolved view at event time, the same way everything else is read.
//!
//! The engine answers "not composed" honestly: [`ContinuityTuning::describe`]
//! distinguishes the floor, what is composed, and what is known but not
//! composed — and `aikit context` attributes each value to that composition.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::id::CapsuleId;
use crate::profile::ConfigTable;
use crate::resolve::ResolvedView;

/// The capability namespace that carries first-party continuity reactions.
pub const CONTINUITY_NAMESPACE: &str = "continuity";

/// The descope floor: Central's temporal reground, delivered at
/// SessionStart / UserPromptSubmit / PreCompact. Law, not a capability; it is
/// never gated and never reported as composed-by-someone.
pub const FLOOR_CAPABILITY: &str = "temporal-reground";

/// The turn ledger: the first first-party reaction. When composed, the engine
/// injects a one-line dispatch ledger at UserPromptSubmit, so a turn names
/// the continuity that produced it.
pub const TURN_LEDGER: &str = "turn-ledger";

/// Entity-aware disclosure (W10 V6): when composed, the engine names the
/// pasu participants present in this context (the bound wiki entities) at
/// SessionStart. Durable facts only, from authored carriers and the world
/// binding — volatile state ("who is the current actor") stays in Frames
/// and NOW, never in this disclosure and never in persistent nodes.
pub const ENTITY_DISCLOSURE: &str = "entity-disclosure";

/// Domain activation (W1/CASE 03): when composed, a prompt matching a
/// declared KnowledgeDomain's triggers makes its guidance operative within
/// the domain's declared horizon span. Dedup keys on rendered content;
/// standing rules are exempt by explicit classification. Domains are
/// declared data in the project layer (`.aikit/domains`), never ambient.
pub const DOMAIN_ACTIVATION: &str = "domain-activation";

/// File context (W1/W3, CASE 05): when composed, an operation about to touch
/// a file arrives with the semantic context the project already holds for it
/// — wiki relations that cite the file, plus any declared KnowledgeDomain
/// whose `path_patterns` address it — through the pre-tool additional-context
/// channel. Dedup keys on rendered content per file, so an unchanged file
/// re-injects nothing; standing rules stay exempt by classification.
pub const FILE_CONTEXT: &str = "file-context";

/// Orientation packet (W2/W1, CASE 02 + CASE 09): when composed, a fresh
/// session in a project is met by that project's own NOW field — assembled
/// through `ctrl projectcentral.now.inspect`, bounded, with the newest open
/// handoff return as the continuation. Horizon apertures are closed by
/// default (the field's human side arrives only by explicit composition
/// authorization), and other projects are structurally absent.
pub const ORIENTATION_PACKET: &str = "orientation-packet";

/// W4/CASE 06: completed tool use appends attributed project `lastActive` evidence.
pub const ACTIVITY_EVIDENCE: &str = "activity-evidence";

/// The reactions the engine itself implements, as opposed to hook capsules
/// that merely ride the chain. A reaction listed here is answerable when
/// asked even while it is not composed: the engine says "not composed"
/// instead of staying silent about what it could do.
pub const ENGINE_REACTIONS: &[&str] = &[
    TURN_LEDGER,
    ENTITY_DISCLOSURE,
    DOMAIN_ACTIVATION,
    FILE_CONTEXT,
    ORIENTATION_PACKET,
    ACTIVITY_EVIDENCE,
];

/// True when the capsule id names a first-party continuity reaction.
pub fn is_continuity_capability(id: &CapsuleId) -> bool {
    id.kind() == crate::capsule::Kind::Hook
        && id
            .path()
            .strip_prefix(&format!("{CONTINUITY_NAMESPACE}/"))
            .is_some()
}

// No `Eq`: the tuning carries a TOML config table, and `toml::Value` is only
// `PartialEq` (it can hold floats). `PartialEq` is all the reports and tests
// need.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContinuityTuning {
    /// The floor: always law, never composed-by-someone.
    pub floor: &'static str,
    /// First-party continuity capabilities the active composition selected,
    /// sorted for stable reports and digests.
    pub composed: Vec<String>,
    /// Continuity capabilities known to the registry but not selected — the
    /// honest "not composed" answer the engine gives when asked.
    pub not_composed: Vec<String>,
    /// The tuning values in effect for each composed continuity capability,
    /// keyed by continuity name, alongside the composition that selected the
    /// capability. This makes an adjusted value *attributable to the active
    /// composition*: it is read from that resolved capability, never from
    /// ambient global state.
    ///
    /// Defaulted for serde so a lock written before attribution existed still
    /// reads back.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tunings: BTreeMap<String, CapabilityTuning>,
}

/// The tuning values one composed continuity capability is running under, and
/// the composition layer they were selected through.
///
/// An empty `values` is a real answer, not a missing one: the capability is
/// composed and running on its own declared defaults, adjusted by nobody.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityTuning {
    /// Where this capsule's active selection came from, as
    /// [`crate::resolve::SelectionOrigin::describe`] renders it.
    pub composition: String,
    /// The `[config.<capsule-id>]` table in effect after the layer merge —
    /// the adjusted values themselves, not a summary of them.
    pub values: ConfigTable,
}

impl ContinuityTuning {
    /// Resolve the tuning from the resolved view at event time.
    pub fn resolve(view: &ResolvedView) -> Self {
        let mut composed = Vec::new();
        for id in view.active.keys() {
            if is_continuity_capability(id) {
                composed.push(continuity_name(id));
            }
        }
        composed.sort();
        // "Not composed" is answered from the engine's own reaction catalogue
        // plus any known continuity capsules the composition did not select.
        let mut not_composed: Vec<String> = ENGINE_REACTIONS
            .iter()
            .filter(|name| !composed.iter().any(|c| c == *name))
            .map(|name| name.to_string())
            .collect();
        for id in view.unavailable.keys().chain(view.declared.keys()) {
            if is_continuity_capability(id) {
                let name = continuity_name(id);
                if !composed.contains(&name) && !not_composed.contains(&name) {
                    not_composed.push(name);
                }
            }
        }
        not_composed.sort();
        // The values each composed reaction is running under, read from the
        // same view at the same instant as the composition itself — so the
        // report can never disagree with the behaviour it describes.
        let mut tunings = BTreeMap::new();
        for (id, active) in &view.active {
            if is_continuity_capability(id) {
                tunings.insert(
                    continuity_name(id),
                    CapabilityTuning {
                        composition: active.origin.describe(),
                        values: active.config.clone(),
                    },
                );
            }
        }
        Self {
            floor: FLOOR_CAPABILITY,
            composed,
            not_composed,
            tunings,
        }
    }

    /// Is this continuity capability operative under the active composition?
    /// The floor is always allowed; everything else must be composed.
    pub fn allows(&self, capability: &str) -> bool {
        capability == FLOOR_CAPABILITY || self.composed.iter().any(|name| name == capability)
    }

    /// The inspection report `aikit context` renders: every value attributed
    /// to the composition that produced it.
    ///
    /// The floor, what the composition selected, what it knowingly did not,
    /// and — per composed capability — the tuning values in effect with the
    /// composition that selected it. An operator reading this can answer
    /// "what is this composition running with" without consulting anything
    /// ambient.
    pub fn describe(&self) -> serde_json::Value {
        serde_json::json!({
            "floor": self.floor,
            "composed": self.composed,
            "not_composed": self.not_composed,
            "tunings": self.tunings,
        })
    }
}

/// The continuity name of a capsule id: `hook/continuity/<name>` → `<name>`.
fn continuity_name(id: &CapsuleId) -> String {
    id.path()
        .strip_prefix(&format!("{CONTINUITY_NAMESPACE}/"))
        .unwrap_or_else(|| id.path())
        .to_string()
}

impl fmt::Display for ContinuityTuning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "floor: {}; composed: [{}]; not composed: [{}]",
            self.floor,
            self.composed.join(", "),
            self.not_composed.join(", "),
        )
    }
}
