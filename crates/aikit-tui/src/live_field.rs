//! The live working-environment field: which terminal environment projections
//! can actually reach the subject in hand, and what each of them can do to it.
//!
//! This crate cannot observe a working environment. `aikit-tui` does not — and
//! must not — depend on `aikit-adapters`, which is where tmux, cmux, Herdr and
//! Hyprland providers perform their I/O. The same seam the Project binding and
//! the versioned World already use applies here: the caller who *can* observe
//! hands the observation over, and a backend that cannot observe answers
//! `None` rather than pretending it looked.
//!
//! What this module adds on top of those observations is one derived reading,
//! rebuilt from scratch every time: for each canonical subject a provider has
//! been explicitly bound to, which providers reach it, and whether each of them
//! can open or focus it *right now*. It holds no cursor, no history and no
//! state — selection remains the one `TuiState.selected`.
//!
//! Two invariants it exists to keep:
//!
//! * A provider-native id (`%12`, `surface-3`, a window handle) is binding and
//!   provenance. It is never AIKit identity, never a fallback identity, and is
//!   never displayed as one.
//! * Capabilities are reported as the provider reports them. When open or focus
//!   is unavailable the reading says which condition failed, so a surface never
//!   has to invent a reason and never offers an affordance that would fail.

use std::collections::{BTreeMap, BTreeSet};

use aikit_core::resource::{ActionStageability, ContextualActionDescriptor, ResourceRef};
use aikit_core::working_environment::{
    NativeBindingKind, WorkingEnvironmentCapabilities, WorkingEnvironmentHealth,
    WorkingEnvironmentObservation,
};
use aikit_core::{AikitError, Result};
use serde::{Deserialize, Serialize};

use crate::application::{reduce_tui, TuiReduction, TuiState, UiAction};

pub const LIVE_WORKING_FIELD_VERSION: &str = "aikit.tui-live-working-field/v1";

/// Why an operation this surface might otherwise offer is not available.
/// Every variant names a condition that was actually checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReachWithheld {
    /// The provider reported itself unavailable.
    ProviderUnavailable,
    /// The provider does not claim this capability at all.
    CapabilityNotClaimed,
    /// The provider is willing and able, but nothing of this subject exists in
    /// it yet. Focusing a pane that has not been created is not a capability
    /// question; opening is what creates it.
    NotBound,
}

impl ReachWithheld {
    pub fn describe(self, operation: &str) -> String {
        match self {
            Self::ProviderUnavailable => {
                format!("provider is unavailable, so {operation} cannot be offered")
            }
            Self::CapabilityNotClaimed => {
                format!("provider does not claim the {operation} capability")
            }
            Self::NotBound => {
                format!("nothing is live in this provider to {operation} yet")
            }
        }
    }
}

/// What one provider can do with one canonical subject, right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderReach {
    pub provider: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_version: Option<String>,
    pub health: WorkingEnvironmentHealth,
    /// Provider-native id this subject is currently bound to, when the
    /// provider has one live. Provenance only — see the module note. `None`
    /// means this subject is projectable here but not yet live, which is the
    /// ordinary state before anything has been opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_kind: Option<NativeBindingKind>,
    /// True when the provider itself reports this native id as focused.
    pub focused: bool,
    pub open: Option<ReachWithheld>,
    pub focus: Option<ReachWithheld>,
    #[serde(default)]
    pub provenance: Vec<String>,
}

impl ProviderReach {
    pub fn can_open(&self) -> bool {
        self.open.is_none()
    }

    pub fn can_focus(&self) -> bool {
        self.focus.is_none()
    }
}

/// Every projection through which one canonical subject is reachable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectReach {
    pub subject: ResourceRef,
    /// The native binding kind vocabulary, not a new ResourceKind taxonomy.
    pub semantic_kind: String,
    /// Ordered by provider Ref so the reading is deterministic across runs.
    pub projections: Vec<ProviderReach>,
}

impl SubjectReach {
    pub fn projection(&self, provider: &ResourceRef) -> Option<&ProviderReach> {
        self.projections
            .iter()
            .find(|reach| &reach.provider == provider)
    }

    pub fn openable(&self) -> impl Iterator<Item = &ProviderReach> {
        self.projections.iter().filter(|reach| reach.can_open())
    }

    pub fn focusable(&self) -> impl Iterator<Item = &ProviderReach> {
        self.projections.iter().filter(|reach| reach.can_focus())
    }

    /// W6's acceptance in one predicate: this subject is reachable through at
    /// least two distinct terminal environment projections.
    pub fn multiply_projected(&self) -> bool {
        self.openable().count() >= 2 || self.focusable().count() >= 2
    }
}

/// A provider that was observed, whether or not anything is bound to it.
///
/// Keeping unbound providers in the reading matters: "cmux is running and
/// healthy but nothing here is bound to it" and "cmux was never observed" are
/// different facts, and a surface that collapses them lies about the field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedProvider {
    pub provider: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_version: Option<String>,
    pub health: WorkingEnvironmentHealth,
    pub capabilities: WorkingEnvironmentCapabilities,
    /// How many canonical subjects the caller bound in this provider.
    pub bound_subjects: usize,
    #[serde(default)]
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWorkingField {
    pub version: String,
    /// Every provider observed, in provider-Ref order.
    pub observed: Vec<ObservedProvider>,
    /// Every canonical subject some provider is bound to, in subject-Ref order.
    pub subjects: Vec<SubjectReach>,
}

impl LiveWorkingField {
    pub fn subject(&self, subject: &ResourceRef) -> Option<&SubjectReach> {
        self.subjects.iter().find(|reach| &reach.subject == subject)
    }

    pub fn provider(&self, provider: &ResourceRef) -> Option<&ObservedProvider> {
        self.observed
            .iter()
            .find(|observed| &observed.provider == provider)
    }

    /// True when no provider was observed at all. Distinct from a backend that
    /// answered `None` — that means nobody looked.
    pub fn is_empty(&self) -> bool {
        self.observed.is_empty()
    }
}

/// Derive the live field from what providers reported and what this world can
/// project.
///
/// `projectable` is the set of canonical subjects the current session plan
/// defines — the panes this world *would* have. They belong in the reading
/// even before anything is live, because `open` is precisely the operation
/// that makes them live, and a field that only listed what already exists
/// could never be used to start anything.
///
/// Pure: same inputs in, same reading out, no I/O and no ambient lookup.
pub fn live_working_field(
    observations: &[WorkingEnvironmentObservation],
    projectable: &[ResourceRef],
) -> LiveWorkingField {
    let mut observed = Vec::new();
    // BTreeMap keys the reading by subject Ref, which gives the deterministic
    // ordering the goldens and the acceptance test both rely on.
    let mut subjects: BTreeMap<ResourceRef, SubjectReach> = BTreeMap::new();

    for observation in observations {
        let mut bound_here: BTreeMap<ResourceRef, (String, NativeBindingKind, Vec<String>)> =
            BTreeMap::new();
        for binding in &observation.bindings {
            // A provider that only knows a native id reports it with no
            // canonical Ref. That is honest and it is also unreachable:
            // nothing in AIKit names it, so nothing can ask for it.
            if let Some(canonical) = binding.canonical_ref.as_ref() {
                bound_here.insert(
                    canonical.clone(),
                    (
                        binding.native_id.clone(),
                        binding.kind,
                        binding.provenance.clone(),
                    ),
                );
            }
        }

        // Every subject this provider could be asked about: the ones it has
        // live, plus the ones the plan says belong here.
        let candidates: BTreeSet<ResourceRef> = bound_here
            .keys()
            .chain(projectable.iter())
            .cloned()
            .collect();

        for subject in candidates {
            let live = bound_here.get(&subject);
            let entry = subjects
                .entry(subject.clone())
                .or_insert_with(|| SubjectReach {
                    subject: subject.clone(),
                    semantic_kind: live
                        .map(|(_, kind, _)| binding_kind_label(*kind).to_string())
                        .unwrap_or_else(|| "Surface".to_string()),
                    projections: Vec::new(),
                });
            if let Some((_, kind, _)) = live {
                // A live binding is more specific than the plan's default, so
                // it wins the label.
                entry.semantic_kind = binding_kind_label(*kind).to_string();
            }
            entry.projections.push(ProviderReach {
                provider: observation.provider.clone(),
                provider_version: observation.provider_version.clone(),
                health: observation.health,
                native_id: live.map(|(native_id, _, _)| native_id.clone()),
                binding_kind: live.map(|(_, kind, _)| *kind),
                focused: live
                    .map(|(native_id, _, _)| {
                        observation.focused_native_id.as_deref() == Some(native_id.as_str())
                    })
                    .unwrap_or(false),
                // Open is available whether or not anything is live: opening is
                // create-or-attach, and refusing it because nothing exists yet
                // would leave no way to start.
                open: withheld(observation.health, observation.capabilities.open),
                // Focus needs something to focus.
                focus: withheld(observation.health, observation.capabilities.focus)
                    .or((live.is_none()).then_some(ReachWithheld::NotBound)),
                provenance: live
                    .map(|(_, _, provenance)| provenance.clone())
                    .unwrap_or_default(),
            });
        }

        observed.push(ObservedProvider {
            provider: observation.provider.clone(),
            provider_version: observation.provider_version.clone(),
            health: observation.health,
            capabilities: observation.capabilities.clone(),
            bound_subjects: bound_here.len(),
            provenance: observation.provenance.clone(),
        });
    }

    observed.sort_by(|left, right| left.provider.cmp(&right.provider));
    let mut subjects: Vec<SubjectReach> = subjects.into_values().collect();
    for subject in &mut subjects {
        subject
            .projections
            .sort_by(|left, right| left.provider.cmp(&right.provider));
    }

    LiveWorkingField {
        version: LIVE_WORKING_FIELD_VERSION.into(),
        observed,
        subjects,
    }
}

/// What a provider was actually asked to do, and what it answered.
///
/// `NotExposed` is a first-class outcome rather than an error because "no
/// provider is wired here" is a truthful state of the application boundary, and
/// the surface should render it as such instead of showing a failure the
/// operator cannot act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum WorkingEnvironmentOutcome {
    Opened {
        provider: ResourceRef,
        subject: ResourceRef,
        native_id: String,
    },
    Focused {
        provider: ResourceRef,
        subject: ResourceRef,
        native_id: String,
    },
    NotExposed {
        provider: ResourceRef,
        subject: ResourceRef,
        reason: String,
    },
}

impl WorkingEnvironmentOutcome {
    pub fn subject(&self) -> &ResourceRef {
        match self {
            Self::Opened { subject, .. }
            | Self::Focused { subject, .. }
            | Self::NotExposed { subject, .. } => subject,
        }
    }

    pub fn provider(&self) -> &ResourceRef {
        match self {
            Self::Opened { provider, .. }
            | Self::Focused { provider, .. }
            | Self::NotExposed { provider, .. } => provider,
        }
    }

    /// One line for the status bar, in the operator's terms.
    pub fn summary(&self) -> String {
        match self {
            Self::Opened {
                provider,
                subject,
                native_id,
            } => format!("opened {subject} in {provider} · native {native_id}"),
            Self::Focused {
                provider,
                subject,
                native_id,
            } => format!("focused {subject} in {provider} · native {native_id}"),
            Self::NotExposed {
                provider,
                subject,
                reason,
            } => format!("{provider} did not open {subject}: {reason}"),
        }
    }
}

/// Guard an open/focus request against the reading before it reaches a
/// provider. A surface that offers an affordance the field says is withheld is
/// a presentation bug; this turns it into a refusal that names the condition.
pub fn reach_for<'field>(
    field: &'field LiveWorkingField,
    provider: &ResourceRef,
    subject: &ResourceRef,
    operation: WorkingEnvironmentOperation,
) -> Result<&'field ProviderReach> {
    let Some(reach) = field.subject(subject) else {
        return Err(AikitError::new(
            "tui.live_field.subject_absent",
            format!("no observed provider is bound to {subject}"),
        ));
    };
    let Some(projection) = reach.projection(provider) else {
        return Err(AikitError::new(
            "tui.live_field.provider_not_bound",
            format!("{provider} has no binding for {subject}"),
        ));
    };
    let withheld = match operation {
        WorkingEnvironmentOperation::Open => projection.open,
        WorkingEnvironmentOperation::Focus => projection.focus,
    };
    if let Some(withheld) = withheld {
        return Err(AikitError::new(
            "tui.live_field.operation_withheld",
            format!(
                "{provider} cannot {} {subject}: {}",
                operation.as_str(),
                withheld.describe(operation.as_str())
            ),
        ));
    }
    Ok(projection)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkingEnvironmentOperation {
    Open,
    Focus,
}

impl WorkingEnvironmentOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Focus => "focus",
        }
    }
}

/// Route live-field selection through the one existing semantic reducer, the
/// same way the composed working field does. Selecting in the live field is
/// selecting, not a second cursor.
pub fn select_live_field_subject(
    state: TuiState,
    field: &LiveWorkingField,
    subject: ResourceRef,
) -> Result<TuiReduction> {
    if field.subject(&subject).is_none() {
        return Err(AikitError::new(
            "tui.live_field.subject_absent",
            format!("{subject} is not reachable through any observed provider"),
        ));
    }
    Ok(reduce_tui(state, UiAction::Select(subject)))
}

fn withheld(health: WorkingEnvironmentHealth, claimed: bool) -> Option<ReachWithheld> {
    if health == WorkingEnvironmentHealth::Unavailable {
        return Some(ReachWithheld::ProviderUnavailable);
    }
    if !claimed {
        return Some(ReachWithheld::CapabilityNotClaimed);
    }
    None
}

fn binding_kind_label(kind: NativeBindingKind) -> &'static str {
    match kind {
        NativeBindingKind::Session => "Session",
        NativeBindingKind::View => "View",
        NativeBindingKind::Surface => "Surface",
        NativeBindingKind::Project => "Project",
        NativeBindingKind::AgentSession => "AgentSession",
    }
}

/// The canonical Action Ref for one operation in one provider.
///
/// The provider's own last path segment carries into the Action Ref, so
/// `provider/tmux/current` and `provider/cmux/current` give two distinct,
/// stable, parseable Actions over the one canonical subject. Nothing about the
/// native pane appears here.
pub fn action_ref(
    provider: &ResourceRef,
    operation: WorkingEnvironmentOperation,
) -> Result<ResourceRef> {
    let family = provider
        .as_str()
        .split('/')
        .nth(1)
        .filter(|segment| !segment.is_empty())
        .ok_or_else(|| {
            AikitError::new(
                "tui.live_field.unnameable_provider",
                format!("{provider} has no provider family segment to name an Action after"),
            )
        })?;
    ResourceRef::parse(format!(
        "action/working-environment/{}/{family}",
        operation.as_str()
    ))
}

/// Every open/focus Action currently applicable to `subject`, one per provider
/// that can actually perform it.
///
/// A withheld capability produces no Action at all rather than a disabled one:
/// the field already explains the absence in the Worlds pane, and an Action
/// that would refuse itself is worse than an Action that is not offered.
pub fn working_environment_actions(
    field: &LiveWorkingField,
    subject: &ResourceRef,
) -> Result<Vec<ContextualActionDescriptor>> {
    let Some(reach) = field.subject(subject) else {
        return Ok(Vec::new());
    };
    let mut actions = Vec::new();
    for projection in &reach.projections {
        for operation in [
            WorkingEnvironmentOperation::Open,
            WorkingEnvironmentOperation::Focus,
        ] {
            let permitted = match operation {
                WorkingEnvironmentOperation::Open => projection.can_open(),
                WorkingEnvironmentOperation::Focus => projection.can_focus(),
            };
            if !permitted {
                continue;
            }
            actions.push(
                ContextualActionDescriptor::new(
                    action_ref(&projection.provider, operation)?,
                    subject.clone(),
                    format!("{} in {}", operation.as_str(), projection.provider.as_str()),
                    match projection.native_id.as_deref() {
                        Some(native_id) => format!(
                            "{} {subject} through {} · native {native_id}",
                            operation.as_str(),
                            projection.provider.as_str(),
                        ),
                        None => format!(
                            "{} {subject} through {} · not live there yet",
                            operation.as_str(),
                            projection.provider.as_str(),
                        ),
                    },
                    ActionStageability::NotStageable,
                )
                .with_keywords([
                    operation.as_str().to_string(),
                    "working environment".to_string(),
                    projection.provider.as_str().to_string(),
                ]),
            );
        }
    }
    Ok(actions)
}

/// Read a working-environment Action Ref back into the provider and operation
/// it names, or `None` when this Action belongs to something else.
pub fn parse_action_ref(
    action: &ResourceRef,
    field: &LiveWorkingField,
) -> Option<(ResourceRef, WorkingEnvironmentOperation)> {
    let mut segments = action.as_str().split('/');
    if segments.next()? != "action" || segments.next()? != "working-environment" {
        return None;
    }
    let operation = match segments.next()? {
        "open" => WorkingEnvironmentOperation::Open,
        "focus" => WorkingEnvironmentOperation::Focus,
        _ => return None,
    };
    let family = segments.next()?;
    // Resolve back through the reading rather than reconstructing a provider
    // Ref by string: the provider that answers must be one that was actually
    // observed, not one this parse invented.
    let provider = field
        .observed
        .iter()
        .map(|observed| &observed.provider)
        .find(|provider| provider.as_str().split('/').nth(1) == Some(family))?;
    Some((provider.clone(), operation))
}
