//! Context-source activation truth across AIKit selection and target-native loading.
//!
//! `ContextSource` already keeps source identity, disclosure and operational state
//! separate. Harness admission already refuses to equate generated projection with
//! live loading. This module joins those two established laws at the point where a
//! consequential act needs to explain **how a particular context source became
//! operative**.
//!
//! The receipt is deliberately about activation, not semantic authority. A native
//! `AGENTS.md` can therefore be materially active because Codex loaded it while its
//! Central provenance is still unresolved; the stable source record carries the
//! latter fact and this receipt carries the former.

use serde::{Deserialize, Serialize};

use crate::context_resolution::{ContextResolution, ResolvedResource};
use crate::platform::TargetId;
use crate::resource::ResourceRef;
use crate::{AikitError, Result};

pub const CONTEXT_ACTIVATION_VERSION: &str = "aikit.context-activation/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextActivationMode {
    /// Selected by AIKit for the resolved context. Selection alone does not prove
    /// that a target process consumed the source.
    AikitSelected,
    /// Retrieved through a ContextSource provider for the act.
    Retrieved,
    /// Projected into a target-owned representation by AIKit.
    Projected,
    /// Loaded by the harness through its own well-known project/user instruction
    /// convention, independently of AIKit selection.
    HarnessNativeAutoLoaded,
    /// Another target-native mechanism whose exact semantics remain adapter-owned.
    TargetNativeOther,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextActivationEvidenceBasis {
    /// Direct target/session/run evidence shows that the source was active.
    Observed,
    /// The accepted adapter/native contract establishes the loading rule and the
    /// source is known to fall within that rule. This is weaker than direct
    /// runtime observation and remains labelled accordingly.
    AdapterSemantics,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContextActivationReceipt {
    pub schema: String,
    pub source: ResourceRef,
    pub target: TargetId,
    pub mode: ContextActivationMode,
    pub evidence_basis: ContextActivationEvidenceBasis,
    /// Native loader/resolver responsible for this activation path.
    pub loader: String,
    /// Human-explainable native applicability scope, kept target-owned rather than
    /// collapsed into one universal precedence model.
    pub scope: String,
    /// Names the system which determines runtime precedence for this activation.
    pub precedence_owner: String,
    /// Whether this source entered the act through AIKit deliberate selection.
    pub ai_kit_selected: bool,
    /// Whether available evidence supports that this source materially conditions
    /// the current act. Presence/projection alone is never sufficient proof.
    pub materially_active: bool,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl ContextActivationReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: ResourceRef,
        target: TargetId,
        mode: ContextActivationMode,
        evidence_basis: ContextActivationEvidenceBasis,
        loader: impl Into<String>,
        scope: impl Into<String>,
        precedence_owner: impl Into<String>,
        ai_kit_selected: bool,
        materially_active: bool,
        evidence_refs: Vec<String>,
    ) -> Result<Self> {
        let receipt = Self {
            schema: CONTEXT_ACTIVATION_VERSION.to_string(),
            source,
            target,
            mode,
            evidence_basis,
            loader: loader.into(),
            scope: scope.into(),
            precedence_owner: precedence_owner.into(),
            ai_kit_selected,
            materially_active,
            evidence_refs,
            note: None,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != CONTEXT_ACTIVATION_VERSION {
            return Err(AikitError::new(
                "context_activation.schema_mismatch",
                format!("context activation schema must be {CONTEXT_ACTIVATION_VERSION}"),
            ));
        }
        for (name, value) in [
            ("loader", self.loader.as_str()),
            ("scope", self.scope.as_str()),
            ("precedence_owner", self.precedence_owner.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(AikitError::new(
                    "context_activation.empty_field",
                    format!("{name} must be non-empty"),
                ));
            }
        }
        if self
            .evidence_refs
            .iter()
            .any(|reference| reference.trim().is_empty())
        {
            return Err(AikitError::new(
                "context_activation.empty_evidence_ref",
                "activation evidence refs cannot contain an empty ref",
            ));
        }
        if self.materially_active && self.evidence_refs.is_empty() {
            return Err(AikitError::new(
                "context_activation.active_without_evidence",
                "materially active context requires target/runtime or adapter-semantics evidence",
            ));
        }
        if self.mode == ContextActivationMode::AikitSelected && !self.ai_kit_selected {
            return Err(AikitError::new(
                "context_activation.selection_contradiction",
                "aikit-selected activation mode requires ai_kit_selected=true",
            ));
        }
        Ok(())
    }
}

/// One Explain view keeps native source standing and runtime activation adjacent
/// without merging them into the same authority relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextActivationExplanation {
    pub source: ResolvedResource,
    pub activations: Vec<ContextActivationReceipt>,
}

/// Attach validated activation observations to an already-composed resolution.
///
/// This is intentionally additive: it does not re-run selection or retrieval and
/// it cannot manufacture a ContextSource which was not present in the resolution.
pub fn attach_context_activations(
    resolution: &mut ContextResolution,
    receipts: impl IntoIterator<Item = ContextActivationReceipt>,
) -> Result<()> {
    for receipt in receipts {
        receipt.validate()?;
        if !resolution
            .context_sources
            .iter()
            .any(|resource| resource.resource.descriptor.id == receipt.source)
        {
            return Err(AikitError::new(
                "context_activation.unknown_source",
                format!(
                    "activation receipt source {} is not a ContextSource in this ContextResolution",
                    receipt.source
                ),
            ));
        }
        resolution.context_activations.push(receipt);
    }
    // Canonical order makes activation-bearing ContextResolution evidence stable
    // even when equivalent observations arrive in a different order.
    resolution.context_activations.sort();
    Ok(())
}

pub fn explain_context_activation(
    resolution: &ContextResolution,
    source: &ResourceRef,
) -> Option<ContextActivationExplanation> {
    let resource = resolution
        .context_sources
        .iter()
        .find(|resource| &resource.resource.descriptor.id == source)?
        .clone();
    let activations = resolution
        .context_activations
        .iter()
        .filter(|receipt| &receipt.source == source)
        .cloned()
        .collect();
    Some(ContextActivationExplanation {
        source: resource,
        activations,
    })
}
