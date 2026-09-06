//! AIKit intake of Actuation's `actuation.harness-capability/v1` records.
//!
//! Actuation owns what each harness IS; AIKit owns what it does about it.
//! Dispatch projections read the descriptor — which events exist, where
//! entries are installed, what may block, whether the harness can be woken —
//! and never rediscover harness facts. A failed intake is a disclosed
//! unavailability, never a silently hard-coded fallback.

use serde::{Deserialize, Serialize};

use crate::runner::CommandRunner;
use aikit_core::hooks::HookEventKind;
use aikit_core::Result;

pub const ACTUATION_HARNESS_CAPABILITY_SCHEMA: &str = "actuation.harness-capability/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityNativeEvent {
    pub event: String,
    pub native_name: String,
    pub transport: String,
    pub can_block: bool,
    pub context_channel: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityInjectionChannel {
    pub kind: String,
    pub mechanism: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityBlockingSemantics {
    pub kind: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityWake {
    pub kind: String,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySeam {
    pub config_path: String,
    pub format: String,
    pub entry_shape: String,
    pub ownership_marker: String,
    pub preserves_foreign_entries: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityProvenance {
    pub authored_by: String,
    #[serde(default)]
    pub source_refs: Option<Vec<String>>,
    #[serde(default)]
    pub catalog_revision: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCapability {
    pub schema: String,
    pub document: String,
    pub harness_slug: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub native_events: Vec<CapabilityNativeEvent>,
    pub injection_channel: CapabilityInjectionChannel,
    pub blocking_semantics: CapabilityBlockingSemantics,
    pub wake_capability: CapabilityWake,
    pub install_seam: CapabilitySeam,
    pub uninstall_seam: CapabilitySeam,
    pub provenance: CapabilityProvenance,
}

/// The `document: "capability-read-model"` envelope `actuation harness
/// capability <slug> --json` emits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReadModel {
    pub schema: String,
    pub document: String,
    pub capability: HarnessCapability,
}

/// What intake yielded: a descriptor, or a disclosed unavailability.
#[derive(Debug, Clone, PartialEq)]
pub enum CapabilityOutcome {
    Descriptor(Box<HarnessCapability>),
    Unavailable { reason: String },
}

impl CapabilityOutcome {
    /// The harness's native events mapped onto AIKit's dispatch boundaries.
    ///
    /// Native events outside AIKit's vocabulary (`event: "custom"`) are real
    /// surface AIKit cannot route yet; they are disclosed, not dropped: the
    /// caller receives them beside the mapped events.
    pub fn dispatch_events(&self) -> (Vec<(HookEventKind, String)>, Vec<String>) {
        match self {
            CapabilityOutcome::Descriptor(capability) => {
                let mut mapped = Vec::new();
                let mut unrouted = Vec::new();
                for event in &capability.native_events {
                    match hook_event_kind(&event.event) {
                        Some(kind) => mapped.push((kind, event.native_name.clone())),
                        None => unrouted.push(format!(
                            "{} ({}) is outside AIKit's boundary vocabulary",
                            event.native_name, event.event
                        )),
                    }
                }
                (mapped, unrouted)
            }
            CapabilityOutcome::Unavailable { .. } => (Vec::new(), Vec::new()),
        }
    }
}

fn hook_event_kind(event: &str) -> Option<HookEventKind> {
    match event {
        "session-start" => Some(HookEventKind::SessionStart),
        "user-prompt-submit" => Some(HookEventKind::UserPromptSubmit),
        "pre-tool-use" => Some(HookEventKind::PreToolUse),
        "post-tool-use" => Some(HookEventKind::PostToolUse),
        "stop" => Some(HookEventKind::Stop),
        "session-end" => Some(HookEventKind::SessionEnd),
        "pre-compact" => Some(HookEventKind::PreCompact),
        "notification" => Some(HookEventKind::Notification),
        _ => None,
    }
}

/// Run `actuation harness capability <slug> --json` through the given runner
/// and parse the read model. A failed or unparsable run is `Unavailable
/// { reason }` — the honest third state, never a hard-coded substitute.
pub fn intake_actuation_capability(
    runner: &dyn CommandRunner,
    actuation_bin: &str,
    slug: &str,
) -> CapabilityOutcome {
    let argv = vec![
        actuation_bin.to_string(),
        "harness".to_string(),
        "capability".to_string(),
        slug.to_string(),
        "--json".to_string(),
    ];
    let output = match runner.run(&argv) {
        Ok(output) => output,
        Err(error) => {
            return CapabilityOutcome::Unavailable {
                reason: format!("could not run {actuation_bin}: {error}"),
            }
        }
    };
    if output.status != 0 {
        return CapabilityOutcome::Unavailable {
            reason: format!(
                "{actuation_bin} harness capability {slug} failed ({}): {}",
                output.status,
                output.stderr.trim().chars().take(200).collect::<String>()
            ),
        };
    }
    let parsed: Result<CapabilityReadModel> = serde_json::from_str(&output.stdout)
        .map_err(|e| aikit_core::AikitError::new("actuation.capability_invalid", e.to_string()));
    match parsed {
        Ok(model)
            if model.schema == ACTUATION_HARNESS_CAPABILITY_SCHEMA
                && model.document == "capability-read-model" =>
        {
            CapabilityOutcome::Descriptor(Box::new(model.capability))
        }
        Ok(model) => CapabilityOutcome::Unavailable {
            reason: format!(
                "unexpected capability document {:?} with schema {:?} (expected \
                 {ACTUATION_HARNESS_CAPABILITY_SCHEMA}/capability-read-model)",
                model.document, model.schema
            ),
        },
        Err(error) => CapabilityOutcome::Unavailable {
            reason: format!("capability output unparsable: {}", error.message()),
        },
    }
}
